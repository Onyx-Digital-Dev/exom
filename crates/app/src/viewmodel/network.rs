//! Network view model bindings

use std::sync::Arc;

use chrono::Utc;
use exom_core::{LastConnection, Message};
use exom_net::{NetMessage, NetRole, PeerInfo};
use slint::{ComponentHandle, ModelRc, VecModel};

use tokio::sync::Mutex;

use crate::bot_runtime::BotRuntime;
use crate::network::{
    ConnectionInfo, ConnectionQuality, NetworkEvent, NetworkManager, NetworkState,
};
use crate::state::{AppState, TrackedMember};
use crate::MainWindow;
use crate::MemberItem;

/// Set network status with connectivity indicator
fn set_network_status(window: &MainWindow, status: &str, connected: bool) {
    window.set_network_status(status.into());
    window.set_is_network_connected(connected);
}

pub fn setup_network_bindings(
    window: &MainWindow,
    state: Arc<AppState>,
    network_manager: Arc<Mutex<NetworkManager>>,
    bot_runtime: Arc<std::sync::Mutex<BotRuntime>>,
) {
    // Poll for network events periodically
    let window_weak = window.as_weak();
    let network_manager_clone = network_manager.clone();
    let state_clone = state.clone();
    let bot_runtime_clone = bot_runtime.clone();

    // Set up a timer to poll network events
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let window_weak = window_weak.clone();
            let state_clone = state_clone.clone();
            let bot_runtime_clone = bot_runtime_clone.clone();

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
                    handle_network_event(&window, &state_clone, &network_manager_clone, &bot_runtime_clone, event);
                }
            }
        },
    );

    // Set up a timer to prune stale typing indicators (250ms interval, 2s stale threshold)
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let typing_prune_timer = slint::Timer::default();
    typing_prune_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(250),
        move || {
            if state_clone.prune_typing_users(2000) {
                if let Some(window) = window_weak.upgrade() {
                    update_typing_indicator(&window, &state_clone);
                }
            }
        },
    );
    // Keep timer alive
    std::mem::forget(typing_prune_timer);

    // Set up a timer for bot scheduler ticks (every 60 seconds)
    let window_weak = window.as_weak();
    let state_clone = state.clone();
    let bot_runtime_clone = bot_runtime.clone();
    let scheduler_timer = slint::Timer::default();
    scheduler_timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_secs(60),
        move || {
            if let Some(hall_id) = state_clone.current_hall_id() {
                if let Ok(mut runtime) = bot_runtime_clone.try_lock() {
                    runtime.tick_scheduled(hall_id);
                }
                // Reload messages to show any archive notifications
                if let Some(window) = window_weak.upgrade() {
                    window.invoke_load_messages();
                }
            }
        },
    );
    // Keep timer alive
    std::mem::forget(scheduler_timer);

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

    // Regenerate invite callback (host only)
    let network_manager_clone = network_manager.clone();
    window.on_regenerate_invite(move || {
        let nm = network_manager_clone.clone();
        tokio::spawn(async move {
            if let Ok(nm) = nm.try_lock() {
                let _ = nm.regenerate_invite().await;
            }
        });
    });
}

