//! WordPress previews created by the bundled test-env.sh. Its state file is
//! writable by the agent; Docker labels and the host project path must prove
//! ownership before the desktop app changes any preview resource.
use crate::{model::*, store::Store};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    future::Future,
    path::{Path, PathBuf},
};

pub const USER: &str = "admin";
pub const PASSWORD: &str = "admin123";

fn plugin_project(name: &str) -> bool {
    let Some(rest) = name.strip_prefix("h2wp-") else {
        return false;
    };
    let Some((slug, run)) = rest.rsplit_once('-') else {
        return false;
    };
    name.len() <= 100
        && !name.contains("clara-test")
        && valid_slug(slug)
        && run.len() == 6
        && run
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn valid_slug(slug: &str) -> bool {
    !slug.is_empty()
        && !slug.starts_with('-')
        && !slug.ends_with('-')
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}
fn local_url(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    let local = parsed.scheme() == "http"
        && matches!(parsed.host_str(), Some("localhost" | "127.0.0.1"))
        && parsed.port().is_some();
    local.then(|| url.trim_end_matches('/').to_string())
}

#[derive(Clone)]
struct PreviewState {
    path: PathBuf,
    value: Value,
    modified: std::time::SystemTime,
}

fn state_files(workspace: &Path) -> Vec<PreviewState> {
    let mut found: Vec<_> = std::fs::read_dir(workspace)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.starts_with(".test-env-") && n.ends_with(".json")
        })
        .filter(|e| {
            e.file_type().is_ok_and(|t| t.is_file())
                && e.metadata().is_ok_and(|m| m.len() <= 256 * 1024)
        })
        .filter_map(|e| {
            Some(PreviewState {
                path: e.path(),
                modified: e.metadata().ok()?.modified().ok()?,
                value: serde_json::from_slice(&std::fs::read(e.path()).ok()?).ok()?,
            })
        })
        .filter(|s| s.value["project"].as_str().is_some_and(plugin_project))
        .collect();
    found.sort_by(|a, b| b.modified.cmp(&a.modified));
    found
}
fn chosen_state(store: &Store, p: &Project) -> Option<PreviewState> {
    let workspace = store.path(&p.id).ok()?.join("workspace");
    let files = state_files(&workspace);
    let named = crate::skill::result(store, p)["preview"]["stateFile"]
        .as_str()
        .and_then(|f| {
            Path::new(f)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
        });
    named
        .and_then(|name| {
            files
                .iter()
                .find(|s| s.path.file_name().and_then(|n| n.to_str()) == Some(name.as_str()))
                .cloned()
        })
        .or_else(|| files.into_iter().next())
}
pub fn state(store: &Store, p: &Project) -> Option<Value> {
    Some(chosen_state(store, p)?.value)
}

