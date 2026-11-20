//! Controles del dock pintados a mano: riel de slider, interruptor,
//! pestañas, chips de preset, botones de icono y glifos.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align, Align2, Color32, CornerRadius, Layout, Rect, Response, RichText, Sense, Stroke,
    StrokeKind, Ui, pos2, vec2,
};

use super::theme::{
    EDGE_HOVER, EDGE_REST, INK, INK_FAINT, INK_MUTED, INK_SOFT, ON_ACCENT, WELL, body, display,
    hairline, mono, with_alpha,
};
use crate::config::VisualPreset;

pub fn fmt_2_dec(v: f64) -> String {
    format!("{v:.2}")
}

pub fn fmt_int(v: f64) -> String {
    format!("{}", v.round() as i64)
}

const RAIL_HEIGHT: f32 = 4.0;
const THUMB_RADIUS: f32 = 6.0;

/// Riel fino con relleno de acento y pulgar circular. Clic o arrastre en
/// cualquier punto del riel fija el valor.
fn rail(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    accent: Color32,
    width: f32,
) -> Response {
    let (min, max) = (*range.start(), *range.end());
    let (rect, mut response) =
        ui.allocate_exact_size(vec2(width.max(40.0), 18.0), Sense::click_and_drag());
    let track = rect.x_range().shrink(THUMB_RADIUS);

    if (response.dragged() || response.clicked())
        && let Some(pointer) = response.interact_pointer_pos()
    {
        let t = ((pointer.x - track.min) / track.span().max(1.0)).clamp(0.0, 1.0);
        let next = min + t * (max - min);
        if (next - *value).abs() > f32::EPSILON {
            *value = next;
            response.mark_changed();
        }
    }

    let t = ((*value - min) / (max - min).max(f32::EPSILON)).clamp(0.0, 1.0);
    let thumb_x = track.min + t * track.span();
    let cy = rect.center().y;
    let rail_rect = Rect::from_min_max(
        pos2(track.min, cy - RAIL_HEIGHT * 0.5),
        pos2(track.max, cy + RAIL_HEIGHT * 0.5),
    );
    let fill_rect = Rect::from_min_max(rail_rect.min, pos2(thumb_x, rail_rect.max.y));

    let painter = ui.painter();
    painter.rect_filled(rail_rect, 2.0, WELL);
    painter.rect_stroke(
        rail_rect,
        2.0,
        Stroke::new(1.0, hairline(EDGE_REST)),
        StrokeKind::Outside,
    );
    painter.rect_filled(fill_rect, 2.0, accent);

    let engaged = response.hovered() || response.dragged();
    let center = pos2(thumb_x, cy);
    if engaged {
        painter.circle_filled(center, THUMB_RADIUS + 4.0, with_alpha(accent, 34));
    }
    painter.circle_filled(center, THUMB_RADIUS, INK);
    painter.circle_stroke(
        center,
        THUMB_RADIUS,
        Stroke::new(1.5, if engaged { accent } else { ON_ACCENT }),
    );

    if engaged {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response
}

/// Cápsula de valor en Geist Mono: se arrastra para ajuste fino o se hace
/// clic para escribir el número.
fn value_capsule(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    accent: Color32,
    fmt: fn(f64) -> String,
) -> Response {
    let (min, max) = (*range.start(), *range.end());
    let speed = ((max - min) / 256.0).max(0.001);
    // DragValue usa Button al mostrar y Body/Monospace al editar.
    for style in [
        egui::TextStyle::Body,
        egui::TextStyle::Button,
        egui::TextStyle::Monospace,
    ] {
        ui.style_mut().text_styles.insert(style, mono(10.0));
    }
    let response = ui.add(
        egui::DragValue::new(value)
            .range(min..=max)
            .speed(speed)
            .min_size(vec2(50.0, 18.0))
            .custom_formatter(move |v, _| fmt(v)),
    );
    let border = if response.hovered() || response.has_focus() || response.dragged() {
        with_alpha(accent, 140)
    } else {
        hairline(EDGE_REST)
    };
    ui.painter().rect_stroke(
        response.rect,
        6.0,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    response
}

/// Fila de control: etiqueta + cápsula de valor arriba, riel debajo.
pub fn slider_fmt(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    accent: Color32,
    fmt: fn(f64) -> String,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).font(body(11.0)).color(INK_SOFT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            value_capsule(ui, value, range.clone(), accent, fmt);
        });
    });
    rail(ui, value, range, accent, ui.available_width());
    ui.add_space(8.0);
}

