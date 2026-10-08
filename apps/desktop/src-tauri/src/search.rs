//! Search over what Sidekick has seen:
//! downloads, screenshots, clipboard text, web pages from the extension,
//! Claude Code sessions, actions, Ask answers, and text files in folders
//! the user opts into. Everything stays in the local SQLite FTS5 index.
//! Secrets are never indexed (the clipboard sensor drops their text).

use std::path::Path;
use std::time::Duration;

use sidekick_core::{Event, SearchHit};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// Sources Claude Code may search through MCP: never the
/// clipboard or page text, which can hold private material.
pub const SHAREABLE: &[&str] = &["file", "download", "screenshot", "action", "chat", "claude"];

/// Sources the local model may search in Ask: everything, since nothing
/// it reads leaves this PC.
pub const LOCAL: &[&str] = &[
    "file",
    "download",
    "screenshot",
    "action",
    "chat",
    "claude",
    "clipboard",
    "page",
    "meeting",
];

/// Clipboard items kept.
const CLIPBOARD_HISTORY: u32 = 500;
const MAX_FILE_BYTES: u64 = 1_000_000;
const MAX_FILES: usize = 20_000;
const MAX_DEPTH: usize = 8;
const TEXT_EXTS: &[&str] = &[
    "txt",
    "md",
    "markdown",
    "csv",
    "tsv",
    "json",
    "log",
    "yaml",
    "yml",
    "toml",
    "ini",
    "py",
    "rs",
    "ts",
    "tsx",
    "js",
    "jsx",
    "html",
    "css",
    "sql",
    "sh",
    "ps1",
    "env.example",
];
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".next",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
];

fn clip(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

/// Indexes the parts of an event worth finding later.
pub fn index_event(app: &AppHandle, e: &Event) {
    let p = &e.payload;
    let s = |k: &str| p[k].as_str().unwrap_or_default();
    let ts = e.ts.to_rfc3339();
    let item: Option<(&str, String, String, String)> = match e.kind.as_str() {
        "file.download_completed" => Some((
            "download",
            s("path").into(),
            s("name").into(),
            format!("{} {}", s("name"), s("dir")),
        )),
        "file.screenshot" => Some((
            "screenshot",
            s("path").into(),
            s("name").into(),
            s("name").into(),
        )),
        "clipboard.changed" if s("kind") != "secret" && !s("text").is_empty() => Some((
            "clipboard",
            e.id.to_string(),
            clip(s("preview"), 80),
            clip(s("text"), 4_000),
        )),
        "browser.long_read" | "browser.upwork_job" => Some((
            "page",
            s("url").into(),
            s("title").into(),
            format!("{} {}", s("url"), clip(s("text"), 20_000)),
        )),
        "claude.stop" | "claude.notification" => Some((
            "claude",
            format!("{}:{}", s("session"), ts),
            format!("Claude Code in {}", s("project")),
            format!("{} {}", s("message"), s("cwd")),
        )),
        "codex.stop" => Some((
            "claude",
            format!("{}:{}", s("session"), ts),
            format!("Codex in {}", s("project")),
            format!("{} {}", s("message"), s("cwd")),
        )),
        _ => None,
    };
    if let Some((source, reference, title, body)) = item {
        put(app, source, &reference, &title, &body, &ts);
        if source == "clipboard" {
            let result = lock(&app.state::<AppState>().storage).trim_source(
                source,
                CLIPBOARD_HISTORY,
                Some(&body),
            );
            if let Err(err) = result {
                log::warn!("could not trim clipboard history: {err}");
            }
        }
    }
}

pub fn put(app: &AppHandle, source: &str, reference: &str, title: &str, body: &str, ts: &str) {
    if let Err(err) =
        lock(&app.state::<AppState>().storage).index(source, reference, title, body, ts)
    {
        log::warn!("could not index {source}: {err}");
    }
}

pub fn search(app: &AppHandle, query: &str, sources: &[&str], limit: u32) -> Vec<SearchHit> {
    lock(&app.state::<AppState>().storage)
        .search(query, sources, limit)
        .unwrap_or_default()
}

// ---------- Semantic search ----------

/// The last embedding problem, for Settings > Search.
static EMBED_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
const EMBED_BATCH: u32 = 24;
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);

