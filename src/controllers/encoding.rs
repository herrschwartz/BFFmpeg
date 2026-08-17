use crate::model::{OutputContainer, VideoFile};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const PREVIEW_DURATION_SECONDS: f64 = 30.0;
const PROGRESS_POLL_INTERVAL: Duration = Duration::from_millis(200);

const STALE_TRACK_STATISTIC_TAGS: &[&str] = &[
    "BPS",
    "BPS-eng",
    "NUMBER_OF_FRAMES",
    "NUMBER_OF_FRAMES-eng",
    "NUMBER_OF_BYTES",
    "NUMBER_OF_BYTES-eng",
    "_STATISTICS_WRITING_APP",
    "_STATISTICS_WRITING_DATE_UTC",
    "_STATISTICS_TAGS",
    "DURATION",
    "DURATION-eng",
];

/// Preserve descriptive metadata while preventing source track statistics from
/// being attached to newly encoded streams. FFmpeg's Matroska muxer writes fresh
/// duration tags; statistics that require a second full-file analysis are omitted
/// instead of publishing values copied from the input.
fn apply_output_metadata_policy(mut arguments: Vec<String>) -> Vec<String> {
    for tag in STALE_TRACK_STATISTIC_TAGS {
        arguments.push("-metadata:s".to_owned());
        arguments.push(format!("{tag}="));
    }
    arguments
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EncodingKind {
    Preview,
    Batch,
}

impl EncodingKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Preview => "Preview encode",
            Self::Batch => "Batch encode",
        }
    }
}

#[derive(Clone, Debug)]
pub struct EncodingProgress {
    pub kind: EncodingKind,
    pub current_file_name: String,
    pub current_file_index: usize,
    pub total_files: usize,
    pub file_progress: f32,
    pub overall_progress: f32,
    pub encoded_seconds: f64,
    pub duration_seconds: Option<f64>,
    pub file_started_at: Instant,
    pub average_fps: Option<f64>,
}

pub struct EncodingController {
    progress: Option<EncodingProgress>,
    update_receiver: Option<Receiver<EncodingUpdate>>,
}

#[derive(Clone)]
struct EncodeJob {
    input_path: PathBuf,
    file_name: String,
    output_path: PathBuf,
    preview_start_seconds: Option<f64>,
    preview_duration_seconds: Option<f64>,
}

enum EncodingUpdate {
    FileStarted {
        file_name: String,
        file_index: usize,
        total_files: usize,
    },
    FileProgress {
        file_progress: f32,
        overall_progress: f32,
        encoded_seconds: f64,
        duration_seconds: Option<f64>,
    },
    AverageFps(f64),
    Completed {
        kind: EncodingKind,
        total_files: usize,
    },
    Failed(String),
}

impl Default for EncodingController {
    fn default() -> Self {
        Self {
            progress: None,
            update_receiver: None,
        }
    }
}

impl EncodingController {
    pub fn is_running(&self) -> bool {
        self.progress.is_some()
    }

    pub fn progress(&self) -> Option<&EncodingProgress> {
        self.progress.as_ref()
    }

    pub fn start_preview(
        &mut self,
        input_path: PathBuf,
        file_name: String,
        duration_seconds: f64,
        output_directory: PathBuf,
        container: OutputContainer,
        ffmpeg_args: Vec<String>,
    ) -> Result<(), String> {
        if !duration_seconds.is_finite() || duration_seconds <= 0.0 {
            return Err(
                "The selected file does not have a usable duration for a preview encode."
                    .to_owned(),
            );
        }

        let preview_duration = duration_seconds.min(PREVIEW_DURATION_SECONDS);
        let preview_start = ((duration_seconds - preview_duration) / 2.0).max(0.0);
        let output_path = available_output_path(preview_output_path(
            &output_directory,
            &input_path,
            container.extension(),
        ));
        self.start(
            EncodingKind::Preview,
            vec![EncodeJob {
                input_path,
                file_name,
                output_path,
                preview_start_seconds: Some(preview_start),
                preview_duration_seconds: Some(preview_duration),
            }],
            ffmpeg_args,
        )
    }

