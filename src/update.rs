//! Actualizaciones: consulta el último Release de GitHub, baja el .exe nuevo,
//! verifica su SHA-256 y reemplaza el ejecutable en uso.
//!
//! Todo corre en hilos aparte; los fallos de red solo se registran y nunca
//! bloquean la app. El reemplazo funciona porque Windows deja renombrar un
//! .exe en ejecución (no borrarlo): el actual pasa a `Veeet.old.exe`, el nuevo
//! ocupa su lugar y el .old se borra en el siguiente arranque.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use serde::Deserialize;
use sha2::{Digest, Sha256};

const REPO: &str = "stkabir/veeet";
/// Nombre fijo del asset en cada Release (la landing enlaza a él).
const ASSET_NAME: &str = "Veeet-x64.exe";
const NEW_NAME: &str = "Veeet.new.exe";
const OLD_NAME: &str = "Veeet.old.exe";
/// Argumento con el que el proceso nuevo espera a que salga el anterior.
pub const WAIT_PID_ARG: &str = "--wait-pid";

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    pub version: String,
    url: String,
    sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading,
    Error(String),
}

#[derive(Deserialize)]
struct ApiRelease {
    tag_name: String,
    #[serde(default)]
    assets: Vec<ApiAsset>,
}

#[derive(Deserialize)]
struct ApiAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

pub struct Updater {
    state: Arc<Mutex<UpdateState>>,
    restart: Arc<AtomicBool>,
}

impl Updater {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(UpdateState::Idle)),
            restart: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn state(&self) -> UpdateState {
        self.state
            .lock()
            .map(|s| s.clone())
            .unwrap_or(UpdateState::Idle)
    }

    /// El reemplazo terminó y el proceso nuevo ya se lanzó: hay que cerrar.
    pub fn restart_requested(&self) -> bool {
        self.restart.load(Ordering::Relaxed)
    }

    fn set(state: &Mutex<UpdateState>, value: UpdateState) {
        if let Ok(mut s) = state.lock() {
            *s = value;
        }
    }

    /// Busca una versión nueva en segundo plano.
    pub fn check(&self) {
        {
            let Ok(mut s) = self.state.lock() else { return };
            if matches!(*s, UpdateState::Checking | UpdateState::Downloading) {
                return;
            }
            *s = UpdateState::Checking;
        }
        let state = self.state.clone();
        std::thread::spawn(move || {
            let next = match fetch_latest() {
                Ok(Some(release)) => {
                    log::info!("Versión nueva disponible: {}", release.version);
                    UpdateState::Available(release)
                }
                Ok(None) => UpdateState::UpToDate,
                Err(err) => {
                    log::warn!("No se pudo buscar actualizaciones: {err:#}");
                    UpdateState::Error("No se pudo buscar actualizaciones".into())
                }
            };
            Self::set(&state, next);
        });
    }

    /// Descarga la versión disponible y reemplaza el .exe.
    pub fn apply(&self) {
        let release = {
            let Ok(mut s) = self.state.lock() else { return };
            let UpdateState::Available(release) = &*s else {
                return;
            };
            let release = release.clone();
            *s = UpdateState::Downloading;
            release
        };
        let state = self.state.clone();
        let restart = self.restart.clone();
        std::thread::spawn(move || match download_and_replace(&release) {
            Ok(()) => restart.store(true, Ordering::Relaxed),
            Err(err) => {
                log::warn!("Falló la actualización: {err:#}");
                Self::set(&state, UpdateState::Error("Falló la actualización".into()));
            }
        });
    }
}

/// Borra restos de una actualización anterior. Llamar al arrancar.
pub fn cleanup_old() {
    if let Some(dir) = exe_dir() {
        let _ = std::fs::remove_file(dir.join(OLD_NAME));
        let _ = std::fs::remove_file(dir.join(NEW_NAME));
    }
}

/// Si el proceso se lanzó tras una actualización, espera a que salga el
/// anterior (comparten hotkeys globales y archivos de config).
pub fn wait_for_previous() {
    let mut args = std::env::args();
    while let Some(arg) = args.next() {
        if arg == WAIT_PID_ARG {
            if let Some(pid) = args.next().and_then(|p| p.parse::<u32>().ok()) {
                wait_for_pid(pid);
            }
            return;
        }
    }
}

