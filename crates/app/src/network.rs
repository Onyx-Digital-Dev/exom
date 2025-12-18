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
    /// Reconnecting after disconnect (with backoff)
    Reconnecting,
}

/// Connection info for persistence and reconnect
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub hall_id: Uuid,
    pub invite_url: String,
    pub host_addr: Option<String>,
    pub epoch: u64,
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
    /// Successfully connected - persist this info
    Connected(ConnectionInfo),
    /// Election in progress (status update)
    ElectionInProgress,
    /// This node became the new host
    BecameHost { port: u16 },
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
    username: Option<String>,
    /// Epoch for detecting stale reconnect attempts
    epoch: u64,
    /// Are we in a reconnect loop?
    reconnecting: bool,
    /// Cancel signal for reconnect loop
    cancel_reconnect: bool,
    /// Token for hosting (stored from invite URL)
    token: Option<String>,
    /// Last known member list (for election)
    members: Vec<PeerInfo>,
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
    /// Start auto-reconnect with backoff
    StartReconnect {
        invite_url: String,
        user_id: Uuid,
        username: String,
        role: NetRole,
    },
    /// Cancel ongoing reconnect
    CancelReconnect,
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
            username: None,
            epoch: 0,
            reconnecting: false,
            cancel_reconnect: false,
            token: None,
            members: Vec::new(),
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

    /// Start auto-reconnect with exponential backoff
    pub async fn start_reconnect(
        &self,
        invite_url: String,
        user_id: Uuid,
        username: String,
        role: NetRole,
    ) -> Result<(), &'static str> {
        self.cmd_tx
            .send(NetworkCommand::StartReconnect {
                invite_url,
                user_id,
                username,
                role,
            })
            .await
            .map_err(|_| "Network task not running")
    }

    /// Cancel any ongoing reconnect attempt
    pub async fn cancel_reconnect(&self) {
        let _ = self.cmd_tx.send(NetworkCommand::CancelReconnect).await;
    }

    /// Get current network state
    pub async fn state(&self) -> NetworkState {
        self.state.read().await.network_state
    }

    /// Get the invite URL (when hosting)
    pub async fn invite_url(&self) -> Option<String> {
        self.state.read().await.invite_url.clone()
    }

    /// Get connection info if connected
    pub async fn connection_info(&self) -> Option<ConnectionInfo> {
        let s = self.state.read().await;
        if s.network_state == NetworkState::Connected || s.network_state == NetworkState::Hosting {
            Some(ConnectionInfo {
                hall_id: s.hall_id?,
                invite_url: s.invite_url.clone()?,
                host_addr: None,
                epoch: s.epoch,
            })
        } else {
            None
        }
    }
}

