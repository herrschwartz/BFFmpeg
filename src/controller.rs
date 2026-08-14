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
            files: FilesController,
            video: VideoController,
            audio: AudioController,
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
    }
}
