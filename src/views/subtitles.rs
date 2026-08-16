use crate::controllers::subtitles::{
    SubtitleFileScan, SubtitleScanResult, SubtitleScanState, SubtitlesController,
};
use crate::model::{AppModel, SubtitleStreamInfo};
use eframe::egui::{self, RichText};
use std::time::Duration;

pub fn show(ui: &mut egui::Ui, model: &AppModel, controller: &mut SubtitlesController) {
    controller.poll_scan();

    ui.heading("Subtitles");
    let response = ui.checkbox(
        &mut controller.passthrough_all_subtitles,
        "Passthrough all Subtitles",
    );
    if response.changed() {
        controller.update_passthrough(model);
    }

    if controller.passthrough_all_subtitles {
        ui.label(
            RichText::new("All source subtitle streams will be copied without filtering.").weak(),
        );
        return;
    }

    ui.add_space(12.0);
    match controller.scan_state().clone() {
        SubtitleScanState::Idle => {}
        SubtitleScanState::Scanning { scanned, total } => {
            ui.label(format!(
                "Scanning subtitle streams: {scanned} of {total} files…"
            ));
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        SubtitleScanState::Complete(result) => show_scan_result(ui, controller, result),
    }
}

fn show_scan_result(
    ui: &mut egui::Ui,
    controller: &mut SubtitlesController,
    result: SubtitleScanResult,
) {
    if result.total_files == 0 {
        ui.label("No video files are available to scan.");
        return;
    }

    ui.label(format!(
        "Scanned subtitle streams from all {} video files.",
        result.total_files
    ));
    ui.add_space(8.0);

    ui.group(|ui| {
        ui.heading("Subtitle tracks in common");
        if result.common_tracks.is_empty() {
            ui.label(RichText::new("No subtitle tracks are shared by every file.").weak());
        } else {
            ui.label(RichText::new("Select the tracks to keep in the batch output.").weak());
            ui.add_space(4.0);
            for (index, stream) in result.common_tracks.iter().enumerate() {
                if let Some(selected) = controller.common_track_selected_mut(index) {
                    ui.checkbox(selected, format_subtitle_track(stream));
                }
            }
        }
    });

    ui.add_space(12.0);
    ui.group(|ui| {
        ui.heading("Subtitle tracks not common to every file");
        ui.label(RichText::new("These tracks cannot be selected for this batch.").weak());

        let mut has_uncommon_tracks = false;
        for file in &result.uncommon_files {
            if file.error.is_some() || !file.uncommon_tracks.is_empty() {
                has_uncommon_tracks = true;
                show_file_tracks(ui, file);
            }
        }

        if !has_uncommon_tracks {
            ui.label(RichText::new("None.").weak());
        }
    });
}

fn show_file_tracks(ui: &mut egui::Ui, file: &SubtitleFileScan) {
    if let Some(error) = &file.error {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            format!("{}: {error}", file.file_name),
        );
        return;
    }

    ui.strong(&file.file_name);
    for track in &file.uncommon_tracks {
        ui.label(format!("  {}", format_subtitle_track(track)));
    }
}

fn format_subtitle_track(stream: &SubtitleStreamInfo) -> String {
    let language = stream.language.as_deref().unwrap_or("und");
    let title = stream
        .title
        .as_deref()
        .map(|title| format!(" — {title}"))
        .unwrap_or_default();
    format!(
        "Stream #0:{} ({language}) • {}{title}",
        stream.index, stream.codec
    )
}
