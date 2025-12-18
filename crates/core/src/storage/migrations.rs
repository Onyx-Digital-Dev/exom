//! Database migration system
//!
//! Tracks schema versions and applies migrations in order.

use rusqlite::Connection;
use tracing::{info, instrument};

use crate::error::Result;

/// A database migration
pub struct Migration {
    /// Version number (must be sequential starting from 1)
    pub version: u32,
    /// Description of what this migration does
    pub description: &'static str,
    /// SQL to run for this migration
    pub sql: &'static str,
}

/// All migrations in order
const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        description: "Initial schema",
        sql: r#"
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
        "#,
    },
    Migration {
        version: 2,
        description: "Add indexes for query performance",
        sql: r#"
            -- Session indexes
            CREATE INDEX IF NOT EXISTS idx_sessions_user ON sessions(user_id);
            CREATE INDEX IF NOT EXISTS idx_sessions_expires ON sessions(expires_at);

            -- Membership indexes
            CREATE INDEX IF NOT EXISTS idx_memberships_user ON memberships(user_id);
            CREATE INDEX IF NOT EXISTS idx_memberships_hall ON memberships(hall_id);

            -- Message indexes
            CREATE INDEX IF NOT EXISTS idx_messages_hall ON messages(hall_id);
            CREATE INDEX IF NOT EXISTS idx_messages_created ON messages(created_at);
            CREATE INDEX IF NOT EXISTS idx_messages_hall_created ON messages(hall_id, created_at);
            CREATE INDEX IF NOT EXISTS idx_messages_sender ON messages(sender_id);

            -- Invite indexes
            CREATE INDEX IF NOT EXISTS idx_invites_token ON invites(token);
            CREATE INDEX IF NOT EXISTS idx_invites_hall ON invites(hall_id);
        "#,
    },
    Migration {
        version: 3,
        description: "Add last_connection table for auto-reconnect",
        sql: r#"
            -- Track last successful connection per user for auto-reconnect
            CREATE TABLE IF NOT EXISTS last_connections (
                user_id TEXT PRIMARY KEY,
                hall_id TEXT NOT NULL,
                invite_url TEXT NOT NULL,
                host_addr TEXT,
                last_connected_at TEXT NOT NULL,
                epoch INTEGER NOT NULL DEFAULT 0,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );
        "#,
    },
    Migration {
        version: 4,
        description: "Add sequence column to messages for deterministic ordering",
        sql: r#"
            -- Add sequence column for network message ordering
            ALTER TABLE messages ADD COLUMN sequence INTEGER;

            -- Index for efficient ordering
            CREATE INDEX IF NOT EXISTS idx_messages_sequence ON messages(hall_id, sequence);
        "#,
    },
];

/// Initialize the migrations table
fn init_migrations_table(conn: &Connection) -> Result<()> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
            version INTEGER PRIMARY KEY,
            description TEXT NOT NULL,
            applied_at TEXT NOT NULL
        )",
        [],
    )?;
    Ok(())
}

/// Get the current schema version
fn get_current_version(conn: &Connection) -> Result<u32> {
    let version: Option<u32> = conn
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap_or(None);
    Ok(version.unwrap_or(0))
}

/// Record that a migration was applied
fn record_migration(conn: &Connection, migration: &Migration) -> Result<()> {
    conn.execute(
        "INSERT INTO schema_migrations (version, description, applied_at) VALUES (?1, ?2, ?3)",
        rusqlite::params![
            migration.version,
            migration.description,
            chrono::Utc::now().to_rfc3339()
        ],
    )?;
    Ok(())
}

/// Run all pending migrations
#[instrument(skip(conn))]
pub fn run_migrations(conn: &Connection) -> Result<()> {
    init_migrations_table(conn)?;

    let current_version = get_current_version(conn)?;
    info!(current_version, "Checking for pending migrations");

    for migration in MIGRATIONS {
        if migration.version > current_version {
            info!(
                version = migration.version,
                description = migration.description,
                "Applying migration"
            );

            conn.execute_batch(migration.sql)?;
            record_migration(conn, migration)?;

            info!(version = migration.version, "Migration complete");
        }
    }

    let new_version = get_current_version(conn)?;
    if new_version > current_version {
        info!(
            from = current_version,
            to = new_version,
            "Database schema updated"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Get the latest migration version (test helper)
    fn latest_version() -> u32 {
        MIGRATIONS.last().map(|m| m.version).unwrap_or(0)
    }

    #[test]
    fn test_migrations_run() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn).unwrap();

        let version = get_current_version(&conn).unwrap();
        assert_eq!(version, latest_version());
    }

    #[test]
    fn test_migrations_idempotent() {
        let conn = Connection::open_in_memory().unwrap();

        // Run twice
        run_migrations(&conn).unwrap();
        run_migrations(&conn).unwrap();

        let version = get_current_version(&conn).unwrap();
        assert_eq!(version, latest_version());
    }

    #[test]
    fn test_migrations_sequential() {
        // Verify migrations are numbered sequentially
        for (i, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                migration.version as usize,
                i + 1,
                "Migration {} should have version {}",
                migration.description,
                i + 1
            );
        }
    }
}
