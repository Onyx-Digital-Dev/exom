//! Network view model bindings

use std::sync::Arc;

use chrono::Utc;
use exom_core::{LastConnection, Message};
use exom_net::{NetMessage, NetRole, PeerInfo};
use slint::{ComponentHandle, ModelRc, VecModel};

use tokio::sync::Mutex;

use crate::network::{ConnectionInfo, NetworkEvent, NetworkManager, NetworkState};
use crate::state::{AppState, TrackedMember};
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

        // Copy to clipboard and set URL after releasing lock
        if let Some(url) = invite_url {
            let copy_result = copy_to_clipboard(&url);

            if let Some(w) = window_weak.upgrade() {
                w.set_invite_url(url.into());

                if !copy_result {
                    // Show failure for 3 seconds, then restore previous status
                    let prev_status = w.get_network_status().to_string();
                    w.set_network_status("Copy failed".into());

                    let window_weak2 = w.as_weak();
                    let timer = slint::Timer::default();
                    timer.start(
                        slint::TimerMode::SingleShot,
                        std::time::Duration::from_secs(3),
                        move || {
                            if let Some(w2) = window_weak2.upgrade() {
                                w2.set_network_status(prev_status.clone().into());
                            }
                        },
                    );
                    // Keep timer alive by forgetting it
                    std::mem::forget(timer);
                }
            }
        }
    });
}

fn handle_network_event(window: &MainWindow, state: &Arc<AppState>, event: NetworkEvent) {
    match event {
        NetworkEvent::StateChanged(net_state) => {
            let status = match net_state {
                NetworkState::Offline => "Offline (local only)".to_string(),
                NetworkState::Connecting => "Connecting...".to_string(),
                NetworkState::Connected => "Connected (Client)".to_string(),
                NetworkState::Hosting => "Connected (Host)".to_string(),
                NetworkState::Reconnecting => "Reconnecting...".to_string(),
            };
            window.set_network_status(status.into());
        }
        NetworkEvent::HostingAt { addr, port } => {
            window.set_network_status(format!("Connected (Host) - {}:{}", addr, port).into());
        }
        NetworkEvent::ConnectedTo { addr } => {
            window.set_network_status(format!("Connected (Client) - {}", addr).into());
        }
        NetworkEvent::ReconnectRetry {
            attempt,
            next_in_secs,
        } => {
            window.set_network_status(
                format!("Reconnecting... (retry {} in {}s)", attempt, next_in_secs).into(),
            );
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
            // Detect who joined and left
            let tracked: Vec<TrackedMember> = members
                .iter()
                .map(|m| TrackedMember {
                    user_id: m.user_id,
                    username: m.username.clone(),
                })
                .collect();
            let (joined, left) = state.update_known_members(tracked);
            let has_changes = !joined.is_empty() || !left.is_empty();

            // Add system messages for joins/leaves
            if let Some(hall_id) = state.current_hall_id() {
                for name in joined {
                    state.add_system_message(hall_id, format!("{} joined the hall", name));
                }
                for name in left {
                    state.add_system_message(hall_id, format!("{} left the hall", name));
                }
            }

            // Update the members list from network peers
            update_network_members(window, state, &members);

            // Refresh messages to show system messages
            if has_changes {
                window.invoke_load_messages();
            }
        }
        NetworkEvent::ConnectionFailed(reason) => {
            window.set_network_status(format!("Error: {}", reason).into());
        }
        NetworkEvent::Disconnected => {
            // Add system message
            if let Some(hall_id) = state.current_hall_id() {
                state.add_system_message(hall_id, "Connection lost".to_string());
                window.invoke_load_messages();
            }
            // Clear known members on disconnect
            state.clear_known_members();
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

            // Add system message with host name
            if let Some(hall_id) = state.current_hall_id() {
                // Try to get host name from known members or database
                let host_name = {
                    let known = state.known_members.lock().unwrap();
                    known
                        .iter()
                        .find(|m| m.user_id == new_host_id)
                        .map(|m| m.username.clone())
                };
                let name = host_name.unwrap_or_else(|| "someone".to_string());
                state.add_system_message(hall_id, format!("Host changed to {}", name));
                window.invoke_load_messages();
            }
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
            // Add system message
            if let Some(hall_id) = state.current_hall_id() {
                state.add_system_message(hall_id, "You are now the host".to_string());
                window.invoke_load_messages();
            }
            window.set_network_status(format!("Hosting (port {})", port).into());
        }
        NetworkEvent::SyncBatchReceived { messages } => {
            // Store all synced messages (deduplication via INSERT OR IGNORE)
            let current_hall = state.current_hall_id();
            if let Some(hall_id) = current_hall {
                let mut stored = 0;
                for net_msg in messages {
                    if net_msg.hall_id == hall_id {
                        store_network_message(state, &net_msg);
                        stored += 1;
                    }
                }
                if stored > 0 {
                    tracing::debug!(count = stored, "Synced messages from batch");
                    window.invoke_load_messages();
                }
            }
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
        sequence: if net_msg.sequence > 0 {
            Some(net_msg.sequence)
        } else {
            None
        },
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

/// Copy text to clipboard with fallback for Wayland.
/// Returns true if successful, false if all methods failed.
fn copy_to_clipboard(text: &str) -> bool {
    // Try arboard first (works on X11 and some Wayland)
    if let Ok(mut clipboard) = arboard::Clipboard::new() {
        if clipboard.set_text(text).is_ok() {
            tracing::debug!("Copied to clipboard via arboard");
            return true;
        }
    }

    // Fallback: try wl-copy for Wayland
    if try_wl_copy(text) {
        tracing::debug!("Copied to clipboard via wl-copy");
        return true;
    }

    tracing::warn!("All clipboard methods failed");
    false
}

/// Try to copy using wl-copy (Wayland clipboard tool).
fn try_wl_copy(text: &str) -> bool {
    use std::io::Write;
    use std::process::{Command, Stdio};

    let mut child = match Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(child) => child,
        Err(_) => return false,
    };

    if let Some(mut stdin) = child.stdin.take() {
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }

    matches!(child.wait(), Ok(status) if status.success())
}
