//! The Chrome extension bridge: a loopback HTTP endpoint through which the
//! html2wp Chrome extension sends an annotated screenshot and a message into
//! the chat of the project open in the app. It listens on 127.0.0.1 only,
//! answers only a paired extension, and refuses any web page origin.
use crate::{chat_ready, model::*, AppState};
use http_body_util::{BodyExt, Full, Limited};
use hyper::{body::Bytes, Request, Response};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{future::Future, path::PathBuf, sync::Mutex};

/// The fixed port and its small fallback range; the extension probes them in order.
pub const PORTS: std::ops::RangeInclusive<u16> = 47811..=47815;
/// Largest decoded screenshot the bridge accepts.
pub const MAX_IMAGE: usize = 10 * 1024 * 1024;
/// Screenshots one message may carry, as the app's chat takes at most 4 images.
pub const MAX_IMAGES: usize = 4;
/// Base64 length of a MAX_IMAGE file: a longer string is refused before decoding.
const MAX_IMAGE_BASE64: usize = MAX_IMAGE.div_ceil(3) * 4;
/// Largest request body: the images, the message and the JSON around them.
const MAX_BODY: usize = MAX_IMAGES * MAX_IMAGE_BASE64 + 256 * 1024;
/// Wrong pairing codes in a row before the code stops working until Regenerate.
const MAX_PAIR_FAILURES: u32 = 5;
const TOKEN_KEY: &str = "chrome-bridge-token";
pub const ACTIVE_PROJECT_KEY: &str = "active-project";
pub const NO_PROJECT: &str = "Open a project in html2wp first.";
const PROJECT_CHANGED: &str = "The project open in html2wp changed. Check the project in the extension and send again.";
const NO_TEXT: &str = "Please look at the attached screenshot.";

/// Pairing state kept in memory: the code shown in Settings and wrong attempts.
#[derive(Default)]
pub struct Pairing {
    code: Mutex<Option<String>>,
    failures: Mutex<u32>,
    pub port: Mutex<Option<u16>>,
}
impl Pairing {
    pub fn new() -> Self {
        let p = Self::default();
        p.regenerate();
        p
    }
    pub fn regenerate(&self) -> String {
        let code = format!("{:06}", uuid::Uuid::new_v4().as_u128() % 1_000_000);
        if let Ok(mut c) = self.code.lock() { *c = Some(code.clone()); }
        if let Ok(mut f) = self.failures.lock() { *f = 0; }
        code
    }
    pub fn code(&self) -> Option<String> {
        self.code.lock().ok()?.clone()
    }
}
fn hash(token: &str) -> String {
    Sha256::digest(token.as_bytes()).iter().map(|b| format!("{b:02x}")).collect()
}
pub fn paired(state: &AppState) -> bool {
    state.store.setting(TOKEN_KEY).is_some_and(|v| !v.is_empty())
}
fn token_ok(state: &AppState, token: Option<&str>) -> bool {
    match (token.filter(|t| !t.is_empty()), state.store.setting(TOKEN_KEY)) {
        (Some(t), Some(stored)) if !stored.is_empty() => hash(t) == stored,
        _ => false,
    }
}
pub fn unpair(state: &AppState) -> Result<()> {
    state.store.set(TOKEN_KEY, "")
}
/// What Settings shows about the extension.
pub fn settings_view(state: &AppState) -> Value {
    let port = state.pairing.port.lock().ok().and_then(|p| *p);
    json!({"paired":paired(state),"code":state.pairing.code(),"port":port})
}

/// The project open in the app, if it still exists in the workspace.
fn active_project(state: &AppState) -> Option<Project> {
    let id = state.store.setting(ACTIVE_PROJECT_KEY).filter(|v| !v.is_empty())?;
    state.store.project(&id).ok().filter(|p| !p.archived)
}

/// Screenshots (1 to MAX_IMAGES) to deliver into a project's chat as one message.
#[derive(Debug, Clone)]
pub struct Delivery {
    pub project_id: String,
    pub text: String,
    pub images: Vec<PathBuf>,
}
/// What the server needs from the app. Tests supply their own.
pub trait Host: Clone + Send + Sync + 'static {
    fn state(&self) -> &AppState;
    /// Start the turn exactly like a typed message with an attachment.
    fn deliver(&self, delivery: Delivery) -> impl Future<Output = Result<()>> + Send;
    /// Pairing changed: Settings refreshes its view.
    fn pairing_changed(&self) {}
}

