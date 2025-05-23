#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release

use eframe::egui;
use std::env; // Added for std::env::current_exe
use std::error::Error;
use std::process::Command;
use serde::{Deserialize, Serialize}; // For IPC message serialization
use std::sync::Arc; // For Arc<tokio::runtime::Runtime>
use tokio::sync::{mpsc, Mutex as TokioMutex}; // mpsc for channels, Mutex for shared writers
use tokio::net::UnixStream;
use tokio_util::codec::{FramedRead, LinesCodec}; // For reading lines from UnixStream
use futures::stream::StreamExt; // For stream.next()
use clap::Parser; // Added for CLI argument parsing
use notify_rust::Notification; // Added for desktop notifications
use home; // Added for persisting username
use std::path::PathBuf; // Added for instance-specific username file path

mod components; // Added to use the components module

// --- IPC Structures ---
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcPeer {
    pub id: String,       // Unique identifier for the peer (e.g., from mDNS)
    pub username: String, // Display name
    // Add other relevant peer info if needed, e.g., IP address, port for direct comms if ever used
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GuiToDaemonCommand {
    GetPeers,
    SendMessage {
        recipient_id: String, // ID of the peer to send to (or a broadcast/group ID)
        content: String,
    },
    RequestHistory { // Example for history
        peer_id: String, 
        since_timestamp: Option<chrono::DateTime<chrono::Utc>>,
    },
    SetUsername { username: String }, // Added command
    ClearDaemonPeerCache, // Added command to tell daemon to clear its peer list
    // Add other commands as needed (e.g., set username, status updates)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DaemonToGuiMessage {
    DaemonStatus { 
        is_connected_to_network: bool, 
        active_interface_name: Option<String> 
    },
    PeerList(Vec<IpcPeer>),
    NewMessage(Message), // Reusing the existing Message struct
    HistoryResponse { 
        peer_id: String, 
        messages: Vec<Message> 
    },
    Error(String), // For generic error reporting from daemon to GUI
    IdentityInfo { user_id: String }, // Added new variant
    Success(String), // Added for success confirmations from daemon
}
// --- End IPC Structures ---

#[derive(Debug, Clone, Serialize, Deserialize)] // Added Serialize/Deserialize for Message struct
pub struct Message {
    pub id: String,
    pub sender: String,      // Now can be "You" or an IpcPeer.username (or IpcPeer.id)
    pub recipient: String, // Who the message is for (peer_id, broadcast, etc.)
    pub content: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub is_self: bool,
}

#[derive(PartialEq, Debug)]
enum CurrentPanel {
    Chat,
    History,
    Settings,
}

// Moved Type Aliases before ChatApp struct definition
// Channel for sending commands from GUI to Daemon
type GuiToDaemonTx = mpsc::Sender<GuiToDaemonCommand>;
// Channel for receiving messages/events from Daemon to GUI
type DaemonToGuiRx = mpsc::Receiver<DaemonToGuiMessage>;

// State for the settings panel
struct SettingsState {
    edit_username_input: String,
}

impl SettingsState {
    fn new() -> Self {
        Self { edit_username_input: String::new() }
    }
}

// Define CLI arguments
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long, default_value_t = 1)]
    instance: u16,
}

struct ChatApp {
    message_input: String,
    messages: Vec<Message>,
    current_panel: CurrentPanel,
    peers: Vec<IpcPeer>,             // To store discovered peers
    daemon_status: Option<DaemonToGuiMessage>, // To store last known daemon status
    current_chat_peer_id: Option<String>, // ID of the peer the user is currently chatting with
    current_user_id: Option<String>, // Changed to Option<String>
    username_input: String, // Added for username prompt
    show_username_prompt: bool, // Added to control username prompt visibility
    username_file_path: Option<PathBuf>, // Added for instance-specific username file path
    is_loading: bool, // Added to show a loader until identity is confirmed
    settings_state: SettingsState, // Added for settings panel UI state
    
