//! TCP server for hosting a Hall
//!
//! The host runs this server. Clients connect and exchange messages.
//! All messages are broadcast to all connected peers.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{broadcast, mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frame::{read_frame, write_frame};
use crate::protocol::{CurrentTool, Message, NetRole, PeerInfo, PresenceStatus};

use chrono::Utc;

use crate::protocol::NetMessage;

/// Maximum number of connected peers
const MAX_PEERS: usize = 32;

/// Heartbeat interval in milliseconds
const HEARTBEAT_INTERVAL_MS: u64 = 2000;

/// Maximum messages to keep in history for sync
const MAX_MESSAGE_HISTORY: usize = 500;

/// Connected peer state
struct Peer {
    user_id: Uuid,
    username: String,
    role: NetRole,
    tx: mpsc::Sender<Message>,
    presence: PresenceStatus,
    current_tool: CurrentTool,
}

/// Server state shared across tasks
struct ServerState {
    hall_id: Uuid,
    host_id: Uuid,
    token: String,
    peers: HashMap<Uuid, Peer>,
    epoch: u64,
    /// Next sequence number for messages
    next_sequence: u64,
    /// Recent message history for sync (circular buffer)
    message_history: Vec<NetMessage>,
}

impl ServerState {
    fn member_list(&self) -> Vec<PeerInfo> {
        self.peers
            .values()
            .map(|p| PeerInfo {
                user_id: p.user_id,
                username: p.username.clone(),
                role: p.role,
                is_host: p.user_id == self.host_id,
                presence: p.presence,
                current_tool: p.current_tool,
            })
            .collect()
    }
}

/// Hall server handle
pub struct Server {
    addr: SocketAddr,
    state: Arc<RwLock<ServerState>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl Server {
    /// Start a new server on the given port
    pub async fn start(
        port: u16,
        hall_id: Uuid,
        host_id: Uuid,
        host_username: String,
        host_role: NetRole,
        token: String,
    ) -> Result<Self> {
        let addr = SocketAddr::from(([0, 0, 0, 0], port));
        let listener = TcpListener::bind(addr).await?;
        let bound_addr = listener.local_addr()?;

        info!(addr = %bound_addr, hall_id = %hall_id, "Server started");

        let (shutdown_tx, _) = broadcast::channel(1);

        // Initialize state with host as first peer
        let (host_tx, _host_rx) = mpsc::channel(64);
        let mut peers = HashMap::new();
        peers.insert(
            host_id,
            Peer {
                user_id: host_id,
                username: host_username,
                role: host_role,
                tx: host_tx,
                presence: PresenceStatus::Active,
                current_tool: CurrentTool::Chat,
            },
        );

        let state = Arc::new(RwLock::new(ServerState {
            hall_id,
            host_id,
            token,
            peers,
            epoch: 1,
            next_sequence: 1,
            message_history: Vec::new(),
        }));

        // Spawn accept loop
        let state_clone = state.clone();
        let shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(accept_loop(listener, state_clone, shutdown_rx));

        // Spawn heartbeat task
        let state_clone = state.clone();
        let shutdown_rx = shutdown_tx.subscribe();
        tokio::spawn(heartbeat_task(state_clone, shutdown_rx));

        Ok(Server {
            addr: bound_addr,
            state,
            shutdown_tx,
        })
    }

    /// Get current epoch
    pub async fn epoch(&self) -> u64 {
        self.state.read().await.epoch
    }

    /// Get the server's bound address
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Broadcast a message to all connected peers
    pub async fn broadcast(&self, msg: Message) {
        let state = self.state.read().await;
        for peer in state.peers.values() {
            if peer.tx.send(msg.clone()).await.is_err() {
                debug!(user_id = %peer.user_id, "Failed to queue message for peer");
            }
        }
    }

    /// Broadcast a message to all peers except one
    pub async fn broadcast_except(&self, msg: Message, except: Uuid) {
        let state = self.state.read().await;
        for peer in state.peers.values() {
            if peer.user_id != except && peer.tx.send(msg.clone()).await.is_err() {
                debug!(user_id = %peer.user_id, "Failed to queue message for peer");
            }
        }
    }

    /// Get current member list
    pub async fn members(&self) -> Vec<PeerInfo> {
        self.state.read().await.member_list()
    }

    /// Shutdown the server
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(());
        info!("Server shutdown initiated");
    }

    /// Regenerate the invite token, invalidating the old one
    /// Returns the new token
    pub async fn regenerate_token(&self) -> String {
        let new_token = generate_token();
        let mut state = self.state.write().await;
        state.token = new_token.clone();
        info!("Invite token regenerated");
        new_token
    }
}

