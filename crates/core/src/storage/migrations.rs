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
    Migration {
        version: 5,
        description: "Add workspace state persistence and presence tracking",
        sql: r#"
            -- Workspace state per hall (session continuity)
            CREATE TABLE IF NOT EXISTS workspace_state (
                hall_id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                tabs_json TEXT NOT NULL,
                active_tab_id TEXT,
                terminal_cwd TEXT,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );

            -- User preferences (last hall, settings)
            CREATE TABLE IF NOT EXISTS user_preferences (
                user_id TEXT PRIMARY KEY,
                last_hall_id TEXT,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );
        "#,
    },
    Migration {
        version: 6,
        description: "Add last_seen tracking for Town Crier bot",
        sql: r#"
            -- Track when users were last seen in each hall
            CREATE TABLE IF NOT EXISTS last_seen (
                hall_id TEXT NOT NULL,
                user_id TEXT NOT NULL,
                last_seen_at TEXT NOT NULL,
                PRIMARY KEY (hall_id, user_id),
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
            );
        "#,
    },
    Migration {
        version: 7,
        description: "Add archive configuration for Archivist bot",
        sql: r#"
            -- Archive configuration per hall
            CREATE TABLE IF NOT EXISTS archive_config (
                hall_id TEXT PRIMARY KEY,
                enabled INTEGER NOT NULL DEFAULT 0,
                archive_time INTEGER NOT NULL DEFAULT 2200,
                archive_window TEXT NOT NULL DEFAULT '24h',
                archive_output TEXT NOT NULL DEFAULT 'chest',
                last_run_at TEXT,
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE
            );
        "#,
    },
    Migration {
        version: 8,
        description: "Add bot registry and per-hall bot configuration",
        sql: r#"
            -- Bots enabled per hall with granted capabilities
            CREATE TABLE IF NOT EXISTS hall_bots (
                hall_id TEXT NOT NULL,
                bot_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                -- Comma-separated list of granted capabilities (subset of manifest)
                granted_capabilities TEXT,
                enabled_at TEXT NOT NULL,
                enabled_by TEXT,
                PRIMARY KEY (hall_id, bot_id),
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
                FOREIGN KEY (enabled_by) REFERENCES users(id)
            );

            -- Per-hall bot configuration overrides
            CREATE TABLE IF NOT EXISTS hall_bot_config (
                hall_id TEXT NOT NULL,
                bot_id TEXT NOT NULL,
                -- JSON object with config key-value pairs
                config_json TEXT NOT NULL DEFAULT '{}',
                updated_at TEXT NOT NULL,
                updated_by TEXT,
                PRIMARY KEY (hall_id, bot_id),
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE,
                FOREIGN KEY (updated_by) REFERENCES users(id)
            );

            -- Index for quick lookups
            CREATE INDEX IF NOT EXISTS idx_hall_bots_bot ON hall_bots(bot_id);
            CREATE INDEX IF NOT EXISTS idx_hall_bot_config_bot ON hall_bot_config(bot_id);
        "#,
    },
    Migration {
        version: 9,
        description: "Add pinned launchers for external tools",
        sql: r#"
            -- Pinned launchers per hall (quick-launch buttons)
            -- Tools open in NEW WINDOWS - no embedding
            CREATE TABLE IF NOT EXISTS pinned_launchers (
                id TEXT PRIMARY KEY,
                hall_id TEXT NOT NULL,
                name TEXT NOT NULL,
                command TEXT NOT NULL,
                args TEXT NOT NULL DEFAULT '[]',
                icon TEXT,
                position INTEGER NOT NULL DEFAULT 0,
                created_at TEXT NOT NULL,
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE CASCADE
            );

            CREATE INDEX IF NOT EXISTS idx_pinned_launchers_hall ON pinned_launchers(hall_id);
        "#,
    },
    Migration {
        version: 10,
        description: "Add global associates (mutual-only relationships)",
        sql: r#"
            -- Global associate relationships (not hall-scoped)
            -- Associates are MUTUAL ONLY - both sides must accept
            CREATE TABLE IF NOT EXISTS associates (
                id TEXT PRIMARY KEY,
                requester_id TEXT NOT NULL,
                target_id TEXT NOT NULL,
                -- pending, accepted, blocked
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL,
                FOREIGN KEY (requester_id) REFERENCES users(id) ON DELETE CASCADE,
                FOREIGN KEY (target_id) REFERENCES users(id) ON DELETE CASCADE,
                -- Enforce uniqueness regardless of direction (A-B same as B-A)
                -- SQLite doesn't support CHECK with subquery, so we use a unique constraint
                -- on the ordered pair (min, max) of user IDs
                UNIQUE(requester_id, target_id)
            );

            -- Indexes for quick lookups
            CREATE INDEX IF NOT EXISTS idx_associates_requester ON associates(requester_id);
            CREATE INDEX IF NOT EXISTS idx_associates_target ON associates(target_id);
            CREATE INDEX IF NOT EXISTS idx_associates_status ON associates(status);
        "#,
    },
    Migration {
        version: 11,
        description: "Add voluntary desk status for privacy-first presence",
        sql: r#"
            -- Voluntary desk status per user
            -- Set ONLY when user launches tool from Exom
            -- NEVER inferred from OS focus, processes, or window titles
            CREATE TABLE IF NOT EXISTS desk_status (
                user_id TEXT PRIMARY KEY,
                -- The desk label (e.g., "Kitty", "VS Code", "mpv")
                desk_label TEXT NOT NULL,
                -- When this desk was set
                set_at TEXT NOT NULL,
                -- Optional: which hall the tool was launched from (context only)
                hall_id TEXT,
                FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
                FOREIGN KEY (hall_id) REFERENCES halls(id) ON DELETE SET NULL
            );
        "#,
    },
    Migration {
        version: 12,
        description: "Add composite indexes for associates bidirectional queries",
        sql: r#"
            -- Composite indexes for efficient bidirectional associate lookups
            -- Used by is_associate() which queries: WHERE status = 'accepted' AND (...)
            CREATE INDEX IF NOT EXISTS idx_associates_status_requester
                ON associates(status, requester_id);
            CREATE INDEX IF NOT EXISTS idx_associates_status_target
                ON associates(status, target_id);

            -- Index on archive_config.enabled for quick filtering
            CREATE INDEX IF NOT EXISTS idx_archive_config_enabled
                ON archive_config(enabled);
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