fn wait_for_pid(pid: u32) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };
    unsafe {
        if let Ok(handle) = OpenProcess(PROCESS_SYNCHRONIZE, false, pid) {
            WaitForSingleObject(handle, 10_000);
            let _ = CloseHandle(handle);
        }
    }
}

fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe()
        .ok()?
        .parent()
        .map(Path::to_path_buf)
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(timeout))
        .user_agent(format!("veeet/{CURRENT_VERSION}"))
        .build()
        .into()
}

fn api_url() -> String {
    // Solo en debug: apunta a un Release de prueba.
    #[cfg(debug_assertions)]
    if let Some(url) = crate::app::env_var("UPDATE_URL") {
        return url;
    }
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

fn fetch_latest() -> Result<Option<Release>> {
    let agent = agent(Duration::from_secs(15));
    let api: ApiRelease = agent
        .get(&api_url())
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_json()?;

    if !is_newer(&api.tag_name, CURRENT_VERSION) {
        return Ok(None);
    }
    let asset = api
        .assets
        .iter()
        .find(|a| a.name == ASSET_NAME)
        .ok_or_else(|| anyhow!("el Release {} no trae {ASSET_NAME}", api.tag_name))?;

    let sha256 = match asset
        .digest
        .as_deref()
        .and_then(|d| d.strip_prefix("sha256:"))
    {
        Some(hex) => hex.to_string(),
        None => {
            let sum_name = format!("{ASSET_NAME}.sha256");
            let sum = api
                .assets
                .iter()
                .find(|a| a.name == sum_name)
                .ok_or_else(|| anyhow!("el Release {} no trae SHA-256", api.tag_name))?;
            let text = agent
                .get(&sum.browser_download_url)
                .call()?
                .body_mut()
                .read_to_string()?;
            text.split_whitespace()
                .next()
                .unwrap_or_default()
                .to_string()
        }
    };
    if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        bail!("SHA-256 inválido en el Release {}", api.tag_name);
    }

    Ok(Some(Release {
        version: api.tag_name.trim_start_matches('v').to_string(),
        url: asset.browser_download_url.clone(),
        sha256: sha256.to_ascii_lowercase(),
    }))
}

fn parse_version(v: &str) -> Option<(u32, u32, u32)> {
    let core = v.trim().trim_start_matches('v').split(['-', '+']).next()?;
    let mut parts = core.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

fn download_and_replace(release: &Release) -> Result<()> {
    let dir = exe_dir().context("no se encontró la carpeta del ejecutable")?;
    let exe = std::env::current_exe()?;
    let new = dir.join(NEW_NAME);
    let old = dir.join(OLD_NAME);

    if let Err(err) = download_verified(release, &new) {
        let _ = std::fs::remove_file(&new);
        return Err(err);
    }

    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).context("renombrando el ejecutable actual")?;
    if let Err(err) = std::fs::rename(&new, &exe) {
        // Deja todo como estaba.
        let _ = std::fs::rename(&old, &exe);
        let _ = std::fs::remove_file(&new);
        return Err(err).context("colocando el ejecutable nuevo");
    }

    if let Err(err) = std::process::Command::new(&exe)
        .arg(WAIT_PID_ARG)
        .arg(std::process::id().to_string())
        .spawn()
    {
        // No arrancó: volver al exe anterior para no dejar la app rota.
        let _ = std::fs::remove_file(&exe);
        let _ = std::fs::rename(&old, &exe);
        return Err(err).context("lanzando la versión nueva");
    }
    Ok(())
}

fn download_verified(release: &Release, dest: &Path) -> Result<()> {
    let agent = agent(Duration::from_secs(300));
    let mut response = agent.get(&release.url).call()?;
    let mut reader = response.body_mut().as_reader();
    let mut file = std::fs::File::create(dest).context("creando el archivo temporal")?;
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        file.write_all(&buf[..n])?;
    }
    file.flush()?;
    drop(file);

    let got = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if got != release.sha256 {
        bail!(
            "SHA-256 no coincide (esperado {}, obtenido {got})",
            release.sha256
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compara_versiones() {
        assert!(is_newer("v0.9.1", "0.9.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("v0.9.0", "0.9.0"));
        assert!(!is_newer("v0.8.9", "0.9.0"));
        assert!(!is_newer("basura", "0.9.0"));
    }
}
