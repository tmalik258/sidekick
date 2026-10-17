//! Live mode: talk and Duskie answers by voice in place, and can act. This
//! module is the logic only (no audio, no models): the session state
//! machine, risk classes and the rules for confirming an action, the guard
//! that keeps mail, web and file text from triggering tools, speculative
//! starts on stable partial transcripts, and the sentence chunker that lets
//! speech start before the answer is finished.

use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::mascot::MascotEvent;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LiveState {
    Idle,
    Listening,
    Thinking,
    ToolRunning,
    ConfirmWait,
    Speaking,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LiveInput {
    /// The wake word, push to talk or the hotkey.
    Wake,
    /// A partial transcript while the user speaks.
    Partial(String),
    /// The user stopped speaking; the final transcript.
    Endpoint(String),
    ToolStart(String),
    ToolDone,
    /// An action needs the user's go ahead.
    NeedConfirm,
    Confirmed,
    Declined,
    /// The first sentence of the answer is being spoken.
    SpeechStart,
    /// Everything has been spoken.
    SpeechEnd,
    /// The user started talking while Duskie spoke or thought.
    BargeIn,
    /// Mute, Stop, Esc or the kill switch.
    Stop,
    /// Nothing heard for a while.
    Timeout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    Mascot(MascotEvent),
    /// Start or restart the answer on this text (read tools only).
    Speculate(String),
    /// The turn is final: use this text, letting writes be proposed.
    Commit(String),
    CancelGeneration,
    StopSpeech,
    ShowConfirm,
    /// Captions for the island.
    Caption(String),
}

/// One step of the session. Unknown inputs in a state change nothing.
pub fn step(state: LiveState, input: LiveInput) -> (LiveState, Vec<Effect>) {
    use Effect as E;
    use LiveInput as I;
    use LiveState as S;
    match (state, input) {
        (_, I::Stop) => (
            S::Idle,
            vec![
                E::StopSpeech,
                E::CancelGeneration,
                E::Mascot(MascotEvent::Cancelled),
            ],
        ),
        (S::Idle, I::Wake) => (S::Listening, vec![E::Mascot(MascotEvent::ListenStart)]),
        (S::Listening, I::Partial(t)) => (S::Listening, vec![E::Caption(t)]),
        (S::Listening, I::Endpoint(t)) if t.trim().is_empty() => {
            (S::Idle, vec![E::Mascot(MascotEvent::Cancelled)])
        }
        (S::Listening, I::Endpoint(t)) => (
            S::Thinking,
            vec![
                E::Caption(t.clone()),
                E::Commit(t),
                E::Mascot(MascotEvent::CommandUnderstood),
            ],
        ),
        (S::Listening, I::Timeout) => (S::Idle, vec![E::Mascot(MascotEvent::Cancelled)]),
        (S::Thinking | S::Speaking, I::ToolStart(name)) => (S::ToolRunning, vec![E::Caption(name)]),
        (S::ToolRunning, I::ToolDone) => (S::Thinking, vec![]),
        (S::Thinking | S::ToolRunning | S::Speaking, I::NeedConfirm) => (
            S::ConfirmWait,
            vec![
                E::StopSpeech,
                E::ShowConfirm,
                E::Mascot(MascotEvent::NeedsApproval),
            ],
        ),
        (S::ConfirmWait, I::Confirmed) => (S::ToolRunning, vec![E::Mascot(MascotEvent::Picked)]),
        (S::ConfirmWait, I::Declined) => (S::Thinking, vec![]),
        (S::Thinking | S::ToolRunning, I::SpeechStart) => (S::Speaking, vec![]),
        (S::Speaking, I::SpeechEnd) => {
            // Stay open for a follow up without the wake word.
            (S::Listening, vec![E::Mascot(MascotEvent::ListenStart)])
        }
        (S::Speaking | S::Thinking | S::ToolRunning, I::BargeIn) => {
            (S::Interrupted, vec![E::StopSpeech, E::CancelGeneration])
        }
        (S::Interrupted, I::Partial(t)) => (S::Listening, vec![E::Caption(t)]),
        (S::Interrupted, I::Timeout) => (S::Idle, vec![E::Mascot(MascotEvent::Cancelled)]),
        (s, _) => (s, vec![]),
    }
}

/// How much an action can hurt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Risk {
    /// Looks only: search, read, summarize.
    Read,
    /// Changes something that can be undone or is outward facing: a draft,
    /// a calendar event, a sent message.
    Write,
    /// Cannot be undone: uninstall, delete.
    Destructive,
}

