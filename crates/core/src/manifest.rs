//! Bot manifest schema and loader
//!
//! Defines the TOML-parseable manifest format for bot packages.
//! Bots are loaded from `bots/<bot_id>/<bot_id>.toml`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::bot::BotCapability;

/// Bot manifest loaded from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotManifestToml {
    /// Bot package metadata (includes capabilities)
    pub bot: BotMeta,
    /// Configuration schema (default values and types)
    #[serde(default)]
    pub config: HashMap<String, ConfigField>,
    /// Slash commands this bot handles
    #[serde(default)]
    pub commands: Vec<CommandDef>,
    /// Asset paths (relative to bot folder)
    #[serde(default)]
    pub assets: Vec<String>,
}

impl BotManifestToml {
    /// Get capabilities (delegates to BotMeta)
    pub fn capabilities(&self) -> &[BotCapability] {
        &self.bot.capabilities
    }
}

/// Bot metadata section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotMeta {
    /// Unique identifier (must match folder name)
    pub id: String,
    /// Display name
    pub name: String,
    /// Semantic version
    pub version: String,
    /// Author name or organization
    #[serde(default)]
    pub author: Option<String>,
    /// Short description
    #[serde(default)]
    pub description: Option<String>,
    /// Bot category for UI grouping
    #[serde(default)]
    pub category: Option<BotCategory>,
    /// Required capabilities
    #[serde(default)]
    pub capabilities: Vec<BotCapability>,
}

/// Bot categories for UI organization
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum BotCategory {
    /// System/utility bots
    #[default]
    Utility,
    /// Presence and announcement bots
    Presence,
    /// Moderation bots
    Moderation,
    /// Integration bots (external services)
    Integration,
    /// Entertainment/fun bots
    Entertainment,
}

/// Configuration field definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigField {
    /// Field type
    #[serde(rename = "type")]
    pub field_type: ConfigFieldType,
    /// Default value (as JSON-compatible value)
    #[serde(default)]
    pub default: Option<toml::Value>,
    /// Human-readable description
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this field is required
    #[serde(default)]
    pub required: bool,
    /// Allowed values for enum types
    #[serde(default)]
    pub options: Vec<String>,
}

/// Configuration field types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConfigFieldType {
    String,
    Integer,
    Float,
    Boolean,
    /// One of predefined options
    Enum,
    /// Time in HH:MM format
    Time,
    /// Duration in seconds
    Duration,
}

/// Slash command definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandDef {
    /// Command prefix (e.g., "/archive")
    pub prefix: String,
    /// Short description for help
    pub description: String,
    /// Usage pattern (e.g., "/archive-set-time <HH:MM>")
    #[serde(default)]
    pub usage: Option<String>,
    /// Required capability to use this command
    #[serde(default)]
    pub requires_capability: Option<BotCapability>,
}

