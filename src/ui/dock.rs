//! Dock inferior y cajón de ajustes.
//!
//! Tres estados: un *handle* discreto (menú cerrado), el dock con presets y
//! dos ajustes rápidos, y el cajón por pestañas que sube desde el dock.

use eframe::egui::{
    self, Align, Color32, Id, Label, Layout, Rect, Response, RichText, Sense, Stroke, StrokeKind,
    Ui, UiBuilder, pos2, vec2,
};

use super::theme::{
    EDGE_HOVER, EDGE_REST, INK, INK_FAINT, INK_MUTED, INK_SOFT, OBSIDIAN, SLATE, apply_style,
    hairline, mono, preset_accent, with_alpha,
};
use super::widgets::{
    Glyph, accent_button, divider, draw_preset_icon, ghost_button, group_label, icon_button,
    paint_glyph, preset_tile, preset_word, quick_slider, set_describe_engine, shuffle_tile, slider,
    slider_fmt, slider_u32, tab, toggle,
};
use super::{MenuAction, MenuTab, TabGroup};
use crate::audio::AudioApp;
use crate::config::{
    CaptureMode, ColorMode, Config, VisualCfg, VisualPreset, WindowMode, range as r, shuffle_range,
};
use crate::preset::{Entry, Origin, PresetStore, builtin_id};
use crate::update::{CURRENT_VERSION, UpdateState};

const DOCK_HEIGHT: f32 = 64.0;
const DOCK_MAX_WIDTH: f32 = 760.0;
const MARGIN: f32 = 16.0;
const DRAWER_GAP: f32 = 8.0;
const DRAWER_MAX_HEIGHT: f32 = 380.0;
/// Ancho mínimo del contenido del cajón para usar dos columnas.
const TWO_COLUMN_MIN: f32 = 520.0;

/// Lo que el dock muestra del estado de la app, sin acceso a ella.
pub struct Status<'a> {
    pub source: &'a str,
    pub audio: &'a str,
    /// Apps con sesión de audio, para elegir la fuente.
    pub apps: &'a [AudioApp],
    pub fps: f32,
    /// Hay algo que deshacer.
    pub can_undo: bool,
    /// Aviso breve para el pie del cajón (importaciones, errores).
    pub notice: Option<&'a str>,
    pub update: &'a UpdateState,
}

fn dock_rect(content: Rect) -> Rect {
    let width = (content.width() - 2.0 * MARGIN).clamp(240.0, DOCK_MAX_WIDTH);
    Rect::from_min_size(
        pos2(
            content.center().x - width * 0.5,
            content.bottom() - MARGIN - DOCK_HEIGHT,
        ),
        vec2(width, DOCK_HEIGHT),
    )
}

/// Píldora abajo-centro que abre el dock.
pub fn draw_handle(ui: &mut Ui, content: Rect, preset: VisualPreset) -> Response {
    let rect = Rect::from_center_size(
        pos2(content.center().x, content.bottom() - MARGIN - 14.0),
        vec2(52.0, 28.0),
    );
    let response = ui.interact(rect, Id::unique("veeet-dock-handle"), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(rect, 14.0, with_alpha(OBSIDIAN, 225));
    painter.rect_stroke(
        rect,
        14.0,
        Stroke::new(1.0, hairline(if hovered { EDGE_HOVER } else { EDGE_REST })),
        StrokeKind::Inside,
    );
    paint_glyph(
        painter,
        rect.center() - vec2(5.0, 0.0),
        Glyph::Gear,
        if hovered { INK } else { INK_SOFT },
    );
    // Punto de señal con el acento del preset activo.
    painter.circle_filled(rect.center() + vec2(12.0, 0.0), 2.5, preset_accent(preset));
    response.on_hover_text("Ajustes")
}

/// Dibuja el dock (y el cajón si está abierto). Muta `config` en sitio y
/// devuelve los efectos que debe ejecutar la app.
pub fn draw_dock(
    ui: &mut Ui,
    content: Rect,
    drawer_open: &mut bool,
    current_tab: &mut MenuTab,
    config: &mut Config,
    presets: &mut PresetStore,
    status: &Status<'_>,
) -> Vec<MenuAction> {
    let mut actions = Vec::new();
    let accent = preset_accent(config.visual.preset);
    let editable = presets.active().params_editable();
    let dock = dock_rect(content);

    let t = ui
        .ctx()
        .animate_bool_with_time(Id::unique("veeet-drawer"), *drawer_open, 0.18);
    if t > 0.001 {
        draw_drawer(
            ui,
            content,
            dock,
            t,
            *drawer_open,
            current_tab,
            config,
            presets,
            status,
            accent,
            &mut actions,
        );
    }

    let painter = ui.painter();
    // Sombra única y suave bajo el dock: lo separa del visualizador.
    painter.rect_filled(
        dock.expand(6.0).translate(vec2(0.0, 3.0)),
        24.0,
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(dock, 18.0, with_alpha(OBSIDIAN, 242));
    painter.rect_stroke(
        dock,
        18.0,
        Stroke::new(1.0, hairline(14)),
        StrokeKind::Inside,
    );

    let inner = dock.shrink2(vec2(12.0, 12.0));
    ui.scope_builder(
        UiBuilder::new()
            .max_rect(inner)
            .layout(Layout::left_to_right(Align::Center)),
        |ui| {
            apply_style(ui, accent);
            ui.spacing_mut().item_spacing = vec2(6.0, 0.0);

            preset_menu(
                ui,
                config.visual.preset,
                presets,
                &mut config.shuffle.enabled,
                accent,
                &mut actions,
            );

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.spacing_mut().item_spacing = vec2(4.0, 0.0);
                if icon_button(ui, Glyph::Close, accent, false)
                    .on_hover_text("Cerrar")
                    .clicked()
                {
                    actions.push(MenuAction::Close);
                }
                let (glyph, hint) = if *drawer_open {
                    (Glyph::ChevronDown, "Ocultar ajustes")
                } else {
                    (Glyph::ChevronUp, "Más ajustes")
                };
                if icon_button(ui, glyph, accent, *drawer_open)
                    .on_hover_text(hint)
                    .clicked()
                {
                    *drawer_open = !*drawer_open;
                }
                ui.add_enabled_ui(status.can_undo, |ui| {
                    if icon_button(ui, Glyph::Undo, accent, false)
                        .on_hover_text("Deshacer")
                        .on_disabled_hover_text("Nada que deshacer")
                        .clicked()
                    {
                        actions.push(MenuAction::Undo);
                    }
                });
                ui.add_space(8.0);
                divider(ui);
                ui.add_space(12.0);

                // Ajustes rápidos: hasta cuatro, los que quepan (≥ 120 px cada uno).
                const GAP: f32 = 18.0;
                let avail = ui.available_width() - 12.0;
                let count = (((avail + GAP) / (120.0 + GAP)) as usize).min(QUICK_CONTROLS);
                if count > 0 {
                    ui.spacing_mut().item_spacing.x = GAP;
                    let width = ((avail - GAP * (count - 1) as f32) / count as f32).min(170.0);
                    // Los importados son de solo lectura.
                    ui.add_enabled_ui(editable, |ui| {
                        // El layout va de derecha a izquierda: el primero queda
                        // junto a los botones.
                        for k in 0..count {
                            let (label, value, range) = quick_control(&mut config.visual, k);
                            quick_slider(ui, label, value, range, accent, width);
                        }
                    });
                }
            });
        },
    );

    actions
}

/// Cantidad máxima de ajustes rápidos en el dock.
const QUICK_CONTROLS: usize = 4;

