//! Hall view model

use std::sync::Arc;

use exom_core::{Hall, HallRole, HostElectionResult, HostingState, Invite, Membership};
use rand::Rng;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::state::AppState;
use crate::HallItem;
use crate::MainWindow;

pub fn setup_hall_bindings(window: &MainWindow, state: Arc<AppState>) {
    // Load halls
    let state_load = state.clone();
    let window_weak = window.as_weak();
    window.on_load_halls(move || {
        let user_id = match state_load.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_load.db.lock().unwrap();
        let halls = match db.halls().list_for_user(user_id) {
            Ok(h) => h,
            Err(_) => return,
        };

        let hall_items: Vec<HallItem> = halls
            .iter()
            .map(|h| {
                let role = db
                    .halls()
                    .get_user_role(user_id, h.id)
                    .ok()
                    .flatten()
                    .unwrap_or(HallRole::HallFellow);

                HallItem {
                    id: h.id.to_string().into(),
                    name: h.name.clone().into(),
                    role: role.short_name().into(),
                }
            })
            .collect();

        drop(db);

        if let Some(w) = window_weak.upgrade() {
            let model = std::rc::Rc::new(VecModel::from(hall_items));
            w.set_halls(ModelRc::from(model));
        }
    });

    // Create hall
    let state_create = state.clone();
    let window_weak = window.as_weak();
    window.on_create_hall(move |name| {
        let user_id = match state_create.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let name = name.to_string();
        if name.is_empty() {
            return;
        }

        let hall = Hall::new(name, user_id);
        let hall_id = hall.id;

        let db = state_create.db.lock().unwrap();

        if db.halls().create(&hall).is_err() {
            return;
        }

        // Add creator as Builder
        let membership = Membership::new(user_id, hall_id, HallRole::HallBuilder);
        if db.halls().add_member(&membership).is_err() {
            return;
        }

        // Initialize chest
        let chest = state_create.chest.lock().unwrap();
        let _ = chest.init_hall_chest(hall_id, &hall.name, HallRole::HallBuilder);

        drop(db);
        drop(chest);

        // Reload halls
        if let Some(w) = window_weak.upgrade() {
            w.invoke_load_halls();
        }
    });

    // Select hall
    let state_select = state.clone();
    let window_weak = window.as_weak();
    window.on_select_hall(move |hall_id_str| {
        let hall_id = match uuid::Uuid::parse_str(&hall_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let user_id = match state_select.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_select.db.lock().unwrap();

        // Get hall
        let hall = match db.halls().find_by_id(hall_id) {
            Ok(Some(h)) => h,
            _ => return,
        };

        // Get user's role
        let role = match db.halls().get_user_role(user_id, hall_id) {
            Ok(Some(r)) => r,
            _ => return,
        };

        // Mark user as online
        let _ = db.halls().update_online_status(user_id, hall_id, true);

        // Handle hosting
        let mut hall = hall;
        let mut hosting = HostingState::new();
        hosting.host_id = hall.current_host_id;
        hosting.election_epoch = hall.election_epoch;

        let current_host_role = if let Some(host_id) = hall.current_host_id {
            db.halls().get_user_role(host_id, hall_id).ok().flatten()
        } else {
            None
        };

        match hosting.on_user_join(user_id, role, current_host_role) {
            Some(HostElectionResult::Elected(new_host)) => {
                hosting.set_host(Some(new_host));
                hall.current_host_id = Some(new_host);
                hall.election_epoch = hosting.election_epoch;
                let _ = db.halls().update(&hall);
            }
            Some(HostElectionResult::PromptTakeover(_)) => {
                // For now, just update UI to show prompt option
                // Future: implement actual takeover dialog
            }
            _ => {}
        }

        // Get host username
        let host_name: SharedString = if let Some(host_id) = hall.current_host_id {
            if host_id == user_id {
                "You".into()
            } else {
                db.users()
                    .find_by_id(host_id)
                    .ok()
                    .flatten()
                    .map(|u| u.username)
                    .unwrap_or_else(|| "Unknown".to_string())
                    .into()
            }
        } else {
            "None".into()
        };

        drop(db);

        state_select.set_current_hall(Some(hall_id));

        if let Some(w) = window_weak.upgrade() {
            w.set_current_hall_id(hall_id.to_string().into());
            w.set_current_hall_name(hall.name.into());
            w.set_current_host_name(host_name);
            w.set_current_user_role(role.display_name().into());
            w.invoke_load_messages();
            w.invoke_load_members();
        }
    });

    // Join via invite
    let state_join = state.clone();
    let window_weak = window.as_weak();
    window.on_join_with_invite(move |token| {
        let user_id = match state_join.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let token = token.to_string();
        let db = state_join.db.lock().unwrap();

        // Find invite
        let invite = match db.invites().find_by_token(&token) {
            Ok(Some(inv)) if inv.is_valid() => inv,
            _ => {
                if let Some(w) = window_weak.upgrade() {
                    w.set_hall_error("Invalid or expired invite".into());
                }
                return;
            }
        };

        // Check not already member
        if let Ok(Some(_)) = db.halls().get_membership(user_id, invite.hall_id) {
            if let Some(w) = window_weak.upgrade() {
                w.set_hall_error("Already a member of this Hall".into());
            }
            return;
        }

        // Get hall name for chest init
        let hall = match db.halls().find_by_id(invite.hall_id) {
            Ok(Some(h)) => h,
            _ => return,
        };

        // Add membership
        let membership = Membership::new(user_id, invite.hall_id, invite.role);
        if db.halls().add_member(&membership).is_err() {
            if let Some(w) = window_weak.upgrade() {
                w.set_hall_error("Failed to join Hall".into());
            }
            return;
        }

        // Increment invite use count
        let _ = db.invites().increment_use_count(invite.id);

        // Init chest if role allows
        if invite.role >= HallRole::HallAgent {
            let chest = state_join.chest.lock().unwrap();
            let _ = chest.init_hall_chest(invite.hall_id, &hall.name, invite.role);
        }

        drop(db);

        if let Some(w) = window_weak.upgrade() {
            w.set_hall_error("".into());
            w.invoke_load_halls();
        }
    });

    // Create invite
    let state_invite = state.clone();
    let _window_weak = window.as_weak();
    window.on_create_invite(move |role_index| {
        let user_id = match state_invite.current_user_id() {
            Some(id) => id,
            None => return "".into(),
        };

        let hall_id = match state_invite.current_hall_id() {
            Some(id) => id,
            None => return "".into(),
        };

        let role = match role_index {
            0 => HallRole::HallFellow,
            1 => HallRole::HallAgent,
            2 => HallRole::HallModerator,
            3 => HallRole::HallPrefect,
            _ => HallRole::HallAgent,
        };

        // Generate random token
        let token: String = rand::thread_rng()
            .sample_iter(&rand::distributions::Alphanumeric)
            .take(16)
            .map(char::from)
            .collect();

        let invite = Invite::new(hall_id, user_id, role, token.clone()).with_expiry(24 * 7); // 1 week

        let db = state_invite.db.lock().unwrap();
        if db.invites().create(&invite).is_err() {
            return "".into();
        }

        drop(db);

        token.into()
    });

    // Leave hall
    let state_leave = state.clone();
    let window_weak = window.as_weak();
    window.on_leave_hall(move || {
        let user_id = match state_leave.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let hall_id = match state_leave.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_leave.db.lock().unwrap();

        // Check if user is owner
        let hall = match db.halls().find_by_id(hall_id) {
            Ok(Some(h)) => h,
            _ => return,
        };

        if hall.owner_id == user_id {
            // Can't leave own hall
            return;
        }

        // Remove membership
        let _ = db.halls().remove_member(user_id, hall_id);

        drop(db);

        state_leave.set_current_hall(None);

        if let Some(w) = window_weak.upgrade() {
            w.set_current_hall_id("".into());
            w.set_current_hall_name("".into());
            w.invoke_load_halls();
        }
    });
}
