use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Config {
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: BTreeMap<String, Profile>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Profile {
    #[serde(default)]
    pub ffmpeg_args: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputContainer {
    Mkv,
    Mp4,
    Webm,
}

impl OutputContainer {
    pub const ALL: [Self; 3] = [Self::Mkv, Self::Mp4, Self::Webm];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mkv => "MKV",
            Self::Mp4 => "MP4",
            Self::Webm => "WEBM",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Self::Mkv => "mkv",
            Self::Mp4 => "mp4",
            Self::Webm => "webm",
        }
    }
}

#[derive(Clone, Debug)]
pub struct VideoFile {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

#[derive(Clone, Debug)]
pub struct MediaInfo {
    pub duration_seconds: Option<f64>,
    pub video_codec: Option<String>,
    pub video_dimensions: Option<VideoDimensions>,
    pub dynamic_range: Option<String>,
    pub video_bitrate: Option<VideoBitrate>,
    pub audio_streams: Vec<AudioStreamInfo>,
    pub subtitle_streams: Vec<SubtitleStreamInfo>,
}

#[derive(Clone, Copy, Debug)]
pub struct VideoDimensions {
    pub width: u32,
    pub height: u32,
    pub display_aspect_ratio: f64,
}

#[derive(Clone, Debug)]
pub struct VideoBitrate {
    pub bits_per_second: u64,
    pub is_estimated: bool,
}

#[derive(Clone, Debug)]
pub struct AudioStreamInfo {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub bitrate: Option<u64>,
    pub channels: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SubtitleStreamInfo {
    pub index: usize,
    pub language: Option<String>,
    pub codec: String,
    pub title: Option<String>,
}

#[derive(Debug)]
pub struct AppModel {
    pub config_path: PathBuf,
    pub config: Config,
    pub selected_profile: String,
    pub current_folder: PathBuf,
    pub video_files: Vec<VideoFile>,
    pub folder_scan_error: Option<String>,
    pub selected_video_index: Option<usize>,
    pub selected_media_info: Option<MediaInfo>,
    pub media_info_error: Option<String>,
    pub media_info_loading: bool,
    pub output_directory: String,
    pub output_container: OutputContainer,
}

impl AppModel {
    pub fn load() -> Result<Self, String> {
        let config_path = find_config_path()?;
        let config_data = fs::read_to_string(&config_path)
            .map_err(|error| format!("Could not read {}: {error}", config_path.display()))?;
        let config: Config = serde_json::from_str(&config_data)
            .map_err(|error| format!("Could not parse {}: {error}", config_path.display()))?;
        let current_folder = env::current_dir()
            .map_err(|error| format!("Could not determine the current folder: {error}"))?;
        let (video_files, folder_scan_error) = read_video_files(&current_folder);

        let selected_profile = config
            .default_profile
            .as_ref()
            .filter(|name| config.profiles.contains_key(*name))
            .cloned()
            .or_else(|| config.profiles.keys().next().cloned())
            .ok_or_else(|| "config.json does not contain any profiles.".to_owned())?;

        Ok(Self {
            config_path,
            config,
            selected_profile,
            output_directory: current_folder.join("out").display().to_string(),
            current_folder,
            video_files,
            folder_scan_error,
            selected_video_index: None,
            selected_media_info: None,
            media_info_error: None,
            media_info_loading: false,
            output_container: OutputContainer::Mkv,
        })
    }

    pub fn set_current_folder(&mut self, folder: PathBuf) {
        let (video_files, folder_scan_error) = read_video_files(&folder);
        self.output_directory = folder.join("out").display().to_string();
        self.current_folder = folder;
        self.video_files = video_files;
        self.folder_scan_error = folder_scan_error;
        self.selected_video_index = None;
        self.selected_media_info = None;
        self.media_info_error = None;
        self.media_info_loading = false;
    }
}

fn read_video_files(folder: &std::path::Path) -> (Vec<VideoFile>, Option<String>) {
    let entries = match fs::read_dir(folder) {
        Ok(entries) => entries,
        Err(error) => {
            return (
                Vec::new(),
                Some(format!("Could not list {}: {error}", folder.display())),
            );
        }
    };

    let mut files = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let metadata = entry.metadata().ok()?;
            let path = entry.path();
            (metadata.is_file() && is_video_file(&path)).then(|| VideoFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                path,
                size_bytes: metadata.len(),
            })
        })
        .collect::<Vec<_>>();
    files.sort_by_cached_key(|file| file.name.to_lowercase());

    (files, None)
}

fn is_video_file(path: &std::path::Path) -> bool {
    const VIDEO_EXTENSIONS: &[&str] = &[
        "avi", "flv", "m2ts", "m4v", "mkv", "mov", "mp4", "mpeg", "mpg", "ts", "webm", "wmv",
    ];

    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| VIDEO_EXTENSIONS.contains(&extension.to_lowercase().as_str()))
}

fn find_config_path() -> Result<PathBuf, String> {
    let executable_path = env::current_exe()
        .map_err(|error| format!("Could not determine the executable location: {error}"))?;
    let executable_directory = executable_path
        .parent()
        .ok_or_else(|| "Could not determine the executable directory.".to_owned())?;
    let executable_config = executable_directory.join("config.json");

    if executable_config.is_file() {
        return Ok(executable_config);
    }

    // This fallback keeps `cargo run` convenient; packaged builds use the file beside the executable.
    let working_directory_config = env::current_dir()
        .map_err(|error| format!("Could not determine the working directory: {error}"))?
        .join("config.json");
    if working_directory_config.is_file() {
        return Ok(working_directory_config);
    }

    Err(format!(
        "config.json was not found beside the executable ({}) or in the working directory.",
        executable_directory.display()
    ))
}
