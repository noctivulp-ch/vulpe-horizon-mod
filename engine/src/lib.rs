//! vulpe-engine: Motor y lógica central del mod nativo de Vulpe para Horizon.
//!
//! Implementa la especificación de `frame.mods` v1 descrita en `vulpe-horizon-spec/flexibilidad/mod-nativo.md`.

pub mod clima;
pub mod config;
pub mod constants;
pub mod table;

pub use config::{FixedChannelMode, VulpeConfig};
pub use constants::{
    CONSTANTS_WGSL_FILENAME, PACK_MANIFEST_FILENAME, PackWriteResult, UNPACKED_PACK_DIR_NAME,
    UNPACKED_PACK_KEY, UNPACKED_STAMP_FILENAME, compute_pack_source_stamp, ensure_unpacked_hshader,
    ensure_unpacked_pack, find_source_hshader_in_dir, find_source_pack_in_dir,
    generate_constants_wgsl, generate_default_constants_wgsl, generate_full_constants_wgsl,
    is_hshader_path, parse_manifest_id_str, read_pack_manifest_id, resolve_shader_pack_in_dir,
    write_pack_constants, write_pack_constants_with_mode,
};
pub use table::{ParamDef, PARAM_TABLE, find_param_by_id, find_param_by_name};

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};

/// Firma base de la cabecera del formato: 1000 + versión_formato.
pub const FORMAT_VERSION: f32 = 2.0;
pub const FORMAT_HEADER_SIG: f32 = 1000.0 + FORMAT_VERSION; // 1002.0 (formato 2: bioma en mods[1].w)

/// Versión actual de la tabla unificada de parámetros.
pub const PARAM_TABLE_VERSION: f32 = 1.0;

/// Banderas de validez en `mods[0].z` (codificadas como bits en un entero f32 exacto).
pub const FLAG_TIME_VALID: u32 = 1 << 0; // Bit 0 (1): datos de tiempo válidos
pub const FLAG_LIGHTNING_VALID: u32 = 1 << 1; // Bit 1 (2): rayo válido
pub const FLAG_SEASON_VALID: u32 = 1 << 2; // Bit 2 (4): estación válida
pub const FLAG_PLAYER_VALID: u32 = 1 << 3; // Bit 3 (8): estado del jugador válido
pub const FLAG_FIXED_CHANNEL_ACTIVE: u32 = 1 << 4; // Bit 4 (16): canal fijo en uso
/// Formato 2: bioma de la cámara válido en `mods[1].w`. La especificación lo pone en el bit 4, que ya era el canal fijo
/// en el formato 1: va en el 5 (docs/decisiones.md, pregunta abierta a A).
pub const FLAG_BIOME_VALID: u32 = 1 << 5; // Bit 5 (32)
/// Lluvia y nieve como partículas del mod encendidas: el pack no dibuja las suyas (subsistemas/26). El bit 6 queda para
/// la altura de entrada al agua del formato 2.
pub const FLAG_PARTICLE_WEATHER: u32 = 1 << 7; // Bit 7 (128)

/// Segundos que tarda el peso del bioma en subir de 0 a 1 o en bajar de 1 a 0 al cruzar una frontera.
pub const BIOME_FADE_SECONDS: f32 = 4.0;

/// Duración por defecto de cada estación en días del juego (24 días).
pub const DEFAULT_SEASON_DAYS: f64 = 24.0;

/// Cantidad de estaciones en un año solar completo (primavera, verano, otoño, invierno).
pub const SEASONS_PER_YEAR: f64 = 4.0;

/// Ticks de Minecraft en un día completo (24 000 ticks).
pub const TICKS_PER_DAY: f64 = 24_000.0;

/// Módulo para el número de día del mundo (2^24 = 16 777 216).
pub const DAY_MODULO: i64 = 16_777_216;

/// Tiempo máximo en segundos durante el cual un rayo se considera activo para el shader (10.0 s).
pub const LIGHTNING_ACTIVE_SECONDS: f32 = 10.0;

/// Cantidad máxima de ajustes en vivo simultáneos en el formato v1 (16 floats: mods[4..8]).
pub const MAX_LIVE_ADJUSTMENTS: usize = 16;

/// Registro en memoria del último rayo ocurrido en el mundo.
#[derive(Clone, Copy, Debug)]
pub struct LightningRecord {
    pub pos: [f64; 3],
    pub time: Instant,
}

/// Ajuste en vivo de un parámetro individual.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiveAdjustment {
    pub id: u32,
    pub value: f32,
}

static LAST_LIGHTNING: Mutex<Option<LightningRecord>> = Mutex::new(None);
static CAMERA_BIOME: Mutex<Option<BiomeFade>> = Mutex::new(None);

/// Categoría del bioma de la cámara con su peso suavizado en el tiempo (formato 2, `mods[1].w`).
#[derive(Clone, Copy, Debug)]
struct BiomeFade {
    category: u32, // 0 desconocido, 1 pantano, 2 jungla, 3 jardín pálido
    weight: f32,   // 0..1
    last: Instant,
}

/// Categoría de niebla local (subsistema 21) de un bioma por su nombre (`minecraft:swamp`): 1 pantano, 2 jungla,
/// 3 jardín pálido, 0 los demás.
pub fn biome_category(name: &str) -> u32 {
    let n = name.rsplit(':').next().unwrap_or(name);
    match n {
        "swamp" | "mangrove_swamp" => 1,
        "jungle" | "sparse_jungle" | "bamboo_jungle" => 2,
        "pale_garden" => 3,
        _ => 0,
    }
}

/// Registra el bioma en la cámara de este fotograma (None si no se sabe) y suaviza su peso.
pub fn observe_camera_biome(name: Option<&str>) {
    observe_camera_biome_at(name, Instant::now());
}

