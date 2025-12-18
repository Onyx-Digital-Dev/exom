//! TCP client for connecting to a Hall server

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use tokio::io::{ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::error::{Error, Result};
use crate::frame::{read_frame, write_frame};
use crate::protocol::{Message, NetRole, PeerInfo};

/// Host is considered dead if no heartbeat for this many milliseconds
const HOST_DEAD_TIMEOUT_MS: u64 = 6000;

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
}

/// Event received from the server
#[derive(Debug, Clone)]
pub enum ServerEvent {
    /// Successfully joined the hall
    Joined {
        host_id: Uuid,
        members: Vec<PeerInfo>,
        epoch: u64,
    },
    /// Join was rejected
    JoinRejected { reason: String },
    /// A chat message was received
    Chat(crate::protocol::NetMessage),
    /// Member list updated
    MemberListUpdated { members: Vec<PeerInfo> },
    /// A peer left
    PeerLeft { user_id: Uuid },
    /// Host changed
    HostChanged { new_host_id: Uuid },
    /// Connection lost
    Disconnected,
    /// Server is shutting down
    ServerShutdown,
    /// Host is dead (no heartbeat for 6s)
    HostDead {
        hall_id: Uuid,
        last_epoch: u64,
        members: Vec<PeerInfo>,
    },
    /// New host elected - reconnect to this address
    HostElected {
        hall_id: Uuid,
        epoch: u64,
        host_user_id: Uuid,
        host_addr: String,
        host_port: u16,
    },
    /// Batch of messages received after sync request
    SyncBatch {
        hall_id: Uuid,
        from_sequence: u64,
        messages: Vec<crate::protocol::NetMessage>,
    },
    /// Message was acknowledged by host
    MessageAcked { message_id: Uuid },
}

/// Client handle for network operations
pub struct Client {
    state: Arc<RwLock<ClientState>>,
    event_rx: mpsc::Receiver<ServerEvent>,
    cmd_tx: mpsc::Sender<ClientCommand>,
}

struct ClientState {
    connection: ConnectionState,
    hall_id: Option<Uuid>,
    host_id: Option<Uuid>,
    members: Vec<PeerInfo>,
    epoch: u64,
    last_heartbeat: Instant,
}

enum ClientCommand {
    Send(Message),
    Disconnect,
}

impl Client {
    /// Connect to a Hall server
    pub async fn connect(
        addr: SocketAddr,
        user_id: Uuid,
        username: String,
        hall_id: Uuid,
        token: String,
        role: NetRole,
    ) -> Result<Self> {
        info!(addr = %addr, hall_id = %hall_id, "Connecting to server");

        let stream = TcpStream::connect(addr).await?;
        let (reader, mut writer) = tokio::io::split(stream);

        // Send join request
        let join_msg = Message::JoinRequest {
            user_id,
            username,
            hall_id,
            token,
            role,
        };
        write_frame(&mut writer, &join_msg).await?;

        let state = Arc::new(RwLock::new(ClientState {
            connection: ConnectionState::Connecting,
            hall_id: Some(hall_id),
            host_id: None,
            members: Vec::new(),
            epoch: 0,
            last_heartbeat: Instant::now(),
        }));

        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(64);

        // Spawn connection handler
        let state_clone = state.clone();
        tokio::spawn(connection_task(
            reader,
            writer,
            state_clone,
            event_tx,
            cmd_rx,
        ));

        Ok(Client {
            state,
            event_rx,
            cmd_tx,
        })
    }

    /// Get the next server event
    pub async fn next_event(&mut self) -> Option<ServerEvent> {
        self.event_rx.recv().await
    }

    /// Send a chat message
    pub async fn send_chat(&self, msg: crate::protocol::NetMessage) -> Result<()> {
        self.cmd_tx
            .send(ClientCommand::Send(Message::Chat(msg)))
            .await
            .map_err(|_| Error::NotConnected)
    }

    /// Send a ping
    pub async fn ping(&self) -> Result<()> {
        self.cmd_tx
            .send(ClientCommand::Send(Message::Ping))
            .await
            .map_err(|_| Error::NotConnected)
    }

    /// Disconnect from the server
    pub async fn disconnect(&self) {
        let _ = self.cmd_tx.send(ClientCommand::Disconnect).await;
    }