/// A Live tool and the words a user would say to ask for it.
#[derive(Debug, Clone, Copy)]
pub struct LiveTool {
    pub name: &'static str,
    pub risk: Risk,
    /// The user's own words must include one of these before a write or
    /// destructive call can follow untrusted text.
    pub verbs: &'static [&'static str],
}

pub const TOOLS: &[LiveTool] = &[
    LiveTool {
        name: "gmail_search",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "gmail_read",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "gmail_summarize",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "gmail_draft",
        risk: Risk::Write,
        verbs: &["draft", "reply", "write", "answer"],
    },
    LiveTool {
        name: "calendar_read",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "calendar_create",
        risk: Risk::Write,
        verbs: &["add", "create", "schedule", "book", "set up", "put"],
    },
    LiveTool {
        name: "diagnose",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "app_help",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "show_me",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "file_search",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "use_skill",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "list_apps",
        risk: Risk::Read,
        verbs: &[],
    },
    LiveTool {
        name: "uninstall",
        risk: Risk::Destructive,
        verbs: &["uninstall", "remove", "get rid of", "delete"],
    },
    LiveTool {
        name: "delete_file",
        risk: Risk::Destructive,
        verbs: &["delete", "remove", "trash", "get rid of"],
    },
];

pub fn tool(name: &str) -> Option<&'static LiveTool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// Unknown tools count as destructive.
pub fn risk_of(name: &str) -> Risk {
    tool(name).map_or(Risk::Destructive, |t| t.risk)
}

/// Suggestion actions that cannot be undone. Voice can never pick these;
/// they need a tap.
pub const DESTRUCTIVE_ACTIONS: &[&str] = &[
    "clean_downloads",
    "clean_installers",
    "empty_recycle_bin",
    "git_delete_branches",
    "kill_port",
    "close_app",
    "trash_download",
    "uninstall",
    "delete_file",
];

pub fn voice_may_choose(action: &str) -> bool {
    !DESTRUCTIVE_ACTIONS.contains(&action)
}

/// Words that ask Duskie to do something rather than tell something.
const ACTION_WORDS: &[&str] = &[
    "open",
    "send",
    "reply",
    "draft",
    "write",
    "create",
    "add",
    "make",
    "delete",
    "remove",
    "uninstall",
    "install",
    "move",
    "rename",
    "copy",
    "turn",
    "switch",
    "set",
    "close",
    "click",
    "type",
    "fill",
    "run",
    "start",
    "stop",
    "play",
    "pause",
    "schedule",
    "book",
    "save",
    "zip",
    "convert",
    "extract",
    "clean",
    "empty",
    "kill",
    "free",
    "commit",
    "push",
    "pull",
    "merge",
    "archive",
    "mark",
    "forward",
    "accept",
    "decline",
    "cancel",
    "mute",
    "unmute",
    "launch",
    "do",
];

/// Whether the user's own words ask for an action at all.
pub fn asks_for_action(request: &str) -> bool {
    request
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| ACTION_WORDS.contains(&w))
}

/// What the user has said and pressed for one pending action.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Consent {
    /// A spoken yes.
    pub voice: bool,
    /// The on-screen confirm button.
    pub screen: bool,
}

/// Whether an action may run now.
///
/// Reads run. Writes need a spoken yes and the on-screen confirm.
/// Destructive actions need the on-screen confirm, which voice can never
/// stand in for. Nothing above Read runs at Auto trust without both, and
/// nothing above Read ever runs speculatively.
pub fn may_run(risk: Risk, consent: Consent, speculative: bool) -> bool {
    match risk {
        Risk::Read => true,
        _ if speculative => false,
        Risk::Write => consent.voice && consent.screen,
        Risk::Destructive => consent.screen,
    }
}

/// A turn knows whether untrusted text (mail, web pages, files, tool
/// results) has entered it, and what the user actually asked.
#[derive(Debug, Clone, Default)]
pub struct Turn {
    pub request: String,
    pub tainted: bool,
}

impl Turn {
    pub fn new(request: &str) -> Self {
        Self {
            request: request.to_lowercase(),
            tainted: false,
        }
    }

    /// Mail, web or file text was read in this turn.
    pub fn taint(&mut self) {
        self.tainted = true;
    }

    /// Whether the model may even propose this call. Once untrusted text is
    /// in the turn, a write or destructive call is refused unless the
    /// user's own words asked for that kind of action, so a mail that says
    /// "delete my files" cannot start a delete.
    pub fn allows(&self, name: &str) -> bool {
        let Some(t) = tool(name) else { return false };
        if t.risk == Risk::Read || !self.tainted {
            return true;
        }
        t.verbs.iter().any(|v| self.request.contains(v))
    }
}

