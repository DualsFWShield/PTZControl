//! Command line parsing and the small IPC message exchanged with an already
//! running instance.  The syntax intentionally mirrors the original MFC tool so
//! existing Bitfocus Companion / Stream Deck buttons keep working.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteCommand {
    /// Zero-based camera index.
    pub camera: usize,
    pub action: RemoteAction,
    pub steps: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemoteAction {
    Home,
    Pan(i32),
    Tilt(i32),
    Zoom(i32),
    Restore(usize),
    Store(usize),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RemoteMessage {
    pub activate: bool,
    pub command: Option<RemoteCommand>,
}

#[derive(Clone, Debug, Default)]
pub struct Options {
    pub device: Option<String>,
    pub show_devices: bool,
    pub no_reset: bool,
    pub no_guard: bool,
    pub command: Option<RemoteCommand>,
}

#[derive(Clone, Debug, Default)]
pub struct Parsed {
    pub options: Options,
    pub syntax_error: bool,
}

#[derive(Default)]
struct Fields {
    camera: usize,
    steps: u32,
    home: bool,
    pan: i32,
    tilt: i32,
    zoom: i32,
    restore: Option<usize>,
    store: Option<usize>,
}

pub fn parse() -> Parsed {
    parse_from(std::env::args().skip(1))
}

pub fn parse_from<I: IntoIterator<Item = String>>(args: I) -> Parsed {
    let mut out = Parsed::default();
    out.options.command = None;
    let mut f = Fields {
        steps: 1,
        ..Fields::default()
    };

    for raw in args {
        let mut arg = raw.trim();
        if arg.is_empty() {
            continue;
        }
        if arg.starts_with('-') || arg.starts_with('/') {
            arg = &arg[1..];
        }
        let lower = arg.to_ascii_lowercase();
        if lower == "noreset" {
            out.options.no_reset = true;
        } else if lower == "noguard" {
            out.options.no_guard = true;
        } else if lower == "showdevices" {
            out.options.show_devices = true;
        } else if let Some(value) = lower.strip_prefix("device:") {
            // Preserve the original case for the friendly-name match.
            let value = &arg[arg.len() - value.len()..];
            let value = value.trim().trim_matches('"').trim();
            out.options.device = Some(value.to_string());
        } else if let Some(value) = lower.strip_prefix("n:") {
            match value.parse::<u32>() {
                Ok(n) if n >= 1 => f.steps = n.min(10),
                _ => out.syntax_error = true,
            }
        } else if let Some(value) = lower.strip_prefix("restore:") {
            match value.parse::<usize>() {
                Ok(n) if (1..=8).contains(&n) => f.restore = Some(n - 1),
                _ => out.syntax_error = true,
            }
        } else if let Some(value) = lower.strip_prefix("store:") {
            match value.parse::<usize>() {
                Ok(n) if (1..=8).contains(&n) => f.store = Some(n - 1),
                _ => out.syntax_error = true,
            }
        } else if lower == "move_up" {
            f.tilt = 1;
        } else if lower == "move_down" {
            f.tilt = -1;
        } else if lower == "move_left" {
            f.pan = -1;
        } else if lower == "move_right" {
            f.pan = 1;
        } else if lower == "move_home" || lower == "home" {
            f.home = true;
        } else if lower == "zoom_in" || lower == "zoom+" {
            f.zoom = 1;
        } else if lower == "zoom_out" || lower == "zoom-" {
            f.zoom = -1;
        } else if let Ok(n) = lower.parse::<usize>() {
            if (1..=3).contains(&n) {
                f.camera = n - 1;
            } else {
                out.syntax_error = true;
            }
        }
        // Unknown switches are ignored, like the original MFC parser.
    }

    // Original execution order: Home -> Pan -> Tilt -> Zoom -> Restore -> Store.
    let action = if f.home {
        Some(RemoteAction::Home)
    } else if f.pan != 0 {
        Some(RemoteAction::Pan(f.pan))
    } else if f.tilt != 0 {
        Some(RemoteAction::Tilt(f.tilt))
    } else if f.zoom != 0 {
        Some(RemoteAction::Zoom(f.zoom))
    } else if let Some(slot) = f.restore {
        Some(RemoteAction::Restore(slot))
    } else if let Some(slot) = f.store {
        Some(RemoteAction::Store(slot))
    } else {
        None
    };

    out.options.command = action.map(|action| RemoteCommand {
        camera: f.camera,
        action,
        steps: f.steps.max(1),
    });
    out
}

pub fn encode(message: &RemoteMessage) -> Vec<u8> {
    serde_json::to_vec(message).unwrap_or_default()
}

pub fn decode(bytes: &[u8]) -> Option<RemoteMessage> {
    serde_json::from_slice(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_legacy_commands() {
        let parsed = parse_from([
            "-2".to_string(),
            "-move_left".to_string(),
            "-n:3".to_string(),
        ]);
        assert!(!parsed.syntax_error);
        let command = parsed.options.command.expect("command");
        assert_eq!(command.camera, 1);
        assert_eq!(command.steps, 3);
        assert!(matches!(command.action, RemoteAction::Pan(-1)));
    }

    #[test]
    fn priority_matches_original_application() {
        let parsed = parse_from([
            "-restore:4".to_string(),
            "-move_home".to_string(),
            "-zoom_in".to_string(),
        ]);
        assert!(matches!(
            parsed.options.command.expect("command").action,
            RemoteAction::Home
        ));
    }

    #[test]
    fn rejects_bad_preset_numbers() {
        let parsed = parse_from(["-restore:9".to_string()]);
        assert!(parsed.syntax_error);
    }
}
