//! The live drive index. On Windows a small service reads each NTFS
//! drive's file table once, then follows its change journal, so file
//! search sees new, renamed and deleted files within seconds instead of
//! waiting for the daily rebuild.
//!
//! The tree and record parser here are plain Rust so they are tested on
//! every platform; only `volume` talks to Windows.

use std::collections::HashMap;

#[cfg(windows)]
pub mod volume;

/// USN reason flags this index cares about.
pub const REASON_CREATE: u32 = 0x0000_0100;
pub const REASON_DELETE: u32 = 0x0000_0200;
pub const REASON_RENAME_OLD: u32 = 0x0000_1000;
pub const REASON_RENAME_NEW: u32 = 0x0000_2000;
pub const REASON_CLOSE: u32 = 0x8000_0000;
const ATTR_DIRECTORY: u32 = 0x10;

/// One file or folder as the file table or journal describes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    pub frn: u64,
    pub parent: u64,
    pub usn: i64,
    pub reason: u32,
    pub dir: bool,
    pub name: String,
}

/// Reads the USN_RECORD_V2 entries packed in `buf` (after the 8-byte
/// header Windows puts first). Stops at anything malformed.
pub fn parse_records(buf: &[u8]) -> Vec<Record> {
    let mut out = Vec::new();
    let mut at = 0usize;
    while at + 60 <= buf.len() {
        let len = u32_at(buf, at) as usize;
        if len < 60 || at + len > buf.len() {
            break;
        }
        let r = &buf[at..at + len];
        if u16::from_le_bytes([r[4], r[5]]) == 2 {
            let name_len = u16::from_le_bytes([r[56], r[57]]) as usize;
            let name_off = u16::from_le_bytes([r[58], r[59]]) as usize;
            if name_off + name_len <= len {
                let units: Vec<u16> = r[name_off..name_off + name_len]
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|c| u16::from_le_bytes(*c))
                    .collect();
                out.push(Record {
                    frn: u64_at(r, 8) & 0x0000_FFFF_FFFF_FFFF,
                    parent: u64_at(r, 16) & 0x0000_FFFF_FFFF_FFFF,
                    usn: u64_at(r, 24) as i64,
                    reason: u32_at(r, 40),
                    dir: u32_at(r, 52) & ATTR_DIRECTORY != 0,
                    name: String::from_utf16_lossy(&units),
                });
            }
        }
        at += len;
    }
    out
}

fn u32_at(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap_or_default())
}

fn u64_at(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap_or_default())
}

#[derive(Debug, Clone)]
struct Node {
    parent: u64,
    name: String,
    dir: bool,
}

/// What a journal record means for the name index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Upsert { path: String, dir: bool },
    Remove { path: String },
}

/// Every file on one drive by file reference number, so a path can be
/// built from any record's parent chain.
pub struct Tree {
    root: String,
    root_frn: u64,
    nodes: HashMap<u64, Node>,
}

/// The root folder's file reference number on NTFS.
pub const ROOT_FRN: u64 = 5;

impl Tree {
    /// `root` is the drive, like `C:\`.
    pub fn new(root: &str) -> Self {
        Self {
            root: root.trim_end_matches('\\').to_string(),
            root_frn: ROOT_FRN,
            nodes: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    pub fn insert(&mut self, r: &Record) {
        if r.frn == self.root_frn {
            return;
        }
        self.nodes.insert(
            r.frn,
            Node {
                parent: r.parent,
                name: r.name.clone(),
                dir: r.dir,
            },
        );
    }

    /// Full path, or None when a parent is unknown (system metadata files).
    pub fn path(&self, frn: u64) -> Option<String> {
        let mut parts = Vec::new();
        let mut at = frn;
        while at != self.root_frn {
            let n = self.nodes.get(&at)?;
            parts.push(n.name.as_str());
            at = n.parent;
            if parts.len() > 512 {
                return None;
            }
        }
        parts.reverse();
        Some(format!("{}\\{}", self.root, parts.join("\\")))
    }

    /// Every (path, is folder) in the tree, for the first full write.
    pub fn entries(&self) -> impl Iterator<Item = (String, bool)> + '_ {
        self.nodes
            .iter()
            .filter_map(|(frn, n)| Some((self.path(*frn)?, n.dir)))
    }

    /// Applies one journal record. Only final records (with CLOSE) and
    /// the old half of a rename produce a change, so a file being written
    /// in many steps updates the index once.
    pub fn apply(&mut self, r: &Record) -> Option<Change> {
        if r.reason & REASON_RENAME_OLD != 0 {
            let path = self.path(r.frn);
            self.nodes.remove(&r.frn);
            return path.map(|path| Change::Remove { path });
        }
        if r.reason & REASON_CLOSE == 0 {
            return None;
        }
        if r.reason & REASON_DELETE != 0 {
            let path = self.path(r.frn);
            self.nodes.remove(&r.frn);
            return path.map(|path| Change::Remove { path });
        }
        if r.reason & (REASON_CREATE | REASON_RENAME_NEW) != 0 {
            self.insert(r);
            return self
                .path(r.frn)
                .map(|path| Change::Upsert { path, dir: r.dir });
        }
        None
    }
}

/// Settings the app writes for the service in ProgramData.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Config {
    /// The signed-in user's home folder; other users' homes are skipped.
    pub profile: String,
}

