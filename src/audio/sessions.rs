//! Sesiones de audio de Windows: qué aplicaciones están reproduciendo sonido
//! y con qué nivel. Sirve para el modo automático (seguir a la app que suena)
//! y para la lista de apps que se puede elegir a mano.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    DEVICE_STATE_ACTIVE, IAudioSessionControl2, IAudioSessionManager2, IMMDeviceEnumerator,
    MMDeviceEnumerator, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, CoCreateInstance};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GetWindowLongPtrW, GetWindowRect, GetWindowTextLengthW,
    GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, WS_EX_TOOLWINDOW,
};
use windows::core::{BOOL, Interface};

/// Pico a partir del cual una sesión cuenta como "sonando".
pub const ACTIVE_PEAK: f32 = 0.004;

/// Apps que reciben el audio de otras y lo vuelven a reproducir (mejoradores,
/// mezcladores virtuales). Suenan siempre que suena cualquier otra cosa, así
/// que en modo automático solo se eligen si no hay nada mejor.
const RELAYS: &[&str] = &[
    "fxsound",
    "voicemeeter",
    "audiorelay",
    "steelseriessonar",
    "sonar",
    "nahimic",
    "boom3d",
    "dfx",
    "equalizerapo",
    "peace",
];

pub fn is_relay(exe: &str) -> bool {
    let exe = exe.to_ascii_lowercase();
    let stem = exe.strip_suffix(".exe").unwrap_or(&exe);
    RELAYS.iter().any(|r| stem.starts_with(r))
}

/// Una aplicación con al menos una sesión de audio en alguna salida activa.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioApp {
    /// Nombre del ejecutable (p. ej. `chrome.exe`).
    pub exe: String,
    /// PID raíz del árbol de procesos de esa app (lo que pide el loopback).
    pub root_pid: u32,
    /// Pico actual (0–1) sumando todas sus sesiones.
    pub peak: f32,
    /// Título de su ventana principal, si tiene.
    pub title: String,
}

impl AudioApp {
    pub fn is_active(&self) -> bool {
        self.peak > ACTIVE_PEAK
    }

    /// Reproduce el audio de otras apps (FxSound, Voicemeeter…).
    pub fn is_relay(&self) -> bool {
        is_relay(&self.exe)
    }

    /// Nombre legible: sin `.exe` y con mayúscula inicial.
    pub fn display_name(&self) -> String {
        display_exe(&self.exe)
    }
}

