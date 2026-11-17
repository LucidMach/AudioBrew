/// One Band's controls on a Track.
#[derive(Debug, Clone, Copy)]
pub struct BandState {
    pub gain: f32,
    pub mute: bool,
    pub solo: bool,
}

/// The gain each Band actually contributes once mute and solo are applied.
pub fn effective_gains(bands: &[BandState]) -> Vec<f32> {
    let any_solo = bands.iter().any(|b| b.solo);
    bands
        .iter()
        .map(|b| if b.mute || (any_solo && !b.solo) { 0.0 } else { b.gain })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(gain: f32, mute: bool, solo: bool) -> BandState {
        BandState { gain, mute, solo }
    }

    #[test]
    fn soloing_a_band_silences_the_others() {
        let bands = [band(0.8, false, false), band(0.5, false, true), band(0.9, false, false)];

        assert_eq!(effective_gains(&bands), vec![0.0, 0.5, 0.0]);
    }

    #[test]
    fn mute_wins_over_solo_on_the_same_band() {
        let bands = [band(0.8, false, false), band(0.5, true, true)];

        assert_eq!(effective_gains(&bands), vec![0.0, 0.0]);
    }

    #[test]
    fn a_muted_band_contributes_nothing() {
        let bands = [band(0.8, false, false), band(0.5, true, false)];

        assert_eq!(effective_gains(&bands), vec![0.8, 0.0]);
    }
}
