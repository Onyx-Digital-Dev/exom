//! Members view model

use std::sync::Arc;

use exom_core::{HallAction, HallRole, PermissionMatrix};
use slint::{ComponentHandle, ModelRc, VecModel};

use crate::state::AppState;
use crate::ChestFileItem;
use crate::MainWindow;
use crate::MemberItem;

pub fn setup_member_bindings(window: &MainWindow, state: Arc<AppState>) {
    // Load members
    let state_load = state.clone();
    let window_weak = window.as_weak();
    window.on_load_members(move || {
        let hall_id = match state_load.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let current_user_id = state_load.current_user_id();

        let db = state_load.db.lock().unwrap();
        let members = match db.halls().list_members(hall_id) {
            Ok(m) => m,
            Err(_) => return,
        };

        let member_items: Vec<MemberItem> = members
            .iter()
            .map(|m| MemberItem {
                id: m.user_id.to_string().into(),
                name: m.username.clone().into(),
                role: m.role.display_name().into(),
                is_online: m.is_online,
                is_host: m.is_host,
                is_you: current_user_id == Some(m.user_id),
            })
            .collect();

        drop(db);

        if let Some(w) = window_weak.upgrade() {
            let model = std::rc::Rc::new(VecModel::from(member_items));
            w.set_members(ModelRc::from(model));
            // Set current user id for context action gating
            if let Some(uid) = current_user_id {
                w.set_current_user_id(uid.to_string().into());
            }
        }
    });

    // Promote member
    let state_promote = state.clone();
    let window_weak = window.as_weak();
    window.on_promote_member(move |target_id_str| {
        let target_id = match uuid::Uuid::parse_str(&target_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let user_id = match state_promote.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let hall_id = match state_promote.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_promote.db.lock().unwrap();

        // Get actor's role
        let actor_role = match db.halls().get_user_role(user_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        // Get target's current role
        let target_role = match db.halls().get_user_role(target_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        // Calculate new role (one level up)
        let new_role = match target_role {
            HallRole::HallFellow => HallRole::HallAgent,
            HallRole::HallAgent => HallRole::HallModerator,
            HallRole::HallModerator => HallRole::HallPrefect,
            HallRole::HallPrefect | HallRole::HallBuilder => return, // Can't promote further
        };

        // Check permission
        if !PermissionMatrix::can_change_role(actor_role, target_role, new_role) {
            return;
        }

        let _ = db.halls().update_role(target_id, hall_id, new_role);
        drop(db);

        // Init chest if promoted to Agent
        if new_role == HallRole::HallAgent {
            if let Ok(Some(hall)) = state_promote.db.lock().unwrap().halls().find_by_id(hall_id) {
                let chest = state_promote.chest.lock().unwrap();
                let _ = chest.init_hall_chest(hall_id, &hall.name, new_role);
            }
        }

        if let Some(w) = window_weak.upgrade() {
            w.invoke_load_members();
        }
    });

    // Demote member
    let state_demote = state.clone();
    let window_weak = window.as_weak();
    window.on_demote_member(move |target_id_str| {
        let target_id = match uuid::Uuid::parse_str(&target_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let user_id = match state_demote.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let hall_id = match state_demote.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_demote.db.lock().unwrap();

        // Get actor's role
        let actor_role = match db.halls().get_user_role(user_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        // Get target's current role
        let target_role = match db.halls().get_user_role(target_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        // Calculate new role (one level down)
        let new_role = match target_role {
            HallRole::HallPrefect => HallRole::HallModerator,
            HallRole::HallModerator => HallRole::HallAgent,
            HallRole::HallAgent => HallRole::HallFellow,
            HallRole::HallFellow | HallRole::HallBuilder => return, // Can't demote further
        };

        // Check permission
        if !PermissionMatrix::can_change_role(actor_role, target_role, new_role) {
            return;
        }

        let _ = db.halls().update_role(target_id, hall_id, new_role);
        drop(db);

        if let Some(w) = window_weak.upgrade() {
            w.invoke_load_members();
        }
    });

    // Kick member
    let state_kick = state.clone();
    let window_weak = window.as_weak();
    window.on_kick_member(move |target_id_str| {
        let target_id = match uuid::Uuid::parse_str(&target_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let user_id = match state_kick.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let hall_id = match state_kick.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_kick.db.lock().unwrap();

        // Get roles
        let actor_role = match db.halls().get_user_role(user_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        let target_role = match db.halls().get_user_role(target_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        // Check permission
        if !PermissionMatrix::can_kick(actor_role, target_role) {
            return;
        }

        let _ = db.halls().remove_member(target_id, hall_id);
        drop(db);

        if let Some(w) = window_weak.upgrade() {
            w.invoke_load_members();
        }
    });

    // Load chest files
    let state_chest = state.clone();
    let window_weak = window.as_weak();
    window.on_load_chest_files(move || {
        let hall_id = match state_chest.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let user_id = match state_chest.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_chest.db.lock().unwrap();
        let role = match db.halls().get_user_role(user_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };
        drop(db);

        // Check chest access - Fellows cannot view
        if !PermissionMatrix::can_perform(role, HallAction::ViewChest) {
            if let Some(w) = window_weak.upgrade() {
                w.set_chest_path("".into());
                w.set_chest_files(ModelRc::default());
                w.set_chest_status("Locked - Fellows cannot view Chest".into());
            }
            return;
        }

        let chest = state_chest.chest.lock().unwrap();
        let files = match chest.list_files(hall_id, None) {
            Ok(f) => f,
            Err(_) => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_chest_status("Failed to load files".into());
                }
                return;
            }
        };

        let file_items: Vec<ChestFileItem> = files
            .iter()
            .map(|f| ChestFileItem {
                name: f.name.clone().into(),
                is_directory: f.is_directory,
                size: format_size(f.size_bytes).into(),
                sync_status: f.sync_status.display().into(),
            })
            .collect();

        let chest_path = chest.hall_path(hall_id);
        drop(chest);

        // Format path for display (replace home dir with ~)
        let path_display = if let Ok(home) = std::env::var("HOME") {
            let path_str = chest_path.to_string_lossy();
            if path_str.starts_with(&home) {
                format!("~{}", &path_str[home.len()..])
            } else {
                path_str.to_string()
            }
        } else {
            chest_path.to_string_lossy().to_string()
        };

        if let Some(w) = window_weak.upgrade() {
            let model = std::rc::Rc::new(VecModel::from(file_items));
            w.set_chest_files(ModelRc::from(model));
            w.set_chest_path(path_display.into());
            w.set_chest_status("Local only \u{2014} sync coming later".into());
        }
    });
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
    }
}