    pub fn start_batch(
        &mut self,
        video_files: Vec<VideoFile>,
        output_directory: PathBuf,
        container: OutputContainer,
        ffmpeg_args: Vec<String>,
    ) -> Result<(), String> {
        if video_files.is_empty() {
            return Err("There are no video files to encode.".to_owned());
        }

        let jobs = video_files
            .into_iter()
            .map(|file| EncodeJob {
                output_path: available_output_path(batch_output_path(
                    &output_directory,
                    &file.path,
                    container.extension(),
                )),
                input_path: file.path,
                file_name: file.name,
                preview_start_seconds: None,
                preview_duration_seconds: None,
            })
            .collect();
        self.start(EncodingKind::Batch, jobs, ffmpeg_args)
    }

    pub fn poll(&mut self) -> Option<String> {
        loop {
            let update = match &self.update_receiver {
                Some(receiver) => receiver.try_recv(),
                None => return None,
            };

            match update {
                Ok(EncodingUpdate::FileStarted {
                    file_name,
                    file_index,
                    total_files,
                }) => {
                    if let Some(progress) = &mut self.progress {
                        progress.current_file_name = file_name;
                        progress.current_file_index = file_index;
                        progress.total_files = total_files;
                        progress.file_progress = 0.0;
                        progress.overall_progress = file_index as f32 / total_files as f32;
                        progress.encoded_seconds = 0.0;
                        progress.duration_seconds = None;
                        progress.file_started_at = Instant::now();
                        progress.average_fps = None;
                    }
                }
                Ok(EncodingUpdate::FileProgress {
                    file_progress,
                    overall_progress,
                    encoded_seconds,
                    duration_seconds,
                }) => {
                    if let Some(progress) = &mut self.progress {
                        progress.file_progress = file_progress;
                        progress.overall_progress = overall_progress;
                        progress.encoded_seconds = encoded_seconds;
                        progress.duration_seconds = duration_seconds;
                    }
                }
                Ok(EncodingUpdate::AverageFps(average_fps)) => {
                    if let Some(progress) = &mut self.progress {
                        progress.average_fps = Some(average_fps);
                    }
                }
                Ok(EncodingUpdate::Completed { kind, total_files }) => {
                    self.progress = None;
                    self.update_receiver = None;
                    return Some(match kind {
                        EncodingKind::Preview => "Preview encode completed.".to_owned(),
                        EncodingKind::Batch => format!("Completed encoding {total_files} file(s)."),
                    });
                }
                Ok(EncodingUpdate::Failed(error)) => {
                    self.progress = None;
                    self.update_receiver = None;
                    return Some(error);
                }
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.progress = None;
                    self.update_receiver = None;
                    return Some("The encoding task ended unexpectedly.".to_owned());
                }
            }
        }
    }

    fn start(
        &mut self,
        kind: EncodingKind,
        jobs: Vec<EncodeJob>,
        ffmpeg_args: Vec<String>,
    ) -> Result<(), String> {
        if self.is_running() {
            return Err("An encode is already in progress.".to_owned());
        }
        let ffmpeg_args = apply_output_metadata_policy(ffmpeg_args);
        let first_job = jobs
            .first()
            .ok_or_else(|| "There are no video files to encode.".to_owned())?;
        let total_files = jobs.len();
        let (sender, receiver) = mpsc::channel();
        self.progress = Some(EncodingProgress {
            kind,
            current_file_name: first_job.file_name.clone(),
            current_file_index: 0,
            total_files,
            file_progress: 0.0,
            overall_progress: 0.0,
            encoded_seconds: 0.0,
            duration_seconds: None,
            file_started_at: Instant::now(),
            average_fps: None,
        });
        self.update_receiver = Some(receiver);

        thread::spawn(move || {
            for (index, job) in jobs.iter().enumerate() {
                if sender
                    .send(EncodingUpdate::FileStarted {
                        file_name: job.file_name.clone(),
                        file_index: index,
                        total_files,
                    })
                    .is_err()
                {
                    return;
                }

                if let Err(error) = encode_job(job, &ffmpeg_args, kind, index, total_files, &sender)
                {
                    let _ = sender.send(EncodingUpdate::Failed(error));
                    return;
                }
            }

            let _ = sender.send(EncodingUpdate::Completed { kind, total_files });
        });
        Ok(())
    }
}

