//! Workspace state persistence
//!
//! Saves and restores workspace tabs per hall for session continuity.

use chrono::Utc;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::Result;

/// Persisted tab information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistedTab {
    pub id: String,
    pub tool_type: String, // "Chat", "Terminal", "Files"
    pub title: String,
}

/// Persisted workspace state
#[derive(Debug, Clone)]
pub struct PersistedWorkspace {
    pub hall_id: Uuid,
    pub user_id: Uuid,
    pub tabs: Vec<PersistedTab>,
    pub active_tab_id: Option<String>,
    pub terminal_cwd: Option<String>,
}

/// Workspace state store
pub struct WorkspaceStore<'a> {
    conn: &'a Connection,
}

impl<'a> WorkspaceStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Save workspace state for a hall
    pub fn save(&self, workspace: &PersistedWorkspace) -> Result<()> {
        let tabs_json = serde_json::to_string(&workspace.tabs)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO workspace_state
             (hall_id, user_id, tabs_json, active_tab_id, terminal_cwd, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                workspace.hall_id.to_string(),
                workspace.user_id.to_string(),
                tabs_json,
                workspace.active_tab_id,
                workspace.terminal_cwd,
                Utc::now().to_rfc3339(),
            ],
        )?;

        Ok(())
    }

    /// Load workspace state for a hall
    pub fn load(&self, hall_id: Uuid, user_id: Uuid) -> Result<Option<PersistedWorkspace>> {
        let result = self.conn.query_row(
            "SELECT tabs_json, active_tab_id, terminal_cwd FROM workspace_state
             WHERE hall_id = ?1 AND user_id = ?2",
            params![hall_id.to_string(), user_id.to_string()],
            |row| {
                let tabs_json: String = row.get(0)?;
                let active_tab_id: Option<String> = row.get(1)?;
                let terminal_cwd: Option<String> = row.get(2)?;
                Ok((tabs_json, active_tab_id, terminal_cwd))
            },
        );

        match result {
            Ok((tabs_json, active_tab_id, terminal_cwd)) => {
                let tabs: Vec<PersistedTab> = serde_json::from_str(&tabs_json)?;
                Ok(Some(PersistedWorkspace {
                    hall_id,
                    user_id,
                    tabs,
                    active_tab_id,
                    terminal_cwd,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Delete workspace state for a hall
    #[allow(dead_code)]
    pub fn delete(&self, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM workspace_state WHERE hall_id = ?1",
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
    use chrono::Utc;

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
    fn test_workspace_save_load() {
        let db = Database::open_in_memory().unwrap();
        let user_id = create_test_user(&db);
        let hall_id = create_test_hall(&db, user_id);

        let store = WorkspaceStore::new(&db.conn);
        let workspace = PersistedWorkspace {
            hall_id,
            user_id,
            tabs: vec![
                PersistedTab {
                    id: "tab1".to_string(),
                    tool_type: "Chat".to_string(),
                    title: "Chat".to_string(),
                },
                PersistedTab {
                    id: "tab2".to_string(),
                    tool_type: "Terminal".to_string(),
                    title: "Terminal".to_string(),
                },
            ],
            active_tab_id: Some("tab2".to_string()),
            terminal_cwd: Some("/home/user".to_string()),
        };

        store.save(&workspace).unwrap();

        let loaded = store.load(hall_id, user_id).unwrap().unwrap();
        assert_eq!(loaded.tabs.len(), 2);
        assert_eq!(loaded.active_tab_id, Some("tab2".to_string()));
        assert_eq!(loaded.terminal_cwd, Some("/home/user".to_string()));
    }

    #[test]
    fn test_workspace_not_found() {
        let db = Database::open_in_memory().unwrap();
        let store = WorkspaceStore::new(&db.conn);

        let result = store.load(Uuid::new_v4(), Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }
}
