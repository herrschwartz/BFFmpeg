use crate::controllers::files::probe_video_dimensions;
use crate::model::{AppModel, VideoDimensions, VideoFile};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;

const ASPECT_RATIO_TOLERANCE: f64 = 0.01;
const COMMON_RATIOS: [(&str, f64); 4] = [
    ("4:3", 4.0 / 3.0),
    ("16:9", 16.0 / 9.0),
    ("21:9", 21.0 / 9.0),
    ("2.39:1", 2.39),
];

pub struct ScaleController {
    pub retain_current_resolution: bool,
    pub scale_percentage: u16,
    selected_resolution: Option<ScaleResolution>,
    scan_state: ScaleScanState,
    scan_receiver: Option<Receiver<ScaleScanUpdate>>,
}

#[derive(Clone)]
pub enum ScaleScanState {
    Idle,
    Scanning { scanned: usize, total: usize },
    Complete(ScaleScanResult),
}

#[derive(Clone)]
pub struct ScaleScanResult {
    pub total_files: usize,
    pub common_aspect_ratio: Option<CommonAspectRatio>,
    pub files: Vec<ScaleFileScan>,
}

#[derive(Clone)]
pub struct CommonAspectRatio {
    pub label: String,
    pub ratio: f64,
}

#[derive(Clone)]
pub struct ScaleFileScan {
    pub file_name: String,
    pub dimensions: Option<VideoDimensions>,
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScaleResolution {
    pub width: u32,
    pub height: u32,
}

enum ScaleScanUpdate {
    Progress { scanned: usize, total: usize },
    Complete(ScaleScanResult),
}

impl Default for ScaleController {
    fn default() -> Self {
        Self {
            retain_current_resolution: true,
            scale_percentage: 100,
            selected_resolution: None,
            scan_state: ScaleScanState::Idle,
            scan_receiver: None,
        }
    }
}

impl ScaleController {
    pub fn update_retain_current_resolution(&mut self, model: &AppModel) {
        if self.retain_current_resolution {
            self.scan_receiver = None;
            self.scan_state = ScaleScanState::Idle;
            self.selected_resolution = None;
        } else {
            self.start_scale_scan(&model.video_files);
        }
    }

    pub fn refresh_for_folder(&mut self, model: &AppModel) {
        self.scan_receiver = None;
        self.scan_state = ScaleScanState::Idle;
        self.selected_resolution = None;

        if !self.retain_current_resolution {
            self.start_scale_scan(&model.video_files);
        }
    }

    pub fn poll_scan(&mut self) {
        loop {
            let update = match &self.scan_receiver {
                Some(receiver) => receiver.try_recv(),
                None => return,
            };

            match update {
                Ok(ScaleScanUpdate::Progress { scanned, total }) => {
                    self.scan_state = ScaleScanState::Scanning { scanned, total };
                }
                Ok(ScaleScanUpdate::Complete(result)) => {
                    self.scan_state = ScaleScanState::Complete(result);
                    self.scan_receiver = None;
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.scan_state = ScaleScanState::Complete(ScaleScanResult {
                        total_files: 0,
                        common_aspect_ratio: None,
                        files: vec![ScaleFileScan {
                            file_name: "Scale scan".to_owned(),
                            dimensions: None,
                            error: Some("The scale-scan task ended unexpectedly.".to_owned()),
                        }],
                    });
                    self.scan_receiver = None;
                    return;
                }
            }
        }
    }

    pub fn scan_state(&self) -> &ScaleScanState {
        &self.scan_state
    }

    pub fn selected_resolution(&self) -> Option<ScaleResolution> {
        self.selected_resolution
    }

    pub fn selected_resolution_mut(&mut self) -> &mut Option<ScaleResolution> {
        &mut self.selected_resolution
    }

    pub fn apply_ffmpeg_args(&self, mut arguments: Vec<String>) -> Vec<String> {
        if self.retain_current_resolution {
            return arguments;
        }

        strip_video_filter_arguments(&mut arguments);
        arguments.push("-vf".to_owned());
        arguments.push(self.scale_filter());
        arguments
    }

    fn scale_filter(&self) -> String {
        match self.selected_resolution {
            Some(resolution) => format!("scale={}:{}", resolution.width, resolution.height),
            None => format!(
                "scale=trunc(iw*{}/100/2)*2:trunc(ih*{}/100/2)*2",
                self.scale_percentage, self.scale_percentage
            ),
        }
    }

