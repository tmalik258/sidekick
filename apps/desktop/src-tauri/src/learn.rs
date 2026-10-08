//! Learned preferences. Dismissing a skill's suggestion three
//! times in a row quiets that skill for a day. Picking the same first
//! option five times in a row offers to make the skill automatic, once,
//! and only when that option is safe to run on its own.

use chrono::{DateTime, Duration, Utc};
use serde_json::json;
use sidekick_core::Habit;
use sidekick_skills::{Proposal, ProposedOption, Trust};
use tauri::{AppHandle, Manager};

use crate::state::{AppState, lock};

const DISMISSALS_TO_MUTE: i64 = 3;
const MUTE_FOR: Duration = Duration::hours(24);
const ACCEPTS_TO_OFFER: i64 = 5;
pub const OFFER_SKILL: &str = "learn.offer-auto";

/// Skills that are Sidekick talking about itself, not habits to learn.
pub fn tracked(skill_id: &str) -> bool {
    !skill_id.starts_with("learn.")
        && !skill_id.starts_with("mcp.")
        && !skill_id.starts_with("debug.")
        // Notifications learn per app instead (inbox.rs).
        && !skill_id.starts_with("notify.")
}

/// Skills never offered as automatic: each time needs its own choice.
/// A screenshot is already on the clipboard, so "always copy" adds nothing.
/// The morning card asks for itself (after five Open alls), so it is left out.
const NEVER_AUTO: &[&str] = &["files.screenshot", "system.morning-brief"];

/// Whether a skill may become automatic ("Always do this").
pub fn can_automate(skill_id: &str) -> bool {
    tracked(skill_id) && !NEVER_AUTO.contains(&skill_id)
}

fn load(app: &AppHandle, skill_id: &str) -> Option<Habit> {
    lock(&app.state::<AppState>().storage).habit(skill_id).ok()
}

fn save(app: &AppHandle, h: &Habit) {
    if let Err(err) = lock(&app.state::<AppState>().storage).save_habit(h) {
        log::warn!("could not save skill habit: {err}");
    }
}

pub fn is_muted(app: &AppHandle, skill_id: &str) -> bool {
    load(app, skill_id)
        .and_then(|h| h.muted_until)
        .and_then(|t| DateTime::parse_from_rfc3339(&t).ok())
        .is_some_and(|until| until > Utc::now())
}

/// The user said "Not now" (a timeout is not a judgement).
pub fn on_dismiss(app: &AppHandle, skill_id: &str, reason: &str) {
    if !crate::learned::on(app) {
        return;
    }
    if reason != "user" || !tracked(skill_id) {
        return;
    }
    let Some(mut h) = load(app, skill_id) else {
        return;
    };
    h.dismiss_streak += 1;
    h.accept_streak = 0;
    if h.dismiss_streak >= DISMISSALS_TO_MUTE {
        h.dismiss_streak = 0;
        h.muted_until = Some((Utc::now() + MUTE_FOR).to_rfc3339());
        log::info!("{skill_id} dismissed {DISMISSALS_TO_MUTE} times in a row; quiet for a day");
    }
    save(app, &h);
}

/// The user picked an option. Returns an offer to make the skill automatic
/// when they have picked the same first option enough times.
pub fn on_accept(
    app: &AppHandle,
    proposal: &Proposal,
    index: usize,
    auto: bool,
) -> Option<Proposal> {
    let option = proposal.options.get(index)?;
    if auto || !tracked(&proposal.skill_id) {
        return None;
    }
    let mut h = load(app, &proposal.skill_id)?;
    h.dismiss_streak = 0;
    if h.last_label == option.label {
        h.accept_streak += 1;
    } else {
        h.last_label = option.label.clone();
        h.accept_streak = 1;
    }
    let already_auto = lock(&app.state::<AppState>().settings)
        .skills
        .get(&proposal.skill_id)
        .and_then(|p| p.auto)
        .unwrap_or(false);
    // Auto runs the first option, so only that one can become automatic.
    let offer = h.accept_streak >= ACCEPTS_TO_OFFER
        && !h.offered
        && !already_auto
        && index == 0
        && can_automate(&proposal.skill_id)
        && proposal.trust == Trust::Suggest
        && sidekick_actions::is_safe(&option.action);
    if offer {
        h.offered = true;
    }
    save(app, &h);
    offer.then(|| offer_auto(&proposal.skill_id, &option.label))
}

fn offer_auto(skill_id: &str, label: &str) -> Proposal {
    let opt = |label: &str, action: &str, args: serde_json::Value| ProposedOption {
        label: label.into(),
        action: action.into(),
        args,
        skill_id: OFFER_SKILL.into(),
    };
    Proposal {
        skill_id: OFFER_SKILL.into(),
        skill_ids: vec![OFFER_SKILL.into()],
        title: format!("Always \"{label}\"?"),
        detail: format!(
            "You picked it {ACCEPTS_TO_OFFER} times in a row. Sidekick can do it without asking."
        ),
        options: vec![
            opt(
                "Do it automatically",
                "skill_auto",
                json!({ "skill": skill_id }),
            ),
            opt("Keep asking", "noop", json!({ "message": "OK" })),
        ],
        trust: Trust::Suggest,
        remember: None,
        priority: 60,
    }
}

/// Turns on Auto for a skill (from the offer above).
pub fn make_auto(app: &AppHandle, skill_id: &str) -> Result<String, String> {
    let mut next = lock(&app.state::<AppState>().settings).clone();
    let pref = next.skills.entry(skill_id.to_owned()).or_default();
    pref.auto = Some(true);
    pref.enabled = Some(true);
    crate::commands::apply_settings(app, next)?;
    Ok("Done. Change it any time in Settings > Skills".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidekick_itself_is_not_learned() {
        assert!(tracked("files.download"));
        assert!(!tracked(OFFER_SKILL));
        assert!(!can_automate("files.screenshot"));
        assert!(can_automate("files.download"));
        assert!(!tracked("mcp.notify"));
    }

    #[test]
    fn the_offer_turns_on_auto_for_that_skill() {
        let p = offer_auto("files.download", "Open");
        assert_eq!(p.options[0].action, "skill_auto");
        assert_eq!(p.options[0].args["skill"], "files.download");
        assert!(p.title.contains("Open"));
    }
}
