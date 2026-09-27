//! Gutenberg from an HTML theme: the html2wp-to-gutenberg skill turns an HTML
//! WordPress theme that html2wp produced into a native block theme. The host
//! checks the input before any AI runs: only a theme html2wp delivered goes in.
use crate::model::*;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The output type of these projects, and how the owner reads it.
pub const TARGET: &str = "h2g";
pub const KIND: &str = "html2wp HTML theme";

/// Where the theme sits in an import: the folder itself, or the one folder
/// a ZIP of it holds.
pub fn theme_root(root: &Path) -> PathBuf {
    if root.join("style.css").is_file() || root.join("clara-content").is_dir() || root.join("theme.json").is_file() { return root.into(); }
    let dirs: Vec<PathBuf> = std::fs::read_dir(root).into_iter().flatten().flatten()
        .map(|e| e.path()).filter(|p| p.is_dir() && !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with('.') || n == "__MACOSX")).collect();
    match dirs.as_slice() { [only] => only.clone(), _ => root.into() }
}

/// The input is an HTML WordPress theme html2wp produced: its content bundle
/// (clara-content/ with sources/, posts.json, terms.json, redirects.json),
/// the theme's parts, templates, theme.json, inc/ and assets/, and html2wp's
/// own marks (the bundle's clara-content/1 manifest and the html2wp runtime).
/// Anything else is refused, naming what is missing, before any AI runs.
pub fn check_input(root: &Path) -> Result<PathBuf> {
    let theme = theme_root(root);
    if theme.join("content").is_dir() && !theme.join("clara-content").exists() && theme.join("theme.json").is_file() {
        return Err("This is a Gutenberg block theme, not an HTML WordPress theme: Gutenberg from an HTML theme takes the HTML theme html2wp made (its ZIP from Exports), which has a clara-content folder.".into());
    }
    let mut missing: Vec<&str> = [
        ("clara-content/sources/", theme.join("clara-content/sources").is_dir()),
        ("clara-content/posts.json", theme.join("clara-content/posts.json").is_file()),
        ("clara-content/terms.json", theme.join("clara-content/terms.json").is_file()),
        ("clara-content/redirects.json", theme.join("clara-content/redirects.json").is_file()),
        ("parts/", theme.join("parts").is_dir()),
        ("templates/", theme.join("templates").is_dir()),
        ("theme.json", theme.join("theme.json").is_file()),
        ("style.css", theme.join("style.css").is_file()),
        ("inc/", theme.join("inc").is_dir()),
        ("assets/", theme.join("assets").is_dir()),
    ].into_iter().filter(|(_, present)| !present).map(|(name, _)| name).collect();
    let manifest: Value = std::fs::read(theme.join("clara-content/manifest.json")).ok()
        .and_then(|raw| serde_json::from_slice(&raw).ok()).unwrap_or(Value::Null);
    if manifest["format"] != "clara-content/1" { missing.push("html2wp's content manifest (clara-content/manifest.json, format clara-content/1)"); }
    if !theme.join("assets/html2wp-runtime").is_dir() && !theme.join("inc/runtime.php").is_file() { missing.push("the html2wp runtime (assets/html2wp-runtime/, inc/runtime.php)"); }
    if missing.is_empty() { return Ok(theme); }
    Err(format!("This is not an HTML WordPress theme made by html2wp, so it cannot become a Gutenberg theme here. Missing: {}. Choose the theme folder or the theme ZIP html2wp delivered (Exports).", missing.join(", ")))
}

/// The html2wp-to-gutenberg workflow's steps (its docs/workflow.md), as
/// the agent reports them and the Overview shows them.
pub const STEPS: [&str; 9] = ["Audit", "Scaffold and theme.json", "Fonts", "CSS", "JS", "Parts, templates and patterns", "Content", "Importer and setup", "Verify"];

fn progress_key(pid: &str) -> String { format!("h2g-progress:{pid}") }
/// The run's progress: {current, started, steps: {"n": {at, note}}}.
pub fn progress(store: &crate::store::Store, pid: &str) -> serde_json::Value {
    store.setting(&progress_key(pid)).and_then(|v| serde_json::from_str(&v).ok()).filter(serde_json::Value::is_object).unwrap_or(serde_json::json!({}))
}
/// report_progress: the agent starts a workflow step. Steps only move
/// forward or repeat; the first report starts the clock.
pub fn report(store: &crate::store::Store, p: &Project, args: &serde_json::Value) -> Result<serde_json::Value> {
    if !p.from_theme() { return Err("report_progress is for Gutenberg from an HTML theme only.".into()); }
    let step = args["step"].as_u64().filter(|n| (1..=9).contains(n)).ok_or("report_progress needs step 1 to 9 (the html2wp-to-gutenberg workflow step you are starting).")?;
    let note: String = args["note"].as_str().unwrap_or("").trim().chars().take(300).collect();
    let mut record = progress(store, &p.id);
    if record["current"].as_u64().is_some_and(|n| step < n) {
        return Err(format!("Step {step} is behind the reported step {}; report the step you are working on now.", record["current"]));
    }
    let at = now();
    if !record["started"].is_string() { record["started"] = serde_json::json!(at); }
    record["current"] = serde_json::json!(step);
    record["steps"][step.to_string()] = serde_json::json!({"at":at,"note":note});
    store.set(&progress_key(&p.id), &record.to_string())?;
    Ok(serde_json::json!({"ok":true,"step":step,"name":STEPS[step as usize - 1]}))
}

