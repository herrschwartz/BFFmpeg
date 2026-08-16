use crate::controllers::files::probe_audio_streams;
use crate::model::{AppModel, AudioStreamInfo, VideoFile};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

pub struct AudioController {
    pub passthrough_all_audio: bool,
    scan_state: AudioScanState,
    track_settings: Vec<AudioTrackSettings>,
    scan_receiver: Option<Receiver<AudioScanUpdate>>,
}

#[derive(Clone)]
pub enum AudioScanState {
    Idle,
    Scanning { scanned: usize, total: usize },
    Complete(AudioScanResult),
}

#[derive(Clone)]
pub struct AudioScanResult {
    pub total_files: usize,
    pub reference_streams: Vec<AudioStreamInfo>,
    pub mismatches: Vec<AudioMismatch>,
}

#[derive(Clone)]
pub struct AudioMismatch {
    pub file_name: String,
    pub streams: Vec<AudioStreamInfo>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AudioCodec {
    Passthrough,
    Aac,
    Eac3,
    Ac3,
    Opus,
    Flac,
}

impl AudioCodec {
    pub const ALL: [Self; 6] = [
        Self::Passthrough,
        Self::Aac,
        Self::Eac3,
        Self::Ac3,
        Self::Opus,
        Self::Flac,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Passthrough => "Passthrough",
            Self::Aac => "AAC",
            Self::Eac3 => "E-AC-3",
            Self::Ac3 => "AC-3",
            Self::Opus => "Opus",
            Self::Flac => "FLAC",
        }
    }

    pub fn uses_bitrate(self) -> bool {
        !matches!(self, Self::Passthrough | Self::Flac)
    }

