//! Hall Chest - Local folder management for Hall files
//!
//! When a user joins a Hall as Agent or higher, local folders are created.
//! Sync is NOT implemented yet, but the interface is designed for future sync.

use std::fs;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::models::HallRole;

/// Manages local Hall Chest folders
pub struct HallChest {
    base_path: PathBuf,
}

/// A file entry in the Hall Chest
#[derive(Debug, Clone)]
pub struct ChestEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_directory: bool,
    pub size_bytes: u64,
    /// Sync status - always "local_only" for now
    pub sync_status: SyncStatus,
}

/// Sync status for chest files
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// File exists locally only (sync not implemented)
    LocalOnly,
    /// File is synced (future)
    Synced,
    /// File is being uploaded (future)
    Uploading,
    /// File is being downloaded (future)
    Downloading,
    /// Sync conflict (future)
    Conflict,
}

impl SyncStatus {
    pub fn display(&self) -> &'static str {
        match self {
            SyncStatus::LocalOnly => "Local only - sync coming later",
            SyncStatus::Synced => "Synced",
            SyncStatus::Uploading => "Uploading...",
            SyncStatus::Downloading => "Downloading...",
            SyncStatus::Conflict => "Conflict",
        }
    }
}

impl HallChest {
    /// Create a new HallChest manager
    pub fn new() -> Result<Self> {
        let base_path = Self::default_base_path()?;
        fs::create_dir_all(&base_path)?;
        Ok(Self { base_path })
    }

    /// Create with custom base path (for testing)
    pub fn with_base_path(base_path: PathBuf) -> Result<Self> {
        fs::create_dir_all(&base_path)?;
        Ok(Self { base_path })
    }

    /// Get default base path for Hall Chests
    fn default_base_path() -> Result<PathBuf> {
        let dirs = ProjectDirs::from("dev", "onyx", "exom").ok_or_else(|| {
            Error::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Could not determine config directory",
            ))
        })?;

        Ok(dirs.data_dir().join("chests"))
    }

    /// Initialize chest folders for a Hall
    /// Called when user joins as Agent or higher
    pub fn init_hall_chest(
        &self,
        hall_id: Uuid,
        hall_name: &str,
        role: HallRole,
    ) -> Result<PathBuf> {
        // Fellows don't get chest access
        if role < HallRole::HallAgent {
            return Err(Error::PermissionDenied(
                "Fellows do not have Hall Chest access".into(),
            ));
        }

        let hall_path = self.hall_path(hall_id);

        // Create main Hall folder
        fs::create_dir_all(&hall_path)?;

        // Create standard subfolders
        let subfolders = ["shared", "personal", "downloads"];
        for folder in &subfolders {
            fs::create_dir_all(hall_path.join(folder))?;
        }

        // Create a metadata file
        let meta_path = hall_path.join(".hall_meta.json");
        if !meta_path.exists() {
            let meta = serde_json::json!({
                "hall_id": hall_id.to_string(),
                "hall_name": hall_name,
                "created_at": chrono::Utc::now().to_rfc3339(),
                "sync_enabled": false,
            });
            fs::write(&meta_path, serde_json::to_string_pretty(&meta)?)?;
        }

        Ok(hall_path)
    }

    /// Get path for a Hall's chest
    pub fn hall_path(&self, hall_id: Uuid) -> PathBuf {
        self.base_path.join(hall_id.to_string())
    }

    /// Check if Hall chest exists
    pub fn chest_exists(&self, hall_id: Uuid) -> bool {
        self.hall_path(hall_id).exists()
    }

    /// List files in a Hall chest directory
    pub fn list_files(&self, hall_id: Uuid, subpath: Option<&str>) -> Result<Vec<ChestEntry>> {
        let mut path = self.hall_path(hall_id);
        if let Some(sub) = subpath {
            path = path.join(sub);
        }

        if !path.exists() {
            return Ok(Vec::new());
        }

        let mut entries = Vec::new();
        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().to_string();

            // Skip hidden files starting with .
            if name.starts_with('.') {
                continue;
            }

            entries.push(ChestEntry {
                name,
                path: entry.path(),
                is_directory: metadata.is_dir(),
                size_bytes: metadata.len(),
                sync_status: SyncStatus::LocalOnly,
            });
        }

        // Sort: directories first, then by name
        entries.sort_by(|a, b| match (a.is_directory, b.is_directory) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });

        Ok(entries)
    }

    /// Get total size of a Hall chest
    pub fn chest_size(&self, hall_id: Uuid) -> Result<u64> {
        let path = self.hall_path(hall_id);
        if !path.exists() {
            return Ok(0);
        }
        Self::dir_size(&path)
    }

    fn dir_size(path: &Path) -> Result<u64> {
        let mut total = 0;
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if metadata.is_dir() {
                total += Self::dir_size(&entry.path())?;
            } else {
                total += metadata.len();
            }
        }
        Ok(total)
    }

    /// Delete a Hall's chest (called when leaving or Hall is deleted)
    pub fn delete_chest(&self, hall_id: Uuid) -> Result<()> {
        let path = self.hall_path(hall_id);
        if path.exists() {
            fs::remove_dir_all(path)?;
        }
        Ok(())
    }

    /// Get the base path for display
    pub fn base_path(&self) -> &Path {
        &self.base_path
    }
}

impl Default for HallChest {
    fn default() -> Self {
        Self::new().expect("Failed to initialize HallChest")
    }
}

/// Trait for future sync implementation
pub trait ChestSync: Send + Sync {
    /// Start syncing a Hall's chest
    fn start_sync(&mut self, hall_id: Uuid) -> Result<()>;

    /// Stop syncing
    fn stop_sync(&mut self, hall_id: Uuid) -> Result<()>;

    /// Get sync status for a file
    fn get_sync_status(&self, path: &Path) -> SyncStatus;

    /// Force upload a file
    fn upload(&mut self, path: &Path) -> Result<()>;

    /// Force download a file
    fn download(&mut self, remote_path: &str, local_path: &Path) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_init_chest() {
        let dir = tempdir().unwrap();
        let chest = HallChest::with_base_path(dir.path().to_path_buf()).unwrap();

        let hall_id = Uuid::new_v4();
        let path = chest
            .init_hall_chest(hall_id, "Test Hall", HallRole::HallAgent)
            .unwrap();

        assert!(path.exists());
        assert!(path.join("shared").exists());
        assert!(path.join("personal").exists());
        assert!(path.join("downloads").exists());
    }

    #[test]
    fn test_fellow_denied() {
        let dir = tempdir().unwrap();
        let chest = HallChest::with_base_path(dir.path().to_path_buf()).unwrap();

        let hall_id = Uuid::new_v4();
        let result = chest.init_hall_chest(hall_id, "Test Hall", HallRole::HallFellow);

        assert!(result.is_err());
    }
}
