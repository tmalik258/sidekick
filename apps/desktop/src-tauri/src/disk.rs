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
    fn writes_sizes_for_people() {
        assert_eq!(human(3 * 1024 * 1024 * 1024 / 2), "1.5 GB");
        assert_eq!(human(200 * 1024 * 1024), "200 MB");
    }
}
