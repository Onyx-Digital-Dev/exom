//! Application state management

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use exom_core::{Database, Error, HallChest, Result};
use exom_net::{CurrentTool, PresenceStatus};
use uuid::Uuid;

use crate::external_tools::{ExternalToolRuntime, SharedToolRuntime};

/// Helper trait for safe mutex locking that recovers from poisoned locks
pub trait SafeLock<T> {
    fn safe_lock(&self) -> MutexGuard<'_, T>;
}

impl<T> SafeLock<T> for Mutex<T> {
    fn safe_lock(&self) -> MutexGuard<'_, T> {
        match self.lock() {
            Ok(guard) => guard,
            Err(poisoned) => {
                tracing::warn!("Mutex poisoned, recovering");
                poisoned.into_inner()
            }
        }
    }
}

/// Ephemeral system message (not persisted)
#[derive(Debug, Clone)]
pub struct SystemMessage {
    pub id: Uuid,
    pub hall_id: Uuid,
    pub content: String,
    pub timestamp: DateTime<Utc>,
}

/// Tracked member for join/leave detection
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackedMember {
    pub user_id: Uuid,
    pub username: String,
}

/// Member presence info for K2/K3
#[derive(Debug, Clone)]
pub struct MemberPresence {
    pub user_id: Uuid,
    pub username: String,
    pub presence: PresenceStatus,
    pub current_tool: CurrentTool,
}

/// Main application state
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub chest: Arc<Mutex<HallChest>>,
    pub current_user_id: Arc<Mutex<Option<Uuid>>>,
    pub current_session_id: Arc<Mutex<Option<Uuid>>>,
    pub current_hall_id: Arc<Mutex<Option<Uuid>>>,
    /// Ephemeral system messages (join/leave/host changes)
    pub system_messages: Arc<Mutex<Vec<SystemMessage>>>,
    /// Currently known members (for detecting joins/leaves)
    pub known_members: Arc<Mutex<Vec<TrackedMember>>>,
    /// Messages pending delivery confirmation
    pub pending_messages: Arc<Mutex<HashSet<Uuid>>>,
    /// Users currently typing in halls: user_id -> (username, last_typing_time)
    pub typing_users: Arc<Mutex<HashMap<Uuid, (String, Instant)>>>,
    /// Member last activity timestamps: user_id -> last_active_time
    pub member_activity: Arc<Mutex<HashMap<Uuid, Instant>>>,
    /// Member presence info: user_id -> presence state (K2/K3)
    pub member_presence: Arc<Mutex<HashMap<Uuid, MemberPresence>>>,
    /// Local user's last activity time for idle detection
    pub last_local_activity: Arc<Mutex<Instant>>,
    /// Local user's current tool
    pub local_current_tool: Arc<Mutex<CurrentTool>>,
    /// Local user's presence status
    pub local_presence: Arc<Mutex<PresenceStatus>>,
    /// External tools runtime (spawned processes per hall)
    pub tools: SharedToolRuntime,
}

impl AppState {
    pub fn new() -> Result<Self> {
        let db_path = Self::data_path()?.join("exom.db");

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let db = Database::open(&db_path)?;
        let chest = HallChest::new()?;

        Ok(Self {
            db: Arc::new(Mutex::new(db)),
            chest: Arc::new(Mutex::new(chest)),
            current_user_id: Arc::new(Mutex::new(None)),
            current_session_id: Arc::new(Mutex::new(None)),
            current_hall_id: Arc::new(Mutex::new(None)),
            system_messages: Arc::new(Mutex::new(Vec::new())),
            known_members: Arc::new(Mutex::new(Vec::new())),
            pending_messages: Arc::new(Mutex::new(HashSet::new())),
            typing_users: Arc::new(Mutex::new(HashMap::new())),
            member_activity: Arc::new(Mutex::new(HashMap::new())),
            member_presence: Arc::new(Mutex::new(HashMap::new())),
            last_local_activity: Arc::new(Mutex::new(Instant::now())),
            local_current_tool: Arc::new(Mutex::new(CurrentTool::Chat)),
            local_presence: Arc::new(Mutex::new(PresenceStatus::Active)),
            tools: Arc::new(Mutex::new(ExternalToolRuntime::new())),
        })
    }