pub fn display_exe(exe: &str) -> String {
    let stem = exe
        .rsplit_once('.')
        .filter(|(_, ext)| ext.eq_ignore_ascii_case("exe"))
        .map_or(exe, |(stem, _)| stem);
    let mut chars = stem.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

#[derive(Clone)]
struct ProcInfo {
    exe: String,
    parent: Option<u32>,
}

/// Lee las sesiones de audio. Hay que usarlo desde un hilo con COM iniciado.
pub struct SessionMonitor {
    procs: HashMap<u32, ProcInfo>,
    last_proc_refresh: Option<Instant>,
    titles: HashMap<String, String>,
    last_titles: Option<Instant>,
    own_pid: u32,
}

impl SessionMonitor {
    pub fn new() -> Self {
        Self {
            procs: HashMap::new(),
            last_proc_refresh: None,
            titles: HashMap::new(),
            last_titles: None,
            own_pid: std::process::id(),
        }
    }

    /// Apps con sesión de audio, la más fuerte primero.
    pub fn scan(&mut self) -> Vec<AudioApp> {
        let sessions = match unsafe { list_sessions() } {
            Ok(sessions) => sessions,
            Err(err) => {
                log::debug!("No pude leer las sesiones de audio: {err}");
                return Vec::new();
            }
        };

        if sessions
            .iter()
            .any(|(pid, _)| !self.procs.contains_key(pid))
        {
            self.refresh_procs();
        }

        let mut by_root: HashMap<u32, AudioApp> = HashMap::new();
        for (pid, peak) in sessions {
            if pid == 0 || pid == self.own_pid {
                continue;
            }
            let Some((root_pid, exe)) = self.root_of(pid) else {
                continue;
            };
            let app = by_root.entry(root_pid).or_insert_with(|| AudioApp {
                exe,
                root_pid,
                peak: 0.0,
                title: String::new(),
            });
            app.peak = app.peak.max(peak);
        }

        let mut apps: Vec<AudioApp> = by_root.into_values().collect();
        self.refresh_titles(&apps);
        for app in &mut apps {
            if let Some(title) = self.titles.get(&app.exe.to_ascii_lowercase()) {
                app.title = title.clone();
            }
        }
        apps.sort_by(|a, b| {
            b.peak
                .total_cmp(&a.peak)
                .then_with(|| a.exe.to_ascii_lowercase().cmp(&b.exe.to_ascii_lowercase()))
        });
        apps
    }

    fn refresh_procs(&mut self) {
        // Refrescar la tabla de procesos es caro; como mucho cada 2 s.
        if self
            .last_proc_refresh
            .is_some_and(|t| t.elapsed() < Duration::from_secs(2))
        {
            return;
        }
        self.last_proc_refresh = Some(Instant::now());
        let Ok(mut sys) = sysinfo::System::new() else {
            return;
        };
        sys.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::All,
            true,
            sysinfo::ProcessRefreshKind::nothing(),
        );
        self.procs = sys
            .processes()
            .iter()
            .map(|(pid, process)| {
                (
                    pid.as_u32(),
                    ProcInfo {
                        exe: process.name().to_string_lossy().into_owned(),
                        parent: process.parent().map(|p| p.as_u32()),
                    },
                )
            })
            .collect();
    }

    /// Sube por el árbol mientras el padre sea el mismo ejecutable. Así las
    /// sesiones de los subprocesos de Chrome/Edge/Firefox se agrupan en el
    /// proceso principal, que es el que se pasa al loopback (con su árbol).
    fn root_of(&self, pid: u32) -> Option<(u32, String)> {
        let info = self.procs.get(&pid)?;
        let mut root = pid;
        let mut seen = HashSet::from([pid]);
        while let Some(parent) = self.procs.get(&root).and_then(|p| p.parent) {
            match self.procs.get(&parent) {
                Some(p) if p.exe.eq_ignore_ascii_case(&info.exe) && seen.insert(parent) => {
                    root = parent;
                }
                _ => break,
            }
        }
        Some((root, info.exe.clone()))
    }

    fn refresh_titles(&mut self, apps: &[AudioApp]) {
        if self
            .last_titles
            .is_some_and(|t| t.elapsed() < Duration::from_millis(1500))
        {
            return;
        }
        self.last_titles = Some(Instant::now());
        let wanted: HashSet<String> = apps.iter().map(|a| a.exe.to_ascii_lowercase()).collect();
        self.titles = window_titles(&wanted);
    }
}

impl Default for SessionMonitor {
    fn default() -> Self {
        Self::new()
    }
}

/// (pid, pico) de cada sesión de todas las salidas activas, sin los sonidos
/// del sistema. Se miran todas porque con FxSound, Voicemeeter y similares la
/// música va a un dispositivo virtual y no a la salida por defecto. El loopback
/// por proceso captura a la app esté en el dispositivo que esté.
unsafe fn list_sessions() -> windows::core::Result<Vec<(u32, f32)>> {
    unsafe {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let devices = enumerator.EnumAudioEndpoints(eRender, DEVICE_STATE_ACTIVE)?;
        let mut out = Vec::new();
        for d in 0..devices.GetCount()? {
            let Ok(device) = devices.Item(d) else {
                continue;
            };
            let Ok(manager) = device.Activate::<IAudioSessionManager2>(CLSCTX_ALL, None) else {
                continue;
            };
            list_device_sessions(&manager, &mut out);
        }
        Ok(out)
    }
}

unsafe fn list_device_sessions(manager: &IAudioSessionManager2, out: &mut Vec<(u32, f32)>) {
    unsafe {
        let Ok(sessions) = manager.GetSessionEnumerator() else {
            return;
        };
        let count = sessions.GetCount().unwrap_or(0);
        for i in 0..count {
            let Ok(control) = sessions.GetSession(i) else {
                continue;
            };
            let Ok(control2) = control.cast::<IAudioSessionControl2>() else {
                continue;
            };
            // S_OK = sesión de sonidos del sistema; S_FALSE = sesión normal.
            if control2.IsSystemSoundsSession().0 == 0 {
                continue;
            }
            let Ok(pid) = control2.GetProcessId() else {
                continue;
            };
            let peak = control
                .cast::<IAudioMeterInformation>()
                .and_then(|meter| meter.GetPeakValue())
                .unwrap_or(0.0);
            out.push((pid, peak));
        }
    }
}