async fn containers(project: &str, all: bool) -> Result<Vec<String>> {
    let mut args = vec![
        "ps".to_string(),
        "--filter".into(),
        format!("label=com.docker.compose.project={project}"),
        "--format".into(),
        "{{.ID}}".into(),
    ];
    if all {
        args.insert(1, "-a".into());
    }
    Ok(crate::runtime::docker(&args, None, 20)
        .await?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

pub async fn status(store: &Store, p: &Project) -> Value {
    let Some(found) = state(store, p) else {
        return json!({"available":false});
    };
    let Some(url) = found["url"].as_str().and_then(local_url) else {
        return json!({"available":false});
    };
    let project = found["project"].as_str().unwrap_or_default();
    let running = containers(project, false)
        .await
        .is_ok_and(|ids| !ids.is_empty());
    let login = |key: &str, fixed: &str| {
        found[key]
            .as_str()
            .filter(|v| !v.is_empty() && v.len() <= 64 && v.chars().all(|c| c.is_ascii_graphic()))
            .unwrap_or(fixed)
            .to_string()
    };
    json!({"available":true,"url":url,"user":login("user", USER),"password":login("password", PASSWORD),"running":running,"project":project})
}

fn refuse(why: &str) -> String {
    format!("Preview ownership could not be verified ({why}); no preview resources were changed.")
}

struct Claim {
    project: String,
    wp: String,
    db: String,
    network: String,
    owner: String,
    state_owner: bool,
    state_path: PathBuf,
    agent: String,
    project_path: PathBuf,
    /// No other project's workspace records this Compose project (`owned`
    /// checks); what lets an unlabelled legacy preview be attributed.
    unique_claim: bool,
}

fn owner_identity(workspace: &Path, agent: &str) -> Result<String> {
    let path = workspace
        .to_str()
        .ok_or_else(|| refuse("workspace path is not UTF-8"))?;
    // Python's json.dumps defaults to ensure_ascii=True. Match the plugin's
    // SHA256(JSON compact [canonical workspace path, H2WP_CONTAINER]).
    let json = serde_json::to_string(&[path, agent]).map_err(err)?;
    let mut ascii = String::new();
    for ch in json.chars() {
        if ch.is_ascii() {
            ascii.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]).iter() {
                ascii.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    Ok(format!("{:x}", Sha256::digest(ascii.as_bytes())))
}

fn claim(workspace: &Path, project_id: &str, state: &PreviewState) -> Result<Claim> {
    let canonical = workspace
        .canonicalize()
        .map_err(|_| refuse("workspace is missing"))?;
    if std::fs::symlink_metadata(workspace).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(refuse("workspace is a symbolic link"));
    }
    let meta =
        std::fs::symlink_metadata(&state.path).map_err(|_| refuse("state file is missing"))?;
    if !meta.is_file() || meta.file_type().is_symlink() || state.path.parent() != Some(workspace) {
        return Err(refuse("state file is outside the workspace"));
    }
    let canonical_state = state
        .path
        .canonicalize()
        .map_err(|_| refuse("state file disappeared"))?;
    if canonical_state.parent() != Some(canonical.as_path()) {
        return Err(refuse("state file resolves outside the workspace"));
    }
    let slug = state.value["slug"]
        .as_str()
        .filter(|s| valid_slug(s))
        .ok_or_else(|| refuse("invalid state slug"))?;
    let project = state.value["project"]
        .as_str()
        .filter(|s| {
            plugin_project(s)
                && s.strip_prefix(&format!("h2wp-{slug}-"))
                    .is_some_and(|run| run.len() == 6)
        })
        .ok_or_else(|| refuse("state project does not match its slug"))?;
    if state.path.file_name().and_then(|s| s.to_str()) != Some(&format!(".test-env-{slug}.json")) {
        return Err(refuse("state filename does not match its slug"));
    }
    let network = format!("{project}_default");
    if state.value["network"].as_str() != Some(&network) {
        return Err(refuse("state network is inconsistent"));
    }
    let named = |key: &str, service: &str| -> Result<String> {
        let name = state.value[key]
            .as_str()
            .ok_or_else(|| refuse("state container is missing"))?;
        if name != format!("{project}-{service}-1") && name != format!("{project}_{service}_1") {
            return Err(refuse("state container name is inconsistent"));
        }
        Ok(name.into())
    };
    let agent = format!("h2wpd-{project_id}-agent");
    let owner = owner_identity(&canonical, &agent)?;
    if !state.value["owner"].is_null() && state.value["owner"].as_str().is_none() {
        return Err(refuse("state owner is invalid"));
    }
    if state.value["owner"]
        .as_str()
        .is_some_and(|s| !s.is_empty() && s != owner)
    {
        return Err(refuse("state owner differs from this project"));
    }
    let state_owner = state.value["owner"].as_str() == Some(&owner);
    Ok(Claim {
        project: project.into(),
        wp: named("wpContainer", "wp")?,
        db: named("dbContainer", "db")?,
        network,
        owner,
        state_owner,
        state_path: canonical_state,
        agent,
        project_path: canonical
            .parent()
            .ok_or_else(|| refuse("invalid workspace path"))?
            .to_path_buf(),
        unique_claim: false,
    })
}

#[derive(Default)]
struct Resources {
    containers: Vec<Value>,
    volumes: Vec<Value>,
    networks: Vec<Value>,
    agent: Option<Value>,
}
fn labels(v: &Value, container: bool) -> &Value {
    if container {
        &v["Config"]["Labels"]
    } else {
        &v["Labels"]
    }
}
fn name(v: &Value) -> Option<&str> {
    v["Name"].as_str().map(|s| s.trim_start_matches('/'))
}
fn listed(raw: &str) -> BTreeSet<String> {
    raw.lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}
fn strs(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| (*s).into()).collect()
}
async fn inspect_with<F, Fut>(
    kind: &str,
    names: &BTreeSet<String>,
    run: &mut F,
) -> Result<Vec<Value>>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    if names.is_empty() {
        return Ok(vec![]);
    }
    let mut args = strs(&[kind, "inspect"]);
    args.extend(names.iter().cloned());
    let raw = run(args).await?;
    let rows: Vec<Value> =
        serde_json::from_str(&raw).map_err(|_| refuse("Docker inspection was invalid"))?;
    if rows.len() != names.len() {
        return Err(refuse("Docker resource listing changed"));
    }
    Ok(rows)
}
async fn resources_with<F, Fut>(c: &Claim, mut run: F) -> Result<Resources>
where
    F: FnMut(Vec<String>) -> Fut,
    Fut: Future<Output = Result<String>>,
{
    // Both queries return names. Joining IDs from -q with names would inspect
    // the same object twice and make a healthy preview look ambiguous.
    let mut names = listed(
        &run([
            strs(&["ps", "-a", "--filter"]),
            vec![format!("label=com.docker.compose.project={}", c.project)],
            strs(&["--format", "{{.Names}}"]),
        ]
        .concat())
        .await?,
    );
    let all = listed(&run(strs(&["ps", "-a", "--format", "{{.Names}}"])).await?);
    for n in [&c.wp, &c.db] {
        if all.contains(n) {
            names.insert(n.clone());
        }
    }
    let containers = inspect_with("container", &names, &mut run).await?;
    let agent = if all.contains(&c.agent) {
        inspect_with("container", &BTreeSet::from([c.agent.clone()]), &mut run)
            .await?
            .into_iter()
            .next()
    } else {
        None
    };
    let mut groups = Vec::new();
    for kind in ["volume", "network"] {
        let mut names = listed(
            &run([
                strs(&[kind, "ls", "--filter"]),
                vec![format!("label=com.docker.compose.project={}", c.project)],
                strs(&["--format", "{{.Name}}"]),
            ]
            .concat())
            .await?,
        );
        let all = listed(&run(strs(&[kind, "ls", "--format", "{{.Name}}"])).await?);
        let expected = if kind == "volume" {
            vec![
                format!("{}_db_data", c.project),
                format!("{}_wp_data", c.project),
            ]
        } else {
            vec![c.network.clone()]
        };
        for n in expected {
            if all.contains(&n) {
                names.insert(n);
            }
        }
        groups.push(inspect_with(kind, &names, &mut run).await?);
    }
    Ok(Resources {
        containers,
        volumes: groups.remove(0),
        networks: groups.remove(0),
        agent,
    })
}
async fn resources(c: &Claim) -> Result<Resources> {
    resources_with(c, |args| async move {
        crate::runtime::docker(&args, None, 20)
            .await
            .map_err(|_| refuse("Docker inspection failed"))
    })
    .await
}