/// Error type for manifest loading
#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("Failed to read manifest file: {0}")]
    IoError(#[from] std::io::Error),
    #[error("Failed to parse manifest TOML: {0}")]
    ParseError(#[from] toml::de::Error),
    #[error("Manifest ID '{found}' does not match folder name '{expected}'")]
    IdMismatch { expected: String, found: String },
    #[error("Bot folder not found: {0}")]
    FolderNotFound(PathBuf),
    #[error("Manifest file not found: {0}")]
    ManifestNotFound(PathBuf),
}

impl BotManifestToml {
    /// Load a manifest from a bot folder
    ///
    /// Expects: `<bots_dir>/<bot_id>/<bot_id>.toml`
    pub fn load_from_folder(bot_folder: &Path) -> Result<Self, ManifestError> {
        if !bot_folder.exists() {
            return Err(ManifestError::FolderNotFound(bot_folder.to_path_buf()));
        }

        let bot_id = bot_folder
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let manifest_path = bot_folder.join(format!("{}.toml", bot_id));
        if !manifest_path.exists() {
            return Err(ManifestError::ManifestNotFound(manifest_path));
        }

        let content = std::fs::read_to_string(&manifest_path)?;
        let manifest: BotManifestToml = toml::from_str(&content)?;

        // Validate ID matches folder name
        if manifest.bot.id != bot_id {
            return Err(ManifestError::IdMismatch {
                expected: bot_id.to_string(),
                found: manifest.bot.id.clone(),
            });
        }

        Ok(manifest)
    }

    /// Load a manifest directly from TOML content (for testing)
    pub fn from_toml(content: &str) -> Result<Self, ManifestError> {
        Ok(toml::from_str(content)?)
    }

    /// Get command prefixes for routing
    pub fn command_prefixes(&self) -> Vec<&str> {
        self.commands.iter().map(|c| c.prefix.as_str()).collect()
    }

    /// Check if bot has a capability
    pub fn has_capability(&self, cap: BotCapability) -> bool {
        self.bot.capabilities.contains(&cap)
    }

    /// Get the bot folder path from a bots directory
    pub fn bot_folder(bots_dir: &Path, bot_id: &str) -> PathBuf {
        bots_dir.join(bot_id)
    }
}

/// Discover all bot manifests in a directory
pub fn discover_bots(bots_dir: &Path) -> Vec<Result<BotManifestToml, ManifestError>> {
    let mut results = Vec::new();

    if !bots_dir.exists() {
        return results;
    }

    let entries = match std::fs::read_dir(bots_dir) {
        Ok(e) => e,
        Err(_) => return results,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            results.push(BotManifestToml::load_from_folder(&path));
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_minimal_manifest() {
        let toml = r#"
[bot]
id = "test-bot"
name = "Test Bot"
version = "1.0.0"
"#;
        let manifest = BotManifestToml::from_toml(toml).unwrap();
        assert_eq!(manifest.bot.id, "test-bot");
        assert_eq!(manifest.bot.name, "Test Bot");
        assert_eq!(manifest.bot.version, "1.0.0");
        assert!(manifest.bot.capabilities.is_empty());
        assert!(manifest.commands.is_empty());
    }

    #[test]
    fn test_parse_full_manifest() {
        let toml = r#"
[bot]
id = "town-crier"
name = "Town Crier"
version = "1.0.0"
author = "Exom Team"
description = "Announces member presence changes"
category = "presence"

capabilities = ["ListenPresence", "EmitSystem"]

[config]
[config.rate_limit_seconds]
type = "integer"
default = 10
description = "Minimum seconds between announcements per user"

[config.announce_joins]
type = "boolean"
default = true
description = "Announce when members join"

[[commands]]
prefix = "/crier-quiet"
description = "Mute announcements for 30 minutes"
"#;
        let manifest = BotManifestToml::from_toml(toml).unwrap();
        assert_eq!(manifest.bot.id, "town-crier");
        assert_eq!(manifest.bot.category, Some(BotCategory::Presence));
        assert_eq!(manifest.bot.capabilities.len(), 2);
        assert!(manifest.has_capability(BotCapability::ListenPresence));
        assert!(manifest.has_capability(BotCapability::EmitSystem));
        assert_eq!(manifest.config.len(), 2);
        assert_eq!(manifest.commands.len(), 1);
        assert_eq!(manifest.commands[0].prefix, "/crier-quiet");
    }

    #[test]
    fn test_command_prefixes() {
        let toml = r#"
[bot]
id = "multi-cmd"
name = "Multi Command"
version = "1.0.0"

[[commands]]
prefix = "/cmd1"
description = "First command"

[[commands]]
prefix = "/cmd2"
description = "Second command"
"#;
        let manifest = BotManifestToml::from_toml(toml).unwrap();
        let prefixes = manifest.command_prefixes();
        assert_eq!(prefixes, vec!["/cmd1", "/cmd2"]);
    }
}
