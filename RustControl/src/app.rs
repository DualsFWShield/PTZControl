//! Modern, always-on-top PTZ control surface.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Sense,
    Stroke, Vec2,
};

use crate::camera::{self, Camera, DeviceInfo, Range, NUM_PRESETS};
use crate::cli::{RemoteAction, RemoteCommand};
use crate::config::{self, Config, Hotkey, HotkeyAction, HotkeyId, NUM_CAMERAS};
use crate::hotkeys;
use crate::platform::{Platform, PlatformEvent};

const BG: Color32 = Color32::from_rgb(11, 15, 21);
const PANEL: Color32 = Color32::from_rgb(20, 26, 35);
const CARD: Color32 = Color32::from_rgb(29, 37, 48);
const CARD_HOVER: Color32 = Color32::from_rgb(38, 48, 62);
const BORDER: Color32 = Color32::from_rgb(49, 61, 77);
const TEXT: Color32 = Color32::from_rgb(230, 237, 243);
const MUTED: Color32 = Color32::from_rgb(139, 151, 168);
const ACCENT: Color32 = Color32::from_rgb(56, 189, 248);
const ACCENT_SOFT: Color32 = Color32::from_rgb(14, 116, 144);
const GREEN: Color32 = Color32::from_rgb(52, 211, 153);
const RED: Color32 = Color32::from_rgb(248, 113, 113);
const ORANGE: Color32 = Color32::from_rgb(251, 146, 60);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SettingsTab {
    General,
    Image,
    Presets,
    Hotkeys,
    Axes,
}

impl SettingsTab {
    fn label(self) -> &'static str {
        match self {
            Self::General => "Général",
            Self::Image => "Image",
            Self::Presets => "Préréglages",
            Self::Hotkeys => "Raccourcis",
            Self::Axes => "Axes",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum HoldAction {
    PanLeft,
    PanRight,
    PanUp,
    PanDown,
    ZoomIn,
    ZoomOut,
}

#[derive(Clone, Copy, Debug)]
struct HoldState {
    active: bool,
    next: Instant,
}

struct DraftHotkey {
    ctrl: bool,
    alt: bool,
    shift: bool,
    win: bool,
    key: String,
    listening: bool,
}

impl DraftHotkey {
    fn from_hotkey(hotkey: Option<&Hotkey>) -> Self {
        match hotkey {
            Some(hotkey) => Self {
                ctrl: hotkey.ctrl,
                alt: hotkey.alt,
                shift: hotkey.shift,
                win: hotkey.win,
                key: hotkey.key.clone(),
                listening: true,
            },
            None => Self {
                ctrl: true,
                alt: false,
                shift: false,
                win: false,
                key: "F1".to_string(),
                listening: true,
            },
        }
    }

    fn to_hotkey(&self) -> Hotkey {
        Hotkey::new(self.ctrl, self.alt, self.shift, self.win, &self.key)
    }
}

pub struct PtzApp {
    config: Config,
    devices: Vec<DeviceInfo>,
    cameras: Vec<Camera>,
    current: usize,

    status: String,
    status_kind: Color32,
    status_at: Instant,

    platform: Option<Platform>,
    platform_rx: Option<Receiver<PlatformEvent>>,
    hotkey_errors: HashMap<HotkeyId, String>,

    show_settings: bool,
    settings_tab: SettingsTab,
    editing_binding: Option<usize>,
    draft: DraftHotkey,
    capture_error: Option<String>,

    memory_armed: bool,
    memory_deadline: Option<Instant>,
    active_preset: Option<usize>,
    holding: HashMap<HoldAction, HoldState>,

    focus: i32,
    focus_auto: bool,
    focus_changed_at: Option<Instant>,
    brightness: i32,
    contrast: i32,
    saturation: i32,
    last_poll: Instant,

    reset_on_start: bool,
    guard_on: bool,
    watchdog_stop: Option<Arc<AtomicBool>>,
    watchdog_thread: Option<JoinHandle<()>>,
    main_hwnd: Option<isize>,

    dirty: bool,
    last_save: Instant,
    frames_since_start: u64,
    position_applied: bool,
    applied_always_on_top: Option<bool>,
    pending_command: Option<RemoteCommand>,
}

impl PtzApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        mut config: Config,
        options: crate::cli::Options,
        pending_command: Option<RemoteCommand>,
    ) -> Self {
        configure_style(&cc.egui_ctx);

        // Command-line device selectors are additive and session-only.
        if let Some(device) = &options.device {
            if !device.trim().is_empty()
                && !config
                    .general
                    .device_tokens
                    .iter()
                    .any(|token| token.eq_ignore_ascii_case(device))
            {
                config.general.device_tokens.push(device.clone());
            }
        }

        let reset_on_start = config.general.reset_on_start && !options.no_reset;
        let guard_on = config.general.guard && !options.no_guard;

        let (platform, platform_rx) = match Platform::start() {
            Ok((platform, rx)) => (Some(platform), Some(rx)),
            Err(err) => {
                eprintln!("PTZControl: {err}");
                (None, None)
            }
        };
        if let Some(platform) = &platform {
            platform.reload_hotkeys(config.hotkeys.clone());
        }

        let mut app = Self {
            config,
            devices: Vec::new(),
            cameras: Vec::new(),
            current: 0,
            status: "Recherche des caméras…".to_string(),
            status_kind: MUTED,
            status_at: Instant::now(),
            platform,
            platform_rx,
            hotkey_errors: HashMap::new(),
            show_settings: options.show_devices,
            settings_tab: if options.show_devices {
                SettingsTab::General
            } else {
                SettingsTab::General
            },
            editing_binding: None,
            draft: DraftHotkey::from_hotkey(None),
            capture_error: None,
            memory_armed: false,
            memory_deadline: None,
            active_preset: None,
            holding: HashMap::new(),
            focus: 0,
            focus_auto: false,
            focus_changed_at: None,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            last_poll: Instant::now(),
            reset_on_start,
            guard_on,
            watchdog_stop: None,
            watchdog_thread: None,
            main_hwnd: None,
            dirty: false,
            last_save: Instant::now(),
            frames_since_start: 0,
            position_applied: false,
            applied_always_on_top: None,
            pending_command,
        };

        app.rescan(reset_on_start);
        app.refresh_image();

        if let Some(command) = app.pending_command.take() {
            app.apply_remote(command);
        }
        app
    }

    fn set_status(&mut self, message: impl Into<String>, kind: Color32) {
        self.status = message.into();
        self.status_kind = kind;
        self.status_at = Instant::now();
    }

    fn persist(&mut self) {
        if let Err(err) = config::save(&self.config) {
            eprintln!("PTZControl: impossible d'enregistrer la configuration: {err}");
        }
        self.dirty = false;
        self.last_save = Instant::now();
    }

    fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    fn rescan(&mut self, reset_to_home: bool) {
        match camera::list_devices() {
            Ok(devices) => self.devices = devices,
            Err(err) => {
                self.devices.clear();
                self.set_status(format!("Impossible de lister les caméras: {err}"), RED);
            }
        }

        let tokens: Vec<String> = self
            .config
            .general
            .device_tokens
            .iter()
            .map(|token| token.trim().to_lowercase())
            .filter(|token| !token.is_empty())
            .collect();

        let mut selected: Vec<DeviceInfo> = self
            .devices
            .iter()
            .filter(|device| {
                let name = device.name.to_lowercase();
                tokens.is_empty() || tokens.iter().any(|token| token == "*" || name.contains(token))
            })
            .cloned()
            .collect();
        if selected.is_empty() {
            selected = self.devices.clone();
        }
        selected.truncate(3);

        let mut errors = Vec::new();
        self.cameras.clear();
        for device in selected {
            match Camera::open(
                &device,
                self.config.general.use_logitech_motion,
                self.config.general.motor_interval_ms,
            ) {
                Ok(camera) => self.cameras.push(camera),
                Err(err) => errors.push(format!("{}: {err}", device.name)),
            }
        }

        if self.current >= self.cameras.len() {
            self.current = 0;
        }
        self.active_preset = None;
        self.memory_armed = false;

        if reset_to_home {
            for camera in &self.cameras {
                let _ = camera.home();
            }
        }

        if self.cameras.is_empty() {
            if errors.is_empty() {
                self.set_status(
                    "Aucune caméra détectée. Vérifiez la connexion USB puis actualisez.",
                    RED,
                );
            } else {
                self.set_status(format!("Aucune caméra accessible — {}", errors.join(" · ")), RED);
            }
        } else if errors.is_empty() {
            self.set_status(
                format!(
                    "{} caméra{} prête{}",
                    self.cameras.len(),
                    if self.cameras.len() > 1 { "s" } else { "" },
                    if self.cameras.len() > 1 { "s" } else { "" }
                ),
                GREEN,
            );
        } else {
            self.set_status(
                format!(
                    "{} caméra(s) active(s), {} inaccessible(s)",
                    self.cameras.len(),
                    errors.len()
                ),
                ORANGE,
            );
        }
        self.refresh_image();
    }

    fn refresh_image(&mut self) {
        self.focus_auto = false;
        if let Some(camera) = self.cameras.get(self.current) {
            if let Some((value, auto)) = camera.focus() {
                self.focus = value;
                self.focus_auto = auto;
            } else if let Some(range) = camera.focus_range {
                self.focus = range.midpoint();
            }
            self.brightness = camera
                .brightness()
                .or(camera.brightness_range.map(|r| r.default))
                .unwrap_or(0);
            self.contrast = camera
                .contrast()
                .or(camera.contrast_range.map(|r| r.default))
                .unwrap_or(0);
            self.saturation = camera
                .saturation()
                .or(camera.saturation_range.map(|r| r.default))
                .unwrap_or(0);
        } else {
            self.focus = 0;
            self.brightness = 0;
            self.contrast = 0;
            self.saturation = 0;
        }
        self.last_poll = Instant::now();
    }

    fn select_camera(&mut self, index: usize) {
        if index < self.cameras.len() && index != self.current {
            self.current = index;
            self.active_preset = None;
            self.memory_armed = false;
            self.refresh_image();
            if let Some(camera) = self.cameras.get(index) {
                self.set_status(
                    format!("Caméra {} · {}", index + 1, camera.info().name),
                    MUTED,
                );
            }
        }
    }

    fn focus_range(&self) -> Option<Range> {
        self.cameras.get(self.current).and_then(|c| c.focus_range)
    }

    fn focus_display(&self) -> i32 {
        match self.focus_range() {
            Some(range) if self.config.axes.invert_focus => range.min + range.max - self.focus,
            _ => self.focus,
        }
    }

