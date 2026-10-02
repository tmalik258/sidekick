//! The rule engine (SRS 4.2): match an event against every enabled skill,
//! merge the matches into one proposal, rank options by what the user picked
//! before, and drop options whose requirements are missing.

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

use regex::Regex;
use serde::Serialize;
use serde_json::Value;
use sidekick_core::Event;

use crate::manifest::{Matcher, Skill, Trust};
use crate::template::{Vars, render, vars_from};

/// Most chips shown at once.
pub const MAX_OPTIONS: usize = 5;

/// What the app knows that skills depend on.
pub trait Env {
    /// True when a capability such as `browser:zen` or `tool:ffmpeg` exists.
    fn has(&self, requirement: &str) -> bool;
    fn skill_enabled(&self, skill: &Skill) -> bool;
    /// The user's trust override for a skill, if any.
    fn trust_override(&self, skill_id: &str) -> Option<Trust>;
    /// How often each option label was chosen under a preference key.
    fn choice_counts(&self, key: &str) -> HashMap<String, u32>;
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProposedOption {
    pub label: String,
    pub action: String,
    pub args: Value,
    pub skill_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Proposal {
    pub skill_id: String,
    pub skill_ids: Vec<String>,
    pub title: String,
    pub detail: String,
    pub options: Vec<ProposedOption>,
    pub trust: Trust,
    /// Where the user's choice is remembered, if the skill learns.
    pub remember: Option<String>,
    /// The skill's priority (higher is more urgent); low ones can wait
    /// quietly and are not read aloud.
    pub priority: i32,
}

pub struct Engine {
    skills: Vec<Skill>,
    regexes: HashMap<String, Regex>,
    fired: HashMap<String, Instant>,
}

impl Engine {
    pub fn new(mut skills: Vec<Skill>) -> Self {
        skills.sort_by(|a, b| b.priority.cmp(&a.priority).then_with(|| a.id.cmp(&b.id)));
        let mut regexes = HashMap::new();
        for skill in &skills {
            let all = skill.trigger.conditions.values().chain(
                skill
                    .suggestion
                    .options
                    .iter()
                    .flat_map(|o| o.when.values()),
            );
            for m in all {
                if let Some(re) = &m.regex
                    && let Ok(compiled) = Regex::new(re)
                {
                    regexes.insert(re.clone(), compiled);
                }
            }
        }
        Self {
            skills,
            regexes,
            fired: HashMap::new(),
        }
    }

    pub fn skills(&self) -> &[Skill] {
        &self.skills
    }

    /// Returns a proposal for this event, or `None` when no skill applies.
    pub fn evaluate(&mut self, event: &Event, env: &dyn Env, now: Instant) -> Option<Proposal> {
        let base_vars = vars_from(&event.payload);
        let mut proposal: Option<Proposal> = None;

        for skill in self.skills.iter().filter(|s| s.trigger.event == event.kind) {
            if !env.skill_enabled(skill) {
                continue;
            }
            let mut vars = base_vars.clone();
            if !matches_all(
                &skill.trigger.conditions,
                &event.payload,
                &self.regexes,
                &mut vars,
            ) {
                continue;
            }

            let cooldown_key = render(skill.dedupe.as_deref().unwrap_or(&skill.id), &vars);
            let cooldown_key = format!("{}|{cooldown_key}", skill.id);
            if skill.cooldown_secs > 0
                && let Some(at) = self.fired.get(&cooldown_key)
                && now.duration_since(*at) < Duration::from_secs(skill.cooldown_secs)
            {
                continue;
            }

            let options: Vec<ProposedOption> = skill
                .suggestion
                .options
                .iter()
                .filter(|o| o.requires.iter().all(|r| env.has(r)))
                .filter(|o| matches_all(&o.when, &event.payload, &self.regexes, &mut vars.clone()))
                .map(|o| ProposedOption {
                    label: render(&o.label, &vars),
                    action: o.action.clone(),
                    args: Value::Object(
                        o.args
                            .iter()
                            .map(|(k, v)| (k.clone(), Value::String(render(v, &vars))))
                            .collect(),
                    ),
                    skill_id: skill.id.clone(),
                })
                .collect();
            if options.is_empty() {
                continue;
            }
            self.fired.insert(cooldown_key, now);

            match proposal.as_mut() {
                None => {
                    proposal = Some(Proposal {
                        skill_id: skill.id.clone(),
                        skill_ids: vec![skill.id.clone()],
                        title: render(&skill.suggestion.title, &vars),
                        detail: render(&skill.suggestion.detail, &vars),
                        options,
                        trust: env.trust_override(&skill.id).unwrap_or(skill.trust),
                        remember: skill.remember.as_ref().map(|r| render(r, &vars)),
                        priority: skill.priority,
                    });
                }
                Some(p) => {
                    p.skill_ids.push(skill.id.clone());
                    for o in options {
                        if !p.options.iter().any(|x| x.label == o.label) {
                            p.options.push(o);
                        }
                    }
                }
            }
        }

        let mut p = proposal?;
        if let Some(key) = &p.remember {
            let counts = env.choice_counts(key);
            // Stable sort keeps the manifest order among equally used options.
            p.options
                .sort_by_key(|o| std::cmp::Reverse(counts.get(&o.label).copied().unwrap_or(0)));
        }
        p.options.truncate(MAX_OPTIONS);
        Some(p)
    }
}

fn field<'a>(payload: &'a Value, path: &str) -> Option<&'a Value> {
    path.split('.').try_fold(payload, |v, key| v.get(key))
}

