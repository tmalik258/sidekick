//! Commands that need no model: "turn on hotspot", "mute", "wifi off",
//! "volume 40". They run at once and can't be misread by a small model.
//! Speech-to-text slips ("horsepot") are matched too. Anything longer or
//! phrased as a question ("how do I turn on hotspot") goes to the AI.

/// A Windows setting to change, as `pc::control` takes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Command {
    pub what: &'static str,
    pub level: Option<u8>,
    /// Focus mode's length, when said.
    pub minutes: Option<u32>,
}

/// Things that can be switched on and off, with the ways people (and
/// speech-to-text) say them.
const SWITCHES: &[(&str, &[&str])] = &[
    (
        "hotspot",
        &[
            "hotspot",
            "hot spot",
            "horsepot",
            "hotpot",
            "hot pot",
            "hospot",
            "mobile hotspot",
        ],
    ),
    ("wifi", &["wifi", "wi fi", "wi-fi", "why fi", "wireless"]),
    ("bluetooth", &["bluetooth", "blue tooth", "bluetoot"]),
    (
        "airplane",
        &[
            "airplane mode",
            "aeroplane mode",
            "flight mode",
            "airplane",
            "aeroplane",
        ],
    ),
    ("dark_mode", &["dark mode", "dark theme"]),
    ("night_light", &["night light", "nightlight", "night mode"]),
    ("dnd", &["do not disturb", "dnd", "focus assist"]),
];

const ON: &[&str] = &["turn on", "switch on", "enable", "start", "on", "activate"];
const OFF: &[&str] = &[
    "turn off",
    "switch off",
    "disable",
    "stop",
    "off",
    "deactivate",
];
/// Words that may sit around a command without changing it.
const FILLER: &[&str] = &[
    "please", "the", "my", "can", "you", "could", "now", "hey", "sidekick", "for", "me", "and",
    "mode",
];
const QUESTION: &[&str] = &[
    "how", "why", "what", "when", "where", "which", "is", "does", "do", "should", "explain",
];