    fn set_focus_display(&mut self, display: i32) {
        let Some(range) = self.focus_range() else {
            return;
        };
        let actual = if self.config.axes.invert_focus {
            range.min + range.max - display
        } else {
            display
        };
        self.focus = range.clamp(actual);
        self.focus_auto = false;
        self.focus_changed_at = Some(Instant::now());
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.set_focus(self.focus, false) {
                self.set_status(err.to_string(), RED);
            }
        }
    }

    fn set_auto_focus(&mut self, auto: bool) {
        self.focus_auto = auto;
        self.focus_changed_at = Some(Instant::now());
        if let Some(camera) = self.cameras.get(self.current) {
            let result = if auto {
                camera.set_focus(0, true)
            } else {
                camera.set_focus(self.focus, false)
            };
            if let Err(err) = result {
                self.set_status(err.to_string(), RED);
            }
        }
    }

    fn toggle_auto_focus(&mut self) {
        self.set_auto_focus(!self.focus_auto);
    }

    fn set_brightness(&mut self, value: i32) {
        self.brightness = value;
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.set_brightness(value) {
                self.set_status(err.to_string(), RED);
            }
        }
    }

    fn set_contrast(&mut self, value: i32) {
        self.contrast = value;
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.set_contrast(value) {
                self.set_status(err.to_string(), RED);
            }
        }
    }

    fn set_saturation(&mut self, value: i32) {
        self.saturation = value;
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.set_saturation(value) {
                self.set_status(err.to_string(), RED);
            }
        }
    }

    fn reset_image(&mut self) {
        let Some(camera) = self.cameras.get(self.current) else {
            return;
        };
        if let Some(range) = camera.focus_range {
            self.focus_auto = true;
            self.focus = range.default;
            let _ = camera.set_focus(range.default, true);
        }
        if let Some(range) = camera.brightness_range {
            self.brightness = range.default;
            let _ = camera.set_brightness(range.default);
        }
        if let Some(range) = camera.contrast_range {
            self.contrast = range.default;
            let _ = camera.set_contrast(range.default);
        }
        if let Some(range) = camera.saturation_range {
            self.saturation = range.default;
            let _ = camera.set_saturation(range.default);
        }
        self.focus_changed_at = Some(Instant::now());
        self.set_status("Image et mise au point réinitialisées", GREEN);
    }

    fn pan(&mut self, logical: i32) {
        let direction = if self.config.axes.invert_pan { -logical } else { logical };
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.pan(direction) {
                self.set_status(err.to_string(), RED);
            }
        }
        self.active_preset = None;
    }

    fn tilt(&mut self, logical: i32) {
        let direction = if self.config.axes.invert_tilt { -logical } else { logical };
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.tilt(direction) {
                self.set_status(err.to_string(), RED);
            }
        }
        self.active_preset = None;
    }

    fn zoom(&mut self, logical: i32) {
        let direction = if self.config.axes.invert_zoom { -logical } else { logical };
        if let Some(camera) = self.cameras.get(self.current) {
            if let Err(err) = camera.zoom(direction) {
                self.set_status(err.to_string(), RED);
            }
        }
        self.active_preset = None;
    }

    fn home(&mut self) {
        self.home_for(self.current);
    }

    fn home_for(&mut self, camera_index: usize) {
        if let Some(camera) = self.cameras.get(camera_index) {
            let name = camera.info().name.clone();
            if let Err(err) = camera.home() {
                self.set_status(err.to_string(), RED);
            } else {
                self.set_status(format!("Position d'accueil · {name}"), GREEN);
            }
        }
        if camera_index == self.current {
            self.active_preset = None;
        }
    }

    fn toggle_memory(&mut self) {
        self.memory_armed = !self.memory_armed;
        self.memory_deadline = if self.memory_armed {
            Some(Instant::now() + Duration::from_secs(6))
        } else {
            None
        };
        if self.memory_armed {
            self.set_status(
                "Mode mémoire : cliquez un préréglage pour l'enregistrer",
                ORANGE,
            );
        }
    }

    fn preset(&mut self, slot: usize) {
        self.preset_for(self.current, slot);
    }

    fn preset_for(&mut self, camera_index: usize, slot: usize) {
        if slot >= NUM_PRESETS {
            return;
        }
        let memory = self.memory_armed;
        if let Some(camera) = self.cameras.get(camera_index) {
            let result = if memory {
                camera.save_preset(slot)
            } else {
                camera.recall_preset(slot)
            };
            match result {
                Ok(()) => {
                    if memory {
                        self.set_status(format!("Préréglage {} mémorisé", slot + 1), GREEN);
                    } else {
                        self.set_status(format!("Préréglage {} rappelé", slot + 1), GREEN);
                    }
                }
                Err(err) => self.set_status(err.to_string(), RED),
            }
        }
        if memory {
            self.memory_armed = false;
            self.memory_deadline = None;
        } else if camera_index == self.current {
            self.active_preset = Some(slot);
        }
    }

    fn stop_motion(&mut self) {
        if let Some(camera) = self.cameras.get(self.current) {
            let _ = camera.stop_motion();
        }
    }

    fn execute_action(&mut self, action: HotkeyAction, camera: Option<usize>) {
        let target = camera.filter(|index| *index < self.cameras.len());
        match action {
            HotkeyAction::Camera1 => self.select_camera(0),
            HotkeyAction::Camera2 => self.select_camera(1),
            HotkeyAction::Camera3 => self.select_camera(2),
            HotkeyAction::Home => match target {
                Some(index) => self.home_for(index),
                None => self.home(),
            },
            HotkeyAction::Preset1
            | HotkeyAction::Preset2
            | HotkeyAction::Preset3
            | HotkeyAction::Preset4
            | HotkeyAction::Preset5
            | HotkeyAction::Preset6
            | HotkeyAction::Preset7
            | HotkeyAction::Preset8 => {
                if let Some(slot) = action.preset_slot() {
                    match target {
                        Some(index) => self.preset_for(index, slot),
                        None => self.preset(slot),
                    }
                }
            }
            HotkeyAction::Memory => self.toggle_memory(),
            HotkeyAction::PanLeft => self.pan(-1),
            HotkeyAction::PanRight => self.pan(1),
            HotkeyAction::PanUp => self.tilt(1),
            HotkeyAction::PanDown => self.tilt(-1),
            HotkeyAction::ZoomIn => self.zoom(1),
            HotkeyAction::ZoomOut => self.zoom(-1),
            HotkeyAction::ToggleAutoFocus => self.toggle_auto_focus(),
            HotkeyAction::OpenSettings => {
                self.show_settings = !self.show_settings;
            }
        }
    }

    fn apply_remote(&mut self, command: RemoteCommand) {
        let Some(camera) = self.cameras.get(command.camera) else {
            self.set_status("Commande vers une caméra non connectée", RED);
            return;
        };
        let steps = command.steps.clamp(1, 10);
        let result = match command.action {
            RemoteAction::Home => camera.home(),
            RemoteAction::Pan(direction) => {
                let direction = if self.config.axes.invert_pan {
                    -direction
                } else {
                    direction
                };
                let mut result = Ok(());
                for _ in 0..steps {
                    result = camera.pan(direction);
                    if result.is_err() {
                        break;
                    }
                }
                result
            }
            RemoteAction::Tilt(direction) => {
                let direction = if self.config.axes.invert_tilt {
                    -direction
                } else {
                    direction
                };
                let mut result = Ok(());
                for _ in 0..steps {
                    result = camera.tilt(direction);
                    if result.is_err() {
                        break;
                    }
                }
                result
            }
            RemoteAction::Zoom(direction) => {
                let direction = if self.config.axes.invert_zoom {
                    -direction
                } else {
                    direction
                };
                let mut result = Ok(());
                for _ in 0..steps {
                    result = camera.zoom(direction);
                    if result.is_err() {
                        break;
                    }
                }
                result
            }
            RemoteAction::Restore(slot) => {
                let result = camera.recall_preset(slot);
                if result.is_ok() && command.camera == self.current {
                    self.active_preset = Some(slot);
                }
                result
            }
            RemoteAction::Store(slot) => camera.save_preset(slot),
        };
        if let Err(err) = result {
            self.set_status(err.to_string(), RED);
        }
    }

    fn poll_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.platform_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                PlatformEvent::Hotkey((action, camera)) => self.execute_action(action, camera),
                PlatformEvent::Message(message) => {
                    if message.activate && message.command.is_none() {
                        self.show_settings = false;
                    }
                    if let Some(command) = message.command {
                        self.apply_remote(command);
                    }
                }
                PlatformEvent::HotkeyError { target, message } => {
                    self.hotkey_errors.insert(target, message);
                }
            }
        }
    }

    fn start_watchdog(&mut self, hwnd: isize) {
        self.stop_watchdog();
        let stop = Arc::new(AtomicBool::new(false));
        let stop_thread = stop.clone();
        self.watchdog_thread = Some(thread::spawn(move || {
            // Mirrors the original MFC guard thread: ask the window to process
            // a harmless message. If a blocking USB/driver call freezes the UI
            // thread, terminate the process so the operator can relaunch.
            while !stop_thread.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_secs(1));
                if stop_thread.load(Ordering::Relaxed) {
                    break;
                }
                let mut result = 0usize;
                let ok = unsafe {
                    windows::Win32::UI::WindowsAndMessaging::SendMessageTimeoutW(
                        windows::Win32::Foundation::HWND(hwnd as *mut core::ffi::c_void),
                        windows::Win32::UI::WindowsAndMessaging::WM_NULL,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                        windows::Win32::UI::WindowsAndMessaging::SMTO_ABORTIFHUNG
                            | windows::Win32::UI::WindowsAndMessaging::SMTO_NORMAL,
                        1000,
                        Some(&mut result),
                    )
                };
                if ok.0 == 0 {
                    std::process::exit(10);
                }
            }
        }));
        self.watchdog_stop = Some(stop);
    }

    fn stop_watchdog(&mut self) {
        if let Some(stop) = self.watchdog_stop.take() {
            stop.store(true, Ordering::Relaxed);
        }
        if let Some(thread) = self.watchdog_thread.take() {
            let _ = thread.join();
        }
    }

    fn reload_hotkeys(&mut self) {
        self.hotkey_errors.clear();
        if let Some(platform) = &self.platform {
            platform.reload_hotkeys(self.config.hotkeys.clone());
        }
    }

    fn start_hotkey_edit(&mut self, index: usize) {
        let Some(binding) = self.config.binding(index) else {
            return;
        };
        self.editing_binding = Some(index);
        self.draft = DraftHotkey::from_hotkey(binding.hotkey.as_ref());
        self.capture_error = None;
        if let Some(platform) = &self.platform {
            platform.reload_hotkeys(Vec::new());
        }
    }

    fn cancel_hotkey_edit(&mut self) {
        self.editing_binding = None;
        self.capture_error = None;
        self.reload_hotkeys();
    }

    fn commit_hotkey_edit(&mut self) {
        let Some(index) = self.editing_binding else {
            return;
        };
        let hotkey = self.draft.to_hotkey();
        if hotkeys::key_to_vk(&hotkey.key).is_none() {
            self.capture_error = Some("Touche invalide".into());
            return;
        }
        if !hotkey.has_modifier() && !hotkey.key.starts_with('F') {
            self.capture_error =
                Some("Ajoutez au moins Ctrl, Alt, Maj ou Win (ou utilisez une touche F).".into());
            return;
        }
        if let Some(binding) = self.config.hotkeys.get_mut(index) {
            binding.hotkey = Some(hotkey);
        }
        self.editing_binding = None;
        self.capture_error = None;
        self.mark_dirty();
        self.persist();
        self.reload_hotkeys();
        self.set_status("Raccourci enregistré", GREEN);
    }

    fn clear_hotkey(&mut self, index: usize) {
        if let Some(binding) = self.config.hotkeys.get_mut(index) {
            binding.hotkey = None;
        }
        self.editing_binding = None;
        self.capture_error = None;
        self.mark_dirty();
        self.persist();
        self.reload_hotkeys();
    }

    fn reset_hotkeys(&mut self) {
        self.config.hotkeys = config::default_hotkeys();
        self.editing_binding = None;
        self.capture_error = None;
        self.mark_dirty();
        self.persist();
        self.reload_hotkeys();
    }

    fn process_hotkey_capture(&mut self, ctx: &egui::Context) {
        if self.editing_binding.is_none() || !self.draft.listening {
            return;
        }
        let pressed = ctx.input(|input| {
            input.events.iter().find_map(|event| {
                if let egui::Event::Key {
                    key,
                    pressed: true,
                    ..
                } = event
                {
                    hotkeys::key_from_egui(*key)
                } else {
                    None
                }
            })
        });
        if let Some(key) = pressed {
            if key == "Escape" {
                self.cancel_hotkey_edit();
                return;
            }
            self.draft.key = key.to_string();
            let modifiers = hotkeys::current_modifiers();
            if modifiers.ctrl || modifiers.alt || modifiers.shift || modifiers.win {
                self.draft.ctrl = modifiers.ctrl;
                self.draft.alt = modifiers.alt;
                self.draft.shift = modifiers.shift;
                self.draft.win = modifiers.win;
            }
            self.draft.listening = false;
            self.capture_error = None;
        }
    }

    fn process_hold(&mut self, action: HoldAction, down: bool, now: Instant) {
        if !down {
            if let Some(state) = self.holding.get_mut(&action) {
                if state.active {
                    state.active = false;
                    if matches!(
                        action,
                        HoldAction::PanLeft
                            | HoldAction::PanRight
                            | HoldAction::PanUp
                            | HoldAction::PanDown
                    ) {
                        self.stop_motion();
                    }
                }
            }
            return;
        }

        let mut trigger = false;
        let state = self.holding.entry(action).or_insert(HoldState {
            active: false,
            next: now,
        });
        if !state.active {
            state.active = true;
            state.next = now + Duration::from_millis(420);
            trigger = true;
        } else if now >= state.next {
            state.next = now + Duration::from_millis(130);
            trigger = true;
        }
        if trigger {
            match action {
                HoldAction::PanLeft => self.pan(-1),
                HoldAction::PanRight => self.pan(1),
                HoldAction::PanUp => self.tilt(1),
                HoldAction::PanDown => self.tilt(-1),
                HoldAction::ZoomIn => self.zoom(1),
                HoldAction::ZoomOut => self.zoom(-1),
            }
        }
    }

    fn apply_always_on_top(&mut self, ctx: &egui::Context) {
        let wanted = self.config.general.always_on_top;
        if self.applied_always_on_top != Some(wanted) {
            let level = if wanted {
                egui::WindowLevel::AlwaysOnTop
            } else {
                egui::WindowLevel::Normal
            };
            ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(level));
            self.applied_always_on_top = Some(wanted);
        }
    }

    fn apply_saved_position(&mut self, ctx: &egui::Context) {
        if self.position_applied {
            return;
        }
        if let (Some(x), Some(y)) = (self.config.window.x, self.config.window.y) {
            ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        }
        self.position_applied = true;
    }

    fn track_window_position(&mut self, ctx: &egui::Context) {
        if self.frames_since_start < 3 {
            return;
        }
        if let Some(rect) = ctx.input(|input| input.viewport().outer_rect) {
            let x = Some(rect.min.x);
            let y = Some(rect.min.y);
            if self.config.window.x != x || self.config.window.y != y {
                self.config.window.x = x;
                self.config.window.y = y;
                self.dirty = true;
            }
        }
    }

    fn refresh_from_hardware(&mut self) {
        let Some(camera) = self.cameras.get(self.current) else {
            return;
        };
        let now = Instant::now();
        if self
            .focus_changed_at
            .map(|at| now.duration_since(at) < Duration::from_millis(900))
            .unwrap_or(false)
        {
            return;
        }
        if let Some((value, auto)) = camera.focus() {
            self.focus = value;
            self.focus_auto = auto;
        }
        if let Some(value) = camera.brightness() {
            self.brightness = value;
        }
        if let Some(value) = camera.contrast() {
            self.contrast = value;
        }
        if let Some(value) = camera.saturation() {
            self.saturation = value;
        }
        self.last_poll = now;
    }

    fn draw_title_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("titlebar")
            .exact_height(22.0)
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(6, 2)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    // Status dot
                    let dot_color = self.status_kind;
                    let (rect, _) = ui.allocate_exact_size(Vec2::new(7.0, 7.0), Sense::hover());
                    ui.painter().circle_filled(rect.center(), 3.5, dot_color);

                    // Camera title
                    let title_text = if let Some(cam) = self.cameras.get(self.current) {
                        let name = &cam.info().name;
                        if name.len() > 18 {
                            format!("PTZ · {}...", &name[..15])
                        } else {
                            format!("PTZ · {}", name)
                        }
                    } else {
                        "PTZ Control".to_string()
                    };
                    let title_lbl = ui.label(
                        RichText::new(title_text)
                            .size(10.5)
                            .color(TEXT)
                            .strong(),
                    );
                    if let Some(cam) = self.cameras.get(self.current) {
                        title_lbl.on_hover_text(format!(
                            "{} ({})",
                            cam.info().name,
                            cam.motion_mode_label()
                        ));
                    }

                    // Right controls
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let close = ui
                            .add_sized(
                                Vec2::new(18.0, 18.0),
                                egui::Button::new(RichText::new("×").size(13.0).color(MUTED))
                                    .fill(Color32::TRANSPARENT)
                                    .corner_radius(CornerRadius::same(4)),
                            )
                            .on_hover_text("Fermer");
                        if close.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }

                        let min = ui
                            .add_sized(
                                Vec2::new(18.0, 18.0),
                                egui::Button::new(RichText::new("−").size(12.0).color(MUTED))
                                    .fill(Color32::TRANSPARENT)
                                    .corner_radius(CornerRadius::same(4)),
                            )
                            .on_hover_text("Réduire");
                        if min.clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
                        }

                        if !self.hotkey_errors.is_empty() {
                            let warn = ui
                                .add(
                                    egui::Button::new(
                                        RichText::new(format!("⚠ {}", self.hotkey_errors.len()))
                                            .size(10.0)
                                            .color(ORANGE),
                                    )
                                    .fill(Color32::from_rgb(45, 30, 20))
                                    .corner_radius(CornerRadius::same(4)),
                                )
                                .on_hover_text(format!(
                                    "{} raccourci(s) indisponible(s). Cliquez pour configurer.",
                                    self.hotkey_errors.len()
                                ));
                            if warn.clicked() {
                                self.show_settings = true;
                                self.settings_tab = SettingsTab::Hotkeys;
                            }
                        }

                        // Middle space can be dragged
                        let avail = ui.available_size_before_wrap();
                        if avail.x > 8.0 {
                            let (_, drag) = ui.allocate_exact_size(
                                Vec2::new(avail.x, 18.0),
                                Sense::click_and_drag(),
                            );
                            if drag.drag_started() {
                                ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
                            }
                        }
                    });
                });
            });
    }

    fn draw_main(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                Frame::new()
                    .fill(BG)
                    .inner_margin(Margin::symmetric(6, 4)),
            )
            .show(ctx, |ui| {
                if self.cameras.is_empty() {
                    self.draw_empty_state(ui);
                } else {
                    self.draw_toolbar(ui);
                }
            });
    }

    fn draw_empty_state(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(6.0);
            ui.label(
                RichText::new("Aucune caméra PTZ détectée")
                    .size(12.0)
                    .color(TEXT)
                    .strong(),
            );
            ui.label(
                RichText::new("Branchez une caméra USB puis actualisez.")
                    .size(10.5)
                    .color(MUTED),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let avail = ui.available_width();
                if avail > 210.0 {
                    ui.add_space((avail - 210.0) / 2.0);
                }
                if compact_button(ui, "⟳ Actualiser", Vec2::new(100.0, 24.0), ACCENT_SOFT, TEXT).clicked() {
                    self.rescan(false);
                }
                if compact_button(ui, "⚙ Paramètres", Vec2::new(100.0, 24.0), CARD, MUTED).clicked() {
                    self.show_settings = true;
                    self.settings_tab = SettingsTab::General;
                }
            });
        });
    }

    fn draw_toolbar(&mut self, ui: &mut egui::Ui) {
        let btn_size = Vec2::new(26.0, 23.0);
        let spacing = 2.5;
        let total_h = btn_size.y * 3.0 + spacing * 2.0;

        ui.horizontal_top(|ui| {
            // 1. Zoom column (ZoomIn, AF, ZoomOut)
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(spacing, spacing);
                let zoom_in = toolbar_nav_button(ui, "+", btn_size, 14.0).on_hover_text("Zoom avant");

                let af_fill = if self.focus_auto { ACCENT_SOFT } else { CARD };
                let af_text = if self.focus_auto { TEXT } else { MUTED };
                let af_tip = if self.focus_auto {
                    "Autofocus actif (cliquez pour manuel)"
                } else {
                    "Autofocus désactivé (cliquez pour auto)"
                };
                let af = ui
                    .add_sized(
                        btn_size,
                        egui::Button::new(RichText::new("AF").size(9.5).strong().color(af_text))
                            .fill(af_fill)
                            .stroke(Stroke::new(1.0_f32, if self.focus_auto { ACCENT } else { BORDER }))
                            .corner_radius(CornerRadius::same(5)),
                    )
                    .on_hover_text(af_tip);
                if af.clicked() {
                    self.toggle_auto_focus();
                }

                let zoom_out = toolbar_nav_button(ui, "−", btn_size, 14.0).on_hover_text("Zoom arrière");

                let now = Instant::now();
                self.process_hold(HoldAction::ZoomIn, zoom_in.is_pointer_button_down_on(), now);
                self.process_hold(HoldAction::ZoomOut, zoom_out.is_pointer_button_down_on(), now);
            });

            ui.add_space(3.0);

            // 2. Focus Vertical Slider
            ui.vertical(|ui| {
                if let Some(range) = self.focus_range() {
                    let mut display = self.focus_display();
                    let slider = egui::Slider::new(&mut display, range.min..=(range.max.max(range.min + 1)))
                        .vertical()
                        .show_value(false);
                    let resp = ui
                        .add_enabled_ui(!self.focus_auto, |ui| {
                            ui.add_sized(Vec2::new(12.0, total_h), slider)
                        })
                        .inner;
                    if resp.changed() {
                        self.set_focus_display(display);
                    }
                    if self.focus_auto {
                        resp.on_hover_text("Mise au point: Auto");
                    } else {
                        resp.on_hover_text(format!("Mise au point: {}", display));
                    }
                } else {
                    let (rect, resp) = ui.allocate_exact_size(Vec2::new(12.0, total_h), Sense::hover());
                    ui.painter().rect_filled(rect, CornerRadius::same(3), CARD);
                    resp.on_hover_text("Mise au point non prise en charge");
                }
            });

            ui.add_space(4.0);

            // 3. D-Pad 3x3 direction cross
            let mut left = None;
            let mut right = None;
            let mut up = None;
            let mut down = None;
            let mut home_clicked = false;

            egui::Grid::new("dpad")
                .spacing([spacing, spacing])
                .show(ui, |ui| {
                    ui.allocate_exact_size(btn_size, Sense::hover());
                    let b_up = toolbar_nav_button(ui, "▲", btn_size, 11.0).on_hover_text("Inclinaison haut");
                    up = Some(b_up);
                    ui.allocate_exact_size(btn_size, Sense::hover());
                    ui.end_row();

                    let b_left = toolbar_nav_button(ui, "◀", btn_size, 11.0).on_hover_text("Panoramique gauche");
                    left = Some(b_left);

                    let home = toolbar_nav_button(ui, "⌂", btn_size, 13.0).on_hover_text("Position d'accueil (Home)");
                    if home.clicked() {
                        home_clicked = true;
                    }

                    let b_right = toolbar_nav_button(ui, "▶", btn_size, 11.0).on_hover_text("Panoramique droite");
                    right = Some(b_right);
                    ui.end_row();

                    ui.allocate_exact_size(btn_size, Sense::hover());
                    let b_down = toolbar_nav_button(ui, "▼", btn_size, 11.0).on_hover_text("Inclinaison bas");
                    down = Some(b_down);
                    ui.allocate_exact_size(btn_size, Sense::hover());
                    ui.end_row();
                });

            if home_clicked {
                self.home();
            }
            let now = Instant::now();
            self.process_hold(
                HoldAction::PanLeft,
                left.map(|r| r.is_pointer_button_down_on()).unwrap_or(false),
                now,
            );
            self.process_hold(
                HoldAction::PanRight,
                right.map(|r| r.is_pointer_button_down_on()).unwrap_or(false),
                now,
            );
            self.process_hold(
                HoldAction::PanUp,
                up.map(|r| r.is_pointer_button_down_on()).unwrap_or(false),
                now,
            );
            self.process_hold(
                HoldAction::PanDown,
                down.map(|r| r.is_pointer_button_down_on()).unwrap_or(false),
                now,
            );

            ui.add_space(4.0);

            // 4. Presets 3x3 (1..8 + M)
            let mut clicked_preset = None;
            egui::Grid::new("presets")
                .spacing([spacing, spacing])
                .show(ui, |ui| {
                    for slot in 0..NUM_PRESETS {
                        let active = self.active_preset == Some(slot);
                        let armed = self.memory_armed;
                        let fill = if active {
                            GREEN
                        } else if armed {
                            Color32::from_rgb(60, 32, 28)
                        } else {
                            CARD
                        };
                        let text_color = if active { BG } else { TEXT };
                        let label = self.config.preset_label(self.current, slot);
                        let resp = ui.add_sized(
                            btn_size,
                            egui::Button::new(
                                RichText::new(format!("{}", slot + 1))
                                    .size(12.0)
                                    .strong()
                                    .color(text_color),
                            )
                            .fill(fill)
                            .stroke(Stroke::new(
                                1.0_f32,
                                if active { GREEN } else if armed { ORANGE } else { BORDER },
                            ))
                            .corner_radius(CornerRadius::same(5)),
                        );
                        let resp = if !label.is_empty() {
                            resp.on_hover_text(format!("P{}: {}", slot + 1, label))
                        } else {
                            resp.on_hover_text(format!("Préréglage {}", slot + 1))
                        };
                        if resp.clicked() {
                            clicked_preset = Some(slot);
                        }

                        if slot % 3 == 2 {
                            ui.end_row();
                        }
                    }

                    // 9th slot in 3x3 grid is Memory button "M"
                    let m_fill = if self.memory_armed { RED } else { CARD };
                    let m_text = if self.memory_armed { TEXT } else { MUTED };
                    let m_btn = ui
                        .add_sized(
                            btn_size,
                            egui::Button::new(RichText::new("M").size(11.5).strong().color(m_text))
                                .fill(m_fill)
                                .stroke(Stroke::new(1.0_f32, if self.memory_armed { RED } else { BORDER }))
                                .corner_radius(CornerRadius::same(5)),
                        )
                        .on_hover_text("Mémoriser: cliquez M puis un bouton 1-8");
                    if m_btn.clicked() {
                        self.toggle_memory();
                    }
                    ui.end_row();
                });

            if let Some(slot) = clicked_preset {
                self.preset(slot);
            }

            ui.add_space(4.0);

            // 5. Camera selector (if multi-camera)
            if self.cameras.len() > 1 {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(spacing, spacing);
                    for index in 0..self.cameras.len().min(3) {
                        let active = index == self.current;
                        let mut resp = ui.add_sized(
                            btn_size,
                            egui::Button::new(
                                RichText::new(format!("C{}", index + 1))
                                    .size(11.0)
                                    .strong()
                                    .color(if active { TEXT } else { MUTED }),
                            )
                            .fill(if active { ACCENT_SOFT } else { CARD })
                            .stroke(Stroke::new(1.0_f32, if active { ACCENT } else { BORDER }))
                            .corner_radius(CornerRadius::same(5)),
                        );
                        if let Some(cam) = self.cameras.get(index) {
                            resp = resp.on_hover_text(format!("Caméra {}: {}", index + 1, cam.info().name));
                        }
                        if resp.clicked() {
                            self.select_camera(index);
                        }
                    }
                });
                ui.add_space(4.0);
            }

            // 6. Utility column (Settings, Rescan, Active Preset)
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(spacing, spacing);
                let settings = ui
                    .add_sized(
                        btn_size,
                        egui::Button::new(
                            RichText::new("⚙")
                                .size(12.0)
                                .color(if self.show_settings { TEXT } else { MUTED }),
                        )
                        .fill(if self.show_settings { ACCENT_SOFT } else { CARD })
                        .stroke(Stroke::new(1.0_f32, if self.show_settings { ACCENT } else { BORDER }))
                        .corner_radius(CornerRadius::same(5)),
                    )
                    .on_hover_text("Paramètres");
                if settings.clicked() {
                    self.show_settings = !self.show_settings;
                }

                let rescan = ui
                    .add_sized(
                        btn_size,
                        egui::Button::new(RichText::new("⟳").size(12.0).color(MUTED))
                            .fill(CARD)
                            .stroke(Stroke::new(1.0_f32, BORDER))
                            .corner_radius(CornerRadius::same(5)),
                    )
                    .on_hover_text("Actualiser les caméras");
                if rescan.clicked() {
                    self.rescan(false);
                }

                // 3rd row: active preset indicator
                let p_label = self
                    .active_preset
                    .map(|p| format!("P{}", p + 1))
                    .unwrap_or_else(|| "—".into());
                let (rect, resp) = ui.allocate_exact_size(btn_size, Sense::hover());
                ui.painter().rect_filled(rect, CornerRadius::same(5), PANEL);
                ui.painter().text(
                    rect.center(),
                    Align2::CENTER_CENTER,
                    p_label,
                    FontId::proportional(10.0),
                    MUTED,
                );
                resp.on_hover_text("Préréglage actif");
            });

            // Remaining space can be dragged
            let avail = ui.available_size_before_wrap();
            if avail.x > 4.0 {
                let (_, drag) = ui.allocate_exact_size(
                    Vec2::new(avail.x, total_h),
                    Sense::click_and_drag(),
                );
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            }
        });
    }

    fn settings_viewport(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let builder = egui::ViewportBuilder::default()
            .with_title("PTZ Control — Paramètres")
            .with_inner_size([780.0, 640.0])
            .with_min_inner_size([660.0, 520.0])
            .with_resizable(true)
            .with_always_on_top();
        let mut close_requested = false;
        ctx.show_viewport_immediate(
            egui::ViewportId::from_hash_of("ptz-settings"),
            builder,
            |ctx, _class| {
                close_requested = ctx.input(|input| input.viewport().close_requested());
                self.settings_ui(ctx);
            },
        );
        if close_requested {
            self.show_settings = false;
        }
        if !self.show_settings && self.editing_binding.is_some() {
            self.cancel_hotkey_edit();
        }
    }

    fn settings_ui(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin::same(16)))
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Paramètres").size(22.0).color(TEXT).strong());
                    ui.label(RichText::new("PTZ Control · Rust Edition").size(11.0).color(MUTED));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if compact_button(ui, "Fermer", Vec2::new(78.0, 28.0), CARD, MUTED).clicked()
                        {
                            self.show_settings = false;
                        }
                    });
                });
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    for tab in [
                        SettingsTab::General,
                        SettingsTab::Image,
                        SettingsTab::Presets,
                        SettingsTab::Hotkeys,
                        SettingsTab::Axes,
                    ] {
                        let selected = self.settings_tab == tab;
                        let text = RichText::new(tab.label())
                            .size(12.5)
                            .color(if selected { TEXT } else { MUTED });
                        if ui
                            .add(
                                egui::Button::new(text)
                                    .fill(if selected { ACCENT_SOFT } else { Color32::TRANSPARENT })
                                    .stroke(Stroke::NONE)
                                    .corner_radius(CornerRadius::same(8))
                                    .min_size(Vec2::new(96.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.settings_tab = tab;
                        }
                    }
                });
                ui.add_space(6.0);
                ui.separator();
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.settings_tab {
                        SettingsTab::General => self.settings_general(ui),
                        SettingsTab::Image => self.settings_image(ui),
                        SettingsTab::Presets => self.settings_presets(ui),
                        SettingsTab::Hotkeys => self.settings_hotkeys(ui),
                        SettingsTab::Axes => self.settings_axes(ui),
                    });
            });
    }

    fn settings_general(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Caméras détectées");
        if self.devices.is_empty() {
            ui.label(RichText::new("Aucun périphérique vidéo détecté.").size(12.0).color(MUTED));
        } else {
            for device in &self.devices {
                Frame::new()
                    .fill(CARD)
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(10, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&device.name).size(12.5).color(TEXT));
                            ui.label(
                                RichText::new(format!("VID {:04X}:{:04X}", device.vid, device.pid))
                                    .size(10.0)
                                    .color(MUTED),
                            );
                        });
                        ui.label(
                            RichText::new(&device.path)
                                .size(9.5)
                                .color(Color32::from_rgb(100, 112, 128)),
                        );
                    });
                ui.add_space(4.0);
            }
        }
        ui.add_space(4.0);
        if accent_button(ui, "Rechercher les caméras", Vec2::new(180.0, 30.0)).clicked() {
            self.rescan(false);
        }

        ui.add_space(12.0);
        section_title(ui, "Démarrage & fenêtre");
        let mut always = self.config.general.always_on_top;
        if ui.checkbox(&mut always, "Toujours au-dessus des autres fenêtres").changed() {
            self.config.general.always_on_top = always;
            self.mark_dirty();
        }
        let mut reset = self.reset_on_start;
        if ui
            .checkbox(&mut reset, "Ramener les caméras à la position d'accueil au démarrage")
            .changed()
        {
            self.reset_on_start = reset;
            self.config.general.reset_on_start = reset;
            self.mark_dirty();
        }
        let mut guard = self.guard_on;
        if ui
            .checkbox(
                &mut guard,
                "Gardien USB (quitte l'application si un pilote se bloque)",
            )
            .changed()
        {
            self.guard_on = guard;
            self.config.general.guard = guard;
            // The watchdog is (re)started from `update` once the native window
            // handle is known.
            self.stop_watchdog();
            self.mark_dirty();
        }

        ui.add_space(12.0);
        section_title(ui, "Moteurs & compatibilité");
        let mut logitech = self.config.general.use_logitech_motion;
        if ui
            .checkbox(&mut logitech, "Utiliser le contrôle moteur Logitech (extension XU)")
            .changed()
        {
            self.config.general.use_logitech_motion = logitech;
            for camera in &mut self.cameras {
                camera.set_motion_options(logitech, self.config.general.motor_interval_ms);
            }
            self.mark_dirty();
        }
        let mut interval = self.config.general.motor_interval_ms as i32;
        if ui
            .add(
                egui::DragValue::new(&mut interval)
                    .range(10..=1000)
                    .suffix(" ms")
                    .speed(5.0),
            )
            .changed()
        {
            self.config.general.motor_interval_ms = interval.clamp(10, 1000) as u32;
            for camera in &mut self.cameras {
                camera.set_motion_options(
                    self.config.general.use_logitech_motion,
                    self.config.general.motor_interval_ms,
                );
            }
            self.mark_dirty();
        }
        ui.label(
            RichText::new(
                "Astuce : le mode Logitech donne un mouvement plus fluide. Désactivez-le si votre caméra n'expose pas les extensions Logitech.",
            )
            .size(10.5)
            .color(MUTED),
        );

        ui.add_space(12.0);
        section_title(ui, "Filtres de noms de périphériques");
        ui.label(
            RichText::new("Un nom par ligne. « * » accepte toutes les caméras. Les filtres par défaut couvrent PTZ Pro, Rally et ConferenceCam.")
                .size(10.5)
                .color(MUTED),
        );
        let mut tokens = self.config.general.device_tokens.join("\n");
        if ui
            .add(
                egui::TextEdit::multiline(&mut tokens)
                    .desired_rows(4)
                    .desired_width(f32::INFINITY)
                    .hint_text("PTZ Pro\nLogi Rally\n*"),
            )
            .changed()
        {
            self.config.general.device_tokens = tokens
                .lines()
                .map(|line| line.trim().to_string())
                .filter(|line| !line.is_empty())
                .collect();
            self.mark_dirty();
        }
    }

    fn settings_image(&mut self, ui: &mut egui::Ui) {
        let (camera_name, focus_range, brightness_range, contrast_range, saturation_range) = {
            let Some(camera) = self.cameras.get(self.current) else {
                ui.label(RichText::new("Aucune caméra active.").size(12.0).color(MUTED));
                return;
            };
            (
                camera.info().name.clone(),
                camera.focus_range,
                camera.brightness_range,
                camera.contrast_range,
                camera.saturation_range,
            )
        };

        ui.label(
            RichText::new(camera_name)
                .size(13.0)
                .color(TEXT)
                .strong(),
        );
        ui.add_space(8.0);

        section_title(ui, "Mise au point");
        if let Some(range) = focus_range {
            let mut auto = self.focus_auto;
            if ui.checkbox(&mut auto, "Autofocus continu (AF)").changed() {
                self.set_auto_focus(auto);
            }
            let mut display = self.focus_display();
            if ui
                .add_enabled(
                    !self.focus_auto,
                    egui::Slider::new(&mut display, range.min..=range.max)
                        .text("Valeur")
                        .show_value(true),
                )
                .changed()
            {
                self.set_focus_display(display);
            }
            ui.label(
                RichText::new(format!(
                    "Plage {} … {} · pas {} · défaut {}",
                    range.min, range.max, range.step, range.default
                ))
                .size(10.0)
                .color(MUTED),
            );
        } else {
            ui.label(
                RichText::new("La mise au point n'est pas exposée par cette caméra.")
                    .size(11.0)
                    .color(MUTED),
            );
        }

        ui.add_space(10.0);
        section_title(ui, "Couleur & luminosité");
        if let Some(range) = brightness_range {
            let mut value = self.brightness;
            if ui
                .add(egui::Slider::new(&mut value, range.min..=range.max).text("Luminosité"))
                .changed()
            {
                self.set_brightness(value);
            }
        }
        if let Some(range) = contrast_range {
            let mut value = self.contrast;
            if ui
                .add(egui::Slider::new(&mut value, range.min..=range.max).text("Contraste"))
                .changed()
            {
                self.set_contrast(value);
            }
        }
        if let Some(range) = saturation_range {
            let mut value = self.saturation;
            if ui
                .add(egui::Slider::new(&mut value, range.min..=range.max).text("Saturation"))
                .changed()
            {
                self.set_saturation(value);
            }
        }
        ui.add_space(10.0);
        if accent_button(ui, "Réinitialiser image & mise au point", Vec2::new(250.0, 32.0))
            .clicked()
        {
            self.reset_image();
        }
    }

    fn settings_presets(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Libellés des préréglages");
        ui.label(
            RichText::new(
                "Les libellés apparaissent au survol des boutons. Les positions sont mémorisées dans le matériel PTZ.",
            )
            .size(10.5)
            .color(MUTED),
        );
        ui.add_space(6.0);

        if self.cameras.len() > 1 {
            let mut selected = self.current;
            egui::ComboBox::from_id_salt("preset-camera")
                .selected_text(format!("Caméra {}", self.current + 1))
                .show_ui(ui, |ui| {
                    for index in 0..self.cameras.len() {
                        ui.selectable_value(
                            &mut selected,
                            index,
                            format!("Caméra {} — {}", index + 1, self.cameras[index].info().name),
                        );
                    }
                });
            if selected != self.current {
                self.select_camera(selected);
            }
        }

        let camera = self.current.min(NUM_CAMERAS - 1);
        for slot in 0..NUM_PRESETS {
            ui.horizontal(|ui| {
                ui.add_sized(
                    Vec2::new(28.0, 24.0),
                    egui::Label::new(RichText::new(format!("{}", slot + 1)).size(14.0).strong()),
                );
                let mut value = self.config.presets[camera][slot].clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut value)
                        .desired_width(420.0)
                        .hint_text(format!("Libellé du préréglage {}", slot + 1)),
                );
                if response.changed() {
                    self.config.presets[camera][slot] = value;
                    self.mark_dirty();
                }
            });
        }
        ui.add_space(8.0);
        if self.cameras.get(self.current).map(|c| c.has_hardware_presets()).unwrap_or(false) {
            ui.label(
                RichText::new("Préréglages matériels Logitech disponibles.")
                    .size(10.5)
                    .color(GREEN),
            );
        } else {
            ui.label(
                RichText::new("Cette caméra ne propose pas de préréglages matériels Logitech.")
                    .size(10.5)
                    .color(ORANGE),
            );
        }
    }

    fn settings_hotkeys(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                section_title(ui, "Raccourcis globaux");
                ui.label(
                    RichText::new(
                        "Ils sont interceptés par Windows même quand PTZ Control n'a pas le focus. Cliquez sur Modifier puis appuyez sur une combinaison.",
                    )
                    .size(10.5)
                    .color(MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Réinitialiser").clicked() {
                    self.reset_hotkeys();
                }
            });
        });
        ui.add_space(8.0);

        let mut last_group = String::new();
        for index in 0..self.config.hotkeys.len() {
            let group = self.config.hotkeys[index].group();
            if group != last_group {
                last_group = group;
                ui.add_space(8.0);
                ui.label(RichText::new(last_group.to_uppercase()).size(9.5).color(ACCENT));
                ui.add_space(2.0);
            }

            let hotkey = self.config.hotkeys[index].hotkey.clone();
            let id = self.config.hotkeys[index].id();
            let label = self.config.hotkeys[index].label();
            let error = self.hotkey_errors.get(&id).cloned();
            let editing = self.editing_binding == Some(index);

            ui.horizontal(|ui| {
                ui.add_sized(
                    Vec2::new(235.0, 26.0),
                    egui::Label::new(RichText::new(label).size(11.5).color(TEXT)),
                );

                if editing {
                    // The editor below owns the row's controls.
                } else {
                    let text = hotkey
                        .as_ref()
                        .map(|hotkey| hotkey.label())
                        .unwrap_or_else(|| "Non assigné".to_string());
                    let fill = if hotkey.is_some() { CARD } else { Color32::TRANSPARENT };
                    ui.add_sized(
                        Vec2::new(150.0, 26.0),
                        egui::Button::new(
                            RichText::new(text)
                                .size(11.0)
                                .color(if hotkey.is_some() { TEXT } else { MUTED }),
                        )
                        .fill(fill)
                        .stroke(Stroke::new(1.0_f32, BORDER))
                        .corner_radius(CornerRadius::same(7)),
                    );
                    if ui.button("Modifier").clicked() {
                        self.start_hotkey_edit(index);
                    }
                    if hotkey.is_some() && ui.button("Effacer").clicked() {
                        self.clear_hotkey(index);
                    }
                }
                if let Some(error) = error {
                    ui.label(RichText::new(error).size(10.0).color(RED));
                }
            });

            if editing {
                Frame::new()
                    .fill(CARD)
                    .corner_radius(CornerRadius::same(9))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            modifier_chip(ui, &mut self.draft.ctrl, "Ctrl");
                            modifier_chip(ui, &mut self.draft.alt, "Alt");
                            modifier_chip(ui, &mut self.draft.shift, "Maj");
                            modifier_chip(ui, &mut self.draft.win, "Win");
                            ui.add_space(8.0);
                            if self.draft.listening {
                                ui.label(
                                    RichText::new("Appuyez sur une touche…")
                                        .size(11.0)
                                        .color(ACCENT),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Combinaison prête")
                                        .size(11.0)
                                        .color(GREEN),
                                );
                            }
                        });
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Touche").size(11.0).color(MUTED));
                            let mut key = self.draft.key.clone();
                            egui::ComboBox::from_id_salt(("hotkey-key", index))
                                .selected_text(hotkeys::key_label(&key))
                                .width(180.0)
                                .show_ui(ui, |ui| {
                                    egui::ScrollArea::vertical().max_height(260.0).show(
                                        ui,
                                        |ui| {
                                            for (canonical, label) in hotkeys::KEY_OPTIONS {
                                                ui.selectable_value(
                                                    &mut key,
                                                    canonical.to_string(),
                                                    *label,
                                                );
                                            }
                                        },
                                    );
                                });
                            if key != self.draft.key {
                                self.draft.key = key;
                            }
                            if ui.button("Réécouter").clicked() {
                                self.draft.listening = true;
                                self.capture_error = None;
                            }
                        });
                        if let Some(error) = &self.capture_error {
                            ui.label(RichText::new(error).size(10.5).color(RED));
                        }
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            if accent_button(ui, "Enregistrer", Vec2::new(110.0, 28.0)).clicked() {
                                self.commit_hotkey_edit();
                            }
                            if ui.button("Désactiver").clicked() {
                                self.clear_hotkey(index);
                            }
                            if ui.button("Annuler").clicked() {
                                self.cancel_hotkey_edit();
                            }
                        });
                    });
                ui.add_space(4.0);
            }
        }
    }

    fn settings_axes(&mut self, ui: &mut egui::Ui) {
        section_title(ui, "Inversion des axes");
        ui.label(
            RichText::new(
                "Utile quand la caméra est installée à l'envers, derrière une vitre ou que le opérateur préfère une orientation inversée. L'inversion s'applique aux boutons, aux raccourcis et aux commandes externes.",
            )
            .size(10.5)
            .color(MUTED),
        );
        ui.add_space(10.0);

        let mut changed = false;
        changed |= ui
            .checkbox(
                &mut self.config.axes.invert_pan,
                "Inverser le panoramique (gauche ↔ droite)",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.config.axes.invert_tilt,
                "Inverser l'inclinaison (haut ↔ bas)",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.config.axes.invert_zoom,
                "Inverser le zoom (+ ↔ −)",
            )
            .changed();
        changed |= ui
            .checkbox(
                &mut self.config.axes.invert_focus,
                "Inverser la mise au point (proche ↔ loin)",
            )
            .changed();
        if changed {
            self.mark_dirty();
        }

        ui.add_space(14.0);
        Frame::new()
            .fill(CARD)
            .corner_radius(CornerRadius::same(9))
            .inner_margin(Margin::same(12))
            .show(ui, |ui| {
                ui.label(RichText::new("Aperçu de la configuration").size(11.0).color(MUTED));
                ui.add_space(4.0);
                ui.label(
                    RichText::new(format!(
                        "Pan {} · Tilt {} · Zoom {} · Focus {}",
                        if self.config.axes.invert_pan { "inversé" } else { "normal" },
                        if self.config.axes.invert_tilt { "inversé" } else { "normal" },
                        if self.config.axes.invert_zoom { "inversé" } else { "normal" },
                        if self.config.axes.invert_focus { "inversé" } else { "normal" },
                    ))
                    .size(12.0)
                    .color(TEXT),
                );
            });

        ui.add_space(10.0);
        if ui.button("Réinitialiser les axes").clicked() {
            self.config.axes = config::Axes::default();
            self.mark_dirty();
        }
    }
}

