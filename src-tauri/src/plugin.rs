//! The html2wp plugin: fetched from its GitHub repository at a release tag,
//! kept on the host under the app's data (one folder per version) and
//! mounted read-only at /opt/html2wp into the containers that run it. The
//! app keeps it current by itself; the runtime image carries no plugin.
use crate::{model::*, runtime};
use serde_json::Value;
use std::{path::{Path, PathBuf}, time::Duration};
pub const REPOSITORY: &str = "https://github.com/iOSDevSK/html2wp-codex-plugin.git";
const VERSION_URL: &str = "https://raw.githubusercontent.com/iOSDevSK/html2wp-codex-plugin/main/VERSION";
/// The plugin root inside the repository (what /opt/html2wp shows).
const SUBDIR: &str = "plugins/html2wp";
/// Where the containers see the plugin; the goals the app sends name it.
pub const TARGET: &str = "/opt/html2wp";
/// The container label naming the plugin commit a container was made with.
pub const LABEL: &str = "dev.html2wp.plugin";
/// The app contract this app speaks (the plugin's `appContract` in
/// .codex-plugin/plugin.json). A plugin without one is taken to speak it.
pub const SUPPORTED_CONTRACT: &str = "1.4";
pub const NEEDS_INTERNET: &str = "The html2wp plugin is not installed yet. The first run needs an internet connection: connect, then choose Prepare environment.";

/// The installed plugin a container mounts.
#[derive(Clone, Debug, PartialEq)]
pub struct Plugin { pub version: String, pub commit: String, pub dir: PathBuf }
impl Plugin {
    /// `docker create` arguments: the commit label and the read-only mount.
    pub fn args(&self) -> Vec<String> {
        vec!["--label".into(), format!("{LABEL}={}", self.commit),
            "--mount".into(), format!("type=bind,source={},target={TARGET},readonly", self.dir.display())]
    }
}
fn root(store: &crate::store::Store) -> PathBuf { store.root.join("plugin") }
/// The plugin root of an installed version.
fn plugin_root(store: &crate::store::Store, version: &str) -> PathBuf { root(store).join(version).join(SUBDIR) }
/// The active plugin, when it is installed and complete.
pub fn active(store: &crate::store::Store) -> Option<Plugin> {
    let (version, commit) = (store.setting("active-plugin-version")?, store.setting("active-plugin-commit")?);
    let dir = plugin_root(store, &version);
    dir.join("skills/html2wp/SKILL.md").is_file().then_some(Plugin { version, commit, dir })
}
pub fn require(store: &crate::store::Store) -> Result<Plugin> { active(store).ok_or_else(|| NEEDS_INTERNET.to_string()) }
/// A tag name's version: digits, letters, dots and dashes only.
fn valid_version(v: &str) -> bool {
    !v.is_empty() && v.len() <= 64 && v.chars().next().is_some_and(|c| c.is_ascii_digit())
        && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}
