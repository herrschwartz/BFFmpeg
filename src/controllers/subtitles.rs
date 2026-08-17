use crate::controllers::files::probe_subtitle_streams;
use crate::model::{AppModel, SubtitleStreamInfo, VideoFile};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

pub struct SubtitlesController {
    pub passthrough_all_subtitles: bool,
    scan_state: SubtitleScanState,
    selected_common_tracks: Vec<bool>,
    scan_receiver: Option<Receiver<SubtitleScanUpdate>>,
}

#[derive(Clone)]
pub enum SubtitleScanState {
    Idle,
    Scanning { scanned: usize, total: usize },
    Complete(SubtitleScanResult),
}

#[derive(Clone)]
pub struct SubtitleScanResult {
    pub total_files: usize,
    pub common_tracks: Vec<SubtitleStreamInfo>,
    pub uncommon_files: Vec<SubtitleFileScan>,
}

#[derive(Clone)]
pub struct SubtitleFileScan {
    pub file_name: String,
    pub uncommon_tracks: Vec<SubtitleStreamInfo>,
    pub error: Option<String>,
}

enum SubtitleScanUpdate {
    Progress { scanned: usize, total: usize },
    Complete(SubtitleScanResult),
}

impl Default for SubtitlesController {
    fn default() -> Self {
        Self {
            passthrough_all_subtitles: true,
            scan_state: SubtitleScanState::Idle,
            selected_common_tracks: Vec::new(),
            scan_receiver: None,
        }
    }
}

impl SubtitlesController {
    pub fn update_passthrough(&mut self, model: &AppModel) {
        if self.passthrough_all_subtitles {
            self.scan_receiver = None;
            self.scan_state = SubtitleScanState::Idle;
            self.selected_common_tracks.clear();
        } else {
            self.start_subtitle_scan(&model.video_files);
        }
    }

    pub fn refresh_for_folder(&mut self, model: &AppModel) {
        self.scan_receiver = None;
        self.scan_state = SubtitleScanState::Idle;
        self.selected_common_tracks.clear();

        if !self.passthrough_all_subtitles {
            self.start_subtitle_scan(&model.video_files);
        }
    }

    pub fn poll_scan(&mut self) {
        loop {
            let update = match &self.scan_receiver {
                Some(receiver) => receiver.try_recv(),
                None => return,
            };

            match update {
                Ok(SubtitleScanUpdate::Progress { scanned, total }) => {
                    self.scan_state = SubtitleScanState::Scanning { scanned, total };
                }
                Ok(SubtitleScanUpdate::Complete(result)) => {
                    self.selected_common_tracks = vec![false; result.common_tracks.len()];
                    self.scan_state = SubtitleScanState::Complete(result);
                    self.scan_receiver = None;
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.selected_common_tracks.clear();
                    self.scan_state = SubtitleScanState::Complete(SubtitleScanResult {
                        total_files: 0,
                        common_tracks: Vec::new(),
                        uncommon_files: vec![SubtitleFileScan {
                            file_name: "Subtitle scan".to_owned(),
                            uncommon_tracks: Vec::new(),
                            error: Some("The subtitle-scan task ended unexpectedly.".to_owned()),
                        }],
                    });
                    self.scan_receiver = None;
                    return;
                }
            }
        }
    }

    pub fn scan_state(&self) -> &SubtitleScanState {
        &self.scan_state
    }

    pub fn common_track_selected_mut(&mut self, index: usize) -> Option<&mut bool> {
        self.selected_common_tracks.get_mut(index)
    }

    pub fn apply_ffmpeg_args(&self, mut arguments: Vec<String>) -> Vec<String> {
        let SubtitleScanState::Complete(result) = &self.scan_state else {
            return arguments;
        };
        if self.passthrough_all_subtitles {
            return arguments;
        }

        strip_subtitle_codec_arguments(&mut arguments);
        arguments.push("-map".to_owned());
        arguments.push("-0:s".to_owned());
        for (stream, selected) in result
            .common_tracks
            .iter()
            .zip(&self.selected_common_tracks)
        {
            if *selected {
                arguments.push("-map".to_owned());
                arguments.push(format!("0:{}?", stream.index));
            }
        }
        arguments.push("-c:s".to_owned());
        arguments.push("copy".to_owned());
        arguments
    }