    fn data_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "onyx", "exom").ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine data directory",
            ))
        })?;

        Ok(dirs.data_dir().to_path_buf())
    }

    /// Get the data directory path
    pub fn data_dir(&self) -> PathBuf {
        Self::data_path().unwrap_or_else(|_| PathBuf::from("."))
    }

    pub fn set_current_user(&self, user_id: Option<Uuid>) {
        *self.current_user_id.lock().unwrap() = user_id;
    }

    pub fn set_current_session(&self, session_id: Option<Uuid>) {
        *self.current_session_id.lock().unwrap() = session_id;
    }

    pub fn set_current_hall(&self, hall_id: Option<Uuid>) {
        *self.current_hall_id.lock().unwrap() = hall_id;
    }

    pub fn current_user_id(&self) -> Option<Uuid> {
        *self.current_user_id.lock().unwrap()
    }

    pub fn current_session_id(&self) -> Option<Uuid> {
        *self.current_session_id.lock().unwrap()
    }

    pub fn current_hall_id(&self) -> Option<Uuid> {
        *self.current_hall_id.lock().unwrap()
    }

    /// Get current host name for the selected hall (if any)
    pub fn current_host_name(&self) -> Option<String> {
        let hall_id = self.current_hall_id()?;
        let db = self.db.lock().unwrap();
        db.halls().get_current_host_name(hall_id).ok().flatten()
    }

    /// Get current username for the logged-in user
    pub fn current_username(&self) -> Option<String> {
        let user_id = self.current_user_id()?;
        let db = self.db.lock().unwrap();
        db.users()
            .find_by_id(user_id)
            .ok()
            .flatten()
            .map(|u| u.username)
    }

    /// Add a system message (join/leave/host change)
    pub fn add_system_message(&self, hall_id: Uuid, content: String) {
        let msg = SystemMessage {
            id: Uuid::new_v4(),
            hall_id,
            content,
            timestamp: Utc::now(),
        };
        self.system_messages.lock().unwrap().push(msg);
    }

    /// Get system messages for a hall
    pub fn get_system_messages(&self, hall_id: Uuid) -> Vec<SystemMessage> {
        self.system_messages
            .lock()
            .unwrap()
            .iter()
            .filter(|m| m.hall_id == hall_id)
            .cloned()
            .collect()
    }

    /// Clear system messages for a hall (e.g., when leaving)
    #[allow(dead_code)]
    pub fn clear_system_messages(&self, hall_id: Uuid) {
        self.system_messages
            .lock()
            .unwrap()
            .retain(|m| m.hall_id != hall_id);
    }

    /// Update known members and return (joined, left) as TrackedMember structs
    pub fn update_known_members(
        &self,
        new_members: Vec<TrackedMember>,
    ) -> (Vec<TrackedMember>, Vec<TrackedMember>) {
        let mut known = self.known_members.lock().unwrap();
        let my_user_id = self.current_user_id();

        // Find who joined (in new but not in known)
        let joined: Vec<TrackedMember> = new_members
            .iter()
            .filter(|m| {
                !known.iter().any(|k| k.user_id == m.user_id) && my_user_id != Some(m.user_id)
            })
            .cloned()
            .collect();

        // Find who left (in known but not in new)
        let left: Vec<TrackedMember> = known
            .iter()
            .filter(|k| {
                !new_members.iter().any(|m| m.user_id == k.user_id) && my_user_id != Some(k.user_id)
            })
            .cloned()
            .collect();

        // Update known members
        *known = new_members;

        (joined, left)
    }

    /// Clear known members (e.g., when disconnecting)
    pub fn clear_known_members(&self) {
        self.known_members.lock().unwrap().clear();
    }

    /// Mark a message as pending delivery
    pub fn add_pending_message(&self, message_id: Uuid) {
        self.pending_messages.lock().unwrap().insert(message_id);
    }

    /// Mark a message as delivered (remove from pending)
    pub fn confirm_message(&self, message_id: Uuid) {
        self.pending_messages.lock().unwrap().remove(&message_id);
    }

    /// Check if a message is pending delivery
    pub fn is_message_pending(&self, message_id: Uuid) -> bool {
        self.pending_messages.lock().unwrap().contains(&message_id)
    }

    /// Reconcile pending messages against database
    /// Messages that exist in DB but are still marked pending should be confirmed
    /// Returns count of messages reconciled
    pub fn reconcile_pending_messages(&self, _hall_id: Uuid) -> usize {
        let pending: Vec<Uuid> = self
            .pending_messages
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect();
        if pending.is_empty() {
            return 0;
        }

        // Check which pending messages exist in DB
        let to_confirm: Vec<Uuid> = {
            let db = self.db.lock().unwrap();
            pending
                .into_iter()
                .filter(|msg_id| db.messages().find_by_id(*msg_id).ok().flatten().is_some())
                .collect()
        };

        // Confirm them (db lock released)
        let count = to_confirm.len();
        for msg_id in to_confirm {
            self.confirm_message(msg_id);
        }

        count
    }

    /// Set a user as typing (or update their last typing time)
    pub fn set_user_typing(&self, user_id: Uuid, username: String) {
        self.typing_users
            .lock()
            .unwrap()
            .insert(user_id, (username, Instant::now()));
    }

    /// Clear a user's typing status
    pub fn clear_user_typing(&self, user_id: Uuid) {
        self.typing_users.lock().unwrap().remove(&user_id);
    }

    /// Clear all typing users (e.g., when disconnecting)
    pub fn clear_all_typing(&self) {
        self.typing_users.lock().unwrap().clear();
    }

    /// Get list of currently typing users (excluding self), returns (user_id, username)
    pub fn get_typing_users(&self) -> Vec<(Uuid, String)> {
        let my_user_id = self.current_user_id();
        self.typing_users
            .lock()
            .unwrap()
            .iter()
            .filter(|(uid, _)| my_user_id != Some(**uid))
            .map(|(uid, (username, _))| (*uid, username.clone()))
            .collect()
    }

    /// Prune stale typing entries (older than threshold)
    /// Returns true if any entries were pruned
    pub fn prune_typing_users(&self, max_age_ms: u64) -> bool {
        let mut typing = self.typing_users.lock().unwrap();
        let before = typing.len();
        typing.retain(|_, (_, instant)| instant.elapsed().as_millis() < max_age_ms as u128);
        typing.len() < before
    }

    /// Get list of pending message IDs
    pub fn get_pending_messages(&self) -> Vec<Uuid> {
        self.pending_messages
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .collect()
    }

    /// Update last activity timestamp for a user
    pub fn update_member_activity(&self, user_id: Uuid) {
        self.member_activity
            .lock()
            .unwrap()
            .insert(user_id, Instant::now());
    }

    /// Get activity hint for a user (e.g., "Active", "5m", "2h", "3d")
    pub fn get_activity_hint(&self, user_id: Uuid) -> String {
        let activity = self.member_activity.lock().unwrap();
        match activity.get(&user_id) {
            Some(instant) => format_activity_hint(instant.elapsed()),
            None => String::new(),
        }
    }

    /// Clear all activity data (e.g., when switching halls)
    #[allow(dead_code)]
    pub fn clear_member_activity(&self) {
        self.member_activity.lock().unwrap().clear();
    }

    // ===== Presence tracking (K2/K3) =====

    /// Update member presence from network
    pub fn update_member_presence(
        &self,
        user_id: Uuid,
        username: String,
        presence: PresenceStatus,
        current_tool: CurrentTool,
    ) {
        self.member_presence.lock().unwrap().insert(
            user_id,
            MemberPresence {
                user_id,
                username,
                presence,
                current_tool,
            },
        );
    }

    /// Get member presence info
    pub fn get_member_presence(&self, user_id: Uuid) -> Option<MemberPresence> {
        self.member_presence.lock().unwrap().get(&user_id).cloned()
    }

    /// Get all member presences
    pub fn get_all_presences(&self) -> Vec<MemberPresence> {
        self.member_presence.lock().unwrap().values().cloned().collect()
    }

    /// Clear presence for a user (e.g., when they leave)
    pub fn clear_member_presence(&self, user_id: Uuid) {
        self.member_presence.lock().unwrap().remove(&user_id);
    }

    /// Clear all presence data (e.g., when switching halls)
    pub fn clear_all_presence(&self) {
        self.member_presence.lock().unwrap().clear();
    }

    /// Record local activity (resets idle timer)
    pub fn record_local_activity(&self) {
        *self.last_local_activity.lock().unwrap() = Instant::now();
        // Also reset to Active if was Idle
        let mut presence = self.local_presence.lock().unwrap();
        if *presence == PresenceStatus::Idle {
            *presence = PresenceStatus::Active;
        }
    }

    /// Check if local user is idle (no activity for 60s)
    pub fn check_idle(&self) -> bool {
        const IDLE_THRESHOLD_SECS: u64 = 60;
        let last = self.last_local_activity.lock().unwrap();
        last.elapsed().as_secs() >= IDLE_THRESHOLD_SECS
    }

    /// Set local presence status
    pub fn set_local_presence(&self, presence: PresenceStatus) {
        *self.local_presence.lock().unwrap() = presence;
    }

    /// Get local presence status
    pub fn get_local_presence(&self) -> PresenceStatus {
        *self.local_presence.lock().unwrap()
    }

    /// Set local current tool
    pub fn set_local_tool(&self, tool: CurrentTool) {
        *self.local_current_tool.lock().unwrap() = tool;
    }

    /// Get local current tool
    pub fn get_local_tool(&self) -> CurrentTool {
        *self.local_current_tool.lock().unwrap()
    }

    /// Check if presence has changed and needs broadcast
    /// Returns (should_broadcast, new_presence) if changed
    pub fn check_presence_change(&self) -> Option<PresenceStatus> {
        let is_idle = self.check_idle();
        let current = self.get_local_presence();

        // Only transition Active -> Idle automatically
        // Away is set explicitly (e.g., window blur)
        if is_idle && current == PresenceStatus::Active {
            self.set_local_presence(PresenceStatus::Idle);
            return Some(PresenceStatus::Idle);
        }

        None
    }

    // ===== Preferences (K4) =====

    /// Save last hall ID for auto-enter
    pub fn save_last_hall(&self, hall_id: Uuid) {
        if let Some(user_id) = self.current_user_id() {
            let db = self.db.lock().unwrap();
            let _ = db.preferences().set_last_hall(user_id, hall_id);
        }
    }

    /// Get last hall ID for auto-enter
    pub fn get_last_hall(&self) -> Option<Uuid> {
        let user_id = self.current_user_id()?;
        let db = self.db.lock().unwrap();
        db.preferences().get_last_hall(user_id).ok().flatten()
    }

    // ===== Desk Status (Privacy-first presence) =====

    /// Set desk status when user launches a tool from Exom
    /// This is VOLUNTARY - only set by explicit user action, never inferred
    pub fn set_desk_status(&self, desk_label: &str) {
        let user_id = match self.current_user_id() {
            Some(id) => id,
            None => return,
        };
        let hall_id = self.current_hall_id();

        let db = self.db.lock().unwrap();
        if let Err(e) = db.desk_status().set_desk(user_id, desk_label, hall_id) {
            tracing::warn!(error = %e, "Failed to set desk status");
        } else {
            tracing::debug!(
                user_id = %user_id,
                desk_label = %desk_label,
                "Desk status set"
            );
        }
    }

    /// Clear desk status (when tool exits or user manually clears)
    pub fn clear_desk_status(&self) {
        let user_id = match self.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let db = self.db.lock().unwrap();
        if let Err(e) = db.desk_status().clear_desk(user_id) {
            tracing::warn!(error = %e, "Failed to clear desk status");
        } else {
            tracing::debug!(user_id = %user_id, "Desk status cleared");
        }
    }

    /// Get current desk status for a user
    pub fn get_desk_status(&self, user_id: Uuid) -> Option<String> {
        let db = self.db.lock().unwrap();
        db.desk_status()
            .get_desk(user_id)
            .ok()
            .flatten()
            .map(|d| d.desk_label)
    }

    /// Check if viewer can see target user's desk status
    /// Visibility rules:
    /// - Same hall: visible
    /// - Mutual associates: visible
    /// - Otherwise: hidden
    pub fn can_view_desk_status(&self, viewer_id: Uuid, target_id: Uuid) -> bool {
        // Same user always visible
        if viewer_id == target_id {
            return true;
        }

        let db = self.db.lock().unwrap();

        // Check if mutual associates
        if db.associates().is_associate(viewer_id, target_id).unwrap_or(false) {
            return true;
        }

        // Check if in same hall (caller should check this separately for performance)
        // For now, this method assumes the caller has already filtered by hall membership
        // and is asking specifically about associate visibility
        false
    }

    /// Get formatted desk status for display (with visibility check)
    /// Returns " — at the {desk} desk" or empty string if not visible/not set
    pub fn get_visible_desk_status(&self, viewer_id: Uuid, target_id: Uuid, same_hall: bool) -> String {
        // Visibility check: same hall OR mutual associates
        if !same_hall && !self.can_view_desk_status(viewer_id, target_id) {
            return String::new();
        }

        // Get desk status
        match self.get_desk_status(target_id) {
            Some(desk_label) => format!(" — at the {} desk", desk_label),
            None => String::new(),
        }
    }
}

