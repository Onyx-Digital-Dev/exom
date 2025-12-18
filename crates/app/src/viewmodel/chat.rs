//! Chat view model

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use exom_core::Message;
use exom_net::{NetMessage, NetRole};
use slint::{ComponentHandle, ModelRc, VecModel};
use tokio::sync::Mutex;

use crate::network::{NetworkManager, NetworkState};
use crate::state::{AppState, SystemMessage};
use crate::MainWindow;
use crate::MessageItem;

/// Combined message for sorting
enum CombinedMessage {
    Chat {
        id: String,
        sender_username: String,
        sender_role: String,
        content: String,
        timestamp: DateTime<Utc>,
        is_edited: bool,
        is_host: bool,
    },
    System {
        id: String,
        content: String,
        timestamp: DateTime<Utc>,
    },
}

pub fn setup_chat_bindings(
    window: &MainWindow,
    state: Arc<AppState>,
    _network_manager: Arc<Mutex<NetworkManager>>,
) {
    // Load messages
    let state_load = state.clone();
    let window_weak = window.as_weak();
    window.on_load_messages(move || {
        let hall_id = match state_load.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let db = state_load.db.lock().unwrap();
        let messages = match db.messages().list_for_hall(hall_id, 100, None) {
            Ok(m) => m,
            Err(_) => return,
        };
        drop(db);

        // Get system messages for this hall
        let system_messages = state_load.get_system_messages(hall_id);

        // Get current host name for comparison
        let current_host = state_load.current_host_name();

        // Combine and sort all messages by timestamp
        let mut combined: Vec<CombinedMessage> = Vec::new();

        for m in messages.iter() {
            let is_host = current_host
                .as_ref()
                .map(|h| h == &m.sender_username)
                .unwrap_or(false);

            combined.push(CombinedMessage::Chat {
                id: m.id.to_string(),
                sender_username: m.sender_username.clone(),
                sender_role: m.sender_role.short_name().to_string(),
                content: m.content.clone(),
                timestamp: m.timestamp,
                is_edited: m.is_edited,
                is_host,
            });
        }

        for sm in system_messages.iter() {
            combined.push(CombinedMessage::System {
                id: sm.id.to_string(),
                content: sm.content.clone(),
                timestamp: sm.timestamp,
            });
        }

        // Sort by timestamp
        combined.sort_by(|a, b| {
            let ts_a = match a {
                CombinedMessage::Chat { timestamp, .. } => timestamp,
                CombinedMessage::System { timestamp, .. } => timestamp,
            };
            let ts_b = match b {
                CombinedMessage::Chat { timestamp, .. } => timestamp,
                CombinedMessage::System { timestamp, .. } => timestamp,
            };
            ts_a.cmp(ts_b)
        });

        // Build message items with grouping
        // Messages are grouped when same sender AND within 5 minutes
        let group_threshold = Duration::minutes(5);
        let mut message_items: Vec<MessageItem> = Vec::with_capacity(combined.len());
        let mut prev_sender: Option<String> = None;
        let mut prev_timestamp: Option<DateTime<Utc>> = None;

        for msg in combined.iter() {
            match msg {
                CombinedMessage::Chat {
                    id,
                    sender_username,
                    sender_role,
                    content,
                    timestamp,
                    is_edited,
                    is_host,
                } => {
                    // Start new group if different sender OR time gap > 5 minutes
                    let is_group_start = match (&prev_sender, prev_timestamp) {
                        (Some(sender), Some(ts)) => {
                            sender != sender_username
                                || timestamp.signed_duration_since(ts) > group_threshold
                        }
                        _ => true,
                    };

                    message_items.push(MessageItem {
                        id: id.clone().into(),
                        sender_name: sender_username.clone().into(),
                        sender_role: sender_role.clone().into(),
                        content: content.clone().into(),
                        timestamp: timestamp.format("%H:%M").to_string().into(),
                        is_edited: *is_edited,
                        is_group_start,
                        is_host: *is_host,
                        is_system: false,
                    });

                    prev_sender = Some(sender_username.clone());
                    prev_timestamp = Some(*timestamp);
                }
                CombinedMessage::System {
                    id,
                    content,
                    timestamp,
                } => {
                    // System messages always start a new group
                    message_items.push(MessageItem {
                        id: id.clone().into(),
                        sender_name: "".into(),
                        sender_role: "".into(),
                        content: content.clone().into(),
                        timestamp: timestamp.format("%H:%M").to_string().into(),
                        is_edited: false,
                        is_group_start: true,
                        is_host: false,
                        is_system: true,
                    });

                    // Reset grouping after system message
                    prev_sender = None;
                    prev_timestamp = None;
                }
            }
        }

        if let Some(w) = window_weak.upgrade() {
            let model = std::rc::Rc::new(VecModel::from(message_items));
            w.set_messages(ModelRc::from(model));
        }
    });

    // Send message
    let state_send = state.clone();
    let network_manager_send = _network_manager.clone();
    let window_weak = window.as_weak();
    window.on_send_message(move |content| {
        let content = content.to_string().trim().to_string();
        if content.is_empty() {
            return;
        }

        let user_id = match state_send.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let hall_id = match state_send.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let message = Message::new(hall_id, user_id, content.clone());
        let message_id = message.id;
        let timestamp = message.created_at;

        // Get user info for network message
        let (username, role_value) = {
            let db = state_send.db.lock().unwrap();
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

        // Store locally
        let db = state_send.db.lock().unwrap();
        if db.messages().create(&message).is_err() {
            return;
        }
        drop(db);

        // Send over network if connected
        let network_manager_clone = network_manager_send.clone();
        tokio::spawn(async move {
            if let Ok(nm) = network_manager_clone.try_lock() {
                let state = nm.state().await;
                if state == NetworkState::Hosting || state == NetworkState::Connected {
                    let net_msg = NetMessage {
                        id: message_id,
                        hall_id,
                        sender_id: user_id,
                        sender_username: username,
                        sender_role: NetRole::from_value(role_value),
                        content,
                        timestamp,
                        sequence: 0, // Assigned by host
                    };
                    let _ = nm.send_chat(net_msg).await;
                }
            }
        });

        // Reload messages
        if let Some(w) = window_weak.upgrade() {
            w.invoke_load_messages();
        }
    });

    // Delete message
    let state_delete = state.clone();
    let window_weak = window.as_weak();
    window.on_delete_message(move |message_id_str| {
        let message_id = match uuid::Uuid::parse_str(&message_id_str) {
            Ok(id) => id,
            Err(_) => return,
        };

        let db = state_delete.db.lock().unwrap();
        let _ = db.messages().delete(message_id);
        drop(db);

        if let Some(w) = window_weak.upgrade() {
            w.invoke_load_messages();
        }
    });
}
