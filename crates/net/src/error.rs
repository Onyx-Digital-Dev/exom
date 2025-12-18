//! Network error types

use std::io;

/// Network result type
pub type Result<T> = std::result::Result<T, Error>;

/// Network errors
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] io::Error),

    #[error("Connection closed")]
    ConnectionClosed,

    #[error("Protocol error: {0}")]
    Protocol(String),

    #[error("Connection rejected: {0}")]
    Rejected(String),

    #[error("Not connected")]
    NotConnected,

    #[error("Already hosting")]
    AlreadyHosting,

    #[error("Server full")]
    ServerFull,
}
