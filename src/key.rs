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
    let steps = reference.number.abs_diff(other.number);
    if reference.minor == other.minor && steps <= 1 {
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
    fn a_neighbouring_key_on_the_wheel_is_green() {
        // 8A and 9A are adjacent on the Camelot wheel.
        assert_eq!(compatibility(key(8, true), key(9, true)), Compatibility::Green);
    }
}
