use crate::{files, model::*, runtime, store::Store};
use std::{fs, path::PathBuf};

fn project_paths(store: &Store, id: &str) -> Result<(PathBuf, PathBuf)> {
    let root = store.path(id)?;
    let private = store.root.join("private").join(id);
    for path in [&root, &private] {
        if path.is_symlink() { return Err("Project folders must not be symbolic links".into()); }
    }
    Ok((root, private))
}

pub async fn change(store: &Store, id: &str, action: &str) -> Result<Option<Project>> {
    let mut p = store.project(id)?;
    if action == "restore" {
        p.archived = false;
    } else if action == "remove" {
        // Removing from the workspace is reversible and keeps local data.
        p.archived = true;
        p.auto_approve = false;
        p.conversion_approval_required = false;
        store.set(&format!("queue:{id}"), "[]")?;
    } else if matches!(action, "delete" | "restart") {
        let (root, private) = project_paths(store, id)?;
        // Verify the immutable input before removing any generated work.
        if action == "restart" && !p.from_theme() { files::detect(&root.join("input"))?; }
        // The plugin's preview WordPress first: its state files are in the workspace.
        crate::preview::remove(store, &p).await?;
        crate::h2g_preview::stop(&p.id);
        if p.thread_id.is_some() || p.last_step.is_some() || p.preview.is_some() || private.exists() {
            runtime::cleanup_project(id).await?;
        }
        if action == "delete" {
            let original = crate::project_downloads::original_path(store, &p);
            if original.exists() { fs::remove_file(original).map_err(err)?; }
            if root.exists() { fs::remove_dir_all(&root).map_err(err)?; }
            if private.exists() { fs::remove_dir_all(private).map_err(err)?; }
            store.clear_project_history(id, true)?;
            return Ok(None);
        }
        restart_files(store, &mut p)?;
    } else { return Err("Unknown project action".into()); }
    p.updated_at = now();
    store.put(&p)?;
    Ok(Some(p))
}

/// Clean & restart: the project as it was imported, its run and chat gone.
/// The original input is checked first; nothing else is kept.
fn restart_files(store: &Store, p: &mut Project) -> Result<()> {
    let (root, private) = project_paths(store, &p.id)?;
    let (kind, pages) = if p.from_theme() { crate::h2g::check_input(&root.join("input"))?; (crate::h2g::KIND.to_string(), vec![]) } else { files::detect(&root.join("input"))? };
    for name in ["source", "workspace", "out", ".tmp", "artifacts"] {
        let path = root.join(name);
        if path.is_symlink() { fs::remove_file(path).map_err(err)?; }
        else if path.exists() { fs::remove_dir_all(path).map_err(err)?; }
    }
    for name in ["workspace", "artifacts"] { fs::create_dir_all(root.join(name)).map_err(err)?; }
    // The plugin's copy of the input, fresh from the original.
    if !p.from_theme() { files::copy_input(&root.join("input"), &root.join("source"))?; }
    if private.exists() { fs::remove_dir_all(&private).map_err(err)?; }
    store.clear_project_history(&p.id, false)?;
    p.kind = kind;
    p.pages = pages;
    p.phase = "imported".into();
    p.last_step = None;
    p.revision += 1;
    p.thread_id = None;
    p.preview = None;
    p.gates.clear();
    p.artifacts.clear();
    p.last_error = None;
    p.reporting = "not_required".into();
    p.auto_approve = false;
    p.conversion_approval_required = false;
    p.archived = false;
    store.message(&p.id, "assistant", "Project reset to its original import. Its containers, preview and generated files were removed. Start the conversion again when you are ready.")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn restart_keeps_the_original_and_other_projects_and_clears_the_run() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        let mut p: Project = serde_json::from_value(json!({"id":id(),"name":"site","sourceName":"site.zip","kind":"Static HTML","createdAt":"","updatedAt":"","phase":"failed","revision":5,"threadId":"old-thread","pages":[],"gates":[],"artifacts":[],"preview":null,"runtimeImage":"pinned","pluginCommit":"pinned","reporting":"pending","lastError":"old","autoApprove":true})).unwrap();
        let root = store.path(&p.id).unwrap();
        fs::create_dir_all(root.join("input")).unwrap();
        fs::write(root.join("input/index.html"), "<html>original</html>").unwrap();
        for name in ["source", "workspace", "artifacts"] { fs::create_dir_all(root.join(name)).unwrap(); fs::write(root.join(name).join("old"), "old").unwrap(); }
        let other = store.root.join("projects").join(id());
        fs::create_dir_all(&other).unwrap(); fs::write(other.join("keep"), "keep").unwrap();
        store.put(&p).unwrap();
        store.set(&format!("goal-reactivations:{}",p.id), "3").unwrap();
        store.message(&p.id,"user","old chat").unwrap();
        let private = store.root.join("private").join(&p.id);
        fs::create_dir_all(&private).unwrap();
        fs::write(private.join("wordpress.json"), "{}").unwrap();
        restart_files(&store, &mut p).unwrap();
        assert_eq!(p.phase,"imported"); assert_eq!(p.revision,6); assert!(!p.auto_approve);
        assert!(p.thread_id.is_none());
        assert_eq!(fs::read(root.join("input/index.html")).unwrap(), b"<html>original</html>");
        assert!(!root.join("workspace/old").exists() && !root.join("source/old").exists() && !private.exists());
        assert_eq!(fs::read(root.join("source/index.html")).unwrap(), b"<html>original</html>", "the plugin's copy, fresh from the original");
        assert!(other.join("keep").exists());
        assert!(store.setting(&format!("goal-reactivations:{}",p.id)).is_none());
        assert_eq!(store.messages(&p.id).unwrap().len(), 1, "only the reset notice");
        store.clear_project_history(&p.id, true).unwrap();
        assert!(store.project(&p.id).is_err()); assert!(store.messages(&p.id).unwrap().is_empty());
    }
}

