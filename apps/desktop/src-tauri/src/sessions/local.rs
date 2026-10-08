//! The local coding agent: a coding model on this PC through Ollama, driven
//! by Sidekick. It can list, read and search the project and propose edits;
//! every edit is asked about, whatever the mode, and nothing is written
//! without the user's OK. When it goes round in circles it offers a handoff.

use std::path::{Component, Path, PathBuf};

use serde_json::{Value, json};
use tauri::AppHandle;
use tokio::sync::mpsc;

use super::{Answer, Cmd, Mode, ask, emit, turn_ended};

/// Tool rounds in one turn before it counts as stuck.
const MAX_ROUNDS: usize = 16;
/// Largest file it reads whole, in bytes.
const MAX_READ: u64 = 200_000;

/// A coding model and its download size, from small to large.
const MODELS: [(&str, &str, u64); 2] = [
    ("qwen2.5-coder:7b", "4.7 GB", 0),
    ("qwen2.5-coder:14b", "9.0 GB", 24),
];

/// The largest coding model that fits this PC's memory (GB of RAM).
pub fn fitting_model(ram_gb: u64) -> &'static (&'static str, &'static str, u64) {
    MODELS
        .iter()
        .rev()
        .find(|(_, _, need)| ram_gb >= *need)
        .unwrap_or(&MODELS[0])
}

fn ram_gb() -> u64 {
    let mut sys = sysinfo::System::new();
    sys.refresh_memory();
    sys.total_memory() / 1_000_000_000
}

/// Ollama's own API root from the OpenAI-style base URL in settings.
fn root(base_url: &str) -> String {
    let b = base_url.trim().trim_end_matches('/');
    let b = if b.is_empty() {
        sidekick_ai::OLLAMA_URL
    } else {
        b
    };
    b.trim_end_matches("/v1").replace("localhost", "127.0.0.1")
}

const SYSTEM: &str = "You are a careful coding agent working in the user's project. \
Use list_files, read_file and search to look before you change anything. \
To change a file, call write_file with the whole new content; the user approves each edit. \
Make small, clear changes. When done, say in one or two sentences what you changed. \
If you cannot do the task, say so plainly.";

fn tools(mode: Mode) -> Value {
    let mut t = vec![
        json!({"type":"function","function":{"name":"list_files","description":"List files and folders in a project folder.","parameters":{"type":"object","properties":{"path":{"type":"string","description":"Folder relative to the project, empty for the root"}}}}}),
        json!({"type":"function","function":{"name":"read_file","description":"Read a file in the project.","parameters":{"type":"object","properties":{"path":{"type":"string"}},"required":["path"]}}}),
        json!({"type":"function","function":{"name":"search","description":"Find lines containing some text across the project.","parameters":{"type":"object","properties":{"text":{"type":"string"}},"required":["text"]}}}),
    ];
    if mode != Mode::Plan {
        t.push(json!({"type":"function","function":{"name":"write_file","description":"Replace a file's content, or create it. The user approves first.","parameters":{"type":"object","properties":{"path":{"type":"string"},"content":{"type":"string"}},"required":["path","content"]}}}));
    }
    Value::Array(t)
}

/// `rel` inside `project`, or None when it would leave it.
pub fn inside(project: &Path, rel: &str) -> Option<PathBuf> {
    let rel = Path::new(rel.trim().trim_start_matches(['/', '\\']));
    if rel
        .components()
        .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return None;
    }
    Some(project.join(rel))
}

fn skip(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "node_modules" | "target" | "dist" | "build")
}

fn list(project: &Path, rel: &str) -> String {
    let Some(dir) = inside(project, rel) else {
        return "That path is outside the project.".into();
    };
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return "No such folder.".into();
    };
    let mut names: Vec<String> = rd
        .flatten()
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            (!skip(&n)).then(|| {
                if e.path().is_dir() {
                    format!("{n}/")
                } else {
                    n
                }
            })
        })
        .collect();
    names.sort();
    names.truncate(300);
    names.join("\n")
}

fn read(project: &Path, rel: &str) -> String {
    let Some(p) = inside(project, rel) else {
        return "That path is outside the project.".into();
    };
    match std::fs::metadata(&p) {
        Ok(m) if m.len() > MAX_READ => "That file is too large to read whole.".into(),
        Ok(_) => std::fs::read_to_string(&p).unwrap_or_else(|_| "Not a text file.".into()),
        Err(_) => "No such file.".into(),
    }
}

