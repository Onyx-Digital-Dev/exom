//! Invite token model

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::HallRole;

/// An invitation to join a Hall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Invite {
    pub id: Uuid,
    pub hall_id: Uuid,
    pub token: String,
    pub created_by: Uuid,
    pub role: HallRole,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub max_uses: Option<u32>,
    pub use_count: u32,
    pub is_revoked: bool,
}

impl Invite {
    pub fn new(hall_id: Uuid, created_by: Uuid, role: HallRole, token: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            hall_id,
            token,
            created_by,
            role,
            created_at: Utc::now(),
            expires_at: None,
            max_uses: None,
            use_count: 0,
            is_revoked: false,
        }
    }

    pub fn with_expiry(mut self, hours: i64) -> Self {
        self.expires_at = Some(Utc::now() + chrono::Duration::hours(hours));
        self
    }

    pub fn with_max_uses(mut self, max: u32) -> Self {
        self.max_uses = Some(max);
        self
    }

    pub fn is_valid(&self) -> bool {
        if self.is_revoked {
            return false;
        }

        if let Some(expires) = self.expires_at {
            if Utc::now() > expires {
                return false;
            }
        }

        if let Some(max) = self.max_uses {
            if self.use_count >= max {
                return false;
            }
        }

        true
    }
}
