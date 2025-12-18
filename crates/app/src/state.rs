//! Application state management

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use directories::ProjectDirs;
use exom_core::{Database, HallChest, Result, Error};
use uuid::Uuid;

/// Main application state
pub struct AppState {
    pub db: Arc<Mutex<Database>>,
    pub chest: Arc<Mutex<HallChest>>,
    pub current_user_id: Arc<Mutex<Option<Uuid>>>,
    pub current_session_id: Arc<Mutex<Option<Uuid>>>,
    pub current_hall_id: Arc<Mutex<Option<Uuid>>>,
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
        })
    }

    fn data_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "onyx", "exom")
            .ok_or_else(|| Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine data directory",
            )))?;

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
}
