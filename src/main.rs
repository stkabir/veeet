#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod config;
mod hotkeys;
mod platform;
mod preset;
mod render;
mod shuffle;
mod ui;
mod update;

use eframe::egui;

use config::{Config, WindowMode};

/// Icono de ventana y barra de tareas (el del .exe lo incrusta build.rs).
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/logo/app-icon-256.png");

fn enable_dpi_awareness() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::HiDpi::{
            DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
        };
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

fn main() -> eframe::Result {
    // Tras una actualización: esperar a que salga la versión anterior y
    // limpiar sus restos.
    update::wait_for_previous();
    update::cleanup_old();
    enable_dpi_awareness();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let fake_audio = std::env::args().any(|arg| arg == "--fake")
        || app::env_var("FAKE").is_some_and(|v| v == "1");

    let config = Config::load();
    let has_saved_position =
        config.window.windowed_x.is_some() && config.window.windowed_y.is_some();

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Veeet")
        .with_inner_size([
            config.window.windowed_width.max(320.0),
            config.window.windowed_height.max(200.0),
        ])
        .with_min_inner_size([320.0, 200.0])
        .with_decorations(true)
        .with_resizable(true)
        .with_taskbar(true)
        .with_fullscreen(config.mode == WindowMode::Fullscreen);

    if let (Some(x), Some(y)) = (config.window.windowed_x, config.window.windowed_y) {
        viewport = viewport.with_position([x, y]);
    }

    match eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
        Ok(icon) => viewport = viewport.with_icon(icon),
        Err(err) => log::warn!("no se pudo cargar el icono de la ventana: {err}"),
    }

    let native_options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Wgpu,
        wgpu_options: eframe::egui_wgpu::WgpuConfiguration {
            surface: eframe::egui_wgpu::SurfaceConfig::LOW_LATENCY,
            ..Default::default()
        },
        centered: !has_saved_position,
        ..Default::default()
    };

    eframe::run_native(
        "Veeet",
        native_options,
        Box::new(move |cc| Ok(Box::new(app::VeeetApp::new(cc, fake_audio, config)))),
    )
}
