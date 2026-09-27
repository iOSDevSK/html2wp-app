//! The Compare tab of a "Gutenberg from an HTML theme" project: the ORIGINAL
//! HTML theme beside the new Gutenberg theme, page by page, at 1440 and 390,
//! with the difference in percent. The h2g skill's own scripts do it:
//! render-original.py (with the config the conversion wrote at its Verify
//! step) composes the originals, and visual-diff.py screenshots them against
//! the sandbox's WordPress and measures each page. The app runs them in the
//! project's sandbox, writes the index the Compare tab reads, and shows the
//! images. No AI turn, and the app never judges the numbers.
use crate::{model::*, store::Store};
use serde_json::{json, Value};
use std::path::Path;

/// Where the comparison goes, in /work.
const DIR: &str = "compare";
const INDEX: &str = "compare/index.json";
const STATUS: &str = "compare/status.json";
const SCHEMA: &str = "h2wpd-h2g-compare/1";
const WIDTHS: [(&str, u32); 2] = [("desktop", 1440), ("mobile", 390)];
const IMAGE_MAX: u64 = 30 * 1024 * 1024;

fn work(store: &Store, p: &Project) -> Result<std::path::PathBuf> { crate::h2g::work_dir(store, &p.id) }
fn read(store: &Store, p: &Project, name: &str) -> Value {
    let Ok(work) = work(store, p) else { return json!({}) };
    let file = work.join(name);
    let found = (crate::h2g::under_work(&work, &file, false) && std::fs::metadata(&file).is_ok_and(|m| m.len() <= 2 * 1024 * 1024))
        .then(|| std::fs::read(&file).ok()).flatten().and_then(|raw| serde_json::from_slice::<Value>(&raw).ok());
    found.filter(|v| v["schema"] == SCHEMA).unwrap_or(json!({}))
}
fn write(store: &Store, p: &Project, name: &str, doc: &Value) -> Result<()> {
    let work = work(store, p)?;
    let dir = work.join(DIR);
    if std::fs::symlink_metadata(&dir).is_ok_and(|m| m.file_type().is_symlink()) { std::fs::remove_file(&dir).map_err(err)?; }
    std::fs::create_dir_all(&dir).map_err(err)?;
    let file = work.join(name);
    if std::fs::symlink_metadata(&file).is_ok_and(|m| m.file_type().is_symlink()) { std::fs::remove_file(&file).map_err(err)?; }
    let temp=dir.join(format!(".write-{}.tmp",id()));
    std::fs::write(&temp, serde_json::to_vec_pretty(doc).map_err(err)?).map_err(err)?;
    std::fs::rename(temp,&file).map_err(err)
}
fn set_status(store: &Store, p: &Project, state: &str, note: &str, started: &str) {
    let _ = write(store, p, STATUS, &json!({"schema":SCHEMA,"state":state,"note":note,"startedAt":started,"updatedAt":now()}));
}

/// The comparison as the Compare tab reads it (the same shape as the HTML
/// one); {} before one was made.
pub fn index(store: &Store, p: &Project) -> Value {
    let found = read(store, p, INDEX);
    if found.as_object().is_some_and(|o| o.is_empty()) { return json!({}); }
    found
}
pub fn status(store: &Store, p: &Project, running: bool) -> Value {
    let found = read(store, p, STATUS);
    json!({"state":found["state"].as_str().filter(|s| matches!(*s, "running" | "done" | "failed")),"note":found["note"],"startedAt":found["startedAt"],"updatedAt":found["updatedAt"],"running":running})
}
/// One image the index names, as a data URL.
pub fn image(store: &Store, p: &Project, path: &str) -> Result<String> {
    use base64::Engine;
    let listed = index(store, p)["pages"].as_array().into_iter().flatten()
        .any(|page| ["desktop", "mobile"].iter().any(|w| page[w]["image"] == path || page[w]["original"] == path));
    let plain = path.starts_with("compare/") && path.ends_with(".png") && path.split('/').all(|c| !c.is_empty() && c != ".." && c != ".");
    if !listed || !plain { return Err("This image is not part of the comparison.".into()); }
    let work = work(store, p)?;
    let file = work.join(path);
    if !crate::h2g::under_work(&work, &file, false) { return Err("The comparison image is missing. Generate the comparison again.".into()); }
    if std::fs::metadata(&file).map_err(err)?.len() > IMAGE_MAX { return Err("The comparison image is larger than 30 MB.".into()); }
    let bytes = std::fs::read(&file).map_err(err)?;
    if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") { return Err("The comparison image is not a PNG.".into()); }
    Ok(format!("data:image/png;base64,{}", base64::engine::general_purpose::STANDARD.encode(bytes)))
}

