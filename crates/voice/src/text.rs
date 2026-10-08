//! Text helpers: the wake phrase out of transcripts, and answers made fit to
//! be read aloud, a sentence at a time.

/// Phrases that wake Sidekick, as the wake word model spells them (BPE
/// tokens of the gigaspeech KWS model), and how they appear in transcripts.
pub const WAKE_KEYWORDS: &str = "▁HE Y ▁ SIDE K IC K @hey_sidekick\n▁HI ▁ SIDE K IC K @hey_sidekick\n▁O K ▁ SIDE K IC K @hey_sidekick\n";

/// A name the user gave the assistant ("Orbi"), as lowercase words. Empty
/// means only "Sidekick". The default name always keeps working too.
static CUSTOM_NAME: std::sync::RwLock<Vec<String>> = std::sync::RwLock::new(Vec::new());

/// Sets the assistant's name for wake matching. "Sidekick" or blank clears it.
pub fn set_name(name: &str) {
    let words: Vec<String> = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_lowercase)
        .collect();
    let words = if words == ["sidekick"] {
        Vec::new()
    } else {
        words
    };
    if let Ok(mut n) = CUSTOM_NAME.write() {
        *n = words;
    }
}

fn custom_name() -> Vec<String> {
    CUSTOM_NAME.read().map(|n| n.clone()).unwrap_or_default()
}

/// Pieces of the wake model's BPE vocabulary with their merge scores.
pub type Vocab = std::collections::HashMap<String, f32>;

/// Reads the pieces and scores out of a sentencepiece `bpe.model`
/// (protobuf: field 1 repeats a piece message of string 1, float 2).
pub fn read_vocab(bytes: &[u8]) -> Vocab {
    fn varint(b: &[u8], at: &mut usize) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let byte = *b.get(*at)?;
            *at += 1;
            v |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }
    /// Each field as (number, bytes for length-delimited, fixed32 bits).
    fn fields(b: &[u8]) -> Vec<(u64, &[u8], u32)> {
        let mut out = Vec::new();
        let mut at = 0;
        while at < b.len() {
            let Some(tag) = varint(b, &mut at) else { break };
            match tag & 7 {
                0 => {
                    if varint(b, &mut at).is_none() {
                        break;
                    }
                }
                1 => at += 8,
                2 => {
                    let Some(len) = varint(b, &mut at) else { break };
                    let end = at + len as usize;
                    let Some(body) = b.get(at..end) else { break };
                    out.push((tag >> 3, body, 0));
                    at = end;
                }
                5 => {
                    let Some(w) = b.get(at..at + 4) else { break };
                    out.push((
                        tag >> 3,
                        &[][..],
                        u32::from_le_bytes([w[0], w[1], w[2], w[3]]),
                    ));
                    at += 4;
                }
                _ => break,
            }
        }
        out
    }
    let mut vocab = Vocab::new();
    for (n, body, _) in fields(bytes) {
        if n != 1 {
            continue;
        }
        let mut piece = None;
        let mut score = 0.0;
        for (m, b, bits) in fields(body) {
            match m {
                1 => piece = std::str::from_utf8(b).ok(),
                2 => score = f32::from_bits(bits),
                _ => {}
            }
        }
        if let Some(p) = piece {
            vocab.insert(p.to_owned(), score);
        }
    }
    vocab
}

/// Spells `phrase` in the wake model's pieces. None when a letter
/// has no piece.
pub fn spell(vocab: &Vocab, phrase: &str) -> Option<String> {
    let mut out = Vec::new();
    for word in phrase.split_whitespace() {
        let word: String = word
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .collect::<String>()
            .to_uppercase();
        if !word.is_empty() {
            out.extend(spell_word(vocab, &word)?);
        }
    }
    (!out.is_empty()).then(|| out.join(" "))
}

/// One word as the wake model's unigram tokenizer splits it: the split of
/// "▁WORD" into known pieces with the highest total score.
fn spell_word(vocab: &Vocab, word: &str) -> Option<Vec<String>> {
    let text = format!("▁{word}");
    let cuts: Vec<usize> = text
        .char_indices()
        .map(|(i, _)| i)
        .skip(1)
        .chain([text.len()])
        .collect();
    // best[end]: (score, pieces) of the best split of text[..end].
    let mut best: std::collections::HashMap<usize, (f32, Vec<String>)> =
        std::collections::HashMap::from([(0, (0.0, Vec::new()))]);
    for start in std::iter::once(0).chain(cuts.iter().copied()) {
        let Some((score, pieces)) = best.get(&start).cloned() else {
            continue;
        };
        for &end in cuts.iter().filter(|&&e| e > start) {
            let piece = &text[start..end];
            let Some(&s) = vocab.get(piece) else { continue };
            if best.get(&end).is_none_or(|(b, _)| score + s > *b) {
                let mut next = pieces.clone();
                next.push(piece.to_owned());
                best.insert(end, (score + s, next));
            }
        }
    }
    best.remove(&text.len()).map(|(_, p)| p)
}

