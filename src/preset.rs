//! Presets por archivo.
//!
//! Cada preset guarda una copia completa de [`VisualCfg`]: no hay ajustes
//! visuales compartidos, así cambiar uno nunca toca a los demás.
//!
//! En disco (`%APPDATA%\veeet\presets`):
//! - `builtin\<motor>.toml`: un preset de fábrica editado. Si no existe, el
//!   preset usa los valores de fábrica.
//! - `user\<id>.toml`: los presets creados por el usuario.
//! - `market\<id>.toml`: presets importados de otros (el "marketplace"
//!   local). Son de solo lectura: para editarlos se duplican.
//!
//! Los presets son solo datos: todo lo que se lee de disco pasa por
//! [`Preset::sanitize`], que limita textos y valores.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{AudioCfg, Config, VisualCfg, VisualPreset};

/// Tamaño máximo de un archivo de preset.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
/// Extensión de los presets exportados.
pub const EXPORT_EXTENSION: &str = "veeet.toml";
const MAX_NAME: usize = 60;
const MAX_DESCRIPTION: usize = 500;
const MAX_TAGS: usize = 8;
const MAX_TAG: usize = 24;
const MAX_ID: usize = 64;
/// Espera tras el último cambio antes de escribir el preset activo.
const SAVE_DELAY: Duration = Duration::from_secs(1);
const BUILTIN_PREFIX: &str = "builtin-";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preset {
    pub id: String,
    pub name: String,
    pub author: String,
    pub description: String,
    pub tags: Vec<String>,
    /// Sube al publicar una versión nueva; al importar, una versión mayor
    /// reemplaza a la instalada.
    pub version: u32,
    /// Todos los ajustes del preset. `params.preset` es el motor.
    pub params: VisualCfg,
}

impl Default for Preset {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: "Sin nombre".to_string(),
            author: String::new(),
            description: String::new(),
            tags: Vec::new(),
            version: 1,
            params: VisualCfg::default(),
        }
    }
}

impl Preset {
    pub fn engine(&self) -> VisualPreset {
        self.params.preset
    }

    /// Limita textos, etiquetas y valores a rangos seguros.
    pub fn sanitize(&mut self) {
        self.id = sanitize_id(&self.id);
        self.name = truncate(self.name.trim(), MAX_NAME);
        if self.name.is_empty() {
            self.name = "Sin nombre".to_string();
        }
        self.author = truncate(self.author.trim(), MAX_NAME);
        self.description = truncate(&self.description, MAX_DESCRIPTION);
        self.tags = self
            .tags
            .iter()
            .map(|t| truncate(t.trim(), MAX_TAG).to_lowercase())
            .filter(|t| !t.is_empty())
            .take(MAX_TAGS)
            .collect();
        self.version = self.version.max(1);
        self.params.clamp();
    }
}

fn truncate(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect()
}

/// Id seguro para usar como nombre de archivo: `[a-z0-9_-]`.
pub fn sanitize_id(raw: &str) -> String {
    let mut id = String::new();
    for c in raw.trim().chars().flat_map(char::to_lowercase) {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            id.push(c);
        } else if (c.is_whitespace() || c == '.') && !id.ends_with('-') {
            id.push('-');
        }
        if id.len() >= MAX_ID {
            break;
        }
    }
    id.trim_matches('-').to_string()
}

/// Nombre visible de cada preset de fábrica.
pub fn builtin_name(engine: VisualPreset) -> &'static str {
    match engine {
        VisualPreset::WavyForm3 => "Onda",
        VisualPreset::WaveRing => "Anillo",
        VisualPreset::Alchemy => "Alquimia",
        VisualPreset::Ambience => "Ambiente",
        VisualPreset::Scope => "Trazo",
        VisualPreset::Niagara => "Niagara",
        VisualPreset::Windmill => "Molino",
        VisualPreset::XMarks => "X Marca",
        VisualPreset::Particles => "Partículas",
    }
}

pub fn builtin_id(engine: VisualPreset) -> String {
    format!("{BUILTIN_PREFIX}{}", engine.key())
}