    fn start_subtitle_scan(&mut self, video_files: &[VideoFile]) {
        let files = video_files
            .iter()
            .filter(|file| file.selected_for_batch)
            .cloned()
            .collect::<Vec<_>>();
        let total = files.len();
        let (sender, receiver) = mpsc::channel();
        self.selected_common_tracks.clear();
        self.scan_receiver = Some(receiver);
        self.scan_state = SubtitleScanState::Scanning { scanned: 0, total };

        thread::spawn(move || {
            let mut file_scans = Vec::with_capacity(total);

            for (index, file) in files.iter().enumerate() {
                let scan = match probe_subtitle_streams(&file.path) {
                    Ok(streams) => SubtitleFileScan {
                        file_name: file.name.clone(),
                        uncommon_tracks: streams,
                        error: None,
                    },
                    Err(error) => SubtitleFileScan {
                        file_name: file.name.clone(),
                        uncommon_tracks: Vec::new(),
                        error: Some(error),
                    },
                };
                file_scans.push(scan);

                if sender
                    .send(SubtitleScanUpdate::Progress {
                        scanned: index + 1,
                        total,
                    })
                    .is_err()
                {
                    return;
                }
            }

            let common_tracks = common_subtitle_tracks(&file_scans);
            for scan in &mut file_scans {
                if scan.error.is_none() {
                    scan.uncommon_tracks =
                        without_common_tracks(&scan.uncommon_tracks, &common_tracks);
                }
            }

            let _ = sender.send(SubtitleScanUpdate::Complete(SubtitleScanResult {
                total_files: total,
                common_tracks,
                uncommon_files: file_scans,
            }));
        });
    }
}

fn strip_subtitle_codec_arguments(arguments: &mut Vec<String>) {
    let mut index = 0;
    while index < arguments.len() {
        if arguments[index].starts_with("-c:s") {
            arguments.drain(index..(index + 2).min(arguments.len()));
        } else {
            index += 1;
        }
    }
}

fn common_subtitle_tracks(file_scans: &[SubtitleFileScan]) -> Vec<SubtitleStreamInfo> {
    if file_scans.is_empty() || file_scans.iter().any(|scan| scan.error.is_some()) {
        return Vec::new();
    }

    let (reference, other_files) = match file_scans.split_first() {
        Some(files) => files,
        None => return Vec::new(),
    };
    let mut used_tracks = other_files
        .iter()
        .map(|file| vec![false; file.uncommon_tracks.len()])
        .collect::<Vec<_>>();
    let mut common_tracks = Vec::new();

    for candidate in &reference.uncommon_tracks {
        let mut matches = Vec::with_capacity(other_files.len());
        for (file_index, file) in other_files.iter().enumerate() {
            let matching_track = file
                .uncommon_tracks
                .iter()
                .enumerate()
                .find(|(track_index, track)| {
                    !used_tracks[file_index][*track_index]
                        && matching_subtitle_tracks(candidate, track)
                })
                .map(|(track_index, _)| track_index);
            let Some(track_index) = matching_track else {
                matches.clear();
                break;
            };
            matches.push((file_index, track_index));
        }

        if matches.len() == other_files.len() {
            for (file_index, track_index) in matches {
                used_tracks[file_index][track_index] = true;
            }
            common_tracks.push(candidate.clone());
        }
    }

    common_tracks
}

fn without_common_tracks(
    tracks: &[SubtitleStreamInfo],
    common_tracks: &[SubtitleStreamInfo],
) -> Vec<SubtitleStreamInfo> {
    let mut common_matches = vec![false; tracks.len()];
    for common_track in common_tracks {
        if let Some((index, _)) = tracks.iter().enumerate().find(|(index, track)| {
            !common_matches[*index] && matching_subtitle_tracks(common_track, track)
        }) {
            common_matches[index] = true;
        }
    }

    tracks
        .iter()
        .zip(common_matches)
        .filter_map(|(track, is_common)| (!is_common).then_some(track.clone()))
        .collect()
}

fn matching_subtitle_tracks(first: &SubtitleStreamInfo, second: &SubtitleStreamInfo) -> bool {
    first.codec.eq_ignore_ascii_case(&second.codec)
        && optional_text_matches(&first.language, &second.language)
        && optional_text_matches(&first.title, &second.title)
}

fn optional_text_matches(first: &Option<String>, second: &Option<String>) -> bool {
    match (first.as_deref(), second.as_deref()) {
        (Some(first), Some(second)) => first.eq_ignore_ascii_case(second),
        (None, None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{SubtitleFileScan, common_subtitle_tracks, without_common_tracks};
    use crate::model::SubtitleStreamInfo;

    fn subtitle(index: usize, language: &str) -> SubtitleStreamInfo {
        SubtitleStreamInfo {
            index,
            language: Some(language.to_owned()),
            codec: "subrip".to_owned(),
            title: None,
        }
    }

    #[test]
    fn finds_only_the_subtitles_shared_by_every_file() {
        let files = vec![
            SubtitleFileScan {
                file_name: "one.mkv".to_owned(),
                uncommon_tracks: vec![subtitle(2, "eng"), subtitle(3, "spa")],
                error: None,
            },
            SubtitleFileScan {
                file_name: "two.mkv".to_owned(),
                uncommon_tracks: vec![subtitle(2, "eng"), subtitle(5, "fra")],
                error: None,
            },
        ];

        let common = common_subtitle_tracks(&files);
        assert_eq!(common, vec![subtitle(2, "eng")]);
        assert_eq!(
            without_common_tracks(&files[0].uncommon_tracks, &common),
            vec![subtitle(3, "spa")]
        );
    }
}
