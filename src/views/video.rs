use crate::controllers::video::VideoController;
use eframe::egui::{self, RichText};

pub fn show(ui: &mut egui::Ui, controller: &mut VideoController) {
    ui.heading("Video");

    let Some(settings) = controller.h265_settings_mut() else {
        ui.label("The selected preset does not use a supported H.265 encoder yet.");
        return;
    };

    ui.group(|ui| {
        ui.label(RichText::new(settings.encoder_label()).strong());
        ui.add_space(8.0);

        let quality_label = settings.quality_label();
        let quality_range = settings.quality_range();
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = 360.0;
            ui.add(
                egui::Slider::new(&mut settings.quality, quality_range)
                    .text(quality_label)
                    .suffix(" / 51"),
            );
        });
        ui.label(
            RichText::new("Lower values target higher quality and usually create larger files.")
                .weak(),
        );

        ui.add_space(12.0);
        let speed_range = settings.speed_range();
        ui.scope(|ui| {
            ui.spacing_mut().slider_width = 240.0;
            ui.add(
                egui::Slider::new(&mut settings.speed, speed_range)
                    .text("Encoding speed")
                    .show_value(false),
            );
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Faster").weak());
            ui.label(RichText::new(settings.speed_label()).strong());
            ui.label(RichText::new("Slower / higher quality").weak());
        });
    });
}
