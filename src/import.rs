use std::io::Cursor;
use std::path::Path;

use rubato::audioadapter_buffers::direct::InterleavedSlice;
use rubato::{Fft, FixedSync, Resampler};
use symphonia::core::codecs::audio::AudioDecoderOptions;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

use crate::load::{FileInfo, LoadError, plan_load};

#[derive(Debug, PartialEq)]
pub enum ImportError {
    /// The bytes are not audio we can decode, or reading failed.
    Decode(String),
    /// Decoded fine, but the file cannot become a Track (see `load::plan_load`).
    Plan(LoadError),
    Resample(String),
}

/// Audio exactly as the file holds it: interleaved f32 in -1.0..=1.0.
#[derive(Debug, Clone, PartialEq)]
pub struct Decoded {
    pub samples: Vec<f32>,
    pub channels: usize,
    pub sample_rate: u32,
}

/// Stereo (L/R interleaved) audio at some sample rate: the only shape the engine uses.
#[derive(Debug, Clone, PartialEq)]
pub struct Stereo {
    pub samples: Vec<f32>,
    pub sample_rate: u32,
}

impl Stereo {
    pub fn frames(&self) -> usize {
        self.samples.len() / 2
    }
}

/// Decodes a whole audio file held in memory. `extension` (e.g. "mp3") is only a hint.
pub fn decode(bytes: Vec<u8>, extension: Option<&str>) -> Result<Decoded, ImportError> {
    let fail = |e: symphonia::core::errors::Error| ImportError::Decode(e.to_string());

    let stream = MediaSourceStream::new(Box::new(Cursor::new(bytes)), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = extension {
        hint.with_extension(ext);
    }
    let mut format = symphonia::default::get_probe()
        .probe(
            &hint,
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(fail)?;

    let track = format
        .default_track(TrackType::Audio)
        .ok_or_else(|| ImportError::Decode("no audio track".into()))?;
    let params = track
        .codec_params
        .as_ref()
        .and_then(|p| p.audio())
        .ok_or_else(|| ImportError::Decode("no audio parameters".into()))?;
    let mut channels = params.channels.as_ref().map_or(0, |c| c.count());
    let mut sample_rate = params.sample_rate.unwrap_or(0);
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &AudioDecoderOptions::default())
        .map_err(fail)?;
    let track_id = track.id;

    let mut samples: Vec<f32> = Vec::new();
    let mut packet_samples: Vec<f32> = Vec::new();
    // A damaged or truncated tail ends the audio; it does not discard what decoded fine.
    while let Ok(Some(packet)) = format.next_packet() {
        if packet.track_id != track_id {
            continue;
        }
        let Ok(buffer) = decoder.decode(&packet) else {
            continue;
        };
        channels = buffer.spec().channels().count();
        sample_rate = buffer.spec().rate();
        packet_samples.resize(buffer.samples_interleaved(), 0.0);
        buffer.copy_to_slice_interleaved(&mut packet_samples);
        samples.extend_from_slice(&packet_samples);
    }
    Ok(Decoded {
        samples,
        channels,
        sample_rate,
    })
}

/// Reads and decodes a file from disk.
pub fn decode_file(path: &Path) -> Result<Decoded, ImportError> {
    let bytes = std::fs::read(path).map_err(|e| ImportError::Decode(e.to_string()))?;
    decode(bytes, path.extension().and_then(|e| e.to_str()))
}

/// Mono is duplicated to L and R, stereo is kept, surround keeps its first two channels.
pub fn to_stereo(decoded: &Decoded) -> Stereo {
    let n = decoded.channels;
    let samples = match n {
        0 => Vec::new(),
        1 => decoded.samples.iter().flat_map(|&s| [s, s]).collect(),
        2 => decoded.samples.clone(),
        _ => decoded
            .samples
            .chunks_exact(n)
            .flat_map(|f| [f[0], f[1]])
            .collect(),
    };
    Stereo {
        samples,
        sample_rate: decoded.sample_rate,
    }
}

/// Converts to `to_rate`, returning exactly `output_frames` frames.
pub fn resample(
    stereo: &Stereo,
    to_rate: u32,
    output_frames: usize,
) -> Result<Stereo, ImportError> {
    let fail = |e: String| ImportError::Resample(e);
    let frames = stereo.frames();
    let input =
        InterleavedSlice::new(&stereo.samples, 2, frames).map_err(|e| fail(e.to_string()))?;
    let mut resampler = Fft::<f32>::new(
        stereo.sample_rate as usize,
        to_rate as usize,
        1024,
        2,
        FixedSync::Both,
    )
    .map_err(|e| fail(e.to_string()))?;
    let output = resampler
        .process_all(&input, frames, None)
        .map_err(|e| fail(e.to_string()))?;
    let mut samples = output.take_data();
    samples.resize(output_frames * 2, 0.0);
    Ok(Stereo {
        samples,
        sample_rate: to_rate,
    })
}

/// The whole pipeline: decode a file, make it stereo, and convert it to the device rate.
pub fn import(
    bytes: Vec<u8>,
    extension: Option<&str>,
    device_rate: u32,
) -> Result<Stereo, ImportError> {
    let decoded = decode(bytes, extension)?;
    let channels = decoded.channels.max(1);
    let info = FileInfo {
        frames: (decoded.samples.len() / channels) as u64,
        channels: decoded.channels as u16,
        sample_rate: decoded.sample_rate,
    };
    let plan = plan_load(info, device_rate).map_err(ImportError::Plan)?;
    let stereo = to_stereo(&decoded);
    if plan.needs_resample() {
        resample(&stereo, device_rate, plan.output_frames as usize)
    } else {
        Ok(stereo)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::load::LoadError;

    /// A canonical RIFF/WAVE file. `format` is 1 (integer PCM) or 3 (IEEE float).
    fn wav(format: u16, channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block = channels * bits / 8;
        let mut out = Vec::new();
        out.extend(b"RIFF");
        out.extend((36 + data.len() as u32).to_le_bytes());
        out.extend(b"WAVEfmt ");
        out.extend(16u32.to_le_bytes());
        out.extend(format.to_le_bytes());
        out.extend(channels.to_le_bytes());
        out.extend(rate.to_le_bytes());
        out.extend((rate * block as u32).to_le_bytes());
        out.extend(block.to_le_bytes());
        out.extend(bits.to_le_bytes());
        out.extend(b"data");
        out.extend((data.len() as u32).to_le_bytes());
        out.extend(data);
        out
    }

    fn wav_i16(channels: u16, rate: u32, samples: &[i16]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        wav(1, channels, rate, 16, &data)
    }

    fn wav_f32(channels: u16, rate: u32, samples: &[f32]) -> Vec<u8> {
        let data: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        wav(3, channels, rate, 32, &data)
    }

    #[test]
    fn a_16_bit_stereo_wav_decodes_to_floats_in_minus_one_to_one() {
        // 16384 / 32768 = 0.5, 8192 / 32768 = 0.25
        let d = decode(wav_i16(2, 44_100, &[16384, -16384, 8192, 0]), Some("wav")).unwrap();
        assert_eq!((d.channels, d.sample_rate), (2, 44_100));
        assert_eq!(d.samples, vec![0.5, -0.5, 0.25, 0.0]);
    }

    #[test]
    fn a_mono_wav_decodes_with_one_channel() {
        let d = decode(wav_i16(1, 22_050, &[16384, -32768]), Some("wav")).unwrap();
        assert_eq!((d.channels, d.sample_rate), (1, 22_050));
        assert_eq!(d.samples, vec![0.5, -1.0]);
    }

    #[test]
    fn a_32_bit_float_wav_keeps_its_values_exactly() {
        let d = decode(wav_f32(2, 48_000, &[0.25, -0.75, 1.0, 0.0]), Some("wav")).unwrap();
        assert_eq!(d.samples, vec![0.25, -0.75, 1.0, 0.0]);
    }

    #[test]
    fn decoding_works_without_a_file_extension_hint() {
        let d = decode(wav_i16(1, 44_100, &[16384]), None).unwrap();
        assert_eq!(d.samples, vec![0.5]);
    }

    #[test]
    fn bytes_that_are_not_audio_are_an_error_not_a_panic() {
        assert!(matches!(
            decode(b"this is not audio".to_vec(), None),
            Err(ImportError::Decode(_))
        ));
        assert!(matches!(
            decode(Vec::new(), None),
            Err(ImportError::Decode(_))
        ));
    }

    #[test]
    fn mono_becomes_the_same_signal_in_left_and_right() {
        let d = Decoded {
            samples: vec![0.5, -0.25],
            channels: 1,
            sample_rate: 44_100,
        };
        assert_eq!(to_stereo(&d).samples, vec![0.5, 0.5, -0.25, -0.25]);
    }

    #[test]
    fn stereo_is_left_unchanged() {
        let d = Decoded {
            samples: vec![0.1, 0.2, 0.3, 0.4],
            channels: 2,
            sample_rate: 44_100,
        };
        assert_eq!(to_stereo(&d).samples, vec![0.1, 0.2, 0.3, 0.4]);
    }

    #[test]
    fn surround_keeps_only_the_first_two_channels_of_each_frame() {
        let d = Decoded {
            samples: vec![
                1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 11.0, 12.0,
            ],
            channels: 6,
            sample_rate: 48_000,
        };
        assert_eq!(to_stereo(&d).samples, vec![1.0, 2.0, 7.0, 8.0]);
    }

    #[test]
    fn importing_at_the_device_rate_does_not_touch_the_samples() {
        let bytes = wav_i16(2, 44_100, &[16384, -16384, 8192, 0]);
        let s = import(bytes, Some("wav"), 44_100).unwrap();
        assert_eq!(s.sample_rate, 44_100);
        assert_eq!(s.samples, vec![0.5, -0.5, 0.25, 0.0]);
        assert_eq!(s.frames(), 2);
    }

    #[test]
    fn importing_a_mono_file_gives_stereo() {
        let s = import(wav_i16(1, 44_100, &[16384, 8192]), Some("wav"), 44_100).unwrap();
        assert_eq!(s.samples, vec![0.5, 0.5, 0.25, 0.25]);
    }

    #[test]
    fn a_wav_with_no_audio_is_an_empty_error() {
        let result = import(wav_i16(2, 44_100, &[]), Some("wav"), 44_100);
        assert!(
            matches!(result, Err(ImportError::Plan(LoadError::Empty))),
            "{result:?}"
        );
    }

    fn sine_i16(freq: f64, rate: u32, secs: f64, amplitude: f64) -> Vec<i16> {
        (0..(rate as f64 * secs) as usize)
            .flat_map(|i| {
                let v = (amplitude
                    * (2.0 * std::f64::consts::PI * freq * i as f64 / rate as f64).sin()
                    * 32768.0) as i16;
                [v, v]
            })
            .collect()
    }

    #[test]
    fn a_44_1k_file_imported_on_a_48k_device_keeps_its_pitch_and_duration() {
        // One second of a 1 kHz sine at amplitude 0.5.
        let bytes = wav_i16(2, 44_100, &sine_i16(1_000.0, 44_100, 1.0, 0.5));
        let s = import(bytes, Some("wav"), 48_000).unwrap();

        assert_eq!(s.sample_rate, 48_000);
        assert_eq!(s.frames(), 48_000); // 44_100 frames * 48_000 / 44_100, exactly

        // Left channel, away from the edges where the resampler's filter is settling.
        let left: Vec<f32> = s.samples.chunks(2).map(|f| f[0]).collect();
        let middle = &left[4_800..43_200];
        let rms =
            (middle.iter().map(|v| (*v as f64).powi(2)).sum::<f64>() / middle.len() as f64).sqrt();
        assert!(
            (rms - 0.5 * std::f64::consts::FRAC_1_SQRT_2).abs() < 0.01,
            "rms {rms}"
        );

        // 1 kHz over 38_400 frames at 48 kHz is 800 cycles = 1600 zero crossings.
        let crossings = middle
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        assert!(
            (1_598..=1_602).contains(&crossings),
            "{crossings} zero crossings"
        );
    }

    #[test]
    fn a_96k_file_imported_on_a_44_1k_device_has_the_right_length() {
        let bytes = wav_i16(2, 96_000, &sine_i16(440.0, 96_000, 0.5, 0.5));
        let s = import(bytes, Some("wav"), 44_100).unwrap();
        assert_eq!(s.frames(), 22_050);
    }

    #[test]
    fn a_device_rate_of_zero_is_an_error() {
        let result = import(wav_i16(2, 44_100, &[1, 2]), Some("wav"), 0);
        assert!(matches!(
            result,
            Err(ImportError::Plan(LoadError::InvalidSampleRate))
        ));
    }
}
