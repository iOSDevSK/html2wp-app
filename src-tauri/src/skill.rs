//! A site's conversion with the html2wp plugin. The plugin owns the whole
//! conversion (its Flash and Full runs, every stage, the service, the
//! checks, the preview WordPress); the app gives its assistant a shell in the
//! project's own container, shows the progress the plugin writes, and puts
//! the files it delivered into Exports. The plugin's docs/APP-CONTRACT.md is
//! the source of every path, variable and field used here.
use crate::model::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// Where the plugin sits in the runtime image (its whole root), and its skill.
pub const PLUGIN: &str = "/opt/html2wp";
pub const SKILL: &str = "/opt/html2wp/skills/html2wp";
/// The project inside its container as the contract names it (a link to
/// its own host path): source (the plugin's copy of the owner's input),
/// workspace, out (what the owner gets) and .tmp.
pub const PROJECT: &str = "/project";
/// The progress the plugin writes (h2wp-progress/1) and its verdict at the
/// end (h2wp-result/1), relative to the project folder.
pub const PROGRESS: &str = "workspace/progress.json";
pub const RESULT: &str = "out/result.json";
/// Longest file the host reads from the project; anything larger is not read.
const READ_MAX: u64 = 1024 * 1024;
/// The newest Visual Edit Lite, which the plugin installs into its preview
/// WordPress; the app refreshes it before every run.
pub const VE_LITE: &str = ".app/visual-edit-lite.zip";
pub const VE_LITE_ENV: &str = "H2WP_VE_LITE_ZIP";

fn container_name(pid: &str) -> Result<String> { Ok(format!("{}-agent", crate::runtime::project_name(pid)?)) }

/// The run: Flash (the default) or Full for an HTML theme, the Astro run for
/// an "Astro 5 project only" project.
pub fn mode(p: &Project) -> &'static str { if p.astro_only() { "astro" } else if p.flash { "flash" } else { "full" } }

/// The run's goal, verbatim from the contract (APP-CONTRACT v1.1 §1, 69e6c68):
/// a new run says so, and the plugin refuses `progress.sh mode` without --new
/// over a run still in progress.
pub fn start_text(p: &Project) -> String {
    match mode(p) {
        "flash" => "Convert the project in /project/source to a WordPress theme with html2wp in Flash mode. Read /opt/html2wp/skills/html2wp/SKILL.md and follow its \"Flash mode\" section: every stage once, never loop, and finish with write-result.py. This is a new run: start it with progress.sh mode flash --new. Done means /project/out/result.json exists.".into(),
        "astro" => "Build the Astro 5 project of /project/source with html2wp. Read /opt/html2wp/skills/html2wp/SKILL.md and follow its \"The Astro 5 project only\" section: use its bounded capture recovery, preserve expected routes, never repeat unchanged failed work, and finish with write-result.py --mode astro. This is a new run: start it with progress.sh mode astro --new. Done means /project/out/result.json exists.".into(),
        _ => "Convert the project in /project/source to a WordPress theme with html2wp in Full mode. Read /opt/html2wp/skills/html2wp/SKILL.md and follow it to the end, write-result.py included. This is a new run: start it with progress.sh mode full --new. Done means /project/out/result.json exists.".into(),
    }
}
/// Continue or resume a run the plugin began, at the stage its progress
/// records (the contract's Continue goal). Only a run the plugin has not
/// begun at all starts with its own goal: once this run has progress, no
/// resume ever says "a new run".
pub fn continue_text(store: &crate::store::Store, p: &Project) -> String {
    let found = progress(store, p);
    if !current_run(store, p, &found) { return start_text(p); }
    match (found["stage"].as_str(), found["label"].as_str()) {
        (Some(stage), Some(label)) => format!("Continue the html2wp run of /project/source from its workspace, in the mode it started with. Resume at stage {stage} ({label}) as /project/workspace/progress.json records it; redo only dependencies invalidated by a documented Full repair and do not run progress.sh mode again."),
        _ => "Continue the html2wp run of /project/source from its workspace, in the mode it started with, where /project/workspace/progress.json records it; redo only dependencies invalidated by a documented Full repair and do not run progress.sh mode again.".into(),
    }
}
/// The progress belongs to the run the owner started last (the plugin
/// began it with progress.sh mode), not to an earlier one.
fn current_run(store: &crate::store::Store, p: &Project, found: &Value) -> bool {
    let at = |v: Option<String>| v.and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok());
    // When the plugin began the run, else when it last wrote: either is this run's only if not before its start.
    let began = found["startedAt"].as_str().or(found["updatedAt"].as_str()).map(String::from);
    match (at(store.setting(&format!("run-started:{}", p.id))), at(began)) {
        (Some(run), Some(began)) => began >= run - chrono::Duration::seconds(5),
        (None, began) => began.is_some(),
        (Some(_), None) => false,
    }
}

/// How the agent works in this app; the task itself is the contract's prompt.
pub fn instructions(p: &Project) -> String {
    format!("You run the html2wp plugin for the html2wp Desktop project \"{}\". The plugin is at {PLUGIN}: read {SKILL}/SKILL.md first and follow it exactly; its assets/scripts/ paths are relative to {SKILL}. The conversion is the skill's, not yours to redesign. \
Your only way to read, run and write files is project_shell: one bash command in the project's own container. The project is {PROJECT} (source/, workspace/, out/, .tmp/); the environment already carries H2WP_MODE, H2WP_TARGET, H2WP_WORKSPACE, H2WP_OUTPUT_DIR and the rest of the plugin's settings, so never change them. The network is available. Start long-running servers with setsid nohup … &. \
The owner watches the stages you report with the skill's progress.sh, and the app ends the run when {PROJECT}/{RESULT} is written. Never ask the owner to run commands or to choose; when the skill says to stop, stop and say why in one sentence.{}", p.name,
        if !takes_changes(p) { "" }
        else if p.astro_only() { " This Astro project was delivered: every change goes into its Astro sources, as SKILL.md's \"Changes after delivery\" section says for an Astro run (edit astro-project/src, apply it with apply-change.py, which runs only the Astro build). What the Astro sources cannot change, say plainly. Ordinary changes never start a pipeline stage or package the project. An explicit owner repair in repair-delivery mode instead follows Astro recovery; astro-delivery.py owns its candidate build and ZIP. Questions never start repair." }
        else { " This project was delivered: every change goes into the live theme, as SKILL.md's \"Changes after delivery\" section says (edit the theme's own files, apply them with apply-change.py). What the theme files cannot change, say plainly. Never start a stage or a new build, and never package the theme: packaging is the owner's, in the app." })
}
/// The Codex goal: the same task, which Codex works on until the host has the result.
pub fn goal(p: &Project) -> String { start_text(p) }

/// Flash or Full starts a new run. What an earlier run delivered stays in
/// Exports as an older revision; its result file is kept beside the new
/// run's, so only this run's result can end it.
pub fn new_run(store: &crate::store::Store, p: &mut Project, flash: bool) -> Result<()> {
    let flash = flash && !p.astro_only();
    if !matches!(p.target.as_str(), "html" | "astro") { return Err("This release converts to an HTML WordPress theme or an Astro 5 project (Gutenberg: import the HTML theme into \"Gutenberg from an HTML theme\"). Use Clean & restart to choose.".into()); }
    if p.phase == "deliverable_ready" || p.artifacts.iter().any(|a| a.revision == p.revision) { p.revision += 1; }
    set_aside_result(store, p)?;
    store.set(&format!("run-started:{}", p.id), &now())?;
    p.flash = flash;
    p.phase = "running".into();
    p.last_error = None;
    p.updated_at = now();
    Ok(())
}

/// Continue after the plugin stopped the run: its "stopped" (the result, and
/// the progress written before now) belongs to the attempt that ended, not
/// to this one. A delivered result stays: a Continue after delivery changes
/// nothing until the plugin delivers again.
pub fn resume_run(store: &crate::store::Store, p: &mut Project) -> Result<()> {
    if result(store, p)["status"] == "stopped" { set_aside_result(store, p)?; }
    store.set(&format!("resumed-at:{}", p.id), &now())?;
    if p.phase != "deliverable_ready" { p.phase = "running".into(); p.last_error = None; }
    Ok(())
}
/// Keep an earlier result beside the new run's, never in its way.
fn set_aside_result(store: &crate::store::Store, p: &Project) -> Result<()> {
    let result = store.path(&p.id)?.join(RESULT);
    if std::fs::symlink_metadata(&result).is_ok() {
        std::fs::rename(&result, result.with_file_name(format!("result-{}.json", chrono::Utc::now().format("%Y%m%dT%H%M%S%3f")))).map_err(err)?;
    }
    Ok(())
}

