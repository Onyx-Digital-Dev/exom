//! Bot system types and runtime
//!
//! Comprehensive bot interaction skeleton for Exom. Bots interact with Exom
//! only through this skeleton:
//! - Events: what bots can listen to
//! - Actions: what bots can do
//! - Capabilities: what bots are allowed to do (permission model)
//!
//! The app owns UI, networking, storage, scheduling. Bots never touch those directly.
//! This creates a "narrow waist" architecture where bots are bridges to the outside world.

use std::collections::HashMap;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

// =============================================================================
// CAPABILITIES - What a bot is allowed to do
// =============================================================================

/// Bot capabilities - the permission model for what bots can do
///
/// Capabilities are granted per-hall and must be a subset of what the bot
/// declares in its manifest. This creates a capability-based security model.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum BotCapability {
    // === Event Listening ===
    /// Listen to presence events (join/leave/online/offline)
    ListenPresence,
    /// Listen to chat events (messages, acks)
    ListenChat,
    /// Listen to chest file events (created/modified/deleted)
    ListenChest,

    // === Message Emission ===
    /// Emit ephemeral system messages (not persisted)
    EmitSystem,
    /// Emit persistent chat messages (as bot identity)
    EmitChat,

    // === File Operations ===
    /// Read files from Hall Chest
    ReadChest,
    /// Write files to Hall Chest
    WriteChest,
    /// Delete files from Hall Chest
    DeleteChest,

    // === Moderation (Requests, not direct actions) ===
    /// Request to kick a member (requires host approval)
    RequestKick,
    /// Request to mute a member (requires host approval)
    RequestMute,
    /// Request to change a member's role (requires host approval)
    RequestRoleChange,

    // === Workspace ===
    /// Manage workspace tabs (open/close/update)
    WorkspaceTabs,

    // === External Tools ===
    /// Spawn external tools/processes
    SpawnExternalTools,

    // === Notifications ===
    /// Send user notifications
    Notifications,

    // === Scheduling ===
    /// Receive scheduled tick events
    ReceiveScheduledTick,
    /// Schedule custom timers
    ScheduleTimers,

    // === Commands ===
    /// Handle slash commands
    HandleCommands,

    // === Chat History ===
    /// Read chat history for summarization, archiving, etc.
    ReadChatHistory,

    // === External Integration ===
    /// Make HTTP requests (sandboxed)
    HttpClient,
    /// Provide external signals (webhooks, etc.)
    ExternalSignals,
}

impl BotCapability {
    /// Get a human-readable description of this capability
    pub fn description(&self) -> &'static str {
        match self {
            Self::ListenPresence => "Listen to member join/leave/online/offline events",
            Self::ListenChat => "Listen to chat messages and acknowledgments",
            Self::ListenChest => "Listen to file changes in the Hall Chest",
            Self::EmitSystem => "Send ephemeral system messages",
            Self::EmitChat => "Send persistent chat messages as the bot",
            Self::ReadChest => "Read files from the Hall Chest",
            Self::WriteChest => "Write files to the Hall Chest",
            Self::DeleteChest => "Delete files from the Hall Chest",
            Self::RequestKick => "Request to kick members (requires approval)",
            Self::RequestMute => "Request to mute members (requires approval)",
            Self::RequestRoleChange => "Request role changes (requires approval)",
            Self::WorkspaceTabs => "Manage workspace tabs",
            Self::SpawnExternalTools => "Run external tools and processes",
            Self::Notifications => "Send user notifications",
            Self::ReceiveScheduledTick => "Receive periodic tick events",
            Self::ScheduleTimers => "Schedule custom timer events",
            Self::HandleCommands => "Handle slash commands",
            Self::ReadChatHistory => "Read chat history for the hall",
            Self::HttpClient => "Make HTTP requests to external services",
            Self::ExternalSignals => "Receive signals from external sources",
        }
    }

    /// Check if this is a sensitive capability requiring extra confirmation
    pub fn is_sensitive(&self) -> bool {
        matches!(
            self,
            Self::RequestKick
                | Self::RequestMute
                | Self::RequestRoleChange
                | Self::SpawnExternalTools
                | Self::HttpClient
                | Self::DeleteChest
        )
    }
}

