//! Signing in to a remote MCP server with OAuth, the way Claude Desktop's
//! Connect button does: find the server's sign-in service, register
//! Sidekick with it, open the browser, catch the answer on a local port
//! and keep the tokens. Works for any server that follows the MCP
//! authorization spec (Composio Connect among them).
//!
//! The refresh token and the client registration live in Credential
//! Manager; the access token is kept in memory and renewed when it runs out.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const TIMEOUT: Duration = Duration::from_secs(20);
/// How long the browser sign-in may take.
pub const SIGN_IN_WAIT: Duration = Duration::from_secs(10 * 60);
/// Renew this long before the access token runs out.
const EARLY: Duration = Duration::from_secs(60);

/// What is kept between runs, under one Credential Manager name.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Grant {
    pub client_id: String,
    pub client_secret: String,
    pub token_endpoint: String,
    pub resource: String,
    pub refresh_token: String,
    /// Only kept when the server gives no refresh token.
    pub access_token: String,
}

/// The server's sign-in details, found from the MCP URL.
#[derive(Debug, Clone, PartialEq)]
pub struct Discovery {
    pub resource: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub registration_endpoint: Option<String>,
    pub scope: Option<String>,
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(TIMEOUT)
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()
        .unwrap_or_default()
}

/// URL-safe base64 without padding (PKCE and state).
pub fn b64url(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = match chunk.len() {
            3 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]),
            2 => (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8),
            _ => u32::from(chunk[0]) << 16,
        };
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(T[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

fn random(n: usize) -> String {
    let mut buf = vec![0u8; n];
    let _ = getrandom::fill(&mut buf);
    b64url(&buf)
}

/// The PKCE challenge for a verifier.
pub fn challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

fn encode(s: &str) -> String {
    sidekick_sensors::entity::encode(s)
}

/// `scheme://host[:port]` of a URL.
fn origin(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    let host = rest.split(['/', '?', '#']).next()?;
    Some(format!("{scheme}://{host}"))
}

/// The path of a URL without a trailing slash ("" for the root).
fn path_of(url: &str) -> String {
    let rest = url.split_once("://").map(|x| x.1).unwrap_or(url);
    let path = rest
        .find('/')
        .map(|i| &rest[i..])
        .unwrap_or("")
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    path.trim_end_matches('/').to_owned()
}

/// `resource_metadata="..."` from a WWW-Authenticate header.
pub fn metadata_hint(www_authenticate: &str) -> Option<String> {
    let at = www_authenticate.find("resource_metadata=")?;
    let rest = &www_authenticate[at + "resource_metadata=".len()..];
    let value = if let Some(q) = rest.strip_prefix('"') {
        q.split('"').next()?
    } else {
        rest.split([',', ' ']).next()?
    };
    Some(value.to_owned()).filter(|v| v.starts_with("http"))
}

async fn get_json(url: &str) -> Option<Value> {
    let resp = client().get(url).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    resp.json().await.ok()
}

/// Finds where to sign in for `mcp_url` (RFC 9728, then RFC 8414 or
/// OpenID discovery).
pub async fn discover(mcp_url: &str) -> Result<Discovery, String> {
    let base = origin(mcp_url).ok_or("not a web link")?;
    let path = path_of(mcp_url);
    // An unsigned request names the metadata in its 401.
    let hint = client()
        .post(mcp_url)
        .header("accept", "application/json, text/event-stream")
        .json(
            &serde_json::json!({ "jsonrpc": "2.0", "id": 0, "method": "initialize",
            "params": { "protocolVersion": "2025-06-18", "capabilities": {},
                "clientInfo": { "name": "sidekick", "version": env!("CARGO_PKG_VERSION") } } }),
        )
        .send()
        .await
        .ok()
        .and_then(|r| {
            r.headers()
                .get("www-authenticate")
                .and_then(|v| v.to_str().ok())
                .and_then(metadata_hint)
        });
    let mut candidates = Vec::new();
    candidates.extend(hint);
    candidates.push(format!("{base}/.well-known/oauth-protected-resource{path}"));
    candidates.push(format!("{base}/.well-known/oauth-protected-resource"));
    let mut prm = None;
    for c in &candidates {
        if let Some(v) = get_json(c).await {
            prm = Some(v);
            break;
        }
    }
    let (resource, issuer, scope) = match &prm {
        Some(v) => (
            v["resource"].as_str().unwrap_or(mcp_url).to_owned(),
            v["authorization_servers"][0]
                .as_str()
                .unwrap_or(&base)
                .trim_end_matches('/')
                .to_owned(),
            v["scopes_supported"].as_array().map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
        ),
        // Older servers: the sign-in service is the MCP host itself.
        None => (mcp_url.to_owned(), base.clone(), None),
    };
    let issuer_base = origin(&issuer).ok_or("bad sign-in service address")?;
    let issuer_path = path_of(&issuer);
    let mut meta = None;
    for c in [
        format!("{issuer_base}/.well-known/oauth-authorization-server{issuer_path}"),
        format!("{issuer_base}/.well-known/openid-configuration{issuer_path}"),
        format!("{issuer}/.well-known/openid-configuration"),
        format!("{issuer_base}/.well-known/oauth-authorization-server"),
    ] {
        if let Some(v) = get_json(&c).await {
            meta = Some(v);
            break;
        }
    }
    let meta = meta.ok_or("This server does not offer a sign-in. Use a key instead.")?;
    let s = |k: &str| meta[k].as_str().map(str::to_owned);
    Ok(Discovery {
        resource,
        authorization_endpoint: s("authorization_endpoint")
            .ok_or("the sign-in service has no authorization page")?,
        token_endpoint: s("token_endpoint").ok_or("the sign-in service has no token address")?,
        registration_endpoint: s("registration_endpoint"),
        scope: scope.filter(|s| !s.is_empty()).or_else(|| {
            meta["scopes_supported"].as_array().map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .filter(|s| *s != "openid")
                    .collect::<Vec<_>>()
                    .join(" ")
            })
        }),
    })
}

