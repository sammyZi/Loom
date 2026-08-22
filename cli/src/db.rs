use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

/// One chat session. The transcript is stored as a JSON array in a single column:
/// it is only ever read and written whole, so a row-per-message table would buy
/// nothing until we want search across messages.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub folder: String,
    pub title: String,
    pub log: serde_json::Value,
    /// Last activity, epoch millis.
    pub at: i64,
    /// Start time, epoch millis. Drives list order so rows never re-shuffle.
    #[serde(default)]
    pub created: i64,
    #[serde(default)]
    pub archived: bool,
}

const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS session (
     id       TEXT PRIMARY KEY,
     folder   TEXT NOT NULL,
     title    TEXT NOT NULL,
     at       INTEGER NOT NULL,
     created  INTEGER NOT NULL,
     archived INTEGER NOT NULL DEFAULT 0
 );
 CREATE TABLE IF NOT EXISTS message (
     id         TEXT PRIMARY KEY,
     session_id TEXT NOT NULL REFERENCES session(id) ON DELETE CASCADE,
     seq        INTEGER NOT NULL,
     kind       TEXT NOT NULL,
     text       TEXT NOT NULL,
     detail     TEXT,
     UNIQUE (session_id, seq)
 );
 CREATE INDEX IF NOT EXISTS session_folder_idx ON session(folder);
 CREATE INDEX IF NOT EXISTS session_created_idx ON session(created DESC);
 CREATE INDEX IF NOT EXISTS message_session_idx ON message(session_id, seq);";

pub struct Db {
    conn: Mutex<Connection>,
    /// Where the file lives; surfaced in logs so users can find/backup it.
    #[allow(dead_code)]
    pub path: PathBuf,
}

