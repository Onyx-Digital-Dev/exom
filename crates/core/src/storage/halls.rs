//! Hall storage operations

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{Hall, HallRole, MemberInfo, Membership, ParlorId};

pub struct HallStore<'a> {
    conn: &'a Connection,
}

impl<'a> HallStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create a new Hall
    pub fn create(&self, hall: &Hall) -> Result<()> {
        self.conn.execute(
            "INSERT INTO halls (id, name, description, owner_id, created_at, active_parlor, current_host_id, election_epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                hall.id.to_string(),
                hall.name,
                hall.description,
                hall.owner_id.to_string(),
                hall.created_at.to_rfc3339(),
                hall.active_parlor.map(|p| p.0.to_string()),
                hall.current_host_id.map(|h| h.to_string()),
                hall.election_epoch,
            ],
        )?;
        Ok(())
    }

    /// Find Hall by ID
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<Hall>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, owner_id, created_at, active_parlor, current_host_id, election_epoch
             FROM halls WHERE id = ?1"
        )?;

        let hall = stmt
            .query_row(params![id.to_string()], |row| {
                Ok(Hall {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    name: row.get(1)?,
                    description: row.get(2)?,
                    owner_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap(),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    active_parlor: row
                        .get::<_, Option<String>>(5)?
                        .map(|s| ParlorId(Uuid::parse_str(&s).unwrap())),
                    current_host_id: row
                        .get::<_, Option<String>>(6)?
                        .map(|s| Uuid::parse_str(&s).unwrap()),
                    election_epoch: row.get(7)?,
                })
            })
            .optional()?;

        Ok(hall)
    }

    /// Update Hall
    pub fn update(&self, hall: &Hall) -> Result<()> {
        self.conn.execute(
            "UPDATE halls SET name = ?1, description = ?2, active_parlor = ?3, current_host_id = ?4, election_epoch = ?5
             WHERE id = ?6",
            params![
                hall.name,
                hall.description,
                hall.active_parlor.map(|p| p.0.to_string()),
                hall.current_host_id.map(|h| h.to_string()),
                hall.election_epoch,
                hall.id.to_string(),
            ],
        )?;
        Ok(())
    }

    /// Delete Hall
    pub fn delete(&self, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM halls WHERE id = ?1",
            params![hall_id.to_string()],
        )?;
        Ok(())
    }

    /// List all Halls for a user
    pub fn list_for_user(&self, user_id: Uuid) -> Result<Vec<Hall>> {
        let mut stmt = self.conn.prepare(
            "SELECT h.id, h.name, h.description, h.owner_id, h.created_at, h.active_parlor, h.current_host_id, h.election_epoch
             FROM halls h
             INNER JOIN memberships m ON m.hall_id = h.id
             WHERE m.user_id = ?1
             ORDER BY h.name"
        )?;

        let halls = stmt
            .query_map(params![user_id.to_string()], |row| {
                Ok(Hall {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    name: row.get(1)?,
                    description: row.get(2)?,
                    owner_id: Uuid::parse_str(&row.get::<_, String>(3)?).unwrap(),
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    active_parlor: row
                        .get::<_, Option<String>>(5)?
                        .map(|s| ParlorId(Uuid::parse_str(&s).unwrap())),
                    current_host_id: row
                        .get::<_, Option<String>>(6)?
                        .map(|s| Uuid::parse_str(&s).unwrap()),
                    election_epoch: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(halls)
    }

    /// Add membership
    pub fn add_member(&self, membership: &Membership) -> Result<()> {
        self.conn.execute(
            "INSERT INTO memberships (id, user_id, hall_id, role, joined_at, is_online)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                membership.id.to_string(),
                membership.user_id.to_string(),
                membership.hall_id.to_string(),
                membership.role as u8,
                membership.joined_at.to_rfc3339(),
                membership.is_online as i32,
            ],
        )?;
        Ok(())
    }

    /// Get membership
    pub fn get_membership(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<Membership>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, user_id, hall_id, role, joined_at, is_online FROM memberships
             WHERE user_id = ?1 AND hall_id = ?2",
        )?;

        let membership = stmt
            .query_row(params![user_id.to_string(), hall_id.to_string()], |row| {
                Ok(Membership {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    user_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                    hall_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap(),
                    role: role_from_u8(row.get::<_, u8>(3)?),
                    joined_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    is_online: row.get::<_, i32>(5)? != 0,
                })
            })
            .optional()?;

        Ok(membership)
    }

    /// Update membership role
    pub fn update_role(&self, user_id: Uuid, hall_id: Uuid, new_role: HallRole) -> Result<()> {
        self.conn.execute(
            "UPDATE memberships SET role = ?1 WHERE user_id = ?2 AND hall_id = ?3",
            params![new_role as u8, user_id.to_string(), hall_id.to_string()],
        )?;
        Ok(())
    }

    /// Update online status
    pub fn update_online_status(
        &self,
        user_id: Uuid,
        hall_id: Uuid,
        is_online: bool,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE memberships SET is_online = ?1 WHERE user_id = ?2 AND hall_id = ?3",
            params![is_online as i32, user_id.to_string(), hall_id.to_string()],
        )?;
        Ok(())
    }

    /// Remove membership
    pub fn remove_member(&self, user_id: Uuid, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM memberships WHERE user_id = ?1 AND hall_id = ?2",
            params![user_id.to_string(), hall_id.to_string()],
        )?;
        Ok(())
    }

    /// List members of a Hall with user info
    pub fn list_members(&self, hall_id: Uuid) -> Result<Vec<MemberInfo>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.id, u.username, m.role, m.is_online, h.current_host_id
             FROM memberships m
             INNER JOIN users u ON u.id = m.user_id
             INNER JOIN halls h ON h.id = m.hall_id
             WHERE m.hall_id = ?1
             ORDER BY m.role DESC, u.username",
        )?;

        let members = stmt
            .query_map(params![hall_id.to_string()], |row| {
                let user_id = Uuid::parse_str(&row.get::<_, String>(0)?).unwrap();
                let host_id = row
                    .get::<_, Option<String>>(4)?
                    .map(|s| Uuid::parse_str(&s).unwrap());

                Ok(MemberInfo {
                    user_id,
                    username: row.get(1)?,
                    role: role_from_u8(row.get::<_, u8>(2)?),
                    is_online: row.get::<_, i32>(3)? != 0,
                    is_host: host_id == Some(user_id),
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(members)
    }

    /// Get user's role in a Hall
    pub fn get_user_role(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<HallRole>> {
        let membership = self.get_membership(user_id, hall_id)?;
        Ok(membership.map(|m| m.role))
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
