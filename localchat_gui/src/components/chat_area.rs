use eframe::egui;
use crate::{Message, IpcPeer, GuiToDaemonCommand}; // Import IpcPeer and GuiToDaemonCommand
use chrono::Utc; // Import Utc for direct use
use freedesktop_icons::lookup; // Added for icon lookup
use tokio::sync::mpsc; // For Sender type
use std::sync::Arc;
use uuid;

// Updated to accept current_chat_peer_id and the list of peers
pub fn show(
    ui: &mut egui::Ui, 
    messages: &mut Vec<Message>, 
    message_input: &mut String, 
    current_user_id: &str, // Placeholder for the actual current user's ID/name
    current_chat_peer_id: &Option<String>,
    gui_to_daemon_tx: &Option<mpsc::Sender<GuiToDaemonCommand>>,
    rt: &Arc<tokio::runtime::Runtime>
) {
    // Modernized styling for chat bubbles and text
    let self_bubble_color = egui::Color32::from_rgb(0, 122, 255);    // iOS-like blue
    let other_bubble_color = egui::Color32::from_rgb(229, 229, 234); // iOS-like light gray
    let self_text_color = egui::Color32::WHITE;
    let other_text_color = egui::Color32::BLACK;
    let self_timestamp_color = egui::Color32::from_gray(200); // Light gray for timestamp on dark blue self-bubble
    let other_timestamp_color = egui::Color32::from_gray(100); // Darker gray for timestamp on light gray other-bubble

    // Input area UI (drawn first to reserve space at the bottom)
    let input_area_height = 40.0; // Includes padding + input field height
    let mut input_ui_rect = ui.available_rect_before_wrap();
    input_ui_rect.min.y = input_ui_rect.max.y - input_area_height;
    
    let mut input_ui = ui.child_ui(input_ui_rect, egui::Layout::top_down(egui::Align::Min), None);
    input_ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
        ui.add_space(5.0); // Small padding below the input section
        ui.horizontal(|ui| {
            ui.add_space(5.0); // Padding on the left of input
            let new_input_height = 30.0; // Defined height for input elements
            // Adjusted width for spacing, considering button and side paddings
            let text_edit_width = ui.available_width() - 35.0 - 15.0; 

            let text_edit_response = ui.add_sized(
                [text_edit_width, new_input_height],
                egui::TextEdit::singleline(message_input).hint_text("Type a message...")
            );

            ui.add_space(5.0); // Space between text edit and button

            // Icon handling for send button
            let send_button_content = 
                match lookup("document-send").find()
                    .or_else(|| lookup("mail-send").find())
                    .or_else(|| lookup("go-next").find())
                {
                    Some(path) => egui::RichText::new(format!(" {} ", path.to_string_lossy())).strong(), // Placeholder, needs actual image loading
                    None => egui::RichText::new("➤").strong(), // Fallback to text
            };
            
            let send_button_response = ui.add_sized([35.0, new_input_height], egui::Button::new(send_button_content));
            ui.add_space(5.0); // Padding on the right of button

            let enter_pressed = text_edit_response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if enter_pressed || send_button_response.clicked() {
                if !message_input.trim().is_empty() {
                    if let Some(recipient_id) = current_chat_peer_id.as_ref() {
                        if !recipient_id.is_empty() { // Ensure a peer is actually selected
                            let command = GuiToDaemonCommand::SendMessage {
                                recipient_id: recipient_id.clone(),
                                content: message_input.trim().to_string(),
                            };

                            if let Some(tx) = gui_to_daemon_tx {
                                let tx_clone = tx.clone();
                                let rt_clone = rt.clone(); // Clone Arc for the new task
                                println!("GUI: Preparing to send command: {:?}", command); // Log before spawn
                                
                                // Create a message object for the sent message and add it to messages vector
                                let msg = Message {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    sender: current_user_id.to_string(),
                                    recipient: recipient_id.clone(),
                                    content: message_input.trim().to_string(),
                                    timestamp: chrono::Utc::now(),
                                    is_self: true,
                                };
                                messages.push(msg);
                                
                                rt_clone.spawn(async move {
                                    println!("GUI: Sending SendMessage command: {:?}", command);
                                    if let Err(e) = tx_clone.send(command).await {
                                        eprintln!("GUI: Failed to send SendMessage command: {}", e);
                                    }
                                });
                            } else {
                                eprintln!("GUI: gui_to_daemon_tx is None, cannot send message.");
                            }
                            message_input.clear();
                            // text_edit_response.request_focus(); // Optional: refocus after sending
                        } else {
                            println!("GUI: No recipient selected (recipient_id is empty string).");
                            // Optionally provide user feedback: e.g., show a temporary error message
                        }
                    } else {
                        println!("GUI: No recipient selected (current_chat_peer_id is None).");
                        // Optionally provide user feedback
                    }
                }
            }
            if text_edit_response.gained_focus() || send_button_response.clicked() {
                // Potentially scroll to bottom or ensure input is visible
            }
        });
        ui.add_space(5.0); // Small padding above the input controls
    });

    // Message display area (takes the rest of the space)
    let mut message_area_rect = ui.available_rect_before_wrap();
    message_area_rect.max.y = input_ui_rect.min.y; // End before the input area starts
    let mut message_ui = ui.child_ui(message_area_rect, egui::Layout::top_down(egui::Align::Min), None);
    
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(&mut message_ui, |ui| {
            ui.add_space(10.0);
            for message in messages.iter() {
                let (bubble_color, text_color, timestamp_color) = if message.is_self {
                    (self_bubble_color, self_text_color, self_timestamp_color)
                } else {
                    (other_bubble_color, other_text_color, other_timestamp_color)
                };
                let alignment = if message.is_self { egui::Align::Max } else { egui::Align::Min };

                ui.with_layout(egui::Layout::top_down(alignment), |ui| {
                    egui::Frame::new()
                        .fill(bubble_color)
                        .corner_radius(10)
                        .inner_margin(egui::Margin::symmetric(10, 5))
                        .outer_margin(egui::Margin {
                            left: if message.is_self { 50 } else { 5 },
                            right: if message.is_self { 5 } else { 50 },
                            top: 2,
                            bottom: 2,
                        })
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.label(egui::RichText::new(&message.sender).strong().small().color(text_color));
                            ui.label(egui::RichText::new(&message.content).color(text_color));
                            ui.add_space(2.0);
                            ui.label(egui::RichText::new(message.timestamp.format("%H:%M").to_string()).small().color(timestamp_color));
                        });
                });
                ui.add_space(5.0);
            }
            ui.add_space(10.0);
        });
} 