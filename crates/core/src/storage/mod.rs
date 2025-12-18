//! SQLite storage layer for Exom

mod halls;
mod invites;
mod messages;
mod parse;
mod schema;
mod users;

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;

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
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Open in-memory database (for testing)
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self { conn };
        db.init()?;
        Ok(db)
    }

    /// Initialize database schema
    fn init(&self) -> Result<()> {
        schema::create_tables(&self.conn)?;
        Ok(())
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