pub fn slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    accent: Color32,
) {
    slider_fmt(ui, label, value, range, accent, fmt_2_dec);
}

pub fn slider_u32(
    ui: &mut Ui,
    label: &str,
    value: &mut u32,
    range: RangeInclusive<u32>,
    accent: Color32,
) {
    let (lo, hi) = (*range.start() as f32, *range.end() as f32);
    let mut tmp = *value as f32;
    slider_fmt(ui, label, &mut tmp, lo..=hi, accent, fmt_int);
    *value = tmp.round().clamp(lo, hi) as u32;
}

/// Slider compacto del dock: microetiqueta y lectura en una línea, riel
/// debajo. Pensado para los dos ajustes que más se tocan en vivo.
pub fn quick_slider(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    accent: Color32,
    width: f32,
) {
    ui.allocate_ui_with_layout(vec2(width, 38.0), Layout::top_down(Align::Min), |ui| {
        ui.spacing_mut().item_spacing = vec2(0.0, 3.0);
        let (row, _) = ui.allocate_exact_size(vec2(width, 14.0), Sense::hover());
        let painter = ui.painter();
        painter.text(
            row.left_center() + vec2(THUMB_RADIUS, 0.0),
            Align2::LEFT_CENTER,
            label,
            display(9.5),
            INK_MUTED,
        );
        painter.text(
            row.right_center() - vec2(THUMB_RADIUS, 0.0),
            Align2::RIGHT_CENTER,
            fmt_2_dec(*value as f64),
            mono(10.0),
            INK,
        );
        rail(ui, value, range, accent, width);
    });
}

/// Interruptor de fila completa: etiqueta a la izquierda, pista a la derecha.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str, accent: Color32) -> Response {
    let width = ui.available_width();
    let (rect, mut response) = ui.allocate_exact_size(vec2(width, 28.0), Sense::click());
    if response.clicked() {
        *on = !*on;
        response.mark_changed();
    }
    let t = ui
        .ctx()
        .animate_bool_with_time(response.id.with("knob"), *on, 0.12);

    let painter = ui.painter();
    if response.hovered() {
        painter.rect_filled(rect.expand2(vec2(4.0, 0.0)), 6.0, hairline(6));
    }
    painter.text(
        rect.left_center(),
        Align2::LEFT_CENTER,
        label,
        body(11.0),
        if *on { INK } else { INK_SOFT },
    );

    let track =
        Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(30.0, 16.0));
    let track_fill = lerp_color(WELL, with_alpha(accent, 70), t);
    painter.rect_filled(track, 8.0, track_fill);
    painter.rect_stroke(
        track,
        8.0,
        Stroke::new(
            1.0,
            lerp_color(hairline(EDGE_HOVER), with_alpha(accent, 170), t),
        ),
        StrokeKind::Inside,
    );
    let knob_x = egui::lerp((track.left() + 8.0)..=(track.right() - 8.0), t);
    painter.circle_filled(
        pos2(knob_x, track.center().y),
        5.0,
        lerp_color(INK_MUTED, accent, t),
    );
    response
}

/// Pestaña subrayada del cajón.
pub fn tab(ui: &mut Ui, label: &str, selected: bool, accent: Color32) -> Response {
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), display(12.0), INK);
    let (rect, response) =
        ui.allocate_exact_size(vec2(galley.size().x + 20.0, 30.0), Sense::click());
    let color = if selected {
        INK
    } else if response.hovered() {
        INK_SOFT
    } else {
        INK_MUTED
    };
    let painter = ui.painter();
    painter.galley_with_override_text_color(
        rect.center() - galley.size() * 0.5 - vec2(0.0, 2.0),
        galley.clone(),
        color,
    );
    if selected {
        let w = galley.size().x;
        painter.rect_filled(
            Rect::from_center_size(pos2(rect.center().x, rect.bottom() - 1.0), vec2(w, 2.0)),
            1.0,
            accent,
        );
    }
    response
}

