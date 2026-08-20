use crate::model::Profile;
use std::ops::RangeInclusive;

const X26X_SPEED_PRESETS: [&str; 10] = [
    "ultrafast",
    "superfast",
    "veryfast",
    "faster",
    "fast",
    "medium",
    "slow",
    "slower",
    "veryslow",
    "placebo",
];

const SVT_AV1_SPEED_PRESETS: [&str; 14] = [
    "0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11", "12", "13",
];

const X265_TUNES: [VideoTune; 7] = [
    VideoTune::None,
    VideoTune::Animation,
    VideoTune::Grain,
    VideoTune::Psnr,
    VideoTune::Ssim,
    VideoTune::FastDecode,
    VideoTune::ZeroLatency,
];

const X264_TUNES: [VideoTune; 9] = [
    VideoTune::None,
    VideoTune::Film,
    VideoTune::Animation,
    VideoTune::Grain,
    VideoTune::StillImage,
    VideoTune::Psnr,
    VideoTune::Ssim,
    VideoTune::FastDecode,
    VideoTune::ZeroLatency,
];

const SVT_AV1_TUNES: [VideoTune; 4] = [
    VideoTune::None,
    VideoTune::VisualQuality,
    VideoTune::Psnr,
    VideoTune::Ssim,
];

#[derive(Default)]
pub struct VideoController {
    settings: Option<VideoSettings>,
}

#[derive(Clone, Debug)]
pub struct VideoSettings {
    encoder: VideoEncoder,
    pub quality: u8,
    pub speed: u8,
    pub tune: VideoTune,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum VideoEncoder {
    Libx265,
    HevcNvenc,
    Libx264,
    LibsvtAv1,
    Av1Nvenc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoTune {
    None,
    VisualQuality,
    Film,
    Animation,
    Grain,
    StillImage,
    Psnr,
    Ssim,
    FastDecode,
    ZeroLatency,
}

impl VideoTune {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::VisualQuality => "Visual quality (VQ)",
            Self::Film => "Film",
            Self::Animation => "Animation",
            Self::Grain => "Grain",
            Self::StillImage => "Still image",
            Self::Psnr => "PSNR",
            Self::Ssim => "SSIM",
            Self::FastDecode => "Fast decode",
            Self::ZeroLatency => "Zero latency",
        }
    }

    fn ffmpeg_name(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::VisualQuality => None,
            Self::Film => Some("film"),
            Self::Animation => Some("animation"),
            Self::Grain => Some("grain"),
            Self::StillImage => Some("stillimage"),
            Self::Psnr => Some("psnr"),
            Self::Ssim => Some("ssim"),
            Self::FastDecode => Some("fastdecode"),
            Self::ZeroLatency => Some("zerolatency"),
        }
    }

    fn from_ffmpeg_name(value: &str) -> Option<Self> {
        match value {
            "film" => Some(Self::Film),
            "animation" => Some(Self::Animation),
            "grain" => Some(Self::Grain),
            "stillimage" => Some(Self::StillImage),
            "psnr" => Some(Self::Psnr),
            "ssim" => Some(Self::Ssim),
            "fastdecode" => Some(Self::FastDecode),
            "zerolatency" => Some(Self::ZeroLatency),
            _ => None,
        }
    }

    fn svt_av1_value(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::VisualQuality => Some("0"),
            Self::Psnr => Some("1"),
            Self::Ssim => Some("2"),
            _ => None,
        }
    }

    fn from_svt_av1_value(value: &str) -> Option<Self> {
        match value {
            "0" => Some(Self::VisualQuality),
            "1" => Some(Self::Psnr),
            "2" => Some(Self::Ssim),
            _ => None,
        }
    }
}

impl VideoController {
    pub fn load_profile(&mut self, profile: &Profile) {
        self.settings = VideoSettings::from_profile(profile);
    }

    pub fn settings_mut(&mut self) -> Option<&mut VideoSettings> {
        self.settings.as_mut()
    }

    pub fn effective_ffmpeg_args(&self, profile: &Profile) -> Vec<String> {
        let Some(settings) = &self.settings else {
            return profile.ffmpeg_args.clone();
        };

        let mut arguments = profile.ffmpeg_args.clone();
        replace_or_append_argument(
            &mut arguments,
            settings.quality_argument(),
            settings.quality.to_string(),
        );
        replace_or_append_argument(
            &mut arguments,
            "-preset",
            settings.speed_argument().to_owned(),
        );
        match settings.encoder {
            VideoEncoder::LibsvtAv1 => replace_or_remove_dictionary_value(
                &mut arguments,
                "-svtav1-params",
                "tune",
                settings.tune.svt_av1_value(),
            ),
            _ => replace_or_remove_argument(&mut arguments, "-tune", settings.tune.ffmpeg_name()),
        }
        arguments
    }
}

