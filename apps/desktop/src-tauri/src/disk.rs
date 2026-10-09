//! "What is taking space?": free space on each drive, the size of the
//! user's big folders, the biggest files, and what in Downloads is safe to
//! clear. Reads only, within a time budget so the answer never waits long.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

/// The walk stops here and sizes become "at least".
const BUDGET: Duration = Duration::from_secs(6);
/// Files this big are listed by name.
const BIG_FILE: u64 = 300 * 1024 * 1024;
const OLD: Duration = Duration::from_secs(30 * 24 * 60 * 60);
/// Folders only the system or apps should clear.
const SKIP: &[&str] = &[
    "$recycle.bin",
    "windows",
    "program files",
    "program files (x86)",
];

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Usage {
    pub bytes: u64,
    /// The time budget ran out before every file was counted.
    pub partial: bool,
}

/// Total size of the files under `dir`, the biggest files met on the way.
pub fn folder_size(dir: &Path, deadline: Instant, big: &mut Vec<(u64, PathBuf)>) -> Usage {
    let mut usage = Usage::default();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        if Instant::now() > deadline {
            usage.partial = true;
            break;
        }
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.flatten() {
            // Links and junctions would count the same files twice.
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                let name = e.file_name().to_string_lossy().to_lowercase();
                if !SKIP.contains(&name.as_str()) {
                    stack.push(e.path());
                }
            } else if let Ok(m) = e.metadata() {
                usage.bytes += m.len();
                if m.len() >= BIG_FILE {
                    big.push((m.len(), e.path()));
                }
            }
        }
    }
    usage
}

/// "12.4 GB", "830 MB".
pub fn human(bytes: u64) -> String {
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.1} GB", b / GB)
    } else {
        format!("{:.0} MB", b / MB)
    }
}

/// Installers and files older than a month, directly in Downloads.
/// Files with their sizes, biggest first.
pub type FileSizes = Vec<(u64, PathBuf)>;

pub fn downloads_to_clear(dir: &Path, now: SystemTime) -> (FileSizes, FileSizes) {
    let mut installers = Vec::new();
    let mut old = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (installers, old);
    };
    for e in entries.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if !m.is_file() {
            continue;
        }
        let path = e.path();
        let ext = path
            .extension()
            .map(|x| x.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ["exe", "msi", "msix", "appx"].contains(&ext.as_str()) {
            installers.push((m.len(), path));
        } else if m
            .modified()
            .ok()
            .and_then(|t| now.duration_since(t).ok())
            .is_some_and(|age| age > OLD)
        {
            old.push((m.len(), path));
        }
    }
    installers.sort_by_key(|x| std::cmp::Reverse(x.0));
    old.sort_by_key(|x| std::cmp::Reverse(x.0));
    (installers, old)
}

fn drives() -> Vec<String> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks
        .list()
        .iter()
        .filter(|d| d.total_space() > 0)
        .map(|d| {
            format!(
                "{} {} free of {}",
                d.mount_point().display(),
                human(d.available_space()),
                human(d.total_space())
            )
        })
        .collect()
}

