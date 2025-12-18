//! Network view model bindings

use std::sync::Arc;

use exom_core::Message;
use exom_net::NetMessage;
use slint::ComponentHandle;
use tokio::sync::Mutex;

use crate::network::{NetworkEvent, NetworkManager, NetworkState};
use crate::state::AppState;
use crate::MainWindow;

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
            // This will be handled in Phase D to update the members list
            tracing::debug!(count = members.len(), "Members updated");
        }
        NetworkEvent::ConnectionFailed(reason) => {
            window.set_network_status(format!("Error: {}", reason).into());
        }
        NetworkEvent::Disconnected => {
            window.set_network_status("Disconnected".into());
        }
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
