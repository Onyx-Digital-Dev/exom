//! SQLite storage layer for Exom

mod connections;
mod halls;
mod invites;
mod last_seen;
mod messages;
mod migrations;
mod parse;
mod preferences;
mod traits;
mod users;
mod workspace;

use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::error::Result;
use crate::models::{
    Hall, HallRole, Invite, MemberInfo, Membership, Message, MessageDisplay, Session, User,
};
use rusqlite::Connection;
use std::path::Path;
use tracing::instrument;

pub use connections::{ConnectionStore, LastConnection};
pub use halls::HallStore;
pub use invites::InviteStore;
pub use last_seen::{LastSeen, LastSeenStore};
pub use messages::MessageStore;
pub use preferences::{PreferencesStore, UserPreferences};
pub use traits::{HallRepository, InviteRepository, MessageRepository, Storage, UserRepository};
pub use users::UserStore;
pub use workspace::{PersistedTab, PersistedWorkspace, WorkspaceStore};

/// Main database handle
pub struct Database {
    conn: Connection,
}

impl Database {
    /// Open or create database at the given path
    #[instrument(skip(path), fields(path = %path.as_ref().display()))]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Open in-memory database (for testing)
    #[instrument]
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys = ON")?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Initialize database schema via migrations
    fn init(&self) -> Result<()> {
        migrations::run_migrations(&self.conn)?;
        Ok(())
    }

    /// Get current schema version
    pub fn schema_version(&self) -> u32 {
        self.conn
            .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
                row.get(0)
            })
            .unwrap_or(0)
    }

    /// Get user store (legacy accessor)
    pub fn users(&self) -> UserStore<'_> {
        UserStore::new(&self.conn)
    }

    /// Get hall store (legacy accessor)
    pub fn halls(&self) -> HallStore<'_> {
        HallStore::new(&self.conn)
    }

    /// Get message store (legacy accessor)
    pub fn messages(&self) -> MessageStore<'_> {
        MessageStore::new(&self.conn)
    }

    /// Get invite store (legacy accessor)
    pub fn invites(&self) -> InviteStore<'_> {
        InviteStore::new(&self.conn)
    }

    /// Get connection store for auto-reconnect
    pub fn connections(&self) -> ConnectionStore<'_> {
        ConnectionStore::new(&self.conn)
    }

    /// Get workspace store for session continuity
    pub fn workspaces(&self) -> WorkspaceStore<'_> {
        WorkspaceStore::new(&self.conn)
    }

    /// Get preferences store for user settings
    pub fn preferences(&self) -> PreferencesStore<'_> {
        PreferencesStore::new(&self.conn)
    }

    /// Get last seen store for Town Crier bot
    pub fn last_seen(&self) -> LastSeenStore<'_> {
        LastSeenStore::new(&self.conn)
    }
}

// Implement repository traits for Database
// This enables using Database through the trait interface

impl UserRepository for Database {
    fn create_user(&self, user: &User) -> Result<()> {
        self.users().create(user)
    }

    fn find_user_by_id(&self, id: Uuid) -> Result<Option<User>> {
        self.users().find_by_id(id)
    }

    fn find_user_by_username(&self, username: &str) -> Result<Option<User>> {
        self.users().find_by_username(username)
    }

    fn update_last_login(&self, user_id: Uuid) -> Result<()> {
        self.users().update_last_login(user_id)
    }

    fn create_session(&self, session: &Session) -> Result<()> {
        self.users().create_session(session)
    }

    fn find_valid_session(&self, session_id: Uuid) -> Result<Option<Session>> {
        self.users().find_valid_session(session_id)
    }

    fn delete_session(&self, session_id: Uuid) -> Result<()> {
        self.users().delete_session(session_id)
    }

    fn delete_user_sessions(&self, user_id: Uuid) -> Result<()> {
        self.users().delete_user_sessions(user_id)
    }

    fn cleanup_expired_sessions(&self) -> Result<u64> {
        self.users().cleanup_expired_sessions()
    }
}

impl HallRepository for Database {
    fn create_hall(&self, hall: &Hall) -> Result<()> {
        self.halls().create(hall)
    }

    fn find_hall_by_id(&self, id: Uuid) -> Result<Option<Hall>> {
        self.halls().find_by_id(id)
    }

    fn update_hall(&self, hall: &Hall) -> Result<()> {
        self.halls().update(hall)
    }

    fn delete_hall(&self, hall_id: Uuid) -> Result<()> {
        self.halls().delete(hall_id)
    }

    fn list_halls_for_user(&self, user_id: Uuid) -> Result<Vec<Hall>> {
        self.halls().list_for_user(user_id)
    }

    fn add_member(&self, membership: &Membership) -> Result<()> {
        self.halls().add_member(membership)
    }

    fn get_membership(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<Membership>> {
        self.halls().get_membership(user_id, hall_id)
    }

    fn update_role(&self, user_id: Uuid, hall_id: Uuid, new_role: HallRole) -> Result<()> {
        self.halls().update_role(user_id, hall_id, new_role)
    }

    fn update_online_status(&self, user_id: Uuid, hall_id: Uuid, is_online: bool) -> Result<()> {
        self.halls()
            .update_online_status(user_id, hall_id, is_online)
    }

    fn remove_member(&self, user_id: Uuid, hall_id: Uuid) -> Result<()> {
        self.halls().remove_member(user_id, hall_id)
    }

    fn list_members(&self, hall_id: Uuid) -> Result<Vec<MemberInfo>> {
        self.halls().list_members(hall_id)
    }

    fn get_user_role(&self, user_id: Uuid, hall_id: Uuid) -> Result<Option<HallRole>> {
        self.halls().get_user_role(user_id, hall_id)
    }
}

impl MessageRepository for Database {
    fn create_message(&self, message: &Message) -> Result<()> {
        self.messages().create(message)
    }

    fn find_message_by_id(&self, id: Uuid) -> Result<Option<Message>> {
        self.messages().find_by_id(id)
    }

    fn list_messages_for_hall(
        &self,
        hall_id: Uuid,
        limit: u32,
        before: Option<DateTime<Utc>>,
    ) -> Result<Vec<MessageDisplay>> {
        self.messages().list_for_hall(hall_id, limit, before)
    }

    fn update_message_content(&self, message_id: Uuid, new_content: &str) -> Result<()> {
        self.messages().update_content(message_id, new_content)
    }

    fn delete_message(&self, message_id: Uuid) -> Result<()> {
        self.messages().delete(message_id)
    }

    fn count_messages_for_hall(&self, hall_id: Uuid) -> Result<u64> {
        self.messages().count_for_hall(hall_id)
    }
}

impl InviteRepository for Database {
    fn create_invite(&self, invite: &Invite) -> Result<()> {
        self.invites().create(invite)
    }

    fn find_invite_by_token(&self, token: &str) -> Result<Option<Invite>> {
        self.invites().find_by_token(token)
    }

    fn list_invites_for_hall(&self, hall_id: Uuid) -> Result<Vec<Invite>> {
        self.invites().list_for_hall(hall_id)
    }

    fn increment_use_count(&self, invite_id: Uuid) -> Result<()> {
        self.invites().increment_use_count(invite_id)
    }

    fn revoke_invite(&self, invite_id: Uuid) -> Result<()> {
        self.invites().revoke(invite_id)
    }

    fn delete_invite(&self, invite_id: Uuid) -> Result<()> {
        self.invites().delete(invite_id)
    }
}
