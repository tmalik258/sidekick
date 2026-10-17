//! The skills library: skills found in Claude Code and Codex folders, copied
//! into one place, then written back to each tool the user switched on.
//! Nothing in a skill runs here; files are only copied and hashed.

use std::path::PathBuf;

use duskie_skills::library::{self, Entry, Found, Library, Origin, SyncReport, Target};

use super::*;

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn target_dir(t: Target) -> Option<PathBuf> {
    match t {
        Target::Claude => Some(home().join(".claude").join("skills")),
        Target::Codex => Some(home().join(".codex").join("skills")),
        // The library itself is what the local model reads.
        Target::Local => None,
    }
}

pub(crate) fn library_of(app: &AppHandle) -> CmdResult<Library> {
    app.path()
        .app_data_dir()
        .map(|d| Library::new(d.join("skills-library")))
        .map_err(|e| e.to_string())
}

fn entries(lib: &Library) -> Vec<Entry> {
    lib.load().skills.into_values().collect()
}

/// Everything on this PC that is not in the library yet, or differs from it.
fn review_now(lib: &Library) -> Vec<Found> {
    let h = home();
    let all = library::dedupe(
        [
            library::scan(&h.join(".claude").join("skills"), Origin::Claude),
            library::scan_plugins(&h.join(".claude").join("plugins")),
            library::scan(&h.join(".codex").join("skills"), Origin::Codex),
        ]
        .concat(),
    );
    let have = lib.load().skills;
    all.into_iter()
        .filter(|f| {
            have.get(&f.name)
                .is_none_or(|e| e.hash != f.hash && !e.synced.values().any(|h| *h == f.hash))
        })
        .collect()
}

#[tauri::command]
pub async fn library_review(app: AppHandle) -> CmdResult<Vec<Found>> {
    let lib = library_of(&app)?;
    off_ui(move || review_now(&lib)).await
}

#[tauri::command]
pub async fn library_list(app: AppHandle) -> CmdResult<Vec<Entry>> {
    let lib = library_of(&app)?;
    off_ui(move || entries(&lib)).await
}

#[tauri::command]
pub async fn library_import(app: AppHandle, names: Vec<String>) -> CmdResult<Vec<Entry>> {
    let lib = library_of(&app)?;
    off_ui(move || {
        for f in review_now(&lib).iter().filter(|f| names.contains(&f.name)) {
            lib.import(f).map_err(|e| e.to_string())?;
        }
        Ok(entries(&lib))
    })
    .await?
}

#[tauri::command]
pub fn library_set_target(
    app: AppHandle,
    name: String,
    target: Target,
    on: bool,
) -> CmdResult<Vec<Entry>> {
    let lib = library_of(&app)?;
    lib.set_target(&name, target, on)
        .map_err(|e| e.to_string())?;
    Ok(entries(&lib))
}

#[tauri::command]
pub fn library_remove(app: AppHandle, name: String) -> CmdResult<Vec<Entry>> {
    let lib = library_of(&app)?;
    lib.remove(&name).map_err(|e| e.to_string())?;
    Ok(entries(&lib))
}

/// Writes the library to every tool. Copies the user changed by hand are
/// reported as conflicts and left alone; anything replaced is backed up.
#[tauri::command]
pub async fn library_sync(app: AppHandle) -> CmdResult<SyncReport> {
    let lib = library_of(&app)?;
    off_ui(move || {
        let mut all = SyncReport::default();
        for t in library::TARGETS {
            let Some(dir) = target_dir(t) else { continue };
            let r = lib.sync(t, &dir).map_err(|e| e.to_string())?;
            all.written.extend(r.written);
            all.conflicts.extend(r.conflicts);
            all.backups.extend(r.backups);
        }
        Ok(all)
    })
    .await?
}

pub const FOUND_EVENT: &str = "library://found";
/// How often the skill folders are looked at again.
const WATCH_EVERY: std::time::Duration = std::time::Duration::from_secs(600);

/// Names in `found` not told before; adds them to `told`.
pub fn fresh(found: &[Found], told: &mut std::collections::BTreeSet<String>) -> Vec<String> {
    found
        .iter()
        .filter(|f| told.insert(format!("{}:{}", f.name, f.hash)))
        .map(|f| f.name.clone())
        .collect()
}