fn encode_job(
    job: &EncodeJob,
    ffmpeg_args: &[String],
    kind: EncodingKind,
    file_index: usize,
    total_files: usize,
    sender: &mpsc::Sender<EncodingUpdate>,
) -> Result<(), String> {
    let output_directory = job.output_path.parent().ok_or_else(|| {
        format!(
            "Could not determine the output folder for {}.",
            job.file_name
        )
    })?;
    fs::create_dir_all(output_directory).map_err(|error| {
        format!(
            "Could not create output folder {}: {error}",
            output_directory.display()
        )
    })?;

    let progress_probe = probe_progress_info(&job.input_path);
    let expected_duration = job
        .preview_duration_seconds
        .or(progress_probe.duration_seconds);
    let expected_frames = job
        .preview_duration_seconds
        .and_then(|duration| {
            progress_probe
                .frame_rate
                .map(|frame_rate| (duration * frame_rate).round() as u64)
        })
        .or(progress_probe.total_frames)
        .filter(|frames| *frames > 0);
    if sender
        .send(EncodingUpdate::FileProgress {
            file_progress: 0.0,
            overall_progress: file_index as f32 / total_files as f32,
            encoded_seconds: 0.0,
            duration_seconds: expected_duration,
        })
        .is_err()
    {
        return Ok(());
    }

    let progress_file = ProgressFile::new(file_index);
    let mut command = ffmpeg_command();
    command
        .args([
            "-hide_banner",
            "-nostdin",
            "-n",
            "-stats_period",
            "0.25",
            "-progress",
        ])
        .arg(progress_file.path())
        .args(["-nostats", "-loglevel", "error"]);
    if let Some(start_seconds) = job.preview_start_seconds {
        command.args(["-ss", &format_seconds(start_seconds)]);
    }
    if let Some(duration_seconds) = job.preview_duration_seconds {
        command.args(["-t", &format_seconds(duration_seconds)]);
    }
    command
        .arg("-i")
        .arg(&job.input_path)
        .args(ffmpeg_args)
        .arg(&job.output_path)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());

    let mut child = command.spawn().map_err(|error| {
        format!(
            "Could not start ffmpeg for {}. Ensure FFmpeg is installed and in PATH. ({error})",
            job.file_name
        )
    })?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| "Could not read ffmpeg error output.".to_owned())?;
    let stderr_reader = thread::spawn(move || {
        let mut stderr = stderr;
        let mut text = String::new();
        let _ = stderr.read_to_string(&mut text);
        text
    });

    let mut progress_offset = 0_u64;
    let mut pending_progress = Vec::new();
    let mut last_file_progress = 0.0_f32;
    let mut report_progress = |line: &str| {
        if let Some(average_fps) = parse_progress_fps(line) {
            let _ = sender.send(EncodingUpdate::AverageFps(average_fps));
        }
        let Some((file_progress, encoded_seconds)) =
            progress_position(line, expected_duration, expected_frames)
        else {
            return;
        };
        if file_progress <= last_file_progress + f32::EPSILON {
            return;
        }
        last_file_progress = file_progress;
        let overall_progress = (file_index as f32 + file_progress) / total_files as f32;
        let _ = sender.send(EncodingUpdate::FileProgress {
            file_progress,
            overall_progress,
            encoded_seconds,
            duration_seconds: expected_duration,
        });
    };

    let status = loop {
        read_new_progress_records(
            progress_file.path(),
            &mut progress_offset,
            &mut pending_progress,
            &mut report_progress,
        )
        .map_err(|error| {
            format!(
                "Could not read ffmpeg progress for {}: {error}",
                job.file_name
            )
        })?;
        match child
            .try_wait()
            .map_err(|error| format!("Could not monitor ffmpeg for {}: {error}", job.file_name))?
        {
            Some(status) => break status,
            None => thread::sleep(PROGRESS_POLL_INTERVAL),
        }
    };
    read_new_progress_records(
        progress_file.path(),
        &mut progress_offset,
        &mut pending_progress,
        &mut report_progress,
    )
    .map_err(|error| {
        format!(
            "Could not read final ffmpeg progress for {}: {error}",
            job.file_name
        )
    })?;
    let stderr = stderr_reader.join().unwrap_or_default();
    if !status.success() {
        let detail = stderr.trim();
        return Err(if detail.is_empty() {
            format!("ffmpeg failed while encoding {}.", job.file_name)
        } else {
            format!("ffmpeg failed while encoding {}: {detail}", job.file_name)
        });
    }

    let _ = sender.send(EncodingUpdate::FileProgress {
        file_progress: 1.0,
        overall_progress: (file_index + 1) as f32 / total_files as f32,
        encoded_seconds: expected_duration.unwrap_or_default(),
        duration_seconds: expected_duration,
    });
    let _ = kind;
    Ok(())
}

