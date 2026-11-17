use crate::bands::BandConfig;

#[derive(Debug, PartialEq, Eq)]
pub enum SplitterError {
    InvalidSampleRate,
    InvalidCrossover,
}

/// One 2nd-order Butterworth section (Q = 1/sqrt 2), direct form I in f64.
#[derive(Debug, Clone, Copy)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    x: [f64; 2],
    y: [f64; 2],
}

impl Biquad {
    fn new(sample_rate: f64, hz: f64, high_pass: bool) -> Self {
        let w0 = 2.0 * std::f64::consts::PI * hz / sample_rate;
        let (sin, cos) = w0.sin_cos();
        let alpha = sin / (2.0 * std::f64::consts::FRAC_1_SQRT_2);
        let a0 = 1.0 + alpha;
        let b = if high_pass {
            [(1.0 + cos) / 2.0, -(1.0 + cos), (1.0 + cos) / 2.0]
        } else {
            [(1.0 - cos) / 2.0, 1.0 - cos, (1.0 - cos) / 2.0]
        };
        Self {
            b: b.map(|v| v / a0),
            a: [-2.0 * cos / a0, (1.0 - alpha) / a0],
            x: [0.0; 2],
            y: [0.0; 2],
        }
    }

    fn process(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.b[1] * self.x[0] + self.b[2] * self.x[1]
            - self.a[0] * self.y[0]
            - self.a[1] * self.y[1];
        self.x = [x, self.x[0]];
        self.y = [y, self.y[0]];
        y
    }
}

/// A Linkwitz-Riley 4th-order crossover: two cascaded Butterworth sections per side.
/// Low + high sum to an all-pass (flat magnitude).
#[derive(Debug, Clone, Copy)]
struct Lr4 {
    low: [Biquad; 2],
    high: [Biquad; 2],
}

impl Lr4 {
    fn new(sample_rate: f64, hz: f64) -> Self {
        let section = |high| Biquad::new(sample_rate, hz, high);
        Self {
            low: [section(false); 2],
            high: [section(true); 2],
        }
    }

    fn split(&mut self, x: f64) -> (f64, f64) {
        let low = self.low[0].process(x);
        let low = self.low[1].process(low);
        let high = self.high[0].process(x);
        let high = self.high[1].process(high);
        (low, high)
    }
}

/// Splits a signal into contiguous Bands at the crossover points. Bands sum back to a
/// flat response: each Band is all-passed by every crossover above it. Allocates only in `new`.
#[derive(Debug, Clone)]
pub struct BandSplitter {
    /// `stages[k][0]` splits the remaining signal at crossover `k`;
    /// `stages[k][1 + b]` all-passes the already-split Band `b` (for `b < k`).
    stages: Vec<Vec<Lr4>>,
    bands: Vec<f64>,
}

impl BandSplitter {
    pub fn new(sample_rate: f64, crossovers_hz: &[f64]) -> Result<Self, SplitterError> {
        if !sample_rate.is_finite() || sample_rate <= 0.0 {
            return Err(SplitterError::InvalidSampleRate);
        }
        let nyquist = sample_rate / 2.0;
        let sorted = crossovers_hz.windows(2).all(|w| w[0] < w[1]);
        let in_range = crossovers_hz
            .iter()
            .all(|&c| c.is_finite() && c > 0.0 && c < nyquist);
        if !sorted || !in_range {
            return Err(SplitterError::InvalidCrossover);
        }
        let stages = crossovers_hz
            .iter()
            .enumerate()
            .map(|(k, &hz)| vec![Lr4::new(sample_rate, hz); k + 1])
            .collect();
        Ok(Self {
            stages,
            bands: vec![0.0; crossovers_hz.len() + 1],
        })
    }

    pub fn from_config(config: &BandConfig, sample_rate: f64) -> Result<Self, SplitterError> {
        Self::new(sample_rate, config.crossovers())
    }

    pub fn band_count(&self) -> usize {
        self.bands.len()
    }

