/// Readouts shown next to every control. Unusable values show an em dash.
const DASH: &str = "—";

pub fn format_bpm(bpm: f64) -> String {
    if bpm.is_finite() {
        format!("{bpm:.1} BPM")
    } else {
        DASH.into()
    }
}

/// A linear gain as decibels with one decimal; 0 or less is silence.
pub fn format_db(gain: f32) -> String {
    if !gain.is_finite() {
        return DASH.into();
    }
    if gain <= 0.0 {
        return "-∞ dB".into();
    }
    let db = ((20.0 * (gain as f64).log10()) * 10.0).round() / 10.0;
    if db == 0.0 {
        "0.0 dB".into()
    } else {
        format!("{db:+.1} dB")
    }
}

pub fn format_offset_ms(secs: f64) -> String {
    if !secs.is_finite() {
        return DASH.into();
    }
    let ms = (secs * 1000.0 * 10.0).round() / 10.0;
    if ms == 0.0 {
        "0.0 ms".into()
    } else {
        format!("{ms:+.1} ms")
    }
}

pub fn format_stretch(factor: f64) -> String {
    if factor.is_finite() {
        format!("{factor:.2}×")
    } else {
        DASH.into()
    }
}

pub fn format_semitones(semitones: i8) -> String {
    if semitones == 0 {
        "0 st".into()
    } else {
        format!("{semitones:+} st")
    }
}

pub fn format_hz(hz: f64) -> String {
    if !hz.is_finite() || hz <= 0.0 {
        return DASH.into();
    }
    if hz.round() < 1000.0 {
        format!("{hz:.0} Hz")
    } else if hz < 10_000.0 {
        format!("{:.2} kHz", hz / 1000.0)
    } else {
        format!("{:.1} kHz", hz / 1000.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bpm_shows_one_decimal() {
        assert_eq!(format_bpm(120.0), "120.0 BPM");
        assert_eq!(format_bpm(97.26), "97.3 BPM");
    }

    #[test]
    fn gain_is_shown_in_decibels() {
        assert_eq!(format_db(1.0), "0.0 dB");
        assert_eq!(format_db(0.5), "-6.0 dB");
        assert_eq!(format_db(2.0), "+6.0 dB");
        assert_eq!(format_db(0.1), "-20.0 dB");
    }

    #[test]
    fn silence_is_minus_infinity_and_nonsense_is_a_dash() {
        assert_eq!(format_db(0.0), "-∞ dB");
        assert_eq!(format_db(-0.5), "-∞ dB");
        assert_eq!(format_db(f32::NAN), "—");
        assert_eq!(format_db(f32::INFINITY), "—");
        assert_eq!(format_bpm(f64::NAN), "—");
    }

    #[test]
    fn a_gain_a_hair_under_unity_does_not_show_negative_zero() {
        assert_eq!(format_db(0.9999), "0.0 dB");
    }

    #[test]
    fn offsets_are_shown_in_milliseconds_with_a_sign() {
        assert_eq!(format_offset_ms(0.25), "+250.0 ms");
        assert_eq!(format_offset_ms(-0.0125), "-12.5 ms");
        assert_eq!(format_offset_ms(0.0), "0.0 ms");
        assert_eq!(format_offset_ms(f64::NAN), "—");
    }

    #[test]
    fn stretch_is_shown_as_a_multiplier() {
        assert_eq!(format_stretch(1.2), "1.20×");
        assert_eq!(format_stretch(0.7), "0.70×");
        assert_eq!(format_stretch(f64::NAN), "—");
    }

    #[test]
    fn pitch_shift_is_shown_in_signed_semitones() {
        assert_eq!(format_semitones(2), "+2 st");
        assert_eq!(format_semitones(-5), "-5 st");
        assert_eq!(format_semitones(0), "0 st");
    }

    #[test]
    fn frequencies_switch_to_khz_at_1000() {
        assert_eq!(format_hz(300.0), "300 Hz");
        assert_eq!(format_hz(999.4), "999 Hz");
        assert_eq!(format_hz(1000.0), "1.00 kHz");
        assert_eq!(format_hz(4000.0), "4.00 kHz");
        assert_eq!(format_hz(12_500.0), "12.5 kHz");
        assert_eq!(format_hz(f64::NAN), "—");
    }
}
