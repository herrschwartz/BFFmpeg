use crate::controller::AppController;
use crate::model::Profile;
use crate::views;
use eframe::egui::{self, Color32, RichText};

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsTab {
    Files,
    Video,
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
                ui.menu_button("File", |ui| {
                    if ui.button("Open folder...").clicked() {
                        browse_for_folder = true;
                    }
                });
                if browse_for_folder {
                    self.controller.browse_for_folder();
                }
                ui.separator();
                tab_button(ui, &mut self.active_tab, SettingsTab::Files, "Files");
                tab_button(ui, &mut self.active_tab, SettingsTab::Video, "Video");
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
                profile_details(ui, &name, &profile);
                ui.add_space(16.0);
            }

            let controller = &mut self.controller;
            let crate::controller::AppController {
                model,
                files,
                video,
                audio,
                subtitles,
                ..
            } = controller;

            if let Some(model) = model {
                match self.active_tab {
                    SettingsTab::Files => views::files::show(ui, model, files),
                    SettingsTab::Video => views::video::show(ui, video),
                    SettingsTab::Audio => views::audio::show(ui, audio),
                    SettingsTab::Subtitles => views::subtitles::show(ui, subtitles),
                }
            } else {
                ui.colored_label(
                    Color32::LIGHT_RED,
                    "The configuration could not be loaded. Add or repair config.json, then restart the application.",
                );
            }
        });
    }
}

fn tab_button(ui: &mut egui::Ui, active_tab: &mut SettingsTab, tab: SettingsTab, label: &str) {
    if ui.selectable_label(*active_tab == tab, label).clicked() {
        *active_tab = tab;
    }
}

fn profile_details(ui: &mut egui::Ui, name: &str, profile: &Profile) {
    ui.group(|ui| {
        ui.horizontal(|ui| {
            ui.heading("FFmpeg command");
            ui.label(RichText::new(format!("Preset: {name}")).weak());
        });
        ui.label(
            RichText::new("This read-only preview comes directly from the selected preset.").weak(),
        );

        let mut command_preview = profile.ffmpeg_args.join(" ");
        ui.add(
            egui::TextEdit::multiline(&mut command_preview)
                .code_editor()
                .desired_rows(5)
                .interactive(false)
                .desired_width(f32::INFINITY),
        );
    });
}