fn as_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Loose equality: `3000` matches `"3000"`, strings compare case-insensitively.
fn same(a: &Value, b: &Value) -> bool {
    a == b || as_text(a).eq_ignore_ascii_case(&as_text(b))
}

fn matches_all(
    conditions: &BTreeMap<String, Matcher>,
    payload: &Value,
    regexes: &HashMap<String, Regex>,
    vars: &mut Vars,
) -> bool {
    conditions.iter().all(|(path, m)| {
        let value = field(payload, path);
        if let Some(should_exist) = m.exists
            && value.is_some_and(|v| !v.is_null()) != should_exist
        {
            return false;
        }
        let Some(value) = value else {
            return m.exists == Some(false)
                && m.equals.is_none()
                && m.one_of.is_none()
                && m.regex.is_none();
        };
        if let Some(eq) = &m.equals
            && !same(value, eq)
        {
            return false;
        }
        if let Some(set) = &m.one_of
            && !set.iter().any(|x| same(value, x))
        {
            return false;
        }
        if let Some(set) = &m.not_one_of
            && set.iter().any(|x| same(value, x))
        {
            return false;
        }
        if let Some([lo, hi]) = m.range {
            match value.as_f64().or_else(|| as_text(value).parse().ok()) {
                Some(n) if n >= lo && n <= hi => {}
                _ => return false,
            }
        }
        if let Some(re) = &m.regex {
            let Some(re) = regexes.get(re) else {
                return false;
            };
            let text = as_text(value);
            let Some(caps) = re.captures(&text) else {
                return false;
            };
            for name in re.capture_names().flatten() {
                if let Some(c) = caps.name(name) {
                    vars.insert(name.to_string(), c.as_str().to_string());
                }
            }
        }
        true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestEnv {
        caps: Vec<&'static str>,
        counts: HashMap<String, u32>,
    }

    impl Env for TestEnv {
        fn has(&self, r: &str) -> bool {
            self.caps.contains(&r)
        }
        fn skill_enabled(&self, skill: &Skill) -> bool {
            skill.enabled_by_default
        }
        fn trust_override(&self, _: &str) -> Option<Trust> {
            None
        }
        fn choice_counts(&self, _: &str) -> HashMap<String, u32> {
            self.counts.clone()
        }
    }

    fn env() -> TestEnv {
        TestEnv {
            caps: vec!["browser:chrome", "tool:magick"],
            counts: HashMap::new(),
        }
    }

    fn skill(yaml: &str) -> Skill {
        Skill::parse("test.yaml", yaml).unwrap()
    }

    const DEV: &str = r#"
id: dev.browser
name: Dev server
remember: "dev:{{port}}"
cooldown_secs: 30
dedupe: "{{port}}"
trigger:
  event: port.listening
  where:
    process: { one_of: [node, python] }
    port: { range: [1024, 65535] }
suggestion:
  title: "Dev server on {{port}}"
  options:
    - { label: Chrome, action: open_url, args: { url: "{{url}}", browser: chrome }, requires: ["browser:chrome"] }
    - { label: Zen, action: open_url, args: { url: "{{url}}", browser: zen }, requires: ["browser:zen"] }
    - { label: Default browser, action: open_url, args: { url: "{{url}}" } }
"#;

    fn port_event(process: &str, port: u16) -> Event {
        Event::new(
            "port.listening",
            "ports",
            serde_json::json!({"process": process, "port": port, "url": format!("http://localhost:{port}")}),
        )
    }

    #[test]
    fn matches_filters_and_drops_missing_capabilities() {
        let mut e = Engine::new(vec![skill(DEV)]);
        let p = e
            .evaluate(&port_event("node", 3000), &env(), Instant::now())
            .unwrap();
        assert_eq!(p.title, "Dev server on 3000");
        let labels: Vec<_> = p.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Chrome", "Default browser"]);
        assert_eq!(p.options[0].args["url"], "http://localhost:3000");
        assert!(
            e.evaluate(&port_event("java", 3001), &env(), Instant::now())
                .is_none()
        );
    }

    #[test]
    fn cooldown_is_per_dedupe_key() {
        let mut e = Engine::new(vec![skill(DEV)]);
        let now = Instant::now();
        assert!(e.evaluate(&port_event("node", 3000), &env(), now).is_some());
        assert!(e.evaluate(&port_event("node", 3000), &env(), now).is_none());
        assert!(e.evaluate(&port_event("node", 5173), &env(), now).is_some());
        assert!(
            e.evaluate(
                &port_event("node", 3000),
                &env(),
                now + Duration::from_secs(31)
            )
            .is_some()
        );
    }

    #[test]
    fn ranks_by_past_choices() {
        let mut e = Engine::new(vec![skill(DEV)]);
        let mut env = env();
        env.counts.insert("Default browser".into(), 3);
        let p = e
            .evaluate(&port_event("node", 3000), &env, Instant::now())
            .unwrap();
        assert_eq!(p.options[0].label, "Default browser");
    }

    #[test]
    fn merges_skills_and_uses_regex_captures() {
        let conflict = skill(
            r#"
id: dev.port-conflict
name: Port conflict
priority: 80
trigger:
  event: clipboard.changed
  where:
    text: { regex: "(EADDRINUSE|address already in use).*?:(?P<port>\\d{2,5})" }
suggestion:
  title: "Port {{port}} is busy"
  options:
    - { label: "Free port {{port}}", action: kill_port, args: { port: "{{port}}" } }
"#,
        );
        let copy = skill(
            r#"
id: clip.any
name: Any text
priority: 10
trigger: { event: clipboard.changed }
suggestion:
  title: Copied
  options: [ { label: Search, action: open_url, args: { url: "x" } } ]
"#,
        );
        let mut e = Engine::new(vec![copy, conflict]);
        let ev = Event::new(
            "clipboard.changed",
            "clipboard",
            serde_json::json!({"text": "Error: listen EADDRINUSE: address already in use :::3000"}),
        );
        let p = e.evaluate(&ev, &env(), Instant::now()).unwrap();
        assert_eq!(p.title, "Port 3000 is busy");
        assert_eq!(p.skill_ids, ["dev.port-conflict", "clip.any"]);
        assert_eq!(p.options[0].label, "Free port 3000");
        assert_eq!(p.options[0].args["port"], "3000");
        assert_eq!(p.options.len(), 2);
    }

    #[test]
    fn option_when_filters_individual_options() {
        let s = skill(
            r#"
id: files.image
name: Image
trigger: { event: file.download_completed, where: { kind: { equals: image } } }
suggestion:
  title: "{{name}}"
  options:
    - { label: Convert to WebP, action: convert, args: { path: "{{path}}", to: webp }, when: { ext: { not_one_of: [webp] } }, requires: ["tool:magick"] }
    - { label: Convert to PNG, action: convert, args: { path: "{{path}}", to: png }, when: { ext: { not_one_of: [png] } }, requires: ["tool:magick"] }
"#,
        );
        let mut e = Engine::new(vec![s]);
        let ev = Event::new(
            "file.download_completed",
            "downloads",
            serde_json::json!({"name": "a.png", "ext": "png", "kind": "image", "path": "/d/a.png"}),
        );
        let p = e.evaluate(&ev, &env(), Instant::now()).unwrap();
        assert_eq!(p.options.len(), 1);
        assert_eq!(p.options[0].label, "Convert to WebP");
    }
}
