//! Skills turn events into suggestions without any AI (tier T0).
//!
//! - [`manifest`]: the YAML skill format (SRS 9.1).
//! - [`engine`]: matching, merging, ranking and cooldowns.
//! - [`template`]: `{{field}}` rendering.

pub mod engine;
pub mod manifest;
pub mod template;

use std::collections::BTreeMap;
use std::path::Path;

pub use engine::{Engine, Env, MAX_OPTIONS, Proposal, ProposedOption};
pub use manifest::{ManifestError, Skill, Trust};

/// Skills compiled into the app, from the repository's `skills/` folder.
const BUILTIN: &[(&str, &str)] = &[
    (
        "files/download.yaml",
        include_str!("../../../skills/files/download.yaml"),
    ),
    (
        "files/copy.yaml",
        include_str!("../../../skills/files/copy.yaml"),
    ),
    (
        "files/convert-image.yaml",
        include_str!("../../../skills/files/convert-image.yaml"),
    ),
    (
        "files/convert-media.yaml",
        include_str!("../../../skills/files/convert-media.yaml"),
    ),
    (
        "files/convert-document.yaml",
        include_str!("../../../skills/files/convert-document.yaml"),
    ),
    (
        "files/archive.yaml",
        include_str!("../../../skills/files/archive.yaml"),
    ),
    (
        "files/installer.yaml",
        include_str!("../../../skills/files/installer.yaml"),
    ),
    (
        "dev/open-in-browser.yaml",
        include_str!("../../../skills/dev/open-in-browser.yaml"),
    ),
    (
        "dev/port-conflict.yaml",
        include_str!("../../../skills/dev/port-conflict.yaml"),
    ),
    (
        "clipboard/secret-guard.yaml",
        include_str!("../../../skills/clipboard/secret-guard.yaml"),
    ),
    (
        "clipboard/open-url.yaml",
        include_str!("../../../skills/clipboard/open-url.yaml"),
    ),
    (
        "clipboard/format-json.yaml",
        include_str!("../../../skills/clipboard/format-json.yaml"),
    ),
    (
        "dev/explain-error.yaml",
        include_str!("../../../skills/dev/explain-error.yaml"),
    ),
    (
        "system/disk-low.yaml",
        include_str!("../../../skills/system/disk-low.yaml"),
    ),
    (
        "system/memory-high.yaml",
        include_str!("../../../skills/system/memory-high.yaml"),
    ),
    (
        "dev/claude-finished.yaml",
        include_str!("../../../skills/dev/claude-finished.yaml"),
    ),
    (
        "dev/claude-needs-you.yaml",
        include_str!("../../../skills/dev/claude-needs-you.yaml"),
    ),
    (
        "browser/login.yaml",
        include_str!("../../../skills/browser/login.yaml"),
    ),
    (
        "browser/many-tabs.yaml",
        include_str!("../../../skills/browser/many-tabs.yaml"),
    ),
    (
        "browser/upwork-job.yaml",
        include_str!("../../../skills/browser/upwork-job.yaml"),
    ),
    (
        "browser/long-read.yaml",
        include_str!("../../../skills/browser/long-read.yaml"),
    ),
    (
        "files/screenshot.yaml",
        include_str!("../../../skills/files/screenshot.yaml"),
    ),
    (
        "dev/unsaved-work.yaml",
        include_str!("../../../skills/dev/unsaved-work.yaml"),
    ),
    (
        "system/day-summary.yaml",
        include_str!("../../../skills/system/day-summary.yaml"),
    ),
    (
        "calendar/meeting-soon.yaml",
        include_str!("../../../skills/calendar/meeting-soon.yaml"),
    ),
    (
        "calendar/follow-up.yaml",
        include_str!("../../../skills/calendar/follow-up.yaml"),
    ),
    (
        "system/morning-brief.yaml",
        include_str!("../../../skills/system/morning-brief.yaml"),
    ),
    (
        "system/focus.yaml",
        include_str!("../../../skills/system/focus.yaml"),
    ),
];

/// Every action a skill may name. Keep in sync with the executor and the
/// app-level actions (`ask_ai`, `browser_*`).
pub const ACTIONS: &[&str] = &[
    "open_path",
    "open_folder",
    "extract_text",
    "reveal_path",
    "copy_file",
    "copy_text",
    "open_url",
    "open_in_editor",
    "open_system_page",
    "convert",
    "extract_archive",
    "run_installer",
    "kill_port",
    "clear_clipboard_later",
    "format_json_clipboard",
    "ask_ai",
    "fathom_followup",
    "browser_fill",
    "browser_close_duplicates",
    "browser_save_session",
    "noop",
];

