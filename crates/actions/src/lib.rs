//! Built-in actions and detection of what is installed, so skills
//! only offer what can actually run.
//!
//! Every external program is started with an argument list, never a shell
//! string built from event data.

mod capabilities;
pub mod cleanup;
mod convert;
pub mod dev;
pub mod dnd;
pub mod doctor;
mod editors;
mod files;
pub mod gitflow;
pub mod office;
pub mod pc;
mod script;
mod system;
pub mod uia;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use capabilities::{Browser, Capabilities, default_browser};
pub use convert::TextBox;
pub use editors::Editor;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, thiserror::Error)]
pub enum ActionError {
    #[error("unknown action {0}")]
    Unknown(String),
    #[error("missing argument {0}")]
    MissingArg(&'static str),
    #[error("{0}")]
    Invalid(String),
    #[error("{0}")]
    Failed(String),
}

/// What happened, shown to the user on the island.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub message: String,
    /// A file the action produced, if any (for "show in folder").
    pub path: Option<String>,
}

impl Outcome {
    fn msg(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            path: None,
        }
    }
}

/// Actions that may run without a click when a skill is set to Auto.
/// Anything destructive or outward-facing is left out on purpose.
const SAFE: &[&str] = &[
    "open_path",
    "reveal_path",
    "copy_text",
    "copy_file",
    "open_url",
    "convert",
    "extract_archive",
    "clear_clipboard_later",
    "format_json_clipboard",
    "open_in_editor",
    "open_folder",
    "extract_text",
    "zip",
    "open_system_page",
    "launch_app",
    "open_app",
    "open_app_and_url",
    "dnd_on",
    "dnd_off",
    "create_env",
    "launch_project",
    "restore_layout",
    "noop",
];

pub fn is_safe(action: &str) -> bool {
    SAFE.contains(&action)
}

pub struct Executor {
    caps: Capabilities,
}

impl Executor {
    pub fn new(caps: Capabilities) -> Self {
        Self { caps }
    }

    pub fn capabilities(&self) -> &Capabilities {
        &self.caps
    }

    /// The text in an image (OCR), for questions about the screen.
    pub async fn read_text(&self, image: &Path) -> Result<String, ActionError> {
        convert::ocr_text(&self.caps, image).await
    }

    /// Where `target` is in an image, by its text.
    pub async fn find_text(
        &self,
        image: &Path,
        target: &str,
    ) -> Result<Option<TextBox>, ActionError> {
        let tsv = convert::ocr_tsv(&self.caps, image).await?;
        Ok(convert::find_text_box(&tsv, target))
    }

