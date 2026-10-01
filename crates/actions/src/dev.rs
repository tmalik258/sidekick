//! Developer actions (FR-DEV-04, 05, 07, 10): pull, install dependencies,
//! create `.env` from its example, start Docker Desktop, and open a project.
//! Every command is a fixed program with argument lists; paths are only
//! ever passed as arguments, never through a shell.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use crate::{ActionError, Outcome, fail, system};

fn hidden(cmd: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    #[cfg(not(windows))]
    let _ = cmd;
}

fn repo(path: &Path) -> Result<(), ActionError> {
    if path.join(".git").exists() {
        Ok(())
    } else {
        Err(ActionError::Invalid(format!(
            "{} is not a git repository",
            path.display()
        )))
    }
}

fn git(path: &Path, args: &[&str]) -> Result<String, ActionError> {
    let mut cmd = Command::new("git");
    cmd.arg("-C")
        .arg(path)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    hidden(&mut cmd);
    let out = cmd.output().map_err(fail)?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let err = String::from_utf8_lossy(&out.stderr);
        Err(ActionError::Failed(
            err.lines().last().unwrap_or("git failed").trim().to_owned(),
        ))
    }
}

/// Lockfiles and the command that installs from each.
const MANAGERS: &[(&str, &str, &[&str])] = &[
    ("pnpm-lock.yaml", "pnpm", &["install"]),
    ("package-lock.json", "npm", &["install"]),
    ("yarn.lock", "yarn", &["install"]),
    ("bun.lockb", "bun", &["install"]),
    ("uv.lock", "uv", &["sync"]),
    ("poetry.lock", "poetry", &["install"]),
    ("Cargo.lock", "cargo", &["fetch"]),
];

pub fn lockfiles_changed(files: &str) -> bool {
    files
        .lines()
        .any(|f| MANAGERS.iter().any(|(lock, _, _)| f.trim().ends_with(lock)))
}

/// Fast-forward pull; says when dependencies changed.
pub fn pull(path: &Path) -> Result<Outcome, ActionError> {
    repo(path)?;
    let before = git(path, &["rev-parse", "HEAD"])?;
    git(path, &["pull", "--ff-only", "--quiet"])?;
    let after = git(path, &["rev-parse", "HEAD"])?;
    if before.trim() == after.trim() {
        return Ok(Outcome::msg("Already up to date"));
    }
    let changed = git(path, &["diff", "--name-only", before.trim(), after.trim()])?;
    let n = git(
        path,
        &[
            "rev-list",
            "--count",
            &format!("{}..{}", before.trim(), after.trim()),
        ],
    )?;
    let mut msg = format!("Pulled {} commits", n.trim());
    if lockfiles_changed(&changed) {
        msg.push_str("; dependencies changed, install them next");
    }
    Ok(Outcome::msg(msg))
}

/// Which package manager a project uses, by its lockfile.
pub fn manager(path: &Path) -> Option<(&'static str, &'static [&'static str])> {
    MANAGERS
        .iter()
        .find(|(lock, _, _)| path.join(lock).exists())
        .map(|(_, tool, args)| (*tool, *args))
}

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);

pub fn install(path: &Path) -> Result<Outcome, ActionError> {
    let (tool, args) =
        manager(path).ok_or_else(|| ActionError::Failed("no lockfile found".into()))?;
    let exe =
        which::which(tool).map_err(|_| ActionError::Failed(format!("{tool} is not installed")))?;
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .current_dir(path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    hidden(&mut cmd);
    let mut child = cmd.spawn().map_err(fail)?;
    let start = std::time::Instant::now();
    loop {
        if let Some(status) = child.try_wait().map_err(fail)? {
            if status.success() {
                return Ok(Outcome::msg(format!("{tool} {} finished", args.join(" "))));
            }
            let mut err = String::new();
            if let Some(mut e) = child.stderr.take() {
                let _ = std::io::Read::read_to_string(&mut e, &mut err);
            }
            return Err(ActionError::Failed(format!(
                "{tool} failed: {}",
                err.lines()
                    .rev()
                    .find(|l| !l.trim().is_empty())
                    .unwrap_or("see the terminal")
                    .trim()
            )));
        }
        if start.elapsed() > INSTALL_TIMEOUT {
            let _ = child.kill();
            return Err(ActionError::Failed(format!("{tool} took too long")));
        }
        std::thread::sleep(Duration::from_millis(250));
    }
}

/// Copies `.env.example` to `.env`; never overwrites.
pub fn create_env(path: &Path) -> Result<Outcome, ActionError> {
    let example = path.join(".env.example");
    let env = path.join(".env");
    if env.exists() {
        return Err(ActionError::Failed(".env already exists".into()));
    }
    std::fs::copy(&example, &env).map_err(fail)?;
    Ok(Outcome {
        message: "Created .env from .env.example; fill in the values".into(),
        path: Some(env.to_string_lossy().into_owned()),
    })
}

fn docker_desktop() -> Option<PathBuf> {
    let base = std::env::var_os("ProgramFiles").map(PathBuf::from)?;
    let exe = base
        .join("Docker")
        .join("Docker")
        .join("Docker Desktop.exe");
    exe.is_file().then_some(exe)
}

pub fn start_docker() -> Result<Outcome, ActionError> {
    let exe = docker_desktop()
        .ok_or_else(|| ActionError::Failed("Docker Desktop is not installed".into()))?;
    system::spawn_detached(Command::new(exe))?;
    Ok(Outcome::msg("Starting Docker Desktop"))
}

/// Editor plus a terminal in the project folder (FR-DEV-10).
pub fn launch(path: &Path, code: Option<&Path>) -> Result<Outcome, ActionError> {
    let mut opened = Vec::new();
    if let Some(code) = code {
        let mut cmd = Command::new(code);
        cmd.arg(path);
        system::spawn_detached(cmd)?;
        opened.push("VS Code");
    }
    if let Ok(wt) = which::which("wt") {
        let mut cmd = Command::new(wt);
        cmd.arg("-d").arg(path);
        // Windows Terminal must be visible, so no hidden flag here.
        cmd.spawn().map(drop).map_err(fail)?;
        opened.push("a terminal");
    }
    if opened.is_empty() {
        open::that_detached(path).map_err(fail)?;
        opened.push("the folder");
    }
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    Ok(Outcome::msg(format!(
        "Opened {name} in {}",
        opened.join(" and ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spots_lockfile_changes_and_managers() {
        assert!(lockfiles_changed("src/a.ts\npnpm-lock.yaml\n"));
        assert!(lockfiles_changed("api/uv.lock"));
        assert!(!lockfiles_changed("README.md\npackage.json"));
        let dir = std::env::temp_dir().join(format!("sidekick-dev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(manager(&dir).is_none());
        std::fs::write(dir.join("uv.lock"), "").unwrap();
        assert_eq!(manager(&dir).unwrap().0, "uv");
        std::fs::write(dir.join(".env.example"), "KEY=").unwrap();
        create_env(&dir).unwrap();
        assert!(dir.join(".env").exists());
        assert!(create_env(&dir).is_err(), "never overwrites");
        assert!(pull(&dir).is_err(), "not a repo");
        std::fs::remove_dir_all(dir).unwrap();
    }
}
