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
    // Undo for actions that created a file (FR-ACT-04).
    "ALTER TABLE actions ADD COLUMN undo_path TEXT;
    ALTER TABLE actions ADD COLUMN undone INTEGER NOT NULL DEFAULT 0;",
    // Time per app and project, per local day (FR-SYS-06). Local only.
    "CREATE TABLE app_time (
        day TEXT NOT NULL,
        app TEXT NOT NULL,
        project TEXT NOT NULL DEFAULT '',
        secs INTEGER NOT NULL,
        PRIMARY KEY (day, app, project)
    );",
    // Keyword search over history and chosen folders (FR-RAG, P4 part 1).
    // `ref` is the path, URL or id a result opens.
    "CREATE VIRTUAL TABLE search USING fts5(
        source UNINDEXED, ref UNINDEXED, title, body, ts UNINDEXED,
        tokenize = 'porter unicode61'
    );",
    // How the user treats each skill (FR-ACT-07).
    "CREATE TABLE skill_habits (
        skill_id TEXT PRIMARY KEY,
        dismiss_streak INTEGER NOT NULL DEFAULT 0,
        muted_until TEXT,
        last_label TEXT NOT NULL DEFAULT '',
        accept_streak INTEGER NOT NULL DEFAULT 0,
        offered INTEGER NOT NULL DEFAULT 0
    );",
    // Embeddings of search items from a local model (semantic search).
    "CREATE TABLE vectors (
        source TEXT NOT NULL,
        ref TEXT NOT NULL,
        model TEXT NOT NULL,
        vec BLOB NOT NULL,
        PRIMARY KEY (source, ref)
    );",
];

/// Cosine similarity; vectors of different lengths score 0.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Merges ranked lists by reciprocal rank fusion: items high in either list
/// rise, items in both rise most.
pub fn fuse(lists: &[Vec<(String, String)>], limit: usize) -> Vec<(String, String)> {
    let mut scores: Vec<((String, String), f32)> = Vec::new();
    for list in lists {
        for (rank, key) in list.iter().enumerate() {
            let s = 1.0 / (60.0 + rank as f32);
            match scores.iter_mut().find(|(k, _)| k == key) {
                Some((_, total)) => *total += s,
                None => scores.push((key.clone(), s)),
            }
        }
    }
    scores.sort_by(|a, b| b.1.total_cmp(&a.1));
    scores.into_iter().take(limit).map(|(k, _)| k).collect()
}

/// How the user has treated one skill lately.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Habit {
    pub skill_id: String,
    pub dismiss_streak: i64,
    pub muted_until: Option<String>,
    pub last_label: String,
    pub accept_streak: i64,
    /// The "do this automatically?" offer was already made.
    pub offered: bool,
}

/// One search hit, with where it came from so it can be opened.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub source: String,
    pub reference: String,
    pub title: String,
    /// The matching part of the text, with matches between [ and ].
    pub snippet: String,
    pub ts: String,
}

/// Turns what the user typed into an FTS5 query: each word is matched as a
/// prefix, and nothing they type can be read as FTS5 syntax.
pub fn fts_query(input: &str) -> Option<String> {
    let words: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(12)
        .map(|w| format!("\"{}\"*", w.to_lowercase()))
        .collect();
    (!words.is_empty()).then(|| words.join(" "))
}

/// Seconds spent in one app (and project, when known) on one day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppTime {
    pub app: String,
    pub project: String,
    pub secs: i64,
}

