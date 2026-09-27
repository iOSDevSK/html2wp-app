use crate::model::*;
use serde_json::{json, Value};
use std::{
    ffi::{OsStr, OsString},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{io::AsyncWriteExt, process::Command};
/// The images of the plugin's preview WordPress (its test-env-compose.yml),
/// pulled while the environment is prepared so the first run need not wait.
pub const WORDPRESS_IMAGE: &str = "wordpress:latest@sha256:fe04ca18a5897be659e0d48686467dc939409b6ee53394507d0d08396595be67";
pub const DATABASE_IMAGE: &str = "mariadb:11@sha256:efb4959ef2c835cd735dbc388eb9ad6aab0c78dd64febcd51bc17481111890c4";
pub const LABEL: &str = "dev.html2wp.desktop";
fn tool_directories() -> Vec<PathBuf> {
    let mut dirs = vec![];
    #[cfg(target_os = "macos")]
    dirs.extend(
        [
            "/Applications/Docker.app/Contents/Resources/bin",
            "/usr/local/bin",
            "/opt/homebrew/bin",
        ]
        .map(PathBuf::from),
    );
    #[cfg(target_os = "linux")]
    dirs.extend(["/usr/local/bin", "/usr/bin", "/opt/docker-desktop/bin"].map(PathBuf::from));
    #[cfg(target_os = "windows")]
    if let Some(program_files) = std::env::var_os("ProgramFiles") {
        dirs.push(PathBuf::from(program_files).join("Docker/Docker/resources/bin"));
    }
    #[cfg(target_os = "windows")]
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        dirs.push(PathBuf::from(local).join("Programs/DockerDesktop/resources/bin"));
    }
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if let Some(home) = home {
        dirs.push(home.join(".docker/bin"));
        dirs.push(home.join(".local/bin"));
        #[cfg(target_os = "macos")]
        dirs.push(home.join("Applications/Docker.app/Contents/Resources/bin"));
    }
    dirs
}
pub fn docker_bin() -> PathBuf {
    let name = if cfg!(windows) {
        "docker.exe"
    } else {
        "docker"
    };
    for dir in tool_directories().into_iter().chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )) {
        let file = dir.join(name);
        if dir.is_absolute() && file.is_file() {
            return file;
        }
    }
    name.into()
}
fn command_path(binary: &Path, inherited: &OsStr, extra: &[PathBuf]) -> Result<OsString> {
    let mut dirs = Vec::new();
    // Docker launches credential helpers and CLI plugins by name. Finder's
    // PATH omits their locations even when Docker itself was found by path.
    let resolved = if binary.is_absolute() {
        binary.canonicalize().ok()
    } else {
        None
    };
    for dir in resolved
        .as_deref()
        .and_then(Path::parent)
        .into_iter()
        .chain(binary.parent())
        .map(Path::to_path_buf)
        .chain(extra.iter().cloned())
        .chain(std::env::split_paths(inherited))
    {
        if dir.is_absolute() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).map_err(err)
}
pub fn docker_command() -> Result<Command> {
    let binary = docker_bin();
    let path = command_path(
        &binary,
        &std::env::var_os("PATH").unwrap_or_default(),
        &tool_directories(),
    )?;
    let mut command = Command::new(binary);
    command.env("PATH", path);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    Ok(command)
}
fn docker_failure(stderr: &str) -> String {
    if stderr.contains("docker-credential-")
        && (stderr.contains("executable file not found") || stderr.contains("not found in $PATH"))
    {
        return "Docker could not find its credential helper. Restart Docker Desktop and this app, then try Prepare environment again. If it persists, repair the Docker Desktop installation.".into();
    }
    let tail: Vec<_> = stderr.chars().rev().take(3000).collect();
    format!(
        "Docker operation failed: {}",
        tail.into_iter().rev().collect::<String>()
    )
}
pub async fn docker(args: &[String], input: Option<&[u8]>, seconds: u64) -> Result<String> {
    let mut cmd = docker_command()?;
    cmd.args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = cmd.spawn().map_err(|e| {
        format!("Cannot start Docker: {e}. Install and start a local Docker runtime.")
    })?;
    if let Some(mut pipe) = child.stdin.take() {
        if let Some(bytes) = input {
            pipe.write_all(bytes).await.map_err(err)?;
        }
        drop(pipe);
    }
    let out = tokio::time::timeout(Duration::from_secs(seconds), child.wait_with_output())
        .await
        .map_err(|_| "Docker operation timed out".to_string())?
        .map_err(err)?;
    let output = String::from_utf8_lossy(&out.stdout).to_string();
    if !out.status.success() {
        return Err(docker_failure(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(output)
}
fn args(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}
/// Whether a project on another runtime or plugin moves to `current` and
/// `plugin_commit`. Every project runs on the installed runtime and plugin:
/// the plugin owns the conversion and resumes its own workspace, and the
/// app's features call that plugin.
pub fn should_adopt(p: &Project, current: &str, plugin_commit: &str) -> bool {
    p.runtime_image != current || p.plugin_commit != plugin_commit
}
/// Moves the project to `current` and the installed plugin, whatever it ran
/// on before: an image since removed, or another plugin commit.
pub fn adopt(p: &mut Project, current: &str, plugin_commit: &str) -> bool {
    if !should_adopt(p, current, plugin_commit) { return false; }
    p.runtime_image = current.into();
    p.plugin_commit = plugin_commit.into();
    true
}
/// Moves the project to the current runtime (and plugin) when that runtime is installed.
pub async fn adopt_current(p: &mut Project, current: &str, plugin_commit: &str) -> bool {
    should_adopt(p, current, plugin_commit) && preflight(current).await["imageReady"] == true && adopt(p, current, plugin_commit)
}
/// The image a reference names now, as its ID; None when it is not here.
pub async fn image_id(reference: &str) -> Option<String> {
    docker(&args(&["image", "inspect", "--format", "{{.Id}}", reference]), None, 15).await.ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}
/// The image a container was created from, as its ID; None when there is no such container.
#[cfg(test)]
pub async fn container_image(name: &str) -> Option<String> {
    made_with(name).await.map(|(image, _)| image)
}
/// The image ID a container was created from and its plugin label ("" when
/// it has none); None when there is no such container.
async fn made_with(name: &str) -> Option<(String, String)> {
    let format = format!("{{{{.Image}}}}|{{{{index .Config.Labels \"{}\"}}}}", crate::plugin::LABEL);
    let out = docker(&args(&["inspect", "--format", &format, name]), None, 15).await.ok()?;
    let (image, plugin) = out.trim().split_once('|')?;
    (!image.is_empty()).then(|| (image.to_string(), plugin.replace("<no value>", "")))
}
/// A container made from another image than the current runtime's is
/// replaced; one whose image cannot be compared (either unknown) is kept
/// for it. A container that mounts the plugin (`plugin`: the installed
/// commit) and was made with another commit, or with none (an older app's,
/// whose image carried the plugin), is replaced too.
pub fn outdated(container: Option<&str>, current: Option<&str>, made_plugin: Option<&str>, plugin: Option<&str>) -> bool {
    matches!((container, current), (Some(made), Some(now)) if made != now)
        || (container.is_some() && plugin.is_some() && made_plugin != plugin)
}
/// Whether the named container runs on an older image than `image` names
/// now, or (one that mounts the plugin) on another plugin than `plugin_commit`.
pub async fn is_outdated(name: &str, image: &str, plugin_commit: Option<&str>) -> bool {
    let made = made_with(name).await;
    outdated(made.as_ref().map(|m| m.0.as_str()), image_id(image).await.as_deref(), made.as_ref().map(|m| m.1.as_str()), plugin_commit)
}
pub async fn preflight(image: &str) -> Value {
    let context = docker(
        &args(&[
            "context",
            "inspect",
            "--format",
            "{{json .Endpoints.docker.Host}}",
        ]),
        None,
        15,
    )
    .await;
    let endpoint = match context {
        Ok(v) => v.trim().trim_matches('"').to_string(),
        Err(e) => return json!({"ready":false,"docker":false,"message":e,"imageReady":false}),
    };
    let endpoint = std::env::var("DOCKER_HOST").unwrap_or(endpoint);
    if !endpoint.starts_with("unix://") && !endpoint.starts_with("npipe://") {
        return json!({"ready":false,"docker":false,"message":"Select a local Docker context. Remote Docker endpoints are not supported.","imageReady":false});
    }
    let version = match docker(&args(&["info", "--format", "{{.ServerVersion}}"]), None, 15).await {
        Ok(v) => v.trim().to_string(),
        Err(e) => return json!({"ready":false,"docker":false,"message":e,"imageReady":false}),
    };
    let platform = match docker(&args(&["info", "--format", "{{.OSType}}|{{.Architecture}}"]), None, 15).await {
        Ok(v) => v.trim().to_string(),
        Err(e) => return json!({"ready":false,"docker":false,"message":e,"imageReady":false}),
    };
    let expected_arch = match std::env::consts::ARCH { "aarch64" => "aarch64", "x86_64" => "x86_64", _ => "unsupported" };
    if platform != format!("linux|{expected_arch}") {
        return json!({"ready":false,"docker":false,"message":format!("The conversion needs a local Linux Docker engine for {expected_arch}. Switch Docker Desktop to Linux containers on this processor, then retry."),"imageReady":false});
    }
    if let Err(e) = docker(&args(&["compose", "version", "--short"]), None, 15).await {
        return json!({"ready":false,"docker":true,"message":e,"imageReady":false});
    }
    let image_ready = docker(&args(&["image", "inspect", image]), None, 10)
        .await
        .is_ok();
    json!({"ready":image_ready,"docker":true,"version":version,"imageReady":image_ready,"message":if image_ready{"Your local environment is ready."}else{"Docker is ready. Prepare the conversion environment to continue."},"architecture":std::env::consts::ARCH})
}
pub async fn require_local() -> Result<()> {
    let p = preflight("html2wp-runtime:desktop-0.1.0").await;
    if p["docker"] != true {
        return Err(p["message"].as_str().unwrap_or("Docker unavailable").into());
    }
    Ok(())
}
pub fn project_name(id: &str) -> Result<String> {
    uuid::Uuid::parse_str(id).map_err(err)?;
    Ok(format!("h2wpd-{id}"))
}
pub async fn assert_owned(name: &str) -> Result<()> {
    let v = docker(
        &args(&[
            "inspect",
            "--format",
            "{{index .Config.Labels \"dev.html2wp.desktop\"}}",
            name,
        ]),
        None,
        10,
    )
    .await?;
    if v.trim() != "true" {
        return Err("Refusing to operate on a container not owned by html2wp".into());
    }
    Ok(())
}
/// The Codex container and the volume that holds its account.
pub const AGENT: &str = "h2wpd-codex";
pub const AGENT_AUTH: &str = "h2wpd-codex-auth";
pub async fn ensure_agent(image: &str, plugin: &crate::plugin::Plugin) -> Result<String> {
    ensure_agent_as(AGENT, AGENT_AUTH, image, plugin).await
}
/// Replace the Codex container when it runs on an older runtime or another
/// plugin than the current ones (the account volume stays); true when it was replaced.
pub async fn replace_outdated_agent(image: &str, plugin: &crate::plugin::Plugin) -> Result<bool> {
    if docker(&args(&["inspect", AGENT]), None, 10).await.is_err() || !is_outdated(AGENT, image, Some(&plugin.commit)).await { return Ok(false); }
    assert_owned(AGENT).await?;
    docker(&args(&["rm", "-f", AGENT]), None, 30).await?;
    Ok(true)
}
/// Where a Codex newer than the runtime's is installed in the Codex
/// container. It lives in the container, so a container made again from a new
/// runtime starts on that runtime's Codex and the next check updates it again.
pub const CODEX_UPDATE: &str = "/home/agent/npm";
/// The `codex` the Codex container runs: the update when there is one.
pub async fn codex_bin(name: &str) -> String {
    let updated = format!("{CODEX_UPDATE}/bin/codex");
    if docker(&args(&["exec", name, "test", "-x", &updated]), None, 10).await.is_ok() { updated } else { "codex".into() }
}
/// "0.157.0" from "codex-cli 0.157.0" (or a bare version).
fn version_of(text: &str) -> Option<Vec<u64>> {
    let v = text.split_whitespace().last()?;
    let parts: Option<Vec<u64>> = v.split('.').map(|p| p.parse().ok()).collect();
    parts.filter(|p| p.len() == 3)
}
fn newer(latest: &str, current: &str) -> bool {
    matches!((version_of(latest), version_of(current)), (Some(l), Some(c)) if l > c)
}
/// Install the latest released Codex into the Codex container when it is newer
/// than the one it runs; `(from, to)` when it did. Offline or no release: None.
pub async fn update_codex(name: &str) -> Result<Option<(String, String)>> {
    let bin = codex_bin(name).await;
    let current = docker(&args(&["exec", name, &bin, "--version"]), None, 20).await?;
    let Ok(latest) = docker(&args(&["exec", name, "npm", "view", "@openai/codex@latest", "version"]), None, 30).await else { return Ok(None) };
    let (current, latest) = (current.trim().to_string(), latest.trim().to_string());
    if !newer(&latest, &current) { return Ok(None); }
    docker(&args(&["exec", name, "npm", "install", "-g", "--no-fund", "--no-audit", "--prefix", CODEX_UPDATE, &format!("@openai/codex@{latest}")]), None, 300).await?;
    let from = version_of(&current).map(|v| v.iter().map(u64::to_string).collect::<Vec<_>>().join(".")).unwrap_or(current);
    Ok(Some((from, latest)))
}
/// Back to the runtime's own Codex (an update the app could not talk to).
pub async fn remove_codex_update(name: &str) -> Result<()> {
    docker(&args(&["exec", name, "rm", "-rf", CODEX_UPDATE]), None, 30).await.map(|_| ())
}
#[cfg(test)]
#[test]
fn only_a_strictly_newer_release_updates_codex() {
    assert!(newer("0.157.0", "codex-cli 0.154.0"));
    assert!(newer("0.160.0", "codex-cli 0.157.3"));
    assert!(!newer("0.157.0", "codex-cli 0.157.0"));
    assert!(!newer("0.156.1", "codex-cli 0.157.0"));
    assert!(!newer("0.158.0-alpha.1", "codex-cli 0.157.0"), "a prerelease is never taken");
    assert!(!newer("", "codex-cli 0.157.0"));
}
/// The Codex container: the runtime image with the plugin mounted read-only
/// at /opt/html2wp (the goals point Codex at its SKILL.md) and the account volume.
pub(crate) async fn ensure_agent_as(name: &str, auth: &str, image: &str, plugin: &crate::plugin::Plugin) -> Result<String> {
    require_local().await?;
    if docker(&args(&["inspect", name]), None, 10).await.is_ok() {
        assert_owned(name).await?;
        // Made from another image than the current runtime's, or with another
        // plugin: made again (the account lives in its volume, not in the container).
        if is_outdated(name, image, Some(&plugin.commit)).await {
            docker(&args(&["rm", "-f", name]), None, 30).await?;
        }
    }
    if docker(&args(&["inspect", name]), None, 10).await.is_err() {
        let mut create = args(&[
                "create",
                "--name",
                name,
                "--label",
                "dev.html2wp.desktop=true",
                "--init",
                "--cap-drop=ALL",
                "--security-opt",
                "no-new-privileges",
                "--pids-limit",
                "256",
                "--memory",
                "4g",
                "--user",
                "1000:1000",
                "--mount",
                &format!("type=volume,source={auth},target=/home/agent/.codex"),
            ]);
        create.extend(plugin.args());
        create.extend(args(&[image, "sleep", "infinity"]));
        docker(&create, None, 60).await?;
    }
    docker(&args(&["start", name]), None, 30).await?;
    Ok(name.into())
}
fn cleanup_owned(kind: &str, resource: &Value, project: &str) -> bool {
    let labels = if kind == "container" { &resource["Config"]["Labels"] } else { &resource["Labels"] };
    if kind == "network" {
        labels["com.docker.compose.project"] == project
    } else {
        labels[LABEL] == "true" && (kind == "container" || labels["com.docker.compose.project"] == project)
    }
}

/// Remove only this UUID's resources. Never prune shared images or Codex volumes.
pub async fn cleanup_project(id: &str) -> Result<()> {
    require_local().await?;
    // A live fix's certification install of an earlier release first.
    cleanup_compose(&format!("{}-cert", project_name(id)?)).await?;
    cleanup_compose(&project_name(id)?).await
}
async fn cleanup_compose(project: &str) -> Result<()> {
    let project = project.to_string();
    let containers = ["agent", "worker", "service", "cli-copy", "wp-1", "db-1", "h2g"]
        .map(|suffix| format!("{project}-{suffix}"));
    let containers = [containers.to_vec(), vec![format!("{project}_wp_1"), format!("{project}_db_1")]].concat();
    let groups = [
        ("container", containers),
        ("volume", vec![format!("{project}_wp"), format!("{project}_db")]),
        ("network", vec![format!("{project}_default")]),
    ];
    let mut remove = vec![];
    // Inspect the entire removal set before deleting anything. A Docker failure
    // must not be confused with an absent resource.
    for (kind, expected) in groups {
        let listing = if kind == "container" {
            args(&["container", "ls", "-a", "--format", "{{.Names}}"])
        } else { args(&[kind, "ls", "--format", "{{.Name}}"] ) };
        let present = docker(&listing, None, 20).await?;
        for name in expected {
            if !present.lines().any(|line| line.trim() == name) { continue; }
            let raw = docker(&args(&[kind, "inspect", &name]), None, 20).await?;
            let values: Value = serde_json::from_str(&raw).map_err(err)?;
            if !cleanup_owned(kind, &values[0], &project) {
                return Err(format!("Refusing to remove {name}: project ownership could not be verified"));
            }
            remove.push((kind, name));
        }
    }
    for (kind, name) in remove {
        let command = if kind == "container" { args(&[kind, "rm", "-f", &name]) }
            else { args(&[kind, "rm", &name]) };
        docker(&command, None, 60).await?;
    }
    Ok(())
}

#[cfg(test)]
mod cleanup_tests {
    use super::*;
    #[test]
    fn cleanup_requires_ownership_and_project_volume_labels() {
        assert!(!cleanup_owned("container", &json!({"Config":{"Labels":{}}}), "project"));
        assert!(cleanup_owned("container", &json!({"Config":{"Labels":{LABEL:"true"}}}), "project"));
        assert!(!cleanup_owned("volume", &json!({"Labels":{LABEL:"true","com.docker.compose.project":"other"}}), "project"));
        assert!(cleanup_owned("volume", &json!({"Labels":{LABEL:"true","com.docker.compose.project":"project"}}), "project"));
        assert!(!cleanup_owned("network", &json!({"Labels":{"com.docker.compose.project":"other"}}), "project"));
    }
}

#[cfg(test)]
mod command_tests {
    use super::*;
    #[test]
    fn every_project_moves_to_the_current_runtime_and_old_containers_are_replaced() {
        let p: Project = serde_json::from_value(json!({
            "id":"0584dcf1-7f08-4efb-85cf-ae7284faf8f9","name":"fixture","sourceName":"fixture","kind":"Web app",
            "createdAt":"","updatedAt":"","phase":"preparing","revision":1,"threadId":null,"pages":[],"gates":[],
            "artifacts":[],"preview":null,"runtimeImage":"sha256:old","pluginCommit":"abc","reporting":"not_required","lastError":null,"autoApprove":false,"conversionApprovalRequired":false})).unwrap();
        assert!(should_adopt(&p, "sha256:new", "abc"));
        assert!(!should_adopt(&p, "sha256:old", "abc"), "already current");
        assert!(should_adopt(&p, "sha256:old", "def"), "another plugin");
        // A container is replaced when both images are known and differ.
        let abc = Some("abc");
        assert!(outdated(Some("sha256:old"), Some("sha256:new"), abc, abc));
        assert!(!outdated(Some("sha256:new"), Some("sha256:new"), abc, abc));
        assert!(!outdated(None, Some("sha256:new"), None, abc), "no such container");
        assert!(!outdated(Some("sha256:old"), None, abc, abc), "the image cannot be compared");
        // ... or, one that mounts the plugin, when it was made with another plugin, or none (an older app's).
        assert!(outdated(Some("sha256:new"), Some("sha256:new"), Some("def"), abc));
        assert!(outdated(Some("sha256:new"), Some("sha256:new"), Some(""), abc), "no plugin label");
        assert!(!outdated(Some("sha256:new"), Some("sha256:new"), Some(""), None), "a container without the plugin");
    }
    /// A project whose image was removed, or that another plugin commit
    /// started, moves to the installed runtime and its plugin; once there, it stays.
    #[test]
    fn a_project_on_a_removed_image_or_another_plugin_moves_to_the_installed_runtime() {
        let project = |image: &str, commit: &str| -> Project { serde_json::from_value(json!({
            "id":"0584dcf1-7f08-4efb-85cf-ae7284faf8f9","name":"fixture","sourceName":"fixture","kind":"Web app",
            "createdAt":"","updatedAt":"","phase":"deliverable_ready","revision":1,"threadId":"t","pages":[],"gates":[],
            "artifacts":[],"preview":null,"runtimeImage":image,"pluginCommit":commit,"reporting":"not_required","lastError":null,"autoApprove":false,"conversionApprovalRequired":false})).unwrap() };
        let installed = ("sha256:7df5bde6", "55c478ca689c");
        // (a) the recorded image is gone (the owner removed fc289da's).
        let mut gone = project("sha256:952094c0", "55c478ca689c");
        assert!(adopt(&mut gone, installed.0, installed.1));
        assert_eq!((gone.runtime_image.as_str(), gone.plugin_commit.as_str()), ("sha256:7df5bde6", "55c478ca689c"));
        // (b) another plugin commit: the plugin resumes its own workspace.
        let mut older = project("sha256:952094c0", "fc289da00000");
        assert!(adopt(&mut older, installed.0, installed.1));
        assert_eq!((older.runtime_image.as_str(), older.plugin_commit.as_str()), ("sha256:7df5bde6", "55c478ca689c"));
        assert!(!adopt(&mut older, installed.0, installed.1), "already on the installed runtime");
        // The work of the project is not touched.
        assert_eq!((older.phase.as_str(), older.thread_id.as_deref()), ("deliverable_ready", Some("t")));
    }
    /// The Codex container made from an older runtime is made again from the
    /// current one, its account volume kept; one on the current runtime is
    /// kept (docker): H2WP_TEST_OLD_RUNTIME_IMAGE and H2WP_TEST_RUNTIME_IMAGE.
    /// It uses its own container and volume, never the app's h2wpd-codex.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_OLD_RUNTIME_IMAGE and H2WP_TEST_RUNTIME_IMAGE to two runtime images"]
    async fn the_codex_container_follows_the_installed_runtime_and_keeps_its_account() {
        let (Ok(old), Ok(current)) = (std::env::var("H2WP_TEST_OLD_RUNTIME_IMAGE"), std::env::var("H2WP_TEST_RUNTIME_IMAGE")) else { return };
        let suffix = std::process::id();
        let (name, auth) = (format!("h2wpd-codex-test-{suffix}"), format!("h2wpd-codex-auth-test-{suffix}"));
        let plugin_dir = tempfile::tempdir().unwrap();
        let plugin = crate::plugin::Plugin { version: "test".into(), commit: "test".into(), dir: plugin_dir.path().to_path_buf() };
        let id = |name: String| async move { docker(&args(&["inspect", "--format", "{{.Id}}", &name]), None, 15).await.unwrap().trim().to_string() };
        ensure_agent_as(&name, &auth, &old, &plugin).await.unwrap();
        let first = id(name.clone()).await;
        docker(&args(&["exec", &name, "sh", "-c", "echo signed-in > /home/agent/.codex/account"]), None, 30).await.unwrap();
        ensure_agent_as(&name, &auth, &current, &plugin).await.unwrap();
        let second = id(name.clone()).await;
        let made_from = container_image(&name).await;
        let wanted = image_id(&current).await;
        let account = docker(&args(&["exec", &name, "cat", "/home/agent/.codex/account"]), None, 30).await;
        ensure_agent_as(&name, &auth, &current, &plugin).await.unwrap();
        let again = id(name.clone()).await;
        let _ = docker(&args(&["rm", "-f", &name]), None, 60).await;
        let _ = docker(&args(&["volume", "rm", &auth]), None, 60).await;
        assert_ne!(first, second, "the Codex container on the older runtime was made again");
        assert_eq!(made_from, wanted, "from the current runtime");
        assert_eq!(account.unwrap().trim(), "signed-in", "the account volume stays");
        assert_eq!(again, second, "a Codex container on the current runtime is kept");
    }
    #[cfg(unix)]
    #[test]
    fn finder_path_resolves_helper_next_to_symlinked_docker() {
        use std::os::unix::fs::{symlink, PermissionsExt};
        let root = tempfile::tempdir().unwrap();
        let resources = root.path().join("Docker.app/Contents/Resources/bin");
        let links = root.path().join("usr/local/bin");
        std::fs::create_dir_all(&resources).unwrap();
        std::fs::create_dir_all(&links).unwrap();
        let binary = resources.join("docker");
        std::fs::write(&binary, b"fixture").unwrap();
        symlink(&binary, links.join("docker")).unwrap();
        let helper = resources.join("docker-credential-desktop");
        std::fs::write(&helper, b"#!/bin/sh\nprintf 'fixture-helper-found\\n'\n").unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
        let path = command_path(
            &links.join("docker"),
            OsStr::new("/usr/bin:/bin:.:../project"),
            &[],
        )
        .unwrap();
        assert!(std::env::split_paths(&path).all(|p| p.is_absolute()));
        let result = std::process::Command::new("/bin/sh")
            .args(["-c", "exec docker-credential-desktop"])
            .env("PATH", path)
            .output()
            .unwrap();
        assert!(result.status.success());
        assert_eq!(
            String::from_utf8_lossy(&result.stdout).trim(),
            "fixture-helper-found"
        );
    }
    #[test]
    fn helper_error_is_actionable_without_a_build_log_wall() {
        let error=docker_failure("#0 building with desktop-linux\nerror getting credentials - err: exec: docker-credential-desktop: executable file not found in $PATH");
        assert!(error.contains("credential helper"));
        assert!(error.contains("Restart Docker Desktop"));
        assert!(!error.contains("#0"));
    }
}
