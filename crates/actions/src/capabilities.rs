//! What is installed: browsers and conversion tools.

use std::path::PathBuf;

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Browser {
    pub id: String,
    pub path: PathBuf,
}

impl Browser {
    pub fn label(&self) -> &str {
        match self.id.as_str() {
            "chrome" => "Chrome",
            "edge" => "Edge",
            "firefox" => "Firefox",
            "zen" => "Zen",
            "brave" => "Brave",
            other => other,
        }
    }

    pub fn private_flag(&self) -> &'static str {
        match self.id.as_str() {
            "edge" => "--inprivate",
            "firefox" | "zen" => "--private-window",
            _ => "--incognito",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Capabilities {
    pub browsers: Vec<Browser>,
    pub ffmpeg: Option<PathBuf>,
    pub magick: Option<PathBuf>,
    pub soffice: Option<PathBuf>,
    pub pandoc: Option<PathBuf>,
    pub tar: Option<PathBuf>,
    /// The code editor for opening projects: VS Code or a fork of it
    /// (Cursor, Windsurf), whichever was used last.
    pub code: Option<PathBuf>,
    /// Its name, e.g. "Cursor".
    pub code_name: Option<String>,
    /// 1Password CLI.
    pub op: Option<PathBuf>,
    /// Bitwarden CLI.
    pub bw: Option<PathBuf>,
    /// Tesseract, for text in screenshots.
    pub tesseract: Option<PathBuf>,
    /// Poppler's pdftotext, for summarizing PDFs.
    pub pdftotext: Option<PathBuf>,
}

impl Capabilities {
    /// Looks for browsers and tools. Takes a few milliseconds; call it at
    /// start and when the user asks to rescan.
    pub fn detect() -> Self {
        let browsers = ["chrome", "edge", "firefox", "zen", "brave"]
            .into_iter()
            .filter_map(|id| {
                find_browser(id).map(|path| Browser {
                    id: id.to_string(),
                    path,
                })
            })
            .collect();
        let editor = find_editor();
        Self {
            browsers,
            ffmpeg: which::which("ffmpeg").ok(),
            magick: which::which("magick").ok(),
            soffice: which::which("soffice")
                .ok()
                .or_else(|| first_existing(&office_paths())),
            pandoc: which::which("pandoc").ok(),
            tar: which::which("tar").ok(),
            code: editor.as_ref().map(|e| e.1.clone()),
            code_name: editor.map(|e| e.0.to_owned()),
            op: which::which("op").ok(),
            bw: which::which("bw").ok(),
            tesseract: which::which("tesseract").ok().or_else(find_tesseract),
            pdftotext: which::which("pdftotext").ok(),
        }
    }

    pub fn browser(&self, id: &str) -> Option<&Browser> {
        self.browsers.iter().find(|b| b.id == id)
    }

    /// Answers a skill's `requires` entry.
    pub fn has(&self, requirement: &str) -> bool {
        match requirement.split_once(':') {
            Some(("browser", id)) => self.browser(id).is_some(),
            Some(("tool", "ffmpeg")) => self.ffmpeg.is_some(),
            Some(("tool", "magick")) => self.magick.is_some(),
            Some(("tool", "image")) => self.magick.is_some() || self.ffmpeg.is_some(),
            Some(("tool", "soffice")) => self.soffice.is_some(),
            Some(("tool", "pandoc")) => self.pandoc.is_some(),
            Some(("tool", "tar")) => self.tar.is_some(),
            Some(("tool", "code")) => self.code.is_some(),
            Some(("tool", "op")) => self.op.is_some(),
            Some(("tool", "bw")) => self.bw.is_some(),
            Some(("tool", "tesseract")) => self.tesseract.is_some(),
            _ => false,
        }
    }

    /// Short list for the settings screen.
    pub fn summary(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .browsers
            .iter()
            .map(|b| b.label().to_string())
            .collect();
        for (name, found) in [
            ("ffmpeg", self.ffmpeg.is_some()),
            ("ImageMagick", self.magick.is_some()),
            ("LibreOffice", self.soffice.is_some()),
            ("pandoc", self.pandoc.is_some()),
            ("tar", self.tar.is_some()),
            (
                self.code_name.as_deref().unwrap_or("Code editor"),
                self.code.is_some(),
            ),
            ("1Password CLI", self.op.is_some()),
            ("Bitwarden CLI", self.bw.is_some()),
            ("Tesseract OCR", self.tesseract.is_some()),
        ] {
            if found {
                out.push(name.to_string());
            }
        }
        out
    }
}

/// The UB Mannheim installer does not add Tesseract to PATH.
#[cfg(windows)]
fn find_tesseract() -> Option<PathBuf> {
    env_path("ProgramFiles", r"Tesseract-OCR\tesseract.exe").filter(|p| p.is_file())
}

#[cfg(not(windows))]
fn find_tesseract() -> Option<PathBuf> {
    None
}

/// VS Code and its forks: name, install paths under LOCALAPPDATA or
/// ProgramFiles, command on PATH, and settings folder (for when it was last
/// used). They all open a folder or file passed as the argument.
const EDITORS: &[(&str, &[&str], &str, &str)] = &[
    (
        "Cursor",
        &[r"Programs\cursor\Cursor.exe"],
        "cursor",
        "Cursor",
    ),
    (
        "VS Code",
        &[
            r"Programs\Microsoft VS Code\Code.exe",
            r"Microsoft VS Code\Code.exe",
        ],
        "code",
        "Code",
    ),
    (
        "Windsurf",
        &[r"Programs\Windsurf\Windsurf.exe"],
        "windsurf",
        "Windsurf",
    ),
    (
        "VS Code Insiders",
        &[r"Programs\Microsoft VS Code Insiders\Code - Insiders.exe"],
        "code-insiders",
        "Code - Insiders",
    ),
];

/// The installed editor used most recently (its state file changes as it
/// is used); with no history, the first one in [`EDITORS`].
fn find_editor() -> Option<(&'static str, PathBuf)> {
    let installed = EDITORS.iter().filter_map(|(name, installs, cli, config)| {
        let exe = editor_exe(installs).or_else(|| which::which(cli).ok())?;
        let used = dirs::config_dir()
            .map(|c| {
                c.join(config)
                    .join("User")
                    .join("globalStorage")
                    .join("storage.json")
            })
            .and_then(|p| std::fs::metadata(p).ok())
            .and_then(|m| m.modified().ok());
        Some((*name, exe, used))
    });
    pick_editor(installed.collect())
}

/// Most recently used first; ties keep the list order.
fn pick_editor(
    found: Vec<(&'static str, PathBuf, Option<std::time::SystemTime>)>,
) -> Option<(&'static str, PathBuf)> {
    let mut best: Option<(&'static str, PathBuf, Option<std::time::SystemTime>)> = None;
    for f in found {
        if best.as_ref().is_none_or(|b| f.2 > b.2) {
            best = Some(f);
        }
    }
    best.map(|(name, exe, _)| (name, exe))
}

/// Prefers the editor's own .exe over the .cmd wrapper on PATH.
#[cfg(windows)]
fn editor_exe(installs: &[&str]) -> Option<PathBuf> {
    installs
        .iter()
        .flat_map(|rest| {
            [
                env_path("LOCALAPPDATA", rest),
                env_path("ProgramFiles", rest),
            ]
        })
        .flatten()
        .find(|p| p.is_file())
}

#[cfg(not(windows))]
fn editor_exe(_installs: &[&str]) -> Option<PathBuf> {
    None
}

fn first_existing(paths: &[PathBuf]) -> Option<PathBuf> {
    paths.iter().find(|p| p.is_file()).cloned()
}

#[cfg(windows)]
fn env_path(var: &str, rest: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(|base| PathBuf::from(base).join(rest))
}

#[cfg(windows)]
fn find_browser(id: &str) -> Option<PathBuf> {
    let candidates: Vec<(&str, &str)> = match id {
        "chrome" => vec![
            ("ProgramFiles", r"Google\Chrome\Application\chrome.exe"),
            ("ProgramFiles(x86)", r"Google\Chrome\Application\chrome.exe"),
            ("LOCALAPPDATA", r"Google\Chrome\Application\chrome.exe"),
        ],
        "edge" => vec![
            (
                "ProgramFiles(x86)",
                r"Microsoft\Edge\Application\msedge.exe",
            ),
            ("ProgramFiles", r"Microsoft\Edge\Application\msedge.exe"),
        ],
        "firefox" => vec![
            ("ProgramFiles", r"Mozilla Firefox\firefox.exe"),
            ("ProgramFiles(x86)", r"Mozilla Firefox\firefox.exe"),
        ],
        "zen" => vec![
            ("ProgramFiles", r"Zen Browser\zen.exe"),
            ("LOCALAPPDATA", r"Zen Browser\zen.exe"),
            ("LOCALAPPDATA", r"Programs\Zen Browser\zen.exe"),
        ],
        "brave" => vec![
            (
                "ProgramFiles",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
            ),
            (
                "LOCALAPPDATA",
                r"BraveSoftware\Brave-Browser\Application\brave.exe",
            ),
        ],
        _ => vec![],
    };
    let paths: Vec<PathBuf> = candidates
        .into_iter()
        .filter_map(|(v, rest)| env_path(v, rest))
        .collect();
    first_existing(&paths)
}

#[cfg(windows)]
fn office_paths() -> Vec<PathBuf> {
    [
        ("ProgramFiles", r"LibreOffice\program\soffice.exe"),
        ("ProgramFiles(x86)", r"LibreOffice\program\soffice.exe"),
    ]
    .into_iter()
    .filter_map(|(v, rest)| env_path(v, rest))
    .collect()
}

#[cfg(target_os = "macos")]
fn find_browser(id: &str) -> Option<PathBuf> {
    let app = match id {
        "chrome" => "Google Chrome.app/Contents/MacOS/Google Chrome",
        "edge" => "Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "firefox" => "Firefox.app/Contents/MacOS/firefox",
        "zen" => "Zen.app/Contents/MacOS/zen",
        "brave" => "Brave Browser.app/Contents/MacOS/Brave Browser",
        _ => return None,
    };
    first_existing(&[PathBuf::from("/Applications").join(app)])
}

#[cfg(target_os = "macos")]
fn office_paths() -> Vec<PathBuf> {
    vec![PathBuf::from(
        "/Applications/LibreOffice.app/Contents/MacOS/soffice",
    )]
}

#[cfg(all(unix, not(target_os = "macos")))]
fn find_browser(id: &str) -> Option<PathBuf> {
    let names: &[&str] = match id {
        "chrome" => &[
            "google-chrome",
            "google-chrome-stable",
            "chromium",
            "chromium-browser",
        ],
        "edge" => &["microsoft-edge", "microsoft-edge-stable"],
        "firefox" => &["firefox"],
        "zen" => &["zen", "zen-browser"],
        "brave" => &["brave-browser", "brave"],
        _ => &[],
    };
    names.iter().find_map(|n| which::which(n).ok())
}

#[cfg(all(unix, not(target_os = "macos")))]
fn office_paths() -> Vec<PathBuf> {
    vec![PathBuf::from("/usr/bin/libreoffice")]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_editor_used_last() {
        use std::time::{Duration, SystemTime};
        let t = |s| Some(SystemTime::UNIX_EPOCH + Duration::from_secs(s));
        let found = vec![
            ("Cursor", PathBuf::from("cursor"), t(100)),
            ("VS Code", PathBuf::from("code"), t(200)),
        ];
        assert_eq!(pick_editor(found).unwrap().0, "VS Code");
        let unused = vec![
            ("Cursor", PathBuf::from("cursor"), None),
            ("VS Code", PathBuf::from("code"), None),
        ];
        assert_eq!(pick_editor(unused).unwrap().0, "Cursor");
        assert!(pick_editor(Vec::new()).is_none());
    }

    #[test]
    fn answers_requirements() {
        let caps = Capabilities {
            browsers: vec![Browser {
                id: "zen".into(),
                path: "/x/zen".into(),
            }],
            ffmpeg: Some("/x/ffmpeg".into()),
            ..Capabilities::default()
        };
        assert!(caps.has("browser:zen"));
        assert!(!caps.has("browser:chrome"));
        assert!(caps.has("tool:image"));
        assert!(!caps.has("tool:magick"));
        assert!(!caps.has("nonsense"));
        assert_eq!(
            caps.browser("zen").unwrap().private_flag(),
            "--private-window"
        );
    }
}