/// Registers Sidekick with the sign-in service (RFC 7591).
async fn register(endpoint: &str, redirect: &str) -> Result<(String, String), String> {
    let resp = client()
        .post(endpoint)
        .json(&serde_json::json!({
            "client_name": "Sidekick",
            "redirect_uris": [redirect],
            "grant_types": ["authorization_code", "refresh_token"],
            "response_types": ["code"],
            "token_endpoint_auth_method": "none",
        }))
        .send()
        .await
        .map_err(|e| format!("could not reach the sign-in service: {e}"))?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "the sign-in service refused Sidekick ({status}): {}",
            v["error_description"].as_str().unwrap_or("no details")
        ));
    }
    let id = v["client_id"]
        .as_str()
        .ok_or("the sign-in service gave no client id")?;
    Ok((
        id.to_owned(),
        v["client_secret"].as_str().unwrap_or_default().to_owned(),
    ))
}

/// Tokens from the token endpoint: (access, refresh, lifetime).
async fn token(endpoint: &str, form: &[(&str, &str)]) -> Result<(String, String, u64), String> {
    let body: String = form
        .iter()
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let resp = client()
        .post(endpoint)
        .header("content-type", "application/x-www-form-urlencoded")
        .header("accept", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|e| format!("could not reach the sign-in service: {e}"))?;
    let status = resp.status();
    let v: Value = resp.json().await.unwrap_or(Value::Null);
    if !status.is_success() {
        return Err(format!(
            "sign-in failed ({status}): {}",
            v["error_description"]
                .as_str()
                .or(v["error"].as_str())
                .unwrap_or("no details")
        ));
    }
    let access = v["access_token"]
        .as_str()
        .ok_or("the sign-in service gave no token")?;
    Ok((
        access.to_owned(),
        v["refresh_token"].as_str().unwrap_or_default().to_owned(),
        v["expires_in"].as_u64().unwrap_or(3600),
    ))
}

