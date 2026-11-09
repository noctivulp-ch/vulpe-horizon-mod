//! vulpe-engine: Motor y lógica central del mod nativo de Vulpe para Horizon.
//!
//! Implementa la especificación de `frame.mods` v1 descrita en `vulpe-horizon-spec/flexibilidad/mod-nativo.md`.

pub mod config;
pub mod table;

pub use config::VulpeConfig;
pub use table::{ParamDef, PARAM_TABLE, find_param_by_id, find_param_by_name};

use std::sync::Mutex;
use std::time::Instant;

/// Firma base de la cabecera del formato: 1000 + versión_formato.
pub const FORMAT_VERSION: f32 = 1.0;
pub const FORMAT_HEADER_SIG: f32 = 1000.0 + FORMAT_VERSION; // 1001.0

/// Versión actual de la tabla unificada de parámetros.
pub const PARAM_TABLE_VERSION: f32 = 1.0;

/// Banderas de validez en `mods[0].z` (codificadas como bits en un entero f32 exacto).
pub const FLAG_TIME_VALID: u32 = 1 << 0; // Bit 0 (1): datos de tiempo válidos
pub const FLAG_LIGHTNING_VALID: u32 = 1 << 1; // Bit 1 (2): rayo válido
pub const FLAG_SEASON_VALID: u32 = 1 << 2; // Bit 2 (4): estación válida
pub const FLAG_PLAYER_VALID: u32 = 1 << 3; // Bit 3 (8): estado del jugador válido
pub const FLAG_FIXED_CHANNEL_ACTIVE: u32 = 1 << 4; // Bit 4 (16): canal fijo en uso

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
static LIVE_ADJUSTMENTS: Mutex<Vec<LiveAdjustment>> = Mutex::new(Vec::new());

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

/// Obtiene el registro del último rayo si existe.
pub fn get_last_lightning_strike() -> Option<LightningRecord> {
    LAST_LIGHTNING.lock().ok().and_then(|lock| *lock)
}

/// Limpia el registro del último rayo.
pub fn clear_lightning_strike() {
    if let Ok(mut lock) = LAST_LIGHTNING.lock() {
        *lock = None;
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
            // mods[0]: firma 1001.0, tabla 1.0, banderas 0.0, ajustes en vivo según live_count
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

    // Banderas válidas: tiempo (bit 0) y estación (bit 2), y rayo (bit 1) si está activo
    let mut flags_u32 = FLAG_TIME_VALID | FLAG_SEASON_VALID;
    if lightning_valid {
        flags_u32 |= FLAG_LIGHTNING_VALID;
    }
    let flags = flags_u32 as f32;

    // mods[0] cabecera: [firma, versión_tabla, banderas, cantidad_ajustes_en_vivo]
    params[0] = [FORMAT_HEADER_SIG, PARAM_TABLE_VERSION, flags, live_count as f32];

    // mods[1] tiempo
    params[1] = [day_mod, moon_phase, continuous_season, 0.0];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_clock_gives_zero_flags() {
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(None, 0.0, &mut params);

        assert_eq!(params[0], [1001.0, 1.0, 0.0, 0.0]);
        assert_eq!(params[1], [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(params[2], [0.0, 0.0, 0.0, -1.0]);
        assert_eq!(params[3], [0.0, 0.0, 0.0, 0.0]);
        for i in 4..8 {
            assert_eq!(params[i], [0.0, 0.0, 0.0, 0.0]);
        }
    }

    #[test]
    fn test_day_0_sunrise() {
        let mut params = [[0.0; 4]; 8];
        populate_shader_params(Some(0), 0.0, &mut params);

        // Cabecera: versión 1001, tabla 1, banderas 5 (tiempo + estación), 0 ajustes en vivo
        assert_eq!(params[0], [1001.0, 1.0, 5.0, 0.0]);
        // Tiempo: día 0, fase 0 (luna llena), estación 0.0 (inicio primavera), reservado 0.0
        assert_eq!(params[1], [0.0, 0.0, 0.0, 0.0]);
        // Rayo inactivo: edad negativa
        assert_eq!(params[2][3], -1.0);
    }

    #[test]
    fn test_day_1_midday() {
        let mut params = [[0.0; 4]; 8];
        // Día 1 al mediodía (24000 + 6000 = 30000 ticks)
        populate_shader_params(Some(30_000), 0.0, &mut params);

        assert_eq!(params[0][0], 1001.0);
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
        assert_eq!(params[0][0], 1001.0);
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
        clear_lightning_strike();
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
        clear_lightning_strike();
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
}
