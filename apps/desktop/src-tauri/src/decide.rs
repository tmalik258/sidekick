//! T1 ranking of suggestion options (SRS 5.5, FR-AI-10/11). Before a
//! suggestion shows, SemIf or the local model guesses which option the user
//! wants, and that option moves to the front. Options the user has picked
//! before always win: learned choices are never overridden.
//!
//! Decisions get a short budget so the island never waits on a slow model.
//! A late answer is cached and used from the next matching suggestion on
//! (SemIf through its CLI loads the model on each run, so this is the usual
//! path for it).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use sidekick_ai::{Decider, Decision, DecisionOption};
use sidekick_skills::{Proposal, Trust};
use tauri::{AppHandle, Manager};

use crate::ai;
use crate::state::{AppState, lock};

const BUDGET: Duration = Duration::from_millis(900);
const MAX_CACHED: usize = 500;

/// Whether this proposal is worth a decision at all.
fn wants_decision(app: &AppHandle, p: &Proposal) -> bool {
    if p.trust == Trust::Auto || p.options.len() < 2 {
        return false;
    }
    match &p.remember {
        Some(key) => lock(&app.state::<AppState>().storage)
            .choice_counts(key)
            .map(|c| c.is_empty())
            .unwrap_or(true),
        None => true,
    }
}

fn cache_key(app: &AppHandle, p: &Proposal) -> String {
    let window = lock(&app.state::<AppState>().last_window).clone();
    let in_app = window
        .as_ref()
        .and_then(|w| w["exe"].as_str())
        .unwrap_or("")
        .to_owned();
    format!("{}|{in_app}", p.skill_id)
}

fn decision(app: &AppHandle, p: &Proposal) -> Decision {
    let window = lock(&app.state::<AppState>().last_window).clone();
    let where_ = window
        .as_ref()
        .and_then(|w| w["app"].as_str())
        .map(|a| format!(" The user is in {a}."))
        .unwrap_or_default();
    Decision {
        id: p.skill_id.clone(),
        state: format!("{} {}.{where_}", p.title, p.detail),
        question: "Which action will the user most likely pick?".into(),
        options: p
            .options
            .iter()
            .enumerate()
            .map(|(i, o)| DecisionOption {
                id: format!("o{i}"),
                description: o.label.clone(),
            })
            .collect(),
    }
}

/// Moves the option labelled `best` to the front.
fn promote(p: &mut Proposal, best: &str) {
    if let Some(i) = p.options.iter().position(|o| o.label == best)
        && i > 0
    {
        let o = p.options.remove(i);
        p.options.insert(0, o);
    }
}

async fn ask(deciders: &[Arc<dyn Decider>], d: &Decision) -> Option<String> {
    for decider in deciders {
        if !decider.available().await {
            continue;
        }
        match decider.decide(d).await {
            Ok(ranked) => {
                let (id, score) = ranked.best()?;
                log::info!("{} picked {id} ({score:.2}) for {}", decider.id(), d.id);
                let i: usize = id.strip_prefix('o')?.parse().ok()?;
                return d.options.get(i).map(|o| o.description.clone());
            }
            Err(err) => log::warn!("{} could not decide: {err}", decider.id()),
        }
    }
    None
}

/// Reorders `p` using a cached or fresh T1 decision, within the budget.
pub async fn rank(app: &AppHandle, mut p: Proposal) -> Proposal {
    if !wants_decision(app, &p) {
        return p;
    }
    let settings = lock(&app.state::<AppState>().settings).clone();
    let deciders = ai::deciders(app, &settings);
    if deciders.is_empty() {
        return p;
    }
    let key = cache_key(app, &p);
    if let Some(best) = lock(&app.state::<AppState>().decisions).get(&key).cloned() {
        promote(&mut p, &best);
        return p;
    }
    let d = decision(app, &p);
    let task = {
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let best = ask(&deciders, &d).await;
            if let Some(best) = &best {
                let state = app.state::<AppState>();
                let mut cache = lock(&state.decisions);
                if cache.len() >= MAX_CACHED {
                    cache.clear();
                }
                cache.insert(key, best.clone());
            }
            best
        })
    };
    // A late answer still lands in the cache for next time.
    if let Ok(Ok(Some(best))) = tokio::time::timeout(BUDGET, task).await {
        promote(&mut p, &best);
    }
    p
}

/// Forgets cached decisions (with "Forget learned choices").
pub fn clear(app: &AppHandle) {
    lock(&app.state::<AppState>().decisions).clear();
}

pub type Cache = HashMap<String, String>;

#[cfg(test)]
mod tests {
    use super::*;
    use sidekick_skills::ProposedOption;

    fn proposal() -> Proposal {
        Proposal {
            skill_id: "dev.open".into(),
            skill_ids: vec!["dev.open".into()],
            title: "Dev server on port 3000".into(),
            detail: "node".into(),
            options: ["Chrome", "Incognito", "Zen"]
                .iter()
                .map(|l| ProposedOption {
                    label: (*l).into(),
                    action: "open_url".into(),
                    args: serde_json::Value::Null,
                    skill_id: "dev.open".into(),
                })
                .collect(),
            trust: Trust::Suggest,
            remember: None,
        }
    }

    #[test]
    fn promotes_without_dropping_options() {
        let mut p = proposal();
        promote(&mut p, "Zen");
        let labels: Vec<_> = p.options.iter().map(|o| o.label.as_str()).collect();
        assert_eq!(labels, ["Zen", "Chrome", "Incognito"]);
        promote(&mut p, "Firefox");
        assert_eq!(p.options.len(), 3);
    }
}
