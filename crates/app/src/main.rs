//! Exom - Hall-based collaboration platform
//!
//! A Wayland-first desktop application for hall-based collaboration.
//! Supports X11 via XWayland for compatibility.

use std::sync::Arc;

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod archivist;
mod bot_runtime;
mod external_tools;
mod network;
mod platform;
mod state;
mod town_crier;
mod viewmodel;
mod workspace;

slint::include_modules!();

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting Exom");

    // Log platform and display server information
    platform::log_platform_info();

    // Initialize tokio runtime for networking
    let runtime = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
    let _guard = runtime.enter();

    // Initialize application state
    let app_state = match state::AppState::new() {
        Ok(state) => Arc::new(state),
        Err(e) => {
            tracing::error!("Failed to initialize application: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize network manager
    let network_manager = Arc::new(tokio::sync::Mutex::new(network::NetworkManager::new()));

    // Initialize workspace manager
    let workspace_manager = Arc::new(std::sync::Mutex::new(
        viewmodel::WorkspaceManager::new(app_state.data_dir().to_path_buf()),
    ));

    // Initialize bot runtime with Town Crier
    let bot_runtime = Arc::new(std::sync::Mutex::new(
        bot_runtime::BotRuntime::new(app_state.clone()),
    ));

    // Create main window
    let main_window = MainWindow::new().unwrap();

    // Set up view model bindings
    viewmodel::setup_bindings(
        &main_window,
        app_state,
        network_manager,
        workspace_manager,
        bot_runtime,
    );

    // Run the application
    main_window.run().unwrap();
}
