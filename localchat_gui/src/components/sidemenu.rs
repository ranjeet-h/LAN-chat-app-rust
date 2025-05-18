use eframe::egui;
use crate::{IpcPeer, GuiToDaemonCommand}; // Assuming IpcPeer is in crate root (main.rs)
use tokio::sync::mpsc; // For Sender type
use std::sync::Arc;

// Modern side menu UI with updated peer list styling
pub fn show(
    ui: &mut egui::Ui, 
    peers: &[IpcPeer], 
    current_chat_peer_id: &mut Option<String>,
    gui_to_daemon_tx: &Option<mpsc::Sender<GuiToDaemonCommand>>,
    rt: &Arc<tokio::runtime::Runtime>,
    current_user_id: &Option<String> // Added parameter for current username
) {
    // Modern color scheme that works with dark background
    let accent_color = egui::Color32::from_rgb(25, 118, 210); // Primary blue
    let hover_color = egui::Color32::from_rgb(35, 35, 40);    // Slightly lighter than background for hover
    let label_color = egui::Color32::from_rgb(220, 220, 220); // Light gray for labels
    let subtle_color = egui::Color32::from_rgb(150, 150, 160); // Subtle gray for secondary text
    let divider_color = egui::Color32::from_rgb(50, 50, 55);   // Slightly lighter for dividers
    
    // Header section with updated styling
    ui.add_space(12.0);
    
    // Define header height and allocate space for it
    let header_height = 40.0; // Adjust as needed for desired header spacing
    let (header_rect, _response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), header_height),
        egui::Sense::hover(), // Can be egui::Sense::focusable_noninteractive() if no direct interaction with header background
    );

    // Create a new UI within the allocated header_rect for the header content
    ui.allocate_ui_at_rect(header_rect, |header_content_ui| {
        // Use a layout that arranges children left-to-right and centers them vertically within header_rect
        header_content_ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |centered_header_ui| {
            centered_header_ui.heading(
                egui::RichText::new("Peers")
                    .size(18.0)
                    .color(label_color)
                    .strong()
            );
            
            // Refresh button section, aligned to the right and vertically centered
            centered_header_ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |refresh_ui| {
                let refresh_btn = refresh_ui.add(
                    egui::Button::new(
                        egui::RichText::new("⟳")
                            .size(16.0)
                            .color(label_color)
                    )
                    .fill(accent_color.linear_multiply(0.8)) // Slightly darkened
                    .rounding(8.0)
                    .min_size(egui::vec2(32.0, 32.0))
                );
                
                if refresh_btn.clicked() {
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
                
                if refresh_btn.hovered() {
                    refresh_ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
            });
        });
    });
    
    // Add a subtle divider
    ui.add_space(8.0);
    ui.separator();
    ui.add_space(12.0);
    
    // Peers list section
    if peers.is_empty() {
        // Styled empty state
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(
                egui::RichText::new("No peers found")
                    .size(14.0)
                    .color(subtle_color)
                    .italics()
            );
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new("Try refreshing the list")
                    .size(12.0)
                    .color(subtle_color.linear_multiply(0.8))
            );
            ui.add_space(40.0);
        });
    } else {
        // Scrollable area with improved styling
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0; // Tighter spacing between items
                
                for peer in peers {
                    let is_selected = current_chat_peer_id.as_ref().map_or(false, |id| *id == peer.id);
                    
                    // Determine background fill based on selection and hover state
                    // The response for hover state will be captured after the frame is shown
                    // We will rely on response.hovered() later to conditionally set cursor icon
                    // and potentially add other hover-specific effects if needed, but background is tricky here.
                    // For now, let's keep the selection background logic and remove the manual hover paint.
                    // The hover background will be handled by a conditional fill in the Frame itself.

                    // We need to get the hover state *before* defining the frame,
                    // so we'll create an invisible button or sense hover on an area first.
                    // This is a common egui pattern. Let's define an interaction area.
                    
                    let item_response = ui.interact(ui.available_rect_before_wrap(), egui::Id::new(&peer.id).with("peer_item"), egui::Sense::click());
                    let is_hovered = item_response.hovered();

                    let background_fill = if is_selected {
                        accent_color.linear_multiply(0.4) // Stronger selected background
                    } else if is_hovered {
                        hover_color // Hover background
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let peer_frame = egui::Frame::none()
                        .fill(background_fill)
                        .rounding(8.0)
                        .inner_margin(egui::vec2(12.0, 10.0))
                        .outer_margin(egui::vec2(0.0, 2.0));
                    
                    // Use the item_response for click handling
                    let inner_response = peer_frame.show(ui, |ui| {
                        ui.horizontal(|h_ui| {
                            // Icon (non-selectable)
                            let icon_color = if is_selected {
                                accent_color.gamma_multiply(1.5) // Brighter icon for selected
                            } else if is_hovered {
                                subtle_color.gamma_multiply(1.2) // Slightly brighter icon on hover
                            } else {
                                subtle_color
                            };
                            let icon_font_id = egui::FontId::proportional(14.0);
                            let mut icon_job = egui::text::LayoutJob::default();
                            icon_job.append(
                                "👤",
                                0.0,
                                egui::TextFormat {
                                    font_id: icon_font_id,
                                    color: icon_color,
                                    ..Default::default()
                                },
                            );
                            let icon_galley = h_ui.fonts(|f| f.layout_job(icon_job));
                            let (icon_rect, _icon_response) = h_ui.allocate_exact_size(icon_galley.size(), egui::Sense::focusable_noninteractive());
                            h_ui.painter().galley(icon_rect.min, icon_galley, egui::Color32::WHITE);

                            h_ui.add_space(8.0);
                            
                            // Username (non-selectable)
                            let username_color = if is_selected {
                                label_color.gamma_multiply(1.2) // Brighter text for selected
                            } else {
                                label_color
                            };
                            // Respect .text_style(egui::TextStyle::Monospace) and .strong()
                            let username_font_id = egui::FontId::monospace(14.0);
                            let mut username_job = egui::text::LayoutJob::default();
                            username_job.append(
                                &peer.username,
                                0.0,
                                egui::TextFormat {
                                    color: username_color,
                                    ..Default::default()
                                },
                            );
                            let username_galley = h_ui.fonts(|f| f.layout_job(username_job));
                            let (username_rect, _username_response) = h_ui.allocate_exact_size(username_galley.size(), egui::Sense::focusable_noninteractive());
                            h_ui.painter().galley(username_rect.min, username_galley, egui::Color32::WHITE);
                         });
                     }).response; // This is the response of the frame's content
                    
                    // Handle click and hover effects using the item_response
                    if item_response.clicked() {
                        if is_selected {
                            *current_chat_peer_id = None; // Click again to deselect
                        } else {
                            *current_chat_peer_id = Some(peer.id.clone());
                            println!("GUI: Selected peer for chat: {} ({})", peer.username, peer.id);
                        }
                    }
                    
                    if item_response.hovered() {
                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                    }
                }
            });
    }
    
    // Bottom section with connection info or status
    ui.add_space(8.0);
    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);
        
        // Show current user info at bottom
        if let Some(username) = current_user_id {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                ui.label(
                    egui::RichText::new("Logged in as:")
                        .size(11.0)
                        .color(subtle_color)
                );
                ui.label(
                    egui::RichText::new(username)
                        .size(11.0)
                        .color(label_color)
                        .strong()
                );
            });
        }
    });
} 