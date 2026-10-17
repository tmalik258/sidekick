//! Rate limits for providers and connectors: a token bucket per name, retries
//! that honor Retry-After with a little jitter, and a breaker that pauses a
//! name after repeated failures so the UI can say until when.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::AiError;

/// Requests a name may burst before it has to wait.
const BURST: f64 = 8.0;
/// Requests per second it refills.
const PER_SEC: f64 = 2.0;
/// Tries in all, the first included.
const TRIES: u32 = 3;
/// Longest single wait between tries, even if the server asks for more.
const MAX_WAIT: Duration = Duration::from_secs(30);
/// Failures in a row that pause a name.
const TRIP_AFTER: u32 = 5;
/// How long a tripped name stays paused.
const PAUSE: Duration = Duration::from_secs(60);

#[derive(Debug, Clone)]
pub struct Bucket {
    tokens: f64,
    last: Instant,
}

impl Bucket {
    pub fn new(now: Instant) -> Self {
        Self {
            tokens: BURST,
            last: now,
        }
    }

    /// Takes one token, or says how long until one is free.
    pub fn take(&mut self, now: Instant) -> Result<(), Duration> {
        let gained = now.saturating_duration_since(self.last).as_secs_f64() * PER_SEC;
        self.tokens = (self.tokens + gained).min(BURST);
        self.last = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            Ok(())
        } else {
            Err(Duration::from_secs_f64((1.0 - self.tokens) / PER_SEC))
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Breaker {
    failures: u32,
    open_until: Option<Instant>,
}

impl Breaker {
    pub fn record(&mut self, ok: bool, now: Instant) {
        if ok {
            *self = Self::default();
            return;
        }
        self.failures += 1;
        if self.failures >= TRIP_AFTER {
            self.open_until = Some(now + PAUSE);
        }
    }

    /// Paused until this moment, if paused now.
    pub fn paused(&self, now: Instant) -> Option<Instant> {
        self.open_until.filter(|t| *t > now)
    }
}

/// Seconds from a Retry-After header (the date form is rare and ignored).
pub fn retry_after(value: Option<&str>) -> Option<Duration> {
    value?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|s| *s >= 0.0)
        .map(Duration::from_secs_f64)
}

/// The wait before try `attempt` (1 for the first retry): what the server
/// asked for, else doubling from half a second, plus up to a fifth of jitter.
pub fn backoff(attempt: u32, asked: Option<Duration>, jitter: f64) -> Duration {
    let base =
        asked.unwrap_or_else(|| Duration::from_millis(500 * 2u64.pow(attempt.saturating_sub(1))));
    base.mul_f64(1.0 + jitter.clamp(0.0, 1.0) * 0.2)
        .min(MAX_WAIT)
}

fn jitter() -> f64 {
    let n = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or_default();
    f64::from(n % 1000) / 1000.0
}

struct Limit {
    bucket: Bucket,
    breaker: Breaker,
}

static LIMITS: Mutex<Option<HashMap<String, Limit>>> = Mutex::new(None);

fn with<T>(name: &str, f: impl FnOnce(&mut Limit) -> T) -> T {
    let mut guard = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    let all = guard.get_or_insert_with(HashMap::new);
    let limit = all.entry(name.to_owned()).or_insert_with(|| Limit {
        bucket: Bucket::new(Instant::now()),
        breaker: Breaker::default(),
    });
    f(limit)
}

/// One limited name, for the usage popover.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LimitState {
    pub name: String,
    /// Seconds until it is tried again; 0 when not paused.
    pub paused_secs: u64,
}