/// One entry of the action log, newest first in [`Storage::recent_actions`].
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionRecord {
    /// Set by the database; ignored when logging.
    pub id: i64,
    pub ts: String,
    pub skill_id: String,
    pub action: String,
    pub label: String,
    pub ok: bool,
    pub message: String,
    pub auto: bool,
    /// A file or folder the action created, which Undo moves to the bin.
    pub undo_path: Option<String>,
    pub undone: bool,
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

    /// Logs an action and returns its id.
    pub fn log_action(&self, record: &ActionRecord) -> Result<i64, StorageError> {
        self.conn.execute(
            "INSERT INTO actions (ts, skill_id, action, label, ok, message, auto, undo_path)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                record.ts,
                record.skill_id,
                record.action,
                record.label,
                record.ok,
                record.message,
                record.auto,
                record.undo_path
            ],
        )?;
        Ok(self.conn.last_insert_rowid())
    }

    const ACTION_COLUMNS: &str =
        "id, ts, skill_id, action, label, ok, message, auto, undo_path, undone";

    fn action_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ActionRecord> {
        Ok(ActionRecord {
            id: r.get(0)?,
            ts: r.get(1)?,
            skill_id: r.get(2)?,
            action: r.get(3)?,
            label: r.get(4)?,
            ok: r.get(5)?,
            message: r.get(6)?,
            auto: r.get(7)?,
            undo_path: r.get(8)?,
            undone: r.get(9)?,
        })
    }

    pub fn recent_actions(&self, limit: u32) -> Result<Vec<ActionRecord>, StorageError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM actions ORDER BY id DESC LIMIT ?1",
            Self::ACTION_COLUMNS
        ))?;
        let rows = stmt.query_map([limit], Self::action_row)?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn action(&self, id: i64) -> Result<Option<ActionRecord>, StorageError> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {} FROM actions WHERE id = ?1",
            Self::ACTION_COLUMNS
        ))?;
        let mut rows = stmt.query_map([id], Self::action_row)?;
        Ok(rows.next().transpose()?)
    }

    pub fn add_time(
        &self,
        day: &str,
        app: &str,
        project: &str,
        secs: i64,
    ) -> Result<(), StorageError> {
        if secs <= 0 {
            return Ok(());
        }
        self.conn.execute(
            "INSERT INTO app_time (day, app, project, secs) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (day, app, project) DO UPDATE SET secs = secs + excluded.secs",
            params![day, app, project, secs],
        )?;
        Ok(())
    }

    /// Time for one day, largest first.
    pub fn time_for_day(&self, day: &str) -> Result<Vec<AppTime>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT app, project, secs FROM app_time WHERE day = ?1 ORDER BY secs DESC")?;
        let rows = stmt.query_map([day], |r| {
            Ok(AppTime {
                app: r.get(0)?,
                project: r.get(1)?,
                secs: r.get(2)?,
            })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn clear_time(&self) -> Result<usize, StorageError> {
        Ok(self.conn.execute("DELETE FROM app_time", [])?)
    }

    pub fn habit(&self, skill_id: &str) -> Result<Habit, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT dismiss_streak, muted_until, last_label, accept_streak, offered
             FROM skill_habits WHERE skill_id = ?1",
        )?;
        let mut rows = stmt.query_map([skill_id], |r| {
            Ok(Habit {
                skill_id: skill_id.to_owned(),
                dismiss_streak: r.get(0)?,
                muted_until: r.get(1)?,
                last_label: r.get(2)?,
                accept_streak: r.get(3)?,
                offered: r.get(4)?,
            })
        })?;
        Ok(rows.next().transpose()?.unwrap_or_else(|| Habit {
            skill_id: skill_id.to_owned(),
            ..Habit::default()
        }))
    }

    pub fn save_habit(&self, h: &Habit) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT INTO skill_habits (skill_id, dismiss_streak, muted_until, last_label, accept_streak, offered)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT (skill_id) DO UPDATE SET dismiss_streak = excluded.dismiss_streak,
               muted_until = excluded.muted_until, last_label = excluded.last_label,
               accept_streak = excluded.accept_streak, offered = excluded.offered",
            params![h.skill_id, h.dismiss_streak, h.muted_until, h.last_label, h.accept_streak, h.offered],
        )?;
        Ok(())
    }

    /// Adds or replaces one searchable item (same source and ref replace).
    pub fn index(
        &self,
        source: &str,
        reference: &str,
        title: &str,
        body: &str,
        ts: &str,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "DELETE FROM search WHERE source = ?1 AND ref = ?2",
            params![source, reference],
        )?;
        // The text changed, so its embedding is stale.
        self.conn.execute(
            "DELETE FROM vectors WHERE source = ?1 AND ref = ?2",
            params![source, reference],
        )?;
        self.conn.execute(
            "INSERT INTO search (source, ref, title, body, ts) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![source, reference, title, body, ts],
        )?;
        Ok(())
    }

    /// Best matches first. `sources` limits where to look (empty = everywhere).
    pub fn search(
        &self,
        input: &str,
        sources: &[&str],
        limit: u32,
    ) -> Result<Vec<SearchHit>, StorageError> {
        let Some(q) = fts_query(input) else {
            return Ok(Vec::new());
        };
        let mut stmt = self.conn.prepare(
            "SELECT source, ref, title, snippet(search, 3, '[', ']', ' ... ', 12), ts
             FROM search WHERE search MATCH ?1 ORDER BY bm25(search, 0, 0, 4.0, 1.0) LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![q, limit * 3], |r| {
            Ok(SearchHit {
                source: r.get(0)?,
                reference: r.get(1)?,
                title: r.get(2)?,
                snippet: r.get(3)?,
                ts: r.get(4)?,
            })
        })?;
        let mut hits: Vec<SearchHit> = rows.collect::<Result<_, _>>()?;
        if !sources.is_empty() {
            hits.retain(|h| sources.contains(&h.source.as_str()));
        }
        hits.truncate(limit as usize);
        Ok(hits)
    }

    /// Items that have no embedding from `model` yet: (source, ref, text).
    pub fn unembedded(
        &self,
        model: &str,
        limit: u32,
    ) -> Result<Vec<(String, String, String)>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT s.source, s.ref, s.title || '. ' || substr(s.body, 1, 1500)
             FROM search s LEFT JOIN vectors v
               ON v.source = s.source AND v.ref = s.ref AND v.model = ?1
             WHERE v.ref IS NULL LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![model, limit], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    pub fn save_vector(
        &self,
        source: &str,
        reference: &str,
        model: &str,
        vec: &[f32],
    ) -> Result<(), StorageError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO vectors (source, ref, model, vec) VALUES (?1, ?2, ?3, ?4)",
            params![source, reference, model, to_blob(vec)],
        )?;
        Ok(())
    }

    pub fn vector_count(&self, model: &str) -> Result<u64, StorageError> {
        let n: i64 = self.conn.query_row(
            "SELECT count(*) FROM vectors WHERE model = ?1",
            [model],
            |r| r.get(0),
        )?;
        Ok(n as u64)
    }

    /// The `limit` items closest in meaning to `query`, best first.
    pub fn nearest(
        &self,
        query: &[f32],
        model: &str,
        sources: &[&str],
        limit: usize,
    ) -> Result<Vec<(String, String, f32)>, StorageError> {
        let mut stmt = self
            .conn
            .prepare("SELECT source, ref, vec FROM vectors WHERE model = ?1")?;
        let mut rows = stmt.query([model])?;
        let mut best: Vec<(String, String, f32)> = Vec::new();
        while let Some(r) = rows.next()? {
            let source: String = r.get(0)?;
            if !sources.is_empty() && !sources.contains(&source.as_str()) {
                continue;
            }
            let blob: Vec<u8> = r.get(2)?;
            let score = cosine(query, &from_blob(&blob));
            if best.len() < limit || best.last().is_some_and(|b| score > b.2) {
                best.push((source, r.get(1)?, score));
                best.sort_by(|a, b| b.2.total_cmp(&a.2));
                best.truncate(limit);
            }
        }
        Ok(best)
    }

    /// One item as a search hit, with the start of its text as the snippet.
    pub fn hit(&self, source: &str, reference: &str) -> Result<Option<SearchHit>, StorageError> {
        let mut stmt = self.conn.prepare(
            "SELECT source, ref, title, substr(body, 1, 160), ts FROM search
             WHERE source = ?1 AND ref = ?2 LIMIT 1",
        )?;
        let mut rows = stmt.query_map(params![source, reference], |r| {
            Ok(SearchHit {
                source: r.get(0)?,
                reference: r.get(1)?,
                title: r.get(2)?,
                snippet: r.get(3)?,
                ts: r.get(4)?,
            })
        })?;
        Ok(rows.next().transpose()?)
    }

    pub fn search_count(&self) -> Result<u64, StorageError> {
        let n: i64 = self
            .conn
            .query_row("SELECT count(*) FROM search", [], |r| r.get(0))?;
        Ok(n as u64)
    }

    pub fn clear_search(&self, source: Option<&str>) -> Result<usize, StorageError> {
        Ok(match source {
            Some(s) => {
                self.conn
                    .execute("DELETE FROM vectors WHERE source = ?1", [s])?;
                self.conn
                    .execute("DELETE FROM search WHERE source = ?1", [s])?
            }
            None => {
                self.conn.execute("DELETE FROM vectors", [])?;
                self.conn.execute("DELETE FROM search", [])?
            }
        })
    }

    pub fn mark_undone(&self, id: i64) -> Result<(), StorageError> {
        self.conn
            .execute("UPDATE actions SET undone = 1 WHERE id = ?1", [id])?;
        Ok(())
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

    #[test]
    fn fuses_rankings() {
        let k = |s: &str| ("f".to_owned(), s.to_owned());
        let fused = fuse(&[vec![k("a"), k("b"), k("c")], vec![k("c"), k("d")]], 3);
        // b and d tie; the first list breaks ties.
        assert_eq!(fused, vec![k("c"), k("a"), k("b")]);
        assert!((cosine(&[1.0, 0.0], &[1.0, 0.0]) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), 0.0);
    }

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
        let mut rec = ActionRecord {
            id: 0,
            ts: "2026-10-01T10:00:00Z".into(),
            skill_id: "dev.open-in-browser".into(),
            action: "open_url".into(),
            label: "Zen".into(),
            ok: true,
            message: "Opened in Zen".into(),
            auto: false,
            undo_path: None,
            undone: false,
        };
        rec.id = s.log_action(&rec).unwrap();
        assert_eq!(s.recent_actions(5).unwrap(), vec![rec.clone()]);

        let undoable = ActionRecord {
            action: "convert".into(),
            undo_path: Some("C:/Users/me/Downloads/photo.webp".into()),
            ..rec.clone()
        };
        let id = s.log_action(&undoable).unwrap();
        assert!(!s.action(id).unwrap().unwrap().undone);
        s.mark_undone(id).unwrap();
        let after = s.action(id).unwrap().unwrap();
        assert!(after.undone);
        assert_eq!(
            after.undo_path.as_deref(),
            Some("C:/Users/me/Downloads/photo.webp")
        );
        assert!(s.action(9999).unwrap().is_none());

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
    fn adds_up_time_per_day_app_and_project() {
        let s = Storage::open_in_memory().unwrap();
        s.add_time("2026-10-01", "Code", "sidekick", 60).unwrap();
        s.add_time("2026-10-01", "Code", "sidekick", 90).unwrap();
        s.add_time("2026-10-01", "Chrome", "", 30).unwrap();
        s.add_time("2026-10-02", "Code", "sidekick", 5).unwrap();
        s.add_time("2026-10-01", "Code", "sidekick", 0).unwrap();
        let day = s.time_for_day("2026-10-01").unwrap();
        assert_eq!(
            day[0],
            AppTime {
                app: "Code".into(),
                project: "sidekick".into(),
                secs: 150
            }
        );
        assert_eq!(day[1].secs, 30);
        assert_eq!(day.len(), 2);
        assert_eq!(s.clear_time().unwrap(), 3);
    }

    #[test]
    fn searches_by_words_and_prefixes_safely() {
        let s = Storage::open_in_memory().unwrap();
        s.index(
            "file",
            "C:/notes/acme.md",
            "acme.md",
            "Invoice for ACME Corp, due Friday. Rate 45 USD per hour.",
            "t1",
        )
        .unwrap();
        s.index(
            "chat",
            "c1",
            "How do I free port 3000",
            "Use netstat -ano to find the process",
            "t2",
        )
        .unwrap();
        s.index(
            "file",
            "C:/notes/acme.md",
            "acme.md",
            "Replaced: proposal draft for ACME",
            "t3",
        )
        .unwrap();
        let hits = s.search("acme", &[], 10).unwrap();
        assert_eq!(hits.len(), 1, "same source and ref replace");
        assert!(hits[0].snippet.contains("[ACME]"));
        assert_eq!(
            s.search("propos", &[], 10).unwrap().len(),
            1,
            "prefix match"
        );
        assert_eq!(
            s.search("netstat", &["file"], 10).unwrap().len(),
            0,
            "source filter"
        );
        assert_eq!(s.search("netstat", &["chat"], 10).unwrap().len(), 1);
        // FTS5 syntax in the input is treated as plain words.
        assert!(s.search("\"acme\" OR body:*", &[], 10).is_ok());
        assert!(s.search("  ", &[], 10).unwrap().is_empty());
        // Vectors: nearest by meaning, dropped when the text changes.
        s.save_vector("file", "C:/notes/acme.md", "m", &[1.0, 0.0])
            .unwrap();
        s.save_vector("chat", "c1", "m", &[0.6, 0.8]).unwrap();
        let near = s.nearest(&[0.0, 1.0], "m", &[], 5).unwrap();
        assert_eq!(near[0].1, "c1");
        assert_eq!(s.nearest(&[0.0, 1.0], "m", &["file"], 5).unwrap().len(), 1);
        assert_eq!(s.vector_count("m").unwrap(), 2);
        assert!(s.unembedded("m", 10).unwrap().is_empty());
        assert_eq!(s.unembedded("other", 10).unwrap().len(), 2);
        let hit = s.hit("chat", "c1").unwrap().unwrap();
        assert!(!hit.title.is_empty());
        assert_eq!(s.search_count().unwrap(), 2);
        assert_eq!(s.clear_search(Some("chat")).unwrap(), 1);
    }

    #[test]
    fn keeps_skill_habits() {
        let s = Storage::open_in_memory().unwrap();
        let mut h = s.habit("files.download").unwrap();
        assert_eq!(h.dismiss_streak, 0);
        h.dismiss_streak = 2;
        h.last_label = "Open".into();
        s.save_habit(&h).unwrap();
        h.accept_streak = 4;
        s.save_habit(&h).unwrap();
        let back = s.habit("files.download").unwrap();
        assert_eq!((back.dismiss_streak, back.accept_streak), (2, 4));
        assert_eq!(back.last_label, "Open");
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