impl eframe::App for PtzApp {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.frames_since_start = self.frames_since_start.wrapping_add(1);

        if self.main_hwnd.is_none() {
            if let Ok(handle) = raw_window_handle::HasWindowHandle::window_handle(frame) {
                if let raw_window_handle::RawWindowHandle::Win32(win32) = handle.as_raw() {
                    self.main_hwnd = Some(win32.hwnd.get());
                }
            }
        }
        if self.guard_on && self.watchdog_thread.is_none() {
            if let Some(hwnd) = self.main_hwnd {
                self.start_watchdog(hwnd);
            }
        }

        self.apply_saved_position(ctx);
        self.apply_always_on_top(ctx);
        self.track_window_position(ctx);
        self.poll_events();
        self.process_hotkey_capture(ctx);

        if self.memory_armed
            && self
                .memory_deadline
                .map(|deadline| Instant::now() >= deadline)
                .unwrap_or(false)
        {
            self.memory_armed = false;
            self.memory_deadline = None;
            self.set_status("Mode mémoire annulé", MUTED);
        }

        if self.last_poll.elapsed() > Duration::from_secs(2) {
            self.refresh_from_hardware();
        }

        self.draw_title_bar(ctx);
        self.draw_main(ctx);
        self.settings_viewport(ctx);