/// The project's container: the runtime image with the plugin, the project
/// folder mounted at its own host path (the plugin hands paths under TMPDIR
/// to the host's Docker for its build sandbox, which resolves them on the
/// host) and reached as /project through a link, the plugin's settings in
/// its environment and the host's Docker socket (the owner's decision: the
/// client does not attack his own machine). It holds no Codex account. The
/// owner's html2wp licence is copied in on start.
pub fn container_args(name: &str, image: &str, project: &Path, target: &str, plugin: &crate::plugin::Plugin) -> Vec<String> {
    let host = project.display().to_string();
    let env = [
        format!("H2WP_TARGET={target}"), format!("H2WP_WORKSPACE={PROJECT}/workspace"), format!("H2WP_OUTPUT_DIR={PROJECT}/out"),
        "H2WP_HOST=codex".into(), format!("H2WP_CONTAINER={name}"), "H2WP_STRICT_JOBS=1".into(),
        // The sandbox's copies: under the same-path mount, so the host's Docker finds them.
        format!("TMPDIR={host}/.tmp"), "HOME=/home/agent".into(),
    ];
    ["create", "--name", name, "--label", "dev.html2wp.desktop=true", "--label", LAYOUT, "--init",
        "--security-opt", "no-new-privileges", "--pids-limit", "2048", "--memory", "8g", "--shm-size", "1g",
        // The plugin's preview WordPress and its build sandbox use the host's
        // Docker: the socket is mounted and the agent user is in its group.
        "--user", "1000:1000", "--group-add", "0"].into_iter().map(String::from)
        .chain(["--workdir".into(), host.clone()])
        .chain(env.into_iter().flat_map(|e| ["--env".to_string(), e]))
        .chain(["--mount".into(), format!("type=bind,source={host},target={host}"),
            // The owner's original and the app's copies of delivered files stay the app's.
            "--mount".into(), format!("type=bind,source={host}/input,target={host}/input,readonly"),
            "--mount".into(), format!("type=bind,source={host}/artifacts,target={host}/artifacts,readonly"),
            "--mount".into(), "type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock".into()])
        // The plugin, read-only at /opt/html2wp: its repair never edits its own scripts.
        .chain(plugin.args())
        .chain([image.into(), "sleep".into(), "infinity".into()])
        .collect()
}
/// The container's layout; a container made for another one is made again.
const LAYOUT: &str = "dev.html2wp.layout=same-path-1";
/// The project's container, made from `image` (the installed runtime): one
/// made from an older runtime, or for another layout, is made again; the
/// project stays in its folder and the preview WordPress keeps running.
pub async fn ensure_container(store: &crate::store::Store, p: &Project, image: &str) -> Result<String> {
    use crate::runtime::docker;
    let name = container_name(&p.id)?;
    let project = store.path(&p.id)?;
    let plugin = crate::plugin::require(store)?;
    for dir in ["source", "workspace", "out", ".tmp", "artifacts", "input"] { std::fs::create_dir_all(project.join(dir)).map_err(err)?; }
    let inspect = [String::from("inspect"), name.clone()];
    let mut was_running = false;
    if docker(&inspect, None, 10).await.is_ok() {
        crate::runtime::assert_owned(&name).await?;
        let found = docker(&["inspect".into(), "--format".into(), "{{index .Config.Labels \"dev.html2wp.layout\"}}|{{.State.Running}}".into(), name.clone()], None, 10).await?;
        let mut parts = found.trim().split('|');
        let (layout, running) = (parts.next().unwrap_or(""), parts.next() == Some("true"));
        if crate::runtime::is_outdated(&name, image, Some(&plugin.commit)).await || Some(layout) != LAYOUT.split('=').nth(1) { docker(&["rm".into(), "-f".into(), name.clone()], None, 30).await?; }
        else { was_running = running; }
    }
    if docker(&inspect, None, 10).await.is_err() {
        let mut args = container_args(&name, image, &project, &p.target, &plugin);
        // Developer override only (H2WP_API in the app's environment): the
        // plugin talks to a local html2wp service.
        if let Some((api, insecure_http)) = crate::licence::container_api() {
            let at = args.len() - 3;
            let mut extra = vec!["--env".to_string(), format!("H2WP_API={api}"), "--add-host".into(), "host.docker.internal:host-gateway".into()];
            if insecure_http { extra.extend(["--env".into(), "H2WP_ALLOW_INSECURE_API=1".into()]); }
            args.splice(at..at, extra);
        }
        // Docker Desktop's file sharing can see a folder made a moment ago only a little later.
        let mut tries = 0;
        loop {
            match docker(&args, None, 60).await {
                Err(e) if e.contains("bind source path does not exist") && tries < 15 => { tries += 1; tokio::time::sleep(std::time::Duration::from_secs(2)).await; }
                other => { other?; break; }
            }
        }
    }
    if !was_running {
        docker(&["start".into(), name.clone()], None, 30).await?;
        // /project, as the contract's prompts name it: the project's host path.
        docker(&["exec".into(), "--user".into(), "0".into(), name.clone(), "sh".into(), "-c".into(), "[ -L /project ] || [ ! -e /project ] || exit 1; ln -sfn \"$1\" /project".into(), "sh".into(), project.display().to_string()], None, 30).await?;
    }
    // The licence the owner saved in Settings, or none (Free), as it is now.
    let licence = std::fs::read(store.root.join("private/licence")).ok();
    let write = "mkdir -p ~/.config/html2wp && cd ~/.config/html2wp && umask 077 && cat > licence.new && if [ -s licence.new ]; then mv licence.new licence; else rm -f licence.new licence; fi";
    docker(&["exec".into(), "-i".into(), name.clone(), "sh".into(), "-c".into(), write.into()], Some(licence.as_deref().unwrap_or(b"")), 30).await?;
    if was_running { return Ok(name); }
    // The container was stopped: the plugin's preview relay starts again (§5, test-env.sh check).
    check_previews(&name).await;
    Ok(name)
}
/// test-env.sh check for every preview of the project (§5): the relay reaches it again.
async fn check_previews(name: &str) {
    let relay = format!("cd {PROJECT}/workspace 2>/dev/null || exit 0; for f in .test-env-*.json; do [ -f \"$f\" ] || continue; s=${{f#.test-env-}}; {SKILL}/assets/scripts/test-env.sh check \"${{s%.json}}\" >/dev/null 2>&1 || true; done");
    let _ = crate::runtime::docker(&["exec".into(), name.into(), "bash".into(), "-c".into(), relay], None, 300).await;
}
/// The UI delegates editor activation to the plugin; status polling stays read-only.
pub async fn prepare_preview_editor(store: &crate::store::Store, p: &Project, image: &str) -> Result<Value> {
    let state = crate::preview::state(store, p).ok_or("No preview state")?;
    let slug = state["slug"].as_str().filter(|s| !s.is_empty() && s.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')).ok_or("Invalid preview slug")?;
    let name = ensure_container(store, p, image).await?;
    let out = crate::runtime::docker(&["exec".into(), name, "python3".into(),
        format!("{SKILL}/assets/scripts/ensure-preview-editor.py"), "--env".into(),
        format!("{PROJECT}/workspace/.test-env-{slug}.json")], None, 180).await?;
    serde_json::from_str(out.trim()).map_err(err)
}

/// Before a change turn, as a Continue has it (v1.3 §7b): apply-change.py
/// replaces the theme in the running preview, so its WordPress runs and the
/// relay reaches it. Best effort: the plugin says so when it cannot apply.
pub async fn ready_preview(store: &crate::store::Store, p: &Project, image: &str) {
    if crate::preview::status(store, p).await["running"] == false { let _ = crate::preview::action(store, p, "start").await; }
    if let Ok(name) = ensure_container(store, p, image).await { check_previews(&name).await; }
}
/// Stop the project's container: every command and server of the run ends.
pub async fn stop_container(pid: &str) -> Result<()> {
    let name = container_name(pid)?;
    if crate::runtime::assert_owned(&name).await.is_ok() {
        crate::runtime::docker(&["stop".into(), "-t".into(), "5".into(), name], None, 30).await?;
    }
    Ok(())
}

/// project_shell: one bash command in the project's container (on `image`,
/// the installed runtime), in the run's mode.
pub async fn exec(store: &crate::store::Store, p: &Project, image: &str, args: &Value) -> Result<Value> {
    // A change never packages: a release is only the owner's Make release button.
    if change_turn(store, p) && args["cmd"].as_str().is_some_and(|c| c.contains("package-theme")) {
        return Ok(json!({"exitCode":3,"output":"Not in a change: the owner packages the theme with the app's Make release. Apply the change with apply-change.py and answer.","truncated":false}));
    }
    let host = store.path(&p.id)?.display().to_string();
    let (cmd, cwd, seconds) = crate::agent::exec_args(args, PROJECT, &[PROJECT, &host, PLUGIN, "/tmp", "/home/agent"])?;
    let name = ensure_container(store, p, image).await?;
    let result = crate::agent::exec(&name, &cwd, &command_env(store, p), cmd, seconds).await?;
    let _ = crate::run_context::tool_log(store, p, &cmd, &result);
    Ok(result)
}
/// After delivery the owner's chat message is a change: to the delivered
/// HTML theme in its live preview (v1.3), or to a delivered Astro run's
/// sources with only the Astro build (v1.4 §7d); never a new run.
pub fn takes_changes(p: &Project) -> bool { p.phase == "deliverable_ready" && matches!(p.target.as_str(), "html" | "astro") }
/// The plugin's final result in the workspace: the one its guards read
/// (APP-CONTRACT v1.4: the run's FINAL result only).
const FINAL: &str = "workspace/result.json";
/// The plugin stopped this site's run, and no run or repair wrote a result
/// since: its guard refuses every stage (v1.4 §7c). The owner's chat or
/// Continue is then a repair-stop, never a new run.
pub fn stopped(store: &crate::store::Store, p: &Project) -> bool {
    if p.target != "html" || p.phase == "deliverable_ready" { return false; }
    final_stopped(store, p)
}
/// The plugin's workspace result says stopped (v1.4: what "stopped" means for
/// the app; a progress that says stopped without it is a run cut short).
pub fn final_stopped(store: &crate::store::Store, p: &Project) -> bool {
    let Ok(root) = store.path(&p.id) else { return false };
    let found = read_json(&root, FINAL);
    found["schema"] == "h2wp-result/1" && found["status"] == "stopped"
}
/// A stopped run's repair-stop goal (APP-CONTRACT v1.4 §1, verbatim): the
/// owner's words, or "none — Continue".
pub fn repair_stop_text(message: Option<&str>) -> String {
    format!("The html2wp run of /project/source stopped; /project/workspace/result.json says at which stage and why. Follow /opt/html2wp/skills/html2wp/SKILL.md's \"A stopped run — repair, then continue\" section: spend the repair attempts it allows on the stopping stage's levers, then continue the run from that stage to the end, write-result.py included. Never start a new run. The owner's message: {}",
        message.map(str::trim).filter(|m| !m.is_empty()).unwrap_or("none — Continue"))
}
/// Capability of the plugin actually mounted in this project's container.
pub async fn astro_repair_supported(store: &crate::store::Store, p: &Project, image: &str) -> bool {
    // Called only for an idle project after chat_ready. Use the ordinary
    // preparation path so a stopped container is resumed and an obsolete
    // idle mount is replaced exactly as it would be for the next agent turn.
    let Ok(name) = ensure_container(store, p, image).await else { return false };
    crate::runtime::docker(&["exec".into(), name, "test".into(), "-f".into(),
        format!("{SKILL}/assets/scripts/astro-delivery.py")], None, 15).await.is_ok()
}
/// Legacy compatibility only offers recovery; the plugin validates the artifact.
pub fn astro_recovery_candidate(found: &Value) -> bool {
    if found["target"] != "astro" || found["status"] != "delivered" || !found["astro"]["file"].is_string() { return false; }
    if found["recovery"]["policy"].is_string() { return found["recovery"]["available"] == true; }
    found["builtSite"].is_null() || found["gates"].as_array().is_some_and(|gates| gates.iter().any(|g|
        g["byDesign"] != true && matches!(g["status"].as_str(), Some("failed" | "not_run"))))
}
pub fn astro_recovery_text(request: &str) -> String {
    format!("The Astro delivery has outstanding checks. Read SKILL.md's Astro recovery section and preserve the owner's intent. A question must be answered without starting repair, changing the session or packaging. If the owner requests a conversion fix, run astro-delivery.py /project/workspace repair: it owns the bounded repair, isolated candidate and delivery, and keeps the previous ZIP on failure. The normal Change prohibition on pipeline stages does not apply to this explicit repair. Never start a new run. The owner's exact request: {}", request.trim())
}
/// Explicit owner request to improve an already available Full product.
pub fn repair_delivery_turn(store: &crate::store::Store, p: &Project) -> bool {
    store.setting(&format!("turn-mode:{}", p.id)).as_deref() == Some("repair-delivery")
}
pub fn repair_delivery(store: &crate::store::Store, p: &mut Project) -> Result<String> {
    let found = result(store, p);
    if !(p.astro_only() && astro_recovery_candidate(&found)) && (p.target != "html" || found["recovery"]["available"] != true) {
        return Err("The plugin has not offered a repair for this delivery.".into());
    }
    store.set(&format!("repair-result-backup:{}", p.id), &found.to_string())?;
    set_aside_result(store, p)?;
    store.set(&format!("turn-mode:{}", p.id), "repair-delivery")?;
    store.set(&format!("resumed-at:{}", p.id), &now())?;
    p.phase = "running".into();
    p.last_error = None;
    if p.astro_only() { return Ok(astro_recovery_text("Repair the remaining conversion issues and deliver the best Astro ZIP.")); }
    Ok("Repair the remaining issues in the delivered Full HTML theme using SKILL.md's Full HTML delivery policy. First run full-delivery.py /project/workspace begin. Preserve the previous ZIP and owner edits, use this owner's bounded attempts, rebuild only invalidated dependencies, then package and write-result.py even if some checks remain red. Never start a new run.".into())
}
/// The host never accepted the requested repair: restore its available product.
pub fn rollback_delivery_repair(store: &crate::store::Store, p: &Project) -> Result<()> {
    if let Some(raw) = store.setting(&format!("repair-result-backup:{}", p.id)) {
        let file = store.path(&p.id)?.join(RESULT);
        let tmp = file.with_extension("json.tmp");
        std::fs::write(&tmp, raw).map_err(err)?;
        std::fs::rename(tmp, file).map_err(err)?;
    }
    store.set(&format!("turn-mode:{}", p.id), "")?;
    Ok(())
}
/// Before a repair-stop turn: the stale out/result.json aside (its end is a
/// new one), the stop of the attempt that ended not this turn's.
pub fn repair_stop(store: &crate::store::Store, p: &mut Project) -> Result<()> {
    set_aside_result(store, p)?;
    store.set(&format!("resumed-at:{}", p.id), &now())?;
    store.set(&format!("turn-mode:{}", p.id), "repair-stop")?;
    p.phase = "running".into();
    p.last_error = None;
    Ok(())
}
/// Each message or start of the owner is a new owner turn; the goal's own
/// continuations keep it (v1.4 H2WP_TURN: 2 repair attempts per message).
pub fn new_owner_turn(store: &crate::store::Store, pid: &str) -> Result<String> {
    let turn = uuid::Uuid::new_v4().to_string();
    store.set(&format!("owner-turn:{pid}"), &turn)?;
    Ok(turn)
}
/// A repair-stop turn ended with the stop repaired (the plugin set its
/// stopped result aside): the run goes on as a Continue, in its own mode.
pub fn repaired(store: &crate::store::Store, p: &Project) -> bool {
    repair_stop_turn(store, p) && !final_stopped(store, p) && result(store, p)["schema"] != "h2wp-result/1"
}
/// The project's turns are a repair-stop's (until a run or a change sets another kind).
pub fn repair_stop_turn(store: &crate::store::Store, p: &Project) -> bool {
    p.target == "html" && store.setting(&format!("turn-mode:{}", p.id)).as_deref() == Some("repair-stop")
}
/// A result is there (delivered or stopped): a new run over it is the
/// owner's "Start over", which the plugin allows only with H2WP_START_OVER=1.
pub fn has_result(store: &crate::store::Store, p: &Project) -> bool {
    let Ok(root) = store.path(&p.id) else { return false };
    read_json(&root, FINAL)["schema"] == "h2wp-result/1" || result(store, p)["schema"] == "h2wp-result/1"
}
/// The owner started this run over a result: its commands carry
/// H2WP_START_OVER=1 until the plugin has begun it (and never otherwise).
pub fn set_start_over(store: &crate::store::Store, p: &Project, over: bool) -> Result<()> {
    store.set(&format!("start-over:{}", p.id), if over { "yes" } else { "" })
}
/// The kind of the project's current turn: a change after delivery, or not.
pub fn set_change_turn(store: &crate::store::Store, p: &Project, change: bool) -> Result<()> {
    store.set(&format!("turn-mode:{}", p.id), if change { "change" } else { "" })
}
fn change_turn(store: &crate::store::Store, p: &Project) -> bool {
    takes_changes(p) && store.setting(&format!("turn-mode:{}", p.id)).as_deref() == Some("change")
}
/// A chat message after delivery: the contract's change goal (APP-CONTRACT
/// v1.3 §1, b9bf4fc, verbatim), the owner's words its request.
pub fn change_text(store: &crate::store::Store, p: &Project, request: &str) -> String {
    // An Astro run's change (v1.4 §7d, 61e6915, verbatim): the Astro sources, only the Astro build.
    if p.astro_only() {
        return format!("The Astro project of /project/source was delivered. Make the owner's change as /opt/html2wp/skills/html2wp/SKILL.md's \"Changes after delivery\" section says for an Astro run: edit only /project/workspace/astro-project/src (and public for an image), apply it with apply-change.py — it runs only the Astro build — look at its screenshots, and answer. What the Astro sources cannot change, say plainly. Never start a pipeline stage or a new run. The owner's request: {}", request.trim());
    }
    format!("The project in /project/source was delivered. Make the owner's change in the live theme as /opt/html2wp/skills/html2wp/SKILL.md's \"Changes after delivery\" section says: edit only /project/workspace/theme/{}/, apply it with apply-change.py, look at its screenshots, and answer. What the theme files cannot change, say plainly. Never start a stage or a new build. The owner's request: {}", theme_slug(store, p).unwrap_or_else(|| "<slug>".into()), request.trim())
}
/// The delivered theme's folder name: the manifest's slug, else the one theme in the workspace.
fn theme_slug(store: &crate::store::Store, p: &Project) -> Option<String> {
    let root = store.path(&p.id).ok()?;
    let safe = |s: &str| !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if let Some(slug) = read_json(&root, "workspace/conversion-manifest.json")["site"]["slug"].as_str().filter(|s| safe(s)) { return Some(slug.into()); }
    let themes: Vec<String> = std::fs::read_dir(root.join("workspace/theme")).ok()?.flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| safe(n)).collect();
    (themes.len() == 1).then(|| themes[0].clone())
}
/// The change log the plugin keeps after delivery (h2wp-changes/1).
pub const CHANGES: &str = "workspace/changes.json";
/// The log as the Overview and Exports show it: the plugin's own count of
/// the changes since the last ZIP (sinceZip), whether the live theme differs
/// from that ZIP (changedSinceZip), and the applied changes since delivery.
/// None before the plugin wrote a log.
pub fn changes(store: &crate::store::Store, p: &Project) -> Value {
    let Ok(root) = store.path(&p.id) else { return Value::Null };
    let found = read_json(&root, CHANGES);
    if found["schema"] != "h2wp-changes/1" { return Value::Null; }
    let applied = found["changes"].as_array().into_iter().flatten().filter(|c| c["applied"] == true).count();
    json!({"count":applied,"sinceZip":found["sinceZip"].as_u64().unwrap_or(0),"changedSinceZip":found["changedSinceZip"] == true})
}
/// Make release (contract v1.3 "Get ZIP"): the plugin packages the live
/// theme, no model turn; only the owner's button does this. A theme that changed since the last ZIP becomes the next revision in
/// Exports; an unchanged one is the ZIP already there.
pub async fn package(store: &crate::store::Store, p: &mut Project, image: &str, emit: &(impl Fn(&str, Value) + Send + Sync)) -> Result<Value> {
    if !takes_changes(p) { return Err("Make release packages a delivered theme or Astro project; this project has none yet.".into()); }
    // A release is not a change turn: its command runs in the run's own mode.
    set_change_turn(store, p, false)?;
    let ran = exec(store, p, image, &json!({"cmd":format!("python3 {SKILL}/assets/scripts/package-theme.py {PROJECT}/workspace"),"timeout":900})).await?;
    let output = ran["output"].as_str().unwrap_or_default();
    let said = output.lines().rev().find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(|v| v.is_object()));
    let Some(said) = said.filter(|_| ran["exitCode"] == 0) else {
        let reason = output.lines().rev().find(|l| l.contains("package-theme")).unwrap_or("the plugin could not package the theme").trim();
        return Err(format!("The theme was not packaged: {}", reason.chars().take(300).collect::<String>()));
    };
    // The release's own file: the theme ZIP, or the Astro project's for an Astro run.
    let main = if p.astro_only() { "astro" } else { "theme" };
    if said["reused"] == true || !new_result(store, p) {
        let file = p.artifacts.iter().find(|a| a.revision == p.revision && a.kind == main).map(|a| a.filename.clone()).unwrap_or_default();
        return Ok(json!({"changed":false,"revision":p.revision,"filename":file}));
    }
    deliver_packaged(store, p, emit)?;
    let file = p.artifacts.iter().find(|a| a.revision == p.revision && a.kind == main).map(|a| a.filename.clone()).unwrap_or_default();
    Ok(json!({"changed":true,"revision":p.revision,"filename":file}))
}
/// What every command of the run gets besides the container's own settings.
fn command_env(store: &crate::store::Store, p: &Project) -> Vec<String> {
    // The turn's kind (v1.4): a change after delivery; a repair-stop while the
    // plugin's result says stopped; else the run's own mode.
    let kind = if repair_delivery_turn(store, p) { "repair-delivery" } else if change_turn(store, p) { "change" } else if repair_stop_turn(store, p) { "repair-stop" } else { mode(p) };
    let mut env = vec![format!("H2WP_MODE={kind}")];
    // The owner's turn (v1.4): the plugin keys a repair-stop's allowance on it.
    if let Some(turn) = store.setting(&format!("owner-turn:{}", p.id)).filter(|t| !t.is_empty()) { env.push(format!("H2WP_TURN={turn}")); }
    if store.setting(&format!("start-over:{}", p.id)).as_deref() == Some("yes") && !current_run(store, p, &progress(store, p)) {
        env.push("H2WP_START_OVER=1".into());
    }
    if store.path(&p.id).is_ok_and(|root| root.join(VE_LITE).is_file()) { env.push(format!("{VE_LITE_ENV}={PROJECT}/{VE_LITE}")); }
    env
}
/// Before a run: the newest Visual Edit Lite where the plugin finds it. Best
/// effort: offline and without a cached release the run goes on without it.
pub async fn stage_visual_edit_lite(store: &crate::store::Store, p: &Project) {
    let Ok((_, source)) = crate::editor::latest(store).await else { return };
    let Ok(root) = store.path(&p.id) else { return };
    let dest = root.join(VE_LITE);
    if std::fs::symlink_metadata(&dest).is_ok_and(|m| m.file_type().is_symlink()) { let _ = std::fs::remove_file(&dest); }
    let _ = std::fs::create_dir_all(root.join(".app")).and_then(|_| std::fs::copy(source, &dest));
}

