//! Archive configuration storage for Archivist bot

use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use crate::error::Result;
use crate::storage::parse::parse_datetime;

/// Archive window configuration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveWindow {
    /// Last 12 hours
    Hours12,
    /// Last 24 hours
    Hours24,
    /// Since last archive run
    SinceLastRun,
}

impl ArchiveWindow {
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "12h" => Some(Self::Hours12),
            "24h" => Some(Self::Hours24),
            "since_last_run" | "since-last-run" => Some(Self::SinceLastRun),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Hours12 => "12h",
            Self::Hours24 => "24h",
            Self::SinceLastRun => "since_last_run",
        }
    }
}

/// Archive output configuration
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchiveOutput {
    /// Write to hall chest archives folder
    Chest,
    /// Write to user's subfolder under archives
    ChestUser(String),
}

impl ArchiveOutput {
    pub fn from_str(s: &str) -> Option<Self> {
        if s == "chest" {
            Some(Self::Chest)
        } else if let Some(username) = s.strip_prefix("chest:") {
            Some(Self::ChestUser(username.to_string()))
        } else {
            None
        }
    }

    pub fn as_str(&self) -> String {
        match self {
            Self::Chest => "chest".to_string(),
            Self::ChestUser(username) => format!("chest:{}", username),
        }
    }
}

/// Archive configuration for a hall
#[derive(Debug, Clone)]
pub struct ArchiveConfig {
    pub hall_id: Uuid,
    pub enabled: bool,
    /// Time as HHMM (e.g., 2200 for 10 PM)
    pub archive_time: u16,
    pub archive_window: ArchiveWindow,
    pub archive_output: ArchiveOutput,
    pub last_run_at: Option<DateTime<Utc>>,
}

impl Default for ArchiveConfig {
    fn default() -> Self {
        Self {
            hall_id: Uuid::nil(),
            enabled: false,
            archive_time: 2200,
            archive_window: ArchiveWindow::Hours24,
            archive_output: ArchiveOutput::Chest,
            last_run_at: None,
        }
    }
}

/// Archive configuration storage operations
pub struct ArchiveConfigStore<'a> {
    conn: &'a rusqlite::Connection,
}

impl<'a> ArchiveConfigStore<'a> {
    pub fn new(conn: &'a rusqlite::Connection) -> Self {
        Self { conn }
    }