fn words(text: &str) -> String {
    text.to_lowercase()
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' {
                c
            } else {
                ' '
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Removes `phrase` from `text` when it is there as whole words.
fn take(text: &str, phrase: &str) -> Option<String> {
    let padded = format!(" {text} ");
    let needle = format!(" {phrase} ");
    padded.find(&needle).map(|i| {
        let rest = format!("{}{}", &padded[..i], &padded[i + needle.len() - 1..]);
        rest.split_whitespace().collect::<Vec<_>>().join(" ")
    })
}

/// True when nothing but filler is left.
fn only_filler(rest: &str) -> bool {
    rest.split_whitespace().all(|w| FILLER.contains(&w))
}

pub fn command(text: &str) -> Option<Command> {
    let t = words(text);
    let n = t.split_whitespace().count();
    if n == 0 || n > 7 {
        return None;
    }
    if !t.starts_with("do not disturb")
        && t.split_whitespace()
            .next()
            .is_some_and(|w| QUESTION.contains(&w))
    {
        return None;
    }
    // Focus mode: "focus for 45 minutes", "start focus", "stop focus".
    if let Some(c) = focus(&t) {
        return Some(c);
    }
    // Volume and mute.
    for (phrase, what) in [
        ("unmute", "mute"),
        ("mute", "mute"),
        ("volume up", "volume_up"),
        ("louder", "volume_up"),
        ("volume down", "volume_down"),
        ("quieter", "volume_down"),
        ("lock", "lock"),
        ("lock the pc", "lock"),
        ("lock my pc", "lock"),
    ] {
        if take(&t, phrase).is_some_and(|rest| only_filler(&rest)) {
            return Some(Command {
                what,
                level: None,
                minutes: None,
            });
        }
    }
    if let Some(rest) = take(&t, "volume").or_else(|| take(&t, "set volume")) {
        let rest = rest
            .replace(" to ", " ")
            .replace("percent", "")
            .replace('%', "");
        let mut level = None;
        let mut extra = false;
        for w in rest.split_whitespace() {
            match w.parse::<u8>() {
                Ok(v) if v <= 100 && level.is_none() => level = Some(v),
                _ if FILLER.contains(&w) || w == "set" || w == "to" => {}
                _ => extra = true,
            }
        }
        if let (Some(level), false) = (level, extra) {
            return Some(Command {
                what: "set_volume",
                level: Some(level),
                minutes: None,
            });
        }
    }
    // On and off switches, in either order: "turn on hotspot", "hotspot on".
    for (id, names) in SWITCHES {
        for name in *names {
            let Some(rest) = take(&t, name) else { continue };
            for (verbs, state) in [(OFF, "off"), (ON, "on")] {
                for verb in verbs {
                    if take(&rest, verb).is_some_and(|left| only_filler(&left)) {
                        return Some(Command {
                            what: what(id, state),
                            level: None,
                            minutes: None,
                        });
                    }
                }
            }
        }
    }
    None
}

/// "focus", "focus for 45 minutes", "focus mode on", "end focus".
fn focus(t: &str) -> Option<Command> {
    let rest = take(t, "focus mode").or_else(|| take(t, "focus"))?;
    let ends = OFF.iter().chain(&["end", "done", "finish", "exit"]);
    if ends
        .clone()
        .any(|v| take(&rest, v).is_some_and(|left| only_filler(&left)))
    {
        return Some(Command {
            what: "focus_off",
            level: None,
            minutes: None,
        });
    }
    let mut count: Option<u32> = None;
    let mut hours = false;
    let mut half = false;
    for w in rest.split_whitespace() {
        match w {
            _ if count.is_none() && w.parse::<u32>().is_ok() => count = w.parse().ok(),
            "an" | "a" | "one" if count.is_none() => count = Some(1),
            "half" => half = true,
            "hour" | "hours" | "hr" | "hrs" => hours = true,
            "minutes" | "minute" | "mins" | "min" | "for" | "start" | "on" | "turn" | "begin"
            | "an" | "a" | "of" => {}
            _ if FILLER.contains(&w) => {}
            _ => return None,
        }
    }
    let minutes = match (count, hours, half) {
        (Some(1), true, true) | (None, true, true) => Some(30),
        (Some(n), true, _) => Some(n * 60),
        (None, true, false) => Some(60),
        (n, false, _) => n,
    };
    Some(Command {
        what: "focus_on",
        level: None,
        minutes,
    })
}

fn what(id: &str, state: &str) -> &'static str {
    match (id, state) {
        ("hotspot", "on") => "hotspot_on",
        ("hotspot", _) => "hotspot_off",
        ("wifi", "on") => "wifi_on",
        ("wifi", _) => "wifi_off",
        ("bluetooth", "on") => "bluetooth_on",
        ("bluetooth", _) => "bluetooth_off",
        ("airplane", "on") => "airplane_on",
        ("airplane", _) => "airplane_off",
        ("dark_mode", "on") => "dark_mode_on",
        ("dark_mode", _) => "dark_mode_off",
        ("night_light", "on") => "night_light_on",
        ("night_light", _) => "night_light_off",
        ("dnd", "on") => "dnd_on",
        _ => "dnd_off",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str) -> Option<&'static str> {
        command(t).map(|c| c.what)
    }

    #[test]
    fn plain_commands_run_without_a_model() {
        assert_eq!(w("turn on hotspot"), Some("hotspot_on"));
        assert_eq!(w("turn on horsepot"), Some("hotspot_on"));
        assert_eq!(w("Hey Sidekick, hotspot off please"), Some("hotspot_off"));
        assert_eq!(w("wifi off"), Some("wifi_off"));
        assert_eq!(w("turn off the bluetooth"), Some("bluetooth_off"));
        assert_eq!(w("enable dark mode"), Some("dark_mode_on"));
        assert_eq!(w("do not disturb on"), Some("dnd_on"));
        assert_eq!(w("mute"), Some("mute"));
        assert_eq!(w("lock my pc"), Some("lock"));
        assert_eq!(
            command("set volume to 40%"),
            Some(Command {
                what: "set_volume",
                level: Some(40),
                minutes: None
            })
        );
        assert_eq!(
            command("volume 70"),
            Some(Command {
                what: "set_volume",
                level: Some(70),
                minutes: None
            })
        );
    }

    #[test]
    fn focus_by_voice() {
        let m = |t: &str| {
            command(t)
                .filter(|c| c.what == "focus_on")
                .map(|c| c.minutes)
        };
        assert_eq!(m("focus"), Some(None));
        assert_eq!(m("focus for 45 minutes"), Some(Some(45)));
        assert_eq!(m("hey sidekick focus for an hour"), Some(Some(60)));
        assert_eq!(m("focus for half an hour"), Some(Some(30)));
        assert_eq!(m("start focus mode"), Some(None));
        assert_eq!(m("focus for 2 hours"), Some(Some(120)));
        assert_eq!(w("stop focus"), Some("focus_off"));
        assert_eq!(w("end focus mode"), Some("focus_off"));
        assert_eq!(w("focus on the report"), None);
        assert_eq!(w("how do I focus"), None);
    }

    #[test]
    fn questions_and_longer_requests_go_to_the_ai() {
        assert_eq!(w("how do I turn on hotspot"), None);
        assert_eq!(
            w("turn on hotspot and send the file to ali on whatsapp"),
            None
        );
        assert_eq!(w("what is a hotspot"), None);
        assert_eq!(w("turn on the lights in the kitchen"), None);
        assert_eq!(w("volume of a sphere"), None);
        assert_eq!(w(""), None);
    }
}