pub struct Reply {
    pub status: u16,
    pub body: Value,
}
fn reply(status: u16, body: Value) -> Reply {
    Reply { status, body }
}
fn refused(status: u16, error: &str) -> Reply {
    reply(status, json!({"error":error}))
}

/// Only the extension may call: a request carrying a web page's Origin is
/// refused, and so is one addressed to a name other than this computer
/// (a DNS-rebinding page never has a chrome-extension origin, but a GET
/// from it carries no Origin at all).
pub fn allowed(origin: Option<&str>, host: Option<&str>) -> bool {
    let origin_ok = origin.is_none_or(|o| o.starts_with("chrome-extension://"));
    let host_ok = host.is_none_or(|h| {
        let name = h.rsplit_once(':').map_or(h, |(n, _)| n);
        name == "127.0.0.1" || name == "localhost"
    });
    origin_ok && host_ok
}

/// Everything after the transport: one request in, one JSON reply out.
pub async fn route<H: Host>(host: &H, method: &str, path: &str, bearer: Option<&str>, body: &[u8]) -> Reply {
    let state = host.state();
    match (method, path) {
        ("GET", "/status") => status(state, bearer),
        ("POST", "/pair") => {
            let Ok(v) = serde_json::from_slice::<Value>(body) else { return refused(400, "bad request") };
            pair(host, v["code"].as_str().unwrap_or("").trim())
        }
        ("POST", "/message") => {
            let Ok(v) = serde_json::from_slice::<Value>(body) else { return refused(400, "bad request") };
            message(host, &v, bearer).await
        }
        _ => refused(404, "not found"),
    }
}

fn status(state: &AppState, bearer: Option<&str>) -> Reply {
    let version = env!("CARGO_PKG_VERSION");
    if !token_ok(state, bearer) {
        return reply(200, json!({"app":"html2wp","version":version,"paired":false,"project":null,"chat":null,"maxImages":MAX_IMAGES}));
    }
    let project = active_project(state);
    let ready = match &project { Some(p) => chat_ready(state, &p.id), None => Err(NO_PROJECT.to_string()) };
    reply(200, json!({"app":"html2wp","version":version,"paired":true,
        "project":project.map(|p|json!({"id":p.id,"name":p.name})),
        "chat":{"available":ready.is_ok(),"reason":ready.err()},"maxImages":MAX_IMAGES}))
}

fn pair<H: Host>(host: &H, code: &str) -> Reply {
    let state = host.state();
    let Ok(mut current) = state.pairing.code.lock() else { return refused(500, "unavailable") };
    let matches = current.as_deref().is_some_and(|c| !code.is_empty() && c == code);
    if !matches {
        if let Ok(mut failures) = state.pairing.failures.lock() {
            *failures += 1;
            // Guessing the code is not an option: it stops working instead.
            if *failures >= MAX_PAIR_FAILURES { *current = None; }
        }
        drop(current);
        host.pairing_changed();
        return refused(403, "wrong code");
    }
    let token = format!("{}{}", uuid::Uuid::new_v4().simple(), uuid::Uuid::new_v4().simple());
    if state.store.set(TOKEN_KEY, &hash(&token)).is_err() { return refused(500, "unavailable"); }
    // A code pairs once; the next extension needs the next code.
    *current = None;
    drop(current);
    state.pairing.regenerate();
    host.pairing_changed();
    reply(200, json!({"token":token}))
}