/// A file of the project read by the host: a regular file inside the
/// project folder, never reached through a link (the container writes the
/// project; a link there could point the host anywhere).
fn project_file(root: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.starts_with('/') || relative.split('/').any(|c| c == ".." || c.is_empty()) { return None; }
    let path = root.join(relative);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    let inside = matches!((std::fs::canonicalize(root), std::fs::canonicalize(&path)), (Ok(r), Ok(f)) if f.starts_with(&r));
    (meta.file_type().is_file() && inside).then_some(path)
}
fn read_json(root: &Path, relative: &str) -> Value {
    project_file(root, relative).filter(|f| std::fs::metadata(f).is_ok_and(|m| m.len() <= READ_MAX))
        .and_then(|f| std::fs::read(f).ok()).and_then(|raw| serde_json::from_slice::<Value>(&raw).ok())
        .filter(Value::is_object).unwrap_or(json!({}))
}
/// The plugin's progress (h2wp-progress/1), or {} before it wrote any.
pub fn progress(store: &crate::store::Store, p: &Project) -> Value {
    let Ok(root) = store.path(&p.id) else { return json!({}) };
    let found = read_json(&root, PROGRESS);
    if found["schema"] == "h2wp-progress/1" { found } else { json!({}) }
}
/// The plugin's verdict (h2wp-result/1), or {} before the run ended.
pub fn result(store: &crate::store::Store, p: &Project) -> Value {
    let Ok(root) = store.path(&p.id) else { return json!({}) };
    let found = read_json(&root, RESULT);
    if found["schema"] == "h2wp-result/1" { found } else { json!({}) }
}
/// This run's progress says the plugin stopped it (a report from an earlier
/// run does not count), or its result says so: the reason.
pub fn failure(store: &crate::store::Store, p: &Project) -> Option<String> {
    let done = result(store, p);
    if done["status"] == "stopped" {
        let at = done["stopped"]["stage"].as_str().map(|s| format!(" (stage {s})")).unwrap_or_default();
        return Some(format!("{}{at}", done["stopped"]["reason"].as_str().unwrap_or("the plugin stopped the run").chars().take(500).collect::<String>()));
    }
    // A repair-stop ends with the plugin's new result only: its progress may
    // still say stopped while the stopping stage is repaired.
    if repair_stop_turn(store, p) { return None; }
    let found = progress(store, p);
    // A stop written before the owner's last start or Continue belongs to the attempt that ended.
    let at = |key: &str| store.setting(&format!("{key}:{}", p.id)).and_then(|at| chrono::DateTime::parse_from_rfc3339(&at).ok());
    let since = at("run-started").into_iter().chain(at("resumed-at")).max();
    let written = found["updatedAt"].as_str().and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok());
    let current = match (since, written) { (Some(s), Some(w)) => w >= s - chrono::Duration::seconds(5), _ => true };
    (found["state"] == "stopped" && current).then(|| {
        let note = found["note"].as_str().filter(|n| !n.trim().is_empty()).unwrap_or("the plugin stopped the run");
        format!("{}{}", note.chars().take(500).collect::<String>(), found["label"].as_str().map(|l| format!(" (at {l})")).unwrap_or_default())
    })
}
/// The plugin delivered again after this revision (a change the owner asked
/// for after delivery): its result names another theme or Astro project.
pub fn new_result(store: &crate::store::Store, p: &Project) -> bool {
    let found = result(store, p);
    if found["status"] != "delivered" { return false; }
    if repair_delivery_turn(store, p) {
        let request = found["repairRequestId"].as_str();
        if request != store.setting(&format!("owner-turn:{}", p.id)).as_deref() { return false; }
        return request.is_some() && request != store.setting(&format!("delivered-repair:{}", p.id)).as_deref();
    }
    let main = if p.astro_only() { "astro" } else { "theme" };
    let Some(sha) = found[main]["sha256"].as_str() else { return false };
    !p.artifacts.iter().any(|a| a.revision == p.revision && a.kind == main && a.sha256.eq_ignore_ascii_case(sha))
}
/// A run the plugin stopped still leaves its report: it goes into Exports
/// with the revision, so the owner can read why.
pub fn keep_report(store: &crate::store::Store, p: &mut Project) -> Result<()> {
    let found = result(store, p);
    if found["status"] != "stopped" { return Ok(()); }
    let root = store.path(&p.id)?;
    let revision = root.join("artifacts").join(format!("revision-{}", p.revision));
    for (kind, name) in [("report", &found["report"]["markdown"]), ("pdf", &found["report"]["pdf"])] {
        let Some(file) = name.as_str().and_then(|n| project_file(&root.join("out"), n)) else { continue };
        let filename = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::create_dir_all(&revision).map_err(err)?;
        std::fs::copy(&file, revision.join(&filename)).map_err(err)?;
        p.artifacts.retain(|a| !(a.revision == p.revision && a.filename == filename));
        p.artifacts.push(Artifact { id: id(), revision: p.revision, sha256: crate::files::hash(&revision.join(&filename))?, filename, created_at: now(), kind: kind.into(), reviewed: true, checks: mode(p).into() });
    }
    store.put(p)
}
/// Where the run got to, in one line for the chat.
pub fn stop_message(store: &crate::store::Store, p: &Project, reason: Option<&str>) -> String {
    let found = progress(store, p);
    let at = found["label"].as_str().filter(|l| !l.is_empty()).map(|l| format!(" at \"{l}\"")).unwrap_or_default();
    format!("The conversion stopped{at}.{} Choose Continue to resume; what is done so far is kept.",
        reason.filter(|r| !r.trim().is_empty()).map(|r| format!(" {}", r.trim())).unwrap_or_default())
}

