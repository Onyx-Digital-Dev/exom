//! Network view model bindings

use std::sync::Arc;

use slint::ComponentHandle;
use tokio::sync::Mutex;

use crate::network::{NetworkEvent, NetworkManager, NetworkState};
use crate::state::AppState;
use crate::MainWindow;

pub fn setup_network_bindings(
    window: &MainWindow,
    _state: Arc<AppState>,
    network_manager: Arc<Mutex<NetworkManager>>,
) {
    // Poll for network events periodically
    let window_weak = window.as_weak();
    let network_manager_clone = network_manager.clone();

    // Set up a timer to poll network events
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(50),
        move || {
            let window_weak = window_weak.clone();

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
                    handle_network_event(&window, event);
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

fn handle_network_event(window: &MainWindow, event: NetworkEvent) {
    match event {
        NetworkEvent::StateChanged(state) => {
            let status = match state {
                NetworkState::Offline => "Offline",
                NetworkState::Connecting => "Connecting...",
                NetworkState::Connected => "Connected",
                NetworkState::Hosting => "Hosting",
            };
            window.set_network_status(status.into());
        }
        NetworkEvent::ChatReceived(msg) => {
            // This will be handled by the chat viewmodel via a callback
            // For now, just log it
            tracing::debug!(sender = %msg.sender_username, content = %msg.content, "Chat received");
        }
        NetworkEvent::MembersUpdated(members) => {
            // This will be handled to update the members list
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
