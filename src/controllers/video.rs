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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoTune {
    None,
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
        replace_or_remove_argument(&mut arguments, "-tune", settings.tune.ffmpeg_name());
        arguments
    }
}

impl VideoSettings {
    fn from_profile(profile: &Profile) -> Option<Self> {
        let encoder = match argument_value(&profile.ffmpeg_args, "-c:v")? {
            "libx265" => VideoEncoder::Libx265,
            "hevc_nvenc" => VideoEncoder::HevcNvenc,
            "libx264" => VideoEncoder::Libx264,
            _ => return None,
        };

        let quality_argument = match encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => "-crf",
            VideoEncoder::HevcNvenc => "-cq",
        };
        let quality = argument_value(&profile.ffmpeg_args, quality_argument)
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|value| (0..=51).contains(value))
            .unwrap_or(23);
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
            VideoEncoder::HevcNvenc => argument_value(&profile.ffmpeg_args, "-preset")
                .and_then(|value| value.strip_prefix('p'))
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|value| (1..=7).contains(value))
                .unwrap_or(4),
        };
        let tune = argument_value(&profile.ffmpeg_args, "-tune")
            .and_then(VideoTune::from_ffmpeg_name)
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
        }
    }

    pub fn quality_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => "Quality (CRF)",
            VideoEncoder::HevcNvenc => "Quality (CQ)",
        }
    }

    pub fn quality_range(&self) -> RangeInclusive<u8> {
        0..=51
    }

    pub fn speed_range(&self) -> RangeInclusive<u8> {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => 0..=9,
            VideoEncoder::HevcNvenc => 1..=7,
        }
    }

    pub fn speed_label(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => {
                X26X_SPEED_PRESETS[self.speed as usize]
            }
            VideoEncoder::HevcNvenc => match self.speed {
                1 => "p1 (fastest)",
                2 => "p2",
                3 => "p3",
                4 => "p4",
                5 => "p5",
                6 => "p6",
                7 => "p7 (slowest / highest quality)",
                _ => "p4",
            },
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
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => "-crf",
            VideoEncoder::HevcNvenc => "-cq",
        }
    }

    fn speed_argument(&self) -> &'static str {
        match self.encoder {
            VideoEncoder::Libx265 | VideoEncoder::Libx264 => {
                X26X_SPEED_PRESETS[self.speed as usize]
            }
            VideoEncoder::HevcNvenc => match self.speed {
                1 => "p1",
                2 => "p2",
                3 => "p3",
                4 => "p4",
                5 => "p5",
                6 => "p6",
                7 => "p7",
                _ => "p4",
            },
        }
    }
}

impl VideoEncoder {
    fn tune_options(self) -> &'static [VideoTune] {
        match self {
            Self::Libx265 => &X265_TUNES,
            Self::Libx264 => &X264_TUNES,
            Self::HevcNvenc => &[],
        }
    }
}

fn argument_value<'a>(arguments: &'a [String], argument: &str) -> Option<&'a str> {
    arguments
        .windows(2)
        .find(|pair| pair[0] == argument)
        .map(|pair| pair[1].as_str())
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

#[cfg(test)]
mod tests {
    use super::{VideoEncoder, VideoSettings, VideoTune};
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
}
