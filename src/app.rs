use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui;
use raw_window_handle::HasWindowHandle;
use windows::Win32::Foundation::HWND;

use crate::audio::{
    AnalysisFrame, AnalysisSettings, Analyzer, AudioApp, AudioEngine, CaptureState, FakeAudio,
};
use crate::config::{CaptureMode, Config, VisualPreset, WindowMode};
use crate::hotkeys::{HotkeyAction, Hotkeys};
use crate::platform::{self, RectI, SpotifyWindow};
use crate::preset::{EXPORT_EXTENSION, ImportOutcome, PresetStore, builtin_id};
use crate::render;
use crate::shuffle::{self, Shuffle};
use crate::ui::history::{self, History, Snapshot};
use crate::ui::{MenuAction, MenuTab, dock};

const TICK: Duration = Duration::from_millis(16);
const PLACEMENT_INTERVAL: Duration = Duration::from_millis(250);
const SPOTIFY_SEARCH_INTERVAL: Duration = Duration::from_secs(2);
const UI_HIDE_DELAY: Duration = Duration::from_millis(2500);
const MODE_TRANSITION_DELAY: Duration = Duration::from_millis(350);
/// Tiempo que se muestra un aviso en el pie del cajón.
const NOTICE_DURATION: Duration = Duration::from_secs(5);

pub struct VeeetApp {
    config: Config,
    /// Todos los presets; `config.visual` es la copia de trabajo del activo.
    presets: PresetStore,
    /// Aviso breve para el usuario (importaciones, errores).
    notice: Option<(String, Instant)>,
    /// Reloj de la escena: avanza a `time_scale` del preset activo.
    scene_time: f32,
    /// Modo aleatorio (ver `config.shuffle`).
    shuffle: Shuffle,
    hwnd: Option<HWND>,
    spotify: Option<SpotifyWindow>,
    last_spotify_search: Instant,
    last_placement_at: Instant,
    last_applied: Option<(RectI, bool)>,
    last_follow_pos: Option<(i32, i32)>,
    applied_mode: Option<WindowMode>,
    mode_changed_at: Instant,
    applied_cursor_hidden: Option<bool>,
    applied_topmost: Option<bool>,
    applied_dark_title: Option<bool>,
    focused_fullscreen: bool,
    effective_visible: bool,
    force_placement: bool,
    user_visible: bool,
    click_through: bool,
    debug: bool,
    show_menu: bool,
    /// Cajón de ajustes abierto sobre el dock.
    drawer_open: bool,
    menu_tab: MenuTab,
    /// Pila de deshacer de los cambios hechos en el menú.
    history: History,
    audio: Option<AudioEngine>,
    /// Fuente con la que arrancó `audio`; si cambia en la config, se reinicia.
    audio_key: (CaptureMode, String),
    /// Apps con sesión de audio (pestaña Audio del menú).
    audio_apps: Vec<AudioApp>,
    fake: Option<FakeAudio>,
    audio_state: CaptureState,
    analyzer: Analyzer,
    analysis: AnalysisFrame,
    last_tick: Instant,
    time: f32,
    fps: f32,
    fps_frames: u32,
    fps_at: Instant,
    hotkeys: Hotkeys,
    esc_down: bool,
    last_ui_interaction: Instant,
}

