//! Exom - Hall-based collaboration platform

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod state;
mod viewmodel;

slint::include_modules!();

fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    tracing::info!("Starting Exom");

    // Initialize application state
    let app_state = match state::AppState::new() {
        Ok(state) => state,
        Err(e) => {
            tracing::error!("Failed to initialize application: {}", e);
            std::process::exit(1);
        }
    };

    // Create main window
    let main_window = MainWindow::new().unwrap();

    // Set up view model bindings
    viewmodel::setup_bindings(&main_window, app_state);

    // Run the application
    main_window.run().unwrap();
}
