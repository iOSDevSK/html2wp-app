//! A project's run with the assistant. The conversion itself belongs to a
//! skill: the html2wp plugin for a site (crate::skill), html2wp-to-gutenberg
//! for Gutenberg from an HTML theme (crate::h2g). The host starts the run,
//! gives the assistant a shell confined to the project, relays the chat and
//! the skill's progress, and packages what the skill delivered. It never
//! sequences conversion steps or judges their results.
use crate::{model::*, store::Store, AppState};
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter};

/// The tools a project's thread gets (fixed when its thread starts).
pub fn tool_specs(p: &Project) -> Value {
    let all: Vec<Value> = serde_json::from_str(include_str!("../../runtime/tools.json")).expect("bundled tools");
    let names: &[&str] = if p.from_theme() { &["report_progress", "sandbox_exec"] } else { &["project_shell"] };
    Value::Array(all.into_iter().filter(|t| t["name"].as_str().is_some_and(|n| names.contains(&n))).collect())
}

/// The developer instructions of a project's thread.
pub fn instructions(p: &Project) -> String {
    if p.from_theme() { crate::h2g::instructions(p) } else { crate::skill::instructions(p) }
}

/// What Codex keeps working towards in a goal run; only the host decides it
/// is done. Set when the owner starts or continues the run.
pub fn goal_objective(store: &Store, p: &Project) -> String {
    if p.from_theme() { return crate::h2g::goal(p); }
    store.setting(&format!("goal:{}", p.id)).filter(|g| !g.is_empty()).unwrap_or_else(|| crate::skill::goal(p))
}
/// The goal's objective once the plugin began this run: the Continue text in
/// place of the new-run goal (which says "--new"), so no turn after the first
/// can start the run over. None while the goal already says continue.
pub fn continuing_objective(store: &Store, p: &Project) -> Option<String> {
    if p.from_theme() { return None; }
    let now = goal_objective(store, p);
    let next = crate::skill::continue_text(store, p);
    (now.contains("--new") && !next.contains("--new")).then_some(next)
}
/// The objective of a goal the host resumes: continue where the skill's
/// progress stands, never start the run over.
pub fn resume_objective(store: &Store, p: &Project) -> String {
    if p.from_theme() { return format!("{}\n\n{}", crate::h2g::goal(p), resume_note(store, p)); }
    // A repair-stop goes on as the repair-stop it is, never as a Continue of a run.
    if crate::skill::repair_stop_turn(store, p) || crate::skill::repair_delivery_turn(store, p) { return goal_objective(store, p); }
    crate::skill::continue_text(store, p)
}

/// A tool call of the project's assistant.
pub async fn tool(app: &AppHandle, state: &AppState, pid: &str, name: &str, args: &Value) -> Result<Value> {
    let p = state.store.project(pid)?;
    match (p.from_theme(), name) {
        // Gutenberg from an HTML theme: the workflow step the agent starts
        // (a line in the chat), and its commands in the project's sandbox.
        (true, "report_progress") => {
            let result = crate::h2g::report(&state.store, &p, args)?;
            let line = crate::h2g::step_line(&crate::h2g::progress(&state.store, &p.id), result["step"].as_u64().unwrap_or(1), args["note"].as_str().unwrap_or("").trim());
            if let Ok(message) = state.store.message(&p.id, "assistant", &line) { let _ = app.emit("chat-message", &message); }
            let _ = app.emit("project-updated", &p);
            Ok(result)
        }
        // Every command runs on the installed runtime, whatever an older container was made from.
        (true, "sandbox_exec") => crate::h2g::exec(&state.store, &p, &state.image(), args).await,
        (false, "project_shell") => crate::skill::exec(&state.store, &p, &state.image(), args).await,
        _ => Err("Unknown project tool".into()),
    }
}

