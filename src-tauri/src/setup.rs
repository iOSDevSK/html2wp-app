//! First-run setup owns its downloads and never overwrites an existing Docker installation.
use crate::{files, model::*, runtime, AppState};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::Duration,
};
use tauri::{AppHandle, Emitter};
use tokio::{io::AsyncWriteExt, process::Command};

// codesign treats -R as a filename unless its argument starts with '='.
#[cfg(target_os = "macos")]
const DOCKER_SIGNING_REQUIREMENT: &str = r#"=identifier "com.docker.docker" and anchor apple generic and certificate leaf[subject.OU] = "9BNSXJN65R""#;

#[derive(Default)]
pub struct Control {
    pub cancel: AtomicBool,
    pub installing: AtomicBool,
}
struct Session<'a> {
    state: &'a AppState,
}
impl Drop for Session<'_> {
    fn drop(&mut self) {
        if let Ok(mut task) = self.state.setup.lock() {
            *task = None;
        }
    }
}
fn progress(app: &AppHandle, phase: &str, message: &str, percent: Option<u64>, cancellable: bool) {
    let _ = app.emit(
        "runtime-progress",
        json!({"phase":phase,"message":message,"percent":percent,"cancellable":cancellable}),
    );
}
fn cancelled(control: &Control) -> Result<()> {
    if control.cancel.load(Ordering::SeqCst) {
        Err("Setup stopped. You can resume it with Prepare environment.".into())
    } else {
        Ok(())
    }
}
pub fn cancel(state: &AppState) -> Result<()> {
    if let Some(task) = state.setup.lock().map_err(err)?.as_ref() {
        if task.installing.load(Ordering::SeqCst) {
            return Err("Complete or cancel the Docker installer in its own window first.".into());
        }
        task.cancel.store(true, Ordering::SeqCst);
    }
    Ok(())
}
async fn command(program: &str, args: &[&str], seconds: u64) -> Result<String> {
    let mut command = Command::new(program);
    command.args(args).kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x08000000);
    let out = tokio::time::timeout(Duration::from_secs(seconds), command.output())
        .await
        .map_err(|_| {
            format!("{program} timed out. Retry setup after the system installer finishes.")
        })?
        .map_err(err)?;
    if !out.status.success() {
        return Err(format!(
            "{program}: {}",
            String::from_utf8_lossy(&out.stderr)
                .chars()
                .take(1200)
                .collect::<String>()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into())
}
fn installer_key(os: &str, arch: &str, distro: &str) -> Result<String> {
    if os == "linux" {
        if arch != "x86_64" {
            return Err("Automatic Docker Desktop installation currently supports Linux x86_64. An existing local Docker Engine can also be used.".into());
        }
        let ids: Vec<_> = distro
            .lines()
            .filter_map(|line| line.split_once('='))
            .filter(|(k, _)| ["ID", "ID_LIKE"].contains(k))
            .flat_map(|(_, v)| v.trim_matches('"').split_whitespace())
            .collect();
        if ids.iter().any(|id| ["ubuntu", "debian"].contains(id)) {
            return Ok("linux-deb".into());
        }
        if ids.contains(&"fedora") {
            return Ok("linux-rpm".into());
        }
        return Err("Automatic Docker installation supports Debian, Ubuntu and Fedora. On this distribution an existing local Docker Engine is required.".into());
    }
    if ["macos", "windows"].contains(&os) && ["aarch64", "x86_64"].contains(&arch) {
        Ok(format!("{os}-{arch}"))
    } else {
        Err("No Docker installer is available for this platform.".into())
    }
}
async fn download(
    app: &AppHandle,
    control: &Control,
    root: &Path,
    entry: &Value,
) -> Result<PathBuf> {
    let url = entry["url"].as_str().ok_or("Missing installer URL")?;
    let parsed = url::Url::parse(url).map_err(err)?;
    if parsed.scheme() != "https" || parsed.host_str() != Some("desktop.docker.com") {
        return Err("Invalid Docker download source".into());
    }
    let expected = entry["sha256"]
        .as_str()
        .ok_or("Missing installer checksum")?;
    let name = entry["file"].as_str().ok_or("Missing installer name")?;
    if Path::new(name).components().count() != 1 {
        return Err("Invalid installer filename".into());
    }
    tokio::fs::create_dir_all(root).await.map_err(err)?;
    let path = root.join(name);
    if path.is_file() && files::hash(&path)? == expected {
        return Ok(path);
    }
    let part = root.join(format!("{name}.part"));
    let client = reqwest::Client::builder()
        .https_only(true)
        .connect_timeout(Duration::from_secs(20))
        .read_timeout(Duration::from_secs(45))
        .timeout(Duration::from_secs(1800))
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 5
                && attempt.url().scheme() == "https"
                && attempt.url().host_str() == Some("desktop.docker.com")
            {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }))
        .build()
        .map_err(err)?;
    let mut response = client
        .get(url)
        .send()
        .await
        .map_err(err)?
        .error_for_status()
        .map_err(err)?;
    let total = response.content_length();
    let mut file = tokio::fs::File::create(&part).await.map_err(err)?;
    let mut hash = Sha256::new();
    let mut received = 0u64;
    let mut last = None;
    while let Some(chunk) = response.chunk().await.map_err(err)? {
        cancelled(control)?;
        received += chunk.len() as u64;
        if received > 3 * 1024 * 1024 * 1024 {
            return Err("Docker installer exceeded the download limit".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk).await.map_err(err)?;
        let percent = total
            .filter(|n| *n > 0)
            .map(|n| (received * 100 / n).min(100));
        if percent != last {
            progress(
                app,
                "download",
                "Downloading Docker Desktop…",
                percent,
                true,
            );
            last = percent;
        }
    }
    file.sync_all().await.map_err(err)?;
    drop(file);
    if format!("{:x}", hash.finalize()) != expected {
        let _ = tokio::fs::remove_file(&part).await;
        return Err("Docker installer checksum did not match. Retry the download.".into());
    }
    if path.exists() {
        tokio::fs::remove_file(&path).await.map_err(err)?;
    }
    tokio::fs::rename(part, &path).await.map_err(err)?;
    Ok(path)
}
fn desktop_path() -> Option<PathBuf> {
    let mut paths: Vec<PathBuf> = vec![];
    #[cfg(target_os = "macos")]
    {
        paths.push("/Applications/Docker.app".into());
        if let Some(home) = std::env::var_os("HOME") {
            paths.push(PathBuf::from(home).join("Applications/Docker.app"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Some(dir) = std::env::var_os("ProgramFiles") {
            paths.push(PathBuf::from(dir).join("Docker/Docker/Docker Desktop.exe"));
        }
        if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
            paths.push(PathBuf::from(dir).join("Programs/DockerDesktop/Docker Desktop.exe"));
        }
    }
    #[cfg(target_os = "linux")]
    paths.push("/opt/docker-desktop/bin/docker-desktop".into());
    paths.into_iter().find(|p| p.exists())
}
#[cfg(target_os = "macos")]
async fn install(path: &Path, root: &Path, _: &str) -> Result<()> {
    let mount = root.join("mounted");
    tokio::fs::create_dir_all(&mount).await.map_err(err)?;
    command(
        "/usr/bin/hdiutil",
        &[
            "attach",
            "-readonly",
            "-nobrowse",
            "-mountpoint",
            &mount.to_string_lossy(),
            &path.to_string_lossy(),
        ],
        120,
    )
    .await?;
    let result=async {
        let app=mount.join("Docker.app");
        command("/usr/bin/codesign",&["--verify","--deep","--strict","-R",DOCKER_SIGNING_REQUIREMENT,&app.to_string_lossy()],120).await?;
        let installer=app.join("Contents/MacOS/install");
        // AppleScript quotes this path as a shell argument; no user content is executable.
        let path=serde_json::to_string(&installer.to_string_lossy()).map_err(err)?;
        command("/usr/bin/osascript",&["-e",&format!("do shell script (quoted form of {path}) with administrator privileges")],1800).await?;
        Ok(())
    }.await;
    let _ = command(
        "/usr/bin/hdiutil",
        &["detach", &mount.to_string_lossy()],
        60,
    )
    .await;
    result
}
#[cfg(target_os = "windows")]
async fn install(path: &Path, _: &Path, _: &str) -> Result<()> {
    let path = path.to_string_lossy().replace('\'', "''");
    // Windows displays elevation/installer UI; Docker retains its own licence acceptance.
    command("powershell.exe",&["-NoProfile","-NonInteractive","-Command",&format!("$p = Start-Process -FilePath '{path}' -ArgumentList 'install','--backend=wsl-2' -Verb RunAs -Wait -PassThru; exit $p.ExitCode")],1800).await?;
    Ok(())
}
#[cfg(target_os = "linux")]
async fn install(path: &Path, _: &Path, key: &str) -> Result<()> {
    // pkexec presents the distribution's authorization dialog. No password enters html2wp.
    if key == "linux-deb" {
        command(
            "pkexec",
            &["apt-get", "install", "-y", &path.to_string_lossy()],
            1800,
        )
        .await?;
    } else {
        command(
            "pkexec",
            &["dnf", "install", "-y", &path.to_string_lossy()],
            1800,
        )
        .await?;
    }
    Ok(())
}
async fn start_desktop() -> Result<()> {
    #[cfg(target_os = "macos")]
    command(
        "/usr/bin/open",
        &[
            "-a",
            &desktop_path()
                .ok_or("Docker Desktop was not installed")?
                .to_string_lossy(),
        ],
        30,
    )
    .await?;
    #[cfg(target_os = "windows")]
    Command::new(desktop_path().ok_or("Docker Desktop was not installed")?)
        .spawn()
        .map_err(err)?;
    #[cfg(target_os = "linux")]
    {
        command("systemctl", &["--user", "start", "docker-desktop"], 60).await?;
        command(
            "xdg-open",
            &["/usr/share/applications/docker-desktop.desktop"],
            30,
        )
        .await?;
    }
    Ok(())
}
async fn ensure_docker(app: &AppHandle, state: &AppState, control: &Control) -> Result<()> {
    let checked = runtime::preflight(&state.image()).await;
    if checked["platformMismatch"] == true {
        return Err(checked["message"].as_str().unwrap_or("Select Linux containers in Docker Desktop").into());
    }
    if checked["docker"] == true {
        return Ok(());
    }
    if desktop_path().is_none() {
        // Never replace an existing alternative engine or silently change its context.
        if runtime::docker(&["--version".into()], None, 15)
            .await
            .is_ok()
        {
            return Err("Your Docker CLI is installed, but its local engine is unavailable. Start that engine, then retry Prepare environment.".into());
        }
        let distro = std::fs::read_to_string("/etc/os-release").unwrap_or_default();
        let key = installer_key(std::env::consts::OS, std::env::consts::ARCH, &distro)?;
        let config: Value =
            serde_json::from_str(include_str!("../../runtime/docker-installers.json"))
                .map_err(err)?;
        progress(
            app,
            "download",
            "Downloading Docker Desktop…",
            Some(0),
            true,
        );
        let root = state.store.root.join("setup");
        let path = download(app, control, &root, &config["installers"][&key]).await?;
        cancelled(control)?;
        control.installing.store(true, Ordering::SeqCst);
        progress(
            app,
            "install",
            "Confirm the system installer. Docker will ask you to accept its terms.",
            None,
            false,
        );
        let result = install(&path, &root, &key).await;
        control.installing.store(false, Ordering::SeqCst);
        result?;
    }
    cancelled(control)?;
    progress(
        app,
        "start",
        "Starting Docker. Complete any Docker setup prompts to continue.",
        None,
        true,
    );
    start_desktop().await?;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(600);
    while tokio::time::Instant::now() < deadline {
        cancelled(control)?;
        let checked = runtime::preflight(&state.image()).await;
        if checked["platformMismatch"] == true {
            return Err(checked["message"].as_str().unwrap_or("Select Linux containers in Docker Desktop").into());
        }
        if checked["docker"] == true {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    Err("Docker is still waiting for setup. Complete its prompts (or the requested Windows restart), then click Prepare environment again.".into())
}
async fn docker_stage(
    app: &AppHandle,
    control: &Control,
    message: &str,
    args: &[String],
) -> Result<String> {
    progress(app, "runtime", message, None, true);
    let task = runtime::docker(args, None, 1800);
    tokio::pin!(task);
    let mut interval = tokio::time::interval(Duration::from_secs(3));
    let start = std::time::Instant::now();
    loop {
        tokio::select! {
            result=&mut task => return result,
            _=interval.tick()=> { cancelled(control)?; progress(app,"runtime",&format!("{message} · {}s",start.elapsed().as_secs()),None,true); }
        }
    }
}
fn remote_runtime_identity(bundle: &Value, loaded: &Value) -> Result<String> {
    let id = loaded["Id"].as_str().ok_or("Docker did not report a runtime image ID")?;
    let valid_digest = |value: &str| value.strip_prefix("sha256:")
        .is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()));
    // The registry digest identifies the download. Docker's local image ID is
    // separate; it must equal the ID inspected and pinned by the publisher.
    let known = bundle["imageId"].as_str() == Some(id)
        || bundle["imageIds"].as_array().is_some_and(|ids| ids.iter().any(|candidate| candidate.as_str()==Some(id)));
    if !valid_digest(id) || !known {
        return Err(format!("Downloaded runtime identity did not match this app release (Docker reported {id}). Retry Prepare environment."));
    }
    let arch = match bundle["architecture"].as_str() {
        Some("aarch64") => "arm64",
        Some("x86_64") => "amd64",
        _ => return Err("Unsupported runtime architecture".into()),
    };
    if loaded["Architecture"] != arch || loaded["Os"] != "linux" {
        return Err("Downloaded runtime platform did not match this app release".into());
    }
    Ok(id.into())
}

fn remote_runtime_reference(bundle: &Value) -> Result<&str> {
    if bundle["published"] != true {
        return Err("This build has no published Docker Hub runtime. Ask the publisher for a complete app release.".into());
    }
    let image = bundle["image"].as_str().ok_or("Missing Docker Hub runtime reference")?;
    let Some((repository, digest)) = image.split_once("@sha256:") else {
        return Err("The runtime must be pinned to a Docker Hub digest".into());
    };
    if !repository.starts_with("docker.io/") || repository.len() <= "docker.io/".len()
        || !repository["docker.io/".len()..].contains('/')
        || !repository.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"/._-".contains(&b))
        || digest.len() != 64 || !digest.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("Invalid Docker Hub runtime digest".into());
    }
    if !bundle["imageId"].as_str().is_some_and(|id| id.strip_prefix("sha256:").is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))) {
        return Err("Invalid pinned runtime image ID".into());
    }
    if !bundle["imageIds"].as_array().is_some_and(|ids| !ids.is_empty() && ids.iter().all(|value| value.as_str().is_some_and(|id| id.strip_prefix("sha256:").is_some_and(|hash| hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))))) {
        return Err("Invalid published runtime identity list".into());
    }
    Ok(image)
}

