/// Converts seconds to whole sample frames; unusable input is 0.
pub fn secs_to_frames(secs: f64, sample_rate: f64) -> i64 {
    let usable = secs.is_finite() && secs >= 0.0 && sample_rate.is_finite() && sample_rate > 0.0;
    if usable {
        (secs * sample_rate).round() as i64
    } else {
        0
    }
}

/// The shared Playhead and play state, in sample frames. Pure: the audio callback drives it
/// with `advance(block_len)`.
#[derive(Debug, Clone)]
pub struct Transport {
    position: i64,
    end: i64,
    playing: bool,
    looping: bool,
}

impl Transport {
    /// `end` is the End marker in frames.
    pub fn new(end: i64) -> Self {
        Self {
            position: 0,
            end: end.max(0),
            playing: false,
            looping: false,
        }
    }

    pub fn position(&self) -> i64 {
        self.position
    }

    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// Starts playing; from the end it restarts at 0. An empty Mashup cannot play.
    pub fn play(&mut self) {
        if self.end == 0 {
            return;
        }
        if self.position >= self.end {
            self.position = 0;
        }
        self.playing = true;
    }

    pub fn pause(&mut self) {
        self.playing = false;
    }

    pub fn set_looping(&mut self, looping: bool) {
        self.looping = looping;
    }

    /// Moves the End marker, pulling the Playhead back if it is now past it.
    pub fn set_end(&mut self, end: i64) {
        self.end = end.max(0);
        self.position = self.position.min(self.end);
        if self.end == 0 {
            self.playing = false;
        }
    }

    pub fn seek(&mut self, frame: i64) {
        self.position = frame.clamp(0, self.end);
    }

    pub fn seek_by(&mut self, delta: i64) {
        self.seek(self.position.saturating_add(delta));
    }

    /// Moves the Playhead forward by one block while playing: wraps when looping,
    /// otherwise stops at the End marker.
    pub fn advance(&mut self, frames: i64) {
        if !self.playing || frames <= 0 {
            return;
        }
        let next = self.position.saturating_add(frames);
        if next < self.end {
            self.position = next;
        } else if self.looping {
            self.position = next % self.end;
        } else {
            self.position = self.end;
            self.playing = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn playing(end: i64) -> Transport {
        let mut t = Transport::new(end);
        t.play();
        t
    }

    #[test]
    fn the_playhead_only_moves_while_playing() {
        let mut t = Transport::new(100);
        t.advance(10);
        assert_eq!(t.position(), 0);

        t.play();
        t.advance(10);
        assert_eq!(t.position(), 10);

        t.pause();
        t.advance(10);
        assert_eq!(t.position(), 10);
    }

    #[test]
    fn without_looping_the_playhead_stops_at_the_end_marker() {
        let mut t = playing(100);
        t.advance(90);
        t.advance(25);
        assert_eq!(t.position(), 100);
        assert!(!t.is_playing());
    }

    #[test]
    fn with_looping_the_playhead_wraps_sample_exactly() {
        let mut t = playing(100);
        t.set_looping(true);
        t.advance(90);
        t.advance(25);
        assert_eq!(t.position(), 15);
        assert!(t.is_playing());
    }

    #[test]
    fn a_block_longer_than_the_whole_mashup_wraps_by_the_remainder() {
        let mut t = playing(100);
        t.set_looping(true);
        t.advance(250);
        assert_eq!(t.position(), 50);
    }

    #[test]
    fn landing_exactly_on_the_end_wraps_to_zero_when_looping() {
        let mut t = playing(100);
        t.set_looping(true);
        t.advance(100);
        assert_eq!(t.position(), 0);
    }

    #[test]
    fn playing_from_the_end_restarts_at_zero() {
        let mut t = playing(100);
        t.advance(100);
        assert!(!t.is_playing());
        t.play();
        assert_eq!(t.position(), 0);
        assert!(t.is_playing());
    }

    #[test]
    fn moving_the_end_marker_below_the_playhead_pulls_the_playhead_back() {
        let mut t = Transport::new(100);
        t.seek(30);
        t.set_end(20);
        assert_eq!(t.position(), 20);
        t.set_end(-5);
        assert_eq!(t.position(), 0);
    }

    #[test]
    fn a_wrapping_loop_survives_the_end_marker_shrinking() {
        let mut t = playing(100);
        t.set_looping(true);
        t.advance(60);
        t.set_end(40);
        t.advance(10);
        assert_eq!(t.position(), 10); // pulled to 40, then 40 + 10 wraps to 10
    }

    #[test]
    fn seeking_clamps_to_the_start_and_the_end() {
        let mut t = Transport::new(100);
        t.seek(-50);
        assert_eq!(t.position(), 0);
        t.seek(500);
        assert_eq!(t.position(), 100);
        t.seek_by(-30);
        assert_eq!(t.position(), 70);
        t.seek_by(i64::MIN);
        assert_eq!(t.position(), 0);
        t.seek_by(i64::MAX);
        assert_eq!(t.position(), 100);
    }

    #[test]
    fn an_empty_mashup_cannot_play() {
        let mut t = Transport::new(0);
        t.play();
        t.advance(10);
        assert_eq!(t.position(), 0);
        assert!(!t.is_playing());
    }

    #[test]
    fn seconds_convert_to_whole_frames() {
        assert_eq!(secs_to_frames(1.0, 44_100.0), 44_100);
        assert_eq!(secs_to_frames(0.5, 48_000.0), 24_000);
        assert_eq!(secs_to_frames(1.0 / 3.0, 48_000.0), 16_000);
    }

    #[test]
    fn unusable_seconds_or_rates_convert_to_zero_frames() {
        assert_eq!(secs_to_frames(-1.0, 48_000.0), 0);
        assert_eq!(secs_to_frames(f64::NAN, 48_000.0), 0);
        assert_eq!(secs_to_frames(f64::INFINITY, 48_000.0), 0);
        assert_eq!(secs_to_frames(1.0, 0.0), 0);
        assert_eq!(secs_to_frames(1.0, f64::NAN), 0);
    }
}
