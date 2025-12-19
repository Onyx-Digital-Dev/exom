//! Application state management

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::{DateTime, Utc};
use directories::ProjectDirs;
use exom_core::{Database, Error, HallChest, Result};
use uuid::Uuid;

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

    /// Update known members and return (joined, left) usernames
    pub fn update_known_members(
        &self,
        new_members: Vec<TrackedMember>,
    ) -> (Vec<String>, Vec<String>) {
        let mut known = self.known_members.lock().unwrap();
        let my_user_id = self.current_user_id();

        // Find who joined (in new but not in known)
        let joined: Vec<String> = new_members
            .iter()
            .filter(|m| {
                !known.iter().any(|k| k.user_id == m.user_id) && my_user_id != Some(m.user_id)
            })
            .map(|m| m.username.clone())
            .collect();

        // Find who left (in known but not in new)
        let left: Vec<String> = known
            .iter()
            .filter(|k| {
                !new_members.iter().any(|m| m.user_id == k.user_id) && my_user_id != Some(k.user_id)
            })
            .map(|k| k.username.clone())
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
