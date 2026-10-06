//! Code editors on this PC: VS Code and its forks (Cursor, Windsurf,
//! Antigravity, VSCodium), Zed, JetBrains IDEs and Visual Studio. Each opens
//! a folder or file passed as its argument.

use std::path::PathBuf;
use std::time::SystemTime;

use serde::Serialize;

/// One installed editor.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Editor {
    pub id: String,
    pub name: String,
    pub exe: PathBuf,
    /// When it last saved its state (VS Code family only): a hint of use.
    #[serde(skip)]
    pub used: Option<SystemTime>,
}

impl Editor {
    /// True when `app`, a window's app name from time tracking, is this
    /// editor ("Cursor", "Visual Studio Code", "PyCharm").
    pub fn is_app(&self, app: &str) -> bool {
        let app = app.trim().to_lowercase();
        let name = self.name.to_lowercase();
        if app == name {
            return true;
        }
        match self.id.as_str() {
            "vscode" => app == "visual studio code" || app == "code",
            "vscode-insiders" => app.contains("insiders"),
            "visual-studio" => app.starts_with("microsoft visual studio") || app == "devenv",
            _ => !name.is_empty() && app.starts_with(&name),
        }
    }
}

/// VS Code and its forks: id, name, install paths under LOCALAPPDATA or
/// ProgramFiles, command on PATH, and settings folder (for when it was last
/// used).
const CODE_FAMILY: &[(&str, &str, &[&str], &str, &str)] = &[
    (
        "cursor",
        "Cursor",
        &[r"Programs\cursor\Cursor.exe"],
        "cursor",
        "Cursor",
    ),
    (
        "vscode",
        "VS Code",
        &[
            r"Programs\Microsoft VS Code\Code.exe",
            r"Microsoft VS Code\Code.exe",
        ],
        "code",
        "Code",
    ),
    (
        "antigravity",
        "Antigravity",
        &[r"Programs\Antigravity\Antigravity.exe"],
        "antigravity",
        "Antigravity",
    ),
    (
        "windsurf",
        "Windsurf",
        &[r"Programs\Windsurf\Windsurf.exe"],
        "windsurf",
        "Windsurf",
    ),
    (
        "vscode-insiders",
        "VS Code Insiders",
        &[r"Programs\Microsoft VS Code Insiders\Code - Insiders.exe"],
        "code-insiders",
        "Code - Insiders",
    ),
    (
        "vscodium",
        "VSCodium",
        &[r"Programs\VSCodium\VSCodium.exe", r"VSCodium\VSCodium.exe"],
        "codium",
        "VSCodium",
    ),
];

/// JetBrains IDEs: id, name, launcher in `bin`.
const JETBRAINS: &[(&str, &str, &str)] = &[
    ("pycharm", "PyCharm", "pycharm64.exe"),
    ("intellij", "IntelliJ IDEA", "idea64.exe"),
    ("webstorm", "WebStorm", "webstorm64.exe"),
    ("rider", "Rider", "rider64.exe"),
    ("goland", "GoLand", "goland64.exe"),
    ("clion", "CLion", "clion64.exe"),
    ("phpstorm", "PhpStorm", "phpstorm64.exe"),
    ("rustrover", "RustRover", "rustrover64.exe"),
];

/// Every editor installed, the most recently used first (by its state file,
/// where it has one).
pub fn detect() -> Vec<Editor> {
    let mut found: Vec<Editor> = CODE_FAMILY
        .iter()
        .filter_map(|(id, name, installs, cli, config)| {
            let exe = installed(installs).or_else(|| which::which(cli).ok())?;
            let used = dirs::config_dir()
                .map(|c| {
                    c.join(config)
                        .join("User")
                        .join("globalStorage")
                        .join("storage.json")
                })
                .and_then(|p| std::fs::metadata(p).ok())
                .and_then(|m| m.modified().ok());
            Some(Editor {
                id: (*id).into(),
                name: (*name).into(),
                exe,
                used,
            })
        })
        .collect();
    found.extend(zed());
    found.extend(jetbrains());
    found.extend(visual_studio());
    sort_by_use(&mut found);
    found
}

/// Most recently used first; ties keep the list order.
pub fn sort_by_use(editors: &mut [Editor]) {
    editors.sort_by_key(|e| std::cmp::Reverse(e.used));
}

/// Picks the editor to open projects in: the chosen one when it is still
/// installed, else (Auto) the one used most this week, else the most
/// recent. `minutes` is time per app name from time tracking.
pub fn choose<'a>(
    editors: &'a [Editor],
    choice: &str,
    minutes: &[(String, i64)],
) -> Option<&'a Editor> {
    if let Some(e) = editors.iter().find(|e| e.id == choice) {
        return Some(e);
    }
    let time = |e: &Editor| -> i64 {
        minutes
            .iter()
            .filter(|(app, _)| e.is_app(app))
            .map(|(_, m)| m)
            .sum()
    };
    let most = editors
        .iter()
        .max_by_key(|e| time(e))
        .filter(|e| time(e) > 0);
    most.or_else(|| editors.first())
}

#[cfg(windows)]
fn installed(installs: &[&str]) -> Option<PathBuf> {
    installs
        .iter()
        .flat_map(|rest| ["LOCALAPPDATA", "ProgramFiles"].map(|var| env_path(var, rest)))
        .flatten()
        .find(|p| p.is_file())
}

#[cfg(not(windows))]
fn installed(_installs: &[&str]) -> Option<PathBuf> {
    None
}

fn env_path(var: &str, rest: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|base| PathBuf::from(base).join(rest))
}

