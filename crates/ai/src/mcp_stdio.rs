//! An MCP client for servers that run as a program on this PC and talk over
//! their standard input and output. A server like this is arbitrary code, so
//! it is started without a shell, with only the variables it was given,
//! killed when dropped, and held to a time limit and an output limit.

use std::collections::BTreeMap;
use std::process::Stdio;
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio::process::{Child, Command};

use crate::AiError;
use crate::mcp::{McpTool, parse_tools, tool_text};

const PROTOCOL: &str = "2025-06-18";
/// One reply line longer than this is refused.
const MAX_LINE: usize = 1024 * 1024;
/// Tool text longer than this is cut, with a note.
pub const MAX_OUTPUT: usize = 64 * 1024;
const CALL_TIMEOUT: Duration = Duration::from_secs(60);
const HELLO_TIMEOUT: Duration = Duration::from_secs(20);

/// How to start a server: a program, its arguments one by one, and the
/// variables it may see. Never a command line to hand to a shell.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    pub command: String,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
}

/// Rejects anything that looks like a shell command line instead of a
/// program: spaces or shell characters in the program name, or an empty one.
pub fn validate(spec: &Spec) -> Result<(), String> {
    let c = spec.command.trim();
    if c.is_empty() {
        return Err("Enter the program to run.".into());
    }
    if c.chars()
        .any(|ch| ch.is_whitespace() && !c.contains(['\\', '/']))
    {
        return Err("Put only the program in the first box and each argument in its own.".into());
    }
    if c.chars().any(|ch| "|&;<>`$*?()\"'".contains(ch)) {
        return Err("That looks like a shell command. Enter just the program.".into());
    }
    if spec.args.iter().any(|a| a.contains('\0'))
        || spec
            .env
            .keys()
            .any(|k| k.is_empty() || k.contains(['=', '\0']))
    {
        return Err("An argument or variable name is not valid.".into());
    }
    Ok(())
}

pub struct StdioMcp<R, W> {
    reader: R,
    writer: W,
    next_id: u64,
    call_timeout: Duration,
    child: Option<Child>,
}

impl StdioMcp<BufReader<tokio::process::ChildStdout>, tokio::process::ChildStdin> {
    /// Starts the program and says hello. Nothing runs through a shell and
    /// the program sees only the variables in `spec.env` plus the system's
    /// own basics.
    pub async fn spawn(spec: &Spec) -> Result<Self, AiError> {
        validate(spec).map_err(AiError::Failed)?;
        let mut cmd = Command::new(spec.command.trim());
        cmd.args(&spec.args)
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        for key in ["PATH", "SystemRoot", "HOME", "USERPROFILE", "TEMP", "TMP"] {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
        }
        cmd.envs(&spec.env);
        #[cfg(windows)]
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        let mut child = cmd
            .spawn()
            .map_err(|e| AiError::Failed(format!("could not start the server: {e}")))?;
        let (Some(out), Some(input)) = (child.stdout.take(), child.stdin.take()) else {
            return Err(AiError::Failed("the server has no input or output".into()));
        };
        let mut me = Self::over(BufReader::new(out), input);
        me.child = Some(child);
        me.hello().await?;
        Ok(me)
    }
}

impl<R: AsyncBufRead + Unpin, W: AsyncWrite + Unpin> StdioMcp<R, W> {
    /// A client over any reader and writer (tests use an in-memory pipe).
    pub fn over(reader: R, writer: W) -> Self {
        Self {
            reader,
            writer,
            next_id: 1,
            call_timeout: CALL_TIMEOUT,
            child: None,
        }
    }

    pub fn with_timeout(mut self, t: Duration) -> Self {
        self.call_timeout = t;
        self
    }

    pub async fn hello(&mut self) -> Result<(), AiError> {
        let result = tokio::time::timeout(
            HELLO_TIMEOUT,
            self.request(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL,
                    "capabilities": {},
                    "clientInfo": { "name": "duskie", "version": env!("CARGO_PKG_VERSION") },
                }),
            ),
        )
        .await
        .map_err(|_| AiError::Failed("the server did not answer in time".into()))??;
        if result.get("protocolVersion").is_none() {
            return Err(AiError::Failed("that program is not an MCP server".into()));
        }
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))
            .await
    }

    pub async fn list_tools(&mut self) -> Result<Vec<McpTool>, AiError> {
        let result = self.request("tools/list", json!({})).await?;
        Ok(parse_tools(&result))
    }

    /// Runs a tool. The text is cut at [`MAX_OUTPUT`]; a tool error comes
    /// back as text so the model can recover.
    pub async fn call_tool(&mut self, name: &str, arguments: &Value) -> Result<String, AiError> {
        let result = self
            .request(
                "tools/call",
                json!({ "name": name, "arguments": arguments }),
            )
            .await?;
        let text = cap(&tool_text(&result));
        Ok(if result["isError"].as_bool() == Some(true) {
            format!("Error: {text}")
        } else {
            text
        })
    }

    async fn send(&mut self, msg: &Value) -> Result<(), AiError> {
        let mut line = msg.to_string();
        line.push('\n');
        self.writer
            .write_all(line.as_bytes())
            .await
            .map_err(|e| AiError::Failed(format!("the server stopped listening: {e}")))?;
        self.writer
            .flush()
            .await
            .map_err(|e| AiError::Failed(format!("the server stopped listening: {e}")))
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value, AiError> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }))
            .await?;
        let limit = self.call_timeout;
        let wait = async {
            loop {
                let mut line = String::new();
                let n = (&mut self.reader)
                    .take(MAX_LINE as u64 + 1)
                    .read_line(&mut line)
                    .await
                    .map_err(|e| AiError::Failed(format!("reading from the server failed: {e}")))?;
                if n == 0 {
                    return Err(AiError::Failed("the server closed the connection".into()));
                }
                if line.len() > MAX_LINE {
                    return Err(AiError::Failed("the server sent too much at once".into()));
                }
                // Logs and notifications between replies are skipped.
                let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
                    continue;
                };
                if msg["id"] != json!(id) {
                    continue;
                }
                if let Some(err) = msg.get("error") {
                    let detail = err["message"].as_str().unwrap_or("unknown error");
                    return Err(AiError::Failed(format!("MCP {method}: {detail}")));
                }
                return Ok(msg["result"].clone());
            }
        };
        tokio::time::timeout(limit, wait)
            .await
            .map_err(|_| AiError::Failed(format!("the server did not answer {method} in time")))?
    }
}

