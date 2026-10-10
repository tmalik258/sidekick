//! The rule engine: match an event against every enabled skill,
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
pub const MAX_OPTIONS: usize = 8;

/// What the app knows that skills depend on.
pub trait Env {
    /// True when a capability such as `browser:zen` or `tool:ffmpeg` exists.
    fn has(&self, requirement: &str) -> bool;
    fn skill_enabled(&self, skill: &Skill) -> bool;
    /// The user's trust override for a skill, if any.
    fn trust_override(&self, skill_id: &str) -> Option<Trust>;
    /// How often each option label was chosen under a preference key.
    fn choice_counts(&self, key: &str) -> HashMap<String, u32>;
    /// OS default browser id when that browser is installed (e.g. `"zen"`).
    fn default_browser_id(&self) -> Option<String>;
    /// The code editor projects open in ("Cursor"), for `{{editor}}`.
    fn editor_name(&self) -> Option<String> {
        None
    }
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
        let mut base_vars = vars_from(&event.payload);
        base_vars
            .entry("editor".into())
            .or_insert_with(|| env.editor_name().unwrap_or_else(|| "your editor".into()));
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
            let mut counts = env.choice_counts(key);
            // Nothing yet for this kind ("url:github.com"): what you pick
            // across all of them ("url").
            if counts.is_empty()
                && let Some((all, _)) = key.split_once(':')
            {
                counts = env.choice_counts(all);
            }
            // Stable sort keeps the manifest order among equally used options.
            p.options
                .sort_by_key(|o| std::cmp::Reverse(counts.get(&o.label).copied().unwrap_or(0)));
        }
        if let Some(id) = env.default_browser_id() {
            pin_named_default(&mut p.options, &id);
        }
        p.options.truncate(MAX_OPTIONS);
        Some(p)
    }
}

/// Display name for a browser id; mirrors `Browser::label` in actions.
fn browser_label(id: &str) -> &str {
    match id {
        "chrome" => "Chrome",
        "edge" => "Edge",
        "firefox" => "Firefox",
        "zen" => "Zen",
        "brave" => "Brave",
        other => other,
    }
}

fn browser_arg(option: &ProposedOption) -> Option<&str> {
    option.args.get("browser").and_then(|v| v.as_str())
}

fn is_private(option: &ProposedOption) -> bool {
    option.args.get("private").and_then(|v| v.as_str()) == Some("true")
}

