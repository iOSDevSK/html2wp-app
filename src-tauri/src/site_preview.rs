//! Serve a project's built Astro site (workspace/astro-project/dist) on
//! http://127.0.0.1:<free port> so the owner can click through it before
//! deploying. One preview at a time; it ends with the app, a project switch
//! or the project's removal. Read-only, loopback-only, GET and HEAD.
use crate::{cloudflare, model::*, AppState};
use serde_json::{json, Value};
use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::State;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

struct Running {
    project_id: String,
    port: u16,
    task: tokio::task::JoinHandle<()>,
}
static SERVER: Mutex<Option<Running>> = Mutex::new(None);

#[derive(Debug, PartialEq)]
pub enum Resolved {
    File(PathBuf, u16),
    Redirect(String),
    NotFound,
    BadRequest,
}

fn percent_decode(segment: &str) -> Option<String> {
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
/// A regular file inside the root, after following symlinks.
fn inside(root: &Path, candidate: &Path) -> Option<PathBuf> {
    let real = candidate.canonicalize().ok()?;
    (real.starts_with(root) && real.is_file()).then_some(real)
}
/// Map a request target to a file the way static hosts serve Astro builds:
/// directory indexes (/about/ → about/index.html), file-format pages
/// (/about → about.html), a trailing-slash redirect for directories, and
/// 404.html when present. Query strings and fragments are ignored.
pub fn resolve(root: &Path, target: &str) -> Resolved {
    let Ok(root) = root.canonicalize() else { return Resolved::NotFound };
    let path = target.split(['?', '#']).next().unwrap_or("");
    if !path.starts_with('/') {
        return Resolved::BadRequest;
    }
    let mut segments = vec![];
    for raw in path[1..].split('/') {
        let Some(segment) = percent_decode(raw) else { return Resolved::BadRequest };
        if segment == ".." || segment.contains(['/', '\\', '\0']) || (cfg!(windows) && segment.contains(':')) {
            return Resolved::BadRequest;
        }
        if !segment.is_empty() && segment != "." {
            segments.push(segment);
        }
    }
    let base = segments.iter().fold(root.clone(), |p, s| p.join(s));
    let found = |candidate: PathBuf| inside(&root, &candidate).map(|f| Resolved::File(f, 200));
    let hit = if path.ends_with('/') || segments.is_empty() {
        found(base.join("index.html")).or_else(|| {
            let last = segments.last()?;
            found(base.with_file_name(format!("{last}.html")))
        })
    } else {
        let last = segments.last().unwrap();
        found(base.clone()).or_else(|| found(base.with_file_name(format!("{last}.html")))).or_else(|| {
            inside(&root, &base.join("index.html")).map(|_| {
                let query = target.find('?').map(|i| &target[i..]).unwrap_or("");
                Resolved::Redirect(format!("{path}/{query}"))
            })
        })
    };
    hit.or_else(|| inside(&root, &root.join("404.html")).map(|f| Resolved::File(f, 404)))
        .unwrap_or(Resolved::NotFound)
}
pub fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).map(|e| e.to_ascii_lowercase()).as_deref() {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("xml") => "application/xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("gif") => "image/gif",
        Some("webp") => "image/webp",
        Some("avif") => "image/avif",
        Some("ico") => "image/x-icon",
        Some("woff") => "font/woff",
        Some("woff2") => "font/woff2",
        Some("ttf") => "font/ttf",
        Some("otf") => "font/otf",
        Some("pdf") => "application/pdf",
        Some("mp4") => "video/mp4",
        Some("webm") => "video/webm",
        Some("mp3") => "audio/mpeg",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    }
}
/// Only this computer's own names for the server (no DNS rebinding).
pub fn allowed_host(host: &str, port: u16) -> bool {
    [format!("127.0.0.1:{port}"), format!("localhost:{port}")].iter().any(|h| h.eq_ignore_ascii_case(host))
}

