//! Presets con feedback: Anillo líquido por bandas, Scope, Huracán, Niagara y
//! Windmill. Scope, Niagara y Windmill son recreaciones originales inspiradas
//! en Windows Media Player (hechas a partir de observar su salida, sin código
//! de Microsoft).
//!
//! Cada builder dibuja su geometría en `out` y devuelve los parámetros del
//! feedback de GPU (`alchemy.wgsl`), que convierte los trazos en estelas.

use std::collections::VecDeque;
use std::f32::consts::{PI, TAU};

use bytemuck::Zeroable;

use super::{Vertex, hsl_to_rgb, push_disc, push_segment, push_tri, resample_bands, scale_rgb};
use crate::audio::AnalysisFrame;
use crate::config::{VisualCfg, VisualPreset};
use crate::render::alchemy::FeedbackParams;
use crate::render::palette;

/// Lo que todos los builders necesitan del frame.
pub struct Frame<'a> {
    pub analysis: &'a AnalysisFrame,
    pub cfg: &'a VisualCfg,
    pub width: f32,
    pub height: f32,
    pub time: f32,
    pub ppp: f32,
}

impl Frame<'_> {
    fn min_dim(&self) -> f32 {
        self.width.min(self.height)
    }
    fn center(&self) -> (f32, f32) {
        (self.width * 0.5, self.height * 0.5)
    }
}

/// Tramo de rayo: de un punto a otro.
type Bolt = ((f32, f32), (f32, f32));

/// Un rayo vivo del Huracán: segmentos en pantalla y su vida restante.
struct Strike {
    segments: Vec<Bolt>,
    life: f32,
    max_life: f32,
}

const STRIKE_LIFE: f32 = 0.22;
/// Posiciones guardadas por esfera para la estela de cometa.
const SPHERE_TRAIL: usize = 48;
/// Los presets de WMP corren a ~30 cuadros por segundo; sus constantes
/// medidas están expresadas por cuadro a esa tasa.
const WMP_FPS: f32 = 30.0;
/// Windmill: giro del aspa en rad/s (medido: ~8 °/s, antihorario).
/// La distancia de emisión de Niagara (0.406), su contracción (0.93) y la
/// media anchura de la franja de Windmill (0.095) son ahora ajustes del
/// preset, con esos valores medidos por defecto.
const WINDMILL_SPIN: f32 = 0.14;

pub struct FxState {
    last_time: f32,
    dt: f32,
    hue: f32,
    bands: Vec<f32>,
    /// Golpe propio: 1 en el beat, cae en ~180 ms.
    kick: f32,
    rng: u32,
    // Huracán
    sphere_time: [f32; 2],
    sphere_trails: [VecDeque<(f32, f32, f32)>; 2],
    hurricane_cooldown: f32,
    strikes: Vec<Strike>,
    // Niagara y Windmill
    windmill_angle: f32,
}

impl FxState {
    pub fn new() -> Self {
        Self {
            last_time: 0.0,
            dt: 1.0 / 60.0,
            hue: 0.0,
            bands: Vec::with_capacity(64),
            kick: 0.0,
            rng: 0x2545_F491,
            sphere_time: [0.0, 7.3],
            sphere_trails: [
                VecDeque::with_capacity(SPHERE_TRAIL + 1),
                VecDeque::with_capacity(SPHERE_TRAIL + 1),
            ],
            hurricane_cooldown: 0.0,
            strikes: Vec::new(),
            windmill_angle: 0.0,
        }
    }

