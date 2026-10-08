//! Clone from the clipboard: copying a GitHub, GitLab or Bitbucket repo
//! link offers to clone it into the folder it most likely belongs in, or,
//! when it is already cloned, to open or pull it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::json;
use sidekick_core::Event;
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

pub const OFFER: &str = "dev.clone_offer";
pub const CLONED: &str = "dev.cloned";

/// A repo named by a link.
#[derive(Debug, Clone, PartialEq)]
pub struct RepoUrl {
    pub host: String,
    pub owner: String,
    pub name: String,
    /// What `git clone` gets.
    pub url: String,
}

const HOSTS: [&str; 3] = ["github.com", "gitlab.com", "bitbucket.org"];

fn clean(part: &str) -> Option<String> {
    let p = part.trim_end_matches(".git");
    (!p.is_empty()
        && p.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && p != "."
        && p != "..")
        .then(|| p.to_owned())
}

/// A repo link (https or ssh) on a known host, or None.
pub fn parse(text: &str) -> Option<RepoUrl> {
    let t = text.trim();
    if t.contains(char::is_whitespace) {
        return None;
    }
    let (host, path, ssh) = if let Some(rest) = t.strip_prefix("git@") {
        let (h, p) = rest.split_once(':')?;
        (h, p, true)
    } else {
        let rest = t
            .strip_prefix("https://")
            .or_else(|| t.strip_prefix("http://"))?;
        let rest = rest.strip_prefix("www.").unwrap_or(rest);
        let (h, p) = rest.split_once('/')?;
        (h, p, false)
    };
    let host = host.to_ascii_lowercase();
    if !HOSTS.contains(&host.as_str()) {
        return None;
    }
    let mut parts = path.split(['?', '#']).next()?.split('/');
    let owner = clean(parts.next()?)?;
    let name = clean(parts.next()?)?;
    let url = if ssh {
        format!("git@{host}:{owner}/{name}.git")
    } else {
        format!("https://{host}/{owner}/{name}.git")
    };
    Some(RepoUrl {
        host,
        owner,
        name,
        url,
    })
}

/// `owner/name` of a repo's origin, lowercased, on any known host.
fn origin(repo: &Path) -> Option<(String, String)> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;
    let r = parse(String::from_utf8_lossy(&out.stdout).trim())?;
    Some((r.owner.to_lowercase(), r.name.to_lowercase()))
}

/// Whether a folder name stands for an owner ("Abdullah" for abdullahalhoothy).
pub fn name_matches(folder: &str, owner: &str) -> bool {
    let (f, o) = (folder.to_lowercase(), owner.to_lowercase());
    f == o || (f.len() >= 4 && o.starts_with(&f)) || (o.len() >= 4 && f.starts_with(&o))
}

/// Where a new clone should go, best first, and whether it is already here.
#[derive(Debug, Default, PartialEq)]
pub struct Plan {
    pub have: Option<PathBuf>,
    pub folders: Vec<PathBuf>,
}

