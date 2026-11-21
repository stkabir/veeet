use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowMode {
    #[default]
    Windowed,
    Fullscreen,
    Docked,
    Overlay,
}

impl WindowMode {
    pub fn next(self) -> Self {
        match self {
            Self::Windowed => Self::Fullscreen,
            Self::Fullscreen => Self::Windowed,
            Self::Docked | Self::Overlay => Self::Windowed,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Windowed => "ventana",
            Self::Fullscreen => "pantalla completa",
            Self::Docked => "acoplado (avanzado)",
            Self::Overlay => "flotante (avanzado)",
        }
    }

    pub fn is_advanced(self) -> bool {
        matches!(self, Self::Docked | Self::Overlay)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualPreset {
    /// Los presets retirados cargan como la onda, así una config vieja no
    /// pierde el resto de los ajustes.
    #[default]
    #[serde(
        alias = "bars",
        alias = "radial",
        alias = "ocean_mist",
        alias = "battery",
        alias = "musical_colors"
    )]
    WavyForm3,
    /// La onda de Vizz envuelta en un círculo centrado.
    WaveRing,
    /// Recreación original inspirada en Alchemy de Windows Media Player.
    Alchemy,
    /// Recreación original inspirada en Ambience de Windows Media Player.
    /// Huracán se retiró: sus presets cargan como Ambiente.
    #[serde(alias = "hurricane")]
    Ambience,
    /// Recreación original inspirada en Scope (Bars and Waves) de WMP.
    Scope,
    /// Recreación original inspirada en Ambience · Niagara de WMP: marcas en
    /// espejo nacen abajo y suben dejando estelas con escalones.
    #[serde(alias = "lightning")]
    Niagara,
    /// Recreación original inspirada en Ambience · Windmill de WMP: una franja
    /// gira como aspa y barre una nube luminosa.
    Windmill,
    /// Una X de ondas a pantalla completa que gira y deja una estela
    /// líquida que llena la pantalla (inspirada en "X Marks the Spot" de WMP).
    #[serde(rename = "xmarks")]
    XMarks,
    /// Partículas que siguen un campo de flujo y estallan en el beat.
    Particles,
}