impl Default for NetworkManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Backoff delays for reconnect: 1s, 2s, 5s, 10s, 30s (capped)
const RECONNECT_DELAYS_MS: &[u64] = &[1000, 2000, 5000, 10000, 30000];

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
                        // Cancel any reconnect
                        {
                            let mut s = state.write().await;
                            s.cancel_reconnect = true;
                            s.reconnecting = false;
                        }
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
                        // Cancel any reconnect
                        {
                            let mut s = state.write().await;
                            s.cancel_reconnect = true;
                            s.reconnecting = false;
                        }
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
                    Some(NetworkCommand::StartReconnect {
                        invite_url,
                        user_id,
                        username,
                        role,
                    }) => {
                        // Start reconnect loop
                        client = handle_reconnect_loop(
                            &state,
                            &event_tx,
                            invite_url,
                            user_id,
                            username,
                            role,
                        )
                        .await;
                    }
                    Some(NetworkCommand::CancelReconnect) => {
                        let mut s = state.write().await;
                        s.cancel_reconnect = true;
                        s.reconnecting = false;
                        info!("Reconnect cancelled");
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
                        // Cancel any reconnect
                        {
                            let mut s = state.write().await;
                            s.cancel_reconnect = true;
                            s.reconnecting = false;
                        }
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

/// Handle reconnect with exponential backoff
async fn handle_reconnect_loop(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    invite_url: String,
    user_id: Uuid,
    username: String,
    role: NetRole,
) -> Option<Client> {
    // Set reconnecting state
    {
        let mut s = state.write().await;
        s.reconnecting = true;
        s.cancel_reconnect = false;
        s.network_state = NetworkState::Reconnecting;
    }
    let _ = event_tx
        .send(NetworkEvent::StateChanged(NetworkState::Reconnecting))
        .await;

    let mut attempt = 0;
    loop {
        // Check if cancelled
        {
            let s = state.read().await;
            if s.cancel_reconnect {
                info!("Reconnect loop cancelled");
                return None;
            }
        }

        info!(attempt = attempt + 1, "Reconnect attempt");

        // Try to connect
        if let Some(client) =
            try_connect(state, event_tx, &invite_url, user_id, &username, role).await
        {
            // Success!
            {
                let mut s = state.write().await;
                s.reconnecting = false;
                s.epoch += 1;
            }
            info!("Reconnect successful");
            return Some(client);
        }

        // Check if cancelled after attempt
        {
            let s = state.read().await;
            if s.cancel_reconnect {
                info!("Reconnect loop cancelled after attempt");
                return None;
            }
        }

        // Calculate backoff delay
        let delay_idx = attempt.min(RECONNECT_DELAYS_MS.len() - 1);
        let delay_ms = RECONNECT_DELAYS_MS[delay_idx];
        info!(delay_ms = delay_ms, "Reconnect backoff");

        // Wait with potential cancel check
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

        attempt += 1;
    }
}

/// Try a single connection attempt (no state changes on failure beyond logging)
async fn try_connect(
    state: &Arc<RwLock<NetworkManagerState>>,
    event_tx: &mpsc::Sender<NetworkEvent>,
    invite_url: &str,
    user_id: Uuid,
    username: &str,
    role: NetRole,
) -> Option<Client> {
    // Parse invite URL
    let invite = match InviteUrl::parse(invite_url) {
        Ok(inv) => inv,
        Err(e) => {
            warn!(error = %e, "Invalid invite URL during reconnect");
            return None;
        }
    };

    // Connect
    match Client::connect(
        invite.socket_addr(),
        user_id,
        username.to_string(),
        invite.hall_id,
        invite.token.clone(),
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
                s.username = Some(username.to_string());
                s.invite_url = Some(invite_url.to_string());
                s.token = Some(invite.token.clone());
            }
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Connected))
                .await;

            // Emit connection info for persistence
            let _ = event_tx
                .send(NetworkEvent::Connected(ConnectionInfo {
                    hall_id: invite.hall_id,
                    invite_url: invite_url.to_string(),
                    host_addr: Some(invite.socket_addr().to_string()),
                    epoch: state.read().await.epoch,
                }))
                .await;

            Some(client)
        }
        Err(e) => {
            debug!(error = %e, "Connection attempt failed");
            None
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
        username.clone(),
        invite.hall_id,
        invite.token.clone(),
        role,
    )
    .await
    {
        Ok(client) => {
            info!("Connected to server");
            let epoch = {
                let mut s = state.write().await;
                s.network_state = NetworkState::Connected;
                s.hall_id = Some(invite.hall_id);
                s.user_id = Some(user_id);
                s.user_role = Some(role);
                s.username = Some(username);
                s.invite_url = Some(invite_url.clone());
                s.token = Some(invite.token.clone());
                s.epoch += 1;
                s.epoch
            };
            let _ = event_tx
                .send(NetworkEvent::StateChanged(NetworkState::Connected))
                .await;

            // Emit connection info for persistence
            let _ = event_tx
                .send(NetworkEvent::Connected(ConnectionInfo {
                    hall_id: invite.hall_id,
                    invite_url,
                    host_addr: Some(invite.socket_addr().to_string()),
                    epoch,
                }))
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
        ServerEvent::Joined {
            host_id,
            members,
            epoch,
        } => {
            debug!(host_id = %host_id, members = members.len(), epoch = epoch, "Joined hall");
            {
                let mut s = state.write().await;
                s.host_id = Some(host_id);
                s.epoch = epoch;
                s.members = members.clone();
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
            {
                let mut s = state.write().await;
                s.members = members.clone();
            }
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
        ServerEvent::HostDead {
            hall_id,
            last_epoch,
            members,
        } => {
            info!(hall_id = %hall_id, last_epoch = last_epoch, "Host dead - starting election");
            let _ = event_tx.send(NetworkEvent::ElectionInProgress).await;

            // Perform deterministic election
            let (user_id, user_role, username, token) = {
                let s = state.read().await;
                (s.user_id, s.user_role, s.username.clone(), s.token.clone())
            };

            if let (Some(user_id), Some(user_role), Some(username)) = (user_id, user_role, username)
            {
                // Determine election winner
                let winner = elect_new_host(&members, user_id, user_role);

                if winner == user_id {
                    info!("This node won election - becoming host");

                    // Try to start server
                    let token = token.unwrap_or_else(|| "failover".to_string());
                    match try_start_server(hall_id, user_id, username, user_role, token, last_epoch)
                        .await
                    {
                        Some((server, port)) => {
                            {
                                let mut s = state.write().await;
                                s.server = Some(server);
                                s.network_state = NetworkState::Hosting;
                                s.host_id = Some(user_id);
                                s.epoch = last_epoch + 1;
                            }
                            let _ = event_tx.send(NetworkEvent::BecameHost { port }).await;
                            let _ = event_tx
                                .send(NetworkEvent::StateChanged(NetworkState::Hosting))
                                .await;
                        }
                        None => {
                            error!("Failed to start server after winning election");
                            let _ = event_tx.send(NetworkEvent::Disconnected).await;
                        }
                    }
                } else {
                    info!(winner = %winner, "Another node won election - waiting for reconnect info");
                    // Will receive HostElected event with new host address
                }
            }
        }
        ServerEvent::HostElected {
            hall_id,
            epoch,
            host_user_id,
            host_addr,
            host_port,
        } => {
            info!(
                hall_id = %hall_id,
                epoch = epoch,
                host = %host_user_id,
                addr = %host_addr,
                port = host_port,
                "New host elected - reconnecting"
            );

            // Update epoch
            {
                let mut s = state.write().await;
                if epoch > s.epoch {
                    s.epoch = epoch;
                }
            }

            // TODO: Auto-reconnect to new host
            // For now, just update the state and let the user reconnect
            let _ = event_tx.send(NetworkEvent::Disconnected).await;
        }
    }
}

/// Elect new host deterministically from members list
/// Returns the user_id of the winner
fn elect_new_host(members: &[PeerInfo], my_user_id: Uuid, my_role: NetRole) -> Uuid {
    // Build candidate list (excluding current host who is dead)
    let mut candidates: Vec<(Uuid, NetRole)> = members
        .iter()
        .filter(|m| !m.is_host) // Exclude dead host
        .map(|m| (m.user_id, m.role))
        .collect();

    // Add self if not in members
    if !candidates.iter().any(|(id, _)| *id == my_user_id) {
        candidates.push((my_user_id, my_role));
    }

    // Sort by: 1) role descending (higher role wins), 2) user_id ascending (tie-breaker)
    candidates.sort_by(|a, b| {
        // First compare by role (descending - higher role wins)
        match b.1.cmp(&a.1) {
            std::cmp::Ordering::Equal => {
                // Tie-breaker: ascending by user_id
                a.0.to_string().cmp(&b.0.to_string())
            }
            other => other,
        }
    });

    // Filter to only candidates who can host (Agent or higher)
    let hostable: Vec<_> = candidates
        .iter()
        .filter(|(_, role)| role.can_host())
        .collect();

    if let Some((winner_id, _)) = hostable.first() {
        *winner_id
    } else {
        // No one can host - return first candidate anyway
        candidates.first().map(|(id, _)| *id).unwrap_or(my_user_id)
    }
}

/// Try to start a server on port 7331, incrementing if busy (up to +20)
async fn try_start_server(
    hall_id: Uuid,
    host_id: Uuid,
    username: String,
    role: NetRole,
    token: String,
    _last_epoch: u64,
) -> Option<(Server, u16)> {
    let base_port = 7331;
    let max_attempts = 20;

    for offset in 0..max_attempts {
        let port = base_port + offset;
        match Server::start(
            port,
            hall_id,
            host_id,
            username.clone(),
            role,
            token.clone(),
        )
        .await
        {
            Ok(server) => {
                info!(port = port, "Server started after election");
                return Some((server, port));
            }
            Err(e) => {
                debug!(port = port, error = %e, "Port busy, trying next");
            }
        }
    }

    None
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
