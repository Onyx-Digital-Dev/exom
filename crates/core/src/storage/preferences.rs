//! User preferences persistence
//!
//! Stores per-user preferences like last hall for auto-enter.

use chrono::Utc;
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;

/// User preferences
#[derive(Debug, Clone)]
pub struct UserPreferences {
    pub user_id: Uuid,
    pub last_hall_id: Option<Uuid>,
}

/// Preferences store
pub struct PreferencesStore<'a> {
    conn: &'a Connection,
}

impl<'a> PreferencesStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Save user preferences
    pub fn save(&self, prefs: &UserPreferences) -> Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO user_preferences (user_id, last_hall_id, updated_at)
             VALUES (?1, ?2, ?3)",
            params![
                prefs.user_id.to_string(),
                prefs.last_hall_id.map(|id| id.to_string()),
                Utc::now().to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Load user preferences
    pub fn load(&self, user_id: Uuid) -> Result<Option<UserPreferences>> {
        let result = self.conn.query_row(
            "SELECT last_hall_id FROM user_preferences WHERE user_id = ?1",
            params![user_id.to_string()],
            |row| {
                let last_hall_str: Option<String> = row.get(0)?;
                Ok(last_hall_str)
            },
        );

        match result {
            Ok(last_hall_str) => {
                let last_hall_id = last_hall_str.and_then(|s| Uuid::parse_str(&s).ok());
                Ok(Some(UserPreferences {
                    user_id,
                    last_hall_id,
                }))
            }
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Set last hall for a user
    pub fn set_last_hall(&self, user_id: Uuid, hall_id: Uuid) -> Result<()> {
        self.save(&UserPreferences {
            user_id,
            last_hall_id: Some(hall_id),
        })
    }

    /// Get last hall for a user
    pub fn get_last_hall(&self, user_id: Uuid) -> Result<Option<Uuid>> {
        Ok(self.load(user_id)?.and_then(|p| p.last_hall_id))
    }

    /// Clear last hall (e.g., when hall is deleted)
    #[allow(dead_code)]
    pub fn clear_last_hall(&self, user_id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE user_preferences SET last_hall_id = NULL, updated_at = ?1 WHERE user_id = ?2",
            params![Utc::now().to_rfc3339(), user_id.to_string()],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::User;
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

    #[test]
    fn test_preferences_save_load() {
        let db = Database::open_in_memory().unwrap();
        let user_id = create_test_user(&db);
        let hall_id = Uuid::new_v4(); // hall_id doesn't need to exist

        let store = PreferencesStore::new(&db.conn);
        store.set_last_hall(user_id, hall_id).unwrap();

        let last = store.get_last_hall(user_id).unwrap();
        assert_eq!(last, Some(hall_id));
    }

    #[test]
    fn test_preferences_not_found() {
        let db = Database::open_in_memory().unwrap();
        let store = PreferencesStore::new(&db.conn);

        let result = store.get_last_hall(Uuid::new_v4()).unwrap();
        assert!(result.is_none());
    }
}
