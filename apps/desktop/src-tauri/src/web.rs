//! The web for Ask: search (DuckDuckGo's plain HTML page, no key needed) and
//! reading one page as text. Every model gets these through the shared tools.

use std::sync::LazyLock;
use std::time::Duration;

use regex::Regex;

const MAX_RESULTS: usize = 6;
/// Most page text handed to the model.
const MAX_PAGE_TEXT: usize = 6000;
const TIMEOUT: Duration = Duration::from_secs(12);
const AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0 Safari/537.36";

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub title: String,
    pub url: String,
    pub snippet: String,
}

fn client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .user_agent(AGENT)
        .build()
        .map_err(|e| e.to_string())
}

static RESULT: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)class="result__a"[^>]*href="([^"]+)"[^>]*>(.*?)</a>"#).expect("result regex")
});
static SNIPPET: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"(?s)class="result__snippet"[^>]*>(.*?)</a>"#).expect("snippet regex")
});
static TAG: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?s)<[^>]*>").expect("tag regex"));
static DROP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?is)<(script|style|noscript|svg|head|nav|footer|form)\b.*?</(script|style|noscript|svg|head|nav|footer|form)>")
        .expect("drop regex")
});
static BLOCK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)</?(p|div|br|li|h[1-6]|tr|section|article|pre|blockquote)\b[^>]*>")
        .expect("block regex")
});
static SPACES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"[ \t\u{a0}]+").expect("spaces"));
static LINES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\n\s*\n+").expect("lines"));

fn decode(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#x27;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ")
}

