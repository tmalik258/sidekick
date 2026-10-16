//! How much a notification deserves the user's attention.
//!
//! Everything here is pure and local: a [`Signal`] is scored from plain
//! features (no message text is stored), the score becomes a [`Level`], and
//! a [`Policy`] turns the level plus the user's context into how it is shown
//! (full pill, compact pill, held for the digest) and whether it makes a
//! sound. [`Lessons`] move a sender's or source's score a little when the
//! user acts on, or asks for less of, what they send.

use std::collections::{BTreeMap, HashMap, VecDeque};

use serde::{Deserialize, Serialize};

// ---- Signals and scoring --------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ci {
    None,
    Red,
    Green,
}

/// What kind of thing arrived, reduced to features. No subject, body or
/// address is kept; `key` names the sender or source for learning only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signal {
    /// Sender or source id used for learning, such as "mail:boss@x" or "github:ci".
    pub key: String,
    /// Coarse source, used for cooldown and batching: "mail", "github", "chat".
    pub source: String,
    pub sender_important: bool,
    pub direct_to_me: bool,
    pub question: bool,
    /// Minutes since it arrived without a reply.
    pub unanswered_mins: u32,
    pub deadline_words: bool,
    pub ci: Ci,
    /// The CI run or conflict is on my own branch or PR.
    pub mine: bool,
    pub merge_conflict: bool,
    /// Newsletters, promotions and automated digests.
    pub bulk: bool,
}

