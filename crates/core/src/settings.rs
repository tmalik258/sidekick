use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};

/// Whether sensors are paused (FR-SET-01).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", content = "until", rename_all = "snake_case")]
pub enum Pause {
    #[default]
    None,
    Until(DateTime<Utc>),
    Indefinite,
}

impl Pause {
    pub fn for_minutes(minutes: u32, now: DateTime<Utc>) -> Self {
        Pause::Until(now + Duration::minutes(i64::from(minutes)))
    }

    pub fn is_active(&self, now: DateTime<Utc>) -> bool {
        match self {
            Pause::None => false,
            Pause::Until(until) => *until > now,
            Pause::Indefinite => true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub muted: bool,
    /// 0.0 to 1.0.
    pub master_volume: f32,
    /// Per-cue volume, 0.0 to 1.0, keyed by cue name. Missing means 1.0.
    pub cue_volumes: BTreeMap<String, f32>,
    /// Seconds before an expanded island collapses with no interaction.
    pub collapse_after_secs: u32,
    pub launch_at_login: bool,
    /// Enabled flag per sensor id. Missing means enabled.
    pub sensors: BTreeMap<String, bool>,
    pub pause: Pause,
    /// Orb appearance: one of [`THEMES`].
    pub theme: String,
    /// UI sound kit: one of [`SOUND_KITS`].
    pub sound_kit: String,
}

pub const THEMES: [&str; 3] = ["pearl", "graphite", "midnight"];
pub const SOUND_KITS: [&str; 1] = ["01"];

impl Default for Settings {
    fn default() -> Self {
        Self {
            muted: false,
            master_volume: 0.6,
            cue_volumes: BTreeMap::new(),
            collapse_after_secs: 8,
            launch_at_login: false,
            sensors: BTreeMap::new(),
            pause: Pause::None,
            theme: THEMES[0].to_string(),
            sound_kit: SOUND_KITS[0].to_string(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Settings {
    /// Loads settings, falling back to defaults when the file is missing.
    /// A corrupt file is logged and replaced by defaults rather than failing
    /// app start.
    pub fn load(path: &Path) -> Self {
        match fs::read_to_string(path) {
            Ok(raw) => serde_json::from_str::<Settings>(&raw)
                .map(Settings::sanitized)
                .unwrap_or_else(|err| {
                    log::warn!(
                        "invalid settings at {}: {err}; using defaults",
                        path.display()
                    );
                    Settings::default()
                }),
            Err(_) => Settings::default(),
        }
    }

    /// Writes atomically: temp file, then rename.
    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    /// Clamps values that came from the UI or a hand-edited file.
    pub fn sanitized(mut self) -> Self {
        self.master_volume = self.master_volume.clamp(0.0, 1.0);
        for v in self.cue_volumes.values_mut() {
            *v = v.clamp(0.0, 1.0);
        }
        self.collapse_after_secs = self.collapse_after_secs.clamp(2, 120);
        if !THEMES.contains(&self.theme.as_str()) {
            self.theme = THEMES[0].to_string();
        }
        if !SOUND_KITS.contains(&self.sound_kit.as_str()) {
            self.sound_kit = SOUND_KITS[0].to_string();
        }
        self
    }

    pub fn sensor_enabled(&self, id: &str) -> bool {
        self.sensors.get(id).copied().unwrap_or(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_until_expires() {
        let now = Utc::now();
        let pause = Pause::for_minutes(15, now);
        assert!(pause.is_active(now));
        assert!(!pause.is_active(now + Duration::minutes(16)));
        assert!(Pause::Indefinite.is_active(now));
        assert!(!Pause::None.is_active(now));
    }

    #[test]
    fn pause_serializes_as_tagged_object() {
        assert_eq!(
            serde_json::to_value(Pause::None).unwrap(),
            serde_json::json!({"kind": "none"})
        );
        assert_eq!(
            serde_json::to_value(Pause::Indefinite).unwrap(),
            serde_json::json!({"kind": "indefinite"})
        );
    }

    #[test]
    fn round_trips_through_disk_and_fills_missing_fields() {
        let dir = std::env::temp_dir().join(format!("sidekick-settings-{}", ulid::Ulid::new()));
        let path = dir.join("settings.json");
        let s = Settings {
            muted: true,
            sensors: BTreeMap::from([("heartbeat".to_string(), false)]),
            ..Settings::default()
        };
        s.save(&path).unwrap();
        let loaded = Settings::load(&path);
        assert_eq!(loaded, s);
        assert!(!loaded.sensor_enabled("heartbeat"));
        assert!(loaded.sensor_enabled("files"));

        fs::write(&path, r#"{"muted": true}"#).unwrap();
        let partial = Settings::load(&path);
        assert!(partial.muted);
        assert_eq!(partial.collapse_after_secs, 8);
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn unknown_theme_and_kit_fall_back_to_defaults() {
        let s = Settings {
            theme: "neon".into(),
            sound_kit: "99".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.theme, "pearl");
        assert_eq!(s.sound_kit, "01");
    }

    #[test]
    fn sanitizes_out_of_range_values() {
        let s = Settings {
            master_volume: 3.0,
            collapse_after_secs: 0,
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.master_volume, 1.0);
        assert_eq!(s.collapse_after_secs, 2);
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("sidekick-settings-{}", ulid::Ulid::new()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("settings.json");
        fs::write(&path, "not json").unwrap();
        assert_eq!(Settings::load(&path), Settings::default());
        fs::remove_dir_all(dir).unwrap();
    }
}