async fn respond(stream: &mut TcpStream, status: u16, headers: &[(&str, String)], body: &[u8]) -> std::io::Result<()> {
    let reason = match status { 200 => "OK", 301 => "Moved Permanently", 400 => "Bad Request", 403 => "Forbidden", 404 => "Not Found", 405 => "Method Not Allowed", _ => "Error" };
    let mut head = format!("HTTP/1.1 {status} {reason}\r\nConnection: close\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n");
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes()).await?;
    stream.write_all(body).await
}
async fn handle(mut stream: TcpStream, root: Arc<PathBuf>, port: u16) -> std::io::Result<()> {
    let mut buffer = vec![0u8; 16 * 1024];
    let mut used = 0;
    let head = loop {
        let n = tokio::time::timeout(Duration::from_secs(10), stream.read(&mut buffer[used..])).await.map_err(std::io::Error::other)??;
        if n == 0 { return Ok(()); }
        used += n;
        if let Some(end) = buffer[..used].windows(4).position(|w| w == b"\r\n\r\n") {
            break String::from_utf8_lossy(&buffer[..end]).to_string();
        }
        if used == buffer.len() {
            return respond(&mut stream, 400, &[], b"Request too large").await;
        }
    };
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let host = lines.filter_map(|l| l.split_once(':')).find(|(k, _)| k.trim().eq_ignore_ascii_case("host")).map(|(_, v)| v.trim()).unwrap_or("");
    if !allowed_host(host, port) {
        return respond(&mut stream, 403, &[], b"Forbidden").await;
    }
    if method != "GET" && method != "HEAD" {
        return respond(&mut stream, 405, &[("Allow", "GET, HEAD".into())], b"").await;
    }
    match resolve(&root, target) {
        Resolved::File(path, status) => {
            let mut file = tokio::fs::File::open(&path).await?;
            let length = file.metadata().await?.len();
            respond(&mut stream, status, &[("Content-Type", content_type(&path).into()), ("Content-Length", length.to_string())], b"").await?;
            if method == "GET" {
                tokio::io::copy(&mut file, &mut stream).await?;
            }
            Ok(())
        }
        Resolved::Redirect(location) => respond(&mut stream, 301, &[("Location", location), ("Content-Length", "0".into())], b"").await,
        Resolved::NotFound => respond(&mut stream, 404, &[("Content-Type", "text/plain; charset=utf-8".into()), ("Content-Length", "9".into())], b"Not found").await,
        Resolved::BadRequest => respond(&mut stream, 400, &[("Content-Length", "11".into())], b"Bad request").await,
    }
}
/// Listen on a free loopback port and serve `root` until the task is aborted.
pub async fn serve(root: PathBuf) -> Result<(u16, tokio::task::JoinHandle<()>)> {
    let listener = TcpListener::bind(("127.0.0.1", 0)).await.map_err(err)?;
    let port = listener.local_addr().map_err(err)?.port();
    let root = Arc::new(root);
    let task = tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let root = root.clone();
            tokio::spawn(async move { let _ = handle(stream, root, port).await; });
        }
    });
    Ok((port, task))
}
fn url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/")
}
fn current() -> Option<(String, u16)> {
    SERVER.lock().ok()?.as_ref().map(|r| (r.project_id.clone(), r.port))
}
/// Stop the preview. With `except`, a preview of that project keeps running.
pub fn stop(except: Option<&str>) {
    if let Ok(mut server) = SERVER.lock() {
        if server.as_ref().is_some_and(|r| Some(r.project_id.as_str()) != except) {
            if let Some(r) = server.take() { r.task.abort(); }
        }
    }
}
pub fn stop_project(project_id: &str) {
    if current().is_some_and(|(id, _)| id == project_id) {
        stop(None);
    }
}