    /// Splits one sample into `out` (lowest Band first). A non-finite input counts as silence.
    pub fn process(&mut self, x: f32, out: &mut [f32]) {
        let mut rest = if x.is_finite() { x as f64 } else { 0.0 };
        for (k, stage) in self.stages.iter_mut().enumerate() {
            let (low, high) = stage[0].split(rest);
            for b in 0..k {
                let (l, h) = stage[1 + b].split(self.bands[b]);
                self.bands[b] = l + h;
            }
            self.bands[k] = low;
            rest = high;
        }
        let last = self.bands.len() - 1;
        self.bands[last] = rest;
        for (slot, v) in out.iter_mut().zip(&self.bands) {
            *slot = *v as f32;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bands::BandConfig;

    const FS: f64 = 48_000.0;

    /// Runs a unit-amplitude sine through the splitter for 2 s and returns each Band's RMS over
    /// the second half (after the filters settle), plus the RMS of the Bands summed.
    fn measure(crossovers: &[f64], freq: f64) -> (Vec<f64>, f64) {
        let mut split = BandSplitter::new(FS, crossovers).unwrap();
        let n = split.band_count();
        let total = (FS * 2.0) as usize;
        let mut out = vec![0.0f32; n];
        let mut band_sq = vec![0.0f64; n];
        let mut sum_sq = 0.0f64;
        let mut counted = 0usize;
        for i in 0..total {
            let x = (2.0 * std::f64::consts::PI * freq * i as f64 / FS).sin() as f32;
            split.process(x, &mut out);
            if i >= total / 2 {
                for (acc, v) in band_sq.iter_mut().zip(&out) {
                    *acc += (*v as f64).powi(2);
                }
                sum_sq += (out.iter().sum::<f32>() as f64).powi(2);
                counted += 1;
            }
        }
        let rms = |sq: f64| (sq / counted as f64).sqrt();
        (band_sq.into_iter().map(rms).collect(), rms(sum_sq))
    }

    const SINE_RMS: f64 = std::f64::consts::FRAC_1_SQRT_2;

    fn assert_flat(crossovers: &[f64], freq: f64) {
        let (_, sum) = measure(crossovers, freq);
        let db = 20.0 * (sum / SINE_RMS).log10();
        assert!(
            db.abs() < 0.1,
            "{freq} Hz through {crossovers:?}: {db:.3} dB"
        );
    }

    #[test]
    fn three_bands_sum_back_to_a_flat_response() {
        for f in [50.0, 150.0, 300.0, 1_000.0, 4_000.0, 10_000.0] {
            assert_flat(&[300.0, 4_000.0], f);
        }
    }

    #[test]
    fn six_bands_sum_back_to_a_flat_response() {
        let xo = [100.0, 300.0, 1_000.0, 3_000.0, 8_000.0];
        for f in [40.0, 100.0, 200.0, 1_000.0, 2_000.0, 8_000.0, 15_000.0] {
            assert_flat(&xo, f);
        }
    }

    #[test]
    fn each_band_is_6_db_down_at_its_crossover() {
        // Linkwitz-Riley: both sides of a crossover meet at -6.02 dB, i.e. half amplitude.
        let (bands, _) = measure(&[1_000.0], 1_000.0);
        let half = 0.5 * SINE_RMS;
        assert!((bands[0] - half).abs() < 0.01, "low {}", bands[0]);
        assert!((bands[1] - half).abs() < 0.01, "high {}", bands[1]);
    }

    #[test]
    fn a_tone_far_below_the_crossover_stays_in_the_low_band() {
        // 24 dB/octave: at crossover / 6 the high band leaks about (1/6)^4 = 0.0008 of the tone.
        let (bands, _) = measure(&[600.0], 100.0);
        assert!(bands[0] > 0.99 * SINE_RMS);
        assert!(bands[1] < 0.01 * SINE_RMS);
    }

    #[test]
    fn a_tone_far_above_the_crossover_stays_in_the_high_band() {
        let (bands, _) = measure(&[600.0], 3_600.0);
        assert!(bands[1] > 0.99 * SINE_RMS);
        assert!(bands[0] < 0.01 * SINE_RMS);
    }

    #[test]
    fn with_no_crossovers_there_is_one_band_and_it_passes_through_unchanged() {
        let mut split = BandSplitter::new(FS, &[]).unwrap();
        assert_eq!(split.band_count(), 1);
        let mut out = [0.0];
        for x in [0.0, 0.5, -0.25, 1.0] {
            split.process(x, &mut out);
            assert_eq!(out[0], x);
        }
    }

    #[test]
    fn the_band_count_is_one_more_than_the_crossover_count() {
        assert_eq!(BandSplitter::new(FS, &[300.0]).unwrap().band_count(), 2);
        assert_eq!(
            BandSplitter::new(FS, &[100.0, 200.0, 300.0])
                .unwrap()
                .band_count(),
            4
        );
    }

    #[test]
    fn unsorted_duplicate_or_out_of_range_crossovers_are_rejected() {
        assert!(BandSplitter::new(FS, &[4_000.0, 300.0]).is_err());
        assert!(BandSplitter::new(FS, &[300.0, 300.0]).is_err());
        assert!(BandSplitter::new(FS, &[0.0]).is_err());
        assert!(BandSplitter::new(FS, &[-10.0]).is_err());
        assert!(BandSplitter::new(FS, &[24_000.0]).is_err()); // Nyquist
        assert!(BandSplitter::new(FS, &[f64::NAN]).is_err());
        assert!(BandSplitter::new(0.0, &[300.0]).is_err());
    }

    #[test]
    fn a_nan_input_sample_does_not_poison_the_filters() {
        let mut split = BandSplitter::new(FS, &[300.0, 4_000.0]).unwrap();
        let mut out = [0.0f32; 3];
        split.process(f32::NAN, &mut out);
        split.process(f32::INFINITY, &mut out);
        for i in 0..2_000 {
            split.process((i as f32 * 0.05).sin(), &mut out);
            assert!(out.iter().all(|v| v.is_finite()), "sample {i}");
        }
    }

    #[test]
    fn silence_in_gives_silence_out() {
        let mut split = BandSplitter::new(FS, &[300.0, 4_000.0]).unwrap();
        let mut out = [1.0; 3];
        split.process(0.0, &mut out);
        assert_eq!(out, [0.0; 3]);
    }

    #[test]
    fn it_can_be_built_from_a_band_config() {
        let split = BandSplitter::from_config(&BandConfig::default(), FS).unwrap();
        assert_eq!(split.band_count(), 3);
    }
}
