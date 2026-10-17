//! A small cache for slow answers (tool lists, model lists, git status):
//! every entry has a time to live, can be dropped on demand, keeps an ETag
//! for a cheap revalidation, and callers asking for the same key at once
//! share one fetch.

use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use tokio::sync::Mutex;

#[derive(Clone)]
struct Entry<V> {
    value: V,
    etag: Option<String>,
    at: Instant,
}

/// What a fetch returns: a new value, or "unchanged" for the ETag it was given.
pub enum Fetched<V> {
    Fresh { value: V, etag: Option<String> },
    NotModified,
}

/// One key's entry, locked while it is being fetched.
type Slot<V> = Arc<Mutex<Option<Entry<V>>>>;

pub struct Cache<V> {
    ttl: Duration,
    slots: std::sync::Mutex<HashMap<String, Slot<V>>>,
    hits: AtomicU64,
    misses: AtomicU64,
}

impl<V: Clone> Cache<V> {
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            slots: std::sync::Mutex::new(HashMap::new()),
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }

    fn slot(&self, key: &str) -> Slot<V> {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(key.to_owned())
            .or_default()
            .clone()
    }

    /// The cached value while fresh, else what `fetch` returns. `fetch` gets
    /// the last ETag and may answer NotModified to keep the old value.
    /// Concurrent callers for one key wait for the same fetch.
    pub async fn get_or_fetch<F, Fut, E>(&self, key: &str, fetch: F) -> Result<V, E>
    where
        F: FnOnce(Option<String>) -> Fut,
        Fut: Future<Output = Result<Fetched<V>, E>>,
    {
        let slot = self.slot(key);
        let mut entry = slot.lock().await;
        if let Some(e) = entry.as_ref()
            && e.at.elapsed() < self.ttl
        {
            self.hits.fetch_add(1, Ordering::Relaxed);
            return Ok(e.value.clone());
        }
        self.misses.fetch_add(1, Ordering::Relaxed);
        let etag = entry.as_ref().and_then(|e| e.etag.clone());
        match fetch(etag).await? {
            Fetched::Fresh { value, etag } => {
                *entry = Some(Entry {
                    value: value.clone(),
                    etag,
                    at: Instant::now(),
                });
                Ok(value)
            }
            Fetched::NotModified => match entry.as_mut() {
                Some(e) => {
                    e.at = Instant::now();
                    Ok(e.value.clone())
                }
                // Nothing cached to keep: the next call fetches again.
                None => unreachable_not_modified(),
            },
        }
    }

    /// Drops one key, so the next call fetches.
    pub fn invalidate(&self, key: &str) {
        self.slots
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(key);
    }

    pub fn clear(&self) {
        self.slots.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    /// Hits and misses so far.
    pub fn stats(&self) -> (u64, u64) {
        (
            self.hits.load(Ordering::Relaxed),
            self.misses.load(Ordering::Relaxed),
        )
    }
}

fn unreachable_not_modified<V, E>() -> Result<V, E> {
    panic!("NotModified without a cached value: a fetch given no ETag must return Fresh")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    #[tokio::test]
    async fn keeps_until_ttl_then_revalidates_with_the_etag() {
        let cache: Cache<u32> = Cache::new(Duration::from_millis(50));
        let calls = AtomicUsize::new(0);
        let fetch = |etag: Option<String>| {
            calls.fetch_add(1, Ordering::SeqCst);
            async move {
                Ok::<_, ()>(if etag.as_deref() == Some("v1") {
                    Fetched::NotModified
                } else {
                    Fetched::Fresh {
                        value: 7,
                        etag: Some("v1".into()),
                    }
                })
            }
        };
        assert_eq!(cache.get_or_fetch("k", fetch).await, Ok(7));
        assert_eq!(cache.get_or_fetch("k", fetch).await, Ok(7));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        tokio::time::sleep(Duration::from_millis(60)).await;
        assert_eq!(cache.get_or_fetch("k", fetch).await, Ok(7));
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        assert_eq!(cache.stats(), (1, 2));
        cache.invalidate("k");
        assert_eq!(cache.get_or_fetch("k", fetch).await, Ok(7));
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn callers_at_once_share_one_fetch() {
        let cache: Arc<Cache<u32>> = Arc::new(Cache::new(Duration::from_secs(60)));
        let calls = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..5 {
            let (cache, calls) = (cache.clone(), calls.clone());
            tasks.push(tokio::spawn(async move {
                cache
                    .get_or_fetch("same", |_| async move {
                        calls.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(30)).await;
                        Ok::<_, ()>(Fetched::Fresh {
                            value: 1,
                            etag: None,
                        })
                    })
                    .await
            }));
        }
        for t in tasks {
            assert_eq!(t.await.unwrap(), Ok(1));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_failed_fetch_caches_nothing() {
        let cache: Cache<u32> = Cache::new(Duration::from_secs(60));
        let r: Result<u32, &str> = cache.get_or_fetch("k", |_| async { Err("down") }).await;
        assert_eq!(r, Err("down"));
        let r: Result<u32, &str> = cache
            .get_or_fetch("k", |_| async {
                Ok(Fetched::Fresh {
                    value: 2,
                    etag: None,
                })
            })
            .await;
        assert_eq!(r, Ok(2));
    }
}