impl Config {
    /// True when `path` belongs in the index: not inside a skipped folder
    /// and not inside another user's home.
    pub fn keeps(&self, path: &str) -> bool {
        let lower = path.to_lowercase();
        let parts: Vec<&str> = lower.split('\\').collect();
        if parts.iter().any(|p| {
            sidekick_core::names::SKIP
                .iter()
                .any(|s| s.eq_ignore_ascii_case(p))
        }) {
            return false;
        }
        // "c:\users\<name>\..." is kept only for this user's own name.
        if parts.len() > 2 && parts[1] == "users" {
            let mine = self.profile.to_lowercase();
            let home = format!("{}\\{}\\{}", parts[0], parts[1], parts[2]);
            return mine.is_empty() || home == mine.trim_end_matches('\\') || parts[2] == "public";
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(frn: u64, parent: u64, name: &str, dir: bool, reason: u32) -> Record {
        Record {
            frn,
            parent,
            usn: 0,
            reason,
            dir,
            name: name.into(),
        }
    }

    fn encode(r: &Record) -> Vec<u8> {
        let name: Vec<u8> = r.name.encode_utf16().flat_map(u16::to_le_bytes).collect();
        let len = (60 + name.len()).div_ceil(8) * 8;
        let mut b = vec![0u8; len];
        b[0..4].copy_from_slice(&(len as u32).to_le_bytes());
        b[4..6].copy_from_slice(&2u16.to_le_bytes());
        b[8..16].copy_from_slice(&(r.frn | 0x0003_0000_0000_0000).to_le_bytes());
        b[16..24].copy_from_slice(&r.parent.to_le_bytes());
        b[24..32].copy_from_slice(&(r.usn as u64).to_le_bytes());
        b[40..44].copy_from_slice(&r.reason.to_le_bytes());
        b[52..56].copy_from_slice(&(if r.dir { ATTR_DIRECTORY } else { 0x20 }).to_le_bytes());
        b[56..58].copy_from_slice(&(name.len() as u16).to_le_bytes());
        b[58..60].copy_from_slice(&60u16.to_le_bytes());
        b[60..60 + name.len()].copy_from_slice(&name);
        b
    }

    #[test]
    fn parses_packed_records_and_strips_sequence_numbers() {
        let a = rec(40, 5, "Projects", true, REASON_CREATE | REASON_CLOSE);
        let b = rec(41, 40, "notes é.txt", false, REASON_CLOSE);
        let mut buf = encode(&a);
        buf.extend(encode(&b));
        buf.extend([0u8; 12]);
        assert_eq!(parse_records(&buf), vec![a, b]);
    }

    #[test]
    fn builds_paths_and_follows_create_rename_delete() {
        let mut t = Tree::new("C:\\");
        t.insert(&rec(40, 5, "Projects", true, 0));
        t.insert(&rec(41, 40, "a.txt", false, 0));
        assert_eq!(t.path(41).as_deref(), Some("C:\\Projects\\a.txt"));
        assert_eq!(t.path(99), None);

        assert_eq!(t.apply(&rec(42, 40, "b.txt", false, REASON_CREATE)), None);
        assert_eq!(
            t.apply(&rec(42, 40, "b.txt", false, REASON_CREATE | REASON_CLOSE)),
            Some(Change::Upsert {
                path: "C:\\Projects\\b.txt".into(),
                dir: false
            })
        );
        assert_eq!(
            t.apply(&rec(41, 40, "a.txt", false, REASON_RENAME_OLD)),
            Some(Change::Remove {
                path: "C:\\Projects\\a.txt".into()
            })
        );
        assert_eq!(
            t.apply(&rec(
                41,
                5,
                "c.txt",
                false,
                REASON_RENAME_NEW | REASON_CLOSE
            )),
            Some(Change::Upsert {
                path: "C:\\c.txt".into(),
                dir: false
            })
        );
        assert_eq!(
            t.apply(&rec(42, 40, "b.txt", false, REASON_DELETE | REASON_CLOSE)),
            Some(Change::Remove {
                path: "C:\\Projects\\b.txt".into()
            })
        );
        assert_eq!(t.len(), 2);
    }

    #[test]
    fn config_skips_other_users_and_noise_folders() {
        let c = Config {
            profile: "C:\\Users\\Ana".into(),
        };
        assert!(c.keeps("C:\\Users\\Ana\\Documents\\cv.pdf"));
        assert!(c.keeps("C:\\Users\\Public\\x.txt"));
        assert!(!c.keeps("C:\\Users\\Bob\\secret.txt"));
        assert!(!c.keeps("C:\\Users\\Ana\\code\\node_modules\\x.js"));
        assert!(c.keeps("D:\\Games\\save.dat"));
    }
}