#[derive(Default)]
struct ProgressProbe {
    duration_seconds: Option<f64>,
    total_frames: Option<u64>,
    frame_rate: Option<f64>,
}

fn probe_progress_info(path: &Path) -> ProgressProbe {
    let output = ffprobe_command()
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration:stream=codec_type,duration,nb_frames,avg_frame_rate,r_frame_rate:stream_tags",
            "-of",
            "json",
        ])
        .arg(path)
        .output();
    let Ok(output) = output else {
        return ProgressProbe::default();
    };
    if !output.status.success() {
        return ProgressProbe::default();
    }
    parse_progress_probe(&output.stdout).unwrap_or_default()
}

#[derive(Deserialize)]
struct DurationProbe {
    #[serde(default)]
    format: Option<DurationProbeFormat>,
    #[serde(default)]
    streams: Vec<DurationProbeStream>,
}

#[derive(Deserialize)]
struct DurationProbeFormat {
    duration: Option<String>,
}

#[derive(Deserialize)]
struct DurationProbeStream {
    codec_type: Option<String>,
    duration: Option<String>,
    nb_frames: Option<String>,
    avg_frame_rate: Option<String>,
    r_frame_rate: Option<String>,
    #[serde(default)]
    tags: BTreeMap<String, String>,
}

#[cfg(test)]
fn parse_duration_probe(json: &[u8]) -> Option<f64> {
    parse_progress_probe(json)?.duration_seconds
}

fn parse_progress_probe(json: &[u8]) -> Option<ProgressProbe> {
    let probe: DurationProbe = serde_json::from_slice(json).ok()?;
    let duration_seconds = probe
        .format
        .as_ref()
        .and_then(|format| format.duration.as_deref())
        .and_then(parse_duration_value)
        .or_else(|| {
            probe
                .streams
                .iter()
                .filter(|stream| stream.codec_type.as_deref() == Some("video"))
                .filter_map(duration_from_stream)
                .next()
        })
        .or_else(|| {
            probe
                .streams
                .iter()
                .filter_map(duration_from_stream)
                .max_by(f64::total_cmp)
        });
    let video_stream = probe
        .streams
        .iter()
        .find(|stream| stream.codec_type.as_deref() == Some("video"));
    let frame_rate = video_stream.and_then(frame_rate_from_stream);
    let total_frames = video_stream.and_then(|stream| {
        stream
            .nb_frames
            .as_deref()
            .and_then(parse_positive_integer)
            .or_else(|| statistic_frame_count(stream))
            .or_else(|| {
                duration_seconds
                    .zip(frame_rate)
                    .and_then(|(duration, rate)| {
                        let frames = (duration * rate).round();
                        (frames.is_finite() && frames > 0.0).then_some(frames as u64)
                    })
            })
    });

    Some(ProgressProbe {
        duration_seconds,
        total_frames,
        frame_rate,
    })
}

fn duration_from_stream(stream: &DurationProbeStream) -> Option<f64> {
    stream
        .duration
        .as_deref()
        .and_then(parse_duration_value)
        .or_else(|| {
            stream.tags.iter().find_map(|(key, value)| {
                (key.eq_ignore_ascii_case("DURATION")
                    || key.to_ascii_uppercase().starts_with("DURATION-"))
                .then(|| parse_duration_value(value))
                .flatten()
            })
        })
}