fn search(project: &Path, text: &str) -> String {
    let mut out = Vec::new();
    let mut stack = vec![project.to_owned()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            if skip(&name) {
                continue;
            }
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if e.metadata().is_ok_and(|m| m.len() <= MAX_READ)
                && let Ok(body) = std::fs::read_to_string(&p)
            {
                let rel = p
                    .strip_prefix(project)
                    .unwrap_or(&p)
                    .to_string_lossy()
                    .replace('\\', "/");
                for (i, line) in body.lines().enumerate() {
                    if line.contains(text) {
                        out.push(format!("{rel}:{}: {}", i + 1, line.trim()));
                        if out.len() >= 60 {
                            return out.join("\n");
                        }
                    }
                }
            }
        }
    }
    if out.is_empty() {
        "No matches.".into()
    } else {
        out.join("\n")
    }
}

/// A short before/after for the question, so the user sees what changes.
pub fn preview(old: &str, new: &str) -> String {
    let (o, n): (Vec<&str>, Vec<&str>) = (old.lines().collect(), new.lines().collect());
    let start = o.iter().zip(&n).take_while(|(a, b)| a == b).count();
    let end = o[start..]
        .iter()
        .rev()
        .zip(n[start..].iter().rev())
        .take_while(|(a, b)| a == b)
        .count();
    let mut lines: Vec<String> = Vec::new();
    lines.extend(
        o[start..o.len() - end]
            .iter()
            .take(12)
            .map(|l| format!("- {l}")),
    );
    lines.extend(
        n[start..n.len() - end]
            .iter()
            .take(12)
            .map(|l| format!("+ {l}")),
    );
    lines.join("\n")
}

/// Makes sure the model is there, offering to download it first.
async fn ensure_model(
    app: &AppHandle,
    id: &str,
    http: &reqwest::Client,
    root: &str,
    chosen: Option<String>,
) -> Result<String, String> {
    let tags: Value = http
        .get(format!("{root}/api/tags"))
        .send()
        .await
        .map_err(|_| "Ollama is not running. Start Ollama, then try again.".to_string())?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let have: Vec<String> = tags["models"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|m| m["name"].as_str().map(String::from))
        .collect();
    // A coding model you already chose or have wins over downloading one.
    if let Some(m) = chosen.filter(|m| m.contains("coder") && have.contains(m)) {
        return Ok(m);
    }
    if let Some(m) = have.iter().find(|m| m.contains("coder")) {
        return Ok(m.clone());
    }
    let (model, size, _) = fitting_model(ram_gb());
    let answer = ask(
        app,
        id,
        &format!("download {model} ({size})"),
        "A coding model that fits this PC. It runs here, so nothing leaves it.",
    )
    .await;
    if answer == Answer::Deny {
        return Err("No local coding model yet.".into());
    }
    emit(
        app,
        id,
        json!({"kind":"step","id":"pull","tool":"Download","label":format!("Downloading {model}"),"detail":size,"state":"running"}),
    );
    let res = http
        .post(format!("{root}/api/pull"))
        .json(&json!({"model": model, "stream": false}))
        .timeout(std::time::Duration::from_secs(60 * 60))
        .send()
        .await
        .and_then(reqwest::Response::error_for_status);
    let state = if res.is_ok() { "done" } else { "failed" };
    emit(app, id, json!({"kind":"step","id":"pull","state":state}));
    res.map_err(|e| format!("Download failed: {e}"))?;
    Ok((*model).to_owned())
}

