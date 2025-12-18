//! Platform detection and display server compatibility
//!
//! Exom is designed Wayland-first but supports X11 via XWayland.
//! This module handles display server detection and provides
//! platform-specific utilities.

use std::env;

/// Detected display server type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayServer {
    /// Native Wayland session
    Wayland,
    /// X11 session (native or XWayland)
    X11,
    /// Unknown or headless
    Unknown,
}

impl DisplayServer {
    /// Detect the current display server from environment
    pub fn detect() -> Self {
        // Check for Wayland first (preferred)
        if env::var("WAYLAND_DISPLAY").is_ok() {
            return DisplayServer::Wayland;
        }

        // Fall back to X11
        if env::var("DISPLAY").is_ok() {
            return DisplayServer::X11;
        }

        DisplayServer::Unknown
    }

    /// Returns true if running under Wayland
    pub fn is_wayland(&self) -> bool {
        matches!(self, DisplayServer::Wayland)
    }

    /// Returns true if running under X11
    pub fn is_x11(&self) -> bool {
        matches!(self, DisplayServer::X11)
    }
}

impl std::fmt::Display for DisplayServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisplayServer::Wayland => write!(f, "Wayland"),
            DisplayServer::X11 => write!(f, "X11"),
            DisplayServer::Unknown => write!(f, "Unknown"),
        }
    }
}

/// Log platform information at startup
pub fn log_platform_info() {
    let display_server = DisplayServer::detect();
    tracing::info!(display_server = %display_server, "Display server detected");

    // Log Slint backend if overridden
    if let Ok(backend) = env::var("SLINT_BACKEND") {
        tracing::info!(backend = %backend, "Slint backend override");
    }

    // Log XDG session type for additional context
    if let Ok(session_type) = env::var("XDG_SESSION_TYPE") {
        tracing::debug!(session_type = %session_type, "XDG session type");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_server_display() {
        assert_eq!(format!("{}", DisplayServer::Wayland), "Wayland");
        assert_eq!(format!("{}", DisplayServer::X11), "X11");
        assert_eq!(format!("{}", DisplayServer::Unknown), "Unknown");
    }
}
