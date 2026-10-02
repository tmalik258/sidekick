use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ulid::Ulid;

/// How sensitive an event's payload is. Secret payloads are never stored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Sensitivity {
    #[default]
    Public,
    Personal,
    Secret,
}

impl Sensitivity {
    pub fn as_str(self) -> &'static str {
        match self {
            Sensitivity::Public => "public",
            Sensitivity::Personal => "personal",
            Sensitivity::Secret => "secret",
        }
    }
}

/// What the user was doing when the event happened.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Context {
    pub active_app: Option<String>,
    pub project_root: Option<String>,
    pub on_battery: Option<bool>,
}

/// A normalized record of something that happened, placed on the event bus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: Ulid,
    pub ts: DateTime<Utc>,
    /// Dotted event kind, for example `file.download_completed`.
    pub kind: String,
    /// Id of the sensor that emitted it.
    pub source: String,
    pub context: Context,
    pub payload: serde_json::Value,
    pub sensitivity: Sensitivity,
}

impl Event {
    pub fn new(
        kind: impl Into<String>,
        source: impl Into<String>,
        payload: serde_json::Value,
    ) -> Self {
        Self {
            id: Ulid::new(),
            ts: Utc::now(),
            kind: kind.into(),
            source: source.into(),
            context: Context::default(),
            payload,
            sensitivity: Sensitivity::Public,
        }
    }

    pub fn with_context(mut self, context: Context) -> Self {
        self.context = context;
        self
    }

    pub fn with_sensitivity(mut self, sensitivity: Sensitivity) -> Self {
        self.sensitivity = sensitivity;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_with_camel_case_and_snake_case_sensitivity() {
        let event = Event::new(
            "file.download_completed",
            "downloads",
            serde_json::json!({"n": 1}),
        )
        .with_sensitivity(Sensitivity::Personal);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["kind"], "file.download_completed");
        assert_eq!(json["sensitivity"], "personal");
        assert!(json["context"].get("activeApp").is_some());
    }
}