impl Signal {
    pub fn new(key: &str, source: &str) -> Self {
        Self {
            key: key.to_owned(),
            source: source.to_owned(),
            sender_important: false,
            direct_to_me: false,
            question: false,
            unanswered_mins: 0,
            deadline_words: false,
            ci: Ci::None,
            mine: false,
            merge_conflict: false,
            bulk: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    ImportantSender,
    DirectToYou,
    QuestionAsked,
    WaitingForReply,
    Deadline,
    CiRedOnYourWork,
    CiRedElsewhere,
    MergeConflictOnYourWork,
    MergeConflictElsewhere,
    Bulk,
    LearnedMore,
    LearnedLess,
}

impl Reason {
    /// A short plain sentence for the Why control.
    pub fn text(self) -> &'static str {
        match self {
            Reason::ImportantSender => "From someone you often act on",
            Reason::DirectToYou => "Sent to you directly",
            Reason::QuestionAsked => "Asks you a question",
            Reason::WaitingForReply => "Has waited a while for a reply",
            Reason::Deadline => "Mentions a deadline",
            Reason::CiRedOnYourWork => "Checks are failing on your work",
            Reason::CiRedElsewhere => "Checks are failing on someone else's work",
            Reason::MergeConflictOnYourWork => "Your branch has a merge conflict",
            Reason::MergeConflictElsewhere => "A merge conflict on someone else's branch",
            Reason::Bulk => "Looks like a newsletter or automated mail",
            Reason::LearnedMore => "You usually act on these",
            Reason::LearnedLess => "You asked for less of these",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scored {
    pub score: i32,
    pub level: Level,
    pub reasons: Vec<Reason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Level {
    /// Held for the digest; never interrupts.
    Silent,
    /// A small pill, no sound unless the profile adds a soft one.
    Quiet,
    /// A full pill with sound.
    Normal,
    /// Breaks through focus and fullscreen.
    Urgent,
}

impl Level {
    /// One step quieter ("Show less").
    pub fn lower(self) -> Self {
        match self {
            Level::Urgent => Level::Normal,
            Level::Normal => Level::Quiet,
            Level::Quiet | Level::Silent => Level::Silent,
        }
    }
}

const QUIET_AT: i32 = 25;
const NORMAL_AT: i32 = 55;
const URGENT_AT: i32 = 85;

/// Scores a signal and explains why. `lessons` shifts the score by what the
/// user has done with this sender or source before.
pub fn score(signal: &Signal, lessons: &Lessons, now: i64) -> Scored {
    let mut score = 0;
    let mut reasons = Vec::new();
    let mut add = |points: i32, reason: Reason| {
        score += points;
        reasons.push(reason);
    };
    if signal.sender_important {
        add(30, Reason::ImportantSender);
    }
    if signal.direct_to_me {
        add(20, Reason::DirectToYou);
    }
    if signal.question {
        add(25, Reason::QuestionAsked);
    }
    if signal.unanswered_mins >= 60 {
        add(10, Reason::WaitingForReply);
    }
    if signal.deadline_words {
        add(15, Reason::Deadline);
    }
    match (signal.ci, signal.mine) {
        (Ci::Red, true) => add(60, Reason::CiRedOnYourWork),
        (Ci::Red, false) => add(8, Reason::CiRedElsewhere),
        _ => {}
    }
    if signal.merge_conflict {
        if signal.mine {
            add(55, Reason::MergeConflictOnYourWork);
        } else {
            add(5, Reason::MergeConflictElsewhere);
        }
    }
    if signal.bulk {
        add(-30, Reason::Bulk);
    }
    let shift = lessons.shift(&signal.key, now);
    if shift > 0 {
        add(shift, Reason::LearnedMore);
    } else if shift < 0 {
        add(shift, Reason::LearnedLess);
    }
    let level = if score >= URGENT_AT {
        Level::Urgent
    } else if score >= NORMAL_AT {
        Level::Normal
    } else if score >= QUIET_AT {
        Level::Quiet
    } else {
        Level::Silent
    };
    Scored {
        score,
        level,
        reasons,
    }
}

// ---- Learning -------------------------------------------------------------

const DAY: i64 = 86_400;
/// Counts halve this often, so old habits fade.
const HALF_LIFE: i64 = 30 * DAY;
const MAX_ENTRIES: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lesson {
    pub acted: f32,
    pub show_less: f32,
    pub updated: i64,
}

/// What the user has done with each sender or source. Dismissing records
/// nothing: only acting and "Show less" teach anything.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Lessons {
    entries: BTreeMap<String, Lesson>,
}

impl Lessons {
    fn decayed(l: &Lesson, now: i64) -> (f32, f32) {
        let age = (now - l.updated).max(0) as f32;
        let f = 0.5_f32.powf(age / HALF_LIFE as f32);
        (l.acted * f, l.show_less * f)
    }

    /// Points added to (or taken from) the score for this key.
    pub fn shift(&self, key: &str, now: i64) -> i32 {
        let Some(l) = self.entries.get(key) else {
            return 0;
        };
        let (acted, less) = Self::decayed(l, now);
        ((acted * 6.0 - less * 14.0).round() as i32).clamp(-45, 20)
    }

    fn touch(&mut self, key: &str, now: i64, f: impl FnOnce(&mut Lesson)) {
        let (acted, show_less) = self
            .entries
            .get(key)
            .map_or((0.0, 0.0), |l| Self::decayed(l, now));
        let mut l = Lesson {
            acted,
            show_less,
            updated: now,
        };
        f(&mut l);
        self.entries.insert(key.to_owned(), l);
        if self.entries.len() > MAX_ENTRIES
            && let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, l)| l.updated)
                .map(|(k, _)| k.clone())
        {
            self.entries.remove(&oldest);
        }
    }

    /// The user opened, replied to or otherwise acted on it.
    pub fn acted(&mut self, key: &str, now: i64) {
        self.touch(key, now, |l| l.acted += 1.0);
    }

    /// The user chose Show less.
    pub fn show_less(&mut self, key: &str, now: i64) {
        self.touch(key, now, |l| l.show_less += 1.0);
    }

    /// Plain Dismiss: counts for nothing.
    pub fn dismissed(&mut self, _key: &str, _now: i64) {}

    pub fn reset(&mut self, key: &str) -> bool {
        self.entries.remove(key).is_some()
    }

    /// Forgets every key that starts with `prefix` ("mail:" for a group).
    pub fn reset_group(&mut self, prefix: &str) -> usize {
        let before = self.entries.len();
        self.entries.retain(|k, _| !k.starts_with(prefix));
        before - self.entries.len()
    }

    pub fn reset_all(&mut self) {
        self.entries.clear();
    }

    /// What Settings lists: key, current shift, and when it last changed,
    /// most recent first.
    pub fn list(&self, now: i64) -> Vec<(String, i32, i64)> {
        let mut out: Vec<_> = self
            .entries
            .iter()
            .map(|(k, l)| (k.clone(), self.shift(k, now), l.updated))
            .collect();
        out.sort_by(|a, b| b.2.cmp(&a.2).then_with(|| a.0.cmp(&b.0)));
        out
    }
}

// ---- Policy ---------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Profile {
    /// Quiet things show as a compact pill with a soft sound.
    #[default]
    Normal,
    /// Compact for most things; sound only for Urgent.
    Calm,
}

/// What the user is doing right now.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Context {
    pub focus: bool,
    pub in_call: bool,
    pub fullscreen: bool,
    pub night: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    Full,
    Compact,
    /// Not shown now: kept for the digest.
    Held,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Delivery {
    pub shape: Shape,
    pub sound: bool,
    /// Soft sound instead of the normal one.
    pub soft: bool,
}

impl Delivery {
    /// Whether this breaks the user's concentration: anything visible with
    /// a sound, or any full pill.
    pub fn interrupts(self) -> bool {
        self.shape == Shape::Full || (self.shape == Shape::Compact && self.sound)
    }
}

const HELD: Delivery = Delivery {
    shape: Shape::Held,
    sound: false,
    soft: false,
};

pub fn deliver(level: Level, profile: Profile, ctx: Context) -> Delivery {
    // Urgent is the only thing that gets through focus, a call or a game.
    if level == Level::Urgent {
        return Delivery {
            shape: Shape::Full,
            sound: !ctx.in_call && !ctx.night,
            soft: false,
        };
    }
    if ctx.focus || ctx.in_call || ctx.fullscreen || level == Level::Silent {
        return HELD;
    }
    let quiet_hours = ctx.night;
    match (level, profile) {
        (Level::Normal, Profile::Normal) => Delivery {
            shape: Shape::Full,
            sound: !quiet_hours,
            soft: false,
        },
        (Level::Normal, Profile::Calm) => Delivery {
            shape: Shape::Compact,
            sound: false,
            soft: false,
        },
        (Level::Quiet, Profile::Normal) => Delivery {
            shape: Shape::Compact,
            sound: !quiet_hours,
            soft: true,
        },
        (Level::Quiet, Profile::Calm) => Delivery {
            shape: Shape::Compact,
            sound: false,
            soft: false,
        },
        _ => HELD,
    }
}

// ---- Batching -------------------------------------------------------------

/// Same source within this many seconds joins the open batch.
const BURST_SECS: i64 = 90;
/// After one interruption from a source, others from it wait this long.
const COOLDOWN_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Batch {
    /// Show it now.
    Show,
    /// Part of a burst already shown: fold it into that item.
    Merge,
    /// The source is cooling down: hold it for the digest.
    Hold,
}

/// Turns bursts into one item and rate-limits each source.
#[derive(Debug, Default)]
pub struct Batcher {
    last_shown: HashMap<String, i64>,
    recent: VecDeque<(String, i64)>,
}

impl Batcher {
    /// Decides what to do with an item for `source` at `now` (seconds).
    /// Urgent items always show.
    pub fn admit(&mut self, source: &str, level: Level, now: i64) -> Batch {
        self.recent.retain(|(_, t)| now - t <= BURST_SECS);
        let burst = self.recent.iter().any(|(s, _)| s == source);
        self.recent.push_back((source.to_owned(), now));
        if level == Level::Urgent {
            self.last_shown.insert(source.to_owned(), now);
            return Batch::Show;
        }
        if burst {
            return Batch::Merge;
        }
        if let Some(&t) = self.last_shown.get(source)
            && now - t < COOLDOWN_SECS
        {
            return Batch::Hold;
        }
        self.last_shown.insert(source.to_owned(), now);
        Batch::Show
    }
}

// ---- Metrics --------------------------------------------------------------

/// Counters only: how many, never what.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    pub shown: u32,
    pub accepted: u32,
    pub dismissed: u32,
    pub held: u32,
    pub interruptions: u32,
}

