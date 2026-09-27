//! Deploy the built Astro site (workspace/astro-project/dist) to Cloudflare Pages.
//! The owner signs in with wrangler's browser OAuth inside the runtime image.
//! Wrangler's credentials live in private/cloudflare and are mounted only into
//! these short-lived containers: never into Codex, workers or project exports.
//! Only non-secret choices (account ID, project name, domain, URLs) are stored.
use crate::{model::*, runtime, store::Store, AppState};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tauri::{Emitter, State};

const LOGIN_CONTAINER: &str = "h2wpd-cloudflare-login";
const CALLBACK_PORT: u16 = 8976;
/// Least privilege: Pages deploys, the account list and zone lookups.
const SCOPES: &[&str] = &["account:read", "user:read", "pages:write", "zone:read"];
const OUTDATED: &str = "Your conversion environment predates Cloudflare deploys. Update the environment in Settings (Prepare or Repair environment), then try again.";

/// The per-project choices remembered for re-deploys. No secrets.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Site {
    pub project_name: String,
    pub cf_project_id: Option<String>,
    pub domain: Option<String>,
    pub account_id: Option<String>,
    pub pages_url: Option<String>,
    pub deployment_url: Option<String>,
    pub deployed_at: Option<String>,
    pub domain_state: Option<Value>,
}

fn setting_key(project_id: &str) -> String {
    format!("cloudflare:{project_id}")
}
pub fn load_site(store: &Store, p: &Project) -> Site {
    store
        .setting(&setting_key(&p.id))
        .and_then(|v| serde_json::from_str(&v).ok())
        .unwrap_or_else(|| Site { project_name: derive_project_name(&p.name), ..Site::default() })
}
fn save_site(store: &Store, project_id: &str, site: &Site) -> Result<()> {
    store.set(&setting_key(project_id), &serde_json::to_string(site).map_err(err)?)
}

/// A Pages project name from the site's name: lowercase ASCII, digits and
/// single hyphens, at most 58 characters (Cloudflare's limit).
pub fn derive_project_name(name: &str) -> String {
    let mut out = String::new();
    for c in name.to_lowercase().chars() {
        let c = match c {
            'á' | 'ä' | 'à' | 'â' | 'ã' | 'å' | 'ą' => 'a',
            'č' | 'ć' | 'ç' => 'c',
            'ď' => 'd',
            'é' | 'ě' | 'ë' | 'è' | 'ê' | 'ę' => 'e',
            'í' | 'ï' | 'ì' | 'î' => 'i',
            'ĺ' | 'ľ' | 'ł' => 'l',
            'ň' | 'ń' | 'ñ' => 'n',
            'ó' | 'ô' | 'ö' | 'ò' | 'õ' | 'ő' | 'ø' => 'o',
            'ŕ' | 'ř' => 'r',
            'š' | 'ś' => 's',
            'ť' => 't',
            'ú' | 'ů' | 'ü' | 'ù' | 'û' | 'ű' => 'u',
            'ý' | 'ÿ' => 'y',
            'ž' | 'ź' | 'ż' => 'z',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    let mut out: String = out.chars().take(58).collect();
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() { "site".into() } else { out }
}
pub fn valid_project_name(name: &str) -> Result<String> {
    let ok = !name.is_empty()
        && name.len() <= 58
        && name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !name.starts_with('-')
        && !name.ends_with('-');
    if ok { Ok(name.into()) } else {
        Err("Use 1–58 lowercase letters, digits or hyphens for the project name, not starting or ending with a hyphen.".into())
    }
}
/// A bare hostname, lowercased and in ASCII (IDNA) form. Empty means none.
pub fn valid_domain(domain: &str) -> Result<Option<String>> {
    let trimmed = domain.trim().trim_end_matches('.').to_lowercase();
    if trimmed.is_empty() {
        return Ok(None);
    }
    let invalid = || "Enter a domain like www.example.com, without https:// or a path.".to_string();
    if trimmed.contains("://") || trimmed.contains(['/', '@', ':', '?', '#', ' ']) {
        return Err(invalid());
    }
    let host = url::Url::parse(&format!("https://{trimmed}/")).map_err(|_| invalid())?
        .host_str().map(str::to_string).ok_or_else(invalid)?;
    let labels: Vec<_> = host.split('.').collect();
    let label_ok = |l: &&str| !l.is_empty() && l.len() <= 63 && !l.starts_with('-') && !l.ends_with('-')
        && l.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if host.len() > 253 || labels.len() < 2 || !labels.iter().all(label_ok)
        || labels.last().is_some_and(|tld| tld.chars().all(|c| c.is_ascii_digit())) {
        return Err(invalid());
    }
    Ok(Some(host))
}
pub fn valid_account(id: &str) -> Result<String> {
    if id.len() == 32 && id.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)) {
        Ok(id.into())
    } else {
        Err("Choose one of your Cloudflare accounts.".into())
    }
}

