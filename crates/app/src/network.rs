//! Network management for the app
//!
//! Handles server hosting and client connections.

use std::sync::Arc;

use exom_net::{Client, InviteUrl, NetMessage, NetRole, PeerInfo, Server, ServerEvent};
use tokio::sync::{mpsc, RwLock};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// Network connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkState {
    /// Not connected to any network
    Offline,
    /// Hosting a hall server
    Hosting,
    /// Connected to a remote host
    Connected,
    /// Attempting to connect
    Connecting,
}

/// Events from the network layer to the UI
#[derive(Debug, Clone)]
pub enum NetworkEvent {
    /// Connection state changed
    StateChanged(NetworkState),
    /// Received a chat message
    ChatReceived(NetMessage),
    /// Member list updated
    MembersUpdated(Vec<PeerInfo>),
    /// Connection failed
    ConnectionFailed(String),
    /// Disconnected
    Disconnected,
    /// Host disconnected - client may need to take over
    HostDisconnected { hall_id: Uuid, was_host: bool },
    /// Host changed to a new user
    HostChanged { new_host_id: Uuid },
}

/// Network manager handle
pub struct NetworkManager {
    state: Arc<RwLock<NetworkManagerState>>,
    event_rx: mpsc::Receiver<NetworkEvent>,
    cmd_tx: mpsc::Sender<NetworkCommand>,
}

struct NetworkManagerState {
    network_state: NetworkState,
    server: Option<Server>,
    invite_url: Option<String>,
    hall_id: Option<Uuid>,
    host_id: Option<Uuid>,
    /// Current user's info for potential host takeover
    user_id: Option<Uuid>,
    user_role: Option<NetRole>,
}

enum NetworkCommand {
    StartHosting {
        hall_id: Uuid,
        host_id: Uuid,
        host_username: String,
        host_role: NetRole,
        token: String,
        port: u16,
    },
    Connect {
        invite_url: String,
        user_id: Uuid,
        username: String,
        role: NetRole,
    },
    SendChat(NetMessage),
    Disconnect,
}

impl NetworkManager {
    /// Create a new network manager
    pub fn new() -> Self {
        let (event_tx, event_rx) = mpsc::channel(64);
        let (cmd_tx, cmd_rx) = mpsc::channel(64);

        let state = Arc::new(RwLock::new(NetworkManagerState {
            network_state: NetworkState::Offline,
            server: None,
            invite_url: None,
            hall_id: None,
            host_id: None,
            user_id: None,
            user_role: None,
        }));

        // Spawn the network task
        let state_clone = state.clone();
        tokio::spawn(network_task(state_clone, event_tx, cmd_rx));

        Self {
            state,
            event_rx,
            cmd_tx,
        }
    }

    /// Get the next network event (non-blocking poll)
    pub fn try_recv_event(&mut self) -> Option<NetworkEvent> {
        self.event_rx.try_recv().ok()
    }