    /// xorshift32 en [0, 1).
    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }

    fn signed(&mut self) -> f32 {
        self.random() * 2.0 - 1.0
    }

    /// Dibuja el preset y devuelve el feedback. Los presets sin feedback
    /// devuelven parámetros en cero (no se usan).
    pub fn build(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        self.dt = (f.time - self.last_time).clamp(0.001, 0.1);
        self.last_time = f.time;
        self.hue = (self.hue + self.dt * 0.02 * f.cfg.color_cycle).rem_euclid(1.0);
        if f.analysis.beat {
            self.kick = 1.0;
        } else {
            self.kick *= (-self.dt * 5.5).exp();
        }
        match f.cfg.preset {
            VisualPreset::WaveRing => self.ring(out, f),
            VisualPreset::Scope => self.scope(out, f),
            VisualPreset::Hurricane => self.hurricane(out, f),
            VisualPreset::Niagara => self.niagara(out, f),
            VisualPreset::Windmill => self.windmill(out, f),
            VisualPreset::WavyForm3 | VisualPreset::Alchemy | VisualPreset::Ambience => {
                FeedbackParams::zeroed()
            }
        }
    }

    /// Frames a 60 fps que representa este `dt` (los parámetros del
    /// feedback están pensados por frame).
    fn frames(&self) -> f32 {
        (self.dt * 60.0).clamp(0.0, 6.0)
    }

    fn feedback(&self, trail: f32) -> FeedbackParams {
        FeedbackParams {
            resolution: [1.0, 1.0],
            zoom: 1.0,
            fade: trail.clamp(0.5, 0.995).powf(self.frames()),
            blur: 1.0,
            inject: 0.6,
            ..FeedbackParams::zeroed()
        }
    }

    // ------------------------------------------------------------------
    // Anillo: un anillo de onda por rango de frecuencia + estela líquida.
    // ------------------------------------------------------------------

    fn ring(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        let a = f.analysis;
        let wave = &a.waveform;
        let n = wave.len();
        let rings = f.cfg.ring_bands.max(1) as usize;
        resample_bands(&a.bands, rings, &mut self.bands);
        if n < 2 || self.bands.len() < rings {
            return self.feedback(f.cfg.ring_trail);
        }

        let (cx, cy) = f.center();
        let outer = f.min_dim() * 0.5 * f.cfg.ring_radius;
        let inner = outer * f.cfg.ring_inner;
        let sensitivity = f.cfg.sensitivity.max(0.0) / 1.5;
        let rotation = f.time * f.cfg.ring_rotation * TAU;
        let base_hw = (f.cfg.line_width * f.ppp * 0.5).max(0.6);
        let steps = f.cfg.ring_steps.max(8) as usize;
        let custom = palette::custom(f.cfg);

        for k in 0..rings {
            let t = if rings > 1 {
                k as f32 / (rings - 1) as f32
            } else {
                0.5
            };
            let energy = self.bands[k];
            // Graves al centro (respiran más), agudos afuera.
            let radius = (inner + (outer - inner) * t) * (1.0 + 0.10 * energy * (1.0 - t));
            let amp = f.min_dim() * f.cfg.ring_amplitude * (0.2 + 1.2 * energy) * sensitivity;
            let dir = if k % 2 == 0 { 1.0 } else { -1.0 };
            let spin = rotation * (1.0 + 0.35 * k as f32) * dir;
            // Color por banda: magenta en graves → cian en agudos.
            let color = match &custom {
                Some(colors) => scale_rgb(
                    palette::cycle3(colors, t * 0.67 + self.hue * 0.3),
                    0.75 + 0.7 * energy,
                ),
                None => {
                    let hue = (0.92 - 0.45 * t + self.hue * 0.3).rem_euclid(1.0);
                    scale_rgb(hsl_to_rgb(hue, 0.9, 0.38 + 0.32 * energy), 1.25)
                }
            };
            let hw = base_hw * (1.0 + 0.8 * energy);

            let point = |s: usize| {
                let u = s as f32 / steps as f32;
                // Ida y vuelta sobre la onda: cierra sin costura. Cada anillo
                // empieza en otra zona de la onda para que no se repitan.
                let m = 1.0 - (2.0 * u - 1.0).abs();
                let idx = ((m * 0.5 + 0.25 * t) * (n - 1) as f32) as usize % n;
                let r = (radius + wave[idx] * amp * 2.5).max(2.0);
                let ang = u * TAU + spin;
                (cx + r * ang.cos(), cy + r * ang.sin())
            };
            for s in 0..steps {
                push_segment(out, point(s), point(s + 1), color, color, hw);
            }
        }

        // Estela líquida: tinta en agua que ondula y se expande despacio.
        let bass = self.bands[0];
        let frames = self.frames();
        let flow = f.cfg.ring_flow;
        FeedbackParams {
            zoom: (1.0 + (0.001 + 0.003 * bass) * flow).powf(frames),
            rotation: 0.0015 * flow * frames,
            swirl: 0.003 * flow * frames,
            warp_amp: 0.0015 * flow * frames.min(2.0),
            warp_freq: 7.0,
            time: f.time,
            // Inyección baja: el anillo actual se ve nítido y la estela no se
            // acumula hasta difuminarlo todo.
            inject: 0.45,
            ..self.feedback(f.cfg.ring_trail)
        }
    }

    // ------------------------------------------------------------------
    // Scope: osciloscopio de la forma de onda con estela.
    // ------------------------------------------------------------------

    fn scope(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        let wave = &f.analysis.waveform;
        let n = wave.len();
        if n < 2 {
            return self.feedback(f.cfg.fx_trail);
        }
        let (_, cy) = f.center();
        let amp = f.height * f.cfg.scope_height * f.cfg.sensitivity.max(0.0) / 1.5;
        let hw = (f.cfg.line_width * f.ppp * 0.75).max(0.8);
        let level = f.analysis.level.clamp(0.0, 1.0);
        let custom = palette::custom(f.cfg);
        let step = (n / 512).max(1);
        let mut prev: Option<((f32, f32), [f32; 4])> = None;
        for i in (0..n).step_by(step) {
            let u = i as f32 / (n - 1) as f32;
            let p = (u * f.width, cy + wave[i] * amp);
            let t = (self.hue + 0.25 * u).rem_euclid(1.0);
            let rgb = match &custom {
                Some(colors) => palette::cycle3(colors, t),
                None => palette::sample(palette::ALCHEMY, t),
            };
            let c = scale_rgb(rgb, 0.8 + 0.6 * level);
            if let Some((pp, pc)) = prev {
                push_segment(out, pp, p, pc, c, hw);
            }
            prev = Some((p, c));
        }
        let frames = self.frames();
        FeedbackParams {
            // Las copias anteriores se abren hacia afuera y se apagan.
            zoom: f.cfg.scope_expand.powf(frames),
            blur: f.cfg.scope_blur,
            inject: 0.7,
            ..self.feedback(f.cfg.fx_trail * 0.97)
        }
    }

    // ------------------------------------------------------------------
    // Huracán: esferas que viajan por la tormenta (la nebulosa la dibuja
    // `ambience.wgsl` en modo 1) y rayos ramificados en el beat.
    // ------------------------------------------------------------------

    /// Posición y profundidad (0.2..1) de una esfera en su tiempo propio `s`.
    /// Entra y sale del ojo, cruza la nube y deriva en curvas tipo ocho.
    fn sphere_at(s: f32, i: usize, center: (f32, f32), eye: f32) -> (f32, f32, f32) {
        let phi = i as f32 * PI;
        let angle = phi + s * 0.9 + 0.6 * (s * 0.37 + phi).sin() + 0.8 * (s * 0.62).sin();
        let radius = eye
            * (1.7 + 1.3 * (s * 0.53 + phi * 1.3).sin() + 0.5 * (s * 1.21 + phi).sin()).max(0.15);
        let depth = 0.6 + 0.4 * (s * 0.71 + phi).sin();
        (
            center.0 + radius * angle.cos(),
            center.1 + radius * angle.sin() * 0.85,
            depth,
        )
    }

    fn hurricane(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        resample_bands(&f.analysis.bands, 3, &mut self.bands);
        let (bass, mid, treble) = (self.bands[0], self.bands[1], self.bands[2]);
        let dt = self.dt;
        let center = f.center();
        let eye = f.min_dim() * f.cfg.hurricane_eye;
        let kick = self.kick;
        let size = f.cfg.hurricane_sphere_size;
        let custom = palette::custom(f.cfg);

        // Las esferas viajan: cada una tiene su propio tiempo, que el beat
        // acelera de golpe.
        let mut spheres = [(0.0, 0.0, 0.0); 2];
        let pairs = self
            .sphere_time
            .iter_mut()
            .zip(self.sphere_trails.iter_mut());
        for (i, ((time, trail), slot)) in pairs.zip(spheres.iter_mut()).enumerate() {
            *time += dt * (0.8 + 0.8 * bass + 3.0 * kick) * f.cfg.hurricane_sphere_speed;
            let p = Self::sphere_at(*time, i, center, eye);
            *slot = p;
            trail.push_front(p);
            trail.truncate(SPHERE_TRAIL);
        }

        for (i, trail) in self.sphere_trails.iter().enumerate() {
            let rgb = match (&custom, i) {
                (Some(colors), _) => colors[i.min(1)],
                (None, 0) => [0.55, 0.85, 1.0],
                (None, _) => [0.85, 0.65, 1.0],
            };
            // Estela de cometa: se afina y se apaga hacia atrás.
            let len = trail.len().max(2) as f32;
            for (k, (a, b)) in trail.iter().zip(trail.iter().skip(1)).enumerate() {
                let fade = 1.0 - k as f32 / len;
                let c = scale_rgb(rgb, 0.9 * fade * a.2);
                push_segment(
                    out,
                    (a.0, a.1),
                    (b.0, b.1),
                    c,
                    c,
                    (0.4 + 3.2 * fade * a.2) * size.max(0.1),
                );
            }
            // Esfera con halo; el tamaño sigue la profundidad.
            let (x, y, z) = spheres[i];
            if size > 0.0 {
                push_disc(
                    out,
                    (x, y),
                    ((10.0 + 8.0 * kick) * z + 4.0) * size,
                    scale_rgb(rgb, 0.18),
                    16,
                );
                push_disc(
                    out,
                    (x, y),
                    (6.0 * z + 1.5) * size,
                    scale_rgb(rgb, 0.75),
                    14,
                );
                push_disc(
                    out,
                    (x, y),
                    (3.0 * z + 0.8) * size,
                    scale_rgb([1.0, 1.0, 1.0], 1.6),
                    10,
                );
            }
        }

        // Rayos: en cada beat (o por energía alta) nacen rayos ramificados
        // entre las esferas y hacia la tormenta.
        self.hurricane_cooldown -= dt;
        let bolts = f.cfg.hurricane_bolts;
        let energetic = f.analysis.level > 0.55 && self.random() < 0.04 * bolts;
        if bolts > 0.0 && self.hurricane_cooldown <= 0.0 && (f.analysis.beat || energetic) {
            self.hurricane_cooldown = 0.12 / bolts.max(0.05);
            let count = 1 + (self.random() * 2.0 + treble) as usize;
            for c in 0..count {
                let from = (spheres[c % 2].0, spheres[c % 2].1);
                let to = if c == 0 {
                    (spheres[1].0, spheres[1].1)
                } else {
                    let a = self.random() * TAU;
                    let r = eye * (1.5 + 2.5 * self.random());
                    (center.0 + r * a.cos(), center.1 + r * a.sin())
                };
                let mut segments = Vec::with_capacity(96);
                self.lightning(from, to, 5, 0.22, &mut segments, true);
                self.strikes.push(Strike {
                    segments,
                    life: STRIKE_LIFE,
                    max_life: STRIKE_LIFE,
                });
            }
        }
        for s in &mut self.strikes {
            s.life -= dt;
        }
        self.strikes.retain(|s| s.life > 0.0);
        let tint = match &custom {
            Some(colors) => BoltTint {
                halo: colors[2],
                core: colors[2].map(|v| 0.5 + 0.5 * v),
            },
            None => STORM_BOLT,
        };
        for strike in &self.strikes {
            let segments = strike.segments.iter().copied();
            draw_bolt(
                out,
                segments,
                strike.segments.len(),
                strike.life / strike.max_life,
                1.0,
                tint,
            );
        }

        let frames = self.frames();
        FeedbackParams {
            // Las estelas giran y caen despacio hacia el ojo.
            zoom: (0.996 - 0.003 * bass).powf(frames),
            rotation: (0.004 + 0.006 * mid) * frames,
            swirl: 0.015 * f.cfg.hurricane_swirl * frames,
            blur: 0.8,
            inject: 0.6,
            ..self.feedback(f.cfg.fx_trail * 0.95)
        }
    }

    // ------------------------------------------------------------------
    // Niagara y Windmill (Ambience de WMP) trabajan con intensidad + paleta:
    // el feedback guarda la intensidad en el alfa y la colorea con
    // `base → mid → high`. La geometría escribe intensidad en gris (canal
    // rojo) y la franja de Windmill borra con color negativo.
    // ------------------------------------------------------------------

    /// Cuadros de WMP (30 fps) que representa este `dt`.
    fn wmp_frames(&self) -> f32 {
        (self.dt * WMP_FPS).clamp(0.0, 3.0)
    }

    /// Niagara: cada cuadro nace a cada lado una línea vertical (la forma de
    /// onda de pie, en espejo) con brillo según el volumen; el feedback
    /// contrae la imagen en horizontal hacia el centro, así las líneas viajan
    /// hacia adentro, se juntan y se apagan sin llegar a tocarse.
    fn niagara(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        let (w, h) = (f.width, f.height);
        let cx = w * 0.5;
        let level = f.analysis.level.clamp(0.0, 1.0);
        // Como en el original, solo los golpes emiten: entre beats casi nada,
        // así quedan pocas líneas separadas en vez de un peine continuo.
        // Una sola línea por golpe: solo el cuadro del beat emite.
        let intensity = if f.analysis.beat {
            (0.75 + 0.5 * level).min(1.0)
        } else if f.cfg.niagara_continuous {
            level * 0.8
        } else {
            0.0
        };
        if intensity < 0.02 {
            return self.niagara_feedback(f);
        }
        let gray = [intensity, intensity, intensity, 1.0];
        let wave = &f.analysis.waveform;
        let n = wave.len().max(1);
        let amp = w * 0.05 * f.cfg.sensitivity.max(0.0) / 1.5;
        let points = f.cfg.niagara_steps.max(2) as usize;
        let line = f.cfg.niagara_line;
        let sample = |k: usize| wave.get(k * (n - 1) / (points - 1)).copied().unwrap_or(0.0);
        for sign in [-1.0_f32, 1.0] {
            // Espejo: el lado derecho invierte la onda.
            let x_emit = cx + sign * w * f.cfg.niagara_emit;
            // Escalones: tramos verticales unidos por saltos horizontales.
            let x_at = |k: usize| x_emit + sign * sample(k) * amp;
            for k in 0..points - 1 {
                let (y0, y1) = (
                    h * k as f32 / (points - 1) as f32,
                    h * (k + 1) as f32 / (points - 1) as f32,
                );
                let x = x_at(k);
                push_segment(out, (x, y0), (x, y1), gray, gray, line);
                push_segment(out, (x, y1), (x_at(k + 1), y1), gray, gray, line);
            }
        }
        self.niagara_feedback(f)
    }

    fn niagara_feedback(&self, f: &Frame<'_>) -> FeedbackParams {
        let frames = self.wmp_frames();
        let speed = f.cfg.niagara_speed;
        let (base, mid, high) = custom_wmp(f.cfg)
            .unwrap_or_else(|| wmp_palette(self.hue * 0.3, PaletteStyle::Contrast));
        FeedbackParams {
            // Las líneas avanzan a pulsos: en el golpe la contracción se
            // acelera y entre golpes casi se detiene.
            stretch: [
                f.cfg
                    .niagara_squeeze
                    .powf(frames * speed * (0.35 + 1.8 * self.kick)),
                1.0,
            ],
            // Y ondulan con el golpe: desplazamiento horizontal según la
            // altura, que se apaga junto con el kick.
            warp_amp: 0.0025 * self.kick,
            warp_freq: 11.0,
            time: f.time * 3.0,
            // `fx_trail` por defecto (0.93) da el decaimiento medido (~0.95).
            fade: remap_trail(f.cfg.fx_trail, 0.88, 0.98).powf(frames),
            blur: 0.8,
            inject: 1.0,
            palette: 1.0,
            base,
            mid,
            high,
            zoom: 1.0,
            resolution: [1.0, 1.0],
            ..FeedbackParams::zeroed()
        }
    }

    /// Windmill: una franja gira despacio por el centro (~8 °/s) y borra la
    /// intensidad a su paso; sus bordes inyectan la forma de onda. Lo que
    /// queda se difunde (blur grande, casi sin decaer) y forma la nube.
    fn windmill(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        let speed = f.cfg.windmill_speed;
        let prev_angle = self.windmill_angle;
        // Antihorario en pantalla (y hacia abajo): el ángulo decrece.
        self.windmill_angle -= self.dt * WINDMILL_SPIN * speed * (1.0 + 0.6 * self.kick);
        let turned = self.windmill_angle - prev_angle;

        let (cx, cy) = f.center();
        let reach = (f.width * f.width + f.height * f.height).sqrt();
        let (sin, cos) = self.windmill_angle.sin_cos();
        let (ux, uy) = (cos, sin);
        let (vx, vy) = (-sin, cos);
        let at = |s: f32, t: f32| (cx + ux * s + vx * t, cy + uy * s + vy * t);
        // La franja respira con el golpe: sus bordes se abren y vuelven.
        let width = f.cfg.windmill_width.max(0.001);
        let half = f.height * width * (1.0 + 0.45 * self.kick);

        let erase = [-1.0, -1.0, -1.0, 1.0];
        let (a, b, c, d) = (
            at(-reach, -half),
            at(reach, -half),
            at(reach, half),
            at(-reach, half),
        );
        push_tri(out, a, b, c, erase, erase, erase);
        push_tri(out, a, c, d, erase, erase, erase);

        // Bordes: forma de onda en líneas dobles, intensidad alta.
        let wave = &f.analysis.waveform;
        let n = wave.len().max(1);
        let amp = f.height
            * f.cfg.windmill_amplitude
            * (1.0 + 2.0 * self.kick)
            * f.cfg.sensitivity.max(0.0)
            / 1.5;
        const STEPS: usize = 120;
        let lines = f.cfg.windmill_edge_lines.max(1);
        let gap = f.cfg.windmill_edge_gap / width;
        for sign in [-1.0_f32, 1.0] {
            for j in 0..lines {
                // La primera línea es la más brillante; las demás se apagan.
                let v = 1.0 - 0.3 * j.min(1) as f32 - 0.1 * j.saturating_sub(1) as f32;
                let (offset, color) = (1.0 + gap * j as f32, [v, v, v, 1.0]);
                let point = |i: usize| {
                    let u = i as f32 / STEPS as f32;
                    let value = wave
                        .get((u * (n - 1) as f32) as usize)
                        .copied()
                        .unwrap_or(0.0);
                    at(
                        -reach + 2.0 * reach * u,
                        sign * (half * offset + value * amp),
                    )
                };
                for i in 0..STEPS {
                    push_segment(out, point(i), point(i + 1), color, color, 1.0);
                }
            }
        }

        let frames = self.wmp_frames();
        let (base, mid, high) =
            custom_wmp(f.cfg).unwrap_or_else(|| wmp_palette(self.hue * 0.3, PaletteStyle::Mono));
        FeedbackParams {
            // La nube gira más lento que el aspa: la franja la barre.
            rotation: turned * 0.5,
            // `fx_trail` por defecto (0.93) da ~0.994: la nube casi no decae.
            fade: remap_trail(f.cfg.fx_trail, 0.96, 0.995).powf(frames),
            blur: f.cfg.windmill_blur,
            // Ondulación: rompe la nube en manchas como en el original.
            // Más ondulación en el golpe: los bordes de la nube se rompen en
            // jirones en vez de quedar lisos al separarse de la franja.
            warp_amp: 0.003 + 0.008 * self.kick,
            warp_freq: 13.0,
            time: f.time,
            inject: 1.0,
            palette: 1.0,
            erase: 1.0,
            base,
            mid,
            high,
            zoom: 1.0,
            resolution: [1.0, 1.0],
            ..FeedbackParams::zeroed()
        }
    }

    /// Rayo por desplazamiento de punto medio: subdivide `depth` veces
    /// moviendo cada punto medio en perpendicular; a veces nace una rama.
    /// Los extremos no se mueven, así el rayo termina exactamente en `b`.
    fn lightning(
        &mut self,
        a: (f32, f32),
        b: (f32, f32),
        depth: u32,
        roughness: f32,
        out: &mut Vec<Bolt>,
        branches: bool,
    ) {
        if depth == 0 {
            out.push((a, b));
            return;
        }
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        let len = (dx * dx + dy * dy).sqrt().max(1.0);
        let (nx, ny) = (-dy / len, dx / len);
        let off = len * roughness * self.signed();
        let mid = ((a.0 + b.0) * 0.5 + nx * off, (a.1 + b.1) * 0.5 + ny * off);
        self.lightning(a, mid, depth - 1, roughness, out, branches);
        self.lightning(mid, b, depth - 1, roughness, out, branches);
        // Rayos finos (aspereza baja): pocas ramas, cortas y solo en los
        // niveles chicos, para que no se abran en abanico.
        let fine = roughness < 0.15;
        let (branch_chance, can_branch) = if fine {
            (0.25, depth == 3)
        } else {
            (0.35, depth >= 3)
        };
        if branches && can_branch && self.random() < branch_chance {
            let angle = dy.atan2(dx) + self.signed() * if fine { 0.6 } else { 0.9 };
            let reach = if fine {
                0.12 + 0.12 * self.random()
            } else {
                0.3 + 0.3 * self.random()
            };
            let blen = len * reach;
            let end = (mid.0 + blen * angle.cos(), mid.1 + blen * angle.sin());
            self.lightning(mid, end, depth - 2, roughness, out, false);
        }
    }
}