fn config_dir(store: &Store) -> Result<PathBuf> {
    let dir = store.root.join("private/cloudflare");
    std::fs::create_dir_all(&dir).map_err(err)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).map_err(err)?;
    }
    Ok(dir)
}
/// Wrangler keeps its OAuth sign-in in $XDG_CONFIG_HOME/.wrangler/config/<profile>.toml.
fn signed_in_locally(store: &Store) -> bool {
    has_credentials(&store.root.join("private/cloudflare"))
}
fn has_credentials(cfg: &Path) -> bool {
    std::fs::read_dir(cfg.join(".wrangler/config")).map(|entries| {
        entries.flatten().any(|e| e.file_type().is_ok_and(|t| t.is_file()))
    }).unwrap_or(false)
}
fn dist_dir(store: &Store, p: &Project) -> Result<PathBuf> {
    Ok(store.path(&p.id)?.join("workspace/astro-project/dist"))
}
/// Why the deploy button is disabled, or None when the built site exists.
pub fn dist_blocker(dist: &Path, phase: &str) -> Option<&'static str> {
    let index = dist.join("index.html");
    if index.is_file() && !index.is_symlink() && !dist.is_symlink() {
        return None;
    }
    Some(if phase == "imported" {
        "Start the conversion first. The site can be deployed once its static build is ready."
    } else {
        "Available once the conversion has built the static site (its “HTML to Astro” stage)."
    })
}

