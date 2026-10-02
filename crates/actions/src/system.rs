//! OS-level actions: reveal in the file manager, clipboard, processes.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use sidekick_sensors::classify::{ClipKind, clip_kind};

use crate::{ActionError, Outcome, file_name};

fn fail(err: impl std::fmt::Display) -> ActionError {
    ActionError::Failed(err.to_string())
}

/// Starts a program without waiting for it and without a console window.
pub fn spawn_detached(mut cmd: Command) -> Result<(), ActionError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    cmd.spawn().map(drop).map_err(fail)
}

/// Opens a folder with the user's default file manager (Files, Explorer, …).
/// Does not call explorer.exe or SHOpenFolderAndSelectItems, which always
/// force Windows Explorer even when another app is the Directory default.
pub fn open_folder_path(path: &Path) -> Result<(), ActionError> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Invoke-Item uses the shell association for Directory / Folder.
        let mut cmd = Command::new("powershell.exe");
        cmd.args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-Command",
            "Invoke-Item -LiteralPath $env:SIDEKICK_OPEN",
        ])
        .env("SIDEKICK_OPEN", path.as_os_str())
        .creation_flags(0x0800_0000);
        cmd.spawn().map(drop).map_err(fail)
    }
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg(path);
        spawn_detached(cmd)
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        open::that_detached(path).map_err(fail)
    }
}

/// Opens the folder that contains `path` (or `path` itself when it is a folder)
/// in the default file manager.
pub fn reveal(path: &Path) -> Result<(), ActionError> {
    #[cfg(target_os = "macos")]
    {
        let mut cmd = Command::new("open");
        cmd.arg("-R").arg(path);
        return spawn_detached(cmd);
    }
    #[cfg(not(target_os = "macos"))]
    {
        let folder = if path.is_dir() {
            path
        } else {
            path.parent().unwrap_or(path)
        };
        open_folder_path(folder)
    }
}

pub fn set_clipboard_text(text: &str) -> Result<(), ActionError> {
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text))
        .map_err(fail)
}

fn clipboard_text() -> Option<String> {
    arboard::Clipboard::new().ok()?.get_text().ok()
}

/// Puts the file itself on the clipboard (paste into Explorer, Slack, mail).
/// Where that is not supported, copies its path instead.
pub fn copy_file_to_clipboard(path: &Path) -> Result<Outcome, ActionError> {
    #[cfg(windows)]
    {
        let text = path.display().to_string();
        let _clip = clipboard_win::Clipboard::new_attempts(10).map_err(fail)?;
        clipboard_win::raw::empty().map_err(fail)?;
        clipboard_win::raw::set_file_list(&[text.as_str()]).map_err(fail)?;
        Ok(Outcome {
            message: format!("Copied {}", file_name(path)),
            path: None,
        })
    }
    #[cfg(not(windows))]
    {
        set_clipboard_text(&path.display().to_string())?;
        Ok(Outcome {
            message: format!("Copied the path of {}", file_name(path)),
            path: None,
        })
    }
}

/// Clears the clipboard after `delay`, but only if it still holds a secret,
/// so anything the user copied since is left alone.
pub fn clear_secret_later(delay: Duration) {
    std::thread::spawn(move || {
        std::thread::sleep(delay);
        if clipboard_text().is_some_and(|t| clip_kind(&t) == ClipKind::Secret)
            && let Ok(mut c) = arboard::Clipboard::new()
        {
            let _ = c.clear();
        }
    });
}

pub fn format_json_clipboard(minify: bool) -> Result<Outcome, ActionError> {
    let text = clipboard_text().ok_or_else(|| ActionError::Failed("clipboard is empty".into()))?;
    let value: serde_json::Value = serde_json::from_str(text.trim())
        .map_err(|e| ActionError::Failed(format!("not valid JSON: {e}")))?;
    let out = if minify {
        serde_json::to_string(&value)
    } else {
        serde_json::to_string_pretty(&value)
    }
    .map_err(fail)?;
    set_clipboard_text(&out)?;
    Ok(Outcome {
        message: if minify {
            "JSON minified"
        } else {
            "JSON formatted"
        }
        .into(),
        path: None,
    })
}

pub fn kill_port(port: u16) -> Result<Outcome, ActionError> {
    let owner = listeners::get_process_by_port(port, listeners::Protocol::TCP)
        .map_err(|_| ActionError::Failed(format!("nothing is listening on port {port}")))?;
    let mut sys = sysinfo::System::new();
    let pid = sysinfo::Pid::from_u32(owner.pid);
    sys.refresh_processes(sysinfo::ProcessesToUpdate::Some(&[pid]), true);
    let process = sys
        .process(pid)
        .ok_or_else(|| ActionError::Failed(format!("process {} already exited", owner.pid)))?;
    if !process.kill() {
        return Err(ActionError::Failed(format!(
            "could not stop {} (PID {})",
            owner.name, owner.pid
        )));
    }
    Ok(Outcome {
        message: format!("Stopped {} (PID {}) on port {port}", owner.name, owner.pid),
        path: None,
    })
}

/// Opens a Windows settings page or tool from a fixed list; skills can
/// never pass an arbitrary URI or program here.
pub fn open_system_page(page: &str) -> Result<Outcome, ActionError> {
    let uri = match page {
        "storage" => "ms-settings:storagesense",
        "apps" => "ms-settings:appsfeatures",
        "power" => "ms-settings:powersleep",
        "battery" => "ms-settings:batterysaver",
        // Focus and Do Not Disturb (Windows 11; Focus assist on Windows 10).
        "focus" => "ms-settings:quiethours",
        "taskmgr" => {
            if !cfg!(windows) {
                return Err(ActionError::Failed("Task Manager is a Windows tool".into()));
            }
            spawn_detached(std::process::Command::new("taskmgr.exe"))?;
            return Ok(Outcome::msg("Opened Task Manager"));
        }
        other => return Err(ActionError::Invalid(format!("unknown system page {other}"))),
    };
    if !cfg!(windows) {
        return Err(ActionError::Failed(
            "Windows settings pages need Windows".into(),
        ));
    }
    open::that_detached(uri).map_err(|e| ActionError::Failed(e.to_string()))?;
    Ok(Outcome::msg("Opened Settings"))
}