    fn start_scale_scan(&mut self, video_files: &[VideoFile]) {
        let files = video_files.to_vec();
        let total = files.len();
        let (sender, receiver) = mpsc::channel();
        self.selected_resolution = None;
        self.scan_receiver = Some(receiver);
        self.scan_state = ScaleScanState::Scanning { scanned: 0, total };

        thread::spawn(move || {
            let mut file_scans = Vec::with_capacity(total);
            for (index, file) in files.iter().enumerate() {
                let scan = match probe_video_dimensions(&file.path) {
                    Ok(dimensions) => ScaleFileScan {
                        file_name: file.name.clone(),
                        dimensions: Some(dimensions),
                        error: None,
                    },
                    Err(error) => ScaleFileScan {
                        file_name: file.name.clone(),
                        dimensions: None,
                        error: Some(error),
                    },
                };
                file_scans.push(scan);

                if sender
                    .send(ScaleScanUpdate::Progress {
                        scanned: index + 1,
                        total,
                    })
                    .is_err()
                {
                    return;
                }
            }

            let _ = sender.send(ScaleScanUpdate::Complete(ScaleScanResult {
                total_files: total,
                common_aspect_ratio: common_aspect_ratio(&file_scans),
                files: file_scans,
            }));
        });
    }
}

impl CommonAspectRatio {
    pub fn resolution_presets(&self) -> [ScaleResolution; 4] {
        [2_160, 1_080, 720, 480].map(|height| ScaleResolution {
            width: rounded_even(self.ratio * height as f64),
            height,
        })
    }
}

impl ScaleResolution {
    pub fn label(self) -> String {
        let tier = match self.height {
            2_160 => "4K / 2160p",
            1_080 => "1080p",
            720 => "720p",
            480 => "480p",
            _ => "Custom",
        };
        format!("{tier} — {} × {}", self.width, self.height)
    }
}

fn strip_video_filter_arguments(arguments: &mut Vec<String>) {
    let mut index = 0;
    while index < arguments.len() {
        if matches!(arguments[index].as_str(), "-vf" | "-filter:v") {
            arguments.drain(index..(index + 2).min(arguments.len()));
        } else {
            index += 1;
        }
    }
}

fn common_aspect_ratio(files: &[ScaleFileScan]) -> Option<CommonAspectRatio> {
    if files.is_empty() {
        return None;
    }
    let dimensions = files
        .iter()
        .map(|file| file.dimensions)
        .collect::<Option<Vec<_>>>()?;
    let average_ratio = dimensions
        .iter()
        .map(|dimensions| dimensions.display_aspect_ratio)
        .sum::<f64>()
        / dimensions.len() as f64;

    dimensions
        .iter()
        .all(|dimensions| {
            (dimensions.display_aspect_ratio - average_ratio).abs()
                <= average_ratio * ASPECT_RATIO_TOLERANCE
        })
        .then(|| {
            let known_ratio = COMMON_RATIOS.iter().find(|(_, ratio)| {
                (*ratio - average_ratio).abs() <= average_ratio * ASPECT_RATIO_TOLERANCE
            });
            let (label, ratio) = known_ratio
                .map(|(label, ratio)| ((*label).to_owned(), *ratio))
                .unwrap_or_else(|| (format!("{average_ratio:.2}:1"), average_ratio));
            CommonAspectRatio { label, ratio }
        })
}

fn rounded_even(value: f64) -> u32 {
    ((value / 2.0).round() * 2.0).max(2.0) as u32
}

#[cfg(test)]
mod tests {
    use super::{ScaleController, ScaleFileScan, common_aspect_ratio};
    use crate::model::VideoDimensions;

    #[test]
    fn accepts_slight_variations_in_a_shared_aspect_ratio() {
        let files = vec![
            ScaleFileScan {
                file_name: "one.mkv".to_owned(),
                dimensions: Some(VideoDimensions {
                    width: 1_920,
                    height: 1_080,
                    display_aspect_ratio: 16.0 / 9.0,
                }),
                error: None,
            },
            ScaleFileScan {
                file_name: "two.mkv".to_owned(),
                dimensions: Some(VideoDimensions {
                    width: 1_918,
                    height: 1_080,
                    display_aspect_ratio: 1_918.0 / 1_080.0,
                }),
                error: None,
            },
        ];

        let aspect_ratio = common_aspect_ratio(&files).expect("common aspect ratio");

        assert_eq!(aspect_ratio.label, "16:9");
        assert_eq!(
            aspect_ratio.resolution_presets()[1],
            super::ScaleResolution {
                width: 1_920,
                height: 1_080,
            }
        );
    }

    #[test]
    fn writes_an_even_percentage_scale_filter() {
        let controller = ScaleController {
            retain_current_resolution: false,
            scale_percentage: 50,
            selected_resolution: None,
            scan_state: super::ScaleScanState::Idle,
            scan_receiver: None,
        };

        assert_eq!(
            controller.apply_ffmpeg_args(Vec::new()),
            vec!["-vf", "scale=trunc(iw*50/100/2)*2:trunc(ih*50/100/2)*2"]
        );
    }
}
