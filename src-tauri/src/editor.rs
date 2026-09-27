//! Visual Edit Lite: the free editor for converted themes, fetched from its
//! public GitHub releases and offered in Exports.
use crate::{model::{err, Result}, store::Store};
use serde_json::{json, Value};
use std::{io::Read, path::PathBuf, time::Duration};

const RELEASES: &str = "https://api.github.com/repos/iOSDevSK/visual-edit-lite/releases/latest";
const PLUGIN_FILE: &str = "visual-edit-lite/visual-edit-lite.php";
const CHECK_EVERY_SECS: i64 = 12 * 60 * 60;
const MAX_BYTES: u64 = 25 * 1024 * 1024;

fn cache_dir(store: &Store) -> PathBuf { store.root.join("cache").join("visual-edit-lite") }

fn cached(store: &Store) -> Option<(String, PathBuf)> {
    let tag = store.setting("visual-edit-lite-tag")?;
    let path = cache_dir(store).join(format!("visual-edit-lite-{tag}.zip"));
    path.is_file().then_some((tag, path))
}

/// A real Visual Edit Lite plugin archive, not an HTML error page.
fn valid_archive(bytes: &[u8]) -> bool {
    let Ok(mut archive) = zip::ZipArchive::new(std::io::Cursor::new(bytes)) else { return false };
    let Ok(mut file) = archive.by_name(PLUGIN_FILE) else { return false };
    let mut head = String::new();
    file.by_ref().take(4096).read_to_string(&mut head).is_ok() && head.contains("Plugin Name")
}

fn safe_tag(tag: &str) -> bool {
    !tag.is_empty() && tag.len() <= 40 && tag.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
}

/// The newest release, cached for 12 hours; a network failure uses the cache.
pub async fn latest(store: &Store) -> Result<(String, PathBuf)> {
    let checked = store.setting("visual-edit-lite-checked").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let now = chrono::Utc::now().timestamp();
    if let Some(hit) = cached(store) {
        if now - checked < CHECK_EVERY_SECS { return Ok(hit); }
    }
    match download(store).await {
        Ok(found) => { store.set("visual-edit-lite-checked", &now.to_string())?; Ok(found) }
        Err(error) => cached(store).ok_or(error),
    }
}

async fn download(store: &Store) -> Result<(String, PathBuf)> {
    let client = reqwest::Client::builder().https_only(true).timeout(Duration::from_secs(60))
        .user_agent(concat!("html2wp-desktop/", env!("CARGO_PKG_VERSION"))).build().map_err(err)?;
    let release: Value = client.get(RELEASES).header("accept", "application/vnd.github+json")
        .send().await.map_err(err)?.error_for_status().map_err(err)?.json().await.map_err(err)?;
    let tag = release["tag_name"].as_str().unwrap_or_default().to_string();
    if !safe_tag(&tag) { return Err("Visual Edit Lite release has no usable version".into()); }
    let target = cache_dir(store).join(format!("visual-edit-lite-{tag}.zip"));
    if !target.is_file() {
        let asset = release["assets"].as_array().into_iter().flatten()
            .find(|a| a["name"].as_str().is_some_and(|n| n.starts_with("visual-edit-lite") && n.ends_with(".zip")))
            .ok_or("Visual Edit Lite release has no plugin ZIP")?;
        if asset["size"].as_u64().unwrap_or(u64::MAX) > MAX_BYTES { return Err("Visual Edit Lite ZIP is unexpectedly large".into()); }
        let url = asset["browser_download_url"].as_str().unwrap_or_default();
        if !url.starts_with("https://github.com/iOSDevSK/visual-edit-lite/releases/download/") {
            return Err("Visual Edit Lite download address is not the official release".into());
        }
        let bytes = client.get(url).send().await.map_err(err)?.error_for_status().map_err(err)?.bytes().await.map_err(err)?;
        if bytes.len() as u64 > MAX_BYTES || !valid_archive(&bytes) {
            return Err("The downloaded Visual Edit Lite ZIP is not a valid plugin".into());
        }
        std::fs::create_dir_all(cache_dir(store)).map_err(err)?;
        let partial = target.with_extension("part");
        std::fs::write(&partial, &bytes).map_err(err)?;
        std::fs::rename(&partial, &target).map_err(err)?;
    }
    store.set("visual-edit-lite-tag", &tag)?;
    Ok((tag, target))
}

pub fn status(store: &Store) -> Value {
    match cached(store) {
        Some((tag, _)) => json!({"available":true,"version":tag}),
        None => json!({"available":false}),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    fn archive(entry: &str, body: &str) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut out);
        zip.start_file(entry, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
        zip.finish().unwrap();
        out.into_inner()
    }
    #[test]
    fn only_a_real_plugin_archive_and_a_plain_tag_are_accepted() {
        assert!(valid_archive(&archive(PLUGIN_FILE, "<?php\n/*\n * Plugin Name: Visual Edit Lite\n */")));
        assert!(!valid_archive(&archive("other/other.php", "Plugin Name: x")));
        assert!(!valid_archive(b"<html>rate limited</html>"));
        assert!(safe_tag("1.31.1") && !safe_tag("../1") && !safe_tag(""));
    }
}