struct TitleCtx<'a> {
    wanted: &'a HashSet<String>,
    exes: HashMap<u32, Option<String>>,
    best: HashMap<String, (i64, String)>,
}

/// Título de la ventana visible más grande de cada ejecutable pedido.
fn window_titles(wanted: &HashSet<String>) -> HashMap<String, String> {
    if wanted.is_empty() {
        return HashMap::new();
    }
    let mut ctx = TitleCtx {
        wanted,
        exes: HashMap::new(),
        best: HashMap::new(),
    };
    unsafe {
        let _ = EnumWindows(Some(enum_title), LPARAM(&mut ctx as *mut _ as isize));
    }
    ctx.best
        .into_iter()
        .map(|(exe, (_, title))| (exe, title))
        .collect()
}

unsafe extern "system" fn enum_title(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let ctx = unsafe { &mut *(lparam.0 as *mut TitleCtx<'_>) };
    if !unsafe { IsWindowVisible(hwnd).as_bool() } {
        return TRUE;
    }
    let style = unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) };
    if style & WS_EX_TOOLWINDOW.0 as isize != 0 {
        return TRUE;
    }
    let len = unsafe { GetWindowTextLengthW(hwnd) };
    if len <= 0 {
        return TRUE;
    }
    let mut pid = 0u32;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
    }
    let exe = ctx.exes.entry(pid).or_insert_with(|| exe_name(pid)).clone();
    let Some(exe) = exe else {
        return TRUE;
    };
    let key = exe.to_ascii_lowercase();
    if !ctx.wanted.contains(&key) {
        return TRUE;
    }
    let mut buf = vec![0u16; len as usize + 1];
    let copied = unsafe { GetWindowTextW(hwnd, &mut buf) } as usize;
    buf.truncate(copied);
    let title = String::from_utf16_lossy(&buf).trim().to_string();
    if title.is_empty() {
        return TRUE;
    }
    let mut rect = RECT::default();
    let area = if unsafe { GetWindowRect(hwnd, &mut rect) }.is_ok() {
        (rect.right - rect.left) as i64 * (rect.bottom - rect.top) as i64
    } else {
        0
    };
    if ctx.best.get(&key).is_none_or(|(best, _)| area > *best) {
        ctx.best.insert(key, (area, title));
    }
    TRUE
}

fn exe_name(pid: u32) -> Option<String> {
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        );
        let _ = CloseHandle(handle);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..size as usize]);
        path.rsplit(['\\', '/']).next().map(str::to_string)
    }
}

/// Decide a qué app seguir en modo automático.
///
/// Se queda con la fuente actual mientras suene. Si la actual lleva un rato en
/// silencio (pausa) y otra app está sonando de forma sostenida, cambia a esa.
pub struct AutoPicker {
    preferred: String,
    current_silent_since: Option<Instant>,
    candidate: Option<(u32, Instant)>,
}

/// Silencio de la fuente actual antes de buscar otra.
const SWITCH_AFTER_SILENCE: Duration = Duration::from_millis(1200);
/// Tiempo que otra app debe sonar seguida para cambiar a ella.
const CANDIDATE_HOLD: Duration = Duration::from_millis(350);

impl AutoPicker {
    pub fn new(preferred: &str) -> Self {
        Self {
            preferred: preferred.to_string(),
            current_silent_since: None,
            candidate: None,
        }
    }

    /// Fuente inicial: la app más fuerte que suene; si no suena nada, la
    /// preferida (Spotify) si está abierta; si no, `None` (todo el sistema).
    pub fn initial(&mut self, apps: &[AudioApp]) -> Option<AudioApp> {
        self.current_silent_since = None;
        self.candidate = None;
        apps.iter()
            .filter(|a| a.is_active())
            .max_by(|a, b| {
                self.rank(a)
                    .cmp(&self.rank(b))
                    .then(a.peak.total_cmp(&b.peak))
            })
            .or_else(|| {
                apps.iter()
                    .find(|a| a.exe.eq_ignore_ascii_case(&self.preferred))
            })
            .cloned()
    }

