//! The Gutenberg theme of a "Gutenberg from an HTML theme" project, on this
//! Mac. The skill's own WordPress (scripts/wp-sandbox: PHP's built-in server
//! on SQLite, the converted block theme and its imported content) answers
//! only inside the project's sandbox container, on the address its
//! wp-config names (http://127.0.0.1:<port>). The app listens on that same
//! address here and carries each connection into the container (docker exec,
//! a byte pipe to 127.0.0.1:<port> there), so WordPress's own links work
//! unchanged and nothing is published. One such preview at a time.
use crate::{model::*, store::Store};
use serde_json::{json, Value};
use std::{path::Path, sync::Mutex};
use tokio::io::AsyncWriteExt;

/// The skill's sandbox login (wp-sandbox/setup.sh: user admin, ADMIN_PASSWORD's default).
pub const USER: &str = "admin";
pub const PASSWORD: &str = "admin-password";
/// In the container: one TCP connection to the sandbox's server, carried on stdin/stdout.
const BRIDGE: &str = "import os,socket,sys,threading\ns=socket.create_connection(('127.0.0.1',int(sys.argv[1])))\ndef up():\n    while True:\n        d=os.read(0,65536)\n        if not d: break\n        s.sendall(d)\n    try: s.shutdown(socket.SHUT_WR)\n    except OSError: pass\nthreading.Thread(target=up,daemon=True).start()\nwhile True:\n    d=s.recv(65536)\n    if not d: break\n    os.write(1,d)\n";

struct Running { project_id: String, port: u16, task: tokio::task::JoinHandle<()> }
static SERVER: Mutex<Option<Running>> = Mutex::new(None);

