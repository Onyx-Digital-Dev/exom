//! Hall storage operations

use rusqlite::{params, Connection};
use tracing::instrument;
use uuid::Uuid;

use super::parse::{
    parse_datetime, parse_parlor_id_opt, parse_uuid, parse_uuid_opt, role_from_u8, OptionalExt,
};
use crate::error::Result;
use crate::models::{Hall, HallRole, MemberInfo, Membership};

pub struct HallStore<'a> {
    conn: &'a Connection,
}

impl<'a> HallStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create a new Hall
    #[instrument(skip(self, hall), fields(hall_name = %hall.name))]
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
    #[instrument(skip(self))]
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<Hall>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, description, owner_id, created_at, active_parlor, current_host_id, election_epoch
             FROM halls WHERE id = ?1",
        )?;

        let hall = stmt
            .query_row(params![id.to_string()], |row| {
                Ok(Hall {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    owner_id: parse_uuid(&row.get::<_, String>(3)?)?,
                    created_at: parse_datetime(&row.get::<_, String>(4)?)?,
                    active_parlor: parse_parlor_id_opt(row.get::<_, Option<String>>(5)?)?,
                    current_host_id: parse_uuid_opt(row.get::<_, Option<String>>(6)?)?,
                    election_epoch: row.get(7)?,
                })
            })
            .optional()?;

        Ok(hall)
    }

    /// Update Hall
    #[instrument(skip(self, hall), fields(hall_id = %hall.id))]
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
    #[instrument(skip(self))]
    pub fn delete(&self, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM halls WHERE id = ?1",
            params![hall_id.to_string()],
        )?;
        Ok(())
    }

    /// List all Halls for a user
    #[instrument(skip(self))]
    pub fn list_for_user(&self, user_id: Uuid) -> Result<Vec<Hall>> {
        let mut stmt = self.conn.prepare(
            "SELECT h.id, h.name, h.description, h.owner_id, h.created_at, h.active_parlor, h.current_host_id, h.election_epoch
             FROM halls h
             INNER JOIN memberships m ON m.hall_id = h.id
             WHERE m.user_id = ?1
             ORDER BY h.name",
        )?;

        let halls = stmt
            .query_map(params![user_id.to_string()], |row| {
                Ok(Hall {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    owner_id: parse_uuid(&row.get::<_, String>(3)?)?,
                    created_at: parse_datetime(&row.get::<_, String>(4)?)?,
                    active_parlor: parse_parlor_id_opt(row.get::<_, Option<String>>(5)?)?,
                    current_host_id: parse_uuid_opt(row.get::<_, Option<String>>(6)?)?,
                    election_epoch: row.get(7)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(halls)
    }

    /// Add membership
    #[instrument(skip(self, membership), fields(user_id = %membership.user_id, hall_id = %membership.hall_id, role = ?membership.role))]
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
    #[instrument(skip(self))]
    pub fn get_membership(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<Membership>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, user_id, hall_id, role, joined_at, is_online FROM memberships
             WHERE user_id = ?1 AND hall_id = ?2",
        )?;

        let membership = stmt
            .query_row(params![user_id.to_string(), hall_id.to_string()], |row| {
                Ok(Membership {
                    id: parse_uuid(&row.get::<_, String>(0)?)?,
                    user_id: parse_uuid(&row.get::<_, String>(1)?)?,
                    hall_id: parse_uuid(&row.get::<_, String>(2)?)?,
                    role: role_from_u8(row.get::<_, u8>(3)?),
                    joined_at: parse_datetime(&row.get::<_, String>(4)?)?,
                    is_online: row.get::<_, i32>(5)? != 0,
                })
            })
            .optional()?;

        Ok(membership)
    }

    /// Update membership role
    #[instrument(skip(self))]
    pub fn update_role(&self, user_id: Uuid, hall_id: Uuid, new_role: HallRole) -> Result<()> {
        self.conn.execute(
            "UPDATE memberships SET role = ?1 WHERE user_id = ?2 AND hall_id = ?3",
            params![new_role as u8, user_id.to_string(), hall_id.to_string()],
        )?;
        Ok(())
    }

    /// Update online status
    #[instrument(skip(self))]
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
    #[instrument(skip(self))]
    pub fn remove_member(&self, user_id: Uuid, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM memberships WHERE user_id = ?1 AND hall_id = ?2",
            params![user_id.to_string(), hall_id.to_string()],
        )?;
        Ok(())
    }

    /// List members of a Hall with user info
    #[instrument(skip(self))]
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
                let user_id = parse_uuid(&row.get::<_, String>(0)?)?;
                let host_id = parse_uuid_opt(row.get::<_, Option<String>>(4)?)?;

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
    #[instrument(skip(self))]
    pub fn get_user_role(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<HallRole>> {
        let membership = self.get_membership(user_id, hall_id)?;
        Ok(membership.map(|m| m.role))
    }

    /// Set hall host (for host election persistence)
    #[instrument(skip(self))]
    pub fn set_hall_host(&self, hall_id: Uuid, user_id: Uuid, epoch: u64) -> Result<()> {
        self.conn.execute(
            "UPDATE halls SET current_host_id = ?1, election_epoch = ?2 WHERE id = ?3",
            params![user_id.to_string(), epoch, hall_id.to_string()],
        )?;
        Ok(())
    }

    /// Get current hall host (user_id, epoch)
    #[instrument(skip(self))]
    pub fn get_hall_host(&self, hall_id: Uuid) -> Result<Option<(Uuid, u64)>> {
        let mut stmt = self
            .conn
            .prepare("SELECT current_host_id, election_epoch FROM halls WHERE id = ?1")?;

        let result = stmt
            .query_row(params![hall_id.to_string()], |row| {
                let host_id: Option<String> = row.get(0)?;
                let epoch: u64 = row.get(1)?;
                Ok((host_id, epoch))
            })
            .optional()?;

        match result {
            Some((Some(host_id_str), epoch)) => {
                let host_id = parse_uuid(&host_id_str)?;
                Ok(Some((host_id, epoch)))
            }
            _ => Ok(None),
        }
    }

    /// Get current host username for a hall
    #[instrument(skip(self))]
    pub fn get_current_host_name(&self, hall_id: Uuid) -> Result<Option<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT u.username FROM halls h
             INNER JOIN users u ON u.id = h.current_host_id
             WHERE h.id = ?1",
        )?;

        let username = stmt
            .query_row(params![hall_id.to_string()], |row| row.get(0))
            .optional()?;

        Ok(username)
    }
}
