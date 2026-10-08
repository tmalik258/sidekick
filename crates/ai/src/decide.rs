//! Decisions: pick one option from a typed list, with a score per option.
//! SemIf reads the answer straight off the model's logits; a local chat
//! model is the fallback when SemIf is not installed.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{AiError, ChatRequest, Message, OpenAiCompat, hide_console};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DecisionOption {
    pub id: String,
    pub description: String,
}

/// One question in SemIf's input format.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub id: String,
    /// What is going on, in plain words.
    pub state: String,
    pub question: String,
    pub options: Vec<DecisionOption>,
}

/// Options with scores, best first.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Ranked {
    pub provider: &'static str,
    pub scores: Vec<(String, f64)>,
}

impl Ranked {
    fn new(provider: &'static str, mut scores: Vec<(String, f64)>) -> Self {
        scores.sort_by(|a, b| b.1.total_cmp(&a.1));
        Self { provider, scores }
    }

    pub fn best(&self) -> Option<(&str, f64)> {
        self.scores.first().map(|(id, p)| (id.as_str(), *p))
    }

    pub fn score(&self, id: &str) -> Option<f64> {
        self.scores.iter().find(|(o, _)| o == id).map(|(_, p)| *p)
    }
}

#[async_trait]
pub trait Decider: Send + Sync {
    fn id(&self) -> &'static str;
    async fn available(&self) -> bool;
    async fn decide(&self, d: &Decision) -> Result<Ranked, AiError>;
}

/// SemIf (formerly OpenJev) through its `semif-score` CLI. The command is
/// configurable, so it can run natively or inside WSL
/// (`wsl.exe -d Ubuntu-22.04 -- /home/me/semif/.venv/bin/semif-score`).
pub struct SemIf {
    pub command: Vec<String>,
    pub mode: String,
    pub backend: String,
    pub model: String,
    pub revision: String,
    pub gguf: Option<String>,
    pub scratch: PathBuf,
    pub timeout: Duration,
}

static RUN: AtomicU64 = AtomicU64::new(0);

impl SemIf {
    fn program(&self) -> Option<&str> {
        self.command
            .first()
            .map(String::as_str)
            .filter(|p| !p.is_empty())
    }

    fn via_wsl(&self) -> bool {
        self.program()
            .and_then(|p| Path::new(p).file_stem()?.to_str())
            .is_some_and(|s| s.eq_ignore_ascii_case("wsl"))
    }

    fn path_arg(&self, p: &Path) -> String {
        let s = p.to_string_lossy().into_owned();
        if self.via_wsl() { to_wsl_path(&s) } else { s }
    }

    fn args(&self, input: &Path, output: &Path) -> Vec<String> {
        let mut args: Vec<String> = self.command.iter().skip(1).cloned().collect();
        args.extend(["--mode".into(), self.mode.clone()]);
        args.extend(["--backend".into(), self.backend.clone()]);
        args.extend(["--model".into(), self.model.clone()]);
        args.extend(["--revision".into(), self.revision.clone()]);
        if let Some(gguf) = self.gguf.as_ref().filter(|g| !g.is_empty()) {
            args.extend(["--gguf".into(), gguf.clone()]);
        }
        args.extend(["--input".into(), self.path_arg(input)]);
        args.extend(["--output".into(), self.path_arg(output)]);
        args
    }
}

/// `C:\Users\me\x` becomes `/mnt/c/Users/me/x` for a program inside WSL.
fn to_wsl_path(p: &str) -> String {
    let b = p.as_bytes();
    if b.len() >= 2 && b[1] == b':' && b[0].is_ascii_alphabetic() {
        let drive = (b[0] as char).to_ascii_lowercase();
        format!("/mnt/{drive}{}", p[2..].replace('\\', "/"))
    } else {
        p.replace('\\', "/")
    }
}

/// Reads one `semif-score` result line.
fn parse_semif(line: &str) -> Result<Vec<(String, f64)>, AiError> {
    let v: Value = serde_json::from_str(line)
        .map_err(|e| AiError::Failed(format!("SemIf output is not JSON: {e}")))?;
    let ids = v["option_ids"].as_array();
    let probs = v["probabilities"].as_array();
    let (Some(ids), Some(probs)) = (ids, probs) else {
        return Err(AiError::Failed("SemIf output has no option scores".into()));
    };
    Ok(ids
        .iter()
        .zip(probs)
        .filter_map(|(i, p)| Some((i.as_str()?.to_owned(), p.as_f64()?)))
        .collect())
}

#[async_trait]
impl Decider for SemIf {
    fn id(&self) -> &'static str {
        "semif"
    }

    async fn available(&self) -> bool {
        self.program()
            .is_some_and(|p| Path::new(p).is_file() || which::which(p).is_ok())
            && !self.model.is_empty()
    }

    async fn decide(&self, d: &Decision) -> Result<Ranked, AiError> {
        let program = self
            .program()
            .ok_or_else(|| AiError::Failed("SemIf command is not set".into()))?;
        tokio::fs::create_dir_all(&self.scratch)
            .await
            .map_err(|e| AiError::Failed(e.to_string()))?;
        let n = RUN.fetch_add(1, Ordering::Relaxed);
        let stem = format!("semif-{}-{n}", std::process::id());
        let input = self.scratch.join(format!("{stem}.in.jsonl"));
        let output = self.scratch.join(format!("{stem}.out.jsonl"));
        let line = serde_json::to_string(d).map_err(|e| AiError::Failed(e.to_string()))? + "\n";
        tokio::fs::write(&input, line)
            .await
            .map_err(|e| AiError::Failed(e.to_string()))?;

        let mut cmd = tokio::process::Command::new(program);
        cmd.args(self.args(&input, &output))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        hide_console(&mut cmd);
        let run = tokio::time::timeout(self.timeout, cmd.output()).await;
        let _ = tokio::fs::remove_file(&input).await;
        let out = match run {
            Err(_) => return Err(AiError::Failed("SemIf took too long".into())),
            Ok(Err(e)) => return Err(AiError::Failed(format!("could not start SemIf: {e}"))),
            Ok(Ok(out)) => out,
        };
        let text = tokio::fs::read_to_string(&output).await.unwrap_or_default();
        let _ = tokio::fs::remove_file(&output).await;
        if !out.status.success() || text.trim().is_empty() {
            let err = String::from_utf8_lossy(&out.stderr);
            let detail = err.lines().last().unwrap_or("no output").trim().to_owned();
            return Err(AiError::Failed(format!("SemIf failed: {detail}")));
        }
        let first = text.lines().next().unwrap_or_default();
        Ok(Ranked::new("semif", parse_semif(first)?))
    }
}

