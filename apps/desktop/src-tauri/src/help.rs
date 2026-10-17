//! Help about Duskie itself, grounded in where things really are, so an
//! answer can say "Settings > Appearance" instead of guessing.

/// Topic words, then where it lives and what to do.
const GUIDE: &[(&[&str], &str)] = &[
    (
        &["look", "colour", "color", "theme", "glass", "transparen"],
        "Look and colours: Settings > Appearance (island colour, mascot colour).",
    ),
    (
        &[
            "start",
            "startup",
            "launch",
            "login",
            "quiet",
            "focus",
            "do not disturb",
            "dnd",
        ],
        "Starting with Windows, quiet hours and Focus: Settings > General.",
    ),
    (
        &[
            "model", "claude", "codex", "ollama", "local", "ai", "brain", "api key",
        ],
        "Which AI answers and which writes code: Settings > AI models.",
    ),
    (
        &[
            "voice",
            "wake",
            "hey duskie",
            "speak",
            "mic",
            "microphone",
            "live",
            "caption",
        ],
        "Voice, wake word, Live answers and Live history: Settings > AI models > Voice.",
    ),
    (
        &["memory", "remember", "forget", "knows"],
        "What Duskie remembers about you, all editable: Settings > Memory.",
    ),
    (
        &["skill", "automation", "library", "sync"],
        "Skills, automations and the skills library with Sync now: Settings > Skills and automations.",
    ),
    (
        &[
            "gmail", "mail", "connect", "composio", "app", "calendar", "mcp", "server",
        ],
        "Connected accounts and MCP servers: Settings > Apps.",
    ),
    (
        &["shortcut", "hotkey", "keys", "alt"],
        "Keyboard shortcuts (click one, then press the new keys): Settings > Shortcuts.",
    ),
    (
        &["privacy", "screen", "notice", "never", "data", "audit"],
        "What Duskie notices and never looks at: Settings > Privacy. A local audit log keeps only which tool ran.",
    ),
    (
        &["space", "disk", "storage", "full drive"],
        "What takes space on this PC: Settings > Storage.",
    ),
    (
        &["editor", "terminal", "code", "file"],
        "Editor and terminal: open a project in Agents > Git, then Full screen; Ctrl P opens a file, Ctrl ` the terminal.",
    ),
    (
        &["update", "version"],
        "Updates show on the island when one is ready; hover it and pick Update.",
    ),
];

const CRASH_WORDS: &[&str] = &[
    "crash",
    "error",
    "broken",
    "not working",
    "stuck",
    "freeze",
    "slow",
    "problem",
    "bug",
    "fail",
];

/// Lines of the guide that fit `topic`; all of them when nothing fits.
pub fn guide(topic: &str) -> String {
    let t = topic.to_lowercase();
    let hits: Vec<&str> = GUIDE
        .iter()
        .filter(|(words, _)| words.iter().any(|w| t.contains(w)))
        .map(|(_, line)| *line)
        .collect();
    if hits.is_empty() {
        GUIDE.iter().map(|(_, l)| *l).collect::<Vec<_>>().join("\n")
    } else {
        hits.join("\n")
    }
}

/// Whether the question is about something going wrong.
pub fn is_problem(topic: &str) -> bool {
    let t = topic.to_lowercase();
    CRASH_WORDS.iter().any(|w| t.contains(w))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_to_the_right_place() {
        assert!(guide("how do I change the mascot colour").contains("Settings > Appearance"));
        assert!(guide("turn off the wake word").contains("Voice"));
        assert!(guide("xyzzy").lines().count() >= GUIDE.len());
        assert!(is_problem("Duskie keeps crashing"));
        assert!(!is_problem("change my theme"));
    }
}
