//! Associates storage layer
//!
//! Global, mutual-only relationships between users.
//! Associates can see each other's desk status regardless of hall membership.

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;

/// Associate relationship status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AssociateStatus {
    /// Request sent, awaiting acceptance
    Pending,
    /// Both parties accepted - mutual associates
    Accepted,
    /// One party blocked the other
    Blocked,
}

impl AssociateStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            AssociateStatus::Pending => "pending",
            AssociateStatus::Accepted => "accepted",
            AssociateStatus::Blocked => "blocked",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "pending" => Some(AssociateStatus::Pending),
            "accepted" => Some(AssociateStatus::Accepted),
            "blocked" => Some(AssociateStatus::Blocked),
            _ => None,
        }
    }
}

/// An associate relationship between two users
#[derive(Debug, Clone)]
pub struct AssociateRelation {
    pub id: Uuid,
    /// The user who initiated the request
    pub requester_id: Uuid,
    /// The user who received the request
    pub target_id: Uuid,
    /// Current status of the relationship
    pub status: AssociateStatus,
    /// When the request was created
    pub created_at: DateTime<Utc>,
    /// When the status was last updated
    pub updated_at: DateTime<Utc>,
}

/// An associate request (incoming or outgoing)
#[derive(Debug, Clone)]
pub struct AssociateRequest {
    pub id: Uuid,
    pub other_user_id: Uuid,
    pub other_username: String,
    pub is_incoming: bool,
    pub created_at: DateTime<Utc>,
}

/// An accepted associate
#[derive(Debug, Clone)]
pub struct Associate {
    pub user_id: Uuid,
    pub username: String,
    pub since: DateTime<Utc>,
}

/// Associates storage operations
pub struct AssociateStore<'a> {
    conn: &'a Connection,
}

