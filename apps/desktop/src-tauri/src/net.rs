//! Internet connection watch. Being on Wi-Fi is not the same as being
//! online, so this tries to reach a few well-known public addresses and
//! tells the island the moment that changes. The page also hears Windows'
//! own "network gone" signal and asks for a check right away.

use std::net::{SocketAddr, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{AppHandle, Emitter};

pub const EVENT: &str = "net://status";

/// Anycast DNS resolvers on 443 and 53: at least one answers on any
/// working connection, including most office networks.
const TARGETS: [&str; 4] = [
    "1.1.1.1:443",
    "8.8.8.8:443",
    "208.67.222.222:443",
    "9.9.9.9:53",
];
const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
const CHECK_ONLINE: Duration = Duration::from_secs(5);
const CHECK_OFFLINE: Duration = Duration::from_secs(2);
/// Failed rounds in a row before calling it offline, so one dropped packet
/// does not raise a notice.
const FAILS_TO_DROP: u32 = 2;

static ONLINE: AtomicBool = AtomicBool::new(true);

pub fn online() -> bool {
    ONLINE.load(Ordering::Relaxed)
}

/// True when any target accepts a connection. The targets are tried one at
/// a time and stop at the first answer, so a healthy check is one connect.
fn reachable() -> bool {
    TARGETS.iter().any(|t| {
        t.parse::<SocketAddr>()
            .is_ok_and(|a| TcpStream::connect_timeout(&a, CONNECT_TIMEOUT).is_ok())
    })
}

async fn probe() -> bool {
    tauri::async_runtime::spawn_blocking(reachable)
        .await
        .unwrap_or(true)
}

fn set(app: &AppHandle, now: bool) {
    if ONLINE.swap(now, Ordering::Relaxed) != now {
        log::info!("net: {}", if now { "online" } else { "offline" });
        let _ = app.emit(EVENT, now);
    }
}

/// Checks right away. `trust_fail` drops to offline on a single failed
/// round, for when Windows already said the network is gone.
pub async fn check(app: &AppHandle, trust_fail: bool) -> bool {
    let ok = probe().await;
    if ok || trust_fail {
        set(app, ok);
    }
    online()
}

pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut fails = 0;
        loop {
            if probe().await {
                fails = 0;
                set(&app, true);
            } else {
                fails += 1;
                if fails >= FAILS_TO_DROP {
                    set(&app, false);
                }
            }
            let wait = if online() && fails == 0 {
                CHECK_ONLINE
            } else {
                CHECK_OFFLINE
            };
            tokio::time::sleep(wait).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_parse() {
        for t in TARGETS {
            assert!(t.parse::<SocketAddr>().is_ok(), "{t}");
        }
    }
}