    /// Start hosting a hall
    pub async fn start_hosting(
        &self,
        hall_id: Uuid,
        host_id: Uuid,
        host_username: String,
        host_role: NetRole,
        token: String,
        port: u16,
    ) -> Result<(), &'static str> {
        self.cmd_tx
            .send(NetworkCommand::StartHosting {
                hall_id,
                host_id,
                host_username,
                host_role,
                token,
                port,
            })
            .await
            .map_err(|_| "Network task not running")
    }

    /// Connect to a hall via invite URL
    pub async fn connect(
        &self,
        invite_url: String,
        user_id: Uuid,
        username: String,
        role: NetRole,
    ) -> Result<(), &'static str> {
        self.cmd_tx
            .send(NetworkCommand::Connect {
                invite_url,
                user_id,
                username,
                role,
            })
            .await
            .map_err(|_| "Network task not running")
    }

    /// Send a chat message
    pub async fn send_chat(&self, msg: NetMessage) -> Result<(), &'static str> {
        self.cmd_tx
            .send(NetworkCommand::SendChat(msg))
            .await
            .map_err(|_| "Network task not running")
    }

    /// Disconnect from network
    pub async fn disconnect(&self) {
        let _ = self.cmd_tx.send(NetworkCommand::Disconnect).await;
    }

    /// Get current network state
    pub async fn state(&self) -> NetworkState {
        self.state.read().await.network_state
    }

    /// Get the invite URL (when hosting)
    pub async fn invite_url(&self) -> Option<String> {
        self.state.read().await.invite_url.clone()
    }
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Main network task
async fn network_task(
    state: Arc<RwLock<NetworkManagerState>>,
    event_tx: mpsc::Sender<NetworkEvent>,
    mut cmd_rx: mpsc::Receiver<NetworkCommand>,
) {
    let mut client: Option<Client> = None;

    loop {
        tokio::select! {
            // Handle commands
            cmd = cmd_rx.recv() => {
                match cmd {
                    Some(NetworkCommand::StartHosting {
                        hall_id,
                        host_id,
                        host_username,
                        host_role,
                        token,
                        port,
                    }) => {
                        handle_start_hosting(
                            &state,
                            &event_tx,
                            hall_id,
                            host_id,
                            host_username,
                            host_role,
                            token,
                            port,
                        )
                        .await;
                    }
                    Some(NetworkCommand::Connect {
                        invite_url,
                        user_id,
                        username,
                        role,
                    }) => {
                        client = handle_connect(
                            &state,
                            &event_tx,
                            invite_url,
                            user_id,
                            username,
                            role,
                        )
                        .await;
                    }
                    Some(NetworkCommand::SendChat(msg)) => {
                        // Send via server broadcast if hosting, or via client if connected
                        let s = state.read().await;
                        if let Some(server) = &s.server {
                            server.broadcast(exom_net::Message::Chat(msg)).await;
                        } else if let Some(c) = &client {
                            let _ = c.send_chat(msg).await;
                        }
                    }
                    Some(NetworkCommand::Disconnect) => {
                        handle_disconnect(&state, &event_tx, &mut client).await;
                    }
                    None => {
                        debug!("Network command channel closed");
                        break;
                    }
                }
            }

            // Poll client events if connected
            event = async {
                if let Some(ref mut c) = client {
                    c.next_event().await
                } else {
                    // No client, just wait forever
                    std::future::pending().await
                }
            } => {
                if let Some(server_event) = event {
                    handle_client_event(&state, &event_tx, server_event).await;
                } else {
                    // Client disconnected
                    handle_client_disconnected(&state, &event_tx).await;
                    client = None;
                }
            }
        }
    }
}

async fn handle_start_hosting(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    hall_id: Uuid,
    host_id: Uuid,
    host_username: String,
    host_role: NetRole,
    token: String,
    port: u16,
) {
    info!(hall_id = %hall_id, port = port, "Starting server");

    match Server::start(
        port,
        hall_id,
        host_id,
        host_username,
        host_role,
        token.clone(),
    )
    .await
    {
        Ok(server) => {
            let addr = server.addr();
            let invite = InviteUrl::from_addr(addr, hall_id, token);
            let invite_url = invite.to_url();

            info!(addr = %addr, invite = %invite_url, "Server started");

            {
                let mut s = state.write().await;
                s.network_state = NetworkState::Hosting;
                s.server = Some(server);
                s.invite_url = Some(invite_url);
                s.hall_id = Some(hall_id);
                s.host_id = Some(host_id);
                s.user_id = Some(host_id);
                s.user_role = Some(host_role);
            }

            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Hosting))
                .await;
        }
        Err(e) => {
            error!(error = %e, "Failed to start server");
            let _ = event_tx
                .send(NetworkEvent::ConnectionFailed(format!(
                    "Failed to start server: {}",
                    e
                )))
                .await;
        }
    }
}

