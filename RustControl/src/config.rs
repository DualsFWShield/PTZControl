//! Persistent application configuration.
//!
//! The original MFC application stored its settings in the registry.  The Rust
//! version keeps everything in a human-readable JSON file in `%APPDATA%` so it
//! can be inspected, backed up and shipped with the portable binary.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

pub const NUM_PRESETS: usize = 8;
pub const NUM_CAMERAS: usize = 3;

pub const DEFAULT_DEVICE_TOKENS: &[&str] = &[
    "PTZ Pro",
    "Logi Rally",
    "ConferenceCam",
    "Logi Group Camera",
];

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub axes: Axes,
    pub presets: Vec<Vec<String>>,
    pub hotkeys: Vec<HotkeyBinding>,
    pub window: WindowState,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            axes: Axes::default(),
            presets: vec![vec![String::new(); NUM_PRESETS]; NUM_CAMERAS],
            hotkeys: default_hotkeys(),
            window: WindowState::default(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub always_on_top: bool,
    pub reset_on_start: bool,
    pub guard: bool,
    pub use_logitech_motion: bool,
    pub motor_interval_ms: u32,
    pub device_tokens: Vec<String>,
}

impl Default for General {
    fn default() -> Self {
        Self {
            always_on_top: true,
            reset_on_start: true,
            guard: true,
            use_logitech_motion: true,
            motor_interval_ms: 70,
            device_tokens: DEFAULT_DEVICE_TOKENS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Axes {
    pub invert_pan: bool,
    pub invert_tilt: bool,
    pub invert_zoom: bool,
    pub invert_focus: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowState {
    pub x: Option<f32>,
    pub y: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hotkey {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    /// Canonical key name. See [`crate::hotkeys::KEY_OPTIONS`].
    pub key: String,
}

impl Hotkey {
    pub fn new(ctrl: bool, alt: bool, shift: bool, win: bool, key: &str) -> Self {
        Self {
            ctrl,
            alt,
            shift,
            win,
            key: key.to_string(),
        }
    }

    pub fn has_modifier(&self) -> bool {
        self.ctrl || self.alt || self.shift || self.win
    }

    pub fn label(&self) -> String {
        crate::hotkeys::describe(self)
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum HotkeyAction {
    Camera1,
    Camera2,
    Camera3,
    Home,
    Preset1,
    Preset2,
    Preset3,
    Preset4,
    Preset5,
    Preset6,
    Preset7,
    Preset8,
    Memory,
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    ZoomIn,
    ZoomOut,
    ToggleAutoFocus,
    OpenSettings,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 21] = [
        HotkeyAction::Camera1,
        HotkeyAction::Camera2,
        HotkeyAction::Camera3,
        HotkeyAction::Home,
        HotkeyAction::Preset1,
        HotkeyAction::Preset2,
        HotkeyAction::Preset3,
        HotkeyAction::Preset4,
        HotkeyAction::Preset5,
        HotkeyAction::Preset6,
        HotkeyAction::Preset7,
        HotkeyAction::Preset8,
        HotkeyAction::Memory,
        HotkeyAction::PanLeft,
        HotkeyAction::PanRight,
        HotkeyAction::PanUp,
        HotkeyAction::PanDown,
        HotkeyAction::ZoomIn,
        HotkeyAction::ZoomOut,
        HotkeyAction::ToggleAutoFocus,
        HotkeyAction::OpenSettings,
    ];

    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::Camera1 => "Caméra 1",
            HotkeyAction::Camera2 => "Caméra 2",
            HotkeyAction::Camera3 => "Caméra 3",
            HotkeyAction::Home => "Position d'accueil",
            HotkeyAction::Preset1 => "Préréglage 1",
            HotkeyAction::Preset2 => "Préréglage 2",
            HotkeyAction::Preset3 => "Préréglage 3",
            HotkeyAction::Preset4 => "Préréglage 4",
            HotkeyAction::Preset5 => "Préréglage 5",
            HotkeyAction::Preset6 => "Préréglage 6",
            HotkeyAction::Preset7 => "Préréglage 7",
            HotkeyAction::Preset8 => "Préréglage 8",
            HotkeyAction::Memory => "Mode mémoire (mémoriser)",
            HotkeyAction::PanLeft => "Panoramique à gauche",
            HotkeyAction::PanRight => "Panoramique à droite",
            HotkeyAction::PanUp => "Inclinaison vers le haut",
            HotkeyAction::PanDown => "Inclinaison vers le bas",
            HotkeyAction::ZoomIn => "Zoom avant",
            HotkeyAction::ZoomOut => "Zoom arrière",
            HotkeyAction::ToggleAutoFocus => "Autofocus (AF)",
            HotkeyAction::OpenSettings => "Ouvrir les paramètres",
        }
    }

    pub fn group(self) -> &'static str {
        match self {
            HotkeyAction::Camera1 | HotkeyAction::Camera2 | HotkeyAction::Camera3 => "Caméras",
            HotkeyAction::Home
            | HotkeyAction::Preset1
            | HotkeyAction::Preset2
            | HotkeyAction::Preset3
            | HotkeyAction::Preset4
            | HotkeyAction::Preset5
            | HotkeyAction::Preset6
            | HotkeyAction::Preset7
            | HotkeyAction::Preset8
            | HotkeyAction::Memory => "Préréglages",
            HotkeyAction::PanLeft
            | HotkeyAction::PanRight
            | HotkeyAction::PanUp
            | HotkeyAction::PanDown
            | HotkeyAction::ZoomIn
            | HotkeyAction::ZoomOut
            | HotkeyAction::ToggleAutoFocus => "Contrôle",
            HotkeyAction::OpenSettings => "Application",
        }
    }

    pub fn preset_slot(self) -> Option<usize> {
        match self {
            HotkeyAction::Preset1 => Some(0),
            HotkeyAction::Preset2 => Some(1),
            HotkeyAction::Preset3 => Some(2),
            HotkeyAction::Preset4 => Some(3),
            HotkeyAction::Preset5 => Some(4),
            HotkeyAction::Preset6 => Some(5),
            HotkeyAction::Preset7 => Some(6),
            HotkeyAction::Preset8 => Some(7),
            _ => None,
        }
    }
}

/// Identity of a shortcut. A shortcut may target a specific camera, which is
/// how the original application exposes one global Home/preset set per camera.
pub type HotkeyId = (HotkeyAction, Option<usize>);

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct HotkeyBinding {
    pub action: HotkeyAction,
    /// `None` acts on the camera currently selected in the UI.
    pub camera: Option<usize>,
    pub hotkey: Option<Hotkey>,
}

impl HotkeyBinding {
    pub fn new(action: HotkeyAction, camera: Option<usize>, hotkey: Option<Hotkey>) -> Self {
        Self {
            action,
            camera,
            hotkey,
        }
    }

    pub fn id(&self) -> HotkeyId {
        (self.action, self.camera)
    }

    pub fn label(&self) -> String {
        match self.camera {
            Some(camera) => format!("Caméra {} · {}", camera + 1, self.action.label()),
            None => self.action.label().to_string(),
        }
    }

    pub fn group(&self) -> String {
        match self.camera {
            Some(camera) => format!("Caméra {}", camera + 1),
            None => self.action.group().to_string(),
        }
    }
}

pub fn default_hotkeys() -> Vec<HotkeyBinding> {
    use HotkeyAction::*;
    let mut out = Vec::with_capacity(40);

    // The original global layout: Win+Numpad for camera 1, Win+Ctrl+Numpad for
    // camera 2 and Win+Alt+Numpad for camera 3.
    for (camera, ctrl, alt) in [(0usize, false, false), (1, true, false), (2, false, true)] {
        out.push(HotkeyBinding::new(
            Home,
            Some(camera),
            Some(Hotkey::new(ctrl, alt, false, true, "Numpad0")),
        ));
        for (slot, action) in [
            Preset1, Preset2, Preset3, Preset4, Preset5, Preset6, Preset7, Preset8,
        ]
        .into_iter()
        .enumerate()
        {
            out.push(HotkeyBinding::new(
                action,
                Some(camera),
                Some(Hotkey::new(
                    ctrl,
                    alt,
                    false,
                    true,
                    &format!("Numpad{}", slot + 1),
                )),
            ));
        }
    }

    for (camera, action, key) in [(0, Camera1, "1"), (1, Camera2, "2"), (2, Camera3, "3")] {
        let _ = camera;
        out.push(HotkeyBinding::new(
            action,
            None,
            Some(Hotkey::new(true, true, false, false, key)),
        ));
    }
    for (action, key) in [
        (PanLeft, "ArrowLeft"),
        (PanRight, "ArrowRight"),
        (PanUp, "ArrowUp"),
        (PanDown, "ArrowDown"),
    ] {
        out.push(HotkeyBinding::new(
            action,
            None,
            Some(Hotkey::new(false, false, true, true, key)),
        ));
    }
    out.push(HotkeyBinding::new(
        ZoomIn,
        None,
        Some(Hotkey::new(false, false, true, true, "PageUp")),
    ));
    out.push(HotkeyBinding::new(
        ZoomOut,
        None,
        Some(Hotkey::new(false, false, true, true, "PageDown")),
    ));
    out.push(HotkeyBinding::new(
        Memory,
        None,
        Some(Hotkey::new(false, false, true, true, "M")),
    ));
    out.push(HotkeyBinding::new(
        ToggleAutoFocus,
        None,
        Some(Hotkey::new(false, false, true, true, "F")),
    ));
    out.push(HotkeyBinding::new(
        OpenSettings,
        None,
        Some(Hotkey::new(false, false, true, true, "S")),
    ));
    out
}

impl Config {
    pub fn normalize(&mut self) {
        self.general.motor_interval_ms = self.general.motor_interval_ms.clamp(10, 1000);
        if self.general.device_tokens.is_empty() {
            self.general.device_tokens = General::default().device_tokens;
        }

        self.presets.resize_with(NUM_CAMERAS, Vec::new);
        for camera in &mut self.presets {
            camera.resize_with(NUM_PRESETS, String::new);
        }

        // Fill in shortcuts introduced by later versions without losing a user's
        // explicit choice (including an explicitly disabled hotkey).
        let defaults = default_hotkeys();
        for default in defaults {
            if !self.hotkeys.iter().any(|b| b.id() == default.id()) {
                self.hotkeys.push(default);
            }
        }
        self.hotkeys.retain(|b| {
            HotkeyAction::ALL.contains(&b.action)
                && b.camera.map(|camera| camera < NUM_CAMERAS).unwrap_or(true)
        });
        self.hotkeys.sort_by_key(|b| (b.camera.unwrap_or(usize::MAX), b.action));
        self.hotkeys.dedup_by_key(|b| b.id());

        // Reject nonsensical keys early. The hotkey editor lets the user fix
        // them, and the platform layer only registers known keys.
        for binding in &mut self.hotkeys {
            if let Some(hotkey) = &binding.hotkey {
                if crate::hotkeys::key_to_vk(&hotkey.key).is_none() {
                    binding.hotkey = None;
                }
            }
        }
    }

    pub fn preset_label(&self, camera: usize, slot: usize) -> &str {
        self.presets
            .get(camera)
            .and_then(|c| c.get(slot))
            .map(String::as_str)
            .unwrap_or("")
    }

    pub fn binding(&self, index: usize) -> Option<&HotkeyBinding> {
        self.hotkeys.get(index)
    }
}

pub fn config_path() -> PathBuf {
    if let Ok(path) = std::env::var("PTZCONTROL_CONFIG") {
        if !path.trim().is_empty() {
            return PathBuf::from(path);
        }
    }
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("PTZControl").join("config.json")
}

pub fn load() -> Config {
    let path = config_path();
    match fs::read_to_string(&path) {
        Ok(text) => match serde_json::from_str::<Config>(&text) {
            Ok(mut config) => {
                config.normalize();
                config
            }
            Err(err) => {
                eprintln!("PTZControl: configuration invalide ({err}), utilisation des valeurs par défaut");
                Config::default()
            }
        },
        Err(_) => Config::default(),
    }
}

pub fn save(config: &Config) -> std::io::Result<()> {
    let path = config_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let text = serde_json::to_string_pretty(config)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::Other, err))?;
    fs::write(&path, text)
}
