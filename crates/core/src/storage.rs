use std::path::Path;

use rusqlite::{Connection, params};
use serde::Serialize;

use crate::event::{Event, Sensitivity};

/// Schema migrations, applied in order. Index + 1 is the `user_version`.
const MIGRATIONS: &[&str] = &[
    "CREATE TABLE events (
        id TEXT PRIMARY KEY,
        ts TEXT NOT NULL,
        kind TEXT NOT NULL,
        source TEXT NOT NULL,
        context_json TEXT NOT NULL,
        payload_json TEXT,
        sensitivity TEXT NOT NULL
    );
    CREATE INDEX events_ts ON events (ts);
    CREATE INDEX events_kind ON events (kind);",
    // Action log (FR-ACT-06) and learned choices (FR-DEV-02, FR-ACT-07).
    "CREATE TABLE actions (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        ts TEXT NOT NULL,
        skill_id TEXT NOT NULL,
        action TEXT NOT NULL,
        label TEXT NOT NULL,
        ok INTEGER NOT NULL,
        message TEXT NOT NULL,
        auto INTEGER NOT NULL DEFAULT 0
    );
    CREATE TABLE choices (
        key TEXT NOT NULL,
        label TEXT NOT NULL,
        count INTEGER NOT NULL,
        last_ts TEXT NOT NULL,
        PRIMARY KEY (key, label)
    );",
];

/// One entry of the action log, newest first in [`Storage::recent_actions`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecord {
    pub ts: String,
    pub skill_id: String,
    pub action: String,
    pub label: String,
    pub ok: bool,
    pub message: String,
    pub auto: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum StorageError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

/// An event as read back from storage, for the history and debug views.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredEvent {
    pub id: String,
    pub ts: String,
    pub kind: String,
    pub source: String,
    pub payload: Option<serde_json::Value>,
    pub sensitivity: String,
}

pub struct Storage {
    conn: Connection,
}

impl Storage {
    pub fn open(path: &Path) -> Result<Self, StorageError> {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::init(conn)
    }