    /// Get current connection state
    pub async fn connection_state(&self) -> ConnectionState {
        self.state.read().await.connection
    }

    /// Get current member list
    pub async fn members(&self) -> Vec<PeerInfo> {
        self.state.read().await.members.clone()
    }

    /// Get current host ID
    pub async fn host_id(&self) -> Option<Uuid> {
        self.state.read().await.host_id
    }
}

/// Main connection task
async fn connection_task(
    mut reader: ReadHalf<TcpStream>,
    mut writer: WriteHalf<TcpStream>,
    state: Arc<RwLock<ClientState>>,
    event_tx: mpsc::Sender<ServerEvent>,
    mut cmd_rx: mpsc::Receiver<ClientCommand>,
) {
    // Wait for join response
    match read_frame(&mut reader).await {
        Ok(Message::JoinAccepted {
            hall_id: _,
            host_id,
            members,
            epoch,
        }) => {
            {
                let mut s = state.write().await;
                s.connection = ConnectionState::Connected;
                s.host_id = Some(host_id);
                s.members = members.clone();
                s.epoch = epoch;
                s.last_heartbeat = Instant::now();
            }
            let _ = event_tx
                .send(ServerEvent::Joined {
                    host_id,
                    members,
                    epoch,
                })
                .await;
            info!("Successfully joined hall");
        }
        Ok(Message::JoinRejected { reason }) => {
            {
                let mut s = state.write().await;
                s.connection = ConnectionState::Disconnected;
            }
            let _ = event_tx
                .send(ServerEvent::JoinRejected {
                    reason: reason.clone(),
                })
                .await;
            warn!(reason = %reason, "Join rejected");
            return;
        }
        Ok(msg) => {
            // First message should be accept/reject, but might be member list
            // Handle MemberList as implicit accept for simpler flow
            if let Message::MemberList { members } = msg {
                {
                    let mut s = state.write().await;
                    s.connection = ConnectionState::Connected;
                    s.members = members.clone();
                    s.last_heartbeat = Instant::now();
                }
                // Find host from members
                let host_id = members
                    .iter()
                    .find(|m| m.is_host)
                    .map(|m| m.user_id)
                    .unwrap_or(Uuid::nil());
                let _ = event_tx
                    .send(ServerEvent::Joined {
                        host_id,
                        members,
                        epoch: 1,
                    })
                    .await;
                info!("Successfully joined hall (via member list)");
            } else {
                warn!("Unexpected first message");
                let mut s = state.write().await;
                s.connection = ConnectionState::Disconnected;
                return;
            }
        }
        Err(e) => {
            error!(error = %e, "Failed to read join response");
            let mut s = state.write().await;
            s.connection = ConnectionState::Disconnected;
            return;
        }
    }

    // Main loop - handle incoming messages and outgoing commands
    let heartbeat_check_interval = std::time::Duration::from_millis(1000);
    let mut host_dead_emitted = false;

    loop {
        tokio::select! {
            // Incoming message from server
            result = read_frame(&mut reader) => {
                match result {
                    Ok(msg) => {
                        handle_server_message(msg, &state, &event_tx).await;
                    }
                    Err(Error::ConnectionClosed) => {
                        debug!("Server closed connection");
                        break;
                    }
                    Err(e) => {
                        warn!(error = %e, "Read error");
                        break;
                    }
                }
            }

            // Outgoing command
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(ClientCommand::Send(msg)) => {
                        if let Err(e) = write_frame(&mut writer, &msg).await {
                            warn!(error = %e, "Write error");
                            break;
                        }
                    }
                    Some(ClientCommand::Disconnect) | None => {
                        debug!("Disconnect requested");
                        break;
                    }
                }
            }

            // Heartbeat watchdog
            _ = tokio::time::sleep(heartbeat_check_interval) => {
                let (hall_id, epoch, members, elapsed) = {
                    let s = state.read().await;
                    let elapsed = s.last_heartbeat.elapsed().as_millis() as u64;
                    (s.hall_id, s.epoch, s.members.clone(), elapsed)
                };

                if elapsed > HOST_DEAD_TIMEOUT_MS && !host_dead_emitted {
                    warn!(elapsed_ms = elapsed, "Host appears dead - no heartbeat");
                    host_dead_emitted = true;

                    if let Some(hall_id) = hall_id {
                        let _ = event_tx
                            .send(ServerEvent::HostDead {
                                hall_id,
                                last_epoch: epoch,
                                members,
                            })
                            .await;
                    }
                    // Don't break - let the connection handle itself
                    // The app layer will handle election
                }
            }
        }
    }

    // Cleanup
    {
        let mut s = state.write().await;
        s.connection = ConnectionState::Disconnected;
    }
    let _ = event_tx.send(ServerEvent::Disconnected).await;
    info!("Disconnected from server");
}