/// The config render-original.py was given at the Verify step (in /work),
/// and the folder its composed originals go to (its "out"), as the sandbox sees them.
pub fn originals(work: &Path) -> Option<(String, String)> {
    let found = walkdir::WalkDir::new(work).max_depth(3).follow_links(false).into_iter().flatten()
        .find(|e| e.file_name() == "render-original.json" && crate::h2g::under_work(work, e.path(), false))?;
    let config: Value = serde_json::from_slice(&std::fs::read(found.path()).ok()?).ok()?;
    let base = found.path().parent()?;
    let out = config["out"].as_str()?;
    let out = if Path::new(out).is_absolute() { Path::new(out).strip_prefix("/work").ok().map(|r| work.join(r))? } else { base.join(out) };
    let inside = |p: &Path| -> Option<String> {
        let rel = p.strip_prefix(work).ok()?;
        let s = Path::new("/work").join(rel).to_string_lossy().into_owned();
        (!s.split('/').any(|c| c == "..") && s.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c))).then_some(s)
    };
    Some((inside(found.path())?, inside(&out)?))
}
/// visual-diff.py's report lines: "  ok about   0.42% differing pixels  …", "  !! key  no converted page".
pub fn parse(report: &str) -> Vec<(String, Option<f64>, String)> {
    report.lines().filter_map(|line| {
        let line = line.strip_prefix("  ")?;
        let (flag, rest) = (line.get(..3)?, line.get(3..)?);
        if !matches!(flag, "ok " | "~  " | "!! ") { return None; }
        let mut parts = rest.split_whitespace();
        let key = parts.next()?.to_string();
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { return None; }
        let tail = rest[rest.find(&key)? + key.len()..].trim();
        match tail.split_once("% differing pixels") {
            Some((pct, note)) => Some((key, pct.trim().parse::<f64>().ok(), note.trim().to_string())),
            None => Some((key, None, tail.to_string())),
        }
    }).collect()
}

