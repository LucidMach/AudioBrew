//! Volume and pan maths for a Track: fader taper, readouts, typed values,
//! nudging and slider break points.

/// Fader position of unity gain (0 dB).
pub const FADER_UNITY: f64 = 0.8;
/// Gain at the top of the fader.
pub const FADER_MAX_DB: f64 = 6.0;
/// Lowest volume shown as a number; anything below is silence.
pub const MIN_DB: f64 = -60.0;
/// Pan readout units on each side: 50L .. 0 .. 50R.
pub const PAN_UNITS: f64 = 50.0;

/// Nudge steps for a typed value: arrows, and Shift+arrows (fine).
pub const VOLUME_NUDGE_DB: f64 = 1.0;
pub const VOLUME_NUDGE_FINE_DB: f64 = 0.1;
pub const PAN_NUDGE: f64 = 5.0;
pub const PAN_NUDGE_FINE: f64 = 1.0;

/// Volume break points (dB): every 1.5 dB near unity, every 3 dB below -6, then wider.
pub const VOLUME_DETENTS_DB: [f64; 18] = [
    -48.0, -36.0, -30.0, -24.0, -21.0, -18.0, -15.0, -12.0, -9.0, -6.0, -4.5, -3.0, -1.5, 0.0, 1.5,
    3.0, 4.5, 6.0,
];
/// Pan break points: 25L, centre, 25R (the ends are the slider's limits).
pub const PAN_DETENTS: [f64; 3] = [-0.5, 0.0, 0.5];

/// Chosen so the top of the fader (position 1) is exactly FADER_MAX_DB.
fn db_per_decade() -> f64 {
    FADER_MAX_DB / (1.0 / FADER_UNITY).log10()
}

/// Position 0 is silence, FADER_UNITY is 0 dB, 1 is +6 dB.
pub fn fader_to_db(position: f64) -> f64 {
    if position <= 0.0 {
        return f64::NEG_INFINITY;
    }
    db_per_decade() * (position / FADER_UNITY).log10()
}

/// Inverse of `fader_to_db`, clamped to the fader; silence is position 0.
pub fn db_to_fader(db: f64) -> f64 {
    if !db.is_finite() {
        return 0.0;
    }
    (FADER_UNITY * 10f64.powf(db / db_per_decade())).min(1.0)
}

fn round1(x: f64) -> f64 {
    let r = (x * 10.0).round() / 10.0;
    if r == 0.0 { 0.0 } else { r } // no "-0.0"
}

/// "-6.0", "0.0", "+3.5", or "-∞" for silence.
pub fn format_db(db: f64) -> String {
    if !db.is_finite() || db < MIN_DB {
        return "-∞".into();
    }
    let r = round1(db);
    if r > 0.0 {
        format!("+{r:.1}")
    } else {
        format!("{r:.1}")
    }
}

/// Pan is -1 (left) .. 1 (right). "0" at centre, otherwise "20L" / "20R".
pub fn format_pan(pan: f64) -> String {
    let v = (pan * PAN_UNITS).round() as i64;
    match v {
        0 => "0".into(),
        v if v < 0 => format!("{}L", -v),
        v => format!("{v}R"),
    }
}

/// Typed volume: "-6", "-6 dB", "+3", "-inf", "-∞". Capped at +6; below -60 is silence.
pub fn parse_db(text: &str) -> Option<f64> {
    let s = text.trim().to_lowercase();
    let s = s.strip_suffix("db").unwrap_or(&s).trim();
    if matches!(s, "-inf" | "inf" | "-∞" | "∞") {
        return Some(f64::NEG_INFINITY);
    }
    let n: f64 = s.parse().ok().filter(|n: &f64| n.is_finite())?;
    if n < MIN_DB {
        Some(f64::NEG_INFINITY)
    } else {
        Some(n.min(FADER_MAX_DB))
    }
}

/// Nudge a dB value by `delta`. Up from silence starts at MIN_DB; below MIN_DB is silence.
pub fn nudge_db(db: f64, delta: f64) -> f64 {
    if !db.is_finite() && delta < 0.0 {
        return f64::NEG_INFINITY;
    }
    let from = if db.is_finite() {
        round1(db)
    } else {
        MIN_DB - delta
    };
    let next = round1(from + delta).min(FADER_MAX_DB);
    if next < MIN_DB {
        f64::NEG_INFINITY
    } else {
        next
    }
}

fn pan_from_units(units: f64) -> f64 {
    units.round().clamp(-PAN_UNITS, PAN_UNITS) / PAN_UNITS
}

/// Typed pan: "0", "c", "center", "20L", "L20", "20 r", "-20" (left), "+20" (right).
pub fn parse_pan(text: &str) -> Option<f64> {
    let s: String = text
        .trim()
        .to_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if matches!(s.as_str(), "c" | "center" | "centre") {
        return Some(0.0);
    }
    let side = |c: char| match c {
        'l' => Some(-1.0),
        'r' => Some(1.0),
        _ => None,
    };
    let number = |t: &str| t.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0);
    if let Some(sign) = s.chars().last().and_then(side) {
        return number(&s[..s.len() - 1]).map(|n| pan_from_units(n * sign));
    }
    if let Some(sign) = s.chars().next().and_then(side) {
        return number(&s[1..]).map(|n| pan_from_units(n * sign));
    }
    s.parse::<f64>()
        .ok()
        .filter(|n| n.is_finite())
        .map(pan_from_units)
}

/// Nudge pan by readout units (50 = fully to one side).
pub fn nudge_pan(pan: f64, delta_units: f64) -> f64 {
    pan_from_units((pan * PAN_UNITS).round() + delta_units)
}

