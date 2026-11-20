use std::collections::{HashSet, VecDeque};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use wasapi::{
    AudioClient, DeviceEnumerator, Direction, SampleType, StreamMode, WaveFormat, initialize_mta,
};

use super::sessions::{AudioApp, AutoPicker, SessionMonitor, display_exe};
use crate::config::{AudioCfg, CaptureMode};

const SAMPLE_RATE: u32 = 48_000;
const CHANNELS: usize = 2;
pub const RING_SECONDS: usize = 2;

#[derive(Clone, Debug)]
pub enum CaptureState {
    Starting,
    Searching,
    Running { source: String },
    Error(String),
}

impl CaptureState {
    pub fn label(&self) -> String {
        match self {
            Self::Starting => "preparando audio".to_string(),
            Self::Searching => "esperando audio".to_string(),
            Self::Running { source } => format!("escuchando {source}"),
            Self::Error(err) => format!("error: {err}"),
        }
    }
}

pub struct AudioShared {
    pub samples: VecDeque<f32>,
    pub state: CaptureState,
    pub last_sample_at: Option<Instant>,
    /// Apps con sesión de audio (para el modo automático y el menú).
    pub apps: Vec<AudioApp>,
}

impl AudioShared {
    fn new() -> Self {
        Self {
            samples: VecDeque::with_capacity(SAMPLE_RATE as usize * RING_SECONDS),
            state: CaptureState::Starting,
            last_sample_at: None,
            apps: Vec::new(),
        }
    }

    fn push(&mut self, chunk: &[f32]) {
        let cap = SAMPLE_RATE as usize * RING_SECONDS;
        if chunk.len() >= cap {
            self.samples.clear();
            self.samples.extend(&chunk[chunk.len() - cap..]);
        } else {
            if self.samples.len() + chunk.len() > cap {
                let drop = self.samples.len() + chunk.len() - cap;
                self.samples.drain(..drop);
            }
            self.samples.extend(chunk);
        }
        self.last_sample_at = Some(Instant::now());
    }
}

pub struct AudioSnapshot {
    pub samples: Vec<f32>,
    pub state: CaptureState,
    pub last_sample_at: Option<Instant>,
    pub apps: Vec<AudioApp>,
}

