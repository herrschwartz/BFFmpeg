use crate::model::{AppModel, AudioStreamInfo, MediaInfo, VideoBitrate};
use rfd::FileDialog;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

#[derive(Default)]
pub struct FilesController {
    media_info_receiver: Option<Receiver<Result<MediaInfo, String>>>,
}

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
        model.media_info_loading = true;

        let (sender, receiver) = mpsc::channel();
        self.media_info_receiver = Some(receiver);
        thread::spawn(move || {
            let _ = sender.send(probe_media_file(&path));
        });
    }

    pub fn poll_media_info(&mut self, model: &mut AppModel) {
        let result = match &self.media_info_receiver {
            Some(receiver) => receiver.try_recv(),
            None => return,
        };

        match result {
            Ok(Ok(info)) => {
                model.selected_media_info = Some(info);
                model.media_info_error = None;
                model.media_info_loading = false;
                self.media_info_receiver = None;
            }
            Ok(Err(error)) => {
                model.selected_media_info = None;
                model.media_info_error = Some(error);
                model.media_info_loading = false;
                self.media_info_receiver = None;
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                model.selected_media_info = None;
                model.media_info_error =
                    Some("The media-inspection task ended unexpectedly.".to_owned());
                model.media_info_loading = false;
                self.media_info_receiver = None;
            }
        }
    }
}

fn probe_media_file(path: &Path) -> Result<MediaInfo, String> {
    let output = ffprobe_command()
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration,bit_rate:stream=index,codec_type,codec_name,bit_rate,channels,color_transfer,color_primaries:stream_tags",
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
            language: stream_tag_value(&stream.tags, &["language"])
                .map(str::to_owned)
                .filter(|language| !language.is_empty()),
            codec: stream
                .codec_name
                .clone()
                .unwrap_or_else(|| "Unknown codec".to_owned()),
            bitrate: stream.bit_rate.as_deref().and_then(parse_bitrate),
            channels: stream.channels,
        })
        .collect::<Vec<_>>();

    Ok(MediaInfo {
        duration_seconds: result
            .format
            .duration
            .as_deref()
            .and_then(|value| value.parse().ok()),
        video_codec: video_stream.and_then(|stream| stream.codec_name.clone()),
        dynamic_range: video_stream.map(dynamic_range),
        video_bitrate: resolve_video_bitrate(
            video_stream.and_then(stream_video_bitrate),
            result.format.bit_rate.as_deref(),
            &audio_streams,
        ),
        audio_streams,
    })
}

pub(crate) fn probe_audio_streams(path: &Path) -> Result<Vec<AudioStreamInfo>, String> {
    probe_media_file(path).map(|media_info| media_info.audio_streams)
}

fn ffprobe_command() -> Command {
    let mut command = Command::new("ffprobe");

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

fn resolve_video_bitrate(
    stream_bitrate: Option<VideoBitrate>,
    total_bitrate: Option<&str>,
    audio_streams: &[AudioStreamInfo],
) -> Option<VideoBitrate> {
    if stream_bitrate.is_some() {
        return stream_bitrate;
    }

    let total_bitrate = total_bitrate.and_then(parse_bitrate)?;
    let audio_bitrate = audio_streams
        .iter()
        .filter_map(|stream| stream.bitrate)
        .sum::<u64>();
    (total_bitrate > audio_bitrate).then_some(VideoBitrate {
        bits_per_second: total_bitrate - audio_bitrate,
        is_estimated: true,
    })
}

fn stream_video_bitrate(stream: &ProbeStream) -> Option<VideoBitrate> {
    if let Some(bits_per_second) = stream.bit_rate.as_deref().and_then(parse_bitrate) {
        return Some(VideoBitrate {
            bits_per_second,
            is_estimated: false,
        });
    }

    if let Some(bits_per_second) =
        stream_tag_value(&stream.tags, &["BPS", "BPS-eng"]).and_then(parse_bitrate)
    {
        return Some(VideoBitrate {
            bits_per_second,
            is_estimated: false,
        });
    }

    let bytes = stream_tag_value(&stream.tags, &["NUMBER_OF_BYTES", "NUMBER_OF_BYTES-eng"])
        .and_then(|value| value.parse::<u64>().ok())?;
    let duration_seconds = stream_tag_value(&stream.tags, &["DURATION", "DURATION-eng"])
        .and_then(parse_tag_duration)?;
    let bits_per_second = ((bytes as f64 * 8.0) / duration_seconds) as u64;

    Some(VideoBitrate {
        bits_per_second,
        is_estimated: true,
    })
}

fn dynamic_range(stream: &ProbeStream) -> String {
    match stream.color_transfer.as_deref() {
        Some("smpte2084") => "HDR (PQ)".to_owned(),
        Some("arib-std-b67") => "HDR (HLG)".to_owned(),
        _ if stream.color_primaries.as_deref() == Some("bt2020") => "HDR (BT.2020)".to_owned(),
        _ => "SDR".to_owned(),
    }
}

fn stream_tag_value<'a>(tags: &'a BTreeMap<String, String>, keys: &[&str]) -> Option<&'a str> {
    tags.iter()
        .find(|(key, _)| {
            keys.iter()
                .any(|candidate| key.eq_ignore_ascii_case(candidate))
        })
        .map(|(_, value)| value.as_str())
}