impl VideoSettings {
    fn from_profile(profile: &Profile) -> Option<Self> {
        let encoder = match argument_value(&profile.ffmpeg_args, "-c:v")? {
            "libx265" => VideoEncoder::Libx265,
            "hevc_nvenc" => VideoEncoder::HevcNvenc,
            "libx264" => VideoEncoder::Libx264,
            "libsvtav1" => VideoEncoder::LibsvtAv1,
            "av1_nvenc" => VideoEncoder::Av1Nvenc,
            _ => return None,
        };

        let quality_argument = match encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 | VideoEncoder::LibsvtAv1 => "-crf",
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => "-cq",
        };
        let quality = argument_value(&profile.ffmpeg_args, quality_argument)
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|value| (0..=encoder.quality_max()).contains(value))
            .unwrap_or_else(|| match encoder {
                VideoEncoder::LibsvtAv1 => 32,
                _ => 23,
            });
        let speed = match encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => {
                argument_value(&profile.ffmpeg_args, "-preset")
                    .and_then(|value| {
                        X26X_SPEED_PRESETS
                            .iter()
                            .position(|preset| preset == &value)
                    })
                    .map(|index| index as u8)
                    .unwrap_or(5)
            }
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => {
                argument_value(&profile.ffmpeg_args, "-preset")
                    .and_then(|value| value.strip_prefix('p'))
                    .and_then(|value| value.parse::<u8>().ok())
                    .filter(|value| (1..=7).contains(value))
                    .unwrap_or(4)
            }
            VideoEncoder::LibsvtAv1 => argument_value(&profile.ffmpeg_args, "-preset")
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|value| (0..=13).contains(value))
                .unwrap_or(6),
        };
        let tune = match encoder {
            VideoEncoder::LibsvtAv1 => {
                dictionary_value(&profile.ffmpeg_args, "-svtav1-params", "tune")
                    .and_then(VideoTune::from_svt_av1_value)
            }
            _ => {
                argument_value(&profile.ffmpeg_args, "-tune").and_then(VideoTune::from_ffmpeg_name)
            }
        };
        let tune = tune
            .filter(|tune| encoder.tune_options().contains(tune))
            .unwrap_or(VideoTune::None);

        Some(Self {
            encoder,
            quality,
            speed,
            tune,
        })
    }

    pub fn encoder_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 => "H.265 / x265 (software)",
            VideoEncoder::HevcNvenc => "H.265 / NVENC (hardware)",
            VideoEncoder::Libx264 => "H.264 / x264 (software)",
            VideoEncoder::LibsvtAv1 => "AV1 / SVT-AV1 (software)",
            VideoEncoder::Av1Nvenc => "AV1 / NVENC (hardware)",
        }
    }

    pub fn quality_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 | VideoEncoder::LibsvtAv1 => {
                "Quality (CRF)"
            }
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => "Quality (CQ)",
        }
    }

    pub fn quality_range(&self) -> RangeInclusive<u8> {
        0..=self.encoder.quality_max()
    }

    pub fn speed_range(&self) -> RangeInclusive<u8> {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => 0..=9,
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => 1..=7,
            VideoEncoder::LibsvtAv1 => 0..=13,
        }
    }

    pub fn speed_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => {
                X26X_SPEED_PRESETS[self.speed as usize]
            }
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => match self.speed {
                1 => "p1 (fastest)",
                2 => "p2",
                3 => "p3",
                4 => "p4",
                5 => "p5",
                6 => "p6",
                7 => "p7 (slowest / highest quality)",
                _ => "p4",
            },
            VideoEncoder::LibsvtAv1 => SVT_AV1_SPEED_PRESETS[self.speed as usize],
        }
    }

    pub fn speed_left_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::LibsvtAv1 => "Slower / higher quality",
            _ => "Faster",
        }
    }

    pub fn speed_right_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::LibsvtAv1 => "Faster",
            _ => "Slower / higher quality",
        }
    }

    pub fn tune_options(&self) -> &'static [VideoTune] {
        self.encoder.tune_options()
    }

    pub fn supports_tunes(&self) -> bool {
        !self.tune_options().is_empty()
    }

    fn quality_argument(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 | VideoEncoder::LibsvtAv1 => "-crf",
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => "-cq",
        }
    }

    fn speed_argument(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => {
                X26X_SPEED_PRESETS[self.speed as usize]
            }
            VideoEncoder::HevcNvenc | VideoEncoder::Av1Nvenc => match self.speed {
                1 => "p1",
                2 => "p2",
                3 => "p3",
                4 => "p4",
                5 => "p5",
                6 => "p6",
                7 => "p7",
                _ => "p4",
            },
            VideoEncoder::LibsvtAv1 => SVT_AV1_SPEED_PRESETS[self.speed as usize],
        }
    }
}

impl VideoEncoder {
    fn quality_max(self) -> u8 {
        match self {
            Self::LibsvtAv1 => 63,
            _ => 51,
        }
    }

    fn tune_options(self) -> &'static [VideoTune] {
        match self {
            Self::Libx265 => &X265_TUNES,
            Self::Libx264 => &X264_TUNES,
            Self::LibsvtAv1 => &SVT_AV1_TUNES,
            Self::HevcNvenc | Self::Av1Nvenc => &[],
        }
    }
}

