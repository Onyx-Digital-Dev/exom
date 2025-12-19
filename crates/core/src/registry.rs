//! Bot registry - discovers and manages bot manifests
//!
//! The registry scans the bots directory, loads manifests, and provides
//! lookup methods for the runtime.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::manifest::{discover_bots, BotManifestToml, ManifestError};
use crate::BotCapability;

/// Bot registry - central store of available bot manifests
#[derive(Debug)]
pub struct BotRegistry {
    /// Directory containing bot packages
    bots_dir: PathBuf,
    /// Loaded manifests by bot ID
    manifests: HashMap<String, BotManifestToml>,
    /// Load errors for diagnostics
    load_errors: Vec<(PathBuf, ManifestError)>,
}

impl BotRegistry {
    /// Create an empty registry
    pub fn new(bots_dir: PathBuf) -> Self {
        Self {
            bots_dir,
            manifests: HashMap::new(),
            load_errors: Vec::new(),
        }
    }

    /// Create and scan a directory for bots
    pub fn scan(bots_dir: PathBuf) -> Self {
        let mut registry = Self::new(bots_dir.clone());
        registry.rescan();
        registry
    }

    /// Rescan the bots directory for new or changed manifests
    pub fn rescan(&mut self) {
        self.manifests.clear();
        self.load_errors.clear();

        for result in discover_bots(&self.bots_dir) {
            match result {
                Ok(manifest) => {
                    let id = manifest.bot.id.clone();
                    tracing::info!(bot_id = %id, "Registered bot from manifest");
                    self.manifests.insert(id, manifest);
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to load bot manifest");
                    // Try to extract the path from the error for diagnostics
                    let path = match &e {
                        ManifestError::FolderNotFound(p) => p.clone(),
                        ManifestError::ManifestNotFound(p) => p.clone(),
                        _ => PathBuf::from("unknown"),
                    };
                    self.load_errors.push((path, e));
                }
            }
        }
    }

    /// Get a manifest by bot ID
    pub fn get(&self, bot_id: &str) -> Option<&BotManifestToml> {
        self.manifests.get(bot_id)
    }

    /// Check if a bot is registered
    pub fn contains(&self, bot_id: &str) -> bool {
        self.manifests.contains_key(bot_id)
    }

    /// Get all registered bot IDs
    pub fn bot_ids(&self) -> impl Iterator<Item = &str> {
        self.manifests.keys().map(|s| s.as_str())
    }

    /// Get all manifests
    pub fn manifests(&self) -> impl Iterator<Item = &BotManifestToml> {
        self.manifests.values()
    }

    /// Get number of registered bots
    pub fn len(&self) -> usize {
        self.manifests.len()
    }

    /// Check if registry is empty
    pub fn is_empty(&self) -> bool {
        self.manifests.is_empty()
    }

    /// Get bots that have a specific capability
    pub fn with_capability(&self, cap: BotCapability) -> Vec<&BotManifestToml> {
        self.manifests
            .values()
            .filter(|m| m.has_capability(cap))
            .collect()
    }

    /// Get bots that handle a specific command prefix
    pub fn with_command_prefix(&self, prefix: &str) -> Option<&BotManifestToml> {
        self.manifests.values().find(|m| {
            m.commands.iter().any(|cmd| prefix.starts_with(&cmd.prefix))
        })
    }

    /// Get load errors for diagnostics
    pub fn load_errors(&self) -> &[(PathBuf, ManifestError)] {
        &self.load_errors
    }

    /// Get the bots directory path
    pub fn bots_dir(&self) -> &Path {
        &self.bots_dir
    }

    /// Get path to a specific bot's folder
    pub fn bot_folder(&self, bot_id: &str) -> PathBuf {
        self.bots_dir.join(bot_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_manifest(dir: &Path, bot_id: &str, capabilities: &[&str]) {
        let bot_dir = dir.join(bot_id);
        fs::create_dir_all(&bot_dir).unwrap();

        let caps_str = capabilities
            .iter()
            .map(|c| format!("\"{}\"", c))
            .collect::<Vec<_>>()
            .join(", ");

        let manifest = format!(
            r#"
[bot]
id = "{}"
name = "Test Bot {}"
version = "1.0.0"
capabilities = [{}]
"#,
            bot_id, bot_id, caps_str
        );

        fs::write(bot_dir.join(format!("{}.toml", bot_id)), manifest).unwrap();
    }

    #[test]
    fn test_scan_empty_directory() {
        let temp = TempDir::new().unwrap();
        let registry = BotRegistry::scan(temp.path().to_path_buf());
        assert!(registry.is_empty());
    }

    #[test]
    fn test_scan_single_bot() {
        let temp = TempDir::new().unwrap();
        create_test_manifest(temp.path(), "test-bot", &["ListenPresence"]);

        let registry = BotRegistry::scan(temp.path().to_path_buf());
        assert_eq!(registry.len(), 1);
        assert!(registry.contains("test-bot"));

        let manifest = registry.get("test-bot").unwrap();
        assert_eq!(manifest.bot.name, "Test Bot test-bot");
    }

    #[test]
    fn test_scan_multiple_bots() {
        let temp = TempDir::new().unwrap();
        create_test_manifest(temp.path(), "bot-a", &["ListenPresence"]);
        create_test_manifest(temp.path(), "bot-b", &["EmitSystem"]);
        create_test_manifest(temp.path(), "bot-c", &["ListenPresence", "EmitSystem"]);

        let registry = BotRegistry::scan(temp.path().to_path_buf());
        assert_eq!(registry.len(), 3);

        // Test capability filtering
        let presence_bots = registry.with_capability(BotCapability::ListenPresence);
        assert_eq!(presence_bots.len(), 2);

        let emit_bots = registry.with_capability(BotCapability::EmitSystem);
        assert_eq!(emit_bots.len(), 2);
    }

    #[test]
    fn test_rescan() {
        let temp = TempDir::new().unwrap();
        create_test_manifest(temp.path(), "bot-a", &[]);

        let mut registry = BotRegistry::scan(temp.path().to_path_buf());
        assert_eq!(registry.len(), 1);

        // Add another bot
        create_test_manifest(temp.path(), "bot-b", &[]);
        registry.rescan();
        assert_eq!(registry.len(), 2);
    }
}
