//! Core building blocks shared by the Sidekick desktop app and its sensors.
//!
//! - [`event`]: the event envelope every sensor emits (SRS 9.2).
//! - [`bus`]: in-process fan-out of events to subscribers.
//! - [`mascot`]: the mascot and island state machine (SRS 7.1).
//! - [`settings`]: user settings persisted as JSON.
//! - [`storage`]: SQLite storage for events and history.

pub mod bus;
pub mod event;
pub mod mascot;
pub mod settings;
pub mod storage;

pub use bus::EventBus;
pub use event::{Context, Event, Sensitivity};
pub use mascot::{Cue, MascotEvent, MascotMachine, MascotState, Transition};
pub use settings::{
    AI_PROVIDERS, AgentSettings, AiSettings, ComposioSettings, NOTIFY_LEVELS, NotificationSettings,
    Pause, Recipe, SHORTCUTS, Settings, SkillPref, Trigger,
};
pub use storage::{
    ActionRecord, AppTime, ChatRow, ChatSummary, Habit, RoutineOpen, SearchHit, Storage, StoredEvent,
};
