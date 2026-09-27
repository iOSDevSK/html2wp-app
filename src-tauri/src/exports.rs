//! Exports: the files a run delivered, kept per revision in the project's
//! artifacts folder and saved wherever the owner chooses.
use crate::{files, model::*, store::Store};
use std::path::Path;

/// Copy a delivered file out of the app, refusing one that changed on disk
/// since it was recorded, and any destination inside the app's own data.
pub fn save(store: &Store, p: &Project, artifact_id: &str, dest: &Path) -> Result<()> {
    let a = p.artifacts.iter().find(|a| a.id == artifact_id).ok_or("Unknown file")?;
    let source = files::resolve(&store.path(&p.id)?, &format!("artifacts/revision-{}/{}", a.revision, a.filename))?;
    if files::hash(&source)? != a.sha256 {
        return Err("This file changed on disk since the conversion delivered it. Run the conversion again for a new copy.".into());
    }
    if dest.starts_with(&store.root) {
        return Err("Choose an export destination outside the application's private data".into());
    }
    std::fs::copy(source, dest).map_err(err)?;
    Ok(())
}

/// Remove one historical delivery, leaving the current revision and any
/// externally saved copies alone. Update the record first: if the process
/// exits before unlinking, only an unreferenced file remains, never a broken
/// export entry. A failed unlink restores the record.
pub fn delete_previous(store: &Store, mut p: Project, artifact_id: &str) -> Result<Project> {
    let a = p.artifacts.iter().find(|a| a.id == artifact_id).ok_or("Unknown file")?.clone();
    if a.revision == p.revision {
        return Err("The current revision cannot be deleted from Previous versions".into());
    }
    if p.artifacts.iter().any(|other| other.id != a.id && other.revision == a.revision && other.filename == a.filename) {
        return Err("Another artifact still uses this file".into());
    }
    let root = store.path(&p.id)?;
    let source = files::resolve(&root, &format!("artifacts/revision-{}/{}", a.revision, a.filename))?;
    if !source.is_file() {
        return Err("The previous version file is missing".into());
    }
    let original = p.clone();
    p.artifacts.retain(|other| other.id != a.id);
    store.put(&p)?;
    if let Err(e) = std::fs::remove_file(&source) {
        store.put(&original)?;
        return Err(err(e));
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_delivered_file_is_saved_only_as_it_was_delivered() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("data")).unwrap();
        let mut p = crate::skill::tests::project();
        let revision = store.path(&p.id).unwrap().join("artifacts/revision-1");
        std::fs::create_dir_all(&revision).unwrap();
        std::fs::write(revision.join("site.zip"), b"zip").unwrap();
        p.artifacts.push(Artifact { id: "a".into(), revision: 1, filename: "site.zip".into(), sha256: files::hash(&revision.join("site.zip")).unwrap(), kind: "theme".into(), reviewed: true, ..Default::default() });
        let out = dir.path().join("site.zip");
        save(&store, &p, "a", &out).unwrap();
        assert_eq!(std::fs::read(&out).unwrap(), b"zip");
        assert!(save(&store, &p, "missing", &out).is_err());
        assert!(save(&store, &p, "a", &store.root.join("copy.zip")).unwrap_err().contains("outside"));
        std::fs::write(revision.join("site.zip"), b"changed").unwrap();
        assert!(save(&store, &p, "a", &out).unwrap_err().contains("changed on disk"));
    }

    #[test]
    fn deletes_only_the_chosen_previous_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().join("data")).unwrap();
        let mut p = crate::skill::tests::project();
        p.revision = 2;
        let root = store.path(&p.id).unwrap();
        for revision in [1, 2] {
            let folder = root.join(format!("artifacts/revision-{revision}"));
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::write(folder.join("site.zip"), b"zip").unwrap();
            p.artifacts.push(Artifact { id: format!("a{revision}"), revision, filename: "site.zip".into(), ..Default::default() });
        }
        store.put(&p).unwrap();
        assert!(delete_previous(&store, p.clone(), "a2").unwrap_err().contains("current revision"));
        assert!(root.join("artifacts/revision-2/site.zip").exists());
        let result = delete_previous(&store, p, "a1").unwrap();
        assert_eq!(result.artifacts.len(), 1);
        assert_eq!(store.project(&result.id).unwrap().artifacts.len(), 1);
        assert!(!root.join("artifacts/revision-1/site.zip").exists());
        assert!(root.join("artifacts/revision-2/site.zip").exists());
    }
}