pub fn observe_camera_biome_at(name: Option<&str>, now: Instant) {
    let Ok(mut lock) = CAMERA_BIOME.lock() else { return };
    let Some(name) = name else {
        *lock = None;
        return;
    };
    let target = biome_category(name);
    let mut st = lock.unwrap_or(BiomeFade { category: target, weight: 0.0, last: now });
    let dt = now.saturating_duration_since(st.last).as_secs_f32().min(1.0);
    let step = dt / BIOME_FADE_SECONDS;
    if st.category == target {
        st.weight = (st.weight + step).min(1.0);
    } else {
        // Primero se apaga la categoría anterior; al llegar a 0 se cambia a la nueva.
        st.weight -= step;
        if st.weight <= 0.0 {
            st.category = target;
            st.weight = 0.0;
        }
    }
    st.last = now;
    *lock = Some(st);
}

/// `categoría + peso` (peso 0..0,99) para `mods[1].w`, o None si no hay bioma observado.
pub fn camera_biome_param() -> Option<f32> {
    let st = (*CAMERA_BIOME.lock().ok()?)?;
    Some(st.category as f32 + st.weight.min(0.99))
}
static LAST_CLIENT_BOLT_ID: Mutex<Option<u32>> = Mutex::new(None);
static LIVE_ADJUSTMENTS: Mutex<Vec<LiveAdjustment>> = Mutex::new(Vec::new());
static FIXED_CHANNEL_ACTIVE: Mutex<bool> = Mutex::new(false);
static PENDING_UNPACKED_PACK_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static PENDING_PACK_REVERT: Mutex<bool> = Mutex::new(false);

/// Obtiene y consume (si existe) la ruta de la carpeta desempaquetada (`shaderpacks/vulpe-mod/`)
/// que la Forma C solicita seleccionar como pack activo en el cliente.
pub fn take_pending_unpacked_pack_path() -> Option<PathBuf> {
    PENDING_UNPACKED_PACK_PATH
        .lock()
        .ok()
        .and_then(|mut lock| lock.take())
}

/// Consulta sin consumir la última carpeta desempaquetada registrada por la Forma C.
pub fn peek_pending_unpacked_pack_path() -> Option<PathBuf> {
    PENDING_UNPACKED_PACK_PATH
        .lock()
        .ok()
        .and_then(|lock| lock.clone())
}

fn set_pending_unpacked_pack_path(path: Option<PathBuf>) {
    if let Ok(mut lock) = PENDING_UNPACKED_PACK_PATH.lock() {
        *lock = path;
    }
}

/// Obtiene y consume si el modo B solicita revertir `horizon:vulpe_mod` de vuelta a `horizon:vulpe`.
pub fn take_pending_pack_revert() -> bool {
    PENDING_PACK_REVERT
        .lock()
        .ok()
        .map(|mut lock| {
            let v = *lock;
            *lock = false;
            v
        })
        .unwrap_or(false)
}

fn set_pending_pack_revert(revert: bool) {
    if let Ok(mut lock) = PENDING_PACK_REVERT.lock() {
        *lock = revert;
    }
}

const RUNTIME_SYNC_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Default)]
struct RuntimeSyncState {
    initialized: bool,
    last_check: Option<Instant>,
    last_config_stamp: Option<(u64, Option<SystemTime>)>,
    last_source_stamp: Option<String>,
    last_pack_path: Option<PathBuf>,
}

static RUNTIME_SYNC: Mutex<RuntimeSyncState> = Mutex::new(RuntimeSyncState {
    initialized: false,
    last_check: None,
    last_config_stamp: None,
    last_source_stamp: None,
    last_pack_path: None,
});

/// Activa o desactiva la bandera del canal fijo (archivo de constantes WGSL del pack).
pub fn set_fixed_channel_active(active: bool) {
    if let Ok(mut lock) = FIXED_CHANNEL_ACTIVE.lock() {
        *lock = active;
    }
}

/// Indica si el canal fijo está activo en la cabecera del formato.
pub fn is_fixed_channel_active() -> bool {
    FIXED_CHANNEL_ACTIVE.lock().map(|lock| *lock).unwrap_or(false)
}

/// Establece un ajuste en vivo por su ID numérico. Si ya existe, actualiza su valor.
pub fn set_live_adjustment(id: u32, value: f32) -> bool {
    if find_param_by_id(id).is_none() {
        return false;
    }
    if let Ok(mut lock) = LIVE_ADJUSTMENTS.lock() {
        if let Some(existing) = lock.iter_mut().find(|a| a.id == id) {
            existing.value = value;
        } else {
            lock.push(LiveAdjustment { id, value });
        }
        true
    } else {
        false
    }
}

/// Establece un ajuste en vivo por su nombre textual.
pub fn set_live_adjustment_by_name(name: &str, value: f32) -> bool {
    if let Some(param) = find_param_by_name(name) {
        set_live_adjustment(param.id, value)
    } else {
        false
    }
}

/// Elimina un ajuste en vivo por su ID numérico.
pub fn remove_live_adjustment(id: u32) {
    if let Ok(mut lock) = LIVE_ADJUSTMENTS.lock() {
        lock.retain(|a| a.id != id);
    }
}

/// Limpia todos los ajustes en vivo activos.
pub fn clear_live_adjustments() {
    if let Ok(mut lock) = LIVE_ADJUSTMENTS.lock() {
        lock.clear();
    }
}

/// Obtiene una copia de los ajustes en vivo registrados actualmente.
pub fn get_live_adjustments() -> Vec<LiveAdjustment> {
    LIVE_ADJUSTMENTS.lock().map(|lock| lock.clone()).unwrap_or_default()
}

/// Registra el impacto de un rayo desde coordenadas enteras de bloque `[i32; 3]`.
pub fn record_lightning_strike(pos: [i32; 3]) {
    record_lightning_strike_pos([pos[0] as f64, pos[1] as f64, pos[2] as f64]);
}

/// Registra el impacto de un rayo con precisión f64 en coordenadas de mundo.
pub fn record_lightning_strike_pos(pos: [f64; 3]) {
    record_lightning_strike_with_time(pos, Instant::now());
}

/// Registra un rayo indicando un instante específico (útil para pruebas unitarias).
pub fn record_lightning_strike_with_time(pos: [f64; 3], time: Instant) {
    if let Ok(mut lock) = LAST_LIGHTNING.lock() {
        *lock = Some(LightningRecord { pos, time });
    }
}