    /// Runs one action. Blocking work happens off the async runtime.
    pub async fn run(&self, action: &str, args: &Value) -> Result<Outcome, ActionError> {
        match action {
            "open_path" | "run_installer" => {
                let path = existing_path(args)?;
                // Folders must use the default file manager; open::that_detached
                // forces Windows Explorer for directories.
                if path.is_dir() {
                    system::open_folder_path(&path)?;
                } else {
                    open::that_detached(&path).map_err(fail)?;
                }
                Ok(Outcome::msg(format!("Opened {}", file_name(&path))))
            }
            "reveal_path" => {
                let path = existing_path(args)?;
                system::reveal(&path)?;
                Ok(Outcome::msg("Shown in folder"))
            }
            "copy_text" => {
                let text = arg(args, "text")?;
                system::set_clipboard_text(text)?;
                Ok(Outcome::msg("Copied"))
            }
            "copy_file" => {
                let path = existing_path(args)?;
                system::copy_file_to_clipboard(&path)
            }
            "open_url" => {
                let url = arg(args, "url")?;
                let private = args.get("private").and_then(Value::as_str) == Some("true");
                let browser = args.get("browser").and_then(Value::as_str);
                self.open_url(url, browser, private)
            }
            "convert" => {
                let path = existing_path(args)?;
                let to = arg(args, "to")?;
                convert::convert(&self.caps, &path, to).await
            }
            "zip" => {
                let paths: Vec<PathBuf> = match args.get("paths").and_then(Value::as_array) {
                    Some(list) => list
                        .iter()
                        .filter_map(Value::as_str)
                        .map(PathBuf::from)
                        .collect(),
                    None => vec![PathBuf::from(arg(args, "path")?)],
                };
                if let Some(gone) = paths.iter().find(|p| !p.exists()) {
                    return Err(ActionError::Failed(format!(
                        "{} no longer exists",
                        file_name(gone)
                    )));
                }
                let name = args.get("name").and_then(Value::as_str).unwrap_or_default();
                convert::zip(&self.caps, &paths, name).await
            }
            "move_file" => {
                let path = existing_path(args)?;
                let to = PathBuf::from(arg(args, "to")?);
                if !to.is_dir() {
                    return Err(ActionError::Failed(format!(
                        "{} is not a folder",
                        to.display()
                    )));
                }
                files::move_into(&path, &to)
            }
            "extract_archive" => {
                let path = existing_path(args)?;
                convert::extract(&self.caps, &path).await
            }
            "kill_port" => {
                let port: u16 = arg(args, "port")?
                    .parse()
                    .map_err(|_| ActionError::Invalid("port must be a number".into()))?;
                tokio::task::spawn_blocking(move || system::kill_port(port))
                    .await
                    .map_err(fail)?
            }
            "clear_clipboard_later" => {
                let secs: u64 = args
                    .get("seconds")
                    .and_then(Value::as_str)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(30);
                system::clear_secret_later(Duration::from_secs(secs));
                Ok(Outcome::msg(if secs == 0 {
                    "Clipboard cleared".to_string()
                } else {
                    format!("Clipboard clears in {secs} s")
                }))
            }
            "format_json_clipboard" => {
                let minify = args.get("minify").and_then(Value::as_str) == Some("true");
                system::format_json_clipboard(minify)
            }
            // Opens a folder in the default file manager. Unlike open_path it
            // never runs a file, so it is the one to use with paths from outside.
            "open_folder" => {
                let path = existing_path(args)?;
                if !path.is_dir() {
                    return Err(ActionError::Invalid(format!(
                        "{} is not a folder",
                        path.display()
                    )));
                }
                system::open_folder_path(&path)?;
                Ok(Outcome::msg(format!("Opened {}", file_name(&path))))
            }
            "open_in_editor" => {
                let path = existing_path(args)?;
                let code = self
                    .caps
                    .code
                    .as_ref()
                    .ok_or_else(|| ActionError::Failed("No code editor found".into()))?;
                let mut cmd = std::process::Command::new(code);
                cmd.arg(&path);
                system::spawn_detached(cmd)?;
                Ok(Outcome::msg(format!(
                    "Opened {} in {}",
                    file_name(&path),
                    self.caps.code_name.as_deref().unwrap_or("your editor")
                )))
            }
            "open_system_page" => system::open_system_page(arg(args, "page")?),
            "launch_app" => {
                let name = arg(args, "name")?.to_owned();
                tokio::task::spawn_blocking(move || pc::launch_app(&name))
                    .await
                    .map_err(fail)?
            }
            "open_app" => {
                let name = arg(args, "name")?.to_owned();
                tokio::task::spawn_blocking(move || pc::open_app(&name))
                    .await
                    .map_err(fail)?
            }
            // A notification with a link: the app (or its website) and the
            // link, in one tap.
            "open_app_and_url" => {
                let url = arg(args, "url")?.to_owned();
                let name = arg(args, "name")?.to_owned();
                let opened = tokio::task::spawn_blocking(move || pc::open_app(&name))
                    .await
                    .map_err(fail)?;
                let link = self.open_url(&url, None, false);
                match (opened, link) {
                    (Ok(a), Ok(_)) => Ok(Outcome::msg(format!("{} and the link", a.message))),
                    (_, Err(e)) | (Err(e), _) => Err(e),
                }
            }
            "close_app" => {
                let name = arg(args, "name")?.to_owned();
                tokio::task::spawn_blocking(move || pc::close_window(&name))
                    .await
                    .map_err(fail)?
            }
            "sleep_pc" => pc::sleep_pc(),
            "dnd_on" | "dnd_off" => {
                let on = action == "dnd_on";
                tokio::task::spawn_blocking(move || pc::set_dnd(on))
                    .await
                    .map_err(fail)?
            }
            "install_app" | "update_app" => {
                let id = arg(args, "id")?.to_owned();
                let upgrade = action == "update_app";
                tokio::task::spawn_blocking(move || pc::app_install(&id, upgrade))
                    .await
                    .map_err(fail)?
            }
            "set_compat" => {
                let exe = arg(args, "exe")?.to_owned();
                let mode = arg(args, "mode")?.to_owned();
                tokio::task::spawn_blocking(move || doctor::set_compat(&exe, &mode))
                    .await
                    .map_err(fail)?
            }
            "clear_compat" => {
                let exe = arg(args, "exe")?.to_owned();
                tokio::task::spawn_blocking(move || doctor::clear_compat(&exe))
                    .await
                    .map_err(fail)?
            }
            "empty_recycle_bin" => tokio::task::spawn_blocking(pc::empty_recycle_bin)
                .await
                .map_err(fail)?,
            "git_commit" => {
                let path = existing_path(args)?;
                let message = arg(args, "message")?.to_owned();
                tokio::task::spawn_blocking(move || gitflow::commit(&path, &message))
                    .await
                    .map_err(fail)?
            }
            "git_delete_branches" => {
                let path = existing_path(args)?;
                let branches: Vec<String> = args["branches"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|b| b.as_str().map(str::to_owned))
                    .collect();
                tokio::task::spawn_blocking(move || gitflow::delete_branches(&path, &branches))
                    .await
                    .map_err(fail)?
            }
            "git_pull" => {
                let path = existing_path(args)?;
                tokio::task::spawn_blocking(move || dev::pull(&path))
                    .await
                    .map_err(fail)?
            }
            "install_deps" => {
                let path = existing_path(args)?;
                tokio::task::spawn_blocking(move || dev::install(&path))
                    .await
                    .map_err(fail)?
            }
            "create_env" => dev::create_env(&existing_path(args)?),
            "start_docker" => dev::start_docker(),
            "trash_download" => cleanup::trash_download(&existing_path(args)?),
            "clean_downloads" => tokio::task::spawn_blocking(cleanup::clean_downloads)
                .await
                .map_err(fail)?,
            "sort_downloads" => tokio::task::spawn_blocking(cleanup::sort_downloads)
                .await
                .map_err(fail)?,
            "clean_installers" => tokio::task::spawn_blocking(cleanup::clean_installers)
                .await
                .map_err(fail)?,
            "launch_project" => dev::launch(
                &existing_path(args)?,
                self.caps
                    .code
                    .as_deref()
                    .map(|p| (p, self.caps.code_name.as_deref().unwrap_or("your editor"))),
            ),
            "extract_text" => {
                let path = existing_path(args)?;
                convert::ocr(&self.caps, &path).await
            }
            "noop" => Ok(Outcome::msg(
                arg(args, "message").unwrap_or("Done").to_string(),
            )),
            "fail" => Err(ActionError::Failed(
                arg(args, "message")
                    .unwrap_or("Simulated failure")
                    .to_string(),
            )),
            other => Err(ActionError::Unknown(other.to_string())),
        }
    }