/// Ajuste rápido `k` del dock: 0 = el que más define el preset, 1 = glow,
/// 2 = uno secundario del preset, 3 = exposición.
fn quick_control(
    visual: &mut VisualCfg,
    k: usize,
) -> (&'static str, &mut f32, std::ops::RangeInclusive<f32>) {
    match k {
        0 => primary_control(visual),
        1 => ("BRILLO", glow_of(visual), r::GLOW),
        2 => secondary_control(visual),
        _ => ("EXPOSICIÓN", &mut visual.exposure, r::EXPOSURE),
    }
}

/// Segundo ajuste de cada preset para el dock.
fn secondary_control(
    visual: &mut VisualCfg,
) -> (&'static str, &mut f32, std::ops::RangeInclusive<f32>) {
    match visual.preset {
        VisualPreset::WavyForm3 => ("SUAVIZADO", &mut visual.smoothing, r::UNIT),
        VisualPreset::WaveRing => ("RADIO", &mut visual.ring_radius, r::RING_RADIUS),
        VisualPreset::Alchemy => ("VELOCIDAD", &mut visual.alchemy_speed, r::ALCHEMY_SPEED),
        VisualPreset::Ambience => ("ESCALA", &mut visual.ambience_scale, r::AMBIENCE_SCALE),
        VisualPreset::Scope => ("SENSIBILIDAD", &mut visual.sensitivity, r::SENSITIVITY),
        VisualPreset::Windmill => ("ANCHO", &mut visual.windmill_width, r::WINDMILL_WIDTH),
        VisualPreset::Niagara => ("ESTELA", &mut visual.fx_trail, r::TRAIL),
        VisualPreset::XMarks => ("FLUJO", &mut visual.xmarks_speed, r::XMARKS_SPEED),
        VisualPreset::Particles => ("EXPLOSIÓN", &mut visual.particles_burst, r::PARTICLES_BURST),
    }
}

/// Botón del preset activo (solo icono) que abre, encima del dock, la
/// rejilla de presets de fábrica y la lista de presets propios e importados.
fn preset_menu(
    ui: &mut Ui,
    engine: VisualPreset,
    presets: &PresetStore,
    shuffle: &mut bool,
    accent: Color32,
    actions: &mut Vec<MenuAction>,
) {
    let menu_id = Id::unique("veeet-preset-menu");
    let ctx = ui.ctx().clone();
    let mut open = ctx.data(|d| d.get_temp::<bool>(menu_id)).unwrap_or(false);
    let mut active_name = presets.active().preset.name.clone();
    if *shuffle {
        active_name.push_str(" · aleatorio");
    }
    let button = preset_tile(ui, engine, true, accent).on_hover_text(active_name);
    // Mismo icono que "Más ajustes", junto al preset activo.
    let (glyph, hint) = if open {
        (Glyph::ChevronDown, "Ocultar escenas")
    } else {
        (Glyph::ChevronUp, "Escenas")
    };
    let toggle_button = icon_button(ui, glyph, accent, open).on_hover_text(hint);
    let toggled = button.clicked() || toggle_button.clicked();
    if toggled {
        open = !open;
    }

    if open {
        const COLS: usize = 4;
        const TILE: f32 = 40.0;
        const GAP: f32 = 6.0;
        const PAD: f32 = 10.0;
        let grid_width = COLS as f32 * TILE + (COLS - 1) as f32 * GAP;
        let width = grid_width.max(220.0);
        let active_id = presets.active_id().to_string();
        let area = egui::Area::new(menu_id.with("area"))
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::LEFT_BOTTOM)
            .fixed_pos(button.rect.left_top() - vec2(PAD, 12.0))
            .show(&ctx, |ui| {
                egui::Frame::new()
                    .fill(with_alpha(SLATE, 248))
                    .stroke(Stroke::new(1.0, hairline(16)))
                    .corner_radius(14.0)
                    .inner_margin(PAD)
                    .show(ui, |ui| {
                        ui.set_width(width);
                        ui.spacing_mut().item_spacing = vec2(GAP, GAP);
                        // «Aleatorio» es una escena más: la última casilla.
                        // Elegir cualquier otra escena lo apaga.
                        let on = *shuffle;
                        let tiles: Vec<Option<VisualPreset>> = VisualPreset::all()
                            .into_iter()
                            .map(Some)
                            .chain([None])
                            .collect();
                        for row in tiles.chunks(COLS) {
                            ui.horizontal(|ui| {
                                for &item in row {
                                    match item {
                                        Some(preset) => {
                                            let id = builtin_id(preset);
                                            let selected = !on && active_id == id;
                                            if preset_tile(
                                                ui,
                                                preset,
                                                selected,
                                                preset_accent(preset),
                                            )
                                            .on_hover_text(preset_word(preset))
                                            .clicked()
                                            {
                                                if active_id != id {
                                                    actions.push(MenuAction::SelectPreset(id));
                                                }
                                                *shuffle = false;
                                                open = false;
                                            }
                                        }
                                        None => {
                                            if shuffle_tile(ui, on, accent)
                                                .on_hover_text(
                                                    "Aleatorio: va cambiando de escena solo \
                                                     (elige cuáles en General · Aleatorio)",
                                                )
                                                .clicked()
                                            {
                                                if on {
                                                    actions.push(MenuAction::ShuffleNext);
                                                } else {
                                                    *shuffle = true;
                                                    // Arranca ya con otra escena.
                                                    actions.push(MenuAction::ShuffleNext);
                                                }
                                                open = false;
                                            }
                                        }
                                    }
                                }
                            });
                        }
                        if on {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("Aleatorio activo")
                                        .font(super::theme::body(11.0))
                                        .color(accent),
                                );
                                if ghost_button(ui, "Siguiente")
                                    .on_hover_text("Pasar ya a otra escena")
                                    .clicked()
                                {
                                    actions.push(MenuAction::ShuffleNext);
                                }
                            });
                        }
                        let custom: Vec<&Entry> = presets
                            .entries()
                            .iter()
                            .filter(|e| e.origin != Origin::Builtin)
                            .collect();
                        if !custom.is_empty() {
                            ui.add_space(4.0);
                            group_label(ui, "MIS PRESETS E IMPORTADOS", accent);
                            egui::ScrollArea::vertical()
                                .max_height(180.0)
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    for entry in custom {
                                        let selected = entry.preset.id == active_id;
                                        if preset_row(ui, entry, selected).clicked() {
                                            if !selected {
                                                actions.push(MenuAction::SelectPreset(
                                                    entry.preset.id.clone(),
                                                ));
                                            }
                                            *shuffle = false;
                                            open = false;
                                        }
                                    }
                                });
                        }
                    });
            });
        if area.response.clicked_elsewhere() && !toggled {
            open = false;
        }
    }
    ctx.data_mut(|d| d.insert_temp(menu_id, open));
}

/// Fila compacta de un preset: icono del motor, nombre y procedencia.
fn preset_row(ui: &mut Ui, entry: &Entry, selected: bool) -> Response {
    let engine = entry.preset.engine();
    let accent = preset_accent(engine);
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    let painter = ui.painter_at(rect);
    let hovered = response.hovered();
    if selected || hovered {
        painter.rect_filled(
            rect,
            8.0,
            if selected {
                with_alpha(accent, 30)
            } else {
                hairline(8)
            },
        );
    }
    let icon = Rect::from_center_size(pos2(rect.left() + 14.0, rect.center().y), vec2(14.0, 14.0));
    draw_preset_icon(
        &painter,
        icon,
        engine,
        if selected { accent } else { INK_MUTED },
    );
    let right = match entry.origin {
        Origin::Builtin => "fábrica",
        Origin::User => "tuyo",
        Origin::Market => "importado",
    };
    let tag = painter.layout_no_wrap(right.to_string(), mono(9.0), INK_FAINT);
    let tag_w = tag.size().x;
    painter.galley(
        pos2(
            rect.right() - 8.0 - tag_w,
            rect.center().y - tag.size().y * 0.5,
        ),
        tag,
        INK_FAINT,
    );
    let mut job = egui::text::LayoutJob::single_section(
        entry.preset.name.clone(),
        egui::TextFormat::simple(
            egui::FontId::proportional(11.5),
            if selected { INK } else { INK_SOFT },
        ),
    );
    job.wrap = egui::text::TextWrapping::truncate_at_width((rect.width() - 46.0 - tag_w).max(10.0));
    let galley = ui.fonts_mut(|f| f.layout_job(job));
    painter.galley(
        pos2(rect.left() + 28.0, rect.center().y - galley.size().y * 0.5),
        galley,
        INK,
    );
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.on_hover_text(format!("{} · {}", entry.preset.name, preset_word(engine)))
}

