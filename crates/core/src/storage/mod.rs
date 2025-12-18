//! SQLite storage layer for Exom

mod halls;
mod invites;
mod messages;
mod migrations;
mod parse;
mod users;

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use tracing::instrument;

pub use halls::HallStore;
pub use invites::InviteStore;
pub use messages::MessageStore;
pub use users::UserStore;

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
            .query_row(
                "SELECT MAX(version) FROM schema_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap_or(0)
    }

    /// Get user store
    pub fn users(&self) -> UserStore<'_> {
        UserStore::new(&self.conn)
    }

    /// Get hall store
    pub fn halls(&self) -> HallStore<'_> {
        HallStore::new(&self.conn)
    }

    /// Get message store
    pub fn messages(&self) -> MessageStore<'_> {
        MessageStore::new(&self.conn)
    }

    /// Get invite store
    pub fn invites(&self) -> InviteStore<'_> {
        InviteStore::new(&self.conn)
    }
}
