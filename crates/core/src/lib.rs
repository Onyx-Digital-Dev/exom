//! Exom Core Library
//!
//! Core models, permissions, hosting logic, and storage for the Exom platform.

pub mod bot;
pub mod chest;
pub mod error;
pub mod hosting;
pub mod invariants;
pub mod models;
pub mod permissions;
pub mod storage;

pub use bot::{Bot, BotAction, BotCapability, BotEvent, BotManifest};
pub use chest::HallChest;
pub use error::{Error, Result};
pub use hosting::*;
pub use models::*;
pub use permissions::*;
pub use storage::{
    Database, HallRepository, InviteRepository, LastConnection, LastSeen, LastSeenStore,
    MessageRepository, PersistedTab, PersistedWorkspace, PreferencesStore, Storage,
    UserPreferences, UserRepository, WorkspaceStore,
};