/// El ajuste que más define cada preset, para el dock.
fn primary_control(
    visual: &mut VisualCfg,
) -> (&'static str, &mut f32, std::ops::RangeInclusive<f32>) {
    match visual.preset {
        VisualPreset::WavyForm3 => ("SENSIBILIDAD", &mut visual.sensitivity, r::SENSITIVITY),
        VisualPreset::WaveRing => ("ESTELA", &mut visual.ring_trail, r::TRAIL),
        VisualPreset::Alchemy => ("ESTELA", &mut visual.alchemy_trail, r::TRAIL),
        VisualPreset::Ambience => ("VELOCIDAD", &mut visual.ambience_speed, r::AMBIENCE_SPEED),
        VisualPreset::Scope | VisualPreset::Particles => ("ESTELA", &mut visual.fx_trail, r::TRAIL),
        VisualPreset::Niagara => ("VELOCIDAD", &mut visual.niagara_speed, r::NIAGARA_SPEED),
        VisualPreset::Windmill => ("GIRO", &mut visual.windmill_speed, r::WINDMILL_SPEED),
        VisualPreset::XMarks => ("GIRO", &mut visual.xmarks_spin, r::XMARKS_SPIN),
    }
}

/// Glow por preset: el feedback y los campos de color acumulan luz, así que
/// no comparten el glow alto de la onda.
fn glow_of(visual: &mut VisualCfg) -> &mut f32 {
    match visual.preset {
        VisualPreset::WavyForm3 => &mut visual.glow,
        VisualPreset::Alchemy => &mut visual.alchemy_glow,
        VisualPreset::Ambience => &mut visual.ambience_glow,
        VisualPreset::WaveRing
        | VisualPreset::Scope
        | VisualPreset::Niagara
        | VisualPreset::Windmill
        | VisualPreset::XMarks
        | VisualPreset::Particles => &mut visual.fx_glow,
    }
}

/// Contexto mutable que comparten las columnas de una pestaña.
struct TabCtx<'a> {
    config: &'a mut Config,
    presets: &'a mut PresetStore,
    actions: &'a mut Vec<MenuAction>,
    accent: Color32,
    audio: &'a str,
    apps: &'a [AudioApp],
    update: &'a UpdateState,
}

#[allow(clippy::too_many_arguments)]
fn draw_drawer(
    ui: &mut Ui,
    content: Rect,
    dock: Rect,
    t: f32,
    open: bool,
    current_tab: &mut MenuTab,
    config: &mut Config,
    presets: &mut PresetStore,
    status: &Status<'_>,
    accent: Color32,
    actions: &mut Vec<MenuAction>,
) {
    let height = DRAWER_MAX_HEIGHT
        .min(dock.top() - content.top() - DRAWER_GAP - MARGIN)
        .max(120.0);
    let slide = (1.0 - t) * 10.0;
    let bottom = dock.top() - DRAWER_GAP + slide;
    let rect = Rect::from_min_max(
        pos2(dock.left(), bottom - height),
        pos2(dock.right(), bottom),
    );

    ui.scope_builder(
        UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::top_down(Align::Min)),
        |ui| {
            ui.multiply_opacity(t);
            if !open {
                ui.disable();
            }
            let painter = ui.painter();
            painter.rect_filled(rect, 16.0, with_alpha(SLATE, 246));
            painter.rect_stroke(
                rect,
                16.0,
                Stroke::new(1.0, hairline(14)),
                StrokeKind::Inside,
            );
            // Hilo de señal superior con el acento del preset.
            painter.hline(
                (rect.left() + 24.0)..=(rect.right() - 24.0),
                rect.top() + 0.5,
                Stroke::new(1.0, with_alpha(accent, 120)),
            );

            let inner = rect.shrink2(vec2(18.0, 10.0));
            ui.scope_builder(
                UiBuilder::new()
                    .max_rect(inner)
                    .layout(Layout::top_down(Align::Min)),
                |ui| {
                    apply_style(ui, accent);
                    ui.set_width(inner.width());

                    // Dos grupos: lo que guarda esta escena y lo general de la
                    // app. Cada grupo recuerda su última pestaña.
                    let group = current_tab.group();
                    let memory = Id::unique("veeet-tab-memory");
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let scene_label = format!("Escena · {}", presets.active().preset.name);
                        for (target, label, hover) in [
                            (
                                TabGroup::Scene,
                                scene_label.as_str(),
                                "Ajustes que se guardan en esta escena",
                            ),
                            (
                                TabGroup::General,
                                "General",
                                "Ajustes de la app, iguales en todas las escenas",
                            ),
                        ] {
                            let response = if group == target {
                                accent_button(ui, label, accent)
                            } else {
                                ghost_button(ui, label)
                            };
                            if response.on_hover_text(hover).clicked() && group != target {
                                ui.ctx().data_mut(|d| {
                                    d.insert_temp(
                                        memory.with(group == TabGroup::Scene),
                                        *current_tab,
                                    )
                                });
                                *current_tab = ui
                                    .ctx()
                                    .data(|d| {
                                        d.get_temp::<MenuTab>(
                                            memory.with(target == TabGroup::Scene),
                                        )
                                    })
                                    .unwrap_or(MenuTab::tabs(target)[0]);
                            }
                        }
                    });
                    ui.add_space(2.0);
                    egui::ScrollArea::horizontal()
                        .id_salt("veeet-tabs")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.spacing_mut().item_spacing.x = 0.0;
                                for &candidate in MenuTab::tabs(current_tab.group()) {
                                    let label = candidate.label(config.visual.preset);
                                    if tab(ui, label, *current_tab == candidate, accent).clicked() {
                                        *current_tab = candidate;
                                    }
                                }
                            });
                        });
                    let y = ui.cursor().top();
                    ui.painter()
                        .hline(inner.x_range(), y, Stroke::new(1.0, hairline(EDGE_REST)));
                    ui.add_space(12.0);

                    let footer_height = 46.0;
                    let scroll_height = (ui.available_height() - footer_height).max(40.0);
                    egui::ScrollArea::vertical()
                        .max_height(scroll_height)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.spacing_mut().item_spacing = vec2(8.0, 4.0);
                            let wide = ui.available_width() >= TWO_COLUMN_MIN;
                            let editable = presets.active().params_editable();
                            let mut cx = TabCtx {
                                config: &mut *config,
                                presets: &mut *presets,
                                actions: &mut *actions,
                                accent,
                                audio: status.audio,
                                apps: status.apps,
                                update: status.update,
                            };
                            let per_preset = current_tab.group() == TabGroup::Scene;
                            if per_preset {
                                preset_banner(ui, &mut cx);
                            }
                            // Las descripciones de los campos dependen del motor.
                            set_describe_engine(ui.ctx(), cx.config.visual.preset);
                            ui.add_enabled_ui(editable || !per_preset, |ui| match *current_tab {
                                MenuTab::Preset => tab_preset(ui, wide, &mut cx),
                                MenuTab::Light => tab_light(ui, wide, &mut cx),
                                MenuTab::Color => tab_color(ui, wide, &mut cx),
                                MenuTab::Grid => tab_grid(ui, wide, &mut cx),
                                MenuTab::Reaction => tab_reaction(ui, wide, &mut cx),
                                MenuTab::Library => tab_library(ui, wide, &mut cx),
                                MenuTab::Market => tab_market(ui, &mut cx),
                                MenuTab::Shuffle => tab_shuffle(ui, wide, &mut cx),
                                MenuTab::Window => tab_window(ui, wide, &mut cx),
                                MenuTab::Audio => tab_audio(ui, wide, &mut cx),
                            });
                        });

                    let y = ui.cursor().top() + 4.0;
                    ui.painter()
                        .hline(inner.x_range(), y, Stroke::new(1.0, hairline(EDGE_REST)));
                    ui.add_space(12.0);
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ghost_button(ui, "Salir").clicked() {
                            actions.push(MenuAction::Quit);
                        }
                        if ghost_button(ui, "Ocultar").clicked() {
                            actions.push(MenuAction::Hide);
                        }
                        let can_undo = status.can_undo;
                        if ui
                            .add_enabled_ui(can_undo, |ui| ghost_button(ui, "Deshacer"))
                            .inner
                            .on_hover_text("Deshacer el último cambio")
                            .clicked()
                        {
                            actions.push(MenuAction::Undo);
                        }
                        let tab_name = current_tab.label(config.visual.preset);
                        let can_reset = current_tab.resettable()
                            && (presets.active().params_editable()
                                || *current_tab == MenuTab::Window);
                        if current_tab.resettable()
                            && ui
                                .add_enabled_ui(can_reset, |ui| ghost_button(ui, "Restablecer"))
                                .inner
                                .on_hover_text(format!(
                                    "Restablecer solo «{tab_name}» de este preset"
                                ))
                                .clicked()
                        {
                            actions.push(MenuAction::Reset(*current_tab));
                        }
                        if accent_button(ui, "Guardar", accent).clicked() {
                            actions.push(MenuAction::Save);
                        }
                        ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                            ui.add(
                                Label::new(match status.notice {
                                    Some(notice) => {
                                        RichText::new(notice).font(mono(9.5)).color(INK_SOFT)
                                    }
                                    None => RichText::new(format!(
                                        "{} · {} · {:.0} fps · F11",
                                        presets.active().preset.name,
                                        status.source,
                                        status.fps
                                    ))
                                    .font(mono(9.5))
                                    .color(INK_FAINT),
                                })
                                .truncate(),
                            );
                        });
                    });
                },
            );
        },
    );
}

