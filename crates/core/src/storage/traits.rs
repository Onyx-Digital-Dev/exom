//! Storage repository traits
//!
//! These traits define the storage interface, allowing for different
//! implementations (SQLite, mock, future network backend).

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{
    Hall, HallRole, Invite, MemberInfo, Membership, Message, MessageDisplay, Session, User,
};

/// User repository operations
pub trait UserRepository {
    /// Create a new user
    fn create_user(&self, user: &User) -> Result<()>;

    /// Find user by ID
    fn find_user_by_id(&self, id: Uuid) -> Result<Option<User>>;

    /// Find user by username
    fn find_user_by_username(&self, username: &str) -> Result<Option<User>>;

    /// Update user's last login time
    fn update_last_login(&self, user_id: Uuid) -> Result<()>;

    /// Create a session
    fn create_session(&self, session: &Session) -> Result<()>;

    /// Find a valid (non-expired) session
    fn find_valid_session(&self, session_id: Uuid) -> Result<Option<Session>>;

    /// Delete a session
    fn delete_session(&self, session_id: Uuid) -> Result<()>;

    /// Delete all sessions for a user
    fn delete_user_sessions(&self, user_id: Uuid) -> Result<()>;

    /// Clean up expired sessions
    fn cleanup_expired_sessions(&self) -> Result<u64>;
}

/// Hall repository operations
pub trait HallRepository {
    /// Create a new Hall
    fn create_hall(&self, hall: &Hall) -> Result<()>;

    /// Find Hall by ID
    fn find_hall_by_id(&self, id: Uuid) -> Result<Option<Hall>>;

    /// Update a Hall
    fn update_hall(&self, hall: &Hall) -> Result<()>;

    /// Delete a Hall
    fn delete_hall(&self, hall_id: Uuid) -> Result<()>;

    /// List all Halls for a user
    fn list_halls_for_user(&self, user_id: Uuid) -> Result<Vec<Hall>>;

    /// Add a membership
    fn add_member(&self, membership: &Membership) -> Result<()>;

    /// Get a membership
    fn get_membership(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<Membership>>;

    /// Update membership role
    fn update_role(&self, user_id: Uuid, hall_id: Uuid, new_role: HallRole) -> Result<()>;

    /// Update online status
    fn update_online_status(&self, user_id: Uuid, hall_id: Uuid, is_online: bool) -> Result<()>;

    /// Remove a membership
    fn remove_member(&self, user_id: Uuid, hall_id: Uuid) -> Result<()>;

    /// List members of a Hall with user info
    fn list_members(&self, hall_id: Uuid) -> Result<Vec<MemberInfo>>;

    /// Get user's role in a Hall
    fn get_user_role(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<HallRole>>;
}

/// Message repository operations
pub trait MessageRepository {
    /// Create a new message
    fn create_message(&self, message: &Message) -> Result<()>;

    /// Find message by ID
    fn find_message_by_id(&self, id: Uuid) -> Result<Option<Message>>;

    /// List messages for a Hall with display info
    fn list_messages_for_hall(
        &self,
        hall_id: Uuid,
        limit: u32,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<MessageDisplay>>;

    /// Update message content
    fn update_message_content(&self, message_id: Uuid, new_content: &str) -> Result<()>;

    /// Soft delete a message
    fn delete_message(&self, message_id: Uuid) -> Result<()>;

    /// Get message count for a Hall
    fn count_messages_for_hall(&self, hall_id: Uuid) -> Result<u64>;
}

/// Invite repository operations
pub trait InviteRepository {
    /// Create a new invite
    fn create_invite(&self, invite: &Invite) -> Result<()>;

    /// Find invite by token
    fn find_invite_by_token(&self, token: &str) -> Result<Option<Invite>>;

    /// List invites for a Hall
    fn list_invites_for_hall(&self, hall_id: Uuid) -> Result<Vec<Invite>>;

    /// Increment invite use count
    fn increment_use_count(&self, invite_id: Uuid) -> Result<()>;

    /// Revoke an invite
    fn revoke_invite(&self, invite_id: Uuid) -> Result<()>;

    /// Delete an invite
    fn delete_invite(&self, invite_id: Uuid) -> Result<()>;
}

/// Combined storage interface
///
/// Provides access to all repository operations.
/// Implementations may be backed by SQLite, mocks, or network.
pub trait Storage: UserRepository + HallRepository + MessageRepository + InviteRepository {}

// Blanket implementation: any type implementing all traits implements Storage
impl<T> Storage for T where T: UserRepository + HallRepository + MessageRepository + InviteRepository
{}
