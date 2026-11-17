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
}
