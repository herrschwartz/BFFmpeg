use crate::controllers::audio::{AudioCodec, AudioController, AudioScanResult, AudioScanState};
use crate::model::{AppModel, AudioStreamInfo};
use eframe::egui::{self, RichText};
use std::time::Duration;

pub fn show(ui: &mut egui::Ui, model: &AppModel, controller: &mut AudioController) {
    controller.poll_scan();

    ui.heading("Audio");
    let response = ui.checkbox(
        &mut controller.passthrough_all_audio,
        "Passthrough all Audio",
    );
    if response.changed() {
        controller.update_passthrough(model);
    }

    if controller.passthrough_all_audio {
        ui.label(
            RichText::new("All source audio streams will be copied without re-encoding.").weak(),
        );
        return;
    }

    ui.add_space(12.0);
    let scan_state = controller.scan_state().clone();
    match scan_state {
        AudioScanState::Idle => {}
        AudioScanState::Scanning { scanned, total } => {
            ui.label(format!(
                "Scanning audio streams: {scanned} of {total} files…"
            ));
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        AudioScanState::Complete(result) => show_scan_result(ui, controller, result),
    }
}

fn show_scan_result(ui: &mut egui::Ui, controller: &mut AudioController, result: AudioScanResult) {
    if result.total_files == 0 {
        ui.label("No video files are available to scan.");
    } else if result.mismatches.is_empty() {
        ui.label(format!(
            "All {} video files have matching audio streams.",
            result.total_files
        ));
        ui.add_space(8.0);

        for (index, stream) in result.reference_streams.iter().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    if let Some(settings) = controller.track_settings_mut(index) {
                        ui.checkbox(&mut settings.selected, format!("Track {}", index + 1));
                    }
                    ui.label(stream_identifier(stream));
                });
                ui.label(format_track(stream));

                if let Some(settings) = controller.track_settings_mut(index) {
                    ui.add_enabled_ui(settings.selected, |ui| {
                        ui.horizontal(|ui| {
                            ui.label("Codec:");
                            let previous_codec = settings.codec;
                            egui::ComboBox::from_id_salt(("audio_track_codec", index))
                                .selected_text(settings.codec.label())
                                .show_ui(ui, |ui| {
                                    for candidate in AudioCodec::ALL {
                                        ui.selectable_value(
                                            &mut settings.codec,
                                            candidate,
                                            candidate.label(),
                                        );
                                    }
                                });

                            if settings.codec != previous_codec {
                                settings.bitrate_kbps = settings.codec.default_bitrate();
                            }

                            if settings.codec.uses_bitrate() {
                                ui.label("Bitrate:");
                                egui::ComboBox::from_id_salt(("audio_track_bitrate", index))
                                    .selected_text(format!("{} kbps", settings.bitrate_kbps))
                                    .show_ui(ui, |ui| {
                                        for &bitrate in settings.codec.bitrate_options() {
                                            ui.selectable_value(
                                                &mut settings.bitrate_kbps,
                                                bitrate,
                                                format!("{bitrate} kbps"),
                                            );
                                        }
                                    });
                            }
                        });
                    });
                }
            });
            ui.add_space(6.0);
        }
    } else {
        ui.colored_label(
            egui::Color32::LIGHT_RED,
            "Audio mismatch — this batch cannot be re-encoded together until the audio tracks are reconciled.",
        );
        ui.label(format!(
            "Reference streams: {}",
            stream_list(&result.reference_streams)
        ));

        for mismatch in &result.mismatches {
            if let Some(error) = &mismatch.error {
                ui.label(format!("{}: {error}", mismatch.file_name));
            } else {
                ui.label(format!(
                    "{}: {}",
                    mismatch.file_name,
                    stream_list(&mismatch.streams)
                ));
            }
        }
    }
}

fn format_track(stream: &AudioStreamInfo) -> String {
    let bitrate = stream
        .bitrate
        .map(format_bitrate)
        .unwrap_or_else(|| "bitrate unavailable".to_owned());
    let channels = stream
        .channels
        .map(|channels| format!("{channels} channels"))
        .unwrap_or_else(|| "channels unavailable".to_owned());
    format!("{} • {bitrate} • {channels}", stream.codec)
}

fn stream_identifier(stream: &AudioStreamInfo) -> String {
    let language = stream
        .language
        .as_deref()
        .map(|language| format!(" ({language})"))
        .unwrap_or_default();
    format!("Stream #0:{}{language}", stream.index)
}

fn stream_list(streams: &[AudioStreamInfo]) -> String {
    if streams.is_empty() {
        "no audio streams".to_owned()
    } else {
        streams
            .iter()
            .map(|stream| format!("{} {}", stream_identifier(stream), stream.codec))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_bitrate(bits_per_second: u64) -> String {
    if bits_per_second >= 1_000_000 {
        format!("{:.2} Mbps", bits_per_second as f64 / 1_000_000.0)
    } else {
        format!("{:.0} kbps", bits_per_second as f64 / 1_000.0)
    }
}
