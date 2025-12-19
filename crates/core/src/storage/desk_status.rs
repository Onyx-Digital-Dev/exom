//! Desk Status storage layer
//!
//! Voluntary desk status tracking. Set ONLY when user launches a tool from Exom.
//! Never inferred from OS focus, running processes, or window titles.
//!
//! Visibility rules:
//! - Visible to everyone in the CURRENT HALL
//! - Visible to user's GLOBAL ASSOCIATES
//! - Visible to nobody else

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;

/// A user's current desk status
#[derive(Debug, Clone)]
pub struct DeskStatus {
    pub user_id: Uuid,
    /// The desk label (e.g., "Kitty", "VS Code", "mpv")
    pub desk_label: String,
    /// When this desk was set
    pub set_at: DateTime<Utc>,
    /// Optional: which hall the tool was launched from (context only, not for visibility)
    pub hall_id: Option<Uuid>,
}

/// Desk status storage operations
pub struct DeskStatusStore<'a> {
    conn: &'a Connection,
}

impl<'a> DeskStatusStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Set desk status for a user (called when tool is launched from Exom)
    pub fn set_desk(&self, user_id: Uuid, desk_label: &str, hall_id: Option<Uuid>) -> Result<()> {
        let now = Utc::now();

        self.conn.execute(
            "INSERT INTO desk_status (user_id, desk_label, set_at, hall_id)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id) DO UPDATE SET
                desk_label = excluded.desk_label,
                set_at = excluded.set_at,
                hall_id = excluded.hall_id",
            params![
                user_id.to_string(),
                desk_label,
                now.to_rfc3339(),
                hall_id.map(|id| id.to_string()),
            ],
        )?;

        Ok(())
    }

    /// Clear desk status for a user (called when tool exits or user manually clears)
    pub fn clear_desk(&self, user_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM desk_status WHERE user_id = ?1",
            params![user_id.to_string()],
        )?;

        Ok(())
    }

    /// Get desk status for a user (if set)
    pub fn get_desk(&self, user_id: Uuid) -> Result<Option<DeskStatus>> {
        let result = self.conn.query_row(
            "SELECT user_id, desk_label, set_at, hall_id FROM desk_status WHERE user_id = ?1",
            params![user_id.to_string()],
            |row| {
                let user_id_str: String = row.get(0)?;
                let desk_label: String = row.get(1)?;
                let set_at_str: String = row.get(2)?;
                let hall_id_str: Option<String> = row.get(3)?;

                Ok(DeskStatus {
                    user_id: Uuid::parse_str(&user_id_str).unwrap_or_default(),
                    desk_label,
                    set_at: DateTime::parse_from_rfc3339(&set_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    hall_id: hall_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
                })
            },
        );

        match result {
            Ok(status) => Ok(Some(status)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Get desk status for multiple users (batch query for member list)
    pub fn get_desks_for_users(&self, user_ids: &[Uuid]) -> Result<Vec<DeskStatus>> {
        if user_ids.is_empty() {
            return Ok(Vec::new());
        }

        // Build placeholders for IN clause
        let placeholders: Vec<String> = user_ids.iter().map(|_| "?".to_string()).collect();
        let sql = format!(
            "SELECT user_id, desk_label, set_at, hall_id FROM desk_status WHERE user_id IN ({})",
            placeholders.join(", ")
        );

        let mut stmt = self.conn.prepare(&sql)?;

        let params: Vec<String> = user_ids.iter().map(|id| id.to_string()).collect();
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|s| s as &dyn rusqlite::ToSql).collect();

        let statuses = stmt
            .query_map(param_refs.as_slice(), |row| {
                let user_id_str: String = row.get(0)?;
                let desk_label: String = row.get(1)?;
                let set_at_str: String = row.get(2)?;
                let hall_id_str: Option<String> = row.get(3)?;

                Ok(DeskStatus {
                    user_id: Uuid::parse_str(&user_id_str).unwrap_or_default(),
                    desk_label,
                    set_at: DateTime::parse_from_rfc3339(&set_at_str)
                        .map(|dt| dt.with_timezone(&Utc))
                        .unwrap_or_else(|_| Utc::now()),
                    hall_id: hall_id_str.and_then(|s| Uuid::parse_str(&s).ok()),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(statuses)
    }

    /// Format desk status for display: " — at the {desk} desk"
    pub fn format_desk_status(desk_label: &str) -> String {
        format!(" — at the {} desk", desk_label)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;

    fn setup_test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        db.conn
            .execute(
                "INSERT INTO users (id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    "11111111-1111-1111-1111-111111111111",
                    "alice",
                    "hash",
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
        db
    }

    #[test]
    fn test_set_and_get_desk() {
        let db = setup_test_db();
        let store = db.desk_status();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();

        // Initially no desk
        assert!(store.get_desk(alice).unwrap().is_none());

        // Set desk (no hall_id to avoid FK constraint in test)
        store.set_desk(alice, "Kitty", None).unwrap();

        let status = store.get_desk(alice).unwrap().unwrap();
        assert_eq!(status.desk_label, "Kitty");
        assert_eq!(status.hall_id, None);
    }

    #[test]
    fn test_clear_desk() {
        let db = setup_test_db();
        let store = db.desk_status();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();

        store.set_desk(alice, "VS Code", None).unwrap();
        assert!(store.get_desk(alice).unwrap().is_some());

        store.clear_desk(alice).unwrap();
        assert!(store.get_desk(alice).unwrap().is_none());
    }

    #[test]
    fn test_update_desk() {
        let db = setup_test_db();
        let store = db.desk_status();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();

        store.set_desk(alice, "Kitty", None).unwrap();
        store.set_desk(alice, "VS Code", None).unwrap();

        let status = store.get_desk(alice).unwrap().unwrap();
        assert_eq!(status.desk_label, "VS Code");
    }

    #[test]
    fn test_format_desk_status() {
        assert_eq!(
            DeskStatusStore::format_desk_status("Kitty"),
            " — at the Kitty desk"
        );
        assert_eq!(
            DeskStatusStore::format_desk_status("VS Code"),
            " — at the VS Code desk"
        );
    }
}