/// The authorization page for the browser.
pub fn authorize_url(
    d: &Discovery,
    client_id: &str,
    redirect: &str,
    verifier: &str,
    state: &str,
) -> String {
    let sep = if d.authorization_endpoint.contains('?') {
        '&'
    } else {
        '?'
    };
    let mut url = format!(
        "{}{sep}response_type=code&client_id={}&redirect_uri={}&code_challenge={}&code_challenge_method=S256&state={}&resource={}",
        d.authorization_endpoint,
        encode(client_id),
        encode(redirect),
        challenge(verifier),
        encode(state),
        encode(&d.resource),
    );
    if let Some(scope) = &d.scope {
        url.push_str(&format!("&scope={}", encode(scope)));
    }
    url
}

/// `code` and `state` from the callback's request line.
pub fn callback_params(request_line: &str) -> (Option<String>, Option<String>, Option<String>) {
    let target = request_line.split_whitespace().nth(1).unwrap_or_default();
    let query = target.split_once('?').map(|x| x.1).unwrap_or_default();
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for pair in query.split('&') {
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        let v = decode(v);
        match k {
            "code" => code = Some(v),
            "state" => state = Some(v),
            "error_description" => error = Some(v),
            "error" if error.is_none() => error = Some(v),
            _ => {}
        }
    }
    (code, state, error)
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                Ok(b) => {
                    out.push(b);
                    i += 2;
                }
                Err(_) => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

const DONE_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Sidekick</title>\
<body style=\"font-family:system-ui;background:#000;color:#fff;display:grid;place-items:center;height:100vh;margin:0\">\
<div style=\"text-align:center\"><h2>Connected</h2><p style=\"color:#aaa\">You can close this tab and go back to Sidekick.</p></div>";

/// Waits for the browser to come back with a code.
async fn catch_code(listener: TcpListener, state: &str) -> Result<String, String> {
    loop {
        let (mut sock, _) = listener.accept().await.map_err(|e| e.to_string())?;
        let mut buf = vec![0u8; 8192];
        let n = tokio::time::timeout(Duration::from_secs(10), sock.read(&mut buf))
            .await
            .ok()
            .and_then(Result::ok)
            .unwrap_or(0);
        let head = String::from_utf8_lossy(&buf[..n]).into_owned();
        let line = head.lines().next().unwrap_or_default().to_owned();
        if !line.starts_with("GET /callback") {
            let _ = sock
                .write_all(
                    b"HTTP/1.1 404 Not Found\r\ncontent-length: 0\r\nconnection: close\r\n\r\n",
                )
                .await;
            continue;
        }
        let (code, got_state, error) = callback_params(&line);
        let (status, page, result) = match (code, error) {
            _ if got_state.as_deref() != Some(state) => (
                "400 Bad Request",
                "Sign-in did not match. Press Connect in Sidekick again.".to_owned(),
                Err("The sign-in did not match. Press Connect again.".to_owned()),
            ),
            (Some(code), _) => ("200 OK", DONE_PAGE.to_owned(), Ok(code)),
            (None, err) => {
                let why = err.unwrap_or_else(|| "no code".into());
                (
                    "400 Bad Request",
                    format!("Sign-in was not finished: {why}"),
                    Err(format!("Sign-in was not finished: {why}")),
                )
            }
        };
        let resp = format!(
            "HTTP/1.1 {status}\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{page}",
            page.len()
        );
        let _ = sock.write_all(resp.as_bytes()).await;
        let _ = sock.shutdown().await;
        return result;
    }
}

/// The whole sign-in: discovery, registration, browser, code, tokens.
/// `open` shows the page in the browser. Returns the grant to keep and a
/// fresh access token.
pub async fn sign_in<F, Fut>(mcp_url: &str, open: F) -> Result<(Grant, String, u64), String>
where
    F: FnOnce(String) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let d = discover(mcp_url).await?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("could not open a local port: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}/callback");
    let reg = d
        .registration_endpoint
        .as_deref()
        .ok_or("This server needs a key instead of a sign-in.")?;
    let (client_id, client_secret) = register(reg, &redirect).await?;
    let verifier = random(48);
    let state = random(16);
    open(authorize_url(&d, &client_id, &redirect, &verifier, &state)).await?;
    let code = tokio::time::timeout(SIGN_IN_WAIT, catch_code(listener, &state))
        .await
        .map_err(|_| "Sign-in timed out. Press Connect again.".to_string())??;
    let (access, refresh, life) = token(
        &d.token_endpoint,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", &redirect),
            ("client_id", &client_id),
            ("client_secret", &client_secret),
            ("code_verifier", &verifier),
            ("resource", &d.resource),
        ],
    )
    .await?;
    let grant = Grant {
        client_id,
        client_secret,
        token_endpoint: d.token_endpoint,
        resource: d.resource,
        access_token: if refresh.is_empty() {
            access.clone()
        } else {
            String::new()
        },
        refresh_token: refresh,
    };
    Ok((grant, access, life))
}