struct Proof {
    containers: Vec<String>,
    volumes: Vec<String>,
    volume_rows: Vec<Value>,
    network: Option<String>,
    detach_agent: Option<String>,
}
fn docker_id(row: &Value) -> Result<String> {
    row["Id"]
        .as_str()
        .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_string)
        .ok_or_else(|| refuse("Docker resource has no stable ID"))
}
fn prove(c: &Claim, r: &Resources) -> Result<Proof> {
    let expected_state = c
        .state_path
        .to_str()
        .ok_or_else(|| refuse("state path is not UTF-8"))?;
    let alias = format!(
        "/project/workspace/{}",
        c.state_path.file_name().unwrap().to_string_lossy()
    );
    let mut owners = Vec::new();
    let mut container_names = Vec::new();
    let mut container_ids = Vec::new();
    for row in &r.containers {
        let n = name(row).ok_or_else(|| refuse("container has no name"))?;
        if n != c.wp && n != c.db || container_names.contains(&n.to_string()) {
            return Err(refuse("unexpected container in preview project"));
        }
        let l = labels(row, true);
        if l["com.docker.compose.project"] != c.project
            || l["com.docker.compose.service"] != if n == c.wp { "wp" } else { "db" }
        {
            return Err(refuse("container Compose identity differs"));
        }
        let path = l["h2wp.state"].as_str().unwrap_or("");
        if path != expected_state && path != alias {
            return Err(refuse("container state label differs"));
        }
        owners.push(l["h2wp.owner"].as_str().unwrap_or(""));
        container_names.push(n.to_string());
        container_ids.push(docker_id(row)?);
    }
    let mut volume_names = Vec::new();
    for row in &r.volumes {
        let n = name(row).ok_or_else(|| refuse("volume has no name"))?;
        if n != format!("{}_db_data", c.project) && n != format!("{}_wp_data", c.project)
            || volume_names.contains(&n.to_string())
        {
            return Err(refuse("unexpected volume in preview project"));
        }
        let l = labels(row, false);
        if l["com.docker.compose.project"] != c.project {
            return Err(refuse("volume Compose identity differs"));
        }
        owners.push(l["h2wp.owner"].as_str().unwrap_or(""));
        volume_names.push(n.to_string());
    }
    if r.networks.len() > 1 {
        return Err(refuse("unexpected network in preview project"));
    }
    let mut network = None;
    let mut detach_agent = None;
    for row in &r.networks {
        if name(row) != Some(&c.network)
            || labels(row, false)["com.docker.compose.project"] != c.project
        {
            return Err(refuse("network identity differs"));
        }
        owners.push(labels(row, false)["h2wp.owner"].as_str().unwrap_or(""));
        if let Some(attachments) = row["Containers"].as_object() {
            for (id, attached) in attachments {
                let n = attached["Name"]
                    .as_str()
                    .ok_or_else(|| refuse("network attachment has no name"))?;
                if n == c.agent {
                    detach_agent = Some(id.clone());
                } else if !container_names
                    .iter()
                    .zip(&container_ids)
                    .any(|(s, known)| s == n && known == id)
                {
                    return Err(refuse(
                        "unrelated container is attached to the preview network",
                    ));
                }
            }
        } else if !row["Containers"].is_null() {
            return Err(refuse("network attachments could not be read"));
        }
        network = Some(docker_id(row)?);
    }
    if let Some(attached_id) = &detach_agent {
        let agent = r
            .agent
            .as_ref()
            .ok_or_else(|| refuse("attached agent could not be inspected"))?;
        if name(agent) != Some(&c.agent)
            || docker_id(agent)? != *attached_id
            || labels(agent, true)["dev.html2wp.desktop"] != "true"
            || !agent["Mounts"].as_array().is_some_and(|mounts| {
                mounts.iter().any(|m| {
                    m["Source"]
                        .as_str()
                        .and_then(|s| Path::new(s).canonicalize().ok())
                        .as_deref()
                        == Some(c.project_path.as_path())
                })
            })
        {
            return Err(refuse("network attachment is not this project's agent"));
        }
    }
    if owners.iter().any(|o| !o.is_empty() && *o != c.owner) {
        return Err(refuse("Docker owner label differs"));
    }
    let modern = if owners.is_empty() {
        c.state_owner
    } else {
        owners.iter().all(|o| *o == c.owner)
    };
    if !modern {
        // Runs from before the plugin labelled an owner. test-env.sh names
        // the Compose project h2wp-<slug>-<random run id> and records it in
        // this workspace's state file; the resources above all carry that
        // project and this state file's path. That is unambiguous once no
        // other project's workspace records the same project. With nothing
        // left in Docker there is nothing to protect.
        if owners.iter().any(|o| !o.is_empty()) {
            return Err(refuse("legacy preview mixes owner-labelled and unlabelled resources"));
        }
        let nothing_left =
            container_names.is_empty() && volume_names.is_empty() && network.is_none();
        if !nothing_left && !c.unique_claim {
            return Err(refuse(
                "another project's state file names the same legacy preview",
            ));
        }
    }
    if !c.state_path.is_file() {
        return Err(refuse("state file disappeared"));
    }
    Ok(Proof {
        containers: container_ids,
        volumes: volume_names,
        volume_rows: r.volumes.clone(),
        network,
        detach_agent,
    })
}
async fn owned(store: &Store, p: &Project, state: &PreviewState) -> Result<Proof> {
    let workspace = store.path(&p.id)?.join("workspace");
    let mut c = claim(&workspace, &p.id, state)?;
    c.unique_claim = !claimed_elsewhere(store, &p.id, &c.project);
    let r = resources(&c).await?;
    prove(&c, &r)
}