impl VisualPreset {
    /// Clave estable del motor (nombre de archivo e id de los presets base).
    pub fn key(self) -> &'static str {
        match self {
            Self::WavyForm3 => "wavy_form3",
            Self::WaveRing => "wave_ring",
            Self::Alchemy => "alchemy",
            Self::Ambience => "ambience",
            Self::Scope => "scope",
            Self::Niagara => "niagara",
            Self::Windmill => "windmill",
            Self::XMarks => "xmarks",
            Self::Particles => "particles",
        }
    }

    /// Motor por nombre o alias (variable `VEEET_PRESET`).
    pub fn from_alias(name: &str) -> Option<Self> {
        Some(match name.trim().to_ascii_lowercase().as_str() {
            "wave" | "onda" | "wavy_form3" | "wavy" => Self::WavyForm3,
            "ring" | "anillo" | "wave_ring" => Self::WaveRing,
            "alchemy" | "alquimia" => Self::Alchemy,
            "ambience" | "ambiente" | "hurricane" | "huracan" => Self::Ambience,
            "scope" | "trazo" => Self::Scope,
            "niagara" | "lightning" | "rayos" => Self::Niagara,
            "windmill" | "molino" => Self::Windmill,
            "xmarks" | "x" | "xmarca" | "x_marks" => Self::XMarks,
            "particles" | "particulas" | "partículas" => Self::Particles,
            _ => return None,
        })
    }

    pub fn all() -> [Self; 9] {
        [
            Self::WavyForm3,
            Self::WaveRing,
            Self::Alchemy,
            Self::Ambience,
            Self::Scope,
            Self::Niagara,
            Self::Windmill,
            Self::XMarks,
            Self::Particles,
        ]
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DockSide {
    #[default]
    Auto,
    Left,
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureMode {
    /// Sigue a la app que esté sonando (Spotify, navegador…). Las configs
    /// viejas guardaban `process` (solo Spotify); pasan a automático.
    #[default]
    #[serde(alias = "process")]
    Auto,
    /// Solo la app indicada en `AudioCfg::process`, elegida a mano.
    App,
    /// Todo lo que suena en la salida por defecto.
    Device,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowCfg {
    /// Ancho de la ventana acoplada (modo avanzado), en puntos lógicos.
    pub dock_width: f32,
    /// Separación entre Spotify y el visualizador, en puntos lógicos.
    pub dock_gap: f32,
    pub dock_side: DockSide,
    /// Mantener la ventana normal siempre encima.
    pub always_on_top: bool,
    pub overlay_click_through: bool,
    pub hide_when_spotify_minimized: bool,
    /// Opacidad del overlay (0 = invisible, 1 = opaco).
    pub overlay_opacity: f32,
    /// Proceso cuya ventana se usa como referencia para acoplar el visualizador.
    pub target_process: String,
    /// En ventana normal, seguir a Spotify con la barra de título.
    pub follow_spotify: bool,
    /// Geometría recordada de la ventana normal, en puntos lógicos.
    pub windowed_width: f32,
    pub windowed_height: f32,
    pub windowed_x: Option<f32>,
    pub windowed_y: Option<f32>,
    /// Ocultar la interfaz y el cursor en pantalla completa tras inactividad.
    pub ui_auto_hide: bool,
}

impl Default for WindowCfg {
    fn default() -> Self {
        Self {
            dock_width: 420.0,
            dock_gap: 0.0,
            dock_side: DockSide::Auto,
            always_on_top: false,
            overlay_click_through: true,
            hide_when_spotify_minimized: true,
            overlay_opacity: 0.55,
            target_process: "Spotify.exe".to_string(),
            follow_spotify: false,
            windowed_width: 1100.0,
            windowed_height: 650.0,
            windowed_x: None,
            windowed_y: None,
            ui_auto_hide: true,
        }
    }
}

/// Rangos válidos de cada ajuste. Los usan [`VisualCfg::clamp`] y los
/// sliders del menú, así nunca discrepan.
pub mod range {
    use std::ops::RangeInclusive;

    type F = RangeInclusive<f32>;
    type U = RangeInclusive<u32>;

    // Comunes
    pub const SENSITIVITY: F = 0.0..=20.0;
    pub const UNIT: F = 0.0..=1.0;
    pub const GLOW: F = 0.0..=6.0;
    pub const EXPOSURE: F = 0.1..=3.0;
    pub const LINE_WIDTH: F = 0.2..=10.0;
    pub const GAIN: F = 0.0..=5.0;
    pub const TIME_SCALE: F = 0.0..=4.0;
    pub const SATURATION: F = 0.0..=2.0;
    pub const CONTRAST: F = 0.25..=2.0;
    pub const SCENE_SCALE: F = 0.25..=3.0;
    pub const OFFSET: F = -0.5..=0.5;
    pub const COLOR_CYCLE: F = 0.0..=4.0;
    pub const TRAIL: F = 0.5..=0.995;
    pub const GRID_ROWS: U = 1..=12;

    // Onda
    pub const LINE_COUNT: U = 1..=32;
    pub const LINE_SPACING: F = 0.0..=1.0;
    pub const VARIATION: F = 0.0..=10.0;
    pub const VARIATION_SPEED: F = 0.0..=5.0;
    pub const CAMERA_Z: F = 0.3..=10.0;
    pub const WAVE_WIDTH: F = 0.2..=4.0;

    // Anillo
    pub const RING_RADIUS: F = 0.05..=1.5;
    pub const RING_ROTATION: F = -1.0..=1.0;
    pub const RING_BANDS: U = 1..=16;
    pub const RING_INNER: F = 0.0..=0.95;
    pub const RING_AMPLITUDE: F = 0.0..=0.3;
    pub const RING_FLOW: F = 0.0..=3.0;
    pub const RING_STEPS: U = 24..=1024;

    // Alquimia
    pub const ALCHEMY_SPEED: F = 0.0..=4.0;
    pub const ALCHEMY_COMPLEXITY: U = 1..=12;
    pub const ALCHEMY_SIZE: F = 0.2..=3.0;
    pub const ALCHEMY_PARTICLES: U = 0..=60;
    pub const ALCHEMY_MODE_TIME: F = 3.0..=120.0;
    pub const ALCHEMY_MOTION: F = 0.0..=3.0;

    // Ambiente
    pub const AMBIENCE_SPEED: F = 0.0..=4.0;
    pub const AMBIENCE_SCALE: F = 0.2..=6.0;
    pub const AMBIENCE_BRIGHTNESS: F = 0.0..=3.0;
    pub const AMBIENCE_MORPH: F = 0.0..=3.0;
    pub const AMBIENCE_DENSITY: F = 0.0..=2.0;
    pub const AMBIENCE_WARP: F = 0.0..=3.0;

    // Scope
    pub const SCOPE_HEIGHT: F = 0.0..=1.0;
    pub const SCOPE_EXPAND: F = 0.97..=1.04;
    pub const SCOPE_BLUR: F = 0.0..=3.0;

    // Niagara
    pub const NIAGARA_SPEED: F = 0.0..=5.0;
    pub const NIAGARA_EMIT: F = 0.02..=0.5;
    pub const NIAGARA_SQUEEZE: F = 0.8..=0.995;
    pub const NIAGARA_STEPS: U = 2..=48;
    pub const NIAGARA_LINE: F = 0.5..=20.0;

    // Molino
    pub const WINDMILL_SPEED: F = -5.0..=5.0;
    pub const WINDMILL_WIDTH: F = 0.0..=0.5;
    pub const WINDMILL_JOIN: F = 0.0..=1.0;
    pub const WINDMILL_EDGE_GAP: F = 0.0..=0.1;
    pub const WINDMILL_EDGE_LINES: U = 1..=4;
    pub const WINDMILL_BLUR: F = 0.0..=5.0;
    pub const WINDMILL_AMPLITUDE: F = 0.0..=0.3;

    // X Marca
    pub const XMARKS_SPIN: F = -2.0..=2.0;
    pub const XMARKS_SPEED: F = 0.0..=4.0;
    pub const XMARKS_CURVE: F = 0.0..=3.0;
    pub const XMARKS_AMPLITUDE: F = 0.0..=0.4;
    pub const XMARKS_ARMS: U = 2..=6;
    pub const XMARKS_ROAD: F = 0.0..=2.0;

    // Partículas
    pub const PARTICLES_COUNT: U = 200..=6000;
    pub const PARTICLES_SPEED: F = 0.0..=4.0;
    pub const PARTICLES_BURST: F = 0.0..=4.0;
    pub const PARTICLES_SCALE: F = 0.2..=6.0;
    pub const PARTICLES_SIZE: F = 0.3..=4.0;
    pub const PARTICLES_LIFE: F = 0.5..=10.0;
}

/// De dónde salen los colores de un preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ColorMode {
    /// La paleta propia del motor.
    #[default]
    Original,
    /// Degradado cíclico entre `color_a`, `color_b` y `color_c`.
    Custom,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VisualCfg {
    /// Motor del preset.
    pub preset: VisualPreset,
    /// Sensibilidad de Vizz: multiplica la amplitud visual y el eje Y.
    pub sensitivity: f32,
    /// Cuánto influye el espectro en la forma y el color (0..1).
    pub freq_influence: f32,
    /// Fuerza del bloom de la onda (UnrealBloom strength).
    pub glow: f32,
    /// Exposición final antes del recorte a blanco.
    pub exposure: f32,
    /// Motion smoothing de Vizz (0..1).
    pub smoothing: f32,
    /// Grosor de la línea en puntos lógicos (Vizz usa 1 px).
    pub line_width: f32,
    /// Cantidad de líneas de Vizz. En 2D quedan superpuestas.
    pub line_count: u32,
    /// Separación en Z de las líneas (no visible en 2D).
    pub line_spacing: f32,
    /// Variación orgánica por línea (0 en Vizz #3).
    pub line_variation: f32,
    /// Velocidad de la variación orgánica.
    pub variation_speed: f32,
    /// Distancia de cámara del preset de Vizz. Menor = más zoom.
    pub camera_z: f32,
    /// Ancho de la onda en unidades de escena (Vizz: 1.5).
    pub wave_width: f32,

    /// Activar el post-proceso Grid Tile.
    pub grid_tile: bool,
    /// Filas autoradas de Grid Tile (Vizz adapta según la altura).
    pub grid_tile_rows: u32,
    /// Solapamiento entre celdas de Grid Tile.
    pub grid_tile_overlap: f32,
    /// Opacidad de las copias de Grid Tile.
    pub grid_tile_opacity: f32,

    /// Radio exterior del anillo como fracción de la mitad del lado menor.
    pub ring_radius: f32,
    /// Rotación del anillo (vueltas por segundo; negativo gira al revés).
    pub ring_rotation: f32,
    /// Cantidad de anillos, uno por rango de frecuencia (graves al centro).
    pub ring_bands: u32,
    /// Persistencia de la estela líquida del anillo.
    pub ring_trail: f32,
    /// Radio del anillo interior como fracción del exterior.
    pub ring_inner: f32,
    /// Amplitud de la onda sobre cada anillo (fracción del lado menor).
    pub ring_amplitude: f32,
    /// Fuerza del flujo líquido de la estela (zoom, remolino, ondas).
    pub ring_flow: f32,
    /// Puntos por anillo: más = curva más suave.
    pub ring_steps: u32,

    /// Persistencia de la imagen anterior en el feedback de Alquimia.
    pub alchemy_trail: f32,
    /// Velocidad del movimiento y del ciclo de modos de Alquimia.
    pub alchemy_speed: f32,
    /// Cantidad de curvas que dibuja Alquimia.
    pub alchemy_complexity: u32,
    /// Permite el modo caleidoscopio en el ciclo de Alquimia.
    pub alchemy_kaleido: bool,
    /// Fuerza del bloom propia de Alquimia (el feedback ya acumula luz).
    pub alchemy_glow: f32,
    /// Tamaño de las curvas.
    pub alchemy_size: f32,
    /// Partículas en órbita.
    pub alchemy_particles: u32,
    /// Estrella de rayos en cada golpe.
    pub alchemy_star: bool,
    /// Segundos promedio en cada modo del ciclo.
    pub alchemy_mode_time: f32,
    /// Fuerza del zoom y la rotación del feedback.
    pub alchemy_motion: f32,

    /// Velocidad del flujo de las nubes de Ambiente.
    pub ambience_speed: f32,
    /// Escala de las nubes: mayor = formas más pequeñas.
    pub ambience_scale: f32,
    /// Fuerza del bloom propia de Ambiente.
    pub ambience_glow: f32,
    /// Brillo de las nubes.
    pub ambience_brightness: f32,
    /// Cuánto cambia de forma el gas en cada golpe.
    pub ambience_morph: f32,
    /// Cuánto gas llena la pantalla: menos deja más huecos oscuros.
    pub ambience_density: f32,
    /// Turbulencia: cuánto se retuerce el gas.
    pub ambience_warp: f32,

    /// Persistencia de la estela de Scope, Niagara, Windmill, X Marca y
    /// Partículas.
    pub fx_trail: f32,
    /// Bloom de Anillo, Scope, Niagara, Windmill, X Marca y Partículas.
    pub fx_glow: f32,

    /// Alto de la onda de Scope (fracción del alto).
    pub scope_height: f32,
    /// Cuánto se abren las copias anteriores por cuadro (1 = nada).
    pub scope_expand: f32,
    /// Desenfoque de la estela de Scope.
    pub scope_blur: f32,
    /// Dibuja también la onda reflejada (arriba y abajo en espejo).
    pub scope_mirror: bool,

    /// Velocidad con la que sube Niagara.
    pub niagara_speed: f32,
    /// Distancia de las líneas nuevas al centro (fracción del ancho).
    pub niagara_emit: f32,
    /// Contracción hacia el centro por cuadro (menor = más rápido).
    pub niagara_squeeze: f32,
    /// Escalones de cada línea.
    pub niagara_steps: u32,
    /// Grosor de las líneas.
    pub niagara_line: f32,
    /// Emitir según el volumen en vez de solo en los golpes.
    pub niagara_continuous: bool,

    /// Velocidad de giro del aspa de Windmill (negativo gira al revés).
    pub windmill_speed: f32,
    /// Media anchura de la franja (fracción del alto): junta o separa las
    /// aspas que giran. En 0 quedan pegadas y no hay franja negra.
    pub windmill_width: f32,
    /// 0 = las ondas de los bordes van en espejo (se abren entre sí);
    /// 1 = siguen la misma onda y viajan juntas.
    pub windmill_join: f32,
    /// Separación entre las líneas de cada borde.
    pub windmill_edge_gap: f32,
    /// Líneas por borde.
    pub windmill_edge_lines: u32,
    /// Difusión de la nube.
    pub windmill_blur: f32,
    /// Amplitud de la onda en los bordes.
    pub windmill_amplitude: f32,

    /// Giro de la X (vueltas por segundo × 0.1; negativo gira al revés).
    pub xmarks_spin: f32,
    /// Flujo del líquido: qué tan rápido se expande y corre la estela.
    pub xmarks_speed: f32,
    /// Ondulación y remolino del líquido.
    pub xmarks_curve: f32,
    /// Amplitud de la onda en los brazos de la X (fracción del lado menor).
    pub xmarks_amplitude: f32,
    /// Líneas que forman la figura: 2 = X, 3 = asterisco, 4 = estrella…
    pub xmarks_arms: u32,
    /// Pulso: empujón del líquido en cada golpe (0 lo apaga).
    pub xmarks_road: f32,

    /// Cantidad de partículas.
    pub particles_count: u32,
    /// Velocidad del flujo.
    pub particles_speed: f32,
    /// Fuerza de la explosión en cada golpe.
    pub particles_burst: f32,
    /// Escala del campo de flujo: mayor = remolinos más chicos.
    pub particles_scale: f32,
    /// Grosor de cada partícula.
    pub particles_size: f32,
    /// Vida media de cada partícula en segundos.
    pub particles_life: f32,

    // --- Reactividad ---
    /// Multiplicador de ganancia aplicado a la onda.
    pub gain: f32,
    /// Suavizado temporal del espectro (smoothingTimeConstant de Web Audio).
    pub spectrum_smoothing: f32,
    /// 0 = insensible, 1 = muy sensible.
    pub beat_sensitivity: f32,

    // --- Tiempo ---
    /// Multiplicador del reloj de la escena: 0 congela, 2 va al doble.
    pub time_scale: f32,

    // --- Bloom ---
    /// Umbral de luminancia a partir del cual brilla (UnrealBloom threshold).
    pub bloom_threshold: f32,
    /// Radio del bloom (UnrealBloom radius).
    pub bloom_radius: f32,

    // --- Colores del motor ---
    pub color_mode: ColorMode,
    pub color_a: [u8; 3],
    pub color_b: [u8; 3],
    pub color_c: [u8; 3],
    /// Velocidad con la que el color recorre la paleta.
    pub color_cycle: f32,

    // --- Encuadre (post-proceso) ---
    /// Tamaño de la escena: 2 = el doble, 0.5 = la mitad.
    pub scene_scale: f32,
    /// Desplazamiento horizontal (fracción del ancho).
    pub offset_x: f32,
    /// Desplazamiento vertical (fracción del alto).
    pub offset_y: f32,

    // --- Color (post-proceso) ---
    /// Rotación del tono, en vueltas (0..1).
    pub hue_shift: f32,
    /// Saturación: 0 = gris, 1 = original, 2 = el doble.
    pub saturation: f32,
    /// Contraste alrededor del gris medio.
    pub contrast: f32,
    /// Color que tiñe la imagen.
    pub tint: [u8; 3],
    /// Cuánto tiñe `tint` (0 = nada, 1 = todo).
    pub tint_amount: f32,
    /// Color del fondo donde no hay luz.
    pub background: [u8; 3],
    /// Oscurecimiento de los bordes.
    pub vignette: f32,
    /// Espejo horizontal: la mitad izquierda se refleja a la derecha.
    pub mirror_x: bool,
    /// Espejo vertical: la mitad de arriba se refleja abajo.
    pub mirror_y: bool,
}

impl Default for VisualCfg {
    fn default() -> Self {
        Self {
            preset: VisualPreset::WavyForm3,
            sensitivity: 1.5,
            freq_influence: 1.0,
            glow: 2.0,
            exposure: 1.0,
            smoothing: 0.95,
            line_width: 1.0,
            line_count: 2,
            line_spacing: 0.002,
            line_variation: 0.0,
            variation_speed: 0.15,
            camera_z: 1.564_709,
            wave_width: 1.5,
            grid_tile: true,
            grid_tile_rows: 3,
            grid_tile_overlap: 0.34,
            grid_tile_opacity: 0.10,
            ring_radius: 0.32,
            ring_rotation: 0.02,
            ring_bands: 5,
            ring_trail: 0.88,
            ring_inner: 0.28,
            ring_amplitude: 0.05,
            ring_flow: 1.0,
            ring_steps: 256,
            alchemy_trail: 0.95,
            alchemy_speed: 1.0,
            alchemy_complexity: 3,
            alchemy_kaleido: true,
            alchemy_glow: 0.6,
            alchemy_size: 1.0,
            alchemy_particles: 9,
            alchemy_star: true,
            alchemy_mode_time: 12.0,
            alchemy_motion: 1.0,
            ambience_speed: 0.6,
            ambience_scale: 1.4,
            ambience_glow: 0.8,
            ambience_brightness: 1.0,
            ambience_morph: 1.0,
            ambience_density: 1.0,
            ambience_warp: 1.0,
            fx_trail: 0.93,
            fx_glow: 0.9,
            scope_height: 0.32,
            scope_expand: 1.006,
            scope_blur: 0.8,
            scope_mirror: false,
            niagara_speed: 1.0,
            niagara_emit: 0.406,
            niagara_squeeze: 0.93,
            niagara_steps: 9,
            niagara_line: 5.0,
            niagara_continuous: false,
            windmill_speed: 1.0,
            windmill_width: 0.095,
            windmill_join: 0.0,
            windmill_edge_gap: 0.012,
            windmill_edge_lines: 2,
            windmill_blur: 2.2,
            windmill_amplitude: 0.05,
            xmarks_spin: 0.25,
            xmarks_speed: 1.0,
            xmarks_curve: 1.0,
            xmarks_amplitude: 0.08,
            xmarks_arms: 2,
            xmarks_road: 1.0,
            particles_count: 2500,
            particles_speed: 1.0,
            particles_burst: 1.0,
            particles_scale: 1.5,
            particles_size: 1.0,
            particles_life: 4.0,
            gain: 1.0,
            spectrum_smoothing: 0.8,
            beat_sensitivity: 0.5,
            time_scale: 1.0,
            bloom_threshold: 0.2,
            bloom_radius: 0.3,
            color_mode: ColorMode::Original,
            color_a: [5, 158, 245],
            color_b: [107, 255, 77],
            color_c: [255, 66, 117],
            color_cycle: 1.0,
            scene_scale: 1.0,
            offset_x: 0.0,
            offset_y: 0.0,
            hue_shift: 0.0,
            saturation: 1.0,
            contrast: 1.0,
            tint: [255, 255, 255],
            tint_amount: 0.0,
            background: [0, 0, 0],
            vignette: 0.0,
            mirror_x: false,
            mirror_y: false,
        }
    }
}

/// `v` dentro de `range`; un NaN cae en `fallback`.
fn clampf(v: &mut f32, range: std::ops::RangeInclusive<f32>, fallback: f32) {
    *v = if v.is_finite() {
        v.clamp(*range.start(), *range.end())
    } else {
        fallback
    };
}

fn clampu(v: &mut u32, range: std::ops::RangeInclusive<u32>) {
    *v = (*v).clamp(*range.start(), *range.end());
}

/// Ajustes que usan todos los motores (post-proceso, audio, colores).
pub const COMMON_KEYS: &[&str] = &[
    "preset",
    "exposure",
    "smoothing",
    "freq_influence",
    "gain",
    "spectrum_smoothing",
    "beat_sensitivity",
    "time_scale",
    "bloom_threshold",
    "bloom_radius",
    "color_mode",
    "color_a",
    "color_b",
    "color_c",
    "color_cycle",
    "scene_scale",
    "offset_x",
    "offset_y",
    "hue_shift",
    "saturation",
    "contrast",
    "tint",
    "tint_amount",
    "background",
    "vignette",
    "mirror_x",
    "mirror_y",
    "grid_tile",
    "grid_tile_rows",
    "grid_tile_overlap",
    "grid_tile_opacity",
];

/// Pestaña Luz.
pub const LIGHT_KEYS: &[&str] = &[
    "exposure",
    "glow",
    "alchemy_glow",
    "ambience_glow",
    "fx_glow",
    "bloom_threshold",
    "bloom_radius",
    "contrast",
    "vignette",
];

/// Pestaña Color.
pub const COLOR_KEYS: &[&str] = &[
    "color_mode",
    "color_a",
    "color_b",
    "color_c",
    "color_cycle",
    "scene_scale",
    "offset_x",
    "offset_y",
    "hue_shift",
    "saturation",
    "tint",
    "tint_amount",
    "background",
    "mirror_x",
    "mirror_y",
    "time_scale",
];

/// Pestaña Mosaico.
pub const GRID_KEYS: &[&str] = &[
    "grid_tile",
    "grid_tile_rows",
    "grid_tile_overlap",
    "grid_tile_opacity",
];

/// Reacción al audio (pestaña Audio).
pub const AUDIO_KEYS: &[&str] = &[
    "gain",
    "spectrum_smoothing",
    "beat_sensitivity",
    "freq_influence",
];

impl VisualPreset {
    /// Ajustes propios del motor, además de [`COMMON_KEYS`]. Lo demás no se
    /// usa y no se guarda en el archivo del preset.
    pub fn keys(self) -> &'static [&'static str] {
        match self {
            Self::WavyForm3 => &[
                "sensitivity",
                "glow",
                "line_width",
                "line_count",
                "line_spacing",
                "line_variation",
                "variation_speed",
                "camera_z",
                "wave_width",
            ],
            Self::WaveRing => &[
                "sensitivity",
                "fx_glow",
                "line_width",
                "ring_radius",
                "ring_rotation",
                "ring_bands",
                "ring_trail",
                "ring_inner",
                "ring_amplitude",
                "ring_flow",
                "ring_steps",
            ],
            Self::Alchemy => &[
                "alchemy_trail",
                "alchemy_speed",
                "alchemy_complexity",
                "alchemy_kaleido",
                "alchemy_glow",
                "alchemy_size",
                "alchemy_particles",
                "alchemy_star",
                "alchemy_mode_time",
                "alchemy_motion",
            ],
            Self::Ambience => &[
                "ambience_speed",
                "ambience_scale",
                "ambience_glow",
                "ambience_brightness",
                "ambience_morph",
                "ambience_density",
                "ambience_warp",
            ],
            Self::Scope => &[
                "sensitivity",
                "fx_glow",
                "fx_trail",
                "line_width",
                "scope_height",
                "scope_expand",
                "scope_blur",
                "scope_mirror",
            ],
            Self::Niagara => &[
                "sensitivity",
                "fx_glow",
                "fx_trail",
                "niagara_speed",
                "niagara_emit",
                "niagara_squeeze",
                "niagara_steps",
                "niagara_line",
                "niagara_continuous",
            ],
            Self::Windmill => &[
                "sensitivity",
                "fx_glow",
                "fx_trail",
                "windmill_speed",
                "windmill_width",
                "windmill_join",
                "windmill_edge_gap",
                "windmill_edge_lines",
                "windmill_blur",
                "windmill_amplitude",
            ],
            Self::XMarks => &[
                "sensitivity",
                "fx_glow",
                "fx_trail",
                "line_width",
                "xmarks_spin",
                "xmarks_speed",
                "xmarks_curve",
                "xmarks_amplitude",
                "xmarks_arms",
                "xmarks_road",
            ],
            Self::Particles => &[
                "fx_glow",
                "fx_trail",
                "particles_count",
                "particles_speed",
                "particles_burst",
                "particles_scale",
                "particles_size",
                "particles_life",
            ],
        }
    }

    /// El ajuste `key` lo usa este motor.
    pub fn uses(self, key: &str) -> bool {
        COMMON_KEYS.contains(&key) || self.keys().contains(&key)
    }
}

impl VisualCfg {
    /// Valores de fábrica de un motor: solo la onda usa Grid Tile, y cada
    /// motor arranca con colores personalizados parecidos a los suyos.
    pub fn defaults_for(engine: VisualPreset) -> Self {
        let (color_a, color_b, color_c) = match engine {
            VisualPreset::WavyForm3 => ([5, 158, 245], [107, 255, 77], [255, 66, 117]),
            VisualPreset::WaveRing => ([230, 40, 180], [120, 60, 255], [40, 220, 255]),
            VisualPreset::Alchemy | VisualPreset::Scope => {
                ([255, 64, 166], [255, 158, 46], [38, 217, 255])
            }
            VisualPreset::Ambience => ([40, 20, 120], [200, 60, 200], [255, 200, 120]),
            VisualPreset::Niagara => ([3, 33, 33], [230, 90, 90], [255, 245, 245]),
            VisualPreset::Windmill => ([71, 71, 25], [156, 161, 97], [243, 246, 222]),
            VisualPreset::XMarks => ([255, 60, 120], [255, 210, 60], [60, 200, 255]),
            VisualPreset::Particles => ([60, 120, 255], [180, 80, 255], [255, 140, 200]),
        };
        Self {
            preset: engine,
            grid_tile: engine == VisualPreset::WavyForm3,
            color_a,
            color_b,
            color_c,
            ..Self::default()
        }
    }

    /// Vuelve a fábrica solo los ajustes `keys` (por nombre de campo).
    pub fn reset_keys(&mut self, keys: &[&str]) {
        let defaults = Self::defaults_for(self.preset);
        let (Ok(toml::Value::Table(mut current)), Ok(toml::Value::Table(fresh))) = (
            toml::Value::try_from(&*self),
            toml::Value::try_from(&defaults),
        ) else {
            return;
        };
        for key in keys {
            if let Some(value) = fresh.get(*key) {
                current.insert((*key).to_string(), value.clone());
            }
        }
        if let Ok(updated) = toml::Value::Table(current).try_into::<Self>() {
            *self = updated;
        }
    }

    /// Lleva cada valor a su rango válido. Se aplica a todo lo que se lee de
    /// disco, así un archivo importado nunca rompe el render.
    pub fn clamp(&mut self) {
        use range as r;
        let d = Self::default();
        clampf(&mut self.sensitivity, r::SENSITIVITY, d.sensitivity);
        clampf(&mut self.freq_influence, r::UNIT, d.freq_influence);
        clampf(&mut self.glow, r::GLOW, d.glow);
        clampf(&mut self.exposure, r::EXPOSURE, d.exposure);
        clampf(&mut self.smoothing, r::UNIT, d.smoothing);
        clampf(&mut self.line_width, r::LINE_WIDTH, d.line_width);
        clampu(&mut self.line_count, r::LINE_COUNT);
        clampf(&mut self.line_spacing, r::LINE_SPACING, d.line_spacing);
        clampf(&mut self.line_variation, r::VARIATION, d.line_variation);
        clampf(
            &mut self.variation_speed,
            r::VARIATION_SPEED,
            d.variation_speed,
        );
        clampf(&mut self.camera_z, r::CAMERA_Z, d.camera_z);
        clampf(&mut self.wave_width, r::WAVE_WIDTH, d.wave_width);
        clampu(&mut self.grid_tile_rows, r::GRID_ROWS);
        clampf(&mut self.grid_tile_overlap, r::UNIT, d.grid_tile_overlap);
        clampf(&mut self.grid_tile_opacity, r::UNIT, d.grid_tile_opacity);
        clampf(&mut self.ring_radius, r::RING_RADIUS, d.ring_radius);
        clampf(&mut self.ring_rotation, r::RING_ROTATION, d.ring_rotation);
        clampu(&mut self.ring_bands, r::RING_BANDS);
        clampf(&mut self.ring_trail, r::TRAIL, d.ring_trail);
        clampf(&mut self.ring_inner, r::RING_INNER, d.ring_inner);
        clampf(
            &mut self.ring_amplitude,
            r::RING_AMPLITUDE,
            d.ring_amplitude,
        );
        clampf(&mut self.ring_flow, r::RING_FLOW, d.ring_flow);
        clampu(&mut self.ring_steps, r::RING_STEPS);
        clampf(&mut self.alchemy_trail, r::TRAIL, d.alchemy_trail);
        clampf(&mut self.alchemy_speed, r::ALCHEMY_SPEED, d.alchemy_speed);
        clampu(&mut self.alchemy_complexity, r::ALCHEMY_COMPLEXITY);
        clampf(&mut self.alchemy_glow, r::GLOW, d.alchemy_glow);
        clampf(&mut self.alchemy_size, r::ALCHEMY_SIZE, d.alchemy_size);
        clampu(&mut self.alchemy_particles, r::ALCHEMY_PARTICLES);
        clampf(
            &mut self.alchemy_mode_time,
            r::ALCHEMY_MODE_TIME,
            d.alchemy_mode_time,
        );
        clampf(
            &mut self.alchemy_motion,
            r::ALCHEMY_MOTION,
            d.alchemy_motion,
        );
        clampf(
            &mut self.ambience_speed,
            r::AMBIENCE_SPEED,
            d.ambience_speed,
        );
        clampf(
            &mut self.ambience_scale,
            r::AMBIENCE_SCALE,
            d.ambience_scale,
        );
        clampf(&mut self.ambience_glow, r::GLOW, d.ambience_glow);
        clampf(
            &mut self.ambience_brightness,
            r::AMBIENCE_BRIGHTNESS,
            d.ambience_brightness,
        );
        clampf(
            &mut self.ambience_morph,
            r::AMBIENCE_MORPH,
            d.ambience_morph,
        );
        clampf(&mut self.fx_trail, r::TRAIL, d.fx_trail);
        clampf(&mut self.fx_glow, r::GLOW, d.fx_glow);
        clampf(&mut self.scope_height, r::SCOPE_HEIGHT, d.scope_height);
        clampf(&mut self.scope_expand, r::SCOPE_EXPAND, d.scope_expand);
        clampf(&mut self.scope_blur, r::SCOPE_BLUR, d.scope_blur);
        clampf(&mut self.niagara_speed, r::NIAGARA_SPEED, d.niagara_speed);
        clampf(&mut self.niagara_emit, r::NIAGARA_EMIT, d.niagara_emit);
        clampf(
            &mut self.niagara_squeeze,
            r::NIAGARA_SQUEEZE,
            d.niagara_squeeze,
        );
        clampu(&mut self.niagara_steps, r::NIAGARA_STEPS);
        clampf(&mut self.niagara_line, r::NIAGARA_LINE, d.niagara_line);
        clampf(
            &mut self.windmill_speed,
            r::WINDMILL_SPEED,
            d.windmill_speed,
        );
        clampf(
            &mut self.windmill_width,
            r::WINDMILL_WIDTH,
            d.windmill_width,
        );
        clampf(
            &mut self.windmill_edge_gap,
            r::WINDMILL_EDGE_GAP,
            d.windmill_edge_gap,
        );
        clampu(&mut self.windmill_edge_lines, r::WINDMILL_EDGE_LINES);
        clampf(&mut self.windmill_blur, r::WINDMILL_BLUR, d.windmill_blur);
        clampf(
            &mut self.windmill_amplitude,
            r::WINDMILL_AMPLITUDE,
            d.windmill_amplitude,
        );
        clampf(&mut self.windmill_join, r::WINDMILL_JOIN, d.windmill_join);
        clampf(&mut self.xmarks_spin, r::XMARKS_SPIN, d.xmarks_spin);
        clampf(&mut self.xmarks_speed, r::XMARKS_SPEED, d.xmarks_speed);
        clampf(&mut self.xmarks_curve, r::XMARKS_CURVE, d.xmarks_curve);
        clampf(
            &mut self.xmarks_amplitude,
            r::XMARKS_AMPLITUDE,
            d.xmarks_amplitude,
        );
        clampu(&mut self.particles_count, r::PARTICLES_COUNT);
        clampu(&mut self.xmarks_arms, r::XMARKS_ARMS);
        clampf(&mut self.xmarks_road, r::XMARKS_ROAD, d.xmarks_road);
        clampf(
            &mut self.particles_size,
            r::PARTICLES_SIZE,
            d.particles_size,
        );
        clampf(
            &mut self.particles_life,
            r::PARTICLES_LIFE,
            d.particles_life,
        );
        clampf(
            &mut self.ambience_density,
            r::AMBIENCE_DENSITY,
            d.ambience_density,
        );
        clampf(&mut self.ambience_warp, r::AMBIENCE_WARP, d.ambience_warp);
        clampf(
            &mut self.particles_speed,
            r::PARTICLES_SPEED,
            d.particles_speed,
        );
        clampf(
            &mut self.particles_burst,
            r::PARTICLES_BURST,
            d.particles_burst,
        );
        clampf(
            &mut self.particles_scale,
            r::PARTICLES_SCALE,
            d.particles_scale,
        );
        clampf(&mut self.gain, r::GAIN, d.gain);
        clampf(&mut self.spectrum_smoothing, r::UNIT, d.spectrum_smoothing);
        clampf(&mut self.beat_sensitivity, r::UNIT, d.beat_sensitivity);
        clampf(&mut self.time_scale, r::TIME_SCALE, d.time_scale);
        clampf(&mut self.bloom_threshold, r::UNIT, d.bloom_threshold);
        clampf(&mut self.bloom_radius, r::UNIT, d.bloom_radius);
        clampf(&mut self.color_cycle, r::COLOR_CYCLE, d.color_cycle);
        clampf(&mut self.scene_scale, r::SCENE_SCALE, d.scene_scale);
        clampf(&mut self.offset_x, r::OFFSET, 0.0);
        clampf(&mut self.offset_y, r::OFFSET, 0.0);
        self.hue_shift = if self.hue_shift.is_finite() {
            self.hue_shift.rem_euclid(1.0)
        } else {
            0.0
        };
        clampf(&mut self.saturation, r::SATURATION, d.saturation);
        clampf(&mut self.contrast, r::CONTRAST, d.contrast);
        clampf(&mut self.tint_amount, r::UNIT, d.tint_amount);
        clampf(&mut self.vignette, r::UNIT, d.vignette);
    }
}

/// Modo aleatorio: va cambiando de preset solo.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShuffleCfg {
    pub enabled: bool,
    /// Segundos en cada escena.
    pub interval_secs: f32,
    /// Al cumplirse el tiempo, esperar al próximo golpe (máximo 4 s).
    pub wait_for_beat: bool,
    /// Duración del fundido a negro (salida + entrada).
    pub fade_secs: f32,
    /// Incluir todos los presets, también los que se creen después.
    pub all: bool,
    /// Ids incluidos cuando `all` es falso.
    pub presets: Vec<String>,
}

impl Default for ShuffleCfg {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_secs: 45.0,
            wait_for_beat: true,
            fade_secs: 0.5,
            all: true,
            presets: Vec::new(),
        }
    }
}

