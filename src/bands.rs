/// The audible range every Track's Bands partition between them.
pub const MIN_HZ: f64 = 20.0;
pub const MAX_HZ: f64 = 20_000.0;
pub const MAX_BANDS: usize = 6;

/// A Track's Bands: contiguous frequency ranges separated by crossover points,
/// so Bands can never overlap or leave gaps.
pub struct BandConfig {
    crossovers: Vec<f64>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum BandError {
    TooManyBands,
    InvalidCrossover,
}

impl Default for BandConfig {
    fn default() -> Self {
        Self { crossovers: vec![300.0, 4_000.0] }
    }
}

impl BandConfig {
    /// Splits the Band containing `hz` in two. Fails at the 6-Band maximum.
    pub fn add_band(&mut self, hz: f64) -> Result<(), BandError> {
        if self.crossovers.len() + 1 >= MAX_BANDS {
            return Err(BandError::TooManyBands);
        }
        if hz <= MIN_HZ || hz >= MAX_HZ || self.crossovers.contains(&hz) {
            return Err(BandError::InvalidCrossover);
        }
        let at = self.crossovers.partition_point(|&c| c < hz);
        self.crossovers.insert(at, hz);
        Ok(())
    }

    /// Moves the boundary between Band `index` and Band `index + 1`.
    /// It may not cross a neighbouring boundary or leave the audible range.
    pub fn move_crossover(&mut self, index: usize, hz: f64) -> Result<(), BandError> {
        if index >= self.crossovers.len() {
            return Err(BandError::InvalidCrossover);
        }
        let lower = if index == 0 { MIN_HZ } else { self.crossovers[index - 1] };
        let upper = self.crossovers.get(index + 1).copied().unwrap_or(MAX_HZ);
        if hz <= lower || hz >= upper {
            return Err(BandError::InvalidCrossover);
        }
        self.crossovers[index] = hz;
        Ok(())
    }

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
    fn adding_a_band_splits_the_band_it_lands_in() {
        let mut bands = BandConfig::default();

        bands.add_band(1_000.0).unwrap();

        assert_eq!(
            bands.ranges(),
            vec![
                (20.0, 300.0),
                (300.0, 1_000.0),
                (1_000.0, 4_000.0),
                (4_000.0, 20_000.0)
            ]
        );
    }

    #[test]
    fn dragging_a_crossover_resizes_the_two_bands_either_side() {
        let mut bands = BandConfig::default();

        bands.move_crossover(0, 500.0).unwrap();

        assert_eq!(
            bands.ranges(),
            vec![(20.0, 500.0), (500.0, 4_000.0), (4_000.0, 20_000.0)]
        );
    }

    #[test]
    fn a_crossover_cannot_be_dragged_past_its_neighbour() {
        let mut bands = BandConfig::default();

        assert_eq!(bands.move_crossover(0, 5_000.0), Err(BandError::InvalidCrossover));
        assert_eq!(
            bands.ranges(),
            vec![(20.0, 300.0), (300.0, 4_000.0), (4_000.0, 20_000.0)]
        );
    }

    #[test]
    fn moving_a_crossover_that_does_not_exist_is_an_error_not_a_panic() {
        let mut bands = BandConfig::default(); // 3 Bands, so crossovers 0 and 1 only

        assert_eq!(bands.move_crossover(2, 5_000.0), Err(BandError::InvalidCrossover));
    }

    #[test]
    fn a_band_cannot_be_added_outside_the_audible_range_or_on_an_existing_crossover() {
        let mut bands = BandConfig::default();

        assert_eq!(bands.add_band(10.0), Err(BandError::InvalidCrossover));
        assert_eq!(bands.add_band(25_000.0), Err(BandError::InvalidCrossover));
        assert_eq!(bands.add_band(300.0), Err(BandError::InvalidCrossover));
        assert_eq!(bands.ranges().len(), 3);
    }

    #[test]
    fn a_track_cannot_have_more_than_six_bands() {
        let mut bands = BandConfig::default();
        bands.add_band(1_000.0).unwrap(); // 4
        bands.add_band(2_000.0).unwrap(); // 5
        bands.add_band(8_000.0).unwrap(); // 6

        assert_eq!(bands.add_band(12_000.0), Err(BandError::TooManyBands));
        assert_eq!(bands.ranges().len(), 6);
    }

    #[test]
    fn a_new_track_has_low_mid_and_high_bands() {
        let ranges = BandConfig::default().ranges();

        assert_eq!(
            ranges,
            vec![(20.0, 300.0), (300.0, 4_000.0), (4_000.0, 20_000.0)]
        );
    }
}
