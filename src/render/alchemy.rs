//! Alquimia: recreación original inspirada en Alchemy de Windows Media
//! Player. Este módulo es el núcleo puro: el ciclo de modos de feedback y
//! los parámetros que recibe `alchemy.wgsl`. La geometría y los pases de GPU
//! viven en `wave.rs`.

use bytemuck::{Pod, Zeroable};

use crate::audio::AnalysisFrame;
use crate::config::VisualCfg;

/// Uniform de `fs_feedback`. Debe coincidir campo a campo con
/// `FeedbackParams` en `alchemy.wgsl`: 80 B, `smear` alineado a 8 y `base`
/// (vec3f) en el offset 64, alineado a 16.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
pub struct FeedbackParams {
    pub resolution: [f32; 2],
    pub time: f32,
    pub zoom: f32,
    pub rotation: f32,
    pub swirl: f32,
    pub warp_amp: f32,
    pub warp_freq: f32,
    pub kaleido: f32,
    pub blur: f32,
    pub fade: f32,
    pub hue_shift: f32,
    pub inject: f32,
    pub radial_blur: f32,
    pub smear: [f32; 2],
    /// Color de fondo hacia el que decae la estela (0 = negro).
    pub base: [f32; 3],
    /// 1 = la geometría con color negativo borra la estela.
    pub erase: f32,
    /// Contracción por eje (0 = sin efecto).
    pub stretch: [f32; 2],
    /// 1 = modo paleta: intensidad en el alfa, color `base` → `mid` → `high`.
    pub palette: f32,
    pub _pad0: f32,
    pub mid: [f32; 3],
    pub _pad1: f32,
    pub high: [f32; 3],
    pub _pad2: f32,
}

const _: () = assert!(std::mem::size_of::<FeedbackParams>() == 128);

/// Transformación de un modo del ciclo, expresada por frame a 60 fps y sin
/// modulación de audio.
#[derive(Clone, Copy, Debug)]
struct Mode {
    zoom: f32,
    rotation: f32,
    swirl: f32,
    warp_amp: f32,
    warp_freq: f32,
    kaleido: f32,
    blur: f32,
    radial_blur: f32,
    smear: [f32; 2],
}

const fn mode(zoom: f32, rotation: f32, swirl: f32, blur: f32) -> Mode {
    Mode {
        zoom,
        rotation,
        swirl,
        warp_amp: 0.0,
        warp_freq: 0.0,
        kaleido: 0.0,
        blur,
        radial_blur: 0.0,
        smear: [0.0, 0.0],
    }
}

const KALEIDO_MODE: usize = 2;

const MODES: [Mode; 5] = [
    // Remolino hacia adentro.
    mode(1.010, 0.006, 0.020, 1.0),
    // Zoom hacia afuera con rotación inversa.
    mode(0.990, -0.009, -0.010, 1.0),
    // Caleidoscopio de 6 segmentos.
    Mode {
        kaleido: 6.0,
        ..mode(1.006, 0.004, 0.0, 0.8)
    },
    // Smear horizontal ondulante.
    Mode {
        warp_amp: 0.004,
        warp_freq: 9.0,
        smear: [0.0025, 0.0],
        ..mode(1.0, 0.0, 0.0, 1.2)
    },
    // Estrella: zoom fuerte con desenfoque hacia el centro.
    Mode {
        radial_blur: 0.04,
        ..mode(1.018, 0.0, 0.0, 0.6)
    },
];

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

impl Mode {
    fn mix(self, other: Self, t: f32) -> Self {
        Self {
            zoom: lerp(self.zoom, other.zoom, t),
            rotation: lerp(self.rotation, other.rotation, t),
            swirl: lerp(self.swirl, other.swirl, t),
            warp_amp: lerp(self.warp_amp, other.warp_amp, t),
            warp_freq: lerp(self.warp_freq, other.warp_freq, t),
            // El pliegue no se interpola: cambia a mitad del crossfade.
            kaleido: if t < 0.5 { self.kaleido } else { other.kaleido },
            blur: lerp(self.blur, other.blur, t),
            radial_blur: lerp(self.radial_blur, other.radial_blur, t),
            smear: [
                lerp(self.smear[0], other.smear[0], t),
                lerp(self.smear[1], other.smear[1], t),
            ],
        }
    }
}