#[cfg(test)]
mod docker_acceptance {
    use super::*;
    use serde_json::json;
    async fn docker(args: &[&str]) -> Result<String> {
        runtime::docker(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>(),None,60).await
    }
    #[tokio::test]
    #[ignore = "requires Docker and H2WP_LIFECYCLE_SOURCE; only creates disposable test projects"]
    async fn lovable_project_cleanup_acceptance() {
        let source = PathBuf::from(std::env::var("H2WP_LIFECYCLE_SOURCE").unwrap());
        let directory = tempfile::tempdir_in(PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().join(".cache")).unwrap();
        let store = Store::open(directory.path().to_path_buf()).unwrap();
        let pid = id(); let other = id();
        let name = runtime::project_name(&pid).unwrap();
        let other_name = runtime::project_name(&other).unwrap();
        let root = store.path(&pid).unwrap();
        files::copy_input(&source,&root.join("input")).unwrap();
        let (kind,pages) = files::detect(&root.join("input")).unwrap();
        for dir in ["workspace","artifacts"] { fs::create_dir_all(root.join(dir)).unwrap(); }
        let p: Project = serde_json::from_value(json!({"id":pid,"name":"Lovable cleanup acceptance","sourceName":"lovable.zip","kind":kind,"pages":pages,"createdAt":now(),"updatedAt":now(),"phase":"failed","revision":1,"threadId":"old","gates":[],"artifacts":[],"preview":null,"runtimeImage":"html2wp-runtime:desktop-0.1.0","pluginCommit":"fixture","reporting":"not_required","lastError":null})).unwrap();
        store.put(&p).unwrap();
        let result: Result<()> = async {
            for project in [&name,&other_name] {
                docker(&["volume","create","--label","dev.html2wp.desktop=true","--label",&format!("com.docker.compose.project={project}"),&format!("{project}_wp")]).await?;
            }
            docker(&["network","create","--label",&format!("com.docker.compose.project={name}"),&format!("{name}_default")]).await?;
            docker(&["run","-d","--name",&format!("{name}-wp-1"),"--label","dev.html2wp.desktop=true","--network",&format!("{name}_default"),"--mount",&format!("type=volume,source={name}_wp,target=/fixture"),&p.runtime_image,"sleep","120"]).await?;
            change(&store,&pid,"remove").await?;
            assert!(store.project(&pid)?.archived);
            docker(&["container","inspect",&format!("{name}-wp-1")]).await?;
            change(&store,&pid,"restore").await?;
            assert!(!store.project(&pid)?.archived);
            change(&store,&pid,"restart").await?;
            let restarted = store.project(&pid)?;
            assert_eq!(restarted.phase,"imported"); assert!(restarted.thread_id.is_none());
            assert_eq!(files::detect(&root.join("input"))?.0,files::detect(&root.join("source"))?.0);
            assert!(docker(&["container","inspect",&format!("{name}-wp-1")]).await.is_err());
            assert!(docker(&["volume","inspect",&format!("{name}_wp")]).await.is_err());
            assert!(docker(&["network","inspect",&format!("{name}_default")]).await.is_err());
            docker(&["volume","inspect",&format!("{other_name}_wp")]).await?;
            change(&store,&pid,"delete").await?;
            assert!(!root.exists()); assert!(store.project(&pid).is_err());
            docker(&["volume","inspect",&format!("{other_name}_wp")]).await?;
            Ok(())
        }.await;
        let cleanup = runtime::cleanup_project(&pid).await;
        let cleanup_other = runtime::cleanup_project(&other).await;
        result.unwrap(); cleanup.unwrap(); cleanup_other.unwrap();
        println!("Lovable import, remove/restore, live container + volume + network cleanup, restart and deletion passed; unrelated volume preserved.");
    }
}
