//! Chat view model

use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Instant;

use chrono::{DateTime, Duration, Utc};
use exom_core::Message;
use exom_net::{NetMessage, NetRole};
use slint::{ComponentHandle, ModelRc, VecModel};
use tokio::sync::Mutex;

use crate::bot_runtime::BotRuntime;
use crate::network::{NetworkManager, NetworkState};
use crate::state::AppState;
use crate::MainWindow;
use crate::MessageItem;

/// Typing throttle state
struct TypingThrottle {
    /// Last time we sent typing=true
    last_sent: Option<Instant>,
    /// Timer ID for stop-typing timeout (stored to cancel)
    stop_timer_active: bool,
}

impl TypingThrottle {
    fn new() -> Self {
        Self {
            last_sent: None,
            stop_timer_active: false,
        }
    }

    /// Check if we should send a typing event (600ms throttle)
    fn should_send(&mut self) -> bool {
        let now = Instant::now();
        match self.last_sent {
            None => {
                self.last_sent = Some(now);
                true
            }
            Some(last) if now.duration_since(last).as_millis() >= 600 => {
                self.last_sent = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Reset throttle (called when stop-typing is sent)
    fn reset(&mut self) {
        self.last_sent = None;
        self.stop_timer_active = false;
    }
}

/// Combined message for sorting
enum CombinedMessage {
    Chat {
        id: String,
        sender_id: uuid::Uuid,
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
    bot_runtime: Arc<StdMutex<BotRuntime>>,
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

        // Get current host name and user for comparison
        let current_host = state_load.current_host_name();
        let current_user_id = state_load.current_user_id();

        // Combine and sort all messages by timestamp
        let mut combined: Vec<CombinedMessage> = Vec::new();

        for m in messages.iter() {
            let is_host = current_host
                .as_ref()
                .map(|h| h == &m.sender_username)
                .unwrap_or(false);

            combined.push(CombinedMessage::Chat {
                id: m.id.to_string(),
                sender_id: m.sender_id,
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
                    sender_id,
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

                    // Check if this is our own message
                    let is_own = current_user_id
                        .map(|uid| uid == *sender_id)
                        .unwrap_or(false);

                    // Check if message is pending delivery (only for own messages)
                    let message_uuid = uuid::Uuid::parse_str(id).ok();
                    let is_pending = is_own
                        && message_uuid
                            .map(|mid| state_load.is_message_pending(mid))
                            .unwrap_or(false);

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
                        is_own,
                        is_pending,
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
                        is_own: false,
                        is_pending: false,
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
    let bot_runtime_send = bot_runtime.clone();
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

        // Check for slash commands before treating as a regular message
        if content.starts_with("/archive") || content.starts_with("/set-archive") {
            if let Ok(mut runtime) = bot_runtime_send.try_lock() {
                let handled = runtime.handle_command(hall_id, user_id, &content);
                if handled {
                    // Reload messages to show system response
                    if let Some(w) = window_weak.upgrade() {
                        w.invoke_load_messages();
                    }
                    return;
                }
            }
        }

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

        // Track own activity
        state_send.update_member_activity(user_id);

        // Store locally
        let db = state_send.db.lock().unwrap();
        if db.messages().create(&message).is_err() {
            return;
        }
        drop(db);

        // Always mark as pending - will be confirmed on ACK or reconcile
        state_send.add_pending_message(message_id);

        // Send over network if connected (otherwise queued for reconnect)
        let network_manager_clone = network_manager_send.clone();
        tokio::spawn(async move {
            if let Ok(nm) = network_manager_clone.try_lock() {
                let net_state = nm.state().await;
                if net_state == NetworkState::Hosting || net_state == NetworkState::Connected {
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
                // If offline, message stays in pending set and DB
                // Will be re-sent on reconnect
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

    // Typing changed - throttle and debounce
    let state_typing = state.clone();
    let network_manager_typing = _network_manager.clone();
    let typing_throttle = Arc::new(StdMutex::new(TypingThrottle::new()));
    let typing_throttle_clone = typing_throttle.clone();
    let stop_typing_timer = Rc::new(StdMutex::new(None::<slint::Timer>));
    let stop_typing_timer_clone = stop_typing_timer.clone();

    window.on_typing_changed(move || {
        let user_id = match state_typing.current_user_id() {
            Some(id) => id,
            None => return,
        };

        let hall_id = match state_typing.current_hall_id() {
            Some(id) => id,
            None => return,
        };

        let username = state_typing.current_username().unwrap_or_default();

        // Check throttle
        let should_send = {
            let mut throttle = typing_throttle_clone.lock().unwrap();
            throttle.should_send()
        };

        if should_send {
            // Send typing=true
            let nm = network_manager_typing.clone();
            tokio::spawn(async move {
                if let Ok(nm) = nm.try_lock() {
                    let _ = nm
                        .send_typing(hall_id, user_id, username.clone(), true)
                        .await;
                }
            });
        }

        // Reset/start the stop-typing timer (1500ms)
        let nm_for_stop = network_manager_typing.clone();
        let throttle_for_stop = typing_throttle_clone.clone();
        let state_for_stop = state_typing.clone();

        // Create new timer for stop-typing
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::SingleShot,
            std::time::Duration::from_millis(1500),
            move || {
                // Reset throttle
                {
                    let mut throttle = throttle_for_stop.lock().unwrap();
                    throttle.reset();
                }

                // Send typing=false
                let user_id = match state_for_stop.current_user_id() {
                    Some(id) => id,
                    None => return,
                };
                let hall_id = match state_for_stop.current_hall_id() {
                    Some(id) => id,
                    None => return,
                };
                let username = state_for_stop.current_username().unwrap_or_default();

                let nm = nm_for_stop.clone();
                tokio::spawn(async move {
                    if let Ok(nm) = nm.try_lock() {
                        let _ = nm.send_typing(hall_id, user_id, username, false).await;
                    }
                });
            },
        );

        // Store timer to keep it alive (replaces previous)
        *stop_typing_timer_clone.lock().unwrap() = Some(timer);
    });
}