        if self.dirty && self.last_save.elapsed() > Duration::from_millis(600) {
            self.persist();
        }

        ctx.request_repaint_after(Duration::from_millis(80));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.stop_watchdog();
        self.persist();
    }
}

impl Drop for PtzApp {
    fn drop(&mut self) {
        self.stop_watchdog();
    }
}

fn configure_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = BG;
    style.visuals.window_fill = PANEL;
    style.visuals.extreme_bg_color = Color32::from_rgb(16, 21, 28);
    style.visuals.faint_bg_color = CARD;
    style.visuals.code_bg_color = CARD;
    style.visuals.selection.bg_fill = ACCENT_SOFT;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.noninteractive.bg_fill = PANEL;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.bg_fill = CARD;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.hovered.bg_fill = CARD_HOVER;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.active.bg_fill = ACCENT_SOFT;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.open.bg_fill = CARD_HOVER;
    style.visuals.widgets.open.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.inactive.corner_radius = CornerRadius::same(8);
    style.visuals.widgets.hovered.corner_radius = CornerRadius::same(8);
    style.visuals.widgets.active.corner_radius = CornerRadius::same(8);
    style.visuals.window_corner_radius = CornerRadius::same(12);
    style.spacing.item_spacing = Vec2::new(8.0, 7.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::proportional(13.0),
    );
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::proportional(13.0),
    );
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::proportional(20.0),
    );
    ctx.set_style(style);
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(12.0).color(TEXT).strong());
}

