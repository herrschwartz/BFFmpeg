use crate::controller::{AppController, parse_advanced_ffmpeg_args};
use crate::model::Profile;
use crate::views;
use eframe::egui::{self, Color32, RichText};
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Files,
    Video,
    Scale,
    Audio,
    Subtitles,
}

pub struct EncoderApp {
    controller: AppController,
    active_tab: SettingsTab,
    advanced_parameters: Vec<AdvancedParameter>,
    advanced_parameter_input: String,
    advanced_parameter_error: Option<String>,
    show_advanced_parameter_dialog: bool,
    new_preset_name: String,
    preset_dialog_error: Option<String>,
    show_save_preset_dialog: bool,
    pending_delete_preset: Option<String>,
}

struct AdvancedParameter {
    display: String,
    arguments: Vec<String>,
}

#[derive(Default)]
struct CommandActions {
    add_advanced_parameter: bool,
    start_encode: bool,
}

impl EncoderApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        creation_context.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            controller: AppController::new(),
            active_tab: SettingsTab::Files,
            advanced_parameters: Vec::new(),
            advanced_parameter_input: String::new(),
            advanced_parameter_error: None,
            show_advanced_parameter_dialog: false,
            new_preset_name: String::new(),
            preset_dialog_error: None,
            show_save_preset_dialog: false,
            pending_delete_preset: None,
        }
    }

    fn preset_arguments_to_save(&self) -> Result<Vec<String>, String> {
        let profile = self
            .controller
            .selected_profile()
            .ok_or_else(|| "No preset is selected.".to_owned())?;
        let mut arguments = self.controller.video.effective_ffmpeg_args(profile);
        for parameter in &self.advanced_parameters {
            arguments.extend(parameter.arguments.iter().cloned());
        }
        Ok(arguments)
    }

    fn current_command_arguments(&self) -> Result<Vec<String>, String> {
        let profile = self
            .controller
            .selected_profile()
            .ok_or_else(|| "No preset is selected.".to_owned())?;
        Ok(effective_command_arguments(
            profile,
            &self.controller.video,
            &self.controller.scale,
            &self.controller.audio,
            &self.controller.subtitles,
            &self.advanced_parameters,
        ))
    }

    fn output_settings(&self) -> Result<(PathBuf, crate::model::OutputContainer), String> {
        let model = self
            .controller
            .model
            .as_ref()
            .ok_or_else(|| "No folder is open.".to_owned())?;
        let output_directory = model.output_directory.trim();
        if output_directory.is_empty() {
            return Err("Enter an output folder before encoding.".to_owned());
        }
        Ok((PathBuf::from(output_directory), model.output_container))
    }

    fn start_preview_encode(&mut self, request: crate::views::files::PreviewEncodeRequest) {
        let result = self.current_command_arguments().and_then(|arguments| {
            let (output_directory, container) = self.output_settings()?;
            self.controller.encoding.start_preview(
                request.input_path,
                request.file_name,
                request.duration_seconds,
                output_directory,
                container,
                arguments,
            )
        });
        self.controller.status_message = match result {
            Ok(()) => "Starting 30-second preview encode…".to_owned(),
            Err(error) => error,
        };
    }

    fn start_batch_encode(&mut self) {
        let result = self.current_command_arguments().and_then(|arguments| {
            let (output_directory, container) = self.output_settings()?;
            let video_files = self
                .controller
                .model
                .as_ref()
                .ok_or_else(|| "No folder is open.".to_owned())?
                .video_files
                .clone();
            self.controller.encoding.start_batch(
                video_files,
                output_directory,
                container,
                arguments,
            )
        });
        self.controller.status_message = match result {
            Ok(()) => "Starting batch encode…".to_owned(),
            Err(error) => error,
        };
    }

    fn show_encoding_overlay(&self, context: &egui::Context) {
        let Some(progress) = self.controller.encoding.progress() else {
            return;
        };

        let screen = context.content_rect();
        egui::Area::new(egui::Id::new("encoding_progress_overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(screen.min)
            .show(context, |ui| {
                ui.set_min_size(screen.size());
                let rect = ui.max_rect();
                ui.painter()
                    .rect_filled(rect, 0.0, Color32::from_black_alpha(180));
                ui.interact(
                    rect,
                    egui::Id::new("encoding_progress_overlay_blocker"),
                    egui::Sense::click(),
                );

                ui.add_space(screen.height() * 0.30);
                ui.vertical_centered(|ui| {
                    egui::Frame::window(ui.style()).show(ui, |ui| {
                        ui.set_width(480.0);
                        ui.heading(progress.kind.label());
                        ui.label(format!(
                            "File {} of {}: {}",
                            progress.current_file_index + 1,
                            progress.total_files,
                            progress.current_file_name
                        ));
                        ui.add_space(8.0);
                        ui.label("Current file");
                        ui.add(
                            egui::ProgressBar::new(progress.file_progress)
                                .show_percentage()
                                .desired_width(450.0),
                        );
                        if let Some(duration_seconds) = progress.duration_seconds {
                            ui.label(
                                RichText::new(format!(
                                    "{} / {} encoded",
                                    format_media_time(progress.encoded_seconds),
                                    format_media_time(duration_seconds)
                                ))
                                .weak(),
                            );
                        } else {
                            ui.label(RichText::new("Reading encoded position…").weak());
                        }
                        ui.label(
                            RichText::new(format!(
                                "BFFmpeg elapsed: {}",
                                format_media_time(progress.file_started_at.elapsed().as_secs_f64())
                            ))
                            .weak(),
                        );
                        ui.add_space(8.0);
                        ui.label("Overall batch progress");
                        ui.add(
                            egui::ProgressBar::new(progress.overall_progress)
                                .show_percentage()
                                .desired_width(450.0),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(
                                "Encoding is running. Controls are temporarily disabled.",
                            )
                            .weak(),
                        );
                    });
                });
            });
    }

    fn show_advanced_parameter_dialog(&mut self, context: &egui::Context) {
        if !self.show_advanced_parameter_dialog {
            return;
        }

        let mut open = true;
        let mut add_parameter = false;
        let mut cancel = false;
        egui::Window::new("Add advanced FFmpeg parameter")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(context, |ui| {
                ui.label("Enter one or more output parameters to append to the command.");
                ui.add(
                    egui::TextEdit::singleline(&mut self.advanced_parameter_input)
                        .desired_width(420.0),
                );
                if let Some(error) = &self.advanced_parameter_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Add parameter").clicked() {
                        add_parameter = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if add_parameter {
            match parse_advanced_ffmpeg_args(&self.advanced_parameter_input) {
                Ok(arguments) => {
                    self.advanced_parameters.push(AdvancedParameter {
                        display: self.advanced_parameter_input.trim().to_owned(),
                        arguments,
                    });
                    self.advanced_parameter_input.clear();
                    self.advanced_parameter_error = None;
                    cancel = true;
                }
                Err(error) => self.advanced_parameter_error = Some(error),
            }
        }
        self.show_advanced_parameter_dialog = open && !cancel;
    }

    fn show_save_preset_dialog(&mut self, context: &egui::Context) {
        if !self.show_save_preset_dialog {
            return;
        }

        let mut open = true;
        let mut save = false;
        let mut cancel = false;
        egui::Window::new("Save new preset")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(context, |ui| {
                ui.label(
                    "Save the current video settings and advanced parameters as a new preset.",
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.new_preset_name)
                        .desired_width(320.0)
                        .hint_text("Preset name"),
                );
                if let Some(error) = &self.preset_dialog_error {
                    ui.colored_label(Color32::LIGHT_RED, error);
                }
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Save preset").clicked() {
                        save = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if save {
            match self.preset_arguments_to_save().and_then(|arguments| {
                self.controller
                    .save_new_preset(&self.new_preset_name, arguments)
            }) {
                Ok(()) => {
                    self.advanced_parameters.clear();
                    self.preset_dialog_error = None;
                    cancel = true;
                }
                Err(error) => self.preset_dialog_error = Some(error),
            }
        }
        self.show_save_preset_dialog = open && !cancel;
    }

    fn show_delete_preset_dialog(&mut self, context: &egui::Context) {
        let Some(preset_name) = self.pending_delete_preset.clone() else {
            return;
        };

        let mut open = true;
        let mut delete = false;
        let mut cancel = false;
        egui::Window::new("Delete preset?")
            .collapsible(false)
            .resizable(false)
            .open(&mut open)
            .show(context, |ui| {
                ui.label(format!("Delete the preset \"{preset_name}\"?"));
                ui.colored_label(Color32::LIGHT_RED, "This cannot be undone.");
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    if ui.button("Delete preset").clicked() {
                        delete = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });

        if delete {
            match self.controller.delete_selected_preset() {
                Ok(()) => cancel = true,
                Err(error) => self.controller.status_message = error,
            }
        }
        if !open || cancel {
            self.pending_delete_preset = None;
        }
    }
}

impl eframe::App for EncoderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(status_message) = self.controller.encoding.poll() {
            self.controller.status_message = status_message;
        }
        let encoding_is_running = self.controller.encoding.is_running();
        if encoding_is_running {
            ui.ctx().request_repaint();
        }

        egui::Panel::top("app_header").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("BFFmpeg");
                ui.label(RichText::new("Batch encoding workspace").weak());
                ui.add_space(20.0);
                let mut browse_for_folder = false;
                let mut select_video_file = false;
                ui.menu_button("File", |ui| {
                    if ui.button("Open folder...").clicked() {
                        browse_for_folder = true;
                    }
                    if ui.button("Select file...").clicked() {
                        select_video_file = true;
                        ui.close();
                    }
                });
                if browse_for_folder {
                    self.controller.browse_for_folder();
                }
                if select_video_file {
                    self.controller.browse_for_video_file();
                    self.active_tab = SettingsTab::Files;
                }
                ui.separator();
                tab_button(ui, &mut self.active_tab, SettingsTab::Files, "Files");
                tab_button(ui, &mut self.active_tab, SettingsTab::Video, "Video");
                tab_button(ui, &mut self.active_tab, SettingsTab::Scale, "Scale");
                tab_button(ui, &mut self.active_tab, SettingsTab::Audio, "Audio");
                tab_button(
                    ui,
                    &mut self.active_tab,
                    SettingsTab::Subtitles,
                    "Subtitles",
                );
            });
        });

        egui::Panel::bottom("status_bar").show(ui, |ui| {
            ui.label(RichText::new(&self.controller.status_message).weak());
        });

        let profile_names = self
            .controller
            .model
            .as_ref()
            .map(|model| model.config.profiles.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default();
        let selected_profile = self
            .controller
            .model
            .as_ref()
            .map(|model| model.selected_profile.clone())
            .unwrap_or_default();
        let can_start_batch = self
            .controller
            .model
            .as_ref()
            .is_some_and(|model| !model.video_files.is_empty());

        let mut open_save_preset_dialog = false;
        let mut open_delete_preset_dialog = false;
        egui::Panel::left("profile_list")
            .resizable(true)
            .default_size(255.0)
            .show(ui, |ui| {
                ui.heading("Presets");
                ui.label(RichText::new("Loaded from config.json").weak());
                ui.separator();

                if profile_names.is_empty() {
                    ui.label("No profiles available.");
                }

                for name in &profile_names {
                    if ui
                        .selectable_label(name == &selected_profile, name)
                        .clicked()
                    {
                        self.controller.select_profile(name.clone());
                    }
                }

                ui.with_layout(egui::Layout::bottom_up(egui::Align::Min), |ui| {
                    ui.separator();
                    ui.horizontal_wrapped(|ui| {
                        if ui.button("Save new preset").clicked() {
                            open_save_preset_dialog = true;
                        }
                        if ui
                            .add_enabled(
                                profile_names.len() > 1,
                                egui::Button::new(
                                    RichText::new("Delete preset").color(Color32::WHITE),
                                )
                                .fill(Color32::from_rgb(142, 45, 45)),
                            )
                            .clicked()
                        {
                            open_delete_preset_dialog = true;
                        }
                    });
                });
            });

        if open_save_preset_dialog {
            self.new_preset_name = format!("{selected_profile} custom");
            self.preset_dialog_error = None;
            self.show_save_preset_dialog = true;
        }
        if open_delete_preset_dialog {
            self.pending_delete_preset = Some(selected_profile.clone());
        }

        let mut preview_encode_request = None;
        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            let selected_profile_data = self
                .controller
                .selected_profile()
                .cloned()
                .map(|profile| (selected_profile.clone(), profile));

            if let Some((name, profile)) = selected_profile_data {
                let command_actions = profile_details(
                    ui,
                    &name,
                    &profile,
                    &self.controller.video,
                    &self.controller.scale,
                    &self.controller.audio,
                    &self.controller.subtitles,
                    &mut self.advanced_parameters,
                    can_start_batch,
                    encoding_is_running,
                );
                if command_actions.add_advanced_parameter {
                    self.advanced_parameter_input.clear();
                    self.advanced_parameter_error = None;
                    self.show_advanced_parameter_dialog = true;
                }
                if command_actions.start_encode {
                    self.start_batch_encode();
                }
                ui.add_space(16.0);
            }

            let tab_content_height = ui.available_height();
            egui::Frame::NONE.fill(ui.visuals().window_fill).show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("settings_tab_content")
                    .max_height(tab_content_height)
                    .auto_shrink([false, false])
                    .show_viewport(ui, |ui, _viewport| {
                        let controller = &mut self.controller;
                        let crate::controller::AppController {
                            model,
                            files,
                            video,
                            scale,
                            audio,
                            subtitles,
                            ..
                        } = controller;

                        if let Some(model) = model {
                            match self.active_tab {
                                SettingsTab::Files => {
                                    preview_encode_request = views::files::show(ui, model, files)
                                }
                                SettingsTab::Video => views::video::show(ui, video),
                                SettingsTab::Scale => views::scale::show(ui, model, scale),
                                SettingsTab::Audio => views::audio::show(ui, model, audio),
                                SettingsTab::Subtitles => {
                                    views::subtitles::show(ui, model, subtitles)
                                }
                            }
                        } else {
                            ui.colored_label(
                                Color32::LIGHT_RED,
                                "The configuration could not be loaded. Add or repair config.json, then restart the application.",
                            );
                        }
                    });
            });
        });

        self.show_advanced_parameter_dialog(ui.ctx());
        self.show_save_preset_dialog(ui.ctx());
        self.show_delete_preset_dialog(ui.ctx());
        if let Some(request) = preview_encode_request {
            self.start_preview_encode(request);
        }
        self.show_encoding_overlay(ui.ctx());
    }
}

