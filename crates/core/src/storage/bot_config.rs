//! Bot configuration storage
//!
//! Stores per-hall bot enablement and configuration overrides.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use uuid::Uuid;

use crate::bot::BotCapability;
use crate::error::Result;

/// Bot enablement record for a hall
#[derive(Debug, Clone)]
pub struct HallBot {
    pub hall_id: Uuid,
    pub bot_id: String,
    pub enabled: bool,
    /// Granted capabilities (subset of manifest capabilities)
    pub granted_capabilities: Vec<BotCapability>,
    pub enabled_at: DateTime<Utc>,
    pub enabled_by: Option<Uuid>,
}

/// Bot configuration for a hall
#[derive(Debug, Clone)]
pub struct HallBotConfig {
    pub hall_id: Uuid,
    pub bot_id: String,
    /// Configuration overrides as key-value pairs
    pub config: HashMap<String, serde_json::Value>,
    pub updated_at: DateTime<Utc>,
    pub updated_by: Option<Uuid>,
}

/// Bot configuration store
pub struct BotConfigStore<'a> {
    conn: &'a Connection,
}

impl<'a> BotConfigStore<'a> {
    pub fn new(conn: &'a Connection) -> Self {
        Self { conn }
    }

    // ==================== Hall Bot Enablement ====================