fn parse_tag_duration(value: &str) -> Option<f64> {
    if let Ok(seconds) = value.parse::<f64>() {
        return (seconds > 0.0).then_some(seconds);
    }

    let mut parts = value.split(':').rev();
    let seconds = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<u64>().ok()?;
    let hours = parts
        .next()
        .map(|value| value.parse::<u64>().ok())
        .flatten()
        .unwrap_or(0);
    (seconds >= 0.0).then_some(hours as f64 * 3_600.0 + minutes as f64 * 60.0 + seconds)
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
    bit_rate: Option<String>,
}

#[derive(Deserialize)]
struct ProbeStream {
    index: Option<usize>,
    codec_type: Option<String>,
    codec_name: Option<String>,
    bit_rate: Option<String>,
    channels: Option<u32>,
    color_transfer: Option<String>,
    color_primaries: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[cfg(test)]
mod tests {
    use super::{ProbeStream, resolve_video_bitrate, stream_video_bitrate};
    use crate::model::{AudioStreamInfo, VideoBitrate};
    use std::collections::BTreeMap;

    #[test]
    fn estimates_missing_video_bitrate_from_total_bitrate() {
        let audio_streams = vec![
            AudioStreamInfo {
                index: 1,
                language: Some("eng".to_owned()),
                codec: "aac".to_owned(),
                bitrate: Some(192_000),
                channels: Some(2),
            },
            AudioStreamInfo {
                index: 2,
                language: Some("eng".to_owned()),
                codec: "ac3".to_owned(),
                bitrate: Some(384_000),
                channels: Some(6),
            },
        ];

        let bitrate = resolve_video_bitrate(None, Some("5000000"), &audio_streams)
            .expect("total bitrate should produce an estimate");

        assert_eq!(bitrate.bits_per_second, 4_424_000);
        assert!(bitrate.is_estimated);
    }

    #[test]
    fn keeps_the_stream_bitrate_when_available() {
        let bitrate = resolve_video_bitrate(
            Some(VideoBitrate {
                bits_per_second: 4_500_000,
                is_estimated: false,
            }),
            Some("5000000"),
            &[],
        )
        .expect("stream bitrate should be used");

        assert_eq!(bitrate.bits_per_second, 4_500_000);
        assert!(!bitrate.is_estimated);
    }

    #[test]
    fn reads_video_bitrate_from_bps_stream_metadata() {
        let stream = ProbeStream {
            index: Some(1),
            codec_type: Some("video".to_owned()),
            codec_name: Some("h264".to_owned()),
            bit_rate: None,
            channels: None,
            color_transfer: None,
            color_primaries: None,
            tags: BTreeMap::from([("BPS-eng".to_owned(), "10509649".to_owned())]),
        };

        let bitrate = stream_video_bitrate(&stream).expect("BPS metadata should be used");

        assert_eq!(bitrate.bits_per_second, 10_509_649);
        assert!(!bitrate.is_estimated);
    }

    #[test]
    fn calculates_video_bitrate_from_stream_bytes_and_duration() {
        let stream = ProbeStream {
            index: Some(1),
            codec_type: Some("video".to_owned()),
            codec_name: Some("h264".to_owned()),
            bit_rate: None,
            channels: None,
            color_transfer: None,
            color_primaries: None,
            tags: BTreeMap::from([
                ("NUMBER_OF_BYTES-eng".to_owned(), "3334759191".to_owned()),
                ("DURATION-eng".to_owned(), "00:42:18.436000000".to_owned()),
            ]),
        };

        let bitrate = stream_video_bitrate(&stream).expect("bytes and duration should be used");

        assert_eq!(bitrate.bits_per_second, 10_509_649);
        assert!(bitrate.is_estimated);
    }
}
