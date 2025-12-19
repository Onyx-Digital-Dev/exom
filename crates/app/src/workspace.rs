//! Workspace state and tool management
//!
//! Manages workspace tabs, tools (Terminal, Files), and child processes.
//! All state is hall-scoped and ephemeral (no persistence across restarts).

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;
use uuid::Uuid;

/// Tool types available in the workspace
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolType {
    Chat,
    Terminal,
    Files,
}

impl ToolType {
    pub fn label(&self) -> &'static str {
        match self {
            ToolType::Chat => "Chat",
            ToolType::Terminal => "Terminal",
            ToolType::Files => "Files",
        }
    }
}

/// A workspace tab
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: Uuid,
    pub tool_type: ToolType,
    pub title: String,
}

/// Terminal process state
pub struct TerminalProcess {
    #[allow(dead_code)]
    pub id: Uuid,
    child: Option<Child>,
    output_buffer: Arc<Mutex<Vec<String>>>,
    exit_status: Arc<Mutex<Option<i32>>>,
}

impl TerminalProcess {
    /// Spawn a new terminal process
    pub fn spawn() -> std::io::Result<Self> {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());

        let mut child = Command::new(&shell)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        let id = Uuid::new_v4();
        let output_buffer = Arc::new(Mutex::new(Vec::new()));
        let exit_status = Arc::new(Mutex::new(None));

        // Spawn thread to read stdout
        if let Some(stdout) = child.stdout.take() {
            let buffer = output_buffer.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for line in reader.lines().map_while(Result::ok) {
                    buffer.lock().unwrap().push(line);
                }
            });
        }

        // Spawn thread to read stderr
        if let Some(stderr) = child.stderr.take() {
            let buffer = output_buffer.clone();
            thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    buffer.lock().unwrap().push(format!("[stderr] {}", line));
                }
            });
        }

        Ok(Self {
            id,
            child: Some(child),
            output_buffer,
            exit_status,
        })
    }

    /// Send input to the terminal
    pub fn send_input(&mut self, input: &str) -> std::io::Result<()> {
        if let Some(ref mut child) = self.child {
            if let Some(ref mut stdin) = child.stdin {
                writeln!(stdin, "{}", input)?;
                stdin.flush()?;
            }
        }
        Ok(())
    }

    /// Get output lines (drains the buffer)
    pub fn get_output(&self) -> Vec<String> {
        let mut buffer = self.output_buffer.lock().unwrap();
        std::mem::take(&mut *buffer)
    }

    /// Check if process has exited and get status
    pub fn check_exit(&mut self) -> Option<i32> {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(status)) => {
                    let code = status.code().unwrap_or(-1);
                    *self.exit_status.lock().unwrap() = Some(code);
                    self.child = None;
                    Some(code)
                }
                Ok(None) => None,
                Err(_) => None,
            }
        } else {
            *self.exit_status.lock().unwrap()
        }
    }

    /// Kill the process
    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }

    /// Check if process is running
    pub fn is_running(&self) -> bool {
        self.child.is_some()
    }
}

impl Drop for TerminalProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

/// External process launched from file browser
pub struct ExternalProcess {
    #[allow(dead_code)]
    pub id: Uuid,
    #[allow(dead_code)]
    pub name: String,
    child: Option<Child>,
}

impl ExternalProcess {
    /// Launch an external application
    pub fn launch(program: &str, args: &[&str]) -> std::io::Result<Self> {
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;

        Ok(Self {
            id: Uuid::new_v4(),
            name: program.to_string(),
            child: Some(child),
        })
    }

    /// Check if still running
    pub fn is_running(&mut self) -> bool {
        if let Some(ref mut child) = self.child {
            match child.try_wait() {
                Ok(Some(_)) => {
                    self.child = None;
                    false
                }
                Ok(None) => true,
                Err(_) => false,
            }
        } else {
            false
        }
    }

    /// Kill the process
    pub fn kill(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
        }
        self.child = None;
    }
}