impl VeeetApp {
    pub fn new(cc: &eframe::CreationContext<'_>, fake_audio: bool, mut config: Config) -> Self {
        apply_env_overrides(&mut config);
        let mut presets = PresetStore::load(&mut config);
        if let Some(value) = env_var("PRESET") {
            apply_env_preset(&mut presets, &mut config, &value);
        }
        // Guardar ya: así la sección `[visual]` vieja no vuelve a migrarse.
        if let Err(err) = config.save() {
            log::warn!("No se pudo guardar la config: {err:#}");
        }
        log::info!(
            "Objetivo de ventana: '{}' · proceso de audio: '{}' · modo: {}",
            config.window.target_process,
            config.audio.process,
            config.mode.label()
        );
        let hwnd = cc
            .window_handle()
            .ok()
            .and_then(|handle| platform::hwnd_from_raw(handle.as_raw()));

        let audio = if fake_audio {
            None
        } else {
            Some(AudioEngine::start(config.audio.clone()))
        };
        let fake = if fake_audio {
            log::info!("Modo de audio sintético activado (--fake)");
            Some(FakeAudio::new())
        } else {
            None
        };

        let click_through =
            config.mode == WindowMode::Overlay && config.window.overlay_click_through;

        let mut fonts = egui::FontDefinitions::default();
        fonts.font_data.insert(
            "outfit".to_owned(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/fonts/Outfit-Bold.ttf"
            ))),
        );
        fonts.font_data.insert(
            "instrument".to_owned(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/fonts/InstrumentSans-Regular.ttf"
            ))),
        );
        fonts.font_data.insert(
            "geist-mono".to_owned(),
            Arc::new(egui::FontData::from_static(include_bytes!(
                "../assets/fonts/GeistMono-Regular.ttf"
            ))),
        );
        if let Some(family) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
            family.insert(0, "instrument".to_owned());
        }
        fonts.families.insert(
            egui::FontFamily::Name("outfit".into()),
            vec!["outfit".to_owned()],
        );
        fonts.families.insert(
            egui::FontFamily::Name("mono".into()),
            vec!["geist-mono".to_owned()],
        );
        cc.egui_ctx.set_fonts(fonts);

        cc.egui_ctx.set_visuals(egui::Visuals::dark());
        cc.egui_ctx.set_theme(egui::ThemePreference::Dark);

        if let Some(render_state) = cc.wgpu_render_state.as_ref() {
            let renderer = render::WaveRenderer::new(
                &render_state.device,
                &render_state.adapter,
                render_state.target_format,
            );
            render_state
                .renderer
                .write()
                .callback_resources
                .insert(renderer);
        } else {
            log::error!("El backend wgpu no está disponible");
        }

        let audio_key = (config.audio.mode, config.audio.process.clone());
        let now = Instant::now();
        Self {
            click_through,
            hwnd,
            spotify: None,
            last_spotify_search: now - SPOTIFY_SEARCH_INTERVAL,
            last_placement_at: now - PLACEMENT_INTERVAL,
            last_applied: None,
            last_follow_pos: None,
            applied_mode: None,
            mode_changed_at: now - MODE_TRANSITION_DELAY,
            applied_cursor_hidden: None,
            applied_topmost: None,
            applied_dark_title: None,
            focused_fullscreen: false,
            effective_visible: true,
            force_placement: true,
            user_visible: true,
            debug: std::env::args().any(|arg| arg == "--debug"),
            show_menu: false,
            drawer_open: false,
            menu_tab: MenuTab::default(),
            history: History::default(),
            audio,
            audio_key,
            audio_apps: Vec::new(),
            fake,
            audio_state: CaptureState::Starting,
            analyzer: Analyzer::new(48_000),
            analysis: AnalysisFrame::default(),
            last_tick: now,
            time: 0.0,
            fps: 0.0,
            fps_frames: 0,
            fps_at: now,
            hotkeys: Hotkeys::new(&config.hotkeys),
            esc_down: false,
            last_ui_interaction: now,
            config,
            presets,
            notice: None,
            scene_time: 0.0,
            shuffle: Shuffle::new(),
        }
    }

    fn tick(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        let now = Instant::now();
        let dt = now
            .duration_since(self.last_tick)
            .as_secs_f32()
            .clamp(0.001, 0.1);
        self.last_tick = now;
        self.time += dt;

        if ctx.input(|i| i.pointer.delta().length_sq() > 0.0 || i.pointer.any_click()) {
            self.last_ui_interaction = now;
        }

        self.handle_hotkeys(ctx);
        self.sync_presets();
        self.poll_audio(dt, now);

        if self.config.mode == WindowMode::Fullscreen {
            let down = platform::escape_pressed();
            if down && !self.esc_down && !self.close_menu_step() {
                self.set_mode(WindowMode::Windowed);
            }
            self.esc_down = down;
        } else {
            self.esc_down = false;
            if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
                self.close_menu_step();
            }
        }

        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        let width_px = screen.width() * ppp;
        let height_px = screen.height() * ppp;
        let visual = &self.config.visual;
        let settings = AnalysisSettings {
            gain: visual.gain,
            spectrum_smoothing: visual.spectrum_smoothing,
            beat_sensitivity: visual.beat_sensitivity,
            motion_smoothing: visual.smoothing,
            freq_influence: visual.freq_influence,
        };
        self.analysis = self.analyzer.analyze(dt, &settings);
        self.scene_time += dt * visual.time_scale.clamp(0.0, 4.0);
        let step = self
            .shuffle
            .step(dt, &self.config.shuffle, self.analysis.beat);
        if step.switch {
            self.shuffle_next();
        }

        if let Some(render_state) = frame.wgpu_render_state()
            && let Some(renderer) = render_state
                .renderer
                .write()
                .callback_resources
                .get_mut::<render::WaveRenderer>()
        {
            renderer.update_scene(
                &self.analysis,
                &self.config.visual,
                width_px,
                height_px,
                self.scene_time,
                ppp,
                step.fade,
            );
        }

        self.update_fps();
        self.update_window(ctx, frame, now);

        ctx.request_repaint_after(TICK);
    }

    fn poll_audio(&mut self, dt: f32, now: Instant) {
        if let Some(fake) = &mut self.fake {
            let frames = ((dt * 48_000.0).round() as usize).max(64);
            let samples = fake.generate(frames, 48_000);
            self.analyzer.push_samples(&samples);
            self.audio_state = CaptureState::Running {
                source: "audio sintético (--fake)".to_string(),
            };
            return;
        }

        let key = (self.config.audio.mode, self.config.audio.process.clone());
        if self.audio.is_some() && key != self.audio_key {
            log::info!("Fuente de audio cambiada: {:?} · {}", key.0, key.1);
            // Soltar el motor viejo antes de abrir el nuevo (join del hilo).
            self.audio = None;
            self.audio = Some(AudioEngine::start(self.config.audio.clone()));
            self.audio_key = key;
            self.audio_state = CaptureState::Starting;
        }

        let Some(engine) = &self.audio else {
            return;
        };
        let snapshot = engine.poll(16_384);
        self.audio_state = snapshot.state;
        self.audio_apps = snapshot.apps;
        if !snapshot.samples.is_empty() {
            self.analyzer.push_samples(&snapshot.samples);
        } else {
            let stale = snapshot
                .last_sample_at
                .map(|t| now.duration_since(t))
                .unwrap_or(Duration::from_secs(10));
            if stale > Duration::from_millis(150) {
                // Sin paquetes nuevos: empujamos silencio para que la onda se aplane.
                let frames = ((dt * 48_000.0).round() as usize).clamp(0, 4096);
                let silence = vec![0.0; frames];
                self.analyzer.push_samples(&silence);
            }
        }
    }

    fn handle_hotkeys(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(egui::Key::F11)) {
            if self.config.mode == WindowMode::Fullscreen {
                self.set_mode(WindowMode::Windowed);
            } else {
                self.set_mode(WindowMode::Fullscreen);
            }
        }

        for action in self.hotkeys.poll() {
            match action {
                HotkeyAction::ToggleVisible => {
                    self.user_visible = !self.user_visible;
                    self.force_placement = true;
                    log::info!(
                        "Ventana {}",
                        if self.user_visible {
                            "visible"
                        } else {
                            "oculta"
                        }
                    );
                }
                HotkeyAction::CycleMode => {
                    let next = self.config.mode.next();
                    self.set_mode(next);
                }
                HotkeyAction::ToggleClickThrough => {
                    if self.config.mode == WindowMode::Overlay {
                        self.click_through = !self.click_through;
                        self.config.window.overlay_click_through = self.click_through;
                        log::info!(
                            "Click-through {}",
                            if self.click_through {
                                "activado"
                            } else {
                                "desactivado"
                            }
                        );
                        if let Err(err) = self.config.save() {
                            log::warn!("No se pudo guardar la config: {err:#}");
                        }
                    }
                }
                HotkeyAction::ToggleDebug => {
                    self.debug = !self.debug;
                }
                HotkeyAction::ToggleMenu => {
                    self.show_menu = !self.show_menu;
                    self.user_visible = true;
                    self.force_placement = true;
                    self.last_ui_interaction = Instant::now();
                    if self.show_menu
                        && let Some(hwnd) = self.hwnd
                    {
                        platform::focus_window(hwnd);
                    }
                }
                HotkeyAction::Quit => {
                    self.save_config();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    fn set_mode(&mut self, mode: WindowMode) {
        if self.config.mode == mode {
            return;
        }
        self.config.mode = mode;
        self.click_through =
            mode == WindowMode::Overlay && self.config.window.overlay_click_through;
        self.applied_mode = None;
        self.mode_changed_at = Instant::now();
        self.last_applied = None;
        self.last_follow_pos = None;
        self.applied_cursor_hidden = None;
        self.applied_dark_title = None;
        self.focused_fullscreen = false;
        self.force_placement = true;
        self.last_ui_interaction = Instant::now();
        log::info!("Modo: {}", mode.label());
        self.save_config();
    }

    fn apply_mode_commands(&mut self, ctx: &egui::Context) {
        match self.config.mode {
            WindowMode::Windowed => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    if self.config.window.always_on_top {
                        egui::WindowLevel::AlwaysOnTop
                    } else {
                        egui::WindowLevel::Normal
                    },
                ));
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    self.config.window.windowed_width.max(320.0),
                    self.config.window.windowed_height.max(200.0),
                )));
                if let (Some(x), Some(y)) =
                    (self.config.window.windowed_x, self.config.window.windowed_y)
                {
                    ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
                }
            }
            WindowMode::Fullscreen => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::AlwaysOnTop,
                ));
            }
            WindowMode::Docked | WindowMode::Overlay => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::Resizable(false));
                ctx.send_viewport_cmd(egui::ViewportCommand::WindowLevel(
                    egui::WindowLevel::AlwaysOnTop,
                ));
            }
        }
    }

    fn update_fps(&mut self) {
        self.fps_frames += 1;
        let elapsed = self.fps_at.elapsed().as_secs_f32();
        if elapsed >= 0.5 {
            self.fps = self.fps_frames as f32 / elapsed;
            self.fps_frames = 0;
            self.fps_at = Instant::now();
        }
    }

    fn update_window(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame, now: Instant) {
        if self.hwnd.is_none() {
            self.hwnd = frame
                .window_handle()
                .ok()
                .and_then(|handle| platform::hwnd_from_raw(handle.as_raw()));
            if let Some(hwnd) = self.hwnd {
                log::info!("HWND de la ventana: {:?}", hwnd.0);
            }
        }
        if self.hwnd.is_none() {
            return;
        }

        if self.applied_mode != Some(self.config.mode) {
            self.apply_mode_commands(ctx);
            self.applied_mode = Some(self.config.mode);
            self.mode_changed_at = now;
            self.last_applied = None;
            self.last_follow_pos = None;
            self.force_placement = true;
        }

        let fullscreen = self.config.mode == WindowMode::Fullscreen;
        let ui_visible = !fullscreen
            || !self.config.window.ui_auto_hide
            || now.duration_since(self.last_ui_interaction) < UI_HIDE_DELAY
            || self.show_menu;
        let want_cursor_hidden = fullscreen && !ui_visible && self.user_visible;
        if self.applied_cursor_hidden != Some(want_cursor_hidden) {
            if let Some(window) = frame.winit_window() {
                window.set_cursor_visible(!want_cursor_hidden);
            }
            if want_cursor_hidden && let Some(hwnd) = self.hwnd {
                platform::focus_window(hwnd);
            }
            self.applied_cursor_hidden = Some(want_cursor_hidden);
        }

        let Some(hwnd) = self.hwnd else {
            return;
        };

        if self.config.mode == WindowMode::Fullscreen && !self.focused_fullscreen {
            platform::focus_window(hwnd);
            self.focused_fullscreen = true;
        } else if self.config.mode != WindowMode::Fullscreen {
            self.focused_fullscreen = false;
        }

        let overlay_active = self.config.mode == WindowMode::Overlay && self.user_visible;
        let effective_click_through =
            self.click_through && overlay_active && !self.debug && !self.show_menu;
        platform::apply_window_styles(
            hwnd,
            overlay_active,
            self.config.window.overlay_opacity,
            effective_click_through,
        );

        let dark_title = self.config.mode == WindowMode::Windowed;
        if self.applied_dark_title != Some(dark_title) {
            platform::set_dark_title_bar(hwnd, dark_title);
            self.applied_dark_title = Some(dark_title);
        }

        if self.config.mode != WindowMode::Fullscreen {
            let want_topmost = match self.config.mode {
                WindowMode::Windowed => self.config.window.always_on_top,
                WindowMode::Docked | WindowMode::Overlay => true,
                WindowMode::Fullscreen => false,
            };
            if self.applied_topmost != Some(want_topmost) {
                platform::set_topmost(hwnd, want_topmost);
                self.applied_topmost = Some(want_topmost);
            }
        } else {
            self.applied_topmost = None;
        }

        if !self.force_placement && now.duration_since(self.last_placement_at) < PLACEMENT_INTERVAL
        {
            return;
        }
        if now.duration_since(self.mode_changed_at) < MODE_TRANSITION_DELAY {
            return;
        }
        self.last_placement_at = now;

        self.refresh_spotify(now);

        match self.config.mode {
            WindowMode::Windowed => {
                self.capture_windowed_geometry(ctx, frame);
                self.set_window_visible(ctx, self.user_visible);
                if self.config.window.follow_spotify {
                    if let Some(spotify) = self.spotify.clone()
                        && !spotify.minimized
                    {
                        self.follow_spotify(hwnd, &spotify, ctx);
                    }
                } else {
                    self.last_follow_pos = None;
                }
            }
            WindowMode::Fullscreen => {
                self.set_window_visible(ctx, self.user_visible);
            }
            WindowMode::Docked | WindowMode::Overlay => {
                let ppp = ctx.pixels_per_point();
                let placement =
                    platform::compute_placement(&self.config, self.spotify.as_ref(), hwnd, ppp);
                let show = self.user_visible && placement.visible;
                self.set_window_visible(ctx, show);
                if show {
                    let signature = (placement.rect, true);
                    if self.last_applied != Some(signature) || self.force_placement {
                        platform::apply_placement(hwnd, &placement);
                        self.last_applied = Some(signature);
                    }
                }
            }
        }

        self.force_placement = false;
    }

    fn set_window_visible(&mut self, ctx: &egui::Context, show: bool) {
        if show != self.effective_visible {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(show));
            self.effective_visible = show;
            if show {
                self.last_applied = None;
                self.force_placement = true;
            }
        }
    }

    fn capture_windowed_geometry(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        let Some(window) = frame.winit_window() else {
            return;
        };
        if window.is_minimized().unwrap_or(false) {
            return;
        }
        let ppp = ctx.pixels_per_point();
        let size = window.inner_size();
        let logical = size.to_logical::<f32>(ppp as f64);
        if logical.width >= 200.0 && logical.height >= 120.0 {
            self.config.window.windowed_width = logical.width;
            self.config.window.windowed_height = logical.height;
        }
        if let Ok(pos) = window.outer_position() {
            let logical = pos.to_logical::<f32>(ppp as f64);
            self.config.window.windowed_x = Some(logical.x);
            self.config.window.windowed_y = Some(logical.y);
        }
    }

    fn follow_spotify(&mut self, hwnd: HWND, spotify: &SpotifyWindow, ctx: &egui::Context) {
        if platform::is_minimized(hwnd) {
            return;
        }
        let Some(own) = platform::outer_rect(hwnd) else {
            return;
        };
        let areas = platform::work_area_for(spotify.hwnd);
        let ppp = ctx.pixels_per_point();
        let gap = (self.config.window.dock_gap * ppp).round() as i32;
        let width = own.width();
        let height = own.height();

        let right_x = spotify.rect.right + gap;
        let left_x = spotify.rect.left - gap - width;
        let x = if right_x + width <= areas.work.right {
            right_x
        } else if left_x >= areas.work.left {
            left_x
        } else {
            (areas.work.right - width).max(areas.work.left)
        };
        let y = spotify.rect.top.clamp(
            areas.work.top,
            (areas.work.bottom - height).max(areas.work.top),
        );

        if own.left == x && own.top == y {
            return;
        }
        if self.last_follow_pos == Some((x, y)) && own.left == x && own.top == y {
            return;
        }
        platform::set_window_position(hwnd, x, y);
        self.last_follow_pos = Some((x, y));
    }

    fn refresh_spotify(&mut self, now: Instant) {
        if let Some(current) = self.spotify.clone() {
            match platform::refresh_spotify(&current) {
                Some(updated) => self.spotify = Some(updated),
                None => {
                    log::info!("La ventana de Spotify ya no existe");
                    self.spotify = None;
                    self.last_spotify_search = now;
                    self.last_applied = None;
                }
            }
        }

        if self.spotify.is_none()
            && now.duration_since(self.last_spotify_search) >= SPOTIFY_SEARCH_INTERVAL
        {
            self.last_spotify_search = now;
            self.spotify = platform::find_spotify_window(&self.config.window.target_process);
            if let Some(spotify) = &self.spotify {
                log::info!(
                    "Ventana objetivo encontrada: '{}' en {:?} (minimizado: {})",
                    spotify.title,
                    spotify.rect,
                    spotify.minimized
                );
                self.last_applied = None;
                self.last_follow_pos = None;
            }
        }
    }

    fn ui_visible(&self, now: Instant) -> bool {
        if self.config.mode != WindowMode::Fullscreen {
            return true;
        }
        !self.config.window.ui_auto_hide
            || now.duration_since(self.last_ui_interaction) < UI_HIDE_DELAY
            || self.show_menu
    }

    /// Cierra un nivel del menú: primero el cajón, luego el dock.
    /// Devuelve `false` si no había nada abierto.
    fn close_menu_step(&mut self) -> bool {
        if self.show_menu && self.drawer_open {
            self.drawer_open = false;
            true
        } else if self.show_menu {
            self.apply_menu_action(MenuAction::Close, None);
            true
        } else {
            false
        }
    }

    fn draw_menu(&mut self, ui: &mut egui::Ui, content: egui::Rect) {
        let ctx = ui.ctx().clone();
        let audio = self.audio_state.label();
        let source = match &self.audio_state {
            _ if self.fake.is_some() => "audio sintético".to_string(),
            CaptureState::Running { source } => source.clone(),
            _ => "sin audio".to_string(),
        };
        let status = dock::Status {
            source: &source,
            audio: &audio,
            apps: &self.audio_apps,
            fps: self.fps,
            can_undo: self.history.can_undo(),
            notice: self
                .notice
                .as_ref()
                .filter(|(_, at)| at.elapsed() < NOTICE_DURATION)
                .map(|(text, _)| text.as_str()),
        };
        let before = Snapshot::of(&self.config);
        let actions = dock::draw_dock(
            ui,
            content,
            &mut self.drawer_open,
            &mut self.menu_tab,
            &mut self.config,
            &mut self.presets,
            &status,
        );
        // Un gesto (arrastre de slider) termina al soltar el puntero.
        let gesture_active = ctx.input(|i| i.pointer.any_down());
        let undoing = actions.contains(&MenuAction::Undo);
        // Los textos de la biblioteca van directo al store.
        self.presets.maybe_flush();
        if !undoing {
            self.history.observe(before, &self.config, gesture_active);
        }
        for action in actions {
            self.apply_menu_action(action, Some(&ctx));
        }
    }

    /// Ejecuta los efectos que pide la UI.
    fn apply_menu_action(&mut self, action: MenuAction, ctx: Option<&egui::Context>) {
        match action {
            MenuAction::Close => {
                self.show_menu = false;
                self.drawer_open = false;
                self.save_config();
            }
            MenuAction::Save => self.save_config(),
            MenuAction::Reset(tab) => {
                self.history.push(Snapshot::of(&self.config));
                history::reset_tab(&mut self.config, tab);
                if tab == MenuTab::Window {
                    self.force_placement = true;
                    self.last_follow_pos = None;
                    self.applied_topmost = None;
                }
            }
            MenuAction::Undo => {
                if self.history.undo(&mut self.config) {
                    self.force_placement = true;
                    self.last_follow_pos = None;
                    self.applied_topmost = None;
                }
            }
            MenuAction::Hide => {
                self.user_visible = false;
                self.show_menu = false;
                self.drawer_open = false;
                self.force_placement = true;
            }
            MenuAction::Quit => {
                self.save_config();
                if let Some(ctx) = ctx {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
            MenuAction::SetMode(mode) => self.set_mode(mode),
            MenuAction::FollowChanged => {
                self.force_placement = true;
                self.last_follow_pos = None;
            }
            MenuAction::TopmostChanged => self.applied_topmost = None,
            MenuAction::SelectPreset(id) => {
                self.select_preset(&id, true);
                self.shuffle.restart();
            }
            MenuAction::ShuffleNext => self.shuffle.skip(),
            MenuAction::NewPreset(engine) => {
                self.presets.sync_params(&self.config.visual);
                let id = self.presets.create(engine);
                self.select_preset(&id, true);
                self.menu_tab = MenuTab::Library;
                self.set_notice("Preset creado: ponle nombre en «Mis presets»");
            }
            MenuAction::DuplicatePreset(id) => {
                self.presets.sync_params(&self.config.visual);
                if let Some(index) = self.presets.find(&id)
                    && let Some(copy) = self.presets.duplicate(index)
                {
                    self.select_preset(&copy, true);
                    self.menu_tab = MenuTab::Library;
                    self.set_notice("Copia creada: ya puedes editarla");
                }
            }
            MenuAction::DeletePreset(id) => {
                let name = self
                    .presets
                    .find(&id)
                    .map(|i| self.presets.entries()[i].preset.name.clone())
                    .unwrap_or_default();
                if self.presets.delete(&id) {
                    self.config.active_preset = self.presets.active_id().to_string();
                    self.config.visual = self.presets.active_params().clone();
                    self.set_notice(&format!("«{name}» borrado"));
                } else {
                    self.set_notice("No se pudo borrar el preset");
                }
            }
            MenuAction::ExportPreset(id) => self.export_preset(&id),
            MenuAction::ImportPresets => {
                let files = rfd::FileDialog::new()
                    .set_title("Importar presets de Veeet")
                    .add_filter("Preset de Veeet", &["toml"])
                    .pick_files()
                    .unwrap_or_default();
                self.import_files(&files);
            }
            MenuAction::OpenMarketFolder => {
                let dir = self.presets.market_dir();
                if let Err(err) = std::fs::create_dir_all(&dir)
                    .and_then(|_| std::process::Command::new("explorer").arg(&dir).spawn())
                {
                    log::warn!("No se pudo abrir {}: {err}", dir.display());
                }
            }
            MenuAction::RescanMarket => {
                self.presets.sync_params(&self.config.visual);
                self.presets.scan(crate::preset::Origin::Market);
                self.adopt_active();
                self.set_notice("Galería actualizada");
            }
        }
    }

    /// Modo aleatorio: pasa a otro de los presets elegidos.
    fn shuffle_next(&mut self) {
        let ids: Vec<&str> = self
            .presets
            .entries()
            .iter()
            .map(|e| e.preset.id.as_str())
            .collect();
        let candidates = shuffle::candidates(&self.config.shuffle, &ids);
        let next = self
            .shuffle
            .pick(&candidates, self.presets.active_id())
            .map(str::to_string);
        if let Some(id) = next {
            self.select_preset(&id, false);
        }
    }

    /// Activa un preset. `record` guarda el estado previo para deshacer.
    fn select_preset(&mut self, id: &str, record: bool) {
        if record {
            self.history.push(Snapshot::of(&self.config));
        }
        // Lo último que se tocó va al preset que se deja.
        self.presets.sync_params(&self.config.visual);
        if let Some(params) = self.presets.select(id) {
            self.config.visual = params;
            self.config.active_preset = id.to_string();
        }
    }

    /// Copia en la config el preset que el store tiene activo.
    fn adopt_active(&mut self) {
        self.config.active_preset = self.presets.active_id().to_string();
        self.config.visual = self.presets.active_params().clone();
    }

    /// Lleva los cambios de la copia de trabajo al store. Si deshacer
    /// cambió el preset activo, primero se cambia de preset.
    fn sync_presets(&mut self) {
        if self.config.active_preset != self.presets.active_id() {
            let id = self.config.active_preset.clone();
            if self.presets.select(&id).is_none() {
                // El preset ya no existe (se borró): queda el activo.
                self.adopt_active();
            }
        }
        self.presets.sync_params(&self.config.visual);
        self.presets.maybe_flush();
    }

    fn set_notice(&mut self, text: &str) {
        self.notice = Some((text.to_string(), Instant::now()));
    }

    fn export_preset(&mut self, id: &str) {
        self.presets.sync_params(&self.config.visual);
        let Some(index) = self.presets.find(id) else {
            return;
        };
        let name = &self.presets.entries()[index].preset.name;
        let mut file = crate::preset::sanitize_id(name);
        if file.is_empty() {
            file = "preset".to_string();
        }
        let Some(path) = rfd::FileDialog::new()
            .set_title("Exportar preset")
            .set_file_name(format!("{file}.{EXPORT_EXTENSION}"))
            .add_filter("Preset de Veeet", &["toml"])
            .save_file()
        else {
            return;
        };
        match self.presets.export(index, &path) {
            Ok(()) => self.set_notice(&format!("Exportado a {}", path.display())),
            Err(err) => {
                log::warn!("No se pudo exportar: {err:#}");
                self.set_notice(&format!("No se pudo exportar: {err}"));
            }
        }
    }

    /// Instala archivos de preset en la galería y activa el último.
    fn import_files(&mut self, files: &[std::path::PathBuf]) {
        if files.is_empty() {
            return;
        }
        self.presets.sync_params(&self.config.visual);
        let (mut added, mut updated, mut same, mut failed) = (0, 0, 0, 0);
        let mut last = None;
        for path in files {
            match self.presets.import(path) {
                Ok(outcome) => {
                    match &outcome {
                        ImportOutcome::Added(_) => added += 1,
                        ImportOutcome::Updated(_) => updated += 1,
                        ImportOutcome::Unchanged(_) => same += 1,
                    }
                    last = Some(outcome.id().to_string());
                }
                Err(err) => {
                    failed += 1;
                    log::warn!("No se pudo importar {}: {err:#}", path.display());
                }
            }
        }
        if let Some(id) = last {
            self.select_preset(&id, true);
            self.menu_tab = MenuTab::Market;
        }
        let mut parts = Vec::new();
        if added > 0 {
            parts.push(format!("{added} nuevo(s)"));
        }
        if updated > 0 {
            parts.push(format!("{updated} actualizado(s)"));
        }
        if same > 0 {
            parts.push(format!("{same} ya instalado(s)"));
        }
        if failed > 0 {
            parts.push(format!("{failed} inválido(s)"));
        }
        self.set_notice(&format!("Importación: {}", parts.join(" · ")));
    }

    fn save_config(&mut self) {
        self.presets.sync_params(&self.config.visual);
        self.presets.flush();
        if let Err(err) = self.config.save() {
            log::warn!("No se pudo guardar la config: {err:#}");
        }
    }

    fn draw_debug(&self, ui: &egui::Ui, rect: egui::Rect) {
        let spotify = self
            .spotify
            .as_ref()
            .map(|s| {
                format!(
                    "'{}' {}x{} min={}",
                    s.title,
                    s.rect.width(),
                    s.rect.height(),
                    s.minimized
                )
            })
            .unwrap_or_else(|| "no encontrado".to_string());

        let text = format!(
            "Veeet · modo {}  (fps {:.0})\naudio: {}\nobjetivo: {}\nrms {:.3} · pico {:.3} · bass {:.2} · beat {} ({:.2}) · silent {}\nseguir Spotify {} · visible {} · ventana {:?}\nF11 pantalla completa · Esc salir · Ctrl+Alt+P ajustes · Ctrl+Alt+D debug · Ctrl+Alt+Q salir",
            self.config.mode.label(),
            self.fps,
            self.audio_state.label(),
            spotify,
            self.analysis.rms,
            self.analysis.peak,
            self.analysis.bass,
            self.analysis.beat,
            self.analysis.beat_env,
            self.analysis.silent,
            if self.config.window.follow_spotify {
                "sí"
            } else {
                "no"
            },
            if self.user_visible { "sí" } else { "no" },
            self.hwnd.map(|h| h.0 as isize),
        );

        let font = egui::FontId::monospace(12.5);
        let galley = ui
            .painter()
            .layout_no_wrap(text, font, egui::Color32::WHITE);
        let pad = 10.0;
        let pos = rect.left_top() + egui::vec2(14.0, 14.0);
        let bg = egui::Rect::from_min_size(pos, galley.size() + egui::vec2(pad * 2.0, pad * 2.0));
        ui.painter()
            .rect_filled(bg, 8.0, egui::Color32::from_black_alpha(185));
        ui.painter()
            .galley(pos + egui::vec2(pad, pad), galley, egui::Color32::WHITE);
    }
}

impl eframe::App for VeeetApp {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 1.0]
    }

    fn logic(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        self.tick(ctx, frame);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let visible = window_content_rect(&ctx, frame);
        ui.painter().rect_filled(visible, 0.0, egui::Color32::BLACK);
        ui.painter()
            .add(eframe::egui_wgpu::Callback::new_paint_callback(
                visible,
                render::WaveCallback,
            ));

        let now = Instant::now();
        let show_handle = !self.show_menu
            && !self.config.mode.is_advanced()
            && (self.config.mode == WindowMode::Windowed
                || (self.config.mode == WindowMode::Fullscreen && self.ui_visible(now)));
        // Soltar archivos de preset sobre la ventana los importa.
        let dropped: Vec<std::path::PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        if !dropped.is_empty() {
            self.import_files(&dropped);
            self.show_menu = true;
            self.drawer_open = true;
            self.last_ui_interaction = now;
        }
        if show_handle && dock::draw_handle(ui, visible, self.config.visual.preset).clicked() {
            self.show_menu = true;
            self.last_ui_interaction = now;
        }
        if self.debug {
            self.draw_debug(ui, visible);
        }
        if self.show_menu {
            self.draw_menu(ui, visible);
        }
    }

    fn on_exit(&mut self) {
        self.save_config();
    }
}

