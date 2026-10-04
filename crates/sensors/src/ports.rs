use std::collections::{HashMap, HashSet};
use std::time::Duration;

use listeners::{Protocol, SocketState};
use sidekick_core::{Event, EventBus};
use tokio::task::JoinHandle;

use crate::{Sensor, SensorGate};

/// Notices local TCP servers starting and stopping. Windows has
/// no push notification for new listening sockets, so this checks once a
/// second, which keeps its cost negligible.
pub struct PortsSensor;

impl PortsSensor {
    pub const ID: &'static str = "ports";
    pub const LISTENING: &'static str = "port.listening";
    pub const CLOSED: &'static str = "port.closed";
}

const CHECK_EVERY: Duration = Duration::from_secs(1);

/// System services that listen on high ports and would only add noise.
const IGNORED: &[&str] = &[
    "system",
    "svchost.exe",
    "lsass.exe",
    "wininit.exe",
    "services.exe",
    "spoolsv.exe",
    "jhi_service.exe",
    "msedgewebview2.exe",
    "steam.exe",
    "discord.exe",
    "spotify.exe",
    "onedrive.exe",
    "sidekick-desktop.exe",
    "sidekick-desktop",
    "systemd",
    "systemd-resolved",
    "sshd",
    "cupsd",
];

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Socket {
    port: u16,
    pid: u32,
    process: String,
    address: String,
}

fn snapshot() -> HashSet<Socket> {
    let Ok(all) = listeners::get_all() else {
        return HashSet::new();
    };
    all.into_iter()
        .filter(|l| {
            l.protocol == Protocol::TCP && l.state == SocketState::Listen && l.socket.port() >= 1024
        })
        .filter(|l| !IGNORED.contains(&l.process.name.to_ascii_lowercase().as_str()))
        .map(|l| Socket {
            port: l.socket.port(),
            pid: l.process.pid,
            process: l.process.name.clone(),
            address: l.socket.ip().to_string(),
        })
        .collect()
}

/// Collapses IPv4 and IPv6 sockets for the same port and process.
fn by_port(sockets: &HashSet<Socket>) -> HashMap<(u16, u32), &Socket> {
    sockets.iter().map(|s| ((s.port, s.pid), s)).collect()
}

impl Sensor for PortsSensor {
    fn id(&self) -> &'static str {
        Self::ID
    }

    fn spawn(self: Box<Self>, bus: EventBus, gate: SensorGate) -> JoinHandle<()> {
        tokio::spawn(async move {
            // Servers already running when Sidekick starts are not news.
            let mut known = tokio::task::spawn_blocking(snapshot)
                .await
                .unwrap_or_default();
            let mut tick = tokio::time::interval(CHECK_EVERY);
            loop {
                tick.tick().await;
                let current = tokio::task::spawn_blocking(snapshot)
                    .await
                    .unwrap_or_default();
                if gate.allows(Self::ID) {
                    let (before, after) = (by_port(&known), by_port(&current));
                    for (key, s) in &after {
                        if !before.contains_key(key) {
                            bus.publish(port_event(Self::LISTENING, s));
                        }
                    }
                    for (key, s) in &before {
                        if !after.contains_key(key) {
                            bus.publish(port_event(Self::CLOSED, s));
                        }
                    }
                }
                known = current;
            }
        })
    }
}

fn port_event(kind: &str, s: &Socket) -> Event {
    let process = s.process.trim_end_matches(".exe").to_ascii_lowercase();
    Event::new(
        kind,
        PortsSensor::ID,
        serde_json::json!({
            "port": s.port,
            "pid": s.pid,
            "process": process,
            "address": s.address,
            "url": format!("http://localhost:{}", s.port),
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::GateState;

    #[tokio::test(flavor = "multi_thread")]
    async fn reports_a_new_server_and_its_shutdown() {
        let bus = EventBus::default();
        let mut rx = bus.subscribe();
        let (_handle, gate) = SensorGate::new(GateState::default());
        let task = Box::new(PortsSensor).spawn(bus, gate);
        tokio::time::sleep(Duration::from_millis(1500)).await;

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let opened = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let e = rx.recv().await.unwrap();
                if e.kind == PortsSensor::LISTENING && e.payload["port"] == port {
                    return e;
                }
            }
        })
        .await
        .expect("listening event in time");
        assert_eq!(opened.payload["url"], format!("http://localhost:{port}"));

        drop(listener);
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let e = rx.recv().await.unwrap();
                if e.kind == PortsSensor::CLOSED && e.payload["port"] == port {
                    return;
                }
            }
        })
        .await
        .expect("closed event in time");
        task.abort();
    }
}