    pub fn open_in_memory() -> Result<Self, StorageError> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Self, StorageError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        let mut storage = Self { conn };
        storage.migrate()?;
        Ok(storage)
    }

    fn migrate(&mut self) -> Result<(), StorageError> {
        let current = self.schema_version()?;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
            let tx = self.conn.transaction()?;
            tx.execute_batch(sql)?;
            tx.pragma_update(None, "user_version", i as i64 + 1)?;
            tx.commit()?;
        }
        Ok(())
    }

    pub fn schema_version(&self) -> Result<usize, StorageError> {
        let version: i64 = self
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))?;
        Ok(usize::try_from(version).unwrap_or(0))
    }

    /// Stores an event. Payloads of secret events are dropped (NFR-PRIV-01).
    pub fn insert_event(&self, event: &Event) -> Result<(), StorageError> {
        let payload = match event.sensitivity {
            Sensitivity::Secret => None,
            _ => Some(serde_json::to_string(&event.payload)?),
        };
        self.conn.execute(
            "INSERT OR IGNORE INTO events (id, ts, kind, source, context_json, payload_json, sensitivity)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.id.to_string(),
                event.ts.to_rfc3339(),
                event.kind,
                event.source,
                serde_json::to_string(&event.context)?,
                payload,
                event.sensitivity.as_str(),
            ],
        )?;
        Ok(())
    }

    /// Newest first, by insertion order.
    pub fn recent_events(&self, limit: u32) -> Result<Vec<StoredEvent>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, ts, kind, source, payload_json, sensitivity
             FROM events ORDER BY rowid DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            let payload: Option<String> = r.get(4)?;
            Ok(StoredEvent {
                id: r.get(0)?,
                ts: r.get(1)?,
                kind: r.get(2)?,
                source: r.get(3)?,
                payload: payload.and_then(|p| serde_json::from_str(&p).ok()),
                sensitivity: r.get(5)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Keeps only the newest `max_rows` events. Returns how many were removed.
    pub fn prune_events(&self, max_rows: u32) -> Result<usize, StorageError> {
        Ok(self.conn.execute(
            "DELETE FROM events WHERE rowid NOT IN (SELECT rowid FROM events ORDER BY rowid DESC LIMIT ?1)",
            [max_rows],
        )?)
    }

    pub fn log_action(&self, record: &ActionRecord) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO actions (ts, skill_id, action, label, ok, message, auto) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                record.ts,
                record.skill_id,
                record.action,
                record.label,
                record.ok,
                record.message,
                record.auto
            ],
        )?;
        Ok(())
    }

    pub fn recent_actions(&self, limit: u32) -> Result<Vec<ActionRecord>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT ts, skill_id, action, label, ok, message, auto FROM actions ORDER BY id DESC LIMIT ?1",
        )?;
        let rows = stmt.query_map([limit], |r| {
            Ok(ActionRecord {
                ts: r.get(0)?,
                skill_id: r.get(1)?,
                action: r.get(2)?,
                label: r.get(3)?,
                ok: r.get(4)?,
                message: r.get(5)?,
                auto: r.get(6)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// Remembers that `label` was picked under `key`.
    pub fn record_choice(&self, key: &str, label: &str, ts: &str) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO choices (key, label, count, last_ts) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT (key, label) DO UPDATE SET count = count + 1, last_ts = excluded.last_ts",
            params![key, label, ts],
        )?;
        Ok(())
    }

    pub fn choice_counts(
        &self,
        key: &str,
    ) -> Result<std::collections::HashMap<String, u32>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT label, count FROM choices WHERE key = ?1")?;
        let rows = stmt.query_map([key], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn clear_choices(&self) -> Result<usize, StorageError> {
        Ok(self.conn.execute("DELETE FROM choices", [])?)
    }

    pub fn count_events(&self) -> Result<u64, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))?;
        Ok(u64::try_from(count).unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str) -> Event {
        Event::new(kind, "test", serde_json::json!({"k": kind}))
    }

    #[test]
    fn migrates_to_latest_version() {
        let s = Storage::open_in_memory().unwrap();
        assert_eq!(s.schema_version().unwrap(), MIGRATIONS.len());
    }

    #[test]
    fn migration_is_idempotent_on_reopen() {
        let dir = std::env::temp_dir().join(format!("sidekick-db-{}", ulid::Ulid::new()));
        let path = dir.join("sidekick.db");
        Storage::open(&path)
            .unwrap()
            .insert_event(&event("a"))
            .unwrap();
        let reopened = Storage::open(&path).unwrap();
        assert_eq!(reopened.count_events().unwrap(), 1);
        drop(reopened);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stores_and_reads_newest_first() {
        let s = Storage::open_in_memory().unwrap();
        for kind in ["a", "b", "c"] {
            s.insert_event(&event(kind)).unwrap();
        }
        let recent = s.recent_events(2).unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].kind, "c");
        assert_eq!(recent[0].payload, Some(serde_json::json!({"k": "c"})));
    }

    #[test]
    fn drops_secret_payloads() {
        let s = Storage::open_in_memory().unwrap();
        s.insert_event(&event("clipboard.changed").with_sensitivity(Sensitivity::Secret))
            .unwrap();
        let stored = &s.recent_events(1).unwrap()[0];
        assert_eq!(stored.payload, None);
        assert_eq!(stored.sensitivity, "secret");
    }

    #[test]
    fn logs_actions_and_learns_choices() {
        let s = Storage::open_in_memory().unwrap();
        let rec = ActionRecord {
            ts: "2026-10-01T10:00:00Z".into(),
            skill_id: "dev.open-in-browser".into(),
            action: "open_url".into(),
            label: "Zen".into(),
            ok: true,
            message: "Opened in Zen".into(),
            auto: false,
        };
        s.log_action(&rec).unwrap();
        assert_eq!(s.recent_actions(5).unwrap(), vec![rec]);

        s.record_choice("dev:3000", "Zen", "t1").unwrap();
        s.record_choice("dev:3000", "Zen", "t2").unwrap();
        s.record_choice("dev:3000", "Chrome", "t3").unwrap();
        let counts = s.choice_counts("dev:3000").unwrap();
        assert_eq!(counts["Zen"], 2);
        assert_eq!(counts["Chrome"], 1);
        assert!(s.choice_counts("dev:8000").unwrap().is_empty());
        assert_eq!(s.clear_choices().unwrap(), 2);
    }

    #[test]
    fn prunes_oldest() {
        let s = Storage::open_in_memory().unwrap();
        for kind in ["a", "b", "c", "d"] {
            s.insert_event(&event(kind)).unwrap();
        }
        assert_eq!(s.prune_events(2).unwrap(), 2);
        let kinds: Vec<_> = s
            .recent_events(10)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, ["d", "c"]);
    }
}
