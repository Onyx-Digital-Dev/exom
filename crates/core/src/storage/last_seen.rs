//! Last seen storage for Town Crier bot

use chrono::{DateTime, Utc};
use rusqlite::params;
use uuid::Uuid;

use crate::error::Result;
use crate::storage::parse::parse_datetime;

/// Last seen record
#[derive(Debug, Clone)]
pub struct LastSeen {
    pub hall_id: Uuid,
    pub user_id: Uuid,
    pub last_seen_at: DateTime<Utc>,
}

/// Last seen storage operations
pub struct LastSeenStore<'a> {
    conn: &'a rusqlite::Connection,
}

impl<'a> LastSeenStore<'a> {
    pub fn new(conn: &'a rusqlite::Connection) -> Self {
        Self { conn }
    }

    /// Get last seen time for a user in a hall
    pub fn get(&self, hall_id: Uuid, user_id: Uuid) -> Result<Option<LastSeen>> {
        let mut stmt = self.conn.prepare(
            "SELECT hall_id, user_id, last_seen_at FROM last_seen WHERE hall_id = ?1 AND user_id = ?2",
        )?;

        let result = stmt.query_row(
            params![hall_id.to_string(), user_id.to_string()],
            |row| {
                let hall_id_str: String = row.get(0)?;
                let user_id_str: String = row.get(1)?;
                let last_seen_str: String = row.get(2)?;
                let last_seen_at = parse_datetime(&last_seen_str)?;
                Ok(LastSeen {
                    hall_id: Uuid::parse_str(&hall_id_str).unwrap_or(hall_id),
                    user_id: Uuid::parse_str(&user_id_str).unwrap_or(user_id),
                    last_seen_at,
                })
            },
        );

        match result {
            Ok(record) => Ok(Some(record)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update last seen time for a user in a hall (upsert)
    pub fn update(&self, hall_id: Uuid, user_id: Uuid) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        self.conn.execute(
            "INSERT INTO last_seen (hall_id, user_id, last_seen_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(hall_id, user_id) DO UPDATE SET last_seen_at = ?3",
            params![hall_id.to_string(), user_id.to_string(), now],
        )?;
        Ok(())
    }

    /// Get duration since last seen (None if never seen)
    pub fn get_duration_since(&self, hall_id: Uuid, user_id: Uuid) -> Result<Option<std::time::Duration>> {
        match self.get(hall_id, user_id)? {
            Some(record) => {
                let now = Utc::now();
                let duration = now.signed_duration_since(record.last_seen_at);
                if duration.num_seconds() > 0 {
                    Ok(Some(std::time::Duration::from_secs(
                        duration.num_seconds() as u64,
                    )))
                } else {
                    Ok(Some(std::time::Duration::ZERO))
                }
            }
            None => Ok(None),
        }
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
    fn test_last_seen_not_found() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).unwrap();

        let user_id = create_test_user(&db);
        let hall_id = create_test_hall(&db, user_id);

        let result = db.last_seen().get(hall_id, user_id).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_last_seen_update_and_get() {
        let dir = tempdir().unwrap();
        let db = Database::open(dir.path().join("test.db")).unwrap();

        let user_id = create_test_user(&db);
        let hall_id = create_test_hall(&db, user_id);

        // Update last seen
        db.last_seen().update(hall_id, user_id).unwrap();

        // Get it back
        let result = db.last_seen().get(hall_id, user_id).unwrap();
        assert!(result.is_some());
        let record = result.unwrap();
        assert_eq!(record.hall_id, hall_id);
        assert_eq!(record.user_id, user_id);
    }
}