    // IPC related fields
    rt: Arc<tokio::runtime::Runtime>,
    gui_to_daemon_tx: Option<GuiToDaemonTx>,
    daemon_to_gui_rx: Arc<TokioMutex<Option<DaemonToGuiRx>>>,
    ipc_connection_status: String, // To display connection status to daemon
    requested_initial_peers: bool, // Flag to ensure we only request once
}

impl ChatApp {
    fn new(
        _cc: &eframe::CreationContext<'_>, 
        daemon_socket_path_for_instance: String,
        username_file_path_for_instance: Option<PathBuf> // Added new parameter
    ) -> Self {
        let rt = Arc::new(tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime"));
        let (daemon_to_gui_tx, daemon_to_gui_rx_local) = mpsc::channel::<DaemonToGuiMessage>(32);
        let daemon_to_gui_rx_arc = Arc::new(TokioMutex::new(Some(daemon_to_gui_rx_local)));

        let (gui_cmd_tx, mut gui_cmd_rx) = mpsc::channel::<GuiToDaemonCommand>(32);
        let task_socket_path = daemon_socket_path_for_instance.clone();

        // Attempt to load username from file
        let mut loaded_username = String::new();
        let mut initial_show_username_prompt = true;
        // let username_file_path = home::home_dir().map(|mut path| { // OLD: Will be replaced by using the parameter
        //     path.push(".localchat_gui_username");
        //     path
        // });

        if let Some(ref path) = username_file_path_for_instance { // Use the passed parameter
            if let Ok(username_from_file) = std::fs::read_to_string(path) {
                let trimmed_username = username_from_file.trim();
                if !trimmed_username.is_empty() {
                    loaded_username = trimmed_username.to_string();
                    initial_show_username_prompt = false;
                    println!("GUI: Loaded username '{}' from file: {:?}", loaded_username, path);
                }
            }
        }

        // Clone rt and gui_cmd_tx for the app instance *before* rt is moved into the IPC task
        let rt_clone_for_app = rt.clone();
        let gui_cmd_tx_clone_for_app = gui_cmd_tx.clone();

        rt.spawn(async move {
            // Connection loop is the same as before
            loop {
                println!("Attempting to connect to daemon at {}", task_socket_path);
                match UnixStream::connect(&task_socket_path).await {
                    Ok(stream) => {
                        println!("Connected to daemon at {}", task_socket_path);
                        let _ = daemon_to_gui_tx.send(DaemonToGuiMessage::DaemonStatus { 
                            is_connected_to_network: true, 
                            active_interface_name: Some("Connected".to_string()) 
                        }).await;
                        
                        let (reader, mut writer) = tokio::io::split(stream);
                        let mut framed_reader = FramedRead::new(reader, LinesCodec::new());
                        
                        loop {
                            tokio::select! {
                                Some(line_result) = framed_reader.next() => {
                                    match line_result {
                                        Ok(line) => {
                                            match serde_json::from_str::<DaemonToGuiMessage>(&line) {
                                                Ok(msg) => {
                                                    if daemon_to_gui_tx.send(msg).await.is_err() {
                                                        eprintln!("Failed to send daemon message to GUI: receiver dropped.");
                                                        break; 
                                                    }
                                                }
                                                Err(e) => {
                                                    eprintln!("Failed to deserialize message from daemon: {}. Line: {}", e, line);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Error reading from daemon socket: {}", e);
                                            let _ = daemon_to_gui_tx.send(DaemonToGuiMessage::Error("Socket read error".to_string())).await;
                                            break; 
                                        }
                                    }
                                },
                                Some(command_to_send) = gui_cmd_rx.recv() => {
                                    match serde_json::to_string(&command_to_send) {
                                        Ok(json_cmd) => {
                                            use tokio::io::AsyncWriteExt;
                                            if let Err(e) = writer.write_all(format!("{}\n", json_cmd).as_bytes()).await {
                                                eprintln!("Failed to send command to daemon: {}", e);
                                                let _ = daemon_to_gui_tx.send(DaemonToGuiMessage::Error("Socket write error".to_string())).await;
                                                break; // Break select loop, will lead to reconnect attempt
                                            }
                                        }
                                        Err(e) => {
                                            eprintln!("Failed to serialize command for daemon: {}", e);
                                        }
                                    }
                                },
                                else => {
                                    println!("IPC streams closed for {}", task_socket_path);
                                    break;
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("Failed to connect to daemon at {}: {}", task_socket_path, e);
                        let _ = daemon_to_gui_tx.send(DaemonToGuiMessage::DaemonStatus { 
                            is_connected_to_network: false, 
                            active_interface_name: Some(format!("Connection failed for {}: {}", task_socket_path, e)) 
                        }).await;
                    }
                }
                println!("Retrying connection to {} in 5s...", task_socket_path);
                tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;
            }
        });

        let mut app = Self {
            message_input: String::new(),
            messages: Vec::new(),
            current_panel: CurrentPanel::Chat,
            peers: Vec::new(),
            daemon_status: None,
            current_chat_peer_id: None,
            current_user_id: None, // Initialized to None
            username_input: loaded_username.clone(), // Use loaded username or empty
            show_username_prompt: initial_show_username_prompt, // Show prompt based on loaded status
            username_file_path: username_file_path_for_instance.clone(), // Store the passed path
            is_loading: !initial_show_username_prompt, // If username loaded, start in loading state, else prompt will show first
            settings_state: SettingsState::new(), // Initialize settings state
            rt: rt_clone_for_app, 
            gui_to_daemon_tx: Some(gui_cmd_tx_clone_for_app), 
            daemon_to_gui_rx: daemon_to_gui_rx_arc,
            ipc_connection_status: "Connecting...".to_string(),
            requested_initial_peers: false, // Initialize flag
        };

        // If username was loaded, send it to the daemon
        if !initial_show_username_prompt {
            if let Some(tx) = &app.gui_to_daemon_tx {
                let command = GuiToDaemonCommand::SetUsername { username: loaded_username.clone() };
                let tx_clone = tx.clone();
                let username_to_send = loaded_username.clone(); // Clone for the async block
                app.rt.spawn(async move {
                    println!("GUI: Sending pre-loaded username '{}' to daemon.", username_to_send);
                    if let Err(e) = tx_clone.send(command).await {
                        eprintln!("Failed to send pre-loaded SetUsername command: {}", e);
                    }
                });
            }
        }
        app
    }
}

impl eframe::App for ChatApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Ok(mut guard) = self.daemon_to_gui_rx.try_lock() {
            if let Some(ref mut rx) = *guard {
                // Explicitly tell try_recv what type to expect
                while let Ok(msg) = rx.try_recv() {
                    match msg {
                        DaemonToGuiMessage::DaemonStatus { is_connected_to_network, active_interface_name } => {
                            self.ipc_connection_status = active_interface_name.unwrap_or_else(|| 
                                if is_connected_to_network { "Connected".to_string() } else { "Disconnected".to_string() }
                            );
                        }
                        DaemonToGuiMessage::PeerList(peers) => {
                            // بسيط: Debug print to see if we ever get here and what peers contains
                            println!("GUI: Received PeerList: {:?}", peers);
                            self.peers = peers;
                        }
                        DaemonToGuiMessage::NewMessage(message) => { // message is already a full Message struct from daemon
                            println!("GUI: NewMessage handler received: ID={}, Sender='{}', Content='{}', is_self (from daemon)={}, CurrentUserID='{:?}'",
                                message.id, message.sender, message.content, message.is_self, self.current_user_id);
                        
                            // Any NewMessage from the daemon is considered a message from a foreign peer, as own messages are added locally
                            // and the daemon is not expected to echo them back as NewMessage.
                            // The daemon's handle_peer_tcp_connection already sets received_message.is_self = false.

                            // We still check sender against current_user_id as a safeguard or if daemon logic changes.
                            if self.current_user_id.as_ref().map_or(false, |uid| *uid == message.sender) {
                                // This case implies an echo of our own message, which we typically don't expect here
                                // if we've added it locally. If daemon *does* echo, this log helps.
                                println!("GUI: Received NewMessage where sender ('{}') matches current_user_id ('{:?}'). ID: {}. This might be an unexpected echo. Ignoring.",
                                    message.sender, self.current_user_id, message.id);
                            } else {
                                // This is confirmed to be a message from another peer.
                                // Add it to our list if its ID is not already present (to prevent daemon resend-duplicates).
                                if !self.messages.iter().any(|m| m.id == message.id) {
                                    println!("GUI: New foreign message (ID: {} from sender '{}'). Adding to list.", message.id, message.sender);
                                    
                                    if let Err(e) = Notification::new()
                                        .summary(&format!("New message from {}", message.sender))
                                        .body(&message.content)
                                        .icon("dialog-information")
                                        .appname("LocalChatGUI")
                                        .show() {
                                        eprintln!("Error displaying notification: {}", e);
                                    }

                                    let mut display_message = message; // clone the received message
                                    display_message.is_self = false; // Ensure is_self is false for display purposes
                                    self.messages.push(display_message);
                                } else {
                                    println!("GUI: Duplicate foreign message (ID: {} from sender '{}'). Not adding.", message.id, message.sender);
                                }
                            }
                        }
                        DaemonToGuiMessage::HistoryResponse { messages, .. } => {
                            self.messages.extend(messages);
                        }
                        DaemonToGuiMessage::Error(err_msg) => {
                            self.ipc_connection_status = format!("Daemon Error: {}", err_msg);
                            eprintln!("Received error from daemon: {}", err_msg);
                        }
                        DaemonToGuiMessage::IdentityInfo { user_id } => {
                            let old_id_log_display = self.current_user_id.as_deref().unwrap_or("None").to_string(); // Clone to avoid borrow issue
                            self.current_user_id = Some(user_id.clone());
                            self.show_username_prompt = false; // Hide prompt after getting identity
                            self.is_loading = false; // Stop loading screen
                            println!("GUI: Received IdentityInfo, current_user_id set from '{}' to: {}", old_id_log_display, user_id);
                            
                            // Request peers once after identity is confirmed and if not already requested
                            if !self.requested_initial_peers {
                                if let Some(tx) = &self.gui_to_daemon_tx {
                                    let tx_clone = tx.clone();
                                    self.rt.spawn(async move {
                                        println!("GUI: Sending initial GetPeers request.");
                                        if let Err(e) = tx_clone.send(GuiToDaemonCommand::GetPeers).await {
                                            eprintln!("Failed to send initial GetPeers request: {}", e);
                                        }
                                    });
                                    self.requested_initial_peers = true;
                                }
                            }
                        }
                        DaemonToGuiMessage::Success(msg) => {
                            self.ipc_connection_status = format!("Success: {}", msg);
                            println!("Received success message from daemon: {}", msg);
                        }
                    }
                }
            }
        }

        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        // --- Username Prompt Modal --- / Loading State --- / Main UI ---
        if self.show_username_prompt {
            let modal_width = 400.0;
            let modal_height = 280.0;
            
            egui::Window::new("")
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .collapsible(false)
                .resizable(false)
                .fixed_size([modal_width, modal_height])
                .frame(egui::Frame::none()
                    .fill(egui::Color32::from_rgb(18, 18, 22))
                    .corner_radius(16.0)
                    .inner_margin(20.0)
                    .shadow(egui::epaint::Shadow {
                        offset: [0, 4],
                        blur: 8,
                        spread: 0,
                        color: egui::Color32::from_black_alpha(60),
                    })
                )
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.add_space(20.0);
                        ui.heading(
                            egui::RichText::new("Set Your Username")
                                .size(24.0)
                                .color(egui::Color32::from_rgb(240, 240, 250))
                                .strong()
                        );
                        
                        ui.add_space(10.0);
                        ui.label(
                            egui::RichText::new("Please enter a username to join the chat:")
                                .size(16.0)
                                .color(egui::Color32::from_rgb(200, 200, 210))
                        );
                        
                        ui.add_space(30.0);        

                        ui.add_sized(
                            [modal_width - 80.0, 50.0],
                            egui::TextEdit::singleline(&mut self.username_input)
                                .hint_text("Your Username")
                                .frame(true)
                                .margin(egui::vec2(8.0, 12.0))
                                .text_color(egui::Color32::from_rgb(240, 240, 240))
                                .font(egui::FontId::proportional(16.0))
                                .horizontal_align(egui::Align::Center)
                        );
                        ui.add_space(30.0);
                        
                        // Styled button
                        let button_bg = egui::Color32::from_rgb(25, 118, 210);
                        if ui.add_sized(
                            [modal_width - 80.0, 50.0],
                            egui::Button::new(
                                egui::RichText::new("Set Username")
                                    .size(16.0)
                                    .color(egui::Color32::WHITE)
                                    .strong()
                            )
                            .fill(button_bg)
                            .corner_radius(12.0)
                            .stroke(egui::Stroke::NONE)
                        ).clicked() || (ui.input(|i| i.key_pressed(egui::Key::Enter)) && !self.username_input.trim().is_empty()) {
                            if !self.username_input.trim().is_empty() {
                                if let Some(tx) = &self.gui_to_daemon_tx {
                                    let command = GuiToDaemonCommand::SetUsername { username: self.username_input.trim().to_string() };
                                    let tx_clone = tx.clone();
                                    let username_to_save = self.username_input.trim().to_string(); // for logging & saving
                                    let username_for_async_block = username_to_save.clone(); // Clone for the async block
                                    self.rt.spawn(async move {
                                        println!("GUI: Sending SetUsername command with username: {}", username_for_async_block);
                                        if let Err(e) = tx_clone.send(command).await {
                                            eprintln!("Failed to send SetUsername command: {}", e);
                                        }
                                    });
                                    // Save the username
                                    if let Some(ref path) = self.username_file_path { 
                                        if let Err(e) = std::fs::write(path, username_to_save.as_bytes()) { 
                                            eprintln!("Failed to save username to file {:?}: {}", path, e);
                                        } else {
                                            println!("GUI: Saved username '{}' to file {:?}.", username_to_save, path);
                                        }
                                    } else {
                                        eprintln!("Failed to determine instance-specific username file path to save username.");
                                    }
                                    // After attempting to send and save, hide prompt and show loader
                                    self.show_username_prompt = false;
                                    self.is_loading = true; 
                                } else {
                                    eprintln!("Error: gui_to_daemon_tx is None, cannot send SetUsername");
                                }
                            }
                        }
                        
                        ui.add_space(20.0);
                        ui.label(
                            egui::RichText::new("(This is required to discover and chat with others)")
                                .size(14.0)
                                .color(egui::Color32::from_rgb(160, 160, 180))
                                .italics()
                        );
                    });
                });
        } else if self.is_loading {
            // --- Loading State ---
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.centered_and_justified(|ui| {
                    ui.add(egui::Spinner::new().size(50.0));
                });
            });
        } else {
            // Only show the main UI if the username prompt is not active and not loading
            egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
                components::topnav::show(
                    ui, 
                    &mut self.current_panel, 
                    &self.current_user_id
                );
            });

            egui::SidePanel::left("side_panel").show(ctx, |ui| {
                components::sidemenu::show(
                    ui, 
                    &self.peers, 
                    &mut self.current_chat_peer_id,
                    &self.gui_to_daemon_tx, // Pass the sender
                    &self.rt, // Pass the Tokio runtime Arc
                    &self.current_user_id // Pass the current username
                );
            });

            egui::CentralPanel::default().show(ctx, |ui| match self.current_panel {
                CurrentPanel::Chat => {
                    components::chat_area::show(
                        ui, 
                        &mut self.messages, 
                        &mut self.message_input, 
                        &self.current_user_id.as_deref().unwrap_or_default(),
                        &self.current_chat_peer_id,
                        &self.gui_to_daemon_tx, // Pass the sender
                        &self.rt // Pass the Tokio runtime Arc
                    );
                }
                CurrentPanel::History => {
                    components::history::show(ui);
                }
                CurrentPanel::Settings => {
                    components::settings::show(
                        ui, 
                        &mut self.settings_state,
                        &mut self.current_user_id,
                        &self.username_file_path,
                        &self.gui_to_daemon_tx,
                        &self.rt,
                        &mut self.show_username_prompt,
                        &mut self.is_loading,
                        &mut self.peers
                    );
                }
            });
        }
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();
    println!("GUI Instance: {}", args.instance);

    let daemon_tcp_port_for_instance = 12345 + (args.instance - 1);
    let daemon_socket_path_for_instance = format!("/tmp/localchat_daemon{}.sock", args.instance);

    // Construct instance-specific username file path
    let username_file_path_for_instance = home::home_dir().map(|mut path| {
        path.push(format!(".localchat_gui_username_{}", args.instance));
        path
    });

    let mut daemon_command = Command::new("");

    if cfg!(debug_assertions) {
        println!("DEBUG mode: Starting daemon with 'cargo run' for instance {}", args.instance);
        daemon_command = Command::new("cargo");
        daemon_command.args(["run", "--quiet", "-p", "localchat_daemon"]);
    } else {
        println!("RELEASE mode: Attempting to start pre-compiled daemon for instance {}", args.instance);
        match env::current_exe() {
            Ok(mut exe_path) => {
                exe_path.pop(); 
                let daemon_name = if cfg!(windows) { "localchat_daemon.exe" } else { "localchat_daemon" };
                exe_path.push(daemon_name);
                println!("Attempting to run daemon from: {:?}", exe_path);
                daemon_command = Command::new(exe_path);
            }
            Err(e) => {
                eprintln!("Failed to get current executable path: {}", e);
                return Err(Box::new(e));
            }
        }
    }

    // Set environment variables for the daemon process
    daemon_command.env("LOCALCHAT_TCP_PORT", daemon_tcp_port_for_instance.to_string());
    daemon_command.env("LOCALCHAT_SOCKET_PATH", &daemon_socket_path_for_instance);

    println!(
        "Attempting to start localchat_daemon for instance {} with TCP Port: {}, Socket: {}, Command: {:?}",
        args.instance, daemon_tcp_port_for_instance, daemon_socket_path_for_instance, daemon_command
    );
    match daemon_command.spawn() {
        Ok(mut child) => {
            println!(
                "Successfully spawned localchat_daemon process for instance {}. PID: {}",
                args.instance, child.id()
            );
            // Optionally, manage child process, e.g., kill on GUI exit
        }
        Err(e) => {
            eprintln!("Failed to start localchat_daemon for instance {}: {}.", args.instance, e);
            // Continue without daemon for GUI testing if needed, or return error
        }
    }

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([1024.0, 768.0]),
        ..Default::default()
    };

    // Pass the correct socket path to ChatApp::new
    let app_socket_path = daemon_socket_path_for_instance.clone();

    eframe::run_native(
        &format!("Local Chat GUI - Instance {}", args.instance), // Unique window title
        options,
        Box::new(move |cc| Ok(Box::new(ChatApp::new(cc, app_socket_path, username_file_path_for_instance)))), // Pass new path
    ).map_err(|e| Box::new(e) as Box<dyn Error>)?;
    Ok(())
}
