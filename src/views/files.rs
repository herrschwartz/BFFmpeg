use crate::controllers::files::FilesController;
use crate::model::{AppModel, OutputContainer, VideoBitrate};
use eframe::egui::{self, RichText};
use std::time::Duration;

pub fn show(ui: &mut egui::Ui, model: &mut AppModel, controller: &mut FilesController) {
    controller.poll_media_info(model);

    ui.heading("Files");
    ui.label(
        RichText::new(format!(
            "Current folder: {}",
            model.current_folder.display()
        ))
        .weak(),
    );
    ui.add_space(12.0);

    ui.horizontal(|ui| {
        ui.label("Output folder:");
        ui.add(
            egui::TextEdit::singleline(&mut model.output_directory)
                .desired_width(f32::INFINITY)
                .hint_text(model.current_folder.join("out").display().to_string()),
        );
    });

    egui::ComboBox::from_label("Output container")
        .selected_text(model.output_container.label())
        .show_ui(ui, |ui| {
            for container in OutputContainer::ALL {
                ui.selectable_value(&mut model.output_container, container, container.label());
            }
        });

    ui.add_space(16.0);
    ui.horizontal(|ui| {
        ui.heading("Video files");
        ui.label(RichText::new(format!("({})", model.video_files.len())).weak());
    });

    if let Some(error) = &model.folder_scan_error {
        ui.colored_label(egui::Color32::LIGHT_RED, error);
        return;
    }

    if model.video_files.is_empty() {
        ui.label("There are no supported video files in the current folder.");
        return;
    }

    let mut newly_selected = None;
    egui::ScrollArea::vertical()
        .id_salt("video_files")
        .max_height(220.0)
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("video_files_grid")
                .striped(true)
                .min_col_width(120.0)
                .show(ui, |ui| {
                    ui.strong("File");
                    ui.strong("Size");
                    ui.end_row();

                    for (index, file) in model.video_files.iter().enumerate() {
                        if ui
                            .selectable_label(model.selected_video_index == Some(index), &file.name)
                            .clicked()
                        {
                            newly_selected = Some(index);
                        }
                        ui.label(format_file_size(file.size_bytes));
                        ui.end_row();
                    }
                });
        });

    if let Some(index) = newly_selected {
        controller.select_video_file(model, index);
    }

    if model.media_info_loading {
        ui.ctx().request_repaint_after(Duration::from_millis(50));
    }

    show_selected_file_details(ui, model);
}

fn show_selected_file_details(ui: &mut egui::Ui, model: &AppModel) {
    let Some(index) = model.selected_video_index else {
        return;
    };
    let Some(file) = model.video_files.get(index) else {
        return;
    };

    ui.add_space(16.0);
    ui.group(|ui| {
        ui.heading("Selected file");
        ui.label(RichText::new(&file.name).strong());

        if let Some(error) = &model.media_info_error {
            ui.colored_label(egui::Color32::LIGHT_RED, error);
            return;
        }

        if model.media_info_loading {
            ui.label("Reading media information…");
            return;
        }

        let Some(info) = &model.selected_media_info else {
            ui.label("Media information is unavailable.");
            return;
        };

        info_row(ui, "Duration", info.duration_seconds.map(format_duration));
        info_row(ui, "Video codec", info.video_codec.clone());
        info_row(ui, "Dynamic range", info.dynamic_range.clone());
        info_row(
            ui,
            "Video bitrate",
            info.video_bitrate.as_ref().map(format_video_bitrate),
        );
        ui.add_space(6.0);
        ui.strong("Audio streams");

        if info.audio_streams.is_empty() {
            ui.label("No audio streams found.");
        } else {
            for stream in &info.audio_streams {
                let bitrate = stream
                    .bitrate
                    .map(format_bitrate)
                    .unwrap_or_else(|| "bitrate unavailable".to_owned());
                let channels = stream
                    .channels
                    .map(|channels| format!("{channels} channels"))
                    .unwrap_or_else(|| "channels unavailable".to_owned());
                ui.label(format!(
                    "#{}{}  {}  ({bitrate}, {channels})",
                    stream.index,
                    stream
                        .language
                        .as_deref()
                        .map(|language| format!(" ({language})"))
                        .unwrap_or_default(),
                    stream.codec,
                ));
            }
        }
    });
}

fn info_row(ui: &mut egui::Ui, label: &str, value: Option<String>) {
    ui.horizontal(|ui| {
        ui.strong(label);
        ui.label(value.unwrap_or_else(|| "Unavailable".to_owned()));
    });
}

fn format_duration(seconds: f64) -> String {
    let total_seconds = seconds.round().max(0.0) as u64;
    let hours = total_seconds / 3_600;
    let minutes = (total_seconds % 3_600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{hours}:{minutes:02}:{seconds:02}")
    } else {
        format!("{minutes}:{seconds:02}")
    }
}

fn format_bitrate(bits_per_second: u64) -> String {
    if bits_per_second >= 1_000_000 {
        format!("{:.2} Mbps", bits_per_second as f64 / 1_000_000.0)
    } else {
        format!("{:.0} kbps", bits_per_second as f64 / 1_000.0)
    }
}

fn format_video_bitrate(bitrate: &VideoBitrate) -> String {
    let bitrate_label = format_bitrate(bitrate.bits_per_second);
    if bitrate.is_estimated {
        format!("{bitrate_label} (estimated from container bitrate)")
    } else {
        bitrate_label
    }
}

fn format_file_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;

    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }

    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