impl<'a> AssociateStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Send an associate request to another user
    /// Returns the request ID if successful
    pub fn send_request(&self, requester_id: Uuid, target_id: Uuid) -> Result<Uuid> {
        // Check if a relationship already exists (in either direction)
        if self.relationship_exists(requester_id, target_id)? {
            return Err(crate::error::Error::InvalidOperation(
                "Relationship already exists".to_string(),
            ));
        }

        // Check if blocked
        if self.is_blocked(requester_id, target_id)? {
            return Err(crate::error::Error::InvalidOperation(
                "Cannot send request - blocked".to_string(),
            ));
        }

        let id = Uuid::new_v4();
        let now = Utc::now();

        self.conn.execute(
            "INSERT INTO associates (id, requester_id, target_id, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                id.to_string(),
                requester_id.to_string(),
                target_id.to_string(),
                AssociateStatus::Pending.as_str(),
                now.to_rfc3339(),
                now.to_rfc3339(),
            ],
        )?;

        Ok(id)
    }

    /// Accept an incoming associate request
    pub fn accept_request(&self, request_id: Uuid, accepting_user_id: Uuid) -> Result<()> {
        // Verify the request exists and is pending and the accepting user is the target
        let now = Utc::now();

        let updated = self.conn.execute(
            "UPDATE associates SET status = ?1, updated_at = ?2
             WHERE id = ?3 AND target_id = ?4 AND status = ?5",
            params![
                AssociateStatus::Accepted.as_str(),
                now.to_rfc3339(),
                request_id.to_string(),
                accepting_user_id.to_string(),
                AssociateStatus::Pending.as_str(),
            ],
        )?;

        if updated == 0 {
            return Err(crate::error::Error::NotFound(
                "Request not found or not pending".to_string(),
            ));
        }

        Ok(())
    }

    /// Decline an incoming associate request
    pub fn decline_request(&self, request_id: Uuid, declining_user_id: Uuid) -> Result<()> {
        // Simply delete the request if the declining user is the target
        let deleted = self.conn.execute(
            "DELETE FROM associates WHERE id = ?1 AND target_id = ?2 AND status = ?3",
            params![
                request_id.to_string(),
                declining_user_id.to_string(),
                AssociateStatus::Pending.as_str(),
            ],
        )?;

        if deleted == 0 {
            return Err(crate::error::Error::NotFound(
                "Request not found or not pending".to_string(),
            ));
        }

        Ok(())
    }

    /// Remove an associate relationship (either party can do this)
    pub fn remove_associate(&self, user_id: Uuid, other_user_id: Uuid) -> Result<()> {
        let deleted = self.conn.execute(
            "DELETE FROM associates
             WHERE status = ?1
             AND ((requester_id = ?2 AND target_id = ?3)
                  OR (requester_id = ?3 AND target_id = ?2))",
            params![
                AssociateStatus::Accepted.as_str(),
                user_id.to_string(),
                other_user_id.to_string(),
            ],
        )?;

        if deleted == 0 {
            return Err(crate::error::Error::NotFound(
                "Associate relationship not found".to_string(),
            ));
        }

        Ok(())
    }

    /// Block a user (removes any existing relationship and prevents new requests)
    pub fn block_user(&self, blocker_id: Uuid, blocked_id: Uuid) -> Result<()> {
        let now = Utc::now();

        // First, try to update existing relationship to blocked
        let updated = self.conn.execute(
            "UPDATE associates SET status = ?1, updated_at = ?2
             WHERE (requester_id = ?3 AND target_id = ?4)
                OR (requester_id = ?4 AND target_id = ?3)",
            params![
                AssociateStatus::Blocked.as_str(),
                now.to_rfc3339(),
                blocker_id.to_string(),
                blocked_id.to_string(),
            ],
        )?;

        // If no existing relationship, create a blocked one
        if updated == 0 {
            let id = Uuid::new_v4();
            self.conn.execute(
                "INSERT INTO associates (id, requester_id, target_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id.to_string(),
                    blocker_id.to_string(),
                    blocked_id.to_string(),
                    AssociateStatus::Blocked.as_str(),
                    now.to_rfc3339(),
                    now.to_rfc3339(),
                ],
            )?;
        }

        Ok(())
    }

    /// Check if two users are mutual associates
    pub fn is_associate(&self, user_a: Uuid, user_b: Uuid) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM associates
             WHERE status = ?1
             AND ((requester_id = ?2 AND target_id = ?3)
                  OR (requester_id = ?3 AND target_id = ?2))",
            params![
                AssociateStatus::Accepted.as_str(),
                user_a.to_string(),
                user_b.to_string(),
            ],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Check if a relationship exists (any status)
    fn relationship_exists(&self, user_a: Uuid, user_b: Uuid) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM associates
             WHERE (requester_id = ?1 AND target_id = ?2)
                OR (requester_id = ?2 AND target_id = ?1)",
            params![user_a.to_string(), user_b.to_string()],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// Check if either user has blocked the other
    fn is_blocked(&self, user_a: Uuid, user_b: Uuid) -> Result<bool> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM associates
             WHERE status = ?1
             AND ((requester_id = ?2 AND target_id = ?3)
                  OR (requester_id = ?3 AND target_id = ?2))",
            params![
                AssociateStatus::Blocked.as_str(),
                user_a.to_string(),
                user_b.to_string(),
            ],
            |row| row.get(0),
        )?;

        Ok(count > 0)
    }

    /// List all accepted associates for a user
    pub fn list_associates(&self, user_id: Uuid) -> Result<Vec<Associate>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                CASE WHEN a.requester_id = ?1 THEN a.target_id ELSE a.requester_id END as other_id,
                u.username,
                a.updated_at
             FROM associates a
             JOIN users u ON u.id = CASE WHEN a.requester_id = ?1 THEN a.target_id ELSE a.requester_id END
             WHERE a.status = ?2
             AND (a.requester_id = ?1 OR a.target_id = ?1)
             ORDER BY u.username",
        )?;

        let associates = stmt
            .query_map(
                params![user_id.to_string(), AssociateStatus::Accepted.as_str()],
                |row| {
                    let user_id_str: String = row.get(0)?;
                    let username: String = row.get(1)?;
                    let since_str: String = row.get(2)?;

                    Ok(Associate {
                        user_id: Uuid::parse_str(&user_id_str).unwrap_or_default(),
                        username,
                        since: DateTime::parse_from_rfc3339(&since_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(associates)
    }

    /// List pending requests (both incoming and outgoing)
    pub fn list_pending_requests(&self, user_id: Uuid) -> Result<Vec<AssociateRequest>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                a.id,
                CASE WHEN a.requester_id = ?1 THEN a.target_id ELSE a.requester_id END as other_id,
                u.username,
                CASE WHEN a.target_id = ?1 THEN 1 ELSE 0 END as is_incoming,
                a.created_at
             FROM associates a
             JOIN users u ON u.id = CASE WHEN a.requester_id = ?1 THEN a.target_id ELSE a.requester_id END
             WHERE a.status = ?2
             AND (a.requester_id = ?1 OR a.target_id = ?1)
             ORDER BY a.created_at DESC",
        )?;

        let requests = stmt
            .query_map(
                params![user_id.to_string(), AssociateStatus::Pending.as_str()],
                |row| {
                    let id_str: String = row.get(0)?;
                    let other_user_id_str: String = row.get(1)?;
                    let other_username: String = row.get(2)?;
                    let is_incoming: i32 = row.get(3)?;
                    let created_at_str: String = row.get(4)?;

                    Ok(AssociateRequest {
                        id: Uuid::parse_str(&id_str).unwrap_or_default(),
                        other_user_id: Uuid::parse_str(&other_user_id_str).unwrap_or_default(),
                        other_username,
                        is_incoming: is_incoming != 0,
                        created_at: DateTime::parse_from_rfc3339(&created_at_str)
                            .map(|dt| dt.with_timezone(&Utc))
                            .unwrap_or_else(|_| Utc::now()),
                    })
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(requests)
    }

    /// Get all associate user IDs for a user (for visibility checks)
    pub fn get_associate_ids(&self, user_id: Uuid) -> Result<Vec<Uuid>> {
        let mut stmt = self.conn.prepare(
            "SELECT
                CASE WHEN requester_id = ?1 THEN target_id ELSE requester_id END as other_id
             FROM associates
             WHERE status = ?2
             AND (requester_id = ?1 OR target_id = ?1)",
        )?;

        let ids = stmt
            .query_map(
                params![user_id.to_string(), AssociateStatus::Accepted.as_str()],
                |row| {
                    let id_str: String = row.get(0)?;
                    Ok(Uuid::parse_str(&id_str).unwrap_or_default())
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(ids)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::Database;

    fn setup_test_db() -> Database {
        let db = Database::open_in_memory().unwrap();
        // Create test users
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
        db.conn
            .execute(
                "INSERT INTO users (id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    "22222222-2222-2222-2222-222222222222",
                    "bob",
                    "hash",
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
        db.conn
            .execute(
                "INSERT INTO users (id, username, password_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![
                    "33333333-3333-3333-3333-333333333333",
                    "charlie",
                    "hash",
                    Utc::now().to_rfc3339()
                ],
            )
            .unwrap();
        db
    }

    #[test]
    fn test_send_and_accept_request() {
        let db = setup_test_db();
        let store = db.associates();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let bob = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();

        // Send request
        let request_id = store.send_request(alice, bob).unwrap();

        // Not associates yet
        assert!(!store.is_associate(alice, bob).unwrap());

        // Bob accepts
        store.accept_request(request_id, bob).unwrap();

        // Now they are associates
        assert!(store.is_associate(alice, bob).unwrap());
        assert!(store.is_associate(bob, alice).unwrap()); // Symmetric
    }

    #[test]
    fn test_decline_request() {
        let db = setup_test_db();
        let store = db.associates();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let bob = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();

        let request_id = store.send_request(alice, bob).unwrap();
        store.decline_request(request_id, bob).unwrap();

        assert!(!store.is_associate(alice, bob).unwrap());
    }

    #[test]
    fn test_remove_associate() {
        let db = setup_test_db();
        let store = db.associates();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let bob = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();

        let request_id = store.send_request(alice, bob).unwrap();
        store.accept_request(request_id, bob).unwrap();

        assert!(store.is_associate(alice, bob).unwrap());

        store.remove_associate(alice, bob).unwrap();

        assert!(!store.is_associate(alice, bob).unwrap());
    }

    #[test]
    fn test_block_prevents_request() {
        let db = setup_test_db();
        let store = db.associates();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let bob = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();

        store.block_user(bob, alice).unwrap();

        // Alice cannot send request to Bob
        let result = store.send_request(alice, bob);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_associates() {
        let db = setup_test_db();
        let store = db.associates();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let bob = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();
        let charlie = Uuid::parse_str("33333333-3333-3333-3333-333333333333").unwrap();

        // Alice sends to Bob, Bob accepts
        let req1 = store.send_request(alice, bob).unwrap();
        store.accept_request(req1, bob).unwrap();

        // Charlie sends to Alice, Alice accepts
        let req2 = store.send_request(charlie, alice).unwrap();
        store.accept_request(req2, alice).unwrap();

        let alice_associates = store.list_associates(alice).unwrap();
        assert_eq!(alice_associates.len(), 2);
    }

    #[test]
    fn test_one_way_request_not_associate() {
        let db = setup_test_db();
        let store = db.associates();

        let alice = Uuid::parse_str("11111111-1111-1111-1111-111111111111").unwrap();
        let bob = Uuid::parse_str("22222222-2222-2222-2222-222222222222").unwrap();

        // Send request but don't accept
        store.send_request(alice, bob).unwrap();

        // Not associates - pending only
        assert!(!store.is_associate(alice, bob).unwrap());
    }
}
