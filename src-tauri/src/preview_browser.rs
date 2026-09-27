//! The preview browser: Chrome for Testing, downloaded once into the app's
//! data, with its own profile and the Shot2AI extension loaded unpacked from
//! the newest commit of its main branch. Branded Google Chrome ignores
//! --load-extension since version 137; Chrome for Testing still loads it.
//!
//! The extension always lives at the same folder: an unpacked extension's ID
//! is made from its path, and its storage (the html2wp pairing) from its ID.
use crate::{model::{err, Result}, store::Store};
use serde_json::{json, Value};
use std::{io::Read, path::{Path, PathBuf}, time::Duration};

const CFT_INDEX: &str = "https://googlechromelabs.github.io/chrome-for-testing/last-known-good-versions-with-downloads.json";
const CFT_DOWNLOADS: &str = "https://storage.googleapis.com/chrome-for-testing-public/";
const BROWSER_APP: &str = "Google Chrome for Testing.app";
const BROWSER_BINARY: &str = "Contents/MacOS/Google Chrome for Testing";
const MAX_BROWSER_BYTES: u64 = 600 * 1024 * 1024;
const HEAD: &str = "https://api.github.com/repos/iOSDevSK/shot2ai/commits/main";
/// Downloaded by the commit the check found, so the recorded commit is the one unpacked.
const ARCHIVE: &str = "https://codeload.github.com/iOSDevSK/shot2ai/zip/";
const CHECK_EVERY_SECS: i64 = 10 * 60;
const MAX_EXTENSION_BYTES: u64 = 50 * 1024 * 1024;
const MAX_EXTENSION_FILES: usize = 5_000;
const INSTALLED_KEY: &str = "shot2ai-sha";
const HEAD_KEY: &str = "shot2ai-head";
const CHECKED_KEY: &str = "shot2ai-checked";
const BROWSER_KEY: &str = "preview-browser-version";

pub fn extension_dir(store: &Store) -> PathBuf { store.root.join("extensions").join("shot2ai") }
pub fn profile_dir(store: &Store) -> PathBuf { store.root.join("preview-browser").join("profile") }
fn browser_dir(store: &Store) -> PathBuf { store.root.join("preview-browser").join("chrome") }
fn binary(dir: &Path) -> PathBuf { dir.join(BROWSER_APP).join(BROWSER_BINARY) }

fn client(seconds: u64) -> Result<reqwest::Client> {
    reqwest::Client::builder().timeout(Duration::from_secs(seconds))
        .user_agent(concat!("html2wp-desktop/", env!("CARGO_PKG_VERSION"))).build().map_err(err)
}

/// A full commit SHA, as the GitHub API answers it.
pub fn valid_sha(sha: &str) -> bool { sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) }
/// Whether a check of main made at `checked` still holds at `now`.
fn fresh(checked: i64, now: i64) -> bool { (0..CHECK_EVERY_SECS).contains(&now.saturating_sub(checked)) }

/// main's head commit, asked of GitHub at most every 10 minutes.
async fn head(store: &Store, api: &str, now: i64) -> Result<String> {
    let checked = store.setting(CHECKED_KEY).and_then(|v| v.parse::<i64>().ok()).unwrap_or(i64::MIN);
    if let Some(sha) = store.setting(HEAD_KEY).filter(|s| valid_sha(s) && fresh(checked, now)) { return Ok(sha); }
    let body = client(15)?.get(api).header("accept", "application/vnd.github.sha")
        .send().await.map_err(err)?.error_for_status().map_err(err)?.text().await.map_err(err)?;
    let sha = body.trim().to_string();
    if !valid_sha(&sha) { return Err("GitHub did not answer with a commit".into()); }
    store.set(HEAD_KEY, &sha)?;
    store.set(CHECKED_KEY, &now.to_string())?;
    Ok(sha)
}

