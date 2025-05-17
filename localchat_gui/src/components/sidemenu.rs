use eframe::egui;
use crate::{IpcPeer, GuiToDaemonCommand}; // Assuming IpcPeer is in crate root (main.rs)
use tokio::sync::mpsc; // For Sender type
use std::sync::Arc;

// Placeholder for side menu UI
pub fn show(
    ui: &mut egui::Ui, 
    peers: &[IpcPeer], 
    current_chat_peer_id: &mut Option<String>,
    gui_to_daemon_tx: &Option<mpsc::Sender<GuiToDaemonCommand>>,
    rt: &Arc<tokio::runtime::Runtime>
) {
    ui.horizontal(|ui| {
        ui.heading("Peers");
        if ui.button("🔃 Refresh").clicked() { // Unicode refresh symbol
            if let Some(tx) = gui_to_daemon_tx {
                let tx_clone = tx.clone();
                rt.spawn(async move {
                    println!("GUI: Sending GetPeers request (manual refresh).");
                    if let Err(e) = tx_clone.send(GuiToDaemonCommand::GetPeers).await {
                        eprintln!("Failed to send manual GetPeers request: {}", e);
                    }
                });
            }
        }
    });

    ui.add_space(10.0);

    if peers.is_empty() {
        ui.label("(No peers found)");
    } else {
        egui::ScrollArea::vertical().show(ui, |ui| {
            for peer in peers {
                let is_selected = current_chat_peer_id.as_ref().map_or(false, |id| *id == peer.id);
                if ui.selectable_label(is_selected, &peer.username).clicked() {
                    if is_selected {
                        *current_chat_peer_id = None; // Click again to deselect
                    } else {
                        *current_chat_peer_id = Some(peer.id.clone());
                        println!("GUI: Selected peer for chat: {} ({})", peer.username, peer.id);
                    }
                }
            }
        });
    }
    
    // You could add other side menu items here, like daemon status, settings shortcuts, etc.
    ui.separator();
    // Example: Display current chat target for debugging
    // if let Some(id) = current_chat_peer_id {
    //     ui.label(format!("Chatting with: {}", id));
    // } else {
    //     ui.label("Chatting with: (Broadcast/None)");
    // }
} 