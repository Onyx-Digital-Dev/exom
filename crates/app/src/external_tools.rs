//! External Tool Runtime
//!
//! Manages external tools spawned from Exom. Tools run as separate OS windows -
//! there is NO embedding. This is by design: Exom launches and tracks tools,
//! but they remain independent windows owned by the compositor/WM.
//!
//! # Design Principles
//! - Honest: Tools open in new windows, not "inside" Exom
//! - Trackable: Exom knows what's running per hall
//! - Controllable: Users/bots can stop tools they launched
//! - Non-invasive: Tools continue running if Exom closes (configurable)

use std::collections::HashMap;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use uuid::Uuid;

/// Status of an external tool
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// Process is running
    Running,
    /// Process has exited normally
    Exited,
    /// Process was killed by user/bot
    Stopped,
    /// Failed to start
    Failed,
}

impl ToolStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Exited => "Exited",
            Self::Stopped => "Stopped",
            Self::Failed => "Failed",
        }
    }

    pub fn is_alive(&self) -> bool {
        matches!(self, Self::Running)
    }
}

/// A launched external tool
pub struct LaunchedTool {
    /// Unique ID for this tool instance
    pub id: Uuid,
    /// Hall this tool was launched from
    pub hall_id: Uuid,
    /// Bot or user that launched it (None = user)
    pub launched_by: Option<String>,
    /// Display name
    pub name: String,
    /// Command that was run
    pub command: String,
    /// Arguments
    pub args: Vec<String>,
    /// When it was launched
    pub launched_at: DateTime<Utc>,
    /// Current status
    status: ToolStatus,
    /// Child process handle (None if exited/failed)
    child: Option<Child>,
    /// Process ID (for display only - not for control)
    pub pid: Option<u32>,
}

impl LaunchedTool {
    /// Get current status (updates from process state)
    pub fn status(&mut self) -> ToolStatus {
        // If we think it's running, check if it still is
        if self.status == ToolStatus::Running {
            if let Some(ref mut child) = self.child {
                match child.try_wait() {
                    Ok(Some(_status)) => {
                        self.status = ToolStatus::Exited;
                        self.child = None;
                    }
                    Ok(None) => {} // Still running
                    Err(_) => {
                        self.status = ToolStatus::Failed;
                        self.child = None;
                    }
                }
            } else {
                // No child but marked running - fix inconsistency
                self.status = ToolStatus::Exited;
            }
        }
        self.status
    }

    /// Stop the tool (kill the process)
    pub fn stop(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.kill() {
                Ok(()) => {
                    self.status = ToolStatus::Stopped;
                    self.child = None;
                    true
                }
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Get a display-friendly summary
    pub fn display_command(&self) -> String {
        if self.args.is_empty() {
            self.command.clone()
        } else {
            format!("{} {}", self.command, self.args.join(" "))
        }
    }
}

impl Drop for LaunchedTool {
    fn drop(&mut self) {
        // By default, don't kill on drop - let tools run independently
        // This is intentional: closing Exom shouldn't kill your terminal
    }
}

/// Snapshot of a tool for UI display (no process handle)
#[derive(Debug, Clone)]
pub struct ToolSnapshot {
    pub id: Uuid,
    pub hall_id: Uuid,
    pub name: String,
    pub command: String,
    pub launched_by: Option<String>,
    pub launched_at: DateTime<Utc>,
    pub status: ToolStatus,
    pub pid: Option<u32>,
}

impl From<&mut LaunchedTool> for ToolSnapshot {
    fn from(tool: &mut LaunchedTool) -> Self {
        Self {
            id: tool.id,
            hall_id: tool.hall_id,
            name: tool.name.clone(),
            command: tool.display_command(),
            launched_by: tool.launched_by.clone(),
            launched_at: tool.launched_at,
            status: tool.status(), // Updates status
            pid: tool.pid,
        }
    }
}

/// Pinned launcher configuration
#[derive(Debug, Clone)]
pub struct PinnedLauncher {
    pub id: Uuid,
    pub name: String,
    pub command: String,
    pub args: Vec<String>,
    pub icon: Option<String>,
}

impl PinnedLauncher {
    pub fn new(name: String, command: String, args: Vec<String>) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            command,
            args,
            icon: None,
        }
    }
}

