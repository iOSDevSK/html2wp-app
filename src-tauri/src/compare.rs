//! The page-by-page comparison the owner asks for (APP-CONTRACT v1.2 §6b):
//! the plugin's visual-compare.py captures the source beside the preview
//! WordPress, page by page, at 1440 and 390; the app runs it when the owner
//! clicks, polls its status, and shows its index and images. No AI turn, and
//! the app never judges the numbers.
use crate::{model::*, store::Store};
use serde_json::{json, Value};

/// The plugin's command, run in the project container with its environment.
pub const COMMAND: &str = "python3 /opt/html2wp/skills/html2wp/assets/scripts/visual-compare.py /project/workspace";
/// Its index and its status, relative to the project folder; images are
/// named relative to the workspace and live under visual-review/.
pub const INDEX: &str = "workspace/visual-review/visual-compare.json";
pub const STATUS: &str = "workspace/visual-review/status.json";
pub const SCHEMA: &str = "h2wp-visual-compare/1";
const REVIEW: &str = "visual-review/";
/// Longest a comparison may take (12 pages at both widths took 64 s).
pub const SECONDS: u64 = 1800;
/// Largest image the app shows.
const IMAGE_MAX: u64 = 30 * 1024 * 1024;

/// An image the index names: a PNG under visual-review/, relative to the workspace.
fn image_path(v: &Value) -> Option<String> {
    let path = v.as_str()?;
    let plain = path.starts_with(REVIEW) && path.len() <= 300 && path.ends_with(".png") && !path.contains('\\')
        && path.split('/').all(|c| !c.is_empty() && c != ".." && c != ".");
    plain.then(|| path.to_string())
}
fn number(v: &Value) -> Value { v.as_f64().filter(|n| n.is_finite() && *n >= 0.0).map_or(Value::Null, |n| json!((n * 100.0).round() / 100.0)) }
fn text(v: &Value, max: usize) -> Value { v.as_str().map_or(Value::Null, |s| json!(s.chars().filter(|c| !c.is_control()).take(max).collect::<String>())) }
fn read(store: &Store, p: &Project, relative: &str, schema: &str) -> Value {
    let Ok(root) = store.path(&p.id) else { return json!({}) };
    let file = root.join(relative);
    let regular = std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_file() && m.len() <= 2 * 1024 * 1024);
    let found = regular.then(|| std::fs::read(&file).ok()).flatten().and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
    found.filter(|v| v["schema"] == schema).unwrap_or(json!({}))
}
/// One width of a page: its image and what the plugin measured, or why it has none.
fn view(v: &Value) -> Value {
    if !v.is_object() { return Value::Null; }
    json!({"referenceKind":text(&v["referenceKind"], 40),"referenceNotice":text(&v["referenceNotice"], 600),"image":image_path(&v["image"]),"diffPercent":number(&v["diffPercent"]),"origHeight":v["origHeight"].as_u64(),"wpHeight":v["wpHeight"].as_u64(),
        "error":if image_path(&v["image"]).is_none() { text(&v["error"], 400) } else { Value::Null }})
}

/// The comparison as the owner sees it: every page with its title, route and
/// its desktop and mobile views; {} before one was made. Only the contract's
/// schema is read (an earlier release left other files in visual-review/).
pub fn index(store: &Store, p: &Project) -> Value {
    let found = read(store, p, INDEX, SCHEMA);
    if !found.is_object() || found.as_object().is_some_and(|o| o.is_empty()) { return json!({}); }
    let pages: Vec<Value> = found["pages"].as_array().into_iter().flatten().take(500)
        .map(|page| json!({"key":text(&page["key"], 120),"title":text(&page["title"], 200),"page":text(&page["page"], 200),"route":text(&page["route"], 400),
            "desktop":view(&page["desktop"]),"mobile":view(&page["mobile"])}))
        .filter(|page| page["key"].is_string()).collect();
    json!({"capturedAt":text(&found["capturedAt"], 40),"preview":text(&found["preview"], 200),"pages":pages})
}
/// The comparison's status as the plugin writes it (running, done, failed,
/// with a note), and whether the app is running one for this project now.
pub fn status(store: &Store, p: &Project, running: bool) -> Value {
    let found = read(store, p, STATUS, &format!("{SCHEMA}-status"));
    let state = found["state"].as_str().filter(|s| matches!(*s, "running" | "done" | "failed"));
    json!({"state":state,"note":text(&found["note"], 600),"startedAt":text(&found["startedAt"], 40),"updatedAt":text(&found["updatedAt"], 40),"running":running})
}