/// Whether any other project's workspace has a state file naming `project`.
/// Unreadable project lists count as claimed: the check fails closed.
fn claimed_elsewhere(store: &Store, id: &str, project: &str) -> bool {
    let Ok(projects) = store.projects() else {
        return true;
    };
    projects.iter().filter(|other| other.id != id).any(|other| {
        store.path(&other.id).is_ok_and(|root| {
            state_files(&root.join("workspace"))
                .iter()
                .any(|s| s.value["project"] == project)
        })
    })
}

pub async fn action(store: &Store, p: &Project, action: &str) -> Result<Value> {
    let verb = match action {
        "start" => "start",
        "stop" => "stop",
        _ => return Err("Unknown preview action".into()),
    };
    let found = chosen_state(store, p)
        .ok_or("The conversion has not started its preview WordPress yet.")?;
    let proof = owned(store, p, &found).await?;
    if proof.containers.len() != 2 {
        return Err("The preview WordPress is incomplete or was removed. Choose Continue: the conversion starts it again.".into());
    }
    crate::runtime::docker(
        &[vec![verb.to_string()], proof.containers].concat(),
        None,
        90,
    )
    .await?;
    Ok(status(store, p).await)
}

pub async fn remove(store: &Store, p: &Project) -> Result<()> {
    let workspace = store.path(&p.id)?.join("workspace");
    // Prove the entire removal set before changing any resource.
    let mut proofs = Vec::new();
    for found in state_files(&workspace) {
        proofs.push(owned(store, p, &found).await?);
    }
    for proof in proofs {
        if !proof.containers.is_empty() {
            crate::runtime::docker(
                &[vec!["rm".into(), "-f".into()], proof.containers].concat(),
                None,
                90,
            )
            .await?;
        }
        if let Some(network) = &proof.network {
            let raw =
                crate::runtime::docker(&strs(&["network", "inspect", network]), None, 20).await?;
            let rows: Vec<Value> = serde_json::from_str(&raw)
                .map_err(|_| "Preview cleanup stopped: Docker network inspection was invalid.")?;
            if rows.len() != 1 || docker_id(&rows[0])? != *network {
                return Err("Preview cleanup stopped: the preview network changed.".into());
            }
            let attached = rows[0]["Containers"].as_object();
            if !rows[0]["Containers"].is_null() && attached.is_none() {
                return Err(
                    "Preview cleanup stopped: network attachments could not be read.".into(),
                );
            }
            let agent = format!("h2wpd-{}-agent", p.id);
            if attached.is_some_and(|items| {
                items
                    .iter()
                    .any(|(id, v)| Some(id) != proof.detach_agent.as_ref() || v["Name"] != agent)
            }) {
                return Err("Preview cleanup stopped because network attachments changed; review the remaining preview resources.".into());
            }
            if let Some(agent_id) = &proof.detach_agent {
                if attached.is_some_and(|items| items.contains_key(agent_id)) {
                    crate::runtime::docker(
                        &strs(&["network", "disconnect", "-f", network, agent_id]),
                        None,
                        30,
                    )
                    .await?;
                }
            }
        }
        if !proof.volumes.is_empty() {
            let raw = crate::runtime::docker(
                &[strs(&["volume", "inspect"]), proof.volumes.clone()].concat(),
                None,
                20,
            )
            .await?;
            let mut current: Vec<Value> = serde_json::from_str(&raw)
                .map_err(|_| "Preview cleanup stopped: Docker volume inspection was invalid.")?;
            let mut expected = proof.volume_rows;
            current.sort_by_key(|v| name(v).unwrap_or("").to_string());
            expected.sort_by_key(|v| name(v).unwrap_or("").to_string());
            if current != expected {
                return Err(
                    "Preview cleanup stopped: preview volumes changed before removal.".into(),
                );
            }
            crate::runtime::docker(
                &[vec!["volume".into(), "rm".into()], proof.volumes].concat(),
                None,
                60,
            )
            .await?;
        }
        if let Some(network) = proof.network {
            crate::runtime::docker(&strs(&["network", "rm", &network]), None, 60).await?;
        }
    }
    sweep_owner(store, p).await
}

