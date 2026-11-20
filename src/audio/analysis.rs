use std::collections::VecDeque;
use std::sync::Arc;
use std::time::{Duration, Instant};

use realfft::num_complex::Complex32;
use realfft::{RealFftPlanner, RealToComplex};

/// Vizz #3 usa un AnalyserNode de Web Audio con fftSize 2048.
const FFT_SIZE: usize = 2048;
/// frequencyBinCount de un FFT de 2048.
const BINS: usize = FFT_SIZE / 2;
/// La geometría de Vizz usa `min(frequencyBinCount, 2048)` puntos.
const POINTS: usize = BINS;
const HISTORY: usize = 16_384;
pub const BANDS: usize = 64;

/// Valores reales del AnalyserNode que usa Vizz #3.
const MIN_DB: f32 = -110.0;
const MAX_DB: f32 = -20.0;
/// El suavizado de Vizz está normalizado a 120 Hz.
const SMOOTH_RATE: f32 = 120.0;

/// Compensación de pendiente espectral: la música pierde ~3–6 dB por octava
/// hacia los agudos; se realza 4.5 dB/oct alrededor de 1 kHz.
const TILT_DB_PER_OCTAVE: f32 = 4.5;
/// Rango en dB (potencia media por bin, ya compensada) que se mapea a 0..1.
const BAND_DB_FLOOR: f32 = -82.0;
const BAND_DB_SPAN: f32 = 45.0;
/// Umbral de desborde de la autosens: solo las bandas más fuertes rozan la
/// cima; el resto ocupa un rango medio.
const AUTOSENS_CEILING: f32 = 0.95;
/// Piso dinámico: se resta esta fracción de la media de las bandas, para que
/// en mezclas densas la columna refleje la diferencia y no el volumen total.
const FLOOR_RATIO: f32 = 0.75;
/// Constante de tiempo con la que se sigue esa media (segundos).
const FLOOR_TIME: f32 = 2.0;
/// Historia de flux para el umbral adaptativo del beat (~0.7 s a 60 fps).
const FLUX_HISTORY: usize = 43;

#[derive(Clone, Copy, Debug)]
pub struct AnalysisSettings {
    /// Ganancia global aplicada antes de cuantizar a 8 bits.
    pub gain: f32,
    /// smoothingTimeConstant del espectro (Web Audio).
    pub spectrum_smoothing: f32,
    /// 0 = insensible, 1 = muy sensible.
    pub beat_sensitivity: f32,
    /// Motion Smoothing de Vizz.
    pub motion_smoothing: f32,
    /// Frequency Impact de Vizz.
    pub freq_influence: f32,
}

impl Default for AnalysisSettings {
    fn default() -> Self {
        Self {
            gain: 1.0,
            spectrum_smoothing: 0.8,
            beat_sensitivity: 0.5,
            motion_smoothing: 0.95,
            freq_influence: 1.0,
        }
    }
}

pub struct AnalysisFrame {
    /// Valor `d` de Vizz: onda suavizada multiplicada por (1 + frecuencia).
    pub waveform: Vec<f32>,
    /// Frecuencia normalizada usada por el color, en 0..1.
    pub spectrum: Vec<f32>,
    /// Bandas para el HUD y futuros presets.
    #[allow(dead_code)]
    pub bands: Vec<f32>,
    pub bass: f32,
    pub rms: f32,
    pub peak: f32,
    #[allow(dead_code)]
    pub level: f32,
    pub beat: bool,
    pub beat_env: f32,
    pub silent: bool,
}

impl Default for AnalysisFrame {
    fn default() -> Self {
        Self {
            waveform: vec![0.0; POINTS],
            spectrum: vec![0.0; POINTS],
            bands: vec![0.0; BANDS],
            bass: 0.0,
            rms: 0.0,
            peak: 0.0,
            level: 0.0,
            beat: false,
            beat_env: 0.0,
            silent: true,
        }
    }
}

pub struct Analyzer {
    sample_rate: f32,
    r2c: Arc<dyn RealToComplex<f32>>,
    window: Vec<f32>,
    fft_scratch: Vec<f32>,
    fft_output: Vec<Complex32>,
    fft_buf: Vec<f32>,
    magnitudes: Vec<f32>,
    smoothed_magnitudes: Vec<f32>,
    band_edges: Vec<(usize, usize)>,
    bands: Vec<f32>,
    /// Compensación de pendiente por banda, en dB.
    band_tilt_db: Vec<f32>,
    /// Bandas con el suavizado de movimiento de Vizz (simétrico).
    band_motion: Vec<f32>,
    /// Ganancia global automática estilo cava (autosens).
    sens: f32,
    /// Rampa inicial de la autosens hasta el primer desborde.
    sens_init: bool,
    /// Media lenta de las bandas, base del piso dinámico.
    floor_track: f32,
    /// Magnitud en dB de cada bin en el frame anterior (spectral flux).
    prev_bin_db: Vec<f32>,
    flux_history: VecDeque<f32>,
    history: VecDeque<f32>,
    wave_smooth: Vec<f32>,
    waveform: Vec<f32>,
    spectrum: Vec<f32>,
    bass_smooth: f32,
    rms_smooth: f32,
    level: f32,
    peak_env: f32,
    silent_time: f32,
    last_beat: Instant,
    beat_env: f32,
}