fn window_content_rect(ctx: &egui::Context, frame: &eframe::Frame) -> egui::Rect {
    if let Some(window) = frame.winit_window() {
        let ppp = ctx.pixels_per_point() as f64;
        let size = window.inner_size();
        if size.width > 0 && size.height > 0 {
            let logical = size.to_logical::<f32>(ppp);
            return egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(logical.width, logical.height),
            );
        }
    }
    ctx.content_rect()
}

/// Variable de entorno `VEEET_<name>`; acepta también el prefijo anterior
/// `AURORA_<name>`.
pub fn env_var(name: &str) -> Option<String> {
    std::env::var(format!("VEEET_{name}"))
        .or_else(|_| std::env::var(format!("AURORA_{name}")))
        .ok()
}

fn apply_env_overrides(config: &mut Config) {
    if let Some(value) = env_var("TARGET") {
        let value = value.trim();
        if !value.is_empty() {
            config.window.target_process = value.to_string();
        }
    }
    if let Some(value) = env_var("AUDIO_PROCESS") {
        let value = value.trim();
        if !value.is_empty() {
            config.audio.process = value.to_string();
            config.audio.mode = CaptureMode::App;
        }
    }
    if let Some(value) = env_var("AUDIO_MODE") {
        match value.trim().to_ascii_lowercase().as_str() {
            "auto" => config.audio.mode = CaptureMode::Auto,
            "app" | "process" => config.audio.mode = CaptureMode::App,
            "device" | "system" | "sistema" => config.audio.mode = CaptureMode::Device,
            other => log::warn!("VEEET_AUDIO_MODE desconocido: {other}"),
        }
    }
    if let Some(value) = env_var("MODE") {
        config.mode = match value.trim().to_ascii_lowercase().as_str() {
            "overlay" => WindowMode::Overlay,
            "docked" => WindowMode::Docked,
            "fullscreen" | "full" => WindowMode::Fullscreen,
            _ => WindowMode::Windowed,
        };
    }
}

/// `VEEET_PRESET`: id o nombre de un preset, o el nombre de un motor.
fn apply_env_preset(presets: &mut PresetStore, config: &mut Config, value: &str) {
    let value = value.trim();
    let id = presets
        .find(value)
        .or_else(|| {
            presets
                .entries()
                .iter()
                .position(|e| e.preset.name.eq_ignore_ascii_case(value))
        })
        .map(|i| presets.entries()[i].preset.id.clone())
        .or_else(|| VisualPreset::from_alias(value).map(builtin_id))
        .unwrap_or_else(|| builtin_id(VisualPreset::WavyForm3));
    if let Some(params) = presets.select(&id) {
        config.visual = params;
        config.active_preset = id;
    }
}