/// Observa el último rayo activo en la lista cliente `game.bolts` (funciona tanto en un jugador
/// como conectado a un servidor remoto, para `/summon lightning_bolt`, `/weather thunder`,
/// tridentes y caballos trampa). Solo reinicia el reloj cuando aparece un `bolt_id` nuevo.
pub fn observe_client_bolt(bolt_id: u32, pos: [f64; 3]) {
    if let Ok(mut id_lock) = LAST_CLIENT_BOLT_ID.lock() {
        if *id_lock == Some(bolt_id) {
            return;
        }
        *id_lock = Some(bolt_id);
    }
    if let Ok(mut lock) = LAST_LIGHTNING.lock() {
        // Si el gancho de servidor `native_lightning_strike` acaba de registrar este mismo rayo
        // hace pocos milisegundos, afinamos la posición a f64 conservando su instante inicial.
        if let Some(existing) = lock.as_mut() {
            if existing.time.elapsed() < Duration::from_millis(250) {
                existing.pos = pos;
                return;
            }
        }
        *lock = Some(LightningRecord {
            pos,
            time: Instant::now(),
        });
    }
}

/// Obtiene el registro del último rayo si existe.
pub fn get_last_lightning_strike() -> Option<LightningRecord> {
    LAST_LIGHTNING.lock().ok().and_then(|lock| *lock)
}

/// Limpia el registro del último rayo.
pub fn clear_lightning_strike() {
    if let Ok(mut lock) = LAST_LIGHTNING.lock() {
        *lock = None;
    }
    if let Ok(mut id_lock) = LAST_CLIENT_BOLT_ID.lock() {
        *id_lock = None;
    }
}

fn file_stamp(path: &Path) -> Option<(u64, Option<SystemTime>)> {
    let meta = std::fs::metadata(path).ok()?;
    Some((meta.len(), meta.modified().ok()))
}

/// Extrae los parámetros de `config` cuyos valores difieren de su valor por defecto en `PARAM_TABLE`.
pub fn modified_params_from_config(config: &VulpeConfig) -> Vec<LiveAdjustment> {
    let mut out = Vec::new();
    for param in PARAM_TABLE {
        let val = config.get(param.name);
        if (val - param.default).abs() > 1e-5 {
            out.push(LiveAdjustment {
                id: param.id,
                value: val,
            });
        }
    }
    out
}

/// Sincroniza periódicamente (cada 500 ms o al forzar) `config/vulpe-horizon.toml` con:
/// 1. Los 16 ajustes en vivo de `frame.mods[4..7]` (`mods[0].w`).
/// 2. El canal fijo `constantes_mod.wgsl` en la carpeta del pack activo (`mods[0].z` bit 4).
pub fn sync_runtime_state(
    config_dir: Option<&Path>,
    active_pack_path: Option<&Path>,
    force: bool,
) {
    let Some(cfg_dir) = config_dir else {
        return;
    };

    let now = Instant::now();
    let pack_buf = active_pack_path
        .filter(|p| !p.as_os_str().is_empty())
        .map(Path::to_path_buf);

    let (should_sync, was_initialized) = {
        let Ok(mut state) = RUNTIME_SYNC.lock() else {
            return;
        };
        let pack_changed = state.last_pack_path != pack_buf;
        let time_due = state
            .last_check
            .is_none_or(|t| now.saturating_duration_since(t) >= RUNTIME_SYNC_INTERVAL);

        if !force && state.initialized && !pack_changed && !time_due {
            return;
        }
        state.last_check = Some(now);

        let cfg_file = cfg_dir.join(config::CONFIG_FILE_NAME);
        if !cfg_file.exists() {
            let _ = VulpeConfig::load_or_create(cfg_dir);
        }
        let current_stamp = file_stamp(&cfg_file);
        let stamp_changed = state.last_config_stamp != current_stamp;

        let source_stamp = pack_buf.as_deref().and_then(|p| {
            if p.file_name().and_then(|n| n.to_str()) == Some(UNPACKED_PACK_DIR_NAME) {
                p.parent()
                    .and_then(find_source_pack_in_dir)
                    .and_then(|src| compute_pack_source_stamp(&src).ok())
            } else if p.exists() {
                compute_pack_source_stamp(p).ok()
            } else {
                None
            }
        });
        let source_changed = state.last_source_stamp != source_stamp;
        let was_init = state.initialized;

        state.initialized = true;
        state.last_config_stamp = current_stamp;
        state.last_source_stamp = source_stamp;
        state.last_pack_path = pack_buf.clone();

        (
            force || !was_init || stamp_changed || source_changed || pack_changed,
            was_init,
        )
    };

    if !should_sync {
        return;
    }

    let Ok(cfg) = VulpeConfig::load_or_create(cfg_dir) else {
        return;
    };

    // Sección [clima] (clima con partículas, port de Particle Rain): se lee del mismo archivo; si falta, se añade con
    // los valores por defecto para que el jugador la vea.
    let cfg_file = cfg_dir.join(config::CONFIG_FILE_NAME);
    if let Ok(texto) = std::fs::read_to_string(&cfg_file) {
        clima::set_config(clima::ClimaConfig::from_toml_str(&texto));
        if !texto.lines().any(|l| l.trim() == "[clima]") {
            let _ = std::fs::write(&cfg_file, format!("{}{}", texto, clima::config().to_toml_section()));
        }
    }

    let modified = modified_params_from_config(&cfg);

    // Actualizar los ajustes en vivo cuando hay parámetros modificados en el TOML,
    // o cuando el archivo fue editado/forzado tras haber estado inicializado.
    if !modified.is_empty() || force || was_initialized {
        if let Ok(mut lock) = LIVE_ADJUSTMENTS.lock() {
            *lock = modified.clone();
        }
    }

    if !modified.is_empty() {
        if let Some(pack_path) = pack_buf.as_deref() {
            match write_pack_constants(pack_path, &cfg) {
                PackWriteResult::UnpackedAndWritten { unpacked_dir, .. } => {
                    set_pending_unpacked_pack_path(Some(unpacked_dir));
                    set_pending_pack_revert(false);
                    set_fixed_channel_active(true);
                }
                PackWriteResult::Disabled => {
                    set_pending_unpacked_pack_path(None);
                    let is_unpacked = pack_path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n == UNPACKED_PACK_DIR_NAME);
                    set_pending_pack_revert(is_unpacked);
                    set_fixed_channel_active(false);
                }
                _ => {
                    set_pending_unpacked_pack_path(None);
                    set_pending_pack_revert(false);
                    set_fixed_channel_active(false);
                }
            }
        } else {
            set_pending_unpacked_pack_path(None);
            set_pending_pack_revert(false);
            set_fixed_channel_active(false);
        }
    } else {
        if let Some(pack_path) = pack_buf.as_deref() {
            let is_unpacked = pack_path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n == UNPACKED_PACK_DIR_NAME);
            if cfg.canal_fijo == FixedChannelMode::Apagado {
                set_pending_unpacked_pack_path(None);
                set_pending_pack_revert(is_unpacked);
            } else if is_unpacked || pack_path.exists() {
                if let PackWriteResult::UnpackedAndWritten { unpacked_dir, .. } =
                    write_pack_constants(pack_path, &cfg)
                {
                    set_pending_unpacked_pack_path(Some(unpacked_dir));
                    set_pending_pack_revert(false);
                } else {
                    set_pending_unpacked_pack_path(None);
                }
            } else {
                set_pending_unpacked_pack_path(None);
            }
        } else {
            set_pending_unpacked_pack_path(None);
        }
        set_fixed_channel_active(false);
    }
}