/// Generate a random token for invites
fn generate_token() -> String {
    // Use UUID v4 which is cryptographically random
    Uuid::new_v4().to_string().replace("-", "")[..16].to_string()
}

/// Accept incoming connections
async fn accept_loop(
    listener: TcpListener,
    state: Arc<RwLock<ServerState>>,
    mut shutdown_rx: broadcast::Receiver<()>,
) {
    loop {
        tokio::select! {
            result = listener.accept() => {
                match result {
                    Ok((stream, addr)) => {
                        debug!(addr = %addr, "New connection");
                        let state = state.clone();
                        tokio::spawn(handle_connection(stream, addr, state));
                    }
                    Err(e) => {
                        error!(error = %e, "Accept failed");
                    }
                }
            }
            _ = shutdown_rx.recv() => {
                info!("Accept loop shutting down");
                break;
            }
        }
    }
}

/// Handle a single client connection
async fn handle_connection(stream: TcpStream, addr: SocketAddr, state: Arc<RwLock<ServerState>>) {
    let (reader, writer) = tokio::io::split(stream);

    // First message must be JoinRequest
    let mut reader = reader;
    let join_result = handle_join(&mut reader, &state).await;

    let (user_id, _tx) = match join_result {
        Ok((id, tx)) => (id, tx),
        Err(e) => {
            warn!(addr = %addr, error = %e, "Join failed");
            return;
        }
    };

    info!(addr = %addr, user_id = %user_id, "Peer joined");

    // Spawn writer task
    let (msg_tx, msg_rx) = mpsc::channel(64);

    // Update peer's tx channel
    {
        let mut s = state.write().await;
        if let Some(peer) = s.peers.get_mut(&user_id) {
            peer.tx = msg_tx.clone();
        }
    }

    let writer_handle = tokio::spawn(writer_task(writer, msg_rx));

    // Send initial member list
    let members = state.read().await.member_list();
    let _ = msg_tx
        .send(Message::MemberList {
            members: members.clone(),
        })
        .await;

    // Broadcast updated member list to others
    broadcast_to_peers(&state, Message::MemberList { members }, Some(user_id)).await;

    // Read loop
    loop {
        match read_frame(&mut reader).await {
            Ok(msg) => {
                handle_message(msg, user_id, &state).await;
            }
            Err(Error::ConnectionClosed) => {
                debug!(user_id = %user_id, "Connection closed");
                break;
            }
            Err(e) => {
                warn!(user_id = %user_id, error = %e, "Read error");
                break;
            }
        }
    }

    // Cleanup
    writer_handle.abort();
    remove_peer(&state, user_id).await;

    info!(user_id = %user_id, "Peer disconnected");
}

/// Handle join request
async fn handle_join(
    reader: &mut ReadHalf<TcpStream>,
    state: &Arc<RwLock<ServerState>>,
) -> Result<(Uuid, mpsc::Sender<Message>)> {
    let msg = read_frame(reader).await?;

    match msg {
        Message::JoinRequest {
            user_id,
            username,
            hall_id,
            token,
            role,
        } => {
            let mut s = state.write().await;

            // Validate hall
            if hall_id != s.hall_id {
                return Err(Error::Rejected("Wrong hall".into()));
            }

            // Validate token
            if token != s.token {
                return Err(Error::Rejected("Invalid token".into()));
            }

            // Check capacity
            if s.peers.len() >= MAX_PEERS {
                return Err(Error::ServerFull);
            }

            // Check for duplicate
            if s.peers.contains_key(&user_id) {
                return Err(Error::Rejected("Already connected".into()));
            }

            // Add peer
            let (tx, _rx) = mpsc::channel(64);
            s.peers.insert(
                user_id,
                Peer {
                    user_id,
                    username,
                    role,
                    tx: tx.clone(),
                    presence: PresenceStatus::Active,
                    current_tool: CurrentTool::Chat,
                },
            );

            Ok((user_id, tx))
        }
        _ => Err(Error::Protocol("Expected JoinRequest".into())),
    }
}

/// Writer task - sends messages to the client
async fn writer_task(mut writer: WriteHalf<TcpStream>, mut rx: mpsc::Receiver<Message>) {
    while let Some(msg) = rx.recv().await {
        if let Err(e) = write_frame(&mut writer, &msg).await {
            debug!(error = %e, "Write failed");
            break;
        }
    }
}