/// The PNGs of a message: `imagesPng` (1 to MAX_IMAGES) or the single
/// `imagePng` of older extensions. Each is size-checked before decoding.
fn images_of(v: &Value) -> std::result::Result<Vec<Vec<u8>>, Reply> {
    use base64::Engine;
    let encoded: Vec<&str> = match v.get("imagesPng") {
        Some(list) => {
            let Some(list) = list.as_array() else { return Err(refused(400, "bad image")) };
            if list.is_empty() || list.len() > MAX_IMAGES { return Err(refused(400, "send 1 to 4 images")); }
            let Some(items) = list.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else { return Err(refused(400, "bad image")) };
            items
        }
        None => vec![v["imagePng"].as_str().unwrap_or("")],
    };
    let mut pngs = Vec::with_capacity(encoded.len());
    for item in encoded {
        if item.len() > MAX_IMAGE_BASE64 { return Err(refused(413, "image too large")); }
        let Ok(png) = base64::engine::general_purpose::STANDARD.decode(item) else { return Err(refused(400, "bad image")) };
        if png.len() > MAX_IMAGE { return Err(refused(413, "image too large")); }
        if !png.starts_with(b"\x89PNG\r\n\x1a\n") { return Err(refused(400, "bad image")); }
        pngs.push(png);
    }
    Ok(pngs)
}

async fn message<H: Host>(host: &H, v: &Value, bearer: Option<&str>) -> Reply {
    let state = host.state();
    if !token_ok(state, v["token"].as_str().or(bearer)) { return refused(401, "not paired"); }
    let pngs = match images_of(v) { Ok(p) => p, Err(r) => return r };
    let busy = |reason: String| reply(409, json!({"reason":reason}));
    let Some(project) = active_project(state) else { return busy(NO_PROJECT.into()) };
    if v["projectId"].as_str() != Some(project.id.as_str()) { return busy(PROJECT_CHANGED.into()); }
    if let Err(reason) = chat_ready(state, &project.id) { return busy(reason); }
    let text = v["text"].as_str().unwrap_or("").trim();
    let text = if text.is_empty() { NO_TEXT } else { text };
    // store.path validates the project ID before it names a directory.
    if state.store.path(&project.id).is_err() { return busy("Invalid project ID".into()); }
    let inbox = state.store.root.join("private").join(&project.id).join("inbox");
    let stamp = chrono::Utc::now().format("%Y%m%d-%H%M%S-%3f");
    let mut images = Vec::with_capacity(pngs.len());
    for (index, png) in pngs.iter().enumerate() {
        let image = inbox.join(if pngs.len() == 1 { format!("{stamp}.png") } else { format!("{stamp}-{}.png", index + 1) });
        if let Err(e) = std::fs::create_dir_all(&inbox).and_then(|_| std::fs::write(&image, png)) {
            for written in &images { let _ = std::fs::remove_file(written); }
            return busy(format!("The screenshot could not be saved: {e}"));
        }
        images.push(image);
    }
    match host.deliver(Delivery { project_id: project.id, text: text.into(), images: images.clone() }).await {
        Ok(()) => reply(200, json!({"ok":true})),
        Err(reason) => {
            for image in &images { let _ = std::fs::remove_file(image); }
            busy(reason)
        }
    }
}

/// The first free port of the range, or None when all are taken.
pub async fn bind() -> Option<tokio::net::TcpListener> {
    for port in PORTS {
        if let Ok(listener) = tokio::net::TcpListener::bind(("127.0.0.1", port)).await { return Some(listener); }
    }
    None
}

pub async fn serve<H: Host>(listener: tokio::net::TcpListener, host: H) {
    loop {
        let Ok((stream, _)) = listener.accept().await else { continue };
        let host = host.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |req| {
                let host = host.clone();
                async move { Ok::<_, std::convert::Infallible>(handle(&host, req).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), service)
                .await;
        });
    }
}

