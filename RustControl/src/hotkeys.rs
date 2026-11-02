//! Keyboard mapping and global hotkey registration helpers.
//!
//! Windows `RegisterHotKey` works even when the application is not focused,
//! which is exactly what a PTZ operator needs while OBS/vMix has the focus.

use crate::config::Hotkey;

/// A curated list of keys that can be assigned to a global shortcut.
/// The canonical name is stored in the JSON configuration.
pub const KEY_OPTIONS: &[(&str, &str)] = &[
    ("F1", "F1"), ("F2", "F2"), ("F3", "F3"), ("F4", "F4"),
    ("F5", "F5"), ("F6", "F6"), ("F7", "F7"), ("F8", "F8"),
    ("F9", "F9"), ("F10", "F10"), ("F11", "F11"), ("F12", "F12"),
    ("F13", "F13"), ("F14", "F14"), ("F15", "F15"), ("F16", "F16"),
    ("F17", "F17"), ("F18", "F18"), ("F19", "F19"), ("F20", "F20"),
    ("F21", "F21"), ("F22", "F22"), ("F23", "F23"), ("F24", "F24"),
    ("A", "A"), ("B", "B"), ("C", "C"), ("D", "D"), ("E", "E"),
    ("F", "F"), ("G", "G"), ("H", "H"), ("I", "I"), ("J", "J"),
    ("K", "K"), ("L", "L"), ("M", "M"), ("N", "N"), ("O", "O"),
    ("P", "P"), ("Q", "Q"), ("R", "R"), ("S", "S"), ("T", "T"),
    ("U", "U"), ("V", "V"), ("W", "W"), ("X", "X"), ("Y", "Y"),
    ("Z", "Z"),
    ("0", "0"), ("1", "1"), ("2", "2"), ("3", "3"), ("4", "4"),
    ("5", "5"), ("6", "6"), ("7", "7"), ("8", "8"), ("9", "9"),
    ("Numpad0", "Pavé num. 0"), ("Numpad1", "Pavé num. 1"),
    ("Numpad2", "Pavé num. 2"), ("Numpad3", "Pavé num. 3"),
    ("Numpad4", "Pavé num. 4"), ("Numpad5", "Pavé num. 5"),
    ("Numpad6", "Pavé num. 6"), ("Numpad7", "Pavé num. 7"),
    ("Numpad8", "Pavé num. 8"), ("Numpad9", "Pavé num. 9"),
    ("NumpadAdd", "Pavé num. +"), ("NumpadSubtract", "Pavé num. -"),
    ("NumpadMultiply", "Pavé num. *"), ("NumpadDivide", "Pavé num. /"),
    ("NumpadDecimal", "Pavé num. ."),
    ("ArrowLeft", "Flèche gauche"), ("ArrowRight", "Flèche droite"),
    ("ArrowUp", "Flèche haut"), ("ArrowDown", "Flèche bas"),
    ("PageUp", "Page haut"), ("PageDown", "Page bas"),
    ("Home", "Début"), ("End", "Fin"),
    ("Insert", "Inser"), ("Delete", "Suppr"),
    ("Space", "Espace"), ("Tab", "Tab"), ("Enter", "Entrée"),
    ("Escape", "Échap"), ("Backspace", "Retour arrière"),
    ("Pause", "Pause"), ("ScrollLock", "Arrêt défil"),
    ("Semicolon", ";"), ("Equals", "="), ("Comma", ","),
    ("Minus", "-"), ("Period", "."), ("Slash", "/"),
    ("Backquote", "`"), ("BracketLeft", "["), ("Backslash", "\\"),
    ("BracketRight", "]"), ("Quote", "'"),
];

pub fn key_to_vk(key: &str) -> Option<u32> {
    let key = key.trim();
    if key.len() == 1 {
        let ch = key.chars().next()?;
        if ch.is_ascii_alphabetic() {
            return Some(ch.to_ascii_uppercase() as u32);
        }
        if ch.is_ascii_digit() {
            return Some(ch as u32);
        }
    }
    if let Some(n) = key.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=24).contains(&n) {
            return Some(0x70 + n - 1);
        }
    }
    if let Some(n) = key.strip_prefix("Numpad").and_then(|n| n.parse::<u32>().ok()) {
        if n <= 9 {
            return Some(0x60 + n);
        }
    }
    let vk = match key {
        "NumpadAdd" => 0x6B,
        "NumpadSubtract" => 0x6D,
        "NumpadMultiply" => 0x6A,
        "NumpadDivide" => 0x6F,
        "NumpadDecimal" => 0x6E,
        "ArrowLeft" => 0x25,
        "ArrowUp" => 0x26,
        "ArrowRight" => 0x27,
        "ArrowDown" => 0x28,
        "PageUp" => 0x21,
        "PageDown" => 0x22,
        "Home" => 0x24,
        "End" => 0x23,
        "Insert" => 0x2D,
        "Delete" => 0x2E,
        "Space" => 0x20,
        "Tab" => 0x09,
        "Enter" => 0x0D,
        "Escape" => 0x1B,
        "Backspace" => 0x08,
        "Pause" => 0x13,
        "ScrollLock" => 0x91,
        "Semicolon" => 0xBA,
        "Equals" => 0xBB,
        "Comma" => 0xBC,
        "Minus" => 0xBD,
        "Period" => 0xBE,
        "Slash" => 0xBF,
        "Backquote" => 0xC0,
        "BracketLeft" => 0xDB,
        "Backslash" => 0xDC,
        "BracketRight" => 0xDD,
        "Quote" => 0xDE,
        _ => return None,
    };
    Some(vk)
}

