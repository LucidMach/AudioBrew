/// The audible range every Track's Bands partition between them.
pub const MIN_HZ: f64 = 20.0;
pub const MAX_HZ: f64 = 20_000.0;

/// A Track's Bands: contiguous frequency ranges separated by crossover points,
/// so Bands can never overlap or leave gaps.
pub struct BandConfig {
    crossovers: Vec<f64>,
}

impl Default for BandConfig {
    fn default() -> Self {
        Self { crossovers: vec![300.0, 4_000.0] }
    }
}

impl BandConfig {
    /// Each Band as `(low_hz, high_hz)`, lowest first.
    pub fn ranges(&self) -> Vec<(f64, f64)> {
        let edges = std::iter::once(MIN_HZ)
            .chain(self.crossovers.iter().copied())
            .chain(std::iter::once(MAX_HZ))
            .collect::<Vec<_>>();
        edges.windows(2).map(|w| (w[0], w[1])).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_track_has_low_mid_and_high_bands() {
        let ranges = BandConfig::default().ranges();

        assert_eq!(
            ranges,
            vec![(20.0, 300.0), (300.0, 4_000.0), (4_000.0, 20_000.0)]
        );
    }
}
