//! Text helpers: the wake phrase out of transcripts, and answers made fit to
//! be read aloud, a sentence at a time.

/// Phrases that wake Sidekick, as the wake word model spells them (BPE
/// tokens of the gigaspeech KWS model), and how they appear in transcripts.
pub const WAKE_KEYWORDS: &str = "▁HE Y ▁ SIDE K IC K @hey_sidekick\n▁HI ▁ SIDE K IC K @hey_sidekick\n▁O K ▁ SIDE K IC K @hey_sidekick\n";

/// Words a greeting can start with.
const GREETINGS: &[&str] = &["hey", "hi", "ok", "okay", "a"];

/// How the name comes out of speech to text, as one word or two: people
/// (and voices) often soften the "d", so "cider kick" is common.
const NAMES: &[&[&str]] = &[
    &["sidekick"],
    &["sidekik"],
    &["side", "kick"],
    &["cider", "kick"],
    &["side", "kik"],
    &["psych", "kick"],
];

/// After a greeting, any word starting with one of these counts as the name,
/// so accents and loose transcripts still work ("hey sidecaig", "hey sidekey").
const NAME_STARTS: &[&str] = &["side", "sight", "syde"];

/// Whether `word` could be the end of a split name ("side caig", "side kick"):
/// a short fragment with a k, c, q or g sound up front.
fn name_tail(word: &str) -> bool {
    word.len() <= 6 && word.starts_with(['k', 'c', 'q', 'g'])
}

/// The transcript without the wake phrase at its start.
pub fn strip_wake(text: &str) -> String {
    let trimmed = text.trim();
    // Words with where they end in `trimmed`, punctuation and case ignored.
    let mut words: Vec<(String, usize)> = Vec::new();
    let mut start = None;
    for (i, c) in trimmed.char_indices().chain([(trimmed.len(), ' ')]) {
        let part = c.is_alphanumeric() || c == '\'';
        match (start, part) {
            (None, true) => start = Some(i),
            (Some(s), false) => {
                words.push((trimmed[s..i].to_lowercase().replace('\'', ""), i));
                start = None;
            }
            _ => {}
        }
    }
    let name_at = |at: usize| {
        NAMES.iter().find_map(|name| {
            let fits = name.len() <= words.len().saturating_sub(at)
                && name.iter().enumerate().all(|(k, w)| words[at + k].0 == *w);
            fits.then_some(at + name.len())
        })
    };
    let first = words.first().map(|(w, _)| w.as_str());
    let end = name_at(0).or_else(|| {
        first
            .filter(|w| GREETINGS.contains(w))
            .and_then(|_| name_at(1).or_else(|| loose_name(&words)))
    });
    match end {
        Some(n) => trimmed[words[n - 1].1..]
            .trim_start_matches(|c: char| c.is_ascii_punctuation() || c.is_whitespace())
            .to_owned(),
        None => trimmed.to_owned(),
    }
}

/// "hey side*": the greeting's next word starts like the name. A bare "side"
/// or "sight" also takes a short k/c/g fragment after it ("side caig").
fn loose_name(words: &[(String, usize)]) -> Option<usize> {
    let word = words.get(1)?.0.as_str();
    if !NAME_STARTS.iter().any(|p| word.starts_with(p)) {
        return None;
    }
    let split = NAME_STARTS.contains(&word) && words.get(2).is_some_and(|(w, _)| name_tail(w));
    Some(if split { 3 } else { 2 })
}

/// Removes what sounds wrong when read aloud: markdown marks, link targets
/// and code blocks.
pub fn speakable(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut in_code = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            if !in_code {
                out.push_str(" The code is on screen. ");
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        let line = line.trim_start_matches(|c: char| c == '#' || c == '>' || c.is_whitespace());
        let line = line
            .strip_prefix("- ")
            .or_else(|| line.strip_prefix("* "))
            .unwrap_or(line);
        out.push_str(&strip_links(line));
        out.push(' ');
    }
    out.replace(['*', '_', '`', '|'], "")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `[label](url)` becomes `label`; bare URLs are dropped.
fn strip_links(line: &str) -> String {
    let mut out = String::new();
    let mut rest = line;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|i| open + i) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|i| close + i) else {
            break;
        };
        out.push_str(&rest[..open]);
        out.push_str(&rest[open + 1..close]);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out.split_whitespace()
        .filter(|w| !w.starts_with("http://") && !w.starts_with("https://"))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Collects streamed text and hands out whole sentences, so speech can
