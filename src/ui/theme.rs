//! Tokens del dock. Superficies de obsidiana que se separan por pasos de
//! luminosidad mínimos, bordes capilares y un único acento que cambia de
//! temperatura con el preset activo.

use eframe::egui::{self, Color32, FontFamily, FontId};

use crate::config::VisualPreset;

/// Superficie base del dock.
pub const OBSIDIAN: Color32 = Color32::from_rgb(10, 12, 16);
/// Cajón: un nivel por encima del dock.
pub const SLATE: Color32 = Color32::from_rgb(14, 17, 22);
/// Pozos de control (rieles, cápsulas, pistas): hundidos, más oscuros.
pub const WELL: Color32 = Color32::from_rgb(5, 7, 10);

/// Texto en cuatro niveles de lectura.
pub const INK: Color32 = Color32::from_gray(238);
pub const INK_SOFT: Color32 = Color32::from_gray(195);
pub const INK_MUTED: Color32 = Color32::from_gray(135);
pub const INK_FAINT: Color32 = Color32::from_gray(95);

/// Bordes capilares: reposo, hover y énfasis.
pub const EDGE_REST: u8 = 12;
pub const EDGE_HOVER: u8 = 26;

/// Texto oscuro sobre el acento.
pub const ON_ACCENT: Color32 = Color32::from_rgb(8, 10, 13);

/// Cada preset tiene su temperatura. El acento solo aparece en el control
/// activo.
pub fn preset_accent(preset: VisualPreset) -> Color32 {
    match preset {
        VisualPreset::WavyForm3 => Color32::from_rgb(96, 226, 255),
        VisualPreset::WaveRing => Color32::from_rgb(255, 178, 92),
        VisualPreset::Alchemy => Color32::from_rgb(110, 235, 170),
        VisualPreset::Ambience => Color32::from_rgb(178, 140, 255),
        VisualPreset::Scope => Color32::from_rgb(150, 255, 100),
        VisualPreset::Niagara => Color32::from_rgb(90, 225, 225),
        VisualPreset::Windmill => Color32::from_rgb(255, 140, 110),
        VisualPreset::XMarks => Color32::from_rgb(255, 214, 80),
        VisualPreset::Particles => Color32::from_rgb(255, 120, 200),
    }
}

pub fn hairline(alpha: u8) -> Color32 {
    Color32::from_white_alpha(alpha)
}

pub fn with_alpha(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Outfit: títulos, pestañas y microetiquetas.
pub fn display(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("outfit".into()))
}

/// Geist Mono: lecturas numéricas y estado.
pub fn mono(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name("mono".into()))
}

/// Instrument Sans: etiquetas de control.
pub fn body(size: f32) -> FontId {
    FontId::new(size, FontFamily::Proportional)
}

/// Alinea los widgets stock que siguen en uso (DragValue, ScrollArea,
/// botones) con los tokens del dock.
pub fn apply_style(ui: &mut egui::Ui, accent: Color32) {
    let v = ui.visuals_mut();
    v.selection.bg_fill = with_alpha(accent, 90);
    v.selection.stroke = egui::Stroke::new(1.0, accent);

    v.widgets.inactive.bg_fill = WELL;
    v.widgets.inactive.weak_bg_fill = WELL;
    v.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, hairline(EDGE_REST));
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, INK_SOFT);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(6);

    v.widgets.hovered.bg_fill = WELL;
    v.widgets.hovered.weak_bg_fill = hairline(10);
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, hairline(EDGE_HOVER));
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, INK);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(6);

    v.widgets.active.bg_fill = with_alpha(accent, 36);
    v.widgets.active.weak_bg_fill = with_alpha(accent, 36);
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, with_alpha(accent, 150));
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
    v.widgets.active.corner_radius = egui::CornerRadius::same(6);

    let style = ui.style_mut();
    style.text_styles.insert(egui::TextStyle::Body, body(11.0));
    style
        .text_styles
        .insert(egui::TextStyle::Button, body(11.0));
    style.text_styles.insert(egui::TextStyle::Small, body(9.5));
    style.spacing.scroll.bar_width = 4.0;
    style.spacing.scroll.floating = true;
}