fn argument_value<'a>(arguments: &'a [String], argument: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == argument)
        .map(|pair| pair[1].as_str())
}

fn dictionary_value<'a>(arguments: &'a [String], argument: &str, key: &str) -> Option<&'a str> {
    argument_value(arguments, argument)?
        .split(':')
        .find_map(|entry| {
            let (entry_key, value) = entry.split_once('=')?;
            (entry_key == key).then_some(value)
        })
}

fn replace_or_append_argument(arguments: &mut Vec<String>, argument: &str, value: String) {
    if let Some(index) = arguments.iter().position(|existing| existing == argument) {
        if let Some(existing_value) = arguments.get_mut(index + 1) {
            *existing_value = value;
            return;
        }
    }

    arguments.push(argument.to_owned());
    arguments.push(value);
}

fn replace_or_remove_argument(arguments: &mut Vec<String>, argument: &str, value: Option<&str>) {
    let existing_index = arguments.iter().position(|existing| existing == argument);
    match (existing_index, value) {
        (Some(index), Some(value)) => {
            if let Some(existing_value) = arguments.get_mut(index + 1) {
                *existing_value = value.to_owned();
            }
        }
        (Some(index), None) => {
            arguments.drain(index..(index + 2).min(arguments.len()));
        }
        (None, Some(value)) => {
            arguments.push(argument.to_owned());
            arguments.push(value.to_owned());
        }
        (None, None) => {}
    }
}

fn replace_or_remove_dictionary_value(
    arguments: &mut Vec<String>,
    argument: &str,
    key: &str,
    value: Option<&str>,
) {
    let existing_index = arguments.iter().position(|existing| existing == argument);
    let mut entries = existing_index
        .and_then(|index| arguments.get(index + 1))
        .map(|dictionary| {
            dictionary
                .split(':')
                .filter(|entry| {
                    entry
                        .split_once('=')
                        .is_none_or(|(entry_key, _)| entry_key != key)
                })
                .map(str::to_owned)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    if let Some(value) = value {
        entries.push(format!("{key}={value}"));
    }

    match (existing_index, entries.is_empty()) {
        (Some(index), true) => {
            arguments.drain(index..(index + 2).min(arguments.len()));
        }
        (Some(index), false) => {
            if let Some(existing_value) = arguments.get_mut(index + 1) {
                *existing_value = entries.join(":");
            }
        }
        (None, false) => {
            arguments.push(argument.to_owned());
            arguments.push(entries.join(":"));
        }
        (None, true) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::{
        VideoController, VideoEncoder, VideoSettings, VideoTune, argument_value, dictionary_value,
    };
    use crate::model::Profile;

    #[test]
    fn loads_nvenc_quality_and_speed_from_a_profile() {
        let profile = Profile {
            ffmpeg_args: ["-c:v", "hevc_nvenc", "-preset", "p7", "-cq", "23"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };

        let settings = VideoSettings::from_profile(&profile).expect("H.265 settings");

        assert!(matches!(settings.encoder, VideoEncoder::HevcNvenc));
        assert_eq!(settings.quality, 23);
        assert_eq!(settings.speed, 7);
    }

    #[test]
    fn loads_an_x264_tune_from_a_profile() {
        let profile = Profile {
            ffmpeg_args: [
                "-c:v", "libx264", "-preset", "veryslow", "-crf", "16", "-tune", "film",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        };

        let settings = VideoSettings::from_profile(&profile).expect("H.264 settings");

        assert!(matches!(settings.encoder, VideoEncoder::Libx264));
        assert_eq!(settings.quality, 16);
        assert_eq!(settings.speed, 8);
        assert_eq!(settings.tune, VideoTune::Film);
    }

    #[test]
    fn loads_and_updates_svt_av1_settings() {
        let profile = Profile {
            ffmpeg_args: [
                "-c:v",
                "libsvtav1",
                "-preset",
                "6",
                "-crf",
                "32",
                "-svtav1-params",
                "film-grain=8:tune=0",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
        };
        let mut controller = VideoController::default();
        controller.load_profile(&profile);
        let settings = controller.settings_mut().expect("AV1 settings");
        assert!(matches!(settings.encoder, VideoEncoder::LibsvtAv1));
        assert_eq!(settings.quality, 32);
        assert_eq!(settings.speed, 6);
        assert_eq!(settings.tune, VideoTune::VisualQuality);
        settings.quality = 35;
        settings.speed = 8;
        settings.tune = VideoTune::Ssim;

        let arguments = controller.effective_ffmpeg_args(&profile);
        assert_eq!(argument_value(&arguments, "-crf"), Some("35"));
        assert_eq!(argument_value(&arguments, "-preset"), Some("8"));
        assert_eq!(
            dictionary_value(&arguments, "-svtav1-params", "film-grain"),
            Some("8")
        );
        assert_eq!(
            dictionary_value(&arguments, "-svtav1-params", "tune"),
            Some("2")
        );
    }
}