/// Procesa comandos del cliente `/vulpe` (API 35: `native_commands` / `native_command`).
/// Devuelve `Some(respuesta)` si el comando pertenecía a `vulpe`, o `None` en caso contrario.
pub fn execute_client_command(
    command: &str,
    config_dir: Option<&Path>,
    active_pack_path: Option<&Path>,
) -> Option<String> {
    let trimmed = command.trim();
    if trimmed != "vulpe" && !trimmed.starts_with("vulpe ") {
        return None;
    }

    let parts: Vec<&str> = trimmed.split_whitespace().collect();
    let sub = parts.get(1).copied().unwrap_or("estado");

    match sub {
        "estado" => {
            let live = get_live_adjustments();
            let live_count = live.len().min(MAX_LIVE_ADJUSTMENTS);
            let fixed = is_fixed_channel_active();
            let (lightning_active, lightning_info) = match get_last_lightning_strike() {
                Some(strike) => {
                    let age = strike.time.elapsed().as_secs_f32();
                    if age >= 0.0 && age < LIGHTNING_ACTIVE_SECONDS {
                        (
                            true,
                            format!(
                                "activo (pos=({:.1}, {:.1}, {:.1}), edad={:.2}s)",
                                strike.pos[0], strike.pos[1], strike.pos[2], age
                            ),
                        )
                    } else {
                        (false, "inactivo (-1.0s)".to_string())
                    }
                }
                None => (false, "inactivo (-1.0s)".to_string()),
            };
            let mut flags = FLAG_TIME_VALID | FLAG_SEASON_VALID;
            if lightning_active {
                flags |= FLAG_LIGHTNING_VALID;
            }
            if fixed {
                flags |= FLAG_FIXED_CHANNEL_ACTIVE;
            }
            let modo_cfg = config_dir
                .and_then(|d| VulpeConfig::load_or_create(d).ok())
                .map(|c| c.canal_fijo.as_str())
                .unwrap_or("c");
            Some(format!(
                "[Vulpe] mods[0]=({:.0}, {:.0}, {:.0}, {:.0}) | canal_fijo={} (modo={}) | en_vivo={}/16 | rayo={}",
                FORMAT_HEADER_SIG, PARAM_TABLE_VERSION, flags as f32, live_count as f32, fixed, modo_cfg, live_count, lightning_info
            ))
        }
        "modo" => {
            let Some(&mode_str) = parts.get(2) else {
                let current = config_dir
                    .and_then(|d| VulpeConfig::load_or_create(d).ok())
                    .map(|c| c.canal_fijo.as_str())
                    .unwrap_or("c");
                return Some(format!(
                    "[Vulpe] Modo de canal fijo actual: '{current}'. Uso: /vulpe modo <c|apagado>"
                ));
            };
            let Some(mode) = FixedChannelMode::from_str_opt(mode_str) else {
                return Some(format!(
                    "[Vulpe] Modo inválido: '{mode_str}'. Use 'c' o 'apagado'."
                ));
            };
            if let Some(dir) = config_dir {
                if let Ok(mut cfg) = VulpeConfig::load_or_create(dir) {
                    cfg.canal_fijo = mode;
                    let _ = cfg.save(dir);
                }
                sync_runtime_state(Some(dir), active_pack_path, true);
            }
            let fixed = is_fixed_channel_active();
            let flags = FLAG_TIME_VALID
                | FLAG_SEASON_VALID
                | if fixed { FLAG_FIXED_CHANNEL_ACTIVE } else { 0 };
            Some(format!(
                "[Vulpe] Modo de canal fijo cambiado a '{}' | canal_fijo={} | banderas={:.0}",
                mode.as_str(),
                fixed,
                flags as f32
            ))
        }
        "ver" => {
            let Some(&name) = parts.get(2) else {
                return Some("Uso: /vulpe ver <parámetro>".to_string());
            };
            let Some(param) = find_param_by_name(name) else {
                return Some(format!("[Vulpe] Parámetro desconocido: '{name}'"));
            };
            let cfg_val = config_dir
                .and_then(|d| VulpeConfig::load_or_create(d).ok())
                .map(|c| c.get(param.name))
                .unwrap_or(param.default);
            let live_val = get_live_adjustments()
                .into_iter()
                .find(|a| a.id == param.id)
                .map(|a| a.value);
            let current = live_val.unwrap_or(cfg_val);
            let encoded = param.encode_live(current);
            Some(format!(
                "[Vulpe] {} (id {}, {}): actual={:.4} (defecto={:.4}, rango={:.4}..={:.4}, vivo={:.0})",
                param.name, param.id, param.subsystem, current, param.default, param.min, param.max, encoded
            ))
        }
        "poner" => {
            let (Some(&name), Some(&val_str)) = (parts.get(2), parts.get(3)) else {
                return Some("Uso: /vulpe poner <parámetro> <valor>".to_string());
            };
            let Some(param) = find_param_by_name(name) else {
                return Some(format!("[Vulpe] Parámetro desconocido: '{name}'"));
            };
            let Ok(raw_val) = val_str.parse::<f32>() else {
                return Some(format!("[Vulpe] Valor numérico inválido: '{val_str}'"));
            };
            let clamped = raw_val.clamp(param.min, param.max);
            if let Some(dir) = config_dir {
                if let Ok(mut cfg) = VulpeConfig::load_or_create(dir) {
                    cfg.set(param.name, clamped);
                    let _ = cfg.save(dir);
                }
                sync_runtime_state(Some(dir), active_pack_path, true);
            } else {
                set_live_adjustment(param.id, clamped);
            }
            let encoded = param.encode_live(clamped);
            let live_count = get_live_adjustments().len().min(MAX_LIVE_ADJUSTMENTS);
            let fixed = is_fixed_channel_active();
            let flags = FLAG_TIME_VALID
                | FLAG_SEASON_VALID
                | if fixed { FLAG_FIXED_CHANNEL_ACTIVE } else { 0 };
            Some(format!(
                "[Vulpe] {} = {:.4} (id={}, código={:.0}) | banderas={:.0}, en_vivo={}",
                param.name, clamped, param.id, encoded, flags as f32, live_count
            ))
        }
        "vivo" => {
            let (Some(&name), Some(&val_str)) = (parts.get(2), parts.get(3)) else {
                return Some("Uso: /vulpe vivo <parámetro> <valor>".to_string());
            };
            let Some(param) = find_param_by_name(name) else {
                return Some(format!("[Vulpe] Parámetro desconocido: '{name}'"));
            };
            let Ok(raw_val) = val_str.parse::<f32>() else {
                return Some(format!("[Vulpe] Valor numérico inválido: '{val_str}'"));
            };
            let clamped = raw_val.clamp(param.min, param.max);
            set_live_adjustment(param.id, clamped);
            let encoded = param.encode_live(clamped);
            let live_count = get_live_adjustments().len().min(MAX_LIVE_ADJUSTMENTS);
            Some(format!(
                "[Vulpe] En vivo: {} = {:.4} (id={}, código={:.0}, total_en_vivo={})",
                param.name, clamped, param.id, encoded, live_count
            ))
        }
        "restablecer" => {
            let sub_filter = parts.get(2).copied();
            if let Some(dir) = config_dir {
                if let Ok(mut cfg) = VulpeConfig::load_or_create(dir) {
                    for p in PARAM_TABLE {
                        if sub_filter.is_none_or(|s| p.subsystem == s) {
                            cfg.set(p.name, p.default);
                        }
                    }
                    let _ = cfg.save(dir);
                }
                sync_runtime_state(Some(dir), active_pack_path, true);
            } else {
                clear_live_adjustments();
                set_fixed_channel_active(false);
            }
            Some("[Vulpe] Ajustes restablecidos a sus valores por defecto (banderas=5, en_vivo=0).".to_string())
        }
        _ => Some(
            "Comandos Vulpe: /vulpe estado | /vulpe modo <c|apagado> | /vulpe ver <param> | /vulpe poner <param> <valor> | /vulpe vivo <param> <valor> | /vulpe restablecer".to_string(),
        ),
    }
}