impl Db {
    pub fn open() -> Result<Self> {
        let path = db_path()?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
        }
        let conn = Connection::open(&path).with_context(|| format!("open {}", path.display()))?;
        conn.execute_batch("PRAGMA journal_mode = WAL; PRAGMA foreign_keys = ON;")?;
        conn.execute_batch(SCHEMA).context("create schema")?;
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
    }

    /// Last-resort store so a failure to open the file does not take the app down.
    pub fn memory() -> Self {
        let conn = Connection::open_in_memory().expect("in-memory sqlite");
        conn.execute_batch("PRAGMA foreign_keys = ON;").expect("pragmas");
        conn.execute_batch(SCHEMA).expect("schema");
        Self {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        }
    }

    /// Non-archived sessions, newest first by start time. Each transcript is
    /// capped at its most recent messages so a huge history cannot stall the
    /// list endpoint that the sidebar polls.
    pub fn list(&self) -> Result<Vec<Session>> {
        const MSG_CAP: usize = 300;
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, folder, title, at, created, archived
             FROM session WHERE archived = 0 ORDER BY created DESC",
        )?;
        let mut sessions: Vec<Session> = stmt
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    folder: r.get(1)?,
                    title: r.get(2)?,
                    log: serde_json::json!([]),
                    at: r.get(3)?,
                    created: r.get(4)?,
                    archived: r.get::<_, i64>(5)? != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Newest-first per session, so truncating keeps the latest messages;
        // reversed again below to restore chronological order.
        let mut msgs = conn.prepare(
            "SELECT session_id, id, kind, text, detail FROM message ORDER BY session_id, seq DESC",
        )?;
        let mut by_session: std::collections::HashMap<String, Vec<serde_json::Value>> =
            std::collections::HashMap::new();
        for row in msgs
            .query_map([], |r| {
                let detail: Option<String> = r.get(4)?;
                Ok((
                    r.get::<_, String>(0)?,
                    serde_json::json!({
                        "id": r.get::<_, String>(1)?,
                        "kind": r.get::<_, String>(2)?,
                        "text": r.get::<_, String>(3)?,
                        "detail": detail,
                    }),
                ))
            })?
            .flatten()
        {
            by_session.entry(row.0).or_default().push(row.1);
        }
        for s in &mut sessions {
            if let Some(mut items) = by_session.remove(&s.id) {
                items.truncate(MSG_CAP);
                items.reverse();
                s.log = serde_json::Value::Array(items);
            }
        }
        Ok(sessions)
    }

    /// Insert or update. The original `created` is preserved so reopening or
    /// re-saving a session never moves it in the list.
    pub fn upsert(&self, s: &Session) -> Result<()> {
        let mut conn = self.conn.lock().unwrap();
        let created = if s.created > 0 { s.created } else { s.at };
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO session (id, folder, title, at, created, archived)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(id) DO UPDATE SET
                 folder = excluded.folder,
                 title  = excluded.title,
                 at     = excluded.at",
            params![s.id, s.folder, s.title, s.at, created, s.archived as i64],
        )?;

        // The transcript is rewritten whole on every save, so replace the rows.
        // Ids are derived from session + position, which keeps them stable across saves.
        tx.execute("DELETE FROM message WHERE session_id = ?1", params![s.id])?;
        if let Some(items) = s.log.as_array() {
            let mut stmt = tx.prepare(
                "INSERT INTO message (id, session_id, seq, kind, text, detail)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            )?;
            for (seq, item) in items.iter().enumerate() {
                let kind = item["kind"].as_str().unwrap_or("token");
                let text = item["text"].as_str().unwrap_or("");
                let detail = item["detail"].as_str();
                stmt.execute(params![
                    format!("{}-{:04}", s.id, seq),
                    s.id,
                    seq as i64,
                    kind,
                    text,
                    detail
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM session WHERE id = ?1", params![id])?;
        Ok(())
    }

    pub fn rename(&self, id: &str, title: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE session SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(())
    }

    pub fn archive(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE session SET archived = 1 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Bring an archived session back into the main list.
    pub fn unarchive(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE session SET archived = 0 WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// Only the archived sessions, newest first — the sidebar's archive view.
    pub fn list_archived(&self) -> Result<Vec<Session>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT s.id, s.folder, s.title, s.at, s.created, s.archived
             FROM session s WHERE s.archived = 1 ORDER BY s.created DESC",
        )?;
        let mut sessions: Vec<Session> = stmt
            .query_map([], |r| {
                Ok(Session {
                    id: r.get(0)?,
                    folder: r.get(1)?,
                    title: r.get(2)?,
                    log: serde_json::json!([]),
                    at: r.get(3)?,
                    created: r.get(4)?,
                    archived: r.get::<_, i64>(5)? != 0,
                })
            })?
            .filter_map(|r| r.ok())
            .collect();

        // Attach each archived transcript (same cap as list()).
        const MSG_CAP: usize = 300;
        let mut msgs = conn.prepare(
            "SELECT m.session_id, m.id, m.kind, m.text, m.detail
             FROM message m JOIN session s ON s.id = m.session_id
             WHERE s.archived = 1 ORDER BY m.session_id, m.seq DESC",
        )?;
        let mut by_session: std::collections::HashMap<String, Vec<serde_json::Value>> =
            std::collections::HashMap::new();
        for row in msgs
            .query_map([], |r| {
                let detail: Option<String> = r.get(4)?;
                Ok((
                    r.get::<_, String>(0)?,
                    serde_json::json!({
                        "id": r.get::<_, String>(1)?,
                        "kind": r.get::<_, String>(2)?,
                        "text": r.get::<_, String>(3)?,
                        "detail": detail,
                    }),
                ))
            })?
            .flatten()
        {
            by_session.entry(row.0).or_default().push(row.1);
        }
        for s in &mut sessions {
            if let Some(mut items) = by_session.remove(&s.id) {
                items.truncate(MSG_CAP);
                items.reverse();
                s.log = serde_json::Value::Array(items);
            }
        }
        Ok(sessions)
    }

    pub fn clear(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM session", [])?;
        Ok(())
    }
}

/// Per-user application data, so history is shared across every workspace and
/// nothing is written into the user's repos.
fn db_path() -> Result<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    }
    .context("no application data directory for this user")?;
    Ok(base.join("ide-ai").join("sessions.db"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Db {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        }
    }

    fn s(id: &str, folder: &str, at: i64, created: i64) -> Session {
        Session {
            id: id.into(),
            folder: folder.into(),
            title: format!("title {id}"),
            log: serde_json::json!([{ "kind": "user", "text": "hi" }]),
            at,
            created,
            archived: false,
        }
    }

    #[test]
    fn round_trips_a_session_with_its_transcript() {
        let db = mem();
        db.upsert(&s("a", "/p", 10, 10)).unwrap();
        let got = db.list().unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, "a");
        assert_eq!(got[0].log[0]["text"], "hi");
    }

    #[test]
    fn upsert_updates_without_duplicating_or_moving() {
        let db = mem();
        db.upsert(&s("a", "/p", 10, 10)).unwrap();
        let mut again = s("a", "/p", 999, 0); // later activity, created omitted
        again.title = "renamed by save".into();
        db.upsert(&again).unwrap();

        let got = db.list().unwrap();
        assert_eq!(got.len(), 1, "upsert must not duplicate");
        assert_eq!(got[0].title, "renamed by save");
        assert_eq!(got[0].at, 999, "activity time updates");
        assert_eq!(got[0].created, 10, "start time is preserved so order is stable");
    }

    #[test]
    fn orders_by_start_time_newest_first() {
        let db = mem();
        db.upsert(&s("old", "/p", 5, 5)).unwrap();
        db.upsert(&s("new", "/p", 6, 100)).unwrap();
        let ids: Vec<_> = db.list().unwrap().into_iter().map(|x| x.id).collect();
        assert_eq!(ids, vec!["new", "old"]);
    }

    #[test]
    fn archive_hides_but_delete_removes() {
        let db = mem();
        db.upsert(&s("a", "/p", 1, 1)).unwrap();
        db.upsert(&s("b", "/p", 2, 2)).unwrap();
        db.archive("a").unwrap();
        assert_eq!(db.list().unwrap().len(), 1, "archived rows are hidden");

        db.delete("b").unwrap();
        assert_eq!(db.list().unwrap().len(), 0);

        db.clear().unwrap();
        assert_eq!(db.list().unwrap().len(), 0);
    }

    #[test]
    fn keeps_folders_separate() {
        let db = mem();
        db.upsert(&s("a", "/one", 1, 1)).unwrap();
        db.upsert(&s("b", "/two", 2, 2)).unwrap();
        let all = db.list().unwrap();
        assert_eq!(all.iter().filter(|x| x.folder == "/one").count(), 1);
        assert_eq!(all.iter().filter(|x| x.folder == "/two").count(), 1);
    }
}

#[cfg(test)]
mod message_tests {
    use super::*;

    fn mem() -> Db {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        Db {
            conn: Mutex::new(conn),
            path: PathBuf::from(":memory:"),
        }
    }

    fn with_log(id: &str, items: serde_json::Value) -> Session {
        Session {
            id: id.into(),
            folder: "/p".into(),
            title: "t".into(),
            log: items,
            at: 10,
            created: 10,
            archived: false,
        }
    }

    fn message_count(db: &Db) -> i64 {
        db.conn
            .lock()
            .unwrap()
            .query_row("SELECT count(*) FROM message", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn every_message_gets_a_stable_id_and_keeps_its_order() {
        let db = mem();
        db.upsert(&with_log(
            "s1",
            serde_json::json!([
                { "kind": "user",  "text": "explain this" },
                { "kind": "tool",  "text": "Read", "detail": "App.tsx" },
                { "kind": "token", "text": "It is a quiz app." }
            ]),
        ))
        .unwrap();

        let log = db.list().unwrap()[0].log.clone();
        let items = log.as_array().unwrap();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0]["id"], "s1-0000");
        assert_eq!(items[2]["id"], "s1-0002");
        // order is by seq, not insertion luck
        assert_eq!(items[0]["kind"], "user");
        assert_eq!(items[1]["detail"], "App.tsx");
        assert_eq!(items[2]["text"], "It is a quiz app.");
    }

    #[test]
    fn resaving_replaces_messages_instead_of_appending() {
        let db = mem();
        db.upsert(&with_log("s1", serde_json::json!([{ "kind": "user", "text": "one" }])))
            .unwrap();
        db.upsert(&with_log(
            "s1",
            serde_json::json!([
                { "kind": "user",  "text": "one" },
                { "kind": "token", "text": "two" }
            ]),
        ))
        .unwrap();
        assert_eq!(message_count(&db), 2, "must not accumulate duplicates");
        assert_eq!(db.list().unwrap()[0].log.as_array().unwrap().len(), 2);
    }

    #[test]
    fn deleting_a_session_cascades_to_its_messages() {
        let db = mem();
        db.upsert(&with_log("s1", serde_json::json!([{ "kind": "user", "text": "x" }])))
            .unwrap();
        db.upsert(&with_log("s2", serde_json::json!([{ "kind": "user", "text": "y" }])))
            .unwrap();
        assert_eq!(message_count(&db), 2);

        db.delete("s1").unwrap();
        assert_eq!(message_count(&db), 1, "orphaned messages must not survive");

        db.clear().unwrap();
        assert_eq!(message_count(&db), 0);
    }

    #[test]
    fn messages_stay_attached_to_the_right_session() {
        let db = mem();
        db.upsert(&with_log("a", serde_json::json!([{ "kind": "user", "text": "for a" }])))
            .unwrap();
        db.upsert(&with_log("b", serde_json::json!([{ "kind": "user", "text": "for b" }])))
            .unwrap();
        for s in db.list().unwrap() {
            let items = s.log.as_array().unwrap();
            assert_eq!(items.len(), 1);
            assert_eq!(items[0]["text"], format!("for {}", s.id));
            assert!(items[0]["id"].as_str().unwrap().starts_with(&s.id));
        }
    }
}