fn builtin_preset(engine: VisualPreset, params: VisualCfg) -> Preset {
    Preset {
        id: builtin_id(engine),
        name: builtin_name(engine).to_string(),
        author: "Veeet".to_string(),
        params,
        ..Preset::default()
    }
}

/// Ajustes de un motor a partir de la sección `[visual]` vieja, que era una
/// sola para todos. Grid Tile se encendía solo para la onda al elegir
/// preset, así que solo el motor activo conserva el valor guardado.
pub fn migrate_legacy(legacy: &VisualCfg, audio: &AudioCfg, engine: VisualPreset) -> VisualCfg {
    let mut params = legacy.clone();
    params.preset = engine;
    if engine != legacy.preset {
        params.grid_tile = engine == VisualPreset::WavyForm3;
    }
    params.gain = audio.gain;
    params.spectrum_smoothing = audio.spectrum_smoothing;
    params.beat_sensitivity = audio.beat_sensitivity;
    params.clamp();
    params
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// De fábrica: se pueden ajustar, no renombrar ni borrar.
    Builtin,
    /// Creado por el usuario: todo editable.
    User,
    /// Importado: solo lectura.
    Market,
}

impl Origin {
    fn dir(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::User => "user",
            Self::Market => "market",
        }
    }
}

pub struct Entry {
    pub preset: Preset,
    pub origin: Origin,
}

impl Entry {
    pub fn params_editable(&self) -> bool {
        self.origin != Origin::Market
    }

    pub fn meta_editable(&self) -> bool {
        self.origin == Origin::User
    }

    /// Se puede borrar (los de fábrica no).
    pub fn removable(&self) -> bool {
        self.origin != Origin::Builtin
    }
}

/// Resultado de importar un archivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ImportOutcome {
    Added(String),
    /// Ya estaba instalado con una versión menor y se reemplazó.
    Updated(String),
    /// Ya estaba instalado con la misma versión o una mayor.
    Unchanged(String),
}

impl ImportOutcome {
    pub fn id(&self) -> &str {
        match self {
            Self::Added(id) | Self::Updated(id) | Self::Unchanged(id) => id,
        }
    }
}

pub struct PresetStore {
    root: PathBuf,
    entries: Vec<Entry>,
    active: usize,
    /// El preset activo cambió y falta escribirlo.
    dirty_since: Option<Instant>,
}

impl PresetStore {
    /// Carga los presets de `%APPDATA%\veeet\presets`, migra una config
    /// vieja y deja en `config.visual` los ajustes del preset activo.
    pub fn load(config: &mut Config) -> Self {
        Self::open(Config::dir().join("presets"), config)
    }

    pub fn open(root: PathBuf, config: &mut Config) -> Self {
        let builtin_dir = root.join(Origin::Builtin.dir());
        let legacy = config.legacy_visual.take();
        let mut entries = Vec::new();
        for engine in VisualPreset::all() {
            let path = builtin_dir.join(format!("{}.toml", engine.key()));
            let params = match read_preset(&path) {
                Ok(mut preset) => {
                    preset.params.preset = engine;
                    preset.params
                }
                Err(_) if !path.exists() => match &legacy {
                    // Primera vez con presets por archivo: la config vieja
                    // pasa a los presets de fábrica.
                    Some(legacy) => {
                        let params = migrate_legacy(legacy, &config.audio, engine);
                        if params != VisualCfg::defaults_for(engine) {
                            let preset = builtin_preset(engine, params.clone());
                            if let Err(err) = write_preset(&path, &preset) {
                                log::warn!("No se pudo migrar {}: {err:#}", path.display());
                            }
                        }
                        params
                    }
                    None => VisualCfg::defaults_for(engine),
                },
                Err(err) => {
                    log::warn!("Preset inválido ({}): {err:#}", path.display());
                    VisualCfg::defaults_for(engine)
                }
            };
            entries.push(Entry {
                preset: builtin_preset(engine, params),
                origin: Origin::Builtin,
            });
        }
        if let Some(legacy) = &legacy
            && config.active_preset.is_empty()
        {
            config.active_preset = builtin_id(legacy.preset);
        }

        let mut store = Self {
            root,
            entries,
            active: 0,
            dirty_since: None,
        };
        store.scan(Origin::User);
        store.scan(Origin::Market);
        store.active = store.find(&config.active_preset).unwrap_or(0);
        config.active_preset = store.active_id().to_string();
        config.visual = store.active_params().clone();
        store
    }