/// Where the skill sits in the runtime image (read-only in the sandbox).
pub const SKILL: &str = "/opt/desktop/skills/html2wp-to-gutenberg";
/// What the agent is told for these projects instead of html2wp's own brief.
pub const INSTRUCTIONS: &str = "You convert an HTML WordPress theme that html2wp made into a native Gutenberg block theme with the html2wp-to-gutenberg skill. The skill is at /opt/desktop/skills/html2wp-to-gutenberg: read its SKILL.md, docs/workflow.md and references/ first and follow them exactly; the owner does not want its method changed. Your only way to read, run and WRITE files is sandbox_exec: it runs a bash command in the project's own sandbox. Your own shell and file-edit tools are off and /work is not in your workspace, so create and edit every file through sandbox_exec (heredocs such as cat > file <<'EOF', python3 scripts, cp, sed); read with cat, sed -n or rg. The input theme is /input (read-only), your work goes under /work (the new sibling theme in /work/output/<new-slug>/, its WordPress sandbox beside it under /work), scratch in /tmp. The skill's scripts run there unchanged (python3, php, wp, node, rsync, curl, unzip) and the network is available (Google Fonts, WordPress). WordPress 7.0.2 with the SQLite drop-in is also at /opt/wp-offline/wordpress: copying it into the sandbox directory before scripts/wp-sandbox/setup.sh saves the download. Background servers: start them with setsid nohup … &. Visual Edit Lite is not available: note the two form and SEO criteria that need it as not checked. report_progress: report each workflow step when you start it (1 Audit, 2 Scaffold and theme.json, 3 Fonts, 4 CSS, 5 JS, 6 Parts, templates and patterns, 7 Content, 8 Importer and setup, 9 Verify). When the skill's verification is done, write /work/VERIFICATION.md (the skill's verification summary) and /work/verification-summary.json {\"pages\":N,\"maxDiffPercent\":X,\"invalidBlocks\":N,\"notes\":\"one line\"}; the host then packages /work/output/<new-slug>/ and tells the owner. Never ask the owner to run commands. Stop and say why, per the skill's Escalation, when it tells you to.";

/// Start conversion, and Continue after a stop: the owner's message that starts the run.
pub const START: &str = "Convert this HTML WordPress theme into a native Gutenberg block theme with the html2wp-to-gutenberg skill. Start by reading the skill (SKILL.md, docs/workflow.md, references/pitfalls.md, references/forms-and-seo.md) in the sandbox, then follow its workflow and report each step.";
pub const CONTINUE: &str = "Continue the conversion from where it stopped: read /work (what you already built and noted), report the workflow step you resume at, and go on to the end of the skill's workflow.";

/// The goal of the automatic run.
pub fn goal(p: &Project) -> String {
    format!("Convert the HTML WordPress theme of the html2wp Desktop project \"{}\" into a native Gutenberg block theme with the html2wp-to-gutenberg skill, following its workflow from step 1 to step 9 and reporting each step (report_progress). Done means: the new theme is in /work/output/<new-slug>/, the skill's verification ran, and /work/VERIFICATION.md and /work/verification-summary.json are written; the host then packages the ZIP. Mark the goal blocked only for a stop the skill's Escalation names, and say it in one sentence.", p.name)
}


/// The project's work folder, the sandbox's /work.
pub fn work_dir(store: &crate::store::Store, pid: &str) -> Result<PathBuf> { Ok(store.path(pid)?.join("workspace/h2g")) }
fn sandbox_name(pid: &str) -> Result<String> { Ok(format!("{}-h2g", crate::runtime::project_name(pid)?)) }

/// The sandbox the agent's commands run in: the runtime image with the
/// network (the skill downloads its fonts and what it needs), no credentials
/// (no Codex account, no licence, no service), its root read-only; the
/// owner's theme read-only at /input, the work at /work (the only writable
/// host folder), scratch in /tmp and the home.
pub fn sandbox_args(name: &str, image: &str, work: &Path, input: &Path) -> Vec<String> {
    [
        "create", "--name", name, "--label", "dev.html2wp.desktop=true", "--init",
        "--cap-drop=ALL", "--security-opt", "no-new-privileges", "--pids-limit", "512",
        "--memory", "6g", "--shm-size", "1g", "--read-only",
        "--tmpfs", "/tmp:rw,exec,nosuid,size=4g", "--tmpfs", "/home/agent:rw,nosuid,size=512m,uid=1000,gid=1000,mode=700",
        "--user", "1000:1000", "--env", "HOME=/home/agent", "--workdir", "/work",
    ].into_iter().map(String::from)
        .chain(["--mount".into(), format!("type=bind,source={},target=/work", work.display()),
            "--mount".into(), format!("type=bind,source={},target=/input,readonly", input.display()),
            image.into(), "sleep".into(), "infinity".into()])
        .collect()
}
/// The sandbox, made from `image` (the installed runtime); one made from an
/// older runtime is made again, the work stays in /work.
pub async fn ensure_sandbox(store: &crate::store::Store, p: &Project, image: &str) -> Result<String> {
    let name = sandbox_name(&p.id)?;
    let work = work_dir(store, &p.id)?;
    std::fs::create_dir_all(work.join("output")).map_err(err)?;
    let input = theme_root(&store.path(&p.id)?.join("input"));
    use crate::runtime::docker;
    let inspect = [String::from("inspect"), name.clone()];
    if docker(&inspect, None, 10).await.is_ok() {
        crate::runtime::assert_owned(&name).await?;
        if crate::runtime::is_outdated(&name, image, None).await { docker(&["rm".into(), "-f".into(), name.clone()], None, 30).await?; }
    }
    if docker(&inspect, None, 10).await.is_err() {
        docker(&sandbox_args(&name, image, &work, &input), None, 60).await?;
    }
    docker(&["start".into(), name.clone()], None, 30).await?;
    Ok(name)
}