/// Reparte dos grupos de controles en columnas si hay ancho, o uno debajo
/// del otro si no.
fn two_col<T>(
    ui: &mut Ui,
    wide: bool,
    data: &mut T,
    left: impl FnOnce(&mut Ui, &mut T),
    right: impl FnOnce(&mut Ui, &mut T),
) {
    if wide {
        ui.spacing_mut().item_spacing.x = 28.0;
        ui.columns(2, |cols| {
            left(&mut cols[0], data);
            right(&mut cols[1], data);
        });
    } else {
        left(ui, data);
        ui.add_space(6.0);
        right(ui, data);
    }
}

fn fmt_3_dec(v: f64) -> String {
    format!("{v:.3}")
}

/// Slider con tres decimales, para ajustes finos.
fn slider3(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    accent: Color32,
) {
    slider_fmt(ui, label, value, range, accent, fmt_3_dec);
}

/// Ajustes propios del motor del preset activo: solo los que ese motor usa.
fn tab_preset(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    match cx.config.visual.preset {
        VisualPreset::WavyForm3 => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "FORMA", a);
                slider(ui, "sensibilidad", &mut v.sensitivity, r::SENSITIVITY, a);
                slider(ui, "ancho", &mut v.wave_width, r::WAVE_WIDTH, a);
                slider(ui, "grosor", &mut v.line_width, r::LINE_WIDTH, a);
                slider_u32(ui, "líneas", &mut v.line_count, r::LINE_COUNT, a);
                slider(ui, "distancia de cámara", &mut v.camera_z, r::CAMERA_Z, a);
                hint(ui, "Menos distancia, más zoom. Por defecto: 1.56.");
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "MOVIMIENTO", a);
                slider(ui, "variación", &mut v.line_variation, r::VARIATION, a);
                slider(
                    ui,
                    "ritmo de variación",
                    &mut v.variation_speed,
                    r::VARIATION_SPEED,
                    a,
                );
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
            },
        ),
        VisualPreset::WaveRing => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "FORMA", a);
                slider_u32(ui, "anillos", &mut v.ring_bands, r::RING_BANDS, a);
                slider(ui, "radio", &mut v.ring_radius, r::RING_RADIUS, a);
                slider(ui, "anillo interior", &mut v.ring_inner, r::RING_INNER, a);
                slider(
                    ui,
                    "amplitud de la onda",
                    &mut v.ring_amplitude,
                    r::RING_AMPLITUDE,
                    a,
                );
                slider(ui, "grosor", &mut v.line_width, r::LINE_WIDTH, a);
                slider_u32(ui, "detalle", &mut v.ring_steps, r::RING_STEPS, a);
                hint(ui, "Radio 1 llega al borde; más de 1 sale de la pantalla.");
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "MOVIMIENTO", a);
                slider(ui, "rotación", &mut v.ring_rotation, r::RING_ROTATION, a);
                slider(ui, "estela líquida", &mut v.ring_trail, r::TRAIL, a);
                slider(ui, "flujo de la estela", &mut v.ring_flow, r::RING_FLOW, a);
                slider(ui, "sensibilidad", &mut v.sensitivity, r::SENSITIVITY, a);
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
                hint(ui, "Rotación negativa gira al revés.");
            },
        ),
        VisualPreset::Alchemy => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "FORMA", a);
                slider_u32(
                    ui,
                    "curvas",
                    &mut v.alchemy_complexity,
                    r::ALCHEMY_COMPLEXITY,
                    a,
                );
                slider(ui, "tamaño", &mut v.alchemy_size, r::ALCHEMY_SIZE, a);
                slider_u32(
                    ui,
                    "partículas",
                    &mut v.alchemy_particles,
                    r::ALCHEMY_PARTICLES,
                    a,
                );
                toggle(ui, &mut v.alchemy_star, "Estrella en cada golpe", a);
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "MOVIMIENTO Y ECO", a);
                slider(ui, "velocidad", &mut v.alchemy_speed, r::ALCHEMY_SPEED, a);
                slider(
                    ui,
                    "fuerza del remolino",
                    &mut v.alchemy_motion,
                    r::ALCHEMY_MOTION,
                    a,
                );
                slider(
                    ui,
                    "segundos por modo",
                    &mut v.alchemy_mode_time,
                    r::ALCHEMY_MODE_TIME,
                    a,
                );
                slider(ui, "estela", &mut v.alchemy_trail, r::TRAIL, a);
                toggle(
                    ui,
                    &mut v.alchemy_kaleido,
                    "Sumar caleidoscopio al ciclo",
                    a,
                );
            },
        ),
        VisualPreset::Ambience => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "NUBES", a);
                slider(ui, "escala", &mut v.ambience_scale, r::AMBIENCE_SCALE, a);
                slider(
                    ui,
                    "brillo de las nubes",
                    &mut v.ambience_brightness,
                    r::AMBIENCE_BRIGHTNESS,
                    a,
                );
                slider(
                    ui,
                    "densidad",
                    &mut v.ambience_density,
                    r::AMBIENCE_DENSITY,
                    a,
                );
                slider(ui, "turbulencia", &mut v.ambience_warp, r::AMBIENCE_WARP, a);
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "FLUJO", a);
                slider(ui, "velocidad", &mut v.ambience_speed, r::AMBIENCE_SPEED, a);
                slider(
                    ui,
                    "cambio con el golpe",
                    &mut v.ambience_morph,
                    r::AMBIENCE_MORPH,
                    a,
                );
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
                hint(ui, "El gas cambia de forma en cada golpe.");
            },
        ),
        VisualPreset::Scope => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "FORMA", a);
                slider(
                    ui,
                    "alto de la onda",
                    &mut v.scope_height,
                    r::SCOPE_HEIGHT,
                    a,
                );
                slider(ui, "grosor", &mut v.line_width, r::LINE_WIDTH, a);
                slider(ui, "sensibilidad", &mut v.sensitivity, r::SENSITIVITY, a);
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "ESTELA", a);
                slider(ui, "estela", &mut v.fx_trail, r::TRAIL, a);
                slider3(ui, "expansión", &mut v.scope_expand, r::SCOPE_EXPAND, a);
                slider(ui, "desenfoque", &mut v.scope_blur, r::SCOPE_BLUR, a);
                toggle(ui, &mut v.scope_mirror, "Onda en espejo", a);
            },
        ),
        VisualPreset::XMarks => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "X", a);
                slider(ui, "giro", &mut v.xmarks_spin, r::XMARKS_SPIN, a);
                slider_u32(ui, "brazos", &mut v.xmarks_arms, r::XMARKS_ARMS, a);
                slider3(
                    ui,
                    "amplitud de la onda",
                    &mut v.xmarks_amplitude,
                    r::XMARKS_AMPLITUDE,
                    a,
                );
                slider(ui, "grosor", &mut v.line_width, r::LINE_WIDTH, a);
                slider(ui, "sensibilidad", &mut v.sensitivity, r::SENSITIVITY, a);
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "LÍQUIDO", a);
                slider(ui, "flujo", &mut v.xmarks_speed, r::XMARKS_SPEED, a);
                slider(ui, "ondulación", &mut v.xmarks_curve, r::XMARKS_CURVE, a);
                slider(ui, "pulso", &mut v.xmarks_road, r::XMARKS_ROAD, a);
                slider(ui, "estela", &mut v.fx_trail, r::TRAIL, a);
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
            },
        ),
        VisualPreset::Particles => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "PARTÍCULAS", a);
                slider_u32(
                    ui,
                    "cantidad",
                    &mut v.particles_count,
                    r::PARTICLES_COUNT,
                    a,
                );
                slider(
                    ui,
                    "explosión en el golpe",
                    &mut v.particles_burst,
                    r::PARTICLES_BURST,
                    a,
                );
                slider(ui, "grosor", &mut v.particles_size, r::PARTICLES_SIZE, a);
                slider(ui, "vida", &mut v.particles_life, r::PARTICLES_LIFE, a);
                slider(ui, "estela", &mut v.fx_trail, r::TRAIL, a);
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "FLUJO", a);
                slider(
                    ui,
                    "velocidad",
                    &mut v.particles_speed,
                    r::PARTICLES_SPEED,
                    a,
                );
                slider(
                    ui,
                    "tamaño de remolinos",
                    &mut v.particles_scale,
                    r::PARTICLES_SCALE,
                    a,
                );
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
            },
        ),
        VisualPreset::Niagara => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "CASCADA", a);
                slider(ui, "velocidad", &mut v.niagara_speed, r::NIAGARA_SPEED, a);
                slider(ui, "separación", &mut v.niagara_emit, r::NIAGARA_EMIT, a);
                slider3(
                    ui,
                    "contracción",
                    &mut v.niagara_squeeze,
                    r::NIAGARA_SQUEEZE,
                    a,
                );
                slider(ui, "estela", &mut v.fx_trail, r::TRAIL, a);
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "LÍNEAS", a);
                slider_u32(ui, "escalones", &mut v.niagara_steps, r::NIAGARA_STEPS, a);
                slider(ui, "grosor", &mut v.niagara_line, r::NIAGARA_LINE, a);
                slider(ui, "sensibilidad", &mut v.sensitivity, r::SENSITIVITY, a);
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
                toggle(
                    ui,
                    &mut v.niagara_continuous,
                    "Emitir con el volumen, no solo en golpes",
                    a,
                );
            },
        ),
        VisualPreset::Windmill => two_col(
            ui,
            wide,
            cx,
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "ASPA", a);
                slider(ui, "giro", &mut v.windmill_speed, r::WINDMILL_SPEED, a);
                slider3(
                    ui,
                    "ancho de la franja",
                    &mut v.windmill_width,
                    r::WINDMILL_WIDTH,
                    a,
                );
                slider_u32(
                    ui,
                    "líneas por borde",
                    &mut v.windmill_edge_lines,
                    r::WINDMILL_EDGE_LINES,
                    a,
                );
                slider3(
                    ui,
                    "separación de líneas dobles",
                    &mut v.windmill_edge_gap,
                    r::WINDMILL_EDGE_GAP,
                    a,
                );
                slider(
                    ui,
                    "juntar ondas",
                    &mut v.windmill_join,
                    r::WINDMILL_JOIN,
                    a,
                );
            },
            |ui, cx| {
                let (a, v) = (cx.accent, &mut cx.config.visual);
                group_label(ui, "NUBE", a);
                slider(ui, "estela", &mut v.fx_trail, r::TRAIL, a);
                slider(ui, "difusión", &mut v.windmill_blur, r::WINDMILL_BLUR, a);
                slider3(
                    ui,
                    "amplitud de la onda",
                    &mut v.windmill_amplitude,
                    r::WINDMILL_AMPLITUDE,
                    a,
                );
                slider(ui, "sensibilidad", &mut v.sensitivity, r::SENSITIVITY, a);
                slider(ui, "suavizado", &mut v.smoothing, r::UNIT, a);
            },
        ),
    }
}

