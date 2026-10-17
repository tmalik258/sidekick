//! Agent skills (SKILL.md folders) shared between Claude Code, Codex and the
//! local model. Duskie keeps one library of copies with a manifest; it writes
//! copies, never links, into each tool's folder; it never runs anything on
//! import or sync; and it never overwrites without a backup.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where a skill was found.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    Claude,
    /// A Claude Code plugin's skill: read only, never written to.
    ClaudePlugin,
    Codex,
    Duskie,
    Url,
}

/// The tools a skill can be shared with.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Claude,
    Codex,
    Local,
}

/// A skill folder on GitHub: owner, repo, ref and the path inside.
#[derive(Debug, Clone, PartialEq)]
pub struct GithubSkill {
    pub owner: String,
    pub repo: String,
    pub reference: String,
    pub path: String,
}

/// Reads `https://github.com/o/r/tree/<ref>/<path>` (or a repo link, which
/// means the default branch at the root).
pub fn parse_github(url: &str) -> Option<GithubSkill> {
    let rest = url
        .trim()
        .trim_end_matches('/')
        .strip_prefix("https://github.com/")?;
    let parts: Vec<&str> = rest.split('/').collect();
    let ok = |p: &str| {
        !p.is_empty()
            && p != ".."
            && p.chars()
                .all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
    };
    let (owner, repo) = (parts.first()?, parts.get(1)?.trim_end_matches(".git"));
    if !ok(owner) || !ok(repo) {
        return None;
    }
    let (reference, path) = match parts.get(2) {
        None => ("HEAD".to_owned(), String::new()),
        Some(&"tree") | Some(&"blob") => {
            let r = parts.get(3)?;
            let path = parts[4..].join("/");
            if !ok(r) || parts[4..].iter().any(|p| !ok(p)) {
                return None;
            }
            (
                r.to_string(),
                path.trim_end_matches("/SKILL.md")
                    .trim_end_matches("SKILL.md")
                    .to_owned(),
            )
        }
        _ => return None,
    };
    Some(GithubSkill {
        owner: owner.to_string(),
        repo: repo.to_owned(),
        reference,
        path,
    })
}

pub const TARGETS: [Target; 3] = [Target::Claude, Target::Codex, Target::Local];

/// Most files and bytes one skill may have.
pub const MAX_FILES: usize = 200;
pub const MAX_BYTES: u64 = 5 * 1024 * 1024;
const SCRIPT_EXT: [&str; 9] = ["sh", "bash", "ps1", "py", "js", "ts", "bat", "cmd", "exe"];

/// A skill folder as found on disk.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Found {
    pub name: String,
    pub description: String,
    pub origin: Origin,
    pub dir: PathBuf,
    pub hash: String,
    /// Files other than SKILL.md, relative to the folder.
    pub files: Vec<String>,
    /// It carries a script: never preselected, and never run by Duskie.
    pub has_scripts: bool,
}

/// One library entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub description: String,
    pub origin: Origin,
    pub hash: String,
    pub has_scripts: bool,
    pub targets: BTreeMap<Target, bool>,
    /// Hash last written to each target, so a later change there is noticed.
    #[serde(default)]
    pub synced: BTreeMap<Target, String>,
    /// For skills installed from a link: the source and pinned commit.
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub commit: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub skills: BTreeMap<String, Entry>,
}

/// Name and description from a SKILL.md frontmatter block.
pub fn frontmatter(text: &str) -> Option<(String, String)> {
    let body = text.strip_prefix("---")?;
    let end = body.find("\n---")?;
    let mut name = None;
    let mut description = None;
    for line in body[..end].lines() {
        if let Some((k, v)) = line.split_once(':') {
            let v = v.trim().trim_matches('"').trim_matches('\'').to_owned();
            match k.trim() {
                "name" => name = Some(v),
                "description" => description = Some(v),
                _ => {}
            }
        }
    }
    Some((name?, description.unwrap_or_default()))
}

