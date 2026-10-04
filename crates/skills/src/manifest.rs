//! Skill manifests. A skill is data: what event it reacts to, which
//! payload fields must match, and the suggestion it offers.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Skill {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Higher runs first and its title wins when skills merge.
    #[serde(default = "default_priority")]
    pub priority: i32,
    /// Off until the user switches it on (noisy skills).
    #[serde(default = "yes")]
    pub enabled_by_default: bool,
    /// The same dedupe key does not fire again within this many seconds.
    #[serde(default)]
    pub cooldown_secs: u64,
    /// Template for the cooldown key. Defaults to the skill id.
    #[serde(default)]
    pub dedupe: Option<String>,
    #[serde(default)]
    pub trust: Trust,
    /// Template for the key under which the user's choices are remembered.
    #[serde(default)]
    pub remember: Option<String>,
    pub trigger: Trigger,
    pub suggestion: SuggestionTemplate,
}

fn default_priority() -> i32 {
    50
}

fn yes() -> bool {
    true
}

/// How much approval a skill's action needs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trust {
    /// Show chips and wait for a click.
    #[default]
    Suggest,
    /// Run the first option right away when it is a safe action.
    Auto,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Trigger {
    pub event: String,
    #[serde(default, rename = "where")]
    pub conditions: BTreeMap<String, Matcher>,
}

/// A test on one payload field. All set tests must pass.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Matcher {
    pub equals: Option<Value>,
    pub one_of: Option<Vec<Value>>,
    pub not_one_of: Option<Vec<Value>>,
    /// Regex on the field's text. Named groups become template variables.
    pub regex: Option<String>,
    /// Inclusive numeric range.
    pub range: Option<[f64; 2]>,
    pub exists: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuggestionTemplate {
    pub title: String,
    #[serde(default)]
    pub detail: String,
    pub options: Vec<OptionTemplate>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionTemplate {
    pub label: String,
    pub action: String,
    /// Argument templates, rendered against the event payload.
    #[serde(default)]
    pub args: BTreeMap<String, String>,
    /// Capabilities that must be present, for example `browser:zen`.
    #[serde(default)]
    pub requires: Vec<String>,
    /// Extra payload tests for this option only.
    #[serde(default)]
    pub when: BTreeMap<String, Matcher>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("{file}: {source}")]
    Yaml {
        file: String,
        source: serde_yaml::Error,
    },
    #[error("{file}: invalid regex for {field}: {source}")]
    Regex {
        file: String,
        field: String,
        source: regex::Error,
    },
}

impl Skill {
    pub fn parse(file: &str, yaml: &str) -> Result<Self, ManifestError> {
        let skill: Skill = serde_yaml::from_str(yaml).map_err(|source| ManifestError::Yaml {
            file: file.to_string(),
            source,
        })?;
        let regexes = skill
            .trigger
            .conditions
            .iter()
            .chain(skill.suggestion.options.iter().flat_map(|o| o.when.iter()));
        for (field, m) in regexes {
            if let Some(re) = &m.regex {
                regex::Regex::new(re).map_err(|source| ManifestError::Regex {
                    file: file.to_string(),
                    field: field.clone(),
                    source,
                })?;
            }
        }
        Ok(skill)
    }
}
