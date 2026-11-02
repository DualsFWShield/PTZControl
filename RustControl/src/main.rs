//! PTZControl — modern Rust rewrite of the original MFC application.
//!
//! Features
//! * Generic DirectShow PTZ control (pan, tilt, zoom, focus, image tuning).
//! * Logitech extension-unit support for the Rally / PTZ Pro family.
//! * System-wide, user-editable keyboard shortcuts.
//! * Axis inversion for ceiling-mounted cameras.
//! * Single instance with command forwarding, compatible CLI syntax.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod camera;
mod cli;
mod config;
mod hotkeys;
mod platform;

use eframe::egui;

const APP_TITLE: &str = "PTZ Control";

fn load_icon() -> Option<egui::IconData> {
    let bytes = include_bytes!("../assets/icon.png");
    let image = image::load_from_memory(bytes).ok()?.into_rgba8();
    let (width, height) = image.dimensions();
    Some(egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}

fn main() -> eframe::Result<()> {
    let parsed = cli::parse();
    let options = parsed.options.clone();

    // Forward the invocation to an already running instance. The original MFC
    // tool behaved the same way so Stream Deck / Companion buttons stay usable
    // even when the control window is already open.
    let (instance_guard, already_running) = match platform::acquire_single_instance() {
        Ok(pair) => pair,
        Err(err) => {
            eprintln!("PTZControl: {err}");
            return Ok(());
        }
    };

    if already_running {
        let message = cli::RemoteMessage {
            activate: true,
            command: options.command.clone(),
        };
        if !platform::send_to_running_instance(&message) {
            eprintln!("PTZControl: une instance tourne déjà mais ne répond pas");
        }
        std::mem::forget(instance_guard);
        return Ok(());
    }

    if parsed.syntax_error {
        eprintln!("PTZControl: syntaxe de ligne de commande invalide (voir README.md)");
    }

    // DirectShow needs a COM apartment on the thread that opens the devices.
    // eframe's window and our main loop both live here.
    unsafe {
        use windows::Win32::System::Com::{CoInitializeEx, COINIT_APARTMENTTHREADED};
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok();
    }

    let config = config::load();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_decorations(false)
        .with_inner_size([375.0, 128.0])
        .with_min_inner_size([340.0, 118.0])
        .with_resizable(true);
    if config.general.always_on_top {
        viewport = viewport.with_always_on_top();
    }
    if let Some(icon) = load_icon() {
        viewport = viewport.with_icon(icon);
    }
    if let Some(x) = config.window.x {
        if let Some(y) = config.window.y {
            viewport = viewport.with_position([x, y]);
        }
    }

    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    let result = eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(move |cc| {
            let command = options.command.clone();
            Ok(Box::new(app::PtzApp::new(
                cc,
                config,
                options,
                command,
            )))
        }),
    );

    // Keep the single-instance mutex alive for the whole process lifetime.
    std::mem::forget(instance_guard);
    unsafe {
        use windows::Win32::System::Com::CoUninitialize;
        CoUninitialize();
    }
    result
}
