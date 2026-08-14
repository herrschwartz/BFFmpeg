use crate::model::{AppModel, AudioStreamInfo, MediaInfo};
use rfd::FileDialog;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::process::Command;

pub struct FilesController;

impl FilesController {
    pub fn pick_folder(&self, current_folder: &Path) -> Option<PathBuf> {
        FileDialog::new()
            .set_directory(current_folder)
            .pick_folder()
    }

    pub fn select_video_file(&mut self, model: &mut AppModel, index: usize) {
        let Some(path) = model.video_files.get(index).map(|file| file.path.clone()) else {
            return;
        };

        model.selected_video_index = Some(index);
        model.selected_media_info = None;
        model.media_info_error = None;

        match probe_media_file(&path) {
            Ok(info) => model.selected_media_info = Some(info),
            Err(error) => model.media_info_error = Some(error),
        }
    }
}

fn probe_media_file(path: &Path) -> Result<MediaInfo, String> {
    let output = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=index,codec_type,codec_name,bit_rate",
            "-of",
            "json",
        ])
        .arg(path)
        .output()
        .map_err(|error| {
            format!("Could not run ffprobe. Ensure FFmpeg is installed and in PATH. ({error})")
        })?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if error.is_empty() {
            "ffprobe could not read the selected file.".to_owned()
        } else {
            format!("ffprobe could not read the selected file: {error}")
        });
    }

    let result: ProbeResult = serde_json::from_slice(&output.stdout)
        .map_err(|error| format!("Could not read the ffprobe result: {error}"))?;

    let video_stream = result
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"));
    let audio_streams = result
        .streams
        .iter()
        .filter(|stream| stream.codec_type.as_deref() == Some("audio"))
        .map(|stream| AudioStreamInfo {
            index: stream.index.unwrap_or_default(),
            codec: stream
                .codec_name
                .clone()
                .unwrap_or_else(|| "Unknown codec".to_owned()),
            bitrate: stream.bit_rate.as_deref().and_then(parse_bitrate),
        })
        .collect();

    Ok(MediaInfo {
        duration_seconds: result
            .format
            .duration
            .as_deref()
            .and_then(|value| value.parse().ok()),
        video_codec: video_stream.and_then(|stream| stream.codec_name.clone()),
        video_bitrate: video_stream
            .and_then(|stream| stream.bit_rate.as_deref())
            .and_then(parse_bitrate),
        audio_streams,
    })
}

fn parse_bitrate(value: &str) -> Option<u64> {
    value.parse().ok()
}

#[derive(Deserialize)]
struct ProbeResult {
    #[serde(default)]
    format: ProbeFormat,
    #[serde(default)]
    streams: Vec<ProbeStream>,
}

#[derive(Default, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: Option<usize>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    bit_rate: Option<String>,
}