/// A new access token from the refresh token. The service may hand back
/// a new refresh token too.
pub async fn refresh(grant: &Grant) -> Result<(String, Option<String>, u64), String> {
    if grant.refresh_token.is_empty() {
        return Err("The sign-in ran out. Connect again.".into());
    }
    let (access, refresh, life) = token(
        &grant.token_endpoint,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", &grant.refresh_token),
            ("client_id", &grant.client_id),
            ("client_secret", &grant.client_secret),
            ("resource", &grant.resource),
        ],
    )
    .await?;
    let rotated = Some(refresh).filter(|r| !r.is_empty() && *r != grant.refresh_token);
    Ok((access, rotated, life))
}

/// The access token in memory, with when it runs out.
pub struct Cached {
    token: Mutex<Option<(String, Instant)>>,
}

impl Default for Cached {
    fn default() -> Self {
        Self {
            token: Mutex::new(None),
        }
    }
}

impl Cached {
    pub fn get(&self) -> Option<String> {
        let g = self.token.lock().ok()?;
        let (t, until) = g.as_ref()?;
        (Instant::now() + EARLY < *until).then(|| t.clone())
    }

    pub fn set(&self, token: String, life_secs: u64) {
        if let Ok(mut g) = self.token.lock() {
            *g = Some((token, Instant::now() + Duration::from_secs(life_secs)));
        }
    }

