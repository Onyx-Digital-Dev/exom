//! Workspace view model bindings

use std::sync::{Arc, Mutex};

use exom_net::CurrentTool;
use slint::{ComponentHandle, ModelRc, VecModel};
use uuid::Uuid;

use crate::external_tools::ToolSnapshot;
use crate::state::AppState;
use crate::workspace::{LauncherAction, ToolType, WorkspaceState, LAUNCHER_ACTIONS};
use crate::{FileItem, LaunchedToolItem, LauncherItem, MainWindow, PinnedLauncherItem, TabItem};

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

    /// Get or create workspace for a hall, optionally restoring from persistence
    pub fn get_or_create_with_restore(
        &mut self,
        hall_id: Uuid,
        state: &AppState,
    ) -> &mut WorkspaceState {
        if !self.workspaces.contains_key(&hall_id) {
            // Try to restore from DB first
            let mut workspace = WorkspaceState::new(hall_id, &self.data_dir);

            if let Some(user_id) = state.current_user_id() {
                if let Ok(Some(persisted)) = state
                    .db
                    .lock()
                    .unwrap()
                    .workspaces()
                    .load(hall_id, user_id)
                {
                    workspace.restore_from_persisted(&persisted);
                    tracing::info!(hall_id = %hall_id, tabs = persisted.tabs.len(), "Restored workspace");
                }
            }

            self.workspaces.insert(hall_id, workspace);
        }

        self.workspaces.get_mut(&hall_id).unwrap()
    }

    /// Get or create workspace for a hall (without restore - for fallback)
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

    /// Save workspace state to DB
    pub fn save_workspace(&self, hall_id: Uuid, state: &AppState) {
        if let (Some(workspace), Some(user_id)) =
            (self.workspaces.get(&hall_id), state.current_user_id())
        {
            let persisted = workspace.to_persisted(user_id);
            if let Err(e) = state.db.lock().unwrap().workspaces().save(&persisted) {
                tracing::warn!(error = %e, "Failed to save workspace state");
            }
        }
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
            0 => LauncherAction::OpenTools,
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
            // Also refresh tools when switching to Tools tab
            if action == LauncherAction::OpenTools {
                refresh_launched_tools(&window, &state_clone, hall_id);
                refresh_pinned_launchers(&window, &state_clone, hall_id);
            }
        }
    });

    // Set up stop tool callback
    let state_clone = state.clone();
    window.on_stop_tool(move |tool_id_str| {
        let mut tools = state_clone.tools.lock().unwrap();
        if let Err(e) = tools.stop_by_id(&tool_id_str) {
            tracing::warn!(error = %e, "Failed to stop tool");
        }
    });

    // Set up launch pinned callback
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    window.on_launch_pinned(move |launcher_id_str| {
        let hall_id = match state_clone.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        // Parse the launcher ID
        let launcher_id = match Uuid::parse_str(&launcher_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        // Get the launcher from storage
        let launcher = match state_clone.db.lock().unwrap().launchers().get(launcher_id) {
            Ok(Some(l)) => l,
            _ => return,
        };

        // Store the name for desk status before we move it
        let desk_label = launcher.name.clone();

        // Launch the tool
        let mut tools = state_clone.tools.lock().unwrap();
        match tools.launch(
            hall_id,
            launcher.name.clone(),
            &launcher.command,
            &launcher.args,
            None, // Launched by user
        ) {
            Ok(id) => {
                tracing::info!(tool_id = %id, "User launched pinned tool");
                // Set desk status (voluntary: user explicitly launched a tool)
                drop(tools); // Release lock before calling set_desk_status
                state_clone.set_desk_status(&desk_label);
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to launch pinned tool");
            }
        }

        // Refresh the launched tools display
        if let Some(window) = window_weak.upgrade() {
            refresh_launched_tools(&window, &state_clone, hall_id);
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

    // Set up timer for tool status polling
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let wm = workspace_manager.clone();
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(500), // Check every 500ms
        move || {
            let hall_id = match state_clone.current_hall_id() {
                Some(id) => id,
                None => return,
            };

            let mut wm = wm.lock().unwrap();
            if let Some(workspace) = wm.get_mut(hall_id) {
                // Check if we're viewing the Tools tab
                let active_tool = workspace.active_tab().map(|t| t.tool_type);
                if active_tool == Some(ToolType::Tools) {
                    if let Some(window) = window_weak.upgrade() {
                        refresh_launched_tools(&window, &state_clone, hall_id);
                    }
                }

                // Cleanup external processes
                workspace.cleanup_external_processes();
            }

            // Check if any user-launched tools are still running
            // If not, clear desk status (tool exit -> clear desk)
            let mut tools = state_clone.tools.lock().unwrap();
            let running_count = tools.running_count(hall_id);
            drop(tools);

            if running_count == 0 {
                // No running tools - clear desk status
                // This is voluntary: status only set/cleared by user action
                state_clone.clear_desk_status();
            }
        },
    );

    // Keep timer alive by leaking it (it lives for the app lifetime)
    std::mem::forget(timer);
}

/// Initialize workspace when hall is selected (with persistence restore)
pub fn init_workspace_for_hall(
    window: &MainWindow,
    workspace_manager: &Arc<Mutex<WorkspaceManager>>,
    state: &Arc<AppState>,
    hall_id: Uuid,
) {
    let mut wm = workspace_manager.lock().unwrap();
    let workspace = wm.get_or_create_with_restore(hall_id, state);

    // Initialize files if Files tab exists
    if workspace.tabs().iter().any(|t| t.tool_type == ToolType::Files) {
        let _ = workspace.refresh_files();
    }

    refresh_tabs(window, workspace);
    refresh_workspace_view(window, workspace);

    // Update local tool based on active tab
    if let Some(tab) = workspace.active_tab() {
        let tool = match tab.tool_type {
            ToolType::Chat => CurrentTool::Chat,
            ToolType::Tools => CurrentTool::Terminal, // Map Tools to Terminal for network compat
            ToolType::Files => CurrentTool::Files,
        };
        state.set_local_tool(tool);
    }

    // Refresh launched tools and pinned launchers
    refresh_launched_tools(window, state, hall_id);
    refresh_pinned_launchers(window, state, hall_id);

    // Save last hall for auto-enter (K4)
    state.save_last_hall(hall_id);
}

/// Save workspace state (call after significant changes)
pub fn save_workspace_state(
    workspace_manager: &Arc<Mutex<WorkspaceManager>>,
    state: &Arc<AppState>,
    hall_id: Uuid,
) {
    let wm = workspace_manager.lock().unwrap();
    wm.save_workspace(hall_id, state);
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
            ToolType::Tools => "tools".into(),
            ToolType::Files => "files".into(),
        });
    }
}

