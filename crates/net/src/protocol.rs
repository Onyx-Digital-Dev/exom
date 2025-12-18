//! Network protocol message types
//!
//! All messages are JSON-serialized and length-prefixed on the wire.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Role transmitted over the network (mirrors HallRole but decoupled)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum NetRole {
    Builder = 5,
    Prefect = 4,
    Moderator = 3,
    Agent = 2,
    Fellow = 1,
}

impl NetRole {
    pub fn hosting_priority(self) -> u8 {
        self as u8
    }

    pub fn can_host(self) -> bool {
        self >= NetRole::Agent
    }

    /// Convert from a role value (matches HallRole values)
    pub fn from_value(v: u8) -> Self {
        match v {
            5 => NetRole::Builder,
            4 => NetRole::Prefect,
            3 => NetRole::Moderator,
            2 => NetRole::Agent,
            _ => NetRole::Fellow,
        }
    }

    /// Convert to a role value (matches HallRole values)
    pub fn to_value(self) -> u8 {
        self as u8
    }
}

/// Information about a connected peer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerInfo {
    pub user_id: Uuid,
    pub username: String,
    pub role: NetRole,
    pub is_host: bool,
}

/// A chat message transmitted over the network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetMessage {
    pub id: Uuid,
    pub hall_id: Uuid,
    pub sender_id: Uuid,
    pub sender_username: String,
    pub sender_role: NetRole,
    pub content: String,
    pub timestamp: DateTime<Utc>,
    /// Sequence number assigned by host for ordering
    #[serde(default)]
    pub sequence: u64,
}

/// Network protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum Message {
    /// Client requests to join a hall
    JoinRequest {
        user_id: Uuid,
        username: String,
        hall_id: Uuid,
        token: String,
        role: NetRole,
    },

    /// Server accepts join request
    JoinAccepted {
        hall_id: Uuid,
        host_id: Uuid,
        members: Vec<PeerInfo>,
        epoch: u64,
    },

    /// Server rejects join request
    JoinRejected { reason: String },

    /// A peer has left the hall
    PeerLeft { user_id: Uuid },

    /// Chat message from a peer
    Chat(NetMessage),

    /// Updated member list (broadcast on join/leave)
    MemberList { members: Vec<PeerInfo> },

    /// Host has changed
    HostChanged { new_host_id: Uuid },

    /// Ping to keep connection alive
    Ping,

    /// Pong response to ping
    Pong,

    /// Server is shutting down
    ServerShutdown,

    /// Host heartbeat (sent every 2s)
    HostHeartbeat {
        hall_id: Uuid,
        epoch: u64,
        host_user_id: Uuid,
        timestamp: DateTime<Utc>,
    },

    /// Election has started (host may be dead)
    HostElectionStarted { hall_id: Uuid, epoch: u64 },

    /// New host elected
    HostElected {
        hall_id: Uuid,
        epoch: u64,
        host_user_id: Uuid,
        host_addr: String,
        host_port: u16,
    },

    /// Request messages since a sequence number (for sync on reconnect)
    SyncSince { hall_id: Uuid, last_sequence: u64 },

    /// Batch of messages in response to SyncSince
    SyncBatch {
        hall_id: Uuid,
        from_sequence: u64,
        messages: Vec<NetMessage>,
    },
}

impl Message {
    /// Serialize message to JSON bytes
    pub fn to_bytes(&self) -> Result<Vec<u8>, serde_json::Error> {
        serde_json::to_vec(self)
    }

    /// Deserialize message from JSON bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, serde_json::Error> {
        serde_json::from_slice(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_roundtrip() {
        let msg = Message::Chat(NetMessage {
            id: Uuid::new_v4(),
            hall_id: Uuid::new_v4(),
            sender_id: Uuid::new_v4(),
            sender_username: "alice".to_string(),
            sender_role: NetRole::Agent,
            content: "Hello".to_string(),
            timestamp: Utc::now(),
            sequence: 0,
        });

        let bytes = msg.to_bytes().unwrap();
        let decoded = Message::from_bytes(&bytes).unwrap();

        match decoded {
            Message::Chat(m) => assert_eq!(m.content, "Hello"),
            _ => panic!("Wrong message type"),
        }
    }

    #[test]
    fn test_role_ordering() {
        assert!(NetRole::Builder > NetRole::Fellow);
        assert!(NetRole::Agent.can_host());
        assert!(!NetRole::Fellow.can_host());
    }
}
