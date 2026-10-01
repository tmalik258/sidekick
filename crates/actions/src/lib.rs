//! Built-in actions (FR-ACT-03) and detection of what is installed, so skills
//! only offer what can actually run.
//!
//! Every external program is started with an argument list, never a shell
//! string built from event data (NFR-SEC-06).

mod capabilities;
mod convert;
mod system;

use std::path::{Path, PathBuf};
use std::time::Duration;

pub use capabilities::{Browser, Capabilities};
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
/// Anything destructive or outward-facing is left out on purpose (FR-ACT-02).
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
    "open_system_page",
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

    /// Runs one action. Blocking work happens off the async runtime.
    pub async fn run(&self, action: &str, args: &Value) -> Result<Outcome, ActionError> {
        match action {
            "open_path" | "run_installer" => {
                let path = existing_path(args)?;
                open::that_detached(&path).map_err(fail)?;
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
            "open_in_editor" => {
                let path = existing_path(args)?;
                let code = self
                    .caps
                    .code
                    .as_ref()
                    .ok_or_else(|| ActionError::Failed("VS Code is not installed".into()))?;
                let mut cmd = std::process::Command::new(code);
                cmd.arg(&path);
                system::spawn_detached(cmd)?;
                Ok(Outcome::msg(format!(
                    "Opened {} in VS Code",
                    file_name(&path)
                )))
            }
            "open_system_page" => system::open_system_page(arg(args, "page")?),
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
        match (browser, browser.and_then(|b| self.caps.browser(b))) {
            (_, Some(b)) => {
                let mut cmd = std::process::Command::new(&b.path);
                if private {
                    cmd.arg(b.private_flag());
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
