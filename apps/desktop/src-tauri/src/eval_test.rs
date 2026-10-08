//! 100 real prompts and the tool each should call first (or none), in
//! evals/prompts.json. The file is checked on every test run; the model run
//! is ignored by default and runs before each release against a local
//! Ollama:
//!
//! SIDEKICK_EVAL_MODEL=qwen3:4b cargo test -p sidekick-desktop eval -- --ignored --nocapture
//!
//! SIDEKICK_EVAL_URL changes the server and SIDEKICK_EVAL_MIN the pass rate
//! (0.85 by default).

use std::sync::Mutex;

use serde::Deserialize;
use serde_json::Value;
use sidekick_ai::{CancellationToken, ChatRequest, Message, OpenAiCompat, Sink, ToolRunner};

const PROMPTS: &str = include_str!("../evals/prompts.json");

#[derive(Deserialize)]
struct Case {
    prompt: String,
    /// One tool name, several that are each fine, or null for no call.
    tool: Value,
    #[serde(default)]
    args: serde_json::Map<String, Value>,
}

fn cases() -> Vec<Case> {
    serde_json::from_str(PROMPTS).expect("evals/prompts.json parses")
}

fn tools_of(case: &Case) -> Vec<&str> {
    match &case.tool {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) => a.iter().filter_map(Value::as_str).collect(),
        _ => Vec::new(),
    }
}

/// Whether an actual value matches what the case expects: words are
/// contained (any case), lists are "any of", numbers compare as numbers.
fn fits(want: &Value, got: &Value) -> bool {
    match want {
        Value::Array(any) => any.iter().any(|w| fits(w, got)),
        Value::String(w) => got
            .as_str()
            .is_some_and(|g| g.to_lowercase().contains(&w.to_lowercase())),
        Value::Number(n) => {
            let g = got
                .as_f64()
                .or_else(|| got.as_str().and_then(|s| s.trim().parse().ok()));
            g == n.as_f64()
        }
        other => got == other,
    }
}

/// What is wrong with the first call, or None when it is right.
fn judge(case: &Case, first: Option<&(String, Value)>) -> Option<String> {
    let want = tools_of(case);
    match (want.is_empty(), first) {
        (true, None) => None,
        (true, Some((name, _))) => Some(format!("called {name}, wanted no tool")),
        (false, None) => Some(format!("no tool, wanted {}", want.join(" or "))),
        (false, Some((name, args))) => {
            if !want.contains(&name.as_str()) {
                return Some(format!("called {name}, wanted {}", want.join(" or ")));
            }
            // Arguments are only checked when the first choice was the main tool.
            if name != want[0] {
                return None;
            }
            case.args
                .iter()
                .find(|(k, v)| !fits(v, &args[k.as_str()]))
                .map(|(k, v)| format!("{name} {k} = {}, wanted {v}", args[k.as_str()]))
        }
    }
}

#[test]
fn prompt_set_names_real_tools_and_arguments() {
    let defs = crate::ask_tools::defs();
    let cases = cases();
    assert_eq!(cases.len(), 100);
    let mut wrong = Vec::new();
    for c in &cases {
        let tools = tools_of(c);
        for t in &tools {
            if !defs.iter().any(|d| d.name == *t) {
                wrong.push(format!("{}: no tool {t}", c.prompt));
            }
        }
        let Some(def) = tools
            .first()
            .and_then(|t| defs.iter().find(|d| d.name == *t))
        else {
            continue;
        };
        for (k, v) in &c.args {
            let prop = &def.parameters["properties"][k.as_str()];
            if prop.is_null() {
                wrong.push(format!("{}: {} has no {k}", c.prompt, def.name));
            } else if let Some(allowed) = prop["enum"].as_array() {
                let vals = v.as_array().cloned().unwrap_or_else(|| vec![v.clone()]);
                if let Some(bad) = vals.iter().find(|x| !allowed.contains(x)) {
                    wrong.push(format!("{}: {k} {bad} is not allowed", c.prompt));
                }
            }
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

#[test]
fn judging_is_exact_about_tools_and_loose_about_words() {
    let c: Case = serde_json::from_value(serde_json::json!({
        "prompt": "open spotify", "tool": "windows",
        "args": { "action": "launch", "name": "spotify" }
    }))
    .unwrap();
    let call = |n: &str, a: Value| Some((n.to_owned(), a));
    let ok = call(
        "windows",
        serde_json::json!({ "action": "launch", "name": "Spotify app" }),
    );
    assert_eq!(judge(&c, ok.as_ref()), None);
    let wrong = call(
        "windows",
        serde_json::json!({ "action": "focus", "name": "spotify" }),
    );
    assert!(judge(&c, wrong.as_ref()).is_some());
    assert!(judge(&c, call("open", Value::Null).as_ref()).is_some());
    assert!(judge(&c, None).is_some());
    assert!(fits(&serde_json::json!(30), &serde_json::json!("30")));
}

/// Records each call and answers "Done" so the model can finish.
struct Recorder(Mutex<Vec<(String, Value)>>);

#[async_trait::async_trait]
impl ToolRunner for Recorder {
    async fn run(&self, name: &str, arguments: &Value) -> String {
        if let Ok(mut c) = self.0.lock() {
            c.push((name.to_owned(), arguments.clone()));
        }
        "Done.".into()
    }
}

#[tokio::test]
#[ignore = "needs a local model; run before each release"]
async fn local_model_picks_the_right_tools() {
    let model = std::env::var("SIDEKICK_EVAL_MODEL").ok();
    let url = std::env::var("SIDEKICK_EVAL_URL").ok();
    let min: f64 = std::env::var("SIDEKICK_EVAL_MIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.85);
    let ai = OpenAiCompat::new(url, model);
    let defs = crate::ask_tools::defs();
    let cases = cases();
    let mut wrong = Vec::new();
    for c in &cases {
        let runner = Recorder(Mutex::new(Vec::new()));
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let req = ChatRequest {
            system: crate::ai::SYSTEM.to_owned(),
            messages: vec![Message::user(c.prompt.clone())],
            image: None,
            think: false,
        };
        let done = ai
            .chat_with_tools(
                &req,
                &defs,
                &runner,
                &Sink::new(tx),
                &CancellationToken::new(),
            )
            .await;
        let calls = runner.0.into_inner().unwrap_or_default();
        let problem = match done {
            Err(e) => Some(format!("error {e}")),
            Ok(_) => judge(c, calls.first()),
        };
        if let Some(p) = problem {
            println!("WRONG  {}  ->  {p}", c.prompt);
            wrong.push(p);
        }
    }
    let rate = 1.0 - wrong.len() as f64 / cases.len() as f64;
    println!(
        "{:.0}% right ({} of {})",
        rate * 100.0,
        cases.len() - wrong.len(),
        cases.len()
    );
    assert!(
        rate >= min,
        "{:.0}% is below {:.0}%",
        rate * 100.0,
        min * 100.0
    );
}
