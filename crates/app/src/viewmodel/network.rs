//! Network view model bindings

use std::sync::Arc;

use chrono::Utc;
use exom_core::{LastConnection, Message};
use exom_net::{NetMessage, NetRole, PeerInfo};
use slint::{ComponentHandle, ModelRc, VecModel};

use tokio::sync::Mutex;

use crate::network::{ConnectionInfo, NetworkEvent, NetworkManager, NetworkState};
use crate::state::AppState;
use crate::MainWindow;
use crate::MemberItem;

pub fn setup_network_bindings(
    window: &MainWindow,
    state: Arc<AppState>,
    network_manager: Arc<Mutex<NetworkManager>>,
) {
    // Poll for network events periodically
    let window_weak = window.as_weak();
    let network_manager_clone = network_manager.clone();
    let state_clone = state.clone();

    // Set up a timer to poll network events
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let window_weak = window_weak.clone();
            let state_clone = state_clone.clone();

            // Collect events while holding lock, then process
            let events: Vec<NetworkEvent> = {
                if let Ok(mut nm) = network_manager_clone.try_lock() {
                    let mut events = Vec::new();
                    while let Some(event) = nm.try_recv_event() {
                        events.push(event);
                    }
                    events
                } else {
                    Vec::new()
                }
            };

            // Process events after releasing lock
            for event in events {
                if let Some(window) = window_weak.upgrade() {
                    handle_network_event(&window, &state_clone, event);
                }
            }
        },
    );

    // Copy invite callback
    let network_manager_clone = network_manager.clone();
    let window_weak = window.as_weak();
    window.on_copy_invite(move || {
        // Get invite URL while holding lock
        let invite_url: Option<String> = {
            if let Ok(nm) = network_manager_clone.try_lock() {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(nm.invite_url())
                })
            } else {
                None
            }
        };

        // Set URL after releasing lock
        if let Some(url) = invite_url {
            if let Some(w) = window_weak.upgrade() {
                w.set_invite_url(url.into());
            }
        }
    });
}

fn handle_network_event(window: &MainWindow, state: &Arc<AppState>, event: NetworkEvent) {
    match event {
        NetworkEvent::StateChanged(net_state) => {
            let status = match net_state {
                NetworkState::Offline => "Offline",
                NetworkState::Connecting => "Connecting...",
                NetworkState::Connected => "Connected",
                NetworkState::Hosting => "Hosting",
                NetworkState::Reconnecting => "Reconnecting...",
            };
            window.set_network_status(status.into());
        }
        NetworkEvent::ChatReceived(net_msg) => {
            // Store incoming message locally if it's for the current hall
            // and not from ourselves
            let current_hall = state.current_hall_id();
            let current_user = state.current_user_id();

            if let Some(hall_id) = current_hall {
                // Only store if message is for current hall and not from self
                if net_msg.hall_id == hall_id
                    && current_user.map_or(true, |uid| uid != net_msg.sender_id)
                {
                    store_network_message(state, &net_msg);
                    // Trigger UI refresh
                    window.invoke_load_messages();
                }
            }
        }
        NetworkEvent::MembersUpdated(members) => {
            // Update the members list from network peers
            update_network_members(window, state, &members);
        }
        NetworkEvent::ConnectionFailed(reason) => {
            window.set_network_status(format!("Error: {}", reason).into());
        }
        NetworkEvent::Disconnected => {
            window.set_network_status("Disconnected".into());
            // Reload members from local database
            window.invoke_load_members();
        }
        NetworkEvent::HostDisconnected { hall_id, was_host } => {
            if was_host {
                // We were the host - just show disconnected
                window.set_network_status("Server stopped".into());
            } else {
                // Host disconnected - we could potentially take over
                tracing::info!(hall_id = %hall_id, "Host disconnected - session ended");
                window.set_network_status("Host left - reconnect needed".into());
            }
            // Reload members from local database
            window.invoke_load_members();
        }
        NetworkEvent::HostChanged { new_host_id } => {
            // Host changed - update UI
            tracing::info!(new_host_id = %new_host_id, "Host changed");
            // The members list will be updated via MembersUpdated event
        }
        NetworkEvent::Connected(conn_info) => {
            // Persist connection info for auto-reconnect
            persist_connection(state, &conn_info);
        }
        NetworkEvent::ElectionInProgress => {
            window.set_network_status("Election in progress...".into());
        }
        NetworkEvent::BecameHost { port } => {
            tracing::info!(port = port, "This node became host after election");
            window.set_network_status(format!("Hosting (port {})", port).into());
        }
    }
}