    pub fn clear(&self) {
        if let Ok(mut g) = self.token.lock() {
            *g = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_example() {
        // RFC 7636 appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        assert_eq!(b64url(b"f"), "Zg");
        assert_eq!(b64url(b"fo"), "Zm8");
        assert_eq!(b64url(b"foo"), "Zm9v");
    }

    #[test]
    fn reads_the_metadata_hint_and_callback() {
        assert_eq!(
            metadata_hint(r#"Bearer realm="x", resource_metadata="https://a.dev/.well-known/oauth-protected-resource/mcp""#)
                .as_deref(),
            Some("https://a.dev/.well-known/oauth-protected-resource/mcp")
        );
        assert_eq!(metadata_hint("Bearer"), None);
        let (code, state, err) = callback_params("GET /callback?code=ab%2Fc&state=xyz HTTP/1.1");
        assert_eq!(
            (code.as_deref(), state.as_deref(), err),
            (Some("ab/c"), Some("xyz"), None)
        );
        let (_, _, err) = callback_params(
            "GET /callback?error=access_denied&error_description=User+said+no&state=s HTTP/1.1",
        );
        assert_eq!(err.as_deref(), Some("User said no"));
        assert_eq!(path_of("https://connect.composio.dev/mcp"), "/mcp");
        assert_eq!(path_of("https://x.dev"), "");
        assert_eq!(
            origin("https://connect.composio.dev/mcp?x=1").as_deref(),
            Some("https://connect.composio.dev")
        );
    }

    /// A fake MCP server with its own sign-in service, all on one port.
    async fn fake_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let b = base.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let b = b.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 16384];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let req = String::from_utf8_lossy(&buf[..n]).into_owned();
                    let line = req.lines().next().unwrap_or_default().to_owned();
                    let body = req.split("\r\n\r\n").nth(1).unwrap_or_default().to_owned();
                    let json = |v: Value| ("200 OK", vec![], v.to_string());
                    let (status, extra, out): (&str, Vec<String>, String) = if line
                        .starts_with("POST /mcp")
                    {
                        (
                            "401 Unauthorized",
                            vec![format!(
                                "www-authenticate: Bearer resource_metadata=\"{b}/.well-known/oauth-protected-resource/mcp\""
                            )],
                            String::new(),
                        )
                    } else if line.starts_with("GET /.well-known/oauth-protected-resource/mcp") {
                        json(
                            serde_json::json!({ "resource": format!("{b}/mcp"), "authorization_servers": [format!("{b}/auth")] }),
                        )
                    } else if line.starts_with("GET /.well-known/oauth-authorization-server/auth") {
                        json(serde_json::json!({
                            "authorization_endpoint": format!("{b}/auth/authorize"),
                            "token_endpoint": format!("{b}/auth/token"),
                            "registration_endpoint": format!("{b}/auth/register"),
                        }))
                    } else if line.starts_with("POST /auth/register") {
                        assert!(body.contains("127.0.0.1"));
                        json(serde_json::json!({ "client_id": "cid" }))
                    } else if line.starts_with("GET /auth/authorize") {
                        let q = line
                            .split_whitespace()
                            .nth(1)
                            .unwrap()
                            .split_once('?')
                            .unwrap()
                            .1
                            .to_owned();
                        let get = |k: &str| {
                            q.split('&')
                                .find_map(|p| p.strip_prefix(&format!("{k}=")))
                                .unwrap()
                                .to_owned()
                        };
                        assert_eq!(get("code_challenge_method"), "S256");
                        let to = format!(
                            "{}?code=c0de&state={}",
                            decode(&get("redirect_uri")),
                            get("state")
                        );
                        ("302 Found", vec![format!("location: {to}")], String::new())
                    } else if line.starts_with("POST /auth/token") {
                        if body.contains("grant_type=authorization_code") {
                            assert!(body.contains("code=c0de") && body.contains("code_verifier="));
                            json(
                                serde_json::json!({ "access_token": "at1", "refresh_token": "rt1", "expires_in": 120 }),
                            )
                        } else {
                            assert!(body.contains("refresh_token=rt1"));
                            json(
                                serde_json::json!({ "access_token": "at2", "refresh_token": "rt2", "expires_in": 120 }),
                            )
                        }
                    } else {
                        ("404 Not Found", vec![], String::new())
                    };
                    let mut resp = format!(
                        "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n",
                        out.len()
                    );
                    for h in extra {
                        resp.push_str(&h);
                        resp.push_str("\r\n");
                    }
                    resp.push_str("\r\n");
                    resp.push_str(&out);
                    let _ = sock.write_all(resp.as_bytes()).await;
                });
            }
        });
        base
    }

    #[tokio::test]
    async fn signs_in_end_to_end_and_refreshes() {
        let base = fake_server().await;
        let mcp = format!("{base}/mcp");
        let d = discover(&mcp).await.unwrap();
        assert_eq!(d.token_endpoint, format!("{base}/auth/token"));
        assert_eq!(d.resource, mcp);
        // "The browser": follow the authorize redirect to the local port.
        let (grant, access, life) = sign_in(&mcp, |url| async move {
            tokio::spawn(async move {
                let page = client()
                    .get(url)
                    .send()
                    .await
                    .unwrap()
                    .text()
                    .await
                    .unwrap();
                assert!(page.contains("Connected"));
            });
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!((access.as_str(), life), ("at1", 120));
        assert_eq!(grant.client_id, "cid");
        assert_eq!(grant.refresh_token, "rt1");
        assert!(grant.access_token.is_empty(), "kept only in memory");
        let (access, rotated, _) = refresh(&grant).await.unwrap();
        assert_eq!(access, "at2");
        assert_eq!(rotated.as_deref(), Some("rt2"));
    }
}