fn tab_light(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let a = cx.accent;
            group_label(ui, "RESPLANDOR", a);
            slider(ui, "brillo", glow_of(&mut cx.config.visual), r::GLOW, a);
            let v = &mut cx.config.visual;
            slider(ui, "umbral", &mut v.bloom_threshold, r::UNIT, a);
            slider(ui, "radio del resplandor", &mut v.bloom_radius, r::UNIT, a);
            hint(ui, "Umbral bajo: brilla todo. Alto: solo lo más intenso.");
        },
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "SALIDA", a);
            slider(ui, "exposición", &mut v.exposure, r::EXPOSURE, a);
            slider(ui, "contraste", &mut v.contrast, r::CONTRAST, a);
            slider(ui, "viñeta", &mut v.vignette, r::UNIT, a);
        },
    );
}

fn tab_color(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "COLORES DEL PRESET", a);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (mode, label) in [
                    (ColorMode::Original, "Originales"),
                    (ColorMode::Custom, "Personalizados"),
                ] {
                    let response = if v.color_mode == mode {
                        accent_button(ui, label, a)
                    } else {
                        ghost_button(ui, label)
                    };
                    if response.clicked() {
                        v.color_mode = mode;
                    }
                }
            });
            ui.add_space(4.0);
            ui.add_enabled_ui(v.color_mode == ColorMode::Custom, |ui| {
                color_row(ui, "color 1", &mut v.color_a);
                color_row(ui, "color 2", &mut v.color_b);
                color_row(ui, "color 3", &mut v.color_c);
            });
            ui.add_space(4.0);
            slider(ui, "ciclo de color", &mut v.color_cycle, r::COLOR_CYCLE, a);
            ui.add_space(6.0);
            group_label(ui, "AJUSTE DE IMAGEN", a);
            slider(ui, "tono", &mut v.hue_shift, r::UNIT, a);
            slider(ui, "saturación", &mut v.saturation, r::SATURATION, a);
            color_row(ui, "tinte", &mut v.tint);
            ui.add_space(4.0);
            slider(ui, "fuerza del tinte", &mut v.tint_amount, r::UNIT, a);
            color_row(ui, "fondo", &mut v.background);
        },
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "ENCUADRE", a);
            slider(ui, "tamaño", &mut v.scene_scale, r::SCENE_SCALE, a);
            slider(ui, "posición horizontal", &mut v.offset_x, r::OFFSET, a);
            slider(ui, "posición vertical", &mut v.offset_y, r::OFFSET, a);
            ui.add_space(6.0);
            group_label(ui, "ESPEJO", a);
            toggle(ui, &mut v.mirror_x, "Reflejar izquierda a derecha", a);
            toggle(ui, &mut v.mirror_y, "Reflejar arriba a abajo", a);
            ui.add_space(8.0);
            group_label(ui, "TIEMPO", a);
            slider(
                ui,
                "velocidad de la escena",
                &mut v.time_scale,
                r::TIME_SCALE,
                a,
            );
            hint(ui, "0 congela la animación; 2 la acelera al doble.");
        },
    );
}