/// Persist connection info for auto-reconnect on next launch
fn persist_connection(state: &Arc<AppState>, conn_info: &ConnectionInfo) {
    let user_id = match state.current_user_id() {
        Some(id) => id,
        None => return,
    };

    let last_conn = LastConnection {
        user_id,
        hall_id: conn_info.hall_id,
        invite_url: conn_info.invite_url.clone(),
        host_addr: conn_info.host_addr.clone(),
        last_connected_at: Utc::now(),
        epoch: conn_info.epoch,
    };

    let db = state.db.lock().unwrap();
    if let Err(e) = db.connections().save_last_connection(&last_conn) {
        tracing::warn!(error = %e, "Failed to persist connection info");
    } else {
        tracing::debug!(hall_id = %conn_info.hall_id, "Connection info persisted");
    }
}

/// Store a network message in the local database
fn store_network_message(state: &Arc<AppState>, net_msg: &NetMessage) {
    // Create a local message from the network message
    let message = Message {
        id: net_msg.id,
        hall_id: net_msg.hall_id,
        sender_id: net_msg.sender_id,
        content: net_msg.content.clone(),
        created_at: net_msg.timestamp,
        edited_at: None,
        is_deleted: false,
    };

    // Store in database
    let db = state.db.lock().unwrap();
    if let Err(e) = db.messages().create(&message) {
        tracing::warn!(error = %e, "Failed to store network message");
    }
}

/// Update the members list from network peer info
fn update_network_members(window: &MainWindow, state: &Arc<AppState>, peers: &[PeerInfo]) {
    let current_user_id = state.current_user_id();

    let member_items: Vec<MemberItem> = peers
        .iter()
        .map(|p| MemberItem {
            id: p.user_id.to_string().into(),
            name: p.username.clone().into(),
            role: net_role_display(p.role).into(),
            is_online: true, // All network peers are online
            is_host: p.is_host,
            is_you: current_user_id == Some(p.user_id),
        })
        .collect();

    let model = std::rc::Rc::new(VecModel::from(member_items));
    window.set_members(ModelRc::from(model));

    // Update current user id for context actions
    if let Some(uid) = current_user_id {
        window.set_current_user_id(uid.to_string().into());
    }
}

/// Convert NetRole to display string
fn net_role_display(role: NetRole) -> &'static str {
    match role {
        NetRole::Builder => "Builder",
        NetRole::Prefect => "Prefect",
        NetRole::Moderator => "Moderator",
        NetRole::Agent => "Agent",
        NetRole::Fellow => "Fellow",
    }
}

/// Attempt auto-reconnect to last hall if available
/// Should be called after session restore
pub fn try_auto_reconnect(state: Arc<AppState>, network_manager: Arc<Mutex<NetworkManager>>) {
    let user_id = match state.current_user_id() {
        Some(id) => id,
        None => return,
    };

    // Get last connection info
    let last_conn = {
        let db = state.db.lock().unwrap();
        match db.connections().get_last_connection(user_id) {
            Ok(Some(conn)) => conn,
            Ok(None) => {
                tracing::debug!("No previous connection to restore");
                return;
            }
            Err(e) => {
                tracing::warn!(error = %e, "Failed to get last connection");
                return;
            }
        }
    };

    // Get username for reconnection
    let username = {
        let db = state.db.lock().unwrap();
        db.users()
            .find_by_id(user_id)
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_else(|| "Unknown".to_string())
    };

    tracing::info!(
        hall_id = %last_conn.hall_id,
        invite_url = %last_conn.invite_url,
        "Starting auto-reconnect to last hall"
    );

    // Start reconnect with backoff
    tokio::spawn(async move {
        if let Ok(nm) = network_manager.try_lock() {
            let _ = nm
                .start_reconnect(
                    last_conn.invite_url,
                    user_id,
                    username,
                    NetRole::Agent, // Default role for reconnect
                )
                .await;
        }
    });
}