/// Looks at the skill folders now and then and says once per new skill
/// that it can be added. Nothing is imported without a tap.
pub fn library_watch(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let a = app.clone();
            let _ = off_ui(move || {
                let Ok(lib) = library_of(&a) else { return };
                let path = lib.root.join("told.json");
                let mut told: std::collections::BTreeSet<String> = std::fs::read(&path)
                    .ok()
                    .and_then(|b| serde_json::from_slice(&b).ok())
                    .unwrap_or_default();
                let names = fresh(&review_now(&lib), &mut told);
                if names.is_empty() {
                    return;
                }
                let _ = std::fs::create_dir_all(&lib.root);
                if let Ok(b) = serde_json::to_vec(&told) {
                    let _ = std::fs::write(&path, b);
                }
                let _ = a.emit(FOUND_EVENT, serde_json::json!({ "names": names }));
            })
            .await;
            tokio::time::sleep(WATCH_EVERY).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn found(name: &str, hash: &str) -> Found {
        Found {
            name: name.into(),
            description: String::new(),
            origin: Origin::Claude,
            dir: PathBuf::new(),
            hash: hash.into(),
            files: Vec::new(),
            has_scripts: false,
        }
    }

    #[test]
    fn tells_once_per_skill_version() {
        let mut told = Default::default();
        assert_eq!(fresh(&[found("pdf", "a")], &mut told), ["pdf"]);
        assert!(fresh(&[found("pdf", "a")], &mut told).is_empty());
        assert_eq!(fresh(&[found("pdf", "b")], &mut told), ["pdf"]);
    }
}

/// One skill found behind a link, and how it compares with the library.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkSkill {
    pub name: String,
    pub description: String,
    pub files: usize,
    pub bytes: u64,
    pub has_scripts: bool,
    /// "new", "changed" or "same".
    pub status: String,
    /// Files added, changed or removed against the library copy.
    pub diff: Vec<String>,
}

/// What a link holds, downloaded at one commit, before anything is added.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkPreview {
    pub url: String,
    pub commit: String,
    pub skills: Vec<LinkSkill>,
}

fn fetch_dir(sha: &str) -> PathBuf {
    std::env::temp_dir().join(format!("duskie-fetch-{sha}"))
}

/// Skill folders under `dir` (a SKILL.md in it), a few levels deep.
fn skill_dirs(dir: &std::path::Path, depth: usize, out: &mut Vec<PathBuf>) {
    if dir.join("SKILL.md").is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    if depth == 0 {
        return;
    }
    let Ok(items) = std::fs::read_dir(dir) else {
        return;
    };
    let mut items: Vec<_> = items.flatten().collect();
    items.sort_by_key(|e| e.file_name());
    for e in items {
        if e.file_type().is_ok_and(|t| t.is_dir()) {
            skill_dirs(&e.path(), depth - 1, out);
        }
    }
}

fn file_hashes(dir: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut out = std::collections::BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(p);
            } else if let (Ok(rel), Ok(b)) = (p.strip_prefix(dir), std::fs::read(&p)) {
                out.insert(rel.to_string_lossy().replace('\\', "/"), b);
            }
        }
    }
    out
}

/// File level differences between the library copy and a new one.
pub fn diff_dirs(old: &std::path::Path, new: &std::path::Path) -> Vec<String> {
    let (a, b) = (file_hashes(old), file_hashes(new));
    let mut out = Vec::new();
    for (k, v) in &b {
        match a.get(k) {
            None => out.push(format!("added {k}")),
            Some(o) if o != v => out.push(format!("changed {k}")),
            _ => {}
        }
    }
    for k in a.keys().filter(|k| !b.contains_key(*k)) {
        out.push(format!("removed {k}"));
    }
    out
}

fn preview_of(lib: &Library, url: &str, sha: &str) -> LinkPreview {
    let mut dirs = Vec::new();
    skill_dirs(&fetch_dir(sha), 4, &mut dirs);
    let have = lib.load().skills;
    let skills = dirs
        .iter()
        .filter_map(|d| library::read_skill(d, Origin::Url))
        .map(|f| {
            let bytes = f
                .files
                .iter()
                .filter_map(|r| std::fs::metadata(f.dir.join(r)).ok())
                .map(|m| m.len())
                .sum();
            let (status, diff) = match have.get(&f.name) {
                None => ("new", Vec::new()),
                Some(e) if e.hash == f.hash => ("same", Vec::new()),
                Some(_) => ("changed", diff_dirs(&lib.dir_of(&f.name), &f.dir)),
            };
            LinkSkill {
                name: f.name,
                description: f.description,
                files: f.files.len(),
                bytes,
                has_scripts: f.has_scripts,
                status: status.into(),
                diff,
            }
        })
        .collect();
    LinkPreview {
        url: url.to_owned(),
        commit: sha.to_owned(),
        skills,
    }
}

