use serde::{Deserialize, Serialize};

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Palette {
    #[default]
    Vizz3,
    Aurora,
}

#[allow(dead_code)]
impl Palette {
    pub fn label(self) -> &'static str {
        match self {
            Self::Vizz3 => "Vizz #3",
            Self::Aurora => "Aurora",
        }
    }

    pub fn all() -> [Self; 2] {
        [Self::Vizz3, Self::Aurora]
    }
}

/// Paleta base de Vizz Wavy Form #3: cian en los extremos, verde/lima,
/// amarillo, coral, rosa/rojo y vuelta al cian.
const VIZZ3: &[(f32, [f32; 3])] = &[
    (0.00, [0.02, 0.62, 0.96]),
    (0.08, [0.08, 0.95, 0.55]),
    (0.20, [0.42, 1.00, 0.30]),
    (0.34, [0.88, 1.00, 0.32]),
    (0.46, [1.00, 0.90, 0.36]),
    (0.58, [1.00, 0.56, 0.28]),
    (0.70, [1.00, 0.26, 0.46]),
    (0.80, [0.98, 0.32, 0.72]),
    (0.90, [0.35, 1.00, 0.65]),
    (1.00, [0.02, 0.62, 0.96]),
];

/// Paleta original de la primera referencia: verde → lima → amarillo →
/// coral → rosa → verde.
const AURORA: &[(f32, [f32; 3])] = &[
    (0.00, [0.05, 0.94, 0.42]),
    (0.20, [0.48, 1.00, 0.26]),
    (0.38, [0.98, 0.88, 0.24]),
    (0.55, [1.00, 0.56, 0.26]),
    (0.70, [1.00, 0.33, 0.56]),
    (0.84, [1.00, 0.18, 0.34]),
    (0.93, [0.35, 1.00, 0.62]),
    (1.00, [0.05, 0.94, 0.42]),
];

/// Alquimia: ciclo cálido-frío continuo (magenta, ámbar, lima, cian,
/// violeta) que se recorre con el tiempo.
pub const ALCHEMY: &[(f32, [f32; 3])] = &[
    (0.00, [1.00, 0.25, 0.65]),
    (0.20, [1.00, 0.62, 0.18]),
    (0.40, [0.62, 1.00, 0.25]),
    (0.60, [0.15, 0.85, 1.00]),
    (0.80, [0.55, 0.30, 1.00]),
    (1.00, [1.00, 0.25, 0.65]),
];

/// Color de 8 bits (sRGB) a `0..1`.
pub fn rgb8(c: [u8; 3]) -> [f32; 3] {
    c.map(|v| v as f32 / 255.0)
}

/// Degradado cíclico a → b → c → a. `t` da la vuelta en 1.
pub fn cycle3(colors: &[[f32; 3]; 3], t: f32) -> [f32; 3] {
    let t = t.rem_euclid(1.0) * 3.0;
    let i = (t as usize).min(2);
    let k = t - i as f32;
    let (a, b) = (colors[i], colors[(i + 1) % 3]);
    [
        a[0] + (b[0] - a[0]) * k,
        a[1] + (b[1] - a[1]) * k,
        a[2] + (b[2] - a[2]) * k,
    ]
}

/// Los tres colores personalizados del preset, si el modo es `Custom`.
pub fn custom(cfg: &crate::config::VisualCfg) -> Option<[[f32; 3]; 3]> {
    (cfg.color_mode == crate::config::ColorMode::Custom)
        .then(|| [rgb8(cfg.color_a), rgb8(cfg.color_b), rgb8(cfg.color_c)])
}

/// Interpola linealmente una tabla de paradas `(t, rgb)` ordenada por `t`.
/// `t` se recorta a `[0, 1]`.
pub fn sample(stops: &[(f32, [f32; 3])], t: f32) -> [f32; 3] {
    let t = t.clamp(0.0, 1.0);
    let mut prev = stops[0];
    for &next in &stops[1..] {
        if t <= next.0 {
            let span = (next.0 - prev.0).max(1e-6);
            let k = (t - prev.0) / span;
            return [
                prev.1[0] + (next.1[0] - prev.1[0]) * k,
                prev.1[1] + (next.1[1] - prev.1[1]) * k,
                prev.1[2] + (next.1[2] - prev.1[2]) * k,
            ];
        }
        prev = next;
    }
    stops[stops.len() - 1].1
}

#[allow(dead_code)]
pub fn color(palette: Palette, t: f32) -> [f32; 3] {
    let stops = match palette {
        Palette::Vizz3 => VIZZ3,
        Palette::Aurora => AURORA,
    };
    sample(stops, t.rem_euclid(1.0))
}
