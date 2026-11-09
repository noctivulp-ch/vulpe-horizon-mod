//! vulpe-engine: Motor y lógica central del mod nativo de Vulpe para Horizon.
//!
//! Implementa la especificación de `frame.mods` v1 descrita en `vulpe-horizon-spec/flexibilidad/mod-nativo.md`.

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

/// Rellena el arreglo `params` (8 vec4s, 32 floats) según el formato v1 de `frame.mods`.
///
/// Primer corte (M-01):
/// - `mods[0]` cabecera: [1001.0, 1.0, banderas, 0.0]
/// - `mods[1]` tiempo: [día mod 2^24, fase luna (0..7), estación continua (0..4), 0.0]
/// - `mods[2]` rayo: [0.0, 0.0, 0.0, -1.0] (edad negativa = sin rayo)
/// - `mods[3]` jugador: [0.0, 0.0, 0.0, 0.0] (no medido en cliente)
/// - `mods[4..8]`: 16 ajustes en vivo (0.0 en este corte)
pub fn populate_shader_params(
    day_time: Option<i64>,
    partial: f32,
    params: &mut [[f32; 4]; 8],
) {
    // Limpiar todo a ceros primero
    for slot in params.iter_mut() {
        *slot = [0.0; 4];
    }

    // mods[2]: rayo ausente por defecto (edad negativa = no hay rayo)
    params[2] = [0.0, 0.0, 0.0, -1.0];

    let ticks = match day_time {
        Some(t) => t,
        None => {
            // Sin reloj disponible (pantalla de título o mundo no cargado)
            // mods[0]: firma 1001.0, tabla 1.0, banderas 0.0, ajustes en vivo 0.0
            params[0] = [FORMAT_HEADER_SIG, PARAM_TABLE_VERSION, 0.0, 0.0];
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

    // Banderas válidas: tiempo (bit 0) y estación (bit 2)
    let flags = (FLAG_TIME_VALID | FLAG_SEASON_VALID) as f32; // 1 | 4 = 5.0

    // mods[0] cabecera
    params[0] = [FORMAT_HEADER_SIG, PARAM_TABLE_VERSION, flags, 0.0];

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
}