/// Starts the answer early on a partial transcript that has stopped
/// changing, restarts when it changes, and commits at the endpoint.
#[derive(Debug, Clone)]
pub struct Speculator {
    stable_after: Duration,
    latest: String,
    changed_at: Option<Instant>,
    running: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Spec {
    /// Start (or restart) a read-only answer on this text.
    Start(String),
    /// The running answer matches the final text: keep it.
    Keep,
    /// The final text differs: restart on it.
    Restart(String),
}

impl Speculator {
    pub fn new(stable_after: Duration) -> Self {
        Self {
            stable_after,
            latest: String::new(),
            changed_at: None,
            running: None,
        }
    }

    fn norm(t: &str) -> String {
        t.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
            .trim_end_matches(['.', '?', '!', ','])
            .to_owned()
    }

    pub fn partial(&mut self, text: &str, now: Instant) {
        let n = Self::norm(text);
        if n != self.latest {
            self.latest = n;
            self.changed_at = Some(now);
        }
    }

    /// Called often; says when to start on a stable partial.
    pub fn tick(&mut self, now: Instant) -> Option<Spec> {
        let at = self.changed_at?;
        if self.latest.split_whitespace().count() < 2
            || now.saturating_duration_since(at) < self.stable_after
            || self.running.as_deref() == Some(self.latest.as_str())
        {
            return None;
        }
        self.running = Some(self.latest.clone());
        Some(Spec::Start(self.latest.clone()))
    }

    pub fn endpoint(&mut self, text: &str) -> Spec {
        let n = Self::norm(text);
        let out = if self.running.as_deref() == Some(n.as_str()) {
            Spec::Keep
        } else {
            Spec::Restart(n.clone())
        };
        *self = Self::new(self.stable_after);
        out
    }
}

/// Splits streamed text into speakable pieces: the first piece goes as
/// soon as there is a clause, the rest by sentence.
#[derive(Debug, Default)]
pub struct Chunker {
    buf: String,
    sent_first: bool,
}

/// The first piece may break at a comma once it is this long.
const FIRST_MIN: usize = 24;
/// Later pieces wait for a sentence end, unless they grow this long.
const MAX_PIECE: usize = 220;

impl Chunker {
    pub fn push(&mut self, delta: &str) -> Vec<String> {
        self.buf.push_str(delta);
        let mut out = Vec::new();
        loop {
            let cut = self.cut();
            let Some(i) = cut else { break };
            let piece: String = self.buf.drain(..i).collect();
            let piece = piece.trim().to_owned();
            if !piece.is_empty() {
                out.push(piece);
                self.sent_first = true;
            }
        }
        out
    }

    fn cut(&self) -> Option<usize> {
        let b = &self.buf;
        let mut prev = None;
        for (i, c) in b.char_indices() {
            let next = b[i + c.len_utf8()..].chars().next();
            let ends = matches!(c, '.' | '!' | '?' | '\n')
                && next.is_some_and(char::is_whitespace)
                // "e.g. " and "3.5 " are not sentence ends.
                && !prev.is_some_and(|p: char| p.is_ascii_digit() && c == '.' && next.is_some_and(|n| n.is_ascii_digit()));
            if ends || c == '\n' {
                return Some(i + c.len_utf8());
            }
            if !self.sent_first
                && matches!(c, ',' | ';' | ':')
                && i >= FIRST_MIN
                && next.is_some_and(char::is_whitespace)
            {
                return Some(i + 1);
            }
            prev = Some(c);
        }
        if b.len() > MAX_PIECE {
            return b[..MAX_PIECE].rfind(' ').map(|i| i + 1);
        }
        None
    }

