//! Search over what Sidekick has seen (FR-RAG keyword part, FR-FILE-08):
//! downloads, screenshots, clipboard text, web pages from the extension,
//! Claude Code sessions, actions, Ask answers, and text files in folders
//! the user opts into. Everything stays in the local SQLite FTS5 index.
//! Secrets are never indexed (the clipboard sensor drops their text).

use std::path::Path;
use std::time::Duration;

use sidekick_core::{Event, SearchHit};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

/// Sources Claude Code may search through MCP (FR-RAG-11): never the
/// clipboard or page text, which can hold private material.
pub const SHAREABLE: &[&str] = &["file", "download", "screenshot", "action", "chat", "claude"];

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
        _ => None,
    };
    if let Some((source, reference, title, body)) = item {
        put(app, source, &reference, &title, &body, &ts);
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

// ---------- Semantic search (FR-RAG-03) ----------

/// The last embedding problem, for Settings > Search.
static EMBED_ERROR: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);
const EMBED_BATCH: u32 = 24;
const QUERY_TIMEOUT: Duration = Duration::from_secs(3);

fn embedder(app: &AppHandle) -> Option<(sidekick_ai::OpenAiCompat, String)> {
    let state = app.state::<AppState>();
    let s = lock(&state.settings);
    if !s.semantic_search.enabled {
        return None;
    }
    let client = sidekick_ai::OpenAiCompat::new(Some(s.ai.local.base_url.clone()), None);
    Some((client, s.semantic_search.model.clone()))
}

/// nomic-embed-text and similar models want a task prefix.
fn prefixed(model: &str, kind: &str, text: &str) -> String {
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
    sidekick_core::storage::fuse(&[keys, near], limit as usize)
        .into_iter()
        .filter_map(|(source, reference)| {
            keyword
                .iter()
                .find(|h| h.source == source && h.reference == reference)
                .cloned()
                .or_else(|| storage.hit(&source, &reference).ok().flatten())
        })
        .collect()
}

fn is_text_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| TEXT_EXTS.contains(&e.to_ascii_lowercase().as_str()))
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
                && entry.metadata().is_ok_and(|m| m.len() <= MAX_FILE_BYTES)
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
                let Ok(text) = tokio::fs::read_to_string(&path).await else {
                    continue;
                };
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ts = std::fs::metadata(&path)
                    .and_then(|m| m.modified())
                    .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339())
                    .unwrap_or_default();
                put(
                    &app,
                    "file",
                    &path.display().to_string(),
                    &name,
                    &clip(&text, 200_000),
                    &ts,
                );
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