fn unescape_url(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// DuckDuckGo wraps each link as `//duckduckgo.com/l/?uddg=<url>&...`.
fn real_url(href: &str) -> String {
    let href = decode(href);
    if let Some(i) = href.find("uddg=") {
        let rest = &href[i + 5..];
        let end = rest.find('&').unwrap_or(rest.len());
        return unescape_url(&rest[..end]);
    }
    if let Some(rest) = href.strip_prefix("//") {
        return format!("https://{rest}");
    }
    href
}

fn plain(html: &str) -> String {
    decode(&TAG.replace_all(html, "")).trim().to_owned()
}

/// Results from DuckDuckGo's HTML page, ads left out.
pub fn parse_results(html: &str) -> Vec<Hit> {
    let snippets: Vec<String> = SNIPPET.captures_iter(html).map(|c| plain(&c[1])).collect();
    RESULT
        .captures_iter(html)
        .map(|c| (real_url(&c[1]), plain(&c[2])))
        .filter(|(url, _)| url.starts_with("http") && !url.contains("duckduckgo.com/y.js"))
        .enumerate()
        .map(|(i, (url, title))| Hit {
            title,
            url,
            snippet: snippets.get(i).cloned().unwrap_or_default(),
        })
        .take(MAX_RESULTS)
        .collect()
}

/// A page's readable text: no scripts, menus or tags.
pub fn page_text(html: &str) -> String {
    let title = Regex::new(r"(?is)<title[^>]*>(.*?)</title>")
        .ok()
        .and_then(|r| r.captures(html).map(|c| plain(&c[1])))
        .unwrap_or_default();
    let body = DROP.replace_all(html, " ");
    let body = BLOCK.replace_all(&body, "\n");
    let text = decode(&TAG.replace_all(&body, " "));
    let text = SPACES.replace_all(&text, " ");
    let text = LINES.replace_all(text.trim(), "\n\n");
    let text: String = text.lines().map(str::trim).collect::<Vec<_>>().join("\n");
    let mut out = if title.is_empty() {
        text
    } else {
        format!("{title}\n\n{text}")
    };
    if out.chars().count() > MAX_PAGE_TEXT {
        out = out.chars().take(MAX_PAGE_TEXT).collect::<String>() + "...";
    }
    out
}

pub fn describe(hits: &[Hit], query: &str) -> String {
    if hits.is_empty() {
        return format!("No web results for \"{query}\".");
    }
    hits.iter()
        .enumerate()
        .map(|(i, h)| format!("{}. {} ({})\n   {}", i + 1, h.title, h.url, h.snippet))
        .collect::<Vec<_>>()
        .join("\n")
}

pub async fn search(query: &str) -> Result<String, String> {
    let query = query.trim();
    if query.is_empty() {
        return Err("say what to search for".into());
    }
    let html = client()?
        .post("https://html.duckduckgo.com/html/")
        .form(&[("q", query)])
        .send()
        .await
        .map_err(|e| format!("could not reach the web: {e}"))?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    Ok(describe(&parse_results(&html), query))
}

/// Only web pages; never files or this PC's own servers.
pub fn allowed_url(url: &str) -> bool {
    let Ok(u) = reqwest::Url::parse(url) else {
        return false;
    };
    let host = u.host_str().unwrap_or_default().to_lowercase();
    matches!(u.scheme(), "http" | "https")
        && !host.is_empty()
        && host != "localhost"
        && !host.ends_with(".local")
        && !host.starts_with("127.")
        && !host.starts_with("10.")
        && !host.starts_with("192.168.")
        && !host.starts_with("169.254.")
        && !host.starts_with('[')
        && host != "0.0.0.0"
        && !(host.starts_with("172.")
            && host
                .split('.')
                .nth(1)
                .and_then(|n| n.parse::<u8>().ok())
                .is_some_and(|n| (16..=31).contains(&n)))
}

pub async fn read(url: &str) -> Result<String, String> {
    let url = url.trim();
    if !allowed_url(url) {
        return Err("only public web pages (http or https) can be read".into());
    }
    let resp = client()?
        .get(url)
        .send()
        .await
        .map_err(|e| format!("could not open the page: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("the page answered {status}"));
    }
    let kind = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("text/html")
        .to_lowercase();
    if !(kind.contains("html") || kind.starts_with("text/") || kind.contains("json")) {
        return Err(format!("that is not a web page ({kind})"));
    }
    let body = resp.text().await.map_err(|e| e.to_string())?;
    Ok(if kind.contains("html") {
        page_text(&body)
    } else {
        body.chars().take(MAX_PAGE_TEXT).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const DDG: &str = r#"
<div class="result results_links results_links_deep web-result ">
  <h2 class="result__title">
    <a rel="nofollow" class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fv2.tauri.app%2Fblog%2Ftauri%2D20%2F&amp;rut=abc">Tauri <b>2.0</b> Stable Release</a>
  </h2>
  <a class="result__snippet" href="//duckduckgo.com/l/?uddg=x">Tauri <b>2.0</b> is out &amp; ready.</a>
</div>
<div class="result">
  <a rel="nofollow" class="result__a" href="https://example.com/docs?a=1&amp;b=2">Example &#x27;docs&#x27;</a>
  <a class="result__snippet" href="https://example.com">Second snippet</a>
</div>"#;

    #[test]
    fn reads_search_results() {
        let hits = parse_results(DDG);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].url, "https://v2.tauri.app/blog/tauri-20/");
        assert_eq!(hits[0].title, "Tauri 2.0 Stable Release");
        assert_eq!(hits[0].snippet, "Tauri 2.0 is out & ready.");
        assert_eq!(hits[1].url, "https://example.com/docs?a=1&b=2");
        assert_eq!(hits[1].title, "Example 'docs'");
        assert!(describe(&hits, "tauri").starts_with("1. Tauri 2.0"));
        assert!(describe(&[], "x").starts_with("No web results"));
    }

    #[test]
    fn reads_a_page_as_text() {
        let html = "<html><head><title>Hello &amp; welcome</title><style>p{}</style></head>\
            <body><nav>Menu</nav><script>evil()</script><h1>Big news</h1><p>First   line.</p>\
            <p>Second<br>line</p><footer>Copyright</footer></body></html>";
        let t = page_text(html);
        assert!(t.starts_with("Hello & welcome"));
        assert!(t.contains("Big news"));
        assert!(t.contains("First line."));
        assert!(!t.contains("evil"));
        assert!(!t.contains("Menu"));
        assert!(!t.contains("Copyright"));
    }

    #[test]
    fn reads_only_public_pages() {
        assert!(allowed_url("https://example.com/a"));
        assert!(!allowed_url("file:///C:/secret.txt"));
        assert!(!allowed_url("http://localhost:47823/mcp"));
        assert!(!allowed_url("http://127.0.0.1:8080"));
        assert!(!allowed_url("http://192.168.1.1"));
        assert!(!allowed_url("http://172.20.0.1"));
        assert!(allowed_url("http://172.64.1.1"));
        assert!(!allowed_url("http://[::1]/"));
    }
}