/// Ciclo de modos: cada 10–14 s (o en un beat fuerte tras 6 s) pasa a otro
/// modo con un crossfade de 1.5 s.
pub struct AlchemyMotion {
    current: usize,
    next: usize,
    /// Progreso del crossfade (0..1), `None` si no hay transición.
    fade: Option<f32>,
    mode_time: f32,
    mode_length: f32,
    hue: f32,
    rng: u32,
}

const CROSSFADE: f32 = 1.5;
const MIN_MODE_TIME: f32 = 6.0;

impl AlchemyMotion {
    pub fn new() -> Self {
        Self {
            current: 0,
            next: 0,
            fade: None,
            mode_time: 0.0,
            mode_length: 12.0,
            hue: 0.0,
            rng: 0x9E37_79B9,
        }
    }

    /// xorshift32: suficiente para variar el ciclo, sin dependencias.
    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn pick_next(&mut self, allow_kaleido: bool) -> usize {
        loop {
            let candidate = (self.random() * MODES.len() as f32) as usize % MODES.len();
            if candidate != self.current && (allow_kaleido || candidate != KALEIDO_MODE) {
                return candidate;
            }
        }
    }

    /// Avanza el ciclo `dt` segundos y devuelve los parámetros del feedback
    /// (sin `resolution`, que pone el shell de GPU).
    pub fn update(
        &mut self,
        dt: f32,
        time: f32,
        analysis: &AnalysisFrame,
        cfg: &VisualCfg,
        bands: [f32; 3],
    ) -> FeedbackParams {
        let speed = cfg.alchemy_speed;
        let allow_kaleido = cfg.alchemy_kaleido;
        let motion = cfg.alchemy_motion;
        self.mode_time += dt * speed.max(0.1);

        let forced = !allow_kaleido && self.current == KALEIDO_MODE;
        let due =
            self.mode_time > self.mode_length || (analysis.beat && self.mode_time > MIN_MODE_TIME);
        if self.fade.is_none() && (due || forced) {
            self.next = self.pick_next(allow_kaleido);
            self.fade = Some(0.0);
        }
        if let Some(t) = self.fade {
            let t = t + dt / CROSSFADE;
            if t >= 1.0 {
                self.current = self.next;
                self.fade = None;
                self.mode_time = 0.0;
                // Duración promedio del ajuste, ±17 % (por defecto 10–14 s).
                self.mode_length = cfg.alchemy_mode_time * (0.83 + 0.34 * self.random());
            } else {
                self.fade = Some(t);
            }
        }
        let shape = match self.fade {
            Some(t) => {
                // Suavizado S para que el cambio no se note mecánico.
                let s = t * t * (3.0 - 2.0 * t);
                MODES[self.current].mix(MODES[self.next], s)
            }
            None => MODES[self.current],
        };

        let [bass, mid, treble] = bands;
        let beat = analysis.beat_env.clamp(0.0, 1.0);
        // Los parámetros del modo están pensados por frame a 60 fps.
        let frames = (dt * 60.0).clamp(0.0, 6.0) * speed;
        let zoom =
            (1.0 + (shape.zoom - 1.0 + 0.004 * bass + 0.008 * beat) * motion).powf(frames.max(0.0));
        let spin = if shape.rotation < 0.0 { -1.0 } else { 1.0 };
        let rotation = (shape.rotation + spin * 0.005 * mid) * motion * frames;

        let hue_speed = (0.002 + 0.012 * treble) * cfg.color_cycle;
        self.hue = (self.hue + hue_speed * frames).rem_euclid(1.0);
        let trail = cfg.alchemy_trail;
        // En silencio la imagen se apaga hacia negro.
        let fade = trail.powf((dt * 60.0).max(0.0)) * if analysis.silent { 0.9 } else { 1.0 };

        FeedbackParams {
            resolution: [1.0, 1.0],
            time: time * speed,
            zoom,
            rotation,
            swirl: shape.swirl * frames,
            warp_amp: shape.warp_amp * frames.min(2.0),
            warp_freq: shape.warp_freq,
            kaleido: shape.kaleido,
            blur: shape.blur,
            fade,
            hue_shift: hue_speed * frames * std::f32::consts::TAU,
            inject: 0.55,
            radial_blur: shape.radial_blur,
            smear: [shape.smear[0] * frames, shape.smear[1] * frames],
            ..FeedbackParams::zeroed()
        }
    }

    /// Tono base que deriva con el tiempo, para la paleta de los trazos.
    pub fn hue(&self) -> f32 {
        self.hue
    }
}