/// After a turn: has the skill delivered? Packages the result into Exports
/// once, and answers true while the current revision is delivered.
pub async fn finish_turn(store: &Store, pid: &str, emit: impl Fn(&str, Value) + Send + Sync) -> Result<bool> {
    let mut p = store.project(pid)?;
    if p.from_theme() { return if p.phase == "deliverable_ready" { Ok(true) } else { crate::h2g::deliver(store, &mut p, &emit) }; }
    // After delivery only a release (the plugin's package-theme) writes a new
    // result: the packaged theme is the next revision.
    if p.phase == "deliverable_ready" {
        if crate::skill::new_result(store, &p) {
            if crate::skill::repair_delivery_turn(store, &p) { crate::skill::deliver(store, &mut p, &emit)?; }
            else { crate::skill::deliver_packaged(store, &mut p, &emit)?; }
        }
        return Ok(true);
    }
    crate::skill::deliver(store, &mut p, &emit)
}

/// The run's own verdict that it cannot go on (the skill said so in its
/// progress); a goal is not resumed over it.
pub fn run_failed(store: &Store, p: &Project) -> Option<String> {
    if p.from_theme() { None } else { crate::skill::failure(store, p) }
}

/// What the skill has reported so far, for the progress signature.
fn progress(store: &Store, p: &Project) -> String {
    if p.from_theme() { crate::h2g::progress(store, &p.id).to_string() } else { crate::skill::progress(store, p).to_string() }
}

/// The chat line when a run ended without its result: where it got to.
pub fn stopped_message(store: &Store, p: &Project, reason: Option<&str>) -> Option<String> {
    if p.phase == "deliverable_ready" { return None; }
    Some(if p.from_theme() { crate::h2g::stop_message(&crate::h2g::progress(store, &p.id), reason) } else { crate::skill::stop_message(store, p, reason) })
}

/// Resumes in a row WITHOUT any change in the project's progress before the
/// host stops the run (the owner: no endless loops). Any progress resets the count.
pub const GOAL_REACTIVATION_LIMIT: u32 = 2;

/// Gutenberg from an HTML theme: where its resumed goal continues.
fn resume_note(store: &Store, p: &Project) -> String {
    let step = crate::h2g::progress(store, &p.id)["current"].as_u64().filter(|n| (1..=9).contains(n));
    match step {
        Some(n) => format!("Resume at workflow step {n} ({}) as you last reported it; do not redo finished steps.", crate::h2g::STEPS[n as usize - 1]),
        None => "Resume where /work shows the conversion stands; do not redo finished steps.".into(),
    }
}

/// The run's state as far as the host can see it: a resumed goal that
/// changes nothing here is standing still.
pub fn progress_signature(store: &Store, p: &Project) -> String {
    let mut snapshot: Value = serde_json::from_str(&progress(store, p)).unwrap_or(Value::Null);
    if let Some(obj) = snapshot.as_object_mut() {
        for key in ["updatedAt", "note", "label", "next", "preview"] { obj.remove(key); }
        if let Some(stages) = obj.get_mut("stages").and_then(Value::as_array_mut) {
            for stage in stages { if let Some(row) = stage.as_object_mut() { row.remove("note"); } }
        }
    }
    format!("{}|{}|{}", p.phase, p.revision, snapshot)
}

/// What the host does after a goal turn ended without the skill's result.
#[derive(Debug, PartialEq)]
pub enum GoalFollowup { KeepRunning, Reactivate(String), Stop(String) }

pub fn goal_followup(status: &str, reactivations: u32, failed: Option<&str>) -> GoalFollowup {
    if let Some(reason) = failed { return GoalFollowup::Stop(format!("The conversion stopped: {reason}")); }
    match status {
        "active" => GoalFollowup::KeepRunning,
        "complete" | "blocked" if reactivations < GOAL_REACTIVATION_LIMIT => GoalFollowup::Reactivate(format!(
            "The previous turn ended as {status}, but the host has no result from the skill yet.")),
        "complete" | "blocked" => GoalFollowup::Stop(format!(
            "The run stopped: {GOAL_REACTIVATION_LIMIT} resumed attempts in a row changed nothing in the project. Choose Continue to try again.")),
        "usageLimited" => GoalFollowup::Stop("The run paused: Codex usage is limited right now. Choose Continue when usage is available again.".into()),
        "budgetLimited" => GoalFollowup::Stop("The run paused: the goal's token budget is used up. Choose Continue to resume.".into()),
        _ => GoalFollowup::Stop(format!("The run paused (goal {status}). Choose Continue to resume.")),
    }
}