/// Modo aleatorio: cada cuánto, cómo y entre qué escenas.
fn tab_shuffle(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let a = cx.accent;
            let s = &mut cx.config.shuffle;
            group_label(ui, "MODO ALEATORIO", a);
            toggle(ui, &mut s.enabled, "Ir cambiando de escena solo", a);
            slider_fmt(
                ui,
                "segundos por escena",
                &mut s.interval_secs,
                shuffle_range::INTERVAL,
                a,
                super::widgets::fmt_int,
            );
            toggle(ui, &mut s.wait_for_beat, "Cambiar en el golpe", a);
            slider(ui, "fundido (s)", &mut s.fade_secs, shuffle_range::FADE, a);
            ui.add_space(6.0);
            if ghost_button(ui, "Siguiente escena ahora").clicked() {
                cx.actions.push(MenuAction::ShuffleNext);
            }
            hint(
                ui,
                "Con «cambiar en el golpe» espera al próximo beat (hasta 4 s).",
            );
        },
        |ui, cx| {
            let a = cx.accent;
            group_label(ui, "ESCENAS", a);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if ghost_button(ui, "Todas").clicked() {
                    cx.config.shuffle.all = true;
                    cx.config.shuffle.presets.clear();
                }
                if ghost_button(ui, "Ninguna").clicked() {
                    cx.config.shuffle.all = false;
                    cx.config.shuffle.presets.clear();
                }
            });
            ui.add_space(4.0);
            let s = &mut cx.config.shuffle;
            toggle(ui, &mut s.all, "Todas, también las que crees después", a);
            ui.add_space(4.0);
            for entry in cx.presets.entries() {
                let id = &entry.preset.id;
                let mut on = s.all || s.presets.iter().any(|p| p == id);
                let label = format!(
                    "{} · {}",
                    entry.preset.name,
                    preset_word(entry.preset.engine())
                );
                if toggle(ui, &mut on, &label, a).changed() {
                    if s.all {
                        // Pasar de "todas" a una lista: todas menos esta.
                        s.all = false;
                        s.presets = cx
                            .presets
                            .entries()
                            .iter()
                            .map(|e| e.preset.id.clone())
                            .filter(|p| p != id)
                            .collect();
                    } else if on {
                        s.presets.push(id.clone());
                    } else {
                        s.presets.retain(|p| p != id);
                    }
                }
            }
        },
    );
}

/// Etiqueta a la izquierda y selector de color a la derecha.
fn color_row(ui: &mut Ui, label: &str, color: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(label)
                .font(super::theme::body(11.0))
                .color(INK_SOFT),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            egui::color_picker::color_edit_button_srgb(ui, color);
        });
    });
}

fn hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).font(mono(9.0)).color(INK_FAINT));
}

/// Aviso sobre de quién es el preset activo y si se puede editar.
fn preset_banner(ui: &mut Ui, cx: &mut TabCtx<'_>) {
    let entry = cx.presets.active();
    if entry.origin != Origin::Market {
        return;
    }
    let id = entry.preset.id.clone();
    ui.horizontal(|ui| {
        hint(ui, "Preset importado: es de solo lectura.");
        if ghost_button(ui, "Duplicar para editar").clicked() {
            cx.actions.push(MenuAction::DuplicatePreset(id));
        }
    });
    ui.add_space(6.0);
}

/// Botón de borrar con confirmación en dos clics.
fn delete_button(ui: &mut Ui, id: &str, actions: &mut Vec<MenuAction>) {
    let key = Id::unique("veeet-confirm-delete");
    let pending = ui.data(|d| d.get_temp::<String>(key));
    let armed = pending.as_deref() == Some(id);
    let label = if armed {
        "¿Borrar? Clic otra vez"
    } else {
        "Borrar"
    };
    if ghost_button(ui, label).clicked() {
        if armed {
            ui.data_mut(|d| d.remove::<String>(key));
            actions.push(MenuAction::DeletePreset(id.to_string()));
        } else {
            ui.data_mut(|d| d.insert_temp(key, id.to_string()));
        }
    }
}

fn origin_label(origin: Origin) -> &'static str {
    match origin {
        Origin::Builtin => "DE FÁBRICA",
        Origin::User => "TUYO",
        Origin::Market => "IMPORTADO",
    }
}

fn text_field(ui: &mut Ui, label: &str, value: &mut String, hint_text: &str) -> bool {
    ui.label(
        RichText::new(label)
            .font(super::theme::body(11.0))
            .color(INK_SOFT),
    );
    let changed = ui
        .add(
            egui::TextEdit::singleline(value)
                .hint_text(hint_text)
                .desired_width(f32::INFINITY),
        )
        .changed();
    ui.add_space(4.0);
    changed
}

