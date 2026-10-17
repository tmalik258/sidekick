//! The editor's file access, all confined to the project folder.

use crate::workspace::{self, Hit, OpenFile, TreeEntry};

use super::*;

fn e(err: std::io::Error) -> String {
    err.to_string()
}

#[tauri::command]
pub async fn editor_tree(root: String) -> CmdResult<Vec<TreeEntry>> {
    off_ui(move || workspace::tree(&workspace::root_of(&root).map_err(e)?).map_err(e)).await?
}

#[tauri::command]
pub async fn editor_read(root: String, path: String) -> CmdResult<OpenFile> {
    off_ui(move || workspace::read(&workspace::root_of(&root).map_err(e)?, &path).map_err(e))
        .await?
}

/// Saves when the file is unchanged since `stamp`; returns the new stamp.
#[tauri::command]
pub async fn editor_write(
    root: String,
    path: String,
    text: String,
    stamp: String,
) -> CmdResult<String> {
    off_ui(move || {
        workspace::write(&workspace::root_of(&root).map_err(e)?, &path, &text, &stamp).map_err(e)
    })
    .await?
}

#[tauri::command]
pub async fn editor_changed(root: String, path: String, stamp: String) -> CmdResult<bool> {
    off_ui(move || {
        workspace::changed(&workspace::root_of(&root).map_err(e)?, &path, &stamp).map_err(e)
    })
    .await?
}

#[tauri::command]
pub async fn editor_head(root: String, path: String) -> CmdResult<String> {
    off_ui(move || workspace::head(&workspace::root_of(&root).map_err(e)?, &path).map_err(e))
        .await?
}

#[tauri::command]
pub async fn editor_search(root: String, query: String) -> CmdResult<Vec<Hit>> {
    off_ui(move || workspace::search(&workspace::root_of(&root).map_err(e)?, &query).map_err(e))
        .await?
}

pub const TERM_EVENT: &str = "term://data";

/// Starts a terminal in the project folder; output arrives as TERM_EVENT.
#[tauri::command]
pub fn term_open(app: AppHandle, root: String, cols: u16, rows: u16) -> CmdResult<u32> {
    let dir = workspace::root_of(&root).map_err(e)?;
    crate::pty::open(&dir, cols, rows, move |id, data| {
        let _ = app.emit(
            TERM_EVENT,
            serde_json::json!({ "id": id, "data": data, "done": data.is_none() }),
        );
    })
}

#[tauri::command]
pub fn term_write(id: u32, data: String) -> CmdResult<()> {
    crate::pty::write(id, &data)
}

#[tauri::command]
pub fn term_resize(id: u32, cols: u16, rows: u16) -> CmdResult<()> {
    crate::pty::resize(id, cols, rows)
}

#[tauri::command]
pub fn term_close(id: u32) {
    crate::pty::close(id);
}
