//! Estado de fuego del jugador deducido en el cliente para `frame.mods[3].z` (N-07).
//!
//! Horizon `dd67b76` no expone en `mod_api` si el jugador local está ardiendo ni los ticks de fuego restantes.
//! Se deduce a partir de los bloques en los pies y en la cabeza del jugador, la entrada en agua y la lluvia
//! directa sobre él (ver `vulpe-horizon-spec/observaciones-del-dueno-2026-11-13.md` §5.2).

use std::sync::Mutex;
use std::time::Instant;

/// Segundos que el jugador sigue ardiendo tras salir de fuego o fuego de almas (vanilla: ~8 s).
pub const FUEGO_DURACION_S: f32 = 8.0;

/// Segundos que el jugador sigue ardiendo tras salir de lava (vanilla: ~15 s).
pub const LAVA_DURACION_S: f32 = 15.0;

/// Paso máximo de tiempo entre observaciones para no saltar todo el temporizador tras una pausa.
const MAX_DT_S: f32 = 1.0;

#[derive(Clone, Copy, Debug)]
struct EstadoFuego {
    restante_s: f32,
    ultimo: Instant,
}

static ESTADO_FUEGO: Mutex<Option<EstadoFuego>> = Mutex::new(None);

fn nombre_corto(id: &str) -> &str {
    id.rsplit(':').next().unwrap_or(id)
}

/// Cierto si el identificador del bloque es fuego o fuego de almas.
pub fn es_bloque_fuego(id: &str) -> bool {
    matches!(nombre_corto(id), "fire" | "soul_fire")
}

/// Cierto si el identificador del bloque es lava.
pub fn es_bloque_lava(id: &str) -> bool {
    nombre_corto(id) == "lava"
}

/// Cierto si el identificador del bloque es agua o columna de burbujas.
pub fn es_bloque_agua(id: &str) -> bool {
    matches!(nombre_corto(id), "water" | "bubble_column")
}

/// Actualiza el estado de fuego del jugador con los bloques de pies y cabeza, si está en agua
/// (por ejemplo un bloque anegado) y si le cae lluvia encima.
pub fn observar_fuego(
    bloque_pies: &str,
    bloque_cabeza: &str,
    agua_extra: bool,
    bajo_lluvia: bool,
) {
    observar_fuego_en(
        bloque_pies,
        bloque_cabeza,
        agua_extra,
        bajo_lluvia,
        Instant::now(),
    );
}

/// Variante determinista con marca de tiempo explícita para pruebas unitarias.
pub fn observar_fuego_en(
    bloque_pies: &str,
    bloque_cabeza: &str,
    agua_extra: bool,
    bajo_lluvia: bool,
    ahora: Instant,
) {
    let Ok(mut lock) = ESTADO_FUEGO.lock() else {
        return;
    };
    let mut st = lock.unwrap_or(EstadoFuego {
        restante_s: 0.0,
        ultimo: ahora,
    });
    let dt = ahora
        .saturating_duration_since(st.ultimo)
        .as_secs_f32()
        .min(MAX_DT_S);
    st.ultimo = ahora;
    st.restante_s = (st.restante_s - dt).max(0.0);

    let en_lava = es_bloque_lava(bloque_pies) || es_bloque_lava(bloque_cabeza);
    let en_fuego = es_bloque_fuego(bloque_pies) || es_bloque_fuego(bloque_cabeza);
    let en_agua = agua_extra || es_bloque_agua(bloque_pies) || es_bloque_agua(bloque_cabeza);

    if en_lava {
        st.restante_s = st.restante_s.max(LAVA_DURACION_S);
    } else if en_fuego {
        st.restante_s = st.restante_s.max(FUEGO_DURACION_S);
    } else if en_agua || bajo_lluvia {
        st.restante_s = 0.0;
    }

    *lock = Some(st);
}

/// Borra el estado observado (sin mundo cargado o pantalla de título).
pub fn limpiar_fuego() {
    if let Ok(mut lock) = ESTADO_FUEGO.lock() {
        *lock = None;
    }
}

/// Devuelve `Some(true)` si el jugador está ardiendo, `Some(false)` si no arde (estado válido),
/// o `None` si aún no hay estado del jugador observado.
pub fn en_llamas() -> Option<bool> {
    let st = (*ESTADO_FUEGO.lock().ok()?)?;
    Some(st.restante_s > 0.0)
}