/// Microetiqueta de grupo dentro del cajón.
pub fn group_label(ui: &mut Ui, text: &str, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 18.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_size(rect.left_center() - vec2(0.0, 4.0), vec2(2.0, 8.0)),
        1.0,
        accent,
    );
    painter.text(
        rect.left_center() + vec2(8.0, 0.0),
        Align2::LEFT_CENTER,
        text,
        display(9.5),
        INK_FAINT,
    );
    ui.add_space(2.0);
}

/// Nombre de una palabra para el tooltip de cada preset.
pub fn preset_word(preset: VisualPreset) -> &'static str {
    match preset {
        VisualPreset::WavyForm3 => "Onda",
        VisualPreset::WaveRing => "Anillo",
        VisualPreset::Alchemy => "Alquimia",
        VisualPreset::Ambience => "Ambiente",
        VisualPreset::Scope => "Trazo",
        VisualPreset::Hurricane => "Huracán",
        VisualPreset::Niagara => "Niagara",
        VisualPreset::Windmill => "Molino",
    }
}

/// Casilla cuadrada de preset (40 × 40), solo icono. La seleccionada se
/// tiñe del acento.
pub fn preset_tile(ui: &mut Ui, preset: VisualPreset, selected: bool, accent: Color32) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(40.0, 40.0), Sense::click());
    let painter = ui.painter_at(rect.expand(1.0));
    let hovered = response.hovered();
    let fill = if selected {
        with_alpha(accent, 26)
    } else if hovered {
        hairline(8)
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 12.0, fill);
    let stroke = if selected {
        with_alpha(accent, 150)
    } else if hovered {
        hairline(EDGE_HOVER)
    } else {
        hairline(EDGE_REST)
    };
    painter.rect_stroke(rect, 12.0, Stroke::new(1.0, stroke), StrokeKind::Inside);
    let color = if selected {
        accent
    } else if hovered {
        INK_SOFT
    } else {
        INK_MUTED
    };
    let icon = Rect::from_center_size(rect.center(), vec2(20.0, 20.0));
    draw_preset_icon(&painter, icon, preset, color);
    response
}

#[derive(Clone, Copy)]
pub enum Glyph {
    Gear,
    ChevronUp,
    ChevronDown,
    Close,
    Undo,
    /// Dado: modo aleatorio.
    Dice,
}

/// Botón cuadrado de icono (32 × 32).
pub fn icon_button(ui: &mut Ui, glyph: Glyph, accent: Color32, active: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(vec2(32.0, 32.0), Sense::click());
    let painter = ui.painter();
    let fill = if response.hovered() {
        hairline(14)
    } else if active {
        with_alpha(accent, 26)
    } else {
        Color32::TRANSPARENT
    };
    painter.rect_filled(rect, 10.0, fill);
    let color = if active {
        accent
    } else if response.hovered() {
        INK
    } else {
        INK_SOFT
    };
    paint_glyph(painter, rect.center(), glyph, color);
    response
}

pub fn paint_glyph(painter: &egui::Painter, c: egui::Pos2, glyph: Glyph, color: Color32) {
    let stroke = Stroke::new(1.6, color);
    match glyph {
        Glyph::Gear => {
            painter.circle_stroke(c, 5.0, stroke);
            for i in 0..8 {
                let a = i as f32 * std::f32::consts::TAU / 8.0;
                painter.circle_filled(c + vec2(a.cos(), a.sin()) * 7.6, 1.4, color);
            }
            painter.circle_filled(c, 1.6, color);
        }
        Glyph::ChevronUp => {
            painter.line_segment([c + vec2(-5.0, 2.5), c + vec2(0.0, -2.5)], stroke);
            painter.line_segment([c + vec2(0.0, -2.5), c + vec2(5.0, 2.5)], stroke);
        }
        Glyph::ChevronDown => {
            painter.line_segment([c + vec2(-5.0, -2.5), c + vec2(0.0, 2.5)], stroke);
            painter.line_segment([c + vec2(0.0, 2.5), c + vec2(5.0, -2.5)], stroke);
        }
        Glyph::Close => {
            painter.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], stroke);
            painter.line_segment([c + vec2(4.5, -4.5), c + vec2(-4.5, 4.5)], stroke);
        }
        Glyph::Undo => {
            // Arco de ~270° con punta de flecha a la izquierda.
            let r = 5.5;
            let points: Vec<egui::Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.0 - 1.5 * i as f32 / 16.0);
                    c + vec2(a.cos(), -a.sin()) * r + vec2(0.5, 1.0)
                })
                .collect();
            let start = points[0];
            painter.add(egui::Shape::line(points, stroke));
            painter.line_segment([start, start + vec2(-2.5, -3.0)], stroke);
            painter.line_segment([start, start + vec2(3.0, -2.0)], stroke);
        }
        Glyph::Dice => {
            let rect = egui::Rect::from_center_size(c, vec2(13.0, 13.0));
            painter.rect_stroke(rect, 3.0, stroke, egui::StrokeKind::Middle);
            for (x, y) in [
                (-3.0, -3.0),
                (3.0, 3.0),
                (0.0, 0.0),
                (3.0, -3.0),
                (-3.0, 3.0),
            ] {
                painter.circle_filled(c + vec2(x, y), 1.2, color);
            }
        }
    }
}