    /// Relee la carpeta de `origin` (los presets nuevos que haya dejado el
    /// usuario a mano). Conserva el preset activo si sigue existiendo.
    pub fn scan(&mut self, origin: Origin) {
        self.flush();
        let active_id = self.entries.get(self.active).map(|e| e.preset.id.clone());
        self.entries.retain(|e| e.origin != origin);
        let dir = self.root.join(origin.dir());
        let mut found: Vec<Preset> = Vec::new();
        if let Ok(read) = std::fs::read_dir(&dir) {
            for item in read.flatten() {
                let path = item.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                match read_preset(&path) {
                    Ok(mut preset) => {
                        if preset.id.is_empty() {
                            preset.id = file_id(&path);
                        }
                        if preset.id.is_empty()
                            || preset.id.starts_with(BUILTIN_PREFIX)
                            || self.find(&preset.id).is_some()
                            || found.iter().any(|p| p.id == preset.id)
                        {
                            log::warn!("Preset con id repetido o inválido: {}", path.display());
                            continue;
                        }
                        found.push(preset);
                    }
                    Err(err) => log::warn!("Preset inválido ({}): {err:#}", path.display()),
                }
            }
        }
        found.sort_by_key(|p| p.name.to_lowercase());
        // Orden fijo: fábrica, usuario, importados.
        let insert_at = match origin {
            Origin::Builtin => 0,
            Origin::User => self
                .entries
                .iter()
                .position(|e| e.origin == Origin::Market)
                .unwrap_or(self.entries.len()),
            Origin::Market => self.entries.len(),
        };
        self.entries.splice(
            insert_at..insert_at,
            found.into_iter().map(|preset| Entry { preset, origin }),
        );
        self.active = active_id.and_then(|id| self.find(&id)).unwrap_or(0);
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> &Entry {
        &self.entries[self.active]
    }

    pub fn active_id(&self) -> &str {
        &self.active().preset.id
    }

    pub fn active_params(&self) -> &VisualCfg {
        &self.active().preset.params
    }

    pub fn find(&self, id: &str) -> Option<usize> {
        self.entries.iter().position(|e| e.preset.id == id)
    }

    pub fn market_dir(&self) -> PathBuf {
        self.root.join(Origin::Market.dir())
    }

    /// Copia la copia de trabajo al preset activo. Llamar cada frame.
    pub fn sync_params(&mut self, params: &VisualCfg) {
        let entry = &mut self.entries[self.active];
        if !entry.params_editable() || entry.preset.params == *params {
            return;
        }
        let engine = entry.preset.params.preset;
        entry.preset.params = params.clone();
        // El motor es parte de la identidad del preset.
        entry.preset.params.preset = engine;
        self.mark_dirty();
    }

    /// Nombre, autor y demás del preset activo, si es del usuario. Tras
    /// cambiarlos, llamar a [`Self::mark_dirty`].
    pub fn active_meta_mut(&mut self) -> Option<&mut Preset> {
        let entry = &mut self.entries[self.active];
        entry.meta_editable().then_some(&mut entry.preset)
    }

    pub fn mark_dirty(&mut self) {
        self.dirty_since.get_or_insert_with(Instant::now);
    }

    /// Escribe el preset activo si lleva un rato sin cambios.
    pub fn maybe_flush(&mut self) {
        if self.dirty_since.is_some_and(|t| t.elapsed() >= SAVE_DELAY) {
            self.flush();
        }
    }

    /// Escribe ya el preset activo si tiene cambios.
    pub fn flush(&mut self) {
        if self.dirty_since.take().is_none() {
            return;
        }
        let Some(entry) = self.entries.get(self.active) else {
            return;
        };
        let path = self.path_of(entry);
        let result = match entry.origin {
            Origin::Builtin
                if entry.preset.params == VisualCfg::defaults_for(entry.preset.engine()) =>
            {
                match std::fs::remove_file(&path) {
                    Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
                    _ => Ok(()),
                }
            }
            Origin::Builtin | Origin::User => write_preset(&path, &entry.preset),
            Origin::Market => Ok(()),
        };
        if let Err(err) = result {
            log::warn!("No se pudo guardar el preset {}: {err:#}", path.display());
        }
    }

    /// Activa el preset `id` y devuelve sus ajustes.
    pub fn select(&mut self, id: &str) -> Option<VisualCfg> {
        let index = self.find(id)?;
        self.flush();
        self.active = index;
        Some(self.active_params().clone())
    }

    /// Crea un preset del usuario con los valores de fábrica de `engine`.
    pub fn create(&mut self, engine: VisualPreset) -> String {
        let name = format!("Mi {}", builtin_name(engine).to_lowercase());
        let preset = Preset {
            name,
            author: default_author(),
            params: VisualCfg::defaults_for(engine),
            ..Preset::default()
        };
        self.add_user(preset)
    }

    /// Copia editable del preset `index`.
    pub fn duplicate(&mut self, index: usize) -> Option<String> {
        self.flush();
        let source = &self.entries.get(index)?.preset;
        let preset = Preset {
            name: truncate(&format!("{} (copia)", source.name), MAX_NAME),
            author: if source.author.is_empty() || source.author == "Veeet" {
                default_author()
            } else {
                source.author.clone()
            },
            version: 1,
            ..source.clone()
        };
        Some(self.add_user(preset))
    }

    fn add_user(&mut self, mut preset: Preset) -> String {
        preset.id = self.new_id(&preset.name);
        preset.sanitize();
        let id = preset.id.clone();
        let at = self
            .entries
            .iter()
            .position(|e| e.origin == Origin::Market)
            .unwrap_or(self.entries.len());
        let entry = Entry {
            preset,
            origin: Origin::User,
        };
        if let Err(err) = write_preset(&self.path_of(&entry), &entry.preset) {
            log::warn!("No se pudo guardar el preset nuevo: {err:#}");
        }
        self.flush();
        self.entries.insert(at, entry);
        if self.active >= at {
            self.active += 1;
        }
        id
    }

    /// Borra un preset del usuario o importado. Si era el activo, queda
    /// activo el de fábrica del mismo motor.
    pub fn delete(&mut self, id: &str) -> bool {
        let Some(index) = self.find(id) else {
            return false;
        };
        if !self.entries[index].removable() {
            return false;
        }
        if index == self.active {
            self.dirty_since = None;
        } else {
            self.flush();
        }
        let path = self.path_of(&self.entries[index]);
        if let Err(err) = std::fs::remove_file(&path)
            && err.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("No se pudo borrar {}: {err:#}", path.display());
            return false;
        }
        let engine = self.entries[index].preset.engine();
        let was_active = index == self.active;
        self.entries.remove(index);
        if was_active {
            self.active = self.find(&builtin_id(engine)).unwrap_or(0);
        } else if self.active > index {
            self.active -= 1;
        }
        true
    }