fn strings(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}
/// Hardened `docker run` prefix shared by the login and the helper. Wrangler
/// telemetry, error reports and log files are switched off or kept in /tmp.
fn container_args(cfg: &Path, name: &str, detached: bool) -> Vec<String> {
    let mut a = strings(&["run", if detached { "-d" } else { "--rm" }]);
    if !detached {
        a.push("-i".into());
    }
    a.extend(strings(&[
        "--name", name, "--label", "dev.html2wp.desktop=true", "--init", "--cap-drop=ALL",
        "--security-opt", "no-new-privileges", "--pids-limit", "256", "--memory", "2g",
        "--user", "1000:1000", "--network", "bridge", "--workdir", "/tmp",
        "--env", "HOME=/tmp", "--env", "XDG_CONFIG_HOME=/cfg",
        "--env", "WRANGLER_SEND_METRICS=false", "--env", "WRANGLER_SEND_ERROR_REPORTS=false",
        "--env", "DO_NOT_TRACK=1", "--env", "WRANGLER_LOG_PATH=/tmp/wrangler-logs",
        "--env", "WRANGLER_NO_SKILLS_UPDATE_PROMPTS=true", "--env", "WRANGLER_HIDE_BANNER=true",
    ]));
    a.extend(["--mount".into(), format!("type=bind,source={},target=/cfg", cfg.display())]);
    a
}
pub fn login_args(cfg: &Path, image: &str) -> Vec<String> {
    let mut a = container_args(cfg, LOGIN_CONTAINER, true);
    // The browser returns to http://localhost:8976 on this computer only.
    a.extend(["--publish".into(), format!("127.0.0.1:{CALLBACK_PORT}:{CALLBACK_PORT}")]);
    a.extend([image.into(), "wrangler".into(), "login".into(), "--browser=false".into(),
        "--callback-host".into(), "0.0.0.0".into(), "--callback-port".into(), CALLBACK_PORT.to_string(), "--scopes".into()]);
    a.extend(strings(SCOPES));
    a
}
pub fn helper_args(cfg: &Path, image: &str, dist: Option<&Path>) -> Vec<String> {
    let mut a = container_args(cfg, &format!("h2wpd-cloudflare-{}", &id()[..8]), false);
    a.extend(["--env".into(), "CI=1".into()]);
    if let Some(dist) = dist {
        a.extend(["--mount".into(), format!("type=bind,source={},target=/site,readonly", dist.display())]);
    }
    a.extend([image.into(), "python3".into(), "/opt/desktop/cloudflare.py".into()]);
    a
}
/// The authorize link wrangler prints. Only Cloudflare's own OAuth page
/// returning to this computer's callback is accepted.
pub fn parse_login_url(logs: &str) -> Option<String> {
    let start = logs.find("https://dash.cloudflare.com/oauth2/auth?")?;
    let candidate: String = logs[start..].chars().take_while(|c| !c.is_whitespace() && *c != '\x1b').collect();
    let parsed = url::Url::parse(&candidate).ok()?;
    let redirect = parsed.query_pairs().find(|(k, _)| k == "redirect_uri")?.1.to_string();
    (parsed.scheme() == "https" && parsed.host_str() == Some("dash.cloudflare.com") && parsed.port().is_none()
        && redirect == format!("http://localhost:{CALLBACK_PORT}/oauth/callback"))
        .then_some(candidate)
}
/// `wrangler whoami --json` as the helper reports it, with account IDs checked again.
pub fn parse_identity(v: &Value) -> Value {
    if v["loggedIn"] != true {
        return json!({"loggedIn":false,"email":null,"accounts":[]});
    }
    let accounts: Vec<Value> = v["accounts"].as_array().into_iter().flatten()
        .filter(|a| a["id"].as_str().is_some_and(|id| valid_account(id).is_ok()))
        .map(|a| json!({"id":a["id"],"name":a["name"].as_str().unwrap_or("").chars().take(200).collect::<String>()}))
        .collect();
    json!({"loggedIn":true,"email":v["email"].as_str(),"accounts":accounts})
}
/// URLs the app may open for a site: its pages.dev address, its custom
/// domain, or the Cloudflare dashboard. Anything else is refused.
pub fn openable(url: &str, domain: Option<&str>) -> Result<String> {
    let parsed = url::Url::parse(url).map_err(|_| "This address cannot be opened".to_string())?;
    let host = parsed.host_str().unwrap_or("");
    let ok = parsed.scheme() == "https" && parsed.username().is_empty() && parsed.port().is_none()
        && (host.ends_with(".pages.dev") || host == "dash.cloudflare.com" || Some(host) == domain);
    if ok { Ok(parsed.to_string()) } else { Err("This address is not an allowed application destination".into()) }
}
fn friendly_docker_error(e: String) -> String {
    if e.contains("cloudflare.py") || e.contains("wrangler: not found") || e.contains("executable file not found") {
        OUTDATED.into()
    } else if e.contains("port is already allocated") || e.contains("address already in use") {
        format!("Port {CALLBACK_PORT} on this computer is in use, most likely by another Cloudflare sign-in (wrangler login). Close it and try again.")
    } else if e.starts_with("Cannot start Docker") || e.contains("Cannot connect to the Docker daemon") {
        "Docker is not running. Start Docker, then try again.".into()
    } else {
        e
    }
}