/// A local chat model asked to answer with one option id. Scores are 1 for
/// the pick and 0 for the rest, since chat APIs give no calibrated numbers.
pub struct LocalDecider {
    pub model: OpenAiCompat,
}

fn decision_prompt(d: &Decision) -> ChatRequest {
    let options: String = d
        .options
        .iter()
        .map(|o| format!("- {}: {}\n", o.id, o.description))
        .collect();
    ChatRequest {
        system: "You pick exactly one option. Reply with JSON only: {\"choice\": \"<option id>\"}."
            .into(),
        messages: vec![Message::user(format!(
            "Situation: {}\nQuestion: {}\nOptions:\n{options}",
            d.state, d.question
        ))],
        image: None,
        think: false,
    }
}

fn parse_choice(text: &str, d: &Decision) -> Option<String> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    let v: Value = serde_json::from_str(text.get(start..=end)?).ok()?;
    let choice = v["choice"].as_str()?;
    d.options
        .iter()
        .find(|o| o.id == choice)
        .map(|o| o.id.clone())
}

#[async_trait]
impl Decider for LocalDecider {
    fn id(&self) -> &'static str {
        "local"
    }

    async fn available(&self) -> bool {
        crate::openai::is_local_url(&self.model.base_url)
            && self.model.models().await.is_some_and(|m| !m.is_empty())
    }

    async fn decide(&self, d: &Decision) -> Result<Ranked, AiError> {
        let text = self
            .model
            .complete(
                &decision_prompt(d),
                json!({"temperature": 0, "response_format": {"type": "json_object"}}),
            )
            .await?;
        let pick = parse_choice(&text, d)
            .ok_or_else(|| AiError::Failed(format!("model gave no valid choice: {text}")))?;
        Ok(Ranked::new(
            "local",
            d.options
                .iter()
                .map(|o| (o.id.clone(), if o.id == pick { 1.0 } else { 0.0 }))
                .collect(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decision() -> Decision {
        Decision {
            id: "d1".into(),
            state: "A file finished downloading".into(),
            question: "Is this worth interrupting the user?".into(),
            options: vec![
                DecisionOption {
                    id: "show".into(),
                    description: "Show it now".into(),
                },
                DecisionOption {
                    id: "skip".into(),
                    description: "Stay quiet".into(),
                },
            ],
        }
    }

    #[test]
    fn reads_semif_results_best_first() {
        let line = r#"{"id":"d1","option_ids":["show","skip"],"probabilities":[0.2,0.8],"option_logits":[1,2]}"#;
        let r = Ranked::new("semif", parse_semif(line).unwrap());
        assert_eq!(r.best(), Some(("skip", 0.8)));
        assert_eq!(r.score("show"), Some(0.2));
        assert!(parse_semif(r#"{"id":"d1"}"#).is_err());
    }

    #[test]
    fn input_matches_semif_format() {
        let v = serde_json::to_value(decision()).unwrap();
        assert_eq!(v["options"][0]["id"], "show");
        assert_eq!(v["question"], "Is this worth interrupting the user?");
    }

    #[test]
    fn translates_paths_for_wsl() {
        assert_eq!(
            to_wsl_path(r"C:\Users\me\x.jsonl"),
            "/mnt/c/Users/me/x.jsonl"
        );
        let s = SemIf {
            command: vec!["wsl.exe".into(), "--".into(), "semif-score".into()],
            mode: "direct".into(),
            backend: "llamacpp".into(),
            model: "Qwen/Qwen3.5-4B".into(),
            revision: "main".into(),
            gguf: Some("/models/q.gguf".into()),
            scratch: PathBuf::from("C:/tmp"),
            timeout: Duration::from_secs(1),
        };
        let args = s.args(Path::new(r"C:\tmp\a.jsonl"), Path::new(r"C:\tmp\b.jsonl"));
        assert_eq!(args[..2], ["--".to_string(), "semif-score".into()]);
        assert!(args.contains(&"/mnt/c/tmp/a.jsonl".to_string()));
        assert!(args.contains(&"--gguf".to_string()));
    }

    #[test]
    fn local_choice_must_be_a_real_option() {
        let d = decision();
        assert_eq!(
            parse_choice(r#"{"choice":"skip"}"#, &d),
            Some("skip".into())
        );
        assert_eq!(
            parse_choice(r#"sure: {"choice": "show"} done"#, &d),
            Some("show".into())
        );
        assert_eq!(parse_choice(r#"{"choice":"delete"}"#, &d), None);
        assert_eq!(parse_choice("no json", &d), None);
    }
}