    /// Escribe el preset `index` en `path` para compartirlo.
    pub fn export(&mut self, index: usize, path: &Path) -> Result<()> {
        self.flush();
        let entry = self.entries.get(index).context("preset inexistente")?;
        let mut preset = entry.preset.clone();
        if entry.origin == Origin::Builtin {
            // Un preset de fábrica exportado es un preset nuevo para quien
            // lo importa.
            preset.id = String::new();
        }
        write_preset(path, &preset)
    }

    /// Instala un archivo de preset en la galería local.
    pub fn import(&mut self, path: &Path) -> Result<ImportOutcome> {
        let mut preset = read_preset(path)?;
        if preset.id.is_empty() {
            preset.id = file_id(path);
        }
        let existing = self.find(&preset.id);
        let outcome = match existing.map(|i| &self.entries[i]) {
            Some(entry) if entry.origin == Origin::Market => {
                if preset.version <= entry.preset.version {
                    return Ok(ImportOutcome::Unchanged(preset.id));
                }
                ImportOutcome::Updated(preset.id.clone())
            }
            // Id libre, de fábrica o de un preset propio: entra como nuevo.
            other => {
                if other.is_some() || preset.id.is_empty() || preset.id.starts_with(BUILTIN_PREFIX)
                {
                    preset.id = self.new_id(&preset.name);
                }
                ImportOutcome::Added(preset.id.clone())
            }
        };
        let entry = Entry {
            preset,
            origin: Origin::Market,
        };
        write_preset(&self.path_of(&entry), &entry.preset)?;
        match existing.filter(|_| matches!(outcome, ImportOutcome::Updated(_))) {
            Some(index) => {
                if index == self.active {
                    self.dirty_since = None;
                }
                self.entries[index] = entry;
            }
            None => self.entries.push(entry),
        }
        Ok(outcome)
    }

