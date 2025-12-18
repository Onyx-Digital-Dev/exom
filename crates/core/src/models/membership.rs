//! Membership and role models

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Hall roles in priority order (highest to lowest)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum HallRole {
    /// Owner - full control
    HallBuilder = 5,
    /// Admin - most permissions except ownership transfer
    HallPrefect = 4,
    /// Moderator - can manage members and messages
    HallModerator = 3,
    /// Member - standard participant
    HallAgent = 2,
    /// Guest - limited access
    HallFellow = 1,
}

impl HallRole {
    pub fn display_name(&self) -> &'static str {
        match self {
            HallRole::HallBuilder => "Hall Builder",
            HallRole::HallPrefect => "Hall Prefect",
            HallRole::HallModerator => "Hall Moderator",
            HallRole::HallAgent => "Hall Agent",
            HallRole::HallFellow => "Hall Fellow",
        }
    }

    pub fn short_name(&self) -> &'static str {
        match self {
            HallRole::HallBuilder => "Builder",
            HallRole::HallPrefect => "Prefect",
            HallRole::HallModerator => "Moderator",
            HallRole::HallAgent => "Agent",
            HallRole::HallFellow => "Fellow",
        }
    }

    /// Returns the hosting priority (higher = more priority)
    pub fn hosting_priority(&self) -> u8 {
        *self as u8
    }

    /// Can this role host a Hall?
    pub fn can_host(&self) -> bool {
        // Fellows cannot host, all others can
        *self >= HallRole::HallAgent
    }

    /// All roles in priority order (highest first)
    pub fn all_by_priority() -> &'static [HallRole] {
        &[
            HallRole::HallBuilder,
            HallRole::HallPrefect,
            HallRole::HallModerator,
            HallRole::HallAgent,
            HallRole::HallFellow,
        ]
    }
}

impl std::fmt::Display for HallRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.display_name())
    }
}

/// A user's membership in a Hall
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Membership {
    pub id: Uuid,
    pub user_id: Uuid,
    pub hall_id: Uuid,
    pub role: HallRole,
    pub joined_at: DateTime<Utc>,
    pub is_online: bool,
}

impl Membership {
    pub fn new(user_id: Uuid, hall_id: Uuid, role: HallRole) -> Self {
        Self {
            id: Uuid::new_v4(),
            user_id,
            hall_id,
            role,
            joined_at: Utc::now(),
            is_online: false,
        }
    }
}

/// Represents a member with their user info for display
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemberInfo {
    pub user_id: Uuid,
    pub username: String,
    pub role: HallRole,
    pub is_online: bool,
    pub is_host: bool,
}
