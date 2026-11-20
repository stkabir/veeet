//! Deshacer y restablecer por pestaña.
//!
//! Solo se versiona lo que el menú puede cambiar: ajustes visuales, de audio
//! y los interruptores de ventana. La geometría de la ventana (que la app
//! actualiza sola) queda fuera, así deshacer nunca mueve la ventana.

use crate::config::{
    AUDIO_KEYS, AudioCfg, COLOR_KEYS, Config, GRID_KEYS, LIGHT_KEYS, ShuffleCfg, VisualCfg,
    WindowCfg,
};

use super::MenuTab;

/// Cambios que se recuerdan.
const MAX_UNDO: usize = 50;

/// Estado del menú que se puede deshacer.
#[derive(Clone, PartialEq)]
pub struct Snapshot {
    /// Preset activo: deshacer también vuelve a él.
    active_preset: String,
    visual: VisualCfg,
    audio: AudioCfg,
    shuffle: ShuffleCfg,
    follow_spotify: bool,
    always_on_top: bool,
    ui_auto_hide: bool,
    overlay_opacity: f32,
    overlay_click_through: bool,
}

impl Snapshot {
    pub fn of(config: &Config) -> Self {
        let w = &config.window;
        Self {
            active_preset: config.active_preset.clone(),
            visual: config.visual.clone(),
            audio: config.audio.clone(),
            shuffle: config.shuffle.clone(),
            follow_spotify: w.follow_spotify,
            always_on_top: w.always_on_top,
            ui_auto_hide: w.ui_auto_hide,
            overlay_opacity: w.overlay_opacity,
            overlay_click_through: w.overlay_click_through,
        }
    }

    fn restore(self, config: &mut Config) {
        config.active_preset = self.active_preset;
        config.visual = self.visual;
        config.audio = self.audio;
        config.shuffle = self.shuffle;
        let w = &mut config.window;
        w.follow_spotify = self.follow_spotify;
        w.always_on_top = self.always_on_top;
        w.ui_auto_hide = self.ui_auto_hide;
        w.overlay_opacity = self.overlay_opacity;
        w.overlay_click_through = self.overlay_click_through;
    }
}

/// Pila de deshacer. Un "cambio" es un gesto completo: arrastrar un slider
/// de principio a fin cuenta como uno solo.
#[derive(Default)]
pub struct History {
    undo: Vec<Snapshot>,
    /// Estado antes del gesto en curso (mientras el puntero sigue apretado).
    pending: Option<Snapshot>,
}

impl History {
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty() || self.pending.is_some()
    }

    /// Llamar una vez por frame con el estado previo a la UI y el actual.
    /// `gesture_active` = el puntero sigue apretado.
    pub fn observe(&mut self, before: Snapshot, after: &Config, gesture_active: bool) {
        if self.pending.is_none() && before != Snapshot::of(after) {
            self.pending = Some(before);
        }
        if !gesture_active && let Some(snapshot) = self.pending.take() {
            self.push(snapshot);
        }
    }

    /// Guarda un estado explícitamente (antes de restablecer).
    pub fn push(&mut self, snapshot: Snapshot) {
        if self.undo.last() != Some(&snapshot) {
            self.undo.push(snapshot);
            if self.undo.len() > MAX_UNDO {
                self.undo.remove(0);
            }
        }
    }

    /// Vuelve al estado anterior al último cambio.
    pub fn undo(&mut self, config: &mut Config) -> bool {
        let snapshot = self.pending.take().or_else(|| self.undo.pop());
        match snapshot {
            Some(s) => {
                s.restore(config);
                true
            }
            None => false,
        }
    }
}

/// Restablece solo los ajustes que muestra la pestaña `tab`, a los valores
/// de fábrica del motor del preset activo.
pub fn reset_tab(config: &mut Config, tab: MenuTab) {
    let v = &mut config.visual;
    match tab {
        MenuTab::Preset => {
            // Lo propio del motor menos el brillo, que está en Luz.
            let keys: Vec<&str> = v
                .preset
                .keys()
                .iter()
                .copied()
                .filter(|k| !LIGHT_KEYS.contains(k))
                .chain(["smoothing"])
                .collect();
            v.reset_keys(&keys);
        }
        MenuTab::Light => v.reset_keys(LIGHT_KEYS),
        MenuTab::Color => v.reset_keys(COLOR_KEYS),
        MenuTab::Grid => v.reset_keys(GRID_KEYS),
        MenuTab::Shuffle => {
            // Conserva si está encendido: restablecer no lo apaga.
            let enabled = config.shuffle.enabled;
            config.shuffle = ShuffleCfg {
                enabled,
                ..ShuffleCfg::default()
            };
        }
        MenuTab::Library | MenuTab::Market => {}
        MenuTab::Window => {
            let dw = WindowCfg::default();
            let w = &mut config.window;
            w.follow_spotify = dw.follow_spotify;
            w.always_on_top = dw.always_on_top;
            w.ui_auto_hide = dw.ui_auto_hide;
            w.overlay_opacity = dw.overlay_opacity;
            w.overlay_click_through = dw.overlay_click_through;
        }
        MenuTab::Audio => {
            let da = AudioCfg::default();
            let a = &mut config.audio;
            a.mode = da.mode;
            a.process = da.process;
            // La reactividad es del preset.
            v.reset_keys(AUDIO_KEYS);
        }
    }
}
