//! Hardware smoke test: plays a quiet 440 Hz tone for two seconds through the default output
//! device, using the real engine. Run it yourself: `cargo run --example play_tone`.

use std::f64::consts::PI;
use std::time::Duration;

use audiobrew::device;
use audiobrew::engine::{Command, Lane};
use audiobrew::import::Stereo;

fn main() {
    let (output, mut handle) = device::start(4, 64).expect("could not open the output device");
    let rate = output.sample_rate;
    println!("Output: {rate} Hz, {} channel(s)", output.channels);

    let frames = rate as usize * 2;
    let samples = (0..frames)
        .flat_map(|i| {
            let v = (0.2 * (2.0 * PI * 440.0 * i as f64 / rate as f64).sin()) as f32;
            [v, v]
        })
        .collect();
    let tone = Stereo {
        samples,
        sample_rate: rate,
    };

    let lane = Lane::new(&tone, &[300.0, 4_000.0], 0, false, 1.0).expect("lane");
    handle.send(Command::SetEnd(frames as i64)).ok();
    handle
        .send(Command::LoadLane {
            lane: 0,
            track: lane,
        })
        .ok();
    handle.send(Command::Play).ok();

    std::thread::sleep(Duration::from_millis(2_500));
    println!(
        "Playhead stopped at frame {} of {frames}",
        handle.playhead()
    );
}