fn parse_duration_value(value: &str) -> Option<f64> {
    value
        .parse::<f64>()
        .ok()
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .or_else(|| {
            parse_timecode(value).filter(|duration| duration.is_finite() && *duration > 0.0)
        })
}

fn frame_rate_from_stream(stream: &DurationProbeStream) -> Option<f64> {
    stream
        .avg_frame_rate
        .as_deref()
        .and_then(parse_frame_rate)
        .or_else(|| stream.r_frame_rate.as_deref().and_then(parse_frame_rate))
}

fn parse_frame_rate(value: &str) -> Option<f64> {
    let rate = if let Some((numerator, denominator)) = value.split_once('/') {
        let numerator = numerator.parse::<f64>().ok()?;
        let denominator = denominator.parse::<f64>().ok()?;
        (denominator != 0.0).then_some(numerator / denominator)?
    } else {
        value.parse::<f64>().ok()?
    };
    (rate.is_finite() && rate > 0.0).then_some(rate)
}

fn statistic_frame_count(stream: &DurationProbeStream) -> Option<u64> {
    stream.tags.iter().find_map(|(key, value)| {
        (key.eq_ignore_ascii_case("NUMBER_OF_FRAMES")
            || key.to_ascii_uppercase().starts_with("NUMBER_OF_FRAMES-"))
        .then(|| parse_positive_integer(value))
        .flatten()
    })
}

fn parse_positive_integer(value: &str) -> Option<u64> {
    value.parse::<u64>().ok().filter(|value| *value > 0)
}

fn ffmpeg_command() -> Command {
    command_without_window("ffmpeg")
}

fn ffprobe_command() -> Command {
    command_without_window("ffprobe")
}

fn command_without_window(program: &str) -> Command {
    let mut command = Command::new(program);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;

        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    command
}

fn batch_output_path(output_directory: &Path, input_path: &Path, extension: &str) -> PathBuf {
    output_directory.join(format!("{}.{}", file_stem(input_path), extension))
}

fn preview_output_path(output_directory: &Path, input_path: &Path, extension: &str) -> PathBuf {
    output_directory.join(format!("{}_preview.{}", file_stem(input_path), extension))
}

fn available_output_path(path: PathBuf) -> PathBuf {
    if !path.exists() {
        return path;
    }

    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let stem = file_stem(&path);
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("mkv");
    for index in 1_u32.. {
        let candidate = parent.join(format!("{stem}_{index}.{extension}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!("a u32 output suffix should always be available")
}

fn file_stem(path: &Path) -> String {
    path.file_stem()
        .and_then(|stem| stem.to_str())
        .filter(|stem| !stem.is_empty())
        .unwrap_or("encode")
        .to_owned()
}

fn format_seconds(seconds: f64) -> String {
    format!("{seconds:.3}")
}

fn parse_progress_seconds(line: &str) -> Option<f64> {
    let machine_progress = line.split_once('=').and_then(|(key, value)| match key {
        "out_time_us" | "out_time_ms" => value
            .parse::<f64>()
            .ok()
            .map(|microseconds| microseconds / 1_000_000.0),
        "out_time" => parse_timecode(value),
        _ => None,
    });
    machine_progress.or_else(|| parse_terminal_status_seconds(line))
}

fn parse_progress_frame(line: &str) -> Option<u64> {
    line.strip_prefix("frame=")?.trim().parse::<u64>().ok()
}

fn parse_progress_fps(line: &str) -> Option<f64> {
    let fps = line.strip_prefix("fps=")?.trim().parse::<f64>().ok()?;
    (fps.is_finite() && fps >= 0.0).then_some(fps)
}

fn progress_position(
    line: &str,
    expected_duration: Option<f64>,
    expected_frames: Option<u64>,
) -> Option<(f32, f64)> {
    parse_progress_seconds(line)
        .filter(|seconds| *seconds >= 0.0)
        .and_then(|seconds| {
            expected_duration.map(|duration| {
                let progress = (seconds / duration).clamp(0.0, 1.0) as f32;
                (progress, seconds)
            })
        })
        .or_else(|| {
            parse_progress_frame(line).and_then(|frame| {
                expected_frames.map(|total_frames| {
                    let progress = (frame as f64 / total_frames as f64).clamp(0.0, 1.0) as f32;
                    let encoded_seconds = expected_duration
                        .map(|duration| duration * progress as f64)
                        .unwrap_or_default();
                    (progress, encoded_seconds)
                })
            })
        })
}

fn parse_terminal_status_seconds(line: &str) -> Option<f64> {
    let (_, after_time) = line.split_once("time=")?;
    let value = after_time.split_whitespace().next()?;
    (value != "N/A").then(|| parse_timecode(value)).flatten()
}

struct ProgressFile {
    path: PathBuf,
}

impl ProgressFile {
    fn new(file_index: usize) -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        Self {
            path: std::env::temp_dir().join(format!(
                "bffmpeg-progress-{}-{file_index}-{nonce}.log",
                std::process::id()
            )),
        }
    }

    fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for ProgressFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn read_new_progress_records(
    path: &Path,
    offset: &mut u64,
    pending: &mut Vec<u8>,
    on_record: &mut impl FnMut(&str),
) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true);

    #[cfg(target_os = "windows")]
    {
        use std::os::windows::fs::OpenOptionsExt;

        const FILE_SHARE_READ: u32 = 0x0000_0001;
        const FILE_SHARE_WRITE: u32 = 0x0000_0002;
        const FILE_SHARE_DELETE: u32 = 0x0000_0004;
        options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }

    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let length = file.metadata()?.len();
    if length < *offset {
        *offset = 0;
        pending.clear();
    }
    file.seek(SeekFrom::Start(*offset))?;
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes)?;
    *offset += bytes.len() as u64;

    for byte in bytes {
        if matches!(byte, b'\r' | b'\n') {
            if !pending.is_empty() {
                let record = String::from_utf8_lossy(pending);
                on_record(&record);
                pending.clear();
            }
        } else {
            pending.push(byte);
        }
    }
    Ok(())
}