pub(crate) fn embedder(app: &AppHandle) -> Option<(sidekick_ai::OpenAiCompat, String)> {
    let state = app.state::<AppState>();
    let s = lock(&state.settings);
    if !s.semantic_search.enabled {
        return None;
    }
    let client = sidekick_ai::OpenAiCompat::new(Some(s.ai.local.base_url.clone()), None);
    Some((client, s.semantic_search.model.clone()))
}

/// nomic-embed-text and similar models want a task prefix.
pub(crate) fn prefixed(model: &str, kind: &str, text: &str) -> String {
    if model.contains("nomic") {
        format!("{kind}: {text}")
    } else {
        text.to_owned()
    }
}

pub fn embed_error() -> Option<String> {
    EMBED_ERROR.lock().ok().and_then(|e| e.clone())
}

fn set_embed_error(err: Option<String>) {
    if let Ok(mut e) = EMBED_ERROR.lock() {
        *e = err;
    }
}

/// Embeds indexed items in the background, a batch at a time.
pub fn start_embedder(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            // Embedding is heavy: only while plugged in and not paused.
            if crate::state::is_paused(&app) || !sidekick_sensors::on_mains() {
                tokio::time::sleep(Duration::from_secs(60)).await;
                continue;
            }
            let pause = match embed_batch(&app).await {
                Ok(true) => Duration::from_secs(1),
                Ok(false) => Duration::from_secs(60),
                Err(err) => {
                    log::debug!("embeddings: {err}");
                    set_embed_error(Some(err));
                    Duration::from_secs(600)
                }
            };
            tokio::time::sleep(pause).await;
        }
    });
}

/// True when a batch was embedded (more may be waiting).
async fn embed_batch(app: &AppHandle) -> Result<bool, String> {
    let Some((client, model)) = embedder(app) else {
        return Ok(false);
    };
    let todo = lock(&app.state::<AppState>().storage)
        .unembedded(&model, EMBED_BATCH)
        .map_err(|e| e.to_string())?;
    if todo.is_empty() {
        return Ok(false);
    }
    let inputs: Vec<String> = todo
        .iter()
        .map(|(_, _, text)| prefixed(&model, "search_document", text))
        .collect();
    let vectors = client
        .embed(&model, &inputs)
        .await
        .map_err(|e| format!("{e}. Run: ollama pull {model}"))?;
    let state = app.state::<AppState>();
    let storage = lock(&state.storage);
    for ((source, reference, _), vec) in todo.iter().zip(vectors) {
        storage
            .save_vector(source, reference, &model, &vec)
            .map_err(|e| e.to_string())?;
    }
    set_embed_error(None);
    Ok(true)
}

pub fn embedded_count(app: &AppHandle) -> u64 {
    let Some((_, model)) = embedder(app) else {
        return 0;
    };
    lock(&app.state::<AppState>().storage)
        .vector_count(&model)
        .unwrap_or(0)
}

/// Keyword and meaning together: keyword hits plus items close in meaning,
/// merged by rank. Falls back to keywords alone when no embedding model is
/// reachable.
pub async fn hybrid(app: &AppHandle, query: &str, sources: &[&str], limit: u32) -> Vec<SearchHit> {
    let keyword = search(app, query, sources, limit * 2);
    let Some((client, model)) = embedder(app) else {
        return keyword.into_iter().take(limit as usize).collect();
    };
    if query.trim().is_empty() || embedded_count(app) == 0 {
        return keyword.into_iter().take(limit as usize).collect();
    }
    let input = vec![prefixed(&model, "search_query", query.trim())];
    let Ok(Ok(mut q)) = tokio::time::timeout(QUERY_TIMEOUT, client.embed(&model, &input)).await
    else {
        return keyword.into_iter().take(limit as usize).collect();
    };
    let Some(qv) = q.pop() else {
        return keyword;
    };
    let state = app.state::<AppState>();
    let storage = lock(&state.storage);
    let near: Vec<(String, String)> = storage
        .nearest(&qv, &model, sources, (limit * 2) as usize)
        .unwrap_or_default()
        .into_iter()
        // Weak matches are noise.
        .filter(|(_, _, score)| *score >= 0.45)
        .map(|(s, r, _)| (s, r))
        .collect();
    let keys: Vec<(String, String)> = keyword
        .iter()
        .map(|h| (h.source.clone(), h.reference.clone()))
        .collect();
    let mut out: Vec<SearchHit> = sidekick_core::storage::fuse(&[keys, near], limit as usize * 2)
        .into_iter()
        .filter_map(|(source, reference)| {
            keyword
                .iter()
                .find(|h| h.source == source && h.reference == reference)
                .cloned()
                .or_else(|| storage.hit(&source, &reference).ok().flatten())
        })
        .collect();
    // One row per file: its best passage, under the file's own path.
    let mut seen = std::collections::HashSet::new();
    out.retain_mut(|h| {
        if h.source == "file" {
            h.reference = file_of(&h.reference).to_owned();
        }
        seen.insert((h.source.clone(), h.reference.clone()))
    });
    out.truncate(limit as usize);
    out
}

