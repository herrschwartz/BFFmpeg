use crate::controllers::video::VideoController;
use eframe::egui::{self, RichText};

pub fn show(ui: &mut egui::Ui, controller: &mut VideoController) {
    ui.heading("Video");

    let Some(settings) = controller.settings_mut() else {
        ui.label("The selected preset does not use a supported H.264 or H.265 encoder yet.");
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

        if settings.supports_tunes() {
            ui.add_space(12.0);
            let tune_options = settings.tune_options();
            ui.horizontal(|ui| {
                ui.label("Encoder tune:");
                egui::ComboBox::from_id_salt("video_encoder_tune")
                    .selected_text(settings.tune.label())
                    .show_ui(ui, |ui| {
                        for &tune in tune_options {
                            ui.selectable_value(&mut settings.tune, tune, tune.label());
                        }
                    });
            });
            ui.label(
                RichText::new("Tunes optimize the encoder for a type of source or delivery goal.")
                    .weak(),
            );
        }
    });
}
