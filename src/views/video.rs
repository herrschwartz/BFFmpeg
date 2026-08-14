use crate::controllers::video::VideoController;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, _controller: &mut VideoController) {
    ui.group(|ui| {
        ui.heading("Video");
        ui.label("Video encoding controls will be added here.");
    });
}