/// Format elapsed duration as activity hint
fn format_activity_hint(elapsed: std::time::Duration) -> String {
    let secs = elapsed.as_secs();
    if secs < 10 {
        "Active".to_string()
    } else if secs < 60 {
        format!("{}s", secs)
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else if secs < 86400 {
        format!("{}h", secs / 3600)
    } else {
        format!("{}d", secs / 86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_format_activity_hint() {
        // <10s => "Active"
        assert_eq!(format_activity_hint(Duration::from_secs(0)), "Active");
        assert_eq!(format_activity_hint(Duration::from_secs(5)), "Active");
        assert_eq!(format_activity_hint(Duration::from_secs(9)), "Active");

        // 10s-59s => "Xs"
        assert_eq!(format_activity_hint(Duration::from_secs(10)), "10s");
        assert_eq!(format_activity_hint(Duration::from_secs(30)), "30s");
        assert_eq!(format_activity_hint(Duration::from_secs(59)), "59s");

        // 1m-59m => "Xm"
        assert_eq!(format_activity_hint(Duration::from_secs(60)), "1m");
        assert_eq!(format_activity_hint(Duration::from_secs(90)), "1m"); // rounds down
        assert_eq!(format_activity_hint(Duration::from_secs(3599)), "59m");

        // 1h-23h => "Xh"
        assert_eq!(format_activity_hint(Duration::from_secs(3600)), "1h");
        assert_eq!(format_activity_hint(Duration::from_secs(7200)), "2h");
        assert_eq!(format_activity_hint(Duration::from_secs(86399)), "23h");

        // >=1d => "Xd"
        assert_eq!(format_activity_hint(Duration::from_secs(86400)), "1d");
        assert_eq!(format_activity_hint(Duration::from_secs(172800)), "2d");
    }
}
