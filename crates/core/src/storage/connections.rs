//! Connection state storage for auto-reconnect

use chrono::{DateTime, Utc};
use rusqlite::Connection;
use tracing::instrument;
use uuid::Uuid;

use crate::error::Result;

/// Stored last connection info for a user
#[derive(Debug, Clone)]
pub struct LastConnection {
    pub user_id: Uuid,
    pub hall_id: Uuid,
    pub invite_url: String,
    pub host_addr: Option<String>,
    pub last_connected_at: DateTime<Utc>,
    pub epoch: u64,
}

/// Connection state store
pub struct ConnectionStore<'a> {
    conn: &'a Connection,
}

impl<'a> ConnectionStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Save last connection for a user (upserts)
    #[instrument(skip(self, last_conn), fields(user_id = %last_conn.user_id, hall_id = %last_conn.hall_id))]
    pub fn save_last_connection(&self, last_conn: &LastConnection) -> Result<()> {
        self.conn.execute(
            "INSERT INTO last_connections (user_id, hall_id, invite_url, host_addr, last_connected_at, epoch)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(user_id) DO UPDATE SET
                hall_id = excluded.hall_id,
                invite_url = excluded.invite_url,
                host_addr = excluded.host_addr,
                last_connected_at = excluded.last_connected_at,
                epoch = excluded.epoch",
            rusqlite::params![
                last_conn.user_id.to_string(),
                last_conn.hall_id.to_string(),
                last_conn.invite_url,
                last_conn.host_addr,
                last_conn.last_connected_at.to_rfc3339(),
                last_conn.epoch as i64,
            ],
        )?;
        Ok(())
    }

    /// Get last connection for a user
    #[instrument(skip(self))]
    pub fn get_last_connection(&self, user_id: Uuid) -> Result<Option<LastConnection>> {
        let mut stmt = self.conn.prepare(
            "SELECT user_id, hall_id, invite_url, host_addr, last_connected_at, epoch
             FROM last_connections WHERE user_id = ?1",
        )?;

        let result = stmt.query_row([user_id.to_string()], |row| {
            let user_id_str: String = row.get(0)?;
            let hall_id_str: String = row.get(1)?;
            let invite_url: String = row.get(2)?;
            let host_addr: Option<String> = row.get(3)?;
            let last_connected_at_str: String = row.get(4)?;
            let epoch: i64 = row.get(5)?;

            Ok(LastConnection {
                user_id: Uuid::parse_str(&user_id_str).unwrap_or_default(),
                hall_id: Uuid::parse_str(&hall_id_str).unwrap_or_default(),
                invite_url,
                host_addr,
                last_connected_at: DateTime::parse_from_rfc3339(&last_connected_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now()),
                epoch: epoch as u64,
            })
        });

        match result {
            Ok(conn) => Ok(Some(conn)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Clear last connection for a user
    #[instrument(skip(self))]
    pub fn clear_last_connection(&self, user_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM last_connections WHERE user_id = ?1",
            [user_id.to_string()],
        )?;
        Ok(())
    }
}