pub fn accent_button(ui: &mut Ui, text: &str, accent: Color32) -> Response {
    ui.add(
        egui::Button::new(
            RichText::new(text)
                .font(body(11.0))
                .strong()
                .color(ON_ACCENT),
        )
        .fill(accent)
        .stroke(Stroke::NONE)
        .corner_radius(CornerRadius::same(8))
        .min_size(vec2(84.0, 30.0)),
    )
}

pub fn ghost_button(ui: &mut Ui, text: &str) -> Response {
    ui.add(
        egui::Button::new(RichText::new(text).font(body(11.0)).color(INK_SOFT))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::new(1.0, hairline(16)))
            .corner_radius(CornerRadius::same(8))
            .min_size(vec2(0.0, 30.0)),
    )
}

/// Separador vertical capilar entre grupos del dock.
pub fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(1.0, 30.0), Sense::hover());
    ui.painter().vline(
        rect.center().x,
        rect.y_range(),
        Stroke::new(1.0, hairline(EDGE_REST)),
    );
}

/// Iconos de preset dibujados con la misma lógica que el visualizador.
pub fn draw_preset_icon(painter: &egui::Painter, rect: Rect, preset: VisualPreset, color: Color32) {
    match preset {
        VisualPreset::WavyForm3 => {
            for layer in 0..2 {
                let points: Vec<egui::Pos2> = (0..24)
                    .map(|i| {
                        let t = i as f32 / 23.0;
                        let x = rect.left() + t * rect.width();
                        let envelope = (t * std::f32::consts::PI).sin();
                        let y = rect.center().y
                            + (t * std::f32::consts::TAU * 1.5 + layer as f32 * 0.9).sin()
                                * rect.height()
                                * 0.32
                                * envelope;
                        pos2(x, y)
                    })
                    .collect();
                let stroke = if layer == 0 {
                    Stroke::new(1.5, color)
                } else {
                    Stroke::new(1.0, with_alpha(color, 110))
                };
                painter.add(egui::Shape::line(points, stroke));
            }
        }
        VisualPreset::WaveRing => {
            // Anillo con borde ondulado, como la onda envuelta en círculo.
            let center = rect.center();
            let radius = rect.width().min(rect.height()) * 0.36;
            for layer in 0..2 {
                let points: Vec<egui::Pos2> = (0..=48)
                    .map(|i| {
                        let a = i as f32 / 48.0 * std::f32::consts::TAU;
                        let wobble = (a * 6.0 + layer as f32 * 0.9).sin() * radius * 0.18;
                        center + vec2(a.cos(), a.sin()) * (radius + wobble)
                    })
                    .collect();
                let stroke = if layer == 0 {
                    Stroke::new(1.5, color)
                } else {
                    Stroke::new(1.0, with_alpha(color, 110))
                };
                painter.add(egui::Shape::line(points, stroke));
            }
        }
        VisualPreset::Scope => {
            // Osciloscopio: onda en una pantalla.
            painter.rect_stroke(
                rect,
                3.0,
                Stroke::new(1.0, with_alpha(color, 90)),
                StrokeKind::Inside,
            );
            let points: Vec<egui::Pos2> = (0..24)
                .map(|i| {
                    let t = i as f32 / 23.0;
                    let y = rect.center().y
                        + (t * std::f32::consts::TAU * 2.0).sin() * rect.height() * 0.28;
                    pos2(rect.left() + 2.0 + t * (rect.width() - 4.0), y)
                })
                .collect();
            painter.add(egui::Shape::line(points, Stroke::new(1.4, color)));
        }
        VisualPreset::Niagara => {
            // Estelas verticales en espejo con escalones.
            let c = rect.center().x;
            for (off, alpha) in [(0.18_f32, 120u8), (0.42, 255)] {
                for sign in [-1.0_f32, 1.0] {
                    let x0 = c + sign * rect.width() * off;
                    let x1 = x0 + sign * 2.5;
                    let mid = rect.top() + rect.height() * 0.45;
                    let points = vec![
                        pos2(x0, rect.top()),
                        pos2(x0, mid),
                        pos2(x1, mid),
                        pos2(x1, rect.bottom()),
                    ];
                    painter.add(egui::Shape::line(
                        points,
                        Stroke::new(1.3, with_alpha(color, alpha)),
                    ));
                }
            }
        }
        VisualPreset::Windmill => {
            // Aspa diagonal con bordes de onda sobre una nube.
            let c = rect.center();
            let r = rect.width().min(rect.height()) * 0.5;
            painter.circle_filled(c, r, with_alpha(color, 50));
            let d = vec2(0.8, -0.6);
            let n = vec2(0.6, 0.8) * (r * 0.22);
            for sign in [-1.0_f32, 1.0] {
                let points: Vec<egui::Pos2> = (0..=10)
                    .map(|i| {
                        let t = i as f32 / 10.0 * 2.0 - 1.0;
                        let wobble = if i % 2 == 0 { 0.0 } else { 1.2 };
                        c + d * (t * r) + n * sign + vec2(0.0, wobble)
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Stroke::new(1.4, color)));
            }
        }
        VisualPreset::Hurricane => {
            // Espiral con dos esferas en el ojo.
            let c = rect.center();
            let radius = rect.width().min(rect.height()) * 0.5;
            let points: Vec<egui::Pos2> = (0..40)
                .map(|i| {
                    let th = i as f32 / 39.0 * std::f32::consts::TAU * 1.4;
                    let r = radius * 0.28 * (0.3 * th).exp();
                    c + vec2(th.cos(), th.sin()) * r.min(radius)
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.0, with_alpha(color, 140)),
            ));
            painter.circle_filled(c + vec2(radius * 0.25, 0.0), 2.2, color);
            painter.circle_filled(c - vec2(radius * 0.25, 0.0), 2.2, color);
        }
        VisualPreset::Ambience => {
            // Tres nubes suaves que se superponen.
            let c = rect.center();
            let r = rect.width().min(rect.height()) * 0.28;
            for (dx, dy, alpha) in [(-0.35, 0.2, 90), (0.35, 0.15, 120), (0.0, -0.3, 170)] {
                painter.circle_filled(
                    c + vec2(dx * r * 1.6, dy * r * 1.6),
                    r,
                    with_alpha(color, alpha),
                );
            }
        }
        VisualPreset::Alchemy => {
            // Espiral de dos brazos: el remolino del feedback.
            let center = rect.center();
            let radius = rect.width().min(rect.height()) * 0.5;
            for arm in 0..2 {
                let phase = arm as f32 * std::f32::consts::PI;
                let points: Vec<egui::Pos2> = (0..32)
                    .map(|i| {
                        let t = i as f32 / 31.0;
                        let a = phase + t * std::f32::consts::TAU * 1.1;
                        center + vec2(a.cos(), a.sin()) * (radius * (0.12 + 0.88 * t))
                    })
                    .collect();
                let stroke = if arm == 0 {
                    Stroke::new(1.5, color)
                } else {
                    Stroke::new(1.0, with_alpha(color, 120))
                };
                painter.add(egui::Shape::line(points, stroke));
            }
            painter.circle_filled(center, 1.8, color);
        }
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    // Interpolación en espacio no premultiplicado para mantener el tono.
    let [ar, ag, ab, aa] = a.to_srgba_unmultiplied();
    let [br, bg, bb, ba] = b.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(mix(ar, br), mix(ag, bg), mix(ab, bb), mix(aa, ba))
}
