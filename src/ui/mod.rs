//! Interfaz del visualizador: un dock flotante abajo, estilo transport de
//! DAW, con un cajón de ajustes por pestañas que sube desde él.
//!
//! La UI solo muta la configuración y devuelve [`MenuAction`]s; los efectos
//! sobre la ventana (guardar, cerrar, cambiar de modo) los ejecuta `app.rs`.

mod describe;
pub mod dock;
pub mod history;
mod theme;
mod widgets;

use crate::config::{VisualPreset, WindowMode};
use crate::preset::builtin_name;

/// Pestañas del cajón de ajustes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MenuTab {
    /// Ajustes propios del motor del preset activo.
    #[default]
    Preset,
    Light,
    /// Color, espejo y tiempo del preset activo.
    Color,
    Grid,
    /// Reacción al audio del preset activo (ganancia, golpes…).
    Reaction,
    /// Presets propios: crear, duplicar, renombrar, exportar.
    Library,
    /// Galería local de presets importados.
    Market,
    /// Modo aleatorio: qué escenas y cada cuánto.
    Shuffle,
    Window,
    Audio,
}

/// Los dos grupos del cajón: lo que guarda cada escena y lo de la app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabGroup {
    Scene,
    General,
}

impl MenuTab {
    /// Ajustes que guarda cada escena (preset).
    pub const SCENE: [Self; 5] = [
        Self::Preset,
        Self::Light,
        Self::Color,
        Self::Grid,
        Self::Reaction,
    ];
    /// Ajustes de la app, iguales para todas las escenas.
    pub const GENERAL: [Self; 5] = [
        Self::Shuffle,
        Self::Library,
        Self::Market,
        Self::Window,
        Self::Audio,
    ];

    pub fn group(self) -> TabGroup {
        if Self::SCENE.contains(&self) {
            TabGroup::Scene
        } else {
            TabGroup::General
        }
    }

    pub fn tabs(group: TabGroup) -> &'static [Self] {
        match group {
            TabGroup::Scene => &Self::SCENE,
            TabGroup::General => &Self::GENERAL,
        }
    }

    pub fn label(self, preset: VisualPreset) -> &'static str {
        match self {
            Self::Preset => builtin_name(preset),
            Self::Light => "Luz",
            Self::Color => "Color",
            Self::Grid => "Mosaico",
            Self::Reaction => "Reacción",
            Self::Library => "Mis presets",
            Self::Market => "Marketplace",
            Self::Shuffle => "Aleatorio",
            Self::Window => "Ventana",
            Self::Audio => "Fuente de audio",
        }
    }

    /// Pestañas con ajustes que "Restablecer" sabe devolver a fábrica.
    pub fn resettable(self) -> bool {
        !matches!(self, Self::Library | Self::Market)
    }
}

/// Efectos que la UI pide a la aplicación.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MenuAction {
    /// Cerrar el dock (y guardar la config).
    Close,
    Save,
    /// Restablecer solo los ajustes de la pestaña indicada.
    Reset(MenuTab),
    /// Deshacer el último cambio.
    Undo,
    /// Ocultar la ventana del visualizador.
    Hide,
    Quit,
    SetMode(WindowMode),
    /// Cambió "seguir a Spotify": hay que recolocar la ventana.
    FollowChanged,
    /// Cambió "siempre visible": hay que reaplicar el z-order.
    TopmostChanged,
    /// Activar el preset con este id.
    SelectPreset(String),
    /// Crear un preset propio con los valores de fábrica de un motor.
    NewPreset(VisualPreset),
    DuplicatePreset(String),
    DeletePreset(String),
    /// Guardar el preset en un archivo para compartirlo.
    ExportPreset(String),
    /// Elegir archivos de preset e instalarlos en la galería.
    ImportPresets,
    /// Abrir la carpeta de la galería en el Explorador.
    OpenMarketFolder,
    /// Releer la carpeta de la galería.
    RescanMarket,
    /// Modo aleatorio: pasar ya a la siguiente escena.
    ShuffleNext,
    /// Buscar una versión nueva ahora.
    CheckUpdate,
    /// Descargar e instalar la versión disponible.
    ApplyUpdate,
}