/// Colores de un rayo: halo ancho y núcleo.
#[derive(Clone, Copy)]
struct BoltTint {
    halo: [f32; 3],
    core: [f32; 3],
}

/// Rayos de tormenta (Huracán): halo violeta, núcleo blanco.
const STORM_BOLT: BoltTint = BoltTint {
    halo: [0.6, 0.55, 1.0],
    core: [0.9, 0.92, 1.0],
};
/// Dibuja un rayo. `life` (0..1) lo apaga; `reveal` (0..1) dibuja solo los
/// primeros tramos (crecimiento).
fn draw_bolt(
    out: &mut Vec<Vertex>,
    segments: impl Iterator<Item = Bolt>,
    count: usize,
    life: f32,
    reveal: f32,
    tint: BoltTint,
) {
    let k = life.clamp(0.0, 1.0);
    let halo = scale_rgb(tint.halo, 0.18 * k);
    let core = scale_rgb(tint.core, 1.5 * k);
    let visible = (count as f32 * reveal.clamp(0.0, 1.0)).ceil() as usize;
    for (a, b) in segments.take(visible) {
        push_segment(out, a, b, halo, halo, 3.0);
        push_segment(out, a, b, core, core, 0.5 + 0.8 * k);
    }
}

/// Lleva el slider `fx_trail` al rango de persistencia propio de un preset:
/// 0.80–0.98 cae en `lo..hi`; fuera de ahí sigue la misma recta.
fn remap_trail(trail: f32, lo: f32, hi: f32) -> f32 {
    let t = (trail - 0.80) / 0.18;
    (lo + (hi - lo) * t).clamp(0.3, 0.998)
}