/// Segundos restantes de fuego, o `None` si no hay estado observado.
pub fn segundos_restantes() -> Option<f32> {
    let st = (*ESTADO_FUEGO.lock().ok()?)?;
    Some(st.restante_s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::GLOBAL_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn avanzar(t0: Instant, segundos: f32) -> Instant {
        t0 + Duration::from_secs_f32(segundos)
    }

    #[test]
    fn test_fuego_dura_8s_y_se_apaga() {
        let _g = test_lock();
        limpiar_fuego();
        assert_eq!(en_llamas(), None);

        let t0 = Instant::now();
        observar_fuego_en("minecraft:fire", "minecraft:air", false, false, t0);
        assert_eq!(en_llamas(), Some(true));
        assert!((segundos_restantes().unwrap() - 8.0).abs() < 1e-4);

        // Sale del fuego: sigue ardiendo a los 7 s, se apaga al pasar 8 s.
        let mut t = t0;
        for _ in 0..7 {
            t = avanzar(t, 1.0);
            observar_fuego_en("minecraft:air", "minecraft:air", false, false, t);
            assert_eq!(en_llamas(), Some(true));
        }
        t = avanzar(t, 1.0);
        observar_fuego_en("minecraft:air", "minecraft:air", false, false, t);
        assert_eq!(en_llamas(), Some(false));
        limpiar_fuego();
    }

    #[test]
    fn test_fuego_almas_en_cabeza() {
        let _g = test_lock();
        limpiar_fuego();
        let t0 = Instant::now();
        observar_fuego_en("minecraft:soul_sand", "minecraft:soul_fire", false, false, t0);
        assert_eq!(en_llamas(), Some(true));
        assert!((segundos_restantes().unwrap() - FUEGO_DURACION_S).abs() < 1e-4);
        limpiar_fuego();
    }

    #[test]
    fn test_lava_dura_15s_y_fuego_no_acorta() {
        let _g = test_lock();
        limpiar_fuego();
        let t0 = Instant::now();
        observar_fuego_en("minecraft:lava", "minecraft:air", false, false, t0);
        assert_eq!(en_llamas(), Some(true));
        assert!((segundos_restantes().unwrap() - LAVA_DURACION_S).abs() < 1e-4);

        // Al segundo siguiente toca fuego: no debe bajar de 14 s a 8 s.
        let t1 = avanzar(t0, 1.0);
        observar_fuego_en("minecraft:fire", "minecraft:air", false, false, t1);
        assert!(segundos_restantes().unwrap() >= 13.9);

        // 14 s después de t1 (15 s desde lava) se apaga.
        let mut t = t1;
        for _ in 0..13 {
            t = avanzar(t, 1.0);
            observar_fuego_en("minecraft:air", "minecraft:air", false, false, t);
            assert_eq!(en_llamas(), Some(true));
        }
        t = avanzar(t, 1.0);
        observar_fuego_en("minecraft:air", "minecraft:air", false, false, t);
        assert_eq!(en_llamas(), Some(false));
        limpiar_fuego();
    }

    #[test]
    fn test_agua_y_lluvia_apagan_al_instante() {
        let _g = test_lock();
        limpiar_fuego();
        let t0 = Instant::now();
        observar_fuego_en("minecraft:fire", "minecraft:air", false, false, t0);
        assert_eq!(en_llamas(), Some(true));

        // Entrar en agua apaga al instante.
        let t1 = avanzar(t0, 0.5);
        observar_fuego_en("minecraft:water", "minecraft:air", false, false, t1);
        assert_eq!(en_llamas(), Some(false));

        // Volver a arder en lava bajo la lluvia: mientras está en lava sigue ardiendo.
        let t2 = avanzar(t1, 0.5);
        observar_fuego_en("minecraft:lava", "minecraft:air", false, true, t2);
        assert_eq!(en_llamas(), Some(true));

        // Al salir de la lava bajo la lluvia se apaga al instante.
        let t3 = avanzar(t2, 0.5);
        observar_fuego_en("minecraft:air", "minecraft:air", false, true, t3);
        assert_eq!(en_llamas(), Some(false));

        // Bloque anegado (agua_extra = true) también apaga.
        let t4 = avanzar(t3, 0.5);
        observar_fuego_en("minecraft:fire", "minecraft:air", false, false, t4);
        assert_eq!(en_llamas(), Some(true));
        let t5 = avanzar(t4, 0.5);
        observar_fuego_en("minecraft:oak_stairs", "minecraft:air", true, false, t5);
        assert_eq!(en_llamas(), Some(false));

        limpiar_fuego();
    }
}
