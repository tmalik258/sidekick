//! File names and paths for every drive, so "where is that invoice" finds a
//! file anywhere in a moment. Kept in its own SQLite file (names.db) so
//! building it never waits on, or holds up, the main database. Only names
//! and paths are stored; file contents are never read here.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::storage::fts_query;

const SCHEMA: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS names USING fts5(
    key,
    path UNINDEXED,
    folder UNINDEXED,
    tokenize = 'unicode61'
);
CREATE TABLE IF NOT EXISTS meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
";

/// Folders never indexed: system, caches, build output and package stores.
pub const SKIP: &[&str] = &[
    "$recycle.bin",
    "system volume information",
    "windows",
    "winsxs",
    "node_modules",
    ".git",
    "target",
    ".next",
    "__pycache__",
    ".venv",
    "venv",
    ".cache",
    "cache",
    "temp",
    "tmp",
    "packages",
    "$windows.~ws",
    "$windows.~bt",
    "programdata",
];

pub struct NameIndex {
    conn: Connection,
}

/// One match: where it is and whether it is a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NameHit {
    pub path: PathBuf,
    pub folder: bool,
}

impl NameIndex {
    pub fn open(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// For a file one process writes and others only read (the indexer
    /// service): a rollback journal, so readers need no write access.
    pub fn open_shared(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "DELETE")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Reads an index another process keeps.
    pub fn open_read_only(path: &Path) -> rusqlite::Result<Self> {
        let conn = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        conn.busy_timeout(std::time::Duration::from_millis(500))?;
        Ok(Self { conn })
    }

    pub fn open_in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch(SCHEMA)?;
        Ok(Self { conn })
    }

    /// Replaces the index with every file and folder under `roots`.
    /// `go_on` is asked now and then; false stops early and keeps the old
    /// index. Returns how many entries were indexed.
    pub fn rebuild(
        &mut self,
        roots: &[PathBuf],
        mut go_on: impl FnMut() -> bool,
    ) -> rusqlite::Result<Option<u64>> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM names", [])?;
        let mut count = 0u64;
        let mut stopped = false;
        {
            let mut insert =
                tx.prepare("INSERT INTO names (key, path, folder) VALUES (?1, ?2, ?3)")?;
            let mut stack: Vec<PathBuf> = roots.to_vec();
            while let Some(dir) = stack.pop() {
                if count.is_multiple_of(2048) && !go_on() {
                    stopped = true;
                    break;
                }
                let Ok(entries) = std::fs::read_dir(&dir) else {
                    continue;
                };
                for e in entries.flatten() {
                    let Ok(ft) = e.file_type() else { continue };
                    // Links and junctions would loop or count twice.
                    if ft.is_symlink() {
                        continue;
                    }
                    let name = e.file_name().to_string_lossy().into_owned();
                    let path = e.path();
                    insert.execute(params![
                        key(&name),
                        path.to_string_lossy(),
                        ft.is_dir() as i32
                    ])?;
                    count += 1;
                    if ft.is_dir()
                        && !name.starts_with('.')
                        && !SKIP.contains(&name.to_lowercase().as_str())
                    {
                        stack.push(path);
                    }
                }
            }
        }
        if stopped {
            tx.rollback()?;
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO meta (k, v) VALUES ('built', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1",
            [chrono::Utc::now().to_rfc3339()],
        )?;
        tx.commit()?;
        Ok(Some(count))
    }

    /// Replaces every entry under `root` with `entries` in one go, for the
    /// drive indexer that reads a whole drive's file table at once.
    pub fn replace_under(
        &mut self,
        root: &str,
        entries: impl IntoIterator<Item = (String, bool)>,
    ) -> rusqlite::Result<u64> {
        let tx = self.conn.transaction()?;
        tx.execute(
            "DELETE FROM names WHERE substr(path, 1, length(?1)) = ?1",
            [root],
        )?;
        let mut n = 0u64;
        {
            let mut insert =
                tx.prepare("INSERT INTO names (key, path, folder) VALUES (?1, ?2, ?3)")?;
            for (path, folder) in entries {
                let name = path.rsplit(['\\', '/']).next().unwrap_or(&path).to_owned();
                insert.execute(params![key(&name), path, folder as i32])?;
                n += 1;
            }
        }
        tx.commit()?;
        Ok(n)
    }