/// The sandbox the skill built under /work: its WordPress folder (as the
/// container sees it) and the port its wp-config's WP_HOME names.
pub fn sandbox(store: &Store, p: &Project) -> Option<(String, u16)> {
    let work = crate::h2g::work_dir(store, &p.id).ok()?;
    let found = walkdir::WalkDir::new(&work).max_depth(4).follow_links(false).into_iter().flatten()
        .find(|e| e.file_name() == "wp-config.php" && e.path().parent().is_some_and(|d| d.file_name().is_some_and(|n| n == "wordpress"))
            && crate::h2g::under_work(&work, e.path(), false))?;
    let config = std::fs::read_to_string(found.path()).ok()?;
    let port = wp_home_port(&config)?;
    let wordpress = found.path().parent()?.strip_prefix(&work).ok()?;
    let inside = Path::new("/work").join(wordpress).to_string_lossy().into_owned();
    inside.chars().all(|c| c.is_ascii_alphanumeric() || "/-_.".contains(c)).then_some((inside, port))
}
/// The port of wp-config's `define( 'WP_HOME', 'http://127.0.0.1:<port>' )`.
pub fn wp_home_port(config: &str) -> Option<u16> {
    let at = config.find("'WP_HOME'")?;
    let rest = &config[at..];
    let url = rest.split('\'').nth(3)?;
    url.strip_prefix("http://127.0.0.1:")?.trim_end_matches('/').parse::<u16>().ok().filter(|p| *p >= 1024)
}
fn running_for(pid: &str) -> Option<u16> {
    SERVER.lock().ok()?.as_ref().filter(|r| r.project_id == pid && !r.task.is_finished()).map(|r| r.port)
}
/// The Preview tab: the address and login, and whether it answers here.
pub async fn status(store: &Store, p: &Project) -> Value {
    let Some((_, port)) = sandbox(store, p) else { return json!({"available":false}) };
    json!({"available":true,"url":format!("http://127.0.0.1:{port}"),"user":USER,"password":PASSWORD,"running":running_for(&p.id) == Some(port)})
}
/// The sandbox's server answers in the container: `curl` there, or the skill's own `php -S` started.
pub(crate) async fn serve_inside(name: &str, wordpress: &str, port: u16) -> Result<()> {
    let probe = format!("curl -s -m 3 -o /dev/null -w '%{{http_code}}' http://127.0.0.1:{port}/wp-login.php");
    let answers = |out: &Value| out["output"].as_str().is_some_and(|c| c.trim().starts_with(['2', '3']));
    if answers(&crate::agent::exec(name, "/work", &[], &probe, 15).await?) { return Ok(()); }
    // The skill's own command (references/verification.md), in the background.
    let serve = format!("cd {wordpress} && PHP_CLI_SERVER_WORKERS=8 setsid nohup php -S 127.0.0.1:{port} -t {wordpress} >/tmp/h2g-preview.log 2>&1 &");
    crate::agent::exec(name, "/work", &[], &serve, 15).await?;
    for _ in 0..20 {
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        if answers(&crate::agent::exec(name, "/work", &[], &probe, 15).await?) { return Ok(()); }
    }
    Err("The Gutenberg sandbox's WordPress did not answer. Ask the assistant to start it (scripts/wp-sandbox).".into())
}
/// Start (or keep) the preview of this project; another h2g preview ends.
pub async fn start(store: &Store, p: &Project, image: &str) -> Result<Value> {
    let (wordpress, port) = sandbox(store, p).ok_or("The Gutenberg sandbox is not set up yet: the conversion builds it at its Verify step.")?;
    let name = crate::h2g::ensure_sandbox(store, p, image).await?;
    serve_inside(&name, &wordpress, port).await?;
    if running_for(&p.id) != Some(port) {
        stop_all();
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", port)).await
            .map_err(|_| format!("Port {port} on this Mac is in use by another program; this sandbox's WordPress is set up for it. Close that program and start the preview again."))?;
        let task = tokio::spawn(accept(listener, name, port));
        if let Ok(mut server) = SERVER.lock() { *server = Some(Running { project_id: p.id.clone(), port, task }); }
    }
    Ok(status(store, p).await)
}
async fn accept(listener: tokio::net::TcpListener, name: String, port: u16) {
    while let Ok((socket, _)) = listener.accept().await {
        let name = name.clone();
        tokio::spawn(async move { let _ = carry(socket, &name, port).await; });
    }
}
/// One connection from this Mac to the sandbox's server, through the container.
async fn carry(socket: tokio::net::TcpStream, name: &str, port: u16) -> Result<()> {
    let mut command = crate::runtime::docker_command()?;
    command.args(["exec", "-i", "--user", "1000:1000", name, "python3", "-u", "-c", BRIDGE, &port.to_string()])
        .stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).kill_on_drop(true);
    let mut child = command.spawn().map_err(err)?;
    let (mut stdin, mut stdout) = (child.stdin.take().ok_or("no stdin")?, child.stdout.take().ok_or("no stdout")?);
    let (mut read, mut write) = socket.into_split();
    let up = async { let _ = tokio::io::copy(&mut read, &mut stdin).await; let _ = stdin.shutdown().await; };
    let down = async { let _ = tokio::io::copy(&mut stdout, &mut write).await; let _ = write.shutdown().await; };
    tokio::join!(up, down);
    let _ = child.kill().await;
    Ok(())
}
/// The preview here ends; the sandbox's server in the container goes on for the conversion.
pub fn stop_all() {
    if let Ok(mut server) = SERVER.lock() { if let Some(r) = server.take() { r.task.abort(); } }
}
/// Stop this project's preview (its port here closes).
pub fn stop(pid: &str) {
    if running_for(pid).is_some() { stop_all(); }
}
/// The page's address in this project's preview, when it runs here.
pub fn url(store: &Store, p: &Project, page: &str) -> Result<String> {
    let port = running_for(&p.id).ok_or("Start the preview first.")?;
    let _ = sandbox(store, p).filter(|(_, at)| *at == port).ok_or("Start the preview first.")?;
    Ok(match page { "admin" => format!("http://127.0.0.1:{port}/wp-admin/"), _ => format!("http://127.0.0.1:{port}/") })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn project() -> Project { let mut p = crate::skill::tests::project(); p.target = crate::h2g::TARGET.into(); p }
    #[test]
    fn the_port_is_the_one_wp_config_names() {
        assert_eq!(wp_home_port("<?php\ndefine( 'WP_HOME', 'http://127.0.0.1:8899' );\ndefine( 'WP_SITEURL', 'http://127.0.0.1:8899' );"), Some(8899));
        assert_eq!(wp_home_port("define('WP_HOME','http://127.0.0.1:9123/');"), Some(9123));
        for other in ["define( 'WP_HOME', 'http://example.com:8899' );", "define( 'WP_HOME', 'http://127.0.0.1:80' );", "no home here"] { assert_eq!(wp_home_port(other), None, "{other}"); }
    }
    #[test]
    fn the_sandbox_is_found_under_work_and_never_through_a_link() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let p = project();
        store.put(&p).unwrap();
        let work = crate::h2g::work_dir(&store, &p.id).unwrap();
        assert_eq!(sandbox(&store, &p), None);
        let wp = work.join("output/clara-hayes-blocks-sandbox/wordpress");
        std::fs::create_dir_all(&wp).unwrap();
        std::fs::write(wp.join("wp-config.php"), "<?php\ndefine( 'WP_HOME', 'http://127.0.0.1:8899' );").unwrap();
        assert_eq!(sandbox(&store, &p), Some(("/work/output/clara-hayes-blocks-sandbox/wordpress".into(), 8899)));
        // A link that leads out of /work is not a sandbox.
        std::fs::remove_dir_all(work.join("output")).unwrap();
        let outside = dir.path().join("elsewhere/wordpress");
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(outside.join("wp-config.php"), "define( 'WP_HOME', 'http://127.0.0.1:8899' );").unwrap();
        #[cfg(unix)] {
            std::os::unix::fs::symlink(dir.path().join("elsewhere"), work.join("linked")).unwrap();
            assert_eq!(sandbox(&store, &p), None);
        }
    }
    /// The real container (docker): the sandbox's server, started by the app
    /// with the skill's own command, answers on this Mac at the address its
    /// wp-config names, through the container (H2WP_TEST_RUNTIME_IMAGE).
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to the runtime image"]
    async fn the_sandbox_s_wordpress_answers_here_at_its_own_address() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let base = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/html2wp-desktop-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new().prefix("app data ").tempdir_in(base).unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let wp = crate::h2g::work_dir(&store, &p.id).unwrap().join("output/studio-blocks-sandbox/wordpress");
        std::fs::create_dir_all(&wp).unwrap();
        std::fs::write(wp.join("wp-config.php"), format!("<?php\ndefine( 'WP_HOME', 'http://127.0.0.1:{port}' );")).unwrap();
        std::fs::write(wp.join("wp-login.php"), "<?php echo 'login at ', $_SERVER['HTTP_HOST'], ' ', strlen(file_get_contents('php://input'));").unwrap();
        let started = start(&store, &p, &image).await;
        let client = reqwest::Client::new();
        let get = client.get(format!("http://127.0.0.1:{port}/wp-login.php")).send().await.map(|r| r.status().as_u16());
        let body = "x".repeat(300_000);
        let post = match client.post(format!("http://127.0.0.1:{port}/wp-login.php")).body(body).send().await { Ok(r) => r.text().await.unwrap_or_default(), Err(e) => e.to_string() };
        let shown = status(&store, &p).await;
        stop(&p.id);
        let after = reqwest::get(format!("http://127.0.0.1:{port}/wp-login.php")).await.is_err();
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), format!("{}-h2g", crate::runtime::project_name(&p.id).unwrap())], None, 60).await;
        assert!(started.is_ok(), "{started:?}");
        assert_eq!(get.unwrap(), 200);
        assert_eq!(post, format!("login at 127.0.0.1:{port} 300000"), "the address WordPress expects, and a large request whole");
        assert_eq!((shown["running"].clone(), shown["url"].clone(), shown["user"].clone()), (json!(true), json!(format!("http://127.0.0.1:{port}")), json!("admin")));
        assert!(after, "Stop closes the port here");
    }
    /// The owner's whole Gutenberg flow after the conversion, in the real
    /// sandbox (docker): the skill's setup.sh WordPress with the new block
    /// theme, the Preview here, a change made as the change goal asks (the
    /// theme edited, the skill's sync.sh), the preview showing it, and Make
    /// release packing it as the next revision. H2WP_TEST_RUNTIME_IMAGE.
    #[tokio::test]
    #[ignore = "real Docker; set H2WP_TEST_RUNTIME_IMAGE to the runtime image"]
    async fn the_gutenberg_theme_is_previewed_changed_and_released() {
        let Ok(image) = std::env::var("H2WP_TEST_RUNTIME_IMAGE") else { return };
        let base = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join("Library/Application Support/html2wp-desktop-tests");
        std::fs::create_dir_all(&base).unwrap();
        let dir = tempfile::Builder::new().prefix("app data ").tempdir_in(base).unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let mut p = project();
        p.id = uuid::Uuid::new_v4().to_string();
        p.phase = "running".into();
        store.put(&p).unwrap();
        std::fs::create_dir_all(store.path(&p.id).unwrap().join("input")).unwrap();
        let work = crate::h2g::work_dir(&store, &p.id).unwrap();
        let theme = work.join("output/studio-blocks");
        std::fs::create_dir_all(theme.join("templates")).unwrap();
        std::fs::write(theme.join("style.css"), "/*\nTheme Name: Studio Blocks\nVersion: 1.0.0\n*/\n").unwrap();
        std::fs::write(theme.join("theme.json"), r#"{"version":3}"#).unwrap();
        std::fs::write(theme.join("templates/index.html"), "<!-- wp:paragraph --><p>Hello from the block theme</p><!-- /wp:paragraph -->").unwrap();
        let port = std::net::TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
        let name = crate::h2g::ensure_sandbox(&store, &p, &image).await.unwrap();
        let sh = |cmd: String| { let name = name.clone(); async move { crate::agent::exec(&name, "/work", &[format!("PORT={port}")], &cmd, 600).await.unwrap() } };
        let skill = crate::h2g::SKILL;
        // During the run: the skill's setup.sh (the Verify step); the Preview is there at once.
        assert_eq!(status(&store, &p).await["available"], false);
        let setup = sh(format!("mkdir -p /work/output/studio-blocks-sandbox && cp -a /opt/wp-offline/wordpress /work/output/studio-blocks-sandbox/ && bash {skill}/scripts/wp-sandbox/setup.sh /work/output/studio-blocks /work/output/studio-blocks-sandbox")).await;
        let during = status(&store, &p).await;
        let started = start(&store, &p, &image).await;
        let home = || async move { reqwest::get(format!("http://127.0.0.1:{port}/")).await.unwrap().text().await.unwrap() };
        let before = home().await;
        // Delivered; the owner's change as the change goal asks: the theme edited, sync.sh.
        std::fs::write(work.join("verification-summary.json"), r#"{"pages":1,"maxDiffPercent":0.2,"invalidBlocks":0,"notes":"ok"}"#).unwrap();
        assert!(crate::h2g::deliver(&store, &mut p, &|_, _| {}).unwrap());
        let mut p = store.project(&p.id).unwrap();
        let goal = crate::h2g::change_text(&store, &p, "make the greeting italic");
        let edit = sh("sed -i 's#<p>Hello#<p><em>Hello</em>#' /work/output/studio-blocks/templates/index.html".into()).await;
        let synced = sh(format!("bash {skill}/scripts/wp-sandbox/sync.sh /work/output/studio-blocks /work/output/studio-blocks-sandbox")).await;
        let after = home().await;
        let changed = crate::h2g::changes(&store, &p);
        let released = crate::h2g::release(&store, &mut p, &|_, _| {});
        stop(&p.id);
        let _ = crate::runtime::docker(&["rm".into(), "-f".into(), name], None, 60).await;
        assert_eq!(setup["exitCode"], 0, "{}", setup["output"]);
        assert_eq!((during["available"].clone(), during["url"].clone()), (json!(true), json!(format!("http://127.0.0.1:{port}"))), "the Preview during the run, once setup.sh ran");
        assert!(started.is_ok(), "{started:?}");
        assert!(before.contains("Hello from the block theme"), "the sandbox's WordPress with the block theme, here");
        assert!(goal.contains("edit only /work/output/studio-blocks/") && goal.contains("sync.sh"));
        assert_eq!((edit["exitCode"].clone(), synced["exitCode"].clone()), (json!(0), json!(0)));
        assert!(after.contains("<em>Hello</em>"), "the preview shows the change after sync.sh");
        assert_eq!(changed["changedSinceZip"], true);
        assert_eq!(released.unwrap(), json!({"changed":true,"revision":2,"filename":"studio-blocks-r2.zip"}));
    }
}