/// One side-by-side image the index names, as a data URL for the webview.
pub fn image(store: &Store, p: &Project, path: &str) -> Result<String> {
    use base64::Engine;
    let listed = index(store, p)["pages"].as_array().into_iter().flatten()
        .any(|page| page["desktop"]["image"] == path || page["mobile"]["image"] == path);
    let Some(path) = image_path(&json!(path)).filter(|_| listed) else { return Err("This image is not part of the comparison.".into()) };
    let root = store.path(&p.id)?;
    let review = root.join("workspace").join(REVIEW);
    let file = root.join("workspace").join(&path);
    let meta = std::fs::symlink_metadata(&file).map_err(|_| "The comparison image is missing. Generate the comparison again.")?;
    let inside = matches!((std::fs::canonicalize(&review), std::fs::canonicalize(&file)), (Ok(dir), Ok(found)) if found.starts_with(&dir));
    if !meta.file_type().is_file() || !inside { return Err("This image is not part of the comparison.".into()); }
    if meta.len() > IMAGE_MAX { return Err("The comparison image is larger than 30 MB.".into()); }
    let bytes = std::fs::read(&file).map_err(err)?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("The comparison image is not a PNG.".into()); }
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// Run the plugin's comparison in the project container, to its end. Its
/// status file says how it went; the error is the plugin's own reason.
pub async fn run(store: &Store, p: &Project, image: &str, desktop_only: bool, page_key: Option<&str>) -> Result<Value> {
    // An HTML theme against its preview WordPress; an Astro run against its built site (v1.4 §6b).
    if !matches!(p.target.as_str(), "html" | "astro") { return Err("The comparison compares a site with its WordPress theme or its built Astro site.".into()); }
    let mut command = if desktop_only { format!("{COMMAND} --desktop-only") } else { COMMAND.to_string() };
    if let Some(key) = page_key {
        if key.is_empty() || key.len()>120 || !key.chars().all(|c|c.is_ascii_alphanumeric() || "-_".contains(c)) {
            return Err("Invalid comparison page key".into());
        }
        command.push_str(&format!(" --page-key {key}"));
    }
    let ran = crate::skill::exec(store, p, image, &json!({"cmd":command,"timeout":SECONDS})).await?;
    if ran["exitCode"] == 0 { return Ok(index(store, p)); }
    let written = status(store, p, false);
    let tail = ran["output"].as_str().unwrap_or("").lines().rev().find(|l| !l.trim().is_empty()).map(str::trim).map(String::from);
    let reason = if ran["timedOut"] == true { format!("it ran past {} minutes and was stopped", SECONDS / 60) }
        else { written["note"].as_str().map(String::from).or(tail).unwrap_or_else(|| "the plugin's comparison did not finish".into()) };
    Err(format!("The comparison stopped: {reason}"))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::path::Path;
    #[test]
    fn reference_provenance_survives_the_ui_boundary() {
        let desktop = view(&json!({"image":"visual-review/new/page.png","referenceKind":"astro-reference","referenceNotice":"Original capture unavailable"}));
        let mobile = view(&json!({"image":"visual-review/old/page.png","referenceKind":"original"}));
        assert_eq!(desktop["referenceKind"], "astro-reference");
        assert_eq!(desktop["referenceNotice"], "Original capture unavailable");
        assert_eq!(mobile["referenceKind"], "original");
        assert!(mobile["referenceNotice"].is_null());
    }

    /// tests/fixtures/visual-review as the plugin leaves visual-review/.
    pub(crate) fn fixture_into(store: &Store, p: &Project) -> std::path::PathBuf {
        let dir = store.path(&p.id).unwrap().join("workspace/visual-review");
        let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/visual-review");
        for entry in walkdir::WalkDir::new(&fixtures).into_iter().flatten().filter(|e| e.file_type().is_file()) {
            let to = dir.join(entry.path().strip_prefix(&fixtures).unwrap());
            std::fs::create_dir_all(to.parent().unwrap()).unwrap();
            std::fs::copy(entry.path(), to).unwrap();
        }
        dir
    }
    #[test]
    fn the_index_lists_every_page_at_both_widths_as_the_plugin_measured_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let p = crate::skill::tests::project();
        assert_eq!(index(&store, &p), json!({}));
        let review = fixture_into(&store, &p);
        let found = index(&store, &p);
        let pages = found["pages"].as_array().unwrap();
        assert_eq!(pages.iter().map(|p| p["key"].as_str().unwrap()).collect::<Vec<_>>(), ["front-page", "about", "contact"]);
        assert_eq!((pages[0]["title"].clone(), pages[0]["desktop"]["image"].clone(), pages[0]["desktop"]["diffPercent"].clone(), pages[0]["mobile"]["image"].clone()),
            (json!("Home"), json!("visual-review/front-page.side-by-side.png"), json!(0.42), json!("visual-review/mobile/front-page.side-by-side.png")));
        assert_eq!((pages[2]["desktop"]["image"].clone(), pages[2]["desktop"]["error"].as_str().unwrap().contains("timed out")), (Value::Null, true), "a page the plugin could not capture");
        for image_of in [&pages[0]["desktop"]["image"], &pages[0]["mobile"]["image"], &pages[1]["desktop"]["image"]] {
            assert!(image(&store, &p, image_of.as_str().unwrap()).unwrap().starts_with("data:image/png;base64,"));
        }
        // Only what the index names, only a PNG under visual-review/.
        for bad in ["visual-review/../conversion-manifest.json", "visual-review/unlisted.png", "visual-review/visual-compare.json", "/etc/passwd", "front-page.side-by-side.png"] {
            assert!(image(&store, &p, bad).is_err(), "{bad}");
        }
        assert_eq!(status(&store, &p, false)["state"], "done");
        std::fs::write(review.join("visual-compare.json"), json!({"schema":"h2wp-visual-review/1","pages":[]}).to_string()).unwrap();
        assert_eq!(index(&store, &p), json!({}), "another schema is not this comparison");
    }
    #[test]
    fn a_hostile_index_names_nothing_outside_its_folder() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let p = crate::skill::tests::project();
        let review = fixture_into(&store, &p);
        std::fs::write(review.join("visual-compare.json"), json!({"schema":SCHEMA,"pages":[
            {"key":"a","desktop":{"image":"../private/licence.png"}},{"key":"b","desktop":{"image":"visual-review/../../x.png","diffPercent":-3}},
            {"key":"c","desktop":{"image":"visual-review/ok.png","diffPercent":"12"}}]}).to_string()).unwrap();
        let pages = index(&store, &p)["pages"].as_array().unwrap().clone();
        assert_eq!(pages.iter().map(|p| p["desktop"]["image"].clone()).collect::<Vec<_>>(), [Value::Null, Value::Null, json!("visual-review/ok.png")]);
        assert_eq!((pages[1]["desktop"]["diffPercent"].clone(), pages[2]["desktop"]["diffPercent"].clone()), (Value::Null, Value::Null), "a number or nothing");
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret.png"), b"\x89PNG\r\n\x1a\nsecret").unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret.png"), review.join("ok.png")).unwrap();
            assert!(image(&store, &p, "visual-review/ok.png").is_err(), "a linked image is refused");
        }
        std::fs::write(review.join("status.json"), json!({"schema":"h2wp-visual-compare/1-status","state":"exploded","note":"x"}).to_string()).unwrap();
        assert_eq!(status(&store, &p, true), json!({"state":null,"note":"x","startedAt":null,"updatedAt":null,"running":true}));
    }
}
