//! A native project path and the exact path seen by the local Linux Docker daemon.
//! Windows Docker Desktop translates bind sources before passing them to the VM.
use crate::{model::*, runtime};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct ProjectPaths {
    pub host: PathBuf,
    pub daemon: String,
}

fn valid_daemon_path(path: &str) -> bool {
    path.starts_with('/') && path != "/" && !path.chars().any(|c| ['\\', ':', ',', '\0'].contains(&c))
        && path.split('/').skip(1).all(|part| !part.is_empty() && part != "." && part != "..")
}

impl ProjectPaths {
    pub async fn discover(project: &Path, image: &str) -> Result<Self> {
        let host = dunce::canonicalize(project).map_err(err)?;
        let host_text = host.to_str().ok_or("The project path must be valid UTF-8")?.to_owned();
        if !cfg!(windows) {
            return Ok(Self { host, daemon: host_text });
        }
        // Ask this Docker daemon how it maps this exact native bind. A guessed
        // /host_mnt or /run/desktop path is unsafe and differs by backend.
        let name = format!("h2wp-path-probe-{}", uuid::Uuid::new_v4());
        let create = runtime::docker(&[
            "create".into(), "--name".into(), name.clone(), "--label".into(),
            "dev.html2wp.desktop=true".into(), "--mount".into(),
            format!("type=bind,source={host_text},target=/h2wp-path-probe,readonly"),
            image.into(), "sleep".into(), "60".into(),
        ], None, 60).await;
        create.map_err(|e| format!("Docker could not share the project folder: {e}"))?;
        let inspected = runtime::docker(&[
            "inspect".into(), "--format".into(), "{{json .Mounts}}".into(), name.clone(),
        ], None, 20).await;
        let _ = runtime::docker(&["rm".into(), "-f".into(), name], None, 20).await;
        let raw = inspected?;
        let mounts: Value = serde_json::from_str(raw.trim()).map_err(err)?;
        let rows = mounts.as_array().ok_or("Docker returned no bind mount details")?;
        let row = rows.iter().filter(|m| m["Type"] == "bind" && m["Destination"] == "/h2wp-path-probe").collect::<Vec<_>>();
        if row.len() != 1 { return Err("Docker did not identify this project's bind mount".into()); }
        let daemon = row[0]["Source"].as_str().ok_or("Docker returned no Linux bind source")?;
        if !valid_daemon_path(daemon) {
            return Err("Docker Desktop did not expose a usable Linux path for this project. Use the Linux containers backend and retry.".into());
        }
        Ok(Self { host, daemon: daemon.into() })
    }

    pub async fn verify_agent(&self, agent: &str, image: &str) -> Result<()> {
        if !cfg!(windows) { return Ok(()); }
        let nonce = format!("path-proof-{}", uuid::Uuid::new_v4());
        let file = self.host.join(".tmp").join(&nonce);
        let write_file = self.host.join(".tmp").join(format!("{nonce}-write"));
        std::fs::write(&file, nonce.as_bytes()).map_err(err)?;
        let result = async {
            let mounts_raw = runtime::docker(&[
                "inspect".into(), "--format".into(), "{{json .Mounts}}".into(), agent.into(),
            ], None, 20).await?;
            let mounts: Value = serde_json::from_str(mounts_raw.trim()).map_err(err)?;
            let mounts = mounts.as_array().ok_or("Docker returned no agent mounts")?;
            let expected = |destination: &str, writable: bool| mounts.iter().any(|m|
                m["Type"] == "bind" && m["Destination"] == destination
                    && m["Source"] == destination && m["RW"] == writable);
            if !expected(&self.daemon, true)
                || !expected(&format!("{}/input", self.daemon), false)
                || !expected(&format!("{}/artifacts", self.daemon), false) {
                return Err("Docker did not preserve the project's writable and read-only mounts".into());
            }
            let path = format!("{}/.tmp/{nonce}", self.daemon);
            let direct = runtime::docker(&[
                "exec".into(), agent.into(), "cat".into(), path,
            ], None, 20).await?;
            if direct != nonce { return Err("The conversion container cannot read this project's files".into()); }
            runtime::docker(&[
                "exec".into(), agent.into(), "sh".into(), "-c".into(),
                "printf '%s' \"$1\" > \"$2\"".into(), "sh".into(), nonce.clone(),
                format!("{}/.tmp/{nonce}-write", self.daemon),
            ], None, 20).await?;
            if std::fs::read(&write_file).map_err(err)? != nonce.as_bytes() {
                return Err("The conversion container cannot write this project's scratch files".into());
            }
            let nested = runtime::docker(&[
                "exec".into(), agent.into(), "docker".into(), "run".into(), "--rm".into(),
                "--network".into(), "none".into(), "--mount".into(),
                format!("type=bind,source={},target=/h2wp-nested-proof,readonly", self.daemon),
                image.into(), "cat".into(), format!("/h2wp-nested-proof/.tmp/{nonce}"),
            ], None, 90).await?;
            if nested != nonce { return Err("The nested Docker build does not see the same project files".into()); }
            Ok(())
        }.await;
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
        assert!(valid_daemon_path("/run/desktop/mnt/host/c/Users/Filip Proj"));
        for bad in ["C:\\Users\\Filip", "/", "relative/path", "/tmp/../other", "/tmp//other", "/tmp,other"] {
            assert!(!valid_daemon_path(bad), "{bad}");
        }
    }
}