async fn inspect_remote_runtime(bundle: &Value, reference: &str) -> Result<String> {
    let raw = runtime::docker(&["image".into(), "inspect".into(), reference.into()], None, 30).await?;
    let images: Value = serde_json::from_str(&raw).map_err(err)?;
    remote_runtime_identity(bundle, &images[0])
}

pub async fn prepare(app: &AppHandle, state: &AppState) -> Result<Value> {
    let control = Arc::new(Control::default());
    {
        let mut task = state.setup.lock().map_err(err)?;
        if task.is_some() {
            return Err("Environment setup is already running".into());
        }
        *task = Some(control.clone());
    }
    let _session = Session { state };
    let result: Result<Value>=async {
        progress(app,"check","Checking your local environment…",None,true);
        // A release pins one immutable, public Docker Hub manifest. No image
        // archive or publisher credentials are shipped with the application.
        let release: Value=serde_json::from_str(include_str!("../../runtime/runtime-release.json")).map_err(err)?;
        let bundle=crate::runtime_manifest::selected(&release)?;
        let reference=remote_runtime_reference(bundle)?;
        ensure_docker(app,state,&control).await?;
        cancelled(&control)?;
        let mut cached=None;
        for candidate in bundle["imageIds"].as_array().ok_or("Missing runtime image IDs")? {
            if let Some(candidate)=candidate.as_str() {
                if let Ok(id)=inspect_remote_runtime(bundle,candidate).await { cached=Some(id); break; }
            }
        }
        let image=match cached {
            Some(id)=>id, // Cache: a verified copy can prepare offline.
            None=>{
                docker_stage(app,&control,"Downloading the conversion environment from Docker Hub…",&["pull".into(),reference.into()]).await.map_err(|e| if e.contains("429") || e.contains("toomanyrequests") { "Docker Hub's download limit was reached. Sign in to Docker Desktop, then retry Prepare environment.".to_string() } else { e })?;
                inspect_remote_runtime(bundle,reference).await?
            }
        };
        for (label,dependency) in [("WordPress",runtime::WORDPRESS_IMAGE),("database",runtime::DATABASE_IMAGE)] {
            if runtime::docker(&["image".into(),"inspect".into(),dependency.into()],None,20).await.is_err() {
                docker_stage(app,&control,&format!("Downloading the prepared {label}…"),&["pull".into(),dependency.into()]).await?;
            }
        }
        let check=docker_stage(app,&control,"Verifying conversion tools…",&["run".into(),"--rm".into(),"--network".into(),"none".into(),image.clone(),"python3".into(),"/opt/desktop/selfcheck.py".into()]).await?;
        if !check.contains("RUNTIME_OK") { return Err("The prepared runtime failed its self-check".into()); }
        cancelled(&control)?;
        // The html2wp plugin comes from GitHub: fetched now when none is installed.
        progress(app,"plugin","Downloading the html2wp plugin…",None,true);
        state.ensure_plugin_with_image(&image).await?;
        let mut status=runtime::preflight(&image).await;
        if status["ready"]!=true { return Err(status["message"].as_str().unwrap_or("Environment is not ready").into()); }
        cancelled(&control)?;
        state.store.set("active-runtime",&image)?;
        status["pluginReady"]=json!(true);
        progress(app,"ready","Your environment is ready. Connect with ChatGPT to continue.",Some(100),false);
        Ok(status)
    }.await;
    if let Err(ref message) = result {
        progress(app, "error", message, None, false);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_runtime_requires_published_digest_and_exact_local_identity() {
        let config = format!("sha256:{}", "a".repeat(64));
        let manifest = format!("sha256:{}", "b".repeat(64));
        let other = format!("sha256:{}", "c".repeat(64));
        let bundle = json!({"published":true,"image":format!("docker.io/example/html2wp-runtime@{manifest}"),"imageId":config,"imageIds":[config,manifest],"architecture":"aarch64"});
        assert_eq!(remote_runtime_reference(&bundle).unwrap(),bundle["image"].as_str().unwrap());
        let loaded = json!({"Id":config,"Architecture":"arm64","Os":"linux"});
        assert_eq!(remote_runtime_identity(&bundle, &loaded).unwrap(), config);
        let loaded = json!({"Id":manifest,"Architecture":"arm64","Os":"linux"});
        assert_eq!(remote_runtime_identity(&bundle, &loaded).unwrap(), manifest);
        let mut loaded = json!({"Id":other,"Architecture":"arm64","Os":"linux"});
        assert!(remote_runtime_identity(&bundle, &loaded).unwrap_err().contains(&other));
        loaded["Id"] = json!(manifest);
        loaded["Architecture"] = json!("amd64");
        assert!(remote_runtime_identity(&bundle, &loaded).is_err());
        loaded["Architecture"] = json!("arm64");
        loaded["Os"] = json!("windows");
        assert!(remote_runtime_identity(&bundle, &loaded).is_err());
        for invalid in [json!({"published":false,"image":bundle["image"],"imageId":config}),json!({"published":true,"image":"example/runtime:latest","imageId":config}),json!({"published":true,"image":"docker.io/example/runtime@sha256:bad","imageId":config})] {
            assert!(remote_runtime_reference(&invalid).is_err());
        }
    }

    #[tokio::test]
    #[ignore = "downloads the pinned public runtime from Docker Hub and runs its self-check"]
    async fn docker_hub_runtime_acceptance() {
        let release: Value = serde_json::from_str(include_str!("../../runtime/runtime-release.json")).unwrap();
        let bundle = crate::runtime_manifest::selected(&release).unwrap();
        let reference = remote_runtime_reference(bundle).unwrap();
        runtime::docker(&["pull".into(), reference.into()], None, 1800).await.unwrap();
        let image = inspect_remote_runtime(bundle,reference).await.unwrap();
        let result = runtime::docker(&["run".into(), "--rm".into(), "--network".into(), "none".into(), image.clone(), "python3".into(), "/opt/desktop/selfcheck.py".into()], None, 120).await.unwrap();
        assert!(result.contains("RUNTIME_OK"));
        println!("Pulled, verified and executed Docker Hub runtime by immutable ID: {image}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn docker_requirement_compiles_as_inline_source() {
        let directory = tempfile::tempdir().unwrap();
        let compiled = directory.path().join("docker.csreq");
        command("/usr/bin/csreq", &["-r", DOCKER_SIGNING_REQUIREMENT, "-b", &compiled.to_string_lossy()], 10)
            .await.expect("Docker's requirement must be parsed as source, not a filename");
        assert!(std::fs::metadata(compiled).unwrap().len() > 0);
    }

    #[test]
    fn installer_selection_respects_os_arch_and_distribution() {
        assert_eq!(
            installer_key("macos", "aarch64", "").unwrap(),
            "macos-aarch64"
        );
        assert_eq!(
            installer_key("windows", "x86_64", "").unwrap(),
            "windows-x86_64"
        );
        assert_eq!(
            installer_key("linux", "x86_64", "ID=linuxmint\nID_LIKE=\"ubuntu debian\"").unwrap(),
            "linux-deb"
        );
        assert_eq!(
            installer_key("linux", "x86_64", "ID=fedora").unwrap(),
            "linux-rpm"
        );
        assert!(installer_key("linux", "aarch64", "ID=ubuntu").is_err());
        assert!(installer_key("linux", "x86_64", "ID=arch").is_err());
    }
}