fn compact_button(
    ui: &mut egui::Ui,
    text: &str,
    size: Vec2,
    fill: Color32,
    text_color: Color32,
) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(RichText::new(text).size(11.5).color(text_color))
            .fill(fill)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(8)),
    )
}

fn accent_button(ui: &mut egui::Ui, text: &str, size: Vec2) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(RichText::new(text).size(12.0).color(TEXT).strong())
            .fill(ACCENT_SOFT)
            .stroke(Stroke::new(1.0_f32, ACCENT))
            .corner_radius(CornerRadius::same(9)),
    )
}

fn toolbar_nav_button(
    ui: &mut egui::Ui,
    text: &str,
    size: Vec2,
    font_size: f32,
) -> egui::Response {
    ui.add_sized(
        size,
        egui::Button::new(RichText::new(text).size(font_size).color(TEXT))
            .fill(CARD)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .corner_radius(CornerRadius::same(5)),
    )
}

fn modifier_chip(ui: &mut egui::Ui, value: &mut bool, label: &str) {
    let response = ui.add(
        egui::Button::new(
            RichText::new(label)
                .size(11.0)
                .color(if *value { TEXT } else { MUTED }),
        )
        .fill(if *value { ACCENT_SOFT } else { Color32::TRANSPARENT })
        .stroke(Stroke::new(1.0_f32, if *value { ACCENT } else { BORDER }))
        .corner_radius(CornerRadius::same(7))
        .min_size(Vec2::new(48.0, 26.0)),
    );
    if response.clicked() {
        *value = !*value;
    }
}