    pub fn bitrate_options(self) -> &'static [u32] {
        match self {
            Self::Aac => &[112, 128, 160, 192, 224, 256, 320, 384, 512],
            Self::Eac3 => &[192, 256, 384, 448, 512, 576, 640, 768, 1_024],
            Self::Ac3 => &[192, 256, 384, 448, 512, 576, 640],
            Self::Opus => &[80, 96, 128, 160, 192, 256, 320, 384, 512],
            Self::Passthrough | Self::Flac => &[],
        }
    }

    pub fn default_bitrate(self) -> u32 {
        match self {
            Self::Aac => 256,
            Self::Eac3 | Self::Ac3 => 640,
            Self::Opus => 192,
            Self::Passthrough | Self::Flac => 0,
        }
    }

    pub fn ffmpeg_encoder(self) -> &'static str {
        match self {
            Self::Passthrough => "copy",
            Self::Aac => "aac",
            Self::Eac3 => "eac3",
            Self::Ac3 => "ac3",
            Self::Opus => "libopus",
            Self::Flac => "flac",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AudioTrackSettings {
    pub selected: bool,
    pub codec: AudioCodec,
    pub bitrate_kbps: u32,
}

impl Default for AudioTrackSettings {
    fn default() -> Self {
        Self {
            selected: true,
            codec: AudioCodec::Passthrough,
            bitrate_kbps: 0,
        }
    }
}

enum AudioScanUpdate {
    Progress { scanned: usize, total: usize },
    Complete(AudioScanResult),
}

impl Default for AudioController {
    fn default() -> Self {
        Self {
            passthrough_all_audio: true,
            scan_state: AudioScanState::Idle,
            track_settings: Vec::new(),
            scan_receiver: None,
        }
    }
}

impl AudioController {
    pub fn update_passthrough(&mut self, model: &AppModel) {
        if self.passthrough_all_audio {
            self.scan_receiver = None;
            self.scan_state = AudioScanState::Idle;
            self.track_settings.clear();
        } else {
            self.start_audio_scan(&model.video_files);
        }
    }

    pub fn refresh_for_folder(&mut self, model: &AppModel) {
        self.scan_receiver = None;
        self.scan_state = AudioScanState::Idle;
        self.track_settings.clear();

        if !self.passthrough_all_audio {
            self.start_audio_scan(&model.video_files);
        }
    }

    pub fn poll_scan(&mut self) {
        loop {
            let update = match &self.scan_receiver {
                Some(receiver) => receiver.try_recv(),
                None => return,
            };

            match update {
                Ok(AudioScanUpdate::Progress { scanned, total }) => {
                    self.scan_state = AudioScanState::Scanning { scanned, total };
                }
                Ok(AudioScanUpdate::Complete(result)) => {
                    self.track_settings =
                        vec![AudioTrackSettings::default(); result.reference_streams.len()];
                    self.scan_state = AudioScanState::Complete(result);
                    self.scan_receiver = None;
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.track_settings.clear();
                    self.scan_state = AudioScanState::Complete(AudioScanResult {
                        total_files: 0,
                        reference_streams: Vec::new(),
                        mismatches: vec![AudioMismatch {
                            file_name: "Audio scan".to_owned(),
                            streams: Vec::new(),
                            error: Some("The audio-scan task ended unexpectedly.".to_owned()),
                        }],
                    });
                    self.scan_receiver = None;
                    return;
                }
            }
        }
    }

    pub fn scan_state(&self) -> &AudioScanState {
        &self.scan_state
    }

    pub fn track_settings_mut(&mut self, index: usize) -> Option<&mut AudioTrackSettings> {
        self.track_settings.get_mut(index)
    }

    pub fn apply_ffmpeg_args(&self, mut arguments: Vec<String>) -> Vec<String> {
        let AudioScanState::Complete(result) = &self.scan_state else {
            return arguments;
        };
        if self.passthrough_all_audio || !result.mismatches.is_empty() {
            return arguments;
        }

        strip_audio_output_arguments(&mut arguments);
        arguments.push("-map".to_owned());
        arguments.push("-0:a".to_owned());
        let mut output_track_index = 0;
        for (source_track_index, settings) in self.track_settings.iter().enumerate() {
            if !settings.selected {
                continue;
            }

            arguments.push("-map".to_owned());
            arguments.push(format!("0:a:{source_track_index}?"));
            arguments.push(format!("-c:a:{output_track_index}"));
            arguments.push(settings.codec.ffmpeg_encoder().to_owned());
            if settings.codec.uses_bitrate() {
                arguments.push(format!("-b:a:{output_track_index}"));
                arguments.push(format!("{}k", settings.bitrate_kbps));
            }
            output_track_index += 1;
        }
        arguments
    }

    fn start_audio_scan(&mut self, video_files: &[VideoFile]) {
        let files = video_files.to_vec();
        let total = files.len();
        let (sender, receiver) = mpsc::channel();
        self.track_settings.clear();
        self.scan_receiver = Some(receiver);
        self.scan_state = AudioScanState::Scanning { scanned: 0, total };

        thread::spawn(move || {
            let mut reference_streams: Option<Vec<AudioStreamInfo>> = None;
            let mut mismatches = Vec::new();

            for (index, file) in files.iter().enumerate() {
                match probe_audio_streams(&file.path) {
                    Ok(streams) => {
                        if let Some(reference) = &reference_streams {
                            if !matching_stream_codecs(reference, &streams) {
                                mismatches.push(AudioMismatch {
                                    file_name: file.name.clone(),
                                    streams,
                                    error: None,
                                });
                            }
                        } else {
                            reference_streams = Some(streams);
                        }
                    }
                    Err(error) => mismatches.push(AudioMismatch {
                        file_name: file.name.clone(),
                        streams: Vec::new(),
                        error: Some(error),
                    }),
                }

                if sender
                    .send(AudioScanUpdate::Progress {
                        scanned: index + 1,
                        total,
                    })
                    .is_err()
                {
                    return;
                }
            }

            let _ = sender.send(AudioScanUpdate::Complete(AudioScanResult {
                total_files: total,
                reference_streams: reference_streams.unwrap_or_default(),
                mismatches,
            }));
        });
    }
}

fn strip_audio_output_arguments(arguments: &mut Vec<String>) {
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index].starts_with("-c:a") || arguments[index].starts_with("-b:a") {
            arguments.drain(index..(index + 2).min(arguments.len()));
        } else {
            index += 1;
        }
    }
}

fn matching_stream_codecs(first: &[AudioStreamInfo], second: &[AudioStreamInfo]) -> bool {
    first.len() == second.len()
        && first
            .iter()
            .zip(second)
            .all(|(first, second)| first.codec.eq_ignore_ascii_case(&second.codec))
}

#[cfg(test)]
mod tests {
    use super::{AudioCodec, AudioController, AudioScanResult, AudioScanState, AudioTrackSettings};

    #[test]
    fn excludes_unselected_audio_tracks_from_the_ffmpeg_mapping() {
        let controller = AudioController {
            passthrough_all_audio: false,
            scan_state: AudioScanState::Complete(AudioScanResult {
                total_files: 1,
                reference_streams: Vec::new(),
                mismatches: Vec::new(),
            }),
            track_settings: vec![
                AudioTrackSettings {
                    selected: false,
                    codec: AudioCodec::Passthrough,
                    bitrate_kbps: 0,
                },
                AudioTrackSettings {
                    selected: true,
                    codec: AudioCodec::Eac3,
                    bitrate_kbps: 640,
                },
            ],
            scan_receiver: None,
        };

        let arguments = controller.apply_ffmpeg_args(vec![
            "-map".to_owned(),
            "0".to_owned(),
            "-c:a".to_owned(),
            "copy".to_owned(),
        ]);

        assert_eq!(
            arguments,
            vec![
                "-map", "0", "-map", "-0:a", "-map", "0:a:1?", "-c:a:0", "eac3", "-b:a:0", "640k"
            ]
        );
    }
}