/// Run a bash command in a project container, with `env` added to the
/// container's own: its exit code and the tail of its output. `seconds`
/// bounds the command; background work outlives it.
pub async fn exec(container: &str, cwd: &str, env: &[String], cmd: &str, seconds: u64) -> Result<Value> {
    let mut command = crate::runtime::docker_command()?;
    command.args(["exec", "-i", "--user", "1000:1000", "--workdir", cwd]);
    for var in env { command.args(["--env", var]); }
    command.args([container, "bash", "-lc", "exec 2>&1; eval \"$1\"", "project_shell", cmd])
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    let child = command.spawn().map_err(err)?;
    let finished = tokio::time::timeout(std::time::Duration::from_secs(seconds), child.wait_with_output()).await;
    let Ok(output) = finished else {
        return Ok(json!({"exitCode":null,"timedOut":true,"output":format!("The command ran past {seconds} s and was stopped. Run long work in the background (setsid nohup … &) and poll it.")}));
    };
    let output = output.map_err(err)?;
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    let truncated = text.len() > EXEC_OUTPUT;
    if truncated { let cut = text.len() - EXEC_OUTPUT; text = text[text.char_indices().map(|(i, _)| i).find(|&i| i >= cut).unwrap_or(cut)..].to_string(); }
    Ok(json!({"exitCode":output.status.code(),"output":text,"truncated":truncated}))
}
/// Longest single command, and how much of its output comes back.
pub const EXEC_MAX_SECONDS: u64 = 3600;
const EXEC_OUTPUT: usize = 60_000;
/// A command's arguments: the command, its folder (checked by `allowed`) and its time limit.
pub fn exec_args<'a>(args: &'a Value, default_cwd: &str, allowed: &[&str]) -> Result<(&'a str, String, u64)> {
    let cmd = args["cmd"].as_str().filter(|c| !c.trim().is_empty() && c.len() <= 100_000).ok_or("The shell needs cmd (a bash command, up to 100 kB)")?;
    let cwd = args["cwd"].as_str().unwrap_or(default_cwd).trim_end_matches('/');
    let cwd = if cwd.is_empty() { "/" } else { cwd };
    let inside = |root: &&str| cwd == *root || cwd.starts_with(&format!("{root}/"));
    if cwd.split('/').any(|part| part == "..") || !allowed.iter().any(|root| inside(root)) {
        return Err(format!("Commands run under {}; {cwd} is outside", allowed.join(", ")));
    }
    Ok((cmd, cwd.to_string(), args["timeout"].as_u64().unwrap_or(900).clamp(1, EXEC_MAX_SECONDS)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_goal_run_resumes_until_the_skill_delivers_and_stops_when_it_stands_still() {
        assert_eq!(goal_followup("active", 0, None), GoalFollowup::KeepRunning);
        assert!(matches!(goal_followup("complete", 0, None), GoalFollowup::Reactivate(_)), "complete without a result is not the end");
        assert!(matches!(goal_followup("blocked", 1, None), GoalFollowup::Reactivate(_)));
        assert_eq!(GOAL_REACTIVATION_LIMIT, 2, "the owner: no endless loops");
        assert!(matches!(goal_followup("complete", 2, None), GoalFollowup::Stop(m) if m.contains("changed nothing")));
        assert!(matches!(goal_followup("usageLimited", 0, None), GoalFollowup::Stop(m) if m.contains("usage")));
        assert!(matches!(goal_followup("budgetLimited", 0, None), GoalFollowup::Stop(m) if m.contains("budget")));
        // The skill's own verdict ends the run, whatever the goal says.
        assert!(matches!(goal_followup("active", 0, Some("the input has no pages")), GoalFollowup::Stop(m) if m.contains("no pages")));
    }
    #[test]
    fn a_resumed_goal_continues_at_the_recorded_stage_never_from_the_start() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let p = crate::skill::tests::project();
        let progress = store.path(&p.id).unwrap().join(crate::skill::PROGRESS);
        std::fs::create_dir_all(progress.parent().unwrap()).unwrap();
        std::fs::write(&progress, crate::skill::tests::fixture("progress-running.json").to_string()).unwrap();
        assert!(resume_objective(&store, &p).contains("Resume at stage 3 (the service builds the theme)") && resume_objective(&store, &p).contains("do not run progress.sh mode again"));
        // The objective the owner's start or Continue chose is the goal's.
        assert_eq!(goal_objective(&store, &p), crate::skill::start_text(&p));
        store.set(&format!("goal:{}", p.id), "the Continue goal").unwrap();
        assert_eq!(goal_objective(&store, &p), "the Continue goal");
    }
    #[test]
    fn once_the_plugin_began_the_run_no_goal_or_resume_says_new_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = crate::skill::tests::project();
        store.put(&p).unwrap();
        let progress = store.path(&p.id).unwrap().join(crate::skill::PROGRESS);
        std::fs::create_dir_all(progress.parent().unwrap()).unwrap();
        // The owner's Flash: the goal is the new-run text until the plugin begins.
        crate::skill::new_run(&store, &mut p, true).unwrap();
        store.set(&format!("goal:{}", p.id), &crate::skill::start_text(&p)).unwrap();
        std::fs::write(&progress, json!({"schema":"h2wp-progress/1","mode":"flash","stage":"7","label":"deliver","state":"finished","startedAt":"2020-01-01T00:00:00Z","updatedAt":"2020-01-01T00:00:00Z"}).to_string()).unwrap();
        assert_eq!(continuing_objective(&store, &p), None, "an earlier run's progress: this run has not begun");
        assert!(crate::skill::continue_text(&store, &p).contains("--new"), "nothing of this run to resume: its own goal");
        // The plugin began this run: from now on the goal says continue.
        let now = chrono::Utc::now().to_rfc3339();
        std::fs::write(&progress, json!({"schema":"h2wp-progress/1","mode":"flash","stage":null,"label":"","state":"running","startedAt":now,"updatedAt":now}).to_string()).unwrap();
        let next = continuing_objective(&store, &p).expect("the new-run goal is replaced");
        assert!(next.starts_with("Continue the html2wp run") && next.contains("do not run progress.sh mode again") && !next.contains("--new"), "{next}");
        std::fs::write(&progress, json!({"schema":"h2wp-progress/1","mode":"flash","stage":"-1","label":"prepare the input","state":"stopped","note":"no DATABASE_URL","startedAt":now,"updatedAt":now}).to_string()).unwrap();
        assert!(resume_objective(&store, &p).contains("Resume at stage -1 (prepare the input)"), "a stopped run resumes at the stage that stopped it");
        store.set(&format!("goal:{}", p.id), &next).unwrap();
        assert_eq!(continuing_objective(&store, &p), None, "already continuing");
        for text in [resume_objective(&store, &p), crate::skill::continue_text(&store, &p)] { assert!(!text.contains("new run") && !text.contains("--new"), "{text}"); }
    }
    #[test]
    fn each_project_type_gets_only_its_own_tools() {
        let mut p = crate::skill::tests::project();
        let names = |p: &Project| tool_specs(p).as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap().to_string()).collect::<Vec<_>>();
        assert_eq!(names(&p), ["project_shell"]);
        p.target = crate::h2g::TARGET.into();
        assert_eq!(names(&p), ["report_progress", "sandbox_exec"]);
    }
    #[test]
    fn a_command_runs_only_in_the_folders_it_is_given() {
        let allowed = ["/work", "/input", "/tmp"];
        let ok = |cwd: &str| exec_args(&json!({"cmd":"ls","cwd":cwd}), "/work", &allowed).map(|(_, c, _)| c);
        for cwd in ["/work", "/work/theme", "/input", "/tmp"] { assert_eq!(ok(cwd).unwrap(), cwd); }
        for cwd in ["/", "/home/agent/.codex", "/work/../etc", "/workspace", "/opt"] { assert!(ok(cwd).is_err(), "{cwd}"); }
        assert_eq!(exec_args(&json!({"cmd":"ls"}), "/work", &allowed).unwrap(), ("ls", "/work".to_string(), 900));
        assert_eq!(exec_args(&json!({"cmd":"ls","timeout":99999}), "/work", &allowed).unwrap().2, EXEC_MAX_SECONDS);
        assert!(exec_args(&json!({"cmd":"  "}), "/work", &allowed).is_err());
    }
}
