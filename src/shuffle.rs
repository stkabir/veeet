//! Modo aleatorio: cambia de preset solo, con un fundido a negro.
//!
//! Al cumplirse el intervalo espera al próximo golpe (como mucho
//! [`BEAT_WAIT`]), apaga la escena, cambia de preset a mitad del fundido y la
//! vuelve a encender.

use crate::config::ShuffleCfg;

/// Espera máxima a un golpe antes de cambiar igual.
const BEAT_WAIT: f32 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Phase {
    Playing,
    /// Se cumplió el tiempo: esperando el golpe.
    Waiting(f32),
    FadingOut(f32),
    FadingIn(f32),
}

pub struct Shuffle {
    phase: Phase,
    /// Segundos en la escena actual.
    elapsed: f32,
    rng: u32,
}

/// Resultado de un paso.
pub struct Step {
    /// Multiplicador de brillo (1 = normal, 0 = negro).
    pub fade: f32,
    /// Toca cambiar de preset ahora (la pantalla está en negro).
    pub switch: bool,
}

impl Shuffle {
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0x1234_5678);
        Self {
            phase: Phase::Playing,
            elapsed: 0.0,
            rng: seed | 1,
        }
    }

    /// El usuario eligió un preset a mano: el próximo cambio cuenta desde ya.
    pub fn restart(&mut self) {
        self.elapsed = 0.0;
        if matches!(self.phase, Phase::Waiting(_)) {
            self.phase = Phase::Playing;
        }
    }

    /// Pasar a la siguiente escena ya.
    pub fn skip(&mut self) {
        if matches!(self.phase, Phase::Playing | Phase::Waiting(_)) {
            self.phase = Phase::FadingOut(0.0);
        }
    }

    pub fn step(&mut self, dt: f32, cfg: &ShuffleCfg, beat: bool) -> Step {
        let half = (cfg.fade_secs * 0.5).max(0.0);
        let mut switch = false;
        if !cfg.enabled {
            // Apagado a mitad de un fundido: termina de encender.
            self.phase = match self.phase {
                Phase::FadingOut(t) => Phase::FadingIn((half - t).max(0.0)),
                Phase::Waiting(_) => Phase::Playing,
                other => other,
            };
            self.elapsed = 0.0;
        }
        self.phase = match self.phase {
            Phase::Playing => {
                self.elapsed += dt;
                if cfg.enabled && self.elapsed >= cfg.interval_secs {
                    if cfg.wait_for_beat {
                        Phase::Waiting(0.0)
                    } else {
                        Phase::FadingOut(0.0)
                    }
                } else {
                    Phase::Playing
                }
            }
            Phase::Waiting(w) => {
                let w = w + dt;
                if beat || w >= BEAT_WAIT {
                    Phase::FadingOut(0.0)
                } else {
                    Phase::Waiting(w)
                }
            }
            Phase::FadingOut(t) => {
                let t = t + dt;
                if t >= half {
                    switch = true;
                    self.elapsed = 0.0;
                    Phase::FadingIn(0.0)
                } else {
                    Phase::FadingOut(t)
                }
            }
            Phase::FadingIn(t) => {
                let t = t + dt;
                if t >= half {
                    Phase::Playing
                } else {
                    Phase::FadingIn(t)
                }
            }
        };
        let fade = match self.phase {
            Phase::FadingOut(t) if half > 0.0 => 1.0 - t / half,
            Phase::FadingIn(t) if half > 0.0 => t / half,
            Phase::FadingIn(_) => 1.0,
            Phase::FadingOut(_) => 0.0,
            Phase::Playing | Phase::Waiting(_) => 1.0,
        };
        Step {
            fade: fade.clamp(0.0, 1.0),
            switch,
        }
    }

    /// Elige al azar el próximo preset entre `candidates`, sin repetir el
    /// actual. `None` si no hay otro.
    pub fn pick<'a>(&mut self, candidates: &[&'a str], current: &str) -> Option<&'a str> {
        let others: Vec<&'a str> = candidates
            .iter()
            .copied()
            .filter(|id| *id != current)
            .collect();
        if others.is_empty() {
            return None;
        }
        Some(others[(self.random() * others.len() as f32) as usize % others.len()])
    }

    fn random(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x >> 8) as f32 / (1u32 << 24) as f32
    }
}

/// Ids que entran en el modo aleatorio, en el orden de `all_ids`.
pub fn candidates<'a>(cfg: &ShuffleCfg, all_ids: &[&'a str]) -> Vec<&'a str> {
    if cfg.all {
        all_ids.to_vec()
    } else {
        all_ids
            .iter()
            .copied()
            .filter(|id| cfg.presets.iter().any(|p| p == id))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ShuffleCfg {
        ShuffleCfg {
            enabled: true,
            interval_secs: 1.0,
            wait_for_beat: false,
            fade_secs: 0.4,
            ..ShuffleCfg::default()
        }
    }

    #[test]
    fn never_repeats_current() {
        let mut s = Shuffle::new();
        for _ in 0..200 {
            assert_eq!(s.pick(&["a", "b"], "a"), Some("b"));
        }
        assert_eq!(s.pick(&["a"], "a"), None);
    }

    #[test]
    fn candidates_skip_missing() {
        let mut c = cfg();
        c.all = false;
        c.presets = vec!["b".into(), "gone".into()];
        assert_eq!(candidates(&c, &["a", "b", "c"]), vec!["b"]);
        c.all = true;
        assert_eq!(candidates(&c, &["a", "b"]), vec!["a", "b"]);
    }

    #[test]
    fn fades_and_switches_once() {
        let c = cfg();
        let mut s = Shuffle::new();
        let dt = 0.05;
        let mut switches = 0;
        let mut min_fade: f32 = 1.0;
        for _ in 0..40 {
            let step = s.step(dt, &c, false);
            min_fade = min_fade.min(step.fade);
            if step.switch {
                switches += 1;
                assert!(step.fade < 0.01);
            }
        }
        // 2 s: un cambio a ~1.2 s (1 s + medio fundido).
        assert_eq!(switches, 1);
        assert!(min_fade < 0.01);
    }

    #[test]
    fn waits_for_beat() {
        let c = ShuffleCfg {
            wait_for_beat: true,
            ..cfg()
        };
        let mut s = Shuffle::new();
        for _ in 0..30 {
            assert!(!s.step(0.05, &c, false).switch);
            assert_eq!(s.step(0.0, &c, false).fade, 1.0);
        }
        // Llega el golpe: empieza el fundido.
        s.step(0.05, &c, true);
        assert!(s.step(0.05, &c, false).fade < 1.0);
    }

    #[test]
    fn disabled_does_nothing() {
        let c = ShuffleCfg {
            enabled: false,
            ..cfg()
        };
        let mut s = Shuffle::new();
        for _ in 0..100 {
            let step = s.step(0.1, &c, true);
            assert!(!step.switch);
            assert_eq!(step.fade, 1.0);
        }
    }
}