/// Every name that is paused right now.
pub fn paused() -> Vec<LimitState> {
    let now = Instant::now();
    let guard = LIMITS.lock().unwrap_or_else(|e| e.into_inner());
    let mut out: Vec<LimitState> = guard
        .iter()
        .flat_map(|m| m.iter())
        .filter_map(|(name, l)| {
            l.breaker.paused(now).map(|until| LimitState {
                name: name.clone(),
                paused_secs: until.saturating_duration_since(now).as_secs().max(1),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

fn retryable(status: reqwest::StatusCode) -> bool {
    status == reqwest::StatusCode::TOO_MANY_REQUESTS
        || status.as_u16() == 529
        || status == reqwest::StatusCode::SERVICE_UNAVAILABLE
        || status == reqwest::StatusCode::BAD_GATEWAY
}

/// Sends a request built by `make` under the limits for `name`: waits for
/// the bucket, refuses while paused, and retries busy answers.
pub async fn send(
    name: &str,
    make: impl Fn() -> reqwest::RequestBuilder,
) -> Result<reqwest::Response, AiError> {
    if let Some(until) = with(name, |l| l.breaker.paused(Instant::now())) {
        let secs = until
            .saturating_duration_since(Instant::now())
            .as_secs()
            .max(1);
        return Err(AiError::Failed(format!(
            "{name} is paused for {secs} s after repeated errors."
        )));
    }
    let mut attempt = 0;
    loop {
        if let Err(wait) = with(name, |l| l.bucket.take(Instant::now())) {
            tokio::time::sleep(wait.min(MAX_WAIT)).await;
            continue;
        }
        attempt += 1;
        let result = make().send().await;
        let ok =
            matches!(&result, Ok(r) if !retryable(r.status()) && !r.status().is_server_error());
        with(name, |l| l.breaker.record(ok, Instant::now()));
        match result {
            Ok(resp) if retryable(resp.status()) && attempt < TRIES => {
                let asked = retry_after(
                    resp.headers()
                        .get(reqwest::header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok()),
                );
                tokio::time::sleep(backoff(attempt, asked, jitter())).await;
            }
            Ok(resp) => return Ok(resp),
            Err(e) if attempt < TRIES && (e.is_connect() || e.is_timeout()) => {
                tokio::time::sleep(backoff(attempt, None, jitter())).await;
            }
            Err(e) => return Err(e.into()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    #[test]
    fn bucket_bursts_then_waits() {
        let t = Instant::now();
        let mut b = Bucket::new(t);
        for _ in 0..8 {
            assert!(b.take(t).is_ok());
        }
        let wait = b.take(t).unwrap_err();
        assert!(wait <= Duration::from_millis(500), "{wait:?}");
        assert!(b.take(t + Duration::from_millis(600)).is_ok());
    }

    #[test]
    fn breaker_pauses_after_repeated_failures() {
        let t = Instant::now();
        let mut b = Breaker::default();
        for _ in 0..4 {
            b.record(false, t);
        }
        assert!(b.paused(t).is_none());
        b.record(false, t);
        assert!(b.paused(t).is_some());
        assert!(b.paused(t + PAUSE + Duration::from_secs(1)).is_none());
        b.record(true, t);
        assert!(b.paused(t).is_none());
    }

    #[test]
    fn backoff_honors_retry_after_and_caps() {
        assert_eq!(retry_after(Some("2")), Some(Duration::from_secs(2)));
        assert_eq!(retry_after(Some("Wed, 21 Oct 2015 07:28:00 GMT")), None);
        assert_eq!(
            backoff(1, Some(Duration::from_secs(2)), 0.0),
            Duration::from_secs(2)
        );
        assert_eq!(backoff(1, None, 0.0), Duration::from_millis(500));
        assert_eq!(backoff(2, None, 0.0), Duration::from_secs(1));
        assert!(backoff(1, Some(Duration::from_secs(2)), 1.0) <= Duration::from_millis(2400));
        assert_eq!(backoff(1, Some(Duration::from_secs(600)), 0.0), MAX_WAIT);
    }

    /// A fake server: 429 with Retry-After: 1 first, then 200.
    #[tokio::test]
    async fn retries_a_429_after_the_asked_wait() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for answer in [
                "HTTP/1.1 429 Too Many Requests\r\nRetry-After: 1\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                "HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
            ] {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut buf = [0u8; 1024];
                let _ = sock.read(&mut buf).await;
                sock.write_all(answer.as_bytes()).await.unwrap();
                sock.shutdown().await.ok();
            }
        });
        let client = reqwest::Client::new();
        let url = format!("http://{addr}/");
        let started = Instant::now();
        let resp = send("test-429", || client.get(&url)).await.unwrap();
        assert_eq!(resp.status(), 200);
        assert!(started.elapsed() >= Duration::from_secs(1));
        assert!(paused().iter().all(|p| p.name != "test-429"));
    }
}
