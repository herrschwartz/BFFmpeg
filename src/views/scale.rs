use crate::controllers::scale::{
    CommonAspectRatio, ScaleController, ScaleFileScan, ScaleScanResult, ScaleScanState,
};
use crate::model::AppModel;
use eframe::egui::{self, RichText};
use std::time::Duration;

pub fn show(ui: &mut egui::Ui, model: &AppModel, controller: &mut ScaleController) {
    controller.poll_scan();

    ui.heading("Scale");
    let response = ui.checkbox(
        &mut controller.retain_current_resolution,
        "Retain Current Resolution",
    );
    if response.changed() {
        controller.update_retain_current_resolution(model);
    }

    if controller.retain_current_resolution {
        ui.label(RichText::new("Video dimensions will be copied without scaling.").weak());
        return;
    }

    ui.add_space(12.0);
    ui.group(|ui| {
        ui.label(RichText::new("Scale by percentage").strong());
        ui.horizontal(|ui| {
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = 360.0;
                ui.add(egui::Slider::new(&mut controller.scale_percentage, 10..=200).suffix(" %"));
            });
            ui.add(
                egui::DragValue::new(&mut controller.scale_percentage)
                    .range(10..=200)
                    .suffix(" %"),
            );
        });
        if controller.selected_resolution().is_none() {
            ui.label(
                RichText::new("This scale setting is active in the FFmpeg command above.").weak(),
            );
        }
    });

    ui.add_space(12.0);
    match controller.scan_state().clone() {
        ScaleScanState::Idle => {}
        ScaleScanState::Scanning { scanned, total } => {
            ui.label(format!(
                "Scanning video dimensions: {scanned} of {total} files…"
            ));
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        ScaleScanState::Complete(result) => show_scan_result(ui, controller, result),
    }
}

fn show_scan_result(ui: &mut egui::Ui, controller: &mut ScaleController, result: ScaleScanResult) {
    if result.total_files == 0 {
        ui.label("No video files are available to scan.");
        return;
    }

    if let Some(aspect_ratio) = result.common_aspect_ratio {
        show_common_aspect_ratio(ui, controller, &aspect_ratio);
    } else {
        ui.group(|ui| {
            ui.heading("No common aspect ratio");
            ui.label(
                RichText::new(
                    "The source files do not share one display aspect ratio within a 1% margin.",
                )
                .weak(),
            );
            show_scanned_dimensions(ui, &result.files);
        });
    }
}

fn show_common_aspect_ratio(
    ui: &mut egui::Ui,
    controller: &mut ScaleController,
    aspect_ratio: &CommonAspectRatio,
) {
    ui.group(|ui| {
        ui.heading(format!("Common aspect ratio: {}", aspect_ratio.label));
        ui.label(
            RichText::new("Choose a standard output resolution that preserves this ratio.").weak(),
        );
        ui.add_space(4.0);

        egui::ComboBox::from_label("Scale to")
            .selected_text(
                controller
                    .selected_resolution()
                    .map(|resolution| resolution.label())
                    .unwrap_or_else(|| "Percentage scale".to_owned()),
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    controller.selected_resolution_mut(),
                    None,
                    "Percentage scale",
                );
                for resolution in aspect_ratio.resolution_presets() {
                    ui.selectable_value(
                        controller.selected_resolution_mut(),
                        Some(resolution),
                        resolution.label(),
                    );
                }
            });
    });
}

fn show_scanned_dimensions(ui: &mut egui::Ui, files: &[ScaleFileScan]) {
    for file in files {
        if let Some(error) = &file.error {
            ui.colored_label(
                egui::Color32::LIGHT_RED,
                format!("{}: {error}", file.file_name),
            );
        } else if let Some(dimensions) = file.dimensions {
            ui.label(format!(
                "{}: {} × {} ({:.3}:1)",
                file.file_name,
                dimensions.width,
                dimensions.height,
                dimensions.display_aspect_ratio
            ));
        }
    }
}