    fn path_of(&self, entry: &Entry) -> PathBuf {
        let name = match entry.origin {
            Origin::Builtin => entry.preset.engine().key().to_string(),
            Origin::User | Origin::Market => entry.preset.id.clone(),
        };
        self.root
            .join(entry.origin.dir())
            .join(format!("{name}.toml"))
    }

    fn new_id(&self, name: &str) -> String {
        let mut base = sanitize_id(name);
        base.truncate(40);
        if base.is_empty() {
            base = "preset".to_string();
        }
        let mut seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        loop {
            let id = format!("{base}-{:06x}", seed & 0xff_ffff);
            if self.find(&id).is_none() {
                return id;
            }
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        }
    }
}

fn default_author() -> String {
    std::env::var("USERNAME").unwrap_or_default()
}

/// Id a partir del nombre del archivo (`foo.veeet.toml` → `foo`).
fn file_id(path: &Path) -> String {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    let stem = name
        .strip_suffix(".veeet.toml")
        .or_else(|| name.strip_suffix(".toml"))
        .unwrap_or(name);
    sanitize_id(stem)
}

pub fn read_preset(path: &Path) -> Result<Preset> {
    let size = std::fs::metadata(path)
        .with_context(|| format!("leyendo {}", path.display()))?
        .len();
    if size > MAX_FILE_BYTES {
        bail!("el archivo pesa {size} bytes (máximo {MAX_FILE_BYTES})");
    }
    let text =
        std::fs::read_to_string(path).with_context(|| format!("leyendo {}", path.display()))?;
    parse_preset(&text)
}

pub fn parse_preset(text: &str) -> Result<Preset> {
    if text.len() as u64 > MAX_FILE_BYTES {
        bail!("el preset es demasiado grande");
    }
    let mut value: toml::Table =
        toml::from_str(text).context("el archivo no es un preset válido")?;
    // Lo que falta en `[params]` toma el valor de fábrica de su motor, no el
    // de la onda: los archivos solo guardan lo que el motor usa.
    if let Some(toml::Value::Table(params)) = value.get_mut("params") {
        let engine = params
            .get("preset")
            .cloned()
            .and_then(|v| v.try_into::<VisualPreset>().ok())
            .unwrap_or_default();
        if let Ok(toml::Value::Table(defaults)) =
            toml::Value::try_from(VisualCfg::defaults_for(engine))
        {
            for (key, default) in defaults {
                params.entry(key).or_insert(default);
            }
        }
    }
    let mut preset: Preset = toml::Value::Table(value)
        .try_into()
        .context("el archivo no es un preset válido")?;
    preset.sanitize();
    Ok(preset)
}

/// Texto del archivo de un preset, sin los ajustes que su motor no usa.
pub fn preset_to_string(preset: &Preset) -> Result<String> {
    let mut value = toml::Value::try_from(preset)?;
    let engine = preset.engine();
    if let Some(toml::Value::Table(params)) = value.get_mut("params") {
        params.retain(|key, _| engine.uses(key));
    }
    Ok(toml::to_string_pretty(&value)?)
}