/// Generate: the originals (composed once), then both widths against the
/// sandbox's WordPress; the index names every page with its images.
pub async fn run(store: &Store, p: &Project, image: &str, page_key: Option<&str>) -> Result<Value> {
    if page_key.is_some_and(|k|k.is_empty() || !k.chars().all(|c|c.is_ascii_alphanumeric() || "-_".contains(c))) {return Err("Invalid comparison page key".into());}
    let started = now();
    let work = work(store, p)?;
    set_status(store, p, "running", "starting the sandbox's WordPress", &started);
    let result = capture(store, p, image, &work, &started, page_key).await;
    match &result {
        Ok(_) => set_status(store, p, "done", "compared", &started),
        Err(e) => set_status(store, p, "failed", e, &started),
    }
    result
}
async fn capture(store: &Store, p: &Project, image: &str, work: &Path, started: &str, page_key: Option<&str>) -> Result<Value> {
    let skill = crate::h2g::SKILL;
    let (wordpress, port) = crate::h2g_preview::sandbox(store, p).ok_or("The Gutenberg sandbox is not set up yet: the conversion builds it at its Verify step.")?;
    let (config, originals) = originals(work).ok_or("The originals are not composed yet: the conversion writes render-original.json at its Verify step.")?;
    let name = crate::h2g::ensure_sandbox(store, p, image).await?;
    crate::h2g_preview::serve_inside(&name, &wordpress, port).await?;
    let sh = |cmd: String| { let name = name.clone(); async move { crate::agent::exec(&name, "/work", &[], &cmd, 1800).await } };
    let has_originals = std::fs::read_dir(work.join(originals.trim_start_matches("/work/"))).into_iter().flatten().flatten()
        .any(|e| e.path().extension().is_some_and(|x| x == "html"));
    if !has_originals {
        set_status(store, p, "running", "composing the original pages", started);
        let rendered = sh(format!("python3 {skill}/scripts/render-original.py --config {config}")).await?;
        if rendered["exitCode"] != 0 { return Err(format!("render-original.py failed: {}", tail(&rendered))); }
    }
    let previous=index(store,p);
    if let Some(key)=page_key {if !previous["pages"].as_array().is_some_and(|ps|ps.iter().any(|p|p["key"]==key)){return Err("Generate all pages before refreshing this page".into());}}
    let capture_dir=if page_key.is_some(){format!("{DIR}/refresh-{}",id())}else{DIR.to_string()};
    let mut pages: Vec<Value> = vec![];
    for (width_name, width) in WIDTHS {
        set_status(store, p, "running", &format!("capturing {width_name} ({width}px)"), started);
        let out = format!("/work/{capture_dir}/{width_name}");
        // --threshold 100: every page is measured and shown; the owner judges.
        let ran = sh(format!("rm -rf {out} && python3 {skill}/scripts/visual-diff.py --original {originals} --live http://127.0.0.1:{port} --out {out} --width {width} --threshold 100 -- {}",page_key.unwrap_or(""))).await?;
        let measured = parse(ran["output"].as_str().unwrap_or_default());
        if measured.is_empty() { return Err(format!("visual-diff.py measured no page: {}", tail(&ran))); }
        for (key, pct, note) in measured {
            let view = match pct {
                Some(pct) => json!({"image":format!("{capture_dir}/{width_name}/{key}-converted.png"),"original":format!("{capture_dir}/{width_name}/{key}-original.png"),"diffPercent":(pct * 100.0).round() / 100.0,"error":Value::Null,"note":note}),
                None => json!({"image":Value::Null,"original":Value::Null,"diffPercent":Value::Null,"error":note}),
            };
            match pages.iter_mut().find(|page| page["key"] == key.as_str()) {
                Some(page) => page[width_name] = view,
                None => pages.push(json!({"key":key,"title":key,"page":Value::Null,"route":if key == "front-page" { "/".to_string() } else { format!("/{key}/") },width_name:view})),
            }
        }
    }
    if let Some(key)=page_key {
        let refreshed=pages.iter().find(|p|p["key"]==key && p["desktop"]["image"].is_string() && p["mobile"]["image"].is_string()).cloned().ok_or("Selected page capture failed; previous comparison retained")?;
        for (width,_) in WIDTHS {
            for side in ["image","original"] {
                let rel=refreshed[width][side].as_str().ok_or("Selected page image missing")?;
                if !crate::h2g::under_work(work,&work.join(rel),false){return Err("Selected page image missing; previous comparison retained".into());}
            }
        }
        pages=previous["pages"].as_array().unwrap().iter().map(|p|if p["key"]==key {refreshed.clone()}else{p.clone()}).collect();
    }
    let doc = json!({"schema":SCHEMA,"capturedAt":now(),"preview":format!("http://127.0.0.1:{port}"),"pages":pages});
    write(store, p, INDEX, &doc)?;
    Ok(doc)
}
fn tail(ran: &Value) -> String {
    let out = ran["output"].as_str().unwrap_or_default().trim();
    out.chars().rev().take(300).collect::<Vec<_>>().into_iter().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn the_skill_s_report_is_read_page_by_page() {
        let report = "\nVisual diff at 1440px — live WordPress at http://127.0.0.1:8899\n----\n  ok front-page                  0.42% differing pixels  \n  ~  about                       3.10% differing pixels  height differs by 12px\n  !! journal                    18.00% differing pixels  \n  !! contact                     no converted page to compare\n----\n  worst page: 18.00%  (threshold 100.0%)\n  images in /work/compare/desktop\n";
        assert_eq!(parse(report), vec![
            ("front-page".to_string(), Some(0.42), String::new()),
            ("about".to_string(), Some(3.10), "height differs by 12px".to_string()),
            ("journal".to_string(), Some(18.0), String::new()),
            ("contact".to_string(), None, "no converted page to compare".to_string()),
        ]);
        assert!(parse("  worst page: 18.00%  (threshold 1.0%)\n  images in x").is_empty());
    }
    #[test]
    fn the_originals_are_the_ones_the_verify_step_configured() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path().join("h2g");
        std::fs::create_dir_all(work.join("verify")).unwrap();
        assert_eq!(originals(&work), None);
        std::fs::write(work.join("verify/render-original.json"), r#"{"old_theme":"/input","out":"preview-original"}"#).unwrap();
        assert_eq!(originals(&work), Some(("/work/verify/render-original.json".into(), "/work/verify/preview-original".into())));
        std::fs::write(work.join("verify/render-original.json"), r#"{"old_theme":"/input","out":"/work/originals"}"#).unwrap();
        assert_eq!(originals(&work), Some(("/work/verify/render-original.json".into(), "/work/originals".into())));
        std::fs::write(work.join("verify/render-original.json"), r#"{"old_theme":"/input","out":"/etc"}"#).unwrap();
        assert_eq!(originals(&work), None, "never outside /work");
    }
    /// The real sandbox (docker): the skill's wp-sandbox/setup.sh WordPress
    /// with a block theme, originals as the Verify step leaves them, and the
    /// skill's visual-diff.py at both widths, read into the Compare index
    /// (H2WP_TEST_RUNTIME_IMAGE).
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to the runtime image"]
    async fn the_original_and_the_gutenberg_theme_are_compared_page_by_page() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let base = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/html2wp-desktop-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new().prefix("app data ").tempdir_in(base).unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = crate::skill::tests::project();
        p.target = crate::h2g::TARGET.into();
        p.id = uuid::Uuid::new_v4().to_string();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let work = crate::h2g::work_dir(&store, &p.id).unwrap();
        let theme = work.join("output/studio-blocks");
        std::fs::create_dir_all(theme.join("templates")).unwrap();
        std::fs::write(theme.join("style.css"), "/*\nTheme Name: Studio Blocks\nVersion: 1.0.0\n*/\n").unwrap();
        std::fs::write(theme.join("theme.json"), r#"{"version":3}"#).unwrap();
        std::fs::write(theme.join("templates/index.html"), "<!-- wp:paragraph --><p>Hello from the block theme</p><!-- /wp:paragraph -->").unwrap();
        std::fs::create_dir_all(work.join("verify/preview-original")).unwrap();
        std::fs::write(work.join("verify/render-original.json"), r#"{"old_theme":"/input","out":"preview-original"}"#).unwrap();
        std::fs::write(work.join("verify/preview-original/front-page.html"), "<!doctype html><html><body><p>Hello from the original</p></body></html>").unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let name = crate::h2g::ensure_sandbox(&store, &p, &image).await.unwrap();
        let setup = crate::agent::exec(&name, "/work", &[format!("PORT={port}")], &format!("cp -a /opt/wp-offline/wordpress /work/output/studio-blocks-sandbox/ 2>/dev/null || mkdir -p /work/output/studio-blocks-sandbox && cp -a /opt/wp-offline/wordpress /work/output/studio-blocks-sandbox/; bash {}/scripts/wp-sandbox/setup.sh /work/output/studio-blocks /work/output/studio-blocks-sandbox", crate::h2g::SKILL), 600).await.unwrap();
        let compared = run(&store, &p, &image, None).await;
        let shown = status(&store, &p, false);
        let desktop = index(&store, &p)["pages"][0]["desktop"].clone();
        let pair = (image_of(&store, &p, &desktop["original"]), image_of(&store, &p, &desktop["image"]));
        crate::h2g_preview::stop(&p.id);
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), name], None, 60).await;
        assert_eq!(setup["exitCode"], 0, "{}", setup["output"]);
        let found = compared.unwrap();
        assert_eq!(found["pages"].as_array().unwrap().len(), 1, "{found}");
        let page = &found["pages"][0];
        assert_eq!((page["key"].as_str(), page["route"].as_str()), (Some("front-page"), Some("/")));
        for width in ["desktop", "mobile"] { assert!(page[width]["diffPercent"].is_number(), "{width}: {page}"); }
        assert_eq!(shown["state"], "done");
        assert!(pair.0 && pair.1, "the original and the Gutenberg page reach the owner's screen");
    }
    fn image_of(store: &Store, p: &Project, path: &Value) -> bool { path.as_str().is_some_and(|path| image(store, p, path).is_ok_and(|v| v.starts_with("data:image/png;base64,"))) }
}