/// The report the model reads, with what it may offer to clear.
pub fn report() -> String {
    let Some(home) = dirs::home_dir() else {
        return "Error: no home folder.".into();
    };
    let deadline = Instant::now() + BUDGET;
    let mut big = Vec::new();
    let mut folders: Vec<(String, Usage)> = Vec::new();
    let mut places: Vec<PathBuf> = std::fs::read_dir(&home)
        .map(|r| {
            r.flatten()
                .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                .map(|e| e.path())
                .collect()
        })
        .unwrap_or_default();
    // Temp is where most clutter hides; it is under AppData, so add it on its own.
    if let Some(local) = dirs::data_local_dir() {
        places.push(local.join("Temp"));
    }
    for dir in places {
        let name = dir
            .strip_prefix(&home)
            .map(|p| p.display().to_string())
            .unwrap_or_else(|_| dir.display().to_string());
        let u = folder_size(&dir, deadline, &mut big);
        if u.bytes > 0 {
            folders.push((name, u));
        }
    }
    folders.sort_by_key(|x| std::cmp::Reverse(x.1.bytes));
    big.sort_by_key(|x| std::cmp::Reverse(x.0));

    let mut out = String::new();
    let d = drives();
    if !d.is_empty() {
        out.push_str(&format!("Drives: {}\n", d.join("; ")));
    }
    out.push_str("Biggest folders in the user's home:\n");
    for (name, u) in folders.iter().take(8) {
        let at_least = if u.partial { "at least " } else { "" };
        out.push_str(&format!("- {name}: {at_least}{}\n", human(u.bytes)));
    }
    if !big.is_empty() {
        out.push_str("Biggest files:\n");
        for (size, p) in big.iter().take(6) {
            out.push_str(&format!("- {} ({})\n", p.display(), human(*size)));
        }
    }
    if let Some(dl) = dirs::download_dir() {
        let (installers, old) = downloads_to_clear(&dl, SystemTime::now());
        let sum = |v: &[(u64, PathBuf)]| v.iter().map(|x| x.0).sum::<u64>();
        if !installers.is_empty() {
            out.push_str(&format!(
                "Installers in Downloads: {} files, {}:\n",
                installers.len(),
                human(sum(&installers))
            ));
            for (_, p) in installers.iter().take(8) {
                out.push_str(&format!("- {}\n", p.display()));
            }
        }
        if !old.is_empty() {
            out.push_str(&format!(
                "Files in Downloads older than a month: {} files, {}; biggest:\n",
                old.len(),
                human(sum(&old))
            ));
            for (size, p) in old.iter().take(5) {
                out.push_str(&format!("- {} ({})\n", p.display(), human(*size)));
            }
        }
    }
    out.push_str(
        "To free space, offer with propose: trash_download {path} for installers or old files \
         in Downloads (they go to the Recycle Bin), empty_recycle_bin {}, and open_system_page \
         {page: storage} for Storage Sense, which clears temporary files. Never offer to delete \
         anything else; say what else is big and let the user decide.",
    );
    out
}

/// One group in the disk view: what it is, how big, and its biggest parts.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Group {
    pub id: &'static str,
    pub label: &'static str,
    /// What it is, in plain words.
    pub what: &'static str,
    pub bytes: u64,
    /// The scan ran out of time, so the size is "at least".
    pub partial: bool,
    /// Its parts, biggest first (at most `SHOWN`).
    pub items: Vec<GroupItem>,
    /// Items here can go to the Recycle Bin after review.
    pub clearable: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct GroupItem {
    pub path: String,
    pub bytes: u64,
}

const SHOWN: usize = 40;
/// Build folders to look for in code projects.
const BUILD_DIRS: &[&str] = &["node_modules", "target", ".next", ".turbo", "__pycache__"];
/// How deep to look for projects under the home folder.
const PROJECT_DEPTH: usize = 5;

