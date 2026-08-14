use crate::controllers::audio::AudioController;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, _controller: &mut AudioController) {
    ui.group(|ui| {
        ui.heading("Audio");
        ui.label("Audio stream controls will be added here.");
    });
}
