use crate::controller::AppController;
use crate::model::Profile;
use crate::views;
use eframe::egui::{self, Color32, RichText};

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
}

impl EncoderApp {
    pub fn new(creation_context: &eframe::CreationContext<'_>) -> Self {
        creation_context.egui_ctx.set_visuals(egui::Visuals::dark());
        Self {
            controller: AppController::new(),
            active_tab: SettingsTab::Files,
        }
    }
}

impl eframe::App for EncoderApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
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

                for name in profile_names {
                    if ui
                        .selectable_label(name == selected_profile, &name)
                        .clicked()
                    {
                        self.controller.select_profile(name);
                    }
                }
            });

        egui::Frame::central_panel(ui.style()).show(ui, |ui| {
            let selected_profile_data = self
                .controller
                .selected_profile()
                .cloned()
                .map(|profile| (selected_profile.clone(), profile));

            if let Some((name, profile)) = selected_profile_data {
                profile_details(
                    ui,
                    &name,
                    &profile,
                    &self.controller.video,
                    &self.controller.scale,
                    &self.controller.audio,
                    &self.controller.subtitles,
                );
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
                                SettingsTab::Files => views::files::show(ui, model, files),
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
) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.heading("FFmpeg command");
            ui.label(RichText::new(format!("Preset: {name}")).weak());
        });
        ui.label(RichText::new("Updates as settings are adjusted.").weak());

        let command_arguments =
            subtitles_controller.apply_ffmpeg_args(audio_controller.apply_ffmpeg_args(
                scale_controller.apply_ffmpeg_args(video_controller.effective_ffmpeg_args(profile)),
            ));
        let mut command_preview = command_arguments.join(" ");
        ui.add(
            egui::TextEdit::multiline(&mut command_preview)
                .code_editor()
                .desired_rows(5)
                .interactive(false)
                .desired_width(f32::INFINITY),
        );
    });
}
