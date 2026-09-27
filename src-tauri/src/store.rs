use crate::model::*;
use rusqlite::{params, Connection};
use std::{
    path::{Path, PathBuf},
    sync::Mutex,
};
pub struct Store {
    db: Mutex<Connection>,
    pub root: PathBuf,
}
impl Store {
    pub fn open(root: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(root.join("projects")).map_err(err)?;
        std::fs::create_dir_all(root.join("private")).map_err(err)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(root.join("private"), std::fs::Permissions::from_mode(0o700))
                .map_err(err)?;
        }
        let db = Connection::open(root.join("desktop.sqlite3")).map_err(err)?;
        db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON; CREATE TABLE IF NOT EXISTS projects(id TEXT PRIMARY KEY,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS messages(id TEXT PRIMARY KEY,project_id TEXT NOT NULL,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS activity(id TEXT PRIMARY KEY,project_id TEXT NOT NULL,data TEXT NOT NULL); CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY,value TEXT NOT NULL); PRAGMA user_version=1;").map_err(err)?;
        Ok(Self {
            db: Mutex::new(db),
            root,
        })
    }
    pub fn path(&self, id: &str) -> Result<PathBuf> {
        uuid::Uuid::parse_str(id).map_err(|_| "Invalid project ID".to_string())?;
        Ok(self.root.join("projects").join(id))
    }
    pub fn put(&self, p: &Project) -> Result<()> {
        self.db
            .lock()
            .map_err(err)?
            .execute(
                "INSERT OR REPLACE INTO projects VALUES (?1,?2)",
                params![p.id, serde_json::to_string(p).map_err(err)?],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn project(&self, id: &str) -> Result<Project> {
        let s: String = self
            .db
            .lock()
            .map_err(err)?
            .query_row("SELECT data FROM projects WHERE id=?1", [id], |r| r.get(0))
            .map_err(err)?;
        serde_json::from_str(&s).map_err(err)
    }
    pub fn projects(&self) -> Result<Vec<Project>> {
        let db = self.db.lock().map_err(err)?;
        let mut q = db
            .prepare("SELECT data FROM projects ORDER BY rowid DESC")
            .map_err(err)?;
        let rows = q
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(err)?
            .map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
            .collect();
        rows
    }
    pub fn message(&self, p: &str, role: &str, text: &str) -> Result<Message> {
        self.message_with_action(p, role, text, None)
    }
    /// A message with a button in the chat (see Message::action).
    pub fn message_with_action(&self, p: &str, role: &str, text: &str, action: Option<&str>) -> Result<Message> {
        let m = Message {
            id: id(),
            project_id: p.into(),
            role: role.into(),
            text: text.into(),
            created_at: now(),
            action: action.map(String::from),
        };
        self.db
            .lock()
            .map_err(err)?
            .execute(
                "INSERT INTO messages VALUES (?1,?2,?3)",
                params![m.id, p, serde_json::to_string(&m).map_err(err)?],
            )
            .map_err(err)?;
        Ok(m)
    }
    pub fn messages(&self, p: &str) -> Result<Vec<Message>> {
        let db = self.db.lock().map_err(err)?;
        let mut q = db
            .prepare("SELECT data FROM messages WHERE project_id=?1 ORDER BY rowid")
            .map_err(err)?;
        let rows = q
            .query_map([p], |r| r.get::<_, String>(0))
            .map_err(err)?
            .map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
            .collect();
        rows
    }
    /// Bounded recent chat for downloadable diagnostics; oversized messages are excluded.
    pub fn diagnostic_messages(&self, p: &str) -> Result<Vec<Message>> {
        let db=self.db.lock().map_err(err)?;
        let mut q=db.prepare("SELECT data FROM messages WHERE project_id=?1 AND length(CAST(data AS BLOB))<=65536 ORDER BY rowid DESC LIMIT 100").map_err(err)?;
        let mut rows=q.query_map([p],|r|r.get::<_,String>(0)).map_err(err)?
            .map(|r|serde_json::from_str(&r.map_err(err)?).map_err(err)).collect::<Result<Vec<Message>>>()?;
        rows.reverse();Ok(rows)
    }
    pub fn activity(&self, p: &str, label: &str, status: &str) -> Result<Activity> {
        let a = Activity {
            id: id(),
            project_id: p.into(),
            label: label.into(),
            status: status.into(),
            created_at: now(),
        };
        self.db
            .lock()
            .map_err(err)?
            .execute(
                "INSERT INTO activity VALUES (?1,?2,?3)",
                params![a.id, p, serde_json::to_string(&a).map_err(err)?],
            )
            .map_err(err)?;
        Ok(a)
    }
    pub fn activities(&self, p: &str) -> Result<Vec<Activity>> {
        let db = self.db.lock().map_err(err)?;
        let mut q = db
            .prepare("SELECT data FROM activity WHERE project_id=?1 ORDER BY rowid DESC LIMIT 100")
            .map_err(err)?;
        let rows = q
            .query_map([p], |r| r.get::<_, String>(0))
            .map_err(err)?
            .map(|r| serde_json::from_str(&r.map_err(err)?).map_err(err))
            .collect();
        rows
    }
    pub fn setting(&self, key: &str) -> Option<String> {
        self.db
            .lock()
            .ok()?
            .query_row("SELECT value FROM settings WHERE key=?1", [key], |r| {
                r.get(0)
            })
            .ok()
    }
    pub fn set(&self, key: &str, v: &str) -> Result<()> {
        self.db
            .lock()
            .map_err(err)?
            .execute(
                "INSERT OR REPLACE INTO settings VALUES (?1,?2)",
                params![key, v],
            )
            .map_err(err)?;
        Ok(())
    }
    pub fn clear_project_history(&self, id: &str, delete: bool) -> Result<()> {
        self.path(id)?;
        let mut db = self.db.lock().map_err(err)?;
        let tx = db.transaction().map_err(err)?;
        for table in ["messages", "activity"] {
            tx.execute(&format!("DELETE FROM {table} WHERE project_id=?1"), [id]).map_err(err)?;
        }
        // Keys of earlier releases' conversion logic are cleared with the rest.
        for prefix in ["queue", "turn", "consent", "auto-stop", "stop-seq", "ve-lite", "goal-reactivations", "goal-progress",
            "gate-history", "stall", "rerun", "run-started", "flash-review", "livefix", "finish", "h2g-progress", "thread-tools", "goal", "run-started", "resumed-at", "comparing", "compare-error"] {
            tx.execute("DELETE FROM settings WHERE key=?1", [format!("{prefix}:{id}")]).map_err(err)?;
        }
        if delete {
            tx.execute("DELETE FROM projects WHERE id=?1", [id]).map_err(err)?;
            // Deploy choices outlive Clean & restart, not deletion.
            tx.execute("DELETE FROM settings WHERE key=?1", [format!("cloudflare:{id}")]).map_err(err)?;
        }
        tx.commit().map_err(err)
    }
}
pub fn write_private(path: &Path, content: &[u8]) -> Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(err)?;
    file.write_all(content).map_err(err)?;
    file.sync_all().map_err(err)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn saved(extra: serde_json::Value) -> String {
        let mut v = json!({"id":"0584dcf1-7f08-4efb-85cf-ae7284faf8f9","name":"old","sourceName":"old.zip","kind":"Static HTML",
            "createdAt":"","updatedAt":"","phase":"imported","revision":1,"threadId":null,"pages":[],"gates":[],"artifacts":[],
            "preview":null,"runtimeImage":"img","pluginCommit":"abc","reporting":"not_required","lastError":null});
        for (k, val) in extra.as_object().unwrap() { v[k] = val.clone(); }
        v.to_string()
    }
    #[test]
    fn projects_saved_before_theme_types_migrate_to_html_and_new_type_persists() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path().to_path_buf()).unwrap();
        // A row written by 0.1.9 has no target field at all.
        store.db.lock().unwrap().execute("INSERT INTO projects VALUES (?1,?2)",
            params!["0584dcf1-7f08-4efb-85cf-ae7284faf8f9", saved(json!({}))]).unwrap();
        let mut p = store.project("0584dcf1-7f08-4efb-85cf-ae7284faf8f9").unwrap();
        assert_eq!(p.target, "html");
        assert!(!p.gutenberg());
        p.target = "gutenberg".into();
        store.put(&p).unwrap();
        let reloaded = &store.projects().unwrap()[0];
        assert_eq!(reloaded.target, "gutenberg");
        assert!(reloaded.gutenberg());
        assert_eq!(serde_json::to_value(reloaded).unwrap()["target"], "gutenberg");
        assert!(valid_target("gutenberg").is_ok() && valid_target("html").is_ok());
        assert!(valid_target("pixel").is_err() && valid_target("").is_err());
    }
}
