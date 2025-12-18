//! Invite storage operations

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{HallRole, Invite};

pub struct InviteStore<'a> {
    conn: &'a Connection,
}

impl<'a> InviteStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create a new invite
    pub fn create(&self, invite: &Invite) -> Result<()> {
        self.conn.execute(
            "INSERT INTO invites (id, hall_id, token, created_by, role, created_at, expires_at, max_uses, use_count, is_revoked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                invite.id.to_string(),
                invite.hall_id.to_string(),
                invite.token,
                invite.created_by.to_string(),
                invite.role as u8,
                invite.created_at.to_rfc3339(),
                invite.expires_at.map(|t| t.to_rfc3339()),
                invite.max_uses,
                invite.use_count,
                invite.is_revoked as i32,
            ],
        )?;
        Ok(())
    }

    /// Find invite by token
    pub fn find_by_token(&self, token: &str) -> Result<Option<Invite>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hall_id, token, created_by, role, created_at, expires_at, max_uses, use_count, is_revoked
             FROM invites WHERE token = ?1"
        )?;

        let invite = stmt
            .query_row(params![token], |row| {
                Ok(Invite {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    hall_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                    token: row.get(2)?,
                    created_by: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap(),
                    role: role_from_u8(row.get::<_, u8>(4)?),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    expires_at: row.get::<_, Option<String>>(6)?.map(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .unwrap()
                            .with_timezone(&Utc)
                    }),
                    max_uses: row.get(7)?,
                    use_count: row.get(8)?,
                    is_revoked: row.get::<_, i32>(9)? != 0,
                })
            })
            .optional()?;

        Ok(invite)
    }

    /// List invites for a Hall
    pub fn list_for_hall(&self, hall_id: Uuid) -> Result<Vec<Invite>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hall_id, token, created_by, role, created_at, expires_at, max_uses, use_count, is_revoked
             FROM invites WHERE hall_id = ?1 ORDER BY created_at DESC"
        )?;

        let invites = stmt
            .query_map(params![hall_id.to_string()], |row| {
                Ok(Invite {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    hall_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                    token: row.get(2)?,
                    created_by: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap(),
                    role: role_from_u8(row.get::<_, u8>(4)?),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(5)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    expires_at: row.get::<_, Option<String>>(6)?.map(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .unwrap()
                            .with_timezone(&Utc)
                    }),
                    max_uses: row.get(7)?,
                    use_count: row.get(8)?,
                    is_revoked: row.get::<_, i32>(9)? != 0,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(invites)
    }

    /// Increment use count
    pub fn increment_use_count(&self, invite_id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE invites SET use_count = use_count + 1 WHERE id = ?1",
            params![invite_id.to_string()],
        )?;
        Ok(())
    }

    /// Revoke invite
    pub fn revoke(&self, invite_id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE invites SET is_revoked = 1 WHERE id = ?1",
            params![invite_id.to_string()],
        )?;
        Ok(())
    }

    /// Delete invite
    pub fn delete(&self, invite_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM invites WHERE id = ?1",
            params![invite_id.to_string()],
        )?;
        Ok(())
    }
}

fn role_from_u8(value: u8) -> HallRole {
    match value {
        5 => HallRole::HallBuilder,
        4 => HallRole::HallPrefect,
        3 => HallRole::HallModerator,
        2 => HallRole::HallAgent,
        _ => HallRole::HallFellow,
    }
}

trait OptionalExt<T> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error>;
}

impl<T> OptionalExt<T> for std::result::Result<T, rusqlite::Error> {
    fn optional(self) -> std::result::Result<Option<T>, rusqlite::Error> {
        match self {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e),
        }
    }
}