impl Metrics {
    pub fn delivered(&mut self, d: Delivery) {
        match d.shape {
            Shape::Held => self.held += 1,
            _ => {
                self.shown += 1;
                if d.interrupts() {
                    self.interruptions += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;

    fn sc(s: &Signal) -> Scored {
        score(s, &Lessons::default(), NOW)
    }

    fn question_mail() -> Signal {
        let mut s = Signal::new("mail:client@x", "mail");
        s.direct_to_me = true;
        s.question = true;
        s
    }

    // Golden cases.

    #[test]
    fn question_email_is_quiet_or_better() {
        let r = sc(&question_mail());
        assert!(r.level >= Level::Quiet, "{r:?}");
        assert!(r.reasons.contains(&Reason::QuestionAsked));
    }

    #[test]
    fn newsletter_stays_silent() {
        let mut s = Signal::new("mail:news@x", "mail");
        s.bulk = true;
        assert_eq!(sc(&s).level, Level::Silent);
    }

    #[test]
    fn ci_red_on_my_pr_is_normal_or_urgent() {
        let mut s = Signal::new("github:ci", "github");
        s.ci = Ci::Red;
        s.mine = true;
        assert!(sc(&s).level >= Level::Normal);
    }

    #[test]
    fn ci_red_elsewhere_is_silent() {
        let mut s = Signal::new("github:ci", "github");
        s.ci = Ci::Red;
        assert_eq!(sc(&s).level, Level::Silent);
    }

    #[test]
    fn merge_conflict_on_my_branch_is_normal() {
        let mut s = Signal::new("github:pr", "github");
        s.merge_conflict = true;
        s.mine = true;
        assert!(sc(&s).level >= Level::Normal);
    }

    #[test]
    fn vip_question_is_urgent() {
        let mut s = question_mail();
        s.sender_important = true;
        s.deadline_words = true;
        let r = sc(&s);
        assert_eq!(r.level, Level::Urgent, "{r:?}");
    }

    #[test]
    fn every_feature_explains_itself() {
        let mut s = Signal::new("k", "mail");
        s.sender_important = true;
        s.direct_to_me = true;
        s.deadline_words = true;
        s.unanswered_mins = 120;
        let r = sc(&s);
        for reason in [
            Reason::ImportantSender,
            Reason::DirectToYou,
            Reason::Deadline,
            Reason::WaitingForReply,
        ] {
            assert!(r.reasons.contains(&reason));
            assert!(!reason.text().is_empty());
        }
    }

    // Learning.

    #[test]
    fn acting_raises_and_show_less_lowers() {
        let mut l = Lessons::default();
        let s = Signal::new("mail:a", "mail");
        assert_eq!(score(&s, &l, NOW).score, 0);
        l.acted("mail:a", NOW);
        l.acted("mail:a", NOW);
        assert!(score(&s, &l, NOW).score > 0);
        l.reset("mail:a");
        l.show_less("mail:a", NOW);
        let r = score(&s, &l, NOW);
        assert!(r.score < 0);
        assert!(r.reasons.contains(&Reason::LearnedLess));
    }

    #[test]
    fn dismiss_teaches_nothing() {
        let mut l = Lessons::default();
        for _ in 0..10 {
            l.dismissed("mail:a", NOW);
        }
        assert_eq!(l, Lessons::default());
    }

    #[test]
    fn learning_is_bounded_and_fades() {
        let mut l = Lessons::default();
        for _ in 0..100 {
            l.acted("k", NOW);
        }
        assert_eq!(l.shift("k", NOW), 20);
        for _ in 0..100 {
            l.show_less("j", NOW);
        }
        assert_eq!(l.shift("j", NOW), -45);
        assert_eq!(l.shift("j", NOW + 400 * DAY), 0);
    }

    #[test]
    fn storage_is_bounded_and_resettable() {
        let mut l = Lessons::default();
        for i in 0..(MAX_ENTRIES as i64 + 50) {
            l.acted(&format!("mail:{i}"), NOW + i);
        }
        assert_eq!(l.list(NOW).len(), MAX_ENTRIES);
        assert_eq!(l.reset_group("mail:"), MAX_ENTRIES);
        l.acted("chat:a", NOW);
        l.reset_all();
        assert!(l.list(NOW).is_empty());
    }

    // Policy.

    #[test]
    fn default_profile_shows_normal_full_with_sound_and_quiet_as_soft_pill() {
        let c = Context::default();
        let n = deliver(Level::Normal, Profile::Normal, c);
        assert_eq!((n.shape, n.sound), (Shape::Full, true));
        let q = deliver(Level::Quiet, Profile::Normal, c);
        assert_eq!((q.shape, q.sound, q.soft), (Shape::Compact, true, true));
    }

    #[test]
    fn calm_profile_is_silent_except_urgent() {
        let c = Context::default();
        assert!(!deliver(Level::Normal, Profile::Calm, c).sound);
        assert!(!deliver(Level::Quiet, Profile::Calm, c).sound);
        assert!(deliver(Level::Urgent, Profile::Calm, c).sound);
    }

    #[test]
    fn context_holds_everything_but_urgent() {
        for ctx in [
            Context {
                focus: true,
                ..Context::default()
            },
            Context {
                in_call: true,
                ..Context::default()
            },
            Context {
                fullscreen: true,
                ..Context::default()
            },
        ] {
            assert_eq!(
                deliver(Level::Normal, Profile::Normal, ctx).shape,
                Shape::Held
            );
            assert_eq!(
                deliver(Level::Urgent, Profile::Normal, ctx).shape,
                Shape::Full
            );
        }
        let call = Context {
            in_call: true,
            ..Context::default()
        };
        assert!(!deliver(Level::Urgent, Profile::Normal, call).sound);
    }

    #[test]
    fn show_less_steps_down_to_silent() {
        assert_eq!(Level::Urgent.lower(), Level::Normal);
        assert_eq!(Level::Quiet.lower(), Level::Silent);
        assert_eq!(Level::Silent.lower(), Level::Silent);
    }

    // Batching.

    #[test]
    fn bursts_merge_and_sources_cool_down() {
        let mut b = Batcher::default();
        assert_eq!(b.admit("mail", Level::Normal, NOW), Batch::Show);
        assert_eq!(b.admit("mail", Level::Normal, NOW + 10), Batch::Merge);
        assert_eq!(b.admit("mail", Level::Normal, NOW + 300), Batch::Hold);
        assert_eq!(b.admit("github", Level::Normal, NOW + 300), Batch::Show);
        assert_eq!(b.admit("mail", Level::Normal, NOW + 700), Batch::Show);
        assert_eq!(b.admit("mail", Level::Urgent, NOW + 710), Batch::Show);
    }

    // A noisy hour.

    #[test]
    fn a_noisy_hour_costs_at_most_two_interruptions() {
        let mut b = Batcher::default();
        let mut m = Metrics::default();
        let lessons = Lessons::default();
        let mut events: Vec<(i64, Signal)> = Vec::new();
        for i in 0..8 {
            let mut s = Signal::new(&format!("mail:news{i}"), "mail");
            s.bulk = true;
            events.push((i * 200, s));
        }
        for i in 0..6 {
            let mut s = Signal::new("github:ci", "github");
            s.ci = Ci::Red;
            events.push((100 + i * 240, s));
        }
        for i in 0..4 {
            let mut s = question_mail();
            s.key = format!("mail:person{i}");
            events.push((150 + i * 700, s));
        }
        let mut mine = Signal::new("github:ci", "github");
        mine.ci = Ci::Red;
        mine.mine = true;
        events.push((1800, mine));
        let mut conflict = Signal::new("github:pr", "github");
        conflict.merge_conflict = true;
        events.push((2400, conflict));
        assert_eq!(events.len(), 20);
        events.sort_by_key(|(t, _)| *t);
        for (t, s) in events {
            let r = score(&s, &lessons, NOW);
            let d = match b.admit(&s.source, r.level, NOW + t) {
                Batch::Show => deliver(r.level, Profile::Normal, Context::default()),
                Batch::Merge | Batch::Hold => HELD,
            };
            m.delivered(d);
        }
        assert!(m.interruptions <= 2, "{m:?}");
        assert!(m.held >= 15, "{m:?}");
    }

    #[test]
    fn signal_holds_no_message_text() {
        // The type has no field that could carry a subject, body or address.
        let json = serde_json::to_string(&question_mail()).unwrap();
        assert!(!json.contains("body") && !json.contains("subject"));
    }
}
