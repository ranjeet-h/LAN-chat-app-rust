use eframe::egui;
use crate::{Message, GuiToDaemonCommand}; // Removed unused IpcPeer import
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
    // Modern styling for chat bubbles and text - optimized for dark background
    let self_bubble_color = egui::Color32::from_rgb(25, 118, 210);    // Modern blue
    let other_bubble_color = egui::Color32::from_rgb(66, 66, 66);     // Dark gray that works on black
    let self_text_color = egui::Color32::WHITE;
    let other_text_color = egui::Color32::from_rgb(240, 240, 240);    // Light gray for better contrast
    let self_timestamp_color = egui::Color32::from_rgb(200, 200, 200); // Light gray for timestamp
    let other_timestamp_color = egui::Color32::from_rgb(180, 180, 180); // Slightly darker gray for timestamps
    let accent_color = self_bubble_color;                             // Use blue as accent color throughout the app

    // UI Design Constants
    let bubble_radius = 12;  // Changed from 12.0 to 12 (u8)
    let input_height = 40.0;
    let input_radius = 20.0;
    let button_size = 38.0;
    
    let total_area = ui.available_rect_before_wrap();
    
    // Calculate fixed heights for input area - increased for visibility
    let input_area_height = 70.0; // Increased from 60.0 to 70.0 for better visibility
    
    // Create the message area (everything except input area)
    let message_area_rect = egui::Rect::from_min_max(
        total_area.min,
        egui::pos2(total_area.max.x, total_area.max.y - input_area_height)
    );
    
    // Create the input area (bottom part)
    let input_area_rect = egui::Rect::from_min_max(
        egui::pos2(total_area.min.x, total_area.max.y - input_area_height),
        total_area.max
    );
    
    // Draw a divider between message area and input area
    ui.painter().line_segment(
        [
            egui::pos2(total_area.min.x, input_area_rect.min.y),
            egui::pos2(total_area.max.x, input_area_rect.min.y),
        ],
        egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 45, 45))
    );
    
    // Draw a subtle background for the input area
    // ui.painter().rect_filled(
    //     input_area_rect,
    //     20.0,
    //     egui::Color32::from_rgb(18, 18, 18) // Slightly lighter than black for separation
    // );
    
    // First, render the messages area (using child_ui but keeping other API updates)
    let mut message_ui = ui.child_ui(message_area_rect, egui::Layout::top_down(egui::Align::Min), None);
    
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(&mut message_ui, |ui| {
            ui.add_space(12.0);
            
            // Group messages by day
            let mut last_message_date = None;
            
            for message in messages.iter() {
                // Check if it's a new day and add date separator if needed
                let current_date = message.timestamp.date_naive();
                if last_message_date.map_or(true, |date| date != current_date) {
                    // Add date separator
                    ui.add_space(8.0);
                    ui.with_layout(egui::Layout::top_down(egui::Align::Center), |ui| {
                        let date_text = if current_date == chrono::Utc::now().date_naive() {
                            "Today".to_string()
                        } else if current_date == (chrono::Utc::now() - chrono::Duration::days(1)).date_naive() {
                            "Yesterday".to_string()
                        } else {
                            message.timestamp.format("%B %d, %Y").to_string()
                        };
                        
                        ui.label(
                            egui::RichText::new(date_text)
                                .size(11.0)
                                .color(egui::Color32::from_rgb(160, 160, 160))
                        );
                    });
                    ui.add_space(8.0);
                    last_message_date = Some(current_date);
                }
                
                // Print message for debugging
                println!("Processing message: is_self={}, sender={}, content={}", 
                    message.is_self, message.sender, message.content);
                
                // Message bubble styling
                let (bubble_color, text_color, timestamp_color) = if message.is_self {
                    (self_bubble_color, self_text_color, self_timestamp_color)
                } else {
                    (other_bubble_color, other_text_color, other_timestamp_color)
                };
                
                let alignment = if message.is_self { egui::Align::Max } else { egui::Align::Min };

                ui.with_layout(egui::Layout::top_down(alignment), |ui| {
                    let max_width = ui.available_width() * 0.7; // Max width for messages is 70% of area
                    
                    // Display sender name above the first message
                    if !message.is_self {
                        ui.add_space(2.0);
                        ui.scope(|ui| {
                            // Ensure sender name is only as wide as needed
                            let sender_text = egui::RichText::new(&message.sender)
                                .size(12.0)
                                .strong()
                                .color(egui::Color32::from_rgb(180, 180, 180));
                            ui.label(sender_text);
                        });
                        ui.add_space(1.0);
                    }
                    
                    // Fixed content width calculation - using a simpler approach
                    // Min width to ensure visibility even for short messages
                    let min_width = 100.0;
                    // Estimate width based on character count
                    let estimated_width = ((message.content.len() as f32 * 8.0) + 40.0).max(min_width).min(max_width);
                    
                    ui.allocate_ui_with_layout(
                        egui::vec2(estimated_width, 0.0),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            egui::Frame::new()
                                .fill(bubble_color)
                                .corner_radius(egui::CornerRadius {
                                    ne: bubble_radius,
                                    nw: bubble_radius,
                                    se: if message.is_self { 4 } else { bubble_radius },
                                    sw: if message.is_self { bubble_radius } else { 4 },
                                })
                                .inner_margin(egui::vec2(12.0, 8.0))
                                .show(ui, |ui: &mut egui::Ui| {
                                    // Message content
                                    ui.label(egui::RichText::new(&message.content).color(text_color).size(14.0));
                                    
                                    // Timestamp with right alignment
                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                                        ui.label(
                                            egui::RichText::new(message.timestamp.format("%H:%M").to_string())
                                                .size(10.0)
                                                .color(timestamp_color)
                                        );
                                    });
                                });
                        }
                    );
                });
                ui.add_space(6.0); // Space between messages
            }
            ui.add_space(12.0);
        });

    // Then, render the input area with simpler layout to ensure controls are visible
    ui.allocate_ui_at_rect(input_area_rect, |ui| {
        ui.horizontal_centered(|ui| {
            // Reserve fixed space for the button on the right
            let button_area_width = button_size + 16.0;
            
            // Define a color that matches the application background
            let input_common_color = egui::Color32::from_rgb(18, 20, 24); // Darker to match app background
            
            // Text input with modern styling - takes most of the width
            let input_width = ui.available_width() - button_area_width - 20.0;
            
            ui.add_space(10.0); // Left margin
            
            // Create a frame for the text edit field with modern styling
            egui::Frame::new()
                .fill(input_common_color)  // Use the common color variable
                .corner_radius(input_radius)
                .stroke(egui::Stroke::new(1.0, input_common_color)) // Use the same color for stroke
                .inner_margin(egui::vec2(12.0, 6.0))
                .show(ui, |ui| {
                    // Clean text edit with custom styling
                    ui.add_sized(
                        [input_width - 24.0, input_height - 12.0],
                        egui::TextEdit::singleline(message_input)
                            .hint_text("Type a message...")
                            .text_color(egui::Color32::from_rgb(240, 240, 240))
                            .margin(egui::vec2(4.0, 8.0))
                            .background_color(input_common_color) // Use the common color variable
                            .frame(false) // Disable the default frame/border completely
                    );
                });
            
            // Empty space between text input and button
            ui.add_space(8.0);
            
            // Modern send button with paper airplane icon
            let send_button = ui.add_sized(
                [button_size, button_size],
                egui::Button::new(
                    egui::RichText::new("✈").size(18.0).strong().color(egui::Color32::WHITE)
                )
                .fill(accent_color)
                .corner_radius(button_size / 2.0)
                .stroke(egui::Stroke::NONE)
            );
            
            // Check if enter was pressed
            let enter_pressed = ui.input(|i| i.key_pressed(egui::Key::Enter));
            
            if send_button.clicked() || enter_pressed {
                send_message(message_input, current_user_id, current_chat_peer_id, gui_to_daemon_tx, rt, messages);
            }
            
            ui.add_space(10.0); // Right margin
        });
    });
}