/// The wake phrases for the keyword model: "hey Sidekick" always, plus
/// "hey <name>" when the user renamed the assistant.
pub fn wake_keywords(vocab: Option<&Vocab>) -> String {
    let mut out = WAKE_KEYWORDS.to_owned();
    let name = custom_name();
    if let (Some(vocab), false) = (vocab, name.is_empty()) {
        let name = name.join(" ");
        for greeting in ["hey", "hi", "ok"] {
            if let Some(spelled) = spell(vocab, &format!("{greeting} {name}")) {
                out.push_str(&format!("{spelled} @hey_{}\n", name.replace(' ', "_")));
            }
        }
    }
    out
}

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
    let custom = custom_name();
    let name_at = |at: usize| {
        let fits = |name: &[&str]| {
            name.len() <= words.len().saturating_sub(at)
                && name.iter().enumerate().all(|(k, w)| words[at + k].0 == *w)
        };
        let own: Vec<&str> = custom.iter().map(String::as_str).collect();
        if !own.is_empty() && fits(&own) {
            return Some(at + own.len());
        }
        NAMES
            .iter()
            .find_map(|name| fits(name).then_some(at + name.len()))
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

/// Where the wake phrase starts in a running transcript: a greeting and the
/// name anywhere ("so, hey sidekick, what..."), or the bare name as the
/// first word. None when it is not there.
pub fn find_wake(text: &str) -> Option<usize> {
    let mut starts = Vec::new();
    let mut in_word = false;
    for (i, c) in text.char_indices() {
        let part = c.is_alphanumeric() || c == '\'';
        if part && !in_word {
            starts.push(i);
        }
        in_word = part;
    }
    starts.into_iter().enumerate().find_map(|(n, s)| {
        let rest = &text[s..];
        let first: String = rest
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '\'')
            .collect::<String>()
            .to_lowercase();
        // "a" is a greeting only at the very start; mid-sentence it is
        // just a word ("a sidekick app").
        let greeting = GREETINGS.contains(&first.as_str()) && (n == 0 || first != "a");
        (greeting || n == 0)
            .then(|| strip_wake(rest) != rest.trim())
            .filter(|found| *found)
            .map(|_| s)
    })
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

/// Words a spoken request usually starts with.
const STARTERS: &[&str] = &[
    "what",
    "what's",
    "whats",
    "how",
    "why",
    "when",
    "where",
    "who",
    "which",
    "is",
    "are",
    "can",
    "could",
    "would",
    "will",
    "do",
    "does",
    "did",
    "should",
    "open",
    "close",
    "start",
    "stop",
    "turn",
    "set",
    "play",
    "pause",
    "mute",
    "unmute",
    "find",
    "search",
    "show",
    "tell",
    "read",
    "send",
    "reply",
    "email",
    "message",
    "call",
    "remind",
    "create",
    "make",
    "add",
    "remove",
    "delete",
    "move",
    "copy",
    "save",
    "take",
    "check",
    "switch",
    "go",
    "launch",
    "lock",
    "sleep",
    "summarize",
    "summarise",
    "translate",
    "explain",
    "write",
    "draft",
    "book",
    "schedule",
    "help",
    "give",
    "put",
    "run",
    "install",
    "update",
    "connect",
    "join",
    "silence",
    "volume",
    "louder",
    "quieter",
    "brightness",
    "dark",
    "light",
    "please",
    "hello",
    "hi",
    "hey",
    "thanks",
    "thank",
    "yes",
    "no",
    "next",
    "cancel",
    "never",
];

/// Whether a short transcript is a request rather than noise or a stray
/// sound the speech model turned into words ("byzant mixed"). Four words
/// or more always count.
pub fn looks_like_request(text: &str) -> bool {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'')
                .to_lowercase()
        })
        .filter(|w| !w.is_empty())
        .collect();
    match words.first() {
        None => false,
        Some(first) => words.len() >= 4 || STARTERS.contains(&first.as_str()),
    }
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
        // Option chips ("OPTION: Open invoice.pdf") are for the screen, and
        // table rules are noise.
        if in_code
            || line.trim_start().to_uppercase().starts_with("OPTION:")
            || line
                .trim()
                .chars()
                .all(|c| matches!(c, '|' | '-' | ':' | ' '))
        {
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
    /// A piece was handed out. Until then the first clause (up to a comma)
    /// is enough, so the voice starts sooner.
    started: bool,
}

