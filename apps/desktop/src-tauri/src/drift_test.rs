//! Guards against the UI and the backend drifting apart: every command the
//! UI calls must be registered with Tauri and have a browser-preview mock,
//! so a rename fails here instead of failing silently at runtime.

use std::collections::BTreeSet;

const BRIDGE: &str = include_str!("../../src/lib/bridge.ts");
const MOCK: &str = include_str!("../../src/lib/mock.ts");
const LIB: &str = include_str!("lib.rs");

/// Names between `marker"` and the next `"`.
fn quoted_after(text: &str, marker: &str) -> BTreeSet<String> {
    text.match_indices(marker)
        .filter_map(|(at, _)| {
            let rest = &text[at + marker.len()..];
            rest.split('"').next().map(str::to_owned)
        })
        .filter(|n| {
            !n.is_empty()
                && n.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit())
        })
        .collect()
}

fn invoked() -> BTreeSet<String> {
    let mut names = quoted_after(BRIDGE, "invoke(\"");
    for (at, _) in BRIDGE.match_indices("invoke<") {
        if let Some(open) = BRIDGE[at..].find(">(\"") {
            let rest = &BRIDGE[at + open + 3..];
            if let Some(name) = rest.split('"').next() {
                names.insert(name.to_owned());
            }
        }
    }
    names
}

#[test]
fn every_ui_command_is_registered() {
    let registered: BTreeSet<String> = LIB
        .match_indices("commands::")
        .map(|(at, _)| {
            LIB[at + "commands::".len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect()
        })
        .collect();
    let missing: Vec<_> = invoked()
        .into_iter()
        .filter(|n| !registered.contains(n))
        .collect();
    assert!(
        missing.is_empty(),
        "the UI calls commands that are not registered: {missing:?}"
    );
}

#[test]
fn every_ui_command_has_a_preview_mock() {
    let missing: Vec<_> = invoked()
        .into_iter()
        .filter(|n| !MOCK.contains(&format!("commands.{n} ")) && !MOCK.contains(&format!("  {n}:")))
        .collect();
    assert!(
        missing.is_empty(),
        "the browser preview has no mock for: {missing:?}"
    );
}
