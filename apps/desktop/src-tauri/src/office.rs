//! The `office` tool: Outlook drafts, Excel ranges, Word documents and PDFs
//! through the desktop Office apps. Drafts only open (the user sends them);
//! writing into a workbook waits for a tap unless Excel is allowed in
//! Settings > Skills > Agent.

use serde_json::{Value, json};
use tauri::AppHandle;

use crate::act::{Gate, Risk, agent_settings, blocking, decide};
use sidekick_actions::office;

fn text(args: &Value, key: &str) -> String {
    args[key].as_str().unwrap_or_default().trim().to_owned()
}

pub async fn tool(app: &AppHandle, chat_id: &str, args: &Value) -> String {
    let path = text(args, "path");
    let out = match args["action"].as_str().unwrap_or_default() {
        "email_draft" => {
            let (to, subject, body) = (text(args, "to"), text(args, "subject"), text(args, "body"));
            let attach: Vec<String> = args["attach"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            blocking(move || office::email_draft(&to, &subject, &body, &attach))
                .await
                .map(|o| o.message)
        }
        "excel_read" => {
            let (sheet, range) = (text(args, "sheet"), text(args, "range"));
            blocking(move || office::excel_read(&path, &sheet, &range))
                .await
                .map(|s| s.describe())
        }
        "excel_write" => {
            let cmd = json!({
                "path": path,
                "sheet": text(args, "sheet"),
                "range": text(args, "range"),
                "values": args["values"].as_str().unwrap_or_default(),
            });
            match decide(&agent_settings(app), "excel", Risk::Outward) {
                Gate::Refuse => {
                    return "Error: the user keeps Sidekick out of Excel (Settings > Skills > Agent)."
                        .into();
                }
                Gate::Tap => {
                    let name = std::path::Path::new(&path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| "the workbook".into());
                    let label = format!("Write into {name} at {}", text(args, "range"));
                    crate::ask_tools::offer(app, chat_id, "excel_write", cmd, &label);
                    return format!(
                        "Not done yet: \"{label}\" is a button the user taps. Say in one \
                         sentence what will be written."
                    );
                }
                Gate::Run => write(&cmd).await,
            }
        }
        "word_create" => {
            let body = args["text"].as_str().unwrap_or_default().to_owned();
            blocking(move || office::word_create(&path, &body))
                .await
                .map(|o| o.message)
        }
        "to_pdf" => blocking(move || office::to_pdf(&path))
            .await
            .map(|o| o.message),
        other => Err(format!("unknown office action {other}")),
    };
    out.unwrap_or_else(|e| format!("Error: {e}"))
}

/// The tapped (or allowed) workbook write.
pub async fn write(cmd: &Value) -> Result<String, String> {
    run_write(cmd).await.map(|o| o.message)
}

pub async fn run_write(cmd: &Value) -> Result<sidekick_actions::Outcome, String> {
    let (path, sheet, range, values) = (
        text(cmd, "path"),
        text(cmd, "sheet"),
        text(cmd, "range"),
        cmd["values"].as_str().unwrap_or_default().to_owned(),
    );
    blocking(move || office::excel_write(&path, &sheet, &range, &values)).await
}
