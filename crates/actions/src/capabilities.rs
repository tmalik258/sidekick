//! What is installed: browsers and conversion tools.

use std::path::PathBuf;

use serde::Serialize;

use crate::editors::Editor;

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
            "samsung" => "Samsung Internet",
            other => other,
        }
    }

    /// Chromium browsers show their profile picker when started without a
    /// profile, and the link is lost. Open in the profile used last instead.
    pub fn profile_arg(&self) -> Option<String> {
        let data = user_data_dir(&self.id)?;
        last_profile(&data).map(|p| format!("--profile-directory={p}"))
    }

    pub fn private_flag(&self) -> &'static str {
        match self.id.as_str() {
            "edge" => "--inprivate",
            "firefox" | "zen" => "--private-window",
            _ => "--incognito",
        }
    }
}

/// Browser id from a Start-menu app name / AppID (Ask adds a private row per browser).
pub fn browser_for_app(name: &str, app_id: &str) -> Option<&'static str> {
    let hay = format!("{name} {app_id}").to_ascii_lowercase();
    if hay.contains("chrome remote") || hay.contains("chromedriver") {
        return None;
    }
    if hay.contains("msedge") || hay.contains("microsoft edge") {
        return Some("edge");
    }
    if hay.contains("brave") {
        return Some("brave");
    }
    if hay.contains("firefox") {
        return Some("firefox");
    }
    if name.eq_ignore_ascii_case("zen")
        || hay.contains("zen browser")
        || hay.contains("\\zen.exe")
        || hay.contains("/zen.exe")
    {
        return Some("zen");
    }
    if hay.contains("samsung") && hay.contains("internet") {
        return Some("samsung");
    }
    if hay.contains("chrome") || hay.contains("chromium") {
        return Some("chrome");
    }
    None
}

/// Which browser opens web links, from its ProgId (`ChromeHTML`,
/// `MSEdgeHTM`, `BraveHTML`, `FirefoxURL-...`).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn browser_for_prog_id(prog_id: &str) -> Option<&'static str> {
    let p = prog_id.to_ascii_lowercase();
    [
        ("chrome", "chrome"),
        ("msedge", "edge"),
        ("brave", "brave"),
        ("firefox", "firefox"),
        ("zen", "zen"),
        ("samsung", "samsung"),
    ]
    .iter()
    .find(|(key, _)| p.starts_with(key))
    .map(|(_, id)| *id)
}

/// Which browser a link handler's command line runs, from the program's
/// file name (`"C:\\...\\zen.exe" -osint -url "%1"`).
#[cfg_attr(not(windows), allow(dead_code))]
pub fn browser_for_command(command: &str) -> Option<&'static str> {
    let lower = command.to_ascii_lowercase();
    let exe = lower.split(".exe").next()?;
    let name = exe.rsplit(['\\', '/']).next()?.trim_matches(['"', ' ']);
    [
        ("chrome", "chrome"),
        ("msedge", "edge"),
        ("brave", "brave"),
        ("firefox", "firefox"),
        ("zen", "zen"),
        ("samsung", "samsung"),
    ]
    .iter()
    .find(|(key, _)| name.starts_with(key))
    .map(|(_, id)| *id)
}

/// The default browser's id on Windows, read from the https link handler.
pub fn default_browser() -> Option<&'static str> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let out = std::process::Command::new("reg")
            .args([
                "query",
                r"HKCU\Software\Microsoft\Windows\Shell\Associations\UrlAssociations\https\UserChoice",
                "/v",
                "ProgId",
            ])
            .creation_flags(0x0800_0000)
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let prog_id = text
            .lines()
            .find(|l| l.contains("ProgId"))?
            .split_whitespace()
            .last()?
            .to_owned();
        // The handler's program is the truth: Zen registers as
        // "FirefoxURL-..." but runs zen.exe.
        let cmd = std::process::Command::new("reg")
            .args([
                "query",
                &format!(r"HKCR\{prog_id}\shell\open\command"),
                "/ve",
            ])
            .creation_flags(0x0800_0000)
            .output()
            .ok();
        cmd.and_then(|o| browser_for_command(&String::from_utf8_lossy(&o.stdout)))
            .or_else(|| browser_for_prog_id(&prog_id))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Chromium user-data root for an installed browser id, if the folder exists.
pub fn user_data_dir(id: &str) -> Option<PathBuf> {
    let base = dirs::data_local_dir()?;
    let candidates: &[&str] = match id {
        "chrome" => &["Google/Chrome/User Data"],
        "edge" => &["Microsoft/Edge/User Data"],
        "brave" => &["BraveSoftware/Brave-Browser/User Data"],
        "samsung" => &[
            "SamsungInternet/User Data",
            "Samsung/SamsungInternet/User Data",
            "Samsung/Internet/User Data",
        ],
        _ => return None,
    };
    candidates.iter().map(|p| base.join(p)).find(|p| p.is_dir())
}