fn parse_timecode(value: &str) -> Option<f64> {
    let mut parts = value.split(':').rev();
    let seconds = parts.next()?.parse::<f64>().ok()?;
    let minutes = parts.next()?.parse::<f64>().ok()?;
    let hours = parts
        .next()
        .map(|part| part.parse::<f64>().ok())
        .flatten()
        .unwrap_or(0.0);
    Some(hours * 3_600.0 + minutes * 60.0 + seconds)
}

#[cfg(test)]
mod tests {
    use super::{
        ProgressFile, apply_output_metadata_policy, batch_output_path, command_without_window,
        parse_duration_probe, parse_progress_fps, parse_progress_frame, parse_progress_probe,
        parse_progress_seconds, preview_output_path, progress_position, read_new_progress_records,
    };
    use std::fs::{self, OpenOptions};
    use std::io::Write;
    use std::path::Path;
    use std::process::Stdio;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn reads_ffmpeg_progress_time() {
        assert_eq!(parse_progress_seconds("out_time_us=1500000"), Some(1.5));
        assert_eq!(
            parse_progress_seconds("out_time=00:00:02.250000"),
            Some(2.25)
        );
        assert_eq!(
            parse_progress_seconds(
                "frame= 120 fps=24 q=28.0 size=1024KiB time=00:00:05.25 bitrate=1.0kbits/s"
            ),
            Some(5.25)
        );
        assert_eq!(parse_progress_frame("frame=34406"), Some(34_406));
        assert_eq!(parse_progress_fps("fps=707.47"), Some(707.47));
    }

    #[test]
    fn reads_only_new_records_from_a_growing_progress_file() {
        let path = std::env::temp_dir().join(format!(
            "bffmpeg-progress-reader-test-{}.log",
            std::process::id()
        ));
        fs::write(&path, b"out_time=00:00:00.25\n").expect("first progress record");
        let mut offset = 0;
        let mut pending = Vec::new();
        let mut records = Vec::new();
        read_new_progress_records(&path, &mut offset, &mut pending, &mut |record| {
            records.push(record.to_owned())
        })
        .expect("first read");
        OpenOptions::new()
            .append(true)
            .open(&path)
            .expect("open progress file")
            .write_all(b"out_time=00:00:00.50\n")
            .expect("second progress record");
        read_new_progress_records(&path, &mut offset, &mut pending, &mut |record| {
            records.push(record.to_owned())
        })
        .expect("second read");
        let _ = fs::remove_file(path);
        assert_eq!(records.len(), 2);
        assert_eq!(parse_progress_seconds(&records[1]), Some(0.5));
    }