/// Documents read through a converter (pdftotext, pandoc) when installed.
const DOC_EXTS: &[&str] = &["pdf", "docx", "odt", "rtf"];
/// Documents can be bigger than plain text files.
const MAX_DOC_BYTES: u64 = 30_000_000;
/// A passage: about a paragraph or two, what one embedding describes well.
const PASSAGE: usize = 1200;
const MAX_PASSAGES: usize = 60;

fn ext(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .unwrap_or_default()
}

fn is_text_file(path: &Path) -> bool {
    let e = ext(path);
    TEXT_EXTS.contains(&e.as_str()) || DOC_EXTS.contains(&e.as_str())
}

/// Splits text into passages of about `PASSAGE` characters, breaking at
/// blank lines, then line ends, so each one reads on its own.
pub fn passages(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut cur = String::new();
    for para in text.split("\n\n") {
        let para = para.trim();
        if para.is_empty() {
            continue;
        }
        if !cur.is_empty() && cur.len() + para.len() > PASSAGE {
            out.push(std::mem::take(&mut cur));
        }
        if para.len() > PASSAGE {
            // One long block: cut at line ends, or at a char boundary.
            for line in para.lines() {
                if !cur.is_empty() && cur.len() + line.len() > PASSAGE {
                    out.push(std::mem::take(&mut cur));
                }
                let mut rest = line;
                while rest.len() > PASSAGE {
                    let mut cut = PASSAGE;
                    while !rest.is_char_boundary(cut) {
                        cut -= 1;
                    }
                    out.push(rest[..cut].to_owned());
                    rest = &rest[cut..];
                }
                cur.push_str(rest);
                cur.push('\n');
            }
        } else {
            cur.push_str(para);
            cur.push_str("\n\n");
        }
        if out.len() >= MAX_PASSAGES {
            break;
        }
    }
    if !cur.trim().is_empty() && out.len() < MAX_PASSAGES {
        out.push(cur);
    }
    out.into_iter()
        .map(|p| p.trim().to_owned())
        .filter(|p| !p.is_empty())
        .collect()
}

/// A file passage's reference is "path#p3"; the file itself is the path.
pub fn file_of(reference: &str) -> &str {
    match reference.rfind("#p") {
        Some(i)
            if reference[i + 2..].chars().all(|c| c.is_ascii_digit())
                && i + 2 < reference.len() =>
        {
            &reference[..i]
        }
        _ => reference,
    }
}

/// Text files under `root`, skipping build output and dependency folders.
pub fn text_files(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if path.is_dir() {
                if depth < MAX_DEPTH
                    && !name.starts_with('.')
                    && !SKIP_DIRS.contains(&name.as_str())
                {
                    stack.push((path, depth + 1));
                }
            } else if is_text_file(&path)
                && entry.metadata().is_ok_and(|m| {
                    m.len()
                        <= if DOC_EXTS.contains(&ext(&path).as_str()) {
                            MAX_DOC_BYTES
                        } else {
                            MAX_FILE_BYTES
                        }
                })
            {
                out.push(path);
                if out.len() >= MAX_FILES {
                    return out;
                }
            }
        }
    }
    out
}