/// Shortest piece worth synthesizing on its own (very short clips sound
/// choppy).
const MIN_SENTENCE: usize = 24;
/// Shortest first clause spoken on its own.
const MIN_CLAUSE: usize = 12;

impl Sentences {
    pub fn push(&mut self, text: &str) -> Vec<String> {
        self.buf.push_str(text);
        let mut out = Vec::new();
        while let Some(end) = self.boundary() {
            let sentence: String = self.buf.drain(..end).collect();
            let sentence = sentence.trim();
            if !sentence.is_empty() {
                out.push(sentence.to_owned());
                self.started = true;
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
            let clause = !self.started && c == ',' && i + 1 >= MIN_CLAUSE;
            if i + 1 < MIN_SENTENCE && !clause {
                continue;
            }
            let ends = clause || matches!(c, '.' | '!' | '?' | ':' | ';');
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
    #[test]
    fn tells_requests_from_noise() {
        assert!(!looks_like_request("Byzant mixed."));
        assert!(!looks_like_request("the"));
        assert!(!looks_like_request(""));
        assert!(looks_like_request("Open Slack"));
        assert!(looks_like_request("what's the time"));
        assert!(looks_like_request("mute"));
        assert!(looks_like_request("my battery is low right now"));
    }

    use super::*;

    #[test]
    fn spells_a_name_in_wake_tokens() {
        let vocab: Vocab = [
            ("▁", -1.0),
            ("H", -2.0),
            ("E", -2.0),
            ("Y", -2.0),
            ("O", -2.0),
            ("R", -2.0),
            ("B", -2.0),
            ("I", -2.0),
            ("Z", -2.0),
            ("▁HE", -3.0),
            ("OR", -4.0),
            ("▁H", -5.0),
        ]
        .into_iter()
        .map(|(p, s)| (p.to_owned(), s))
        .collect();
        assert_eq!(spell(&vocab, "hey Orbi").as_deref(), Some("▁HE Y ▁ OR B I"));
        assert_eq!(spell(&vocab, "hey Kai"), None);
    }

    #[test]
    fn spells_sidekick_like_the_built_in_phrase() {
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../apps/desktop/src-tauri/resources/voice-models/",
            "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01/bpe.model"
        );
        // The model is downloaded on first build; skip when it is not here.
        let Ok(bytes) = std::fs::read(path) else {
            return;
        };
        let vocab = read_vocab(&bytes);
        assert_eq!(
            spell(&vocab, "hey sidekick").as_deref(),
            Some("▁HE Y ▁ SIDE K IC K")
        );
        assert_eq!(
            spell(&vocab, "ok sidekick").as_deref(),
            Some("▁O K ▁ SIDE K IC K")
        );
        // Checked against the sentencepiece library.
        assert_eq!(spell(&vocab, "hey orbi").as_deref(), Some("▁HE Y ▁OR B I"));
        assert_eq!(spell(&vocab, "light up").as_deref(), Some("▁ L IGHT ▁UP"));
    }

    #[test]
    fn a_custom_name_wakes_and_sidekick_still_does() {
        set_name("Orbi");
        assert_eq!(strip_wake("hey orbi, open mail"), "open mail");
        assert_eq!(strip_wake("hey sidekick open mail"), "open mail");
        assert!(find_wake("so hey orbi what time is it").is_some());
        set_name("Sidekick");
        assert_eq!(strip_wake("hey orbi open mail"), "hey orbi open mail");
    }

    #[test]
    fn options_are_not_read_aloud() {
        assert_eq!(
            speakable("Done, it is on.\nOPTION: Turn it off\noption: Open Settings"),
            "Done, it is on."
        );
    }

    #[test]
    fn finds_the_wake_phrase_mid_sentence() {
        assert_eq!(find_wake("hey sidekick what time is it"), Some(0));
        let t = "so anyway hey sidekick open my notes";
        assert_eq!(&t[find_wake(t).unwrap()..], "hey sidekick open my notes");
        assert_eq!(find_wake("sidekick open my notes"), Some(0));
        assert_eq!(find_wake("i built a sidekick app"), None);
        assert_eq!(find_wake("my sidekick is great"), None);
        assert_eq!(find_wake("ok cider kick play music"), Some(0));
        assert_eq!(find_wake("nothing to see here"), None);
    }

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

    #[test]
    fn the_first_clause_is_spoken_on_its_own() {
        let mut s = Sentences::default();
        assert!(s.push("Yes, it").is_empty(), "too short for a clause");
        let mut s = Sentences::default();
        assert_eq!(
            s.push("Your next meeting, the design review, starts"),
            vec!["Your next meeting,"]
        );
        assert!(
            s.push(" soon").is_empty(),
            "later commas wait for the whole sentence"
        );
        assert_eq!(
            s.push(" at three. Then"),
            vec!["the design review, starts soon at three."]
        );
    }
}
