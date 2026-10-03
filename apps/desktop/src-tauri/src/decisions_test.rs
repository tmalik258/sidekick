//! Sidekick's everyday decisions against real requests, as tables: what
//! counts as a multi-step task, which steps wait for a tap, how a
//! notification is sorted, what a copied text is, and when a recipe fires.
//! Each table reports every wrong row at once, so a change in one rule shows
//! all it affects.

use chrono::NaiveDate;
use serde_json::json;
use sidekick_core::{AgentSettings, NotificationSettings, Trigger};

use crate::act::{Gate, Risk, decide, risk};
use crate::inbox::{Level, sort};
use crate::recipes::{event_matches, notification_matches, time_due};

fn report(table: &str, wrong: Vec<String>) {
    assert!(
        wrong.is_empty(),
        "{table}: {} wrong\n{}",
        wrong.len(),
        wrong.join("\n")
    );
}

#[test]
fn which_requests_are_tasks() {
    let rows: &[(&str, bool)] = &[
        ("Reply to Ali and attach the invoice", true),
        ("find the contract then email it to Sara", true),
        (
            "book a meeting with the design team and send the agenda",
            true,
        ),
        ("move the PDFs to Invoices then zip them", true),
        ("install VS Code and create a project folder", true),
        ("fill the leave form", false),
        ("what's on my screen", false),
        ("how much battery is left", false),
        ("turn on dark mode", false),
        ("open my last download", false),
        ("summarize this page", false),
        ("what did Ali send me today", false),
    ];
    let wrong = rows
        .iter()
        .filter(|(q, want)| crate::ai::looks_multistep(q) != *want)
        .map(|(q, want)| format!("  {q:?}: expected multistep={want}"))
        .collect();
    report("multistep", wrong);
}

#[test]
fn which_steps_wait_for_a_tap() {
    let outward = AgentSettings::default();
    let each = AgentSettings {
        ask: "each".into(),
        ..AgentSettings::default()
    };
    let mut allow_wa = AgentSettings::default();
    allow_wa.places.insert("whatsapp".into(), "allow".into());
    allow_wa.places.insert("bank.com".into(), "never".into());

    // (settings, place, action, kind, label, key, expected)
    let rows: Vec<(&AgentSettings, &str, &str, &str, &str, &str, Gate)> = vec![
        (
            &outward,
            "mail.google.com",
            "click",
            "button",
            "Send",
            "",
            Gate::Tap,
        ),
        (
            &outward,
            "mail.google.com",
            "click",
            "link",
            "Inbox",
            "",
            Gate::Run,
        ),
        (
            &outward,
            "mail.google.com",
            "type",
            "textbox",
            "To",
            "",
            Gate::Run,
        ),
        (
            &outward,
            "amazon.com",
            "click",
            "button",
            "Place order",
            "",
            Gate::Tap,
        ),
        (
            &outward,
            "whatsapp",
            "press",
            "textbox",
            "Type a message",
            "Enter",
            Gate::Tap,
        ),
        (
            &outward,
            "chrome",
            "press",
            "searchbox",
            "Search",
            "Enter",
            Gate::Run,
        ),
        (
            &outward,
            "explorer",
            "click",
            "button",
            "Delete",
            "",
            Gate::Tap,
        ),
        (
            &each,
            "notepad",
            "type",
            "document",
            "Text editor",
            "",
            Gate::Tap,
        ),
        (&each, "outlook", "click", "button", "Send", "", Gate::Tap),
        (
            &allow_wa,
            "whatsapp",
            "click",
            "button",
            "Send",
            "",
            Gate::Run,
        ),
        (
            &allow_wa,
            "web.whatsapp.com",
            "click",
            "button",
            "Send",
            "",
            Gate::Run,
        ),
        (
            &allow_wa,
            "whatsapp",
            "click",
            "button",
            "Delete chat",
            "",
            Gate::Tap,
        ),
        (
            &allow_wa,
            "online.bank.com",
            "click",
            "link",
            "Statements",
            "",
            Gate::Refuse,
        ),
        (&allow_wa, "slack", "click", "button", "Send", "", Gate::Tap),
    ];
    let wrong = rows
        .iter()
        .filter_map(|(s, place, action, kind, label, key, want)| {
            let r = risk(action, kind, label, key);
            let got = decide(s, place, r);
            (got != *want).then(|| {
                format!(
                    "  ask={} {place}: {action} {kind} {label:?} {key}: risk {r:?}, got {got:?}, expected {want:?}",
                    s.ask
                )
            })
        })
        .collect();
    report("tap", wrong);

    // "Every step" waits even for a plain step like typing.
    assert_eq!(risk("type", "document", "Text editor", ""), Risk::Step);
}