// =============================================================================
// EVENTS - What bots can receive
// =============================================================================

/// Events that bots can receive from the Exom runtime
///
/// Events are dispatched based on bot capabilities. A bot will only receive
/// events for which it has the required capability.
#[derive(Debug, Clone)]
pub enum BotEvent {
    // === Hall Lifecycle ===
    /// Hall was created
    HallCreated { hall_id: Uuid },
    /// Hall was deleted
    HallDeleted { hall_id: Uuid },
    /// Hall was selected by user (became active)
    HallSelected { hall_id: Uuid },
    /// Connected to hall (network established)
    HallConnected { hall_id: Uuid },
    /// Disconnected from hall
    HallDisconnected { hall_id: Uuid },

    // === Presence Events ===
    /// A member joined the hall (first time or returning)
    MemberJoined {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
        is_first_time: bool,
        last_seen_duration: Option<Duration>,
    },
    /// A member left the hall
    MemberLeft {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
    },
    /// A member came online (was offline, now active)
    MemberOnline {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
    },
    /// A member went offline (was active, now away)
    MemberOffline {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
    },
    /// Host changed
    HostChanged {
        hall_id: Uuid,
        old_host_id: Option<Uuid>,
        new_host_id: Uuid,
        new_host_username: String,
    },
    /// A member's role changed
    RoleChanged {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
        old_role: u8,
        new_role: u8,
    },

    // === Chat Events ===
    /// A chat message was received
    ChatMessageReceived {
        hall_id: Uuid,
        message_id: Uuid,
        sender_id: Uuid,
        sender_username: String,
        content: String,
        timestamp: chrono::DateTime<chrono::Utc>,
    },
    /// A chat message was acknowledged (delivered)
    ChatMessageAcked {
        hall_id: Uuid,
        message_id: Uuid,
    },

    // === Commands ===
    /// A slash command was received (for bots that handle commands)
    CommandReceived {
        hall_id: Uuid,
        user_id: Uuid,
        username: String,
        command: String,
        args: Vec<String>,
    },

    // === Scheduling ===
    /// Periodic tick (every minute while hall is active)
    ScheduledTick {
        hall_id: Uuid,
        /// Current local time as HHMM (e.g., 2200 for 10 PM)
        current_time_hhmm: u16,
    },
    /// Daily tick (once per day at configured time)
    DailyTick {
        hall_id: Uuid,
        /// Day of year (1-366)
        day_of_year: u16,
    },
    /// Custom timer fired
    TimerFired {
        hall_id: Uuid,
        timer_id: String,
        /// Payload set when timer was scheduled
        payload: Option<String>,
    },

    // === Chest Events ===
    /// A file was created in the Hall Chest
    ChestFileCreated {
        hall_id: Uuid,
        path: String,
        size_bytes: u64,
    },
    /// A file was modified in the Hall Chest
    ChestFileModified {
        hall_id: Uuid,
        path: String,
        size_bytes: u64,
    },
    /// A file was deleted from the Hall Chest
    ChestFileDeleted { hall_id: Uuid, path: String },

    // === External Integration ===
    /// An external signal was received (webhook, etc.)
    ExternalSignalReceived {
        hall_id: Uuid,
        signal_type: String,
        payload: String,
    },
}

