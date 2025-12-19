//! Exom Core Library
//!
//! Core models, permissions, hosting logic, and storage for the Exom platform.

pub mod bot;
pub mod chest;
pub mod error;
pub mod hosting;
pub mod invariants;
pub mod manifest;
pub mod models;
pub mod permissions;
pub mod registry;
pub mod storage;

pub use bot::{Bot, BotAction, BotCapability, BotEvent, BotManifest};
pub use manifest::{
    BotCategory, BotManifestToml, BotMeta, CommandDef, ConfigField, ConfigFieldType, ManifestError,
};
pub use registry::BotRegistry;
pub use chest::HallChest;
pub use error::{Error, Result};
pub use hosting::*;
pub use models::*;
pub use permissions::*;
pub use storage::{
    ArchiveConfig, ArchiveConfigStore, ArchiveOutput, ArchiveWindow, BotConfigStore, Database,
    HallBot, HallBotConfig, HallRepository, InviteRepository, LastConnection, LastSeen,
    LastSeenStore, MessageRepository, PersistedTab, PersistedWorkspace, PreferencesStore, Storage,
    UserPreferences, UserRepository, WorkspaceStore,
};