pub mod shuffle_range {
    use std::ops::RangeInclusive;
    pub const INTERVAL: RangeInclusive<f32> = 5.0..=600.0;
    pub const FADE: RangeInclusive<f32> = 0.0..=3.0;
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioCfg {
    /// Ejecutable a capturar en modo `App`; en modo `Auto` es la app
    /// preferida cuando varias suenan a la vez.
    pub process: String,
    pub mode: CaptureMode,
    /// Solo lectura de configs viejas: ahora cada preset guarda los suyos
    /// (`VisualCfg::gain` y compañía).
    #[serde(skip_serializing)]
    pub gain: f32,
    #[serde(skip_serializing)]
    pub spectrum_smoothing: f32,
    #[serde(skip_serializing)]
    pub beat_sensitivity: f32,
}

impl Default for AudioCfg {
    fn default() -> Self {
        Self {
            process: "Spotify.exe".to_string(),
            mode: CaptureMode::Auto,
            gain: 1.0,
            spectrum_smoothing: 0.8,
            beat_sensitivity: 0.5,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct HotkeyCfg {
    pub toggle_visible: String,
    pub cycle_mode: String,
    pub toggle_click_through: String,
    pub toggle_debug: String,
    pub toggle_menu: String,
    pub quit: String,
}

impl Default for HotkeyCfg {
    fn default() -> Self {
        Self {
            toggle_visible: "ctrl+alt+v".to_string(),
            cycle_mode: "ctrl+alt+shift+m".to_string(),
            toggle_click_through: "ctrl+alt+t".to_string(),
            toggle_debug: "ctrl+alt+d".to_string(),
            toggle_menu: "ctrl+alt+p".to_string(),
            quit: "ctrl+alt+q".to_string(),
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: WindowMode,
    pub window: WindowCfg,
    /// Id del preset activo (ver `crate::preset`).
    pub active_preset: String,
    /// Copia de trabajo de los ajustes del preset activo. No va a
    /// config.toml: cada preset se guarda en su propio archivo.
    #[serde(skip)]
    pub visual: VisualCfg,
    /// Sección `[visual]` de las configs anteriores a los presets por
    /// archivo. Solo se lee, para migrarla.
    #[serde(rename = "visual", skip_serializing)]
    pub legacy_visual: Option<VisualCfg>,
    pub audio: AudioCfg,
    pub hotkeys: HotkeyCfg,
    /// Modo aleatorio (global, no por preset).
    pub shuffle: ShuffleCfg,
    pub update: UpdateCfg,
}

/// Actualizaciones automáticas (ver `crate::update`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateCfg {
    /// Buscar una versión nueva al abrir la app.
    pub check_on_start: bool,
}

impl Default for UpdateCfg {
    fn default() -> Self {
        Self {
            check_on_start: true,
        }
    }
}

impl Config {
    /// Carpeta de datos de Veeet (`%APPDATA%\veeet`).
    pub fn dir() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("veeet")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    /// Ruta de la configuración cuando la app se llamaba Aurora Wave.
    fn legacy_path() -> PathBuf {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("aurora-wave")
            .join("config.toml")
    }

    pub fn load() -> Self {
        let mut path = Self::path();
        // Primera vez con el nombre nuevo: se lee la config anterior y se
        // guarda luego en la ruta nueva.
        if !path.exists() && Self::legacy_path().exists() {
            path = Self::legacy_path();
        }
        match std::fs::read_to_string(&path) {
            Ok(text) => match toml::from_str::<Self>(&text) {
                Ok(cfg) => {
                    log::info!("Configuración cargada de {}", path.display());
                    cfg
                }
                Err(err) => {
                    log::warn!("Configuración inválida ({}): {err}", path.display());
                    Self::default()
                }
            },
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(err) => {
                log::warn!("No se pudo leer {}: {err}", path.display());
                Self::default()
            }
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path();
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creando {}", dir.display()))?;
        }
        let text = toml::to_string_pretty(self)?;
        std::fs::write(&path, text).with_context(|| format!("escribiendo {}", path.display()))?;
        Ok(())
    }
}
