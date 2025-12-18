//! Chat view model

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use exom_core::Message;
use slint::{ComponentHandle, ModelRc, VecModel};
use tokio::sync::Mutex;

use crate::network::NetworkManager;
use crate::state::AppState;
use crate::MainWindow;
use crate::MessageItem;

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

        // Get current host name for comparison
        let current_host = state_load.current_host_name();

        // Build message items with grouping
        // Messages are grouped when same sender AND within 5 minutes
        let group_threshold = Duration::minutes(5);
        let mut message_items: Vec<MessageItem> = Vec::with_capacity(messages.len());
        let mut prev_sender: Option<String> = None;
        let mut prev_timestamp: Option<DateTime<Utc>> = None;

        for m in messages.iter() {
            // Start new group if different sender OR time gap > 5 minutes
            let is_group_start = match (&prev_sender, prev_timestamp) {
                (Some(sender), Some(ts)) => {
                    sender != &m.sender_username
                        || m.timestamp.signed_duration_since(ts) > group_threshold
                }
                _ => true,
            };

            let is_host = current_host
                .as_ref()
                .map(|h| h == &m.sender_username)
                .unwrap_or(false);

            message_items.push(MessageItem {
                id: m.id.to_string().into(),
                sender_name: m.sender_username.clone().into(),
                sender_role: m.sender_role.short_name().into(),
                content: m.content.clone().into(),
                timestamp: m.format_timestamp().into(),
                is_edited: m.is_edited,
                is_group_start,
                is_host,
            });

            prev_sender = Some(m.sender_username.clone());
            prev_timestamp = Some(m.timestamp);
        }

        drop(db);

        if let Some(w) = window_weak.upgrade() {
            let model = std::rc::Rc::new(VecModel::from(message_items));
            w.set_messages(ModelRc::from(model));
        }
    });

    // Send message
    let state_send = state.clone();
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

        let message = Message::new(hall_id, user_id, content);

        let db = state_send.db.lock().unwrap();
        if db.messages().create(&message).is_err() {
            return;
        }
        drop(db);

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
