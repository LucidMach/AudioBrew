/// What the decoder found in a file.
#[derive(Debug, Clone, Copy)]
pub struct FileInfo {
    pub frames: u64,
    pub channels: u16,
    pub sample_rate: u32,
}

/// How to get from the file's channels to a stereo Channel pair (L/R only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChannelPlan {
    AsIs,
    DuplicateMono,
    /// Surround: keep the front left and right.
    FirstTwo,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoadPlan {
    pub channels: ChannelPlan,
    /// Device rate divided by file rate.
    pub resample_ratio: f64,
    pub output_frames: u64,
}

impl LoadPlan {
    pub fn needs_resample(&self) -> bool {
        self.resample_ratio != 1.0
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum LoadError {
    Empty,
    NoChannels,
    InvalidSampleRate,
}

/// Decides how a decoded file becomes stereo at the device rate, or why it cannot load.
pub fn plan_load(info: FileInfo, device_rate: u32) -> Result<LoadPlan, LoadError> {
    if info.frames == 0 {
        return Err(LoadError::Empty);
    }
    if info.channels == 0 {
        return Err(LoadError::NoChannels);
    }
    if info.sample_rate == 0 || device_rate == 0 {
        return Err(LoadError::InvalidSampleRate);
    }
    let channels = match info.channels {
        1 => ChannelPlan::DuplicateMono,
        2 => ChannelPlan::AsIs,
        _ => ChannelPlan::FirstTwo,
    };
    let ratio = device_rate as f64 / info.sample_rate as f64;
    Ok(LoadPlan {
        channels,
        resample_ratio: ratio,
        output_frames: (info.frames as f64 * ratio).round() as u64,
    })
}

#[derive(Debug, Clone, Copy)]
pub struct DroppedFile {
    pub is_audio: bool,
}

/// Where one dropped file goes. `new_lane` means `lane` is past the existing lanes and
/// must be added first.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DropAssignment {
    pub file: usize,
    pub lane: usize,
    pub new_lane: bool,
}

/// Assigns dropped audio files to lanes. A drop onto a lane puts the first audio file there
/// (replacing what is loaded); the rest, or all of them when untargeted, fill empty lanes in
/// order and then new lanes. Non-audio files are skipped but keep their indices.
pub fn plan_drop(
    files: &[DroppedFile],
    occupied: &[bool],
    target: Option<usize>,
) -> Vec<DropAssignment> {
    let target = target.filter(|&t| t < occupied.len());
    let mut audio = files
        .iter()
        .enumerate()
        .filter(|(_, f)| f.is_audio)
        .map(|(i, _)| i);
    let mut plan = Vec::new();

    if let Some(lane) = target
        && let Some(file) = audio.next()
    {
        plan.push(DropAssignment {
            file,
            lane,
            new_lane: false,
        });
    }
    let mut free = (0..occupied.len()).filter(|&l| !occupied[l] && Some(l) != target);
    let mut next_new = occupied.len();
    for file in audio {
        plan.push(match free.next() {
            Some(lane) => DropAssignment {
                file,
                lane,
                new_lane: false,
            },
            None => {
                next_new += 1;
                DropAssignment {
                    file,
                    lane: next_new - 1,
                    new_lane: true,
                }
            }
        });
    }
    plan
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(frames: u64, channels: u16, sample_rate: u32) -> FileInfo {
        FileInfo {
            frames,
            channels,
            sample_rate,
        }
    }

    #[test]
    fn a_stereo_file_at_the_device_rate_needs_no_conversion() {
        let plan = plan_load(info(44_100, 2, 44_100), 44_100).unwrap();
        assert_eq!(plan.channels, ChannelPlan::AsIs);
        assert!(!plan.needs_resample());
        assert_eq!(plan.output_frames, 44_100);
    }

    #[test]
    fn a_mono_file_is_duplicated_to_left_and_right() {
        let plan = plan_load(info(1_000, 1, 48_000), 48_000).unwrap();
        assert_eq!(plan.channels, ChannelPlan::DuplicateMono);
    }

    #[test]
    fn surround_files_keep_only_the_front_left_and_right() {
        let plan = plan_load(info(1_000, 6, 48_000), 48_000).unwrap();
        assert_eq!(plan.channels, ChannelPlan::FirstTwo);
    }

    #[test]
    fn a_22_05k_file_is_resampled_up_to_a_48k_device() {
        let plan = plan_load(info(22_050, 2, 22_050), 48_000).unwrap();
        assert!(plan.needs_resample());
        assert!((plan.resample_ratio - 48_000.0 / 22_050.0).abs() < 1e-12);
        // One second of audio stays one second: 22_050 frames become 48_000.
        assert_eq!(plan.output_frames, 48_000);
    }

    #[test]
    fn a_96k_file_is_resampled_down_to_a_44_1k_device() {
        let plan = plan_load(info(96_000, 2, 96_000), 44_100).unwrap();
        assert_eq!(plan.output_frames, 44_100);
    }

    #[test]
    fn output_length_is_rounded_to_the_nearest_frame() {
        // 10 frames at 44_100 -> 48_000 is 10.884 frames -> 11.
        assert_eq!(
            plan_load(info(10, 2, 44_100), 48_000)
                .unwrap()
                .output_frames,
            11
        );
    }

    #[test]
    fn a_file_with_no_audio_is_an_empty_error_and_a_lane_stays_empty() {
        assert_eq!(plan_load(info(0, 2, 44_100), 44_100), Err(LoadError::Empty));
    }

    #[test]
    fn a_file_with_no_channels_is_an_error() {
        assert_eq!(
            plan_load(info(100, 0, 44_100), 44_100),
            Err(LoadError::NoChannels)
        );
    }

    #[test]
    fn a_zero_sample_rate_on_either_side_is_an_error() {
        assert_eq!(
            plan_load(info(100, 2, 0), 44_100),
            Err(LoadError::InvalidSampleRate)
        );
        assert_eq!(
            plan_load(info(100, 2, 44_100), 0),
            Err(LoadError::InvalidSampleRate)
        );
    }

    fn file(is_audio: bool) -> DroppedFile {
        DroppedFile { is_audio }
    }

    fn a(file: usize, lane: usize, new_lane: bool) -> DropAssignment {
        DropAssignment {
            file,
            lane,
            new_lane,
        }
    }

    #[test]
    fn dropping_two_files_fills_the_two_empty_lanes_in_order() {
        let plan = plan_drop(&[file(true), file(true)], &[false, false], None);
        assert_eq!(plan, vec![a(0, 0, false), a(1, 1, false)]);
    }

    #[test]
    fn extra_files_get_new_lanes_after_the_existing_ones() {
        let plan = plan_drop(
            &[file(true), file(true), file(true), file(true)],
            &[false, false],
            None,
        );
        assert_eq!(
            plan,
            vec![a(0, 0, false), a(1, 1, false), a(2, 2, true), a(3, 3, true)]
        );
    }

    #[test]
    fn files_that_are_not_audio_are_skipped_but_keep_their_indices() {
        let plan = plan_drop(
            &[file(false), file(true), file(false), file(true)],
            &[false, false],
            None,
        );
        assert_eq!(plan, vec![a(1, 0, false), a(3, 1, false)]);
    }

    #[test]
    fn occupied_lanes_are_left_alone_when_no_lane_is_targeted() {
        let plan = plan_drop(&[file(true), file(true)], &[true, false], None);
        assert_eq!(plan, vec![a(0, 1, false), a(1, 2, true)]);
    }

    #[test]
    fn a_drop_onto_a_lane_puts_the_first_file_there_replacing_what_was_loaded() {
        let plan = plan_drop(&[file(true), file(true)], &[true, false, true], Some(0));
        assert_eq!(plan, vec![a(0, 0, false), a(1, 1, false)]);
    }

    #[test]
    fn a_drop_onto_a_lane_that_does_not_exist_is_treated_as_untargeted() {
        let plan = plan_drop(&[file(true)], &[false, false], Some(9));
        assert_eq!(plan, vec![a(0, 0, false)]);
    }

    #[test]
    fn a_non_audio_first_file_does_not_take_the_targeted_lane() {
        let plan = plan_drop(&[file(false), file(true)], &[false, false], Some(1));
        assert_eq!(plan, vec![a(1, 1, false)]);
    }

    #[test]
    fn dropping_nothing_playable_changes_nothing() {
        assert!(plan_drop(&[file(false)], &[false, false], None).is_empty());
        assert!(plan_drop(&[], &[false, false], Some(0)).is_empty());
    }
}