impl Drop for ExternalProcess {
    fn drop(&mut self) {
        // Don't kill external processes on drop - let them run independently
    }
}

/// File entry in the chest browser
#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_directory: bool,
    pub size: u64,
    pub path: std::path::PathBuf,
}

impl FileEntry {
    /// Get human-readable size
    pub fn size_string(&self) -> String {
        if self.is_directory {
            return String::new();
        }
        if self.size < 1024 {
            format!("{} B", self.size)
        } else if self.size < 1024 * 1024 {
            format!("{:.1} KB", self.size as f64 / 1024.0)
        } else if self.size < 1024 * 1024 * 1024 {
            format!("{:.1} MB", self.size as f64 / (1024.0 * 1024.0))
        } else {
            format!("{:.1} GB", self.size as f64 / (1024.0 * 1024.0 * 1024.0))
        }
    }

    /// Determine if this is a text file based on extension
    pub fn is_text(&self) -> bool {
        if self.is_directory {
            return false;
        }
        let text_extensions = [
            "txt", "md", "rs", "py", "js", "ts", "json", "toml", "yaml", "yml",
            "xml", "html", "css", "sh", "bash", "zsh", "fish", "conf", "cfg",
            "ini", "log", "csv", "sql", "c", "cpp", "h", "hpp", "java", "go",
        ];
        self.path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| text_extensions.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false)
    }

    /// Determine if this is a media file
    pub fn is_media(&self) -> bool {
        if self.is_directory {
            return false;
        }
        let media_extensions = [
            "mp4", "mkv", "avi", "mov", "webm", "mp3", "wav", "flac", "ogg",
            "png", "jpg", "jpeg", "gif", "webp", "bmp", "svg", "pdf",
        ];
        self.path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| media_extensions.contains(&e.to_lowercase().as_str()))
            .unwrap_or(false)
    }
}

/// Workspace state for a single hall
pub struct WorkspaceState {
    #[allow(dead_code)]
    pub hall_id: Uuid,
    tabs: Vec<Tab>,
    active_tab_id: Option<Uuid>,
    terminals: HashMap<Uuid, TerminalProcess>,
    external_processes: Vec<ExternalProcess>,
    /// Current path in file browser
    pub files_current_path: std::path::PathBuf,
    /// Cached file entries
    pub files_entries: Vec<FileEntry>,
}

impl WorkspaceState {
    /// Create new workspace state for a hall
    pub fn new(hall_id: Uuid, data_dir: &std::path::Path) -> Self {
        // Default to Chat tab
        let chat_tab = Tab {
            id: Uuid::new_v4(),
            tool_type: ToolType::Chat,
            title: "Chat".to_string(),
        };
        let chat_id = chat_tab.id;

        // Set initial files path to hall chest
        let chest_path = data_dir.join("halls").join(hall_id.to_string()).join("chest");

        Self {
            hall_id,
            tabs: vec![chat_tab],
            active_tab_id: Some(chat_id),
            terminals: HashMap::new(),
            external_processes: Vec::new(),
            files_current_path: chest_path,
            files_entries: Vec::new(),
        }
    }

    /// Get all tabs
    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Get active tab
    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_tab_id
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    /// Get active tab ID
    pub fn active_tab_id(&self) -> Option<Uuid> {
        self.active_tab_id
    }

    /// Set active tab
    pub fn set_active_tab(&mut self, tab_id: Uuid) {
        if self.tabs.iter().any(|t| t.id == tab_id) {
            self.active_tab_id = Some(tab_id);
        }
    }

