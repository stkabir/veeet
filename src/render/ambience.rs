//! Ambiente: recreación original inspirada en Ambience de Windows Media
//! Player. Núcleo puro: suaviza el audio y avanza el flujo de las nubes; el
//! dibujo es un shader de pantalla completa (`ambience.wgsl`).

use bytemuck::{Pod, Zeroable};

use crate::audio::AnalysisFrame;
use crate::config::VisualCfg;

/// Uniform de `fs_ambience`. Debe coincidir campo a campo con
/// `AmbienceParams` en `ambience.wgsl` (112 B).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct AmbienceParams {
    pub resolution: [f32; 2],
    pub time: f32,
    pub scale: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub level: f32,
    pub beat: f32,
    pub hue: f32,
    /// 0 = Ambiente (nubes), 1 = Huracán (nebulosa en espiral; `scale` es
    /// entonces el radio del ojo en unidades del shader).
    pub mode: f32,
    /// 1 = usar `color_a..c` en vez de la paleta propia.
    pub custom: f32,
    pub brightness: f32,
    pub _pad: [f32; 3],
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub color_c: [f32; 4],
}

const _: () = assert!(std::mem::size_of::<AmbienceParams>() == 112);

/// Constante de tiempo del suavizado del audio: las nubes respiran, no
/// tiemblan.
const AUDIO_SMOOTH: f32 = 0.25;

pub struct AmbienceMotion {
    bass: f32,
    mid: f32,
    treble: f32,
    level: f32,
    /// Tiempo del flujo: avanza más rápido con más energía, sin saltos.
    flow: f32,
    hue: f32,
    /// Golpe: salta a 1 en cada beat y decae en ~180 ms. Ambiente lo usa
    /// como pulso; Huracán además acelera el flujo del túnel.
    kick: f32,
}

impl AmbienceMotion {
    pub fn new() -> Self {
        Self {
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            level: 0.0,
            flow: 0.0,
            hue: 0.0,
            kick: 0.0,
        }
    }

    /// Avanza `dt` segundos. `bands` son graves, medios y agudos (0..1).
    pub fn update(
        &mut self,
        dt: f32,
        analysis: &AnalysisFrame,
        cfg: &VisualCfg,
        bands: [f32; 3],
        mode: f32,
        eye_radius: f32,
    ) -> AmbienceParams {
        let k = 1.0 - (-dt / AUDIO_SMOOTH).exp();
        let [bass, mid, treble] = bands;
        self.bass += (bass - self.bass) * k;
        self.mid += (mid - self.mid) * k;
        self.treble += (treble - self.treble) * k;
        self.level += (analysis.level.clamp(0.0, 1.0) - self.level) * k;

        // Pulso: ataque instantáneo en el beat y caída rápida. El shader lo
        // convierte en latido de escala y brillo, no en cambio de escena.
        if analysis.beat {
            self.kick = 1.0;
        } else {
            self.kick *= (-dt * 5.5).exp();
        }
        let hurricane = mode > 0.5;
        let speed = if hurricane {
            // El túnel del huracán fluye siempre y el beat lo empuja.
            0.6 + 1.6 * self.kick
        } else {
            cfg.ambience_speed
        };
        self.flow += dt * speed * (0.5 + 0.9 * self.level + 0.4 * self.mid);
        self.hue =
            (self.hue + dt * 0.012 * (1.0 + 2.0 * self.treble) * cfg.color_cycle).rem_euclid(1.0);
        let custom = crate::render::palette::custom(cfg);
        let color = |i: usize| match &custom {
            Some(c) => [c[i][0], c[i][1], c[i][2], 1.0],
            None => [0.0; 4],
        };

        AmbienceParams {
            resolution: [1.0, 1.0],
            time: self.flow,
            scale: if hurricane {
                eye_radius
            } else {
                cfg.ambience_scale
            },
            bass: self.bass,
            mid: self.mid,
            treble: self.treble,
            level: self.level,
            beat: self.kick.max(analysis.beat_env.clamp(0.0, 1.0) * 0.6),
            hue: self.hue,
            mode,
            custom: if custom.is_some() { 1.0 } else { 0.0 },
            brightness: if hurricane {
                1.0
            } else {
                cfg.ambience_brightness
            },
            _pad: [0.0; 3],
            color_a: color(0),
            color_b: color(1),
            color_c: color(2),
        }
    }
}
