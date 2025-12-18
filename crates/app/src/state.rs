//! Application state management

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

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
                !known.iter().any(|k| k.user_id == m.user_id)
                    && my_user_id.map_or(true, |uid| uid != m.user_id)
            })
            .map(|m| m.username.clone())
            .collect();

        // Find who left (in known but not in new)
        let left: Vec<String> = known
            .iter()
            .filter(|k| {
                !new_members.iter().any(|m| m.user_id == k.user_id)
                    && my_user_id.map_or(true, |uid| uid != k.user_id)
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
}
