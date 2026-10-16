//! Where a tool can come from, which source wins when the same app is
//! connected more than one way, and the guard that keeps tools which change
//! things out of reach of the local model. Pure, so the order and the guard
//! are tested.

use serde::{Deserialize, Serialize};

/// Words in a tool's name that mean it changes something.
const WRITE_WORDS: &[&str] = &[
    "ADD",
    "APPROVE",
    "ARCHIVE",
    "ASSIGN",
    "BASH",
    "CANCEL",
    "CLOSE",
    "COMMENT",
    "COPY",
    "CREATE",
    "DELETE",
    "DISABLE",
    "EDIT",
    "ENABLE",
    "EXECUTE",
    "FORWARD",
    "INITIATE",
    "INSERT",
    "INVITE",
    "LABEL",
    "MANAGE",
    "MARK",
    "MERGE",
    "MODIFY",
    "MOVE",
    "PATCH",
    "POST",
    "PUBLISH",
    "PUT",
    "REJECT",
    "REMOVE",
    "RENAME",
    "REPLY",
    "RUN",
    "SCHEDULE",
    "SEND",
    "SET",
    "SHARE",
    "STAR",
    "START",
    "STOP",
    "SUBMIT",
    "TRANSITION",
    "TRASH",
    "TRIGGER",
    "UPDATE",
    "UPLOAD",
    "UPSERT",
    "WORKBENCH",
    "WRITE",
];

fn words(name: &str) -> impl Iterator<Item = String> + '_ {
    name.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_ascii_uppercase)
}

/// The tool's name says it changes something.
pub fn is_write(name: &str) -> bool {
    words(name).any(|w| WRITE_WORDS.contains(&w.as_str()))
}

/// Where a tool or app comes from. Earlier wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// Duskie's own built-in actions.
    Native,
    /// An MCP server the user connected directly.
    Direct,
    /// Through Composio.
    Composio,
}

impl Source {
    /// How the answer says where it came from.
    pub fn label(self) -> &'static str {
        match self {
            Source::Native => "this PC",
            Source::Direct => "direct connection",
            Source::Composio => "Composio",
        }
    }
}

/// One connected app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Connection {
    pub id: String,
    /// Lower case app name: "github", "gmail".
    pub app: String,
    pub source: Source,
    /// Tools the user allowed to change things (by exact name). Everything
    /// else that looks like a write is refused.
    #[serde(default)]
    pub writes_allowed: Vec<String>,
}

/// The connection to use for `app`: native, then direct, then Composio.
pub fn resolve<'a>(app: &str, all: &'a [Connection]) -> Option<&'a Connection> {
    let app = app.trim().to_lowercase();
    all.iter().filter(|c| c.app == app).min_by_key(|c| c.source)
}

/// An app connected through more than one source: the one that is used and
/// the ones left unused, so the user can be told once.
pub fn duplicates(all: &[Connection]) -> Vec<(String, Source, Vec<Source>)> {
    let mut apps: Vec<&str> = all.iter().map(|c| c.app.as_str()).collect();
    apps.sort_unstable();
    apps.dedup();
    apps.into_iter()
        .filter_map(|app| {
            let mut sources: Vec<Source> = all
                .iter()
                .filter(|c| c.app == app)
                .map(|c| c.source)
                .collect();
            sources.sort_unstable();
            sources.dedup();
            let (first, rest) = sources.split_first()?;
            (!rest.is_empty()).then(|| (app.to_owned(), *first, rest.to_vec()))
        })
        .collect()
}

/// Whether the local model may call `tool` on this connection: reads always,
/// writes only when the user allowed that exact tool.
pub fn may_call(tool: &str, conn: &Connection) -> bool {
    !is_write(tool) || conn.writes_allowed.iter().any(|t| t == tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str, app: &str, source: Source) -> Connection {
        Connection {
            id: id.into(),
            app: app.into(),
            source,
            writes_allowed: vec![],
        }
    }

    #[test]
    fn native_beats_direct_beats_composio() {
        let all = [
            conn("c", "github", Source::Composio),
            conn("d", "github", Source::Direct),
            conn("n", "files", Source::Native),
        ];
        assert_eq!(resolve("GitHub", &all).unwrap().id, "d");
        assert_eq!(resolve("files", &all).unwrap().source, Source::Native);
        assert!(resolve("gmail", &all).is_none());
    }

    #[test]
    fn an_app_connected_twice_is_reported_once_with_the_winner() {
        let all = [
            conn("c", "github", Source::Composio),
            conn("d", "github", Source::Direct),
            conn("g", "gmail", Source::Composio),
        ];
        let dup = duplicates(&all);
        assert_eq!(
            dup,
            vec![("github".to_owned(), Source::Direct, vec![Source::Composio])]
        );
    }

    #[test]
    fn reads_pass_and_writes_need_the_users_say_so() {
        let mut c = conn("d", "github", Source::Direct);
        assert!(may_call("list_issues", &c));
        assert!(!may_call("create_issue", &c));
        c.writes_allowed.push("create_issue".into());
        assert!(may_call("create_issue", &c));
        assert!(!may_call("delete_repo", &c));
    }

    #[test]
    fn source_names_read_plainly() {
        assert_eq!(Source::Direct.label(), "direct connection");
    }
}