/// What can be cleared, from the last scan; cleanup only takes these.
static LAST: std::sync::Mutex<Vec<(&'static str, Vec<PathBuf>)>> =
    std::sync::Mutex::new(Vec::new());

/// Every group, biggest first. Reads only; takes up to about 15 seconds.
pub fn groups() -> Vec<Group> {
    let deadline = Instant::now() + Duration::from_secs(15);
    let home = dirs::home_dir().unwrap_or_default();
    let local = dirs::data_local_dir().unwrap_or_default();
    let mut out = Vec::new();

    if let Some(dl) = dirs::download_dir() {
        let (installers, old) = downloads_to_clear(&dl, SystemTime::now());
        out.push(files_group(
            "installers",
            "Old installers",
            "Setup files in Downloads, already installed or not needed.",
            installers,
            true,
        ));
        out.push(files_group(
            "downloads",
            "Old downloads",
            "Files in Downloads you have not touched for a month.",
            old,
            true,
        ));
    }

    // Build folders inside projects: rebuilt by the next install or build.
    let mut builds = Vec::new();
    find_build_dirs(&home, 0, deadline, &mut builds);
    let mut partial = Instant::now() > deadline;
    let mut sized = Vec::new();
    for dir in builds {
        let u = folder_size(&dir, deadline, &mut Vec::new());
        partial |= u.partial;
        sized.push((u.bytes, dir));
    }
    let mut g = files_group(
        "build",
        "Build folders",
        "node_modules, target and .next in your projects. The next install or build makes them again.",
        sized,
        true,
    );
    g.partial |= partial;
    out.push(g);

    let caches = [
        local.join("Temp"),
        local.join("npm-cache"),
        local.join("pip").join("cache"),
        local.join("Yarn").join("Cache"),
        local.join("pnpm-cache"),
        home.join(".cache"),
        home.join(".cargo").join("registry"),
        local.join("NuGet").join("v3-cache"),
    ];
    out.push(folders_group(
        "caches",
        "Caches",
        "Temporary files and package caches. Storage Sense clears the safe ones.",
        &caches,
        deadline,
        false,
    ));

    out.push(folders_group(
        "games",
        "Games",
        "Steam and Epic libraries. Uninstall a game from its launcher.",
        &game_dirs(),
        deadline,
        false,
    ));

    let mut vms: Vec<(u64, PathBuf)> = Vec::new();
    let packages = local.join("Packages");
    if let Ok(entries) = std::fs::read_dir(&packages) {
        for e in entries.flatten() {
            let disk = e.path().join("LocalState").join("ext4.vhdx");
            if let Ok(m) = disk.metadata() {
                vms.push((m.len(), disk));
            }
        }
    }
    for p in [
        local
            .join("Docker")
            .join("wsl")
            .join("disk")
            .join("docker_data.vhdx"),
        local
            .join("Docker")
            .join("wsl")
            .join("data")
            .join("ext4.vhdx"),
    ] {
        if let Ok(m) = p.metadata() {
            vms.push((m.len(), p));
        }
    }
    out.push(files_group(
        "vms",
        "WSL and Docker",
        "Linux disks grow and do not shrink on their own. Prune inside Docker or compact the disk.",
        vms,
        false,
    ));

    out.retain(|g| g.bytes > 0);
    out.sort_by_key(|g| std::cmp::Reverse(g.bytes));
    if let Ok(mut last) = LAST.lock() {
        *last = out
            .iter()
            .filter(|g| g.clearable)
            .map(|g| {
                (
                    g.id,
                    g.items.iter().map(|i| PathBuf::from(&i.path)).collect(),
                )
            })
            .collect();
    }
    out
}

fn files_group(
    id: &'static str,
    label: &'static str,
    what: &'static str,
    mut files: Vec<(u64, PathBuf)>,
    clearable: bool,
) -> Group {
    files.sort_by_key(|x| std::cmp::Reverse(x.0));
    Group {
        id,
        label,
        what,
        bytes: files.iter().map(|x| x.0).sum(),
        partial: false,
        items: files
            .into_iter()
            .take(SHOWN)
            .map(|(bytes, p)| GroupItem {
                path: p.display().to_string(),
                bytes,
            })
            .collect(),
        clearable,
    }
}

fn folders_group(
    id: &'static str,
    label: &'static str,
    what: &'static str,
    dirs: &[PathBuf],
    deadline: Instant,
    clearable: bool,
) -> Group {
    let mut partial = false;
    let sized = dirs
        .iter()
        .filter(|d| d.is_dir())
        .map(|d| {
            let u = folder_size(d, deadline, &mut Vec::new());
            partial |= u.partial;
            (u.bytes, d.clone())
        })
        .filter(|x| x.0 > 0)
        .collect();
    let mut g = files_group(id, label, what, sized, clearable);
    g.partial = partial;
    g
}

/// node_modules, target and the like under `dir`, not looking inside them.
fn find_build_dirs(dir: &Path, depth: usize, deadline: Instant, out: &mut Vec<PathBuf>) {
    if depth > PROJECT_DEPTH || Instant::now() > deadline {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_dir() || ft.is_symlink() {
            continue;
        }
        let name = e.file_name().to_string_lossy().to_lowercase();
        if BUILD_DIRS.contains(&name.as_str()) {
            // "target" counts only in a Rust project.
            if name != "target" || dir.join("Cargo.toml").is_file() {
                out.push(e.path());
            }
            continue;
        }
        // App data and hidden folders hold no projects worth this walk.
        if name.starts_with('.') || name == "appdata" || SKIP.contains(&name.as_str()) {
            continue;
        }
        find_build_dirs(&e.path(), depth + 1, deadline, out);
    }
}

/// Steam and Epic game folders found on this PC.
fn game_dirs() -> Vec<PathBuf> {
    let mut libs = vec![
        PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\common"),
        PathBuf::from(r"C:\Program Files\Epic Games"),
    ];
    // Other Steam libraries are listed in libraryfolders.vdf.
    let vdf = PathBuf::from(r"C:\Program Files (x86)\Steam\steamapps\libraryfolders.vdf");
    if let Ok(text) = std::fs::read_to_string(vdf) {
        for line in text.lines() {
            let line = line.trim();
            if let Some(rest) = line.strip_prefix("\"path\"") {
                let path = rest.trim().trim_matches('"').replace("\\\\", "\\");
                libs.push(PathBuf::from(path).join("steamapps").join("common"));
            }
        }
    }
    libs.sort();
    libs.dedup();
    libs.into_iter()
        .filter(|l| l.is_dir())
        .flat_map(|l| {
            std::fs::read_dir(l)
                .map(|r| {
                    r.flatten()
                        .map(|e| e.path())
                        .filter(|p| p.is_dir())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        })
        .collect()
}

/// Sends the chosen items of a clearable group to the Recycle Bin. Only
/// paths the last scan listed in that group are taken.
pub fn clean(group: &str, paths: &[String]) -> Result<(usize, u64), String> {
    let allowed: Vec<PathBuf> = LAST
        .lock()
        .map_err(|e| e.to_string())?
        .iter()
        .find(|(id, _)| *id == group)
        .map(|(_, p)| p.clone())
        .ok_or("Scan again first")?;
    let mut count = 0;
    let mut freed = 0;
    for p in paths {
        let path = PathBuf::from(p);
        if !allowed.contains(&path) {
            return Err(format!("{p} was not in the list"));
        }
        let size = if path.is_dir() {
            folder_size(&path, Instant::now() + BUDGET, &mut Vec::new()).bytes
        } else {
            path.metadata().map(|m| m.len()).unwrap_or(0)
        };
        if trash::delete(&path).is_ok() {
            count += 1;
            freed += size;
        }
    }
    Ok((count, freed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_a_folder_and_lists_big_files() {
        let dir = std::env::temp_dir().join(format!("sidekick-disk-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a.txt"), vec![0u8; 1000]).unwrap();
        std::fs::write(dir.join("sub").join("b.txt"), vec![0u8; 500]).unwrap();
        let mut big = Vec::new();
        let u = folder_size(&dir, Instant::now() + BUDGET, &mut big);
        assert_eq!(
            u,
            Usage {
                bytes: 1500,
                partial: false
            }
        );
        assert!(big.is_empty());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finds_installers_in_downloads() {
        let dir = std::env::temp_dir().join(format!("sidekick-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("setup.exe"), b"x").unwrap();
        std::fs::write(dir.join("notes.txt"), b"x").unwrap();
        let later = SystemTime::now() + OLD + Duration::from_secs(60);
        let (installers, old) = downloads_to_clear(&dir, later);
        assert_eq!(installers.len(), 1);
        assert_eq!(old.len(), 1, "notes.txt is a month old by then");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn finds_build_folders_in_projects_only() {
        let dir = std::env::temp_dir().join(format!("sidekick-build-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("web").join("node_modules").join("x")).unwrap();
        std::fs::create_dir_all(dir.join("rs").join("target")).unwrap();
        std::fs::write(dir.join("rs").join("Cargo.toml"), "").unwrap();
        std::fs::create_dir_all(dir.join("photos").join("target")).unwrap();
        let mut found = Vec::new();
        find_build_dirs(&dir, 0, Instant::now() + BUDGET, &mut found);
        found.sort();
        assert_eq!(
            found,
            vec![
                dir.join("rs").join("target"),
                dir.join("web").join("node_modules")
            ]
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn cleans_only_what_the_scan_listed() {
        assert!(clean("build", &["C:\\Windows".into()]).is_err());
    }

    #[test]
    fn writes_sizes_for_people() {
        assert_eq!(human(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(human(200 * 1024 * 1024), "200 MB");
    }
}