/// The end of a run: the plugin's result says delivered, so the host copies
/// the theme (checked against the result's SHA-256), the Astro project and
/// the report into this revision's Exports and tells the owner. Not there
/// yet: the run goes on.
pub fn deliver(store: &crate::store::Store, p: &mut Project, emit: &(impl Fn(&str, Value) + Send + Sync)) -> Result<bool> {
    deliver_as(store, p, emit, false)
}
/// A release after changes (the plugin's package-theme.py): the next revision, packaged, not checked again.
pub fn deliver_packaged(store: &crate::store::Store, p: &mut Project, emit: &(impl Fn(&str, Value) + Send + Sync)) -> Result<bool> {
    deliver_as(store, p, emit, true)
}
/// `packaged`: a release after changes, not a run's delivery.
fn deliver_as(store: &crate::store::Store, p: &mut Project, emit: &(impl Fn(&str, Value) + Send + Sync), packaged: bool) -> Result<bool> {
    let found = result(store, p);
    if found["status"] != "delivered" { return Ok(false); }
    if repair_delivery_turn(store, p) && found["repairRequestId"].as_str() != store.setting(&format!("owner-turn:{}", p.id)).as_deref() { return Ok(false); }
    // What this revision already delivered stays as it is; a new result is the next revision.
    if p.artifacts.iter().any(|a| a.revision == p.revision) { p.revision += 1; }
    let root = store.path(&p.id)?;
    let revision = root.join("artifacts").join(format!("revision-{}", p.revision));
    std::fs::create_dir_all(&revision).map_err(err)?;
    let files = [("theme", &found["theme"]["file"], &found["theme"]["sha256"]), ("astro", &found["astro"]["file"], &found["astro"]["sha256"]),
        ("report", &found["report"]["markdown"], &Value::Null), ("pdf", &found["report"]["pdf"], &Value::Null)];
    let mut delivered = vec![];
    for (kind, name, sha) in files {
        let Some(name) = name.as_str() else { continue };
        let Some(file) = project_file(&root.join("out"), name) else {
            if kind == "theme" || kind == "astro" { return Err(format!("The plugin delivered {name}, which is not a file in /project/out.")); }
            continue;
        };
        let filename = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        std::fs::copy(&file, revision.join(&filename)).map_err(err)?;
        let hash = crate::files::hash(&revision.join(&filename))?;
        if let Some(expected) = sha.as_str().filter(|s| s.len() == 64) {
            if !expected.eq_ignore_ascii_case(&hash) { return Err(format!("{filename} does not match the SHA-256 in the plugin's result.")); }
        }
        // A packaged theme was not checked again; the report beside it is still the run's.
        let checks = if packaged && (kind == "theme" || (kind == "astro" && p.astro_only())) { "packaged" } else { mode(p) };
        delivered.push(Artifact { id: id(), revision: p.revision, sha256: hash, filename, created_at: now(), kind: kind.into(), reviewed: true, checks: checks.into() });
    }
    let main = if p.astro_only() { "astro" } else { "theme" };
    if !delivered.iter().any(|a| a.kind == main) { return Err(format!("The plugin's result says delivered but names no {} ZIP.", if p.astro_only() { "Astro project" } else { "theme" })); }
    p.artifacts.retain(|a| a.revision != p.revision);
    p.artifacts.extend(delivered);
    if repair_delivery_turn(store, p) {
        if let Some(turn) = found["repairRequestId"].as_str() { store.set(&format!("delivered-repair:{}", p.id), turn)?; }
    }
    p.phase = "deliverable_ready".into();
    p.last_error = None;
    p.updated_at = now();
    store.put(p)?;
    emit("project-updated", value(&*p));
    let file = p.artifacts.iter().find(|a| a.revision == p.revision && a.kind == main).map(|a| a.filename.clone()).unwrap_or_default();
    let verdict = found["verdict"].as_str().map(|v| format!(" {}.", v.trim().trim_end_matches('.').chars().take(200).collect::<String>())).unwrap_or_default();
    let (what, rest) = if p.astro_only() { ("Astro 5 project", "The project ZIP and everything else the run delivered are in Exports; the built site can be previewed or deployed from there.") }
        else { ("theme", "The theme, the report and everything else the conversion delivered are in Exports.") };
    let text = if packaged { format!("Release ready: {file}, revision {}. Packaged after changes, not checked again.", p.revision) }
        else { format!("Your {what} is ready: {file}.{verdict} {rest}") };
    if let Ok(m) = store.message_with_action(&p.id, "assistant", &text, Some("exports")) { emit("chat-message", value(m)); }
    if !packaged { crate::notifications::queue(emit, &format!("Your {what} is ready"), &format!("{}: {file} is in Exports.", p.name)); }
    Ok(true)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    /// A site project as import makes it.
    pub(crate) fn project() -> Project {
        serde_json::from_value(json!({"id":"0584dcf1-7f08-4efb-85cf-ae7284faf8f9","name":"site","sourceName":"site.zip","kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"running","revision":1,"threadId":"thread","pages":[],"gates":[],"artifacts":[],
            "preview":null,"runtimeImage":"img","pluginCommit":"abc","reporting":"not_required","lastError":null,"flash":true})).unwrap()
    }
    /// The plugin's own files, as tests/fixtures holds them (made by its progress.sh, and the contract's result).
    pub(crate) fn fixture(name: &str) -> Value {
        serde_json::from_str(&std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures").join(name)).unwrap()).unwrap()
    }
    fn write(store: &crate::store::Store, p: &Project, relative: &str, v: &Value) {
        let file = store.path(&p.id).unwrap().join(relative);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, v.to_string()).unwrap();
    }
    #[tokio::test]
    #[ignore = "requires H2WP_ASTRO_REPAIR_FIXTURE from the real helper"]
    async fn real_astro_repair_result_is_accepted_including_retained_zip() {
        let source = PathBuf::from(std::env::var("H2WP_ASTRO_REPAIR_FIXTURE").unwrap());
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project(); p.target="astro".into(); p.phase="deliverable_ready".into();
        store.put(&p).unwrap();
        let found: Value=serde_json::from_slice(&std::fs::read(source.join("result.json")).unwrap()).unwrap();
        let turn=found["repairRequestId"].as_str().expect("helper must bind the result to this request");
        store.set(&format!("turn-mode:{}",p.id),"repair-delivery").unwrap();
        store.set(&format!("owner-turn:{}",p.id),turn).unwrap();
        let out=store.path(&p.id).unwrap().join("out");std::fs::create_dir_all(&out).unwrap();
        for entry in std::fs::read_dir(&source).unwrap().flatten() {
            if entry.file_type().unwrap().is_file() { std::fs::copy(entry.path(),out.join(entry.file_name())).unwrap(); }
        }
        assert!(new_result(&store,&p));
        assert!(crate::agent::finish_turn(&store, &p.id, |_,_|{}).await.unwrap());
        p = store.project(&p.id).unwrap();
        assert!(p.artifacts.iter().filter(|a| a.kind == "astro").all(|a| a.checks != "packaged"));
        assert!(!new_result(&store,&p),"accepted repair cannot be delivered twice");
        // A later repair may retain exactly the same ZIP and still has a new outcome.
        store.set(&format!("owner-turn:{}",p.id),"retained-owner").unwrap();
        let mut retained=found; retained["repairRequestId"]=json!("retained-owner");
        write(&store,&p,RESULT,&retained);
        assert!(new_result(&store,&p),"same artifact hash must not hide the new repair report");
        assert!(crate::agent::finish_turn(&store, &p.id, |_,_|{}).await.unwrap());
    }

    #[tokio::test]
    #[ignore = "real Docker; requires H2WP_TEST_RUNTIME_IMAGE"]
    async fn astro_recovery_capability_resumes_a_stopped_container() {
        let image = std::env::var("H2WP_TEST_RUNTIME_IMAGE").unwrap();
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let plugin_dir = dir.path().join("plugin/1.0.0-test/plugins/html2wp");
        std::fs::create_dir_all(plugin_dir.join("skills/html2wp/assets/scripts")).unwrap();
        std::fs::write(plugin_dir.join("skills/html2wp/SKILL.md"), "test").unwrap();
        crate::plugin::activate(&store, "1.0.0-test", "capability-test").unwrap();
        let helper = plugin_dir.join("skills/html2wp/assets/scripts/astro-delivery.py");
        std::fs::write(&helper, "# test capability marker").unwrap();
        let mut p = project(); p.id = uuid::Uuid::new_v4().to_string(); p.target = "astro".into();
        store.put(&p).unwrap();
        let name = ensure_container(&store, &p, &image).await.unwrap();
        crate::runtime::docker(&["stop".into(), name.clone()], None, 30).await.unwrap();
        let resumed = astro_repair_supported(&store, &p, &image).await;
        std::fs::remove_file(&helper).unwrap();
        let unsupported = astro_repair_supported(&store, &p, &image).await;
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), name], None, 60).await;
        assert!(resumed, "stopped container must prepare normally before testing capability");
        assert!(!unsupported, "running container without the mounted helper is unsupported");
    }

    #[test]
    fn astro_recovery_context_keeps_questions_as_questions() {
        let legacy = json!({"target":"astro","status":"delivered","astro":{"file":"a.zip"},"builtSite":null,"recovery":{"available":false}});
        assert!(astro_recovery_candidate(&legacy));
        let mut explicit = legacy.clone();
        explicit["recovery"]["policy"] = json!("astro-delivery/1");
        assert!(!astro_recovery_candidate(&explicit), "new plugin refusal wins");
        explicit["recovery"]["available"] = json!(true);
        assert!(astro_recovery_candidate(&explicit));
        let question = "Why did the capture fail?";
        let prompt = astro_recovery_text(question);
        assert!(prompt.ends_with(question));
        assert!(prompt.contains("A question must be answered without starting repair"));
        assert!(prompt.contains("If the owner requests a conversion fix"));
        assert_eq!(legacy["recovery"]["available"], false);
    }

    #[test]
    fn the_container_carries_the_project_and_the_plugin_s_settings() {
        let host = "/Users/o/Library/Application Support/dev.html2wp.desktop/projects/x";
        let plugin = crate::plugin::Plugin { version: "1.0.0".into(), commit: "4667c22".into(), dir: "/Users/o/Library/Application Support/dev.html2wp.desktop/plugin/1.0.0/plugins/html2wp".into() };
        let args = container_args("h2wpd-x-agent", "img", Path::new(host), "html", &plugin);
        let joined = args.join(" ");
        assert!(joined.contains("--user 1000:1000 --group-add 0") && joined.contains("no-new-privileges") && joined.contains(&format!("--workdir {host}")));
        let mounts: Vec<String> = args.iter().enumerate().filter(|(i, _)| *i > 0 && args[i - 1] == "--mount").map(|(_, m)| m.clone()).collect();
        assert_eq!(mounts, [format!("type=bind,source={host},target={host}"), format!("type=bind,source={host}/input,target={host}/input,readonly"), format!("type=bind,source={host}/artifacts,target={host}/artifacts,readonly"),
            "type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock".into(), format!("type=bind,source={},target=/opt/html2wp,readonly", plugin.dir.display())], "the project at its own path (the host's Docker resolves the plugin's paths there), the owner's original and the app's copies read-only, the host's Docker for the plugin, the plugin read-only");
        assert!(joined.contains("--label dev.html2wp.plugin=4667c22") && joined.ends_with("img sleep infinity"));
        let env: Vec<&String> = args.iter().enumerate().filter(|(i, _)| *i > 0 && args[i - 1] == "--env").map(|(_, e)| e).collect();
        let tmp = format!("TMPDIR={host}/.tmp");
        for e in ["H2WP_TARGET=html", "H2WP_WORKSPACE=/project/workspace", "H2WP_OUTPUT_DIR=/project/out", "H2WP_HOST=codex", "H2WP_CONTAINER=h2wpd-x-agent", "H2WP_STRICT_JOBS=1", tmp.as_str()] {
            assert!(env.iter().any(|v| *v == e), "{e}");
        }
        assert!(!joined.contains(".codex") && !joined.contains("H2WP_KEY"), "no Codex account, and the licence is a private file, not the environment");
    }
    #[test]
    fn the_prompts_are_the_contract_s_for_an_html_theme_and_an_astro_project() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        // APP-CONTRACT v1.1 §1 (69e6c68), verbatim.
        assert_eq!(start_text(&p), "Convert the project in /project/source to a WordPress theme with html2wp in Flash mode. Read /opt/html2wp/skills/html2wp/SKILL.md and follow its \"Flash mode\" section: every stage once, never loop, and finish with write-result.py. This is a new run: start it with progress.sh mode flash --new. Done means /project/out/result.json exists.");
        p.flash = false;
        assert_eq!(goal(&p), "Convert the project in /project/source to a WordPress theme with html2wp in Full mode. Read /opt/html2wp/skills/html2wp/SKILL.md and follow it to the end, write-result.py included. This is a new run: start it with progress.sh mode full --new. Done means /project/out/result.json exists.");
        new_run(&store, &mut p, false).unwrap();
        assert_eq!((p.phase.as_str(), mode(&p)), ("running", "full"));
        // The Astro 5 project is its own run; Flash and Full are the theme's.
        p.target = "astro".into();
        new_run(&store, &mut p, true).unwrap();
        assert_eq!((mode(&p), p.flash), ("astro", false));
        assert_eq!(start_text(&p), "Build the Astro 5 project of /project/source with html2wp. Read /opt/html2wp/skills/html2wp/SKILL.md and follow its \"The Astro 5 project only\" section: use its bounded capture recovery, preserve expected routes, never repeat unchanged failed work, and finish with write-result.py --mode astro. This is a new run: start it with progress.sh mode astro --new. Done means /project/out/result.json exists.");
        // No Gutenberg target in this release: the h2g card covers it.
        p.target = "gutenberg".into();
        assert!(new_run(&store, &mut p, false).unwrap_err().contains("Gutenberg from an HTML theme"));
    }
    #[test]
    fn the_overview_reads_the_plugin_s_progress_and_a_stop_names_where() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        assert_eq!(progress(&store, &p), json!({}));
        write(&store, &p, PROGRESS, &json!({"schema":"something-else","state":"stopped"}));
        assert_eq!(progress(&store, &p), json!({}), "only the contract's schema is read");
        write(&store, &p, PROGRESS, &fixture("progress-running.json"));
        assert_eq!(progress(&store, &p)["label"], "the service builds the theme");
        assert_eq!(failure(&store, &p), None);
        assert_eq!(stop_message(&store, &p, Some("Codex usage is limited.")), "The conversion stopped at \"the service builds the theme\". Codex usage is limited. Choose Continue to resume; what is done so far is kept.");
        assert_eq!(continue_text(&store, &p), "Continue the html2wp run of /project/source from its workspace, in the mode it started with. Resume at stage 3 (the service builds the theme) as /project/workspace/progress.json records it; redo only dependencies invalidated by a documented Full repair and do not run progress.sh mode again.");
        // A new run whose plugin has not begun yet: the old run's stage is not this run's.
        let mut fresh = p.clone();
        new_run(&store, &mut fresh, true).unwrap();
        assert_eq!(continue_text(&store, &fresh), start_text(&fresh), "nothing of this run to resume: its own goal");
        write(&store, &p, PROGRESS, &fixture("progress-stopped.json"));
        new_run(&store, &mut p, true).unwrap();
        assert_eq!(failure(&store, &p), None, "an earlier run's stop does not end this one");
        store.set(&format!("run-started:{}", p.id), "2020-01-01T00:00:00Z").unwrap();
        assert_eq!(failure(&store, &p).as_deref(), Some("the build needs a DATABASE_URL the project does not provide (at prepare the input (prerender or static build))"));
    }
    #[test]
    fn the_run_ends_with_the_plugin_s_result_and_its_files_go_to_exports() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        let out = store.path(&p.id).unwrap().join("out");
        assert!(!deliver(&store, &mut p, &|_, _| {}).unwrap(), "no result yet: the run goes on");
        write(&store, &p, PROGRESS, &fixture("progress-finished.json"));
        assert!(!deliver(&store, &mut p, &|_, _| {}).unwrap(), "the progress is not the result");
        std::fs::create_dir_all(&out).unwrap();
        for (file, body) in [("clara-hayes-1.0.0.zip", "zip"), ("clara-hayes-astro-1.0.0.zip", "astro"), ("CONVERSION-REPORT.md", "# Report"), ("conversion-report.pdf", "%PDF")] { std::fs::write(out.join(file), body).unwrap(); }
        let mut delivered = fixture("result-delivered.json");
        delivered["theme"]["sha256"] = json!(crate::files::hash(&out.join("clara-hayes-1.0.0.zip")).unwrap());
        delivered["astro"]["sha256"] = json!("0".repeat(64));
        write(&store, &p, RESULT, &delivered);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap_err().contains("clara-hayes-astro-1.0.0.zip does not match the SHA-256"));
        delivered["astro"]["sha256"] = json!(crate::files::hash(&out.join("clara-hayes-astro-1.0.0.zip")).unwrap());
        write(&store, &p, RESULT, &delivered);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.phase, "deliverable_ready");
        assert_eq!(p.artifacts.iter().map(|a| (a.kind.as_str(), a.filename.as_str(), a.checks.as_str())).collect::<Vec<_>>(),
            [("theme", "clara-hayes-1.0.0.zip", "flash"), ("astro", "clara-hayes-astro-1.0.0.zip", "flash"), ("report", "CONVERSION-REPORT.md", "flash"), ("pdf", "conversion-report.pdf", "flash")]);
        assert!(store.path(&p.id).unwrap().join("artifacts/revision-1/clara-hayes-1.0.0.zip").is_file());
        let said = store.messages(&p.id).unwrap().pop().unwrap();
        assert_eq!(said.text, "Your theme is ready: clara-hayes-1.0.0.zip. Flash: not visually repaired. The theme, the report and everything else the conversion delivered are in Exports.");
        assert_eq!(said.action.as_deref(), Some("exports"));
        assert!(!new_result(&store, &p), "the delivered result is not new");
        // A change after delivery: the plugin delivers a new theme, the next revision.
        std::fs::write(out.join("clara-hayes-1.0.0.zip"), "zip, changed").unwrap();
        delivered["theme"]["sha256"] = json!(crate::files::hash(&out.join("clara-hayes-1.0.0.zip")).unwrap());
        write(&store, &p, RESULT, &delivered);
        assert!(new_result(&store, &p));
        let mut changed = p.clone();
        assert!(deliver(&store, &mut changed, &|_, _| {}).unwrap());
        let changed = store.project(&p.id).unwrap();
        assert_eq!(changed.revision, 2);
        assert_eq!(changed.artifacts.iter().filter(|a| a.revision == 1).count(), 4, "revision 1 stays as it was delivered");
        assert!(!new_result(&store, &changed));
        let p = changed;
        // A new run moves the old result aside: only its own result ends it.
        let mut again = p.clone();
        new_run(&store, &mut again, true).unwrap();
        assert_eq!(again.revision, 3);
        assert!(!deliver(&store, &mut again, &|_, _| {}).unwrap());
    }
    #[test]
    fn after_delivery_a_message_is_a_change_to_the_live_preview_never_a_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.target = "html".into();
        p.phase = "deliverable_ready".into();
        store.put(&p).unwrap();
        assert!(takes_changes(&p));
        let mode_of = |p: &Project| command_env(&store, p).into_iter().find(|e| e.starts_with("H2WP_MODE=")).unwrap();
        set_change_turn(&store, &p, true).unwrap();
        assert_eq!(mode_of(&p), "H2WP_MODE=change", "every command of a change turn");
        assert!(instructions(&p).contains("every change goes into the live theme") && instructions(&p).contains("What the theme files cannot change, say plainly. Never start a stage or a new build"));
        assert!(!instructions(&p).to_lowercase().contains("rebuild") && !instructions(&p).to_lowercase().contains("start over"), "the agent never suggests starting over (the owner)");
        assert!(instructions(&p).contains("never package the theme: packaging is the owner's"));
        assert!(!change_text(&store, &p, "x").to_lowercase().contains("package") && !change_text(&store, &p, "x").contains("ZIP"), "the change goal says nothing about packaging");
        // Starting over is a run: its commands are the run's, whatever came before.
        let mut rebuild = p.clone();
        rebuild.phase = "running".into();
        assert_eq!(mode_of(&rebuild), "H2WP_MODE=flash");
        set_change_turn(&store, &p, false).unwrap();
        assert_eq!(mode_of(&p), "H2WP_MODE=flash");
        // A delivered Astro run changes too (v1.4 §7d); a run under way never.
        let mut astro = p.clone();
        astro.target = "astro".into();
        let mut running = p.clone();
        running.phase = "interrupted".into();
        assert!(takes_changes(&astro) && !takes_changes(&running));
        assert_eq!(change_text(&store, &astro, " swap the hero photo "), "The Astro project of /project/source was delivered. Make the owner's change as /opt/html2wp/skills/html2wp/SKILL.md's \"Changes after delivery\" section says for an Astro run: edit only /project/workspace/astro-project/src (and public for an image), apply it with apply-change.py — it runs only the Astro build — look at its screenshots, and answer. What the Astro sources cannot change, say plainly. Never start a pipeline stage or a new run. The owner's request: swap the hero photo");
        assert!(instructions(&astro).contains("every change goes into its Astro sources") && instructions(&astro).contains("Ordinary changes never start a pipeline stage or package the project") && instructions(&astro).contains("explicit owner repair"));
        set_change_turn(&store, &astro, true).unwrap();
        assert_eq!(command_env(&store, &astro).into_iter().find(|e| e.starts_with("H2WP_MODE=")).unwrap(), "H2WP_MODE=change");
        set_change_turn(&store, &astro, false).unwrap();
        assert!(!instructions(&running).contains("was delivered"), "a run's instructions stay the run's");
        // APP-CONTRACT v1.3 §1 (b9bf4fc), verbatim, the theme's folder named.
        write(&store, &p, "workspace/conversion-manifest.json", &json!({"site":{"slug":"clara-hayes","version":"1.0.0"}}));
        assert_eq!(change_text(&store, &p, "  make the heading italic \n"), "The project in /project/source was delivered. Make the owner's change in the live theme as /opt/html2wp/skills/html2wp/SKILL.md's \"Changes after delivery\" section says: edit only /project/workspace/theme/clara-hayes/, apply it with apply-change.py, look at its screenshots, and answer. What the theme files cannot change, say plainly. Never start a stage or a new build. The owner's request: make the heading italic");
        assert!(!change_text(&store, &p, "x").to_lowercase().contains("rebuild"));
        write(&store, &p, "workspace/conversion-manifest.json", &json!({"site":{"slug":"../etc"}}));
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("workspace/theme/clara-hayes")).unwrap();
        assert!(change_text(&store, &p, "x").contains("/project/workspace/theme/clara-hayes/"), "the one theme folder when the manifest names none usable");
        assert!(!change_text(&store, &p, "x").contains("--new") && !change_text(&store, &p, "x").contains("progress.sh mode"), "a change never starts a run");
    }
    #[test]
    fn a_stopped_run_is_repaired_and_continued_never_started_over() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.target = "html".into();
        p.phase = "failed".into();
        store.put(&p).unwrap();
        let mode_of = |p: &Project| command_env(&store, p).into_iter().find(|e| e.starts_with("H2WP_MODE=")).unwrap();
        assert!(!stopped(&store, &p));
        let stop = json!({"schema":"h2wp-result/1","status":"stopped","stopped":{"stage":"3.5","reason":"no article layout"}});
        write(&store, &p, FINAL, &stop);
        write(&store, &p, RESULT, &stop);
        assert!(stopped(&store, &p), "the plugin's result says stopped");
        // APP-CONTRACT v1.4 §1, verbatim.
        assert_eq!(repair_stop_text(None), "The html2wp run of /project/source stopped; /project/workspace/result.json says at which stage and why. Follow /opt/html2wp/skills/html2wp/SKILL.md's \"A stopped run — repair, then continue\" section: spend the repair attempts it allows on the stopping stage's levers, then continue the run from that stage to the end, write-result.py included. Never start a new run. The owner's message: none — Continue");
        assert!(repair_stop_text(Some(" the blog uses /journal/ \n")).ends_with("The owner's message: the blog uses /journal/"));
        repair_stop(&store, &mut p).unwrap();
        store.put(&p).unwrap();
        assert!(!store.path(&p.id).unwrap().join(RESULT).exists(), "the stale out/result.json is set aside: its end is a new one");
        assert_eq!(p.phase, "running");
        assert_eq!(mode_of(&p), "H2WP_MODE=repair-stop", "every command of the repair-stop turn");
        assert!(!command_env(&store, &p).iter().any(|e| e.starts_with("H2WP_START_OVER")));
        // Its progress may still say stopped while the stage is repaired: not the end.
        let now = chrono::Utc::now().to_rfc3339();
        write(&store, &p, PROGRESS, &json!({"schema":"h2wp-progress/1","mode":"flash","stage":"3.5","label":"article layout","state":"stopped","startedAt":now,"updatedAt":now,"repairs":[{"stage":"3.5","attempt":1,"of":2,"lever":"article-part-residue","outcome":"open","by":"owner"}]}));
        assert_eq!(failure(&store, &p), None);
        assert!(crate::agent::resume_objective(&store, &{ store.set(&format!("goal:{}", p.id), &repair_stop_text(None)).unwrap(); p.clone() }).starts_with("The html2wp run of /project/source stopped"), "resumed as the repair-stop it is");
        assert!(!repaired(&store, &p), "the stop is still there");
        // Fixed: the plugin put its stop aside. The turn keeps repair-stop to its end;
        // then (goal_after_turn) the run goes on as a Continue in its own mode.
        std::fs::remove_file(store.path(&p.id).unwrap().join(FINAL)).unwrap();
        assert_eq!(mode_of(&p), "H2WP_MODE=repair-stop", "never switched back within the turn");
        assert!(repaired(&store, &p));
        set_change_turn(&store, &p, false).unwrap();
        assert_eq!(mode_of(&p), "H2WP_MODE=flash");
        assert!(!continue_text(&store, &p).contains("--new"));
        store.set(&format!("turn-mode:{}", p.id), "repair-stop").unwrap();
        // Not fixed: a new stopped result ends the goal (a stop is terminal).
        write(&store, &p, RESULT, &stop);
        assert!(failure(&store, &p).is_some());
    }
    #[test]
    fn delivered_repair_preserves_history_and_rejects_a_previous_result() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.phase = "deliverable_ready".into();
        p.flash = false;
        store.put(&p).unwrap();
        let mut delivered = fixture("result-delivered.json");
        delivered["recovery"] = json!({"available":true,"stages":["-1"],"action":"repair-delivery"});
        write(&store, &p, RESULT, &delivered);
        write(&store, &p, FINAL, &delivered);
        let goal = repair_delivery(&store, &mut p).unwrap();
        store.set(&format!("goal:{}", p.id), &goal).unwrap();
        assert_eq!(p.phase, "running");
        assert!(store.path(&p.id).unwrap().join(FINAL).exists());
        assert!(!store.path(&p.id).unwrap().join(RESULT).exists());
        assert!(command_env(&store, &p).contains(&"H2WP_MODE=repair-delivery".into()));
        assert_eq!(crate::agent::resume_objective(&store, &p), goal);
        let turn = new_owner_turn(&store, &p.id).unwrap();
        write(&store, &p, RESULT, &delivered);
        assert!(!new_result(&store, &p), "an old result cannot finish the repair");
        delivered["repairRequestId"] = json!(turn);
        write(&store, &p, RESULT, &delivered);
        assert!(new_result(&store, &p));
        rollback_delivery_repair(&store, &p).unwrap();
        assert_eq!(result(&store, &p)["recovery"]["available"], true);
        assert!(!repair_delivery_turn(&store, &p));
    }
    #[test]
    fn a_note_or_timestamp_is_not_new_conversion_progress() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let p = project();
        store.put(&p).unwrap();
        let mut progress = fixture("progress-running.json");
        write(&store, &p, PROGRESS, &progress);
        let before = crate::agent::progress_signature(&store, &p);
        progress["note"] = json!("still waiting");
        progress["updatedAt"] = json!("2099-01-01T00:00:00Z");
        write(&store, &p, PROGRESS, &progress);
        assert_eq!(before, crate::agent::progress_signature(&store, &p));
        progress["stages"][0]["state"] = json!("warned");
        write(&store, &p, PROGRESS, &progress);
        assert_ne!(before, crate::agent::progress_signature(&store, &p));
    }
    #[test]
    fn every_command_of_an_owner_turn_names_it_and_its_goal_s_own_turns_keep_it() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let p = project();
        store.put(&p).unwrap();
        let turn_of = |p: &Project| command_env(&store, p).into_iter().find_map(|e| e.strip_prefix("H2WP_TURN=").map(String::from));
        assert_eq!(turn_of(&p), None, "no owner turn yet");
        let first = new_owner_turn(&store, &p.id).unwrap();
        assert_eq!(turn_of(&p).as_deref(), Some(first.as_str()));
        assert_eq!(turn_of(&p).as_deref(), Some(first.as_str()), "the goal's own turns: the same owner turn");
        let second = new_owner_turn(&store, &p.id).unwrap();
        assert_ne!(first, second, "the owner's next message: a new turn, a new allowance");
        assert_eq!(turn_of(&p), Some(second));
    }
    #[test]
    fn only_the_owner_s_start_over_carries_h2wp_start_over_until_the_run_begins() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        let over = |p: &Project| command_env(&store, p).iter().any(|e| e == "H2WP_START_OVER=1");
        assert!(!has_result(&store, &p), "a first run: nothing to start over");
        write(&store, &p, FINAL, &json!({"schema":"h2wp-result/1","status":"delivered"}));
        assert!(has_result(&store, &p));
        new_run(&store, &mut p, true).unwrap();
        set_start_over(&store, &p, true).unwrap();
        assert!(over(&p), "the owner's Start over, before the plugin begins it");
        let now = (chrono::Utc::now() + chrono::Duration::seconds(1)).to_rfc3339();
        write(&store, &p, PROGRESS, &json!({"schema":"h2wp-progress/1","mode":"flash","stage":null,"label":"","state":"running","startedAt":now,"updatedAt":now}));
        assert!(!over(&p), "once the new run began, never again");
        set_start_over(&store, &p, false).unwrap();
        std::fs::remove_file(store.path(&p.id).unwrap().join(PROGRESS)).unwrap();
        assert!(!over(&p), "any other run: never");
    }
    #[test]
    fn the_change_log_says_how_many_changes_the_last_zip_lacks() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let p = project();
        store.put(&p).unwrap();
        assert_eq!(changes(&store, &p), Value::Null, "no log before the first change");
        let entry = |id: u32, applied: bool| json!({"id":id,"at":"2026-09-25T08:00:00Z","what":"x","files":["style.css"],"applied":applied});
        // The plugin's own count (v1.3, b9bf4fc): sinceZip for the Overview, changedSinceZip for a release.
        let log = json!({"schema":"h2wp-changes/1","changedSinceZip":true,"sinceZip":2,"lastZip":null,"changes":[entry(1, true), entry(2, false), entry(3, true)]});
        write(&store, &p, CHANGES, &log);
        assert_eq!(changes(&store, &p), json!({"count":2,"sinceZip":2,"changedSinceZip":true}), "applied changes only");
        write(&store, &p, CHANGES, &json!({"schema":"h2wp-changes/1","changedSinceZip":false,"sinceZip":1,"changes":[entry(1, true)]}));
        assert_eq!(changes(&store, &p), json!({"count":1,"sinceZip":1,"changedSinceZip":false}), "a change undone: counted, but no new ZIP");
        write(&store, &p, CHANGES, &json!({"schema":"other","sinceZip":4}));
        assert_eq!(changes(&store, &p), Value::Null);
    }
    #[test]
    fn a_release_is_the_next_revision_packaged_not_checked_again() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        let out = store.path(&p.id).unwrap().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("clara-hayes-1.0.0.zip"), "zip").unwrap();
        let mut delivered = fixture("result-delivered.json");
        delivered["theme"]["sha256"] = json!(crate::files::hash(&out.join("clara-hayes-1.0.0.zip")).unwrap());
        delivered["astro"] = Value::Null;
        delivered["report"] = json!({"markdown":"CONVERSION-REPORT.md","pdf":null});
        std::fs::write(out.join("CONVERSION-REPORT.md"), "# Report of revision 1").unwrap();
        write(&store, &p, RESULT, &delivered);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        // package-theme.py after a change: the next ZIP beside the first, result.json names it.
        std::fs::write(out.join("clara-hayes-1.0.0-r2.zip"), "zip, italic heading").unwrap();
        delivered["theme"] = json!({"file":"clara-hayes-1.0.0-r2.zip","sha256":crate::files::hash(&out.join("clara-hayes-1.0.0-r2.zip")).unwrap(),"bytes":19});
        delivered["revision"] = json!(2);
        delivered["checkedRevision"] = json!(1);
        write(&store, &p, RESULT, &delivered);
        let mut p = store.project(&p.id).unwrap();
        assert!(new_result(&store, &p));
        // After a change turn the host finds it the same way.
        assert!(tokio::runtime::Runtime::new().unwrap().block_on(crate::agent::finish_turn(&store, &p.id, |_, _| {})).unwrap());
        p = store.project(&p.id).unwrap();
        assert_eq!((p.revision, p.phase.as_str()), (2, "deliverable_ready"));
        assert_eq!(p.artifacts.iter().map(|a| (a.revision, a.filename.as_str(), a.checks.as_str())).collect::<Vec<_>>(),
            [(1, "clara-hayes-1.0.0.zip", "flash"), (1, "CONVERSION-REPORT.md", "flash"), (2, "clara-hayes-1.0.0-r2.zip", "packaged"), (2, "CONVERSION-REPORT.md", "flash")],
            "the packaged theme is not checked again; the report beside it stays the run's");
        let said = store.messages(&p.id).unwrap().pop().unwrap().text;
        assert_eq!(said, "Release ready: clara-hayes-1.0.0-r2.zip, revision 2. Packaged after changes, not checked again.");
        assert!(!new_result(&store, &p), "delivered once");
    }
    #[test]
    fn an_astro_release_is_the_next_revision_of_the_astro_project() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.target = "astro".into();
        store.put(&p).unwrap();
        let out = store.path(&p.id).unwrap().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("studio-astro-1.0.0.zip"), "astro").unwrap();
        let mut done = fixture("result-delivered.json");
        done["mode"] = json!("astro");
        done["theme"] = Value::Null;
        done["report"] = Value::Null;
        done["astro"] = json!({"file":"studio-astro-1.0.0.zip","sha256":crate::files::hash(&out.join("studio-astro-1.0.0.zip")).unwrap()});
        write(&store, &p, RESULT, &done);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        // package-theme.py for an Astro run (v1.4 §7d): the project as it is now, the next revision.
        std::fs::write(out.join("studio-astro-1.0.0-r2.zip"), "astro, new hero").unwrap();
        done["astro"] = json!({"file":"studio-astro-1.0.0-r2.zip","sha256":crate::files::hash(&out.join("studio-astro-1.0.0-r2.zip")).unwrap()});
        done["revision"] = json!(2);
        write(&store, &p, RESULT, &done);
        let p = store.project(&p.id).unwrap();
        assert!(new_result(&store, &p));
        assert!(tokio::runtime::Runtime::new().unwrap().block_on(crate::agent::finish_turn(&store, &p.id, |_, _| {})).unwrap());
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.artifacts.iter().map(|a| (a.revision, a.kind.as_str(), a.filename.as_str(), a.checks.as_str())).collect::<Vec<_>>(),
            [(1, "astro", "studio-astro-1.0.0.zip", "astro"), (2, "astro", "studio-astro-1.0.0-r2.zip", "packaged")]);
        assert!(store.messages(&p.id).unwrap().pop().unwrap().text.starts_with("Release ready: studio-astro-1.0.0-r2.zip, revision 2."));
    }
    #[test]
    fn the_output_the_plugin_s_own_writer_made_is_delivered() {
        // tests/fixtures/written-out: out/ as the plugin's write-result.py left it.
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        let written = Path::new(env!("CARGO_MANIFEST_DIR")).join("../tests/fixtures/written-out");
        let out = store.path(&p.id).unwrap().join("out");
        std::fs::create_dir_all(&out).unwrap();
        for file in ["result.json", "clara-hayes-1.0.0.zip", "CONVERSION-REPORT.md"] { std::fs::copy(written.join(file), out.join(file)).unwrap(); }
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.artifacts.iter().map(|a| (a.kind.as_str(), a.filename.as_str())).collect::<Vec<_>>(),
            [("theme", "clara-hayes-1.0.0.zip"), ("report", "CONVERSION-REPORT.md")], "the checked theme and the report; no PDF was rendered");
    }
    #[test]
    fn an_astro_run_delivers_the_astro_project() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.target = "astro".into();
        store.put(&p).unwrap();
        let out = store.path(&p.id).unwrap().join("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("clara-hayes-astro-1.0.0.zip"), "astro").unwrap();
        let mut done = fixture("result-delivered.json");
        done["mode"] = json!("astro");
        done["theme"] = Value::Null;
        done["report"] = Value::Null;
        done["astro"]["sha256"] = json!(crate::files::hash(&out.join("clara-hayes-astro-1.0.0.zip")).unwrap());
        write(&store, &p, RESULT, &done);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap(), "no theme is needed from an Astro run");
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.artifacts.iter().map(|a| (a.kind.as_str(), a.checks.as_str())).collect::<Vec<_>>(), [("astro", "astro")]);
        assert!(store.messages(&p.id).unwrap().pop().unwrap().text.starts_with("Your Astro 5 project is ready: clara-hayes-astro-1.0.0.zip."));
    }
    #[test]
    fn a_stopped_result_ends_the_run_with_the_plugin_s_reason() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        write(&store, &p, RESULT, &fixture("result-stopped.json"));
        assert!(!deliver(&store, &mut p, &|_, _| {}).unwrap());
        assert_eq!(failure(&store, &p).as_deref(), Some("The service refused the conversion: the licence has no conversions left. (stage 3)"));
        // Continue: the stop belongs to the attempt that ended.
        let mut resumed = p.clone();
        resume_run(&store, &mut resumed).unwrap();
        assert_eq!((failure(&store, &resumed), resumed.phase.as_str()), (None, "running"));
        write(&store, &p, RESULT, &fixture("result-stopped.json"));
        // Its report still reaches Exports, so the owner can read why.
        std::fs::write(store.path(&p.id).unwrap().join("out/CONVERSION-REPORT.md"), "# Stopped at stage 3").unwrap();
        keep_report(&store, &mut p).unwrap();
        assert_eq!(store.project(&p.id).unwrap().artifacts.iter().map(|a| (a.kind.as_str(), a.filename.as_str())).collect::<Vec<_>>(), [("report", "CONVERSION-REPORT.md")]);
        assert_ne!(store.project(&p.id).unwrap().phase, "deliverable_ready");
    }
    #[test]
    fn nothing_outside_the_project_is_read_or_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        store.put(&p).unwrap();
        let mut result = fixture("result-delivered.json");
        result["theme"] = json!({"file":"../../../private/licence"});
        write(&store, &p, RESULT, &result);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap_err().contains("not a file in /project/out"));
        result["theme"] = json!({"file":"/etc/passwd"});
        write(&store, &p, RESULT, &result);
        assert!(deliver(&store, &mut p, &|_, _| {}).is_err());
        #[cfg(unix)]
        {
            let outside = tempfile::tempdir().unwrap();
            std::fs::write(outside.path().join("secret.txt"), "private").unwrap();
            let root = store.path(&p.id).unwrap();
            std::os::unix::fs::symlink(outside.path().join("secret.txt"), root.join("out/theme.zip")).unwrap();
            result["theme"] = json!({"file":"theme.zip"});
            write(&store, &p, RESULT, &result);
            assert!(deliver(&store, &mut p, &|_, _| {}).is_err(), "a linked output is refused");
            std::fs::remove_file(root.join(PROGRESS)).ok();
            std::fs::create_dir_all(root.join("workspace")).unwrap();
            std::fs::write(outside.path().join("progress.json"), fixture("progress-running.json").to_string()).unwrap();
            std::os::unix::fs::symlink(outside.path().join("progress.json"), root.join(PROGRESS)).unwrap();
            assert_eq!(progress(&store, &p), json!({}), "a linked progress file is not read");
        }
    }
    /// A data folder like the app's own: under ~/Library/Application Support,
    /// with a space in its path, which Docker Desktop shares with its VM.
    fn spaced_dir() -> tempfile::TempDir {
        let base = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/html2wp-desktop-tests");
        std::fs::create_dir_all(&base).unwrap();
        tempfile::Builder::new().prefix("app data ").tempdir_in(base).unwrap()
    }
    /// The plugin's own scripts in the real container, read back by the host
    /// (docker): H2WP_TEST_RUNTIME_IMAGE, an image carrying the gamma plugin.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to a runtime image with the gamma plugin"]
    async fn the_plugin_s_scripts_in_the_container_reach_the_host() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.runtime_image = image;
        p.phase = "running".into();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let sh = |cmd: &str| { let (store, p, cmd) = (&store, &p, cmd.to_string()); async move { exec(store, p, &p.runtime_image, &json!({"cmd":cmd,"timeout":120})).await.unwrap() } };
        let scripts = format!("{SKILL}/assets/scripts");
        // The stages as the agent reports them, with only the container's environment.
        let ran = sh(&format!("{scripts}/progress.sh mode $H2WP_MODE && {scripts}/progress.sh start -3 && {scripts}/progress.sh done -3 && {scripts}/progress.sh start -1 'twelve routes'")).await;
        let seen = progress(&store, &p);
        // The verdict, written last into /project/out.
        let wrote = sh(&format!("cd $H2WP_WORKSPACE && printf '{{\"site\":{{\"name\":\"Site\",\"slug\":\"site\",\"version\":\"1.0.0\"}},\"pages\":[]}}' > conversion-manifest.json && echo zip > site-1.0.0.zip && echo '# Report' > CONVERSION-REPORT.md && python3 {scripts}/write-result.py $H2WP_WORKSPACE --no-pdf")).await;
        // The build sandbox: it bind-mounts copies under TMPDIR through the host's Docker.
        let sandbox = sh(&format!("bash {scripts}/test-build-sandbox.sh 2>&1 | tail -3")).await;
        let delivered = deliver(&store, &mut p, &|_, _| {});
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        assert!(sandbox["output"].as_str().unwrap_or("").contains("ALL OK"), "the plugin's build sandbox works in the app's layout: {}", sandbox["output"]);
        assert_eq!(ran["exitCode"], 0, "{}", ran["output"]);
        assert_eq!((seen["schema"].as_str(), seen["mode"].as_str(), seen["stage"].as_str(), seen["state"].as_str()), (Some("h2wp-progress/1"), Some("flash"), Some("-1"), Some("running")), "{seen}");
        assert_eq!(wrote["exitCode"], 0, "{}", wrote["output"]);
        assert!(delivered.unwrap(), "the host delivers what the plugin wrote in the container");
        let p = store.project(&p.id).unwrap();
        assert_eq!(p.artifacts.iter().map(|a| a.filename.as_str()).collect::<Vec<_>>(), ["site-1.0.0.zip", "CONVERSION-REPORT.md"]);
    }
    /// The plugin's preview WordPress, started from the app's container
    /// through the host's Docker, answers the owner's browser and the agent at
    /// the same address; the app shows, stops and removes it (docker, minutes).
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to a runtime image with the gamma plugin"]
    async fn the_plugin_s_preview_answers_here_and_in_the_container() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.runtime_image = image;
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let sh = |cmd: &str| { let (store, p, cmd) = (&store, &p, cmd.to_string()); async move { exec(store, p, &p.runtime_image, &json!({"cmd":cmd,"timeout":900})).await.unwrap() } };
        let slug = format!("apptest-{}", &p.id[..8]);
        let up = sh(&format!("cd $H2WP_WORKSPACE && {SKILL}/assets/scripts/test-env.sh up {slug}")).await;
        let status = crate::preview::status(&store, &p).await;
        let url = status["url"].as_str().unwrap_or("").to_string();
        let inside = sh(&format!("curl -s -o /dev/null -w '%{{http_code}}' {url}/wp-login.php")).await;
        let host = reqwest::get(format!("{url}/wp-login.php")).await.map(|r| r.status().as_u16()).unwrap_or(0);
        // A browser that has used several localhost sites carries many cookies:
        // WordPress admin must still answer (the owner's run met a 400 here).
        let cookies = |n: usize| (0..n).map(|i| format!("site{i}_session={}", "x".repeat(480))).collect::<Vec<_>>().join("; ");
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let mut admin = vec![];
        for n in [42, 420] {
            let sent = cookies(n);
            let here = client.get(format!("{url}/wp-admin/")).header("cookie", &sent).send().await.map(|r| r.status().as_u16()).unwrap_or(0);
            // In the container the header is made there and read from a file: a 200 KB
            // command or argument would exceed the shell tool's and Linux's limits.
            let inside = sh(&format!("python3 -c 'print(\"Cookie: \" + \"; \".join(\"site%d_session=%s\" % (i, \"x\" * 480) for i in range({n})))' > /tmp/cookie-header && curl -s -o /dev/null -w '%{{http_code}}' -H @/tmp/cookie-header {url}/wp-admin/")).await;
            admin.push((sent.len(), here, inside["output"].as_str().unwrap_or("").to_string()));
        }
        let stopped = crate::preview::action(&store, &p, "stop").await.map(|v| v["running"].clone());
        let started = crate::preview::action(&store, &p, "start").await.map(|v| v["running"].clone());
        let removed = crate::preview::remove(&store, &p).await;
        let left = crate::runtime::docker(&["ps".into(), "-aq".into(), "--filter".into(), format!("label=com.docker.compose.project={}", status["project"].as_str().unwrap_or("none"))], None, 30).await.unwrap_or_default();
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        assert_eq!(up["exitCode"], 0, "{}", up["output"]);
        assert_eq!((status["available"].clone(), status["running"].clone(), status["user"].clone()), (json!(true), json!(true), json!("admin")), "{status}");
        assert!(url.starts_with("http://localhost:"), "{url}");
        assert_eq!(inside["output"], "200", "the agent's browser reaches the same address");
        assert_eq!(host, 200, "the owner's browser reaches it on this computer");
        assert!(admin[0].0 >= 20_000 && admin[1].0 >= 200_000, "{admin:?}");
        assert_eq!(admin.iter().map(|(_, here, inside)| (*here, inside.as_str())).collect::<Vec<_>>(), [(302, "302"), (302, "302")],
            "WordPress admin answers (to its login) with 20 KB and 200 KB of cookies, here and in the container, never 400");
        assert_eq!((stopped.unwrap(), started.unwrap()), (json!(false), json!(true)));
        assert!(removed.is_ok() && left.trim().is_empty(), "Delete removes the preview: {removed:?} {left}");
    }
    /// The owner's Compare button against the real plugin: its comparison of
    /// the source and the preview WordPress, read back by the app (docker, minutes).
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to a runtime image with the gamma plugin"]
    async fn the_plugin_s_comparison_runs_from_the_app() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.runtime_image = image;
        store.put(&p).unwrap();
        let root = store.path(&p.id).unwrap();
        std::fs::create_dir_all(root.join("workspace/static-src")).unwrap();
        std::fs::create_dir_all(root.join("input")).unwrap();
        std::fs::write(root.join("workspace/static-src/index.html"), "<!doctype html><html><head><title>Home</title></head><body><h1>Hello from the source</h1><p>One page.</p></body></html>").unwrap();
        let slug = format!("cmp-{}", &p.id[..8]);
        std::fs::write(root.join("workspace/conversion-manifest.json"), json!({"site":{"name":"Compare test","slug":slug,"version":"1.0.0"},
            "input":{"dir":"/project/workspace/static-src"},"pages":[{"key":"front-page","file":"index.html","title":"Home"}]}).to_string()).unwrap();
        let up = exec(&store, &p, &p.runtime_image, &json!({"cmd":format!("cd $H2WP_WORKSPACE && {SKILL}/assets/scripts/test-env.sh up {slug}"),"timeout":900})).await.unwrap();
        let compared = crate::compare::run(&store, &p, &p.runtime_image, true, None).await;
        let status = crate::compare::status(&store, &p, false);
        let shown = compared.as_ref().ok().and_then(|found| found["pages"][0]["desktop"]["image"].as_str().map(String::from)).map(|path| crate::compare::image(&store, &p, &path));
        let _ = crate::preview::remove(&store, &p).await;
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        assert_eq!(up["exitCode"], 0, "{}", up["output"]);
        let found = compared.unwrap();
        assert_eq!(status["state"], "done", "{status}");
        assert_eq!(found["pages"][0]["key"], "front-page", "{found}");
        assert!(found["pages"][0]["desktop"]["diffPercent"].is_number(), "{found}");
        assert!(shown.unwrap().unwrap().starts_with("data:image/png;base64,"), "the composite reaches the owner's screen");
    }
    /// After delivery, in the real container (docker): a change turn's commands
    /// say H2WP_MODE=change, the plugin refuses every stage start, the change
    /// reaches the change log without touching progress.json, the agent may
    /// not package, and Make release delivers the changed theme as the next
    /// revision, then reuses it.
    /// apply-change.py runs with --skip-install: no preview WordPress here
    /// (the plugin proves the live install itself). H2WP_TEST_RUNTIME_IMAGE:
    /// a runtime image with gamma.2.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to a runtime image with gamma.2"]
    async fn a_change_turn_starts_no_stage_and_make_release_delivers_the_change() {
        use std::io::Read;
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.target = "html".into();
        p.runtime_image = image.clone();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let sh = |p: &Project, cmd: &str| { let (store, p, cmd, image) = (&store, p.clone(), cmd.to_string(), image.clone()); async move { exec(store, &p, &image, &json!({"cmd":cmd,"timeout":300})).await.unwrap() } };
        // A delivered Flash run, as the plugin leaves it (its make-zip.sh, its result).
        let made = sh(&p, &format!(r#"set -e
T=/project/workspace/theme/studio; mkdir -p "$T/clara-content/sources" "$T/templates"
printf '/*\nTheme Name: Studio\nVersion: 1.0.0\n*/\n' > "$T/style.css"; echo '{{"version": 3}}' > "$T/theme.json"; echo '<?php' > "$T/functions.php"
echo '<!-- wp:post-content /-->' > "$T/templates/index.html"; echo '<h1>Studio</h1>' > "$T/clara-content/sources/front-page.html"
python3 -c "from PIL import Image; Image.new('RGB', (1200, 900), (240, 240, 240)).save('$T/screenshot.png')"
echo '{{"schema":"html2wp/1","site":{{"name":"Studio","slug":"studio","version":"1.0.0"}},"workspace":"/project/workspace","pages":[{{"file":"index.html","key":"front-page","kind":"front","title":"Studio","chrome":"consensus"}}]}}' > /project/workspace/conversion-manifest.json
MAKE_ZIP_MANIFEST=/project/workspace/conversion-manifest.json bash {SKILL}/assets/scripts/make-zip.sh "$T" /project/out/studio-1.0.0.zip >/dev/null
SHA=$(sha256sum /project/out/studio-1.0.0.zip | cut -d' ' -f1)
for f in /project/workspace /project/out; do echo "{{\"schema\":\"h2wp-result/1\",\"mode\":\"flash\",\"target\":\"html\",\"status\":\"delivered\",\"revision\":1,\"checkedRevision\":1,\"verdict\":\"Flash: not visually repaired\",\"theme\":{{\"file\":\"studio-1.0.0.zip\",\"sha256\":\"$SHA\"}}}}" > $f/result.json; done
echo '{{"schema":"h2wp-progress/1","mode":"flash","state":"finished","stages":[{{"stage":"7","state":"done"}}]}}' > /project/workspace/progress.json"#)).await;
        assert_eq!(made["exitCode"], 0, "{}", made["output"]);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        let mut p = store.project(&p.id).unwrap();
        assert!(takes_changes(&p));
        let progress_before = std::fs::read(store.path(&p.id).unwrap().join(PROGRESS)).unwrap();
        // The owner's chat message: a change turn.
        set_change_turn(&store, &p, true).unwrap();
        let turn_mode = sh(&p, "echo $H2WP_MODE").await["output"].as_str().unwrap().trim().to_string();
        let guard: Vec<(Value, bool)> = {
            let mut refused = vec![];
            for args in ["start -1", "start 0", "mode flash", "mode full"] {
                let ran = sh(&p, &format!("bash {SKILL}/assets/scripts/progress.sh {args}")).await;
                refused.push((ran["exitCode"].clone(), ran["output"].as_str().unwrap_or_default().contains("apply-change.py")));
            }
            refused
        };
        let edit = sh(&p, "sed -i 's#<h1>Studio</h1>#<h1><em>Studio</em></h1>#' /project/workspace/theme/studio/clara-content/sources/front-page.html").await;
        let applied = sh(&p, &format!("python3 {SKILL}/assets/scripts/apply-change.py /project/workspace --what 'make the heading italic' --skip-install")).await;
        let counted = changes(&store, &p);
        let result_before = std::fs::read(store.path(&p.id).unwrap().join(RESULT)).unwrap();
        let result_write = sh(&p, &format!("python3 {SKILL}/assets/scripts/write-result.py /project/workspace")).await;
        let result_after = std::fs::read(store.path(&p.id).unwrap().join(RESULT)).unwrap();
        let progress_after = std::fs::read(store.path(&p.id).unwrap().join(PROGRESS)).unwrap();
        // The agent may not package in a change; Make release does it (no turn).
        let refused = sh(&p, &format!("python3 {SKILL}/assets/scripts/package-theme.py /project/workspace")).await;
        let no_zip = !store.path(&p.id).unwrap().join("out/studio-1.0.0-r2.zip").exists();
        let first = package(&store, &mut p, &image, &|_, _| {}).await;
        let zip = store.path(&p.id).unwrap().join(format!("artifacts/revision-{}/studio-1.0.0-r2.zip", p.revision));
        let italic = std::fs::File::open(&zip).ok().and_then(|f| zip::ZipArchive::new(f).ok()).and_then(|mut z| {
            let mut body = String::new();
            z.by_name("studio/clara-content/sources/front-page.html").ok()?.read_to_string(&mut body).ok()?;
            Some(body)
        });
        let again = package(&store, &mut p, &image, &|_, _| {}).await;
        let after_zip = changes(&store, &p);
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        assert_eq!(turn_mode, "change", "every command of a change turn");
        assert!(guard.iter().all(|(code, named)| *code == 3 && *named), "the plugin refuses every stage start while delivered: {guard:?}");
        assert_eq!(edit["exitCode"], 0);
        assert_eq!(applied["exitCode"], 0, "{}", applied["output"]);
        assert_eq!(progress_after, progress_before, "no stage started: progress.json untouched");
        assert_eq!(counted, json!({"count":1,"sinceZip":1,"changedSinceZip":true}));
        assert_eq!(result_write["exitCode"], 3, "write-result.py refuses in a change turn: {}", result_write["output"]);
        assert_eq!(result_after, result_before, "the delivered result.json is not rewritten by a change turn");
        assert!(refused["exitCode"] == 3 && refused["output"].as_str().unwrap().contains("Make release") && no_zip, "a change turn never packages: {refused}");
        assert_eq!(first.unwrap(), json!({"changed":true,"revision":2,"filename":"studio-1.0.0-r2.zip"}));
        assert!(italic.is_some_and(|b| b.contains("<em>Studio</em>")), "the new ZIP carries the change");
        assert_eq!(p.artifacts.iter().map(|a| (a.revision, a.filename.as_str(), a.checks.as_str())).collect::<Vec<_>>(),
            [(1, "studio-1.0.0.zip", "flash"), (2, "studio-1.0.0-r2.zip", "packaged")]);
        assert_eq!(again.unwrap(), json!({"changed":false,"revision":2,"filename":"studio-1.0.0-r2.zip"}), "nothing changed: the ZIP already here");
        assert_eq!(after_zip["sinceZip"], 0);
    }
    /// A stopped run in the real container (docker, gamma.4): the owner's chat
    /// is a repair-stop turn (H2WP_MODE=repair-stop, H2WP_TURN), the plugin
    /// refuses a new run over the stopped result, and only the owner's
    /// Start over (H2WP_START_OVER=1) puts it aside. H2WP_TEST_RUNTIME_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to a runtime image with gamma.4"]
    async fn a_stopped_project_s_chat_is_a_repair_stop_never_a_new_run() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.target = "html".into();
        p.phase = "failed".into();
        p.runtime_image = image.clone();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let sh = |p: &Project, cmd: &str| { let (store, p, cmd, image) = (&store, p.clone(), cmd.to_string(), image.clone()); async move { exec(store, &p, &image, &json!({"cmd":cmd,"timeout":300})).await.unwrap() } };
        // The run the plugin stopped: its mode, its progress and its final result.
        let made = sh(&p, r#"set -e
echo flash > /project/workspace/.h2wp-mode
echo '{"schema":"h2wp-progress/1","mode":"flash","stage":"3.5","label":"the theme ZIP","state":"stopped","stages":[{"stage":"3.5","state":"failed"}]}' > /project/workspace/progress.json
for f in /project/workspace /project/out; do echo '{"schema":"h2wp-result/1","mode":"flash","target":"html","status":"stopped","stopped":{"stage":"3.5","reason":"no article layout"},"writtenAt":"2026-09-25T09:00:00.000Z"}' > $f/result.json; done"#).await;
        assert_eq!(made["exitCode"], 0, "{}", made["output"]);
        assert!(stopped(&store, &p));
        // The owner's message.
        repair_stop(&store, &mut p).unwrap();
        store.put(&p).unwrap();
        let turn = new_owner_turn(&store, &p.id).unwrap();
        let env = sh(&p, "echo $H2WP_MODE $H2WP_TURN ${H2WP_START_OVER:-none}").await["output"].as_str().unwrap().trim().to_string();
        let new_run = sh(&p, &format!("bash {SKILL}/assets/scripts/progress.sh mode flash --new")).await;
        let still = final_stopped(&store, &p);
        // The owner's Start over: only it puts the stopped run aside.
        set_change_turn(&store, &p, false).unwrap();
        new_run_for_test(&store, &mut p);
        set_start_over(&store, &p, true).unwrap();
        let over = sh(&p, &format!("bash {SKILL}/assets/scripts/progress.sh mode flash --new")).await;
        let aside = !final_stopped(&store, &p);
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        assert_eq!(env, format!("repair-stop {turn} none"));
        assert!(!repair_stop_text(Some("x")).contains("--new"));
        assert_eq!(new_run["exitCode"], 3, "no new run over the stopped result: {}", new_run["output"]);
        assert!(still, "the stopped result stays");
        assert_eq!(over["exitCode"], 0, "the owner's Start over: {}", over["output"]);
        assert!(aside, "Start over puts the stopped result aside");
    }
    fn new_run_for_test(store: &crate::store::Store, p: &mut Project) { new_run(store, p, true).unwrap(); store.put(p).unwrap(); }
    /// A delivered Astro run in the real container (docker, gamma.4): its
    /// change turn (H2WP_MODE=change) is built by the Astro build alone, Make
    /// release gives the next revision of the Astro project, and Compare puts
    /// the original beside the built site. H2WP_TEST_RUNTIME_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to a runtime image with gamma.4"]
    async fn an_astro_run_changes_releases_and_compares_after_delivery() {
        use std::io::Read;
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.target = "astro".into();
        p.flash = false;
        p.runtime_image = image.clone();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let sh = |p: &Project, cmd: &str| { let (store, p, cmd, image) = (&store, p.clone(), cmd.to_string(), image.clone()); async move { exec(store, &p, &image, &json!({"cmd":cmd,"timeout":600})).await.unwrap() } };
        // A delivered Astro run as the plugin leaves it (its own zip of the project).
        let made = sh(&p, &format!(r#"set -e
A=/project/workspace/astro-project; mkdir -p $A/src/pages /project/workspace/static-src
echo '{{"name":"studio","scripts":{{"build":"node build.cjs"}}}}' > $A/package.json
printf '%s\n' "const fs=require('fs'),path=require('path');fs.rmSync('dist',{{recursive:true,force:true}});for(const f of fs.readdirSync('src/pages')){{fs.mkdirSync('dist',{{recursive:true}});fs.copyFileSync(path.join('src/pages',f),path.join('dist',f));}}" > $A/build.cjs
echo 'export default {{}};' > $A/astro.config.mjs
P='<!doctype html><html><body style="margin:0"><h1 style="font:40px serif">Studio</h1></body></html>'
echo "$P" > $A/src/pages/index.html; echo "$P" > /project/workspace/static-src/index.html; (cd $A && node build.cjs)
echo '{{"schema":"html2wp/1","site":{{"name":"Studio","slug":"studio","version":"1.0.0"}},"pages":[{{"file":"index.html","key":"front-page","kind":"front","title":"Studio","chrome":"consensus"}}]}}' > /project/workspace/conversion-manifest.json
cd {SKILL}/assets/scripts && python3 -c "import sys; from pathlib import Path; sys.path.insert(0,'lib'); import theme_state as ts; ts.zip_astro_project(Path('$A'),Path('/project/out/studio-astro-1.0.0.zip'),'studio-astro')"
SHA=$(sha256sum /project/out/studio-astro-1.0.0.zip | cut -d' ' -f1)
for f in /project/workspace /project/out; do echo "{{\"schema\":\"h2wp-result/1\",\"mode\":\"astro\",\"target\":\"astro\",\"status\":\"delivered\",\"revision\":1,\"theme\":null,\"astro\":{{\"file\":\"studio-astro-1.0.0.zip\",\"sha256\":\"$SHA\"}}}}" > $f/result.json; done
echo '{{"schema":"h2wp-progress/1","mode":"astro","state":"finished","stages":[{{"stage":"7","state":"done"}}]}}' > /project/workspace/progress.json"#)).await;
        assert_eq!(made["exitCode"], 0, "{}", made["output"]);
        assert!(deliver(&store, &mut p, &|_, _| {}).unwrap());
        let mut p = store.project(&p.id).unwrap();
        assert!(takes_changes(&p));
        // The owner's change turn.
        set_change_turn(&store, &p, true).unwrap();
        let turn_mode = sh(&p, "echo $H2WP_MODE $H2WP_TARGET").await["output"].as_str().unwrap().trim().to_string();
        let edit = sh(&p, "sed -i 's#Studio</h1>#<em>Studio</em></h1>#' /project/workspace/astro-project/src/pages/index.html").await;
        let applied = sh(&p, &format!("python3 {SKILL}/assets/scripts/apply-change.py /project/workspace --what 'italic heading'")).await;
        let refused = sh(&p, &format!("python3 {SKILL}/assets/scripts/package-theme.py /project/workspace")).await;
        let counted = changes(&store, &p);
        // Make release, then Compare, as the buttons run them.
        let first = package(&store, &mut p, &image, &|_, _| {}).await;
        let released = store.path(&p.id).unwrap().join(format!("artifacts/revision-{}/studio-astro-1.0.0-r2.zip", p.revision));
        let italic = std::fs::File::open(&released).ok().and_then(|f| zip::ZipArchive::new(f).ok()).and_then(|mut z| {
            let mut body = String::new();
            z.by_name("studio-astro/dist/index.html").ok()?.read_to_string(&mut body).ok()?;
            Some(body)
        });
        let again = package(&store, &mut p, &image, &|_, _| {}).await;
        let compared = crate::compare::run(&store, &p, &image, true, None).await;
        let index = crate::compare::index(&store, &p);
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        assert_eq!(turn_mode, "change astro");
        assert_eq!(edit["exitCode"], 0);
        assert_eq!(applied["exitCode"], 0, "{}", applied["output"]);
        assert!(refused["exitCode"] == 3 && refused["output"].as_str().unwrap().contains("Make release"), "a change turn never packages: {refused}");
        assert_eq!(counted["sinceZip"], 1, "{counted}");
        assert_eq!(first.unwrap(), json!({"changed":true,"revision":2,"filename":"studio-astro-1.0.0-r2.zip"}));
        assert!(italic.is_some_and(|b| b.contains("<em>Studio</em>")), "the release carries the rebuilt site");
        assert_eq!(again.unwrap()["changed"], false);
        assert!(compared.is_ok(), "{compared:?}");
        assert_eq!(index["pages"][0]["key"], "front-page", "{index}");
        assert!(index["pages"][0]["desktop"]["diffPercent"].is_number(), "{index}");
    }
    /// A project container made from an older runtime is made again from the
    /// current one (the project and its files stay); one on the current
    /// runtime is kept (docker): H2WP_TEST_OLD_RUNTIME_IMAGE and H2WP_TEST_RUNTIME_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_OLD_RUNTIME_IMAGE and H2WP_TEST_RUNTIME_IMAGE to two runtime images"]
    async fn a_container_on_an_older_runtime_is_replaced_and_a_current_one_kept() {
        let (Ok(old), Ok(current)) = (std::env::var("H2WP_TEST_OLD_RUNTIME_IMAGE"), std::env::var("H2WP_TEST_RUNTIME_IMAGE")) else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        store.put(&p).unwrap();
        let root = store.path(&p.id).unwrap();
        std::fs::create_dir_all(root.join("input")).unwrap();
        let name = container_name(&p.id).unwrap();
        let id = |name: String| async move { crate::runtime::docker(&["inspect".into(), "--format".into(), "{{.Id}}".into(), name], None, 15).await.unwrap().trim().to_string() };
        // Made by an earlier release, on its runtime, with work in the project.
        ensure_container(&store, &p, &old).await.unwrap();
        let first = id(name.clone()).await;
        exec(&store, &p, &old, &json!({"cmd":"echo kept > /project/workspace/work.txt"})).await.unwrap();
        // This release: the container follows the installed runtime.
        let replaced = ensure_container(&store, &p, &current).await;
        let second = id(name.clone()).await;
        let made_from = crate::runtime::container_image(&name).await;
        let wanted = crate::runtime::image_id(&current).await;
        let work = exec(&store, &p, &current, &json!({"cmd":"cat /project/workspace/work.txt"})).await.unwrap();
        let again = { ensure_container(&store, &p, &current).await.unwrap(); id(name.clone()).await };
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), name], None, 60).await;
        replaced.unwrap();
        assert_ne!(first, second, "the container on the older runtime was made again");
        assert_eq!(made_from, wanted, "from the current runtime");
        assert_eq!(work["output"], "kept\n", "the project's work stays");
        assert_eq!(again, second, "a container on the current runtime is kept");
    }
    /// A project whose recorded runtime image was removed, started by another
    /// plugin commit, moves to the installed runtime and runs its commands
    /// there (docker): H2WP_TEST_RUNTIME_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to the runtime image"]
    async fn a_project_whose_image_was_removed_runs_on_the_installed_runtime() {
        let Ok(current) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.runtime_image = format!("sha256:{}", "0".repeat(64));
        p.plugin_commit = "fc289da".into();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let name = container_name(&p.id).unwrap();
        assert!(crate::runtime::image_id(&p.runtime_image).await.is_none(), "the recorded image is not here");
        let moved = crate::runtime::adopt_current(&mut p, &current, "55c478ca689c").await;
        let ran = exec(&store, &p, &p.runtime_image, &json!({"cmd":"test -f /opt/html2wp/skills/html2wp/assets/scripts/visual-compare.py && echo plugin"})).await;
        let made_from = crate::runtime::container_image(&name).await;
        let wanted = crate::runtime::image_id(&current).await;
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), name], None, 60).await;
        assert!(moved, "the project moved to the installed runtime");
        assert_eq!((p.runtime_image.as_str(), p.plugin_commit.as_str()), (current.as_str(), "55c478ca689c"));
        assert_eq!(ran.unwrap()["output"], "plugin\n", "its commands reach the installed plugin");
        assert_eq!(made_from, wanted, "its container is on the installed runtime");
    }
    /// The container against the real image (docker): H2WP_TEST_RUNTIME_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to the built runtime image"]
    async fn the_real_container_runs_the_shell_in_the_project() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let dir = spaced_dir();
        let store = crate::store::Store::open(dir.path().to_path_buf()).unwrap();
        crate::plugin::install_vendored(&store);
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.runtime_image = image;
        store.put(&p).unwrap();
        let root = store.path(&p.id).unwrap();
        std::fs::create_dir_all(root.join("input")).unwrap();
        std::fs::write(root.join("input/index.html"), "<p>original</p>").unwrap();
        crate::store::write_private(&store.root.join("private/licence"), b"H2WP-TEST-KEY").unwrap();
        let sh = |cmd: &str| { let (store, p, cmd) = (&store, &p, cmd.to_string()); async move { exec(store, p, &p.runtime_image, &json!({"cmd":cmd})).await.unwrap() } };
        let ok = |v: &Value| v["exitCode"] == 0;
        let results = (
            sh("pwd -P && readlink /project").await["output"].as_str().unwrap().trim().to_string(),
            ok(&sh("cat /project/input/index.html").await),
            ok(&sh("touch /project/input/x").await),
            ok(&sh("echo built > /project/source/index.html && echo out > /project/out/x.txt && touch /project/.tmp/t").await),
            ok(&sh(&format!("test -f {SKILL}/SKILL.md && test -f {PLUGIN}/VERSION")).await),
            sh("stat -c %a ~/.config/html2wp/licence && cat ~/.config/html2wp/licence").await["output"].as_str().unwrap().to_string(),
            sh("echo $H2WP_MODE $H2WP_TARGET $H2WP_WORKSPACE $H2WP_OUTPUT_DIR $H2WP_HOST $H2WP_STRICT_JOBS $TMPDIR").await["output"].as_str().unwrap().trim().to_string(),
            ok(&sh("ls /home/agent/.codex/auth.json").await),
            ok(&sh("curl -sfI https://wordpress.org -o /dev/null").await),
            ok(&sh("docker ps -q >/dev/null && docker compose version").await),
        );
        std::fs::remove_file(store.root.join("private/licence")).unwrap();
        let free = ok(&sh("test -e ~/.config/html2wp/licence").await);
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), container_name(&p.id).unwrap()], None, 60).await;
        let host = root.display().to_string();
        assert_eq!(results.0, format!("{host}\n{host}"), "commands start in /project, the project's own path");
        assert!(results.1 && !results.2, "the owner's original is readable and read-only");
        assert!(results.3 && std::fs::read_to_string(root.join("source/index.html")).unwrap() == "built\n" && root.join("out/x.txt").is_file(), "the work lands in the project");
        assert!(results.4, "the plugin's skill is in the image");
        assert_eq!(results.5, "600\nH2WP-TEST-KEY", "the owner's licence, private to the agent user");
        assert_eq!(results.6, format!("flash html /project/workspace /project/out codex 1 {}/.tmp", root.display()));
        assert!(!results.7, "no Codex account");
        assert!(results.8, "the network is available");
        assert!(results.9, "the host's Docker and compose for the plugin's preview and sandbox");
        assert!(!free, "Free removes the licence");
    }
}
