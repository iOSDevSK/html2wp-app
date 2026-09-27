mod agent;
mod bridge;
mod cloudflare;
mod codex;
mod compare;
mod editor;
mod disk_space;
mod exports;
mod files;
mod h2g;
mod h2g_compare;
mod h2g_preview;
mod licence;
mod lifecycle;
mod model;
mod models;
mod notifications;
mod plugin;
mod preview;
mod project_downloads;
mod project_paths;
// Retain the old managed-browser fixtures, but exclude its launcher from the app.
#[cfg(test)]
mod preview_browser;
mod runtime;
mod runtime_manifest;
mod run_context;
mod setup;
mod app_update;
mod site_preview;
mod skill;
mod store;
use model::*;
use serde_json::{json, Value};
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tauri::{Emitter, Manager, State};
pub struct AppState {
    pub store: store::Store,
    pub resources: PathBuf,
    pub versions: Value,
    pub rpc: Mutex<Option<Arc<codex::Rpc>>>,
    pub connecting: tokio::sync::Mutex<()>,
    /// Environment maintenance (prepare, runtime update, rollback) takes this
    /// for writing; every project step holds it for reading, so projects run
    /// side by side but never while the environment is being replaced.
    pub work: tokio::sync::RwLock<()>,
    /// One lock per project: its steps run one at a time, other projects'
    /// steps do not wait for them.
    pub project_locks: Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>,
    /// Projects whose conversation is running. Several can run at once.
    pub active: Mutex<HashSet<String>>,
    pub requests: tokio::sync::Mutex<HashMap<String, Value>>,
    pub setup: Mutex<Option<Arc<setup::Control>>>,
    /// The Chrome extension's pairing code and the bridge's port.
    pub pairing: bridge::Pairing,
}
/// A project's step: shared environment access plus that project's own lock.
pub struct ProjectWork<'a> {
    _env: tokio::sync::RwLockReadGuard<'a, ()>,
    _project: tokio::sync::OwnedMutexGuard<()>,
}
impl AppState {
    pub fn new(store: store::Store, resources: PathBuf, versions: Value) -> Self {
        AppState{store,resources,versions,rpc:Mutex::new(None),connecting:tokio::sync::Mutex::new(()),work:tokio::sync::RwLock::new(()),project_locks:Mutex::new(HashMap::new()),active:Mutex::new(HashSet::new()),requests:tokio::sync::Mutex::new(HashMap::new()),setup:Mutex::new(None),pairing:bridge::Pairing::new()}
    }
    fn project_mutex(&self, pid: &str) -> Result<Arc<tokio::sync::Mutex<()>>> {
        Ok(self.project_locks.lock().map_err(err)?.entry(pid.to_string()).or_default().clone())
    }
    /// Wait for this project's current step, then hold its lock.
    pub async fn lock_project(&self, pid: &str) -> Result<ProjectWork<'_>> {
        let env = self.work.read().await;
        let project = self.project_mutex(pid)?.lock_owned().await;
        Ok(ProjectWork { _env: env, _project: project })
    }
    /// Hold this project's lock only if no step of it is running now.
    pub fn try_lock_project(&self, pid: &str, busy: &str) -> Result<ProjectWork<'_>> {
        let env = self.work.try_read().map_err(|_| "The conversion environment is being updated. Try again when it finishes.")?;
        let project = self.project_mutex(pid)?.try_lock_owned().map_err(|_| busy.to_string())?;
        Ok(ProjectWork { _env: env, _project: project })
    }
    /// Exclusive access for environment maintenance: no project may be
    /// running a conversation or a step.
    pub fn try_lock_environment(&self, busy: &str) -> Result<tokio::sync::RwLockWriteGuard<'_, ()>> {
        if self.any_running()? { return Err(busy.into()); }
        self.work.try_write().map_err(|_| busy.to_string())
    }
    pub fn is_running(&self, pid: &str) -> Result<bool> {
        Ok(self.active.lock().map_err(err)?.contains(pid))
    }
    pub fn any_running(&self) -> Result<bool> {
        Ok(!self.active.lock().map_err(err)?.is_empty())
    }
    /// Mark a project running; false when it already is.
    pub fn claim(&self, pid: &str) -> Result<bool> {
        Ok(self.active.lock().map_err(err)?.insert(pid.to_string()))
    }
    pub fn release(&self, pid: &str) {
        if let Ok(mut active) = self.active.lock() { active.remove(pid); }
    }
    pub fn running(&self) -> Result<Vec<String>> {
        let mut ids: Vec<String> = self.active.lock().map_err(err)?.iter().cloned().collect();
        ids.sort();
        Ok(ids)
    }
    /// An installed app may require a newer published runtime. The old image
    /// stays available for existing projects until Prepare succeeds.
    fn release_runtime_is_newer(&self) -> bool {
        let Some(active) = self.store.setting("active-runtime") else { return false };
        let Ok(release) = serde_json::from_str::<Value>(include_str!("../../runtime/runtime-release.json")) else { return true };
        let Ok(runtime) = runtime_manifest::selected(&release) else { return true };
        runtime["imageId"].as_str() != Some(&active)
            && !runtime["imageIds"].as_array().is_some_and(|ids| ids.iter().any(|id| id.as_str()==Some(&active)))
    }
    /// The Codex container on the installed runtime: one made from an older
    /// runtime is replaced (its account volume stays) and the connection to
    /// it dropped, so the next request connects to the new one. Only while no
    /// conversation runs (`own`: the project about to start one): a replaced
    /// container would end it.
    pub(crate) async fn refresh_agent(&self, own: Option<&str>) -> Result<bool> {
        if self.running()?.iter().any(|id| Some(id.as_str()) != own) { return Ok(false); }
        let Some(plugin) = plugin::active(&self.store) else { return Ok(false) };
        let _guard = self.connecting.lock().await;
        let replaced = runtime::replace_outdated_agent(&self.image(), &plugin).await?;
        if replaced { self.rpc.lock().map_err(err)?.take(); }
        Ok(replaced)
    }
    /// A Codex release newer than the one in the Codex container is installed
    /// there and used from then on; the owner is told. Once per launch, only
    /// while no conversation runs. A Codex the app cannot talk to is removed
    /// again and the runtime's own Codex kept.
    pub(crate) async fn update_codex(&self, app: &tauri::AppHandle) -> Result<()> {
        if !self.running()?.is_empty() { return Ok(()); }
        let update = {
            let _guard = self.connecting.lock().await;
            let name = runtime::ensure_agent(&self.image(), &plugin::require(&self.store)?).await?;
            let Some(update) = runtime::update_codex(&name).await? else { return Ok(()) };
            self.rpc.lock().map_err(err)?.take();
            (name, update)
        };
        let (name, (from, to)) = update;
        // The handshake is the check (the model list needs a signed-in account).
        let works = self.connection(app).await.is_ok();
        if works {
            let _ = app.emit("codex-updated", json!({"from":from,"to":to,"ok":true,"message":format!("Codex was updated from {from} to {to}. New models are now in the model list.")}));
            return Ok(());
        }
        {
            let _guard = self.connecting.lock().await;
            runtime::remove_codex_update(&name).await?;
            self.rpc.lock().map_err(err)?.take();
        }
        let _ = app.emit("codex-updated", json!({"from":from,"to":to,"ok":false,"message":format!("Codex {to} is available, but this app version cannot use it yet. Codex {from} is kept.")}));
        Ok(())
    }
    /// The plugin released upstream, when it is another version than the
    /// installed one, is fetched from GitHub and used from then on; the owner
    /// is told. Once per launch, only while no conversation runs. Offline, or
    /// a fetch that fails: the installed plugin stays and nothing is said. A
    /// plugin for another app contract is refused and the owner told why.
    pub(crate) async fn update_plugin(&self, app: &tauri::AppHandle) -> Result<()> {
        if !self.running()?.is_empty() { return Ok(()); }
        let Ok(upstream) = plugin::upstream().await else { return Ok(()) };
        let installed = plugin::active(&self.store);
        if installed.as_ref().is_some_and(|p| p.version == upstream) { return Ok(()); }
        // Fetched beside the installed one; switched only while no step runs.
        let commit = match plugin::fetch(&self.store, &self.image(), &upstream).await {
            Ok(commit) => commit,
            Err(e) if e.starts_with("Update the app") => { let _ = app.emit("plugin-updated", json!({"ok":false,"message":e})); return Ok(()); }
            Err(_) => return Ok(()),
        };
        {
            let _env = self.try_lock_environment("A conversion is running")?;
            plugin::activate(&self.store, &upstream, &commit)?;
        }
        let (from, to) = (installed.map(|p| p.version), upstream);
        // The Codex container and every project move to it.
        let _ = self.refresh_agent(None).await;
        for p in self.store.projects().unwrap_or_default() { adopt_stored(app, self, &p.id).await; }
        let message = match &from {
            Some(from) => format!("The html2wp plugin was updated from {from} to {to}."),
            None => format!("The html2wp plugin {to} was installed."),
        };
        let _ = app.emit("plugin-updated", json!({"ok":true,"from":from,"to":to,"message":message}));
        Ok(())
    }
    /// Fetch the released plugin when none is installed (preparing the
    /// environment, or a check that finds none); offline, a clear message.
    pub(crate) async fn ensure_plugin(&self) -> Result<plugin::Plugin> {
        self.ensure_plugin_with_image(&self.image()).await
    }
    pub(crate) async fn ensure_plugin_with_image(&self, image: &str) -> Result<plugin::Plugin> {
        if let Some(installed) = plugin::active(&self.store) { return Ok(installed); }
        // One fetch at a time (a check and Prepare can both ask); the second finds it installed.
        let _guard = self.connecting.lock().await;
        if let Some(installed) = plugin::active(&self.store) { return Ok(installed); }
        let version = plugin::upstream().await.map_err(|_| plugin::NEEDS_INTERNET.to_string())?;
        plugin::install(&self.store, image, &version).await.map_err(|e| if e.starts_with("Update the app") { e } else { format!("{} ({e})", plugin::NEEDS_INTERNET) })?;
        plugin::require(&self.store)
    }
    /// The installed runtime every container runs on.
    pub(crate) fn image(&self) -> String {
        self.store
            .setting("active-runtime")
            .unwrap_or_else(|| serde_json::from_str::<Value>(include_str!("../../runtime/runtime-release.json"))
                .ok().and_then(|release| runtime_manifest::selected(&release).ok().and_then(|runtime| runtime["imageId"].as_str().map(str::to_owned)))
                .unwrap_or_else(|| self.versions["image"].as_str().unwrap().into()))
    }
    async fn connection(&self, app: &tauri::AppHandle) -> Result<Arc<codex::Rpc>> {
        let _guard = self.connecting.lock().await;
        if let Some(rpc) = self
            .rpc
            .lock()
            .map_err(err)?
            .clone()
            .filter(|rpc| rpc.is_alive())
        {
            return Ok(rpc);
        }
        let rpc = codex::Rpc::connect(app.clone(), &self.image(), &plugin::require(&self.store)?).await?;
        {
            let mut current = self.rpc.lock().map_err(err)?;
            if !rpc.is_alive() {
                return Err(
                    "Codex stopped while connecting. Try Connect with ChatGPT again.".into(),
                );
            }
            *current = Some(rpc.clone());
        }
        Ok(rpc)
    }
}
#[tauri::command]
fn get_bootstrap(state: State<AppState>) -> Result<Value> {
    let mut versions = state.versions.clone();
    versions["appVersion"] = json!(env!("CARGO_PKG_VERSION"));
    // The plugin fetched from GitHub; none before the first run.
    let installed = plugin::active(&state.store);
    versions["pluginVersion"] = json!(installed.as_ref().map(|p| p.version.clone()).unwrap_or_default());
    versions["pluginCommit"] = json!(installed.map(|p| p.commit).unwrap_or_default());
    Ok(
        json!({"projects":state.store.projects()?,"versions":versions,"platform":std::env::consts::OS,"architecture":std::env::consts::ARCH,"disclosureAccepted":state.store.setting("disclosure").as_deref()==Some("accepted"),"licenceConfigured":state.store.root.join("private/licence").exists(),"activeProjects":state.running()?,"experimentalGutenberg":experimental_gutenberg(&state.store),"maxParallel":max_parallel(&state.store),"maxParallelLimit":MAX_PARALLEL_LIMIT,"selectedModel":state.store.setting("selected-model").unwrap_or_default(),"activeProject":state.store.setting(bridge::ACTIVE_PROJECT_KEY).filter(|v|!v.is_empty())}),
    )
}
#[tauri::command]
async fn check_runtime(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Value> {
    let mut status = runtime::preflight(&state.image()).await;
    if status["imageReady"] == true && state.release_runtime_is_newer() {
        status["ready"] = json!(false);
        status["imageReady"] = json!(false);
        status["message"] = json!("This version needs its published conversion environment. Choose Prepare environment to download it; your projects are kept.");
    }
    // The plugin comes from GitHub: fetched here when none is installed.
    if status["ready"] == true && plugin::active(&state.store).is_none() && !state.any_running()? {
        if let Err(e) = state.ensure_plugin().await {
            status["ready"] = json!(false);
            status["message"] = json!(e);
        }
    }
    status["pluginReady"] = json!(plugin::active(&state.store).is_some());
    // Once per launch, in the background: a newer plugin release, then a
    // newer Codex release (new models).
    static CHECKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if status["ready"] == true && !CHECKED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let plugin = state.update_plugin(&app).await;
            let codex = state.update_codex(&app).await;
            if plugin.is_err() || codex.is_err() { CHECKED.store(false, std::sync::atomic::Ordering::SeqCst); }
        });
    }
    Ok(status)
}
#[tauri::command]
async fn prepare_runtime(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Value> {
    let _guard=state.try_lock_environment("Finish or stop every running conversion before preparing the environment")?;
    let prepared=setup::prepare(&app,&state).await?;
    // The Codex container follows the runtime just installed.
    drop(_guard);
    let _=state.refresh_agent(None).await;
    Ok(prepared)
}
#[tauri::command]
fn cancel_setup(state: State<AppState>) -> Result<()> {
    setup::cancel(&state)
}
#[tauri::command]
fn accept_disclosure(state: State<AppState>) -> Result<()> {
    state.store.set("disclosure", "accepted")
}
#[tauri::command]
fn save_licence(state: State<AppState>, key: String) -> Result<()> {
    if state.any_running()? {
        return Err("Finish every running conversion before changing licences".into());
    }
    if key.trim().is_empty() || key.len() > 512 || key.chars().any(|c| c.is_control()) {
        return Err("Invalid licence key".into());
    }
    state.store.set("licence-status", "null")?;
    store::write_private(
        &state.store.root.join("private/licence"),
        key.trim().as_bytes(),
    )
}
#[tauri::command]
async fn licence_status(state: State<'_, AppState>) -> Result<Value> {
    licence::check(&state.store).await
}
#[tauri::command]
fn use_free(state: State<AppState>) -> Result<()> {
    if state.any_running()? {
        return Err("Finish every running conversion before changing licences".into());
    }
    let path = state.store.root.join("private/licence");
    if path.exists() {
        std::fs::remove_file(path).map_err(err)?;
    }
    state.store.set("licence-status", "null")
}
#[tauri::command]
fn save_queue(state: State<AppState>, project_id: String, messages: Vec<String>) -> Result<()> {
    state.store.project(&project_id)?;
    if messages.len() > 50 || messages.iter().any(|m| m.len() > 32000) {
        return Err("Message queue is full".into());
    }
    state.store.set(
        &format!("queue:{project_id}"),
        &serde_json::to_string(&messages).map_err(err)?,
    )
}
#[tauri::command]
async fn account_read(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Value> {
    let rpc = state.connection(&app).await?;
    let v = rpc
        .request("account/read", json!({"refreshToken":false}))
        .await?;
    let limits = if v["account"].is_null() {
        Value::Null
    } else {
        rpc.request("account/rateLimits/read", json!({}))
            .await
            .unwrap_or(Value::Null)
    };
    Ok(json!({"account":v["account"],"requiresOpenaiAuth":v["requiresOpenaiAuth"],"limits":limits}))
}
#[tauri::command]
async fn model_catalog(app: tauri::AppHandle, state: State<'_, AppState>, project_id: Option<String>) -> Result<Value> {
    let rpc = state.connection(&app).await?;
    if rpc
        .request("account/read", json!({"refreshToken":false}))
        .await?["account"]
        .is_null()
    {
        return Err("Connect your ChatGPT account to load models".into());
    }
    let catalog = models::catalog(&rpc).await?;
    // With a project: that project's model. Without: the Settings default.
    let (selected, effort) = match &project_id {
        Some(pid) => {
            let p = state.store.project(pid)?;
            let (model, effort) = models::project_choice(&state.store, &p);
            (p.model.clone().filter(|m| !m.is_empty()).unwrap_or(model), effort)
        }
        None => {
            let selected = state.store.setting("selected-model").unwrap_or_default();
            let effort = models::resolve(&catalog, &selected)
                .ok()
                .and_then(|model| state.store.setting(&models::effort_key(&model)))
                .unwrap_or_default();
            (selected, effort)
        }
    };
    Ok(json!({"models":catalog,"selectedModel":selected,"selectedEffort":effort}))
}
/// Codex catalog check shared by the model and effort choices.
async fn signed_in_catalog(app: &tauri::AppHandle, state: &AppState) -> Result<Vec<models::CodexModel>> {
    let rpc = state.connection(app).await?;
    if rpc
        .request("account/read", json!({"refreshToken":false}))
        .await?["account"]
        .is_null()
    {
        return Err("Connect your ChatGPT account first".into());
    }
    models::catalog(&rpc).await
}
/// A project's model can change whenever that project is not running; other
/// projects' conversations are unaffected. The Settings default applies to
/// projects that have not chosen one yet, so it can change at any time.
#[tauri::command]
async fn select_model(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model: String,
    project_id: Option<String>,
) -> Result<Value> {
    if model.len() > 256 {
        return Err("Invalid model selection".into());
    }
    if let Some(pid) = &project_id {
        if state.is_running(pid)? {
            return Err("You can change this project's model when its assistant finishes".into());
        }
    }
    let catalog = signed_in_catalog(&app, &state).await?;
    let canonical = models::resolve(&catalog, &model)?;
    match &project_id {
        Some(pid) => {
            if state.is_running(pid)? {
                return Err("This project started while models were loading. Try again when it finishes.".into());
            }
            let mut p = state.store.project(pid)?;
            // An empty choice means the catalog default, kept as that model.
            // A different model drops the effort chosen for the old one.
            if p.model.as_deref() != Some(canonical.as_str()) { p.effort = None; }
            p.model = Some(canonical.clone());
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            let effort = state.store.setting(&models::effort_key(&canonical)).unwrap_or_default();
            Ok(json!({"selectedModel":canonical,"selectedEffort":effort}))
        }
        None => {
            state.store.set("selected-model", &model)?;
            let effort = state
                .store
                .setting(&models::effort_key(&canonical))
                .unwrap_or_default();
            Ok(json!({"selectedModel":model,"selectedEffort":effort}))
        }
    }
}
#[tauri::command]
async fn select_effort(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    model: String,
    effort: String,
    project_id: Option<String>,
) -> Result<()> {
    if effort.len() > 32 || model.len() > 256 {
        return Err("Invalid reasoning preference".into());
    }
    if let Some(pid) = &project_id {
        if state.is_running(pid)? {
            return Err("You can change this project's reasoning effort when its assistant finishes".into());
        }
    }
    let catalog = signed_in_catalog(&app, &state).await?;
    let canonical = models::resolve(&catalog, &model)?;
    models::resolve_effort(&catalog, &canonical, &effort)?;
    match &project_id {
        Some(pid) => {
            if state.is_running(pid)? {
                return Err("This project started while preferences were loading. Try again when it finishes.".into());
            }
            let mut p = state.store.project(pid)?;
            // A project still on the default takes that model with the effort.
            if p.model.as_deref().unwrap_or("").is_empty() { p.model = Some(canonical.clone()); }
            if p.model.as_deref().unwrap_or("") != canonical {
                return Err("The model changed. Refresh the model catalog and choose the effort again.".into());
            }
            p.effort = if effort.is_empty() { None } else { Some(effort) };
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            Ok(())
        }
        None => {
            if state.store.setting("selected-model").unwrap_or_default() != model {
                return Err(
                    "The model changed. Refresh the model catalog and choose the effort again.".into(),
                );
            }
            state.store.set(&models::effort_key(&canonical), &effort)
        }
    }
}
#[tauri::command]
async fn account_login(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Value> {
    let rpc = state.connection(&app).await?;
    let r = rpc
        .request("account/login/start", json!({"type":"chatgptDeviceCode"}))
        .await?;
    Ok(
        json!({"loginId":r["loginId"],"verificationUrl":r["verificationUrl"],"userCode":r["userCode"]}),
    )
}
#[tauri::command]
async fn account_cancel(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    login_id: String,
) -> Result<Value> {
    state
        .connection(&app)
        .await?
        .request("account/login/cancel", json!({"loginId":login_id}))
        .await
}
#[tauri::command]
async fn account_logout(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<Value> {
    if state.any_running()? {
        return Err("Stop every running conversation before signing out".into());
    }
    state
        .connection(&app)
        .await?
        .request("account/logout", json!({}))
        .await
}
#[tauri::command]
async fn import_project(state: State<'_, AppState>, path: String, target: Option<String>) -> Result<Project> {
    let target = offered_target(target.as_deref().unwrap_or("html"))?;
    let source = Path::new(&path);
    let name = source
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let id = id();
    let root = state.store.path(&id)?;
    let result = (|| {
        files::copy_input(source, &root.join("input"))?;
        // The plugin converts its own copy (/project/source); the input stays as imported.
        if target != h2g::TARGET { files::copy_input(&root.join("input"), &root.join("source"))?; }
        // Gutenberg from an HTML theme takes only a theme html2wp made,
        // checked before any AI runs.
        let (kind, pages) = if target == h2g::TARGET { h2g::check_input(&root.join("input"))?; (h2g::KIND.to_string(), vec![]) } else { files::detect(&root.join("input"))? };
        std::fs::create_dir_all(root.join("workspace")).map_err(err)?;
        std::fs::create_dir_all(root.join("artifacts")).map_err(err)?;
        let p = Project {
            id: id.clone(),
            name,
            source_name: source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into(),
            kind,
            created_at: now(),
            updated_at: now(),
            phase: "imported".into(),
            last_step: None,
            revision: 1,
            thread_id: None,
            pages,
            gates: vec![],
            artifacts: vec![],
            preview: None,
            runtime_image: state.image(),
            plugin_commit: plugin::active(&state.store).map(|p| p.commit).unwrap_or_default(),
            reporting: "not_required".into(),
            last_error: None,
            auto_approve: false,
            conversion_approval_required: false,
            archived: false,
            target,
            // A new project starts with the default from Settings; the owner
            // can change it per project without touching the others.
            model: state.store.setting("selected-model").filter(|m| !m.is_empty()),
            effort: None,
            flash: false,
        };
        project_downloads::retain_original(&state.store, &p, source)?;
        state.store.put(&p)?;
        state.store.message(&p.id,"assistant",if p.from_theme() { "Your theme is ready to convert. Choose Start conversion when you are ready; it usually takes 3–5 hours." } else { "Your project is ready. Choose Flash for a fast conversion, or Full for the complete, checked one. Your original files stay unchanged." })?;
        Ok(p)
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(root);
        let _ = std::fs::remove_file(state.store.root.join("private/original-imports").join(format!("{id}.zip")));
    }
    result
}
/// The theme type can change only before the conversion starts: a
/// prepared workspace, service job and checks all belong to one type.
pub(crate) fn change_target(p: &mut Project, target: &str) -> Result<()> {
    let target = offered_target(target)?;
    // The input decides it: a theme html2wp made, or a site.
    if (target == h2g::TARGET) != p.from_theme() {
        return Err("Gutenberg from an HTML theme takes a theme html2wp made, and the other types take a site: import the input again with the output you want.".into());
    }
    if p.phase != "imported" || p.last_step.is_some() || p.thread_id.is_some() {
        return Err("The theme type is fixed once the conversion has started. Use Clean & restart to choose another type.".into());
    }
    p.target = target;
    p.updated_at = now();
    Ok(())
}
/// The project moves to the installed runtime and its plugin: its container
/// on an older image is made again when next used. Every turn and every
/// comparison starts through here.
async fn refresh_runtime(app: &tauri::AppHandle, state: &AppState, p: &mut Project) -> bool {
    let Some(plugin) = plugin::active(&state.store) else { return false };
    if !runtime::adopt_current(p, &state.image(), &plugin.commit).await { return false; }
    if let Ok(activity) = state.store.activity(&p.id, "Updated this project to the current conversion environment", "complete") { let _ = app.emit("activity", activity); }
    true
}
/// The stored project moves to the installed runtime (app start, opening it,
/// its preview, a new run). A running project moved when its turn began.
async fn adopt_stored(app: &tauri::AppHandle, state: &AppState, project_id: &str) {
    if state.is_running(project_id).unwrap_or(true) { return; }
    let Ok(mut p) = state.store.project(project_id) else { return };
    if refresh_runtime(app, state, &mut p).await && state.store.put(&p).is_ok() { let _ = app.emit("project-updated", &p); }
}
/// A conversion must not start on the previous release's runner while this
/// release's environment waits to be installed.
fn require_current_runtime(state: &AppState) -> Result<()> {
    if state.release_runtime_is_newer() {
        return Err("This version needs its published conversion environment. Choose Prepare environment in Settings first; your projects are kept.".into());
    }
    Ok(())
}
/// Gutenberg from an HTML theme: the workflow step the agent reported.
#[tauri::command]
fn h2g_progress(state: State<AppState>, project_id: String) -> Result<Value> {
    let mut found = h2g::progress(&state.store, &project_id);
    // After delivery: whether the theme differs from the last release.
    if let Ok(p) = state.store.project(&project_id) { found["changes"] = h2g::changes(&state.store, &p); }
    Ok(found)
}
/// A site's conversion, for the Overview: the progress the plugin wrote and,
/// once the run ended, its verdict.
#[tauri::command]
fn skill_progress(state: State<AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let result = skill::result(&state.store, &p);
    Ok(json!({"progress":skill::progress(&state.store, &p),"result":{"status":result["status"],"verdict":result["verdict"],"stopped":result["stopped"],"wired":result["wired"],"repairs":result["repairs"],"couldNotFix":result["couldNotFix"],"recovery":result["recovery"]},
        "changes":skill::changes(&state.store, &p),"stoppedRun":skill::stopped(&state.store, &p)}))
}
#[tauri::command]
fn set_project_target(app: tauri::AppHandle, state: State<AppState>, project_id: String, target: String) -> Result<Project> {
    if state.is_running(&project_id)? {
        return Err("Finish this project's conversation before changing its theme type".into());
    }
    let mut p = state.store.project(&project_id)?;
    change_target(&mut p, &target)?;
    state.store.put(&p)?;
    let _ = app.emit("project-updated", &p);
    Ok(p)
}
#[tauri::command]
async fn project_detail(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> Result<Value> {
    adopt_stored(&app, &state, &project_id).await;
    Ok(
        json!({"project":state.store.project(&project_id)?,"messages":state.store.messages(&project_id)?,"activity":state.store.activities(&project_id)?,"queue":state.store.setting(&format!("queue:{project_id}")).and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(json!([]))}),
    )
}
#[tauri::command]
async fn project_action(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, action: String) -> Result<Value> {
    let _tools = state.try_lock_project(&project_id, "Wait for this project's current step to finish")?;
    if !state.claim(&project_id)? { return Err("Stop this project's conversion before managing it".into()); }
    // Removing, deleting or resetting a project ends its built-site preview.
    site_preview::stop_project(&project_id);
    let result = lifecycle::change(&state.store, &project_id, &action).await;
    state.release(&project_id);
    let project = result?;
    if let Some(p) = &project { let _ = app.emit("project-updated", p); }
    else { let _ = app.emit("project-deleted", json!({"projectId":project_id})); }
    Ok(value(project))
}
/// Whether this project's chat can take a new message now. The typed chat
/// and the Chrome extension bridge share it, so both give the same reason.
/// It never claims the project.
pub(crate) fn chat_ready(state: &AppState, project_id: &str) -> Result<()> {
    let _env = state.work.try_read().map_err(|_| "Wait for the app update or environment setup to finish before starting a conversation")?;
    if state.setup.lock().map_err(err)?.is_some() {
        return Err("Wait for environment setup to finish before starting a conversation".into());
    }
    if state.store.setting("disclosure").as_deref() != Some("accepted") {
        return Err("Read and accept the data flow notice in Settings first".into());
    }
    require_current_runtime(state)?;
    if state.is_running(project_id)? {
        let name = state.store.project(project_id).map(|p| p.name).unwrap_or_else(|_| "this project".into());
        return Err(format!("The assistant is working on {name}. Send the screenshot when it finishes."));
    }
    // Several projects may convert at once, each on its own thread and model.
    // The machine must have room for another run next to the ones going.
    parallel_room(state, project_id)
}
/// A message from the owner: the typed chat, or a screenshot from the
/// Chrome extension, which arrives as a message with one attached image. It
/// is one turn of the assistant; it never starts or resumes a run.
#[tauri::command]
async fn send_message(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, text: String, images: Option<Vec<String>>) -> Result<Value> {
    send_message_inner(&app, &state, &project_id, &text, images.as_deref().unwrap_or(&[])).await
}
pub(crate) async fn send_message_inner(app: &tauri::AppHandle, state: &AppState, project_id: &str, text: &str, images: &[String]) -> Result<Value> {
    let _env = state.work.try_read().map_err(|_| "Wait for the app update or environment setup to finish before starting a conversation")?;
    if text.trim().is_empty() || text.len() > 32000 {
        return Err("Enter a message up to 32,000 characters".into());
    }
    // After delivery a message is a change to the live preview, never a new run (contract v1.3).
    let mut p = state.store.project(project_id)?;
    // A stopped run: the owner's message is the repair-stop goal (v1.4 §7c), never a plain turn or a new run.
    if skill::stopped(&state.store, &p) && !state.is_running(project_id)? {
        chat_ready(state, project_id)?;
        let before = p.phase.clone();
        skill::repair_stop(&state.store, &mut p)?;
        state.store.put(&p)?;
        let _ = app.emit("project-updated", &p);
        skill::stage_visual_edit_lite(&state.store, &p).await;
        let goal = skill::repair_stop_text(Some(text));
        state.store.set(&format!("goal:{}", p.id), &goal)?;
        let started = run_turn_shown(app, state, project_id, &goal, text, true, images).await;
        // Codex did not take it: the project is as it was (the plugin's stopped result still says so).
        if started.is_err() { if let Ok(mut now) = state.store.project(project_id) { if now.phase == "running" { now.phase = before; let _ = state.store.put(&now); let _ = app.emit("project-updated", &now); } } }
        return started;
    }
    if p.astro_only() && skill::astro_recovery_candidate(&skill::result(&state.store, &p))
        && !state.is_running(project_id)? {
        chat_ready(state, project_id)?;
        if skill::astro_repair_supported(&state.store, &p, &state.image()).await {
        // Recovery context is not a repair action. In particular, retain the
        // current result and phase for questions; only the plugin begins work.
        state.store.set(&format!("turn-mode:{}", p.id), "repair-delivery")?;
        let context = skill::astro_recovery_text(text);
        return run_turn_shown(app, state, project_id, &context, text, false, images).await;
        }
    }
    let change = skill::takes_changes(&p);
    if !state.is_running(project_id)? { skill::set_change_turn(&state.store, &p, change)?; }
    let sent = if change { skill::change_text(&state.store, &p, text) }
        else if h2g::takes_changes(&p) { h2g::change_text(&state.store, &p, text) }
        else { text.to_string() };
    if change && !state.is_running(project_id)? { skill::ready_preview(&state.store, &p, &state.image()).await; }
    run_turn_shown(app, state, project_id, &sent, text, false, images).await
}
/// Flash or Full (a new conversion run of a site), Start (Gutenberg from an
/// HTML theme), or Continue (the run that stopped): a Codex thread goal with
/// the skill's goal, which Codex keeps working on until the skill delivers.
#[tauri::command]
async fn start_run(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, mode: String) -> Result<Value> {
    let _env = state.work.try_read().map_err(|_| "Wait for the app update or environment setup to finish before starting a conversion")?;
    chat_ready(&state, &project_id)?;
    adopt_stored(&app, &state, &project_id).await;
    let mut p = state.store.project(&project_id)?;
    let before = p.phase.clone();
    let text = match (p.from_theme(), mode.as_str()) {
        (true, "start") => h2g::START.to_string(),
        (true, "continue") => h2g::CONTINUE.to_string(),
        (false, "flash" | "full" | "astro") => {
            if (mode == "astro") != p.astro_only() { return Err("This project's output decides its run: Flash or Full for an HTML theme, the Astro run for an Astro 5 project.".into()); }
            // A new run: what an earlier one delivered stays in Exports as history.
            // Over a result (delivered or stopped) it is the owner's Start over (v1.4).
            let over = skill::has_result(&state.store, &p);
            skill::new_run(&state.store, &mut p, mode == "flash")?;
            skill::set_start_over(&state.store, &p, over)?;
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            skill::stage_visual_edit_lite(&state.store, &p).await;
            let goal = skill::start_text(&p);
            state.store.set(&format!("goal:{}", p.id), &goal)?;
            goal
        }
        (false, "repair-delivery") => {
            if p.astro_only() && !skill::astro_repair_supported(&state.store, &p, &state.image()).await {
                return Err("The plugin mounted in this project does not support Astro recovery yet. Update the plugin while the project is idle.".into());
            }
            let goal = skill::repair_delivery(&state.store, &mut p)?;
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            state.store.set(&format!("goal:{}", p.id), &goal)?;
            goal
        }
        // Continue on a stopped run is its repair-stop (v1.4 §7c), never the Continue goal.
        (false, "continue") if skill::stopped(&state.store, &p) => {
            skill::repair_stop(&state.store, &mut p)?;
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            skill::stage_visual_edit_lite(&state.store, &p).await;
            let goal = skill::repair_stop_text(None);
            state.store.set(&format!("goal:{}", p.id), &goal)?;
            goal
        }
        (false, "continue") if skill::repair_delivery_turn(&state.store, &p) => {
            skill::resume_run(&state.store, &mut p)?;
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            agent::goal_objective(&state.store, &p)
        }
        (false, "continue") => {
            skill::resume_run(&state.store, &mut p)?;
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            skill::stage_visual_edit_lite(&state.store, &p).await;
            let goal = skill::continue_text(&state.store, &p);
            state.store.set(&format!("goal:{}", p.id), &goal)?;
            goal
        }
        _ => return Err("Unknown conversion run".into()),
    };
    // A run, never a change: "Start over from the original" after delivery starts one too.
    // (A repair-stop set its own kind above.)
    if (!skill::repair_stop_turn(&state.store, &p) && !skill::repair_delivery_turn(&state.store, &p)) || matches!(mode.as_str(), "flash" | "full" | "astro") { skill::set_change_turn(&state.store, &p, false)?; }
    let started = run_turn(&app, &state, &project_id, &text, true, &[]).await;
    // Codex did not take the run (not connected, usage): the project is as it was.
    if started.is_err() {
        if mode == "repair-delivery" { let _ = skill::rollback_delivery_repair(&state.store, &p); }
        if let Ok(mut p) = state.store.project(&project_id) {
            if p.phase == "running" { p.phase = before; let _ = state.store.put(&p); let _ = app.emit("project-updated", &p); }
        }
    }
    started
}
/// One turn of the project's assistant; `goal`: a conversion run.
async fn run_turn(app: &tauri::AppHandle, state: &AppState, project_id: &str, text: &str, goal: bool, images: &[String]) -> Result<Value> {
    run_turn_shown(app, state, project_id, text, text, goal, images).await
}
/// A turn whose input to Codex (`text`) differs from what the chat shows (`shown`).
async fn run_turn_shown(app: &tauri::AppHandle, state: &AppState, project_id: &str, text: &str, shown: &str, goal: bool, images: &[String]) -> Result<Value> {
    let _env = state.work.try_read().map_err(|_| "Wait for the app update or environment setup to finish before starting a conversation")?;
    chat_ready(state, project_id)?;
    if !state.claim(project_id)? {
        return Err("This project's assistant is already running. Your message can be sent when it finishes.".into());
    }
    // The owner's start is a fresh budget of resumes, and a new owner turn.
    let _ = state.store.set(&format!("goal-reactivations:{project_id}"), "0");
    if let Err(e) = skill::new_owner_turn(&state.store, project_id) { state.release(project_id); return Err(e); }
    // Codex on the installed runtime, when no other conversation would be cut.
    if let Err(e) = state.refresh_agent(Some(project_id)).await { state.release(project_id); return Err(e); }
    let result = match state.connection(app).await {
        Ok(rpc) => start_turn(app, state, rpc, project_id, (text, shown), goal, images).await,
        Err(e) => Err(e),
    };
    match &result {
        Ok(_) => { let _ = app.emit("turn-started", json!({"projectId":project_id})); }
        Err(_) => state.release(project_id),
    }
    result
}
/// The app's side of the Chrome extension bridge.
#[derive(Clone)]
struct AppBridge(tauri::AppHandle);
impl bridge::Host for AppBridge {
    fn state(&self) -> &AppState { self.0.state::<AppState>().inner() }
    fn deliver(&self, d: bridge::Delivery) -> impl std::future::Future<Output = Result<()>> + Send {
        let app = self.0.clone();
        async move {
            let state = app.state::<AppState>();
            let images: Vec<String> = d.images.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            send_message_inner(&app, &state, &d.project_id, &d.text, &images).await?;
            let _ = app.emit("bridge-message", json!({"projectId":d.project_id}));
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
            Ok(())
        }
    }
    fn pairing_changed(&self) {
        let _ = self.0.emit("chrome-bridge", bridge::settings_view(self.state()));
    }
}
/// The project the owner has open; the Chrome extension sends into its chat.
#[tauri::command]
fn set_active_project(state: State<AppState>, project_id: Option<String>) -> Result<()> {
    state.store.set(bridge::ACTIVE_PROJECT_KEY, project_id.as_deref().unwrap_or(""))
}
#[tauri::command]
fn chrome_bridge_status(state: State<AppState>) -> Value {
    bridge::settings_view(&state)
}
#[tauri::command]
fn chrome_bridge_regenerate(state: State<AppState>) -> Value {
    state.pairing.regenerate();
    bridge::settings_view(&state)
}
#[tauri::command]
fn chrome_bridge_unpair(state: State<AppState>) -> Result<Value> {
    bridge::unpair(&state)?;
    Ok(bridge::settings_view(&state))
}
/// Optional import card; existing Gutenberg projects stay accessible.
fn experimental_gutenberg(store: &store::Store) -> bool {
    store.setting("experimental-gutenberg").as_deref() == Some("true")
}
#[tauri::command]
fn set_experimental_gutenberg(state: State<AppState>, enabled: bool) -> Result<bool> {
    state.store.set("experimental-gutenberg", if enabled { "true" } else { "false" })?;
    Ok(enabled)
}
/// Default number of conversions that may run side by side.
pub const DEFAULT_MAX_PARALLEL: usize = 2;
/// Highest parallel limit the owner can choose in Settings.
pub const MAX_PARALLEL_LIMIT: usize = 6;
/// The parallel limit in force: the owner's choice, or the default.
pub fn max_parallel(store: &store::Store) -> usize {
    store.setting("max-parallel").and_then(|v| v.parse::<usize>().ok())
        .filter(|n| (1..=MAX_PARALLEL_LIMIT).contains(n)).unwrap_or(DEFAULT_MAX_PARALLEL)
}
/// Settings: how many conversions may run at once. A lower limit only
/// affects conversions started from now on; running ones continue.
#[tauri::command]
fn set_max_parallel(state: State<AppState>, value: usize) -> Result<usize> {
    if !(1..=MAX_PARALLEL_LIMIT).contains(&value) {
        return Err(format!("Choose between 1 and {MAX_PARALLEL_LIMIT} conversions at once"));
    }
    state.store.set("max-parallel", &value.to_string())?;
    Ok(value)
}
/// Free disk space needed before another conversion starts next to a running
/// one: each run keeps its own WordPress preview and screenshots.
pub const PARALLEL_FREE_GB: u64 = 15;
fn parallel_room(state: &AppState, project_id: &str) -> Result<()> {
    let running: Vec<String> = state.running()?.into_iter().filter(|id| id != project_id).collect();
    if running.is_empty() { return Ok(()); }
    let limit = max_parallel(&state.store);
    if running.len() >= limit {
        return Err(format!("{} conversions are already running (limit {limit}). Start this one when one of them finishes.", running.len()));
    }
    if let Some(capacity) = disk_space::available(&state.store.root) {
        if let Some(message) = capacity.parallel_error(PARALLEL_FREE_GB) { return Err(message); }
    }
    Ok(())
}
/// Images the owner attached for the assistant, as data URLs for Codex.
/// Paths come from the native file dialog; only small raster images pass.
fn image_inputs(paths: &[String]) -> Result<Vec<Value>> {
    use base64::Engine;
    if paths.len() > 4 { return Err("Attach at most 4 images to one message".into()); }
    paths.iter().map(|path| {
        let file = std::path::Path::new(path);
        let ext = file.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).unwrap_or_default();
        let mime = match ext.as_str() {
            "png" => "image/png", "jpg" | "jpeg" => "image/jpeg", "webp" => "image/webp", "gif" => "image/gif",
            _ => return Err(format!("{} is not a PNG, JPEG, WebP or GIF image", file.display())),
        };
        let meta = std::fs::metadata(file).map_err(err)?;
        if !meta.is_file() || meta.len() > 10 * 1024 * 1024 { return Err(format!("{} must be an image file up to 10 MB", file.display())); }
        let data = base64::engine::general_purpose::STANDARD.encode(std::fs::read(file).map_err(err)?);
        Ok(json!({"type":"image","url":format!("data:{mime};base64,{data}")}))
    }).collect()
}

/// Start a Codex turn for a project whose `active` slot is already claimed.
/// `goal`: a conversion run, which Codex continues on its own until the host
/// has the skill's result; otherwise one turn answering the owner. `text` is
/// what Codex gets, `shown` what the chat shows.
pub(crate) async fn start_turn(
    app: &tauri::AppHandle,
    state: &AppState,
    rpc: Arc<codex::Rpc>,
    project_id: &str,
    (text, shown): (&str, &str),
    goal: bool,
    images: &[String],
) -> Result<Value> {
    let attached = image_inputs(images)?;
    let mut p = state.store.project(project_id)?;
    if p.archived { return Err("Restore this project to the workspace before continuing".into()); }
    let account = rpc.request("account/read", json!({})).await?;
    if account["account"].is_null() { return Err("Connect your ChatGPT account first".into()); }
    if let Ok(limits) = rpc.request("account/rateLimits/read", json!({})).await {
        if limits["ordinaryUsageAllowed"] == false { return Err("Codex reports that included usage is currently unavailable. Review your account before continuing.".into()); }
    }
    let catalog = models::catalog(&rpc).await?;
    // Each project keeps its own model; one without a choice yet takes the
    // Settings default once and keeps it, so a later change of the default
    // never switches a conversion that is already under way.
    let (chosen, chosen_effort) = models::project_choice(&state.store, &p);
    let model = models::resolve(&catalog, &chosen)?;
    let effort = models::resolve_effort(&catalog, &model, &chosen_effort)?;
    if p.model.as_deref().unwrap_or("").is_empty() { p.model = Some(model.clone()); }
    refresh_runtime(app, state, &mut p).await;
    p.auto_approve = goal;
    if goal && p.phase != "deliverable_ready" { p.phase = "running".into(); p.last_error = None; }
    state.store.set(&format!("auto-stop:{}", p.id), "no")?;
    let tools = agent::tool_specs(&p);
    let mut params = json!({"cwd":"/home/agent/empty","sandbox":"read-only","approvalPolicy":codex::APPROVAL_POLICY,"developerInstructions":agent::instructions(&p),"dynamicTools":tools});
    models::apply_selection(&mut params, &model);
    models::apply_effort(&mut params, &effort, false);
    // A thread keeps the tools it started with: one started with other tools
    // (an earlier release's) is not resumed; a new one starts.
    let tools_key = format!("thread-tools:{}", p.id);
    let tools_id = files::hash_bytes(tools.to_string().as_bytes());
    if state.store.setting(&tools_key).as_deref() != Some(tools_id.as_str()) { p.thread_id = None; }
    let thread = if let Some(id) = &p.thread_id {
        params.as_object_mut().unwrap().remove("dynamicTools");
        params["threadId"] = json!(id);
        params["excludeTurns"] = json!(true);
        rpc.request("thread/resume", params).await?
    } else {
        rpc.request("thread/start", params).await?
    };
    let tid = thread["thread"]["id"].as_str().ok_or("Codex returned no thread ID")?.to_string();
    state.store.set(&tools_key, &tools_id)?;
    // Resuming a thread restarts an active goal at once; a chat turn must not.
    if !goal { let _ = rpc.request("thread/goal/clear", json!({"threadId":tid})).await; }
    p.thread_id = Some(tid.clone());
    p.updated_at = now();
    state.store.put(&p)?;
    let shown = if images.is_empty() { shown.to_string() } else {
        format!("{shown}\n\n📎 {}", images.iter().filter_map(|i| Path::new(i).file_name()).map(|n| n.to_string_lossy().into_owned()).collect::<Vec<_>>().join(", "))
    };
    let message = state.store.message(&p.id, "user", &shown)?;
    let _ = app.emit("chat-message", &message);
    let _ = app.emit("project-updated", &p);
    let mut input = vec![json!({"type":"text","text":text,"text_elements":[]})];
    input.extend(attached);
    let mut turn_params = json!({"threadId":tid,"input":input});
    models::apply_selection(&mut turn_params, &model);
    models::apply_effort(&mut turn_params, &effort, true);
    let turn = rpc.request("turn/start", turn_params).await?;
    let _ = run_context::selection(&state.store, &p, &model, &effort);
    // A run is a Codex thread goal: Codex keeps starting turns until the host
    // confirms the skill's result. A chat turn must not auto-continue.
    if goal {
        rpc.request("thread/goal/set", json!({"threadId":tid,"objective":agent::goal_objective(&state.store, &p),"status":"active"})).await?;
    }
    let activity = state.store.activity(&p.id, &format!("Codex model: {model} · effort: {effort}"), "complete")?;
    let _ = app.emit("activity", activity);
    state.store.set(&format!("turn:{}", p.id), turn["turn"]["id"].as_str().unwrap_or(""))?;
    Ok(turn)
}

/// After a goal turn ended without the skill's result. Returns true while
/// the Codex goal keeps the run going (Codex starts the next turn itself).
pub(crate) async fn goal_after_turn(app: &tauri::AppHandle, state: &AppState, pid: &str) -> Result<bool> {
    let mut p = state.store.project(pid)?;
    let Some(tid) = p.thread_id.clone() else { return Ok(false) };
    let rpc = state.rpc.lock().map_err(err)?.clone().ok_or("Codex disconnected. Choose Continue to resume the run.")?;
    let goal = rpc.request("thread/goal/get", json!({"threadId":tid})).await?;
    let status = goal["goal"]["status"].as_str().unwrap_or("complete").to_string();
    // Progress since the last resume restarts the count: only a run that is
    // truly standing still is ever stopped by the host.
    let signature = agent::progress_signature(&state.store, &p);
    let mut reactivations = state.store.setting(&format!("goal-reactivations:{pid}")).and_then(|v| v.parse::<u32>().ok()).unwrap_or(0);
    if state.store.setting(&format!("goal-progress:{pid}")).as_deref() != Some(signature.as_str()) {
        reactivations = 0;
        state.store.set(&format!("goal-progress:{pid}"), &signature)?;
    }
    // A repair-stop that repaired the stop goes on as the ordinary Continue, in the run's own mode (v1.4).
    if skill::repaired(&state.store, &p) {
        skill::set_change_turn(&state.store, &p, false)?;
        let objective = skill::continue_text(&state.store, &p);
        state.store.set(&format!("goal:{pid}"), &objective)?;
        if status == "active" { rpc.request("thread/goal/set", json!({"threadId":tid,"objective":objective,"status":"active"})).await?; }
    }
    let failed = agent::run_failed(&state.store, &p);
    match agent::goal_followup(&status, reactivations, failed.as_deref()) {
        agent::GoalFollowup::KeepRunning => {
            // Once the plugin began this run, the goal says continue: a turn
            // Codex starts on its own never reads "a new run" again.
            if let Some(objective) = agent::continuing_objective(&state.store, &p) {
                rpc.request("thread/goal/set", json!({"threadId":tid,"objective":objective,"status":"active"})).await?;
                state.store.set(&format!("goal:{pid}"), &objective)?;
            }
            Ok(true)
        }
        agent::GoalFollowup::Reactivate(note) => {
            state.store.set(&format!("goal-reactivations:{pid}"), &(reactivations + 1).to_string())?;
            let objective = format!("{}\n\n{note}", agent::resume_objective(&state.store, &p));
            rpc.request("thread/goal/set", json!({"threadId":tid,"objective":objective,"status":"active"})).await?;
            let label = format!("Run resumed by the host: no result from the skill yet ({} without progress, limit {})", reactivations + 1, agent::GOAL_REACTIVATION_LIMIT);
            if let Ok(activity) = state.store.activity(pid, &label, "complete") { let _ = app.emit("activity", activity); }
            Ok(true)
        }
        agent::GoalFollowup::Stop(reason) => {
            let _ = rpc.request("thread/goal/set", json!({"threadId":tid,"status":"paused"})).await;
            p.auto_approve = false;
            if failed.is_some() {
                p.phase = "failed".into();
                p.last_error = Some(reason.clone());
                if !p.from_theme() { skill::keep_report(&state.store, &mut p)?; }
            }
            state.store.put(&p)?;
            let _ = app.emit("project-updated", &p);
            let message = state.store.message(pid, "assistant", &reason)?;
            let _ = app.emit("chat-message", &message);
            Ok(false)
        }
    }
}

/// Codex starts the next goal turn itself. If none starts, nudge the goal once
/// and then end the run visibly instead of leaving the UI "running".
pub(crate) fn goal_watchdog(app: tauri::AppHandle, pid: String, ended_turn: String) {
    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let stalled = |state: &AppState| state.store.setting(&format!("turn:{pid}")).as_deref() == Some(ended_turn.as_str())
            && state.is_running(&pid).unwrap_or(false);
        for attempt in 0..2 {
            tokio::time::sleep(std::time::Duration::from_secs(90)).await;
            if !stalled(&state) { return; }
            if attempt == 0 {
                let tid = state.store.project(&pid).ok().and_then(|p| p.thread_id);
                let rpc = state.rpc.lock().ok().and_then(|r| r.clone());
                if let (Some(tid), Some(rpc)) = (tid, rpc) {
                    let _ = rpc.request("thread/goal/set", json!({"threadId":tid,"status":"active"})).await;
                }
            }
        }
        state.release(&pid);
        let text = "The run paused: Codex did not start its next step. Choose Continue to resume from where it stopped.";
        if let Ok(message) = state.store.message(&pid, "assistant", text) { let _ = app.emit("chat-message", &message); }
        let _ = app.emit("turn-completed", json!({"projectId":pid,"error":text}));
    });
}

/// Mark the goal done once the host has the skill's result.
pub(crate) async fn goal_delivered(state: &AppState, pid: &str) {
    let Ok(p) = state.store.project(pid) else { return };
    let (Some(tid), Ok(Some(rpc))) = (p.thread_id, state.rpc.lock().map(|r| r.clone())) else { return };
    let _ = rpc.request("thread/goal/set", json!({"threadId":tid,"status":"complete"})).await;
}

#[tauri::command]
async fn stop_conversion(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> Result<()> {
    stop_project(&app, &state, project_id).await
}

async fn stop_project(app: &tauri::AppHandle, state: &AppState, project_id: String) -> Result<()> {
    // Stopping one project never touches the others that are running.
    let p = state.store.project(&project_id)?;
    stop_signals(state, &p.id)?;
    // Codex is told in the background; local work ends now.
    if let (Some(tid), Some(rpc)) = (p.thread_id.clone(), state.rpc.lock().map_err(err)?.clone()) {
        let turn = state.store.setting(&format!("turn:{}", p.id));
        tauri::async_runtime::spawn(async move {
            // A paused goal does not start another turn after the interrupt.
            let _ = rpc.request("thread/goal/set", json!({"threadId":tid,"status":"paused"})).await;
            if let Some(turn) = turn {
                let _ = rpc.request("turn/interrupt", json!({"threadId":tid,"turnId":turn})).await;
            }
        });
    }
    // Stopping the project's container ends every command and server of the
    // run, so a tool call in flight answers at once.
    if !p.from_theme() { skill::stop_container(&p.id).await?; }
    let locked = tokio::time::timeout(std::time::Duration::from_secs(20), state.lock_project(&project_id)).await;
    match locked {
        Ok(lock) => {
            let _tools = lock?;
            mark_stopped(state, &project_id, |p| { let _ = app.emit("project-updated", p); })?;
        }
        Err(_) => {
            // A command still finishing is marked stopped when it ends,
            // unless the owner has started the project again meanwhile.
            let app = app.clone();
            tauri::async_runtime::spawn(async move {
                let state = app.state::<AppState>();
                let Ok(_tools) = state.lock_project(&project_id).await else { return };
                if state.store.setting(&format!("auto-stop:{project_id}")).as_deref() == Some("yes") {
                    let _ = mark_stopped(&state, &project_id, |p| { let _ = app.emit("project-updated", p); });
                }
            });
        }
    }
    Ok(())
}

/// What Stop changes at once, before any command or Codex has answered: no
/// tool call of this conversation runs from here on (they are answered
/// "Conversation was stopped") and a goal turn Codex starts is not claimed.
fn stop_signals(state: &AppState, pid: &str) -> Result<()> {
    state.store.set(&format!("auto-stop:{pid}"), "yes")?;
    state.release(pid);
    Ok(())
}

/// The stopped project, recorded once its running command has ended.
fn mark_stopped(state: &AppState, pid: &str, emit: impl FnOnce(&Project)) -> Result<()> {
    let mut p = state.store.project(pid)?;
    p.auto_approve = false;
    if p.phase != "deliverable_ready" { p.phase = "interrupted".into(); }
    state.store.put(&p)?;
    state.release(pid);
    emit(&p);
    Ok(())
}

/// A goal turn Codex started on its own after the plugin stopped the run
/// (its stop is on disk): not the owner's, so none of its tools run.
pub(crate) fn stray_goal_turn(state: &AppState, p: &Project) -> bool {
    p.auto_approve && agent::run_failed(&state.store, p).is_some()
}
/// Interrupts that turn and pauses the goal. The stop itself is handled when
/// the turn that wrote it completes (goal_after_turn).
pub(crate) fn end_stray_turn(app: &tauri::AppHandle, p: &Project, turn: &str) {
    let state = app.state::<AppState>();
    let (Some(tid), Some(rpc)) = (p.thread_id.clone(), state.rpc.lock().ok().and_then(|r| r.clone())) else { return };
    if let Ok(activity) = state.store.activity(&p.id, "The plugin stopped the run: a turn Codex started after the stop was ended", "complete") { let _ = app.emit("activity", activity); }
    let turn = turn.to_string();
    tauri::async_runtime::spawn(async move {
        let _ = rpc.request("thread/goal/set", json!({"threadId":tid,"status":"paused"})).await;
        let _ = rpc.request("turn/interrupt", json!({"threadId":tid,"turnId":turn})).await;
    });
}
/// A goal continuation turn is started by Codex, not by the owner; it runs
/// only in a conversion run the owner has not stopped.
pub(crate) fn claims_goal_turn(state: &AppState, p: &Project) -> bool {
    p.auto_approve && state.store.setting(&format!("auto-stop:{}", p.id)).as_deref() != Some("yes")
}

#[tauri::command]
async fn pending_question(state: State<'_, AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let requests = state.requests.lock().await;
    Ok(requests
        .iter()
        .find(|(_, v)| v["params"]["threadId"].as_str() == p.thread_id.as_deref())
        .map(|(k, v)| json!({"id":k,"params":v["params"]}))
        .unwrap_or(Value::Null))
}
#[tauri::command]
async fn answer_question(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request_id: String,
    answers: Value,
) -> Result<()> {
    let request = state
        .requests
        .lock()
        .await
        .remove(&request_id)
        .ok_or("This question is no longer active")?;
    state
        .connection(&app)
        .await?
        .send(json!({"id":request["id"],"result":{"answers":answers}}))
        .await
}
/// The newest Visual Edit Lite (downloaded or cached) for the Exports tab.
#[tauri::command]
async fn visual_edit_lite(state: State<'_, AppState>) -> Result<Value> {
    let _ = editor::latest(&state.store).await;
    Ok(editor::status(&state.store))
}
#[tauri::command]
async fn export_visual_edit_lite(state: State<'_, AppState>, destination: String) -> Result<()> {
    let (_, source) = editor::latest(&state.store).await?;
    let dest = Path::new(&destination);
    if dest.starts_with(&state.store.root) {
        return Err("Choose an export destination outside the application's private data".into());
    }
    std::fs::copy(source, dest).map_err(err)?;
    Ok(())
}
/// The page-by-page comparison the plugin made last: its pages, or {}.
#[tauri::command]
fn compare_index(state: State<AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    Ok(if p.from_theme() { h2g_compare::index(&state.store, &p) } else { compare::index(&state.store, &p) })
}
/// One side-by-side image of the comparison, as a data URL.
#[tauri::command]
fn compare_image(state: State<AppState>, project_id: String, path: String) -> Result<String> {
    let p = state.store.project(&project_id)?;
    if p.from_theme() { h2g_compare::image(&state.store, &p, &path) } else { compare::image(&state.store, &p, &path) }
}
/// Where the comparison stands: the plugin's status, whether the app is
/// running one now, and why the last one could not run, if it could not.
#[tauri::command]
fn compare_status(state: State<AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let running = state.store.setting(&format!("comparing:{project_id}")).as_deref() == Some("yes") && state.is_running(&project_id)?;
    let mut found = if p.from_theme() { h2g_compare::status(&state.store, &p, running) } else { compare::status(&state.store, &p, running) };
    found["error"] = json!(state.store.setting(&format!("compare-error:{project_id}")).filter(|e| !e.is_empty()));
    Ok(found)
}
/// Generate comparison: the plugin's command in the project container, in
/// the background (the owner polls compare_status). The project is held
/// meanwhile, so no run or message starts in the middle of it.
#[tauri::command]
async fn compare_generate(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, desktop_only: Option<bool>, page_key: Option<String>) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    if !matches!(p.target.as_str(), "html" | "astro") && !p.from_theme() { return Err("The comparison compares a site with its WordPress theme or its built Astro site.".into()); }
    if state.is_running(&project_id)? { return Err("Wait for the conversion to finish, or stop it, before comparing.".into()); }
    drop(state.try_lock_project(&project_id, "Wait for this project's current step to finish before comparing")?);
    if !state.claim(&project_id)? { return Err("Wait for the conversion to finish, or stop it, before comparing.".into()); }
    state.store.set(&format!("comparing:{project_id}"), "yes")?;
    state.store.set(&format!("compare-error:{project_id}"), "")?;
    let app2 = app.clone();
    let mut p = p;
    if refresh_runtime(&app, &state, &mut p).await { let _ = state.store.put(&p); let _ = app.emit("project-updated", &p); }
    let image = state.image();
    tauri::async_runtime::spawn(async move {
        let state = app2.state::<AppState>();
        // Gutenberg from an HTML theme: the h2g skill's own render-original.py and visual-diff.py.
        let result = if p.from_theme() { h2g_compare::run(&state.store, &p, &image, page_key.as_deref()).await } else { compare::run(&state.store, &p, &image, desktop_only.unwrap_or(false), page_key.as_deref()).await };
        let _ = state.store.set(&format!("comparing:{}", p.id), "no");
        let _ = state.store.set(&format!("compare-error:{}", p.id), result.as_ref().err().map_or("", String::as_str));
        state.release(&p.id);
        let label = match &result { Ok(found) => format!("Comparison generated: {} pages", found["pages"].as_array().map_or(0, Vec::len)), Err(e) => e.clone() };
        if let Ok(activity) = state.store.activity(&p.id, &label, if result.is_ok() { "complete" } else { "failed" }) { let _ = app2.emit("activity", activity); }
        let _ = app2.emit("compare-finished", json!({"projectId":p.id,"error":result.err()}));
    });
    Ok(json!({"started":true}))
}
/// The preview WordPress the plugin started: its address, login and state.
#[tauri::command]
async fn preview_status(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> Result<Value> {
    adopt_stored(&app, &state, &project_id).await;
    let p = state.store.project(&project_id)?;
    // Gutenberg from an HTML theme: the skill's own sandbox WordPress.
    if p.from_theme() { return Ok(h2g_preview::status(&state.store, &p).await); }
    Ok(preview::status(&state.store, &p).await)
}
/// Make release: the plugin packages the live theme in the project's container,
/// no AI; a changed theme is the next revision in Exports.
#[tauri::command]
async fn package_theme(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String) -> Result<Value> {
    let busy = "Wait for the change to finish, or stop it, before making a release.";
    if state.is_running(&project_id)? { return Err(busy.into()); }
    let _work = state.try_lock_project(&project_id, busy)?;
    if !state.claim(&project_id)? { return Err(busy.into()); }
    let mut p = match state.store.project(&project_id) { Ok(p) => p, Err(e) => { state.release(&project_id); return Err(e); } };
    if refresh_runtime(&app, &state, &mut p).await { let _ = state.store.put(&p); }
    let emit = |event: &str, v: Value| { let _ = app.emit(event, v); };
    let packaged = if p.from_theme() { h2g::release(&state.store, &mut p, &emit) } else { skill::package(&state.store, &mut p, &state.image(), &emit).await };
    state.release(&project_id);
    packaged
}
/// Open the preview site or WordPress admin in the owner's default browser.
#[tauri::command]
async fn preview_open(state: State<'_, AppState>, project_id: String, page: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    if !matches!(page.as_str(), "site" | "admin") { return Err("Unknown preview page".into()); }
    let url = if p.from_theme() { h2g_preview::url(&state.store, &p, &page)? } else {
        let site = preview::status(&state.store, &p).await;
        let base = site["url"].as_str().filter(|_| site["running"] == true).ok_or("Start the preview first.")?;
        if page == "admin" { format!("{base}/wp-admin/") } else { base.to_string() }
    };
    let mut opened = preview_browser_status();
    if !p.from_theme() {
        match skill::prepare_preview_editor(&state.store, &p, &state.image()).await {
            Ok(result) if result["status"] == "not_installed" => opened["note"] = json!("Visual Edit is not installed in this preview."),
            Ok(_) => {},
            Err(error) => {
                let _ = state.store.activity(&p.id, &format!("Visual Edit activation: {}",project_downloads::redact(&error)), "failed");
                opened["note"] = json!("The preview opened, but Visual Edit activation could not be confirmed. Check the conversion diagnostics.");
            },
        }
    }
    system_open(&url)?;
    Ok(opened)
}
/// Opening a preview uses the OS default browser, without managed extensions.
#[tauri::command]
fn preview_browser_status() -> Value {
    json!({"browser":"default","extension":null,"note":null})
}
/// Start or stop the plugin's preview WordPress; nothing in it is lost.
#[tauri::command]
async fn preview_action(app: tauri::AppHandle, state: State<'_, AppState>, project_id: String, action: String) -> Result<Value> {
    adopt_stored(&app, &state, &project_id).await;
    let p = state.store.project(&project_id)?;
    if p.from_theme() {
        return match action.as_str() {
            "start" => h2g_preview::start(&state.store, &p, &state.image()).await,
            "stop" => { h2g_preview::stop(&p.id); Ok(h2g_preview::status(&state.store, &p).await) }
            _ => Err("Unknown preview action".into()),
        };
    }
    let status = preview::action(&state.store, &p, &action).await?;
    if action == "start" { skill::prepare_preview_editor(&state.store, &p, &state.image()).await?; }
    Ok(status)
}
#[tauri::command]
async fn download_project(state: State<'_, AppState>, project_id: String, kind: String, destination: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let root = state.store.root.clone();
    tokio::task::spawn_blocking(move || {
        let store = store::Store::open(root)?;
        match kind.as_str() {
            "original" => project_downloads::original(&store, &p, Path::new(&destination)),
            "diagnostics" => project_downloads::diagnostics(&store, &p, Path::new(&destination)),
            _ => Err("Unknown project download".into()),
        }
    }).await.map_err(err)?
}
#[tauri::command]
fn export_artifact(
    state: State<AppState>,
    project_id: String,
    artifact_id: String,
    destination: String,
) -> Result<()> {
    let p = state.store.project(&project_id)?;
    exports::save(&state.store, &p, &artifact_id, Path::new(&destination))
}
#[tauri::command]
fn delete_previous_artifact(
    app: tauri::AppHandle,
    state: State<AppState>,
    project_id: String,
    artifact_id: String,
) -> Result<Project> {
    let _work = state.try_lock_project(&project_id, "Wait for this project's current step to finish before deleting a previous version")?;
    if !state.claim(&project_id)? {
        return Err("Finish or stop this project's running conversion before deleting a previous version".into());
    }
    let result = state.store.project(&project_id).and_then(|p| exports::delete_previous(&state.store, p, &artifact_id));
    state.release(&project_id);
    if let Ok(ref p) = result { let _ = app.emit("project-updated", p); }
    result
}
#[tauri::command]
async fn open_url(url: String) -> Result<()> {
    let parsed = url::Url::parse(&url).map_err(err)?;
    let host = parsed.host_str().unwrap_or("");
    if !(parsed.scheme() == "http" && ["localhost", "127.0.0.1"].contains(&host)
        || parsed.scheme() == "https"
            && [
                "auth.openai.com",
                "chatgpt.com",
                "www.docker.com",
                "docs.docker.com",
                "html2wp.dev",
            ]
            .contains(&host))
    {
        return Err("This address is not an allowed application destination".into());
    }
    system_open(&url)
}
/// The address in the owner's default browser.
fn system_open(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    let mut c = {
        let mut c = tokio::process::Command::new("/usr/bin/open");
        c.arg(url);
        c
    };
    #[cfg(target_os = "windows")]
    let mut c = {
        let mut c = tokio::process::Command::new("rundll32.exe");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    };
    #[cfg(target_os = "linux")]
    let mut c = {
        let mut c = tokio::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    c.spawn().map_err(err)?;
    Ok(())
}
pub fn run() {
    tauri::Builder::default().plugin(tauri_plugin_dialog::init()).plugin(tauri_plugin_notification::init()).plugin(tauri_plugin_updater::Builder::new().build()).setup(|app|{
 notifications::listen(app.handle());
 let root=app.path().app_data_dir()?;let resources=if cfg!(debug_assertions){PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../runtime")}else{app.path().resource_dir()?.join("runtime")};
 let versions:Value=serde_json::from_str(include_str!("../../runtime/versions.json"))?;
 let store=store::Store::open(root).map_err(std::io::Error::other)?;for mut p in store.projects().map_err(std::io::Error::other)?{if ["running","preparing","converting_remote","verifying","packaging"].contains(&p.phase.as_str()){p.phase="interrupted".into();p.last_error=Some("The application closed while the conversion ran. Choose Continue to resume it.".into());}p.auto_approve=false;if let Some(preview)=&mut p.preview{preview.running=false;}store.put(&p).map_err(std::io::Error::other)?;}
 app.manage(AppState::new(store,resources,versions));
 // A Codex container left from an earlier runtime is replaced at start (the account volume stays),
 // and every project moves to the installed runtime.
 let started=app.handle().clone();tauri::async_runtime::spawn(async move{let state=started.state::<AppState>();let _=state.refresh_agent(None).await;for p in state.store.projects().unwrap_or_default(){adopt_stored(&started,&state,&p.id).await;}});
 // The Chrome extension bridge; without a free port the app runs on without it.
 let handle=app.handle().clone();tauri::async_runtime::spawn(async move{match bridge::bind().await{Some(listener)=>{if let (Ok(addr),Ok(mut port))=(listener.local_addr(),handle.state::<AppState>().pairing.port.lock()){*port=Some(addr.port());}bridge::serve(listener,AppBridge(handle)).await}None=>eprintln!("Chrome extension bridge: ports {}-{} are in use; the extension cannot connect",bridge::PORTS.start(),bridge::PORTS.end())}});Ok(())
 }).on_window_event(|window,event|{if let tauri::WindowEvent::CloseRequested{api,..}=event{if window.state::<AppState>().any_running().unwrap_or(true){api.prevent_close();let _=window.emit("close-active",json!({}));}}}).invoke_handler(tauri::generate_handler![app_update::check_app_update,app_update::install_app_update,get_bootstrap,check_runtime,prepare_runtime,cancel_setup,accept_disclosure,save_licence,licence_status,use_free,save_queue,account_read,model_catalog,select_model,select_effort,set_max_parallel,set_experimental_gutenberg,start_run,h2g_progress,skill_progress,account_login,account_cancel,account_logout,import_project,set_project_target,project_detail,project_action,send_message,set_active_project,chrome_bridge_status,chrome_bridge_regenerate,chrome_bridge_unpair,stop_conversion,answer_question,pending_question,preview_status,preview_action,preview_open,preview_browser_status,package_theme,compare_index,compare_image,compare_status,compare_generate,export_artifact,delete_previous_artifact,download_project,visual_edit_lite,export_visual_edit_lite,open_url,cloudflare::cloudflare_site,cloudflare::cloudflare_status,cloudflare::cloudflare_login_start,cloudflare::cloudflare_open_login,cloudflare::cloudflare_login_wait,cloudflare::cloudflare_login_cancel,cloudflare::cloudflare_select_account,cloudflare::cloudflare_logout,cloudflare::cloudflare_deploy,cloudflare::cloudflare_remove,cloudflare::cloudflare_domain_check,cloudflare::cloudflare_open,site_preview::site_preview_status,site_preview::site_preview_start,site_preview::site_preview_stop]).run(tauri::generate_context!()).expect("Unable to start html2wp Desktop");
}

#[cfg(test)]
mod target_tests {
    use super::*;
    #[test]
    fn experimental_gutenberg_defaults_off_and_persists() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("settings");
        {
            let store = store::Store::open(root.clone()).unwrap();
            assert!(!experimental_gutenberg(&store));
            store.set("experimental-gutenberg", "true").unwrap();
        }
        {
            let store = store::Store::open(root.clone()).unwrap();
            assert!(experimental_gutenberg(&store));
            store.set("experimental-gutenberg", "false").unwrap();
        }
        assert!(!experimental_gutenberg(&store::Store::open(root).unwrap()));
    }

    #[test]
    fn theme_type_changes_only_before_the_conversion_starts() {
        let mut p: Project = serde_json::from_value(json!({"id":"0584dcf1-7f08-4efb-85cf-ae7284faf8f9","name":"t","sourceName":"t","kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"imported","revision":1,"threadId":null,"pages":[],"gates":[],"artifacts":[],
            "preview":null,"runtimeImage":"img","pluginCommit":"abc","reporting":"not_required","lastError":null})).unwrap();
        change_target(&mut p, "astro").unwrap();
        assert_eq!(p.target, "astro");
        assert!(change_target(&mut p, "classic").is_err());
        assert!(change_target(&mut p, "gutenberg").unwrap_err().contains("Gutenberg from an HTML theme"), "no Gutenberg target in this release");
        assert_eq!(p.target, "astro");
        change_target(&mut p, "html").unwrap();
        p.last_step = Some("analyze".into());
        assert!(change_target(&mut p, "astro").unwrap_err().contains("Clean & restart"));
        p.last_step = None;
        p.phase = "preparing".into();
        assert!(change_target(&mut p, "astro").is_err());
        assert_eq!(p.target, "html");
    }
}
#[cfg(test)]
mod parallel_tests {
    use super::*;
    fn state(root: &Path) -> AppState {
        AppState::new(store::Store::open(root.into()).unwrap(), root.into(), json!({}))
    }
    fn project(id: &str, model: Option<&str>, effort: Option<&str>) -> Project {
        serde_json::from_value(json!({"id":id,"name":id,"sourceName":id,"kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"imported","revision":1,"threadId":null,"pages":[],"gates":[],"artifacts":[],
            "preview":null,"runtimeImage":"img","pluginCommit":"abc","reporting":"not_required","lastError":null,
            "model":model,"effort":effort})).unwrap()
    }

    #[test]
    fn two_projects_run_at_once_and_stop_independently() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        assert!(s.claim("a").unwrap());
        assert!(s.claim("b").unwrap(), "a second project starts while the first runs");
        assert!(!s.claim("a").unwrap(), "the same project cannot run twice");
        assert_eq!(s.running().unwrap(), vec!["a".to_string(), "b".to_string()]);
        s.release("a");
        assert!(!s.is_running("a").unwrap());
        assert!(s.is_running("b").unwrap(), "stopping one project leaves the other running");
        assert!(s.any_running().unwrap());
    }

    #[tokio::test]
    async fn stop_ends_the_run_at_once_even_while_its_step_holds_the_lock() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        s.claim("a").unwrap();
        s.claim("b").unwrap();
        let step = s.lock_project("a").await.unwrap();
        // Nothing here waits for the running step or for Codex.
        stop_signals(&s, "a").unwrap();
        assert!(!s.is_running("a").unwrap(), "a queued tool call is answered 'Conversation was stopped'");
        assert!(s.is_running("b").unwrap(), "stopping one project leaves the other running");
        assert_eq!(s.store.setting("auto-stop:a").as_deref(), Some("yes"));
        assert_eq!(s.store.setting("auto-stop:b"), None);
        drop(step);
    }

    #[test]
    fn a_stopped_automatic_project_claims_no_goal_turn() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let mut p = project("a", None, None);
        assert!(!claims_goal_turn(&s, &p), "a manual project's turns start from send_message");
        p.auto_approve = true;
        assert!(claims_goal_turn(&s, &p));
        stop_signals(&s, "a").unwrap();
        assert!(!claims_goal_turn(&s, &p), "a goal turn Codex starts after Stop does not bring the run back");
        // Starting the conversion again clears the stop.
        s.store.set("auto-stop:a", "no").unwrap();
        assert!(claims_goal_turn(&s, &p));
    }

    #[test]
    fn a_goal_turn_codex_starts_after_the_plugin_s_stop_is_not_the_owner_s() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let mut p = crate::skill::tests::project();
        p.auto_approve = true;
        s.store.put(&p).unwrap();
        assert!(!stray_goal_turn(&s, &p), "a run going on");
        let progress = s.store.path(&p.id).unwrap().join(crate::skill::PROGRESS);
        std::fs::create_dir_all(progress.parent().unwrap()).unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        std::fs::write(&progress, json!({"schema":"h2wp-progress/1","mode":"flash","stage":"-1","label":"prepare the input","state":"stopped","note":"no DATABASE_URL","startedAt":now,"updatedAt":now}).to_string()).unwrap();
        assert!(stray_goal_turn(&s, &p), "the plugin stopped the run: a turn Codex starts now is not the owner's");
        // The owner's Continue (or chat) after the stop: its own turn goes on.
        let mut resumed = p.clone();
        std::thread::sleep(std::time::Duration::from_millis(10));
        s.store.set(&format!("resumed-at:{}", p.id), &(chrono::Utc::now() + chrono::Duration::seconds(10)).to_rfc3339()).unwrap();
        resumed.auto_approve = true;
        assert!(!stray_goal_turn(&s, &resumed));
        p.auto_approve = false;
        assert!(!stray_goal_turn(&s, &p), "a chat turn is never a goal turn");
    }

    #[test]
    fn a_stopped_run_is_recorded_interrupted_and_a_delivered_project_stays_delivered() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let mut p = project("0584dcf1-7f08-4efb-85cf-ae7284faf8f9", None, None);
        p.auto_approve = true;
        p.phase = "running".into();
        s.store.put(&p).unwrap();
        s.claim(&p.id).unwrap();
        let mut emitted = Vec::new();
        mark_stopped(&s, &p.id, |p| emitted.push(p.phase.clone())).unwrap();
        let stopped = s.store.project(&p.id).unwrap();
        assert_eq!(stopped.phase, "interrupted");
        assert!(!stopped.auto_approve, "a goal turn Codex starts later is not claimed");
        assert!(!s.is_running(&p.id).unwrap());
        assert_eq!(emitted, vec!["interrupted".to_string()]);
        // Stopping a chat turn after delivery keeps the delivery.
        let mut done = stopped;
        done.phase = "deliverable_ready".into();
        s.store.put(&done).unwrap();
        mark_stopped(&s, &done.id, |_| {}).unwrap();
        assert_eq!(s.store.project(&done.id).unwrap().phase, "deliverable_ready");
    }

    #[tokio::test]
    async fn one_projects_step_never_blocks_another_project() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let _a = s.lock_project("a").await.unwrap();
        assert!(s.try_lock_project("b", "busy").is_ok(), "project b's steps do not wait for project a");
        assert_eq!(s.try_lock_project("a", "busy").err().as_deref(), Some("busy"), "a project's own steps still run one at a time");
        assert!(s.try_lock_environment("env").is_err(), "the environment is never replaced under a running step");
    }

    #[tokio::test]
    async fn a_tool_call_waits_while_the_host_holds_the_project() {
        // Managing a project (Clean & restart, delete) holds it
        // (try_lock_project); every tool call of the assistant first awaits
        // lock_project (codex.rs), so none runs in the middle of it.
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        let managing = s.try_lock_project("a", "busy").unwrap();
        let waiting = tokio::time::timeout(std::time::Duration::from_millis(200), s.lock_project("a")).await;
        assert!(waiting.is_err(), "a tool call of the same project waits while the host holds it");
        drop(managing);
        assert!(tokio::time::timeout(std::time::Duration::from_millis(200), s.lock_project("a")).await.is_ok(), "and runs once it is done");
    }

    #[test]
    fn environment_maintenance_waits_for_every_running_project() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        assert!(s.try_lock_environment("env").is_ok());
        s.claim("a").unwrap();
        assert_eq!(s.try_lock_environment("env").err().as_deref(), Some("env"));
        s.release("a");
        let env = s.try_lock_environment("env").unwrap();
        assert!(s.try_lock_project("a", "busy").is_err(), "no project step starts during maintenance");
        assert!(chat_ready(&s, "a").unwrap_err().contains("app update"), "the chat and extension cannot start during updater installation");
        drop(env);
    }

    #[test]
    fn each_project_keeps_its_own_model_and_effort() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        s.store.set("selected-model", "model-default").unwrap();
        s.store.set(&models::effort_key("model-default"), "medium").unwrap();
        s.store.set(&models::effort_key("model-b"), "low").unwrap();
        let a = project("a", Some("model-a"), Some("high"));
        let b = project("b", Some("model-b"), None);
        let c = project("c", None, None);
        assert_eq!(models::project_choice(&s.store, &a), ("model-a".into(), "high".into()));
        assert_eq!(models::project_choice(&s.store, &b), ("model-b".into(), "low".into()), "no project effort: the model's saved effort");
        assert_eq!(models::project_choice(&s.store, &c), ("model-default".into(), "medium".into()), "no project model: the Settings default");
        // A new Settings default never switches a project that chose its own.
        s.store.set("selected-model", "model-new").unwrap();
        assert_eq!(models::project_choice(&s.store, &a).0, "model-a");
        assert_eq!(models::project_choice(&s.store, &c).0, "model-new");
    }

    #[test]
    fn parallel_runs_stop_at_the_configured_limit() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        assert!(parallel_room(&s, "a").is_ok(), "the first conversion always starts");
        s.claim("a").unwrap();
        s.store.set("max-parallel", "1").unwrap();
        let refused = parallel_room(&s, "b").unwrap_err();
        assert!(refused.contains("limit 1"), "{refused}");
        s.store.set("disclosure", "accepted").unwrap();
        assert_eq!(chat_ready(&s, "b").unwrap_err(), refused, "the typed chat and the extension give the parallel limit's reason");
        s.store.set("max-parallel", "3").unwrap();
        // The disk check may refuse on a full machine; the limit must not.
        if let Err(e) = parallel_room(&s, "b") { assert!(e.contains("disk space"), "{e}"); }
        assert!(parallel_room(&s, "a").is_ok(), "a running project's own next message is not counted twice");
    }

    #[test]
    fn the_parallel_limit_is_chosen_between_one_and_six() {
        let root = tempfile::tempdir().unwrap();
        let s = state(root.path());
        assert_eq!(max_parallel(&s.store), DEFAULT_MAX_PARALLEL);
        s.store.set("max-parallel", "6").unwrap();
        assert_eq!(max_parallel(&s.store), 6);
        // A value outside the range, however it got there, falls back.
        s.store.set("max-parallel", "7").unwrap();
        assert_eq!(max_parallel(&s.store), DEFAULT_MAX_PARALLEL);
        s.store.set("max-parallel", "0").unwrap();
        assert_eq!(max_parallel(&s.store), DEFAULT_MAX_PARALLEL);
    }
}