/// Works out the plan from known repos (path and origin), the user's rules,
/// and folders to look through by name.
pub fn plan(
    r: &RepoUrl,
    repos: &[(PathBuf, Option<(String, String)>)],
    rules: &std::collections::BTreeMap<String, String>,
    candidates: &[PathBuf],
) -> Plan {
    let owner = r.owner.to_lowercase();
    let name = r.name.to_lowercase();
    let have = repos
        .iter()
        .find(|(_, o)| o.as_ref() == Some(&(owner.clone(), name.clone())))
        .map(|(p, _)| p.clone());
    let mut folders: Vec<PathBuf> = Vec::new();
    fn add(folders: &mut Vec<PathBuf>, p: PathBuf) {
        if !folders.contains(&p) {
            folders.push(p);
        }
    }
    // 1. A folder that already holds this owner's clones.
    let mut counts: HashMap<PathBuf, usize> = HashMap::new();
    for (p, o) in repos {
        if o.as_ref().is_some_and(|(ow, _)| *ow == owner)
            && let Some(parent) = p.parent()
        {
            *counts.entry(parent.to_owned()).or_default() += 1;
        }
    }
    let mut by_owner: Vec<_> = counts.into_iter().collect();
    by_owner.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    by_owner.into_iter().for_each(|(p, _)| add(&mut folders, p));
    // 2. A folder named after the owner.
    for c in candidates {
        if c.file_name()
            .is_some_and(|n| name_matches(&n.to_string_lossy(), &owner))
        {
            add(&mut folders, c.clone());
        }
    }
    // 3. A rule learned before.
    if let Some(dir) = rules
        .iter()
        .find(|(k, _)| k.to_lowercase() == owner)
        .map(|(_, v)| v)
    {
        add(&mut folders, PathBuf::from(dir));
    }
    // 4. A new owner: the main repos folder, or a new folder for the owner.
    if folders.is_empty() {
        let mut parents: HashMap<PathBuf, usize> = HashMap::new();
        for (p, _) in repos {
            if let Some(parent) = p.parent() {
                *parents.entry(parent.to_owned()).or_default() += 1;
            }
        }
        if let Some((main, _)) = parents
            .into_iter()
            .max_by(|a, b| a.1.cmp(&b.1).then(b.0.cmp(&a.0)))
        {
            add(&mut folders, main.clone());
            add(&mut folders, main.join(&r.owner));
        }
    }
    Plan { have, folders }
}

/// Folders worth matching by name: each code folder, what is in it, its
/// siblings, and on Windows the top of each drive.
fn candidates(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut look = |dir: &Path| {
        if let Ok(rd) = std::fs::read_dir(dir) {
            out.extend(rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()));
        }
    };
    for r in roots {
        look(r);
        if let Some(parent) = r.parent() {
            look(parent);
        }
    }
    #[cfg(windows)]
    for d in b'C'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", d as char));
        if root.is_dir() {
            look(&root);
        }
    }
    out
}

/// A copy happened: offer a clone when it is a repo link.
pub fn observe(app: &AppHandle, event: &Event) {
    if event.kind != "clipboard.changed" {
        return;
    }
    let Some(r) = event.payload["text"].as_str().and_then(parse) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let repos: Vec<(PathBuf, Option<(String, String)>)> = crate::projects::list(&app)
            .into_iter()
            .map(|p| {
                let o = origin(&p);
                (p, o)
            })
            .collect();
        let rules = lock(&app.state::<AppState>().settings).clone_rules.clone();
        let roots = crate::projects::roots(&app);
        let plan = plan(&r, &repos, &rules, &candidates(&roots));
        let show = |p: &Path| p.display().to_string();
        let payload = match &plan.have {
            Some(have) => json!({
                "slug": format!("{}/{}", r.owner, r.name),
                "name": r.name,
                "cloned": true,
                "path": show(have),
            }),
            None => {
                let dir = plan.folders.first().map(|p| show(p)).unwrap_or_default();
                if dir.is_empty() {
                    return;
                }
                json!({
                    "slug": format!("{}/{}", r.owner, r.name),
                    "name": r.name,
                    "owner": r.owner,
                    "url": r.url,
                    "cloned": false,
                    "dir": dir,
                    "dir2": plan.folders.get(1).map(|p| show(p)).unwrap_or_default(),
                    "has_alt": plan.folders.len() > 1,
                })
            }
        };
        app.state::<AppState>()
            .bus
            .publish(Event::new(OFFER, "clipboard", payload));
    });
}

