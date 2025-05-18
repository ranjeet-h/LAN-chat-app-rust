use eframe::egui;
use crate::CurrentPanel;

// Modern top navigation UI
pub fn show(
    ui: &mut egui::Ui, 
    current_panel: &mut CurrentPanel,
    username: &Option<String>,
) {
    // Apply styling similar to chat_area.rs
    let accent_color = egui::Color32::from_rgb(242, 242, 247);  // Modern blue from chat_area
    let bg_color = egui::Color32::from_rgb(30, 30, 30);       // Keep current background
    let text_color = egui::Color32::from_rgb(240, 240, 240);  // Light text for contrast
    
    // Top bar background
    let top_rect = ui.max_rect();
    ui.painter().rect_filled(
        top_rect,
        0.0,
        bg_color
    );
    
    ui.horizontal(|ui| {
        // Logo and app name with accent color
        ui.add_space(10.0);
        ui.heading(
            egui::RichText::new("Local Network Chat")
                .color(accent_color)
                .size(20.0)
        );
        ui.add_space(20.0);
        
        // Navigation tabs with modern styling
        ui.separator();
        
        if ui.add(egui::SelectableLabel::new(
            *current_panel == CurrentPanel::Chat,
            egui::RichText::new("Chat").color(text_color).size(16.0)
        )).clicked() {
            *current_panel = CurrentPanel::Chat;
        }
        
        if ui.add(egui::SelectableLabel::new(
            *current_panel == CurrentPanel::History,
            egui::RichText::new("History").color(text_color).size(16.0)
        )).clicked() {
            *current_panel = CurrentPanel::History;
        }
        
        if ui.add(egui::SelectableLabel::new(
            *current_panel == CurrentPanel::Settings,
            egui::RichText::new("Settings").color(text_color).size(16.0)
        )).clicked() {
            *current_panel = CurrentPanel::Settings;
        }
        
        ui.separator();
        ui.add_space(10.0);
        
        // Right-aligned content - username display
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(20.0);
            if let Some(username) = username {
                ui.label(
                    egui::RichText::new(format!("👤 {}", username))
                        .color(text_color)
                        .size(16.0)
                );
            }
        });
    });
} 