    /// Open a new tool tab
    pub fn open_tool(&mut self, tool_type: ToolType) -> Uuid {
        // For Chat, just switch to existing tab
        if tool_type == ToolType::Chat {
            if let Some(chat_tab) = self.tabs.iter().find(|t| t.tool_type == ToolType::Chat) {
                self.active_tab_id = Some(chat_tab.id);
                return chat_tab.id;
            }
        }

        // For Files, reuse existing tab if any
        if tool_type == ToolType::Files {
            if let Some(files_tab) = self.tabs.iter().find(|t| t.tool_type == ToolType::Files) {
                self.active_tab_id = Some(files_tab.id);
                return files_tab.id;
            }
        }

        // Create new tab
        let tab = Tab {
            id: Uuid::new_v4(),
            tool_type,
            title: tool_type.label().to_string(),
        };
        let tab_id = tab.id;
        self.tabs.push(tab);
        self.active_tab_id = Some(tab_id);

        // If terminal, spawn process
        if tool_type == ToolType::Terminal {
            if let Ok(process) = TerminalProcess::spawn() {
                self.terminals.insert(tab_id, process);
            }
        }

        tab_id
    }

    /// Close a tab
    pub fn close_tab(&mut self, tab_id: Uuid) {
        // Can't close Chat tab
        if let Some(tab) = self.tabs.iter().find(|t| t.id == tab_id) {
            if tab.tool_type == ToolType::Chat {
                return;
            }
        }

        // Kill terminal if any
        if let Some(mut terminal) = self.terminals.remove(&tab_id) {
            terminal.kill();
        }

        // Remove tab
        self.tabs.retain(|t| t.id != tab_id);

        // Switch to another tab if active was closed
        if self.active_tab_id == Some(tab_id) {
            self.active_tab_id = self.tabs.first().map(|t| t.id);
        }
    }

    /// Get terminal for a tab
    #[allow(dead_code)]
    pub fn get_terminal(&self, tab_id: Uuid) -> Option<&TerminalProcess> {
        self.terminals.get(&tab_id)
    }

    /// Get mutable terminal for a tab
    pub fn get_terminal_mut(&mut self, tab_id: Uuid) -> Option<&mut TerminalProcess> {
        self.terminals.get_mut(&tab_id)
    }

