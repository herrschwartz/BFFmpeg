use crate::controllers::{
    audio::AudioController, encoding::EncodingController, files::FilesController,
    scale::ScaleController, subtitles::SubtitlesController, video::VideoController,
};
use crate::model::{AppModel, Profile};
use std::fs;
use std::path::PathBuf;

pub struct AppController {
    pub model: Option<AppModel>,
    pub status_message: String,
    pub files: FilesController,
    pub video: VideoController,
    pub scale: ScaleController,
    pub encoding: EncodingController,
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
            scale: ScaleController::default(),
            encoding: EncodingController::default(),
            audio: AudioController::default(),
            subtitles: SubtitlesController::default(),
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

    pub fn save_new_preset(&mut self, name: &str, ffmpeg_args: Vec<String>) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("Enter a name for the new preset.".to_owned());
        }

        {
            let model = self
                .model
                .as_mut()
                .ok_or_else(|| "No configuration is loaded.".to_owned())?;
            if model.config.profiles.contains_key(name) {
                return Err(format!("A preset named \"{name}\" already exists."));
            }

            let mut updated_config = model.config.clone();
            updated_config
                .profiles
                .insert(name.to_owned(), Profile { ffmpeg_args });
            write_config(&model.config_path, &updated_config)?;
            model.config = updated_config;
            model.selected_profile = name.to_owned();
            self.status_message = format!("Saved preset \"{name}\".");
        }
        self.sync_video_settings();
        Ok(())
    }

    pub fn delete_selected_preset(&mut self) -> Result<(), String> {
        let next_selection = {
            let model = self
                .model
                .as_mut()
                .ok_or_else(|| "No configuration is loaded.".to_owned())?;
            let deleted_name = model.selected_profile.clone();
            if model.config.profiles.len() <= 1 {
                return Err("At least one preset must remain.".to_owned());
            }

            let mut updated_config = model.config.clone();
            updated_config.profiles.remove(&deleted_name);
            if updated_config.default_profile.as_deref() == Some(&deleted_name) {
                updated_config.default_profile = updated_config.profiles.keys().next().cloned();
            }
            let next_selection = updated_config
                .profiles
                .keys()
                .next()
                .cloned()
                .ok_or_else(|| "At least one preset must remain.".to_owned())?;
            write_config(&model.config_path, &updated_config)?;
            model.config = updated_config;
            model.selected_profile = next_selection.clone();
            self.status_message = format!("Deleted preset \"{deleted_name}\".");
            next_selection
        };
        self.sync_video_settings();
        debug_assert!(!next_selection.is_empty());
        Ok(())
    }

    pub fn browse_for_folder(&mut self) {
        let Some(model) = &mut self.model else {
            return;
        };

        if let Some(folder) = self.files.pick_folder(&model.current_folder) {
            self.set_current_folder(folder);
        }
    }

    pub fn browse_for_video_file(&mut self) {
        let Some(model) = &self.model else {
            return;
        };
        let Some(path) = self.files.pick_video_file(&model.current_folder) else {
            return;
        };
        let Some(folder) = path.parent().map(|folder| folder.to_path_buf()) else {
            return;
        };

        self.set_current_folder(folder);
        if let Some(model) = &mut self.model {
            if let Some(index) = model.video_files.iter().position(|file| file.path == path) {
                self.files.select_video_file(model, index);
                self.status_message = format!("Selected {}", path.display());
            } else {
                self.status_message = format!(
                    "Opened {}, but the selected file is not a supported video format.",
                    path.display()
                );
            }
        }
    }

    pub fn set_current_folder(&mut self, folder: PathBuf) {
        if let Some(model) = &mut self.model {
            model.set_current_folder(folder);
            self.status_message = format!("Opened {}", model.current_folder.display());
        }

        if let Some(model) = &self.model {
            self.scale.refresh_for_folder(model);
            self.audio.refresh_for_folder(model);
            self.subtitles.refresh_for_folder(model);
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

fn write_config(path: &std::path::Path, config: &crate::model::Config) -> Result<(), String> {
    let serialized = serde_json::to_string_pretty(config)
        .map_err(|error| format!("Could not prepare config.json: {error}"))?;
    fs::write(path, format!("{serialized}\n"))
        .map_err(|error| format!("Could not save {}: {error}", path.display()))
}

pub fn parse_advanced_ffmpeg_args(input: &str) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut current = String::new();
    let mut quote = None;
    let mut characters = input.chars().peekable();

    while let Some(character) = characters.next() {
        if character == '\\' {
            match characters.peek().copied() {
                Some(next) if matches!(next, '\\' | '\'' | '\"') || next.is_whitespace() => {
                    current.push(next);
                    characters.next();
                }
                _ => current.push(character),
            }
            continue;
        }
        if matches!(character, '\'' | '\"') {
            if quote == Some(character) {
                quote = None;
            } else if quote.is_none() {
                quote = Some(character);
            } else {
                current.push(character);
            }
            continue;
        }
        if character.is_whitespace() && quote.is_none() {
            if !current.is_empty() {
                arguments.push(std::mem::take(&mut current));
            }
        } else {
            current.push(character);
        }
    }

    if quote.is_some() {
        return Err("Close the quoted value before adding the parameter.".to_owned());
    }
    if !current.is_empty() {
        arguments.push(current);
    }
    if arguments.is_empty() {
        return Err("Enter at least one FFmpeg parameter.".to_owned());
    }
    if !arguments[0].starts_with('-') {
        return Err(
            "Advanced parameters must start with an FFmpeg option, such as -rc-lookahead 32."
                .to_owned(),
        );
    }

    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::parse_advanced_ffmpeg_args;

    #[test]
    fn parses_quoted_advanced_parameter_values() {
        assert_eq!(
            parse_advanced_ffmpeg_args("-metadata title=\"My Encode\"").expect("arguments"),
            vec!["-metadata", "title=My Encode"]
        );
    }

    #[test]
    fn retains_backslashes_in_windows_style_values() {
        assert_eq!(
            parse_advanced_ffmpeg_args("-metadata comment=C:\\encodes\\test").expect("arguments"),
            vec!["-metadata", "comment=C:\\encodes\\test"]
        );
    }
}
