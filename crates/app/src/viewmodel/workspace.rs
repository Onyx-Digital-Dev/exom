//! Workspace view model bindings

use std::sync::{Arc, Mutex};

use slint::{ComponentHandle, ModelRc, VecModel};
use uuid::Uuid;

use crate::state::AppState;
use crate::workspace::{LauncherAction, ToolType, WorkspaceState, LAUNCHER_ACTIONS};
use crate::{LauncherItem, MainWindow, TabItem, FileItem};

/// Workspace manager - holds per-hall workspace state
pub struct WorkspaceManager {
    workspaces: std::collections::HashMap<Uuid, WorkspaceState>,
    data_dir: std::path::PathBuf,
}

impl WorkspaceManager {
    pub fn new(data_dir: std::path::PathBuf) -> Self {
        Self {
            workspaces: std::collections::HashMap::new(),
            data_dir,
        }
    }

    /// Get or create workspace for a hall
    pub fn get_or_create(&mut self, hall_id: Uuid) -> &mut WorkspaceState {
        self.workspaces
            .entry(hall_id)
            .or_insert_with(|| WorkspaceState::new(hall_id, &self.data_dir))
    }

    /// Get workspace for a hall
    #[allow(dead_code)]
    pub fn get(&self, hall_id: Uuid) -> Option<&WorkspaceState> {
        self.workspaces.get(&hall_id)
    }

    /// Get mutable workspace for a hall
    pub fn get_mut(&mut self, hall_id: Uuid) -> Option<&mut WorkspaceState> {
        self.workspaces.get_mut(&hall_id)
    }

    /// Close workspace for a hall (kills all processes)
    #[allow(dead_code)]
    pub fn close(&mut self, hall_id: Uuid) {
        if let Some(mut workspace) = self.workspaces.remove(&hall_id) {
            workspace.kill_all();
        }
    }
}

pub fn setup_workspace_bindings(
    window: &MainWindow,
    state: Arc<AppState>,
    workspace_manager: Arc<Mutex<WorkspaceManager>>,
) {
    // Initialize launcher items
    let launcher_items: Vec<LauncherItem> = LAUNCHER_ACTIONS
        .iter()
        .enumerate()
        .map(|(idx, action)| LauncherItem {
            id: idx as i32,
            label: action.label().into(),
        })
        .collect();
    window.set_launcher_items(ModelRc::new(VecModel::from(launcher_items)));

    // Set up tab selection callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    window.on_select_tab(move |tab_id_str| {
        let tab_id = match Uuid::parse_str(&tab_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let mut wm = wm.lock().unwrap();
        if let Some(workspace) = wm.get_mut(hall_id) {
            workspace.set_active_tab(tab_id);
            if let Some(window) = window_weak.upgrade() {
                refresh_tabs(&window, workspace);
                refresh_workspace_view(&window, workspace);
            }
        }
    });

    // Set up tab close callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    window.on_close_tab(move |tab_id_str| {
        let tab_id = match Uuid::parse_str(&tab_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let mut wm = wm.lock().unwrap();
        if let Some(workspace) = wm.get_mut(hall_id) {
            workspace.close_tab(tab_id);
            if let Some(window) = window_weak.upgrade() {
                refresh_tabs(&window, workspace);
                refresh_workspace_view(&window, workspace);
            }
        }
    });

    // Set up launcher action callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    window.on_launcher_select(move |action_id| {
        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let action = match action_id {
            0 => LauncherAction::OpenTerminal,
            1 => LauncherAction::OpenFiles,
            2 => LauncherAction::OpenChat,
            _ => return,
        };

        let mut wm = wm.lock().unwrap();
        let workspace = wm.get_or_create(hall_id);
        workspace.open_tool(action.to_tool_type());

        if let Some(window) = window_weak.upgrade() {
            window.set_show_launcher(false);
            refresh_tabs(&window, workspace);
            refresh_workspace_view(&window, workspace);
        }
    });

    // Set up terminal input callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    window.on_terminal_send(move |input| {
        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let mut wm = wm.lock().unwrap();
        if let Some(workspace) = wm.get_mut(hall_id) {
            if let Some(tab) = workspace.active_tab() {
                if tab.tool_type == ToolType::Terminal {
                    if let Some(terminal) = workspace.get_terminal_mut(tab.id) {
                        let _ = terminal.send_input(&input);
                    }
                }
            }
            // Refresh terminal output
            if let Some(window) = window_weak.upgrade() {
                refresh_terminal_output(&window, workspace);
            }
        }
    });

    // Set up file browser navigation callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    window.on_files_navigate(move |path_str| {
        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let mut wm = wm.lock().unwrap();
        if let Some(workspace) = wm.get_mut(hall_id) {
            let path = std::path::PathBuf::from(path_str.as_str());
            let _ = workspace.navigate_to(path);
            if let Some(window) = window_weak.upgrade() {
                refresh_files(&window, workspace);
            }
        }
    });

    // Set up file open callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    window.on_files_open(move |path_str| {
        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let mut wm = wm.lock().unwrap();
        if let Some(workspace) = wm.get_mut(hall_id) {
            let path = std::path::PathBuf::from(path_str.as_str());

            // Find the entry
            if let Some(entry) = workspace.files_entries.iter().find(|e| e.path == path).cloned() {
                workspace.open_file(&entry);
                if let Some(window) = window_weak.upgrade() {
                    refresh_tabs(&window, workspace);
                    refresh_workspace_view(&window, workspace);
                    refresh_files(&window, workspace);
                }
            }
        }
    });

    // Set up timer for terminal output polling
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let hall_id = match state_clone.current_hall_id() {
                Some(id) => id,
                None => return,
            };

            let mut wm = wm.lock().unwrap();
            if let Some(workspace) = wm.get_mut(hall_id) {
                // Check for terminal output
                let active_tool = workspace.active_tab().map(|t| t.tool_type);
                if active_tool == Some(ToolType::Terminal) {
                    if let Some(window) = window_weak.upgrade() {
                        refresh_terminal_output(&window, workspace);
                    }
                }

                // Cleanup external processes
                workspace.cleanup_external_processes();
            }
        },
    );

    // Keep timer alive by leaking it (it lives for the app lifetime)
    std::mem::forget(timer);
}

