//! Warm agent sessions. Starting `claude` or `codex` costs a second or two
//! (the CLI, its MCP servers, the sign-in check), so a session is started
//! when Ask opens and kept between messages of the same chat.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::Message;

/// A session not used for this long is closed.
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// Most sessions kept per agent; the least recently used goes first.
pub const MAX: usize = 3;

struct Entry<T> {
    /// What the session was started with (exe, args, folder).
    key: String,
    /// The chat so far, answers included. Empty for a spare session that
    /// has not been used yet.
    history: Vec<Message>,
    used: Instant,
    item: T,
}

pub struct Pool<T> {
    items: Mutex<Vec<Entry<T>>>,
}

impl<T> Pool<T> {
    pub const fn new() -> Self {
        Self {
            items: Mutex::new(Vec::new()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Entry<T>>> {
        self.items.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Takes the session that holds exactly `earlier` (this chat, one
    /// message back), else a spare one. The flag says it continues a chat.
    pub fn take(&self, key: &str, earlier: &[Message]) -> Option<(T, bool)> {
        let mut items = self.lock();
        prune(&mut items);
        if !earlier.is_empty()
            && let Some(i) = items
                .iter()
                .position(|e| e.key == key && e.history == earlier)
        {
            return Some((items.swap_remove(i).item, true));
        }
        let i = items
            .iter()
            .position(|e| e.key == key && e.history.is_empty())?;
        Some((items.swap_remove(i).item, false))
    }

    /// A spare session with this key is waiting.
    pub fn has_spare(&self, key: &str) -> bool {
        let mut items = self.lock();
        prune(&mut items);
        items.iter().any(|e| e.key == key && e.history.is_empty())
    }

    pub fn put(&self, key: String, history: Vec<Message>, item: T) {
        let mut items = self.lock();
        items.push(Entry {
            key,
            history,
            used: Instant::now(),
            item,
        });
        prune(&mut items);
    }

    /// Closes every session (settings changed, or the app is quitting).
    pub fn clear(&self) {
        self.lock().clear();
    }

    /// Closes sessions idle for longer than [`IDLE`].
    pub fn sweep(&self) {
        prune(&mut self.lock());
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lock().len()
    }
}

fn prune<T>(items: &mut Vec<Entry<T>>) {
    items.retain(|e| e.used.elapsed() < IDLE);
    if items.len() > MAX {
        items.sort_by_key(|e| std::cmp::Reverse(e.used));
        items.truncate(MAX);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Role;

    fn chat(n: usize) -> Vec<Message> {
        (0..n)
            .map(|i| Message {
                role: if i % 2 == 0 {
                    Role::User
                } else {
                    Role::Assistant
                },
                content: format!("m{i}"),
            })
            .collect()
    }

    #[test]
    fn continues_the_same_chat_first() {
        let pool = Pool::new();
        pool.put("k".into(), Vec::new(), "spare");
        pool.put("k".into(), chat(2), "chat");
        assert_eq!(pool.take("k", &chat(2)), Some(("chat", true)));
        assert_eq!(pool.take("k", &chat(2)), Some(("spare", false)));
        assert_eq!(pool.take("k", &chat(2)), None);
    }

    #[test]
    fn other_settings_do_not_match() {
        let pool = Pool::new();
        pool.put("a".into(), Vec::new(), 1);
        assert!(!pool.has_spare("b"));
        assert_eq!(pool.take("b", &[]), None);
        assert!(pool.has_spare("a"));
    }

    #[test]
    fn keeps_at_most_max_sessions() {
        let pool = Pool::new();
        for i in 0..MAX + 2 {
            pool.put("k".into(), chat(i * 2 + 2), i);
        }
        assert_eq!(pool.len(), MAX);
        // The oldest went first.
        assert_eq!(pool.take("k", &chat(2)), None);
    }
}