fn tab_library(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let a = cx.accent;
            let index = cx.presets.active_index();
            let origin = cx.presets.active().origin;
            let id = cx.presets.active_id().to_string();
            group_label(ui, &format!("PRESET ACTIVO · {}", origin_label(origin)), a);
            if let Some(meta) = cx.presets.active_meta_mut() {
                let mut changed = false;
                changed |= text_field(ui, "nombre", &mut meta.name, "Nombre del preset");
                changed |= text_field(ui, "autor", &mut meta.author, "Tu nombre");
                let tags_key = Id::unique(("veeet-tags", meta.id.clone()));
                let mut tags = ui
                    .data(|d| d.get_temp::<String>(tags_key))
                    .unwrap_or_else(|| meta.tags.join(", "));
                if text_field(ui, "etiquetas", &mut tags, "chill, neón, techno") {
                    meta.tags = tags
                        .split(',')
                        .map(|t| t.trim().to_lowercase())
                        .filter(|t| !t.is_empty())
                        .take(8)
                        .collect();
                    changed = true;
                }
                ui.data_mut(|d| d.insert_temp(tags_key, tags));
                ui.label(
                    RichText::new("descripción")
                        .font(super::theme::body(11.0))
                        .color(INK_SOFT),
                );
                changed |= ui
                    .add(
                        egui::TextEdit::multiline(&mut meta.description)
                            .desired_rows(2)
                            .desired_width(f32::INFINITY),
                    )
                    .changed();
                if changed {
                    cx.presets.mark_dirty();
                }
            } else {
                let p = &cx.presets.active().preset;
                ui.label(
                    RichText::new(&p.name)
                        .font(super::theme::display(14.0))
                        .color(INK),
                );
                if !p.author.is_empty() {
                    hint(ui, &format!("por {} · v{}", p.author, p.version));
                }
                if !p.description.is_empty() {
                    ui.label(RichText::new(&p.description).color(INK_SOFT));
                }
                hint(
                    ui,
                    "Duplica este preset para ponerle nombre y guardarlo como tuyo.",
                );
            }
            ui.add_space(8.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                if ghost_button(ui, "Duplicar").clicked() {
                    cx.actions.push(MenuAction::DuplicatePreset(id.clone()));
                }
                if ghost_button(ui, "Exportar…")
                    .on_hover_text("Guardar como archivo .veeet.toml para compartirlo")
                    .clicked()
                {
                    cx.actions.push(MenuAction::ExportPreset(id.clone()));
                }
                if cx.presets.entries()[index].removable() {
                    delete_button(ui, &id, cx.actions);
                }
            });
        },
        |ui, cx| {
            let a = cx.accent;
            group_label(ui, "NUEVO PRESET", a);
            hint(ui, "Elige el motor; empieza con sus valores de fábrica.");
            ui.add_space(4.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                for engine in VisualPreset::all() {
                    if preset_tile(ui, engine, false, preset_accent(engine))
                        .on_hover_text(format!("Nuevo preset de {}", preset_word(engine)))
                        .clicked()
                    {
                        cx.actions.push(MenuAction::NewPreset(engine));
                    }
                }
            });
            ui.add_space(10.0);
            group_label(ui, "MIS PRESETS", a);
            let active_id = cx.presets.active_id().to_string();
            let mut any = false;
            for entry in cx
                .presets
                .entries()
                .iter()
                .filter(|e| e.origin == Origin::User)
            {
                any = true;
                let selected = entry.preset.id == active_id;
                if preset_row(ui, entry, selected).clicked() && !selected {
                    cx.actions
                        .push(MenuAction::SelectPreset(entry.preset.id.clone()));
                }
            }
            if !any {
                hint(ui, "Todavía no creaste ninguno.");
            }
        },
    );
}

fn tab_market(ui: &mut Ui, cx: &mut TabCtx<'_>) {
    let a = cx.accent;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
        if accent_button(ui, "Importar…", a)
            .on_hover_text("Instalar archivos .veeet.toml")
            .clicked()
        {
            cx.actions.push(MenuAction::ImportPresets);
        }
        if ghost_button(ui, "Abrir carpeta").clicked() {
            cx.actions.push(MenuAction::OpenMarketFolder);
        }
        if ghost_button(ui, "Recargar")
            .on_hover_text("Volver a leer la carpeta de la galería")
            .clicked()
        {
            cx.actions.push(MenuAction::RescanMarket);
        }
    });
    hint(
        ui,
        "Comparte tus presets con «Exportar…» en Mis presets. También puedes arrastrar archivos a la ventana.",
    );
    ui.add_space(6.0);

    let search_key = Id::unique("veeet-market-search");
    let filter_key = Id::unique("veeet-market-engine");
    let mut search = ui
        .data(|d| d.get_temp::<String>(search_key))
        .unwrap_or_default();
    let mut filter = ui
        .data(|d| d.get_temp::<Option<VisualPreset>>(filter_key))
        .unwrap_or(None);
    ui.horizontal(|ui| {
        ui.add(
            egui::TextEdit::singleline(&mut search)
                .hint_text("Buscar por nombre, autor o etiqueta")
                .desired_width((ui.available_width() - 130.0).max(80.0)),
        );
        egui::ComboBox::from_id_salt("veeet-market-filter")
            .width(110.0)
            .selected_text(filter.map_or("Todos", preset_word))
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut filter, None, "Todos");
                for engine in VisualPreset::all() {
                    ui.selectable_value(&mut filter, Some(engine), preset_word(engine));
                }
            });
    });
    ui.data_mut(|d| {
        d.insert_temp(search_key, search.clone());
        d.insert_temp(filter_key, filter);
    });
    ui.add_space(6.0);

    let needle = search.trim().to_lowercase();
    let active_id = cx.presets.active_id().to_string();
    let mut shown = 0;
    let mut installed = 0;
    for entry in cx
        .presets
        .entries()
        .iter()
        .filter(|e| e.origin == Origin::Market)
    {
        installed += 1;
        let p = &entry.preset;
        if filter.is_some_and(|f| f != p.engine()) {
            continue;
        }
        if !needle.is_empty() {
            let haystack = format!(
                "{} {} {} {}",
                p.name,
                p.author,
                p.description,
                p.tags.join(" ")
            )
            .to_lowercase();
            if !haystack.contains(&needle) {
                continue;
            }
        }
        shown += 1;
        market_card(ui, entry, entry.preset.id == active_id, a, cx.actions);
    }
    if installed == 0 {
        hint(
            ui,
            "La galería está vacía. Importa un archivo .veeet.toml que te hayan pasado.",
        );
    } else if shown == 0 {
        hint(ui, "Ningún preset coincide con la búsqueda.");
    }
}

fn market_card(
    ui: &mut Ui,
    entry: &Entry,
    active: bool,
    accent: Color32,
    actions: &mut Vec<MenuAction>,
) {
    let p = &entry.preset;
    let engine = p.engine();
    egui::Frame::new()
        .fill(if active {
            with_alpha(accent, 22)
        } else {
            hairline(6)
        })
        .stroke(Stroke::new(
            1.0,
            if active {
                with_alpha(accent, 120)
            } else {
                hairline(EDGE_REST)
            },
        ))
        .corner_radius(10.0)
        .inner_margin(10.0)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(28.0, 28.0), Sense::hover());
                draw_preset_icon(
                    ui.painter(),
                    rect.shrink(4.0),
                    engine,
                    preset_accent(engine),
                );
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(&p.name)
                            .font(super::theme::display(12.5))
                            .color(INK),
                    );
                    let mut meta = preset_word(engine).to_string();
                    if !p.author.is_empty() {
                        meta = format!("{meta} · por {}", p.author);
                    }
                    meta = format!("{meta} · v{}", p.version);
                    if !p.tags.is_empty() {
                        meta = format!("{meta} · #{}", p.tags.join(" #"));
                    }
                    hint(ui, &meta);
                });
            });
            if !p.description.is_empty() {
                ui.add(Label::new(RichText::new(&p.description).color(INK_SOFT)).wrap());
            }
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                if active {
                    ui.add_enabled_ui(false, |ui| accent_button(ui, "En uso", accent));
                } else if accent_button(ui, "Usar", accent).clicked() {
                    actions.push(MenuAction::SelectPreset(p.id.clone()));
                }
                if ghost_button(ui, "Duplicar para editar").clicked() {
                    actions.push(MenuAction::DuplicatePreset(p.id.clone()));
                }
                delete_button(ui, &p.id, actions);
            });
        });
    ui.add_space(6.0);
}

fn tab_grid(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "COPIAS", a);
            toggle(ui, &mut v.grid_tile, "Repetir en mosaico", a);
            ui.add_enabled_ui(v.grid_tile, |ui| {
                slider_u32(ui, "filas", &mut v.grid_tile_rows, r::GRID_ROWS, a);
            });
        },
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "MEZCLA", a);
            ui.add_enabled_ui(v.grid_tile, |ui| {
                slider(ui, "solape", &mut v.grid_tile_overlap, r::UNIT, a);
                slider(ui, "opacidad", &mut v.grid_tile_opacity, r::UNIT, a);
            });
        },
    );
}