/// Initialize workspace when hall is selected
pub fn init_workspace_for_hall(
    window: &MainWindow,
    workspace_manager: &Arc<Mutex<WorkspaceManager>>,
    hall_id: Uuid,
) {
    let mut wm = workspace_manager.lock().unwrap();
    let workspace = wm.get_or_create(hall_id);

    // Initialize files if Files tab exists
    if workspace.tabs().iter().any(|t| t.tool_type == ToolType::Files) {
        let _ = workspace.refresh_files();
    }

    refresh_tabs(window, workspace);
    refresh_workspace_view(window, workspace);
}

/// Refresh tab bar
fn refresh_tabs(window: &MainWindow, workspace: &WorkspaceState) {
    let active_id = workspace.active_tab_id();
    let tabs: Vec<TabItem> = workspace
        .tabs()
        .iter()
        .map(|tab| TabItem {
            id: tab.id.to_string().into(),
            label: tab.title.clone().into(),
            is_active: Some(tab.id) == active_id,
            is_closable: tab.tool_type != ToolType::Chat,
        })
        .collect();
    window.set_workspace_tabs(ModelRc::new(VecModel::from(tabs)));
}

/// Refresh which tool view is shown
fn refresh_workspace_view(window: &MainWindow, workspace: &WorkspaceState) {
    if let Some(tab) = workspace.active_tab() {
        window.set_active_tool(match tab.tool_type {
            ToolType::Chat => "chat".into(),
            ToolType::Terminal => "terminal".into(),
            ToolType::Files => "files".into(),
        });
    }
}

/// Refresh terminal output
fn refresh_terminal_output(window: &MainWindow, workspace: &mut WorkspaceState) {
    if let Some(tab) = workspace.active_tab() {
        if tab.tool_type == ToolType::Terminal {
            if let Some(terminal) = workspace.get_terminal_mut(tab.id) {
                let new_output = terminal.get_output();
                if !new_output.is_empty() {
                    // Append to existing output
                    let mut current = window.get_terminal_output().to_string();
                    for line in new_output {
                        if !current.is_empty() {
                            current.push('\n');
                        }
                        current.push_str(&line);
                    }
                    window.set_terminal_output(current.into());
                }

                // Check exit status
                if let Some(code) = terminal.check_exit() {
                    let mut current = window.get_terminal_output().to_string();
                    current.push_str(&format!("\n[Process exited with code {}]", code));
                    window.set_terminal_output(current.into());
                    window.set_terminal_running(false);
                } else {
                    window.set_terminal_running(terminal.is_running());
                }
            }
        }
    }
}

/// Refresh file browser
fn refresh_files(window: &MainWindow, workspace: &WorkspaceState) {
    window.set_files_path(workspace.files_current_path.display().to_string().into());

    let files: Vec<FileItem> = workspace
        .files_entries
        .iter()
        .map(|entry| FileItem {
            name: entry.name.clone().into(),
            path: entry.path.display().to_string().into(),
            is_directory: entry.is_directory,
            size: entry.size_string().into(),
        })
        .collect();
    window.set_files_entries(ModelRc::new(VecModel::from(files)));
}
