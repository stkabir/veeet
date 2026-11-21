//! Presets con feedback: Anillo líquido por bandas, Scope, Niagara, Windmill,
//! X Marca y Partículas. Scope, Niagara, Windmill y X Marca son recreaciones
//! originales inspiradas en Windows Media Player (hechas a partir de observar
//! su salida, sin código de Microsoft).
//!
//! Cada builder dibuja su geometría en `out` y devuelve los parámetros del
//! feedback de GPU (`alchemy.wgsl`), que convierte los trazos en estelas.

use std::f32::consts::{PI, TAU};

use bytemuck::Zeroable;

use super::{Vertex, hsl_to_rgb, push_segment, push_tri, resample_bands, scale_rgb};
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

/// Los presets de WMP corren a ~30 cuadros por segundo; sus constantes
/// medidas están expresadas por cuadro a esa tasa.
const WMP_FPS: f32 = 30.0;
/// Windmill: giro del aspa en rad/s (medido: ~8 °/s, antihorario).
/// La distancia de emisión de Niagara (0.406), su contracción (0.93) y la
/// media anchura de la franja de Windmill (0.095) son ahora ajustes del
/// preset, con esos valores medidos por defecto.
const WINDMILL_SPIN: f32 = 0.14;
/// X Marca: puntos por brazo de la X.
const X_STEPS: usize = 256;

/// Una partícula del campo de flujo, en coordenadas normalizadas (alto = 1,
/// ancho = aspecto).
#[derive(Clone, Copy)]
struct Particle {
    x: f32,
    y: f32,
    px: f32,
    py: f32,
    /// Impulso propio (explosiones), decae solo.
    vx: f32,
    vy: f32,
    life: f32,
    max_life: f32,
    /// 0..1 fijo por partícula: varía el color y la velocidad.
    seed: f32,
}