/// Unpacks a GitHub archive of the extension into `into` (made here): its one
/// top folder is dropped; links, special files, paths outside and names
/// Chrome reserves are refused. Returns the manifest.
pub fn unpack(bytes: &[u8], into: &Path) -> Result<Value> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).map_err(|_| "The Shot2AI download is not a ZIP archive".to_string())?;
    if zip.len() > MAX_EXTENSION_FILES { return Err("The Shot2AI archive has too many files".into()); }
    std::fs::create_dir(into).map_err(err)?;
    let (mut top, mut total) = (None::<String>, 0u64);
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(err)?;
        let rel = crate::files::relative_safe(entry.name().trim_end_matches('/')).map_err(|_| "The Shot2AI archive has a path outside its folder".to_string())?;
        if let Some(mode) = entry.unix_mode() {
            if !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000) { return Err("The Shot2AI archive contains links or special files".into()); }
        }
        let mut parts = rel.components();
        let first = parts.next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        if top.get_or_insert_with(|| first.clone()) != &first { return Err("The Shot2AI archive has more than one top folder".into()); }
        let inner = parts.as_path();
        if inner.as_os_str().is_empty() { continue; }
        let name = inner.components().next().map(|c| c.as_os_str().to_string_lossy().into_owned()).unwrap_or_default();
        if name.starts_with('_') && name != "_locales" { return Err(format!("Chrome refuses an extension with the top-level name {name}")); }
        let target = into.join(inner);
        if entry.is_dir() { std::fs::create_dir_all(&target).map_err(err)?; continue; }
        total = total.saturating_add(entry.size());
        if total > MAX_EXTENSION_BYTES { return Err("The Shot2AI archive is larger than 50 MB".into()); }
        std::fs::create_dir_all(target.parent().ok_or("Invalid archive path")?).map_err(err)?;
        let mut out = std::fs::OpenOptions::new().write(true).create_new(true).open(&target).map_err(err)?;
        if std::io::copy(&mut entry.by_ref().take(MAX_EXTENSION_BYTES + 1), &mut out).map_err(err)? > MAX_EXTENSION_BYTES {
            return Err("The Shot2AI archive is larger than 50 MB".into());
        }
    }
    let manifest: Value = std::fs::read(into.join("manifest.json")).ok().and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("The Shot2AI archive has no manifest.json at its root")?;
    if manifest["manifest_version"] != 3 || !manifest["version"].is_string() { return Err("The Shot2AI manifest is not a Chrome extension manifest".into()); }
    Ok(manifest)
}

/// The installed copy: its version and commit; None when there is none.
pub fn installed(store: &Store) -> Option<Value> {
    let manifest: Value = serde_json::from_slice(&std::fs::read(extension_dir(store).join("manifest.json")).ok()?).ok()?;
    Some(json!({"version":manifest["version"].as_str()?,"sha":store.setting(INSTALLED_KEY).filter(|s| valid_sha(s))}))
}

/// Replaces the installed copy with the archive of `sha`, whole or not at all.
fn install(store: &Store, sha: &str, bytes: &[u8]) -> Result<()> {
    let dir = extension_dir(store);
    let root = dir.parent().ok_or("Invalid extension folder")?.to_path_buf();
    std::fs::create_dir_all(&root).map_err(err)?;
    let (new, old) = (root.join("shot2ai.new"), root.join("shot2ai.old"));
    for leftover in [&new, &old] { if leftover.exists() { std::fs::remove_dir_all(leftover).map_err(err)?; } }
    if let Err(e) = unpack(bytes, &new) { let _ = std::fs::remove_dir_all(&new); return Err(e); }
    if dir.exists() { std::fs::rename(&dir, &old).map_err(err)?; }
    if let Err(e) = std::fs::rename(&new, &dir) {
        if old.exists() { let _ = std::fs::rename(&old, &dir); }
        return Err(err(e));
    }
    let _ = std::fs::remove_dir_all(&old);
    store.set(INSTALLED_KEY, sha)
}