fn handle_network_event(
    window: &MainWindow,
    state: &Arc<AppState>,
    network_manager: &Arc<Mutex<NetworkManager>>,
    bot_runtime: &Arc<std::sync::Mutex<BotRuntime>>,
    event: NetworkEvent,
) {
    match event {
        NetworkEvent::StateChanged(net_state) => {
            let (status, connected) = match net_state {
                NetworkState::Offline => ("Working offline", false),
                NetworkState::Connecting => ("Connecting...", false),
                NetworkState::Connected => ("Connected", true),
                NetworkState::Hosting => ("Hosting", true),
                NetworkState::Reconnecting => ("Reconnecting...", false),
            };
            set_network_status(window, status, connected);
        }
        NetworkEvent::HostingAt { addr: _, port } => {
            set_network_status(window, &format!("Hosting on port {}", port), true);
        }
        NetworkEvent::ConnectedTo { addr: _ } => {
            set_network_status(window, "Connected", true);
        }
        NetworkEvent::ReconnectRetry {
            attempt: _,
            next_in_secs,
        } => {
            set_network_status(
                window,
                &format!("Reconnecting in {}s...", next_in_secs),
                false,
            );
        }
        NetworkEvent::ChatReceived(net_msg) => {
            // Store incoming message locally if it's for the current hall
            // and not from ourselves
            let current_hall = state.current_hall_id();
            let current_user = state.current_user_id();

            // Track sender activity
            state.update_member_activity(net_msg.sender_id);

            if let Some(hall_id) = current_hall {
                // Only store if message is for current hall and not from self
                if net_msg.hall_id == hall_id && current_user != Some(net_msg.sender_id) {
                    store_network_message(state, &net_msg);
                    // Trigger UI refresh
                    window.invoke_load_messages();
                    // Refresh members to update activity hints
                    window.invoke_load_members();
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

            // Dispatch presence events to bot runtime (Town Crier)
            if let Some(hall_id) = state.current_hall_id() {
                if let Ok(mut runtime) = bot_runtime.try_lock() {
                    for member in joined {
                        runtime.on_member_joined(hall_id, member.user_id, member.username);
                    }
                    for member in left {
                        runtime.on_member_left(hall_id, member.user_id, member.username);
                    }
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
            // Log the technical reason, show human-friendly message
            tracing::warn!(reason = %reason, "Connection failed");
            set_network_status(window, "Connection failed", false);
        }
        NetworkEvent::Disconnected => {
            // Add system message
            if let Some(hall_id) = state.current_hall_id() {
                state.add_system_message(hall_id, "Connection lost".to_string());
                window.invoke_load_messages();
            }
            // Clear known members and typing users on disconnect
            state.clear_known_members();
            state.clear_all_typing();
            update_typing_indicator(window, state);
            set_network_status(window, "Disconnected", false);
            // Clear connection quality
            window.set_connection_quality("".into());
            // Reload members from local database
            window.invoke_load_members();
        }
        NetworkEvent::HostDisconnected { hall_id, was_host } => {
            if was_host {
                // We were the host - just show disconnected
                set_network_status(window, "Session ended", false);
            } else {
                // Host disconnected - we could potentially take over
                tracing::info!(hall_id = %hall_id, "Host disconnected - session ended");
                set_network_status(window, "Host disconnected", false);
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

            // Re-send any pending messages that weren't synced
            resend_pending_messages(state, network_manager);

            // Reconcile any pending messages from previous session
            if let Some(hall_id) = state.current_hall_id() {
                let reconciled = state.reconcile_pending_messages(hall_id);
                if reconciled > 0 {
                    tracing::debug!(count = reconciled, "Reconciled pending messages on connect");
                    window.invoke_load_messages();
                }

                // Notify bots that hall is connected (for startup tasks like missed runs check)
                if let Ok(mut runtime) = bot_runtime.try_lock() {
                    runtime.on_hall_connected(hall_id);
                }
            }
        }
        NetworkEvent::ElectionInProgress => {
            set_network_status(window, "Choosing new host...", false);
        }
        NetworkEvent::BecameHost { port } => {
            tracing::info!(port = port, "This node became host after election");
            // Add system message
            if let Some(hall_id) = state.current_hall_id() {
                state.add_system_message(hall_id, "You are now the host".to_string());
                window.invoke_load_messages();
            }
            set_network_status(window, "Now hosting", true);
        }
        NetworkEvent::SyncBatchReceived { messages } => {
            // Store all synced messages (deduplication via INSERT OR IGNORE)
            let current_hall = state.current_hall_id();
            if let Some(hall_id) = current_hall {
                let mut stored = 0;
                let mut confirmed = 0;
                for net_msg in messages {
                    if net_msg.hall_id == hall_id {
                        store_network_message(state, &net_msg);
                        stored += 1;

                        // Reconcile: if this message was pending, confirm it
                        if state.is_message_pending(net_msg.id) {
                            state.confirm_message(net_msg.id);
                            confirmed += 1;
                        }
                    }
                }
                if stored > 0 {
                    tracing::debug!(
                        count = stored,
                        confirmed = confirmed,
                        "Synced messages from batch"
                    );
                    window.invoke_load_messages();
                }
            }
        }
        NetworkEvent::MessageAcked { message_id } => {
            // Mark message as delivered and refresh UI
            state.confirm_message(message_id);
            window.invoke_load_messages();
        }
        NetworkEvent::TypingReceived {
            hall_id: _,
            user_id,
            username,
            is_typing,
        } => {
            // Track user activity on typing
            state.update_member_activity(user_id);

            // Update typing state (exclude self via get_typing_users)
            if is_typing {
                state.set_user_typing(user_id, username);
            } else {
                state.clear_user_typing(user_id);
            }
            // Update typing indicator in UI
            update_typing_indicator(window, state);
            // Refresh members to update activity hints
            window.invoke_load_members();
        }
        NetworkEvent::QualityChanged(quality) => {
            let hint = match quality {
                ConnectionQuality::Good => "Good",
                ConnectionQuality::Ok => "OK",
                ConnectionQuality::Poor => "Poor",
            };
            window.set_connection_quality(hint.into());
        }
        NetworkEvent::InviteChanged(new_url) => {
            // Update the invite URL in the UI
            window.set_invite_url(new_url.into());
        }
    }
}

/// Re-send any pending messages that weren't ACKed before disconnect
fn resend_pending_messages(state: &Arc<AppState>, network_manager: &Arc<Mutex<NetworkManager>>) {
    let pending_ids = state.get_pending_messages();
    if pending_ids.is_empty() {
        return;
    }

    let current_hall = state.current_hall_id();
    let current_user = state.current_user_id();
    let (hall_id, user_id) = match (current_hall, current_user) {
        (Some(h), Some(u)) => (h, u),
        _ => return,
    };

    // Get user info for network messages
    let (username, role_value) = {
        let db = state.db.lock().unwrap();
        let username = db
            .users()
            .find_by_id(user_id)
            .ok()
            .flatten()
            .map(|u| u.username)
            .unwrap_or_else(|| "Unknown".to_string());
        let role = db
            .halls()
            .get_user_role(user_id, hall_id)
            .ok()
            .flatten()
            .map(|r| r as u8)
            .unwrap_or(1);
        (username, role)
    };

    // Get pending messages from database
    let messages_to_send: Vec<(uuid::Uuid, String, chrono::DateTime<chrono::Utc>)> = {
        let db = state.db.lock().unwrap();
        pending_ids
            .iter()
            .filter_map(|&msg_id| {
                db.messages()
                    .find_by_id(msg_id)
                    .ok()
                    .flatten()
                    .filter(|m| m.sender_id == user_id && m.hall_id == hall_id)
                    .map(|m| (m.id, m.content, m.created_at))
            })
            .collect()
    };

    if messages_to_send.is_empty() {
        return;
    }

    tracing::info!(
        count = messages_to_send.len(),
        "Re-sending pending messages"
    );

    // Send each pending message
    let nm = network_manager.clone();
    let username = username.clone();
    tokio::spawn(async move {
        if let Ok(nm) = nm.try_lock() {
            for (msg_id, content, timestamp) in messages_to_send {
                let net_msg = NetMessage {
                    id: msg_id,
                    hall_id,
                    sender_id: user_id,
                    sender_username: username.clone(),
                    sender_role: NetRole::from_value(role_value),
                    content,
                    timestamp,
                    sequence: 0, // Assigned by host
                };
                if let Err(e) = nm.send_chat(net_msg).await {
                    tracing::warn!(error = %e, msg_id = %msg_id, "Failed to resend message");
                }
            }
        }
    });
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
        .map(|p| {
            // Update state with peer's presence info (K2/K3)
            state.update_member_presence(
                p.user_id,
                p.username.clone(),
                p.presence,
                p.current_tool,
            );

            MemberItem {
                id: p.user_id.to_string().into(),
                name: p.username.clone().into(),
                role: net_role_display(p.role).into(),
                is_online: true, // All network peers are online
                is_host: p.is_host,
                is_you: current_user_id == Some(p.user_id),
                activity_hint: state.get_activity_hint(p.user_id).into(),
                presence: p.presence.label().into(),
                current_tool: p.current_tool.activity_label().into(),
            }
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

/// Update the typing indicator text in the UI
fn update_typing_indicator(window: &MainWindow, state: &Arc<AppState>) {
    let typing_users = state.get_typing_users();
    let text = format_typing_text(&typing_users);
    window.set_typing_indicator(text.into());
}

/// Format typing indicator text
fn format_typing_text(typing_users: &[(uuid::Uuid, String)]) -> String {
    match typing_users.len() {
        0 => String::new(),
        1 => format!("{} typing...", typing_users[0].1),
        2 => format!("{}, {} typing...", typing_users[0].1, typing_users[1].1),
        3 => format!(
            "{}, {}, {} typing...",
            typing_users[0].1, typing_users[1].1, typing_users[2].1
        ),
        _ => "Several people typing...".to_string(),
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