impl BotEvent {
    /// Get the capability required to receive this event
    pub fn required_capability(&self) -> BotCapability {
        match self {
            // Hall lifecycle events require scheduled tick capability
            Self::HallCreated { .. }
            | Self::HallDeleted { .. }
            | Self::HallSelected { .. }
            | Self::HallConnected { .. }
            | Self::HallDisconnected { .. } => BotCapability::ReceiveScheduledTick,

            // Presence events
            Self::MemberJoined { .. }
            | Self::MemberLeft { .. }
            | Self::MemberOnline { .. }
            | Self::MemberOffline { .. }
            | Self::HostChanged { .. }
            | Self::RoleChanged { .. } => BotCapability::ListenPresence,

            // Chat events
            Self::ChatMessageReceived { .. } | Self::ChatMessageAcked { .. } => {
                BotCapability::ListenChat
            }

            // Commands
            Self::CommandReceived { .. } => BotCapability::HandleCommands,

            // Scheduling
            Self::ScheduledTick { .. } | Self::DailyTick { .. } | Self::TimerFired { .. } => {
                BotCapability::ReceiveScheduledTick
            }

            // Chest events
            Self::ChestFileCreated { .. }
            | Self::ChestFileModified { .. }
            | Self::ChestFileDeleted { .. } => BotCapability::ListenChest,

            // External signals
            Self::ExternalSignalReceived { .. } => BotCapability::ExternalSignals,
        }
    }
}

// =============================================================================
// ACTIONS - What bots can do
// =============================================================================

/// Actions a bot can emit for the Exom runtime to execute
///
/// Actions are validated against bot capabilities before execution.
/// Some actions are "requests" that require host/admin approval.
#[derive(Debug, Clone)]
pub enum BotAction {
    // === Message Emission ===
    /// Emit an ephemeral system message (not persisted, local only)
    EmitSystemMessage { hall_id: Uuid, content: String },
    /// Emit a persistent chat message (as bot identity)
    EmitChatMessage { hall_id: Uuid, content: String },
    /// React to a message with an emoji
    ReactToMessage {
        hall_id: Uuid,
        message_id: Uuid,
        emoji: String,
    },

    // === Moderation Requests ===
    /// Request to kick a member (requires host approval)
    RequestKickMember {
        hall_id: Uuid,
        user_id: Uuid,
        reason: Option<String>,
    },
    /// Request to mute a member (requires host approval)
    RequestMuteMember {
        hall_id: Uuid,
        user_id: Uuid,
        duration_seconds: Option<u64>,
        reason: Option<String>,
    },
    /// Request to change a member's role (requires host approval)
    RequestRoleChange {
        hall_id: Uuid,
        user_id: Uuid,
        new_role: u8,
        reason: Option<String>,
    },

    // === File Operations ===
    /// Write a file to the Hall Chest
    WriteFileToChest {
        hall_id: Uuid,
        /// Relative path within chest (e.g., "archives/ARCHIVE_2024-01-15.md")
        path: String,
        /// File contents
        contents: String,
    },
    /// Read a file from the Hall Chest
    ReadFileFromChest {
        hall_id: Uuid,
        path: String,
        /// Response channel ID for async response
        response_id: String,
    },
    /// List files in the Hall Chest
    ListChestFiles {
        hall_id: Uuid,
        /// Optional path prefix filter
        prefix: Option<String>,
        /// Response channel ID for async response
        response_id: String,
    },
    /// Delete a file from the Hall Chest
    DeleteChestFile { hall_id: Uuid, path: String },

    // === Workspace Tabs ===
    /// Open a new workspace tab
    OpenWorkspaceTab {
        hall_id: Uuid,
        tab_id: String,
        title: String,
        /// Content type: "markdown", "html", "iframe", etc.
        content_type: String,
        content: String,
    },
    /// Close a workspace tab
    CloseWorkspaceTab { hall_id: Uuid, tab_id: String },
    /// Update a workspace tab's content
    UpdateWorkspaceTab {
        hall_id: Uuid,
        tab_id: String,
        title: Option<String>,
        content: Option<String>,
    },

