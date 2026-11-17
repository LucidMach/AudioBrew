//! The audio callback must never allocate, free, or lock. A counting global allocator
//! makes that a test: every allocation or free on the audio thread while `process` runs
//! is counted, and the count must be zero.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::f64::consts::PI;

use audiobrew::engine::{Command, Engine, Lane};
use audiobrew::import::Stereo;

struct CountingAlloc;

thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static EVENTS: Cell<usize> = const { Cell::new(0) };
}

fn record() {
    // `try_with` so allocations during thread teardown don't panic.
    let _ = COUNTING.try_with(|on| {
        if on.get() {
            let _ = EVENTS.try_with(|n| n.set(n.get() + 1));
        }
    });
}

unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        record();
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        record();
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        record();
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

/// Runs `f` and returns how many allocator events happened on this thread during it.
fn count_events(f: impl FnOnce()) -> usize {
    EVENTS.with(|n| n.set(0));
    COUNTING.with(|on| on.set(true));
    f();
    COUNTING.with(|on| on.set(false));
    EVENTS.with(|n| n.get())
}

const RATE: u32 = 48_000;

fn tone(freq: f64, frames: usize) -> Stereo {
    let s = |i: usize| (0.5 * (2.0 * PI * freq * i as f64 / RATE as f64).sin()) as f32;
    Stereo {
        samples: (0..frames).flat_map(|i| [s(i), s(i)]).collect(),
        sample_rate: RATE,
    }
}

fn lane(freq: f64) -> Box<Lane> {
    Lane::new(
        &tone(freq, 48_000),
        &[100.0, 300.0, 1_000.0, 3_000.0, 8_000.0],
        0,
        true,
        1.0,
    )
    .unwrap()
}

#[test]
fn the_counting_allocator_really_counts() {
    // Guards the test itself: if this passed with zero, the zero below would mean nothing.
    assert!(count_events(|| drop(vec![1u8; 64])) >= 2);
}

#[test]
fn process_never_allocates_or_frees_even_while_handling_every_kind_of_command() {
    let (mut engine, mut handle) = Engine::new(RATE, 4, 64);
    let mut out = vec![0.0f32; 512]; // allocated here, outside the counted region

    handle.send(Command::SetEnd(48_000)).unwrap();
    handle.send(Command::SetLooping(true)).unwrap();
    handle
        .send(Command::LoadLane {
            lane: 0,
            track: lane(440.0),
        })
        .unwrap();
    handle
        .send(Command::LoadLane {
            lane: 1,
            track: lane(880.0),
        })
        .unwrap();
    handle.send(Command::Play).unwrap();
    engine.process(&mut out); // setup block, uncounted

    let blocks = 600; // about 3.2 s: crosses the loop point several times
    let events = count_events(|| {
        for n in 0..blocks {
            match n % 12 {
                0 => handle
                    .send(Command::SetFader { lane: 0, gain: 0.5 })
                    .unwrap(),
                1 => handle
                    .send(Command::SetBandMute {
                        lane: 1,
                        band: 2,
                        mute: true,
                    })
                    .unwrap(),
                2 => handle
                    .send(Command::SetBandSolo {
                        lane: 0,
                        band: 4,
                        solo: true,
                    })
                    .unwrap(),
                3 => handle
                    .send(Command::SetBandGain {
                        lane: 1,
                        band: 0,
                        gain: 0.7,
                    })
                    .unwrap(),
                4 => handle.send(Command::Audition(Some(1))).unwrap(),
                5 => handle
                    .send(Command::SetBandSolo {
                        lane: 0,
                        band: 4,
                        solo: false,
                    })
                    .unwrap(),
                6 => handle.send(Command::Audition(None)).unwrap(),
                7 => handle
                    .send(Command::SetOffset {
                        lane: 1,
                        offset: 100,
                    })
                    .unwrap(),
                8 => handle.send(Command::Seek(10_000)).unwrap(),
                9 => handle.send(Command::Pause).unwrap(),
                10 => handle.send(Command::Play).unwrap(),
                _ => handle
                    .send(Command::SetBandMute {
                        lane: 1,
                        band: 2,
                        mute: false,
                    })
                    .unwrap(),
            }
            engine.process(&mut out);
        }
    });
    assert_eq!(
        events, 0,
        "the audio thread allocated or freed {events} times"
    );
    assert!(out.iter().all(|v| v.is_finite()));
}

#[test]
fn replacing_a_lane_on_the_audio_thread_frees_nothing_there() {
    let (mut engine, mut handle) = Engine::new(RATE, 2, 16);
    let mut out = vec![0.0f32; 256];
    handle.send(Command::SetEnd(48_000)).unwrap();
    handle
        .send(Command::LoadLane {
            lane: 0,
            track: lane(440.0),
        })
        .unwrap();
    handle.send(Command::Play).unwrap();
    engine.process(&mut out);

    handle
        .send(Command::LoadLane {
            lane: 0,
            track: lane(660.0),
        })
        .unwrap(); // built outside
    let events = count_events(|| engine.process(&mut out));
    assert_eq!(
        events, 0,
        "replacing a lane allocated or freed {events} times on the audio thread"
    );
    assert_eq!(handle.collect_garbage(), 1); // the old lane is freed here, on the UI side
}
