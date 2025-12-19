//! Pinned launchers storage
//!
//! Stores per-hall quick-launch buttons for external tools.
//! Tools open in NEW WINDOWS - there is no embedding.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::error::Result;

/// A pinned launcher entry
#[derive(Debug, Clone)]
pub struct PinnedLauncher {
    pub id: Uuid,
    pub hall_id: Uuid,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub icon: Option<String>,
    pub position: i32,
    pub created_at: DateTime<Utc>,
}

impl PinnedLauncher {
    /// Create a new pinned launcher
    pub fn new(hall_id: Uuid, name: String, command: String, args: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            hall_id,
            name,
            command,
            args,
            icon: None,
            position: 0,
            created_at: Utc::now(),
        }
    }
}

/// Pinned launchers store
pub struct LauncherStore<'a> {
    conn: &'a Connection,
}

impl<'a> LauncherStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Add a pinned launcher
    pub fn add(&self, launcher: &PinnedLauncher) -> Result<()> {
        let args_json = serde_json::to_string(&launcher.args).unwrap_or_else(|_| "[]".to_string());

        self.conn.execute(
            "INSERT INTO pinned_launchers (id, hall_id, name, command, args, icon, position, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                launcher.id.to_string(),
                launcher.hall_id.to_string(),
                launcher.name,
                launcher.command,
                args_json,
                launcher.icon,
                launcher.position,
                launcher.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Remove a pinned launcher
    pub fn remove(&self, launcher_id: Uuid) -> Result<bool> {
        let rows = self.conn.execute(
            "DELETE FROM pinned_launchers WHERE id = ?1",
            params![launcher_id.to_string()],
        )?;
        Ok(rows > 0)
    }

    /// Get a launcher by ID
    pub fn get(&self, launcher_id: Uuid) -> Result<Option<PinnedLauncher>> {
        let result = self
            .conn
            .query_row(
                "SELECT id, hall_id, name, command, args, icon, position, created_at
                 FROM pinned_launchers WHERE id = ?1",
                params![launcher_id.to_string()],
                |row| {
                    let args_json: String = row.get(4)?;
                    let args: Vec<String> =
                        serde_json::from_str(&args_json).unwrap_or_default();

                    Ok(PinnedLauncher {
                        id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                        hall_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                        name: row.get(2)?,
                        command: row.get(3)?,
                        args,
                        icon: row.get(5)?,
                        position: row.get(6)?,
                        created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                            .unwrap()
                            .with_timezone(&Utc),
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// List launchers for a hall (ordered by position)
    pub fn list_for_hall(&self, hall_id: Uuid) -> Result<Vec<PinnedLauncher>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hall_id, name, command, args, icon, position, created_at
             FROM pinned_launchers WHERE hall_id = ?1 ORDER BY position, created_at",
        )?;

        let rows = stmt.query_map(params![hall_id.to_string()], |row| {
            let args_json: String = row.get(4)?;
            let args: Vec<String> = serde_json::from_str(&args_json).unwrap_or_default();

            Ok(PinnedLauncher {
                id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                hall_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                name: row.get(2)?,
                command: row.get(3)?,
                args,
                icon: row.get(5)?,
                position: row.get(6)?,
                created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(7)?)
                    .unwrap()
                    .with_timezone(&Utc),
            })
        })?;

        let mut launchers = Vec::new();
        for row in rows {
            launchers.push(row?);
        }
        Ok(launchers)
    }

    /// Update launcher position
    pub fn set_position(&self, launcher_id: Uuid, position: i32) -> Result<()> {
        self.conn.execute(
            "UPDATE pinned_launchers SET position = ?2 WHERE id = ?1",
            params![launcher_id.to_string(), position],
        )?;
        Ok(())
    }

    /// Update launcher details
    pub fn update(
        &self,
        launcher_id: Uuid,
        name: Option<&str>,
        command: Option<&str>,
        args: Option<&[String]>,
        icon: Option<Option<&str>>,
    ) -> Result<()> {
        if let Some(name) = name {
            self.conn.execute(
                "UPDATE pinned_launchers SET name = ?2 WHERE id = ?1",
                params![launcher_id.to_string(), name],
            )?;
        }
        if let Some(command) = command {
            self.conn.execute(
                "UPDATE pinned_launchers SET command = ?2 WHERE id = ?1",
                params![launcher_id.to_string(), command],
            )?;
        }
        if let Some(args) = args {
            let args_json = serde_json::to_string(args).unwrap_or_else(|_| "[]".to_string());
            self.conn.execute(
                "UPDATE pinned_launchers SET args = ?2 WHERE id = ?1",
                params![launcher_id.to_string(), args_json],
            )?;
        }
        if let Some(icon) = icon {
            self.conn.execute(
                "UPDATE pinned_launchers SET icon = ?2 WHERE id = ?1",
                params![launcher_id.to_string(), icon],
            )?;
        }
        Ok(())
    }

    /// Delete all launchers for a hall
    pub fn delete_for_hall(&self, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM pinned_launchers WHERE hall_id = ?1",
            params![hall_id.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Hall, User};
    use crate::storage::Database;

    fn create_test_hall(db: &Database) -> Uuid {
        let user_id = Uuid::new_v4();
        let user = User::new("testuser".to_string(), "password_hash".to_string());
        db.users().create(&User { id: user_id, ..user }).unwrap();

        let hall_id = Uuid::new_v4();
        let hall = Hall::new("Test Hall".to_string(), user_id);
        db.halls().create(&Hall { id: hall_id, ..hall }).unwrap();

        hall_id
    }

    #[test]
    fn test_add_and_list_launchers() {
        let db = Database::open_in_memory().unwrap();
        let hall_id = create_test_hall(&db);
        let store = db.launchers();

        // Add launchers
        let launcher1 = PinnedLauncher::new(
            hall_id,
            "Terminal".to_string(),
            "kitty".to_string(),
            vec![],
        );
        let launcher2 = PinnedLauncher::new(
            hall_id,
            "Editor".to_string(),
            "code".to_string(),
            vec![".".to_string()],
        );

        store.add(&launcher1).unwrap();
        store.add(&launcher2).unwrap();

        // List
        let launchers = store.list_for_hall(hall_id).unwrap();
        assert_eq!(launchers.len(), 2);
    }

    #[test]
    fn test_remove_launcher() {
        let db = Database::open_in_memory().unwrap();
        let hall_id = create_test_hall(&db);
        let store = db.launchers();

        let launcher = PinnedLauncher::new(
            hall_id,
            "Test".to_string(),
            "echo".to_string(),
            vec!["hello".to_string()],
        );
        let id = launcher.id;
        store.add(&launcher).unwrap();

        assert!(store.get(id).unwrap().is_some());
        store.remove(id).unwrap();
        assert!(store.get(id).unwrap().is_none());
    }
}