    // === External Tools ===
    /// Spawn an external tool/process
    SpawnExternalTool {
        hall_id: Uuid,
        tool_id: String,
        command: String,
        args: Vec<String>,
        /// Working directory (within allowed paths)
        cwd: Option<String>,
    },
    /// Stop a running external tool
    StopExternalTool { hall_id: Uuid, tool_id: String },

    // === Notifications ===
    /// Send a notification to the user
    NotifyUser {
        hall_id: Uuid,
        /// Target user (None = all members)
        user_id: Option<Uuid>,
        title: String,
        body: String,
        /// Notification urgency: "low", "normal", "high"
        urgency: String,
    },
    /// Flash the UI to get attention
    FlashUI {
        hall_id: Uuid,
        /// Flash type: "taskbar", "window", "both"
        flash_type: String,
    },

    // === Configuration ===
    /// Set a hall config value (bot's own namespace only)
    SetHallConfig {
        hall_id: Uuid,
        key: String,
        value: serde_json::Value,
    },
    /// Get a hall config value (response via event)
    GetHallConfig {
        hall_id: Uuid,
        key: String,
        response_id: String,
    },

    // === Timers ===
    /// Schedule a timer to fire after a delay
    ScheduleTimer {
        hall_id: Uuid,
        timer_id: String,
        delay_seconds: u64,
        /// Payload to include when timer fires
        payload: Option<String>,
    },
    /// Cancel a scheduled timer
    CancelTimer { hall_id: Uuid, timer_id: String },

    // === HTTP Requests ===
    /// Make an HTTP request (sandboxed)
    HttpRequest {
        hall_id: Uuid,
        request_id: String,
        method: String,
        url: String,
        headers: HashMap<String, String>,
        body: Option<String>,
        /// Timeout in seconds (max enforced by runtime)
        timeout_seconds: Option<u64>,
    },
}

impl BotAction {
    /// Get the capability required to emit this action
    pub fn required_capability(&self) -> BotCapability {
        match self {
            Self::EmitSystemMessage { .. } => BotCapability::EmitSystem,
            Self::EmitChatMessage { .. } | Self::ReactToMessage { .. } => BotCapability::EmitChat,

            Self::RequestKickMember { .. } => BotCapability::RequestKick,
            Self::RequestMuteMember { .. } => BotCapability::RequestMute,
            Self::RequestRoleChange { .. } => BotCapability::RequestRoleChange,

            Self::WriteFileToChest { .. } => BotCapability::WriteChest,
            Self::ReadFileFromChest { .. } | Self::ListChestFiles { .. } => BotCapability::ReadChest,
            Self::DeleteChestFile { .. } => BotCapability::DeleteChest,

            Self::OpenWorkspaceTab { .. }
            | Self::CloseWorkspaceTab { .. }
            | Self::UpdateWorkspaceTab { .. } => BotCapability::WorkspaceTabs,

            Self::SpawnExternalTool { .. } | Self::StopExternalTool { .. } => {
                BotCapability::SpawnExternalTools
            }

            Self::NotifyUser { .. } | Self::FlashUI { .. } => BotCapability::Notifications,

            Self::SetHallConfig { .. } | Self::GetHallConfig { .. } => {
                BotCapability::ReceiveScheduledTick
            }

            Self::ScheduleTimer { .. } | Self::CancelTimer { .. } => BotCapability::ScheduleTimers,

            Self::HttpRequest { .. } => BotCapability::HttpClient,
        }
    }
}

// =============================================================================
// BOT MANIFEST - Describes a bot's identity and declared capabilities
// =============================================================================

/// Bot manifest - describes a bot's identity and capabilities
///
/// This is the runtime manifest, which may be loaded from a TOML file
/// or constructed programmatically for built-in bots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotManifest {
    /// Unique identifier for this bot
    pub id: String,
    /// Display name
    pub name: String,
    /// Version string
    pub version: String,
    /// Declared capabilities (what the bot wants)
    pub capabilities: Vec<BotCapability>,
}