/// External Tool Runtime
///
/// Tracks all external tools launched from Exom, organized by hall.
/// Does NOT embed tools - they run as separate OS windows.
pub struct ExternalToolRuntime {
    /// Tools indexed by tool ID
    tools: HashMap<Uuid, LaunchedTool>,
    /// Pinned launchers per hall
    pinned: HashMap<Uuid, Vec<PinnedLauncher>>,
}

impl ExternalToolRuntime {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
            pinned: HashMap::new(),
        }
    }

    /// Launch a new external tool
    ///
    /// Returns the tool ID on success.
    /// The tool opens in a NEW WINDOW - not embedded in Exom.
    pub fn launch(
        &mut self,
        hall_id: Uuid,
        name: String,
        command: &str,
        args: &[String],
        launched_by: Option<String>,
    ) -> Result<Uuid, String> {
        // Spawn the process
        let result = Command::new(command)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();

        match result {
            Ok(child) => {
                let id = Uuid::new_v4();
                let pid = child.id();

                let tool = LaunchedTool {
                    id,
                    hall_id,
                    launched_by,
                    name,
                    command: command.to_string(),
                    args: args.to_vec(),
                    launched_at: Utc::now(),
                    status: ToolStatus::Running,
                    child: Some(child),
                    pid: Some(pid),
                };

                tracing::info!(
                    tool_id = %id,
                    hall_id = %hall_id,
                    command = %command,
                    pid = pid,
                    "Launched external tool (opens in new window)"
                );

                self.tools.insert(id, tool);
                Ok(id)
            }
            Err(e) => {
                tracing::warn!(
                    hall_id = %hall_id,
                    command = %command,
                    error = %e,
                    "Failed to launch external tool"
                );
                Err(format!("Failed to launch {}: {}", command, e))
            }
        }
    }

    /// Stop a running tool
    pub fn stop(&mut self, tool_id: Uuid) -> Result<(), String> {
        if let Some(tool) = self.tools.get_mut(&tool_id) {
            if tool.stop() {
                tracing::info!(tool_id = %tool_id, "Stopped external tool");
                Ok(())
            } else {
                Err("Tool is not running".to_string())
            }
        } else {
            Err("Tool not found".to_string())
        }
    }

    /// Stop a tool by ID string (for bot actions)
    pub fn stop_by_id(&mut self, tool_id_str: &str) -> Result<(), String> {
        let tool_id = Uuid::parse_str(tool_id_str)
            .map_err(|_| "Invalid tool ID".to_string())?;
        self.stop(tool_id)
    }

    /// Get a tool by ID
    pub fn get(&mut self, tool_id: Uuid) -> Option<ToolSnapshot> {
        self.tools.get_mut(&tool_id).map(|t| t.into())
    }

    /// List all tools for a hall
    pub fn list_for_hall(&mut self, hall_id: Uuid) -> Vec<ToolSnapshot> {
        self.tools
            .values_mut()
            .filter(|t| t.hall_id == hall_id)
            .map(|t| t.into())
            .collect()
    }

    /// List running tools for a hall
    pub fn list_running_for_hall(&mut self, hall_id: Uuid) -> Vec<ToolSnapshot> {
        // Update all statuses first
        for tool in self.tools.values_mut() {
            tool.status();
        }
        // Then filter and collect
        self.tools
            .values_mut()
            .filter(|t| t.hall_id == hall_id && t.status == ToolStatus::Running)
            .map(|t| t.into())
            .collect()
    }

    /// Clean up exited tools (optional - for memory management)
    pub fn cleanup_exited(&mut self) {
        // Update all statuses first
        for tool in self.tools.values_mut() {
            tool.status();
        }
        // Remove non-running tools
        self.tools.retain(|_, t| t.status == ToolStatus::Running);
    }

    /// Clean up exited tools for a specific hall
    pub fn cleanup_hall(&mut self, hall_id: Uuid) {
        for tool in self.tools.values_mut() {
            if tool.hall_id == hall_id {
                tool.status();
            }
        }
        self.tools
            .retain(|_, t| t.hall_id != hall_id || t.status == ToolStatus::Running);
    }

    /// Get count of running tools for a hall
    pub fn running_count(&mut self, hall_id: Uuid) -> usize {
        // Update all statuses first
        for tool in self.tools.values_mut() {
            tool.status();
        }
        // Then count
        self.tools
            .values()
            .filter(|t| t.hall_id == hall_id && t.status == ToolStatus::Running)
            .count()
    }

    // === Pinned Launchers ===

    /// Add a pinned launcher for a hall
    pub fn add_pinned(&mut self, hall_id: Uuid, launcher: PinnedLauncher) {
        self.pinned
            .entry(hall_id)
            .or_insert_with(Vec::new)
            .push(launcher);
    }

    /// Remove a pinned launcher
    pub fn remove_pinned(&mut self, hall_id: Uuid, launcher_id: Uuid) -> bool {
        if let Some(launchers) = self.pinned.get_mut(&hall_id) {
            let len_before = launchers.len();
            launchers.retain(|l| l.id != launcher_id);
            launchers.len() < len_before
        } else {
            false
        }
    }

    /// Get pinned launchers for a hall
    pub fn get_pinned(&self, hall_id: Uuid) -> Vec<PinnedLauncher> {
        self.pinned.get(&hall_id).cloned().unwrap_or_default()
    }

    /// Launch a pinned launcher
    pub fn launch_pinned(
        &mut self,
        hall_id: Uuid,
        launcher_id: Uuid,
        launched_by: Option<String>,
    ) -> Result<Uuid, String> {
        let launcher = self
            .pinned
            .get(&hall_id)
            .and_then(|ls| ls.iter().find(|l| l.id == launcher_id))
            .cloned()
            .ok_or_else(|| "Pinned launcher not found".to_string())?;

        self.launch(
            hall_id,
            launcher.name,
            &launcher.command,
            &launcher.args,
            launched_by,
        )
    }

    /// Set default pinned launchers for a hall (if none exist)
    pub fn ensure_default_pinned(&mut self, hall_id: Uuid) {
        if self.pinned.contains_key(&hall_id) {
            return;
        }

        // Detect available terminals
        let default_launchers = vec![
            ("Terminal", "kitty", vec![]),
            ("Terminal", "alacritty", vec![]),
            ("Terminal", "gnome-terminal", vec![]),
            ("Files", "nautilus", vec![]),
            ("Files", "dolphin", vec![]),
        ];

        let mut added = vec![];
        for (name, cmd, args) in default_launchers {
            // Check if command exists
            if which_exists(cmd) && !added.iter().any(|(n, _): &(&str, &str)| *n == name) {
                added.push((name, cmd));
                self.add_pinned(
                    hall_id,
                    PinnedLauncher::new(
                        name.to_string(),
                        cmd.to_string(),
                        args.iter().map(|s: &&str| s.to_string()).collect(),
                    ),
                );
            }
        }
    }
}