    /// Adds one entry, replacing any with the same path.
    pub fn upsert(&self, path: &str, folder: bool) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM names WHERE path = ?1", [path])?;
        let name = path.rsplit(['\\', '/']).next().unwrap_or(path);
        self.conn.execute(
            "INSERT INTO names (key, path, folder) VALUES (?1, ?2, ?3)",
            params![key(name), path, folder as i32],
        )?;
        Ok(())
    }

    /// Removes an entry and, for a folder, everything inside it.
    pub fn remove(&self, path: &str) -> rusqlite::Result<()> {
        // Either separator: Windows paths can carry both.
        let (back, fwd) = (format!("{path}\\"), format!("{path}/"));
        self.conn.execute(
            "DELETE FROM names WHERE path = ?1 OR substr(path, 1, length(?2)) = ?2 \
             OR substr(path, 1, length(?3)) = ?3",
            params![path, back, fwd],
        )?;
        Ok(())
    }

    /// Notes that a live indexer is keeping this index up to date.
    pub fn mark_live(&self) -> rusqlite::Result<()> {
        let now = chrono::Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO meta (k, v) VALUES ('live', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1",
            [&now],
        )?;
        self.conn.execute(
            "INSERT INTO meta (k, v) VALUES ('built', ?1) ON CONFLICT(k) DO UPDATE SET v = ?1",
            [&now],
        )?;
        Ok(())
    }

    /// When a live indexer last said it was keeping this index up to date.
    pub fn live(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.conn
            .query_row("SELECT v FROM meta WHERE k = 'live'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    }

    /// When the index was last built in full, if ever.
    pub fn built(&self) -> Option<chrono::DateTime<chrono::Utc>> {
        self.conn
            .query_row("SELECT v FROM meta WHERE k = 'built'", [], |r| {
                r.get::<_, String>(0)
            })
            .ok()
            .and_then(|v| chrono::DateTime::parse_from_rfc3339(&v).ok())
            .map(|t| t.with_timezone(&chrono::Utc))
    }

    pub fn count(&self) -> u64 {
        self.conn
            .query_row("SELECT count(*) FROM names", [], |r| r.get::<_, i64>(0))
            .map(|n| n as u64)
            .unwrap_or(0)
    }

    /// Names with every word of `query` (as a word start), shortest names
    /// first: "invoice march" finds "Invoice_March_2026.pdf".
    pub fn search(&self, query: &str, limit: usize) -> Vec<NameHit> {
        let Some(q) = fts_query(query) else {
            return Vec::new();
        };
        let Ok(mut stmt) = self.conn.prepare(
            "SELECT path, folder FROM names WHERE names MATCH ?1 ORDER BY length(key) LIMIT ?2",
        ) else {
            return Vec::new();
        };
        stmt.query_map(params![q, limit as i64], |r| {
            Ok(NameHit {
                path: PathBuf::from(r.get::<_, String>(0)?),
                folder: r.get::<_, i32>(1)? != 0,
            })
        })
        .map(|rows| rows.flatten().filter(|h| h.path.exists()).collect())
        .unwrap_or_default()
    }
}

/// How a name is indexed: lower case, with separators as spaces so each
/// part is a word ("Invoice_March-2026.pdf" -> "invoice march 2026 pdf").
pub fn key(name: &str) -> String {
    name.to_lowercase().replace(['_', '-', '.'], " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_files_anywhere_by_name_words() {
        let dir = std::env::temp_dir().join(format!("sidekick-names-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a").join("deep").join("er")).unwrap();
        std::fs::create_dir_all(dir.join("node_modules").join("pkg")).unwrap();
        std::fs::write(
            dir.join("a")
                .join("deep")
                .join("er")
                .join("Invoice_March-2026.pdf"),
            "",
        )
        .unwrap();
        std::fs::write(dir.join("node_modules").join("pkg").join("invoice.js"), "").unwrap();

        let mut idx = NameIndex::open_in_memory().unwrap();
        let n = idx.rebuild(std::slice::from_ref(&dir), || true).unwrap();
        assert!(n.unwrap() >= 5);
        assert!(idx.built().is_some());

        let hits = idx.search("invoice march", 10);
        assert_eq!(hits.len(), 1);
        assert!(hits[0].path.ends_with("Invoice_March-2026.pdf"));
        assert!(!hits[0].folder);
        // Package folders are named but not walked into.
        assert!(
            idx.search("invoice", 10)
                .iter()
                .all(|h| !h.path.to_string_lossy().contains("pkg"))
        );
        assert_eq!(idx.search("deep", 10).first().map(|h| h.folder), Some(true));
        assert!(idx.search("\"; DROP", 10).is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_stopped_rebuild_keeps_the_old_index() {
        let dir = std::env::temp_dir().join(format!("sidekick-names-stop-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("report.docx"), "").unwrap();
        let mut idx = NameIndex::open_in_memory().unwrap();
        idx.rebuild(std::slice::from_ref(&dir), || true).unwrap();
        assert_eq!(
            idx.rebuild(std::slice::from_ref(&dir), || false).unwrap(),
            None
        );
        assert_eq!(idx.search("report", 5).len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn live_updates_add_and_remove_paths() {
        let dir = std::env::temp_dir().join(format!("sidekick-names-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("proj")).unwrap();
        std::fs::write(dir.join("proj").join("budget.xlsx"), "").unwrap();
        std::fs::write(dir.join("notes.txt"), "").unwrap();
        let root = dir.to_string_lossy().into_owned();
        let p = |rel: &str| dir.join(rel).to_string_lossy().into_owned();
        let mut idx = NameIndex::open_in_memory().unwrap();
        let n = idx
            .replace_under(
                &root,
                vec![
                    (p("proj"), true),
                    (
                        dir.join("proj")
                            .join("budget.xlsx")
                            .to_string_lossy()
                            .into_owned(),
                        false,
                    ),
                ],
            )
            .unwrap();
        assert_eq!(n, 2);
        assert_eq!(idx.search("budget", 5).len(), 1);
        idx.upsert(&p("notes.txt"), false).unwrap();
        idx.upsert(&p("notes.txt"), false).unwrap();
        assert_eq!(idx.search("notes", 5).len(), 1);
        // Removing a folder takes what is inside with it.
        idx.remove(&p("proj")).unwrap();
        assert!(idx.search("budget", 5).is_empty());
        assert!(idx.live().is_none());
        idx.mark_live().unwrap();
        assert!(idx.live().is_some() && idx.built().is_some());
        let _ = std::fs::remove_dir_all(dir);
    }
}
