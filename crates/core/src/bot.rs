//! Bot system types and runtime
//!
//! Minimal bot spine for first-party bots. No WASM yet - just native Rust bots.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bot capabilities - what a bot is allowed to do
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BotCapability {
    /// Listen to presence events (join/leave)
    ListenPresence,
    /// Emit ephemeral system messages
    EmitSystem,
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
}

impl BotEvent {
    /// Get the capability required to receive this event
    pub fn required_capability(&self) -> BotCapability {
        match self {
            BotEvent::MemberJoined { .. } | BotEvent::MemberLeft { .. } => {
                BotCapability::ListenPresence
            }
        }
    }
}

/// Actions a bot can emit
#[derive(Debug, Clone)]
pub enum BotAction {
    /// Emit an ephemeral system message
    EmitSystem { hall_id: Uuid, content: String },
}

impl BotAction {
    /// Get the capability required to emit this action
    pub fn required_capability(&self) -> BotCapability {
        match self {
            BotAction::EmitSystem { .. } => BotCapability::EmitSystem,
        }
    }
}

/// Trait for bot implementations
pub trait Bot: Send + Sync {
    /// Get the bot's manifest
    fn manifest(&self) -> &BotManifest;

    /// Handle an event and return any actions
    fn on_event(&mut self, event: &BotEvent) -> Vec<BotAction>;

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
