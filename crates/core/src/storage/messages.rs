//! Message storage operations

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{HallRole, Message, MessageDisplay};

pub struct MessageStore<'a> {
    conn: &'a Connection,
}

impl<'a> MessageStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    /// Create a new message
    pub fn create(&self, message: &Message) -> Result<()> {
        self.conn.execute(
            "INSERT INTO messages (id, hall_id, sender_id, content, created_at, edited_at, is_deleted)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                message.id.to_string(),
                message.hall_id.to_string(),
                message.sender_id.to_string(),
                message.content,
                message.created_at.to_rfc3339(),
                message.edited_at.map(|t| t.to_rfc3339()),
                message.is_deleted as i32,
            ],
        )?;
        Ok(())
    }

    /// Get message by ID
    pub fn find_by_id(&self, id: Uuid) -> Result<Option<Message>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, hall_id, sender_id, content, created_at, edited_at, is_deleted
             FROM messages WHERE id = ?1",
        )?;

        let message = stmt
            .query_row(params![id.to_string()], |row| {
                Ok(Message {
                    id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                    hall_id: Uuid::parse_str(&row.get::<_, String>(1)?).unwrap(),
                    sender_id: Uuid::parse_str(&row.get::<_, String>(2)?).unwrap(),
                    content: row.get(3)?,
                    created_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                        .unwrap()
                        .with_timezone(&Utc),
                    edited_at: row.get::<_, Option<String>>(5)?.map(|s| {
                        DateTime::parse_from_rfc3339(&s)
                            .unwrap()
                            .with_timezone(&Utc)
                    }),
                    is_deleted: row.get::<_, i32>(6)? != 0,
                })
            })
            .optional()?;

        Ok(message)
    }

    /// List messages for a Hall with display info
    pub fn list_for_hall(
        &self,
        hall_id: Uuid,
        limit: u32,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<MessageDisplay>> {
        let query = if before.is_some() {
            "SELECT m.id, u.username, mb.role, m.content, m.created_at, m.edited_at
             FROM messages m
             INNER JOIN users u ON u.id = m.sender_id
             LEFT JOIN memberships mb ON mb.user_id = m.sender_id AND mb.hall_id = m.hall_id
             WHERE m.hall_id = ?1 AND m.is_deleted = 0 AND m.created_at < ?2
             ORDER BY m.created_at DESC
             LIMIT ?3"
        } else {
            "SELECT m.id, u.username, mb.role, m.content, m.created_at, m.edited_at
             FROM messages m
             INNER JOIN users u ON u.id = m.sender_id
             LEFT JOIN memberships mb ON mb.user_id = m.sender_id AND mb.hall_id = m.hall_id
             WHERE m.hall_id = ?1 AND m.is_deleted = 0
             ORDER BY m.created_at DESC
             LIMIT ?2"
        };

        let mut stmt = self.conn.prepare(query)?;

        let messages: Vec<MessageDisplay> = if let Some(before_time) = before {
            stmt.query_map(
                params![hall_id.to_string(), before_time.to_rfc3339(), limit],
                Self::map_message_display,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        } else {
            stmt.query_map(
                params![hall_id.to_string(), limit],
                Self::map_message_display,
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?
        };

        // Reverse to get chronological order
        let mut messages = messages;
        messages.reverse();
        Ok(messages)
    }

    fn map_message_display(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageDisplay> {
        Ok(MessageDisplay {
            id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
            sender_username: row.get(1)?,
            sender_role: row
                .get::<_, Option<u8>>(2)?
                .map(role_from_u8)
                .unwrap_or(HallRole::HallFellow),
            content: row.get(3)?,
            timestamp: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                .unwrap()
                .with_timezone(&Utc),
            is_edited: row.get::<_, Option<String>>(5)?.is_some(),
        })
    }

    /// Update message content
    pub fn update_content(&self, message_id: Uuid, new_content: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET content = ?1, edited_at = ?2 WHERE id = ?3",
            params![new_content, Utc::now().to_rfc3339(), message_id.to_string()],
        )?;
        Ok(())
    }

    /// Soft delete message
    pub fn delete(&self, message_id: Uuid) -> Result<()> {
        self.conn.execute(
            "UPDATE messages SET is_deleted = 1 WHERE id = ?1",
            params![message_id.to_string()],
        )?;
        Ok(())
    }

    /// Get message count for Hall
    pub fn count_for_hall(&self, hall_id: Uuid) -> Result<u64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE hall_id = ?1 AND is_deleted = 0",
            params![hall_id.to_string()],
            |row| row.get(0),
        )?;
        Ok(count as u64)
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