/// The profile folder a Chromium browser used last, from its `Local State`
/// file, if that folder still exists.
fn last_profile(user_data: &std::path::Path) -> Option<String> {
    let text = std::fs::read_to_string(user_data.join("Local State")).ok()?;
    let state: serde_json::Value = serde_json::from_str(&text).ok()?;
    let last = state["profile"]["last_used"]
        .as_str()
        .filter(|p| !p.is_empty())
        .unwrap_or("Default");
    user_data.join(last).is_dir().then(|| last.to_owned())
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Capabilities {
    pub browsers: Vec<Browser>,
    pub ffmpeg: Option<PathBuf>,
    pub magick: Option<PathBuf>,
    pub soffice: Option<PathBuf>,
    pub pandoc: Option<PathBuf>,
    pub tar: Option<PathBuf>,
    /// Every code editor found, the most recently used first.
    pub editors: Vec<Editor>,
    /// The editor projects open in (see [`Capabilities::use_editor`]).
    pub code: Option<PathBuf>,
    /// Its name, e.g. "Cursor".
    pub code_name: Option<String>,
    /// Tesseract, for text in screenshots.
    pub tesseract: Option<PathBuf>,
    /// Poppler's pdftotext, for summarizing PDFs.
    pub pdftotext: Option<PathBuf>,
}

impl Capabilities {
    /// Looks for browsers and tools. Takes a few milliseconds; call it at
    /// start and when the user asks to rescan.
    pub fn detect() -> Self {
        let browsers = ["chrome", "edge", "firefox", "zen", "brave", "samsung"]
            .into_iter()
            .filter_map(|id| {
                find_browser(id).map(|path| Browser {
                    id: id.to_string(),
                    path,
                })
            })
            .collect();
        let editors = crate::editors::detect();
        let editor = editors.first().map(|e| (e.name.clone(), e.exe.clone()));
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
            code_name: editor.map(|e| e.0),
            editors,
            tesseract: which::which("tesseract").ok().or_else(find_tesseract),
            pdftotext: which::which("pdftotext").ok(),
        }
    }

    /// Sets the editor projects open in: `choice` is an editor id, or
    /// "auto" for the one used most (`minutes` per app name this week).
    pub fn use_editor(&mut self, choice: &str, minutes: &[(String, i64)]) {
        let picked = crate::editors::choose(&self.editors, choice, minutes)
            .map(|e| (e.exe.clone(), e.name.clone()));
        self.code = picked.as_ref().map(|p| p.0.clone());
        self.code_name = picked.map(|p| p.1);
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
        "samsung" => vec![
            (
                "LOCALAPPDATA",
                r"Samsung\SamsungInternet\Application\samsung_internet.exe",
            ),
            (
                "LOCALAPPDATA",
                r"SamsungInternet\Application\samsung_internet.exe",
            ),
            (
                "ProgramFiles",
                r"Samsung\SamsungInternet\Application\samsung_internet.exe",
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
    fn knows_the_default_browser_by_prog_id() {
        assert_eq!(browser_for_prog_id("ChromeHTML"), Some("chrome"));
        assert_eq!(browser_for_prog_id("MSEdgeHTM"), Some("edge"));
        assert_eq!(
            browser_for_prog_id("FirefoxURL-308046B0AF4A39CB"),
            Some("firefox")
        );
        assert_eq!(browser_for_prog_id("SomethingElse"), None);
    }

    #[test]
    fn knows_browsers_from_start_menu_names() {
        assert_eq!(browser_for_app("Google Chrome", "Chrome"), Some("chrome"));
        assert_eq!(browser_for_app("Microsoft Edge", "MSEdge"), Some("edge"));
        assert_eq!(browser_for_app("Slack", "Slack"), None);
        assert_eq!(browser_for_app("Chrome Remote Desktop", "crd"), None);
    }

    #[test]
    fn opens_links_in_the_last_used_profile() {
        let dir = std::env::temp_dir().join(format!("sidekick-profile-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Profile 2")).unwrap();
        assert_eq!(last_profile(&dir), None, "no Local State yet");
        std::fs::write(
            dir.join("Local State"),
            r#"{"profile":{"last_used":"Profile 2"}}"#,
        )
        .unwrap();
        assert_eq!(last_profile(&dir).as_deref(), Some("Profile 2"));
        std::fs::write(
            dir.join("Local State"),
            r#"{"profile":{"last_used":"Gone"}}"#,
        )
        .unwrap();
        assert_eq!(last_profile(&dir), None, "a deleted profile is skipped");
        let _ = std::fs::remove_dir_all(dir);
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
        assert!(!caps.has("tool:op"));
        assert!(!caps.has("tool:bw"));
        assert!(!caps.has("nonsense"));
        assert_eq!(
            caps.browser("zen").unwrap().private_flag(),
            "--private-window"
        );
    }
}

#[cfg(test)]
mod command_tests {
    use super::browser_for_command;

    #[test]
    fn reads_the_browser_from_the_handler_program() {
        let zen =
            r#"(Default)    REG_SZ    "C:\Program Files\Zen Browser\zen.exe" -osint -url "%1""#;
        assert_eq!(browser_for_command(zen), Some("zen"));
        let chrome =
            r#""C:\Program Files\Google\Chrome\Application\chrome.exe" --single-argument %1"#;
        assert_eq!(browser_for_command(chrome), Some("chrome"));
        assert_eq!(browser_for_command("nothing here"), None);
    }
}