pub struct AudioEngine {
    shared: Arc<Mutex<AudioShared>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl AudioEngine {
    pub fn start(cfg: AudioCfg) -> Self {
        let shared = Arc::new(Mutex::new(AudioShared::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_shared = Arc::clone(&shared);
        let thread_stop = Arc::clone(&stop);

        let handle = thread::Builder::new()
            .name("veeet-audio".to_string())
            .spawn(move || capture_thread(thread_shared, thread_stop, cfg))
            .expect("no se pudo crear el hilo de audio");

        Self {
            shared,
            stop,
            handle: Some(handle),
        }
    }

    /// Drena hasta `max` muestras mono del buffer compartido.
    pub fn poll(&self, max: usize) -> AudioSnapshot {
        let mut shared = self.shared.lock().expect("audio mutex envenenado");
        let take = shared.samples.len().min(max);
        let samples: Vec<f32> = shared.samples.drain(..take).collect();
        AudioSnapshot {
            samples,
            state: shared.state.clone(),
            last_sample_at: shared.last_sample_at,
            apps: shared.apps.clone(),
        }
    }
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn set_state(shared: &Arc<Mutex<AudioShared>>, state: CaptureState) {
    if let Ok(mut shared) = shared.lock() {
        if !matches!(state, CaptureState::Running { .. })
            || !matches!(shared.state, CaptureState::Running { .. })
        {
            log::info!("Audio: {}", state.label());
        }
        shared.state = state;
    }
}

/// Qué se captura en este momento.
#[derive(Clone, Debug)]
enum Target {
    /// Todo lo que suena en la salida por defecto.
    Device,
    /// Una app (y su árbol de procesos).
    App { pid: u32, exe: String },
}

impl Target {
    fn pid(&self) -> Option<u32> {
        match self {
            Self::Device => None,
            Self::App { pid, .. } => Some(*pid),
        }
    }
}

/// Cada cuánto se revisan las sesiones de audio.
const SCAN_INTERVAL: Duration = Duration::from_millis(400);

fn capture_thread(shared: Arc<Mutex<AudioShared>>, stop: Arc<AtomicBool>, cfg: AudioCfg) {
    let _ = initialize_mta();
    let mut backoff = Duration::from_millis(400);
    let mut monitor = SessionMonitor::new();
    let mut picker = AutoPicker::new(&cfg.process);
    // Fuente decidida por el modo automático al salir del stream anterior.
    let mut next: Option<Target> = None;

    while !stop.load(Ordering::Relaxed) {
        let apps = monitor.scan();
        publish_apps(&shared, &apps);
        let target = match cfg.mode {
            CaptureMode::Device => Target::Device,
            CaptureMode::App => apps
                .iter()
                .find(|a| a.exe.eq_ignore_ascii_case(&cfg.process))
                .map(|a| a.root_pid)
                .or_else(|| find_process_root_pid(&cfg.process))
                .map_or(Target::Device, |pid| Target::App {
                    pid,
                    exe: cfg.process.clone(),
                }),
            CaptureMode::Auto => next.take().unwrap_or_else(|| {
                picker
                    .initial(&apps)
                    .map_or(Target::Device, |app| Target::App {
                        pid: app.root_pid,
                        exe: app.exe,
                    })
            }),
        };

        let started = Instant::now();
        let mut session = Session {
            shared: &shared,
            stop: &stop,
            cfg: &cfg,
            monitor: &mut monitor,
            picker: &mut picker,
            next: &mut next,
        };
        let result = session.capture(&target);
        if stop.load(Ordering::Relaxed) {
            break;
        }

        match result {
            Ok(()) => {
                backoff = Duration::from_millis(400);
                if next.is_some() {
                    // Cambio de fuente en modo automático: sin espera.
                    continue;
                }
                set_state(&shared, CaptureState::Searching);
            }
            Err(err) => {
                log::warn!("Captura interrumpida: {err:#}");
                set_state(&shared, CaptureState::Error(format!("{err:#}")));
                if started.elapsed() > Duration::from_secs(5) {
                    backoff = Duration::from_millis(400);
                } else {
                    backoff = (backoff * 2).min(Duration::from_secs(3));
                }
            }
        }

        let until = Instant::now() + backoff;
        while Instant::now() < until && !stop.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
        }
    }
}

fn publish_apps(shared: &Arc<Mutex<AudioShared>>, apps: &[AudioApp]) {
    if let Ok(mut shared) = shared.lock() {
        shared.apps = apps.to_vec();
    }
}

fn source_label(mode: CaptureMode, target: &Target) -> String {
    let what = match target {
        Target::Device => "todo el sistema".to_string(),
        Target::App { exe, .. } => display_exe(exe),
    };
    match mode {
        CaptureMode::Auto => format!("auto · {what}"),
        _ => what,
    }
}

/// Estado del hilo que necesita un stream de captura.
struct Session<'a> {
    shared: &'a Arc<Mutex<AudioShared>>,
    stop: &'a Arc<AtomicBool>,
    cfg: &'a AudioCfg,
    monitor: &'a mut SessionMonitor,
    picker: &'a mut AutoPicker,
    next: &'a mut Option<Target>,
}

impl Session<'_> {
    /// Revisa las sesiones; `true` si hay que cerrar el stream y reabrir con
    /// otra fuente.
    fn should_switch(&mut self, pid: Option<u32>) -> bool {
        let apps = self.monitor.scan();
        publish_apps(self.shared, &apps);
        match self.cfg.mode {
            CaptureMode::Auto => {
                if let Some(app) = self.picker.update(&apps, pid) {
                    log::info!("Modo automático: ahora suena {}", app.exe);
                    *self.next = Some(Target::App {
                        pid: app.root_pid,
                        exe: app.exe,
                    });
                    return true;
                }
                false
            }
            // Capturando todo el dispositivo porque la app aún no existía:
            // en cuanto aparezca, pasamos a captura por app.
            CaptureMode::App => {
                let appeared = pid.is_none()
                    && apps
                        .iter()
                        .any(|a| a.exe.eq_ignore_ascii_case(&self.cfg.process));
                if appeared {
                    log::info!(
                        "El proceso '{}' ya está disponible; cambiando a captura por aplicación",
                        self.cfg.process
                    );
                }
                appeared
            }
            CaptureMode::Device => false,
        }
    }

    fn capture(&mut self, target: &Target) -> Result<()> {
        let pid = target.pid();
        let mut client = if let Some(pid) = pid {
            AudioClient::new_application_loopback_client(pid, true)
                .context("creando captura por aplicación")?
        } else {
            if self.cfg.mode == CaptureMode::App {
                log::info!(
                    "No encontré el proceso '{}'; usando el dispositivo de salida completo",
                    self.cfg.process
                );
            }
            let enumerator = DeviceEnumerator::new().context("enumerando dispositivos")?;
            let device = enumerator
                .get_default_device(Direction::Render)
                .context("dispositivo de salida por defecto")?;
            device
                .get_iaudioclient()
                .context("abriendo cliente de audio")?
        };
        let source = source_label(self.cfg.mode, target);
        log::info!("Fuente de audio: {source}");

        let desired = WaveFormat::new(
            32,
            32,
            SampleType::Float,
            SAMPLE_RATE as usize,
            CHANNELS,
            None,
        );
        let mode = StreamMode::EventsShared {
            autoconvert: true,
            buffer_duration_hns: 200_000,
        };
        client
            .initialize_client(&desired, Direction::Capture, mode, None)
            .context("inicializando captura loopback")?;

        set_state(self.shared, CaptureState::Running { source });

        let event = client.set_get_eventhandle()?;
        let capture = client.get_audiocaptureclient()?;
        client.start_stream().context("iniciando stream")?;
        log::info!("Stream de captura iniciado");

        let mut bytes = vec![0.0f32; 4096 * CHANNELS];
        let mut mono = Vec::with_capacity(4096);
        let stream_started = Instant::now();
        let mut last_data = Instant::now();
        let mut last_scan = Instant::now();

        loop {
            if self.stop.load(Ordering::Relaxed) {
                break;
            }

            if last_scan.elapsed() >= SCAN_INTERVAL {
                last_scan = Instant::now();
                if self.should_switch(pid) {
                    break;
                }
            }

            if event
                .wait_for_event(SCAN_INTERVAL.as_millis() as u32)
                .is_err()
            {
                // Silencio, pausa o aplicación cerrada: no es un error por sí mismo.
                if let Some(pid) = pid
                    && !process_alive(pid)
                {
                    log::info!(
                        "La aplicación capturada ya no está en ejecución; reiniciando captura"
                    );
                    break;
                }
                if pid.is_none()
                    && self.cfg.mode == CaptureMode::App
                    && last_data.elapsed() > Duration::from_secs(15)
                {
                    break;
                }
                continue;
            }

            loop {
                let frames = match capture.get_next_packet_size() {
                    Ok(Some(frames)) => frames as usize,
                    Ok(None) => 0,
                    Err(err) => return Err(err).context("consultando paquete"),
                };
                if frames == 0 {
                    break;
                }
                let needed = frames * CHANNELS;
                if bytes.len() < needed {
                    bytes.resize(needed, 0.0);
                }
                let byte_slice = bytemuck::cast_slice_mut::<f32, u8>(&mut bytes[..needed]);
                let (read_frames, _info) = capture
                    .read_from_device(byte_slice)
                    .context("leyendo paquete de audio")?;
                if read_frames == 0 {
                    break;
                }

                mono.clear();
                for frame in 0..read_frames as usize {
                    let l = bytes[frame * CHANNELS];
                    let r = bytes[frame * CHANNELS + 1];
                    mono.push((l + r) * 0.5);
                }
                if let Ok(mut shared) = self.shared.lock() {
                    shared.push(&mono);
                }
                last_data = Instant::now();
            }

            // Reapertura periódica por si el stream se queda colgado.
            if stream_started.elapsed() > Duration::from_secs(3600) {
                break;
            }
        }

        let _ = client.stop_stream();
        Ok(())
    }
}

fn process_alive(pid: u32) -> bool {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(handle) => {
                let _ = windows::Win32::Foundation::CloseHandle(handle);
                true
            }
            Err(_) => false,
        }
    }
}

/// Devuelve el PID raíz de un proceso (el padre del árbol), que es el que
/// necesita el loopback por aplicación de WASAPI.
fn find_process_root_pid(name: &str) -> Option<u32> {
    let Ok(sys) = sysinfo::System::new_all() else {
        return None;
    };
    let mut found: Vec<(u32, Option<u32>)> = Vec::new();

    for (pid, process) in sys.processes() {
        if process.name().to_string_lossy().eq_ignore_ascii_case(name) {
            found.push((pid.as_u32(), process.parent().map(|p| p.as_u32())));
        }
    }

    if found.is_empty() {
        return None;
    }

    let pids: HashSet<u32> = found.iter().map(|(pid, _)| *pid).collect();
    found
        .iter()
        .find(|(_, parent)| parent.is_none_or(|parent| !pids.contains(&parent)))
        .or_else(|| found.first())
        .map(|(pid, _)| *pid)
}
