//! Fitting a chat into a small local model's context. Ollama silently drops
//! the start of a prompt that is too long, so the model forgets the rules and
//! the question, and re-reads everything on every step. This trims instead,
//! oldest and least needed first, and never cuts the rules or the newest
//! question.

use serde_json::Value;

/// About how many characters one token holds in English text and JSON.
pub const CHARS_PER_TOKEN: usize = 3;
/// Room left for the answer itself.
pub const ANSWER_TOKENS: usize = 900;
/// Where the fixed rules end in the system prompt; what follows (the time,
/// memory, clipboard, page) can be shortened.
const FIXED_END: &str = "\n\nRight now:";

fn len(m: &Value) -> usize {
    m["content"].as_str().map_or(0, str::len) + 40
}

fn total(messages: &[Value]) -> usize {
    messages.iter().map(len).sum()
}

fn clip(m: &mut Value, max: usize) {
    if let Some(text) = m["content"].as_str()
        && text.len() > max
    {
        let mut cut: String = text.chars().take(max).collect();
        cut.push_str("\n[cut to fit]");
        m["content"] = Value::String(cut);
    }
}

/// Trims `messages` (system first) to `budget` characters. In order: the
/// oldest earlier turns, long tool results, then the attached context at the
/// end of the system prompt. The newest user message and the rules stay.
pub fn fit(messages: &mut Vec<Value>, budget: usize) {
    if total(messages) <= budget {
        return;
    }
    // 1. Earlier turns, oldest first: everything between the system prompt
    // and the newest question.
    while total(messages) > budget {
        let newest_user = messages
            .iter()
            .rposition(|m| m["role"] == "user")
            .unwrap_or(0);
        let first = usize::from(messages.first().is_some_and(|m| m["role"] == "system"));
        if first >= newest_user {
            break;
        }
        messages.remove(first);
    }
    // 2. Tool results, the longest first, down to a useful size.
    for max in [2_000, 1_000, 500] {
        if total(messages) <= budget {
            return;
        }
        for m in messages.iter_mut().filter(|m| m["role"] == "tool") {
            clip(m, max);
        }
    }
    // 3. The changing context after the rules: clipboard, page, memory.
    let rest = total(messages.get(1..).unwrap_or_default());
    if total(messages) > budget
        && let Some(sys) = messages.first_mut().filter(|m| m["role"] == "system")
        && let Some(text) = sys["content"].as_str().map(str::to_owned)
        && let Some(at) = text.find(FIXED_END)
    {
        let rules = &text[..at];
        let ctx = &text[at..];
        let room = budget.saturating_sub(rest + rules.len() + 60);
        let kept: String = ctx.chars().take(room.max(200)).collect();
        sys["content"] = Value::String(format!("{rules}{kept}"));
    }
    // 4. Still too long: the newest question itself is huge (a pasted file).
    if total(messages) > budget
        && let Some(m) = messages.iter_mut().rev().find(|m| m["role"] == "user")
    {
        clip(m, 6_000);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn msg(role: &str, n: usize) -> Value {
        json!({ "role": role, "content": "x".repeat(n) })
    }

    #[test]
    fn leaves_a_short_chat_alone() {
        let mut m = vec![msg("system", 100), msg("user", 50)];
        let before = m.clone();
        fit(&mut m, 10_000);
        assert_eq!(m, before);
    }

    #[test]
    fn drops_oldest_turns_first_and_keeps_the_question() {
        let mut m = vec![
            msg("system", 1_000),
            json!({ "role": "user", "content": "first question" }),
            msg("assistant", 3_000),
            json!({ "role": "user", "content": "the newest question" }),
        ];
        fit(&mut m, 1_500);
        assert_eq!(m[0]["role"], "system");
        assert_eq!(m.last().unwrap()["content"], "the newest question");
        assert_eq!(m.len(), 2, "{m:?}");
    }

    #[test]
    fn shortens_tool_results_before_the_rules() {
        let mut m = vec![
            msg("system", 800),
            json!({ "role": "user", "content": "q" }),
            json!({ "role": "assistant", "content": "", "tool_calls": [] }),
            msg("tool", 9_000),
        ];
        fit(&mut m, 3_000);
        assert_eq!(m[0]["content"].as_str().unwrap().len(), 800);
        assert!(m[3]["content"].as_str().unwrap().len() <= 2_100);
    }

    #[test]
    fn trims_attached_context_but_not_the_rules() {
        let text = format!("RULES{FIXED_END}\n{}", "c".repeat(20_000));
        let mut m = vec![
            json!({ "role": "system", "content": text }),
            json!({ "role": "user", "content": "q" }),
        ];
        fit(&mut m, 2_000);
        let s = m[0]["content"].as_str().unwrap();
        assert!(s.starts_with("RULES"));
        assert!(s.len() < 2_100, "{}", s.len());
    }
}