    #[test]
    #[ignore = "requires the locally installed ffmpeg executable"]
    fn reads_a_live_ffmpeg_progress_file_on_windows() {
        let progress_file = ProgressFile::new(999);
        let mut command = command_without_window("ffmpeg");
        command
            .args([
                "-hide_banner",
                "-nostdin",
                "-re",
                "-f",
                "lavfi",
                "-i",
                "testsrc2=size=128x72:rate=24",
                "-t",
                "2",
                "-stats_period",
                "0.2",
                "-progress",
            ])
            .arg(progress_file.path())
            .args(["-nostats", "-loglevel", "error", "-f", "null", "-"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = command.spawn().expect("start ffmpeg fixture");
        let mut offset = 0;
        let mut pending = Vec::new();
        let mut samples = Vec::new();
        loop {
            read_new_progress_records(
                progress_file.path(),
                &mut offset,
                &mut pending,
                &mut |record| {
                    if let Some(seconds) = parse_progress_seconds(record) {
                        samples.push(seconds);
                    }
                },
            )
            .expect("read live progress");
            if child.try_wait().expect("monitor ffmpeg").is_some() {
                break;
            }
            thread::sleep(Duration::from_millis(100));
        }
        assert!(samples.iter().any(|seconds| *seconds > 0.25));
        assert!(samples.iter().any(|seconds| *seconds > 1.0));
    }

    #[test]
    fn reads_duration_from_stream_tags_when_container_duration_is_missing() {
        let probe = br#"{
            "format": {},
            "streams": [{
                "codec_type": "video",
                "duration": "N/A",
                "tags": {"DURATION-eng": "00:23:55.017000000"}
            }]
        }"#;
        assert_eq!(parse_duration_probe(probe), Some(1_435.017));
    }

    #[test]
    fn reads_total_frames_for_timestamp_free_nvenc_progress() {
        let probe = br#"{
            "format": {"duration": "1435.017"},
            "streams": [{
                "codec_type": "video",
                "avg_frame_rate": "24000/1001",
                "r_frame_rate": "24000/1001",
                "tags": {"NUMBER_OF_FRAMES": "34406"}
            }]
        }"#;
        let progress = parse_progress_probe(probe).expect("progress probe");
        assert_eq!(progress.total_frames, Some(34_406));
        assert!((progress.frame_rate.expect("frame rate") - 23.976).abs() < 0.001);

        let (file_progress, encoded_seconds) =
            progress_position("frame=17203", Some(1_435.017), Some(34_406))
                .expect("frame-based position");
        assert!((file_progress - 0.5).abs() < f32::EPSILON);
        assert!((encoded_seconds - 717.5085).abs() < 0.001);
        assert_eq!(
            progress_position("out_time=N/A", Some(1_435.017), Some(34_406)),
            None
        );
    }

    #[test]
    fn appends_track_statistics_cleanup_to_output_arguments() {
        let arguments = apply_output_metadata_policy(vec!["-c:v".to_owned(), "libx265".to_owned()]);
        assert!(arguments.ends_with(&["-metadata:s".to_owned(), "DURATION-eng=".to_owned()]));
        assert!(
            arguments
                .windows(2)
                .any(|pair| pair == ["-metadata:s", "BPS="])
        );
    }

    #[test]
    fn generates_safe_output_names() {
        let input = Path::new(r"D:\media\Episode 01.mkv");
        let output_directory = Path::new(r"D:\media\out");
        assert_eq!(
            batch_output_path(output_directory, input, "mp4"),
            Path::new(r"D:\media\out\Episode 01.mp4")
        );
        assert_eq!(
            preview_output_path(output_directory, input, "mkv"),
            Path::new(r"D:\media\out\Episode 01_preview.mkv")
        );
    }
}
