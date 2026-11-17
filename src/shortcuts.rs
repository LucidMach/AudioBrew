/// What a key press asks the app to do. Views map these to actions; no logic lives in views.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    PlayPause,
    Rewind,
    Pause,
    Forward,
    MuteFocused,
    SoloFocused,
    ToggleHelp,
    Undo,
    Redo,
}

pub const ALL_COMMANDS: [Command; 9] = [
    Command::PlayPause,
    Command::Rewind,
    Command::Pause,
    Command::Forward,
    Command::MuteFocused,
    Command::SoloFocused,
    Command::ToggleHelp,
    Command::Undo,
    Command::Redo,
];

#[derive(Debug, Clone, Copy)]
pub struct KeyPress<'a> {
    /// Lower-case key name: a letter, "space", or a punctuation character.
    pub key: &'a str,
    pub cmd: bool,
    pub shift: bool,
}

pub struct Binding {
    pub key: &'static str,
    pub cmd: bool,
    pub shift: bool,
    pub command: Command,
    /// How the overlay writes the keys.
    pub label: &'static str,
    pub description: &'static str,
}

const fn bind(
    key: &'static str,
    cmd: bool,
    shift: bool,
    command: Command,
    label: &'static str,
    description: &'static str,
) -> Binding {
    Binding {
        key,
        cmd,
        shift,
        command,
        label,
        description,
    }
}

/// The single keymap. J/K/L follow editing-software convention: J back, K pause, L forward.
pub const BINDINGS: &[Binding] = &[
    bind(
        "space",
        false,
        false,
        Command::PlayPause,
        "Space",
        "Play / pause",
    ),
    bind("j", false, false, Command::Rewind, "J", "Step back"),
    bind("k", false, false, Command::Pause, "K", "Pause"),
    bind("l", false, false, Command::Forward, "L", "Step forward"),
    bind(
        "m",
        false,
        false,
        Command::MuteFocused,
        "M",
        "Mute the focused lane",
    ),
    bind(
        "s",
        false,
        false,
        Command::SoloFocused,
        "S",
        "Solo the focused lane",
    ),
    bind(
        "?",
        false,
        false,
        Command::ToggleHelp,
        "?",
        "Show shortcuts",
    ),
    // "?" is Shift+/ on a US keyboard, so accept it that way too.
    bind("/", false, true, Command::ToggleHelp, "?", "Show shortcuts"),
    bind("z", true, false, Command::Undo, "⌘Z", "Undo"),
    bind("z", true, true, Command::Redo, "⌘⇧Z", "Redo"),
];

/// The command for a key press, or `None`. While a text field has focus nothing fires,
/// so typing a BPM never plays, mutes or solos.
pub fn resolve(press: KeyPress, text_focused: bool) -> Option<Command> {
    if text_focused {
        return None;
    }
    let key = press.key.to_lowercase();
    BINDINGS
        .iter()
        .find(|b| b.key == key && b.cmd == press.cmd && b.shift == press.shift)
        .map(|b| b.command)
}

pub struct HelpEntry {
    pub command: Command,
    pub keys: &'static str,
    pub description: &'static str,
}

/// One line per command for the "?" overlay, in a stable order.
pub fn help_entries() -> Vec<HelpEntry> {
    ALL_COMMANDS
        .iter()
        .filter_map(|&command| {
            let b = BINDINGS.iter().find(|b| b.command == command)?;
            Some(HelpEntry {
                command,
                keys: b.label,
                description: b.description,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn press(key: &str) -> KeyPress<'_> {
        KeyPress {
            key,
            cmd: false,
            shift: false,
        }
    }

    #[test]
    fn the_documented_keys_map_to_their_commands() {
        assert_eq!(resolve(press("space"), false), Some(Command::PlayPause));
        assert_eq!(resolve(press("j"), false), Some(Command::Rewind));
        assert_eq!(resolve(press("k"), false), Some(Command::Pause));
        assert_eq!(resolve(press("l"), false), Some(Command::Forward));
        assert_eq!(resolve(press("m"), false), Some(Command::MuteFocused));
        assert_eq!(resolve(press("s"), false), Some(Command::SoloFocused));
        assert_eq!(resolve(press("?"), false), Some(Command::ToggleHelp));
    }

    #[test]
    fn undo_and_redo_use_cmd_z_and_cmd_shift_z() {
        let z = |cmd, shift| KeyPress {
            key: "z",
            cmd,
            shift,
        };
        assert_eq!(resolve(z(true, false), false), Some(Command::Undo));
        assert_eq!(resolve(z(true, true), false), Some(Command::Redo));
        assert_eq!(resolve(z(false, false), false), None);
    }

    #[test]
    fn letters_are_matched_case_insensitively() {
        assert_eq!(resolve(press("J"), false), Some(Command::Rewind));
        assert_eq!(resolve(press("SPACE"), false), Some(Command::PlayPause));
    }

    #[test]
    fn a_modifier_the_binding_does_not_ask_for_makes_it_not_match() {
        // Cmd+M minimises the window on macOS; it must not mute a Track.
        assert_eq!(
            resolve(
                KeyPress {
                    key: "m",
                    cmd: true,
                    shift: false
                },
                false
            ),
            None
        );
        assert_eq!(
            resolve(
                KeyPress {
                    key: "space",
                    cmd: false,
                    shift: true
                },
                false
            ),
            None
        );
    }

    #[test]
    fn nothing_fires_while_a_text_field_has_focus() {
        // Typing a BPM must not play, mute or solo anything.
        for key in ["space", "j", "k", "l", "m", "s", "?"] {
            assert_eq!(resolve(press(key), true), None, "{key}");
        }
        assert_eq!(
            resolve(
                KeyPress {
                    key: "z",
                    cmd: true,
                    shift: false
                },
                true
            ),
            None
        );
    }

    #[test]
    fn unknown_keys_do_nothing() {
        assert_eq!(resolve(press("q"), false), None);
        assert_eq!(resolve(press(""), false), None);
    }

    #[test]
    fn no_two_bindings_share_a_key_combination() {
        for (i, a) in BINDINGS.iter().enumerate() {
            for b in &BINDINGS[i + 1..] {
                assert!(
                    (a.key, a.cmd, a.shift) != (b.key, b.cmd, b.shift),
                    "duplicate binding for {}",
                    a.key
                );
            }
        }
    }

    #[test]
    fn every_command_appears_once_in_the_help_overlay() {
        let help = help_entries();
        for command in ALL_COMMANDS {
            assert_eq!(
                help.iter().filter(|e| e.command == command).count(),
                1,
                "{command:?}"
            );
        }
        assert!(
            help.iter()
                .all(|e| !e.keys.is_empty() && !e.description.is_empty())
        );
    }

    #[test]
    fn the_help_overlay_opens_with_question_mark_or_shift_slash() {
        assert_eq!(resolve(press("?"), false), Some(Command::ToggleHelp));
        let shift_slash = KeyPress {
            key: "/",
            cmd: false,
            shift: true,
        };
        assert_eq!(resolve(shift_slash, false), Some(Command::ToggleHelp));
        assert_eq!(resolve(press("/"), false), None);
    }
}
