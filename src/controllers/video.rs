use crate::model::Profile;
use std::ops::RangeInclusive;

const X265_SPEED_PRESETS: [&str; 10] = [
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

#[derive(Default)]
pub struct VideoController {
    h265_settings: Option<H265Settings>,
}

#[derive(Clone, Debug)]
pub struct H265Settings {
    encoder: H265Encoder,
    pub quality: u8,
    pub speed: u8,
}

#[derive(Clone, Copy, Debug)]
enum H265Encoder {
    Libx265,
    HevcNvenc,
}

impl VideoController {
    pub fn load_profile(&mut self, profile: &Profile) {
        self.h265_settings = H265Settings::from_profile(profile);
    }

    pub fn h265_settings_mut(&mut self) -> Option<&mut H265Settings> {
        self.h265_settings.as_mut()
    }

    pub fn effective_ffmpeg_args(&self, profile: &Profile) -> Vec<String> {
        let Some(settings) = &self.h265_settings else {
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
        arguments
    }
}

impl H265Settings {
    fn from_profile(profile: &Profile) -> Option<Self> {
        let encoder = match argument_value(&profile.ffmpeg_args, "-c:v")? {
            "libx265" => H265Encoder::Libx265,
            "hevc_nvenc" => H265Encoder::HevcNvenc,
            _ => return None,
        };

        let quality_argument = match encoder {
            H265Encoder::Libx265 => "-crf",
            H265Encoder::HevcNvenc => "-cq",
        };
        let quality = argument_value(&profile.ffmpeg_args, quality_argument)
            .and_then(|value| value.parse::<u8>().ok())
            .filter(|value| (0..=51).contains(value))
            .unwrap_or(23);
        let speed = match encoder {
            H265Encoder::Libx265 => argument_value(&profile.ffmpeg_args, "-preset")
                .and_then(|value| {
                    X265_SPEED_PRESETS
                        .iter()
                        .position(|preset| preset == &value)
                })
                .map(|index| index as u8)
                .unwrap_or(5),
            H265Encoder::HevcNvenc => argument_value(&profile.ffmpeg_args, "-preset")
                .and_then(|value| value.strip_prefix('p'))
                .and_then(|value| value.parse::<u8>().ok())
                .filter(|value| (1..=7).contains(value))
                .unwrap_or(4),
        };

        Some(Self {
            encoder,
            quality,
            speed,
        })
    }

    pub fn encoder_label(&self) -> &'static str {
        match self.encoder {
            H265Encoder::Libx265 => "H.265 / x265 (software)",
            H265Encoder::HevcNvenc => "H.265 / NVENC (hardware)",
        }
    }

    pub fn quality_label(&self) -> &'static str {
        match self.encoder {
            H265Encoder::Libx265 => "Quality (CRF)",
            H265Encoder::HevcNvenc => "Quality (CQ)",
        }
    }

    pub fn quality_range(&self) -> RangeInclusive<u8> {
        0..=51
    }

    pub fn speed_range(&self) -> RangeInclusive<u8> {
        match self.encoder {
            H265Encoder::Libx265 => 0..=9,
            H265Encoder::HevcNvenc => 1..=7,
        }
    }

    pub fn speed_label(&self) -> &'static str {
        match self.encoder {
            H265Encoder::Libx265 => X265_SPEED_PRESETS[self.speed as usize],
            H265Encoder::HevcNvenc => match self.speed {
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

    fn quality_argument(&self) -> &'static str {
        match self.encoder {
            H265Encoder::Libx265 => "-crf",
            H265Encoder::HevcNvenc => "-cq",
        }
    }

    fn speed_argument(&self) -> &'static str {
        match self.encoder {
            H265Encoder::Libx265 => X265_SPEED_PRESETS[self.speed as usize],
            H265Encoder::HevcNvenc => match self.speed {
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

#[cfg(test)]
mod tests {
    use super::{H265Encoder, H265Settings};
    use crate::model::Profile;

    #[test]
    fn loads_nvenc_quality_and_speed_from_a_profile() {
        let profile = Profile {
            ffmpeg_args: ["-c:v", "hevc_nvenc", "-preset", "p7", "-cq", "23"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
        };

        let settings = H265Settings::from_profile(&profile).expect("H.265 settings");

        assert!(matches!(settings.encoder, H265Encoder::HevcNvenc));
        assert_eq!(settings.quality, 23);
        assert_eq!(settings.speed, 7);
    }
}
