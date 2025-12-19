//! Bot system types and runtime
//!
//! Minimal bot spine for first-party bots. No WASM yet - just native Rust bots.
//!
//! Bots interact with Exom only through this skeleton:
//! - Events: what bots can listen to
//! - Actions: what bots can do
//! - Commands: slash commands bots can handle
//!
//! The app owns UI, networking, storage, scheduling. Bots never touch those directly.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bot capabilities - what a bot is allowed to do
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BotCapability {
    /// Listen to presence events (join/leave)
    ListenPresence,
    /// Emit ephemeral system messages
    EmitSystem,
    /// Read chat history for a hall
    ReadChatHistory,
    /// Write files to Hall Chest
    WriteChest,
    /// Receive scheduled tick events
    ReceiveScheduledTick,
    /// Handle slash commands
    HandleCommands,
}

/// Bot manifest - describes a bot's identity and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotManifest {
    /// Unique identifier for this bot
    pub id: String,
    /// Display name
    pub name: String,
    /// Version string
    pub version: String,
    /// Declared capabilities
    pub capabilities: Vec<BotCapability>,
}

/// Events that bots can receive
#[derive(Debug, Clone)]
pub enum BotEvent {
    /// A member joined the hall
    MemberJoined {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
        is_first_time: bool,
        last_seen_duration: Option<std::time::Duration>,
    },
    /// A member left the hall
    MemberLeft {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
    },
    /// Scheduled tick for periodic bot tasks (e.g., nightly archiving)
    ScheduledTick {
        hall_id: Uuid,
        /// Current local time as HHMM (e.g., 2200 for 10 PM)
        current_time_hhmm: u16,
    },
    /// Hall connected - fired when app connects to a hall
    /// Bots can use this for startup tasks (e.g., checking missed runs)
    HallConnected {
        hall_id: Uuid,
    },
}

impl BotEvent {
    /// Get the capability required to receive this event
    pub fn required_capability(&self) -> BotCapability {
        match self {
            BotEvent::MemberJoined { .. } | BotEvent::MemberLeft { .. } => {
                BotCapability::ListenPresence
            }
            BotEvent::ScheduledTick { .. } | BotEvent::HallConnected { .. } => {
                BotCapability::ReceiveScheduledTick
            }
        }
    }
}

/// Actions a bot can emit
#[derive(Debug, Clone)]
pub enum BotAction {
    /// Emit an ephemeral system message
    EmitSystem { hall_id: Uuid, content: String },
    /// Write a file to the Hall Chest
    WriteFileToChest {
        hall_id: Uuid,
        /// Relative path within chest (e.g., "archives/ARCHIVE_2024-01-15.md")
        path: String,
        /// File contents
        contents: String,
    },
}

impl BotAction {
    /// Get the capability required to emit this action
    pub fn required_capability(&self) -> BotCapability {
        match self {
            BotAction::EmitSystem { .. } => BotCapability::EmitSystem,
            BotAction::WriteFileToChest { .. } => BotCapability::WriteChest,
        }
    }
}

/// Trait for bot implementations
pub trait Bot: Send + Sync {
    /// Get the bot's manifest
    fn manifest(&self) -> &BotManifest;

    /// Handle an event and return any actions
    fn on_event(&mut self, event: &BotEvent) -> Vec<BotAction>;

    /// Handle a slash command. Returns Some(actions) if this bot handles the command,
    /// None if the command is not for this bot.
    ///
    /// Requires HandleCommands capability to be called.
    fn handle_command(
        &mut self,
        _hall_id: Uuid,
        _user_id: Uuid,
        _command: &str,
    ) -> Option<Vec<BotAction>> {
        None
    }

    /// Return command prefixes this bot handles (e.g., ["/archive", "/set-archive"])
    /// Used by BotRuntime to route commands efficiently.
    fn command_prefixes(&self) -> &[&str] {
        &[]
    }

    /// Check if this bot has a capability
    fn has_capability(&self, cap: BotCapability) -> bool {
        self.manifest().capabilities.contains(&cap)
    }

    /// Check if this bot should receive an event
    fn should_receive(&self, event: &BotEvent) -> bool {
        self.has_capability(event.required_capability())
    }

    /// Check if this bot can emit an action
    fn can_emit(&self, action: &BotAction) -> bool {
        self.has_capability(action.required_capability())
    }
}