    fn open_url(
        &self,
        url: &str,
        browser: Option<&str>,
        private: bool,
    ) -> Result<Outcome, ActionError> {
        // Only web links: a skill must never be able to open file:// or javascript: URLs.
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(ActionError::Invalid(format!("not a web link: {url}")));
        }
        // No browser named: use the default one directly, so a Chromium
        // default still gets its profile and skips the picker.
        let browser = browser.or_else(|| {
            capabilities::default_browser().filter(|id| self.caps.browser(id).is_some())
        });
        match (browser, browser.and_then(|b| self.caps.browser(b))) {
            (_, Some(b)) => {
                let mut cmd = std::process::Command::new(&b.path);
                if private {
                    cmd.arg(b.private_flag());
                } else if let Some(profile) = b.profile_arg() {
                    cmd.arg(profile);
                }
                cmd.arg(url);
                system::spawn_detached(cmd)?;
                Ok(Outcome::msg(format!(
                    "Opened in {}{}",
                    b.label(),
                    if private { " (private)" } else { "" }
                )))
            }
            (Some(name), None) => Err(ActionError::Failed(format!("{name} is not installed"))),
            (None, None) => {
                open::that_detached(url).map_err(fail)?;
                Ok(Outcome::msg("Opened in your default browser"))
            }
        }
    }
}

fn fail(err: impl std::fmt::Display) -> ActionError {
    ActionError::Failed(err.to_string())
}

fn arg<'a>(args: &'a Value, name: &'static str) -> Result<&'a str, ActionError> {
    args.get(name)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or(ActionError::MissingArg(name))
}

fn existing_path(args: &Value) -> Result<PathBuf, ActionError> {
    let path = PathBuf::from(arg(args, "path")?);
    if !path.exists() {
        return Err(ActionError::Failed(format!(
            "{} no longer exists",
            file_name(&path)
        )));
    }
    Ok(path)
}

pub(crate) fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn open_folder_refuses_files() {
        let file =
            std::env::temp_dir().join(format!("sidekick-not-a-folder-{}.exe", std::process::id()));
        std::fs::write(&file, b"x").unwrap();
        let err = exec()
            .run(
                "open_folder",
                &serde_json::json!({ "path": file.to_string_lossy() }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a folder"));
        let _ = std::fs::remove_file(file);
    }

    #[tokio::test]
    async fn system_pages_come_from_a_fixed_list() {
        let err = exec()
            .run(
                "open_system_page",
                &serde_json::json!({ "page": "cmd.exe" }),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("unknown system page"));
        assert!(is_safe("open_system_page"));
        assert!(!is_safe("kill_port"));
    }

    fn exec() -> Executor {
        Executor::new(Capabilities::default())
    }

    #[tokio::test]
    async fn rejects_non_web_urls_and_unknown_actions() {
        let e = exec();
        let err = e
            .run(
                "open_url",
                &serde_json::json!({"url": "file:///etc/passwd"}),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not a web link"));
        assert!(matches!(
            e.run("rm_rf", &Value::Null).await,
            Err(ActionError::Unknown(_))
        ));
        let err = e
            .run(
                "open_url",
                &serde_json::json!({"url": "https://x.dev", "browser": "zen"}),
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "zen is not installed");
    }

    #[tokio::test]
    async fn reports_missing_files_and_args() {
        let e = exec();
        let err = e
            .run(
                "open_path",
                &serde_json::json!({"path": "/nope/missing.pdf"}),
            )
            .await
            .unwrap_err();
        assert_eq!(err.to_string(), "missing.pdf no longer exists");
        assert!(matches!(
            e.run("convert", &serde_json::json!({})).await,
            Err(ActionError::MissingArg("path"))
        ));
    }

    #[test]
    fn destructive_actions_are_never_safe() {
        assert!(is_safe("open_url"));
        assert!(!is_safe("kill_port"));
        assert!(!is_safe("run_installer"));
    }
}
