//! View model bindings for Slint UI

mod auth;
mod chat;
mod halls;
mod members;

use crate::state::AppState;
use crate::MainWindow;
use std::sync::Arc;

pub fn setup_bindings(window: &MainWindow, state: AppState) {
    let state = Arc::new(state);

    auth::setup_auth_bindings(window, state.clone());
    halls::setup_hall_bindings(window, state.clone());
    chat::setup_chat_bindings(window, state.clone());
    members::setup_member_bindings(window, state.clone());
}
