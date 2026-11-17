/// A musical key on the Camelot wheel: number 1-12, and `minor` (the "A" ring) or major ("B").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub number: u8,
    pub minor: bool,
}

impl Key {
    /// A Key, or `None` if the number is not on the wheel (1-12).
    pub fn new(number: u8, minor: bool) -> Option<Key> {
        (1..=12).contains(&number).then_some(Key { number, minor })
    }

    fn is_valid(self) -> bool {
        Key::new(self.number, self.minor).is_some()
    }
}

/// The colour of the Key badge, relative to the first Track's key.
#[derive(Debug, PartialEq, Eq)]
pub enum Compatibility {
    Green,
    Amber,
    Red,
}

pub fn compatibility(reference: Key, other: Key) -> Compatibility {
    if !reference.is_valid() || !other.is_valid() {
        return Compatibility::Red;
    }
    let diff = reference.number.abs_diff(other.number);
    let steps = diff.min(12 - diff); // the wheel wraps: 12 sits next to 1
    let same_ring = reference.minor == other.minor;
    if (same_ring && steps <= 1) || (!same_ring && steps == 0) {
        Compatibility::Green
    } else if same_ring && steps == 2 {
        Compatibility::Amber
    } else {
        Compatibility::Red
    }
}

/// Pitch class (0 = C) of the major key sharing this Key's notes; a Camelot step is a fifth.
fn major_pitch_class(key: Key) -> u8 {
    (key.number * 7 + 4) % 12
}

/// Semitones (-5 to +6) to pitch-shift `other` so it fits `reference`, taking the shorter
/// way round. Relative major/minor share notes, so they need no shift. A tie at 6 is +6.
pub fn match_key_shift(reference: Key, other: Key) -> Option<i8> {
    if !reference.is_valid() || !other.is_valid() {
        return None;
    }
    let up = (major_pitch_class(reference) + 12 - major_pitch_class(other)) % 12;
    Some(if up > 6 { up as i8 - 12 } else { up as i8 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(number: u8, minor: bool) -> Key {
        Key { number, minor }
    }

    #[test]
    fn two_steps_around_the_wheel_is_amber() {
        // 8A to 10A is a risky but workable energy-boost mix.
        assert_eq!(
            compatibility(key(8, true), key(10, true)),
            Compatibility::Amber
        );
    }

    #[test]
    fn distant_keys_are_red() {
        assert_eq!(
            compatibility(key(8, true), key(2, true)),
            Compatibility::Red
        );
        assert_eq!(
            compatibility(key(8, true), key(3, false)),
            Compatibility::Red
        );
    }

    #[test]
    fn the_wheel_wraps_around_from_12_to_1() {
        assert_eq!(
            compatibility(key(12, true), key(1, true)),
            Compatibility::Green
        );
    }

    #[test]
    fn the_relative_major_or_minor_with_the_same_number_is_green() {
        // 8A (A minor) and 8B (C major) share the same notes.
        assert_eq!(
            compatibility(key(8, true), key(8, false)),
            Compatibility::Green
        );
    }

    #[test]
    fn a_neighbouring_key_on_the_wheel_is_green() {
        // 8A and 9A are adjacent on the Camelot wheel.
        assert_eq!(
            compatibility(key(8, true), key(9, true)),
            Compatibility::Green
        );
    }

    #[test]
    fn a_key_number_outside_1_to_12_is_rejected() {
        assert_eq!(Key::new(0, true), None);
        assert_eq!(Key::new(13, false), None);
        assert_eq!(Key::new(255, true), None);
        assert_eq!(Key::new(1, true), Some(key(1, true)));
        assert_eq!(Key::new(12, false), Some(key(12, false)));
    }

    #[test]
    fn an_invalid_key_number_is_red_instead_of_panicking() {
        assert_eq!(
            compatibility(key(8, true), key(13, true)),
            Compatibility::Red
        );
        assert_eq!(
            compatibility(key(0, true), key(8, true)),
            Compatibility::Red
        );
        assert_eq!(
            compatibility(key(255, true), key(255, true)),
            Compatibility::Red
        );
    }

    #[test]
    fn compatibility_is_symmetric_for_every_pair_of_keys() {
        for a in 1..=12 {
            for b in 1..=12 {
                for (ra, rb) in [(true, true), (true, false), (false, true), (false, false)] {
                    assert_eq!(
                        compatibility(key(a, ra), key(b, rb)),
                        compatibility(key(b, rb), key(a, ra)),
                        "{a}{ra} vs {b}{rb}"
                    );
                }
            }
        }
    }

    #[test]
    fn three_steps_on_the_same_ring_is_red_and_two_is_the_amber_edge() {
        assert_eq!(
            compatibility(key(8, true), key(11, true)),
            Compatibility::Red
        );
        assert_eq!(
            compatibility(key(8, true), key(6, true)),
            Compatibility::Amber
        );
    }

    #[test]
    fn matching_8a_to_a_reference_of_10a_shifts_up_two_semitones() {
        // 8A is A minor, 10A is B minor: A up to B is +2.
        assert_eq!(match_key_shift(key(10, true), key(8, true)), Some(2));
        assert_eq!(match_key_shift(key(8, true), key(10, true)), Some(-2));
    }

    #[test]
    fn a_neighbour_on_the_wheel_is_five_semitones_away_not_one() {
        // 12A is C# minor, 1A is G# minor: a fifth apart, so the short way is -5.
        assert_eq!(match_key_shift(key(1, true), key(12, true)), Some(-5));
        assert_eq!(match_key_shift(key(12, true), key(1, true)), Some(5));
    }

    #[test]
    fn relative_major_and_minor_need_no_shift() {
        // 8A (A minor) and 8B (C major) share notes.
        assert_eq!(match_key_shift(key(8, true), key(8, false)), Some(0));
        assert_eq!(match_key_shift(key(8, false), key(8, true)), Some(0));
    }

    #[test]
    fn a_tritone_apart_is_a_shift_of_plus_six() {
        // 2A (D#m... pitch class 3) vs 8A (A minor, pitch class 9): exactly 6 either way; pinned to +6.
        assert_eq!(match_key_shift(key(2, true), key(8, true)), Some(6));
        assert_eq!(match_key_shift(key(8, true), key(2, true)), Some(6));
    }

    #[test]
    fn a_key_off_the_wheel_has_no_shift() {
        assert_eq!(match_key_shift(key(0, true), key(8, true)), None);
        assert_eq!(match_key_shift(key(8, true), key(13, false)), None);
    }

    #[test]
    fn every_shift_is_within_six_semitones() {
        for a in 1..=12 {
            for b in 1..=12 {
                let shift = match_key_shift(key(a, true), key(b, false)).unwrap();
                assert!((-5..=6).contains(&shift), "{a} vs {b}: {shift}");
            }
        }
    }
}