/// Re-indexes the opted-in folders in the background. Returns at once.
pub fn reindex_folders(app: &AppHandle) {
    let folders = lock(&app.state::<AppState>().settings)
        .index_folders
        .clone();
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // Let startup finish before reading files.
        tokio::time::sleep(Duration::from_secs(10)).await;
        let _ = lock(&app.state::<AppState>().storage).clear_search(Some("file"));
        let mut count = 0usize;
        for folder in folders {
            let files = tokio::task::spawn_blocking({
                let f = folder.clone();
                move || text_files(Path::new(&f))
            })
            .await
            .unwrap_or_default();
            for path in files {
                let text = if DOC_EXTS.contains(&ext(&path).as_str()) {
                    let (handle, p) = (app.clone(), path.clone());
                    match tokio::task::spawn_blocking(move || crate::files::text_of(&handle, &p))
                        .await
                    {
                        Ok(Ok(t)) => t,
                        // No converter installed, or an unreadable file.
                        _ => continue,
                    }
                } else {
                    let Ok(t) = tokio::fs::read_to_string(&path).await else {
                        continue;
                    };
                    t
                };
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ts = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
                    .unwrap_or_default();
                // Each passage is searched and embedded on its own, so a
                // question finds the part of a long file that answers it.
                let at = path.display().to_string();
                for (i, part) in passages(&clip(&text, 200_000)).iter().enumerate() {
                    let reference = if i == 0 {
                        at.clone()
                    } else {
                        format!("{at}#p{i}")
                    };
                    put(&app, "file", &reference, &name, part, &ts);
                }
                count += 1;
                // Stay gentle on the disk and CPU.
                if count.is_multiple_of(50) {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                }
            }
        }
        if count > 0 {
            log::info!("indexed {count} files for search");
        }
    });
}

/// Indexes a finished Ask answer.
pub fn index_chat(app: &AppHandle, id: &str, question: &str, answer: &str) {
    put(
        app,
        "chat",
        id,
        &clip(question, 120),
        &format!("{question}\n{answer}"),
        &chrono::Utc::now().to_rfc3339(),
    );
}

pub fn index_action(app: &AppHandle, label: &str, message: &str, skill: &str, ts: &str) {
    put(
        app,
        "action",
        &format!("{skill}:{ts}"),
        label,
        &format!("{label} {message} {skill}"),
        ts,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_long_text_into_passages() {
        let para = "The landlord will return the deposit within 30 days. ".repeat(10);
        let text = format!("{para}\n\n{para}\n\n{para}");
        let parts = passages(&text);
        assert!(parts.len() >= 2);
        assert!(parts.iter().all(|p| p.len() <= PASSAGE + 2));
        assert!(passages("").is_empty());
        let long = "x".repeat(5000);
        assert!(passages(&long).iter().all(|p| p.len() <= PASSAGE));
    }

    #[test]
    fn passage_references_point_at_the_file() {
        assert_eq!(file_of(r"C:\docs\lease.pdf#p3"), r"C:\docs\lease.pdf");
        assert_eq!(file_of(r"C:\docs\lease.pdf"), r"C:\docs\lease.pdf");
        assert_eq!(file_of(r"C:\a#pages\b.md"), r"C:\a#pages\b.md");
    }

    #[test]
    fn walks_text_files_and_skips_build_folders() {
        let root = std::env::temp_dir().join(format!("sidekick-index-{}", std::process::id()));
        std::fs::create_dir_all(root.join("notes")).unwrap();
        std::fs::create_dir_all(root.join("node_modules/pkg")).unwrap();
        std::fs::write(root.join("notes/a.md"), "hello").unwrap();
        std::fs::write(root.join("b.txt"), "hi").unwrap();
        std::fs::write(root.join("photo.png"), [0u8; 4]).unwrap();
        std::fs::write(root.join("node_modules/pkg/index.js"), "x").unwrap();
        let mut files: Vec<String> = text_files(&root)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        files.sort();
        assert_eq!(files, ["a.md", "b.txt"]);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn shareable_sources_leave_out_private_ones() {
        assert!(!SHAREABLE.contains(&"clipboard"));
        assert!(!SHAREABLE.contains(&"page"));
    }
}
