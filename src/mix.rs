/// One Band's controls on a Track.
#[derive(Debug, Clone, Copy)]
pub struct BandState {
    pub gain: f32,
    pub mute: bool,
    pub solo: bool,
}

/// The gain each Band actually contributes once mute and solo are applied.
pub fn effective_gains(bands: &[BandState]) -> Vec<f32> {
    bands.iter().map(|b| if b.mute { 0.0 } else { b.gain }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn band(gain: f32, mute: bool, solo: bool) -> BandState {
        BandState { gain, mute, solo }
    }

    #[test]
    fn a_muted_band_contributes_nothing() {
        let bands = [band(0.8, false, false), band(0.5, true, false)];

        assert_eq!(effective_gains(&bands), vec![0.8, 0.0]);
    }
}