#[cfg(windows)]
fn zed() -> Option<Editor> {
    let exe = installed(&[r"Programs\Zed\Zed.exe", r"Zed\Zed.exe"])
        .or_else(|| which::which("zed").ok())?;
    Some(Editor {
        id: "zed".into(),
        name: "Zed".into(),
        exe,
        used: None,
    })
}

#[cfg(not(windows))]
fn zed() -> Option<Editor> {
    None
}

/// JetBrains IDEs live in a folder per version (`PyCharm 2025.2`), under
/// Program Files\JetBrains or, from the Toolbox, LOCALAPPDATA\Programs.
fn jetbrains() -> Vec<Editor> {
    let roots: Vec<PathBuf> = [
        env_path("ProgramFiles", "JetBrains"),
        env_path("LOCALAPPDATA", "Programs"),
    ]
    .into_iter()
    .flatten()
    .collect();
    jetbrains_in(&roots)
}

fn jetbrains_in(roots: &[PathBuf]) -> Vec<Editor> {
    let mut out: Vec<(Editor, String)> = Vec::new();
    for root in roots {
        let Ok(dirs) = std::fs::read_dir(root) else {
            continue;
        };
        for dir in dirs.flatten() {
            let folder = dir.file_name().to_string_lossy().into_owned();
            for (id, name, launcher) in JETBRAINS {
                let exe = dir.path().join("bin").join(launcher);
                if !exe.is_file() {
                    continue;
                }
                // Several versions: keep the newest (highest folder name).
                match out.iter_mut().find(|(e, _)| e.id == *id) {
                    Some((e, seen)) if folder > *seen => {
                        e.exe = exe;
                        *seen = folder.clone();
                    }
                    Some(_) => {}
                    None => out.push((
                        Editor {
                            id: (*id).into(),
                            name: (*name).into(),
                            exe,
                            used: None,
                        },
                        folder.clone(),
                    )),
                }
            }
        }
    }
    out.into_iter().map(|(e, _)| e).collect()
}

/// Visual Studio, found through its installer's vswhere.
#[cfg(windows)]
fn visual_studio() -> Option<Editor> {
    let vswhere = env_path(
        "ProgramFiles(x86)",
        r"Microsoft Visual Studio\Installer\vswhere.exe",
    )
    .filter(|p| p.is_file())?;
    let mut cmd = std::process::Command::new(vswhere);
    cmd.args(["-latest", "-property", "productPath"]);
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    let out = cmd.output().ok()?;
    let exe = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
    is_devenv(&exe).then(|| Editor {
        id: "visual-studio".into(),
        name: "Visual Studio".into(),
        exe,
        used: None,
    })
}

#[cfg(not(windows))]
fn visual_studio() -> Option<Editor> {
    None
}

#[cfg(windows)]
fn is_devenv(p: &std::path::Path) -> bool {
    p.is_file()
        && p.file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("devenv.exe"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn ed(id: &str, name: &str, used: Option<u64>) -> Editor {
        Editor {
            id: id.into(),
            name: name.into(),
            exe: PathBuf::from(id),
            used: used.map(|s| SystemTime::UNIX_EPOCH + Duration::from_secs(s)),
        }
    }

    #[test]
    fn sorts_by_last_use_and_keeps_order_on_ties() {
        let mut list = vec![
            ed("cursor", "Cursor", Some(100)),
            ed("vscode", "VS Code", Some(200)),
            ed("zed", "Zed", None),
        ];
        sort_by_use(&mut list);
        let ids: Vec<&str> = list.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["vscode", "cursor", "zed"]);
    }

    #[test]
    fn auto_picks_the_editor_used_most_this_week() {
        let list = vec![
            ed("vscode", "VS Code", Some(200)),
            ed("cursor", "Cursor", Some(100)),
            ed("pycharm", "PyCharm", None),
        ];
        let week = vec![
            ("Visual Studio Code".to_owned(), 40),
            ("Cursor".to_owned(), 840),
            ("PyCharm Professional".to_owned(), 30),
        ];
        assert_eq!(choose(&list, "auto", &week).unwrap().id, "cursor");
        assert_eq!(
            choose(&list, "auto", &[]).unwrap().id,
            "vscode",
            "no time yet: the most recent"
        );
        assert_eq!(choose(&list, "pycharm", &week).unwrap().id, "pycharm");
        assert_eq!(
            choose(&list, "zed", &week).unwrap().id,
            "cursor",
            "a removed choice falls back to Auto"
        );
        assert!(choose(&[], "auto", &week).is_none());
    }

    #[test]
    fn tells_editor_windows_apart() {
        let code = ed("vscode", "VS Code", None);
        assert!(code.is_app("Visual Studio Code"));
        assert!(!code.is_app("Microsoft Visual Studio 2022"));
        let vs = ed("visual-studio", "Visual Studio", None);
        assert!(vs.is_app("Microsoft Visual Studio 2022"));
        assert!(ed("pycharm", "PyCharm", None).is_app("PyCharm Community Edition"));
    }

    #[test]
    fn finds_the_newest_jetbrains_ide() {
        let root = std::env::temp_dir().join(format!("sidekick-jb-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for v in ["PyCharm 2024.3", "PyCharm 2025.2"] {
            let bin = root.join(v).join("bin");
            std::fs::create_dir_all(&bin).unwrap();
            std::fs::write(bin.join("pycharm64.exe"), "").unwrap();
        }
        std::fs::create_dir_all(root.join("Other")).unwrap();
        let found = jetbrains_in(std::slice::from_ref(&root));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "PyCharm");
        assert!(found[0].exe.to_string_lossy().contains("2025.2"));
        let _ = std::fs::remove_dir_all(root);
    }
}