/// The skill format, for people and for AI writing skills.
pub const FORMAT_GUIDE: &str = include_str!("../../../skills/README.md");

/// Checks a skill written by the user or by AI before it is installed:
/// valid YAML, a plain id that does not replace a built-in, only known
/// actions, and Suggest trust (Auto is something the user turns on).
pub fn validate_new(yaml: &str) -> Result<Skill, String> {
    let skill = Skill::parse("new skill", yaml).map_err(|e| e.to_string())?;
    let id_ok = skill.id.len() >= 3
        && skill.id.len() <= 64
        && skill
            .id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '-'))
        && skill.id.starts_with(|c: char| c.is_ascii_lowercase());
    if !id_ok {
        return Err(format!(
            "id {:?} must be lowercase letters, digits, dots or dashes",
            skill.id
        ));
    }
    if builtin().iter().any(|b| b.id == skill.id) {
        return Err(format!("{} is a built-in skill; pick another id", skill.id));
    }
    if skill.trust == Trust::Auto {
        return Err(
            "new skills start at Suggest; switch on Auto in Settings if you want it".into(),
        );
    }
    if skill.suggestion.options.is_empty() {
        return Err("the skill has no options".into());
    }
    if let Some(o) = skill
        .suggestion
        .options
        .iter()
        .find(|o| !ACTIONS.contains(&o.action.as_str()))
    {
        return Err(format!("unknown action {} in option {}", o.action, o.label));
    }
    Ok(skill)
}

pub fn builtin() -> Vec<Skill> {
    BUILTIN
        .iter()
        .filter_map(|(file, yaml)| match Skill::parse(file, yaml) {
            Ok(s) => Some(s),
            Err(err) => {
                log::error!("built-in skill failed to load: {err}");
                None
            }
        })
        .collect()
}

/// Built-ins plus the user's own skills (FR-SKL-01). A user skill with the
/// same id replaces the built-in one. Broken files are skipped and reported.
pub fn load_all(user_dir: &Path) -> (Vec<Skill>, Vec<String>) {
    let mut by_id: BTreeMap<String, Skill> =
        builtin().into_iter().map(|s| (s.id.clone(), s)).collect();
    let mut errors = Vec::new();
    if let Ok(entries) = std::fs::read_dir(user_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let is_yaml = path.extension().is_some_and(|e| e == "yaml" || e == "yml");
            if !is_yaml {
                continue;
            }
            let file = path.display().to_string();
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|yaml| Skill::parse(&file, &yaml).map_err(|e| e.to_string()))
            {
                Ok(skill) => {
                    by_id.insert(skill.id.clone(), skill);
                }
                Err(err) => errors.push(err),
            }
        }
    }
    (by_id.into_values().collect(), errors)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_skill_parses() {
        assert_eq!(builtin().len(), BUILTIN.len());
    }

    #[test]
    fn builtin_skills_only_use_known_actions() {
        for s in builtin() {
            for o in &s.suggestion.options {
                assert!(
                    ACTIONS.contains(&o.action.as_str()),
                    "{}: {}",
                    s.id,
                    o.action
                );
            }
        }
    }

    #[test]
    fn new_skills_are_checked() {
        let ok = "id: my.screenshots\nname: Screens\ntrigger: { event: file.download_completed }\nsuggestion: { title: t, options: [ { label: Open, action: open_path, args: { path: \"{{path}}\" } } ] }\n";
        assert_eq!(validate_new(ok).unwrap().id, "my.screenshots");
        let bad_action = ok.replace("open_path", "run_shell");
        assert!(
            validate_new(&bad_action)
                .unwrap_err()
                .contains("unknown action")
        );
        let builtin_id = ok.replace("my.screenshots", "files.download");
        assert!(validate_new(&builtin_id).unwrap_err().contains("built-in"));
        let auto = format!("{ok}trust: auto\n");
        assert!(validate_new(&auto).unwrap_err().contains("Suggest"));
        let bad_id = ok.replace("my.screenshots", "../../evil");
        assert!(validate_new(&bad_id).is_err());
        assert!(validate_new("not: [yaml").is_err());
    }

    #[test]
    fn user_skills_override_builtins_and_bad_files_are_reported() {
        let dir = std::env::temp_dir().join(format!("sidekick-skills-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mine.yaml"),
            "id: files.copy\nname: My copy\ntrigger: { event: x }\nsuggestion: { title: t, options: [ { label: a, action: b } ] }\n",
        )
        .unwrap();
        std::fs::write(dir.join("broken.yaml"), "id: [").unwrap();
        let (skills, errors) = load_all(&dir);
        assert_eq!(
            skills.iter().find(|s| s.id == "files.copy").unwrap().name,
            "My copy"
        );
        assert_eq!(errors.len(), 1);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