/// Downloads what a GitHub link points at (a repo, a folder or a SKILL.md)
/// at the commit it names now, and lists the skills in it. Nothing is
/// added and nothing runs.
#[tauri::command]
pub async fn library_fetch_url(app: AppHandle, url: String) -> CmdResult<LinkPreview> {
    let g =
        library::parse_github(&url).ok_or("Paste a GitHub link to a repo or a skill folder.")?;
    let lib = library_of(&app)?;
    let client = reqwest::Client::builder()
        .user_agent("duskie")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let api = format!("https://api.github.com/repos/{}/{}", g.owner, g.repo);
    let get = |u: String| {
        let client = client.clone();
        async move {
            let r = client.get(&u).send().await.map_err(|e| e.to_string())?;
            if !r.status().is_success() {
                return Err(format!("GitHub answered {}.", r.status()));
            }
            r.bytes().await.map_err(|e| e.to_string())
        }
    };
    let commit: serde_json::Value =
        serde_json::from_slice(&get(format!("{api}/commits/{}", g.reference)).await?)
            .map_err(|e| e.to_string())?;
    let sha = commit["sha"]
        .as_str()
        .ok_or("No such branch or commit.")?
        .to_owned();
    let tree: serde_json::Value =
        serde_json::from_slice(&get(format!("{api}/git/trees/{sha}?recursive=1")).await?)
            .map_err(|e| e.to_string())?;
    let prefix = if g.path.is_empty() {
        String::new()
    } else {
        format!("{}/", g.path)
    };
    let files: Vec<String> = tree["tree"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|t| t["type"] == "blob")
        .filter_map(|t| t["path"].as_str())
        .filter(|p| p.starts_with(&prefix) && !p.split('/').any(|s| s == ".." || s.is_empty()))
        .map(str::to_owned)
        .collect();
    if files.len() > library::MAX_FILES * 5 {
        return Err("That repo is too large to scan for skills; link a folder instead.".into());
    }
    let skill_roots: Vec<String> = files
        .iter()
        .filter_map(|f| f.strip_suffix("SKILL.md"))
        .map(str::to_owned)
        .collect();
    if skill_roots.is_empty() {
        return Err("No SKILL.md behind that link.".into());
    }
    let wanted: Vec<&String> = files
        .iter()
        .filter(|f| skill_roots.iter().any(|r| f.starts_with(r.as_str())))
        .collect();
    let tmp = fetch_dir(&sha);
    let _ = std::fs::remove_dir_all(&tmp);
    let mut total = 0u64;
    for f in wanted {
        let body = get(format!(
            "https://raw.githubusercontent.com/{}/{}/{sha}/{f}",
            g.owner, g.repo
        ))
        .await?;
        total += body.len() as u64;
        if total > library::MAX_BYTES * 5 {
            let _ = std::fs::remove_dir_all(&tmp);
            return Err("Those skills are too large.".into());
        }
        let dest = tmp.join(&f[prefix.len()..]);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(&dest, &body).map_err(|e| e.to_string())?;
    }
    off_ui(move || preview_of(&lib, &url, &sha)).await
}

/// Adds the chosen skills from a fetched link. Anything replaced is backed
/// up first.
#[tauri::command]
pub async fn library_install_fetched(
    app: AppHandle,
    url: String,
    commit: String,
    names: Vec<String>,
) -> CmdResult<Vec<Entry>> {
    if !commit.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err("Bad commit.".into());
    }
    let lib = library_of(&app)?;
    off_ui(move || {
        let mut dirs = Vec::new();
        skill_dirs(&fetch_dir(&commit), 4, &mut dirs);
        if dirs.is_empty() {
            return Err("Fetch the link again.".to_owned());
        }
        for f in dirs
            .iter()
            .filter_map(|d| library::read_skill(d, Origin::Url))
            .filter(|f| names.contains(&f.name))
        {
            lib.import(&f)
                .and_then(|_| lib.set_source(&f.name, &url, &commit))
                .map_err(|e| e.to_string())?;
        }
        let _ = std::fs::remove_dir_all(fetch_dir(&commit));
        Ok(entries(&lib))
    })
    .await?
}

#[cfg(test)]
mod link_tests {
    use super::*;

    #[test]
    fn diff_lists_added_changed_and_removed_files() {
        let base = std::env::temp_dir().join(format!("duskie-diff-{}", std::process::id()));
        let (a, b) = (base.join("a"), base.join("b"));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("SKILL.md"), "v1").unwrap();
        std::fs::write(a.join("old.txt"), "x").unwrap();
        std::fs::write(b.join("SKILL.md"), "v2").unwrap();
        std::fs::write(b.join("new.txt"), "y").unwrap();
        let mut d = diff_dirs(&a, &b);
        d.sort();
        assert_eq!(d, ["added new.txt", "changed SKILL.md", "removed old.txt"]);
    }

    #[test]
    fn finds_every_skill_folder() {
        let base = std::env::temp_dir().join(format!("duskie-dirs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["skills/pdf", "skills/xlsx", "other/deep/x/y"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
            std::fs::write(base.join(d).join("SKILL.md"), "---\nname: a\n---").unwrap();
        }
        let mut out = Vec::new();
        skill_dirs(&base, 4, &mut out);
        assert_eq!(out.len(), 3);
    }
}
