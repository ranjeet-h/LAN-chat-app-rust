use eframe::egui;
use std::sync::Arc;
use std::path::PathBuf;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use crate::{GuiToDaemonCommand, IpcPeer, SettingsState};

pub fn show(
    ui: &mut egui::Ui,
    settings_state: &mut SettingsState,
    current_user_id: &mut Option<String>,
    username_file_path: &Option<PathBuf>,
    gui_to_daemon_tx: &Option<mpsc::Sender<GuiToDaemonCommand>>,
    rt: &Arc<Runtime>,
    show_username_prompt: &mut bool,
    is_loading: &mut bool,
    // current_username_input: &mut String, // This is the ChatApp.username_input for the main prompt
    peers: &mut Vec<IpcPeer>,
) {
    ui.heading("User Settings");
    ui.add_space(10.0);

    // Display Current Username
    if let Some(id) = current_user_id.as_ref() {
        // Extract the user-provided part of the ID for display if possible
        let display_name = id.split(" - ").next().unwrap_or(id.as_str());
        ui.label(format!("Current Username: {}", display_name));
        ui.label(format!("(Full ID: {})", id));
    } else {
        ui.label("No username is currently set.");
    }
    ui.add_space(15.0);

    // Edit Username Section
    ui.label("Edit Username:");
    ui.horizontal(|ui| {
        ui.label("New username:");
        ui.add(egui::TextEdit::singleline(&mut settings_state.edit_username_input).desired_width(200.0));
    });

    if ui.button("Update Username").clicked() {
        let new_username_trimmed = settings_state.edit_username_input.trim();
        if !new_username_trimmed.is_empty() {
            if let Some(tx) = gui_to_daemon_tx {
                let command = GuiToDaemonCommand::SetUsername { username: new_username_trimmed.to_string() };
                let tx_clone = tx.clone();
                let username_to_save = new_username_trimmed.to_string();
                
                // 1. Save to file first
                if let Some(path) = username_file_path {
                    if let Err(e) = std::fs::write(path, username_to_save.as_bytes()) {
                        eprintln!("Settings: Failed to save updated username to file {:?}: {}", path, e);
                        // Optionally, show this error in the UI
                    } else {
                        println!("Settings: Saved updated username '{}' to file {:?}.", username_to_save, path);
                    }
                }

                // 2. Send to daemon
                rt.spawn(async move {
                    println!("Settings: Sending SetUsername command with new username: {}", username_to_save);
                    if let Err(e) = tx_clone.send(command).await {
                        eprintln!("Settings: Failed to send SetUsername command for update: {}", e);
                    }
                });
                
                // 3. Set GUI to loading state to wait for new IdentityInfo
                *is_loading = true;
                settings_state.edit_username_input.clear(); // Clear input field after submission
            } else {
                eprintln!("Settings: gui_to_daemon_tx is None. Cannot update username.");
            }
        }
    }
    ui.add_space(15.0);

    // Delete Username Section
    if ui.button("Delete Current Username and Reset").clicked() {
        // 1. Delete the username file
        if let Some(path) = username_file_path {
            if std::fs::remove_file(path).is_ok() {
                println!("Settings: Deleted username file: {:?}", path);
            } else {
                eprintln!("Settings: Failed to delete username file: {:?}", path);
            }
        }
        
        // 2. Clear app state related to username
        *current_user_id = None;
        // current_username_input.clear(); // Clear the main prompt's input field too
        settings_state.edit_username_input.clear(); // Clear settings input

        // 3. Show the username prompt
        *show_username_prompt = true;
        *is_loading = false; // Not loading, showing prompt

        // Note: No specific command to daemon to "ClearIdentity". 
        // The daemon will keep its old identity until it's restarted or a new SetUsername is received.
        // Or if GUI disconnects, it might clear itself. This is acceptable for now.
        println!("Settings: Username deleted. Application will show username prompt.");
    }

    ui.separator();
    ui.add_space(10.0);
    ui.heading("Peer Management");
    ui.add_space(10.0);

    if ui.button("Clear Cached Peer List and Refresh").clicked() {
        peers.clear(); // Clear GUI's local cache immediately
        println!("Settings: Cleared local peer cache (GUI).");
        
        if let Some(tx) = gui_to_daemon_tx {
            let tx_clone_clear = tx.clone();
            let tx_clone_get_peers = tx.clone(); // Separate clone for GetPeers
            rt.spawn(async move {
                println!("Settings: Sending ClearDaemonPeerCache command to daemon.");
                if let Err(e) = tx_clone_clear.send(GuiToDaemonCommand::ClearDaemonPeerCache).await {
                    eprintln!("Settings: Failed to send ClearDaemonPeerCache command: {}", e);
                    // Optionally, you might not want to request peers if this fails, or show an error.
                }
                // Regardless of the above outcome for now, or after a slight delay, 
                // or after handling a success message (more complex), request the peer list.
                // Sending it immediately after should be fine as daemon processes commands sequentially.
                println!("Settings: Sending GetPeers command to daemon after ClearDaemonPeerCache.");
                if let Err(e) = tx_clone_get_peers.send(GuiToDaemonCommand::GetPeers).await {
                    eprintln!("Settings: Failed to send GetPeers command after clearing: {}", e);
                }
            });
        }
    }
} 