/// Handle a message from the server
async fn handle_server_message(
    msg: Message,
    state: &Arc<RwLock<ClientState>>,
    event_tx: &mpsc::Sender<ServerEvent>,
) {
    match msg {
        Message::Chat(chat_msg) => {
            let _ = event_tx.send(ServerEvent::Chat(chat_msg)).await;
        }
        Message::MemberList { members } => {
            {
                let mut s = state.write().await;
                s.members = members.clone();
                // Update host from member list
                if let Some(host) = members.iter().find(|m| m.is_host) {
                    s.host_id = Some(host.user_id);
                }
            }
            let _ = event_tx
                .send(ServerEvent::MemberListUpdated { members })
                .await;
        }
        Message::PeerLeft { user_id } => {
            let _ = event_tx.send(ServerEvent::PeerLeft { user_id }).await;
        }
        Message::HostChanged { new_host_id } => {
            {
                let mut s = state.write().await;
                s.host_id = Some(new_host_id);
            }
            let _ = event_tx
                .send(ServerEvent::HostChanged { new_host_id })
                .await;
        }
        Message::ServerShutdown => {
            let _ = event_tx.send(ServerEvent::ServerShutdown).await;
        }
        Message::Pong => {
            debug!("Received pong");
        }
        Message::HostHeartbeat {
            hall_id: _,
            epoch,
            host_user_id: _,
            timestamp: _,
        } => {
            // Update last heartbeat time and epoch
            let mut s = state.write().await;
            if epoch >= s.epoch {
                s.epoch = epoch;
                s.last_heartbeat = Instant::now();
            }
        }
        Message::HostElected {
            hall_id,
            epoch,
            host_user_id,
            host_addr,
            host_port,
        } => {
            // Ignore stale elections
            let current_epoch = state.read().await.epoch;
            if epoch <= current_epoch {
                debug!(
                    epoch = epoch,
                    current = current_epoch,
                    "Ignoring stale HostElected"
                );
                return;
            }

            // Update epoch
            {
                let mut s = state.write().await;
                s.epoch = epoch;
            }

            let _ = event_tx
                .send(ServerEvent::HostElected {
                    hall_id,
                    epoch,
                    host_user_id,
                    host_addr,
                    host_port,
                })
                .await;
        }
        Message::SyncBatch {
            hall_id,
            from_sequence,
            messages,
        } => {
            debug!(
                hall_id = %hall_id,
                from_sequence = from_sequence,
                count = messages.len(),
                "Received sync batch"
            );
            let _ = event_tx
                .send(ServerEvent::SyncBatch {
                    hall_id,
                    from_sequence,
                    messages,
                })
                .await;
        }
        Message::MessageAck { message_id } => {
            debug!(message_id = %message_id, "Received message ack");
            let _ = event_tx
                .send(ServerEvent::MessageAcked { message_id })
                .await;
        }
        _ => {
            debug!("Ignoring unexpected message");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::server::Server;

    #[tokio::test]
    async fn test_client_connect() {
        let hall_id = Uuid::new_v4();
        let host_id = Uuid::new_v4();
        let token = "test-token".to_string();

        // Start server
        let server = Server::start(
            0,
            hall_id,
            host_id,
            "host".to_string(),
            NetRole::Builder,
            token.clone(),
        )
        .await
        .unwrap();

        let addr = server.addr();

        // Connect client
        let client_id = Uuid::new_v4();
        let mut client = Client::connect(
            addr,
            client_id,
            "client".to_string(),
            hall_id,
            token,
            NetRole::Agent,
        )
        .await
        .unwrap();

        // Wait for join event
        if let Some(ServerEvent::Joined { .. }) = client.next_event().await {
            // Success
        } else {
            panic!("Expected Joined event");
        }

        client.disconnect().await;
        server.shutdown();
    }
}