#[tauri::command]
pub fn site_preview_status(state: State<AppState>, project_id: String) -> Result<Value> {
    let p = state.store.project(&project_id)?;
    let blocker = cloudflare::dist_blocker(&state.store.path(&p.id)?.join("workspace/astro-project/dist"), &p.phase);
    let running = current().filter(|(id, _)| *id == p.id).map(|(_, port)| url(port));
    Ok(json!({"distReady":blocker.is_none(),"distReason":blocker,"url":running}))
}
#[tauri::command]
pub async fn site_preview_start(state: State<'_, AppState>, project_id: String) -> Result<Value> {
    let _work = state.try_lock_project(&project_id, "Wait for this project's current step to finish before opening its built-site preview")?;
    if state.is_running(&project_id)? {
        return Err("Finish or stop this project's running conversion before opening its built-site preview".into());
    }
    let p = state.store.project(&project_id)?;
    let dist = state.store.path(&p.id)?.join("workspace/astro-project/dist");
    if let Some(reason) = cloudflare::dist_blocker(&dist, &p.phase) {
        return Err(reason.into());
    }
    if let Some((id, port)) = current() {
        if id == p.id { return Ok(json!({"url":url(port)})); }
    }
    stop(None);
    let (port, task) = serve(dist).await?;
    *SERVER.lock().map_err(err)? = Some(Running { project_id: p.id, port, task });
    Ok(json!({"url":url(port)}))
}
#[tauri::command]
pub fn site_preview_stop(except_project: Option<String>) -> Result<()> {
    stop(except_project.as_deref());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn site() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for (file, body) in [
            ("index.html", "home"), ("about/index.html", "about dir"), ("contact.html", "contact file"),
            ("blog/2024/first-post/index.html", "nested"), ("o-nás/index.html", "diacritics"),
            ("_astro/app.Bx1.css", "css"), ("images/hero photo.webp", "img"), ("404.html", "missing"),
            ("docs/index.html", "docs"), ("docs.html", "docs file wins without slash"),
        ] {
            let path = root.join(file);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        dir
    }
    fn body(root: &Path, target: &str) -> (String, u16) {
        match resolve(root, target) {
            Resolved::File(path, status) => (std::fs::read_to_string(path).unwrap(), status),
            other => panic!("{target}: {other:?}"),
        }
    }
    #[test]
    fn serves_directory_and_file_format_astro_builds() {
        let dir = site();
        let root = dir.path();
        assert_eq!(body(root, "/"), ("home".into(), 200));
        assert_eq!(body(root, "/?utm=1#top"), ("home".into(), 200));
        assert_eq!(body(root, "/about/"), ("about dir".into(), 200));
        assert_eq!(resolve(root, "/about"), Resolved::Redirect("/about/".into()));
        assert_eq!(resolve(root, "/about?x=1"), Resolved::Redirect("/about/?x=1".into()));
        assert_eq!(body(root, "/contact"), ("contact file".into(), 200));
        assert_eq!(body(root, "/contact/"), ("contact file".into(), 200));
        assert_eq!(body(root, "/contact.html"), ("contact file".into(), 200));
        assert_eq!(body(root, "/docs"), ("docs file wins without slash".into(), 200));
        assert_eq!(body(root, "/docs/"), ("docs".into(), 200));
        assert_eq!(body(root, "/blog/2024/first-post/"), ("nested".into(), 200));
        assert_eq!(body(root, "/o-n%C3%A1s/"), ("diacritics".into(), 200));
        assert_eq!(body(root, "/_astro/app.Bx1.css?v=3"), ("css".into(), 200));
        assert_eq!(body(root, "/images/hero%20photo.webp"), ("img".into(), 200));
        assert_eq!(body(root, "/missing/page"), ("missing".into(), 404));
    }
    #[test]
    fn never_leaves_the_build_directory() {
        let dir = site();
        let root = dir.path().join("site");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("index.html"), "inner").unwrap();
        std::fs::write(dir.path().join("secret.txt"), "secret").unwrap();
        for bad in ["/../secret.txt", "/%2e%2e/secret.txt", "/..%2fsecret.txt", "/a%5c..%5csecret.txt", "/%00", "/%zz", "secret.txt", "/%C3"] {
            assert!(matches!(resolve(&root, bad), Resolved::BadRequest | Resolved::NotFound), "{bad}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path().join("secret.txt"), root.join("link.txt")).unwrap();
            assert_eq!(resolve(&root, "/link.txt"), Resolved::NotFound);
        }
        // Without a 404.html the answer is a plain 404.
        assert_eq!(resolve(&root, "/nothing"), Resolved::NotFound);
    }
    #[test]
    fn types_and_hosts() {
        assert_eq!(content_type(Path::new("a/b.HTML")), "text/html; charset=utf-8");
        assert_eq!(content_type(Path::new("x.woff2")), "font/woff2");
        assert_eq!(content_type(Path::new("x.mjs")), "text/javascript; charset=utf-8");
        assert_eq!(content_type(Path::new("x.unknown")), "application/octet-stream");
        assert!(allowed_host("127.0.0.1:4000", 4000) && allowed_host("LOCALHOST:4000", 4000));
        assert!(!allowed_host("evil.example:4000", 4000) && !allowed_host("127.0.0.1:4001", 4000) && !allowed_host("", 4000));
    }
    #[tokio::test]
    async fn serves_over_loopback_http() {
        let dir = site();
        let (port, task) = serve(dir.path().to_path_buf()).await.unwrap();
        let client = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let get = |path: &str| client.get(format!("http://127.0.0.1:{port}{path}")).send();
        let home = get("/").await.unwrap();
        assert_eq!(home.status(), 200);
        assert_eq!(home.headers()["content-type"], "text/html; charset=utf-8");
        assert_eq!(home.text().await.unwrap(), "home");
        let redirect = get("/about").await.unwrap();
        assert_eq!((redirect.status().as_u16(), redirect.headers()["location"].to_str().unwrap()), (301, "/about/"));
        let missing = get("/nope").await.unwrap();
        assert_eq!((missing.status().as_u16(), missing.text().await.unwrap()), (404, "missing".into()));
        assert_eq!(get("/o-n%C3%A1s/").await.unwrap().text().await.unwrap(), "diacritics");
        assert_eq!(client.head(format!("http://127.0.0.1:{port}/contact")).send().await.unwrap().headers()["content-length"], "12");
        assert_eq!(client.post(format!("http://127.0.0.1:{port}/")).send().await.unwrap().status(), 405);
        let rebinding = client.get(format!("http://127.0.0.1:{port}/")).header("Host", "evil.example").send().await.unwrap();
        assert_eq!(rebinding.status(), 403);
        task.abort();
    }
}
