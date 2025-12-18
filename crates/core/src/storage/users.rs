//! User storage operations

use chrono::Utc;
use rusqlite::{params, Connection};
use tracing::instrument;
use uuid::Uuid;

use super::parse::{parse_datetime, parse_datetime_opt, parse_uuid, OptionalExt};
use crate::error::Result;
use crate::models::{Session, User};

pub struct UserStore<'a> {
    conn: &'a Connection,
}

impl<'a> UserStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create a new user
    #[instrument(skip(self, user), fields(username = %user.username))]
    pub fn create(&self, user: &User) -> Result<()> {
        self.conn.execute(
            "INSERT INTO users (id, username, password_hash, created_at, last_login) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                user.id.to_string(),
                user.username,
                user.password_hash,
                user.created_at.to_rfc3339(),
                user.last_login.map(|t| t.to_rfc3339()),
            ],
        )?;
        Ok(())
    }

    /// Find user by ID
    #[instrument(skip(self))]
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, password_hash, created_at, last_login FROM users WHERE id = ?1",
        )?;

        let user = stmt
            .query_row(params![id.to_string()], |row| {
                Ok(User {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    created_at: parse_datetime(&row.get::<_, String>(3)?)?,
                    last_login: parse_datetime_opt(row.get::<_, Option<String>>(4)?)?,
                })
            })
            .optional()?;

        Ok(user)
    }

    /// Find user by username
    #[instrument(skip(self))]
    pub fn find_by_username(&self, username: &str) -> Result<Option<User>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, username, password_hash, created_at, last_login FROM users WHERE username = ?1",
        )?;

        let user = stmt
            .query_row(params![username], |row| {
                Ok(User {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    created_at: parse_datetime(&row.get::<_, String>(3)?)?,
                    last_login: parse_datetime_opt(row.get::<_, Option<String>>(4)?)?,
                })
            })
            .optional()?;

        Ok(user)
    }

    /// Update last login time
    pub fn update_last_login(&self, user_id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE users SET last_login = ?1 WHERE id = ?2",
            params![Utc::now().to_rfc3339(), user_id.to_string()],
        )?;
        Ok(())
    }

    /// Create a session
    #[instrument(skip(self, session), fields(user_id = %session.user_id))]
    pub fn create_session(&self, session: &Session) -> Result<()> {
        self.conn.execute(
            "INSERT INTO sessions (id, user_id, created_at, expires_at) VALUES (?1, ?2, ?3, ?4)",
            params![
                session.id.to_string(),
                session.user_id.to_string(),
                session.created_at.to_rfc3339(),
                session.expires_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    /// Find valid session
    #[instrument(skip(self))]
    pub fn find_valid_session(&self, session_id: Uuid) -> Result<Option<Session>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, user_id, created_at, expires_at FROM sessions WHERE id = ?1 AND expires_at > ?2",
        )?;

        let now = Utc::now().to_rfc3339();
        let session = stmt
            .query_row(params![session_id.to_string(), now], |row| {
                Ok(Session {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    user_id: parse_uuid(&row.get::<_, String>(1)?)?,
                    created_at: parse_datetime(&row.get::<_, String>(2)?)?,
                    expires_at: parse_datetime(&row.get::<_, String>(3)?)?,
                })
            })
            .optional()?;

        Ok(session)
    }

    /// Delete session
    pub fn delete_session(&self, session_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM sessions WHERE id = ?1",
            params![session_id.to_string()],
        )?;
        Ok(())
    }

    /// Delete all sessions for user
    pub fn delete_user_sessions(&self, user_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM sessions WHERE user_id = ?1",
            params![user_id.to_string()],
        )?;
        Ok(())
    }

    /// Clean up expired sessions
    pub fn cleanup_expired_sessions(&self) -> Result<u64> {
        let count = self.conn.execute(
            "DELETE FROM sessions WHERE expires_at < ?1",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(count as u64)
    }
}