    /// Get archive config for a hall
    pub fn get(&self, hall_id: Uuid) -> Result<Option<ArchiveConfig>> {
        let mut stmt = self.conn.prepare(
            "SELECT hall_id, enabled, archive_time, archive_window, archive_output, last_run_at
             FROM archive_config WHERE hall_id = ?1",
        )?;

        let result = stmt.query_row(params![hall_id.to_string()], |row| {
            let hall_id_str: String = row.get(0)?;
            let enabled: i32 = row.get(1)?;
            let archive_time: i32 = row.get(2)?;
            let archive_window_str: String = row.get(3)?;
            let archive_output_str: String = row.get(4)?;
            let last_run_str: Option<String> = row.get(5)?;

            Ok((
                hall_id_str,
                enabled,
                archive_time,
                archive_window_str,
                archive_output_str,
                last_run_str,
            ))
        });

        match result {
            Ok((hall_id_str, enabled, archive_time, window_str, output_str, last_run_str)) => {
                let last_run_at = last_run_str
                    .as_ref()
                    .and_then(|s| parse_datetime(s).ok());

                Ok(Some(ArchiveConfig {
                    hall_id: Uuid::parse_str(&hall_id_str).unwrap_or(hall_id),
                    enabled: enabled != 0,
                    archive_time: archive_time as u16,
                    archive_window: ArchiveWindow::from_str(&window_str)
                        .unwrap_or(ArchiveWindow::Hours24),
                    archive_output: ArchiveOutput::from_str(&output_str)
                        .unwrap_or(ArchiveOutput::Chest),
                    last_run_at,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get or create default config for a hall
    pub fn get_or_default(&self, hall_id: Uuid) -> Result<ArchiveConfig> {
        match self.get(hall_id)? {
            Some(config) => Ok(config),
            None => {
                let config = ArchiveConfig {
                    hall_id,
                    ..Default::default()
                };
                self.save(&config)?;
                Ok(config)
            }
        }
    }

    /// Save archive config
    pub fn save(&self, config: &ArchiveConfig) -> Result<()> {
        let last_run_str = config.last_run_at.map(|dt| dt.to_rfc3339());

        self.conn.execute(
            "INSERT INTO archive_config (hall_id, enabled, archive_time, archive_window, archive_output, last_run_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(hall_id) DO UPDATE SET
                enabled = ?2,
                archive_time = ?3,
                archive_window = ?4,
                archive_output = ?5,
                last_run_at = ?6",
            params![
                config.hall_id.to_string(),
                config.enabled as i32,
                config.archive_time as i32,
                config.archive_window.as_str(),
                config.archive_output.as_str(),
                last_run_str,
            ],
        )?;
        Ok(())
    }

    /// Update last run timestamp
    pub fn update_last_run(&self, hall_id: Uuid, timestamp: DateTime<Utc>) -> Result<()> {
        self.conn.execute(
            "UPDATE archive_config SET last_run_at = ?2 WHERE hall_id = ?1",
            params![hall_id.to_string(), timestamp.to_rfc3339()],
        )?;
        Ok(())
    }

    /// Enable archiving for a hall
    pub fn set_enabled(&self, hall_id: Uuid, enabled: bool) -> Result<()> {
        // Ensure config exists
        let _ = self.get_or_default(hall_id)?;
        self.conn.execute(
            "UPDATE archive_config SET enabled = ?2 WHERE hall_id = ?1",
            params![hall_id.to_string(), enabled as i32],
        )?;
        Ok(())
    }

    /// Set archive time (HHMM format)
    pub fn set_time(&self, hall_id: Uuid, time: u16) -> Result<()> {
        let _ = self.get_or_default(hall_id)?;
        self.conn.execute(
            "UPDATE archive_config SET archive_time = ?2 WHERE hall_id = ?1",
            params![hall_id.to_string(), time as i32],
        )?;
        Ok(())
    }

    /// Set archive window
    pub fn set_window(&self, hall_id: Uuid, window: ArchiveWindow) -> Result<()> {
        let _ = self.get_or_default(hall_id)?;
        self.conn.execute(
            "UPDATE archive_config SET archive_window = ?2 WHERE hall_id = ?1",
            params![hall_id.to_string(), window.as_str()],
        )?;
        Ok(())
    }

    /// Set archive output
    pub fn set_output(&self, hall_id: Uuid, output: &ArchiveOutput) -> Result<()> {
        let _ = self.get_or_default(hall_id)?;
        self.conn.execute(
            "UPDATE archive_config SET archive_output = ?2 WHERE hall_id = ?1",
            params![hall_id.to_string(), output.as_str()],
        )?;
        Ok(())
    }

    /// Get all enabled halls
    pub fn get_enabled_halls(&self) -> Result<Vec<Uuid>> {
        let mut stmt = self
            .conn
            .prepare("SELECT hall_id FROM archive_config WHERE enabled = 1")?;
        let rows = stmt.query_map([], |row| {
            let hall_id_str: String = row.get(0)?;
            Ok(hall_id_str)
        })?;

        let mut halls = Vec::new();
        for row in rows {
            if let Ok(hall_id_str) = row {
                if let Ok(uuid) = Uuid::parse_str(&hall_id_str) {
                    halls.push(uuid);
                }
            }
        }
        Ok(halls)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Hall, User};
    use crate::storage::Database;
    use tempfile::tempdir;

    fn create_test_user(db: &Database) -> Uuid {
        let user_id = Uuid::new_v4();
        let user = User {
            id: user_id,
            username: format!("testuser_{}", user_id),
            password_hash: "hash".to_string(),
            created_at: Utc::now(),
            last_login: None,
        };
        db.users().create(&user).unwrap();
        user_id
    }

    fn create_test_hall(db: &Database, owner_id: Uuid) -> Uuid {
        let hall_id = Uuid::new_v4();
        let hall = Hall {
            id: hall_id,
            name: "Test Hall".to_string(),
            description: None,
            owner_id,
            created_at: Utc::now(),
            active_parlor: None,
            current_host_id: None,
            election_epoch: 0,
        };
        db.halls().create(&hall).unwrap();
        hall_id
    }

    #[test]
    fn test_archive_config_default() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).unwrap();

        let user_id = create_test_user(&db);
        let hall_id = create_test_hall(&db, user_id);

        let config = db.archive_config().get_or_default(hall_id).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.archive_time, 2200);
        assert_eq!(config.archive_window, ArchiveWindow::Hours24);
        assert_eq!(config.archive_output, ArchiveOutput::Chest);
    }

    #[test]
    fn test_archive_config_enable() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).unwrap();

        let user_id = create_test_user(&db);
        let hall_id = create_test_hall(&db, user_id);

        db.archive_config().set_enabled(hall_id, true).unwrap();

        let config = db.archive_config().get(hall_id).unwrap().unwrap();
        assert!(config.enabled);
    }

    #[test]
    fn test_archive_window_parsing() {
        assert_eq!(ArchiveWindow::from_str("12h"), Some(ArchiveWindow::Hours12));
        assert_eq!(ArchiveWindow::from_str("24h"), Some(ArchiveWindow::Hours24));
        assert_eq!(
            ArchiveWindow::from_str("since_last_run"),
            Some(ArchiveWindow::SinceLastRun)
        );
        assert_eq!(ArchiveWindow::from_str("invalid"), None);
    }

    #[test]
    fn test_archive_output_parsing() {
        assert_eq!(ArchiveOutput::from_str("chest"), Some(ArchiveOutput::Chest));
        assert_eq!(
            ArchiveOutput::from_str("chest:alice"),
            Some(ArchiveOutput::ChestUser("alice".to_string()))
        );
        assert_eq!(ArchiveOutput::from_str("invalid"), None);
    }
}
