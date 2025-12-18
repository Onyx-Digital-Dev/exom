//! Database schema definitions

use rusqlite::Connection;
use crate::error::Result;

pub fn create_tables(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        -- Users table
        CREATE TABLE IF NOT EXISTS users (
            id TEXT PRIMARY KEY,
            username TEXT NOT NULL UNIQUE,
            password_hash TEXT NOT NULL,
            created_at TEXT NOT NULL,
            last_login TEXT
        );

        -- Sessions table
        CREATE TABLE IF NOT EXISTS sessions (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT NOT NULL,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
        );

        -- Halls table
        CREATE TABLE IF NOT EXISTS halls (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            description TEXT,
            owner_id TEXT NOT NULL,
            created_at TEXT NOT NULL,
            active_parlor TEXT,
            current_host_id TEXT,
            election_epoch INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (owner_id) REFERENCES users(id)
        );

        -- Memberships table
        CREATE TABLE IF NOT EXISTS memberships (
            id TEXT PRIMARY KEY,
            user_id TEXT NOT NULL,
            hall_id TEXT NOT NULL,
            role INTEGER NOT NULL,
            joined_at TEXT NOT NULL,
            is_online INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
            FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
            UNIQUE(user_id, hall_id)
        );

        -- Messages table
        CREATE TABLE IF NOT EXISTS messages (
            id TEXT PRIMARY KEY,
            hall_id TEXT NOT NULL,
            sender_id TEXT NOT NULL,
            content TEXT NOT NULL,
            created_at TEXT NOT NULL,
            edited_at TEXT,
            is_deleted INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
            FOREIGN KEY (sender_id) REFERENCES users(id)
        );

        -- Invites table
        CREATE TABLE IF NOT EXISTS invites (
            id TEXT PRIMARY KEY,
            hall_id TEXT NOT NULL,
            token TEXT NOT NULL UNIQUE,
            created_by TEXT NOT NULL,
            role INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            expires_at TEXT,
            max_uses INTEGER,
            use_count INTEGER NOT NULL DEFAULT 0,
            is_revoked INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
            FOREIGN KEY (created_by) REFERENCES users(id)
        );

        -- Indexes
        CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
        CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);
        CREATE INDEX IF NOT EXISTS idx_memberships_user ON memberships(user_id);
        CREATE INDEX IF NOT EXISTS idx_memberships_hall ON memberships(hall_id);
        CREATE INDEX IF NOT EXISTS idx_messages_hall ON messages(hall_id);
        CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);
        CREATE INDEX IF NOT EXISTS idx_invites_token ON invites(token);
        CREATE INDEX IF NOT EXISTS idx_invites_hall ON invites(hall_id);
        "#,
    )?;

    Ok(())
}