pub struct FxState {
    last_time: f32,
    dt: f32,
    hue: f32,
    bands: Vec<f32>,
    /// Golpe propio: 1 en el beat, cae en ~180 ms.
    kick: f32,
    rng: u32,
    // Niagara y Windmill
    windmill_angle: f32,
    // X Marca
    x_angle: f32,
    road_travel: f32,
    // Partículas
    particles: Vec<Particle>,
    burst_flip: bool,
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
            windmill_angle: 0.0,
            x_angle: PI * 0.25,
            road_travel: 0.0,
            particles: Vec::new(),
            burst_flip: false,
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
            VisualPreset::Niagara => self.niagara(out, f),
            VisualPreset::Windmill => self.windmill(out, f),
            VisualPreset::XMarks => self.xmarks(out, f),
            VisualPreset::Particles => self.particles(out, f),
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
        // Espejo: una segunda onda invertida, con el tono corrido.
        let passes: &[(f32, f32)] = if f.cfg.scope_mirror {
            &[(1.0, 0.0), (-1.0, 0.5)]
        } else {
            &[(1.0, 0.0)]
        };
        for &(sign, shift) in passes {
            let mut prev: Option<((f32, f32), [f32; 4])> = None;
            for i in (0..n).step_by(step) {
                let u = i as f32 / (n - 1) as f32;
                let p = (u * f.width, cy + sign * wave[i] * amp);
                let t = (self.hue + 0.25 * u + shift).rem_euclid(1.0);
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
        // La franja respira con el golpe en proporción a su ancho: con ancho
        // 0 las aspas siguen pegadas.
        let width = f.cfg.windmill_width.max(0.0);
        let half = f.height * width * (1.0 + 0.45 * self.kick);

        let wave = &f.analysis.waveform;
        let n = wave.len().max(1);
        let amp = f.height
            * f.cfg.windmill_amplitude
            * (1.0 + 2.0 * self.kick)
            * f.cfg.sensitivity.max(0.0)
            / 1.5;
        // Juntar: 0 = cada borde lleva la onda en espejo (se abren entre sí),
        // 1 = los dos bordes siguen la misma onda, en paralelo.
        let join = f.cfg.windmill_join.clamp(0.0, 1.0);
        const STEPS: usize = 120;
        let sample = |i: usize| {
            let u = i as f32 / STEPS as f32;
            wave.get((u * (n - 1) as f32) as usize)
                .copied()
                .unwrap_or(0.0)
        };
        // Desplazamiento perpendicular de un borde en el punto `i`.
        let edge = |sign: f32, i: usize, extra: f32| {
            let shift = sample(i) * amp * (sign * (1.0 - join) + join);
            sign * (half + extra) + shift
        };
        let along = |i: usize| -reach + 2.0 * reach * i as f32 / STEPS as f32;

        // Borrado: solo entre las dos ondas internas, no un rectángulo, así
        // no queda negro entre el borde de la franja y la onda.
        let erase = [-1.0, -1.0, -1.0, 1.0];
        for i in 0..STEPS {
            let (s0, s1) = (along(i), along(i + 1));
            let (lo0, hi0) = (edge(-1.0, i, 0.0), edge(1.0, i, 0.0));
            let (lo1, hi1) = (edge(-1.0, i + 1, 0.0), edge(1.0, i + 1, 0.0));
            if hi0 - lo0 <= 0.5 && hi1 - lo1 <= 0.5 {
                continue;
            }
            let (a, b, c, d) = (at(s0, lo0), at(s1, lo1), at(s1, hi1), at(s0, hi0));
            push_tri(out, a, b, c, erase, erase, erase);
            push_tri(out, a, c, d, erase, erase, erase);
        }

        // Bordes: forma de onda en líneas dobles, intensidad alta.
        let lines = f.cfg.windmill_edge_lines.max(1);
        let gap = f.height * f.cfg.windmill_edge_gap;
        for sign in [-1.0_f32, 1.0] {
            for j in 0..lines {
                // La primera línea es la más brillante; las demás se apagan.
                let v = 1.0 - 0.3 * j.min(1) as f32 - 0.1 * j.saturating_sub(1) as f32;
                let color = [v, v, v, 1.0];
                let extra = gap * j as f32;
                let point = |i: usize| at(along(i), edge(sign, i, extra));
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

    // ------------------------------------------------------------------
    // X Marca: una X de ondas a pantalla completa que gira; cada vibración
    // deja estela y el feedback la convierte en un líquido que llena la
    // pantalla y se agita al ritmo del golpe.
    // ------------------------------------------------------------------

    fn xmarks(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        resample_bands(&f.analysis.bands, 3, &mut self.bands);
        let (bass, mid) = (self.bands[0], self.bands[1]);
        let (w, h) = (f.width, f.height);
        let (cx, cy) = f.center();
        let dt = self.dt;
        let kick = self.kick;
        let custom = palette::custom(f.cfg);
        let level = f.analysis.level.clamp(0.0, 1.0);
        let color_at = |t: f32| match &custom {
            Some(colors) => palette::cycle3(colors, t),
            None => palette::sample(palette::ALCHEMY, t.rem_euclid(1.0)),
        };

        // Fase del líquido: avanza con los graves y salta en cada golpe.
        let flow = f.cfg.xmarks_speed;
        self.road_travel += dt * flow * (0.4 + 0.9 * bass + 2.5 * kick);

        // --- X ---
        self.x_angle += dt * f.cfg.xmarks_spin * TAU * 0.1 * (1.0 + 0.8 * kick);
        let wave = &f.analysis.waveform;
        let n = wave.len();
        if n >= 2 {
            let reach = (w * w + h * h).sqrt() * 0.55;
            let amp = f.min_dim()
                * f.cfg.xmarks_amplitude
                * (1.0 + 1.5 * kick)
                * f.cfg.sensitivity.max(0.0)
                / 1.5;
            let hw = (f.cfg.line_width * f.ppp * 0.9).max(0.8);
            let arms = f.cfg.xmarks_arms.max(2) as usize;
            for arm in 0..arms {
                let ang = self.x_angle + arm as f32 * PI / arms as f32;
                let (sin, cos) = ang.sin_cos();
                let (ux, uy, vx, vy) = (cos, sin, -sin, cos);
                // Cada brazo lee otra parte de la onda para no repetirse.
                let start = arm * n / arms;
                let point = |i: usize| {
                    let u = i as f32 / X_STEPS as f32;
                    let s = -reach + 2.0 * reach * u;
                    let idx = (start + (u * (n - 1) as f32) as usize) % n;
                    // La onda se calma en el cruce y crece hacia las puntas.
                    let taper = 0.35 + 0.65 * (2.0 * u - 1.0).abs();
                    let t = wave[idx] * amp * taper;
                    (cx + ux * s + vx * t, cy + uy * s + vy * t)
                };
                let mut prev = point(0);
                for i in 1..=X_STEPS {
                    let p = point(i);
                    let u = i as f32 / X_STEPS as f32;
                    let c = scale_rgb(
                        color_at(self.hue + 0.3 * u + arm as f32 / arms as f32),
                        0.85 + 0.6 * level + 0.6 * kick,
                    );
                    push_segment(out, prev, p, c, c, hw * (1.0 + 0.6 * mid));
                    prev = p;
                }
            }
        }

        let frames = self.frames();
        let wobble = f.cfg.xmarks_curve;
        let pulse = f.cfg.xmarks_road;
        // El líquido gira un poco contra la X: las estelas se enroscan.
        let spin = f.cfg.xmarks_spin;
        FeedbackParams {
            // Se expande despacio y da un empujón en cada golpe: la estela
            // llena la pantalla.
            zoom: (1.0 + flow * (0.0015 + 0.004 * bass) + pulse * 0.018 * kick).powf(frames),
            rotation: -0.0015 * spin * frames,
            swirl: wobble * (0.002 + 0.01 * kick) * frames.min(2.0),
            // Ondulación: el agua se agita más con el golpe.
            warp_amp: wobble * (0.0015 + 0.006 * kick + 0.002 * bass) * frames.min(2.0),
            warp_freq: 6.0 + 2.0 * mid,
            time: self.road_travel * 2.0,
            hue_shift: 0.004 * f.cfg.color_cycle * frames,
            blur: 0.8,
            inject: 0.7,
            fade: remap_trail(f.cfg.fx_trail, 0.93, 0.985).powf(frames),
            ..self.feedback(f.cfg.fx_trail)
        }
    }

    // ------------------------------------------------------------------
    // Partículas: miles de puntos siguen un campo de flujo suave y dejan
    // estela; en cada golpe estallan desde el centro o desde un punto al
    // azar.
    // ------------------------------------------------------------------

    fn spawn(&mut self, aspect: f32, life: f32) -> Particle {
        let (x, y) = (self.random() * aspect, self.random());
        let max_life = life * (0.5 + self.random());
        Particle {
            x,
            y,
            px: x,
            py: y,
            vx: 0.0,
            vy: 0.0,
            life: max_life,
            max_life,
            seed: self.random(),
        }
    }

    fn particles(&mut self, out: &mut Vec<Vertex>, f: &Frame<'_>) -> FeedbackParams {
        resample_bands(&f.analysis.bands, 3, &mut self.bands);
        let (bass, mid, treble) = (self.bands[0], self.bands[1], self.bands[2]);
        let (w, h) = (f.width, f.height);
        let aspect = w / h.max(1.0);
        let dt = self.dt;
        let t = f.time;
        let level = f.analysis.level.clamp(0.0, 1.0);
        let cfg = f.cfg;

        let count = cfg.particles_count.max(1) as usize;
        while self.particles.len() < count {
            let mut p = self.spawn(aspect, cfg.particles_life);
            // Vidas escalonadas: no renacen todas juntas.
            p.life *= self.random();
            self.particles.push(p);
        }
        self.particles.truncate(count);

        // Explosión: alterna entre el centro y un punto al azar.
        let burst = if f.analysis.beat && cfg.particles_burst > 0.0 {
            self.burst_flip = !self.burst_flip;
            let origin = if self.burst_flip {
                (aspect * 0.5, 0.5)
            } else {
                (
                    aspect * (0.2 + 0.6 * self.random()),
                    0.2 + 0.6 * self.random(),
                )
            };
            Some((origin, cfg.particles_burst * (0.6 + 0.8 * level)))
        } else {
            None
        };

        let sc = cfg.particles_scale;
        let speed = 0.06 * cfg.particles_speed * (0.4 + 1.3 * level + 0.6 * mid);
        let damp = (-dt * 2.5).exp();
        let jitter = 0.08 * treble;
        let mut parts = std::mem::take(&mut self.particles);
        for p in parts.iter_mut() {
            p.px = p.x;
            p.py = p.y;
            // Campo de flujo: suma de senos rotados que cambian con el tiempo
            // (barato y sin rejilla). El ángulo sale del campo.
            let (x, y) = (p.x * sc, p.y * sc);
            let field = (x * 1.7 + t * 0.21).sin()
                + (y * 2.3 - t * 0.17 + (x * 0.9).sin()).sin()
                + ((x + y) * 1.1 + t * 0.13 + bass).sin() * 0.8;
            let ang = field * PI;
            let local = speed * (0.6 + 0.8 * p.seed);
            if let Some(((ox, oy), force)) = burst {
                let (dx, dy) = (p.x - ox, p.y - oy);
                let d = (dx * dx + dy * dy).sqrt().max(0.02);
                let push = force * 0.25 / (0.15 + d);
                p.vx += dx / d * push;
                p.vy += dy / d * push;
            }
            p.vx *= damp;
            p.vy *= damp;
            p.x += (ang.cos() * local + p.vx) * dt;
            p.y += (ang.sin() * local + p.vy) * dt;
            p.life -= dt;
            p.x += jitter * dt * (p.seed * 97.0 + t * 7.0).sin();
            p.y += jitter * dt * (p.seed * 61.0 + t * 9.0).cos();
        }
        for p in parts.iter_mut() {
            let gone =
                p.life <= 0.0 || p.x < -0.05 || p.x > aspect + 0.05 || p.y < -0.05 || p.y > 1.05;
            if gone {
                *p = self.spawn(aspect, cfg.particles_life);
            }
        }

        let custom = palette::custom(cfg);
        let hw = (f.ppp * 0.75 * cfg.particles_size).max(0.4);
        for p in &parts {
            let (dx, dy) = (p.x - p.px, p.y - p.py);
            let vel = (dx * dx + dy * dy).sqrt() / dt.max(1e-3);
            // Aparece y se apaga suave al nacer y al morir.
            let age = 1.0 - p.life / p.max_life;
            let alpha = (age * 6.0).min(1.0) * (p.life / p.max_life * 4.0).min(1.0);
            let tone = self.hue + p.seed * 0.35 + (vel * 2.0).min(0.3);
            let rgb = match &custom {
                Some(colors) => palette::cycle3(colors, tone),
                None => palette::sample(palette::ALCHEMY, tone.rem_euclid(1.0)),
            };
            let c = scale_rgb(rgb, alpha * (0.5 + 0.8 * level + (vel * 4.0).min(1.5)));
            push_segment(
                out,
                (p.px / aspect * w, p.py * h),
                (p.x / aspect * w, p.y * h),
                c,
                c,
                hw,
            );
        }
        self.particles = parts;

        FeedbackParams {
            blur: 0.5,
            inject: 0.8,
            ..self.feedback(cfg.fx_trail)
        }
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