// Helper function to send a message
fn send_message(
    message_input: &mut String,
    current_user_id: &str,
    current_chat_peer_id: &Option<String>,
    gui_to_daemon_tx: &Option<mpsc::Sender<GuiToDaemonCommand>>,
    rt: &Arc<tokio::runtime::Runtime>,
    messages: &mut Vec<Message>
) {
    if !message_input.trim().is_empty() {
        if let Some(recipient_id) = current_chat_peer_id.as_ref() {
            if !recipient_id.is_empty() { // Ensure a peer is actually selected
                let content_to_send = message_input.trim().to_string();
                message_input.clear(); // Clear input field immediately

                let command = GuiToDaemonCommand::SendMessage {
                    recipient_id: recipient_id.clone(),
                    content: content_to_send.clone(), // Use cloned content
                };

                // Add to local messages immediately with is_self = true
                let new_message = Message {
                    id: uuid::Uuid::new_v4().to_string(), // Generate unique ID
                    sender: current_user_id.to_string(),    // Current user is the sender
                    recipient: recipient_id.clone(),
                    content: content_to_send.clone(), // Use cloned content
                    timestamp: chrono::Utc::now(),
                    is_self: true, // This message is from the current user
                };
                messages.push(new_message);
                println!("GUI: Locally added self-message to chat area. Content: {}", content_to_send);

                if let Some(tx) = gui_to_daemon_tx {
                    let tx_clone = tx.clone();
                    // let rt_clone = rt.clone(); // rt is Arc, cloning it like this is fine, or just use rt directly
                    rt.spawn(async move {
                        println!("GUI: Sending SendMessage command to daemon for content: {}", content_to_send);
                        if let Err(e) = tx_clone.send(command).await {
                            eprintln!("Failed to send SendMessage command: {}", e);
                            // Optionally, update the local message to indicate failure or provide a retry mechanism
                        }
                    });
                } else {
                    eprintln!("Error: gui_to_daemon_tx is None, cannot send message.");
                    // Message is already added locally, but won't be sent. Consider visual indication.
                }
            } else {
                println!("GUI: SendMessage attempted but no recipient_id (peer not selected or empty).");
            }
        } else {
            println!("GUI: SendMessage attempted but no peer selected (current_chat_peer_id is None).");
        }
    } else {
        message_input.clear(); // Also clear if it was just whitespace
    }
} 