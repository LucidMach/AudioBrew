/// How many samples a ramp of `ms` milliseconds lasts at `sample_rate`; unusable input is 0.
pub fn ramp_len(sample_rate: f64, ms: f64) -> usize {
    let usable = sample_rate.is_finite() && sample_rate > 0.0 && ms.is_finite() && ms > 0.0;
    if usable {
        (sample_rate * ms / 1000.0).round() as usize
    } else {
        0
    }
}

/// A parameter value that glides linearly to its target so changes never click.
/// Retargeting continues from the current value. `next` does no allocation.
#[derive(Debug, Clone)]
pub struct Ramp {
    start: f64,
    target: f64,
    current: f64,
    step: usize,
    len: usize,
}

impl Ramp {
    pub fn new(value: f32) -> Self {
        let v = value as f64;
        Self {
            start: v,
            target: v,
            current: v,
            step: 0,
            len: 0,
        }
    }

    /// Glides to `target` over `len` samples, from wherever the value is now.
    /// A non-finite target is ignored; `len == 0` jumps.
    pub fn set_target(&mut self, target: f32, len: usize) {
        if !target.is_finite() {
            return;
        }
        let target = target as f64;
        self.start = self.current;
        self.target = target;
        self.step = 0;
        if len == 0 || self.start == target {
            self.current = target;
            self.len = 0;
        } else {
            self.len = len;
        }
    }

    pub fn is_ramping(&self) -> bool {
        self.step < self.len
    }

    /// The next sample's value.
    pub fn next_value(&mut self) -> f32 {
        if self.is_ramping() {
            self.step += 1;
            self.current = if self.step == self.len {
                self.target
            } else {
                self.start + (self.target - self.start) * self.step as f64 / self.len as f64
            };
        }
        self.current as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_milliseconds_is_882_samples_at_44_1k_and_960_at_48k() {
        assert_eq!(ramp_len(44_100.0, 20.0), 882);
        assert_eq!(ramp_len(48_000.0, 20.0), 960);
    }

    #[test]
    fn an_unusable_rate_or_time_gives_a_zero_length_ramp() {
        assert_eq!(ramp_len(0.0, 20.0), 0);
        assert_eq!(ramp_len(48_000.0, -5.0), 0);
        assert_eq!(ramp_len(f64::NAN, 20.0), 0);
        assert_eq!(ramp_len(48_000.0, f64::INFINITY), 0);
    }

    #[test]
    fn a_ramp_from_0_to_1_rises_linearly_one_step_at_a_time() {
        let mut r = Ramp::new(0.0);
        r.set_target(1.0, 10);
        let got: Vec<f32> = (0..10).map(|_| r.next_value()).collect();
        let want: Vec<f32> = (1..=10).map(|k| k as f32 / 10.0).collect();
        assert_eq!(got, want);
    }

    #[test]
    fn it_lands_exactly_on_the_target_and_stays_there() {
        let mut r = Ramp::new(0.3);
        r.set_target(0.7, 882);
        for _ in 0..882 {
            r.next_value();
        }
        assert_eq!(r.next_value(), 0.7);
        assert_eq!(r.next_value(), 0.7);
        assert!(!r.is_ramping());
    }

    #[test]
    fn retargeting_mid_ramp_continues_from_the_current_value_without_a_jump() {
        let mut r = Ramp::new(0.0);
        r.set_target(1.0, 10);
        for _ in 0..5 {
            r.next_value(); // now at 0.5
        }
        r.set_target(0.0, 10);
        assert_eq!(r.next_value(), 0.45);
    }

    #[test]
    fn a_ramp_of_zero_length_jumps_straight_to_the_target() {
        let mut r = Ramp::new(0.0);
        r.set_target(0.8, 0);
        assert_eq!(r.next_value(), 0.8);
    }

    #[test]
    fn a_non_finite_target_is_ignored() {
        let mut r = Ramp::new(0.5);
        r.set_target(f32::NAN, 10);
        r.set_target(f32::INFINITY, 10);
        assert_eq!(r.next_value(), 0.5);
        assert!(!r.is_ramping());
    }

    #[test]
    fn a_ramp_with_nothing_to_do_holds_its_value() {
        let mut r = Ramp::new(0.25);
        assert_eq!(r.next_value(), 0.25);
        r.set_target(0.25, 100);
        assert_eq!(r.next_value(), 0.25);
    }
}