fn write_preset(path: &Path, preset: &Preset) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("creando {}", dir.display()))?;
    }
    let text = preset_to_string(preset)?;
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).with_context(|| format!("escribiendo {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("escribiendo {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("veeet-test-{name}-{nanos}"));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn round_trip() {
        let mut preset = Preset {
            id: "mi-preset".into(),
            name: "Mi preset".into(),
            tags: vec!["chill".into()],
            params: VisualCfg::defaults_for(VisualPreset::Alchemy),
            ..Preset::default()
        };
        preset.params.tint = [10, 20, 30];
        preset.params.mirror_x = true;
        let text = toml::to_string_pretty(&preset).unwrap();
        assert_eq!(parse_preset(&text).unwrap(), preset);
    }

    #[test]
    fn files_keep_only_used_keys() {
        let mut preset = Preset {
            id: "aro".into(),
            params: VisualCfg::defaults_for(VisualPreset::WaveRing),
            ..Preset::default()
        };
        preset.params.ring_radius = 1.2;
        preset.params.color_mode = crate::config::ColorMode::Custom;
        let text = preset_to_string(&preset).unwrap();
        assert!(text.contains("ring_radius"));
        assert!(!text.contains("alchemy_"));
        assert!(!text.contains("windmill_"));
        assert!(!text.contains("camera_z"));
        assert_eq!(parse_preset(&text).unwrap(), preset);
    }

    #[test]
    fn missing_keys_use_engine_defaults() {
        let text = "id = \"m\"
[params]
preset = \"windmill\"
";
        let preset = parse_preset(text).unwrap();
        assert_eq!(
            preset.params,
            VisualCfg::defaults_for(VisualPreset::Windmill)
        );
        assert!(!preset.params.grid_tile);
    }

    #[test]
    fn retired_hurricane_loads_as_ambience() {
        let text = "id = \"h\"
[params]
preset = \"hurricane\"
";
        let preset = parse_preset(text).unwrap();
        assert_eq!(preset.params.preset, VisualPreset::Ambience);
        assert_eq!(
            VisualPreset::from_alias("huracan"),
            Some(VisualPreset::Ambience)
        );
    }

    #[test]
    fn every_field_has_an_owner() {
        let toml::Value::Table(fields) = toml::Value::try_from(VisualCfg::default()).unwrap()
        else {
            panic!()
        };
        for key in fields.keys() {
            assert!(
                VisualPreset::all().iter().any(|e| e.uses(key)),
                "ningún motor usa `{key}`"
            );
        }
        for list in [
            crate::config::LIGHT_KEYS,
            crate::config::COLOR_KEYS,
            crate::config::GRID_KEYS,
            crate::config::AUDIO_KEYS,
        ] {
            for key in list {
                assert!(fields.contains_key(*key), "`{key}` no existe");
            }
        }
        for engine in VisualPreset::all() {
            for key in engine.keys() {
                assert!(fields.contains_key(*key), "`{key}` no existe");
            }
        }
    }

    #[test]
    fn reset_keys_only_touches_listed() {
        let mut p = VisualCfg::defaults_for(VisualPreset::Alchemy);
        p.exposure = 2.0;
        p.alchemy_speed = 3.0;
        p.reset_keys(&["exposure"]);
        assert_eq!(p.exposure, 1.0);
        assert_eq!(p.alchemy_speed, 3.0);
    }

    #[test]
    fn clamp_extremes() {
        let mut p = VisualCfg {
            sensitivity: f32::NAN,
            exposure: 1e9,
            hue_shift: -0.25,
            ring_bands: 999,
            ..VisualCfg::default()
        };
        p.clamp();
        assert_eq!(p.sensitivity, VisualCfg::default().sensitivity);
        assert_eq!(p.exposure, 3.0);
        assert!((p.hue_shift - 0.75).abs() < 1e-6);
        assert_eq!(p.ring_bands, 16);
    }

    #[test]
    fn sanitize_limits_text() {
        let mut preset = Preset {
            id: "../../evil name".into(),
            name: "x".repeat(500),
            tags: (0..50).map(|i| format!("t{i}")).collect(),
            ..Preset::default()
        };
        preset.sanitize();
        assert_eq!(preset.id, "evil-name");
        assert_eq!(preset.name.chars().count(), MAX_NAME);
        assert_eq!(preset.tags.len(), MAX_TAGS);
    }

    #[test]
    fn migrates_old_config() {
        let old = r#"
mode = "windowed"
[visual]
preset = "alchemy"
exposure = 1.7
glow = 2.5
grid_tile = false
alchemy_trail = 0.9
[audio]
gain = 2.0
beat_sensitivity = 0.9
"#;
        let mut config: Config = toml::from_str(old).unwrap();
        assert!(config.legacy_visual.is_some());
        let root = temp_root("migrate");
        let store = PresetStore::open(root.clone(), &mut config);
        assert_eq!(config.active_preset, "builtin-alchemy");
        assert_eq!(config.visual.preset, VisualPreset::Alchemy);
        assert_eq!(config.visual.alchemy_trail, 0.9);
        assert_eq!(config.visual.gain, 2.0);
        // Los valores compartidos pasan a cada preset por separado.
        let wave = &store.entries()[store.find("builtin-wavy_form3").unwrap()];
        assert_eq!(wave.preset.params.exposure, 1.7);
        assert!(wave.preset.params.grid_tile);
        assert!(root.join("builtin").join("alchemy.toml").exists());
        // `[visual]` ya no se escribe.
        assert!(!toml::to_string(&config).unwrap().contains("[visual]"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn presets_are_independent() {
        let root = temp_root("independent");
        let mut config = Config::default();
        let mut store = PresetStore::open(root.clone(), &mut config);
        let mut edited = config.visual.clone();
        edited.exposure = 2.5;
        store.sync_params(&edited);
        let ring = store.select("builtin-wave_ring").unwrap();
        assert_eq!(ring.exposure, VisualCfg::default().exposure);
        let wave = store.select("builtin-wavy_form3").unwrap();
        assert_eq!(wave.exposure, 2.5);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn create_duplicate_delete() {
        let root = temp_root("crud");
        let mut config = Config::default();
        let mut store = PresetStore::open(root.clone(), &mut config);
        let id = store.create(VisualPreset::Windmill);
        assert!(root.join("user").join(format!("{id}.toml")).exists());
        let copy = store.duplicate(store.find(&id).unwrap()).unwrap();
        assert_ne!(id, copy);
        store.select(&copy).unwrap();
        assert!(store.delete(&copy));
        assert_eq!(store.active_id(), "builtin-windmill");
        assert!(!store.delete("builtin-windmill"));

        // Al reabrir, el preset del usuario sigue ahí.
        let reopened = PresetStore::open(root.clone(), &mut Config::default());
        assert!(reopened.find(&id).is_some());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn import_rules() {
        let root = temp_root("import");
        let mut store = PresetStore::open(root.join("presets"), &mut Config::default());

        let mut shared = Preset {
            id: "nebula".into(),
            name: "Nébula".into(),
            author: "ana".into(),
            params: VisualCfg::defaults_for(VisualPreset::Ambience),
            ..Preset::default()
        };
        let file = root.join("nebula.veeet.toml");
        write_preset(&file, &shared).unwrap();
        assert_eq!(
            store.import(&file).unwrap(),
            ImportOutcome::Added("nebula".into())
        );
        assert_eq!(
            store.import(&file).unwrap(),
            ImportOutcome::Unchanged("nebula".into())
        );
        shared.version = 2;
        write_preset(&file, &shared).unwrap();
        assert_eq!(
            store.import(&file).unwrap(),
            ImportOutcome::Updated("nebula".into())
        );
        assert_eq!(
            store
                .entries()
                .iter()
                .filter(|e| e.preset.id == "nebula")
                .count(),
            1
        );

        // Corrupto y demasiado grande: se rechazan.
        let corrupt = root.join("corrupt.veeet.toml");
        std::fs::write(&corrupt, "esto = [no es toml").unwrap();
        assert!(store.import(&corrupt).is_err());
        let big = root.join("big.veeet.toml");
        std::fs::write(&big, "#".repeat(MAX_FILE_BYTES as usize + 1)).unwrap();
        assert!(store.import(&big).is_err());

        // Los importados no se editan.
        store.select("nebula").unwrap();
        let mut edited = store.active_params().clone();
        edited.exposure = 0.1;
        store.sync_params(&edited);
        assert_ne!(store.active_params().exposure, 0.1);
        let _ = std::fs::remove_dir_all(root);
    }
}