impl<R, W> Drop for StdioMcp<R, W> {
    fn drop(&mut self) {
        // Disconnecting ends the program.
        if let Some(child) = self.child.as_mut() {
            let _ = child.start_kill();
        }
    }
}

/// Cuts `text` at [`MAX_OUTPUT`] on a character boundary and says so.
fn cap(text: &str) -> String {
    if text.len() <= MAX_OUTPUT {
        return text.to_owned();
    }
    let end = (0..=MAX_OUTPUT)
        .rev()
        .find(|i| text.is_char_boundary(*i))
        .unwrap_or(0);
    format!(
        "{}\n... (cut: the tool returned {} more bytes)",
        &text[..end],
        text.len() - end
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, duplex};

    /// A fake server: answers initialize, lists one tool, and calls it.
    async fn serve(stream: tokio::io::DuplexStream, big: bool, silent_calls: bool) {
        let (r, mut w) = tokio::io::split(stream);
        let mut r = BufReader::new(r);
        let mut line = String::new();
        loop {
            line.clear();
            if r.read_line(&mut line).await.unwrap() == 0 {
                return;
            }
            let msg: Value = serde_json::from_str(line.trim()).unwrap();
            let id = msg["id"].clone();
            let reply = match msg["method"].as_str() {
                Some("initialize") => json!({"result": {"protocolVersion": PROTOCOL}}),
                Some("tools/list") => json!({"result": {"tools": [
                    {"name": "search", "description": "find", "inputSchema": {"type": "object"}}
                ]}}),
                Some("tools/call") if silent_calls => continue,
                Some("tools/call") => {
                    let text = if big {
                        "x".repeat(MAX_OUTPUT * 2)
                    } else {
                        "found it".to_owned()
                    };
                    // A log line first: must be skipped.
                    w.write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\"}\n")
                        .await
                        .unwrap();
                    if msg["params"]["name"] == "boom" {
                        json!({"error": {"message": "no such tool"}})
                    } else {
                        json!({"result": {"content": [{"type": "text", "text": text}]}})
                    }
                }
                _ => continue,
            };
            let mut out = json!({"jsonrpc": "2.0", "id": id});
            out.as_object_mut()
                .unwrap()
                .extend(reply.as_object().unwrap().clone());
            w.write_all(format!("{out}\n").as_bytes()).await.unwrap();
        }
    }

    async fn client(
        big: bool,
        silent: bool,
    ) -> StdioMcp<
        BufReader<tokio::io::ReadHalf<tokio::io::DuplexStream>>,
        tokio::io::WriteHalf<tokio::io::DuplexStream>,
    > {
        let (a, b) = duplex(1 << 20);
        tokio::spawn(serve(b, big, silent));
        let (r, w) = tokio::io::split(a);
        let mut c = StdioMcp::over(BufReader::new(r), w);
        c.hello().await.unwrap();
        c
    }

    #[tokio::test]
    async fn lists_and_calls_tools() {
        let mut c = client(false, false).await;
        assert_eq!(c.list_tools().await.unwrap()[0].name, "search");
        assert_eq!(c.call_tool("search", &json!({})).await.unwrap(), "found it");
    }

    #[tokio::test]
    async fn a_server_error_is_an_error() {
        let mut c = client(false, false).await;
        let err = c.call_tool("boom", &json!({})).await.unwrap_err();
        assert!(err.to_string().contains("no such tool"));
    }

    #[tokio::test]
    async fn huge_output_is_cut() {
        let mut c = client(true, false).await;
        let text = c.call_tool("search", &json!({})).await.unwrap();
        assert!(text.len() < MAX_OUTPUT + 200);
        assert!(text.contains("cut:"));
    }

    #[tokio::test(start_paused = true)]
    async fn a_silent_server_times_out() {
        let (a, b) = duplex(1 << 16);
        tokio::spawn(serve(b, false, true));
        let (r, w) = tokio::io::split(a);
        let mut c = StdioMcp::over(BufReader::new(r), w).with_timeout(Duration::from_secs(3));
        c.hello().await.unwrap();
        let err = c.call_tool("search", &json!({})).await.unwrap_err();
        assert!(err.to_string().contains("in time"), "{err}");
    }

    #[test]
    fn shell_command_lines_are_refused() {
        let spec = |c: &str| Spec {
            command: c.into(),
            args: vec![],
            env: BTreeMap::new(),
        };
        assert!(validate(&spec("npx")).is_ok());
        assert!(validate(&spec("C:\\Program Files\\node\\node.exe")).is_ok());
        for bad in [
            "",
            "npx -y server",
            "node; rm -rf /",
            "a | b",
            "$(x)",
            "`x`",
        ] {
            assert!(validate(&spec(bad)).is_err(), "{bad}");
        }
        let bad_env = Spec {
            command: "npx".into(),
            args: vec![],
            env: [("A=B".to_owned(), "1".to_owned())].into(),
        };
        assert!(validate(&bad_env).is_err());
    }
}