impl Default for ExternalToolRuntime {
    fn default() -> Self {
        Self::new()
    }
}

/// Thread-safe wrapper for ExternalToolRuntime
pub type SharedToolRuntime = Arc<Mutex<ExternalToolRuntime>>;

/// Create a new shared tool runtime
pub fn create_shared_runtime() -> SharedToolRuntime {
    Arc::new(Mutex::new(ExternalToolRuntime::new()))
}

/// Check if a command exists in PATH
fn which_exists(cmd: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_launch_and_stop() {
        let mut runtime = ExternalToolRuntime::new();
        let hall_id = Uuid::new_v4();

        // Launch a simple command that exits immediately
        let result = runtime.launch(
            hall_id,
            "Test".to_string(),
            "true", // Unix command that just exits 0
            &[],
            None,
        );

        assert!(result.is_ok());
        let tool_id = result.unwrap();

        // Should show up in list
        let tools = runtime.list_for_hall(hall_id);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].id, tool_id);
    }

    #[test]
    fn test_pinned_launchers() {
        let mut runtime = ExternalToolRuntime::new();
        let hall_id = Uuid::new_v4();

        // Add pinned
        runtime.add_pinned(
            hall_id,
            PinnedLauncher::new("Test".to_string(), "echo".to_string(), vec!["hello".to_string()]),
        );

        let pinned = runtime.get_pinned(hall_id);
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].name, "Test");

        // Remove
        let launcher_id = pinned[0].id;
        assert!(runtime.remove_pinned(hall_id, launcher_id));
        assert!(runtime.get_pinned(hall_id).is_empty());
    }

    #[test]
    fn test_tool_snapshot() {
        let mut runtime = ExternalToolRuntime::new();
        let hall_id = Uuid::new_v4();

        runtime.launch(hall_id, "Sleep".to_string(), "sleep", &["1".to_string()], Some("test-bot".to_string())).ok();

        let tools = runtime.list_for_hall(hall_id);
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].launched_by, Some("test-bot".to_string()));
    }
}