    /// Enable a bot for a hall with specific capabilities
    pub fn enable_bot(
        &self,
        hall_id: Uuid,
        bot_id: &str,
        capabilities: &[BotCapability],
        enabled_by: Option<Uuid>,
    ) -> Result<()> {
        let caps_str = serialize_capabilities(capabilities);
        let now = Utc::now();

        self.conn.execute(
            "INSERT INTO hall_bots (hall_id, bot_id, enabled, granted_capabilities, enabled_at, enabled_by)
             VALUES (?1, ?2, 1, ?3, ?4, ?5)
             ON CONFLICT(hall_id, bot_id) DO UPDATE SET
                enabled = 1,
                granted_capabilities = ?3,
                enabled_at = ?4,
                enabled_by = ?5",
            params![
                hall_id.to_string(),
                bot_id,
                caps_str,
                now.to_rfc3339(),
                enabled_by.map(|u| u.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Disable a bot for a hall (keeps record but sets enabled=0)
    pub fn disable_bot(&self, hall_id: Uuid, bot_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE hall_bots SET enabled = 0 WHERE hall_id = ?1 AND bot_id = ?2",
            params![hall_id.to_string(), bot_id],
        )?;
        Ok(())
    }

    /// Check if a bot is enabled for a hall
    pub fn is_enabled(&self, hall_id: Uuid, bot_id: &str) -> Result<bool> {
        let result: Option<bool> = self
            .conn
            .query_row(
                "SELECT enabled FROM hall_bots WHERE hall_id = ?1 AND bot_id = ?2",
                params![hall_id.to_string(), bot_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result.unwrap_or(false))
    }

    /// Get a hall bot record
    pub fn get_hall_bot(&self, hall_id: Uuid, bot_id: &str) -> Result<Option<HallBot>> {
        let result = self
            .conn
            .query_row(
                "SELECT hall_id, bot_id, enabled, granted_capabilities, enabled_at, enabled_by
                 FROM hall_bots WHERE hall_id = ?1 AND bot_id = ?2",
                params![hall_id.to_string(), bot_id],
                |row| {
                    Ok(HallBot {
                        hall_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                        bot_id: row.get(1)?,
                        enabled: row.get(2)?,
                        granted_capabilities: deserialize_capabilities(&row.get::<_, Option<String>>(3)?),
                        enabled_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        enabled_by: row
                            .get::<_, Option<String>>(5)?
                            .and_then(|s| Uuid::parse_str(&s).ok()),
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// List all enabled bots for a hall
    pub fn list_enabled_for_hall(&self, hall_id: Uuid) -> Result<Vec<HallBot>> {
        let mut stmt = self.conn.prepare(
            "SELECT hall_id, bot_id, enabled, granted_capabilities, enabled_at, enabled_by
             FROM hall_bots WHERE hall_id = ?1 AND enabled = 1",
        )?;

        let rows = stmt.query_map(params![hall_id.to_string()], |row| {
            Ok(HallBot {
                hall_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                bot_id: row.get(1)?,
                enabled: row.get(2)?,
                granted_capabilities: deserialize_capabilities(&row.get::<_, Option<String>>(3)?),
                enabled_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(4)?)
                    .unwrap()
                    .with_timezone(&Utc),
                enabled_by: row
                    .get::<_, Option<String>>(5)?
                    .and_then(|s| Uuid::parse_str(&s).ok()),
            })
        })?;

        let mut bots = Vec::new();
        for row in rows {
            bots.push(row?);
        }
        Ok(bots)
    }

    /// List all halls where a bot is enabled
    pub fn list_halls_for_bot(&self, bot_id: &str) -> Result<Vec<Uuid>> {
        let mut stmt = self.conn.prepare(
            "SELECT hall_id FROM hall_bots WHERE bot_id = ?1 AND enabled = 1",
        )?;

        let rows = stmt.query_map(params![bot_id], |row| {
            Ok(Uuid::parse_str(&row.get::<_, String>(0)?).unwrap())
        })?;

        let mut halls = Vec::new();
        for row in rows {
            halls.push(row?);
        }
        Ok(halls)
    }

    /// Update granted capabilities for a bot in a hall
    pub fn update_capabilities(
        &self,
        hall_id: Uuid,
        bot_id: &str,
        capabilities: &[BotCapability],
    ) -> Result<()> {
        let caps_str = serialize_capabilities(capabilities);
        self.conn.execute(
            "UPDATE hall_bots SET granted_capabilities = ?3 WHERE hall_id = ?1 AND bot_id = ?2",
            params![hall_id.to_string(), bot_id, caps_str],
        )?;
        Ok(())
    }

    /// Check if a bot has a specific capability granted in a hall
    pub fn has_capability(&self, hall_id: Uuid, bot_id: &str, cap: BotCapability) -> Result<bool> {
        if let Some(hall_bot) = self.get_hall_bot(hall_id, bot_id)? {
            Ok(hall_bot.enabled && hall_bot.granted_capabilities.contains(&cap))
        } else {
            Ok(false)
        }
    }

    // ==================== Bot Configuration ====================

    /// Get configuration for a bot in a hall
    pub fn get_config(&self, hall_id: Uuid, bot_id: &str) -> Result<Option<HallBotConfig>> {
        let result = self
            .conn
            .query_row(
                "SELECT hall_id, bot_id, config_json, updated_at, updated_by
                 FROM hall_bot_config WHERE hall_id = ?1 AND bot_id = ?2",
                params![hall_id.to_string(), bot_id],
                |row| {
                    let config_json: String = row.get(2)?;
                    let config: HashMap<String, serde_json::Value> =
                        serde_json::from_str(&config_json).unwrap_or_default();

                    Ok(HallBotConfig {
                        hall_id: Uuid::parse_str(&row.get::<_, String>(0)?).unwrap(),
                        bot_id: row.get(1)?,
                        config,
                        updated_at: DateTime::parse_from_rfc3339(&row.get::<_, String>(3)?)
                            .unwrap()
                            .with_timezone(&Utc),
                        updated_by: row
                            .get::<_, Option<String>>(4)?
                            .and_then(|s| Uuid::parse_str(&s).ok()),
                    })
                },
            )
            .optional()?;
        Ok(result)
    }

    /// Set configuration for a bot in a hall (replaces existing)
    pub fn set_config(
        &self,
        hall_id: Uuid,
        bot_id: &str,
        config: &HashMap<String, serde_json::Value>,
        updated_by: Option<Uuid>,
    ) -> Result<()> {
        let config_json = serde_json::to_string(config).unwrap_or_else(|_| "{}".to_string());
        let now = Utc::now();

        self.conn.execute(
            "INSERT INTO hall_bot_config (hall_id, bot_id, config_json, updated_at, updated_by)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(hall_id, bot_id) DO UPDATE SET
                config_json = ?3,
                updated_at = ?4,
                updated_by = ?5",
            params![
                hall_id.to_string(),
                bot_id,
                config_json,
                now.to_rfc3339(),
                updated_by.map(|u| u.to_string()),
            ],
        )?;
        Ok(())
    }

    /// Update a single config value for a bot in a hall
    pub fn set_config_value(
        &self,
        hall_id: Uuid,
        bot_id: &str,
        key: &str,
        value: serde_json::Value,
        updated_by: Option<Uuid>,
    ) -> Result<()> {
        let mut config = self
            .get_config(hall_id, bot_id)?
            .map(|c| c.config)
            .unwrap_or_default();

        config.insert(key.to_string(), value);
        self.set_config(hall_id, bot_id, &config, updated_by)
    }

    /// Get a single config value for a bot in a hall
    pub fn get_config_value(
        &self,
        hall_id: Uuid,
        bot_id: &str,
        key: &str,
    ) -> Result<Option<serde_json::Value>> {
        if let Some(config) = self.get_config(hall_id, bot_id)? {
            Ok(config.config.get(key).cloned())
        } else {
            Ok(None)
        }
    }

    /// Delete configuration for a bot in a hall
    pub fn delete_config(&self, hall_id: Uuid, bot_id: &str) -> Result<()> {
        self.conn.execute(
            "DELETE FROM hall_bot_config WHERE hall_id = ?1 AND bot_id = ?2",
            params![hall_id.to_string(), bot_id],
        )?;
        Ok(())
    }

    /// Delete all bot data for a hall (used when deleting hall)
    pub fn delete_hall_bots(&self, hall_id: Uuid) -> Result<()> {
        self.conn.execute(
            "DELETE FROM hall_bots WHERE hall_id = ?1",
            params![hall_id.to_string()],
        )?;
        self.conn.execute(
            "DELETE FROM hall_bot_config WHERE hall_id = ?1",
            params![hall_id.to_string()],
        )?;
        Ok(())
    }
}

/// Serialize capabilities to comma-separated string
fn serialize_capabilities(caps: &[BotCapability]) -> Option<String> {
    if caps.is_empty() {
        None
    } else {
        Some(
            caps.iter()
                .map(|c| format!("{:?}", c))
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

/// Deserialize capabilities from comma-separated string
fn deserialize_capabilities(s: &Option<String>) -> Vec<BotCapability> {
    match s {
        None => Vec::new(),
        Some(s) if s.is_empty() => Vec::new(),
        Some(s) => s
            .split(',')
            .filter_map(|cap_str| {
                let cap_str = cap_str.trim();
                match cap_str {
                    // Event Listening
                    "ListenPresence" => Some(BotCapability::ListenPresence),
                    "ListenChat" => Some(BotCapability::ListenChat),
                    "ListenChest" => Some(BotCapability::ListenChest),
                    // Message Emission
                    "EmitSystem" => Some(BotCapability::EmitSystem),
                    "EmitChat" => Some(BotCapability::EmitChat),
                    // File Operations
                    "ReadChest" => Some(BotCapability::ReadChest),
                    "WriteChest" => Some(BotCapability::WriteChest),
                    "DeleteChest" => Some(BotCapability::DeleteChest),
                    // Moderation
                    "RequestKick" => Some(BotCapability::RequestKick),
                    "RequestMute" => Some(BotCapability::RequestMute),
                    "RequestRoleChange" => Some(BotCapability::RequestRoleChange),
                    // Workspace
                    "WorkspaceTabs" => Some(BotCapability::WorkspaceTabs),
                    // External Tools
                    "SpawnExternalTools" => Some(BotCapability::SpawnExternalTools),
                    // Notifications
                    "Notifications" => Some(BotCapability::Notifications),
                    // Scheduling
                    "ReceiveScheduledTick" => Some(BotCapability::ReceiveScheduledTick),
                    "ScheduleTimers" => Some(BotCapability::ScheduleTimers),
                    // Commands
                    "HandleCommands" => Some(BotCapability::HandleCommands),
                    // Chat History
                    "ReadChatHistory" => Some(BotCapability::ReadChatHistory),
                    // External Integration
                    "HttpClient" => Some(BotCapability::HttpClient),
                    "ExternalSignals" => Some(BotCapability::ExternalSignals),
                    _ => None,
                }
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Hall, User};
    use crate::storage::Database;

    /// Create a test hall and return its ID
    fn create_test_hall(db: &Database) -> Uuid {
        let user_id = Uuid::new_v4();
        let user = User::new("testuser".to_string(), "password_hash".to_string());
        db.users().create(&User { id: user_id, ..user }).unwrap();

        let hall_id = Uuid::new_v4();
        let hall = Hall::new("Test Hall".to_string(), user_id);
        db.halls().create(&Hall { id: hall_id, ..hall }).unwrap();

        hall_id
    }

    #[test]
    fn test_enable_disable_bot() {
        let db = Database::open_in_memory().unwrap();
        let hall_id = create_test_hall(&db);
        let store = db.bot_config();

        // Initially not enabled
        assert!(!store.is_enabled(hall_id, "test-bot").unwrap());

        // Enable
        store
            .enable_bot(
                hall_id,
                "test-bot",
                &[BotCapability::ListenPresence, BotCapability::EmitSystem],
                None,
            )
            .unwrap();
        assert!(store.is_enabled(hall_id, "test-bot").unwrap());

        // Check capabilities
        let hall_bot = store.get_hall_bot(hall_id, "test-bot").unwrap().unwrap();
        assert_eq!(hall_bot.granted_capabilities.len(), 2);
        assert!(hall_bot.granted_capabilities.contains(&BotCapability::ListenPresence));
        assert!(hall_bot.granted_capabilities.contains(&BotCapability::EmitSystem));

        // Disable
        store.disable_bot(hall_id, "test-bot").unwrap();
        assert!(!store.is_enabled(hall_id, "test-bot").unwrap());
    }

    #[test]
    fn test_bot_config() {
        let db = Database::open_in_memory().unwrap();
        let hall_id = create_test_hall(&db);
        let store = db.bot_config();

        // No config initially
        assert!(store.get_config(hall_id, "test-bot").unwrap().is_none());

        // Set config
        let mut config = HashMap::new();
        config.insert("rate_limit".to_string(), serde_json::json!(10));
        config.insert("enabled_feature".to_string(), serde_json::json!(true));
        store.set_config(hall_id, "test-bot", &config, None).unwrap();

        // Get config
        let result = store.get_config(hall_id, "test-bot").unwrap().unwrap();
        assert_eq!(result.config.get("rate_limit"), Some(&serde_json::json!(10)));
        assert_eq!(result.config.get("enabled_feature"), Some(&serde_json::json!(true)));

        // Update single value
        store
            .set_config_value(hall_id, "test-bot", "rate_limit", serde_json::json!(20), None)
            .unwrap();
        let value = store.get_config_value(hall_id, "test-bot", "rate_limit").unwrap();
        assert_eq!(value, Some(serde_json::json!(20)));
    }

    #[test]
    fn test_list_enabled_bots() {
        let db = Database::open_in_memory().unwrap();
        let hall_id = create_test_hall(&db);
        let store = db.bot_config();

        store.enable_bot(hall_id, "bot-a", &[], None).unwrap();
        store.enable_bot(hall_id, "bot-b", &[], None).unwrap();
        store.enable_bot(hall_id, "bot-c", &[], None).unwrap();
        store.disable_bot(hall_id, "bot-b").unwrap();

        let enabled = store.list_enabled_for_hall(hall_id).unwrap();
        assert_eq!(enabled.len(), 2);

        let bot_ids: Vec<_> = enabled.iter().map(|b| b.bot_id.as_str()).collect();
        assert!(bot_ids.contains(&"bot-a"));
        assert!(bot_ids.contains(&"bot-c"));
        assert!(!bot_ids.contains(&"bot-b"));
    }

    #[test]
    fn test_capability_check() {
        let db = Database::open_in_memory().unwrap();
        let hall_id = create_test_hall(&db);
        let store = db.bot_config();

        store
            .enable_bot(hall_id, "test-bot", &[BotCapability::ListenPresence], None)
            .unwrap();

        assert!(store.has_capability(hall_id, "test-bot", BotCapability::ListenPresence).unwrap());
        assert!(!store.has_capability(hall_id, "test-bot", BotCapability::EmitSystem).unwrap());
    }
}