    /// `Some(app)` si hay que cambiar de fuente. `current` es el PID raíz que se
    /// está capturando (`None` = todo el sistema).
    pub fn update(&mut self, apps: &[AudioApp], current: Option<u32>) -> Option<AudioApp> {
        let now = Instant::now();
        let current_app = current.and_then(|pid| apps.iter().find(|a| a.root_pid == pid));
        let current_active = current_app.is_some_and(AudioApp::is_active);
        // Un reenviador (FxSound…) "suena" siempre que suena otra app: si hay
        // una app real sonando, la preferimos.
        let relay_shadowed = current_app.is_some_and(AudioApp::is_relay)
            && apps.iter().any(|a| a.is_active() && !a.is_relay());

        if current_active && !relay_shadowed {
            self.current_silent_since = None;
            self.candidate = None;
            return None;
        }
        // Capturando todo el sistema: en cuanto una app suene, la tomamos.
        // Con una app en pausa: esperamos un poco antes de soltarla.
        if current.is_some() && !relay_shadowed {
            let since = *self.current_silent_since.get_or_insert(now);
            if now.duration_since(since) < SWITCH_AFTER_SILENCE {
                return None;
            }
        }

        let best = apps
            .iter()
            .filter(|a| a.is_active() && Some(a.root_pid) != current)
            .max_by(|a, b| {
                self.rank(a)
                    .cmp(&self.rank(b))
                    .then(a.peak.total_cmp(&b.peak))
            });
        let Some(best) = best else {
            self.candidate = None;
            return None;
        };
        match self.candidate {
            Some((pid, since)) if pid == best.root_pid => {
                if now.duration_since(since) >= CANDIDATE_HOLD {
                    self.candidate = None;
                    self.current_silent_since = None;
                    return Some(best.clone());
                }
            }
            _ => self.candidate = Some((best.root_pid, now)),
        }
        None
    }

    /// Orden de preferencia: apps reales antes que reenviadores; entre ellas,
    /// la preferida (Spotify) primero.
    fn rank(&self, app: &AudioApp) -> (bool, bool) {
        (
            !app.is_relay(),
            app.exe.eq_ignore_ascii_case(&self.preferred),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(exe: &str, pid: u32, peak: f32) -> AudioApp {
        AudioApp {
            exe: exe.to_string(),
            root_pid: pid,
            peak,
            title: String::new(),
        }
    }

    /// Llama a `update` durante `ms` y devuelve el primer cambio.
    fn run(p: &mut AutoPicker, apps: &[AudioApp], cur: Option<u32>, ms: u64) -> Option<AudioApp> {
        let end = Instant::now() + Duration::from_millis(ms);
        while Instant::now() < end {
            if let Some(a) = p.update(apps, cur) {
                return Some(a);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }

    #[test]
    fn prefiere_app_real_sobre_reenviador() {
        let mut p = AutoPicker::new("Spotify.exe");
        let apps = [app("FxSound.exe", 1, 0.5), app("Spotify.exe", 2, 0.4)];
        assert_eq!(p.initial(&apps).unwrap().root_pid, 2);
        // Capturando FxSound: cambia a Spotify sin esperar silencio.
        assert_eq!(run(&mut p, &apps, Some(1), 600).unwrap().root_pid, 2);
    }

    #[test]
    fn spotify_en_pausa_pasa_al_navegador() {
        let mut p = AutoPicker::new("Spotify.exe");
        let sonando = [app("Spotify.exe", 2, 0.4), app("chrome.exe", 3, 0.3)];
        // Mientras Spotify suene, no se cambia.
        assert!(run(&mut p, &sonando, Some(2), 600).is_none());
        let pausa = [app("Spotify.exe", 2, 0.0), app("chrome.exe", 3, 0.3)];
        // Poco silencio: aún no.
        assert!(run(&mut p, &pausa, Some(2), 800).is_none());
        // Pasado el umbral, cambia a Chrome.
        assert_eq!(run(&mut p, &pausa, Some(2), 1500).unwrap().root_pid, 3);
    }

    #[test]
    fn todo_el_sistema_toma_la_primera_app() {
        let mut p = AutoPicker::new("Spotify.exe");
        let apps = [app("chrome.exe", 3, 0.3)];
        assert_eq!(run(&mut p, &apps, None, 800).unwrap().root_pid, 3);
    }

    #[test]
    fn nombres() {
        assert!(is_relay("FxSound.exe"));
        assert!(is_relay("voicemeeter8x64.exe"));
        assert!(!is_relay("Spotify.exe"));
        assert_eq!(display_exe("chrome.exe"), "Chrome");
    }
}