/// Paleta `base → mid → high` con los colores personalizados del preset.
fn custom_wmp(cfg: &VisualCfg) -> Option<([f32; 3], [f32; 3], [f32; 3])> {
    palette::custom(cfg).map(|[a, b, c]| (srgb_to_linear(a), srgb_to_linear(b), srgb_to_linear(c)))
}

/// Paletas de las visualizaciones Ambience de WMP.
#[derive(Clone, Copy)]
enum PaletteStyle {
    /// Fondo oscuro de un tono y líneas del tono opuesto (Niagara).
    Contrast,
    /// Un solo tono: fondo oscuro, medio y casi blanco (Windmill).
    Mono,
}

/// Paleta `base → mid → high` en espacio lineal. Los valores salen de medir
/// el original (sRGB): Niagara fondo (3,33,33), líneas (230,90,90), núcleo
/// casi blanco; Windmill (71,71,25) → (156,161,97) → (243,246,222).
fn wmp_palette(hue: f32, style: PaletteStyle) -> ([f32; 3], [f32; 3], [f32; 3]) {
    let h = hue.rem_euclid(1.0);
    let (base, mid, high) = match style {
        PaletteStyle::Contrast => (
            hsl_to_rgb(h, 0.83, 0.07),
            hsl_to_rgb((h + 0.5).rem_euclid(1.0), 0.75, 0.63),
            hsl_to_rgb((h + 0.5).rem_euclid(1.0), 1.0, 0.97),
        ),
        PaletteStyle::Mono => (
            hsl_to_rgb(h, 0.48, 0.19),
            hsl_to_rgb(h, 0.3, 0.6),
            hsl_to_rgb(h, 0.6, 0.91),
        ),
    };
    (
        srgb_to_linear(base),
        srgb_to_linear(mid),
        srgb_to_linear(high),
    )
}

fn srgb_to_linear(c: [f32; 3]) -> [f32; 3] {
    c.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    })
}