/// Every preview resource labelled with this project's owner, from any run.
/// An earlier run whose state file was replaced or removed has no other link
/// to the project and would stay in Docker for good. The owner label is a hash
/// of this workspace and its agent, so it names this project's runs only.
async fn sweep_owner(store: &Store, p: &Project) -> Result<()> {
    let Ok(workspace) = store.path(&p.id)?.join("workspace").canonicalize() else {
        return Ok(());
    };
    let agent = format!("h2wpd-{}-agent", p.id);
    sweep(&owner_identity(&workspace, &agent)?, &agent).await
}
async fn sweep(owner: &str, agent: &str) -> Result<()> {
    let filter = vec!["--filter".to_string(), format!("label=h2wp.owner={owner}")];
    let mut run = |args: Vec<String>| async move { crate::runtime::docker(&args, None, 20).await };
    let ours = |row: &Value, container: bool| {
        let l = labels(row, container);
        l["h2wp.owner"] == owner
            && l["com.docker.compose.project"].as_str().is_some_and(plugin_project)
    };
    let stop = |what: &str| format!("Preview cleanup stopped: {what}; review it in Docker Desktop.");
    // Inspect the whole set before removing anything.
    let containers = inspect_with(
        "container",
        &listed(&run([strs(&["ps", "-a"]), filter.clone(), strs(&["--format", "{{.Names}}"])].concat()).await?),
        &mut run,
    )
    .await?;
    let networks = inspect_with(
        "network",
        &listed(&run([strs(&["network", "ls"]), filter.clone(), strs(&["--format", "{{.Name}}"])].concat()).await?),
        &mut run,
    )
    .await?;
    let volumes = inspect_with(
        "volume",
        &listed(&run([strs(&["volume", "ls"]), filter.clone(), strs(&["--format", "{{.Name}}"])].concat()).await?),
        &mut run,
    )
    .await?;
    if !containers.iter().all(|r| ours(r, true))
        || !networks.iter().all(|r| ours(r, false))
        || !volumes.iter().all(|r| ours(r, false))
    {
        return Err(stop("a resource carrying this project's owner label is not a plugin preview"));
    }
    let container_ids = containers.iter().map(docker_id).collect::<Result<Vec<_>>>()?;
    let mut detach = Vec::new();
    for row in &networks {
        if let Some(attached) = row["Containers"].as_object() {
            for (id, a) in attached {
                if container_ids.contains(id) {
                    continue;
                }
                if a["Name"] != agent {
                    return Err(stop("another container is attached to an old preview network"));
                }
                detach.push((docker_id(row)?, id.clone()));
            }
        } else if !row["Containers"].is_null() {
            return Err(stop("old preview network attachments could not be read"));
        }
    }
    if !container_ids.is_empty() {
        crate::runtime::docker(&[strs(&["rm", "-f"]), container_ids].concat(), None, 90).await?;
    }
    for (network, container) in &detach {
        crate::runtime::docker(&strs(&["network", "disconnect", "-f", network, container]), None, 30)
            .await?;
    }
    for row in &networks {
        crate::runtime::docker(&strs(&["network", "rm", &docker_id(row)?]), None, 60).await?;
    }
    let volume_names: Vec<String> = volumes.iter().filter_map(|v| name(v).map(str::to_string)).collect();
    if !volume_names.is_empty() {
        crate::runtime::docker(&[strs(&["volume", "rm"]), volume_names].concat(), None, 60).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    fn fixture() -> (tempfile::TempDir, String, PreviewState, Claim, Resources) {
        let dir = tempfile::tempdir().unwrap();
        let workspace = dir.path().join("workspace");
        std::fs::create_dir(&workspace).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let agent = format!("h2wpd-{id}-agent");
        let owner = owner_identity(&workspace.canonicalize().unwrap(), &agent).unwrap();
        let project = "h2wp-maison-abcdef";
        let path = workspace.join(".test-env-maison.json");
        let value = json!({"slug":"maison","project":project,"owner":owner,"wpContainer":format!("{project}-wp-1"),
            "dbContainer":format!("{project}-db-1"),"network":format!("{project}_default")});
        std::fs::write(&path, value.to_string()).unwrap();
        let state = PreviewState {
            path,
            value,
            modified: std::time::SystemTime::now(),
        };
        let c = claim(&workspace, &id, &state).unwrap();
        let wp_id = "a".repeat(64);
        let db_id = "b".repeat(64);
        let agent_id = "c".repeat(64);
        let network_id = "d".repeat(64);
        let container = |service: &str| {
            json!({"Id":if service == "wp" { &wp_id } else { &db_id },"Name":format!("/{project}-{service}-1"),"Config":{"Labels":{
            "com.docker.compose.project":project,"com.docker.compose.service":service,
            "h2wp.owner":owner,"h2wp.state":format!("/project/workspace/.test-env-maison.json")}}})
        };
        let volume = |service: &str| {
            json!({"Name":format!("{project}_{service}_data"),"Labels":{
            "com.docker.compose.project":project,"h2wp.owner":owner}})
        };
        let mut attachments = serde_json::Map::new();
        attachments.insert(wp_id.clone(), json!({"Name":format!("{project}-wp-1")}));
        attachments.insert(db_id.clone(), json!({"Name":format!("{project}-db-1")}));
        attachments.insert(agent_id.clone(), json!({"Name":agent}));
        let network = json!({"Id":network_id,"Name":format!("{project}_default"),"Labels":{
            "com.docker.compose.project":project,"h2wp.owner":owner},"Containers":attachments});
        let agent_row = json!({"Id":agent_id,"Name":format!("/{agent}"),"Config":{"Labels":{"dev.html2wp.desktop":"true"}},
            "Mounts":[{"Source":c.project_path.to_string_lossy()}]});
        let r = Resources {
            containers: vec![container("wp"), container("db")],
            volumes: vec![volume("wp"), volume("db")],
            networks: vec![network],
            agent: Some(agent_row),
        };
        (dir, id, state, c, r)
    }
    #[test]
    fn owner_hash_matches_the_plugin_even_with_unicode() {
        assert_eq!(
            owner_identity(Path::new("/tmp/workspace"), "h2wpd-123-agent").unwrap(),
            "53f5052d3535f2a5b325865d8edafe175605405e77a96e7643a8c81ec7d9d7e8"
        );
        assert_eq!(
            owner_identity(Path::new("/tmp/café/workspace"), "h2wpd-123-agent").unwrap(),
            "d08add60177d87e15cb456fd11887a145d0049a9ce02cbb00d0cfd8490810d5c"
        );
    }
    #[test]
    fn modern_preview_proves_every_resource_and_only_detaches_its_agent() {
        let (_dir, _id, _state, c, r) = fixture();
        let proof = prove(&c, &r).unwrap();
        assert_eq!(proof.containers.len(), 2);
        assert!(proof.containers.contains(&"a".repeat(64)));
        assert!(proof.containers.contains(&"b".repeat(64)));
        assert_eq!(proof.volumes.len(), 2);
        assert_eq!(proof.network.as_deref(), Some("d".repeat(64).as_str()));
        assert_eq!(proof.detach_agent.as_deref(), Some("c".repeat(64).as_str()));
        let empty = Resources::default();
        let already_removed = prove(&c, &empty).unwrap();
        assert!(already_removed.containers.is_empty() && already_removed.network.is_none());
    }
    #[tokio::test]
    async fn docker_name_inventory_flows_through_resources_and_proof_once() {
        let (_dir, _id, _state, c, expected) = fixture();
        let label = format!("label=com.docker.compose.project={}", c.project);
        let mut output: HashMap<Vec<String>, String> = HashMap::new();
        output.insert(
            strs(&["ps", "-a", "--filter", &label, "--format", "{{.Names}}"]),
            format!("{}\n{}\n{}\n", c.wp, c.db, c.wp),
        );
        output.insert(
            strs(&["ps", "-a", "--format", "{{.Names}}"]),
            format!("{}\n{}\n{}\nforeign-container\n", c.wp, c.db, c.agent),
        );
        output.insert(
            [
                strs(&["container", "inspect"]),
                vec![c.db.clone(), c.wp.clone()],
            ]
            .concat(),
            serde_json::to_string(&expected.containers).unwrap(),
        );
        output.insert(
            strs(&["container", "inspect", &c.agent]),
            serde_json::to_string(&vec![expected.agent.as_ref().unwrap()]).unwrap(),
        );
        let db_volume = format!("{}_db_data", c.project);
        let wp_volume = format!("{}_wp_data", c.project);
        output.insert(
            strs(&["volume", "ls", "--filter", &label, "--format", "{{.Name}}"]),
            format!("{db_volume}\n{wp_volume}\n"),
        );
        output.insert(
            strs(&["volume", "ls", "--format", "{{.Name}}"]),
            format!("{db_volume}\n{wp_volume}\nforeign-volume\n"),
        );
        output.insert(
            strs(&["volume", "inspect", &db_volume, &wp_volume]),
            serde_json::to_string(&expected.volumes).unwrap(),
        );
        output.insert(
            strs(&["network", "ls", "--filter", &label, "--format", "{{.Name}}"]),
            format!("{}\n", c.network),
        );
        output.insert(
            strs(&["network", "ls", "--format", "{{.Name}}"]),
            format!("{}\nbridge\n", c.network),
        );
        output.insert(
            strs(&["network", "inspect", &c.network]),
            serde_json::to_string(&expected.networks).unwrap(),
        );
        let actual = resources_with(&c, |args| {
            let response = output
                .remove(&args)
                .ok_or_else(|| format!("unexpected Docker query: {args:?}"));
            async move { response }
        })
        .await
        .unwrap();
        assert!(output.is_empty(), "every expected Docker query was used");
        let proof = prove(&c, &actual).unwrap();
        assert_eq!(proof.containers.len(), 2);
        assert_eq!(proof.volumes.len(), 2);
        assert!(proof.detach_agent.is_some());
    }
    #[test]
    fn forged_state_cannot_name_another_preview_or_network() {
        let (dir, id, mut state, _c, _r) = fixture();
        state.value["project"] = json!("h2wp-other-123456");
        assert!(claim(&dir.path().join("workspace"), &id, &state).is_err());
        state.value["project"] = json!("h2wp-maison-abcdef");
        state.value["network"] = json!("h2wp-other-123456_default");
        assert!(claim(&dir.path().join("workspace"), &id, &state).is_err());
        state.value["network"] = json!("h2wp-maison-abcdef_default");
        state.path = dir.path().join("workspace/.test-env-other.json");
        std::fs::write(&state.path, state.value.to_string()).unwrap();
        assert!(claim(&dir.path().join("workspace"), &id, &state).is_err());
    }
    #[test]
    fn mismatched_labels_or_orphan_resources_refuse_removal() {
        let (_dir, _id, _state, c, mut r) = fixture();
        r.containers[0]["Config"]["Labels"]["h2wp.owner"] = json!("foreign");
        assert!(prove(&c, &r).is_err());
        r.containers[0]["Config"]["Labels"]["h2wp.owner"] = json!(c.owner);
        r.containers[0]["Config"]["Labels"]["com.docker.compose.project"] =
            json!("h2wp-other-123456");
        assert!(
            prove(&c, &r).is_err(),
            "a recorded name can carry another project's label"
        );
        r.containers[0]["Config"]["Labels"]["com.docker.compose.project"] = json!(c.project);
        r.volumes.push(json!({"Name":"h2wp-maison-abcdef_unknown","Labels":{"com.docker.compose.project":c.project,"h2wp.owner":c.owner}}));
        assert!(prove(&c, &r).is_err());
        r.volumes.pop();
        r.networks[0]["Containers"]["foreign"] = json!({"Name":"someone-else"});
        assert!(prove(&c, &r).is_err());
        r.networks[0]["Containers"]
            .as_object_mut()
            .unwrap()
            .remove("foreign");
        r.networks[0]["Containers"]
            .as_object_mut()
            .unwrap()
            .remove(&"a".repeat(64));
        r.networks[0]["Containers"]
            .as_object_mut()
            .unwrap()
            .insert("e".repeat(64), json!({"Name":c.wp}));
        assert!(
            prove(&c, &r).is_err(),
            "a familiar name with a new Docker ID is foreign"
        );
        r.networks.clear();
        r.containers.clear();
        r.volumes[0]["Labels"]["h2wp.owner"] = json!("foreign");
        assert!(
            prove(&c, &r).is_err(),
            "an orphan volume still needs owner proof"
        );
    }
    fn legacy(r: &mut Resources, state_label: &str) {
        for row in &mut r.containers {
            row["Config"]["Labels"].as_object_mut().unwrap().remove("h2wp.owner");
            row["Config"]["Labels"]["h2wp.state"] = json!(state_label);
        }
        for row in &mut r.volumes {
            row["Labels"].as_object_mut().unwrap().remove("h2wp.owner");
        }
        r.networks[0]["Labels"].as_object_mut().unwrap().remove("h2wp.owner");
    }
    #[test]
    fn legacy_preview_is_attributed_when_no_other_project_records_it() {
        let (_dir, _id, _state, mut c, mut r) = fixture();
        c.state_owner = false;
        legacy(&mut r, "/project/workspace/.test-env-maison.json");
        assert!(prove(&c, &r).is_err(), "another workspace may record the same run");
        c.unique_claim = true;
        let proof = prove(&c, &r).unwrap();
        assert_eq!((proof.containers.len(), proof.volumes.len()), (2, 2));
        r.containers.pop();
        r.networks[0]["Containers"].as_object_mut().unwrap().remove(&"b".repeat(64));
        assert_eq!(prove(&c, &r).unwrap().containers.len(), 1, "a half-removed run is still ours");
        let state_path = c.state_path.to_string_lossy().into_owned();
        r.containers[0]["Config"]["Labels"]["h2wp.state"] = json!(state_path);
        assert!(prove(&c, &r).is_ok(), "the host path label of older runs too");
        r.containers[0]["Config"]["Labels"]["h2wp.state"] = json!("/project/workspace/.test-env-other.json");
        assert!(prove(&c, &r).is_err(), "another state file's run is not ours");
        r.containers[0]["Config"]["Labels"]["h2wp.state"] = json!(state_path);
        r.volumes[0]["Labels"]["h2wp.owner"] = json!(c.owner);
        assert!(prove(&c, &r).is_err(), "labelled and unlabelled resources mixed");
    }
    #[test]
    fn legacy_preview_with_nothing_left_needs_no_proof() {
        let (_dir, _id, _state, mut c, _r) = fixture();
        c.state_owner = false;
        let proof = prove(&c, &Resources::default()).unwrap();
        assert!(proof.containers.is_empty() && proof.volumes.is_empty() && proof.network.is_none());
    }
    #[test]
    fn a_run_recorded_by_another_project_is_claimed_elsewhere() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let mut a = crate::skill::tests::project();
        let mut b = a.clone();
        a.id = uuid::Uuid::new_v4().to_string();
        b.id = uuid::Uuid::new_v4().to_string();
        store.put(&a).unwrap();
        store.put(&b).unwrap();
        let write = |id: &str, project: &str| {
            let workspace = store.path(id).unwrap().join("workspace");
            std::fs::create_dir_all(&workspace).unwrap();
            std::fs::write(workspace.join(".test-env-maison.json"),
                json!({"slug":"maison","project":project}).to_string()).unwrap();
        };
        write(&a.id, "h2wp-maison-abcdef");
        write(&b.id, "h2wp-maison-123456");
        assert!(!claimed_elsewhere(&store, &a.id, "h2wp-maison-abcdef"));
        write(&b.id, "h2wp-maison-abcdef");
        assert!(claimed_elsewhere(&store, &a.id, "h2wp-maison-abcdef"));
    }
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_SWEEP_IMAGE to any local image"]
    async fn sweep_removes_every_run_labelled_with_this_owner_and_nothing_else() {
        let Ok(image) = std::env::var("H2WP_TEST_SWEEP_IMAGE") else { return };
        let run = uuid::Uuid::new_v4().simple().to_string();
        let owner = format!("{run}{run}");
        let project = format!("h2wp-sweep-{}", &run[..6]);
        let docker = |a: &[&str]| {
            let args = strs(a);
            async move { crate::runtime::docker(&args, None, 60).await }
        };
        let mine = [format!("label=com.docker.compose.project={project}"), format!("label=h2wp.owner={owner}")];
        let lp = format!("com.docker.compose.project={project}");
        let lo = format!("h2wp.owner={owner}");
        let (net, vol, foreign) = (format!("{project}_default"), format!("{project}_wp_data"), format!("{project}_foreign"));
        docker(&["network", "create", "--label", &lp, "--label", &lo, &net]).await.unwrap();
        docker(&["volume", "create", "--label", &lp, "--label", &lo, &vol]).await.unwrap();
        docker(&["volume", "create", "--label", &lp, "--label", "h2wp.owner=someone-else", &foreign]).await.unwrap();
        docker(&["create", "--name", &format!("{project}-wp-1"), "--label", &lp, "--label", &lo,
            "--network", &net, "-v", &format!("{vol}:/data"), &image]).await.unwrap();
        sweep(&owner, "h2wpd-none-agent").await.unwrap();
        for kind in [vec!["ps", "-a"], vec!["volume", "ls"], vec!["network", "ls"]] {
            let left = docker(&[kind.clone(), vec!["-q", "--filter", &mine[1]]].concat()).await.unwrap();
            assert!(left.trim().is_empty(), "{kind:?} still has {left}");
        }
        assert!(docker(&["volume", "ls", "-q", "--filter", &mine[0]]).await.unwrap().contains(&foreign));
        docker(&["volume", "rm", &foreign]).await.unwrap();
    }
    #[test]
    fn only_the_plugin_s_own_preview_on_this_computer_is_touched() {
        assert!(plugin_project("h2wp-clara-hayes-3fa9c1"));
        for bad in [
            "h2wpd-0584dcf1-7f08-4efb-85cf-ae7284faf8f9",
            "h2wp-clara-test-wp",
            "other",
            "h2wp-X",
            "h2wp-a;rm",
            "h2wp-a-1",
            "",
        ] {
            assert!(!plugin_project(bad), "{bad}");
        }
        assert_eq!(
            local_url("http://localhost:53412/").as_deref(),
            Some("http://localhost:53412")
        );
        for bad in [
            "https://evil.example",
            "http://localhost",
            "file:///etc/passwd",
            "http://192.168.1.2:80",
        ] {
            assert!(local_url(bad).is_none(), "{bad}");
        }
    }
    #[test]
    fn the_preview_is_the_one_the_run_handed_over_else_the_newest() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let p = crate::skill::tests::project();
        let workspace = store.path(&p.id).unwrap().join("workspace");
        std::fs::create_dir_all(&workspace).unwrap();
        assert!(state(&store, &p).is_none());
        let write = |slug: &str, project: &str, port: u16| {
            std::fs::write(workspace.join(format!(".test-env-{slug}.json")),
            json!({"slug":slug,"project":project,"wpContainer":format!("{project}-wp-1"),"network":format!("{project}_default"),"port":port,"url":format!("http://localhost:{port}")}).to_string()).unwrap()
        };
        write("clara-hayes", "h2wp-clara-hayes-3fa9c1", 53412);
        std::thread::sleep(std::time::Duration::from_millis(20));
        write("clara-copy", "h2wp-clara-copy-0b12aa", 53413);
        write("forged", "h2wpd-someone-else", 1);
        assert_eq!(state(&store, &p).unwrap()["slug"], "clara-copy");
        let result = store.path(&p.id).unwrap().join(crate::skill::RESULT);
        std::fs::create_dir_all(result.parent().unwrap()).unwrap();
        let mut delivered = crate::skill::tests::fixture("result-delivered.json");
        delivered["preview"]["stateFile"] = json!("/project/workspace/.test-env-clara-hayes.json");
        std::fs::write(&result, delivered.to_string()).unwrap();
        assert_eq!(state(&store, &p).unwrap()["url"], "http://localhost:53412");
    }
}