    /// Whatever is left at the end of the answer.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buf).trim().to_owned();
        (!rest.is_empty()).then_some(rest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_full_turn_walks_the_states() {
        fn run(s: &mut LiveState, i: LiveInput) -> Vec<Effect> {
            let (n, e) = step(*s, i);
            *s = n;
            e
        }
        let mut s = LiveState::Idle;
        run(&mut s, LiveInput::Wake);
        assert_eq!(s, LiveState::Listening);
        run(&mut s, LiveInput::Partial("what is on".into()));
        let e = run(&mut s, LiveInput::Endpoint("what is on my calendar".into()));
        assert!(e.contains(&Effect::Commit("what is on my calendar".into())));
        run(&mut s, LiveInput::ToolStart("Checking Calendar".into()));
        assert_eq!(s, LiveState::ToolRunning);
        run(&mut s, LiveInput::ToolDone);
        run(&mut s, LiveInput::SpeechStart);
        assert_eq!(s, LiveState::Speaking);
        run(&mut s, LiveInput::SpeechEnd);
        assert_eq!(s, LiveState::Listening);
    }

    #[test]
    fn barge_in_stops_speech_and_generation() {
        let (s, e) = step(LiveState::Speaking, LiveInput::BargeIn);
        assert_eq!(s, LiveState::Interrupted);
        assert!(e.contains(&Effect::StopSpeech) && e.contains(&Effect::CancelGeneration));
        let (s, _) = step(s, LiveInput::Partial("no wait".into()));
        assert_eq!(s, LiveState::Listening);
    }

    #[test]
    fn stop_always_returns_to_idle() {
        for s in [
            LiveState::Listening,
            LiveState::Thinking,
            LiveState::ToolRunning,
            LiveState::ConfirmWait,
            LiveState::Speaking,
        ] {
            assert_eq!(step(s, LiveInput::Stop).0, LiveState::Idle);
        }
    }

    #[test]
    fn voice_alone_never_confirms_a_destructive_action() {
        let voice = Consent {
            voice: true,
            screen: false,
        };
        let screen = Consent {
            voice: false,
            screen: true,
        };
        let both = Consent {
            voice: true,
            screen: true,
        };
        assert!(!may_run(Risk::Destructive, voice, false));
        assert!(may_run(Risk::Destructive, screen, false));
        assert!(!may_run(Risk::Write, voice, false));
        assert!(!may_run(Risk::Write, screen, false));
        assert!(may_run(Risk::Write, both, false));
        assert!(!may_run(Risk::Write, both, true));
        assert!(!may_run(Risk::Destructive, both, true));
        assert!(may_run(Risk::Read, Consent::default(), true));
    }

    #[test]
    fn a_mail_that_says_delete_my_files_cannot_start_a_delete() {
        let mut t = Turn::new("Summarize my unread Gmail");
        assert!(t.allows("gmail_summarize"));
        t.taint();
        assert!(!t.allows("delete_file"));
        assert!(!t.allows("uninstall"));
        assert!(!t.allows("gmail_draft"));
        assert!(t.allows("gmail_read"));
        let mut asked = Turn::new("read the mail from Sam and draft a reply");
        asked.taint();
        assert!(asked.allows("gmail_draft"));
        assert!(!asked.allows("delete_file"));
        assert!(!Turn::new("anything").allows("format_disk"));
        assert_eq!(risk_of("format_disk"), Risk::Destructive);
    }

    #[test]
    fn voice_cannot_pick_destructive_actions() {
        assert!(!voice_may_choose("empty_recycle_bin"));
        assert!(!voice_may_choose("kill_port"));
        assert!(voice_may_choose("open_path"));
        assert!(asks_for_action("Please delete the old builds"));
        assert!(asks_for_action("open my downloads"));
        assert!(!asks_for_action("summarize my unread mail"));
        assert!(!asks_for_action("what does this page say"));
    }

    #[test]
    fn speculation_starts_on_a_stable_partial_and_keeps_or_restarts() {
        let t0 = Instant::now();
        let ms = Duration::from_millis;
        let mut s = Speculator::new(ms(250));
        s.partial("what is", t0);
        assert_eq!(s.tick(t0 + ms(300)), Some(Spec::Start("what is".into())));
        s.partial("what is on my calendar", t0 + ms(400));
        assert_eq!(s.tick(t0 + ms(500)), None);
        assert_eq!(
            s.tick(t0 + ms(700)),
            Some(Spec::Start("what is on my calendar".into()))
        );
        assert_eq!(s.tick(t0 + ms(900)), None);
        assert_eq!(s.endpoint("What is on my calendar?"), Spec::Keep);
        let mut s = Speculator::new(ms(250));
        s.partial("what is on my", t0);
        s.tick(t0 + ms(300));
        assert_eq!(
            s.endpoint("what is on my calendar tomorrow"),
            Spec::Restart("what is on my calendar tomorrow".into())
        );
    }

    #[test]
    fn chunker_speaks_the_first_clause_early_then_sentences() {
        let mut c = Chunker::default();
        assert!(c.push("You have three meetings").is_empty());
        assert_eq!(
            c.push(" tomorrow, the first at nine"),
            ["You have three meetings tomorrow,"]
        );
        assert!(
            c.push(". Version 3.5 is out")
                .iter()
                .any(|p| p == "the first at nine.")
        );
        assert!(c.push(", and more").is_empty());
        assert_eq!(c.finish().as_deref(), Some("Version 3.5 is out, and more"));
    }
}
