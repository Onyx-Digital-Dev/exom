//! View model bindings for Slint UI

mod auth;
mod chat;
mod halls;
mod members;
mod network;
pub mod workspace;

use crate::bot_runtime::BotRuntime;
use crate::network::NetworkManager;
use crate::state::AppState;
use crate::MainWindow;

pub use self::workspace::WorkspaceManager;
use std::sync::Arc;
use tokio::sync::Mutex;

pub fn setup_bindings(
    window: &MainWindow,
    state: Arc<AppState>,
    network_manager: Arc<Mutex<NetworkManager>>,
    workspace_manager: Arc<std::sync::Mutex<WorkspaceManager>>,
    bot_runtime: Arc<std::sync::Mutex<BotRuntime>>,
) {
    auth::setup_auth_bindings(window, state.clone());
    halls::setup_hall_bindings(
        window,
        state.clone(),
        network_manager.clone(),
        workspace_manager.clone(),
    );
    chat::setup_chat_bindings(window, state.clone(), network_manager.clone());
    members::setup_member_bindings(window, state.clone());
    network::setup_network_bindings(window, state.clone(), network_manager.clone(), bot_runtime);
    workspace::setup_workspace_bindings(window, state.clone(), workspace_manager.clone());

    // Attempt auto-reconnect after session is restored (if user is logged in)
    network::try_auto_reconnect(state, network_manager);
}