// =============================================================================
// BOT TRAIT - The interface that bot implementations must satisfy
// =============================================================================

/// Trait for bot implementations
///
/// Bots implement this trait to receive events and emit actions.
/// The runtime handles all capability validation and action execution.
pub trait Bot: Send + Sync {
    /// Get the bot's manifest
    fn manifest(&self) -> &BotManifest;

    /// Handle an event and return any actions
    ///
    /// Called by the runtime when an event occurs that the bot should receive.
    /// The runtime has already validated that the bot has the required capability.
    fn on_event(&mut self, event: &BotEvent) -> Vec<BotAction>;

    /// Handle a slash command
    ///
    /// Returns Some(actions) if this bot handles the command, None otherwise.
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
    ///
    /// Used by BotRuntime to route commands efficiently.
    fn command_prefixes(&self) -> &[&str] {
        &[]
    }

    /// Check if this bot has a capability declared in its manifest
    fn has_capability(&self, cap: BotCapability) -> bool {
        self.manifest().capabilities.contains(&cap)
    }

    /// Check if this bot should receive an event (has required capability)
    fn should_receive(&self, event: &BotEvent) -> bool {
        self.has_capability(event.required_capability())
    }

    /// Check if this bot can emit an action (has required capability)
    fn can_emit(&self, action: &BotAction) -> bool {
        self.has_capability(action.required_capability())
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_descriptions() {
        // Ensure all capabilities have descriptions
        let caps = [
            BotCapability::ListenPresence,
            BotCapability::ListenChat,
            BotCapability::ListenChest,
            BotCapability::EmitSystem,
            BotCapability::EmitChat,
            BotCapability::ReadChest,
            BotCapability::WriteChest,
            BotCapability::DeleteChest,
            BotCapability::RequestKick,
            BotCapability::RequestMute,
            BotCapability::RequestRoleChange,
            BotCapability::WorkspaceTabs,
            BotCapability::SpawnExternalTools,
            BotCapability::Notifications,
            BotCapability::ReceiveScheduledTick,
            BotCapability::ScheduleTimers,
            BotCapability::HandleCommands,
            BotCapability::ReadChatHistory,
            BotCapability::HttpClient,
            BotCapability::ExternalSignals,
        ];

        for cap in caps {
            assert!(!cap.description().is_empty());
        }
    }

    #[test]
    fn test_sensitive_capabilities() {
        assert!(BotCapability::RequestKick.is_sensitive());
        assert!(BotCapability::SpawnExternalTools.is_sensitive());
        assert!(BotCapability::HttpClient.is_sensitive());
        assert!(!BotCapability::ListenPresence.is_sensitive());
        assert!(!BotCapability::EmitSystem.is_sensitive());
    }

    #[test]
    fn test_event_capability_mapping() {
        let event = BotEvent::MemberJoined {
            hall_id: Uuid::new_v4(),
            user_id: Uuid::new_v4(),
            username: "test".to_string(),
            is_first_time: true,
            last_seen_duration: None,
        };
        assert_eq!(event.required_capability(), BotCapability::ListenPresence);

        let event = BotEvent::ChatMessageReceived {
            hall_id: Uuid::new_v4(),
            message_id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_username: "test".to_string(),
            content: "hello".to_string(),
            timestamp: chrono::Utc::now(),
        };
        assert_eq!(event.required_capability(), BotCapability::ListenChat);
    }

    #[test]
    fn test_action_capability_mapping() {
        let action = BotAction::EmitSystemMessage {
            hall_id: Uuid::new_v4(),
            content: "test".to_string(),
        };
        assert_eq!(action.required_capability(), BotCapability::EmitSystem);

        let action = BotAction::WriteFileToChest {
            hall_id: Uuid::new_v4(),
            path: "test.txt".to_string(),
            contents: "hello".to_string(),
        };
        assert_eq!(action.required_capability(), BotCapability::WriteChest);
    }
}
