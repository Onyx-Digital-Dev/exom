//! Message model for Hall chat

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::HallRole;

/// A chat message in a Hall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Uuid,
    pub hall_id: Uuid,
    pub sender_id: Uuid,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub edited_at: Option<DateTime<Utc>>,
    pub is_deleted: bool,
}

impl Message {
    pub fn new(hall_id: Uuid, sender_id: Uuid, content: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            hall_id,
            sender_id,
            content,
            created_at: Utc::now(),
            edited_at: None,
            is_deleted: false,
        }
    }
}

/// Message with sender information for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageDisplay {
    pub id: Uuid,
    pub sender_username: String,
    pub sender_role: HallRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    pub is_edited: bool,
}

impl MessageDisplay {
    pub fn format_timestamp(&self) -> String {
        self.timestamp.format("%H:%M").to_string()
    }

    pub fn format_date(&self) -> String {
        self.timestamp.format("%Y-%m-%d").to_string()
    }
}