pub fn key_label(key: &str) -> &str {
    KEY_OPTIONS
        .iter()
        .find(|(canonical, _)| *canonical == key)
        .map(|(_, label)| *label)
        .unwrap_or(key)
}

pub fn describe(hotkey: &Hotkey) -> String {
    let mut parts = Vec::new();
    if hotkey.win {
        parts.push("Win");
    }
    if hotkey.ctrl {
        parts.push("Ctrl");
    }
    if hotkey.alt {
        parts.push("Alt");
    }
    if hotkey.shift {
        parts.push("Maj");
    }
    parts.push(key_label(&hotkey.key));
    parts.join(" + ")
}

pub fn key_from_egui(key: egui::Key) -> Option<&'static str> {
    use egui::Key;
    Some(match key {
        Key::F1 => "F1", Key::F2 => "F2", Key::F3 => "F3", Key::F4 => "F4",
        Key::F5 => "F5", Key::F6 => "F6", Key::F7 => "F7", Key::F8 => "F8",
        Key::F9 => "F9", Key::F10 => "F10", Key::F11 => "F11", Key::F12 => "F12",
        Key::F13 => "F13", Key::F14 => "F14", Key::F15 => "F15", Key::F16 => "F16",
        Key::F17 => "F17", Key::F18 => "F18", Key::F19 => "F19", Key::F20 => "F20",
        Key::A => "A", Key::B => "B", Key::C => "C", Key::D => "D", Key::E => "E",
        Key::F => "F", Key::G => "G", Key::H => "H", Key::I => "I", Key::J => "J",
        Key::K => "K", Key::L => "L", Key::M => "M", Key::N => "N", Key::O => "O",
        Key::P => "P", Key::Q => "Q", Key::R => "R", Key::S => "S", Key::T => "T",
        Key::U => "U", Key::V => "V", Key::W => "W", Key::X => "X", Key::Y => "Y",
        Key::Z => "Z",
        Key::Num0 => "0", Key::Num1 => "1", Key::Num2 => "2", Key::Num3 => "3",
        Key::Num4 => "4", Key::Num5 => "5", Key::Num6 => "6", Key::Num7 => "7",
        Key::Num8 => "8", Key::Num9 => "9",
        Key::ArrowLeft => "ArrowLeft", Key::ArrowRight => "ArrowRight",
        Key::ArrowUp => "ArrowUp", Key::ArrowDown => "ArrowDown",
        Key::PageUp => "PageUp", Key::PageDown => "PageDown",
        Key::Home => "Home", Key::End => "End",
        Key::Insert => "Insert", Key::Delete => "Delete",
        Key::Space => "Space", Key::Tab => "Tab", Key::Enter => "Enter",
        Key::Escape => "Escape", Key::Backspace => "Backspace",
        Key::Comma => "Comma", Key::Minus => "Minus", Key::Period => "Period",
        Key::Plus => "Equals", Key::Equals => "Equals", Key::Semicolon => "Semicolon",
        Key::Slash => "Slash", Key::Backslash => "Backslash",
        Key::OpenBracket => "BracketLeft", Key::CloseBracket => "BracketRight",
        Key::Backtick => "Backquote", Key::Quote => "Quote",
        _ => return None,
    })
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ModifierState {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
}

/// Read the real Windows modifier state. egui does not expose the Windows key,
/// so this is also used while the user records a shortcut.
pub fn current_modifiers() -> ModifierState {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
        let down = |vk: i32| (GetAsyncKeyState(vk) as u16 & 0x8000) != 0;
        ModifierState {
            ctrl: down(0x11),
            alt: down(0x12),
            shift: down(0x10),
            win: down(0x5B) || down(0x5C),
        }
    }
    #[cfg(not(windows))]
    {
        ModifierState::default()
    }
}

/// Modifier bits in the format expected by `RegisterHotKey`.
#[cfg(windows)]
pub fn modifier_bits(hotkey: &Hotkey) -> u32 {
    let mut bits = 0x4000; // MOD_NOREPEAT: one action per key press.
    if hotkey.alt {
        bits |= 0x0001;
    }
    if hotkey.ctrl {
        bits |= 0x0002;
    }
    if hotkey.shift {
        bits |= 0x0004;
    }
    if hotkey.win {
        bits |= 0x0008;
    }
    bits
}