/// Most a break point pulls the thumb, in pixels.
pub const SNAP_PX: f64 = 6.0;
/// Where break points are close together, each pulls only this share of the gap to its
/// neighbours, so there is always free travel between them.
pub const SNAP_SHARE_OF_GAP: f64 = 0.3;

/// Snap a slider fraction `t` (0..1) onto the nearest break point whose pull reaches it.
/// `detents` are fractions in ascending order; `width` is the slider's length in pixels.
pub fn snap(t: f64, detents: &[f64], width: f64) -> f64 {
    let mut best = t;
    let mut best_px = f64::INFINITY;
    for (i, &d) in detents.iter().enumerate() {
        let mut radius = SNAP_PX;
        if i > 0 {
            radius = radius.min((d - detents[i - 1]).abs() * width * SNAP_SHARE_OF_GAP);
        }
        if let Some(&next) = detents.get(i + 1) {
            radius = radius.min((next - d).abs() * width * SNAP_SHARE_OF_GAP);
        }
        let dist = (t - d).abs() * width;
        if dist <= radius && dist < best_px {
            best = d;
            best_px = dist;
        }
    }
    best
}

/// Volume break points as fader fractions.
pub fn volume_detents() -> Vec<f64> {
    VOLUME_DETENTS_DB
        .iter()
        .map(|&db| db_to_fader(db))
        .collect()
}

/// Pan break points as slider fractions (pan -1..1 maps to 0..1).
pub fn pan_detents() -> Vec<f64> {
    PAN_DETENTS.iter().map(|&p| (p + 1.0) / 2.0).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    #[test]
    fn fader_taper_hits_unity_and_plus_six_exactly() {
        assert!(close(fader_to_db(FADER_UNITY), 0.0));
        assert!(close(fader_to_db(1.0), 6.0));
        assert_eq!(fader_to_db(0.0), f64::NEG_INFINITY);
        // 40% of travel: worked by hand, 6/log10(1.25) * log10(0.5) = -18.64 dB
        assert!((fader_to_db(0.4) - -18.64).abs() < 0.01);
        assert!(close(db_to_fader(fader_to_db(0.37)), 0.37));
    }

    #[test]
    fn db_readout() {
        assert_eq!(format_db(0.0), "0.0");
        assert_eq!(format_db(-0.04), "0.0");
        assert_eq!(format_db(3.0), "+3.0");
        assert_eq!(format_db(-12.34), "-12.3");
        assert_eq!(format_db(-61.0), "-∞");
        assert_eq!(format_db(f64::NEG_INFINITY), "-∞");
    }

    #[test]
    fn pan_readout() {
        assert_eq!(format_pan(0.0), "0");
        assert_eq!(format_pan(-0.4), "20L");
        assert_eq!(format_pan(1.0), "50R");
    }

    #[test]
    fn typed_volume() {
        assert_eq!(parse_db("-6 dB"), Some(-6.0));
        assert_eq!(parse_db("+3"), Some(3.0));
        assert_eq!(parse_db("-inf"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_db("20"), Some(6.0));
        assert_eq!(parse_db("-70"), Some(f64::NEG_INFINITY));
        assert_eq!(parse_db("abc"), None);
        assert_eq!(parse_db(""), None);
    }

    #[test]
    fn volume_nudge() {
        assert_eq!(nudge_db(-6.0, 1.0), -5.0);
        assert_eq!(nudge_db(-6.0, 0.1), -5.9);
        assert_eq!(nudge_db(5.5, 1.0), 6.0);
        assert_eq!(nudge_db(f64::NEG_INFINITY, 1.0), -60.0);
        assert_eq!(nudge_db(f64::NEG_INFINITY, -1.0), f64::NEG_INFINITY);
        assert_eq!(nudge_db(-59.5, -1.0), f64::NEG_INFINITY);
    }

    #[test]
    fn typed_pan() {
        let f = |s| parse_pan(s).map(format_pan);
        assert_eq!(f("20L").as_deref(), Some("20L"));
        assert_eq!(f("L20").as_deref(), Some("20L"));
        assert_eq!(f("20 r").as_deref(), Some("20R"));
        assert_eq!(f("-20").as_deref(), Some("20L"));
        assert_eq!(f("+20").as_deref(), Some("20R"));
        assert_eq!(f("c").as_deref(), Some("0"));
        assert_eq!(f("80R").as_deref(), Some("50R"));
        assert_eq!(f("x"), None);
        assert_eq!(f("L"), None);
    }

    #[test]
    fn pan_nudge() {
        assert_eq!(format_pan(nudge_pan(0.0, -5.0)), "5L");
        assert_eq!(format_pan(nudge_pan(1.0, 5.0)), "50R");
    }

    #[test]
    fn snapping_pulls_within_six_pixels_only() {
        let detents = [0.5];
        assert_eq!(snap(0.5 + 5.0 / 200.0, &detents, 200.0), 0.5);
        assert_eq!(snap(0.5 + 7.0 / 200.0, &detents, 200.0), 0.5 + 7.0 / 200.0);
    }

    #[test]
    fn close_break_points_leave_free_travel_between_them() {
        // 10px apart on a 100px slider: each pulls at most 3px.
        let detents = [0.4, 0.5];
        assert_eq!(snap(0.4 + 0.02, &detents, 100.0), 0.4);
        let midway = 0.45;
        assert_eq!(snap(midway, &detents, 100.0), midway);
    }

    #[test]
    fn detents_map_onto_the_sliders() {
        let v = volume_detents();
        assert!(close(v[13], FADER_UNITY)); // 0 dB
        assert!(close(*v.last().unwrap(), 1.0)); // +6 dB is the top
        assert_eq!(pan_detents(), vec![0.25, 0.5, 0.75]);
    }
}