/// The newest Shot2AI of main, or the last good copy, or none; and what to
/// tell the owner when it is not the newest.
async fn update_extension(store: &Store, api: &str, archive: &str, now: i64) -> (Option<Value>, Option<String>) {
    let fallback = |why: String| match installed(store) {
        Some(copy) => (Some(copy), Some(format!("{why}; the preview browser uses the Shot2AI it has."))),
        None => (None, Some(format!("{why}; the preview browser opens without Shot2AI."))),
    };
    let sha = match head(store, api, now).await {
        Ok(sha) => sha,
        Err(e) => return fallback(format!("Shot2AI could not be checked for updates ({e})")),
    };
    if installed(store).is_some_and(|copy| copy["sha"] == sha) { return (installed(store), None); }
    let downloaded = async {
        let response = client(120)?.get(format!("{archive}{sha}")).send().await.map_err(err)?.error_for_status().map_err(err)?;
        if response.content_length().unwrap_or(0) > MAX_EXTENSION_BYTES { return Err("the archive is larger than 50 MB".to_string()); }
        let bytes = response.bytes().await.map_err(err)?;
        if bytes.len() as u64 > MAX_EXTENSION_BYTES { return Err("the archive is larger than 50 MB".into()); }
        install(store, &sha, &bytes)
    }.await;
    match downloaded {
        Ok(()) => (installed(store), None),
        Err(e) => fallback(format!("Shot2AI main@{} could not be installed ({e})", &sha[..7])),
    }
}

/// Whether the preview browser runs: its profile's lock names a live Chrome for Testing.
pub fn running(profile: &Path) -> bool {
    let Ok(target) = std::fs::read_link(profile.join("SingletonLock")) else { return false };
    let Some(pid) = target.to_string_lossy().rsplit('-').next().and_then(|p| p.parse::<u32>().ok()) else { return false };
    std::process::Command::new("ps").args(["-p", &pid.to_string(), "-o", "comm="]).output()
        .is_ok_and(|o| o.status.success() && String::from_utf8_lossy(&o.stdout).contains("Chrome for Testing"))
}

/// The preview browser's command line: its own profile, the extension when
/// there is one, the page. A running browser takes the page as a new tab.
pub fn launch_args(profile: &Path, extension: Option<&Path>, url: &str) -> Vec<String> {
    let mut args = vec![format!("--user-data-dir={}", profile.display()), "--no-first-run".into(), "--no-default-browser-check".into()];
    if let Some(extension) = extension { args.push(format!("--load-extension={}", extension.display())); }
    args.push(url.into());
    args
}

/// Chrome for Testing, downloaded once (the current Stable) into the app's data.
async fn ensure_browser(store: &Store, progress: &(dyn Fn(&str) + Sync)) -> Result<PathBuf> {
    let dir = browser_dir(store);
    if binary(&dir).is_file() { return Ok(binary(&dir)); }
    let platform = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "mac-arm64",
        ("macos", _) => "mac-x64",
        _ => return Err("the preview browser is available on macOS only".into()),
    };
    progress("Downloading the preview browser (once, about 190 MB)…");
    let index: Value = client(30)?.get(CFT_INDEX).send().await.map_err(err)?.error_for_status().map_err(err)?.json().await.map_err(err)?;
    let stable = &index["channels"]["Stable"];
    let version = stable["version"].as_str().filter(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.')).ok_or("Chrome for Testing names no Stable version")?;
    let url = stable["downloads"]["chrome"].as_array().into_iter().flatten()
        .find(|d| d["platform"] == platform).and_then(|d| d["url"].as_str())
        .filter(|u| u.starts_with(CFT_DOWNLOADS)).ok_or("Chrome for Testing has no official download for this Mac")?;
    let root = dir.parent().ok_or("Invalid browser folder")?.to_path_buf();
    std::fs::create_dir_all(&root).map_err(err)?;
    let (zip, unpacked) = (root.join("chrome.zip.part"), root.join("chrome.new"));
    if unpacked.exists() { std::fs::remove_dir_all(&unpacked).map_err(err)?; }
    let mut response = client(1800)?.get(url).send().await.map_err(err)?.error_for_status().map_err(err)?;
    {
        use std::io::Write;
        let mut out = std::fs::File::create(&zip).map_err(err)?;
        let mut total = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(err)? {
            total += chunk.len() as u64;
            if total > MAX_BROWSER_BYTES { drop(out); let _ = std::fs::remove_file(&zip); return Err("the browser download is unexpectedly large".into()); }
            out.write_all(&chunk).map_err(err)?;
        }
    }
    progress("Unpacking the preview browser…");
    // ditto keeps the app bundle's framework links, which Chrome needs.
    let unzipped = tokio::process::Command::new("/usr/bin/ditto").args(["-x", "-k"]).arg(&zip).arg(&unpacked).output().await.map_err(err)?;
    let _ = std::fs::remove_file(&zip);
    let made = unpacked.join(format!("chrome-{platform}"));
    if !unzipped.status.success() || !binary(&made).is_file() {
        let _ = std::fs::remove_dir_all(&unpacked);
        return Err("the browser download could not be unpacked".into());
    }
    std::fs::rename(&made, &dir).map_err(err)?;
    let _ = std::fs::remove_dir_all(&unpacked);
    store.set(BROWSER_KEY, version)?;
    Ok(binary(&dir))
}

