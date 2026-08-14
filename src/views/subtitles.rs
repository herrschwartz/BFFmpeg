use crate::controllers::subtitles::SubtitlesController;
use eframe::egui;

pub fn show(ui: &mut egui::Ui, _controller: &mut SubtitlesController) {
    ui.group(|ui| {
        ui.heading("Subtitles");
        ui.label("Subtitle stream controls will be added here.");
    });
}