/// The released version on the repository's main branch.
pub async fn upstream() -> Result<String> {
    let text = reqwest::Client::builder().https_only(true).timeout(Duration::from_secs(20))
        .user_agent(concat!("html2wp-desktop/", env!("CARGO_PKG_VERSION"))).build().map_err(err)?
        .get(VERSION_URL).send().await.map_err(err)?.error_for_status().map_err(err)?.text().await.map_err(err)?;
    let version = text.trim().to_string();
    if !valid_version(&version) { return Err(format!("The plugin repository names an invalid version: {version:?}")); }
    Ok(version)
}
/// Refuse a plugin that speaks another app contract.
pub fn compatible(manifest: &Value, version: &str) -> Result<()> {
    let contract = match &manifest["appContract"] {
        Value::Null => return Ok(()),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if contract == SUPPORTED_CONTRACT { Ok(()) } else { Err(format!("Update the app to use plugin {version} (it needs app contract {contract}; this app supports {SUPPORTED_CONTRACT}).")) }
}
/// A cloned plugin is what the tag says and this app can use.
fn check(dir: &Path, version: &str) -> Result<()> {
    let plugin = dir.join(SUBDIR);
    if !plugin.join("skills/html2wp/SKILL.md").is_file() { return Err(format!("Plugin {version} has no skills/html2wp/SKILL.md")); }
    let found = std::fs::read_to_string(plugin.join("VERSION")).unwrap_or_default();
    if found.trim() != version { return Err(format!("Plugin tag v{version} carries VERSION {:?}", found.trim())); }
    let manifest: Value = std::fs::read(plugin.join(".codex-plugin/plugin.json")).ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or(Value::Null);
    compatible(&manifest, version)
}
/// Clone tag v`version` into <app data>/plugin/<version> (with git in the
/// runtime `image`) and return its commit. A clone that fails or does not
/// check out leaves nothing behind.
pub async fn fetch(store: &crate::store::Store, image: &str, version: &str) -> Result<String> {
    if !valid_version(version) { return Err(format!("Invalid plugin version {version:?}")); }
    let root = root(store);
    std::fs::create_dir_all(&root).map_err(err)?;
    let (temp, done) = (root.join(format!("{version}.tmp")), root.join(version));
    let _ = std::fs::remove_dir_all(&temp);
    let script = "git -c advice.detachedHead=false clone --quiet --depth 1 --branch \"v$1\" \"$2\" \"/out/$1.tmp\" && git -C \"/out/$1.tmp\" rev-parse HEAD";
    let out = runtime::docker(&[
        "run", "--rm", "--label", "dev.html2wp.desktop=true", "--user", "1000:1000", "--env", "HOME=/tmp",
        "--mount", &format!("type=bind,source={},target=/out", root.display()),
        image, "sh", "-c", script, "sh", version, REPOSITORY,
    ].map(String::from), None, 300).await;
    let commit = match out {
        Ok(out) => out.trim().to_string(),
        Err(e) => { let _ = std::fs::remove_dir_all(&temp); return Err(e); }
    };
    if commit.len() != 40 || !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        let _ = std::fs::remove_dir_all(&temp);
        return Err(format!("The plugin clone reported no commit: {commit:?}"));
    }
    if let Err(e) = check(&temp, version) { let _ = std::fs::remove_dir_all(&temp); return Err(e); }
    let _ = std::fs::remove_dir_all(&done);
    std::fs::rename(&temp, &done).map_err(err)?;
    Ok(commit)
}
/// Make `version` at `commit` the active plugin; the one before stays as the
/// previous plugin, older folders go.
pub fn activate(store: &crate::store::Store, version: &str, commit: &str) -> Result<()> {
    if let Some(old) = active(store) {
        if old.version != version {
            store.set("previous-plugin", &old.version)?;
            store.set("previous-plugin-commit", &old.commit)?;
        }
    }
    store.set("active-plugin-version", version)?;
    store.set("active-plugin-commit", commit)?;
    let keep = [Some(version.to_string()), store.setting("previous-plugin")];
    if let Ok(entries) = std::fs::read_dir(root(store)) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if !keep.iter().flatten().any(|k| *k == name) { let _ = std::fs::remove_dir_all(entry.path()); }
        }
    }
    Ok(())
}
/// Fetch and activate `version` (nothing may run on the plugin meanwhile); `(from, to)` of the switch.
pub async fn install(store: &crate::store::Store, image: &str, version: &str) -> Result<(Option<String>, String)> {
    let from = active(store).map(|p| p.version);
    let commit = fetch(store, image, version).await?;
    activate(store, version, &commit)?;
    Ok((from, version.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn a_plugin_without_a_contract_is_taken_to_speak_the_app_s() {
        assert!(compatible(&json!({"name":"html2wp"}), "1.0.0-gamma.5").is_ok());
        assert!(compatible(&json!({"appContract":SUPPORTED_CONTRACT}), "1.0.0").is_ok());
        let refused = compatible(&json!({"appContract":"2.0"}), "1.1.0").unwrap_err();
        assert!(refused.starts_with("Update the app to use plugin 1.1.0"), "{refused}");
        assert!(compatible(&json!({"appContract":2}), "1.1.0").is_err());
    }
    #[test]
    fn only_a_tag_like_version_is_fetched() {
        assert!(valid_version("1.0.0-gamma.5"));
        for bad in ["", "main", "../x", "1.0 ; rm", "1.0.0/../../etc"] { assert!(!valid_version(bad), "{bad}"); }
    }
    #[test]
    fn the_previous_plugin_stays_and_older_ones_go() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let install = |v: &str| { std::fs::create_dir_all(plugin_root(&store, v).join("skills/html2wp")).unwrap(); std::fs::write(plugin_root(&store, v).join("skills/html2wp/SKILL.md"), "x").unwrap(); };
        for (v, c) in [("1.0.0", "a"), ("1.0.1", "b"), ("1.0.2", "c")] { install(v); activate(&store, v, c).unwrap(); }
        let active = active(&store).unwrap();
        assert_eq!((active.version.as_str(), active.commit.as_str()), ("1.0.2", "c"));
        assert_eq!(store.setting("previous-plugin").as_deref(), Some("1.0.1"));
        assert!(!root(&store).join("1.0.0").exists() && root(&store).join("1.0.1").exists());
        assert!(active.args()[3].ends_with(&format!("target={TARGET},readonly")));
    }
    /// The plugin from GitHub, mounted read-only into the Codex container and
    /// a project's container (docker, network): H2WP_TEST_RUNTIME_IMAGE.
    /// Its own data folder, container and volume; never the app's.
    #[tokio::test]
    #[ignore = "real Docker and GitHub; set H2WP_TEST_RUNTIME_IMAGE to a runtime image without a plugin"]
    async fn the_plugin_from_github_is_mounted_read_only() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let base = PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/html2wp-desktop-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new().prefix("app data ").tempdir_in(base).unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let version = upstream().await.unwrap();
        let (from, to) = install(&store, &image, &version).await.unwrap();
        let plugin = require(&store).unwrap();
        println!("plugin {from:?} -> {to} at {} in {}", plugin.commit, plugin.dir.display());
        let suffix = std::process::id();
        let (name, auth) = (format!("h2wpd-codex-test-{suffix}"), format!("h2wpd-codex-auth-test-{suffix}"));
        runtime::ensure_agent_as(&name, &auth, &image, &plugin).await.unwrap();
        let sh = |c: &str, cmd: &str| { let (c, cmd) = (c.to_string(), cmd.to_string()); async move { runtime::docker(&["exec".into(), c, "sh".into(), "-c".into(), cmd], None, 30).await } };
        // The /opt/html2wp mount's (source, writable).
        let mounts = |c: &str| { let c = c.to_string(); async move {
            let raw = runtime::docker(&["inspect".into(), "--format".into(), "{{json .Mounts}}".into(), c], None, 15).await.unwrap();
            let all: Vec<Value> = serde_json::from_str(&raw).unwrap();
            all.into_iter().find(|m| m["Destination"] == TARGET).map(|m| (m["Source"].as_str().unwrap_or("").to_string(), m["RW"].as_bool()))
        } };
        let agent = (sh(&name, "ls /opt/html2wp/skills/html2wp/SKILL.md && cat /opt/html2wp/VERSION").await, sh(&name, "touch /opt/html2wp/x").await, mounts(&name).await);
        // The project's container.
        let mut p: Project = serde_json::from_value(serde_json::json!({
            "id":uuid::Uuid::new_v4().to_string(),"name":"fixture","sourceName":"fixture","kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"imported","revision":1,"threadId":null,"pages":[],"gates":[],
            "artifacts":[],"preview":null,"runtimeImage":image,"pluginCommit":plugin.commit,"reporting":"not_required","lastError":null})).unwrap();
        p.runtime_image = image.clone();
        store.put(&p).unwrap();
        let project = crate::skill::ensure_container(&store, &p, &image).await;
        let pname = format!("h2wpd-{}-agent", p.id);
        let seen = (sh(&pname, "ls /opt/html2wp/skills/html2wp/SKILL.md").await, sh(&pname, "touch /opt/html2wp/x").await, mounts(&pname).await);
        // The same container is kept on the same plugin, made again on another.
        let outdated_same = runtime::is_outdated(&pname, &image, Some(&plugin.commit)).await;
        let outdated_other = runtime::is_outdated(&pname, &image, Some("0000000")).await;
        let _ = runtime::docker(&["rm".into(), "-f".into(), name.clone(), pname.clone()], None, 60).await;
        let _ = runtime::docker(&["volume".into(), "rm".into(), auth], None, 60).await;
        project.unwrap();
        assert_eq!(to, version);
        assert!(agent.0.as_ref().unwrap().contains(&version), "{agent:?}");
        assert!(agent.1.is_err(), "the Codex container cannot write the plugin");
        assert!(agent.2.as_ref().is_some_and(|(source, rw)| source.ends_with(&plugin.dir.display().to_string()) && *rw == Some(false)), "{:?}: {}", agent.2, "the Codex container mounts the plugin read-only");
        assert!(seen.0.is_ok(), "{seen:?}");
        assert!(seen.1.is_err(), "the project's container cannot write the plugin");
        assert!(seen.2.as_ref().is_some_and(|(source, rw)| source.ends_with(&plugin.dir.display().to_string()) && *rw == Some(false)), "{:?}: {}", seen.2, "the project's container mounts the plugin read-only");
        assert!(!outdated_same && outdated_other);
    }
}
/// Tests: the vendored plugin (vendor/html2wp) as the installed one.
#[cfg(test)]
pub fn install_vendored(store: &crate::store::Store) -> Plugin {
    fn copy(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).unwrap();
        for entry in std::fs::read_dir(from).unwrap().flatten() {
            if entry.file_name() == ".git" { continue; }
            let (source, target) = (entry.path(), to.join(entry.file_name()));
            if entry.file_type().unwrap().is_dir() { copy(&source, &target) } else { std::fs::copy(&source, &target).unwrap(); }
        }
    }
    let vendored = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../vendor/html2wp");
    let version = std::fs::read_to_string(vendored.join(SUBDIR).join("VERSION")).unwrap().trim().to_string();
    copy(&vendored, &root(store).join(&version));
    activate(store, &version, "vendored").unwrap();
    require(store).unwrap()
}