/// Runs one session until it is stopped.
pub async fn run(
    app: &AppHandle,
    id: &str,
    project: &Path,
    mode: Mode,
    chosen: Option<String>,
    base_url: &str,
    mut rx: mpsc::UnboundedReceiver<Cmd>,
) -> Result<(), String> {
    let http = reqwest::Client::new();
    let root = root(base_url);
    let mut messages = vec![json!({"role":"system","content":SYSTEM})];
    let mut model: Option<String> = None;
    let mut always = false;
    let mut step = 0usize;
    while let Some(cmd) = rx.recv().await {
        let Cmd::Send(text) = cmd else { return Ok(()) };
        emit(app, id, json!({"kind":"working"}));
        let m = match &model {
            Some(m) => m.clone(),
            None => match ensure_model(app, id, &http, &root, chosen.clone()).await {
                Ok(m) => {
                    model = Some(m.clone());
                    m
                }
                Err(e) => {
                    emit(app, id, json!({"kind":"turn","error":e}));
                    continue;
                }
            },
        };
        messages.push(json!({"role":"user","content":text}));
        let mut error = None;
        let mut done = false;
        for _ in 0..MAX_ROUNDS {
            let req = http
                .post(format!("{root}/api/chat"))
                .json(&json!({"model":m,"messages":messages,"tools":tools(mode),"stream":false}))
                .send();
            let reply: Value = tokio::select! {
                r = req => match r {
                    Ok(r) => r.json().await.unwrap_or(Value::Null),
                    Err(e) => { error = Some(e.to_string()); break; }
                },
                c = rx.recv() => match c {
                    Some(Cmd::Stop) | None => return Ok(()),
                    // A message mid-turn waits for the next turn.
                    Some(Cmd::Send(_)) => continue,
                },
            };
            let msg = reply["message"].clone();
            if let Some(t) = msg["content"].as_str().filter(|t| !t.trim().is_empty()) {
                emit(app, id, json!({"kind":"text","text":t}));
            }
            messages.push(msg.clone());
            let calls = msg["tool_calls"].as_array().cloned().unwrap_or_default();
            if calls.is_empty() {
                done = true;
                break;
            }
            for call in calls {
                let name = call["function"]["name"].as_str().unwrap_or("");
                let args = &call["function"]["arguments"];
                let path = args["path"].as_str().unwrap_or("");
                step += 1;
                let sid = format!("l{step}");
                let (label, detail) = match name {
                    "list_files" => ("List".to_string(), path.to_string()),
                    "read_file" => ("Read".to_string(), path.to_string()),
                    "search" => (
                        "Search".to_string(),
                        args["text"].as_str().unwrap_or("").to_string(),
                    ),
                    _ => ("Edit".to_string(), path.to_string()),
                };
                emit(
                    app,
                    id,
                    json!({"kind":"step","id":sid,"tool":name,"label":label,"detail":detail,"state":"running"}),
                );
                let (result, ok) = match name {
                    "list_files" => (list(project, path), true),
                    "read_file" => (read(project, path), true),
                    "search" => (search(project, args["text"].as_str().unwrap_or("")), true),
                    "write_file" if mode != Mode::Plan => {
                        let content = args["content"].as_str().unwrap_or("");
                        match inside(project, path) {
                            None => ("That path is outside the project.".into(), false),
                            Some(p) => {
                                let old = std::fs::read_to_string(&p).unwrap_or_default();
                                let answer = if always {
                                    Answer::Allow
                                } else {
                                    ask(app, id, &format!("edit {path}"), &preview(&old, content))
                                        .await
                                };
                                always |= answer == Answer::Always;
                                if answer == Answer::Deny {
                                    ("The user said no to this edit.".into(), false)
                                } else {
                                    if let Some(dir) = p.parent() {
                                        let _ = std::fs::create_dir_all(dir);
                                    }
                                    match std::fs::write(&p, content) {
                                        Ok(()) => ("Written.".into(), true),
                                        Err(e) => (format!("Could not write: {e}"), false),
                                    }
                                }
                            }
                        }
                    }
                    _ => ("No such tool.".into(), false),
                };
                emit(
                    app,
                    id,
                    json!({"kind":"step","id":sid,"state":if ok {"done"} else {"failed"}}),
                );
                messages.push(json!({"role":"tool","content":result}));
            }
        }
        if !done && error.is_none() {
            emit(app, id, json!({"kind":"stuck"}));
        }
        emit(app, id, json!({"kind":"turn","error":error}));
        let id = id.to_owned();
        tokio::task::spawn_blocking(move || turn_ended(&id));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_a_model_that_fits() {
        assert_eq!(fitting_model(8).0, "qwen2.5-coder:7b");
        assert_eq!(fitting_model(32).0, "qwen2.5-coder:14b");
    }

    #[test]
    fn stays_inside_the_project() {
        let p = Path::new("/p");
        assert!(inside(p, "src/a.rs").is_some());
        assert!(inside(p, "../etc/passwd").is_none());
        assert!(inside(p, "/etc/passwd").is_some_and(|x| x.starts_with(p)));
    }

    #[test]
    fn previews_only_what_changed() {
        assert_eq!(preview("a\nb\nc", "a\nB\nc"), "- b\n+ B");
    }
}
