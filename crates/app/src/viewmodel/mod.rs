//! View model bindings for Slint UI

mod auth;
mod chat;
mod halls;
mod members;
mod network;

use crate::network::NetworkManager;
use crate::state::AppState;
use crate::MainWindow;
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn setup_bindings(
    window: &MainWindow,
    state: AppState,
    network_manager: Arc<Mutex<NetworkManager>>,
) {
    let state = Arc::new(state);

    auth::setup_auth_bindings(window, state.clone());
    halls::setup_hall_bindings(window, state.clone(), network_manager.clone());
    chat::setup_chat_bindings(window, state.clone(), network_manager.clone());
    members::setup_member_bindings(window, state.clone());
    network::setup_network_bindings(window, state.clone(), network_manager);
}