/// After a clone: remember where this owner goes, and offer next steps.
pub fn after_clone(app: &AppHandle, url: &str, dir: &str, path: &str) {
    if let Some(r) = parse(url) {
        let state = app.state::<AppState>();
        let mut s = lock(&state.settings).clone();
        s.clone_rules.insert(r.owner.clone(), dir.to_owned());
        if let Err(e) = crate::commands::apply_settings(app, s) {
            log::warn!("could not keep the clone rule: {e}");
        }
    }
    let p = Path::new(path);
    let deps = sidekick_actions::dev::manager(p).is_some();
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    app.state::<AppState>().bus.publish(Event::new(
        CLONED,
        "clipboard",
        json!({ "name": name, "path": path, "deps_needed": deps }),
    ));
}

/// "Pick a folder": asks where, then clones there.
pub async fn pick_and_clone(
    app: &AppHandle,
    url: &str,
) -> Result<sidekick_actions::Outcome, String> {
    let picked = tauri::async_runtime::spawn_blocking(|| {
        rfd::FileDialog::new()
            .set_title("Clone into which folder?")
            .pick_folder()
    })
    .await
    .map_err(|e| e.to_string())?
    .ok_or("No folder picked")?;
    let (url2, dir2) = (url.to_owned(), picked.clone());
    let out =
        tauri::async_runtime::spawn_blocking(move || sidekick_actions::dev::clone(&url2, &dir2))
            .await
            .map_err(|e| e.to_string())?
            .map_err(|e| e.to_string())?;
    if let Some(path) = &out.path {
        after_clone(app, url, &picked.to_string_lossy(), path);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn reads_repo_links() {
        let r = parse("https://github.com/abdullahalhoothy/website_northernacs.com").unwrap();
        assert_eq!(
            (r.owner.as_str(), r.name.as_str()),
            ("abdullahalhoothy", "website_northernacs.com")
        );
        assert_eq!(
            r.url,
            "https://github.com/abdullahalhoothy/website_northernacs.com.git"
        );
        let s = parse("git@gitlab.com:team/app.git").unwrap();
        assert_eq!(s.url, "git@gitlab.com:team/app.git");
        assert_eq!(
            parse("https://github.com/a/b/tree/main/src").unwrap().name,
            "b"
        );
        assert!(parse("https://example.com/a/b").is_none());
        assert!(parse("https://github.com/a").is_none());
        assert!(parse("see https://github.com/a/b").is_none());
    }

    #[test]
    fn matches_owner_folders() {
        assert!(name_matches("Abdullah", "abdullahalhoothy"));
        assert!(!name_matches("abc", "abcdef"));
        assert!(!name_matches("code", "abdullah"));
    }

    #[test]
    fn plans_where_to_clone() {
        let r = parse("https://github.com/acme/new").unwrap();
        let repos = vec![
            (
                PathBuf::from("/d/Acme/one"),
                Some(("acme".into(), "one".into())),
            ),
            (PathBuf::from("/d/code/x"), Some(("me".into(), "x".into()))),
            (PathBuf::from("/d/code/y"), Some(("me".into(), "y".into()))),
        ];
        let p = plan(&r, &repos, &BTreeMap::new(), &[]);
        assert_eq!(p.folders[0], PathBuf::from("/d/Acme"));
        assert!(p.have.is_none());

        let have = plan(
            &parse("git@github.com:acme/one.git").unwrap(),
            &repos,
            &BTreeMap::new(),
            &[],
        );
        assert_eq!(have.have, Some(PathBuf::from("/d/Acme/one")));

        let fresh = plan(
            &parse("https://github.com/zed/q").unwrap(),
            &repos,
            &BTreeMap::new(),
            &[],
        );
        assert_eq!(
            fresh.folders,
            vec![PathBuf::from("/d/code"), PathBuf::from("/d/code/zed")]
        );

        let named = plan(
            &parse("https://github.com/abdullahalhoothy/w").unwrap(),
            &repos,
            &BTreeMap::new(),
            &[PathBuf::from("/d/Abdullah")],
        );
        assert_eq!(named.folders[0], PathBuf::from("/d/Abdullah"));
    }
}