    /// Refresh file listing for current path
    pub fn refresh_files(&mut self) -> std::io::Result<()> {
        self.files_entries.clear();

        // Ensure directory exists
        if !self.files_current_path.exists() {
            std::fs::create_dir_all(&self.files_current_path)?;
        }

        // Add parent directory entry if not at root
        let chest_root = self.files_current_path
            .ancestors()
            .find(|p| p.ends_with("chest"))
            .map(|p| p.to_path_buf());

        if let Some(ref root) = chest_root {
            if self.files_current_path != *root {
                if let Some(parent) = self.files_current_path.parent() {
                    self.files_entries.push(FileEntry {
                        name: "..".to_string(),
                        is_directory: true,
                        size: 0,
                        path: parent.to_path_buf(),
                    });
                }
            }
        }

        // Read directory entries
        for entry in std::fs::read_dir(&self.files_current_path)? {
            let entry = entry?;
            let metadata = entry.metadata()?;
            let name = entry.file_name().to_string_lossy().to_string();

            self.files_entries.push(FileEntry {
                name,
                is_directory: metadata.is_dir(),
                size: metadata.len(),
                path: entry.path(),
            });
        }

        // Sort: directories first, then alphabetically
        self.files_entries.sort_by(|a, b| {
            match (a.is_directory, b.is_directory) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        Ok(())
    }

    /// Navigate to a directory
    pub fn navigate_to(&mut self, path: std::path::PathBuf) -> std::io::Result<()> {
        if path.is_dir() {
            self.files_current_path = path;
            self.refresh_files()?;
        }
        Ok(())
    }

    /// Open a file (text in terminal, media in external app)
    pub fn open_file(&mut self, entry: &FileEntry) -> Option<Uuid> {
        if entry.is_directory {
            // Navigate into directory
            let _ = self.navigate_to(entry.path.clone());
            return None;
        }

        if entry.is_text() {
            // Open in terminal with $EDITOR or cat
            let editor = std::env::var("EDITOR").unwrap_or_else(|_| "cat".to_string());
            let tab_id = self.open_tool(ToolType::Terminal);
            if let Some(terminal) = self.get_terminal_mut(tab_id) {
                let _ = terminal.send_input(&format!("{} \"{}\"", editor, entry.path.display()));
            }
            return Some(tab_id);
        }

        if entry.is_media() {
            // Launch external app (xdg-open handles all media types)
            if let Ok(process) = ExternalProcess::launch("xdg-open", &[entry.path.to_str().unwrap_or("")]) {
                self.external_processes.push(process);
            }
        }

        None
    }

    /// Clean up finished external processes
    pub fn cleanup_external_processes(&mut self) {
        self.external_processes.retain_mut(|p| p.is_running());
    }

    /// Kill all processes (called on workspace close)
    pub fn kill_all(&mut self) {
        for (_, mut terminal) in self.terminals.drain() {
            terminal.kill();
        }
        for mut process in self.external_processes.drain(..) {
            process.kill();
        }
    }

    // ===== Persistence (K1) =====

    /// Convert workspace state to persisted format for saving
    pub fn to_persisted(&self, user_id: uuid::Uuid) -> exom_core::PersistedWorkspace {
        let tabs = self
            .tabs
            .iter()
            .map(|tab| exom_core::PersistedTab {
                id: tab.id.to_string(),
                tool_type: tab.tool_type.label().to_string(),
                title: tab.title.clone(),
            })
            .collect();

        // Get terminal cwd if active tab is terminal
        let terminal_cwd = self.active_tab().and_then(|tab| {
            if tab.tool_type == ToolType::Terminal {
                // For now, use the files path as cwd hint
                Some(self.files_current_path.display().to_string())
            } else {
                None
            }
        });

        exom_core::PersistedWorkspace {
            hall_id: self.hall_id,
            user_id,
            tabs,
            active_tab_id: self.active_tab_id.map(|id| id.to_string()),
            terminal_cwd,
        }
    }

    /// Restore workspace state from persisted format
    pub fn restore_from_persisted(&mut self, persisted: &exom_core::PersistedWorkspace) {
        // Clear existing tabs except Chat
        self.tabs.retain(|t| t.tool_type == ToolType::Chat);

        // Restore tabs
        for ptab in &persisted.tabs {
            let tool_type = match ptab.tool_type.as_str() {
                "Chat" => continue, // Already have Chat
                "Terminal" => ToolType::Terminal,
                "Files" => ToolType::Files,
                _ => continue,
            };

            let tab_id = uuid::Uuid::parse_str(&ptab.id).unwrap_or_else(|_| uuid::Uuid::new_v4());
            let tab = Tab {
                id: tab_id,
                tool_type,
                title: ptab.title.clone(),
            };
            self.tabs.push(tab);

            // Spawn terminal process for Terminal tabs
            if tool_type == ToolType::Terminal {
                if let Ok(mut process) = TerminalProcess::spawn() {
                    // If we have a cwd hint, try to cd there
                    if let Some(ref cwd) = persisted.terminal_cwd {
                        let _ = process.send_input(&format!("cd {}", cwd));
                    }
                    self.terminals.insert(tab_id, process);
                }
            }
        }

        // Restore active tab
        if let Some(ref active_id) = persisted.active_tab_id {
            if let Ok(id) = uuid::Uuid::parse_str(active_id) {
                if self.tabs.iter().any(|t| t.id == id) {
                    self.active_tab_id = Some(id);
                }
            }
        }

        // Restore terminal cwd for file browsing
        if let Some(ref cwd) = persisted.terminal_cwd {
            let path = std::path::PathBuf::from(cwd);
            if path.exists() {
                self.files_current_path = path;
            }
        }
    }
}

impl Drop for WorkspaceState {
    fn drop(&mut self) {
        self.kill_all();
    }
}

/// Launcher action
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::enum_variant_names)]
pub enum LauncherAction {
    OpenTerminal,
    OpenFiles,
    OpenChat,
}