/// Rellena el arreglo `params` (8 vec4s, 32 floats) según el formato v1 de `frame.mods`.
pub fn populate_shader_params(
    day_time: Option<i64>,
    partial: f32,
    params: &mut [[f32; 4]; 8],
) {
    populate_shader_params_full(day_time, partial, None, params);
}

/// Rellena el arreglo `params` con soporte para cámara (cálculo de rayo relativo).
pub fn populate_shader_params_full(
    day_time: Option<i64>,
    partial: f32,
    camera_pos: Option<[f64; 3]>,
    params: &mut [[f32; 4]; 8],
) {
    let live = get_live_adjustments();
    populate_shader_params_live(day_time, partial, camera_pos, Some(&live), params);
}

/// Rellena el arreglo `params` permitiendo pasar ajustes en vivo explícitos.
pub fn populate_shader_params_live(
    day_time: Option<i64>,
    partial: f32,
    camera_pos: Option<[f64; 3]>,
    live: Option<&[LiveAdjustment]>,
    params: &mut [[f32; 4]; 8],
) {
    // Limpiar todo a ceros primero
    for slot in params.iter_mut() {
        *slot = [0.0; 4];
    }

    // mods[2]: rayo ausente por defecto (edad negativa = no hay rayo)
    params[2] = [0.0, 0.0, 0.0, -1.0];

    // mods[3]: estado del jugador [salud, daño, en_llamas, mojado]
    // Límite de la API de Horizon: el cliente no dispone de lecturas de salud ni efectos de estado
    // del jugador en render thread. Permanece en 0.0 con FLAG_PLAYER_VALID apagada.
    params[3] = [0.0, 0.0, 0.0, 0.0];

    // Codificar hasta 16 ajustes en vivo en mods[4..8]
    let empty_adjustments = [];
    let adjustments = live.unwrap_or(&empty_adjustments);
    let live_count = adjustments.len().min(MAX_LIVE_ADJUSTMENTS);

    for (i, adj) in adjustments.iter().take(live_count).enumerate() {
        if let Some(param) = find_param_by_id(adj.id) {
            let encoded = param.encode_live(adj.value);
            let slot_idx = 4 + (i / 4);
            let elem_idx = i % 4;
            params[slot_idx][elem_idx] = encoded;
        }
    }

    let ticks = match day_time {
        Some(t) => t,
        None => {
            // Sin reloj disponible (pantalla de título o mundo no cargado)
            // mods[0]: firma 1002.0, tabla 1.0, banderas 0.0, ajustes en vivo según live_count
            params[0] = [FORMAT_HEADER_SIG, PARAM_TABLE_VERSION, 0.0, live_count as f32];
            return;
        }
    };

    // 1. Número de día del mundo: día entero euclídeo (ticks / 24000)
    let day_number = ticks.div_euclid(24_000);
    let day_mod = (day_number.rem_euclid(DAY_MODULO)) as f32;

    // 2. Fase de la luna: día módulo 8 (0 = luna llena ... 7)
    let moon_phase = (day_number.rem_euclid(8)) as f32;

    // 3. Estación continua: 0.0 a 4.0
    // 0: primavera, 1: verano, 2: otoño, 3: invierno. La parte decimal es el avance intradía.
    let partial_clamped = if partial.is_finite() {
        (partial as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let total_ticks = ticks as f64 + partial_clamped;
    let ticks_per_season = DEFAULT_SEASON_DAYS * TICKS_PER_DAY; // 24 * 24000 = 576 000
    let ticks_per_year = SEASONS_PER_YEAR * ticks_per_season; // 4 * 576 000 = 2 304 000
    let ticks_in_year = total_ticks.rem_euclid(ticks_per_year);
    let continuous_season = (ticks_in_year / ticks_per_season) as f32;

    // 4. Último rayo relativo a la cámara
    let mut lightning_valid = false;
    if let (Some(cam), Ok(lock)) = (camera_pos, LAST_LIGHTNING.lock()) {
        if let Some(strike) = *lock {
            let age = strike.time.elapsed().as_secs_f32();
            if age >= 0.0 && age < LIGHTNING_ACTIVE_SECONDS {
                let rel_x = (strike.pos[0] - cam[0]) as f32;
                let rel_y = (strike.pos[1] - cam[1]) as f32;
                let rel_z = (strike.pos[2] - cam[2]) as f32;
                params[2] = [rel_x, rel_y, rel_z, age];
                lightning_valid = true;
            }
        }
    }

    // Banderas válidas: tiempo (bit 0), estación (bit 2), rayo (bit 1) y canal fijo (bit 4)
    let mut flags_u32 = FLAG_TIME_VALID | FLAG_SEASON_VALID;
    if lightning_valid {
        flags_u32 |= FLAG_LIGHTNING_VALID;
    }
    if is_fixed_channel_active() {
        flags_u32 |= FLAG_FIXED_CHANNEL_ACTIVE;
    }
    let biome = camera_biome_param();
    if biome.is_some() {
        flags_u32 |= FLAG_BIOME_VALID;
    }
    if clima::particulas_activas() {
        flags_u32 |= FLAG_PARTICLE_WEATHER;
    }
    let flags = flags_u32 as f32;

    // mods[0] cabecera: [firma, versión_tabla, banderas, cantidad_ajustes_en_vivo]
    params[0] = [FORMAT_HEADER_SIG, PARAM_TABLE_VERSION, flags, live_count as f32];

    // mods[1] tiempo y, en el formato 2, el bioma de la cámara con su peso
    params[1] = [day_mod, moon_phase, continuous_season, biome.unwrap_or(0.0)];
}

#[cfg(test)]
mod tests {
    use super::*;

    static TEST_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_biome_category_and_fade() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(biome_category("minecraft:swamp"), 1);
        assert_eq!(biome_category("minecraft:bamboo_jungle"), 2);
        assert_eq!(biome_category("minecraft:pale_garden"), 3);
        assert_eq!(biome_category("minecraft:plains"), 0);
        let t0 = Instant::now();
        observe_camera_biome_at(Some("minecraft:swamp"), t0);
        observe_camera_biome_at(Some("minecraft:swamp"), t0 + std::time::Duration::from_secs_f32(0.9));
        observe_camera_biome_at(Some("minecraft:swamp"), t0 + std::time::Duration::from_secs_f32(1.8));
        let v = camera_biome_param().unwrap();
        assert!((v - 1.45).abs() < 0.02, "pantano a 1,8 s: {v}");
        // Al salir a llanura, el peso baja antes de cambiar de categoría.
        observe_camera_biome_at(Some("minecraft:plains"), t0 + std::time::Duration::from_secs_f32(2.6));
        let v = camera_biome_param().unwrap();
        assert!(v > 1.0 && v < 1.45, "bajando: {v}");
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(Some(0), 0.0, &mut params);
        assert_eq!(params[0][2] as u32 & FLAG_BIOME_VALID, FLAG_BIOME_VALID);
        observe_camera_biome(None);
        populate_shader_params(Some(0), 0.0, &mut params);
        assert_eq!(params[0][2] as u32 & FLAG_BIOME_VALID, 0);
        assert_eq!(params[1][3], 0.0);
    }

    #[test]
    fn test_no_clock_gives_zero_flags() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_live_adjustments();
        set_fixed_channel_active(false);
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(None, 0.0, &mut params);

        assert_eq!(params[0], [1002.0, 1.0, 0.0, 0.0]);
        assert_eq!(params[1], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(params[2], [0.0, 0.0, 0.0, -1.0]);
        assert_eq!(params[3], [0.0, 0.0, 0.0, 0.0]);
        for i in 4..8 {
            assert_eq!(params[i], [0.0, 0.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn test_day_0_sunrise() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_live_adjustments();
        set_fixed_channel_active(false);
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(Some(0), 0.0, &mut params);

        // Cabecera: versión 1002, tabla 1, banderas 5 (tiempo + estación), 0 ajustes en vivo
        assert_eq!(params[0], [1002.0, 1.0, 5.0, 0.0]);
        // Tiempo: día 0, fase 0 (luna llena), estación 0.0 (inicio primavera), reservado 0.0
        assert_eq!(params[1], [0.0, 0.0, 0.0, 0.0]);
        // Rayo inactivo: edad negativa
        assert_eq!(params[2][3], -1.0);
    }

    #[test]
    fn test_day_1_midday() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_live_adjustments();
        set_fixed_channel_active(false);
        let mut params = [[0.0; 4]; 8];
        // Día 1 al mediodía (24000 + 6000 = 30000 ticks)
        populate_shader_params(Some(30_000), 0.0, &mut params);

        assert_eq!(params[0][0], 1002.0);
        assert_eq!(params[0][2], 5.0);
        assert_eq!(params[1][0], 1.0); // Día 1
        assert_eq!(params[1][1], 1.0); // Fase luna 1
        // Avance de estación: 1.25 días / 24 días por estación = 0.05208333...
        let expected_season = (30_000.0 / 576_000.0) as f32;
        assert!((params[1][2] - expected_season).abs() < 1e-6);
    }

    #[test]
    fn test_moon_phases_cycle() {
        let mut params = [[0.0; 4]; 8];
        for day in 0..16 {
            populate_shader_params(Some(day * 24_000), 0.0, &mut params);
            assert_eq!(params[1][0], day as f32);
            assert_eq!(params[1][1], (day % 8) as f32);
        }
    }

    #[test]
    fn test_season_progression_and_wrap() {
        let mut params = [[0.0; 4]; 8];

        // Inicio Primavera (día 0)
        populate_shader_params(Some(0), 0.0, &mut params);
        assert!((params[1][2] - 0.0).abs() < 1e-6);

        // Mitad Primavera (día 12)
        populate_shader_params(Some(12 * 24_000), 0.0, &mut params);
        assert!((params[1][2] - 0.5).abs() < 1e-6);

        // Inicio Verano (día 24)
        populate_shader_params(Some(24 * 24_000), 0.0, &mut params);
        assert!((params[1][2] - 1.0).abs() < 1e-6);

        // Inicio Otoño (día 48)
        populate_shader_params(Some(48 * 24_000), 0.0, &mut params);
        assert!((params[1][2] - 2.0).abs() < 1e-6);

        // Inicio Invierno (día 72)
        populate_shader_params(Some(72 * 24_000), 0.0, &mut params);
        assert!((params[1][2] - 3.0).abs() < 1e-6);

        // Final Invierno / Víspera Primavera (día 96 - 1 tick)
        populate_shader_params(Some(96 * 24_000 - 1), 0.0, &mut params);
        assert!(params[1][2] > 3.999);
        assert!(params[1][2] < 4.0);

        // Vuelta a Primavera (día 96)
        populate_shader_params(Some(96 * 24_000), 0.0, &mut params);
        assert!((params[1][2] - 0.0).abs() < 1e-6);
    }

    #[test]
    fn test_partial_tick_interpolation() {
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(Some(0), 0.5, &mut params);
        let expected_season = (0.5 / 576_000.0) as f32;
        assert!((params[1][2] - expected_season).abs() < 1e-7);
    }

    #[test]
    fn test_negative_time_robustness() {
        let mut params = [[0.0; 4]; 8];
        // /time set puede provocar ticks negativos
        populate_shader_params(Some(-24_000), 0.0, &mut params);
        assert_eq!(params[0][0], 1002.0);
        assert!(params[1][0] >= 0.0);
        assert!(params[1][1] >= 0.0 && params[1][1] <= 7.0);
        assert!(params[1][2] >= 0.0 && params[1][2] < 4.0);
    }

    #[test]
    fn test_day_modulo_2_pow_24() {
        let mut params = [[0.0; 4]; 8];
        let huge_day = 16_777_216i64 + 42;
        populate_shader_params(Some(huge_day * 24_000), 0.0, &mut params);
        assert_eq!(params[1][0], 42.0);
    }

    #[test]
    fn test_lightning_strike_relative_to_camera() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_lightning_strike();
        set_fixed_channel_active(false);
        let mut params = [[0.0; 4]; 8];

        // Impacto de rayo en (100.0, 64.0, 200.0)
        record_lightning_strike_pos([100.0, 64.0, 200.0]);

        // Cámara situada en (90.0, 60.0, 180.0)
        let camera_pos = Some([90.0, 60.0, 180.0]);
        populate_shader_params_full(Some(0), 0.0, camera_pos, &mut params);

        // Cabecera: flags = 1 (tiempo) | 2 (rayo) | 4 (estación) = 7.0
        assert_eq!(params[0][2], 7.0);

        // Rayo en mods[2]: (10.0, 4.0, 20.0, edad >= 0.0)
        assert!((params[2][0] - 10.0).abs() < 1e-5);
        assert!((params[2][1] - 4.0).abs() < 1e-5);
        assert!((params[2][2] - 20.0).abs() < 1e-5);
        assert!(params[2][3] >= 0.0 && params[2][3] < LIGHTNING_ACTIVE_SECONDS);

        clear_lightning_strike();
    }

    #[test]
    fn test_expired_lightning_strike() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_lightning_strike();
        set_fixed_channel_active(false);
        let mut params = [[0.0; 4]; 8];

        // Rayo registrado con fecha pasada (hace 15 segundos)
        let old_time = Instant::now() - std::time::Duration::from_secs(15);
        record_lightning_strike_with_time([100.0, 64.0, 200.0], old_time);

        let camera_pos = Some([90.0, 60.0, 180.0]);
        populate_shader_params_full(Some(0), 0.0, camera_pos, &mut params);

        // Flag de rayo apagada (flags = 5.0) y edad negativa
        assert_eq!(params[0][2], 5.0);
        assert_eq!(params[2], [0.0, 0.0, 0.0, -1.0]);

        clear_lightning_strike();
    }

    #[test]
    fn test_player_state_defaults() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(Some(0), 0.0, &mut params);
        // mods[3] debe ser [0,0,0,0] y bit 3 (8) apagado
        assert_eq!(params[3], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!((params[0][2] as u32) & FLAG_PLAYER_VALID, 0);
    }

    #[test]
    fn test_live_adjustments_encoding_and_decoding() {
        let param = find_param_by_id(0).expect("Param 0 debe existir");
        // Param 0: min 0.0, max 5.0, def 1.0
        let encoded = param.encode_live(2.5); // t = 0.5, q = 2048, id = 0 -> 2048.0
        let (id, q) = crate::table::decode_live_raw(encoded);
        assert_eq!(id, 0);
        assert_eq!(q, 2048);
        let decoded_val = param.dequantize(q);
        assert!((decoded_val - 2.5).abs() < 0.005);
    }

    #[test]
    fn test_populate_with_live_adjustments() {
        let mut params = [[0.0; 4]; 8];
        let live = vec![
            LiveAdjustment { id: 0, value: 2.5 },
            LiveAdjustment { id: 15, value: 2.0 },
            LiveAdjustment { id: 25, value: 500.0 },
        ];

        populate_shader_params_live(Some(0), 0.0, None, Some(&live), &mut params);

        // Cabecera w: 3 ajustes en vivo
        assert_eq!(params[0][3], 3.0);

        // mods[4] tiene los 3 ajustes en sus tres primeras componentes
        let (id0, _q0) = crate::table::decode_live_raw(params[4][0]);
        assert_eq!(id0, 0);
        let (id1, _q1) = crate::table::decode_live_raw(params[4][1]);
        assert_eq!(id1, 15);
        let (id2, _q2) = crate::table::decode_live_raw(params[4][2]);
        assert_eq!(id2, 25);

        assert_eq!(params[4][3], 0.0);
        for i in 5..8 {
            assert_eq!(params[i], [0.0, 0.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn test_live_adjustments_max_cap_16() {
        let mut params = [[0.0; 4]; 8];
        let live: Vec<LiveAdjustment> = (0..20)
            .map(|i| LiveAdjustment {
                id: i,
                value: 1.0,
            })
            .collect();

        populate_shader_params_live(Some(0), 0.0, None, Some(&live), &mut params);

        // Tope de 16 ajustes
        assert_eq!(params[0][3], 16.0);
        // Cada uno de los 16 floats en mods[4..8] debe estar ocupado (> 0.0)
        for vec_idx in 4..8 {
            for elem_idx in 0..4 {
                assert!(params[vec_idx][elem_idx] >= 0.0);
            }
        }
    }

    #[test]
    fn test_exact_integer_f32_precision() {
        // 4095 * 4096 + 4095 = 16 777 215 (2^24 - 1), exacto en float f32
        let max_code: u32 = 4095 * 4096 + 4095;
        let as_f32 = max_code as f32;
        let back_to_u32 = as_f32 as u32;
        assert_eq!(max_code, back_to_u32);
        assert_eq!(back_to_u32, 16_777_215);
    }

    #[test]
    fn test_fixed_channel_flag_in_shader_params() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_lightning_strike();
        set_fixed_channel_active(false);
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(Some(0), 0.0, &mut params);
        // Sin canal fijo: banderas = 5.0 (tiempo 1 + estación 4)
        assert_eq!(params[0][2], 5.0);

        set_fixed_channel_active(true);
        populate_shader_params(Some(0), 0.0, &mut params);
        // Con canal fijo: banderas = 21.0 (tiempo 1 + estación 4 + canal fijo 16)
        assert_eq!(params[0][2], 21.0);
        assert_eq!((params[0][2] as u32) & FLAG_FIXED_CHANNEL_ACTIVE, 16);

        set_fixed_channel_active(false);
    }

    #[test]
    fn test_observe_client_bolt_deduplicates_by_id() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_lightning_strike();
        set_fixed_channel_active(false);

        observe_client_bolt(42, [180.0, 90.0, 186.0]);
        let first = get_last_lightning_strike().expect("Debe registrar rayo");
        assert_eq!(first.pos, [180.0, 90.0, 186.0]);

        // Mismo ID de entidad en fotogramas sucesivos no reinicia el reloj
        observe_client_bolt(42, [180.0, 90.0, 186.0]);
        let second = get_last_lightning_strike().unwrap();
        assert_eq!(first.time, second.time);

        // Nuevo ID de entidad actualiza el rayo
        observe_client_bolt(43, [200.0, 70.0, 190.0]);
        let third = get_last_lightning_strike().unwrap();
        assert_eq!(third.pos, [200.0, 70.0, 190.0]);

        clear_lightning_strike();
    }

    #[test]
    fn test_sync_runtime_state_and_commands() {
        let _guard = TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        clear_lightning_strike();
        clear_live_adjustments();
        set_fixed_channel_active(false);

        let temp_dir = std::env::temp_dir().join(format!("vulpe_sync_test_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&temp_dir);
        let cfg_dir = temp_dir.join("config");
        let pack_dir = temp_dir.join("pack");
        std::fs::create_dir_all(&cfg_dir).unwrap();
        std::fs::create_dir_all(&pack_dir).unwrap();

        // 1. En reposo: crea config/vulpe-horizon.toml con defaults, banderas = 5, en_vivo = 0
        sync_runtime_state(Some(&cfg_dir), Some(&pack_dir), true);
        assert!(cfg_dir.join(config::CONFIG_FILE_NAME).is_file());
        assert!(!is_fixed_channel_active());
        assert!(get_live_adjustments().is_empty());

        // 2. Comando /vulpe poner activa tanto canal fijo (constantes_mod.wgsl en vulpe-mod/ + bit 4 = 16) como ajuste en vivo
        let reply = execute_client_command(
            "vulpe poner cielo_brillo_manual 2.5",
            Some(&cfg_dir),
            Some(&pack_dir),
        );
        assert!(reply.is_some());
        assert!(is_fixed_channel_active());
        assert_eq!(get_live_adjustments().len(), 1);
        assert!(
            temp_dir
                .join(UNPACKED_PACK_DIR_NAME)
                .join(CONSTANTS_WGSL_FILENAME)
                .is_file()
        );

        let mut params = [[0.0; 4]; 8];
        populate_shader_params_full(Some(0), 0.0, Some([0.0, 0.0, 0.0]), &mut params);
        assert_eq!(params[0][2], 21.0); // 1 (tiempo) + 4 (estación) + 16 (canal fijo)
        assert_eq!(params[0][3], 1.0); // 1 ajuste en vivo
        assert_eq!(params[4][0], 2048.0); // id 0 * 4096 + 2048

        // 3. Comando /vulpe restablecer vuelve a reposo (banderas = 5, en_vivo = 0)
        let _ = execute_client_command("vulpe restablecer", Some(&cfg_dir), Some(&pack_dir));
        assert!(!is_fixed_channel_active());
        assert!(get_live_adjustments().is_empty());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }
}
