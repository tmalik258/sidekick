//! Voice: models, listening, speaking and the welcome lines.

use super::*;

#[tauri::command]
pub async fn voice_status(app: AppHandle) -> CmdResult<crate::voice::VoiceStatus> {
    // Reads model files and audio devices: off the UI thread.
    super::off_ui(move || crate::voice::status(&app)).await
}

#[tauri::command]
pub fn voice_download(app: AppHandle) -> CmdResult<()> {
    crate::voice::download(&app)
}

#[tauri::command]
pub fn voice_cancel_download(app: AppHandle) {
    crate::voice::cancel_download(&app);
}

#[tauri::command]
pub fn voice_listen(app: AppHandle) -> CmdResult<()> {
    crate::voice::listen(&app)
}

#[tauri::command]
pub fn voice_stop(app: AppHandle) {
    crate::voice::stop(&app);
}

/// The welcome line and when each part of it is heard.
#[tauri::command]
pub fn voice_welcome(app: AppHandle) -> crate::voice::WelcomeSpeech {
    crate::voice::welcome_speech(&app)
}

/// Speaks welcome step `step` (Next and Back in the welcome).
#[tauri::command]
pub fn voice_welcome_step(app: AppHandle, step: u32) {
    if !lock(&app.state::<AppState>().settings).onboarded {
        crate::voice::speak_welcome_step(&app, step);
    }
}

/// Says a short line, such as "Done. Composio is connected."
#[tauri::command]
pub fn voice_say(app: AppHandle, text: String) {
    let text: String = text.chars().take(200).collect();
    crate::voice::say_now(&app, &text);
}

/// Reads one answer aloud, markdown stripped. Stops whatever was speaking.
#[tauri::command]
pub fn voice_read(app: AppHandle, text: String) {
    crate::voice::stop(&app);
    let plain = plain_text(&text);
    let text: String = plain.chars().take(4000).collect();
    if !text.trim().is_empty() {
        crate::voice::say_now(&app, &text);
    }
}

/// Answer text as it should sound: no markdown marks, code or link targets.
fn plain_text(md: &str) -> String {
    let mut out = String::new();
    let mut fenced = false;
    for line in md.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        let line = line.trim_start_matches(|c: char| c == '#' || c == '>' || c.is_whitespace());
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        let mut s = String::new();
        let mut rest = line;
        // [label](url) reads as label.
        while let Some(i) = rest.find("](") {
            let Some(close) = rest[i..].find(')') else {
                break;
            };
            s.push_str(&rest[..i]);
            rest = &rest[i + close + 1..];
        }
        s.push_str(rest);
        let s: String = s
            .chars()
            .filter(|c| !matches!(c, '*' | '_' | '`' | '[' | '|'))
            .collect();
        if !s.trim().is_empty() {
            out.push_str(s.trim());
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn plain_text_drops_markdown() {
        let md = "## Hi\n- **Bold** item with [a link](https://x.y)\n```\ncode\n```\nDone.";
        assert_eq!(super::plain_text(md), "Hi\nBold item with a link\nDone.\n");
    }
}

/// Off the UI thread: it waits while a new voice loads.
#[tauri::command]
pub async fn voice_test(app: AppHandle) -> CmdResult<()> {
    tauri::async_runtime::spawn_blocking(move || crate::voice::test(&app))
        .await
        .map_err(|e| e.to_string())?
}
