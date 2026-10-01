//! Mascot and island state machine (SRS 7.1).
//!
//! The machine is pure: it only decides transitions. Timers (for example
//! Success back to Idle after 1.5 s) are driven by the app, which feeds the
//! matching [`MascotEvent`] back in.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MascotState {
    Idle,
    Sleeping,
    Noticing,
    Suggesting,
    Listening,
    Working,
    Success,
    Error,
}

impl MascotState {
    pub const ALL: [MascotState; 8] = [
        MascotState::Idle,
        MascotState::Sleeping,
        MascotState::Noticing,
        MascotState::Suggesting,
        MascotState::Listening,
        MascotState::Working,
        MascotState::Success,
        MascotState::Error,
    ];

    /// The sound cue played when entering this state.
    pub fn cue(self) -> Option<Cue> {
        match self {
            MascotState::Working => None,
            MascotState::Idle => Some(Cue::Settle),
            MascotState::Sleeping => Some(Cue::Yawn),
            MascotState::Noticing => Some(Cue::Chirp),
            MascotState::Suggesting => Some(Cue::Pop),
            MascotState::Listening => Some(Cue::Open),
            MascotState::Success => Some(Cue::Ding),
            MascotState::Error => Some(Cue::Boop),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cue {
    Yawn,
    Settle,
    Chirp,
    Pop,
    Open,
    Ding,
    Boop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MascotEvent {
    /// No input for a while, or sensors paused.
    Rest,
    /// User active again, or sensors resumed.
    Wake,
    SkillMatched,
    ConditionsFailed,
    SuggestionReady,
    /// Dismissed by the user or timed out.
    Dismissed,
    /// The user picked an option, or an Auto action started.
    Picked,
    ListenStart,
    CommandUnderstood,
    Cancelled,
    ActionDone,
    ActionFailed,
    NeedsApproval,
    SuccessElapsed,
    OfferRetry,
    ErrorDismissed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transition {
    pub previous: MascotState,
    pub state: MascotState,
    pub cue: Option<Cue>,
}

#[derive(Debug, Clone)]
pub struct MascotMachine {
    state: MascotState,
}

impl Default for MascotMachine {
    fn default() -> Self {
        Self {
            state: MascotState::Idle,
        }
    }
}

impl MascotMachine {
    pub fn state(&self) -> MascotState {
        self.state
    }

    /// Applies an event. Returns the transition, or `None` when the event is
    /// not valid in the current state.
    pub fn dispatch(&mut self, event: MascotEvent) -> Option<Transition> {
        let next = next_state(self.state, event)?;
        Some(self.enter(next))
    }

    /// Jumps straight to a state. Used by the debug panel only.
    pub fn force(&mut self, state: MascotState) -> Transition {
        self.enter(state)
    }

    fn enter(&mut self, state: MascotState) -> Transition {
        let previous = self.state;
        self.state = state;
        Transition {
            previous,
            state,
            cue: state.cue(),
        }
    }
}

fn next_state(from: MascotState, event: MascotEvent) -> Option<MascotState> {
    use MascotEvent as E;
    use MascotState as S;

    Some(match (from, event) {
        (S::Idle | S::Noticing | S::Suggesting | S::Success | S::Error, E::Rest) => S::Sleeping,
        (S::Sleeping, E::Wake) => S::Idle,
        (S::Idle, E::SkillMatched) => S::Noticing,
        (S::Noticing, E::ConditionsFailed) => S::Idle,
        (S::Noticing, E::SuggestionReady) => S::Suggesting,
        (S::Suggesting, E::Dismissed) => S::Idle,
        (S::Suggesting, E::Picked) => S::Working,
        (S::Idle, E::ListenStart) => S::Listening,
        (S::Listening, E::CommandUnderstood) => S::Working,
        (S::Listening, E::Cancelled) => S::Idle,
        (S::Working, E::ActionDone) => S::Success,
        (S::Working, E::ActionFailed) => S::Error,
        (S::Working, E::NeedsApproval) => S::Suggesting,
        (S::Success, E::SuccessElapsed) => S::Idle,
        (S::Error, E::OfferRetry) => S::Suggesting,
        (S::Error, E::ErrorDismissed) => S::Idle,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use MascotEvent as E;
    use MascotState as S;

    fn run(events: &[MascotEvent]) -> MascotState {
        let mut m = MascotMachine::default();
        for e in events {
            m.dispatch(*e)
                .unwrap_or_else(|| panic!("{e:?} rejected in {:?}", m.state()));
        }
        m.state()
    }

    #[test]
    fn suggestion_happy_path_returns_to_idle() {
        assert_eq!(
            run(&[
                E::SkillMatched,
                E::SuggestionReady,
                E::Picked,
                E::ActionDone,
                E::SuccessElapsed
            ]),
            S::Idle
        );
    }

    #[test]
    fn failed_action_can_retry() {
        assert_eq!(
            run(&[
                E::SkillMatched,
                E::SuggestionReady,
                E::Picked,
                E::ActionFailed,
                E::OfferRetry
            ]),
            S::Suggesting
        );
    }

    #[test]
    fn working_can_ask_for_approval() {
        assert_eq!(
            run(&[E::ListenStart, E::CommandUnderstood, E::NeedsApproval]),
            S::Suggesting
        );
    }

    #[test]
    fn invalid_events_are_ignored() {
        let mut m = MascotMachine::default();
        assert!(m.dispatch(E::ActionDone).is_none());
        assert_eq!(m.state(), S::Idle);
    }

    #[test]
    fn rest_is_ignored_while_working() {
        let mut m = MascotMachine::default();
        m.force(S::Working);
        assert!(m.dispatch(E::Rest).is_none());
    }

    #[test]
    fn transitions_carry_the_cue_of_the_new_state() {
        let mut m = MascotMachine::default();
        let t = m.dispatch(E::SkillMatched).unwrap();
        assert_eq!(t.previous, S::Idle);
        assert_eq!(t.state, S::Noticing);
        assert_eq!(t.cue, Some(Cue::Chirp));
        assert_eq!(
            m.dispatch(E::ConditionsFailed).unwrap().cue,
            Some(Cue::Settle)
        );
        assert_eq!(m.dispatch(E::ListenStart).unwrap().cue, Some(Cue::Open));
    }

    #[test]
    fn serializes_as_snake_case() {
        assert_eq!(
            serde_json::to_string(&S::Suggesting).unwrap(),
            "\"suggesting\""
        );
        assert_eq!(
            serde_json::to_string(&E::SkillMatched).unwrap(),
            "\"skill_matched\""
        );
    }
}