async fn helper(store: &Store, image: &str, request: Value, dist: Option<&Path>, seconds: u64) -> Result<Value> {
    let cfg = config_dir(store)?;
    let output = runtime::docker(&helper_args(&cfg, image, dist), Some(request.to_string().as_bytes()), seconds)
        .await.map_err(friendly_docker_error)?;
    let result: Value = serde_json::from_str(output.lines().rev().find(|l| l.starts_with('{')).ok_or(OUTDATED)?)
        .map_err(err)?;
    if result.get("ok") == Some(&json!(false)) {
        return Err(result["message"].as_str().unwrap_or("Cloudflare Pages could not complete this step.").into());
    }
    Ok(result)
}
async fn identity(store: &Store, image: &str) -> Result<Value> {
    if !signed_in_locally(store) {
        return Ok(parse_identity(&Value::Null));
    }
    Ok(parse_identity(&helper(store, image, json!({"action":"whoami"}), None, 120).await?))
}
/// The account to use: the saved choice while it is still available,
/// otherwise the only account. Several accounts and no choice → None.
pub fn chosen_account(identity: &Value, saved: Option<String>) -> Option<String> {
    let ids: Vec<&str> = identity["accounts"].as_array().into_iter().flatten().filter_map(|a| a["id"].as_str()).collect();
    match saved {
        Some(id) if ids.contains(&id.as_str()) => Some(id),
        _ if ids.len() == 1 => Some(ids[0].into()),
        _ => None,
    }
}
fn open_in_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut c = {
        let mut c = std::process::Command::new("/usr/bin/open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut c = {
        let mut c = std::process::Command::new("rundll32.exe");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "linux")]
    let mut c = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    c.spawn().map_err(err)?;
    Ok(())
}
fn progress(app: &tauri::AppHandle, project_id: &str, stage: &str, message: &str) {
    let _ = app.emit("cloudflare-progress", json!({"projectId":project_id,"stage":stage,"message":message}));
}