impl Analyzer {
    pub fn new(sample_rate: u32) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let r2c = planner.plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| {
                let x = i as f32 / FFT_SIZE as f32;
                0.42 - 0.5 * (std::f32::consts::TAU * x).cos()
                    + 0.08 * (2.0 * std::f32::consts::TAU * x).cos()
            })
            .collect();

        let mut band_edges = Vec::with_capacity(BANDS);
        let f_min = 30.0f32;
        let f_max = 16_000.0f32;
        let mut prev_hi = 1usize;
        for i in 0..BANDS {
            let t0 = i as f32 / BANDS as f32;
            let t1 = (i + 1) as f32 / BANDS as f32;
            let f0 = f_min * (f_max / f_min).powf(t0);
            let f1 = f_min * (f_max / f_min).powf(t1);
            let bin = |f: f32| {
                ((f * FFT_SIZE as f32 / sample_rate as f32).round() as usize).clamp(1, BINS - 1)
            };
            let lo = bin(f0).max(prev_hi);
            let hi = bin(f1).max(lo + 1).min(BINS);
            prev_hi = hi;
            band_edges.push((lo, hi));
        }
        let band_tilt_db = band_edges
            .iter()
            .map(|&(lo, hi)| {
                let center_hz = (lo + hi) as f32 * 0.5 * sample_rate as f32 / FFT_SIZE as f32;
                TILT_DB_PER_OCTAVE * (center_hz / 1000.0).log2()
            })
            .collect();

        Self {
            sample_rate: sample_rate as f32,
            r2c,
            window,
            fft_scratch: vec![0.0; FFT_SIZE],
            fft_output: vec![Complex32::default(); FFT_SIZE / 2 + 1],
            fft_buf: vec![0.0; FFT_SIZE],
            magnitudes: vec![0.0; BINS],
            smoothed_magnitudes: vec![0.0; BINS],
            band_edges,
            bands: vec![0.0; BANDS],
            band_tilt_db,
            band_motion: vec![0.0; BANDS],
            sens: 1.0,
            sens_init: true,
            floor_track: 0.0,
            prev_bin_db: vec![BAND_DB_FLOOR; BINS],
            flux_history: VecDeque::with_capacity(FLUX_HISTORY + 1),
            history: VecDeque::with_capacity(HISTORY),
            wave_smooth: vec![0.0; POINTS],
            waveform: vec![0.0; POINTS],
            spectrum: vec![0.0; POINTS],
            bass_smooth: 0.0,
            rms_smooth: 0.0,
            level: 0.0,
            peak_env: 0.0,
            silent_time: 1.0,
            last_beat: Instant::now() - Duration::from_secs(1),
            beat_env: 0.0,
        }
    }

    pub fn push_samples(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        if samples.len() >= HISTORY {
            self.history.clear();
            self.history.extend(&samples[samples.len() - HISTORY..]);
        } else {
            if self.history.len() + samples.len() > HISTORY {
                let drop = self.history.len() + samples.len() - HISTORY;
                self.history.drain(..drop);
            }
            self.history.extend(samples.iter().copied());
        }
    }

    /// Nivel 0..1 de una banda: potencia media de sus bins en dB, con la
    /// compensación de pendiente aplicada.
    fn band_value(mags: &[f32], (lo, hi): (usize, usize), tilt_db: f32) -> f32 {
        let hi = hi.min(mags.len());
        if lo >= hi {
            return 0.0;
        }
        let power = mags[lo..hi].iter().map(|m| m * m).sum::<f32>() / (hi - lo) as f32;
        let db = 10.0 * (power + 1e-20).log10() + tilt_db;
        ((db - BAND_DB_FLOOR) / BAND_DB_SPAN).clamp(0.0, 1.0)
    }

    /// Bandas para Bars y Radial.
    ///
    /// Mismo modelo de movimiento que la onda de Vizz: suavizado exponencial
    /// simétrico normalizado a 120 Hz (`motion_smoothing`), así sube y baja
    /// con la misma inercia. Encima, la autosens de cava (`cavacore.c`):
    /// una ganancia global que baja rápido cuando alguna banda desborda y
    /// sube lento cuando no, para usar el rango sin saturar todas las barras.
    fn update_bands(&mut self, dt: f32, motion_smoothing: f32, silent: bool) {
        let k = 1.0 - motion_smoothing.clamp(0.0, 0.999).powf(dt * SMOOTH_RATE);
        for i in 0..BANDS {
            let level = Self::band_value(
                &self.smoothed_magnitudes,
                self.band_edges[i],
                self.band_tilt_db[i],
            );
            let m = &mut self.band_motion[i];
            *m += (level - *m) * k;
        }

        let mean = self.band_motion.iter().sum::<f32>() / BANDS as f32;
        self.floor_track += (mean - self.floor_track) * (1.0 - (-dt / FLOOR_TIME).exp());
        let floor = (FLOOR_RATIO * self.floor_track).min(0.9);

        let mut overshoot = false;
        for i in 0..BANDS {
            let lifted = ((self.band_motion[i] - floor) / (1.0 - floor)).max(0.0);
            let out = lifted * self.sens;
            overshoot |= out > AUTOSENS_CEILING;
            self.bands[i] = out.clamp(0.0, 1.0);
        }

        // cava usa framerate_mod = 66 / fps; aquí 66 · dt es lo mismo.
        let fm = 66.0 * dt;
        if overshoot {
            self.sens *= 1.0 - 0.02 * fm;
            self.sens_init = false;
        } else if !silent {
            let rate = if self.sens_init { 0.1 } else { 0.001 };
            self.sens *= 1.0 + rate * fm;
        }
        self.sens = self.sens.clamp(0.05, 20.0);
    }

    /// Spectral flux positivo (dB por bin) de graves (bombo, 30–150 Hz) y
    /// medios-altos (caja/palmas, 1.5–5 kHz). Actualiza la magnitud previa.
    fn spectral_flux(&mut self) -> f32 {
        let bin = |hz: f32| {
            ((hz * FFT_SIZE as f32 / self.sample_rate).round() as usize).clamp(1, BINS - 1)
        };
        let (low_lo, low_hi) = (bin(30.0), bin(150.0));
        let (mid_lo, mid_hi) = (bin(1500.0), bin(5000.0));
        let mut low = 0.0f32;
        let mut mid = 0.0f32;
        for (i, prev) in self.prev_bin_db.iter_mut().enumerate() {
            let db = (20.0 * (self.magnitudes[i] + 1e-9).log10()).max(BAND_DB_FLOOR);
            let rise = (db - *prev).max(0.0);
            if (low_lo..low_hi).contains(&i) {
                low += rise;
            } else if (mid_lo..mid_hi).contains(&i) {
                mid += rise;
            }
            *prev = db;
        }
        low / (low_hi - low_lo).max(1) as f32 + 0.6 * mid / (mid_hi - mid_lo).max(1) as f32
    }

    pub fn analyze(&mut self, dt: f32, settings: &AnalysisSettings) -> AnalysisFrame {
        let dt = dt.clamp(0.001, 0.1);

        // Ventana de FFT: las últimas 2048 muestras, con relleno de ceros.
        let available = self.history.len().min(FFT_SIZE);
        let start = self.history.len() - available;
        let pad = FFT_SIZE - available;
        self.fft_buf[..pad].fill(0.0);
        for (i, &v) in self.history.iter().skip(start).enumerate() {
            self.fft_buf[pad + i] = v;
        }

        let mut sum_sq = 0.0f32;
        let mut peak = 0.0f32;
        for &v in &self.fft_buf {
            sum_sq += v * v;
            peak = peak.max(v.abs());
        }
        let rms = (sum_sq / FFT_SIZE as f32).sqrt();
        self.rms_smooth += (rms - self.rms_smooth) * (1.0 - (-dt * 12.0).exp());

        let silent = rms < 0.0015;
        if silent {
            self.silent_time += dt;
        } else {
            self.silent_time = 0.0;
        }

        if peak > self.peak_env {
            self.peak_env += (peak - self.peak_env) * (1.0 - (-dt * 30.0).exp());
        } else {
            self.peak_env *= (-dt * 1.6).exp();
        }

        // FFT con ventana Blackman, equivalente a AnalyserNode.
        for i in 0..FFT_SIZE {
            self.fft_scratch[i] = self.fft_buf[i] * self.window[i];
        }

        if self
            .r2c
            .process(&mut self.fft_scratch, &mut self.fft_output)
            .is_ok()
        {
            let norm = 1.0 / FFT_SIZE as f32;
            for (mag, bin) in self
                .magnitudes
                .iter_mut()
                .zip(self.fft_output.iter().take(BINS))
            {
                *mag = bin.norm() * norm;
            }
        } else {
            self.magnitudes.fill(0.0);
        }

        // Web Audio suaviza las magnitudes del espectro con
        // smoothingTimeConstant antes de convertirlas a bytes.
        let spectrum_smoothing = settings.spectrum_smoothing.clamp(0.0, 1.0);
        for i in 0..BINS {
            self.smoothed_magnitudes[i] = spectrum_smoothing * self.smoothed_magnitudes[i]
                + (1.0 - spectrum_smoothing) * self.magnitudes[i];
        }

        let freq_influence = settings.freq_influence.clamp(0.0, 1.0);
        for i in 0..POINTS {
            // Vizz espeja el espectro desde el centro hacia los bordes.
            let bin = (POINTS / 2).abs_diff(i).min(BINS - 1);

            let db = 20.0 * (self.smoothed_magnitudes[bin] + 1e-12).log10();
            let byte = ((db - MIN_DB) / (MAX_DB - MIN_DB)).clamp(0.0, 1.0) * 255.0;
            let byte = byte.round() / 255.0;
            self.spectrum[i] = byte * freq_influence;
        }

        // Onda de Vizz: últimas 1024 muestras cuantizadas a 8 bits,
        // suavizadas con la misma exponencial normalizada a 120 Hz.
        let smoothing = settings.motion_smoothing.clamp(0.0, 1.0);
        let k = 1.0 - smoothing.powf(dt * SMOOTH_RATE);
        let gain = settings.gain.max(0.0);
        let first = FFT_SIZE - POINTS;
        for i in 0..POINTS {
            let sample = (self.fft_buf[first + i] * gain).clamp(-1.0, 1.0);
            let byte = (((sample * 0.5 + 0.5) * 255.0).round()).clamp(0.0, 255.0);
            let target = (byte / 128.0 - 1.0) * 0.7;
            self.wave_smooth[i] += (target - self.wave_smooth[i]) * k;
            self.waveform[i] = self.wave_smooth[i] * (1.0 + self.spectrum[i]);
        }

        self.update_bands(dt, settings.motion_smoothing, silent);
        let flux = self.spectral_flux();

        let bass_lo = ((30.0 * FFT_SIZE as f32 / self.sample_rate).round() as usize).max(1);
        let bass_hi = ((140.0 * FFT_SIZE as f32 / self.sample_rate).round() as usize)
            .clamp(bass_lo + 1, BINS);
        let bass_avg =
            self.magnitudes[bass_lo..bass_hi].iter().sum::<f32>() / (bass_hi - bass_lo) as f32;
        let bass_db = 20.0 * (bass_avg + 1e-9).log10();
        let bass_raw = ((bass_db + 60.0) / 60.0).clamp(0.0, 1.0);
        let bass_rate = if bass_raw > self.bass_smooth {
            40.0
        } else {
            8.0
        };
        self.bass_smooth += (bass_raw - self.bass_smooth) * (1.0 - (-bass_rate * dt).exp());

        if self.flux_history.len() >= FLUX_HISTORY {
            self.flux_history.pop_front();
        }
        self.flux_history.push_back(flux);
        let n = self.flux_history.len() as f32;
        let avg = self.flux_history.iter().sum::<f32>() / n;
        let var = self
            .flux_history
            .iter()
            .map(|v| (v - avg) * (v - avg))
            .sum::<f32>()
            / n;
        let dev = var.sqrt();
        let sensitivity = settings.beat_sensitivity.clamp(0.0, 1.0);
        let threshold = avg + dev * (1.9 - sensitivity * 1.1) + 0.15;
        let now = Instant::now();
        let beat = !silent
            && flux > threshold
            && flux > 0.6
            && now.duration_since(self.last_beat) > Duration::from_millis(130);
        if beat {
            self.last_beat = now;
            self.beat_env = 1.0;
        } else {
            self.beat_env *= (-dt * 5.5).exp();
        }

        self.level += (self.rms_smooth * 4.0 - self.level) * (1.0 - (-dt * 10.0).exp());

        AnalysisFrame {
            waveform: self.waveform.clone(),
            spectrum: self.spectrum.clone(),
            bands: self.bands.clone(),
            bass: self.bass_smooth,
            rms: self.rms_smooth,
            peak: self.peak_env,
            level: self.level.clamp(0.0, 1.0),
            beat,
            beat_env: self.beat_env,
            silent: self.silent_time > 0.35,
        }
    }
}
