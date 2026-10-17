//! Terminals for the editor dock. The user's shell starts in the project
//! folder with no arguments; what the user types goes in as raw bytes, so no
//! command line is ever built from text. Every terminal closes with the dock.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub struct Term {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

static TERMS: Mutex<Option<HashMap<u32, Term>>> = Mutex::new(None);
static NEXT: AtomicU32 = AtomicU32::new(1);

fn with<T>(f: impl FnOnce(&mut HashMap<u32, Term>) -> T) -> T {
    let mut g = TERMS.lock().unwrap_or_else(|e| e.into_inner());
    f(g.get_or_insert_with(HashMap::new))
}

fn shell() -> CommandBuilder {
    #[cfg(windows)]
    {
        let mut c = CommandBuilder::new("powershell.exe");
        c.arg("-NoLogo");
        c
    }
    #[cfg(not(windows))]
    {
        CommandBuilder::new(std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into()))
    }
}

/// Starts a shell in `cwd`. `on_data` gets its output until it exits, then
/// `None` once.
pub fn open(
    cwd: &Path,
    cols: u16,
    rows: u16,
    on_data: impl Fn(u32, Option<String>) + Send + 'static,
) -> Result<u32, String> {
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: rows.max(2),
            cols: cols.max(10),
            pixel_width: 0,
            pixel_height: 0,
        })
        .map_err(|e| e.to_string())?;
    let mut cmd = shell();
    cmd.cwd(cwd);
    let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
    let writer = pair.master.take_writer().map_err(|e| e.to_string())?;
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    with(|t| {
        t.insert(
            id,
            Term {
                master: pair.master,
                writer,
                child,
            },
        )
    });
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        let mut carry = Vec::new();
        loop {
            match reader.read(&mut buf) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    carry.extend_from_slice(&buf[..n]);
                    // Send only whole UTF-8 characters.
                    let ok = match std::str::from_utf8(&carry) {
                        Ok(_) => carry.len(),
                        Err(e) if e.error_len().is_none() => e.valid_up_to(),
                        Err(_) => carry.len(),
                    };
                    let text = String::from_utf8_lossy(&carry[..ok]).into_owned();
                    carry.drain(..ok);
                    on_data(id, Some(text));
                }
            }
        }
        on_data(id, None);
        with(|t| t.remove(&id));
    });
    Ok(id)
}

pub fn write(id: u32, data: &str) -> Result<(), String> {
    with(|t| match t.get_mut(&id) {
        Some(term) => term
            .writer
            .write_all(data.as_bytes())
            .and_then(|_| term.writer.flush())
            .map_err(|e| e.to_string()),
        None => Err("That terminal has closed.".into()),
    })
}

pub fn resize(id: u32, cols: u16, rows: u16) -> Result<(), String> {
    with(|t| match t.get(&id) {
        Some(term) => term
            .master
            .resize(PtySize {
                rows: rows.max(2),
                cols: cols.max(10),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string()),
        None => Ok(()),
    })
}

pub fn close(id: u32) {
    if let Some(mut term) = with(|t| t.remove(&id)) {
        let _ = term.child.kill();
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    #[test]
    fn starts_in_the_project_folder() {
        let dir = std::env::temp_dir().join(format!("duskie-pty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = std::fs::canonicalize(dir).unwrap();
        let (tx, rx) = mpsc::channel();
        let id = open(&dir, 80, 24, move |_, d| {
            let _ = tx.send(d);
        })
        .unwrap();
        write(id, "pwd\n").unwrap();
        let mut out = String::new();
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end && !out.contains(dir.to_str().unwrap()) {
            if let Ok(Some(d)) = rx.recv_timeout(Duration::from_millis(200)) {
                out.push_str(&d);
            }
        }
        close(id);
        assert!(out.contains(dir.to_str().unwrap()), "{out}");
    }
}