/// The folder a command runs in: under /work, or reading /input or the skill.
pub fn exec_cwd(cwd: Option<&str>) -> Result<String> {
    let cwd = cwd.unwrap_or("/work").trim_end_matches('/');
    let cwd = if cwd.is_empty() { "/" } else { cwd };
    let inside = |root: &str| cwd == root || cwd.starts_with(&format!("{root}/"));
    if cwd.split('/').any(|part| part == "..") || !(inside("/work") || inside("/input") || inside(SKILL) || inside("/tmp")) {
        return Err(format!("sandbox_exec runs under /work (or reads /input, /tmp and {SKILL}); {cwd} is outside"));
    }
    Ok(cwd.into())
}
/// sandbox_exec: one bash command in the project's sandbox, its exit code
/// and the tail of its output.
pub async fn exec(store: &crate::store::Store, p: &Project, image: &str, args: &Value) -> Result<Value> {
    if !p.from_theme() { return Err("sandbox_exec is for Gutenberg from an HTML theme only.".into()); }
    let (cmd, _, seconds) = crate::agent::exec_args(args, "/work", &["/work", "/input", SKILL, "/tmp"])?;
    let cwd = exec_cwd(args["cwd"].as_str())?;
    let name = ensure_sandbox(store, p, image).await?;
    let result = crate::agent::exec(&name, &cwd, &[], cmd, seconds).await?;
    let _ = crate::run_context::tool_log(store, p, cmd, &result);
    Ok(result)
}

/// A real folder or file under /work, never reached through a link (the
/// sandbox writes /work; a link there could point the host anywhere).
pub(crate) fn under_work(work: &Path, path: &Path, dir: bool) -> bool {
    let kind = std::fs::symlink_metadata(path).map(|m| m.file_type());
    let real = kind.is_ok_and(|k| !k.is_symlink() && if dir { k.is_dir() } else { k.is_file() });
    real && matches!((std::fs::canonicalize(work), std::fs::canonicalize(path)), (Ok(root), Ok(found)) if found.starts_with(&root))
}
/// Translate a native file below the work folder to the Linux sandbox path.
/// `Path::join("/work")` cannot be used here: Windows treats that as a path on
/// the current drive and inserts backslashes into the container command.
pub(crate) fn sandbox_path(work: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(work).ok()?;
    let parts = rel.components().map(|part| match part {
        std::path::Component::Normal(name) => name.to_str().filter(|s|
            !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))),
        _ => None,
    }).collect::<Option<Vec<_>>>()?;
    (!parts.is_empty()).then(|| format!("/work/{}", parts.join("/")))
}
/// A file of /work as it is, never through a link.
fn read_work(work: &Path, name: &str) -> Option<Vec<u8>> {
    let path = work.join(name);
    if !under_work(work, &path, false) { return None; }
    std::fs::read(path).ok()
}
/// The new sibling theme under /work/output: the one real folder with
/// style.css and theme.json (a link is never followed out of /work).
pub fn output_theme(work: &Path) -> Result<PathBuf> {
    let output = work.join("output");
    if !under_work(work, &output, true) { return Err("No new theme in /work/output (a real folder, not a link).".into()); }
    let themes: Vec<PathBuf> = std::fs::read_dir(&output).into_iter().flatten().flatten().map(|e| e.path())
        .filter(|d| under_work(work, d, true) && under_work(work, &d.join("style.css"), false) && under_work(work, &d.join("theme.json"), false)).collect();
    match themes.as_slice() {
        [only] => Ok(only.clone()),
        [] => Err("No new theme in /work/output (a folder with style.css and theme.json).".into()),
        _ => Err("More than one theme in /work/output; keep only the new one there.".into()),
    }
}
/// The skill's verification summary the agent wrote at the end.
pub fn summary(work: &Path) -> Option<Value> {
    serde_json::from_slice::<Value>(&read_work(work, "verification-summary.json")?).ok().filter(Value::is_object)
}
/// The result in one message: the ZIP, the verification in numbers, where to find it.
pub fn result_message(zip: &str, summary: &Value) -> String {
    let n = |k: &str| summary[k].as_f64();
    let mut parts = vec![];
    if let Some(pages) = n("pages") { parts.push(format!("{} pages checked", pages as u64)); }
    if let Some(diff) = n("maxDiffPercent") { parts.push(format!("largest pixel difference {diff:.2}%")); }
    if let Some(invalid) = n("invalidBlocks") { parts.push(format!("{} invalid block{}", invalid as u64, if invalid as u64 == 1 { "" } else { "s" })); }
    let notes = summary["notes"].as_str().map(|s| format!(" {}", s.trim())).unwrap_or_default();
    format!("Your Gutenberg block theme is ready: {zip}. Verification: {}.{notes} The ZIP and the verification summary are in Exports.",
        if parts.is_empty() { "see the summary".to_string() } else { parts.join(", ") })
}
/// How the run stopped, for the chat and the notification: the step it got to.
pub fn stop_message(progress: &Value, reason: Option<&str>) -> String {
    let step = progress["current"].as_u64().filter(|n| (1..=9).contains(n));
    let at = step.map(|n| format!(" at step {n} of 9 ({})", STEPS[n as usize - 1])).unwrap_or_default();
    format!("The conversion to a Gutenberg block theme stopped{at}.{} Choose Continue to resume from there; what is done so far is kept.",
        reason.filter(|r| !r.trim().is_empty()).map(|r| format!(" {}", r.trim())).unwrap_or_default())
}
/// The chat line for a reported step, with the time the run has taken.
pub fn step_line(record: &Value, step: u64, note: &str) -> String {
    let minutes = record["started"].as_str().and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
        .map_or(0, |at| (chrono::Utc::now() - at.with_timezone(&chrono::Utc)).num_minutes().max(0));
    let took = if minutes >= 60 { format!("{} h {} min", minutes / 60, minutes % 60) } else { format!("{minutes} min") };
    let name = STEPS.get(step as usize - 1).copied().unwrap_or("");
    format!("Step {step} of 9: {name}{} ({took} so far)", if note.is_empty() { String::new() } else { format!(" — {note}") })
}