/// What the Preview shows before a launch: the browser and the installed Shot2AI.
pub fn status(store: &Store) -> Value {
    json!({"browser":binary(&browser_dir(store)).is_file().then(|| store.setting(BROWSER_KEY)).flatten(),
           "running":running(&profile_dir(store)),"extension":installed(store)})
}

/// Opens `url` in the preview browser. Before a new browser starts, Shot2AI
/// follows main; a running browser gets a new tab and keeps the copy it
/// loaded. Err: the browser itself could not be set up.
pub async fn open(store: &Store, url: &str, progress: &(dyn Fn(&str) + Sync)) -> Result<Value> {
    let browser = ensure_browser(store, progress).await?;
    let profile = profile_dir(store);
    let now = chrono::Utc::now().timestamp();
    let (extension, note) = if running(&profile) {
        let copy = installed(store);
        let newer = head(store, HEAD, now).await.ok().filter(|sha| copy.as_ref().is_some_and(|c| c["sha"] != sha.as_str()));
        (copy, newer.map(|sha| format!("Shot2AI main@{} loads when the preview browser starts again.", &sha[..7])))
    } else {
        progress("Checking Shot2AI…");
        update_extension(store, HEAD, ARCHIVE, now).await
    };
    std::fs::create_dir_all(&profile).map_err(err)?;
    let dir = extension_dir(store);
    let args = launch_args(&profile, extension.is_some().then_some(dir.as_path()), url);
    let mut child = std::process::Command::new(&browser).args(&args)
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null())
        .spawn().map_err(err)?;
    std::thread::spawn(move || { let _ = child.wait(); });
    Ok(json!({"browser":"preview","extension":extension,"note":note}))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn store() -> (tempfile::TempDir, Store) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("app data")).unwrap();
        (dir, store)
    }
    const SHA: &str = "3dab645c4c9cf208c405320534b208dc745988b0";
    const NEWER: &str = "4eab645c4c9cf208c405320534b208dc745988b1";
    /// An archive as codeload.github.com makes it: one top folder.
    fn archive(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.add_directory("shot2ai-3dab645/", options).unwrap();
        for (name, body) in entries {
            if let Some(target) = name.strip_prefix("link:") { zip.add_symlink(format!("shot2ai-3dab645/{target}"), "/etc/passwd", options).unwrap(); continue; }
            zip.start_file(*name, options).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.finish().unwrap().into_inner()
    }
    const MANIFEST: &str = r#"{"manifest_version":3,"name":"Shot2AI","version":"0.4.0"}"#;
    /// A GitHub API stand-in on 127.0.0.1: answers `body` and counts the requests.
    async fn github(body: &'static str) -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/repos/iOSDevSK/shot2ai/commits/main", listener.local_addr().unwrap());
        let hits = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = hits.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                counted.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let reply = format!("HTTP/1.1 200 OK\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len());
                let _ = socket.write_all(reply.as_bytes()).await;
            }
        });
        (url, hits)
    }

    #[test]
    fn a_commit_is_forty_lowercase_hex_digits() {
        assert!(valid_sha(SHA));
        assert!(!valid_sha(&SHA[..39]) && !valid_sha(&SHA.to_uppercase()) && !valid_sha("<html>not found</html>"));
    }

    #[tokio::test]
    async fn main_is_asked_at_most_every_ten_minutes() {
        let (_dir, store) = store();
        let (api, hits) = github(SHA).await;
        let now = 1_800_000_000;
        assert_eq!(head(&store, &api, now).await.unwrap(), SHA);
        assert_eq!(head(&store, &api, now + 599).await.unwrap(), SHA, "cached");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "one request in ten minutes");
        head(&store, &api, now + 600).await.unwrap();
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 2, "asked again after ten minutes");
        // A clock set back does not keep an old answer forever.
        head(&store, &api, now - 5).await.unwrap();
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 3);
        // An answer that is not a commit (a proxy's page) is not taken.
        let (_other, store2) = self::store();
        let (bad, _) = github("<html>blocked</html>").await;
        assert!(head(&store2, &bad, now).await.is_err());
        assert!(store2.setting(HEAD_KEY).is_none());
    }

    #[test]
    fn the_archive_unpacks_without_its_top_folder() {
        let dir = tempfile::tempdir().unwrap();
        let into = dir.path().join("shot2ai");
        let manifest = unpack(&archive(&[("shot2ai-3dab645/manifest.json", MANIFEST), ("shot2ai-3dab645/src/background.js", "//"), ("shot2ai-3dab645/_locales/en/messages.json", "{}")]), &into).unwrap();
        assert_eq!(manifest["version"], "0.4.0");
        assert!(into.join("manifest.json").is_file() && into.join("src/background.js").is_file() && into.join("_locales/en/messages.json").is_file());
    }

    #[test]
    fn an_unsafe_archive_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let cases: &[(&[(&str, &str)], &str)] = &[
            (&[("shot2ai-3dab645/manifest.json", MANIFEST), ("shot2ai-3dab645/../../escaped.txt", "x")], "outside"),
            (&[("shot2ai-3dab645/manifest.json", MANIFEST), ("/etc/escaped.txt", "x")], "outside"),
            (&[("shot2ai-3dab645/manifest.json", MANIFEST), ("link:src/passwd", "")], "links"),
            (&[("shot2ai-3dab645/manifest.json", MANIFEST), ("other/manifest.json", MANIFEST)], "top folder"),
            (&[("shot2ai-3dab645/src/background.js", "//")], "no manifest.json"),
            (&[("shot2ai-3dab645/manifest.json", r#"{"name":"page"}"#)], "not a Chrome extension"),
            (&[("shot2ai-3dab645/manifest.json", MANIFEST), ("shot2ai-3dab645/_cache/x", "x")], "top-level name _cache"),
        ];
        for (i, (entries, why)) in cases.iter().enumerate() {
            let into = dir.path().join(format!("case-{i}"));
            let refused = unpack(&archive(entries), &into).unwrap_err();
            assert!(refused.contains(why), "case {i}: {refused}");
        }
        assert!(!dir.path().join("escaped.txt").exists() && !Path::new("/etc/escaped.txt").exists());
        assert!(unpack(b"<html>rate limited</html>", &dir.path().join("html")).unwrap_err().contains("not a ZIP"));
    }

    #[test]
    fn a_failed_update_keeps_the_last_good_copy_at_the_same_folder() {
        let (_dir, store) = store();
        install(&store, SHA, &archive(&[("shot2ai-3dab645/manifest.json", MANIFEST)])).unwrap();
        let folder = extension_dir(&store);
        assert_eq!(installed(&store).unwrap(), json!({"version":"0.4.0","sha":SHA}));
        assert!(install(&store, NEWER, &archive(&[("shot2ai-3dab645/../x", "x")])).is_err());
        assert_eq!(installed(&store).unwrap(), json!({"version":"0.4.0","sha":SHA}), "the last good copy stays");
        let newer = MANIFEST.replace("0.4.0", "0.4.1");
        install(&store, NEWER, &archive(&[("shot2ai-3dab645/manifest.json", &newer)])).unwrap();
        assert_eq!(installed(&store).unwrap(), json!({"version":"0.4.1","sha":NEWER}));
        assert_eq!(extension_dir(&store), folder, "one folder: the extension keeps its ID and pairing");
        let left: Vec<_> = std::fs::read_dir(folder.parent().unwrap()).unwrap().map(|e| e.unwrap().file_name()).collect();
        assert_eq!(left, vec![std::ffi::OsString::from("shot2ai")], "nothing half-made is left");
    }

    #[tokio::test]
    async fn offline_the_last_good_copy_or_none_is_used_and_said() {
        let (_dir, store) = store();
        let offline = "http://127.0.0.1:9/repos/iOSDevSK/shot2ai/commits/main";
        let (none, why) = update_extension(&store, offline, "http://127.0.0.1:9/", 1_800_000_000).await;
        assert!(none.is_none() && why.unwrap().contains("opens without Shot2AI"));
        install(&store, SHA, &archive(&[("shot2ai-3dab645/manifest.json", MANIFEST)])).unwrap();
        let (copy, why) = update_extension(&store, offline, "http://127.0.0.1:9/", 1_800_000_000).await;
        assert_eq!(copy.unwrap()["sha"], SHA);
        assert!(why.unwrap().contains("uses the Shot2AI it has"));
        // main moved on but its archive cannot be fetched: the last good copy, said.
        let (api, _) = github(NEWER).await;
        let (copy, why) = update_extension(&store, &api, "http://127.0.0.1:9/", 1_800_000_000).await;
        assert_eq!(copy.unwrap()["sha"], SHA);
        assert!(why.unwrap().starts_with("Shot2AI main@4eab645 could not be installed"));
        // Up to date: nothing downloaded, nothing to say.
        let (_d, current) = self::store();
        install(&current, SHA, &archive(&[("shot2ai-3dab645/manifest.json", MANIFEST)])).unwrap();
        let (api, _) = github(SHA).await;
        assert_eq!(update_extension(&current, &api, "http://127.0.0.1:9/", 1_800_000_000).await, (Some(json!({"version":"0.4.0","sha":SHA})), None));
    }

    #[test]
    fn the_launch_has_its_own_profile_and_the_extension_when_there_is_one() {
        let profile = Path::new("/Users/o/Library/Application Support/dev.html2wp.desktop/preview-browser/profile");
        let extension = Path::new("/Users/o/Library/Application Support/dev.html2wp.desktop/extensions/shot2ai");
        let args = launch_args(profile, Some(extension), "http://localhost:8123/wp-admin/");
        assert_eq!(args, vec![
            "--user-data-dir=/Users/o/Library/Application Support/dev.html2wp.desktop/preview-browser/profile",
            "--no-first-run", "--no-default-browser-check",
            "--load-extension=/Users/o/Library/Application Support/dev.html2wp.desktop/extensions/shot2ai",
            "http://localhost:8123/wp-admin/"]);
        let without = launch_args(profile, None, "http://localhost:8123/");
        assert!(!without.iter().any(|a| a.starts_with("--load-extension")) && without.last().unwrap() == "http://localhost:8123/");
        assert!(!args.iter().any(|a| a.contains("remote-debugging")));
    }

    #[cfg(unix)]
    #[test]
    fn a_stale_profile_lock_is_not_a_running_browser() {
        let dir = tempfile::tempdir().unwrap();
        assert!(!running(dir.path()), "no lock");
        std::os::unix::fs::symlink("Mac-999999", dir.path().join("SingletonLock")).unwrap();
        assert!(!running(dir.path()), "no such process");
        std::fs::remove_file(dir.path().join("SingletonLock")).unwrap();
        std::os::unix::fs::symlink(format!("Mac-{}", std::process::id()), dir.path().join("SingletonLock")).unwrap();
        assert!(!running(dir.path()), "a live process that is not the preview browser");
    }

    /// The real browser loads the unpacked extension from a spaced path
    /// (H2WP_TEST_CHROME: the Chrome for Testing binary; H2WP_TEST_SHOT2AI:
    /// an unpacked Shot2AI). Headless, with a port only this test opens;
    /// headless Chrome does not hand a second launch to the first one, so the
    /// new-tab reuse is the headed test's.
    #[tokio::test]
    #[ignore = "real Chrome for Testing; set H2WP_TEST_CHROME and H2WP_TEST_SHOT2AI"]
    async fn chrome_for_testing_loads_shot2ai_from_a_spaced_path() {
        let (Ok(chrome), Ok(extension)) = (std::env::var("H2WP_TEST_CHROME"), std::env::var("H2WP_TEST_SHOT2AI")) else { return };
        let dir = tempfile::tempdir().unwrap();
        let profile = dir.path().join("app data/preview-browser/profile");
        let copy = dir.path().join("app data/extensions/shot2ai");
        std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
        assert!(std::process::Command::new("cp").arg("-R").arg(&extension).arg(&copy).status().unwrap().success());
        let mut args = launch_args(&profile, Some(&copy), "about:blank");
        args.splice(0..0, ["--headless=new".to_string(), "--remote-debugging-port=9334".into()]);
        let mut browser = std::process::Command::new(&chrome).args(args).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let mut worker = false;
        for _ in 0..40 {
            tokio::time::sleep(Duration::from_millis(500)).await;
            let list = async { reqwest::get("http://127.0.0.1:9334/json/list").await.ok()?.json::<Value>().await.ok() }.await;
            worker = list.and_then(|l| l.as_array().cloned()).unwrap_or_default().iter().any(|t| t["type"] == "service_worker" && t["url"].as_str().is_some_and(|u| u.ends_with("/src/background.js")));
            if worker { break; }
        }
        let running_now = running(&profile);
        let _ = browser.kill();
        let _ = browser.wait();
        assert!(worker, "Shot2AI's service worker runs");
        assert!(running_now, "the profile lock names the running browser");
    }
    /// The whole path, headed, as the owner sees it: Chrome for Testing
    /// downloaded once, Shot2AI from main, the launch, and a second open that
    /// becomes a tab of the running browser (H2WP_TEST_PREVIEW_DATA: a folder
    /// kept between runs, so the browser is downloaded once).
    #[tokio::test]
    #[ignore = "real downloads and a browser window; set H2WP_TEST_PREVIEW_DATA"]
    async fn the_preview_browser_opens_with_the_newest_shot2ai_and_reuses_itself() {
        let Ok(data) = std::env::var("H2WP_TEST_PREVIEW_DATA") else { return };
        let store = Store::open(PathBuf::from(data)).unwrap();
        let said = std::sync::Mutex::new(Vec::<String>::new());
        let progress = |m: &str| said.lock().unwrap().push(m.to_string());
        let first = open(&store, "data:text/html,<h1>one</h1>", &progress).await.unwrap();
        let profile = profile_dir(&store);
        let mut up = false;
        for _ in 0..30 { tokio::time::sleep(Duration::from_millis(500)).await; if running(&profile) { up = true; break; } }
        let second = open(&store, "data:text/html,<h1>two</h1>", &progress).await.unwrap();
        tokio::time::sleep(Duration::from_secs(3)).await;
        // One browser process for the profile: the second launch handed its page over and left.
        let listed = std::process::Command::new("pgrep").args(["-f", &format!("user-data-dir={}", profile.display())]).output().unwrap();
        let pids: Vec<String> = String::from_utf8_lossy(&listed.stdout).lines().map(String::from).collect();
        let browsers = pids.iter().filter(|pid| std::process::Command::new("ps").args(["-p", pid, "-o", "args="]).output().is_ok_and(|o| !String::from_utf8_lossy(&o.stdout).contains("--type="))).count();
        eprintln!("first: {first}\nsecond: {second}\nsaid: {:?}\nbrowser processes: {browsers}", said.lock().unwrap());
        assert!(up, "the preview browser runs");
        assert_eq!(first["browser"], "preview");
        assert!(first["extension"]["sha"].as_str().is_some_and(valid_sha) && first["note"].is_null(), "the newest Shot2AI, nothing to say");
        assert_eq!(second["extension"], first["extension"]);
        assert_eq!(browsers, 1, "the second open is a tab of the running browser");
    }
}