/// start before the answer is complete.
#[derive(Debug, Default)]
pub struct Sentences {
    buf: String,
}

/// Shortest piece worth synthesizing on its own (very short clips sound
/// choppy).
const MIN_SENTENCE: usize = 24;

impl Sentences {
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.buf.push_str(text);
        let mut out = Vec::new();
        while let Some(end) = self.boundary() {
            let sentence: String = self.buf.drain(..end).collect();
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                out.push(sentence.to_owned());
            }
        }
        out
    }

    /// Whatever is left, at the end of the answer.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.buf);
        let rest = rest.trim();
        (!rest.is_empty()).then(|| rest.to_owned())
    }

    /// Byte index just past the first sentence end that leaves a piece of
    /// at least [`MIN_SENTENCE`] bytes, followed by whitespace.
    fn boundary(&self) -> Option<usize> {
        let bytes = self.buf.as_bytes();
        for (i, c) in self.buf.char_indices() {
            // A line break always ends a piece (lists, code fences).
            if c == '\n' {
                return Some(i + 1);
            }
            if i + 1 < MIN_SENTENCE {
                continue;
            }
            let ends = matches!(c, '.' | '!' | '?' | ':' | ';');
            if ends
                && bytes
                    .get(i + c.len_utf8())
                    .is_some_and(|b| b.is_ascii_whitespace())
            {
                return Some(i + c.len_utf8());
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_the_wake_phrase() {
        assert_eq!(
            strip_wake(" Hey sidekick, what time is it?"),
            "what time is it?"
        );
        assert_eq!(
            strip_wake("Sidekick. Open my downloads"),
            "Open my downloads"
        );
        assert_eq!(strip_wake("Hey Sidekick"), "");
        assert_eq!(strip_wake("What is a sidekick?"), "What is a sidekick?");
        // Common mishearings of the name.
        assert_eq!(
            strip_wake("hey, cider kick, what time is it in london?"),
            "what time is it in london?"
        );
        assert_eq!(strip_wake("Okay side kick open settings"), "open settings");
        assert_eq!(strip_wake("Hi Sidekik. Pause"), "Pause");
        assert_eq!(strip_wake("hey there"), "hey there");
        assert_eq!(
            strip_wake("Hey sidecaig, open my downloads"),
            "open my downloads"
        );
        assert_eq!(
            strip_wake("hey side caig what time is it"),
            "what time is it"
        );
        assert_eq!(strip_wake("Hey sidekey. Next"), "Next");
        assert_eq!(strip_wake("hey side open settings"), "open settings");
        assert_eq!(
            strip_wake("sidecaig open settings"),
            "sidecaig open settings"
        );
    }

    #[test]
    fn makes_markdown_speakable() {
        let md = "## Steps\n- Run **cargo build**\n- See [the docs](https://x.dev) or https://y.dev\n```rust\nfn main() {}\n```\nDone.";
        assert_eq!(
            speakable(md),
            "Steps Run cargo build See the docs or The code is on screen. Done."
        );
    }

    #[test]
    fn splits_streamed_text_into_sentences() {
        let mut s = Sentences::default();
        assert!(
            s.push("Sure. It is three").is_empty(),
            "too short to speak alone"
        );
        assert_eq!(
            s.push(" in the afternoon. Anything else"),
            vec!["Sure. It is three in the afternoon."]
        );
        assert!(s.push(" for e.g").is_empty());
        assert_eq!(s.finish().as_deref(), Some("Anything else for e.g"));
        assert_eq!(s.finish(), None);
        assert_eq!(s.push("Hi\n```rust\n"), vec!["Hi", "```rust"]);
    }
}
