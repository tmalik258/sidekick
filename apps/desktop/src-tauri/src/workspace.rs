//! Files of one project for the editor. Every path is relative to the
//! project root and is checked to stay inside it, symlinks included. Big
//! and binary files are refused; a save that would overwrite a change made
//! on disk since the file was opened is refused too, so an agent's write is
//! never lost.

use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

/// Largest file the editor opens.
pub const MAX_OPEN: u64 = 2 * 1024 * 1024;
/// Most entries the tree lists.
const MAX_TREE: usize = 5000;
/// Most matches a project search returns.
const MAX_HITS: usize = 200;
/// Folders never listed or searched.
const SKIP: [&str; 9] = [
    ".git",
    "node_modules",
    "target",
    ".next",
    "out",
    "dist",
    "build",
    ".venv",
    "__pycache__",
];

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TreeEntry {
    /// Relative path with forward slashes.
    pub path: String,
    pub dir: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenFile {
    pub text: String,
    /// What was on disk when opened; a save sends it back.
    pub stamp: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    pub path: String,
    pub line: usize,
    pub text: String,
}

fn err(msg: &str) -> io::Error {
    io::Error::other(msg.to_owned())
}

/// The project root, which must be a folder.
pub fn root_of(root: &str) -> io::Result<PathBuf> {
    let r = fs::canonicalize(root)?;
    if !r.is_dir() {
        return Err(err("Not a folder."));
    }
    Ok(r)
}

/// `rel` inside `root`, or an error if it would leave it by `..`, an
/// absolute path, or a symlink anywhere along the way.
pub fn confine(root: &Path, rel: &str) -> io::Result<PathBuf> {
    let rel = Path::new(rel);
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(err("That path is outside the project."));
    }
    let joined = root.join(rel);
    // The deepest part that exists must resolve inside the root.
    let mut probe = joined.as_path();
    while !probe.exists() {
        probe = probe.parent().ok_or_else(|| err("Bad path."))?;
    }
    if !fs::canonicalize(probe)?.starts_with(root) {
        return Err(err("That path is outside the project."));
    }
    Ok(joined)
}

fn slash(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Every file and folder, folders first, skipping build output and links.
pub fn tree(root: &Path) -> io::Result<Vec<TreeEntry>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut items: Vec<_> = fs::read_dir(&dir)?.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let Ok(kind) = e.file_type() else { continue };
            if kind.is_symlink() {
                continue;
            }
            let name = e.file_name();
            if kind.is_dir() && SKIP.contains(&name.to_string_lossy().as_ref()) {
                continue;
            }
            let path = e.path();
            let rel = slash(path.strip_prefix(root).unwrap_or(&path));
            out.push(TreeEntry {
                path: rel,
                dir: kind.is_dir(),
            });
            if out.len() >= MAX_TREE {
                return Ok(out);
            }
            if kind.is_dir() {
                stack.push(path);
            }
        }
    }
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(out)
}

fn stamp(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{h:016x}")
}

fn looks_binary(bytes: &[u8]) -> bool {
    bytes.iter().take(8000).any(|b| *b == 0)
}

pub fn read(root: &Path, rel: &str) -> io::Result<OpenFile> {
    let path = confine(root, rel)?;
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(err("Links are not opened."));
    }
    if fs::metadata(&path)?.len() > MAX_OPEN {
        return Err(err("That file is too large to open here."));
    }
    let bytes = fs::read(&path)?;
    if looks_binary(&bytes) {
        return Err(err("That file is not text."));
    }
    let text = String::from_utf8(bytes.clone()).map_err(|_| err("That file is not text."))?;
    Ok(OpenFile {
        text,
        stamp: stamp(&bytes),
    })
}

/// Saves `text` if the file is still what was opened (`stamp`), or new when
/// `stamp` is empty. Returns the new stamp.
pub fn write(root: &Path, rel: &str, text: &str, expected: &str) -> io::Result<String> {
    let path = confine(root, rel)?;
    let now = fs::read(&path).ok();
    match (&now, expected.is_empty()) {
        (Some(b), false) if stamp(b) != expected => {
            return Err(err("Changed on disk since you opened it."));
        }
        (Some(_), true) => return Err(err("That file already exists.")),
        _ => {}
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, text)?;
    Ok(stamp(text.as_bytes()))
}

