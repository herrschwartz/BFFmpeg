use crate::controllers::{
    audio::AudioController, files::FilesController, subtitles::SubtitlesController,
    video::VideoController,
};
use crate::model::{AppModel, Profile};
use std::path::PathBuf;

pub struct AppController {
    pub model: Option<AppModel>,
    pub status_message: String,
    pub files: FilesController,
    pub video: VideoController,
    pub audio: AudioController,
    pub subtitles: SubtitlesController,
}

impl AppController {
    pub fn new() -> Self {
        let mut controller = Self {
            model: None,
            status_message: String::new(),
            files: FilesController::default(),
            video: VideoController::default(),
            audio: AudioController::default(),
            subtitles: SubtitlesController,
        };
        controller.reload_config();
        controller
    }

    pub fn reload_config(&mut self) {
        let previous_selection = self
            .model
            .as_ref()
            .map(|model| model.selected_profile.clone());

        match AppModel::load() {
            Ok(mut model) => {
                if let Some(previous_selection) = previous_selection
                    && model.config.profiles.contains_key(&previous_selection)
                {
                    model.selected_profile = previous_selection;
                }
                self.status_message = format!("Loaded {}", model.config_path.display());
                self.model = Some(model);
                self.sync_video_settings();
            }
            Err(error) => {
                self.status_message = error;
                self.model = None;
            }
        }
    }

    pub fn select_profile(&mut self, name: String) {
        if let Some(model) = &mut self.model {
            model.selected_profile = name;
        }
        self.sync_video_settings();
    }

    pub fn selected_profile(&self) -> Option<&Profile> {
        let model = self.model.as_ref()?;
        model.config.profiles.get(&model.selected_profile)
    }

    pub fn browse_for_folder(&mut self) {
        let Some(model) = &mut self.model else {
            return;
        };

        if let Some(folder) = self.files.pick_folder(&model.current_folder) {
            self.set_current_folder(folder);
        }
    }

    pub fn set_current_folder(&mut self, folder: PathBuf) {
        if let Some(model) = &mut self.model {
            model.set_current_folder(folder);
            self.status_message = format!("Opened {}", model.current_folder.display());
        }

        if let Some(model) = &self.model {
            self.audio.refresh_for_folder(model);
        }
    }

    fn sync_video_settings(&mut self) {
        let selected_profile = self
            .model
            .as_ref()
            .and_then(|model| model.config.profiles.get(&model.selected_profile).cloned());

        if let Some(profile) = selected_profile {
            self.video.load_profile(&profile);
        }
    }
}