/// Rewrite the "Default browser" marker to the real OS default name, drop the
/// duplicate named chip, and pin that option first. Other open_url chips
/// (e.g. "Open {{first_title}}" on the morning card) are left alone.
fn pin_named_default(options: &mut Vec<ProposedOption>, id: &str) {
    let label = browser_label(id).to_string();
    let marker = options.iter().position(|o| {
        o.action == "open_url" && o.label == "Default browser" && browser_arg(o).is_none()
    });

    let Some(mut marker) = marker else {
        if let Some(i) = options
            .iter()
            .position(|o| o.action == "open_url" && browser_arg(o) == Some(id) && !is_private(o))
        {
            let opt = options.remove(i);
            options.insert(0, opt);
        }
        return;
    };

    options[marker].label = label;
    if let Value::Object(map) = &mut options[marker].args {
        map.insert("browser".into(), Value::String(id.to_string()));
    }

    let mut i = 0;
    while i < options.len() {
        if i != marker
            && options[i].action == "open_url"
            && browser_arg(&options[i]) == Some(id)
            && !is_private(&options[i])
        {
            options.remove(i);
            if i < marker {
                marker -= 1;
            }
        } else {
            i += 1;
        }
    }

    let opt = options.remove(marker);
    options.insert(0, opt);
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
        default_browser: Option<&'static str>,
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
        fn default_browser_id(&self) -> Option<String> {
            self.default_browser.map(|id| id.to_string())
        }
        fn editor_name(&self) -> Option<String> {
            self.caps.contains(&"tool:code").then(|| "Cursor".into())
        }
    }

    fn env() -> TestEnv {
        TestEnv {
            caps: vec!["browser:chrome", "tool:magick"],
            counts: HashMap::new(),
            default_browser: None,
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
    fn start_my_day_card_shows_the_routine() {
        let brief = crate::builtin()
            .into_iter()
            .find(|s| s.id == "system.morning-brief")
            .unwrap();
        let mut e = Engine::new(vec![brief]);
        let event = |routine: u32, auto: &str, offer: &str| {
            Event::new(
                "day.morning_brief",
                "time",
                serde_json::json!({
                    "headline": "Your usual: Code, github.com",
                    "text": "Usual start:\n- Code (app)",
                    "first_url": "", "first_title": "",
                    "first_repo": "", "first_repo_path": "",
                    "routine_count": routine, "item1": if routine > 0 { "Code" } else { "" },
                    "item2": "", "item3": "",
                    "auto": auto, "offer_auto": offer,
                }),
            )
        };
        let labels = |p: crate::Proposal| -> Vec<String> {
            p.options.into_iter().map(|o| o.label).collect()
        };
        let now = Instant::now();
        let p = e.evaluate(&event(2, "", "1"), &env(), now).unwrap();
        assert_eq!(
            labels(p),
            [
                "Copy",
                "Open all",
                "Code",
                "Not today",
                "Don't ask about apps",
                "Always open these",
            ]
        );
        let mut e2 = Engine::new(vec![
            crate::builtin()
                .into_iter()
                .find(|s| s.id == "system.morning-brief")
                .unwrap(),
        ]);
        let p = e2.evaluate(&event(2, "1", ""), &env(), now).unwrap();
        assert_eq!(labels(p), ["Copy", "Stop opening these by itself"]);
        let mut e3 = Engine::new(vec![
            crate::builtin()
                .into_iter()
                .find(|s| s.id == "system.morning-brief")
                .unwrap(),
        ]);
        let p = e3.evaluate(&event(0, "", ""), &env(), now).unwrap();
        assert_eq!(labels(p), ["Copy"]);
    }

    #[test]
    fn morning_open_pr_is_not_renamed_to_default_browser() {
        let brief = crate::builtin()
            .into_iter()
            .find(|s| s.id == "system.morning-brief")
            .unwrap();
        let mut e = Engine::new(vec![brief]);
        let mut env = env();
        env.default_browser = Some("zen");
        let ev = Event::new(
            "day.morning_brief",
            "time",
            serde_json::json!({
                "headline": "1 review waiting",
                "text": "Reviews:\n- Fix login",
                "first_url": "https://github.com/me/api/pull/1",
                "first_title": "Fix login",
                "first_repo": "", "first_repo_path": "",
                "routine_count": 1, "item1": "Zen", "item2": "", "item3": "",
                "auto": "", "offer_auto": "",
            }),
        );
        let p = e.evaluate(&ev, &env, Instant::now()).unwrap();
        let labels: Vec<_> = p.options.iter().map(|o| o.label.as_str()).collect();
        assert!(labels.contains(&"Open Fix login"), "{labels:?}");
        assert!(labels.contains(&"Zen"), "{labels:?}");
        assert_eq!(
            labels.iter().filter(|l| **l == "Zen").count(),
            1,
            "routine Zen must not collide with renamed open_url: {labels:?}"
        );
    }

    #[test]
    fn buttons_name_the_users_editor() {
        let claude = crate::builtin()
            .into_iter()
            .find(|s| s.id == "dev.claude-finished")
            .unwrap();
        let mut e = Engine::new(vec![claude]);
        let env = TestEnv {
            caps: vec!["tool:code"],
            counts: HashMap::new(),
            default_browser: None,
        };
        let event = Event::new(
            "claude.stop",
            "claude",
            serde_json::json!({ "project": "sidekick", "cwd": "C:\\code\\sidekick", "session": "s", "message": "" }),
        );
        let p = e.evaluate(&event, &env, Instant::now()).unwrap();
        assert!(
            p.options.iter().any(|o| o.label == "Open in Cursor"),
            "{:?}",
            p.options
        );
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
    fn names_and_pins_default_browser() {
        let mut e = Engine::new(vec![skill(DEV)]);
        let mut env = env();
        env.caps = vec!["browser:chrome", "browser:zen"];
        env.default_browser = Some("zen");
        let p = e
            .evaluate(&port_event("node", 3000), &env, Instant::now())
            .unwrap();
        let labels: Vec<_> = p.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Zen", "Chrome"]);
        assert_eq!(p.options[0].args["browser"], "zen");
        assert!(!labels.contains(&"Default browser"));
    }

    #[test]
    fn default_chrome_keeps_incognito() {
        let yaml = r#"
id: clip.open
name: Open
remember: "url"
trigger: { event: clipboard.changed, where: { kind: { equals: url } } }
suggestion:
  title: Link copied
  options:
    - { label: Chrome, action: open_url, args: { url: "{{text}}", browser: chrome }, requires: ["browser:chrome"] }
    - { label: Incognito, action: open_url, args: { url: "{{text}}", browser: chrome, private: "true" }, requires: ["browser:chrome"] }
    - { label: Zen, action: open_url, args: { url: "{{text}}", browser: zen }, requires: ["browser:zen"] }
    - { label: Default browser, action: open_url, args: { url: "{{text}}" } }
"#;
        let mut e = Engine::new(vec![skill(yaml)]);
        let mut env = env();
        env.caps = vec!["browser:chrome", "browser:zen"];
        env.default_browser = Some("chrome");
        let ev = Event::new(
            "clipboard.changed",
            "clipboard",
            serde_json::json!({"kind": "url", "text": "https://example.com"}),
        );
        let p = e.evaluate(&ev, &env, Instant::now()).unwrap();
        let labels: Vec<_> = p.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Chrome", "Incognito", "Zen"]);
        assert_eq!(p.options[0].args["browser"], "chrome");
        assert!(
            !p.options[0]
                .args
                .get("private")
                .is_some_and(|v| v == "true")
        );
    }

    #[test]
    fn default_stays_first_despite_ranking() {
        let mut e = Engine::new(vec![skill(DEV)]);
        let mut env = env();
        env.caps = vec!["browser:chrome", "browser:zen"];
        env.default_browser = Some("zen");
        env.counts.insert("Chrome".into(), 9);
        let p = e
            .evaluate(&port_event("node", 3000), &env, Instant::now())
            .unwrap();
        assert_eq!(p.options[0].label, "Zen");
        assert_eq!(p.options[1].label, "Chrome");
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

    #[test]
    fn late_night_offers_only_what_is_off() {
        let late = crate::builtin()
            .into_iter()
            .find(|s| s.id == "system.late-night")
            .unwrap();
        let ev = |night: &str, dnd: &str| {
            Event::new(
                "time.late_night",
                "time",
                serde_json::json!({ "time": "23:10", "night_light": night, "dnd": dnd }),
            )
        };
        let labels = |night: &str, dnd: &str| {
            Engine::new(vec![late.clone()])
                .evaluate(&ev(night, dnd), &env(), Instant::now())
                .map(|p| p.options.into_iter().map(|o| o.label).collect::<Vec<_>>())
        };
        assert_eq!(
            labels("off", "off").unwrap(),
            ["Night light", "Do Not Disturb"]
        );
        // Night light on (or on its schedule right now): not offered.
        assert_eq!(labels("on", "off").unwrap(), ["Do Not Disturb"]);
        assert_eq!(
            labels("unknown", "on").unwrap(),
            ["Night light"],
            "unknown still offers"
        );
        assert!(labels("on", "on").is_none(), "nothing to offer, no card");
    }

    #[test]
    fn notification_card_offers_copy_only_with_a_code() {
        let now = crate::builtin()
            .into_iter()
            .find(|s| s.id == "notify.now")
            .unwrap();
        let ev = |id: i64, code: serde_json::Value| {
            Event::new(
                "notification.now",
                "notifications",
                serde_json::json!({ "id": id, "app": "Chrome", "title": "Google", "body": "x", "code": code }),
            )
        };
        let mut e = Engine::new(vec![now]);
        let p = e
            .evaluate(&ev(1, serde_json::json!("482913")), &env(), Instant::now())
            .unwrap();
        assert_eq!(p.options[0].label, "Copy 482913");
        assert_eq!(p.options[0].args["text"], "482913");
        let p = e
            .evaluate(&ev(2, serde_json::Value::Null), &env(), Instant::now())
            .unwrap();
        assert_eq!(p.options[0].label, "Open Chrome");
    }
}