/// Refresh launched tools list
fn refresh_launched_tools(window: &MainWindow, state: &AppState, hall_id: Uuid) {
    let mut tools_runtime = state.tools.lock().unwrap();
    let snapshots = tools_runtime.list_for_hall(hall_id);

    let items: Vec<LaunchedToolItem> = snapshots
        .iter()
        .map(|t| snapshot_to_item(t))
        .collect();

    window.set_launched_tools(ModelRc::new(VecModel::from(items)));
}

/// Refresh pinned launchers list
fn refresh_pinned_launchers(window: &MainWindow, state: &AppState, hall_id: Uuid) {
    let launchers = match state.db.lock().unwrap().launchers().list_for_hall(hall_id) {
        Ok(l) => l,
        Err(_) => Vec::new(),
    };

    let items: Vec<PinnedLauncherItem> = launchers
        .iter()
        .map(|l| PinnedLauncherItem {
            id: l.id.to_string().into(),
            name: l.name.clone().into(),
            icon: l.icon.clone().unwrap_or_default().into(),
        })
        .collect();

    window.set_pinned_launchers(ModelRc::new(VecModel::from(items)));
}

/// Convert a ToolSnapshot to a LaunchedToolItem for the UI
fn snapshot_to_item(t: &ToolSnapshot) -> LaunchedToolItem {
    LaunchedToolItem {
        id: t.id.to_string().into(),
        name: t.name.clone().into(),
        command: t.command.clone().into(),
        status: t.status.as_str().into(),
        pid: t.pid.map(|p| p.to_string()).unwrap_or_default().into(),
        launched_by: t.launched_by.clone().unwrap_or_default().into(),
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