/// The ZIP of the new theme, its folder at the top as WordPress installs it.
fn zip_theme(theme: &Path, dest: &Path, top: &str) -> Result<()> {
    let partial = dest.with_extension("part");
    let result = (|| {
        let mut zip = zip::ZipWriter::new(std::fs::File::create(&partial).map_err(err)?);
        let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated).large_file(true);
        // No link is followed, the theme folder's own included: only what is really in it.
        for entry in walkdir::WalkDir::new(theme).follow_links(false).follow_root_links(false).sort_by_file_name().into_iter()
            .filter_entry(|e| e.depth() == 0 || !matches!(e.file_name().to_string_lossy().as_ref(), ".git" | "node_modules" | ".DS_Store" | "__MACOSX")) {
            let entry = entry.map_err(err)?;
            if entry.depth() == 0 || entry.path_is_symlink() { continue; }
            let name = entry.path().strip_prefix(theme).map_err(err)?.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
            if entry.file_type().is_dir() { zip.add_directory(format!("{top}/{name}/"), options).map_err(err)?; }
            else if entry.file_type().is_file() {
                zip.start_file(format!("{top}/{name}"), options).map_err(err)?;
                std::io::copy(&mut std::fs::File::open(entry.path()).map_err(err)?, &mut zip).map_err(err)?;
            }
        }
        zip.finish().map_err(err)?;
        std::fs::rename(&partial, dest).map_err(err)
    })();
    if result.is_err() { let _ = std::fs::remove_file(&partial); }
    result
}
/// The end of a run: the new theme and the skill's verification summary are
/// there, so the host packages the ZIP, puts both in Exports and tells the
/// owner (the chat, and a notification). Not there yet: the run goes on.
pub fn deliver(store: &crate::store::Store, p: &mut Project, emit: &(impl Fn(&str, Value) + Send + Sync)) -> Result<bool> {
    let work = work_dir(store, &p.id)?;
    let (Ok(theme), Some(summary)) = (output_theme(&work), summary(&work)) else { return Ok(false) };
    let slug = theme.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "theme".into());
    let revision = store.path(&p.id)?.join("artifacts").join(format!("revision-{}", p.revision));
    std::fs::create_dir_all(&revision).map_err(err)?;
    let zip = format!("{slug}.zip");
    zip_theme(&theme, &revision.join(&zip), &slug)?;
    let notes = format!("{slug}-verification.md");
    let text = read_work(&work, "VERIFICATION.md").map(|raw| String::from_utf8_lossy(&raw).into_owned())
        .unwrap_or_else(|| format!("# Verification\n\n{}\n", serde_json::to_string_pretty(&summary).unwrap_or_default()));
    std::fs::write(revision.join(&notes), text + &crate::run_context::markdown(store, p)).map_err(err)?;
    p.artifacts.retain(|a| a.revision != p.revision);
    for (kind, filename) in [("theme", zip.clone()), ("summary", notes)] {
        p.artifacts.push(Artifact { id: id(), revision: p.revision, sha256: crate::files::hash(&revision.join(&filename))?, filename, created_at: now(),
            kind: kind.into(), reviewed: true, checks: "h2g".into() });
    }
    p.phase = "deliverable_ready".into();
    p.last_error = None;
    p.updated_at = now();
    store.put(p)?;
    emit("project-updated", value(&*p));
    let text = result_message(&zip, &summary);
    if let Ok(m) = store.message_with_action(&p.id, "assistant", &text, Some("exports")) { emit("chat-message", value(m)); }
    crate::notifications::queue(emit, "Your Gutenberg block theme is ready", &format!("{}: {zip} is in Exports.", p.name));
    Ok(true)
}

