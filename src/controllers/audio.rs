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
            Self::Aac => &[128, 160, 192, 256, 320, 384, 512],
            Self::Eac3 => &[192, 256, 384, 448, 512, 640, 768, 1_024],
            Self::Ac3 => &[192, 256, 384, 448, 512, 640],
            Self::Opus => &[96, 128, 160, 192, 256, 320, 384, 512],
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
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct AudioTrackSettings {
    pub codec: AudioCodec,
    pub bitrate_kbps: u32,
}

impl Default for AudioTrackSettings {
    fn default() -> Self {
        Self {
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

fn matching_stream_codecs(first: &[AudioStreamInfo], second: &[AudioStreamInfo]) -> bool {
    first.len() == second.len()
        && first
            .iter()
            .zip(second)
            .all(|(first, second)| first.codec.eq_ignore_ascii_case(&second.codec))
}