#[test]
fn how_notifications_are_sorted() {
    let mut s = NotificationSettings {
        vip: vec!["Sara".into()],
        ..NotificationSettings::default()
    };
    s.apps.insert("Spotify".into(), "never".into());
    s.apps.insert("Cursor".into(), "soon".into());
    let rows: &[(&str, &str, &str, Level)] = &[
        ("WhatsApp", "Sara Ahmed", "are you joining?", Level::Now),
        (
            "WhatsApp",
            "Ali Khan",
            "Can you send the invoice?",
            Level::Soon,
        ),
        (
            "Slack",
            "#general",
            "Bilal mentioned you in #release",
            Level::Soon,
        ),
        (
            "Outlook",
            "Microsoft account",
            "Your security code is 482913",
            Level::Now,
        ),
        (
            "Outlook",
            "Security alert",
            "New sign-in to your account",
            Level::Now,
        ),
        (
            "Chrome",
            "Payment declined",
            "Your card was declined at Netflix",
            Level::Now,
        ),
        ("Teams", "Standup", "starts in 5 minutes", Level::Now),
        (
            "Zen",
            "Daraz",
            "Flash sale: 50% off today only",
            Level::Digest,
        ),
        (
            "Windows Update",
            "Restart to update",
            "Updates are ready",
            Level::Digest,
        ),
        (
            "Spotify",
            "New release",
            "Your daily mix is ready",
            Level::Never,
        ),
        ("Cursor", "Build finished", "Task completed", Level::Soon),
        ("Files", "Copy finished", "12 items copied", Level::Digest),
    ];
    let wrong = rows
        .iter()
        .filter_map(|(app, title, body, want)| {
            let got = sort(app, title, body, &s);
            (got.level != *want).then(|| {
                format!(
                    "  {app}: {title:?} / {body:?}: got {:?} ({}), expected {want:?}",
                    got.level, got.why
                )
            })
        })
        .collect();
    report("notifications", wrong);
    assert_eq!(
        sort(
            "Outlook",
            "Microsoft account",
            "Your security code is 482913",
            &s
        )
        .code,
        Some("482913".into()),
        "the code is lifted out for one-tap copy"
    );
}

#[test]
fn what_copied_text_is() {
    let today = NaiveDate::from_ymd_opt(2026, 10, 3).expect("date");
    let rows: &[(&str, Option<&str>)] = &[
        ("37.27.195.216", Some("ip")),
        ("192.168.1.10", Some("ip")),
        ("+92 300 1234567", Some("phone")),
        ("0300-1234567", Some("phone")),
        ("next Friday 3pm", Some("date")),
        ("12 Oct 2026", Some("date")),
        ("hello world", None),
        ("", None),
    ];
    let wrong = rows
        .iter()
        .filter_map(|(text, want)| {
            let got = sidekick_sensors::entity::detect(text, today);
            let kind = got
                .as_ref()
                .and_then(|v| v["entity"].as_str().map(str::to_owned));
            (kind.as_deref() != *want)
                .then(|| format!("  {text:?}: got {kind:?}, expected {want:?}"))
        })
        .collect();
    report("entities", wrong);
}

#[test]
fn when_recipes_fire() {
    let days = |d: &[&str]| d.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    let time_rows: &[(&str, Vec<String>, &str, &str, bool)] = &[
        ("17:00", days(&["fri"]), "17:00", "fri", true),
        ("17:00", days(&["fri"]), "17:00", "thu", false),
        ("9:05", days(&[]), "09:05", "mon", true),
        (
            "09:00",
            days(&["Monday", "Wednesday"]),
            "09:00",
            "wed",
            true,
        ),
        ("17:00", days(&[]), "17:01", "fri", false),
    ];
    let mut wrong: Vec<String> = time_rows
        .iter()
        .filter(|(t, d, now, day, want)| time_due(t, d, now, day) != *want)
        .map(|(t, d, now, day, want)| format!("  time {t} {d:?} at {now} {day}: expected {want}"))
        .collect();

    let invoice = Trigger::Notification {
        app: "WhatsApp".into(),
        contains: "invoice".into(),
    };
    let any_slack = Trigger::Notification {
        app: "slack".into(),
        contains: String::new(),
    };
    let notif_rows: &[(&Trigger, &str, &str, &str, bool)] = &[
        (
            &invoice,
            "WhatsApp",
            "Ali",
            "Here is the Invoice for May",
            true,
        ),
        (&invoice, "WhatsApp", "Ali", "Lunch?", false),
        (&invoice, "Slack", "Ali", "invoice attached", false),
        (&any_slack, "Slack", "#dev", "build failed", true),
    ];
    wrong.extend(
        notif_rows
            .iter()
            .filter(|(t, from, title, body, want)| {
                notification_matches(t, from, title, body) != *want
            })
            .map(|(_, from, title, body, want)| {
                format!("  notification {from} {title:?} {body:?}: expected {want}")
            }),
    );

    let pdf = Trigger::Download { kind: "pdf".into() };
    let cursor = Trigger::AppOpened {
        app: "Cursor".into(),
    };
    let event_rows = [
        (
            &pdf,
            "file.download_completed",
            json!({"name": "invoice.pdf", "ext": "pdf"}),
            true,
        ),
        (
            &pdf,
            "file.download_completed",
            json!({"name": "photo.jpg", "ext": "jpg"}),
            false,
        ),
        (
            &Trigger::MeetingEnded,
            "calendar.meeting_ended",
            json!({"title": "Standup"}),
            true,
        ),
        (&cursor, "window.focused", json!({"app": "cursor"}), true),
        (&cursor, "window.focused", json!({"app": "zen"}), false),
    ];
    wrong.extend(
        event_rows
            .iter()
            .filter(|(t, kind, payload, want)| event_matches(t, kind, payload).is_some() != *want)
            .map(|(_, kind, payload, want)| format!("  event {kind} {payload}: expected {want}")),
    );
    report("recipes", wrong);
}
