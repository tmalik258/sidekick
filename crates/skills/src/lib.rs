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
];

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