async fn handle<H: Host>(host: &H, req: Request<hyper::body::Incoming>) -> Response<Full<Bytes>> {
    let header = |name: &str| req.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_string);
    let (origin, host_name) = (header("origin"), header("host"));
    let bearer = header("authorization").and_then(|v| v.strip_prefix("Bearer ").map(str::to_string));
    let (method, path) = (req.method().to_string(), req.uri().path().to_string());
    let out = if !allowed(origin.as_deref(), host_name.as_deref()) {
        refused(403, "forbidden")
    } else if req.headers().get("content-length").and_then(|v| v.to_str().ok()?.parse::<usize>().ok()).is_some_and(|n| n > MAX_BODY) {
        refused(413, "request too large")
    } else {
        match Limited::new(req.into_body(), MAX_BODY).collect().await {
            Ok(body) => route(host, &method, &path, bearer.as_deref(), &body.to_bytes()).await,
            Err(e) if e.downcast_ref::<http_body_util::LengthLimitError>().is_some() => refused(413, "request too large"),
            Err(_) => refused(400, "bad request"),
        }
    };
    Response::builder()
        .status(out.status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(Full::new(Bytes::from(out.body.to_string())))
        .unwrap_or_else(|_| Response::new(Full::new(Bytes::new())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    #[derive(Clone)]
    struct TestHost {
        state: Arc<AppState>,
        delivered: Arc<Mutex<Vec<Delivery>>>,
    }
    impl Host for TestHost {
        fn state(&self) -> &AppState { &self.state }
        fn deliver(&self, delivery: Delivery) -> impl Future<Output = Result<()>> + Send {
            self.delivered.lock().unwrap().push(delivery);
            async { Ok(()) }
        }
    }
    const PID: &str = "0584dcf1-7f08-4efb-85cf-ae7284faf8f9";
    fn host(root: &std::path::Path) -> TestHost {
        let state = AppState::new(crate::store::Store::open(root.into()).unwrap(), root.into(), json!({}));
        let p: Project = serde_json::from_value(json!({"id":PID,"name":"Studio site","sourceName":"t","kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"imported","revision":1,"threadId":null,"pages":[],"gates":[],"artifacts":[],
            "preview":null,"runtimeImage":"img","pluginCommit":"abc","reporting":"not_required","lastError":null})).unwrap();
        state.store.put(&p).unwrap();
        state.store.set("disclosure", "accepted").unwrap();
        state.store.set(ACTIVE_PROJECT_KEY, PID).unwrap();
        TestHost { state: Arc::new(state), delivered: Arc::default() }
    }
    fn code(h: &TestHost) -> String { h.state.pairing.code().unwrap() }
    async fn paired_token(h: &TestHost) -> String {
        let r = route(h, "POST", "/pair", None, json!({"code":code(h)}).to_string().as_bytes()).await;
        assert_eq!(r.status, 200);
        r.body["token"].as_str().unwrap().to_string()
    }
    fn png(extra: usize) -> String {
        use base64::Engine;
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.resize(8 + extra, 0);
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }
    async fn chat(h: &TestHost, token: &str) -> Value {
        route(h, "GET", "/status", Some(token), b"").await.body["chat"].clone()
    }

    #[tokio::test]
    async fn status_gives_the_apps_own_reason_why_the_chat_is_closed() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let token = paired_token(&h).await;
        let s = route(&h, "GET", "/status", Some(&token), b"").await.body;
        assert_eq!((s["app"].as_str(), s["paired"].as_bool()), (Some("html2wp"), Some(true)));
        assert_eq!(s["project"], json!({"id":PID,"name":"Studio site"}));
        assert_eq!(s["chat"], json!({"available":true,"reason":null}));

        h.state.claim(PID).unwrap();
        assert_eq!(chat(&h, &token).await["reason"], "The assistant is working on Studio site. Send the screenshot when it finishes.");
        h.state.release(PID);

        h.state.store.set("disclosure", "").unwrap();
        assert_eq!(chat(&h, &token).await["reason"], "Read and accept the data flow notice in Settings first");
        h.state.store.set("disclosure", "accepted").unwrap();

        *h.state.setup.lock().unwrap() = Some(Arc::new(crate::setup::Control::default()));
        assert_eq!(chat(&h, &token).await["reason"], "Wait for environment setup to finish before starting a conversation");
        *h.state.setup.lock().unwrap() = None;

        h.state.store.set(ACTIVE_PROJECT_KEY, "").unwrap();
        let s = route(&h, "GET", "/status", Some(&token), b"").await.body;
        assert_eq!((s["project"].clone(), s["chat"].clone()), (Value::Null, json!({"available":false,"reason":NO_PROJECT})));
    }

    #[tokio::test]
    async fn an_unpaired_caller_learns_nothing_about_the_project() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let s = route(&h, "GET", "/status", None, b"").await.body;
        assert_eq!((s["paired"].as_bool(), s["project"].clone(), s["chat"].clone()), (Some(false), Value::Null, Value::Null));
        let s = route(&h, "GET", "/status", Some("guess"), b"").await.body;
        assert_eq!(s["paired"], false);
    }

    #[tokio::test]
    async fn a_wrong_code_is_refused_and_guessing_locks_the_code() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let right = code(&h);
        let wrong = if right == "000000" { "000001" } else { "000000" };
        assert_eq!(route(&h, "POST", "/pair", None, br#"{"code":""}"#).await.status, 403);
        for _ in 1..MAX_PAIR_FAILURES {
            assert_eq!(route(&h, "POST", "/pair", None, json!({"code":wrong}).to_string().as_bytes()).await.status, 403);
        }
        assert!(h.state.pairing.code().is_none(), "the code stops working after repeated wrong guesses");
        assert_eq!(route(&h, "POST", "/pair", None, json!({"code":right}).to_string().as_bytes()).await.status, 403);
        h.state.pairing.regenerate();
        let token = paired_token(&h).await;
        assert!(paired(&h.state) && token.len() >= 64);
        assert_ne!(h.state.store.setting(TOKEN_KEY).unwrap(), token, "only the token's hash is stored");
    }

    #[tokio::test]
    async fn a_message_needs_the_paired_token() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let body = |token: Option<&str>| json!({"token":token,"projectId":PID,"text":"hi","imagePng":png(16)}).to_string();
        assert_eq!(route(&h, "POST", "/message", None, body(None).as_bytes()).await.status, 401);
        let token = paired_token(&h).await;
        assert_eq!(route(&h, "POST", "/message", None, body(Some("forged")).as_bytes()).await.status, 401);
        let ok = route(&h, "POST", "/message", None, body(Some(&token)).as_bytes()).await;
        assert_eq!((ok.status, ok.body.clone()), (200, json!({"ok":true})));
        let sent = h.delivered.lock().unwrap().clone();
        assert_eq!(sent.len(), 1);
        assert_eq!((sent[0].project_id.as_str(), sent[0].text.as_str()), (PID, "hi"));
        assert_eq!(sent[0].images.len(), 1);
        assert!(sent[0].images[0].starts_with(root.path().join("private").join(PID).join("inbox")) && sent[0].images[0].exists());
        unpair(&h.state).unwrap();
        assert_eq!(route(&h, "POST", "/message", None, body(Some(&token)).as_bytes()).await.status, 401, "unpairing revokes the token");
    }

    #[tokio::test]
    async fn a_busy_chat_or_another_project_gets_409_with_the_reason() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let token = paired_token(&h).await;
        let send = |pid: &str| json!({"token":token,"projectId":pid,"text":"","imagePng":png(16)}).to_string();
        h.state.claim(PID).unwrap();
        let r = route(&h, "POST", "/message", None, send(PID).as_bytes()).await;
        assert_eq!((r.status, r.body["reason"].as_str()), (409, Some("The assistant is working on Studio site. Send the screenshot when it finishes.")));
        h.state.release(PID);
        let r = route(&h, "POST", "/message", None, send("1f0e2d3c-0000-4000-8000-000000000000").as_bytes()).await;
        assert_eq!((r.status, r.body["reason"].as_str()), (409, Some(PROJECT_CHANGED)));
        assert!(h.delivered.lock().unwrap().is_empty());
        assert_eq!(route(&h, "POST", "/message", None, send(PID).as_bytes()).await.status, 200);
        assert_eq!(h.delivered.lock().unwrap()[0].text, NO_TEXT, "a screenshot without words still reads as a message");
    }

    #[tokio::test]
    async fn one_message_carries_one_to_four_screenshots() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let token = paired_token(&h).await;
        let status = route(&h, "GET", "/status", Some(&token), b"").await.body;
        assert_eq!(status["maxImages"], 4, "the extension reads the limit instead of assuming it");
        let many = |images: Vec<String>| json!({"token":token,"projectId":PID,"text":"these","imagesPng":images}).to_string();
        assert_eq!(route(&h, "POST", "/message", None, many(vec![png(16)]).as_bytes()).await.status, 200);
        assert_eq!(route(&h, "POST", "/message", None, many((0..4).map(|i| png(16 + i)).collect()).as_bytes()).await.status, 200);
        {
            let sent = h.delivered.lock().unwrap();
            assert_eq!(sent.iter().map(|d| d.images.len()).collect::<Vec<_>>(), vec![1, 4]);
            assert!(sent[1].images.iter().all(|p| p.exists()), "each screenshot is written to the inbox");
            let names: std::collections::HashSet<_> = sent[1].images.iter().collect();
            assert_eq!(names.len(), 4, "four files, not one overwritten four times");
        }
        assert_eq!(route(&h, "POST", "/message", None, many((0..5).map(|_| png(16)).collect()).as_bytes()).await.status, 400, "five is one too many");
        assert_eq!(route(&h, "POST", "/message", None, many(vec![]).as_bytes()).await.status, 400);
        assert_eq!(route(&h, "POST", "/message", None, many(vec![png(16), "aGVsbG8=".into()]).as_bytes()).await.status, 400, "one non-PNG refuses the set");
        assert_eq!(route(&h, "POST", "/message", None, many(vec![png(16), png(MAX_IMAGE + 1)]).as_bytes()).await.status, 413, "one oversize image refuses the set");
        assert_eq!(h.delivered.lock().unwrap().len(), 2, "a refused set delivers nothing");
        // Older extensions keep sending a single imagePng.
        let legacy = json!({"token":token,"projectId":PID,"text":"one","imagePng":png(16)}).to_string();
        assert_eq!(route(&h, "POST", "/message", None, legacy.as_bytes()).await.status, 200);
        assert_eq!(h.delivered.lock().unwrap().last().unwrap().images.len(), 1);
    }

    #[tokio::test]
    async fn an_oversize_image_is_refused_before_decoding() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let token = paired_token(&h).await;
        let big = json!({"token":token,"projectId":PID,"text":"x","imagePng":png(MAX_IMAGE + 1)}).to_string();
        assert_eq!(route(&h, "POST", "/message", None, big.as_bytes()).await.status, 413);
        let not_png = json!({"token":token,"projectId":PID,"text":"x","imagePng":"aGVsbG8="}).to_string();
        assert_eq!(route(&h, "POST", "/message", None, not_png.as_bytes()).await.status, 400);
        assert!(h.delivered.lock().unwrap().is_empty());
    }

    #[test]
    fn only_the_extension_on_this_computer_may_call() {
        assert!(allowed(None, Some("127.0.0.1:47811")));
        assert!(allowed(Some("chrome-extension://abcdefghijklmnop"), Some("127.0.0.1:47811")));
        assert!(!allowed(Some("https://example.com"), Some("127.0.0.1:47811")));
        assert!(!allowed(Some("null"), None));
        assert!(!allowed(None, Some("rebound.example:47811")), "a DNS-rebinding page is refused by its Host");
    }

    #[tokio::test]
    async fn the_server_refuses_foreign_origins_and_oversize_bodies() {
        let root = tempfile::tempdir().unwrap();
        let h = host(root.path());
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let base = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        tokio::spawn(serve(listener, h.clone()));
        let client = reqwest::Client::new();
        let s: Value = client.get(format!("{base}/status")).send().await.unwrap().json().await.unwrap();
        assert_eq!((s["app"].as_str(), s["paired"].as_bool()), (Some("html2wp"), Some(false)));
        let r = client.get(format!("{base}/status")).header("origin", "https://example.com").send().await.unwrap();
        assert_eq!(r.status().as_u16(), 403);
        let r = client.post(format!("{base}/pair")).header("origin", "https://example.com").body(json!({"code":code(&h)}).to_string()).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 403, "a web page cannot pair even with the right code");
        let r = client.post(format!("{base}/pair")).header("origin", "chrome-extension://abc").body(json!({"code":code(&h)}).to_string()).send().await.unwrap();
        assert_eq!(r.status().as_u16(), 200);
        // An oversize body is refused from its Content-Length alone. The request
        // is written by hand without its body: a client still sending the body
        // could see the connection closed after the 413 instead of the answer.
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let mut stream = tokio::net::TcpStream::connect(base.trim_start_matches("http://")).await.unwrap();
        stream.write_all(format!("POST /message HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{{", MAX_BODY + 1).as_bytes()).await.unwrap();
        let mut answer = vec![0u8; 512];
        let n = tokio::time::timeout(std::time::Duration::from_secs(5), stream.read(&mut answer)).await.unwrap().unwrap();
        assert!(String::from_utf8_lossy(&answer[..n]).starts_with("HTTP/1.1 413"), "{}", String::from_utf8_lossy(&answer[..n]));
    }
}