fn tab_window(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let a = cx.accent;
            let w = &mut cx.config.window;
            group_label(ui, "COMPORTAMIENTO", a);
            if toggle(ui, &mut w.follow_spotify, "Seguir a Spotify", a).changed() {
                cx.actions.push(MenuAction::FollowChanged);
            }
            if toggle(ui, &mut w.always_on_top, "Siempre visible", a).changed() {
                cx.actions.push(MenuAction::TopmostChanged);
            }
            toggle(
                ui,
                &mut w.ui_auto_hide,
                "Ocultar controles en pantalla completa",
                a,
            );
            ui.add_space(8.0);
            group_label(ui, "ACTUALIZACIONES", a);
            toggle(
                ui,
                &mut cx.config.update.check_on_start,
                "Buscar al abrir",
                a,
            );
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let (text, button) = match cx.update {
                    UpdateState::Checking => ("Buscando…".to_string(), None),
                    UpdateState::Downloading => ("Descargando…".to_string(), None),
                    UpdateState::Available(r) => (
                        format!("Disponible: {}", r.version),
                        Some((
                            format!("Actualizar a {}", r.version),
                            MenuAction::ApplyUpdate,
                        )),
                    ),
                    UpdateState::UpToDate => (
                        format!("Veeet {CURRENT_VERSION} está al día"),
                        Some(("Buscar ahora".to_string(), MenuAction::CheckUpdate)),
                    ),
                    UpdateState::Error(msg) => (
                        msg.clone(),
                        Some(("Reintentar".to_string(), MenuAction::CheckUpdate)),
                    ),
                    UpdateState::Idle => (
                        format!("Veeet {CURRENT_VERSION}"),
                        Some(("Buscar ahora".to_string(), MenuAction::CheckUpdate)),
                    ),
                };
                ui.label(text);
                if let Some((label, action)) = button {
                    let response = if action == MenuAction::ApplyUpdate {
                        accent_button(ui, &label, a)
                    } else {
                        ghost_button(ui, &label)
                    };
                    if response.clicked() {
                        cx.actions.push(action);
                    }
                }
            });
        },
        |ui, cx| {
            let a = cx.accent;
            group_label(ui, "MODOS SIN BORDES", a);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let mode = cx.config.mode;
                let mut mode_button = |ui: &mut Ui, label: &str, target: WindowMode| {
                    let response = if mode == target {
                        accent_button(ui, label, a)
                    } else {
                        ghost_button(ui, label)
                    };
                    if response.clicked() && mode != target {
                        cx.actions.push(MenuAction::SetMode(target));
                    }
                };
                mode_button(ui, "Ventana", WindowMode::Windowed);
                mode_button(ui, "Acoplar", WindowMode::Docked);
                mode_button(ui, "Flotante", WindowMode::Overlay);
            });
            ui.add_space(8.0);
            let w = &mut cx.config.window;
            slider(
                ui,
                "opacidad flotante",
                &mut w.overlay_opacity,
                0.15..=1.0,
                a,
            );
            toggle(
                ui,
                &mut w.overlay_click_through,
                "Ignorar clics en modo flotante",
                a,
            );
        },
    );
}

fn tab_audio(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let a = cx.accent;
            group_label(ui, "FUENTE", a);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                let au = &mut cx.config.audio;
                let mut mode_button =
                    |ui: &mut Ui, label: &str, hint: &str, target: CaptureMode| {
                        let response = if au.mode == target {
                            accent_button(ui, label, a)
                        } else {
                            ghost_button(ui, label)
                        };
                        if response.on_hover_text(hint).clicked() {
                            au.mode = target;
                        }
                    };
                mode_button(
                    ui,
                    "Automático",
                    "Elige sola la app que está sonando",
                    CaptureMode::Auto,
                );
                mode_button(
                    ui,
                    "Todo el sistema",
                    "Mezcla todo lo que suena en el PC",
                    CaptureMode::Device,
                );
            });
            ui.add_space(4.0);
            ui.add(Label::new(RichText::new(cx.audio).font(mono(10.0)).color(INK_SOFT)).truncate());
            ui.add_space(8.0);
            group_label(ui, "APPS CON AUDIO", a);
            if cx.apps.is_empty() {
                ui.label(RichText::new("No suena nada").color(INK_FAINT));
            }
            for app in cx.apps {
                let au = &mut cx.config.audio;
                let selected =
                    au.mode == CaptureMode::App && au.process.eq_ignore_ascii_case(&app.exe);
                if app_row(ui, app, selected, a)
                    .on_hover_text("Escuchar solo esta app")
                    .clicked()
                {
                    au.mode = CaptureMode::App;
                    au.process = app.exe.clone();
                }
            }
        },
        |ui, cx| {
            let a = cx.accent;
            group_label(ui, "CÓMO ELIGE", a);
            hint(
                ui,
                "Automático sigue a la app que suena; elegir una app la fija.",
            );
            hint(
                ui,
                "La reacción al audio de cada escena está en Escena · Reacción.",
            );
        },
    );
}

/// Cómo reacciona al audio la escena activa (se guarda en el preset).
fn tab_reaction(ui: &mut Ui, wide: bool, cx: &mut TabCtx<'_>) {
    two_col(
        ui,
        wide,
        cx,
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "VOLUMEN Y ESPECTRO", a);
            slider(ui, "ganancia", &mut v.gain, r::GAIN, a);
            slider(
                ui,
                "suavizado del espectro",
                &mut v.spectrum_smoothing,
                r::UNIT,
                a,
            );
            slider(ui, "peso de frecuencias", &mut v.freq_influence, r::UNIT, a);
        },
        |ui, cx| {
            let (a, v) = (cx.accent, &mut cx.config.visual);
            group_label(ui, "GOLPES", a);
            slider(
                ui,
                "sensibilidad a golpes",
                &mut v.beat_sensitivity,
                r::UNIT,
                a,
            );
            hint(ui, "Cada escena guarda su propia reacción al audio.");
        },
    );
}

/// Fila de la lista de apps: punto de actividad, nombre y título de ventana.
fn app_row(ui: &mut Ui, app: &AudioApp, selected: bool, accent: Color32) -> Response {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::click());
    let painter = ui.painter_at(rect);
    let hovered = response.hovered();
    if selected || hovered {
        let fill = if selected {
            with_alpha(accent, 34)
        } else {
            hairline(8)
        };
        painter.rect(
            rect,
            8.0,
            fill,
            Stroke::new(
                1.0,
                if selected {
                    with_alpha(accent, 140)
                } else {
                    hairline(EDGE_HOVER)
                },
            ),
            StrokeKind::Inside,
        );
    }
    let dot = pos2(rect.left() + 12.0, rect.center().y);
    let active = app.is_active();
    painter.circle_filled(dot, 3.5, if active { accent } else { INK_FAINT });
    if active {
        painter.circle_stroke(dot, 6.0, Stroke::new(1.0, with_alpha(accent, 90)));
    }

    let text_left = rect.left() + 24.0;
    let name = app.display_name();
    let galley = painter.layout_no_wrap(
        name,
        egui::FontId::proportional(11.5),
        if selected { INK } else { INK_SOFT },
    );
    let name_w = galley.size().x;
    painter.galley(
        pos2(text_left, rect.center().y - galley.size().y * 0.5),
        galley,
        INK,
    );
    if !app.title.is_empty() {
        let left = text_left + name_w + 8.0;
        let max_w = (rect.right() - 8.0 - left).max(0.0);
        let mut job = egui::text::LayoutJob::single_section(
            app.title.clone(),
            egui::TextFormat::simple(mono(9.5), INK_FAINT),
        );
        job.wrap = egui::text::TextWrapping::truncate_at_width(max_w);
        let galley = ui.fonts_mut(|f| f.layout_job(job));
        painter.galley(
            pos2(left, rect.center().y - galley.size().y * 0.5),
            galley,
            INK_FAINT,
        );
    }
    if hovered {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}
