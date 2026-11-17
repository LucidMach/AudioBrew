use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, Error, FromSample, Sample, SampleFormat, SizedSample, Stream, StreamConfig};

use crate::engine::{Engine, EngineHandle};

/// The most frames the engine is asked for at once. The callback's scratch buffer is this big
/// and allocated up front; a larger device buffer is filled in several chunks.
pub const MAX_BLOCK_FRAMES: usize = 4096;

/// Fills one device buffer from a stereo processor.
///
/// `processor` fills interleaved stereo `f32` (an `Engine::process`). A mono device gets the
/// average of left and right; more than two channels get left and right on the first two and
/// silence on the rest. Samples that do not make a whole frame stay silent. Does not allocate.
pub fn render_device_buffer<T>(
    mut processor: impl FnMut(&mut [f32]),
    scratch: &mut [f32],
    out: &mut [T],
    channels: usize,
) where
    T: Sample + FromSample<f32>,
{
    out.fill(T::EQUILIBRIUM);
    let max_frames = scratch.len() / 2;
    if channels == 0 || max_frames == 0 {
        return;
    }
    let total_frames = out.len() / channels;
    let mut done = 0;
    while done < total_frames {
        let n = max_frames.min(total_frames - done);
        processor(&mut scratch[..n * 2]);
        for i in 0..n {
            let (l, r) = (scratch[2 * i], scratch[2 * i + 1]);
            let frame = &mut out[(done + i) * channels..][..channels];
            if channels == 1 {
                frame[0] = T::from_sample((l + r) * 0.5);
            } else {
                frame[0] = T::from_sample(l);
                frame[1] = T::from_sample(r);
            }
        }
        done += n;
    }
}

#[derive(Debug)]
pub enum DeviceError {
    NoOutputDevice,
    Config(String),
    UnsupportedFormat(String),
    Stream(String),
}

/// A running output stream. Dropping it stops the sound.
pub struct Output {
    _stream: Stream,
    pub sample_rate: u32,
    pub channels: u16,
}

/// Opens the default output device and starts the engine on it. Returns the handle the UI
/// uses to control playback. The stream runs at the device's own sample rate, which Tracks
/// must be imported at (see `import::import`).
pub fn start(
    max_lanes: usize,
    queue_capacity: usize,
) -> Result<(Output, EngineHandle), DeviceError> {
    let device = cpal::default_host()
        .default_output_device()
        .ok_or(DeviceError::NoOutputDevice)?;
    let supported = device
        .default_output_config()
        .map_err(|e| DeviceError::Config(e.to_string()))?;
    let format = supported.sample_format();
    let config: StreamConfig = supported.into();
    let (engine, handle) = Engine::new(config.sample_rate, max_lanes, queue_capacity);
    let (sample_rate, channels) = (config.sample_rate, config.channels);

    let stream = match format {
        SampleFormat::F32 => build::<f32>(&device, config, engine),
        SampleFormat::I16 => build::<i16>(&device, config, engine),
        SampleFormat::I32 => build::<i32>(&device, config, engine),
        other => return Err(DeviceError::UnsupportedFormat(other.to_string())),
    }
    .map_err(|e| DeviceError::Stream(e.to_string()))?;
    stream
        .play()
        .map_err(|e| DeviceError::Stream(e.to_string()))?;

    Ok((
        Output {
            _stream: stream,
            sample_rate,
            channels,
        },
        handle,
    ))
}