fn tab_button(ui: &mut egui::Ui, active_tab: &mut SettingsTab, tab: SettingsTab, label: &str) {
    if ui.selectable_label(*active_tab == tab, label).clicked() {
        *active_tab = tab;
    }
}

fn profile_details(
    ui: &mut egui::Ui,
    name: &str,
    profile: &Profile,
    video_controller: &crate::controllers::video::VideoController,
    scale_controller: &crate::controllers::scale::ScaleController,
    audio_controller: &crate::controllers::audio::AudioController,
    subtitles_controller: &crate::controllers::subtitles::SubtitlesController,
    advanced_parameters: &mut Vec<AdvancedParameter>,
    can_start_batch: bool,
    encoding_is_running: bool,
) -> CommandActions {
    let mut actions = CommandActions::default();
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.heading("FFmpeg command");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(
                        can_start_batch && !encoding_is_running,
                        egui::Button::new(
                            RichText::new("Start Encode").color(Color32::from_rgb(20, 55, 20)),
                        )
                        .fill(Color32::from_rgb(166, 220, 166))
                        .min_size(egui::vec2(132.0, 30.0)),
                    )
                    .clicked()
                {
                    actions.start_encode = true;
                }
                ui.label(RichText::new(format!("Preset: {name}")).weak());
            });
        });
        ui.label(RichText::new("Updates as settings are adjusted.").weak());

        ui.horizontal_wrapped(|ui| {
            if ui.button("Add advanced parameter...").clicked() {
                actions.add_advanced_parameter = true;
            }

            let mut remove_parameter = None;
            for (index, parameter) in advanced_parameters.iter().enumerate() {
                ui.label(RichText::new(&parameter.display).code());
                if ui.small_button("×").clicked() {
                    remove_parameter = Some(index);
                }
            }
            if let Some(index) = remove_parameter {
                advanced_parameters.remove(index);
            }
        });

        let command_arguments = effective_command_arguments(
            profile,
            video_controller,
            scale_controller,
            audio_controller,
            subtitles_controller,
            advanced_parameters,
        );
        let mut command_preview = command_arguments.join(" ");
        ui.add(
            egui::TextEdit::multiline(&mut command_preview)
                .code_editor()
                .desired_rows(5)
                .interactive(false)
                .desired_width(f32::INFINITY),
        );
    });
    actions
}

fn effective_command_arguments(
    profile: &Profile,
    video_controller: &crate::controllers::video::VideoController,
    scale_controller: &crate::controllers::scale::ScaleController,
    audio_controller: &crate::controllers::audio::AudioController,
    subtitles_controller: &crate::controllers::subtitles::SubtitlesController,
    advanced_parameters: &[AdvancedParameter],
) -> Vec<String> {
    let mut arguments = subtitles_controller.apply_ffmpeg_args(audio_controller.apply_ffmpeg_args(
        scale_controller.apply_ffmpeg_args(video_controller.effective_ffmpeg_args(profile)),
    ));
    for parameter in advanced_parameters {
        arguments.extend(parameter.arguments.iter().cloned());
    }
    crate::controllers::encoding::apply_output_metadata_policy(arguments)
}

fn format_media_time(seconds: f64) -> String {
    let seconds = seconds.max(0.0).round() as u64;
    let hours = seconds / 3_600;
    let minutes = (seconds % 3_600) / 60;
    let seconds = seconds % 60;
    format!("{hours:02}:{minutes:02}:{seconds:02}")
}