/// A stable hash of a folder's files and their contents (FNV-1a 64).
pub fn hash_dir(dir: &Path) -> io::Result<String> {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |bytes: &[u8]| {
        for b in bytes {
            h ^= u64::from(*b);
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
    };
    for rel in files_of(dir)? {
        eat(rel.as_bytes());
        eat(&[0]);
        eat(&fs::read(dir.join(&rel))?);
        eat(&[0]);
    }
    Ok(format!("{h:016x}"))
}

/// Every file under `dir`, relative, sorted, with forward slashes. Links are
/// skipped so nothing outside the folder is read.
fn files_of(dir: &Path) -> io::Result<Vec<String>> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in fs::read_dir(&d)? {
            let e = e?;
            let kind = e.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                stack.push(e.path());
            } else if let Ok(rel) = e.path().strip_prefix(dir) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
            if out.len() > MAX_FILES {
                return Err(io::Error::other("too many files"));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn is_script(rel: &str) -> bool {
    rel.rsplit_once('.')
        .is_some_and(|(_, ext)| SCRIPT_EXT.contains(&ext.to_ascii_lowercase().as_str()))
}

/// Reads one skill folder; None when it has no usable SKILL.md or is too big.
pub fn read_skill(dir: &Path, origin: Origin) -> Option<Found> {
    let text = fs::read_to_string(dir.join("SKILL.md")).ok()?;
    let (name, description) = frontmatter(&text)?;
    let files = files_of(dir).ok()?;
    let size: u64 = files
        .iter()
        .filter_map(|f| fs::metadata(dir.join(f)).ok())
        .map(|m| m.len())
        .sum();
    if size > MAX_BYTES || !valid_name(&name) {
        return None;
    }
    Some(Found {
        has_scripts: files.iter().any(|f| is_script(f)),
        files: files.into_iter().filter(|f| f != "SKILL.md").collect(),
        hash: hash_dir(dir).ok()?,
        name,
        description,
        origin,
        dir: dir.to_path_buf(),
    })
}

/// A name usable as a folder: letters, digits, dash and underscore.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

/// Every skill directly under `root` (each in its own folder).
pub fn scan(root: &Path, origin: Origin) -> Vec<Found> {
    let Ok(entries) = fs::read_dir(root) else {
        return Vec::new();
    };
    let mut out: Vec<Found> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| read_skill(&e.path(), origin))
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// Plugin skills: `<plugins>/**/skills/<name>/SKILL.md`, a few levels deep.
pub fn scan_plugins(root: &Path) -> Vec<Found> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(entries) = fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            if !e.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            if e.file_name() == "skills" {
                out.extend(scan(&e.path(), Origin::ClaudePlugin));
            } else if depth < 5 {
                stack.push((e.path(), depth + 1));
            }
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

/// The review list: one row per name and content, first source wins.
pub fn dedupe(mut all: Vec<Found>) -> Vec<Found> {
    all.sort_by(|a, b| a.origin.cmp(&b.origin).then_with(|| a.name.cmp(&b.name)));
    let mut seen: Vec<(String, String)> = Vec::new();
    all.retain(|f| {
        let key = (f.name.clone(), f.hash.clone());
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    all
}

fn copy_dir(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for rel in files_of(from)? {
        let dest = to.join(&rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(from.join(&rel), dest)?;
    }
    Ok(())
}

/// The library folder: `skills/<name>` copies, `backups/`, `manifest.json`.
pub struct Library {
    pub root: PathBuf,
}

/// What a sync did to one target.
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub written: Vec<String>,
    /// Same name there with other content Duskie did not write: left alone.
    pub conflicts: Vec<String>,
    pub backups: Vec<String>,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn manifest_path(&self) -> PathBuf {
        self.root.join("manifest.json")
    }

    pub fn dir_of(&self, name: &str) -> PathBuf {
        self.root.join("skills").join(name)
    }

    pub fn load(&self) -> Manifest {
        fs::read(self.manifest_path())
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, m: &Manifest) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;
        let tmp = self.manifest_path().with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(m)?)?;
        fs::rename(tmp, self.manifest_path())
    }

    fn backup(&self, from: &Path, label: &str) -> io::Result<String> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or_default();
        let to = self.root.join("backups").join(format!("{stamp}-{label}"));
        copy_dir(from, &to)?;
        Ok(to.to_string_lossy().into_owned())
    }

    /// Copies a found skill into the library and shares it with every target
    /// except where it came from. Nothing in it runs. A same-name skill with
    /// other content is backed up first.
    pub fn import(&self, found: &Found) -> io::Result<()> {
        let mut m = self.load();
        let dest = self.dir_of(&found.name);
        if dest.exists() {
            if m.skills
                .get(&found.name)
                .is_some_and(|e| e.hash == found.hash)
            {
                return Ok(());
            }
            self.backup(&dest, &found.name)?;
            fs::remove_dir_all(&dest)?;
        }
        copy_dir(&found.dir, &dest)?;
        let home = match found.origin {
            Origin::Claude => Some(Target::Claude),
            Origin::Codex => Some(Target::Codex),
            _ => None,
        };
        // Keep what earlier syncs wrote, so their copies still count as ours.
        let mut synced = m
            .skills
            .get(&found.name)
            .map(|e| e.synced.clone())
            .unwrap_or_default();
        if let Some(t) = home {
            synced.insert(t, found.hash.clone());
        }
        m.skills.insert(
            found.name.clone(),
            Entry {
                name: found.name.clone(),
                description: found.description.clone(),
                origin: found.origin,
                hash: found.hash.clone(),
                has_scripts: found.has_scripts,
                targets: TARGETS.iter().map(|t| (*t, true)).collect(),
                synced,
                url: None,
                commit: None,
            },
        );
        self.save(&m)
    }

    pub fn set_target(&self, name: &str, target: Target, on: bool) -> io::Result<()> {
        let mut m = self.load();
        if let Some(e) = m.skills.get_mut(name) {
            e.targets.insert(target, on);
        }
        self.save(&m)
    }

    /// Records where a skill came from, so it can be updated later.
    pub fn set_source(&self, name: &str, url: &str, commit: &str) -> io::Result<()> {
        let mut m = self.load();
        if let Some(e) = m.skills.get_mut(name) {
            e.url = Some(url.to_owned());
            e.commit = Some(commit.to_owned());
        }
        self.save(&m)
    }

    /// Removes a skill from the library (copies already written stay).
    pub fn remove(&self, name: &str) -> io::Result<()> {
        let mut m = self.load();
        if m.skills.remove(name).is_some() {
            let dir = self.dir_of(name);
            if dir.exists() {
                self.backup(&dir, name)?;
                fs::remove_dir_all(dir)?;
            }
        }
        self.save(&m)
    }

    /// Writes every skill switched on for `target` into `dir`. A folder there
    /// is replaced only when Duskie wrote it last (its hash matches what was
    /// synced) and always after a backup; anything else is a conflict and is
    /// left alone. Hash based, so running it twice changes nothing.
    pub fn sync(&self, target: Target, dir: &Path) -> io::Result<SyncReport> {
        let mut m = self.load();
        let mut report = SyncReport::default();
        for e in m.skills.values_mut() {
            if !e.targets.get(&target).copied().unwrap_or(false) {
                continue;
            }
            let dest = dir.join(&e.name);
            if dest.exists() {
                let there = hash_dir(&dest)?;
                if there == e.hash {
                    e.synced.insert(target, there);
                    continue;
                }
                if e.synced.get(&target) != Some(&there) {
                    report.conflicts.push(e.name.clone());
                    continue;
                }
                report.backups.push(self.backup(&dest, &e.name)?);
                fs::remove_dir_all(&dest)?;
            }
            copy_dir(&self.dir_of(&e.name), &dest)?;
            e.synced.insert(target, e.hash.clone());
            report.written.push(e.name.clone());
        }
        self.save(&m)?;
        Ok(report)
    }

    /// Skills changed in a target folder since Duskie wrote them: the new
    /// content is waiting for the user's one tap to be shared.
    pub fn changed_in(&self, target: Target, dir: &Path) -> Vec<Found> {
        let m = self.load();
        let origin = match target {
            Target::Claude => Origin::Claude,
            Target::Codex => Origin::Codex,
            Target::Local => Origin::Duskie,
        };
        scan(dir, origin)
            .into_iter()
            .filter(|f| match m.skills.get(&f.name) {
                // Unknown here: new in that tool.
                None => true,
                Some(e) => f.hash != e.hash && e.synced.get(&target) != Some(&f.hash),
            })
            .collect()
    }

    /// The body of a skill for the local model, without its frontmatter.
    pub fn body(&self, name: &str) -> Option<String> {
        if !valid_name(name) {
            return None;
        }
        let text = fs::read_to_string(self.dir_of(name).join("SKILL.md")).ok()?;
        let rest = text.strip_prefix("---")?;
        let end = rest.find("\n---")?;
        Some(rest[end + 4..].trim().to_owned())
    }
}

/// A short index for the local model: the skills most related to the
/// question first, as many as fit in `budget` tokens (about 4 characters each).
pub fn index(entries: &[Entry], question: &str, budget: usize) -> String {
    let words: Vec<String> = question
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(str::to_lowercase)
        .collect();
    let score = |e: &Entry| {
        let hay = format!("{} {}", e.name, e.description).to_lowercase();
        words.iter().filter(|w| hay.contains(w.as_str())).count()
    };
    let mut ranked: Vec<&Entry> = entries
        .iter()
        .filter(|e| e.targets.get(&Target::Local).copied().unwrap_or(false))
        .collect();
    ranked.sort_by(|a, b| score(b).cmp(&score(a)).then_with(|| a.name.cmp(&b.name)));
    let mut out = String::new();
    for e in ranked {
        let line = format!(
            "- {}: {}\n",
            e.name,
            e.description.chars().take(200).collect::<String>()
        );
        if (out.len() + line.len()) / 4 > budget {
            break;
        }
        out.push_str(&line);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "duskie-lib-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn skill(root: &Path, name: &str, body: &str, script: bool) {
        let d = root.join(name);
        fs::create_dir_all(&d).unwrap();
        fs::write(
            d.join("SKILL.md"),
            format!("---\nname: {name}\ndescription: Does {name} things\n---\n{body}\n"),
        )
        .unwrap();
        if script {
            fs::write(d.join("run.sh"), "touch SHOULD_NOT_EXIST\n").unwrap();
        }
    }

    #[test]
    fn reads_frontmatter_and_marks_scripts() {
        let claude = tmp("fm");
        skill(&claude, "pdf", "Read PDFs.", true);
        skill(&claude, "notes", "Take notes.", false);
        let found = scan(&claude, Origin::Claude);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].name, "notes");
        assert!(!found[0].has_scripts);
        assert!(found[1].has_scripts);
        assert_eq!(found[1].files, ["run.sh"]);
        assert_eq!(
            frontmatter("---\nname: a\ndescription: \"b c\"\n---\nbody"),
            Some(("a".into(), "b c".into()))
        );
        assert!(!valid_name("../x"));
    }

    #[test]
    fn round_trip_between_targets_without_loops_or_running_scripts() {
        let (lib_dir, claude, codex) = (tmp("lib"), tmp("claude"), tmp("codex"));
        skill(&claude, "pdf", "Read PDFs.", true);
        let lib = Library::new(&lib_dir);
        for f in scan(&claude, Origin::Claude) {
            lib.import(&f).unwrap();
        }
        // Nothing was run on import.
        assert!(!lib_dir.join("skills/pdf/SHOULD_NOT_EXIST").exists());
        let r = lib.sync(Target::Codex, &codex).unwrap();
        assert_eq!(r.written, ["pdf"]);
        assert!(codex.join("pdf/SKILL.md").exists());
        assert!(!codex.join("pdf/SHOULD_NOT_EXIST").exists());
        // Back to Claude: already there with the same content, nothing written.
        assert!(
            lib.sync(Target::Claude, &claude)
                .unwrap()
                .written
                .is_empty()
        );
        // Again: no loop.
        assert_eq!(
            lib.sync(Target::Codex, &codex).unwrap(),
            SyncReport::default()
        );
        assert!(lib.changed_in(Target::Codex, &codex).is_empty());
        // A change made in Codex is noticed, not copied on its own.
        fs::write(
            codex.join("pdf/SKILL.md"),
            "---\nname: pdf\ndescription: new\n---\nNew.",
        )
        .unwrap();
        let changed = lib.changed_in(Target::Codex, &codex);
        assert_eq!(changed.len(), 1);
        assert!(
            !fs::read_to_string(claude.join("pdf/SKILL.md"))
                .unwrap()
                .contains("new")
        );
    }

    #[test]
    fn a_skill_duskie_did_not_write_is_a_conflict_and_left_alone() {
        let (lib_dir, claude, codex) = (tmp("lib2"), tmp("claude2"), tmp("codex2"));
        skill(&claude, "pdf", "Read PDFs.", false);
        skill(&codex, "pdf", "Something else.", false);
        let lib = Library::new(&lib_dir);
        lib.import(&scan(&claude, Origin::Claude)[0]).unwrap();
        let r = lib.sync(Target::Codex, &codex).unwrap();
        assert_eq!(r.conflicts, ["pdf"]);
        assert!(
            fs::read_to_string(codex.join("pdf/SKILL.md"))
                .unwrap()
                .contains("Something else")
        );
    }

    #[test]
    fn replacing_keeps_a_backup_that_restores() {
        let (lib_dir, claude, codex) = (tmp("lib3"), tmp("claude3"), tmp("codex3"));
        skill(&claude, "pdf", "v1", false);
        let lib = Library::new(&lib_dir);
        lib.import(&scan(&claude, Origin::Claude)[0]).unwrap();
        lib.sync(Target::Codex, &codex).unwrap();
        skill(&claude, "pdf", "v2", false);
        lib.import(&scan(&claude, Origin::Claude)[0]).unwrap();
        let r = lib.sync(Target::Codex, &codex).unwrap();
        assert_eq!(r.written, ["pdf"]);
        assert_eq!(r.backups.len(), 1);
        let backup = PathBuf::from(&r.backups[0]);
        assert!(
            fs::read_to_string(backup.join("SKILL.md"))
                .unwrap()
                .contains("v1")
        );
        assert!(
            fs::read_to_string(codex.join("pdf/SKILL.md"))
                .unwrap()
                .contains("v2")
        );
    }

    #[test]
    fn reads_github_links_and_refuses_odd_ones() {
        let g = parse_github("https://github.com/acme/skills/tree/main/pdf").unwrap();
        assert_eq!(
            (
                g.owner.as_str(),
                g.repo.as_str(),
                g.reference.as_str(),
                g.path.as_str()
            ),
            ("acme", "skills", "main", "pdf")
        );
        let g = parse_github("https://github.com/acme/skills/blob/v1/a/b/SKILL.md").unwrap();
        assert_eq!(g.path, "a/b");
        assert_eq!(
            parse_github("https://github.com/acme/skills")
                .unwrap()
                .reference,
            "HEAD"
        );
        assert!(parse_github("https://github.com/acme/skills/tree/main/../x").is_none());
        assert!(parse_github("http://github.com/acme/skills").is_none());
        assert!(parse_github("https://evil.com/acme/skills").is_none());
    }

    #[test]
    fn dedupes_by_name_and_content() {
        let (a, b) = (tmp("da"), tmp("db"));
        skill(&a, "pdf", "same", false);
        skill(&b, "pdf", "same", false);
        skill(&b, "x", "other", false);
        let all = dedupe([scan(&a, Origin::Claude), scan(&b, Origin::Codex)].concat());
        assert_eq!(
            all.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["pdf", "x"]
        );
        assert_eq!(all[0].origin, Origin::Claude);
    }

    #[test]
    fn local_index_ranks_and_fits_the_budget() {
        let entry = |name: &str, desc: &str| Entry {
            name: name.into(),
            description: desc.into(),
            origin: Origin::Claude,
            hash: String::new(),
            has_scripts: false,
            targets: TARGETS.iter().map(|t| (*t, true)).collect(),
            synced: BTreeMap::new(),
            url: None,
            commit: None,
        };
        let mut all: Vec<Entry> = (0..400)
            .map(|i| {
                entry(
                    &format!("skill-{i}"),
                    "a long description of something unrelated to the task",
                )
            })
            .collect();
        all.push(entry("pdf", "Read and fill PDF forms"));
        let idx = index(&all, "fill this pdf form", 1500);
        assert!(idx.starts_with("- pdf:"), "{}", &idx[..40]);
        assert!(idx.len() / 4 <= 1500);
        let lib = Library::new(tmp("body"));
        assert_eq!(lib.body("../etc"), None);
    }
}
