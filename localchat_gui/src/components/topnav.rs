use eframe::egui;

// Placeholder for top navigation UI
pub fn show(ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        ui.heading("Local Network Chat");
        ui.separator();
        // Potentially add buttons for history, settings etc. here or manage active panel state
        if ui.button("History").clicked() {
            // TODO: logic to show history panel
        }
        if ui.button("Settings").clicked() {
            // TODO: logic to show settings panel
        }
    });
} 