use tokio::net::{UnixListener, UnixStream, TcpListener, TcpStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::{mpsc, Mutex};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::path::Path;
use std::sync::Arc;
use std::collections::HashMap;
use std::net::IpAddr;
use std::env; // For reading environment variables

// mDNS related imports
use mdns_sd::{ServiceDaemon, ServiceInfo, ServiceEvent};
use rand::Rng;

// network-interface for IP detection
use network_interface::{NetworkInterface, NetworkInterfaceConfig};

// --- IPC Structures (These should ideally be in a shared crate with the GUI) ---
// For now, we redefine them or ensure they are compatible with GUI's version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPeer {
    pub id: String,
    pub username: String,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuiToDaemonCommand {
    GetPeers,
    SendMessage {
        recipient_id: String,
        content: String,
    },
    RequestHistory {
        peer_id: String,
        since_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    },
    SetUsername { username: String }, // New command
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonToGuiMessage {
    DaemonStatus {
        is_connected_to_network: bool,
        active_interface_name: Option<String>,
    },
    PeerList(Vec<IpcPeer>),
    NewMessage(Message),
    HistoryResponse {
        peer_id: String,
        messages: Vec<Message>,
    },
    Error(String),
    IdentityInfo { user_id: String },
    Success(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)] // Make sure chrono is a dependency if using it here
pub struct Message {
    pub id: String,
    pub sender: String,
    pub recipient: String,
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub is_self: bool, // This will be determined by the GUI based on sender
}
// --- End IPC Structures ---

// Represents the various identifiers for the current daemon instance once username is set
#[derive(Debug, Clone)]
struct UserIdentity {
    user_provided_name: String, // Raw name from GUI, e.g., "My Cool Name"
    m_dns_instance_name: String, // Sanitized & suffixed for mDNS, e.g., "MyCoolName_a1b2c3d4"
    full_message_id: String,    // Used in messages and for GUI display, e.g., "My Cool Name - a1b2c3d4"
}

// const DAEMON_SOCKET_PATH: &str = "/tmp/localchat_daemon.sock"; // Old hardcoded
// const DAEMON_TCP_PORT: u16 = 12345; // Old hardcoded

fn get_daemon_tcp_port() -> u16 {
    env::var("LOCALCHAT_TCP_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(12345) // Default if not set or invalid
}

fn get_daemon_socket_path() -> String {
    env::var("LOCALCHAT_SOCKET_PATH")
        .unwrap_or_else(|_| "/tmp/localchat_daemon.sock".to_string()) // Default if not set
}

const MDNS_SERVICE_TYPE: &str = "_localchat._tcp.local.";

fn get_local_ip_and_interface_name() -> Option<(IpAddr, String)> {
    if let Ok(interfaces) = NetworkInterface::show() {
        for itf in interfaces {
            for addr in itf.addr {
                if !addr.ip().is_loopback() && addr.ip().is_ipv4() {
                    tracing::info!("Found suitable local IP: {} on interface: {}", addr.ip(), itf.name);
                    return Some((addr.ip(), itf.name.clone()));
                }
            }
        }
    }
    tracing::warn!("Could not find a suitable local IPv4 address.");
    None
}

// Represents a connected GUI client
struct ConnectedClient {
    stream_writer: tokio::io::WriteHalf<UnixStream>,
    // If each client needs to receive specific messages, add a tx channel here
}

// Shared state for the daemon (e.g., peer list, active connections)
// Arc<tokio::sync::Mutex<...>> would be used for actual shared mutable state

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let daemon_tcp_port = get_daemon_tcp_port();
    let daemon_socket_path = get_daemon_socket_path();

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("localchat_daemon=info".parse().map_err(|e| Box::new(e) as Box<dyn Error>)?)
        )
        .init();

    tracing::info!("LocalChat Daemon starting on TCP port: {}, Socket: {}", daemon_tcp_port, daemon_socket_path);

    if Path::new(&daemon_socket_path).exists() {
        tracing::info!("Removing existing socket file at {}", daemon_socket_path);
        if let Err(e) = std::fs::remove_file(&daemon_socket_path) {
            tracing::error!("Failed to remove existing socket file: {}. Please check permissions or remove manually.", e);
            return Err(Box::new(e) as Box<dyn Error>);
        }
    }

    let listener = match UnixListener::bind(&daemon_socket_path) {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind to Unix socket {}: {}", daemon_socket_path, e);
            return Err(Box::new(e) as Box<dyn Error>);
        }
    };

    tracing::info!("Daemon listening on Unix socket: {}", daemon_socket_path);

    let peers_map: Arc<Mutex<HashMap<String, IpcPeer>>> = Arc::new(Mutex::new(HashMap::new()));
    let active_gui_tx: Arc<Mutex<Option<mpsc::Sender<DaemonToGuiMessage>>>> = Arc::new(Mutex::new(None));
    let user_identity: Arc<Mutex<Option<UserIdentity>>> = Arc::new(Mutex::new(None)); // New state for identity

    // --- mDNS Setup (daemon only, registration deferred) ---
    let mdns_daemon = match ServiceDaemon::new() {
        Ok(daemon) => Arc::new(daemon), // Store in Arc for sharing
        Err(e) => return Err(Box::new(e) as Box<dyn Error>),
    };
    tracing::info!("mDNS ServiceDaemon created. Registration will occur after username is set.");

    // The mDNS browsing and registration logic will be moved to a new function
    // and triggered by handle_gui_connection after username is set.
    // For now, we just prepare the daemon object.

    // --- End mDNS Setup ---

    // Clone for the IPC accept loop
    let active_gui_tx_ipc_clone = active_gui_tx.clone(); 
    let peers_map_ipc_clone = peers_map.clone();
    // let instance_name_ipc_clone = arc_instance_name.clone(); // Old instance name, will be replaced by user_identity
    let user_identity_ipc_clone = user_identity.clone();
    let mdns_daemon_ipc_clone = mdns_daemon.clone(); // Pass Arc<ServiceDaemon>

    // IPC Listener Loop (for GUI connections)
    tokio::spawn(async move {
        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    tracing::info!("Accepted new GUI connection");
                    let current_peers_map_clone = peers_map_ipc_clone.clone();
                    // let current_instance_name_clone = instance_name_ipc_clone.clone(); // Old
                    let current_user_identity_clone = user_identity_ipc_clone.clone();
                    let current_mdns_daemon_clone = mdns_daemon_ipc_clone.clone(); 

                    let (to_gui_sender, to_gui_receiver) = mpsc::channel::<DaemonToGuiMessage>(32);
                    *active_gui_tx_ipc_clone.lock().await = Some(to_gui_sender);
                    
                    // Clone Arc for the handler and for passing to mDNS initialization separately
                    let active_gui_tx_for_handler = active_gui_tx_ipc_clone.clone();
                    let active_gui_tx_for_mDNS = active_gui_tx_ipc_clone.clone();

                    tokio::spawn(async move {
                        handle_gui_connection(
                            stream, 
                            current_peers_map_clone, 
                            to_gui_receiver, 
                            active_gui_tx_for_handler, // For this GUI handler's direct use
                            current_user_identity_clone, 
                            current_mdns_daemon_clone, 
                            daemon_tcp_port, 
                            active_gui_tx_for_mDNS // To be used by mDNS setup
                        ).await;
                    });
                }
                Err(e) => {
                    tracing::error!("Failed to accept incoming GUI connection: {}", e);
                }
            }
        }
    });

    // TCP Listener for peer-to-peer messages - this should ideally also only start after identity is confirmed
    // For now, it will start, but handle_peer_tcp_connection might need checks or rely on GUI not sending messages too early
    let tcp_listener_active_gui_tx = active_gui_tx.clone();
    tokio::spawn(async move {
        let listen_addr = format!("0.0.0.0:{}", daemon_tcp_port); // Use dynamic TCP port
        tracing::info!("Starting TCP listener for peer messages on {}", listen_addr);
        match TcpListener::bind(&listen_addr).await {
            Ok(tcp_listener) => {
                loop {
                    match tcp_listener.accept().await {
                        Ok((socket, addr)) => {
                            tracing::info!("Accepted new TCP connection from peer: {}", addr);
                            let gui_tx_clone_for_tcp_handler = tcp_listener_active_gui_tx.clone();
                            tokio::spawn(async move {
                                handle_peer_tcp_connection(socket, gui_tx_clone_for_tcp_handler).await;
                            });
                        }
                        Err(e) => {
                            tracing::error!("Failed to accept incoming TCP connection from peer: {}", e);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to bind TCP listener on {}: {}", listen_addr, e);
                // This task will exit, which might be problematic. Consider robust error handling.
            }
        }
    });

    // Keep the main task alive
    std::future::pending::<()>().await;
    Ok(())
}

async fn handle_peer_tcp_connection(socket: TcpStream, gui_sender_arc: Arc<Mutex<Option<mpsc::Sender<DaemonToGuiMessage>>>>) {
    let (reader, _writer) = tokio::io::split(socket);
    let mut buf_reader = BufReader::new(reader);
    let mut line_buffer = String::new();

    loop {
        line_buffer.clear();
        match buf_reader.read_line(&mut line_buffer).await {
            Ok(0) => {
                tracing::info!("Peer TCP connection closed (EOF).");
                break;
            }
            Ok(bytes_read) => {
                let trimmed_line = line_buffer.trim();
                if trimmed_line.is_empty() {
                    continue;
                }
                tracing::trace!("[TCP_RECV] Read {} bytes. Raw data: '{}'", bytes_read, trimmed_line);
                tracing::trace!("[TCP_RECV] Attempting to deserialize: '{}'", trimmed_line);
                match serde_json::from_str::<Message>(trimmed_line) {
                    Ok(mut received_message) => {
                        tracing::info!("[TCP_RECV] Deserialized message ID: {}, From: {}, To: {}", received_message.id, received_message.sender, received_message.recipient);
                        received_message.is_self = false; 

                        let gui_message = DaemonToGuiMessage::NewMessage(received_message.clone());
                        let guard = gui_sender_arc.lock().await;
                        if let Some(tx) = guard.as_ref() {
                            tracing::trace!("[TCP_RECV] Forwarding message ID: {} to GUI channel.", received_message.id);
                            if let Err(e) = tx.send(gui_message).await {
                                tracing::warn!("[TCP_RECV] Failed to send message ID: {} to GUI channel: {}. GUI client might have disconnected.", received_message.id, e);
                            } else {
                                tracing::info!("[TCP_RECV] Successfully forwarded message ID: {} to active GUI.", received_message.id);
                            }
                        } else {
                            tracing::warn!("[TCP_RECV] No active GUI client to forward message ID: {} to.", received_message.id);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to deserialize TCP message from peer: {}. Line: '{}'", e, trimmed_line);
                    }
                }
            }
            Err(e) => {
                tracing::error!("Error reading from TCP connection with peer: {}", e);
                break;
            }
        }
    }
    tracing::info!("Peer TCP connection handler finished.");
}

async fn handle_gui_connection(
    stream: UnixStream, 
    peers_map: Arc<Mutex<HashMap<String, IpcPeer>>>,
    mut messages_from_daemon_tasks: mpsc::Receiver<DaemonToGuiMessage>,
    active_gui_tx_arc: Arc<Mutex<Option<mpsc::Sender<DaemonToGuiMessage>>>>,
    user_identity_arc: Arc<Mutex<Option<UserIdentity>>>,
    mdns_daemon: Arc<ServiceDaemon>,
    daemon_tcp_port: u16,
    active_gui_tx_for_mDNS: Arc<Mutex<Option<mpsc::Sender<DaemonToGuiMessage>>>>
) {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut buf_reader = BufReader::new(reader);
    let mut line_buffer = String::new();

    // Send initial status
    let initial_status = DaemonToGuiMessage::DaemonStatus {
        is_connected_to_network: get_local_ip_and_interface_name().is_some(), 
        active_interface_name: get_local_ip_and_interface_name().map(|(_, name)| name),
    };
    if let Ok(json_status) = serde_json::to_string(&initial_status) {
        if writer.write_all(format!("{}\n", json_status).as_bytes()).await.is_err() {
            tracing::warn!("Failed to send initial status to GUI, closing connection.");
            *active_gui_tx_arc.lock().await = None;
            return;
        }
    }

    // Don't send IdentityInfo immediately. Wait for SetUsername command.
    // tracing::info!("Sent IdentityInfo (user_id: {}) to GUI", user_identity_arc.lock().await.as_ref().map(|u| u.full_message_id.clone()).unwrap_or_default());

    loop {
        tokio::select! {
            read_result = buf_reader.read_line(&mut line_buffer) => {
                match read_result {
                    Ok(0) => {
                        tracing::info!("GUI connection closed (EOF)");
                        break;
                    }
                    Ok(_) => {
                        let trimmed_line = line_buffer.trim();
                        if trimmed_line.is_empty() {
                            line_buffer.clear();
                            continue;
                        }
                        tracing::debug!("Received from GUI: {}", trimmed_line);
                        match serde_json::from_str::<GuiToDaemonCommand>(trimmed_line) {
                            Ok(command) => {
                                match command {
                                    GuiToDaemonCommand::SetUsername { username } => {
                                        tracing::info!("Processing SetUsername from GUI: {}", username);
                                        let mut identity_guard = user_identity_arc.lock().await;
                                        if identity_guard.is_some() {
                                            tracing::warn!("Username already set. Ignoring SetUsername command.");
                                            let err_response = DaemonToGuiMessage::Error("Username already set.".to_string());
                                            if let Ok(json_err) = serde_json::to_string(&err_response) {
                                                if writer.write_all(format!("{}\n", json_err).as_bytes()).await.is_err() { break; }
                                            }
                                        } else {
                                            let mut rng = rand::thread_rng();
                                            let suffix: String = rng.sample_iter(&rand::distributions::Alphanumeric)
                                                .take(8)
                                                .map(char::from)
                                                .collect();
                                            
                                            let sanitized_username_for_mdns = username
                                                .chars()
                                                .filter(|c| c.is_alphanumeric())
                                                .collect::<String>();
                                            let m_dns_instance_name = format!("{}_{}", 
                                                if sanitized_username_for_mdns.is_empty() { "LocalChat" } else { &sanitized_username_for_mdns }, 
                                                suffix
                                            );
                                            let full_message_id = format!("{} - {}", username, suffix);

                                            *identity_guard = Some(UserIdentity {
                                                user_provided_name: username.clone(),
                                                m_dns_instance_name: m_dns_instance_name.clone(),
                                                full_message_id: full_message_id.clone(),
                                            });
                                            drop(identity_guard); // Release lock before async operations

                                            tracing::info!("User identity set: Full ID = '{}', mDNS Name = '{}'", full_message_id, m_dns_instance_name);

                                            // Spawn mDNS initialization
                                            let mdns_daemon_clone = mdns_daemon.clone();
                                            let user_identity_clone_for_mdns = user_identity_arc.clone();
                                            let peers_map_clone_for_mdns = peers_map.clone();
                                            tokio::spawn(async move {
                                                tracing::info!("Spawning mDNS initialization task...");
                                                if let Err(e) = initialize_mdns_and_register(
                                                    mdns_daemon_clone,
                                                    user_identity_clone_for_mdns,
                                                    peers_map_clone_for_mdns,
                                                    daemon_tcp_port,
                                                    // active_gui_tx_for_mDNS.clone() // Pass if mDNS needs to send to GUI
                                                ).await {
                                                    tracing::error!("mDNS initialization failed: {}", e);
                                                    // Notify GUI of mDNS failure?
                                                    let err_msg = DaemonToGuiMessage::Error(format!("mDNS failed to start: {}", e));
                                                    if let Ok(json_err) = serde_json::to_string(&err_msg) {
                                                        // Need writer here, or send through active_gui_tx_arc
                                                        // This task doesn't have direct access to writer. 
                                                        // Best to send through active_gui_tx_arc if GUI needs this error.
                                                    }
                                                }
                                            });

                                            // Send IdentityInfo back to GUI
                                            let identity_msg = DaemonToGuiMessage::IdentityInfo { user_id: full_message_id };
                                            if let Ok(json_msg) = serde_json::to_string(&identity_msg) {
                                                tracing::info!("Sending IdentityInfo to GUI: {}", json_msg);
                                                if writer.write_all(format!("{}\n", json_msg).as_bytes()).await.is_err() { 
                                                    tracing::warn!("Failed to send IdentityInfo to GUI");
                                                    break; // Break select loop, will close connection
                                                }
                                            } else {
                                                tracing::error!("Failed to serialize IdentityInfo for GUI");
                                            }
                                        }
                                    }
                                    _ => { // Other commands (GetPeers, SendMessage, RequestHistory)
                                        // These are processed by process_gui_command
                                        let response = process_gui_command(command, peers_map.clone(), user_identity_arc.clone()).await;
                                        if let Ok(json_response) = serde_json::to_string(&response) {
                                            tracing::debug!("Sending to GUI (command response): {}", json_response);
                                            if writer.write_all(format!("{}\n", json_response).as_bytes()).await.is_err() {
                                                tracing::warn!("Failed to send command response to GUI: {}", trimmed_line);
                                                break; 
                                            }
                                        } else {
                                            tracing::error!("Failed to serialize command response for GUI");
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to deserialize command from GUI: {}. Line: '{}'", e, trimmed_line);
                                let err_response = DaemonToGuiMessage::Error(format!("Invalid command format: {}", e));
                                if let Ok(json_err) = serde_json::to_string(&err_response) {
                                     if let Err(e_write) = writer.write_all(format!("{}\n", json_err).as_bytes()).await {
                                        tracing::warn!("Failed to send error response to GUI: {}", e_write);
                                        break;
                                    }
                                }
                            }
                        }
                        line_buffer.clear();
                    }
                    Err(e) => {
                        tracing::error!("Error reading from GUI connection: {}", e);
                        break;
                    }
                }
            },
            Some(daemon_message) = messages_from_daemon_tasks.recv() => {
                // Log for messages forwarded from other tasks (like TCP listener)
                if let DaemonToGuiMessage::NewMessage(ref msg) = daemon_message {
                    tracing::info!("Forwarding NewMessage (from other peer) to GUI: {:?}", msg);
                }
                if let Ok(json_message) = serde_json::to_string(&daemon_message) {
                    tracing::info!("Sending to GUI (from daemon task): {}", json_message);
                    if let Err(e) = writer.write_all(format!("{}\n", json_message).as_bytes()).await {
                        tracing::warn!("Failed to forward message to GUI: {}", e);
                        break; 
                    }
                } else {
                    tracing::error!("Failed to serialize daemon message for GUI");
                }
            },
            else => {
                tracing::info!("Both GUI command stream and internal daemon message channel closed.");
                break;
            }
        }
    }
    tracing::info!("GUI connection handler finished.");
    *active_gui_tx_arc.lock().await = None;
    tracing::info!("Cleared active GUI sender.");
}

async fn initialize_mdns_and_register(
    mdns_daemon: Arc<ServiceDaemon>,
    user_identity_arc: Arc<Mutex<Option<UserIdentity>>>, // To read the generated m_dns_instance_name and full_message_id
    peers_map: Arc<Mutex<HashMap<String, IpcPeer>>>,
    daemon_tcp_port: u16,
    // active_gui_tx: Arc<Mutex<Option<mpsc::Sender<DaemonToGuiMessage>>>>, // For sending updates to this specific GUI.
                                                                        // Simpler: mDNS updates peers_map, GUI uses GetPeers.
) -> Result<(), Box<dyn Error>> {
    let identity_guard = user_identity_arc.lock().await;
    let current_identity = match identity_guard.as_ref() {
        Some(id) => id.clone(), // Clone the UserIdentity data
        None => {
            tracing::error!("mDNS: Cannot initialize, user identity not set.");
            return Err("User identity not set for mDNS initialization".into());
        }
    };
    // Dropping the guard quickly
    drop(identity_guard);

    let (host_ip, iface_name) = get_local_ip_and_interface_name()
        .unwrap_or_else(|| (IpAddr::V4("0.0.0.0".parse().unwrap()), "DefaultIface".to_string()));

    // Use m_dns_instance_name for the service instance field, and full_message_id for TXT record
    let m_dns_instance_name = &current_identity.m_dns_instance_name;

    // Sanitize interface name to create a valid hostname component
    let mut sanitized_hostname_component = iface_name
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>();
    
    // Remove leading/trailing hyphens and ensure it's not empty
    sanitized_hostname_component = sanitized_hostname_component.trim_matches('-').to_string();
    if sanitized_hostname_component.is_empty() || sanitized_hostname_component == "-" {
        sanitized_hostname_component = "localchat-host".to_string();
    }
    let service_host_fqdn = format!("{}.local.", sanitized_hostname_component);

    let own_full_registered_name_for_check = format!("{}.{}", m_dns_instance_name, MDNS_SERVICE_TYPE);

    let mut txt_records = HashMap::new();
    txt_records.insert("username".to_string(), current_identity.user_provided_name.clone()); // The human-readable name
    txt_records.insert("full_id".to_string(), current_identity.full_message_id.clone()); // The ID used for messages
    txt_records.insert("version".to_string(), env!("CARGO_PKG_VERSION").to_string());

    tracing::info!("Registering mDNS service: Instance Name='{}', User Provided='{}', Full ID='{}', Host='{}', Port={}", 
        m_dns_instance_name, current_identity.user_provided_name.clone(), current_identity.full_message_id.clone(), service_host_fqdn.clone(), daemon_tcp_port);

    let service_info = ServiceInfo::new(
        MDNS_SERVICE_TYPE,
        m_dns_instance_name,       // Instance name (e.g., "MyFriendlyName_suffix")
        &service_host_fqdn,       // Host FQDN (e.g., "mymachine.local.") - pass as borrow
        host_ip,
        daemon_tcp_port,
        Some(txt_records)
    ).map_err(|e| {
        tracing::error!("Failed to create ServiceInfo: {}", e);
        Box::new(e) as Box<dyn Error>
    })?;

    mdns_daemon.register(service_info.clone()).map_err(|e| {
        tracing::error!("Failed to register mDNS service: {}", e);
        Box::new(e) as Box<dyn Error>
    })?;
    tracing::info!("Registered mDNS service: '{}' on type {}", m_dns_instance_name, MDNS_SERVICE_TYPE);

    let browser = mdns_daemon.browse(MDNS_SERVICE_TYPE).map_err(|e| {
        tracing::error!("Failed to start mDNS browser: {}", e);
        Box::new(e) as Box<dyn Error>
    })?;
    let peers_map_mdns_clone = peers_map.clone();
    // own_full_name_check was <Instance>.<Domain> = <instance_name>.<iface_name.replace(" ", "_")>.local.
    // Now it should be <m_dns_instance_name>.<MDNS_SERVICE_TYPE> for the check
    // This is what ServiceEvent::ServiceResolved(info).get_fullname() returns.
    let own_fullname_check = own_full_registered_name_for_check.clone();

    tokio::spawn(async move {
        tracing::info!("mDNS: Started browsing for type: {} (My service fullname for self-check: {})", MDNS_SERVICE_TYPE, own_fullname_check);
        loop {
            match browser.recv_async().await {
                Ok(event) => {
                    match event {
                        ServiceEvent::ServiceResolved(info) => {
                            let discovered_fullname = info.get_fullname(); // This is <instance_name>.<service_type>
                            // discovered_host is info.get_hostname() -> <hostname>.local.
                            // addresses are info.get_addresses()
                            // port is info.get_port()

                            if discovered_fullname == own_fullname_check {
                                tracing::info!("mDNS: Ignored own service by fullname: {}", discovered_fullname);
                                continue;
                            }
                            
                            // Use "full_id" from TXT for the peer's message ID, fallback to discovered_fullname.
                            // Use "username" from TXT for display name.
                            let peer_message_id = info.get_property_val_str("full_id")
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| discovered_fullname.to_string());
                            
                            let peer_display_name = info.get_property_val_str("username")
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| discovered_fullname.split('.').next().unwrap_or_default().to_string());


                            let mut chosen_peer_ip_str: Option<String> = None;
                            for peer_ip_addr in info.get_addresses() {
                                if peer_ip_addr.is_ipv4() && !peer_ip_addr.is_loopback() { // Ensure it's not loopback
                                    chosen_peer_ip_str = Some(peer_ip_addr.to_string());
                                    break; 
                                }
                            }

                            if let Some(peer_ip_str) = chosen_peer_ip_str {
                                let peer_port = info.get_port();
                                tracing::info!("mDNS: Service Resolved: ID='{}', DisplayName='{}', Addr='{}:{}', FullName='{}'", 
                                             &peer_message_id, &peer_display_name, &peer_ip_str, peer_port, discovered_fullname);

                                let mut peers = peers_map_mdns_clone.lock().await;
                                peers.insert(peer_message_id.clone(), IpcPeer { 
                                    id: peer_message_id, // Use the unique full_message_id from TXT record
                                    username: peer_display_name, 
                                    ip: peer_ip_str, 
                                    port: peer_port 
                                });
                                tracing::info!("mDNS: Updated peer list, size: {}", peers.len());
                            } else {
                                tracing::warn!("mDNS: Resolved service {} but no suitable IPv4 address found. Addresses: {:?}", 
                                             discovered_fullname, info.get_addresses());
                            }
                        }
                        ServiceEvent::ServiceRemoved(_iface_index, fullname) => { // Changed from (_iface, fullname)
                            tracing::info!("mDNS: Service Removed (by fullname): {}", fullname);
                            
                            let mut peers_guard = peers_map_mdns_clone.lock().await;
                            let mut key_identified_for_removal: Option<String> = None;

                            // First, find the key that needs to be removed without holding up the iteration
                            let instance_part_of_removed_fullname = fullname.split('.').next().unwrap_or_default();
                            for (id_key, ipc_peer) in peers_guard.iter() {
                                 // This is a heuristic. A better way is to store the m_dns_instance_name in IpcPeer.
                                if ipc_peer.username.starts_with(instance_part_of_removed_fullname) || 
                                   id_key.contains(instance_part_of_removed_fullname) { // Assuming id_key is full_message_id
                                    key_identified_for_removal = Some(id_key.clone());
                                    break;
                                }
                            }

                            // Now, after the iteration is complete and the immutable borrow from .iter() is released,
                            // we can attempt to remove the item.
                            if let Some(key_to_remove) = key_identified_for_removal {
                                if peers_guard.remove(&key_to_remove).is_some() {
                                    tracing::info!("mDNS: Removed peer with key '{}' based on removed service fullname '{}'. Updated list size: {}", key_to_remove, fullname, peers_guard.len());
                                } else {
                                     tracing::warn!("mDNS: Service '{}' removed, matching key '{}' found but failed to remove from map (already gone?).", fullname, key_to_remove);
                                }
                            } else {
                                 tracing::warn!("mDNS: Service '{}' removed, but could not find matching peer to remove from list.", fullname);
                            }
                        }
                        _ => { /* Other events like ServiceFound (before resolve), etc. */ }
                    }
                }
                Err(e) => {
                    tracing::error!("mDNS: Error receiving browse event: {}", e);
                    tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                }
            }
        }
        // tracing::warn!("mDNS browsing task ended."); // This loop is infinite
    });

    Ok(())
}

async fn process_gui_command(
    command: GuiToDaemonCommand, 
    peers_map: Arc<Mutex<HashMap<String, IpcPeer>>>, 
    user_identity_arc: Arc<Mutex<Option<UserIdentity>>>
) -> DaemonToGuiMessage {
    match command {
        GuiToDaemonCommand::SetUsername { .. } => {
            // This is handled directly in handle_gui_connection now
            // Should not reach here if logic is correct, but as a safeguard:
            tracing::warn!("SetUsername command unexpectedly reached process_gui_command.");
            DaemonToGuiMessage::Error("SetUsername should be handled internally by connection handler.".to_string())
        }
        GuiToDaemonCommand::GetPeers => {
            let peers = peers_map.lock().await;
            let peer_list: Vec<IpcPeer> = peers.values().cloned().collect();
            tracing::info!("Responding to GetPeers with {} peers", peer_list.len());
            DaemonToGuiMessage::PeerList(peer_list)
        }
        GuiToDaemonCommand::SendMessage { ref recipient_id, ref content } => { 
            let identity_guard = user_identity_arc.lock().await;
            let current_user_full_id = match identity_guard.as_ref() {
                Some(identity) => identity.full_message_id.clone(),
                None => {
                    tracing::warn!("SendMessage: User identity not set. Cannot send message.");
                    return DaemonToGuiMessage::Error("Cannot send message: User identity not set. Please set username first.".to_string());
                }
            };
            drop(identity_guard); // Release lock

            let peer_info: Option<IpcPeer>;
            {
                let peers_guard = peers_map.lock().await;
                peer_info = peers_guard.get(recipient_id).cloned();
            }

            if let Some(recipient_peer) = peer_info {
                tracing::info!("Attempting to send message to peer IpcPeer {{ id: {}, username: {}, ip: {}, port: {} }}, content: \"{}\"", 
                    recipient_peer.id, recipient_peer.username, recipient_peer.ip, recipient_peer.port, content);
                let target_addr = format!("{}:{}", recipient_peer.ip, recipient_peer.port);
                
                match TcpStream::connect(&target_addr).await {
                    Ok(mut stream) => {
                        let message_to_send = Message {
                            id: uuid::Uuid::new_v4().to_string(),
                            sender: current_user_full_id.clone(), // Use the full_message_id
                            recipient: recipient_id.clone(),
                            content: content.clone(),
                            timestamp: chrono::Utc::now(),
                            is_self: false, // This will be set to true by the sending GUI for its own echo
                        };
                        tracing::debug!("[TCP_SEND] Constructed message struct for peer {}: ID={}", recipient_peer.id, message_to_send.id);

                        tracing::trace!("[TCP_SEND] Attempting to serialize message ID: {}", message_to_send.id);
                        match serde_json::to_string(&message_to_send) {
                            Ok(json_payload) => {
                                tracing::trace!("[TCP_SEND] Message ID: {} serialized. Attempting to write to stream for peer {}.
Payload (first 100 chars): {:.100}", 
                                    message_to_send.id, recipient_peer.id, json_payload);
                                if let Err(e) = stream.write_all(format!("{}\n", json_payload).as_bytes()).await {
                                    tracing::error!("[TCP_SEND] Failed to write message ID: {} to TCP stream for peer {}: {}", message_to_send.id, recipient_peer.id, e);
                                    return DaemonToGuiMessage::Error(format!("Failed to send message to {}: {}", recipient_peer.username, e));
                                }
                                tracing::trace!("[TCP_SEND] Message ID: {} written to stream. Attempting to flush for peer {}.", message_to_send.id, recipient_peer.id);
                                if let Err(e) = stream.flush().await {
                                    tracing::error!("[TCP_SEND] Failed to flush TCP stream for peer {}: {}", recipient_peer.id, e);
                                    return DaemonToGuiMessage::Error(format!("Network error sending to {}: {}", recipient_peer.username, e));
                                }
                                tracing::info!("[TCP_SEND] Successfully sent message ID: {} to peer {} ({})", message_to_send.id, recipient_peer.username, target_addr);
                                
                                return DaemonToGuiMessage::Success(format!("Message successfully sent to {}", recipient_peer.username));
                            }
                            Err(e) => {
                                tracing::error!("[TCP_SEND] Failed to serialize message ID: {} for TCP sending: {}", message_to_send.id, e);
                                return DaemonToGuiMessage::Error(format!("Internal error: Failed to prepare message: {}", e));
                            }
                        }
                    }
                    Err(e) => {
                        tracing::error!("[TCP_SEND] Failed to connect to peer {} (ID: {}) at {}: {}", recipient_peer.username, recipient_peer.id, target_addr, e);
                        DaemonToGuiMessage::Error(format!("Could not connect to {}: {}", recipient_peer.username, e))
                    }
                }
            }
            else {
                tracing::warn!("SendMessage: Recipient peer with ID '{}' not found.", recipient_id);
                DaemonToGuiMessage::Error(format!("Recipient '{}' not found.", recipient_id))
            }
        }
        GuiToDaemonCommand::RequestHistory { .. } => {
            tracing::info!("RequestHistory command received (not implemented yet)");
            DaemonToGuiMessage::Error("History feature not yet implemented".to_string())
        }
    }
} 