async fn handle_connect(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    invite_url: String,
    user_id: Uuid,
    username: String,
    role: NetRole,
) -> Option<Client> {
    info!(invite = %invite_url, "Connecting to server");

    // Update state to connecting
    {
        let mut s = state.write().await;
        s.network_state = NetworkState::Connecting;
    }
    let _ = event_tx
        .send(NetworkEvent::StateChanged(NetworkState::Connecting))
        .await;

    // Parse invite URL
    let invite = match InviteUrl::parse(&invite_url) {
        Ok(inv) => inv,
        Err(e) => {
            warn!(error = %e, "Invalid invite URL");
            let mut s = state.write().await;
            s.network_state = NetworkState::Offline;
            let _ = event_tx
                .send(NetworkEvent::ConnectionFailed(format!(
                    "Invalid invite: {}",
                    e
                )))
                .await;
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Offline))
                .await;
            return None;
        }
    };

    // Connect
    match Client::connect(
        invite.socket_addr(),
        user_id,
        username,
        invite.hall_id,
        invite.token,
        role,
    )
    .await
    {
        Ok(client) => {
            info!("Connected to server");
            {
                let mut s = state.write().await;
                s.network_state = NetworkState::Connected;
                s.hall_id = Some(invite.hall_id);
                s.user_id = Some(user_id);
                s.user_role = Some(role);
            }
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Connected))
                .await;
            Some(client)
        }
        Err(e) => {
            error!(error = %e, "Connection failed");
            let mut s = state.write().await;
            s.network_state = NetworkState::Offline;
            let _ = event_tx
                .send(NetworkEvent::ConnectionFailed(format!(
                    "Connection failed: {}",
                    e
                )))
                .await;
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Offline))
                .await;
            None
        }
    }
}

async fn handle_disconnect(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    client: &mut Option<Client>,
) {
    info!("Disconnecting");

    // Shutdown server if hosting
    {
        let mut s = state.write().await;
        if let Some(server) = s.server.take() {
            server.shutdown();
        }
        s.network_state = NetworkState::Offline;
        s.invite_url = None;
        s.hall_id = None;
        s.host_id = None;
    }

    // Disconnect client
    if let Some(c) = client.take() {
        c.disconnect().await;
    }

    let _ = event_tx
        .send(NetworkEvent::StateChanged(NetworkState::Offline))
        .await;
}

async fn handle_client_event(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    event: ServerEvent,
) {
    match event {
        ServerEvent::Joined { host_id, members } => {
            debug!(host_id = %host_id, members = members.len(), "Joined hall");
            {
                let mut s = state.write().await;
                s.host_id = Some(host_id);
            }
            let _ = event_tx.send(NetworkEvent::MembersUpdated(members)).await;
        }
        ServerEvent::JoinRejected { reason } => {
            warn!(reason = %reason, "Join rejected");
            let mut s = state.write().await;
            s.network_state = NetworkState::Offline;
            let _ = event_tx.send(NetworkEvent::ConnectionFailed(reason)).await;
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Offline))
                .await;
        }
        ServerEvent::Chat(msg) => {
            let _ = event_tx.send(NetworkEvent::ChatReceived(msg)).await;
        }
        ServerEvent::MemberListUpdated { members } => {
            let _ = event_tx.send(NetworkEvent::MembersUpdated(members)).await;
        }
        ServerEvent::PeerLeft { user_id } => {
            debug!(user_id = %user_id, "Peer left");
        }
        ServerEvent::HostChanged { new_host_id } => {
            {
                let mut s = state.write().await;
                s.host_id = Some(new_host_id);
            }
            let _ = event_tx
                .send(NetworkEvent::HostChanged { new_host_id })
                .await;
        }
        ServerEvent::ServerShutdown => {
            // Host has shut down - emit special event for potential takeover
            let (hall_id, user_id, host_id) = {
                let mut s = state.write().await;
                let hall_id = s.hall_id;
                let user_id = s.user_id;
                let host_id = s.host_id;
                s.network_state = NetworkState::Offline;
                s.host_id = None;
                (hall_id, user_id, host_id)
            };

            if let Some(hall_id) = hall_id {
                // Check if this user was the host
                let was_host = user_id == host_id;
                let _ = event_tx
                    .send(NetworkEvent::HostDisconnected { hall_id, was_host })
                    .await;
            }
            let _ = event_tx.send(NetworkEvent::Disconnected).await;
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Offline))
                .await;
        }
        ServerEvent::Disconnected => {
            let mut s = state.write().await;
            s.network_state = NetworkState::Offline;
            let _ = event_tx.send(NetworkEvent::Disconnected).await;
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Offline))
                .await;
        }
    }
}

async fn handle_client_disconnected(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
) {
    let mut s = state.write().await;
    s.network_state = NetworkState::Offline;
    let _ = event_tx.send(NetworkEvent::Disconnected).await;
    let _ = event_tx
        .send(NetworkEvent::StateChanged(NetworkState::Offline))
        .await;
}