impl LauncherAction {
    pub fn label(&self) -> &'static str {
        match self {
            LauncherAction::OpenTerminal => "Open Terminal",
            LauncherAction::OpenFiles => "Open Hall Chest (Files)",
            LauncherAction::OpenChat => "Open Chat",
        }
    }

    pub fn to_tool_type(self) -> ToolType {
        match self {
            LauncherAction::OpenTerminal => ToolType::Terminal,
            LauncherAction::OpenFiles => ToolType::Files,
            LauncherAction::OpenChat => ToolType::Chat,
        }
    }
}

/// All available launcher actions
pub const LAUNCHER_ACTIONS: &[LauncherAction] = &[
    LauncherAction::OpenTerminal,
    LauncherAction::OpenFiles,
    LauncherAction::OpenChat,
];

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_tool_type_labels() {
        assert_eq!(ToolType::Chat.label(), "Chat");
        assert_eq!(ToolType::Terminal.label(), "Terminal");
        assert_eq!(ToolType::Files.label(), "Files");
    }

    #[test]
    fn test_launcher_action_labels() {
        assert_eq!(LauncherAction::OpenTerminal.label(), "Open Terminal");
        assert_eq!(LauncherAction::OpenFiles.label(), "Open Hall Chest (Files)");
        assert_eq!(LauncherAction::OpenChat.label(), "Open Chat");
    }

    #[test]
    fn test_launcher_action_to_tool_type() {
        assert_eq!(LauncherAction::OpenTerminal.to_tool_type(), ToolType::Terminal);
        assert_eq!(LauncherAction::OpenFiles.to_tool_type(), ToolType::Files);
        assert_eq!(LauncherAction::OpenChat.to_tool_type(), ToolType::Chat);
    }

    #[test]
    fn test_file_entry_size_string() {
        let entry = FileEntry {
            name: "test.txt".to_string(),
            is_directory: false,
            size: 512,
            path: std::path::PathBuf::from("/test.txt"),
        };
        assert_eq!(entry.size_string(), "512 B");

        let entry_kb = FileEntry {
            name: "test.txt".to_string(),
            is_directory: false,
            size: 2048,
            path: std::path::PathBuf::from("/test.txt"),
        };
        assert_eq!(entry_kb.size_string(), "2.0 KB");

        let entry_mb = FileEntry {
            name: "test.txt".to_string(),
            is_directory: false,
            size: 2 * 1024 * 1024,
            path: std::path::PathBuf::from("/test.txt"),
        };
        assert_eq!(entry_mb.size_string(), "2.0 MB");

        let dir_entry = FileEntry {
            name: "dir".to_string(),
            is_directory: true,
            size: 4096,
            path: std::path::PathBuf::from("/dir"),
        };
        assert_eq!(dir_entry.size_string(), "");
    }

    #[test]
    fn test_file_entry_is_text() {
        let text_file = FileEntry {
            name: "script.rs".to_string(),
            is_directory: false,
            size: 100,
            path: std::path::PathBuf::from("/script.rs"),
        };
        assert!(text_file.is_text());

        let binary_file = FileEntry {
            name: "app.exe".to_string(),
            is_directory: false,
            size: 100,
            path: std::path::PathBuf::from("/app.exe"),
        };
        assert!(!binary_file.is_text());

        let dir = FileEntry {
            name: "src".to_string(),
            is_directory: true,
            size: 0,
            path: std::path::PathBuf::from("/src"),
        };
        assert!(!dir.is_text());
    }

    #[test]
    fn test_file_entry_is_media() {
        let media_file = FileEntry {
            name: "video.mp4".to_string(),
            is_directory: false,
            size: 1000,
            path: std::path::PathBuf::from("/video.mp4"),
        };
        assert!(media_file.is_media());

        let image_file = FileEntry {
            name: "photo.png".to_string(),
            is_directory: false,
            size: 500,
            path: std::path::PathBuf::from("/photo.png"),
        };
        assert!(image_file.is_media());

        let text_file = FileEntry {
            name: "readme.md".to_string(),
            is_directory: false,
            size: 100,
            path: std::path::PathBuf::from("/readme.md"),
        };
        assert!(!text_file.is_media());
    }

    #[test]
    fn test_workspace_state_new() {
        let temp_dir = TempDir::new().unwrap();
        let hall_id = Uuid::new_v4();
        let workspace = WorkspaceState::new(hall_id, temp_dir.path());

        // Should have one Chat tab by default
        assert_eq!(workspace.tabs().len(), 1);
        assert_eq!(workspace.tabs()[0].tool_type, ToolType::Chat);
        assert!(workspace.active_tab().is_some());
        assert_eq!(workspace.active_tab().unwrap().tool_type, ToolType::Chat);
    }

    #[test]
    fn test_workspace_open_chat_reuses_tab() {
        let temp_dir = TempDir::new().unwrap();
        let hall_id = Uuid::new_v4();
        let mut workspace = WorkspaceState::new(hall_id, temp_dir.path());

        let first_id = workspace.active_tab_id().unwrap();
        let second_id = workspace.open_tool(ToolType::Chat);

        // Should reuse the same Chat tab
        assert_eq!(first_id, second_id);
        assert_eq!(workspace.tabs().len(), 1);
    }

    #[test]
    fn test_workspace_open_files_reuses_tab() {
        let temp_dir = TempDir::new().unwrap();
        let hall_id = Uuid::new_v4();
        let mut workspace = WorkspaceState::new(hall_id, temp_dir.path());

        let files_id1 = workspace.open_tool(ToolType::Files);
        let files_id2 = workspace.open_tool(ToolType::Files);

        // Should reuse the same Files tab
        assert_eq!(files_id1, files_id2);
        assert_eq!(workspace.tabs().len(), 2); // Chat + Files
    }

    #[test]
    fn test_workspace_cannot_close_chat() {
        let temp_dir = TempDir::new().unwrap();
        let hall_id = Uuid::new_v4();
        let mut workspace = WorkspaceState::new(hall_id, temp_dir.path());

        let chat_id = workspace.active_tab_id().unwrap();
        workspace.close_tab(chat_id);

        // Chat tab should still exist
        assert_eq!(workspace.tabs().len(), 1);
        assert_eq!(workspace.tabs()[0].tool_type, ToolType::Chat);
    }

    #[test]
    fn test_workspace_close_files_tab() {
        let temp_dir = TempDir::new().unwrap();
        let hall_id = Uuid::new_v4();
        let mut workspace = WorkspaceState::new(hall_id, temp_dir.path());

        let files_id = workspace.open_tool(ToolType::Files);
        assert_eq!(workspace.tabs().len(), 2);

        workspace.close_tab(files_id);

        // Should switch back to Chat tab
        assert_eq!(workspace.tabs().len(), 1);
        assert_eq!(workspace.active_tab().unwrap().tool_type, ToolType::Chat);
    }

    #[test]
    fn test_workspace_refresh_files() {
        let temp_dir = TempDir::new().unwrap();

        // Create a chest directory structure
        let hall_id = Uuid::new_v4();
        let chest_path = temp_dir.path().join("halls").join(hall_id.to_string()).join("chest");
        std::fs::create_dir_all(&chest_path).unwrap();

        // Create some test files
        std::fs::write(chest_path.join("test.txt"), "hello").unwrap();
        std::fs::create_dir(chest_path.join("subdir")).unwrap();

        let mut workspace = WorkspaceState::new(hall_id, temp_dir.path());
        workspace.refresh_files().unwrap();

        // Should have the test file and subdirectory
        assert!(workspace.files_entries.len() >= 2);
        assert!(workspace.files_entries.iter().any(|e| e.name == "test.txt"));
        assert!(workspace.files_entries.iter().any(|e| e.name == "subdir" && e.is_directory));
    }

    #[test]
    fn test_launcher_actions_count() {
        assert_eq!(LAUNCHER_ACTIONS.len(), 3);
    }
}