fn build<T>(device: &Device, config: StreamConfig, mut engine: Engine) -> Result<Stream, Error>
where
    T: SizedSample + FromSample<f32>,
{
    let channels = config.channels as usize;
    let mut scratch = vec![0.0f32; MAX_BLOCK_FRAMES * 2];
    device.build_output_stream(
        config,
        move |data: &mut [T], _| {
            render_device_buffer(|block| engine.process(block), &mut scratch, data, channels);
        },
        |err: Error| eprintln!("audio stream error: {err}"),
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::{Command, Engine, Lane};
    use crate::import::Stereo;

    /// A processor that fills stereo frames with 1, 2, 3 ... on both channels.
    fn counter() -> impl FnMut(&mut [f32]) {
        let mut n = 0.0f32;
        move |buf: &mut [f32]| {
            for frame in buf.chunks_exact_mut(2) {
                n += 1.0;
                frame[0] = n;
                frame[1] = -n;
            }
        }
    }

    #[test]
    fn a_stereo_device_gets_left_and_right_unchanged() {
        let mut scratch = vec![0.0f32; 16];
        let mut out = vec![9.0f32; 6];
        render_device_buffer(counter(), &mut scratch, &mut out, 2);
        assert_eq!(out, vec![1.0, -1.0, 2.0, -2.0, 3.0, -3.0]);
    }

    #[test]
    fn a_mono_device_gets_the_average_of_left_and_right() {
        let mut scratch = vec![0.0f32; 16];
        let mut out = vec![9.0f32; 3];
        let mut p = |buf: &mut [f32]| {
            for frame in buf.chunks_exact_mut(2) {
                frame[0] = 0.5;
                frame[1] = 0.25;
            }
        };
        render_device_buffer(&mut p, &mut scratch, &mut out, 1);
        assert_eq!(out, vec![0.375; 3]);
    }

    #[test]
    fn a_surround_device_gets_front_left_and_right_and_silence_elsewhere() {
        let mut scratch = vec![0.0f32; 16];
        let mut out = vec![9.0f32; 12]; // two frames of six channels
        render_device_buffer(counter(), &mut scratch, &mut out, 6);
        assert_eq!(&out[..6], &[1.0, -1.0, 0.0, 0.0, 0.0, 0.0]);
        assert_eq!(&out[6..], &[2.0, -2.0, 0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn a_device_buffer_bigger_than_the_scratch_is_filled_in_continuous_chunks() {
        let mut scratch = vec![0.0f32; 8]; // room for 4 frames
        let mut out = vec![9.0f32; 20]; // 10 stereo frames
        render_device_buffer(counter(), &mut scratch, &mut out, 2);
        let left: Vec<f32> = out.chunks(2).map(|f| f[0]).collect();
        assert_eq!(left, (1..=10).map(|n| n as f32).collect::<Vec<_>>());
    }

    #[test]
    fn samples_that_do_not_make_a_whole_frame_are_left_silent() {
        let mut scratch = vec![0.0f32; 16];
        let mut out = vec![9.0f32; 5]; // 2 stereo frames + 1 stray sample
        render_device_buffer(counter(), &mut scratch, &mut out, 2);
        assert_eq!(out, vec![1.0, -1.0, 2.0, -2.0, 0.0]);
    }

    #[test]
    fn zero_channels_silence_the_buffer_and_an_empty_buffer_does_nothing() {
        let mut scratch = vec![0.0f32; 16];
        let mut out = vec![9.0f32; 4];
        render_device_buffer(counter(), &mut scratch, &mut out, 0);
        assert_eq!(out, vec![0.0; 4]); // never leave stale values for the device to play
        render_device_buffer(counter(), &mut scratch, &mut [] as &mut [f32], 2);
    }

    #[test]
    fn a_scratch_too_small_for_one_frame_still_leaves_the_output_silent_not_garbage() {
        let mut scratch = vec![0.0f32; 1];
        let mut out = vec![9.0f32; 4];
        render_device_buffer(counter(), &mut scratch, &mut out, 2);
        assert_eq!(out, vec![0.0; 4]);
    }

    #[test]
    fn integer_devices_get_converted_samples_with_silence_staying_zero() {
        let mut scratch = vec![0.0f32; 16];
        let mut out = vec![7i16; 4];
        let mut p = |buf: &mut [f32]| {
            for frame in buf.chunks_exact_mut(2) {
                frame[0] = 0.0;
                frame[1] = 0.5;
            }
        };
        render_device_buffer(&mut p, &mut scratch, &mut out, 2);
        assert_eq!((out[0], out[2]), (0, 0));
        assert!(out[1] > 10_000 && out[1] < 20_000, "{}", out[1]); // about half of i16 full scale
    }

    #[test]
    fn a_real_engine_driven_through_the_device_layer_matches_calling_it_directly() {
        let audio = Stereo {
            samples: (0..1_000)
                .flat_map(|i| [i as f32 * 1e-4, -(i as f32) * 1e-4])
                .collect(),
            sample_rate: 48_000,
        };
        let build = || {
            let (e, mut h) = Engine::new(48_000, 2, 16);
            h.send(Command::SetEnd(1_000)).unwrap();
            h.send(Command::LoadLane {
                lane: 0,
                track: Lane::new(&audio, &[], 0, false, 1.0).unwrap(),
            })
            .unwrap();
            h.send(Command::Play).unwrap();
            (e, h)
        };
        let (mut direct, _h1) = build();
        let mut expected = vec![0.0f32; 600];
        direct.process(&mut expected);

        let (mut via_device, _h2) = build();
        let mut scratch = vec![0.0f32; 64]; // forces many chunks
        let mut out = vec![9.0f32; 600];
        render_device_buffer(
            |buf: &mut [f32]| via_device.process(buf),
            &mut scratch,
            &mut out,
            2,
        );

        assert_eq!(out, expected);
        assert!(out[2..].iter().any(|v| *v != 0.0));
    }
}
