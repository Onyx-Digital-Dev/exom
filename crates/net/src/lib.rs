//! Exom Network Library
//!
//! Provides TCP-based networking for Hall collaboration.
//!
//! # Architecture
//!
//! - **Server**: Run by the host, accepts connections from peers
//! - **Client**: Connects to a host's server
//! - **Protocol**: Length-prefixed JSON messages
//!
//! # Usage
//!
//! ```ignore
//! // Host starts a server
//! let server = Server::start(7331, hall_id, host_id, "host", NetRole::Builder, token).await?;
//!
//! // Client connects
//! let client = Client::connect(addr, user_id, "user", hall_id, token, NetRole::Agent).await?;
//!
//! // Process events
//! while let Some(event) = client.next_event().await {
//!     match event {
//!         ServerEvent::Chat(msg) => { /* handle */ }
//!         _ => {}
//!     }
//! }
//! ```

pub mod client;
pub mod error;
mod frame;
pub mod invite;
pub mod protocol;
pub mod server;

pub use client::{Client, ConnectionState, ServerEvent};
pub use error::{Error, Result};
pub use invite::InviteUrl;
pub use protocol::{Message, NetMessage, NetRole, PeerInfo};
pub use server::Server;

/// Default port for Exom servers
pub const DEFAULT_PORT: u16 = 7331;