/// Handle an incoming message
async fn handle_message(msg: Message, sender_id: Uuid, state: &Arc<RwLock<ServerState>>) {
    match msg {
        Message::Chat(mut chat_msg) => {
            let message_id = chat_msg.id;

            // Assign sequence number and store in history
            let sequence = {
                let mut s = state.write().await;
                let seq = s.next_sequence;
                s.next_sequence += 1;
                chat_msg.sequence = seq;

                // Store in history (circular buffer)
                if s.message_history.len() >= MAX_MESSAGE_HISTORY {
                    s.message_history.remove(0);
                }
                s.message_history.push(chat_msg.clone());

                seq
            };
            debug!(sequence = sequence, "Assigned sequence to message");

            // Broadcast to all peers including sender
            broadcast_to_peers(state, Message::Chat(chat_msg), None).await;

            // Send acknowledgment back to sender
            let s = state.read().await;
            if let Some(peer) = s.peers.get(&sender_id) {
                let _ = peer.tx.send(Message::MessageAck { message_id }).await;
            }
        }
        Message::Ping => {
            // Send pong back to sender only
            let s = state.read().await;
            if let Some(peer) = s.peers.get(&sender_id) {
                let _ = peer.tx.send(Message::Pong).await;
            }
        }
        Message::Typing {
            hall_id,
            user_id,
            username,
            is_typing,
        } => {
            // Broadcast typing status to all peers except sender
            broadcast_to_peers(
                state,
                Message::Typing {
                    hall_id,
                    user_id,
                    username,
                    is_typing,
                },
                Some(sender_id),
            )
            .await;
        }
        Message::SyncSince {
            hall_id,
            last_sequence,
        } => {
            // Find messages after last_sequence and send them
            let (messages, from_sequence) = {
                let s = state.read().await;
                if hall_id != s.hall_id {
                    return;
                }
                let messages: Vec<NetMessage> = s
                    .message_history
                    .iter()
                    .filter(|m| m.sequence > last_sequence)
                    .cloned()
                    .collect();
                let from_seq = messages
                    .first()
                    .map(|m| m.sequence)
                    .unwrap_or(last_sequence);
                (messages, from_seq)
            };

            // Send sync batch to requester
            let s = state.read().await;
            if let Some(peer) = s.peers.get(&sender_id) {
                let _ = peer
                    .tx
                    .send(Message::SyncBatch {
                        hall_id,
                        from_sequence,
                        messages,
                    })
                    .await;
            }
        }
        Message::PresenceUpdate {
            user_id,
            presence,
            current_tool,
        } => {
            // Update peer's presence state
            {
                let mut s = state.write().await;
                if let Some(peer) = s.peers.get_mut(&user_id) {
                    peer.presence = presence;
                    peer.current_tool = current_tool;
                }
            }

            // Broadcast to all peers except sender
            broadcast_to_peers(
                state,
                Message::PresenceUpdate {
                    user_id,
                    presence,
                    current_tool,
                },
                Some(sender_id),
            )
            .await;
        }
        _ => {
            debug!(sender_id = %sender_id, "Ignoring unexpected message type");
        }
    }
}

/// Remove a peer and broadcast updated member list
async fn remove_peer(state: &Arc<RwLock<ServerState>>, user_id: Uuid) {
    let members = {
        let mut s = state.write().await;
        s.peers.remove(&user_id);
        s.member_list()
    };

    // Broadcast peer left
    broadcast_to_peers(state, Message::PeerLeft { user_id }, None).await;
    broadcast_to_peers(state, Message::MemberList { members }, None).await;
}

/// Broadcast to all peers, optionally excluding one
async fn broadcast_to_peers(state: &Arc<RwLock<ServerState>>, msg: Message, except: Option<Uuid>) {
    let s = state.read().await;
    for peer in s.peers.values() {
        if except != Some(peer.user_id) {
            let _ = peer.tx.send(msg.clone()).await;
        }
    }
}

/// Heartbeat task - sends heartbeats to all peers every 2s
async fn heartbeat_task(state: Arc<RwLock<ServerState>>, mut shutdown_rx: broadcast::Receiver<()>) {
    let interval = std::time::Duration::from_millis(HEARTBEAT_INTERVAL_MS);

    loop {
        tokio::select! {
            _ = tokio::time::sleep(interval) => {
                let (hall_id, host_id, epoch) = {
                    let s = state.read().await;
                    (s.hall_id, s.host_id, s.epoch)
                };

                let heartbeat = Message::HostHeartbeat {
                    hall_id,
                    epoch,
                    host_user_id: host_id,
                    timestamp: Utc::now(),
                };

                broadcast_to_peers(&state, heartbeat, None).await;
            }
            _ = shutdown_rx.recv() => {
                debug!("Heartbeat task shutting down");
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_start() {
        let server = Server::start(
            0, // Random port
            Uuid::new_v4(),
            Uuid::new_v4(),
            "host".to_string(),
            NetRole::Builder,
            "test-token".to_string(),
        )
        .await
        .unwrap();

        assert!(server.addr().port() > 0);
        server.shutdown();
    }
}