/// Whether the file on disk still matches what the editor has.
pub fn changed(root: &Path, rel: &str, expected: &str) -> io::Result<bool> {
    let path = confine(root, rel)?;
    Ok(fs::read(path)
        .map(|b| stamp(&b) != expected)
        .unwrap_or(true))
}

/// The file as committed at HEAD, for the change markers. Empty when new.
pub fn head(root: &Path, rel: &str) -> io::Result<String> {
    confine(root, rel)?;
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(root)
        .arg("show")
        .arg(format!("HEAD:{}", rel.replace('\\', "/")))
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output()?;
    if !out.status.success() || looks_binary(&out.stdout) {
        return Ok(String::new());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Lines containing `query` (case insensitive) in text files of the tree.
pub fn search(root: &Path, query: &str) -> io::Result<Vec<Hit>> {
    let q = query.to_lowercase();
    if q.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut hits = Vec::new();
    for e in tree(root)?.into_iter().filter(|e| !e.dir) {
        let Ok(f) = read(root, &e.path) else { continue };
        for (i, line) in f.text.lines().enumerate() {
            if line.to_lowercase().contains(&q) {
                hits.push(Hit {
                    path: e.path.clone(),
                    line: i + 1,
                    text: line.trim().chars().take(200).collect(),
                });
                if hits.len() >= MAX_HITS {
                    return Ok(hits);
                }
            }
        }
    }
    Ok(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("duskie-ws-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        fs::canonicalize(d).unwrap()
    }

    #[test]
    fn refuses_paths_that_leave_the_project() {
        let root = tmp("esc");
        fs::write(root.join("a.txt"), "hi").unwrap();
        assert!(confine(&root, "a.txt").is_ok());
        assert!(confine(&root, "new/dir/b.txt").is_ok());
        for bad in ["../x", "a/../../x", "/etc/passwd"] {
            assert!(confine(&root, bad).is_err(), "{bad}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_symlink_out_of_the_project() {
        let root = tmp("link");
        let outside = tmp("link-out");
        fs::write(outside.join("secret"), "s").unwrap();
        std::os::unix::fs::symlink(&outside, root.join("out")).unwrap();
        assert!(read(&root, "out/secret").is_err());
        assert!(write(&root, "out/new", "x", "").is_err());
        assert!(tree(&root).unwrap().iter().all(|e| e.path != "out"));
    }

    #[test]
    fn refuses_binary_and_huge_files() {
        let root = tmp("bin");
        fs::write(root.join("b.bin"), [0u8, 1, 2]).unwrap();
        assert!(read(&root, "b.bin").is_err());
        fs::write(root.join("big.txt"), vec![b'a'; MAX_OPEN as usize + 1]).unwrap();
        assert!(read(&root, "big.txt").is_err());
    }

    #[test]
    fn a_change_on_disk_blocks_the_save() {
        let root = tmp("race");
        fs::write(root.join("f.txt"), "one").unwrap();
        let open = read(&root, "f.txt").unwrap();
        assert!(!changed(&root, "f.txt", &open.stamp).unwrap());
        fs::write(root.join("f.txt"), "agent wrote this").unwrap();
        assert!(changed(&root, "f.txt", &open.stamp).unwrap());
        assert!(write(&root, "f.txt", "mine", &open.stamp).is_err());
        let fresh = read(&root, "f.txt").unwrap();
        let s = write(&root, "f.txt", "mine", &fresh.stamp).unwrap();
        assert_eq!(read(&root, "f.txt").unwrap().stamp, s);
    }

    #[test]
    fn tree_skips_build_output_and_search_finds_lines() {
        let root = tmp("tree");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(root.join("node_modules/x")).unwrap();
        fs::write(root.join("src/main.rs"), "fn main() {\n    hello();\n}").unwrap();
        fs::write(root.join("node_modules/x/i.js"), "hello").unwrap();
        let t = tree(&root).unwrap();
        assert_eq!(
            t.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["src", "src/main.rs"]
        );
        let hits = search(&root, "HELLO").unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].path.as_str(), hits[0].line), ("src/main.rs", 2));
    }
}
