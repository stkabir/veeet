/// Generador de audio sintético para probar el visualizador sin Spotify.
pub struct FakeAudio {
    t: f64,
    noise: u32,
}

impl FakeAudio {
    pub fn new() -> Self {
        Self {
            t: 0.0,
            noise: 0x1234_5678,
        }
    }

    pub fn generate(&mut self, frames: usize, sample_rate: u32) -> Vec<f32> {
        let sr = sample_rate as f64;
        let dt = 1.0 / sr;
        let mut out = Vec::with_capacity(frames);

        for _ in 0..frames {
            self.t += dt;
            let t = self.t;

            let beat_period = 0.5;
            let beat_phase = t % beat_period;
            let kick = (-beat_phase * 18.0).exp() * (std::f64::consts::TAU * 52.0 * t).sin() * 0.85;

            let snare_phase = (t + 0.25) % beat_period;
            let snare = (-snare_phase * 22.0).exp() * self.noise() * 0.55;

            let bass = (std::f64::consts::TAU * 82.4 * t).sin() * 0.18
                + (std::f64::consts::TAU * 110.0 * t).sin() * 0.10;
            let mid = (std::f64::consts::TAU * 261.6 * t).sin() * 0.10
                + (std::f64::consts::TAU * 392.0 * t).sin() * 0.07;
            let high = (std::f64::consts::TAU * 2093.0 * t).sin() * 0.045
                + (std::f64::consts::TAU * 3136.0 * t).sin() * 0.035;
            let noise = self.noise() * 0.04;
            let swell = 0.55 + 0.45 * ((std::f64::consts::TAU * 0.21 * t).sin() * 0.5 + 0.5);

            let sample = (kick + bass + mid + high + noise) * swell + snare * 0.35;
            out.push((sample * 0.55) as f32);
        }
        out
    }

    fn noise(&mut self) -> f64 {
        self.noise = self
            .noise
            .wrapping_mul(1_664_525)
            .wrapping_add(1_013_904_223);
        ((self.noise >> 9) as f64 / (1u32 << 23) as f64) * 2.0 - 1.0
    }
}
