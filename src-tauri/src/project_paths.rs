//! A native project path and the exact path seen by the local Linux Docker daemon.
//! Windows Docker Desktop translates bind sources before passing them to the VM.
use crate::{model::*, runtime};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Write,
    path::{Path, PathBuf},
};

pub const DAEMON_PATH_LABEL: &str = "dev.html2wp.daemon-path";
pub const HOST_PATH_LABEL: &str = "dev.html2wp.host-path-sha256";
const PROOF_FILE: &str = "/run/html2wp-project-path-proof";

#[derive(Clone, Debug)]
pub struct ProjectPaths {
    pub host: PathBuf,
    pub daemon: String,
}

fn valid_daemon_path(path: &str) -> bool {
    path.starts_with('/')
        && path != "/"
        && !path.chars().any(|c| ['\\', ':', ',', '\0'].contains(&c))
        && path
            .split('/')
            .skip(1)
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn host_text(path: &Path) -> Result<String> {
    path.to_str()
        .ok_or_else(|| "The project path must be valid UTF-8".into())
        .map(str::to_owned)
}

pub fn host_identity(path: &Path) -> Result<String> {
    Ok(format!("{:x}", Sha256::digest(host_text(path)?.as_bytes())))
}

fn proof_value(host: &Path, daemon: &str) -> Result<String> {
    let mut digest = Sha256::new();
    digest.update(host_text(host)?.as_bytes());
    digest.update([0]);
    digest.update(daemon.as_bytes());
    Ok(format!("{:x}", digest.finalize()))
}

fn native_form(path: &str) -> String {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    path.replace('\\', "/")
}

fn source_matches(found: &str, native: &Path, daemon: &str) -> bool {
    found == daemon
        || native_form(found).eq_ignore_ascii_case(&native_form(&native.to_string_lossy()))
}

fn windows_daemon_candidates(path: &str) -> Vec<String> {
    let path = path.strip_prefix(r"\\?\").unwrap_or(path);
    let bytes = path.as_bytes();
    if bytes.len() < 3
        || !bytes[0].is_ascii_alphabetic()
        || bytes[1] != b':'
        || !matches!(bytes[2], b'\\' | b'/')
    {
        return vec![];
    }
    let drive = (bytes[0] as char).to_ascii_lowercase();
    let suffix = path[3..].replace('\\', "/");
    if suffix.split('/').any(|part| {
        part.is_empty() || part == "." || part == ".." || part.contains([',', ':', '\0'])
    }) {
        return vec![];
    }
    [
        format!("/run/desktop/mnt/host/{drive}/{suffix}"),
        format!("/host_mnt/{drive}/{suffix}"),
    ]
    .into_iter()
    .filter(|candidate| valid_daemon_path(candidate))
    .collect()
}

fn mount_source(raw: &str, destination: &str) -> Option<String> {
    let mounts: Value = serde_json::from_str(raw.trim()).ok()?;
    let rows = mounts.as_array()?;
    let mut rows = rows
        .iter()
        .filter(|m| m["Type"] == "bind" && m["Destination"] == destination);
    let source = rows.next()?["Source"].as_str()?.to_string();
    rows.next().is_none().then_some(source)
}

impl ProjectPaths {
    async fn cached(host: &Path) -> Option<Self> {
        let project_id = host.file_name()?.to_str()?;
        let agent = runtime::project_name(project_id)
            .ok()
            .map(|name| format!("{name}-agent"))?;
        let raw = runtime::docker(
            &[
                "inspect".into(),
                "--format".into(),
                "{{json .}}".into(),
                agent.clone(),
            ],
            None,
            15,
        )
        .await
        .ok()?;
        let value: Value = serde_json::from_str(raw.trim()).ok()?;
        if value["State"]["Running"] != true
            || value["Config"]["Labels"]["dev.html2wp.desktop"] != "true"
        {
            return None;
        }
        let daemon = value["Config"]["Labels"][DAEMON_PATH_LABEL].as_str()?;
        let expected_host = host_identity(host).ok()?;
        if !valid_daemon_path(daemon) || value["Config"]["Labels"][HOST_PATH_LABEL] != expected_host
        {
            return None;
        }
        let proof = runtime::docker(
            &[
                "exec".into(),
                "--user".into(),
                "0".into(),
                agent,
                "cat".into(),
                PROOF_FILE.into(),
            ],
            None,
            15,
        )
        .await
        .ok()?;
        (proof.trim() == proof_value(host, daemon).ok()?).then(|| Self {
            host: host.to_path_buf(),
            daemon: daemon.into(),
        })
    }

    pub async fn discover(project: &Path, image: &str) -> Result<Self> {
        let host = dunce::canonicalize(project).map_err(err)?;
        let native = host_text(&host)?;
        if !cfg!(windows) {
            return Ok(Self {
                host,
                daemon: native,
            });
        }
        if let Some(found) = Self::cached(&host).await {
            return Ok(found);
        }

        std::fs::create_dir_all(host.join(".tmp")).map_err(err)?;
        let nonce = format!("path-proof-{}", uuid::Uuid::new_v4());
        let nonce_file = host.join(".tmp").join(&nonce);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&nonce_file)
            .map_err(err)?;
        file.write_all(nonce.as_bytes()).map_err(err)?;
        drop(file);

        // The probe uses the same Linux runtime and mounted Docker socket as
        // the project agent. This avoids trusting host-side path rewriting.
        let probe = format!("h2wp-path-probe-{}", uuid::Uuid::new_v4());
        let nested = format!("h2wp-path-read-{}", uuid::Uuid::new_v4());
        let outcome = async {
            runtime::docker(&[
                "create".into(), "--name".into(), probe.clone(), "--label".into(),
                "dev.html2wp.desktop=true".into(), "--network".into(), "none".into(),
                "--user".into(), "1000:1000".into(), "--group-add".into(), "0".into(),
                "--mount".into(), format!("type=bind,source={native},target=/h2wp-native,readonly"),
                "--mount".into(), "type=bind,source=/var/run/docker.sock,target=/var/run/docker.sock".into(),
                image.into(), "sleep".into(), "120".into(),
            ], None, 60).await.map_err(|e| format!("Docker could not share the project folder: {e}"))?;
            runtime::docker(&["start".into(), probe.clone()], None, 30).await?;

            let mut candidates = vec![];
            if let Ok(raw) = runtime::docker(&[
                "exec".into(), probe.clone(), "docker".into(), "inspect".into(), "--format".into(),
                "{{json .Mounts}}".into(), probe.clone(),
            ], None, 20).await {
                if let Some(source) = mount_source(&raw, "/h2wp-native").filter(|source| valid_daemon_path(source)) {
                    candidates.push(source);
                }
            }
            candidates.extend(windows_daemon_candidates(&native));
            candidates.dedup();
            if candidates.is_empty() {
                return Err("Docker Desktop cannot safely translate this project location. Store the project on a local Windows drive and retry.".into());
            }
            for candidate in candidates {
                let read = runtime::docker(&[
                    "exec".into(), probe.clone(), "docker".into(), "run".into(), "--rm".into(),
                    "--name".into(), nested.clone(), "--label".into(), "dev.html2wp.desktop=true".into(),
                    "--network".into(), "none".into(), "--mount".into(),
                    format!("type=bind,source={candidate},target=/h2wp-proof,readonly"),
                    image.into(), "cat".into(), format!("/h2wp-proof/.tmp/{nonce}"),
                ], None, 90).await;
                let _ = runtime::docker(&["rm".into(), "-f".into(), nested.clone()], None, 20).await;
                if read.is_ok_and(|read| read == nonce) {
                    return Ok(Self { host: host.clone(), daemon: candidate });
                }
            }
            Err("Docker Desktop did not expose the project to nested Linux builds. Use the WSL 2 Linux containers backend and retry.".into())
        }.await;
        let _ = runtime::docker(&["rm".into(), "-f".into(), nested], None, 20).await;
        let _ = runtime::docker(&["rm".into(), "-f".into(), probe], None, 20).await;
        let _ = std::fs::remove_file(nonce_file);
        outcome
    }

    pub async fn agent_is_proven(&self, agent: &str) -> bool {
        let proof = runtime::docker(
            &[
                "exec".into(),
                "--user".into(),
                "0".into(),
                agent.into(),
                "cat".into(),
                PROOF_FILE.into(),
            ],
            None,
            15,
        )
        .await;
        matches!(proof, Ok(value) if value.trim() == proof_value(&self.host, &self.daemon).unwrap_or_default())
    }

    pub async fn mark_agent_proven(&self, agent: &str) -> Result<()> {
        let proof = proof_value(&self.host, &self.daemon)?;
        runtime::docker(
            &[
                "exec".into(),
                "-i".into(),
                "--user".into(),
                "0".into(),
                agent.into(),
                "sh".into(),
                "-c".into(),
                format!("umask 077; cat > {PROOF_FILE}"),
            ],
            Some(proof.as_bytes()),
            20,
        )
        .await
        .map(|_| ())
    }

    pub async fn verify_agent(&self, agent: &str, image: &str) -> Result<()> {
        if !cfg!(windows) {
            return Ok(());
        }
        let nonce = format!("path-proof-{}", uuid::Uuid::new_v4());
        let file = self.host.join(".tmp").join(&nonce);
        let write_file = self.host.join(".tmp").join(format!("{nonce}-write"));
        std::fs::write(&file, nonce.as_bytes()).map_err(err)?;
        let result = async {
            let mounts_raw = runtime::docker(
                &[
                    "inspect".into(),
                    "--format".into(),
                    "{{json .Mounts}}".into(),
                    agent.into(),
                ],
                None,
                20,
            )
            .await?;
            let mounts: Value = serde_json::from_str(mounts_raw.trim()).map_err(err)?;
            let mounts = mounts.as_array().ok_or("Docker returned no agent mounts")?;
            let expected = |destination: &str, writable: bool, native: &Path| {
                let rows = mounts
                    .iter()
                    .filter(|m| m["Type"] == "bind" && m["Destination"] == destination)
                    .collect::<Vec<_>>();
                rows.len() == 1
                    && rows[0]["RW"] == writable
                    && rows[0]["Source"]
                        .as_str()
                        .is_some_and(|source| source_matches(source, native, destination))
            };
            if !expected(&self.daemon, true, &self.host)
                || !expected(
                    &format!("{}/input", self.daemon),
                    false,
                    &self.host.join("input"),
                )
                || !expected(
                    &format!("{}/artifacts", self.daemon),
                    false,
                    &self.host.join("artifacts"),
                )
            {
                return Err(
                    "Docker did not preserve the project's writable and read-only mounts".into(),
                );
            }
            let path = format!("{}/.tmp/{nonce}", self.daemon);
            let direct =
                runtime::docker(&["exec".into(), agent.into(), "cat".into(), path], None, 20)
                    .await?;
            if direct != nonce {
                return Err("The conversion container cannot read this project's files".into());
            }
            runtime::docker(
                &[
                    "exec".into(),
                    agent.into(),
                    "sh".into(),
                    "-c".into(),
                    "printf '%s' \"$1\" > \"$2\"".into(),
                    "sh".into(),
                    nonce.clone(),
                    format!("{}/.tmp/{nonce}-write", self.daemon),
                ],
                None,
                20,
            )
            .await?;
            if std::fs::read(&write_file).map_err(err)? != nonce.as_bytes() {
                return Err(
                    "The conversion container cannot write this project's scratch files".into(),
                );
            }
            let nested_name = format!("h2wp-agent-path-proof-{}", uuid::Uuid::new_v4());
            let nested = runtime::docker(
                &[
                    "exec".into(),
                    agent.into(),
                    "docker".into(),
                    "run".into(),
                    "--rm".into(),
                    "--name".into(),
                    nested_name.clone(),
                    "--label".into(),
                    "dev.html2wp.desktop=true".into(),
                    "--network".into(),
                    "none".into(),
                    "--mount".into(),
                    format!(
                        "type=bind,source={},target=/h2wp-nested-proof,readonly",
                        self.daemon
                    ),
                    image.into(),
                    "cat".into(),
                    format!("/h2wp-nested-proof/.tmp/{nonce}"),
                ],
                None,
                90,
            )
            .await;
            let _ = runtime::docker(&["rm".into(), "-f".into(), nested_name], None, 20).await;
            if nested? != nonce {
                return Err("The nested Docker build does not see the same project files".into());
            }
            Ok(())
        }
        .await;
        let _ = std::fs::remove_file(file);
        let _ = std::fs::remove_file(write_file);
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn daemon_source_must_be_one_absolute_linux_path() {
        assert!(valid_daemon_path(
            "/run/desktop/mnt/host/c/Users/Filip Proj"
        ));
        for bad in [
            "C:\\Users\\Filip",
            "/",
            "relative/path",
            "/tmp/../other",
            "/tmp//other",
            "/tmp,other",
        ] {
            assert!(!valid_daemon_path(bad), "{bad}");
        }
    }

    #[test]
    fn local_windows_drive_has_only_bounded_docker_desktop_candidates() {
        assert_eq!(
            windows_daemon_candidates(r"C:\Users\Filip D\AppData\Roaming\html2wp"),
            [
                "/run/desktop/mnt/host/c/Users/Filip D/AppData/Roaming/html2wp",
                "/host_mnt/c/Users/Filip D/AppData/Roaming/html2wp",
            ]
        );
        assert!(
            windows_daemon_candidates(r"\\server\share\project").is_empty(),
            "UNC paths are not guessed"
        );
        assert!(windows_daemon_candidates(r"C:\safe\..\other").is_empty());
        assert!(windows_daemon_candidates(r"C:\comma,folder\project").is_empty());
    }

    #[test]
    fn mount_source_accepts_exact_native_or_proven_alias_only() {
        let native = Path::new(r"C:\Users\Filip\project");
        let alias = "/run/desktop/mnt/host/c/Users/Filip/project";
        assert!(source_matches(r"C:\Users\Filip\project", native, alias));
        assert!(source_matches("C:/Users/Filip/project", native, alias));
        assert!(source_matches(alias, native, alias));
        assert!(!source_matches("/host_mnt/c/Users/other", native, alias));
    }
}
