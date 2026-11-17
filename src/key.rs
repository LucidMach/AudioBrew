/// A musical key on the Camelot wheel: number 1-12, and `minor` (the "A" ring) or major ("B").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    pub number: u8,
    pub minor: bool,
}

/// The colour of the Key badge, relative to the first Track's key.
#[derive(Debug, PartialEq, Eq)]
pub enum Compatibility {
    Green,
    Amber,
    Red,
}

pub fn compatibility(reference: Key, other: Key) -> Compatibility {
    let diff = reference.number.abs_diff(other.number);
    let steps = diff.min(12 - diff); // the wheel wraps: 12 sits next to 1
    let same_ring = reference.minor == other.minor;
    if (same_ring && steps <= 1) || (!same_ring && steps == 0) {
        Compatibility::Green
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
    fn the_wheel_wraps_around_from_12_to_1() {
        assert_eq!(compatibility(key(12, true), key(1, true)), Compatibility::Green);
    }

    #[test]
    fn the_relative_major_or_minor_with_the_same_number_is_green() {
        // 8A (A minor) and 8B (C major) share the same notes.
        assert_eq!(compatibility(key(8, true), key(8, false)), Compatibility::Green);
    }

    #[test]
    fn a_neighbouring_key_on_the_wheel_is_green() {
        // 8A and 9A are adjacent on the Camelot wheel.
        assert_eq!(compatibility(key(8, true), key(9, true)), Compatibility::Green);
    }
}