/// After delivery the owner's chat message is a change to the delivered
/// Gutenberg theme in its sandbox WordPress, never a new conversion.
pub fn takes_changes(p: &Project) -> bool { p.from_theme() && p.phase == "deliverable_ready" }
/// The change goal: the theme in /work/output, pushed into the sandbox with
/// the skill's own sync.sh and checked there; the owner's words its request.
pub fn change_text(store: &crate::store::Store, p: &Project, request: &str) -> String {
    let slug = work_dir(store, &p.id).ok().and_then(|w| output_theme(&w).ok()).and_then(|t| t.file_name().map(|n| n.to_string_lossy().into_owned())).unwrap_or_else(|| "<slug>".into());
    format!("The Gutenberg block theme of this project was delivered. Make the owner's change in it: edit only /work/output/{slug}/, push it into the sandbox WordPress with the skill's scripts/wp-sandbox/sync.sh (and the theme's importer when the change is in its imported content), look at the changed pages there (the skill's visual-diff.py and editor-validity.py), and answer. What the theme files cannot change, say plainly. Never start the conversion over or redo the workflow's steps, and never package the theme: packaging is the owner's, in the app. The owner's request: {}", request.trim())
}
/// How the agent works in this sandbox; after delivery, changes only.
pub fn instructions(p: &Project) -> String {
    if !takes_changes(p) { return INSTRUCTIONS.into(); }
    format!("{INSTRUCTIONS} This theme was delivered: every owner message is a change in /work/output/<slug>/, pushed into the sandbox WordPress with scripts/wp-sandbox/sync.sh and checked there. Never start the conversion over or package the theme; packaging is the owner's, in the app.")
}
/// {path in the theme: sha256} of what zip_theme packs: real files, no links, no build residue.
fn tree(theme: &Path) -> std::collections::BTreeMap<String, String> {
    walkdir::WalkDir::new(theme).follow_links(false).follow_root_links(false).into_iter()
        .filter_entry(|e| e.depth() == 0 || !matches!(e.file_name().to_string_lossy().as_ref(), ".git" | "node_modules" | ".DS_Store" | "__MACOSX"))
        .flatten().filter(|e| e.depth() > 0 && e.file_type().is_file() && !e.path_is_symlink())
        .filter_map(|e| Some((e.path().strip_prefix(theme).ok()?.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/"), crate::files::hash(e.path()).ok()?)))
        .collect()
}
/// The same for a theme ZIP, its top folder left out.
fn zip_tree(zip: &Path) -> Option<std::collections::BTreeMap<String, String>> {
    use sha2::Digest;
    let mut archive = zip::ZipArchive::new(std::fs::File::open(zip).ok()?).ok()?;
    let mut out = std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).ok()?;
        if entry.is_dir() { continue; }
        let Some((_, name)) = entry.name().split_once('/') else { continue };
        let name = name.to_string();
        let mut hasher = sha2::Sha256::new();
        std::io::copy(&mut entry, &mut hasher).ok()?;
        out.insert(name, format!("{:x}", hasher.finalize()));
    }
    Some(out)
}
/// The delivered theme ZIP of the current revision, on disk.
fn current_zip(store: &crate::store::Store, p: &Project) -> Option<(PathBuf, String)> {
    let theme = p.artifacts.iter().find(|a| a.revision == p.revision && a.kind == "theme")?;
    let path = store.path(&p.id).ok()?.join("artifacts").join(format!("revision-{}", p.revision)).join(&theme.filename);
    path.is_file().then(|| (path, theme.filename.clone()))
}
/// Whether the theme in /work/output differs from the last release's ZIP
/// (there is no change log here: the files themselves say it).
pub fn changes(store: &crate::store::Store, p: &Project) -> Value {
    if !takes_changes(p) { return Value::Null; }
    let (Ok(work), Some((zip, _))) = (work_dir(store, &p.id), current_zip(store, p)) else { return Value::Null };
    let Ok(theme) = output_theme(&work) else { return Value::Null };
    let changed = zip_tree(&zip).is_none_or(|packed| packed != tree(&theme));
    serde_json::json!({"count":Value::Null,"sinceZip":Value::Null,"changedSinceZip":changed})
}
/// Make release: the theme as it is now in /work/output, the next revision
/// (with the delivered verification beside it), when it differs from the
/// last release; otherwise that release.
pub fn release(store: &crate::store::Store, p: &mut Project, emit: &(impl Fn(&str, Value) + Send + Sync)) -> Result<Value> {
    if !takes_changes(p) { return Err("Make release packages a delivered Gutenberg theme; this project has none yet.".into()); }
    let work = work_dir(store, &p.id)?;
    let theme = output_theme(&work)?;
    let (last, filename) = current_zip(store, p).ok_or("The delivered theme ZIP is missing from Exports.")?;
    if zip_tree(&last).is_some_and(|packed| packed == tree(&theme)) {
        return Ok(serde_json::json!({"changed":false,"revision":p.revision,"filename":filename}));
    }
    let slug = theme.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "theme".into());
    let summary = p.artifacts.iter().find(|a| a.revision == p.revision && a.kind == "summary").cloned();
    let previous = store.path(&p.id)?.join("artifacts").join(format!("revision-{}", p.revision));
    p.revision += 1;
    let revision = store.path(&p.id)?.join("artifacts").join(format!("revision-{}", p.revision));
    std::fs::create_dir_all(&revision).map_err(err)?;
    let zip = format!("{slug}-r{}.zip", p.revision);
    zip_theme(&theme, &revision.join(&zip), &slug)?;
    p.artifacts.retain(|a| a.revision != p.revision);
    p.artifacts.push(Artifact { id: id(), revision: p.revision, sha256: crate::files::hash(&revision.join(&zip))?, filename: zip.clone(), created_at: now(), kind: "theme".into(), reviewed: true, checks: "packaged".into() });
    // The verification is the delivered revision's: it goes beside the release as it is.
    if let Some(summary) = summary {
        if std::fs::copy(previous.join(&summary.filename), revision.join(&summary.filename)).is_ok() {
            p.artifacts.push(Artifact { id: id(), revision: p.revision, created_at: now(), ..summary });
        }
    }
    p.updated_at = now();
    store.put(p)?;
    emit("project-updated", value(&*p));
    let text = format!("Release ready: {zip}, revision {}. Packaged after changes, not checked again.", p.revision);
    if let Ok(m) = store.message_with_action(&p.id, "assistant", &text, Some("exports")) { emit("chat-message", value(m)); }
    Ok(serde_json::json!({"changed":true,"revision":p.revision,"filename":zip}))
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn after_delivery_a_message_is_a_change_and_make_release_packs_the_theme_as_it_is_now() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = crate::skill::tests::project();
        p.target = TARGET.into();
        store.put(&p).unwrap();
        let work = work_dir(&store, &p.id).unwrap();
        let theme = work.join("output/studio-blocks");
        fs::create_dir_all(theme.join("templates")).unwrap();
        fs::write(theme.join("style.css"), "/*\nTheme Name: Studio Blocks\n*/").unwrap();
        fs::write(theme.join("theme.json"), "{}").unwrap();
        fs::write(theme.join("templates/index.html"), "<!-- wp:paragraph --><p>Hello</p><!-- /wp:paragraph -->").unwrap();
        fs::write(work.join("verification-summary.json"), r#"{"pages":1,"maxDiffPercent":0.4,"invalidBlocks":0,"notes":"ok"}"#).unwrap();
        assert!(!takes_changes(&p) && changes(&store, &p).is_null(), "a conversion under way: no change yet");
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        let mut p = store.project(&p.id).unwrap();
        assert!(takes_changes(&p));
        assert!(instructions(&p).contains("This theme was delivered") && instructions(&p).contains("Never start the conversion over or package the theme"));
        let text = change_text(&store, &p, " make the heading italic ");
        assert!(text.contains("edit only /work/output/studio-blocks/") && text.contains("scripts/wp-sandbox/sync.sh") && text.ends_with("The owner's request: make the heading italic"), "{text}");
        assert!(text.contains("Never start the conversion over") && text.contains("never package the theme"));
        assert_eq!(changes(&store, &p)["changedSinceZip"], false, "as delivered");
        // Nothing changed: the release is the delivered ZIP.
        assert_eq!(release(&store, &mut p, &|_, _| {}).unwrap(), serde_json::json!({"changed":false,"revision":1,"filename":"studio-blocks.zip"}));
        // A change in the theme: the next revision, packaged as it is now.
        fs::write(theme.join("templates/index.html"), "<!-- wp:paragraph --><p><em>Hello</em></p><!-- /wp:paragraph -->").unwrap();
        assert_eq!(changes(&store, &p)["changedSinceZip"], true);
        assert_eq!(release(&store, &mut p, &|_, _| {}).unwrap(), serde_json::json!({"changed":true,"revision":2,"filename":"studio-blocks-r2.zip"}));
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.artifacts.iter().filter(|a| a.revision == 2).map(|a| (a.kind.as_str(), a.filename.as_str(), a.checks.as_str())).collect::<Vec<_>>(),
            [("theme", "studio-blocks-r2.zip", "packaged"), ("summary", "studio-blocks-verification.md", "h2g")]);
        let packed = zip_tree(&store.path(&p.id).unwrap().join("artifacts/revision-2/studio-blocks-r2.zip")).unwrap();
        assert_eq!(packed.get("templates/index.html"), Some(&crate::files::hash(&theme.join("templates/index.html")).unwrap()));
        assert_eq!(changes(&store, &p)["changedSinceZip"], false, "released");
        assert_eq!(store.messages(&p.id).unwrap().pop().unwrap().text, "Release ready: studio-blocks-r2.zip, revision 2. Packaged after changes, not checked again.");
    }
    /// A theme as html2wp delivers an HTML theme (the shape of its ZIP).
    pub(crate) fn html2wp_theme(root: &Path) {
        for dir in ["clara-content/sources", "parts", "templates", "inc", "assets/html2wp-runtime"] { fs::create_dir_all(root.join(dir)).unwrap(); }
        for file in ["clara-content/posts.json", "clara-content/terms.json", "clara-content/redirects.json", "theme.json"] { fs::write(root.join(file), "{}").unwrap(); }
        fs::write(root.join("clara-content/manifest.json"), r#"{"format":"clara-content/1","generator":"dist-to-bundle/2"}"#).unwrap();
        fs::write(root.join("style.css"), "/*\nTheme Name: Site\n*/").unwrap();
        fs::write(root.join("inc/runtime.php"), "<?php").unwrap();
    }
    #[test]
    fn only_an_html_theme_html2wp_made_goes_in() {
        let dir = tempfile::tempdir().unwrap();
        // Ours, as a folder and as the one folder of an unpacked ZIP.
        html2wp_theme(dir.path());
        assert_eq!(check_input(dir.path()).unwrap(), dir.path());
        let zipped = tempfile::tempdir().unwrap();
        html2wp_theme(&zipped.path().join("site"));
        fs::create_dir_all(zipped.path().join("__MACOSX")).unwrap();
        assert_eq!(check_input(zipped.path()).unwrap(), zipped.path().join("site"));
        // A random folder: every missing piece named.
        let random = tempfile::tempdir().unwrap();
        fs::write(random.path().join("index.html"), "<p>hi</p>").unwrap();
        let refused = check_input(random.path()).unwrap_err();
        assert!(refused.starts_with("This is not an HTML WordPress theme made by html2wp") && refused.contains("clara-content/posts.json") && refused.contains("html2wp runtime"), "{refused}");
        // A block theme from elsewhere (theme.json, templates, parts) is not ours.
        let foreign = tempfile::tempdir().unwrap();
        for d in ["parts", "templates", "assets", "inc"] { fs::create_dir_all(foreign.path().join(d)).unwrap(); }
        fs::write(foreign.path().join("theme.json"), "{}").unwrap();
        fs::write(foreign.path().join("style.css"), "/* Theme Name: Other */").unwrap();
        let refused = check_input(foreign.path()).unwrap_err();
        assert!(refused.contains("clara-content/sources/") && refused.contains("content manifest"), "{refused}");
        // Our own Gutenberg output is named as such.
        let gutenberg = tempfile::tempdir().unwrap();
        fs::create_dir_all(gutenberg.path().join("content")).unwrap();
        fs::write(gutenberg.path().join("theme.json"), "{}").unwrap();
        assert!(check_input(gutenberg.path()).unwrap_err().starts_with("This is a Gutenberg block theme"));
        // A forged bundle without html2wp's marks is refused too.
        let forged = tempfile::tempdir().unwrap();
        html2wp_theme(forged.path());
        fs::write(forged.path().join("clara-content/manifest.json"), "{}").unwrap();
        assert!(check_input(forged.path()).unwrap_err().contains("clara-content/1"));
    }
    #[test]
    fn the_agent_reports_the_workflow_step_it_starts() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = crate::skill::tests::project();
        assert!(report(&store, &p, &serde_json::json!({"step":1})).unwrap_err().contains("only"));
        p.target = TARGET.into();
        assert!(report(&store, &p, &serde_json::json!({"step":10})).is_err());
        assert_eq!(report(&store, &p, &serde_json::json!({"step":1,"note":"inventory of 12 pages"})).unwrap()["name"], "Audit");
        let started = progress(&store, &p.id)["started"].clone();
        report(&store, &p, &serde_json::json!({"step":4})).unwrap();
        report(&store, &p, &serde_json::json!({"step":4,"note":"again"})).unwrap();
        assert!(report(&store, &p, &serde_json::json!({"step":2})).unwrap_err().contains("behind"));
        let record = progress(&store, &p.id);
        assert_eq!((record["current"].clone(), record["started"].clone(), record["steps"]["1"]["note"].clone()), (serde_json::json!(4), started, serde_json::json!("inventory of 12 pages")));
        assert!(step_line(&record, 4, "site.css bridged").starts_with("Step 4 of 9: CSS — site.css bridged (0 min so far)"));
    }
    #[test]
    fn the_sandbox_holds_no_credentials_and_writes_only_work() {
        let args = sandbox_args("h2wpd-x-h2g", "img", Path::new("/p/workspace/h2g"), Path::new("/p/input/site"));
        let joined = args.join(" ");
        for flag in ["--read-only", "--cap-drop=ALL", "no-new-privileges", "--user 1000:1000"] { assert!(joined.contains(flag), "{flag}"); }
        let mounts: Vec<&String> = args.iter().enumerate().filter(|(i, _)| *i > 0 && args[i - 1] == "--mount").map(|(_, m)| m).collect();
        assert_eq!(mounts, ["type=bind,source=/p/workspace/h2g,target=/work", "type=bind,source=/p/input/site,target=/input,readonly"], "only /work is a writable host folder; no Codex account, licence or service");
        assert!(!joined.contains(".codex") && !joined.contains("auth") && !joined.contains("licence") && !joined.contains("volume"));
        for cwd in ["/work", "/work/output/site-blocks", "/input", "/tmp", SKILL] { assert!(exec_cwd(Some(cwd)).is_ok(), "{cwd}"); }
        for cwd in ["/", "/home/agent/.codex", "/work/../etc", "/opt", "/workspace"] { assert!(exec_cwd(Some(cwd)).is_err(), "{cwd}"); }
        assert_eq!(exec_cwd(None).unwrap(), "/work");
    }
    #[test]
    fn the_run_ends_with_the_zip_and_the_verification_summary() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = crate::skill::tests::project();
        p.target = TARGET.into();
        p.artifacts.clear();
        store.put(&p).unwrap();
        let work = work_dir(&store, &p.id).unwrap();
        assert!(!deliver(&store, &mut p, &|_, _| {}).unwrap(), "nothing yet: the run goes on");
        std::fs::create_dir_all(work.join("output/site-blocks/templates")).unwrap();
        for f in ["style.css", "theme.json", "templates/index.html"] { std::fs::write(work.join("output/site-blocks").join(f), "x").unwrap(); }
        assert!(!deliver(&store, &mut p, &|_, _| {}).unwrap(), "the theme without its verification summary is not done");
        std::fs::write(work.join("verification-summary.json"), r#"{"pages":15,"maxDiffPercent":0.4,"invalidBlocks":0,"notes":"Forms not checked (no Visual Edit Lite)."}"#).unwrap();
        std::fs::write(work.join("VERIFICATION.md"), "# Verification\n15 pages").unwrap();
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.phase, "deliverable_ready");
        assert_eq!(p.artifacts.iter().map(|a| (a.kind.as_str(), a.filename.as_str())).collect::<Vec<_>>(), [("theme", "site-blocks.zip"), ("summary", "site-blocks-verification.md")]);
        let zip = zip::ZipArchive::new(std::fs::File::open(store.path(&p.id).unwrap().join("artifacts/revision-1/site-blocks.zip")).unwrap()).unwrap();
        assert!(zip.file_names().any(|n| n == "site-blocks/templates/index.html"));
        let said = store.messages(&p.id).unwrap().pop().unwrap();
        assert_eq!(said.text, "Your Gutenberg block theme is ready: site-blocks.zip. Verification: 15 pages checked, largest pixel difference 0.40%, 0 invalid blocks. Forms not checked (no Visual Edit Lite). The ZIP and the verification summary are in Exports.");
        assert_eq!(said.action.as_deref(), Some("exports"));
        #[cfg(unix)]
        {
            // Nothing is read through a link out of /work: a linked theme
            // folder, a link inside the theme, a linked summary or VERIFICATION.md.
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret.txt"), "private").unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret.txt"), work.join("output/site-blocks/leak.txt")).unwrap();
            let mut again = store.project(&p.id).unwrap();
            again.phase = "preparing".into();
            assert!(deliver(&store, &mut again, &|_, _| {}).unwrap());
            let zip = zip::ZipArchive::new(std::fs::File::open(store.path(&p.id).unwrap().join("artifacts/revision-1/site-blocks.zip")).unwrap()).unwrap();
            assert!(!zip.file_names().any(|n| n.contains("leak")), "a link inside the theme is not packed");
            std::fs::rename(work.join("VERIFICATION.md"), outside.path().join("VERIFICATION.md")).unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret.txt"), work.join("VERIFICATION.md")).unwrap();
            assert!(deliver(&store, &mut again, &|_, _| {}).unwrap());
            assert!(!std::fs::read_to_string(store.path(&p.id).unwrap().join("artifacts/revision-1/site-blocks-verification.md")).unwrap().contains("private"));
            let theme = outside.path().join("theme");
            std::fs::create_dir_all(&theme).unwrap();
            for f in ["style.css", "theme.json"] { std::fs::write(theme.join(f), "x").unwrap(); }
            std::fs::remove_dir_all(work.join("output/site-blocks")).unwrap();
            std::os::unix::fs::symlink(&theme, work.join("output/linked")).unwrap();
            assert!(output_theme(&work).is_err(), "a linked theme folder is refused");
            std::fs::rename(work.join("verification-summary.json"), outside.path().join("summary.json")).unwrap();
            std::os::unix::fs::symlink(outside.path().join("summary.json"), work.join("verification-summary.json")).unwrap();
            assert!(summary(&work).is_none(), "a linked summary is not read");
        }
        assert_eq!(stop_message(&serde_json::json!({"current":6}), Some("The service stopped.")), "The conversion to a Gutenberg block theme stopped at step 6 of 9 (Parts, templates and patterns). The service stopped. Choose Continue to resume from there; what is done so far is kept.");
    }
    /// The confinement against the real image (docker): H2WP_TEST_H2G_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_H2G_IMAGE to the built runtime image"]
    async fn the_real_sandbox_reaches_no_account_and_writes_only_work() {
        let Ok(image) = std::env::var("H2WP_TEST_H2G_IMAGE") else { return };
        let dir = tempfile::tempdir().unwrap();
        let (work, input) = (dir.path().join("work"), dir.path().join("input"));
        std::fs::create_dir_all(&work).unwrap();
        tests::html2wp_theme(&input);
        let name = format!("h2wpd-test-{}-h2g", std::process::id());
        crate::runtime::docker(&sandbox_args(&name, &image, &work, &input), None, 120).await.unwrap();
        crate::runtime::docker(&["start".into(), name.clone()], None, 120).await.unwrap();
        let sh = |cmd: &str| { let (name, cmd) = (name.clone(), cmd.to_string()); async move { crate::runtime::docker(&["exec".into(), "--user".into(), "1000:1000".into(), name, "bash".into(), "-lc".into(), cmd], None, 120).await } };
        let results = (
            sh("cat /home/agent/.codex/auth.json").await.is_err(),
            sh("ls /home/agent/.codex 2>/dev/null | wc -l | grep -qx 0 || ls /home/agent/.codex").await.is_ok(),
            sh("touch /opt/desktop/x").await.is_err(),
            sh("touch /input/x").await.is_err(),
            sh("touch /work/written && test -f /work/written").await.is_ok(),
            sh("mkdir -p ~/.cache && touch ~/.cache/x && touch /tmp/x").await.is_ok(),
            sh(&format!("test -f {SKILL}/SKILL.md && test -f {SKILL}/scripts/doctor.sh && test -f /opt/wp-offline/wordpress/wp-includes/version.php")).await.is_ok(),
        );
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), name], None, 120).await;
        assert_eq!(results, (true, true, true, true, true, true, true), "(no auth.json, no .codex account, root read-only, input read-only, /work writable, a writable home and /tmp for the tools, the skill and WordPress present)");
        assert!(work.join("written").is_file());
    }
}