/// The saved site and whether a build exists. No Docker, for the Exports row.
#[tauri::command]
pub fn cloudflare_site(state: State<AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let blocker = dist_blocker(&dist_dir(&state.store, &p)?, &p.phase);
    Ok(json!({"distReady":blocker.is_none(),"distReason":blocker,"site":load_site(&state.store, &p)}))
}
#[tauri::command]
pub async fn cloudflare_status(state: State<'_, AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let site = load_site(&state.store, &p);
    let blocker = dist_blocker(&dist_dir(&state.store, &p)?, &p.phase);
    let (identity, auth_error) = match identity(&state.store, &state.image()).await {
        Ok(v) => (v, None),
        Err(e) => (parse_identity(&Value::Null), Some(e)),
    };
    let account = chosen_account(&identity, site.account_id.clone().or_else(|| state.store.setting("cloudflare-account")));
    Ok(json!({"distReady":blocker.is_none(),"distReason":blocker,"site":site,"identity":identity,"accountId":account,"authError":auth_error,"signedInLocally":signed_in_locally(&state.store)}))
}
/// Start wrangler's OAuth sign-in and open Cloudflare's page in the browser.
#[tauri::command]
pub async fn cloudflare_login_start(state: State<'_, AppState>) -> Result<Value> {
    runtime::require_local().await?;
    if runtime::assert_owned(LOGIN_CONTAINER).await.is_ok() {
        runtime::docker(&strings(&["rm", "-f", LOGIN_CONTAINER]), None, 30).await?;
    }
    let cfg = config_dir(&state.store)?;
    runtime::docker(&login_args(&cfg, &state.image()), None, 60).await.map_err(friendly_docker_error)?;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(750)).await;
        let logs = runtime::docker(&strings(&["logs", LOGIN_CONTAINER]), None, 15).await.unwrap_or_default();
        if let Some(url) = parse_login_url(&logs) {
            open_in_browser(&url)?;
            return Ok(json!({"url":url}));
        }
        let running = runtime::docker(&strings(&["inspect", "--format", "{{.State.Running}}", LOGIN_CONTAINER]), None, 15).await.unwrap_or_default();
        if running.trim() != "true" {
            break;
        }
    }
    let _ = runtime::docker(&strings(&["rm", "-f", LOGIN_CONTAINER]), None, 30).await;
    Err("Cloudflare sign-in could not start. Check your internet connection and try again.".into())
}
/// Open the pending sign-in page again, if the owner closed the tab.
#[tauri::command]
pub fn cloudflare_open_login(url: String) -> Result<()> {
    open_in_browser(&parse_login_url(&url).ok_or("This is not a Cloudflare sign-in page")?)
}
/// Wait until the owner finishes (or abandons) the browser sign-in.
#[tauri::command]
pub async fn cloudflare_login_wait(state: State<'_, AppState>) -> Result<Value> {
    runtime::assert_owned(LOGIN_CONTAINER).await.map_err(|_| "Sign-in was cancelled.".to_string())?;
    let waited = runtime::docker(&strings(&["wait", LOGIN_CONTAINER]), None, 600).await;
    let _ = runtime::docker(&strings(&["rm", "-f", LOGIN_CONTAINER]), None, 30).await;
    match waited {
        Ok(code) if code.trim() == "0" => {}
        Ok(_) => return Err("Cloudflare sign-in was not completed. Choose Sign in to try again.".into()),
        Err(e) if e.contains("timed out") => return Err("Cloudflare sign-in timed out after 10 minutes. Choose Sign in to try again.".into()),
        Err(_) => return Err("Sign-in was cancelled.".into()),
    }
    // Ask wrangler itself rather than trusting where it saved the sign-in.
    let identity = parse_identity(&helper(&state.store, &state.image(), json!({"action":"whoami"}), None, 120).await?);
    if identity["loggedIn"] != true {
        return Err("Cloudflare did not confirm the sign-in. Choose Sign in to try again.".into());
    }
    if let Some(account) = chosen_account(&identity, None) {
        state.store.set("cloudflare-account", &account)?;
    }
    Ok(identity)
}
#[tauri::command]
pub async fn cloudflare_login_cancel() -> Result<()> {
    if runtime::assert_owned(LOGIN_CONTAINER).await.is_ok() {
        runtime::docker(&strings(&["rm", "-f", LOGIN_CONTAINER]), None, 30).await?;
    }
    Ok(())
}
#[tauri::command]
pub async fn cloudflare_select_account(state: State<'_, AppState>, account_id: String) -> Result<()> {
    let account = valid_account(&account_id)?;
    let identity = identity(&state.store, &state.image()).await?;
    if chosen_account(&identity, Some(account.clone())).as_deref() != Some(account.as_str()) {
        return Err("This account is not available for your Cloudflare sign-in.".into());
    }
    state.store.set("cloudflare-account", &account)
}
/// Revoke wrangler's token when Cloudflare is reachable, then delete it locally.
#[tauri::command]
pub async fn cloudflare_logout(state: State<'_, AppState>) -> Result<()> {
    let _ = cloudflare_login_cancel().await;
    if signed_in_locally(&state.store) {
        let _ = helper(&state.store, &state.image(), json!({"action":"logout"}), None, 60).await;
    }
    let dir = state.store.root.join("private/cloudflare");
    if dir.exists() {
        std::fs::remove_dir_all(dir).map_err(err)?;
    }
    state.store.set("cloudflare-account", "")
}
/// Create the Pages project when needed, upload dist, attach the domain.
#[tauri::command]
pub async fn cloudflare_deploy(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, project_name: String, domain: String, account_id: String, replace_existing: bool) -> Result<Site> {
    let name = valid_project_name(&project_name)?;
    let domain = valid_domain(&domain)?;
    let account = valid_account(&account_id)?;
    let _tools = state.try_lock_project(&project_id, "Wait for the current step to finish, then deploy.")?;
    // No conversion of this project may rebuild dist, and closing the app
    // asks first. Other projects keep running.
    if !state.claim(&project_id)? { return Err("Finish or stop this project's running conversion before deploying.".into()); }
    let result = deploy(&app, &state, &project_id, name, domain, account, replace_existing).await;
    state.release(&project_id);
    result
}
async fn deploy(app: &tauri::AppHandle, state: &AppState, project_id: &str, name: String, domain: Option<String>, account: String, replace_existing: bool) -> Result<Site> {
    let p = state.store.project(project_id)?;
    let dist = dist_dir(&state.store, &p)?;
    if let Some(reason) = dist_blocker(&dist, &p.phase) {
        return Err(reason.into());
    }
    let image = state.image();
    let mut site = load_site(&state.store, &p);
    // A project this site deployed before is ours; any other one needs consent.
    let known = replace_existing || (site.pages_url.is_some() && site.project_name == name && site.account_id.as_deref() == Some(account.as_str()));
    let request = json!({"accountId":account,"projectName":name,"domain":domain,"known":known});
    progress(app, &p.id, "project", "Preparing the Cloudflare Pages project…");
    let mut prepare = request.clone();
    prepare["action"] = json!("project");
    let project = helper(&state.store, &image, prepare, None, 180).await?;
    site = Site { project_name: name.clone(), cf_project_id: project["cfProjectId"].as_str().map(str::to_string), account_id: Some(account.clone()), pages_url: project["pagesUrl"].as_str().map(str::to_string), domain: domain.clone(), domain_state: if domain == site.domain { site.domain_state } else { None }, ..site };
    save_site(&state.store, &p.id, &site)?;
    progress(app, &p.id, "upload", "Uploading your site to Cloudflare…");
    let mut upload = request.clone();
    upload["action"] = json!("deploy");
    let deployed = helper(&state.store, &image, upload, Some(&dist), 1800).await?;
    site.deployment_url = deployed["deploymentUrl"].as_str().map(str::to_string);
    site.deployed_at = Some(now());
    save_site(&state.store, &p.id, &site)?;
    if domain.is_some() {
        progress(app, &p.id, "domain", "Connecting your domain…");
        let mut attach = request;
        attach["action"] = json!("domain");
        site.domain_state = Some(helper(&state.store, &image, attach, None, 180).await?["domain"].clone());
        save_site(&state.store, &p.id, &site)?;
    }
    state.store.set("cloudflare-account", &account)?;
    if let Ok(activity) = state.store.activity(&p.id, &format!("Deployed to Cloudflare Pages ({name})"), "complete") {
        let _ = app.emit("activity", activity);
    }
    progress(app, &p.id, "done", "Your site is live.");
    Ok(site)
}
/// Delete the exact remote Pages project saved for this workspace. A failed
/// remote operation leaves the local mapping intact so it can be retried.
#[tauri::command]
pub async fn cloudflare_remove(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> Result<Site> {
    let _tools = state.try_lock_project(&project_id, "Wait for the current step to finish, then remove the Pages site.")?;
    if !state.claim(&project_id)? { return Err("Finish or stop this project's running conversion before removing its Pages site.".into()); }
    let result = remove(&app, &state, &project_id).await;
    state.release(&project_id);
    result
}
async fn remove(app: &tauri::AppHandle, state: &AppState, project_id: &str) -> Result<Site> {
    let p = state.store.project(project_id)?;
    let site = load_site(&state.store, &p);
    let account = valid_account(site.account_id.as_deref().ok_or("No Cloudflare account is saved for this site.")?)?;
    let name = valid_project_name(&site.project_name)?;
    let pages_url = site.pages_url.as_deref().ok_or("No Cloudflare Pages project is saved for this site.")?;
    helper(&state.store, &state.image(), json!({
        "action":"remove", "accountId":account, "projectName":name,
        "expectedProjectId":site.cf_project_id, "expectedPagesUrl":pages_url,
    }), None, 180).await?;
    let cleared = Site { project_name: name.clone(), ..Site::default() };
    save_site(&state.store, &p.id, &cleared)?;
    if let Ok(activity) = state.store.activity(&p.id, &format!("Removed Cloudflare Pages project ({name})"), "complete") {
        let _ = app.emit("activity", activity);
    }
    Ok(cleared)
}
/// Ask Cloudflare to validate the custom domain again and report its status.
#[tauri::command]
pub async fn cloudflare_domain_check(state: State<'_, AppState>, project_id: String) -> Result<Site> {
    let p = state.store.project(&project_id)?;
    let mut site = load_site(&state.store, &p);
    let (Some(domain), Some(account)) = (site.domain.clone(), site.account_id.clone()) else {
        return Err("Deploy the site with a domain first.".into());
    };
    let request = json!({"action":"domain-check","accountId":valid_account(&account)?,"projectName":valid_project_name(&site.project_name)?,"domain":valid_domain(&domain)?});
    site.domain_state = Some(helper(&state.store, &state.image(), request, None, 120).await?["domain"].clone());
    save_site(&state.store, &p.id, &site)?;
    Ok(site)
}
/// Open one of this site's stored addresses in the default browser.
#[tauri::command]
pub fn cloudflare_open(state: State<AppState>, project_id: String, target: String) -> Result<()> {
    let p = state.store.project(&project_id)?;
    let site = load_site(&state.store, &p);
    let url = match target.as_str() {
        "pages" => site.pages_url.clone(),
        "deployment" => site.deployment_url.clone(),
        "domain" => site.domain.as_ref().map(|d| format!("https://{d}")),
        "dashboard" => site.domain_state.as_ref().and_then(|d| d["dashboardUrl"].as_str().map(str::to_string)),
        _ => None,
    }
    .ok_or("This address is not available yet.")?;
    open_in_browser(&openable(&url, site.domain.as_deref())?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn project_names_are_derived_and_validated() {
        assert_eq!(derive_project_name("Kaviareň Žilina · Nový web!"), "kaviaren-zilina-novy-web");
        assert_eq!(derive_project_name("---"), "site");
        assert_eq!(derive_project_name(&"a".repeat(80)).len(), 58);
        assert_eq!(derive_project_name("My Site-"), "my-site");
        for ok in ["a", "my-site", "site2", &"b".repeat(58)] { assert!(valid_project_name(ok).is_ok(), "{ok}"); }
        for bad in ["", "-a", "a-", "My", "a_b", "a b", "a;rm", "--help", &"b".repeat(59)] { assert!(valid_project_name(bad).is_err(), "{bad}"); }
    }
    #[test]
    fn domains_are_bare_ascii_hostnames() {
        assert_eq!(valid_domain("  WWW.Example.com. ").unwrap().as_deref(), Some("www.example.com"));
        assert_eq!(valid_domain("").unwrap(), None);
        assert_eq!(valid_domain("kaviareň.sk").unwrap().as_deref(), Some("xn--kaviare-6kb.sk"));
        for bad in ["https://example.com", "example.com/x", "localhost", "a..com", "-a.com", "1.2.3.4", "a b.com", "user@example.com", "example.com:8080", "--help.com"] {
            assert!(valid_domain(bad).is_err(), "{bad}");
        }
        assert!(valid_account("0123456789abcdef0123456789abcdef").is_ok());
        for bad in ["", "0123456789ABCDEF0123456789ABCDEF", "0123", "0123456789abcdef0123456789abcdeg"] { assert!(valid_account(bad).is_err(), "{bad}"); }
    }
    #[test]
    fn login_publishes_the_callback_on_loopback_with_least_privilege_scopes() {
        let a = login_args(Path::new("/data/private/cloudflare"), "img:1");
        let joined = a.join(" ");
        assert!(joined.contains("--publish 127.0.0.1:8976:8976"));
        assert!(!joined.contains("0.0.0.0:8976"), "never published beyond loopback");
        assert!(joined.contains("--mount type=bind,source=/data/private/cloudflare,target=/cfg"));
        assert!(joined.ends_with("img:1 wrangler login --browser=false --callback-host 0.0.0.0 --callback-port 8976 --scopes account:read user:read pages:write zone:read"));
        assert!(joined.contains("WRANGLER_SEND_METRICS=false") && joined.contains("--cap-drop=ALL") && joined.contains("--user 1000:1000"));
        assert!(a.iter().all(|arg| !arg.contains("/project")), "no project files in the login container");
    }
    #[test]
    fn helper_mounts_only_credentials_and_the_read_only_build() {
        let a = helper_args(Path::new("/p/cf"), "img:1", Some(Path::new("/p/projects/x/workspace/astro-project/dist")));
        let mounts: Vec<_> = a.windows(2).filter(|w| w[0] == "--mount").map(|w| w[1].as_str()).collect();
        assert_eq!(mounts, ["type=bind,source=/p/cf,target=/cfg", "type=bind,source=/p/projects/x/workspace/astro-project/dist,target=/site,readonly"]);
        assert!(a.ends_with(&strings(&["img:1", "python3", "/opt/desktop/cloudflare.py"])));
        assert!(a.contains(&"CI=1".to_string()) && a.contains(&"--rm".to_string()));
        assert!(helper_args(Path::new("/p/cf"), "img:1", None).iter().all(|arg| !arg.contains("/site")));
    }
    #[test]
    fn only_cloudflares_authorize_page_returning_here_is_opened() {
        let logs = "Temporary login server listening on 0.0.0.0:8976\n\x1b[2mVisit this link to authenticate: https://dash.cloudflare.com/oauth2/auth?response_type=code&client_id=54d1&redirect_uri=http%3A%2F%2Flocalhost%3A8976%2Foauth%2Fcallback&scope=pages%3Awrite&state=abc\x1b[0m\n";
        let url = parse_login_url(logs).unwrap();
        assert!(url.starts_with("https://dash.cloudflare.com/oauth2/auth?") && url.ends_with("state=abc"));
        assert!(parse_login_url("https://dash.cloudflare.com/oauth2/auth?redirect_uri=http%3A%2F%2Fevil.example%2Fcb").is_none());
        assert!(parse_login_url("https://dash.cloudflare.com.evil.example/oauth2/auth?redirect_uri=http%3A%2F%2Flocalhost%3A8976%2Foauth%2Fcallback").is_none());
        assert!(parse_login_url("Attempting to login via OAuth...").is_none());
    }
    #[test]
    fn identity_and_account_choice() {
        let me = parse_identity(&json!({"loggedIn":true,"email":"o@example.invalid","accounts":[{"id":"0123456789abcdef0123456789abcdef","name":"One"},{"id":"../x","name":"Bad"}]}));
        assert_eq!(me["accounts"].as_array().unwrap().len(), 1);
        assert_eq!(chosen_account(&me, None).as_deref(), Some("0123456789abcdef0123456789abcdef"));
        let two = json!({"accounts":[{"id":"a".repeat(32)},{"id":"b".repeat(32)}]});
        assert_eq!(chosen_account(&two, None), None);
        assert_eq!(chosen_account(&two, Some("b".repeat(32))), Some("b".repeat(32)));
        assert_eq!(chosen_account(&two, Some("c".repeat(32))), None);
        assert_eq!(parse_identity(&Value::Null)["loggedIn"], false);
    }
    #[test]
    fn only_the_sites_own_addresses_open() {
        assert!(openable("https://my-site.pages.dev", None).is_ok());
        assert!(openable("https://www.example.com", Some("www.example.com")).is_ok());
        assert!(openable("https://dash.cloudflare.com/abc/pages/view/x/domains", None).is_ok());
        for bad in ["http://my-site.pages.dev", "https://evil.example", "https://www.example.com", "https://u:p@x.pages.dev", "file:///etc/passwd", "https://x.pages.dev:8443"] {
            assert!(openable(bad, None).is_err(), "{bad}");
        }
    }
    #[test]
    fn deploy_is_blocked_until_the_static_build_exists_and_sites_persist() {
        let dir = tempfile::tempdir().unwrap();
        let dist = dir.path().join("dist");
        assert!(dist_blocker(&dist, "imported").unwrap().contains("Start the conversion"));
        assert!(dist_blocker(&dist, "running").unwrap().contains("built the static site"));
        std::fs::create_dir_all(&dist).unwrap();
        std::fs::write(dist.join("index.html"), "<h1>ok</h1>").unwrap();
        assert!(dist_blocker(&dist, "preparing").is_none());
        let store = Store::open(dir.path().join("data")).unwrap();
        let p: Project = serde_json::from_value(json!({"id":"0584dcf1-7f08-4efb-85cf-ae7284faf8f9","name":"Kaviareň Web","sourceName":"t","kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"imported","revision":1,"threadId":null,"pages":[],"gates":[],"artifacts":[],
            "preview":null,"runtimeImage":"img","pluginCommit":"abc","reporting":"not_required","lastError":null})).unwrap();
        assert_eq!(load_site(&store, &p), Site { project_name: "kaviaren-web".into(), ..Site::default() });
        let site = Site { project_name: "kaviaren".into(), domain: Some("www.example.com".into()), account_id: Some("a".repeat(32)), pages_url: Some("https://kaviaren.pages.dev".into()), ..Site::default() };
        save_site(&store, &p.id, &site).unwrap();
        assert_eq!(load_site(&store, &p), site);
        let raw = store.setting("cloudflare:0584dcf1-7f08-4efb-85cf-ae7284faf8f9").unwrap();
        assert!(!raw.contains("token"), "only non-secret choices are stored");
        let cfg = dir.path().join("cf");
        assert!(!has_credentials(&cfg));
        std::fs::create_dir_all(cfg.join(".wrangler/config")).unwrap();
        std::fs::write(cfg.join(".wrangler/metrics.json"), "{}").unwrap();
        assert!(!has_credentials(&cfg), "telemetry settings are not a sign-in");
        std::fs::write(cfg.join(".wrangler/config/default.toml"), "oauth_token = \"x\"").unwrap();
        assert!(has_credentials(&cfg));
    }
}
