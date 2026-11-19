//! Estado con memoria y configuración del efecto «el tiempo en los cristales» (subsistema 27, N-08).
//!
//! Guarda entre fotogramas lo que el pack WGSL no puede recordar solo:
//! - tiempo de secado tras la lluvia (~30 s) y fuerza pico de la tormenta;
//! - escarcha, copos y montón de nieve en biomas fríos;
//! - vaho del aliento (posición del último cristal cercano y desvanecido de ~5 s, excluido espectador);
//! - arena acumulada tras tormenta en bioma seco (amarilla o roja en mesa, desvanecida en ~3 min).
//!
//! Todo viaja en `frame.mods[6]` y `frame.mods[7]` (formato v3, bandera bit 8 `FLAG_GLASS_VALID`).

use std::sync::Mutex;
use std::time::Instant;

/// Duración del secado completo de las gotas tras parar la lluvia (segundos, spec 27 §1).
pub const LLUVIA_SECADO_S: f32 = 30.0;

/// Umbral de temperatura de nieve en Minecraft (biomas fríos o altura con nieve, spec 27 §2).
pub const TEMP_UMBRAL_NIEVE: f32 = 0.15;

/// Tiempos de crecimiento y retirada gradual de la escarcha (segundos, spec 27 §2).
pub const ESCARCHA_CRECE_S: f32 = 16.0;
pub const ESCARCHA_RETIRA_S: f32 = 24.0;

/// Tiempos de copos pegados y montón de nieve inferior (segundos, spec 27 §3).
pub const COPOS_CRECEN_S: f32 = 3.0;
pub const COPOS_CAEN_S: f32 = 8.0;
pub const MONTON_CRECE_S: f32 = 18.0;
pub const MONTON_FUNDE_S: f32 = 45.0;

/// Tiempos y alcance del vaho del aliento (spec 27 §4).
pub const VAHO_DISTANCIA_MAX: f64 = 1.5;
pub const VAHO_CRECE_S: f32 = 6.0;
pub const VAHO_DESVANECE_S: f32 = 5.0;

/// Tiempos de acumulación y limpieza por el viento de la arena en bioma seco (segundos, spec 27 §5).
pub const ARENA_CRECE_S: f32 = 12.0;
pub const ARENA_LIMPIA_S: f32 = 180.0;

/// Cierto si un bioma del Overworld es seco (desierto, sabana, mesa y secos de mods).
pub fn es_bioma_seco(bioma: &str, has_precipitation: bool, temperature: f32) -> bool {
    crate::clima::es_overworld(bioma) && !has_precipitation && temperature >= 1.0
}

/// Cierto si la arena del bioma es roja (mesas / badlands y variantes de mods).
pub fn es_bioma_mesa(bioma: &str) -> bool {
    let n = bioma.rsplit(':').next().unwrap_or(bioma);
    n.contains("badlands") || n.contains("mesa") || n.contains("red_desert")
}

/// Cierto si el identificador de bloque corresponde a un cristal o panel de cristal (27).
pub fn es_bloque_cristal(id: &str) -> bool {
    let nombre = id.rsplit(':').next().unwrap_or(id);
    nombre == "glass"
        || nombre == "glass_pane"
        || nombre == "tinted_glass"
        || nombre.ends_with("_stained_glass")
        || nombre.ends_with("_stained_glass_pane")
}

/// Punto más cercano de la caja unitaria de un bloque `[bx, by, bz]` a una posición de cámara.
pub fn punto_mas_cercano_bloque(cam: [f64; 3], bloque: [i32; 3]) -> ([f64; 3], f64) {
    let bx = f64::from(bloque[0]);
    let by = f64::from(bloque[1]);
    let bz = f64::from(bloque[2]);
    let px = cam[0].clamp(bx, bx + 1.0);
    let py = cam[1].clamp(by, by + 1.0);
    let pz = cam[2].clamp(bz, bz + 1.0);
    let dx = px - cam[0];
    let dy = py - cam[1];
    let dz = pz - cam[2];
    ([px, py, pz], (dx * dx + dy * dy + dz * dz).sqrt())
}

/// Paso máximo de tiempo entre observaciones para no saltar temporizadores tras una pausa.
const MAX_DT_S: f32 = 1.0;

/// Intensidad configurable por parte en `[cristales]` de `config/vulpe-horizon.toml`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NivelCristales {
    Apagada = 0,
    Ligera = 1,
    Normal = 2,
    Intensa = 3,
}

impl NivelCristales {
    pub fn codigo(self) -> u32 {
        self as u32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apagada => "apagada",
            Self::Ligera => "ligera",
            Self::Normal => "normal",
            Self::Intensa => "intensa",
        }
    }

    pub fn from_toml_value(raw: &str) -> Option<Self> {
        let limpio = raw.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase();
        match limpio.as_str() {
            "apagada" | "off" | "0" | "false" => Some(Self::Apagada),
            "ligera" | "baja" | "1" => Some(Self::Ligera),
            "normal" | "media" | "2" | "true" => Some(Self::Normal),
            "intensa" | "alta" | "3" => Some(Self::Intensa),
            _ => None,
        }
    }
}

/// Ajustes de la sección `[cristales]` en `config/vulpe-horizon.toml` (spec 27 §Opciones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CristalesConfig {
    pub lluvia: NivelCristales,
    pub nieve: NivelCristales,
    pub arena: NivelCristales,
    pub aliento: bool,
    pub escarcha_interior: bool,
}

impl Default for CristalesConfig {
    fn default() -> Self {
        Self {
            lluvia: NivelCristales::Normal,
            nieve: NivelCristales::Normal,
            arena: NivelCristales::Normal,
            aliento: true,
            escarcha_interior: false,
        }
    }
}

impl CristalesConfig {
    /// Lee la sección `[cristales]` de un archivo TOML simple; lo que falte queda por defecto.
    pub fn from_toml_str(content: &str) -> Self {
        let mut c = Self::default();
        let mut en_cristales = false;
        for linea in content.lines() {
            let l = linea.split('#').next().unwrap_or("").trim();
            if l.starts_with('[') && l.ends_with(']') {
                en_cristales = l[1..l.len() - 1].trim() == "cristales";
                continue;
            }
            if !en_cristales {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = matches!(v.trim_matches('"'), "true" | "1" | "1.0" | "si" | "sí");
            match k {
                "lluvia" => {
                    if let Some(n) = NivelCristales::from_toml_value(v) {
                        c.lluvia = n;
                    }
                }
                "nieve" => {
                    if let Some(n) = NivelCristales::from_toml_value(v) {
                        c.nieve = n;
                    }
                }
                "arena" => {
                    if let Some(n) = NivelCristales::from_toml_value(v) {
                        c.arena = n;
                    }
                }
                "aliento" => c.aliento = b,
                "escarcha_interior" => c.escarcha_interior = b,
                _ => {}
            }
        }
        c
    }

    /// Sección `[cristales]` para añadir a `config/vulpe-horizon.toml`.
    pub fn to_toml_section(&self) -> String {
        format!(
            "\n[cristales]\n\
             # Intensidad por parte: apagada | ligera | normal | intensa\n\
             lluvia = \"{}\"\n\
             nieve = \"{}\"\n\
             arena = \"{}\"\n\
             # Vaho del aliento al acercarse a un cristal con escarcha\n\
             aliento = {}\n\
             # Escarcha también en la cara interior de ventanas hacia el exterior\n\
             escarcha_interior = {}\n",
            self.lluvia.as_str(),
            self.nieve.as_str(),
            self.arena.as_str(),
            self.aliento,
            self.escarcha_interior,
        )
    }
}

/// Estado dinámico observado cada fotograma para los cristales.
#[derive(Clone, Copy, Debug)]
struct EstadoCristales {
    lluvia_pico: f32,
    lluvia_secado_s: f32,
    viento_xz: [f32; 2],
    escarcha: f32,
    copos: f32,
    monton: f32,
    arena: f32,
    arena_roja: bool,
    espectador: bool,
    bioma_seco: bool,
    bioma_frio: bool,
    vaho_peso: f32,
    vaho_pos_mundo: [f64; 3],
    tiene_vaho_pos: bool,
    ultimo: Instant,
    ultimo_aliento: Instant,
    ultimo_arena: Instant,
}

static CONFIG: Mutex<CristalesConfig> = Mutex::new(CristalesConfig {
    lluvia: NivelCristales::Normal,
    nieve: NivelCristales::Normal,
    arena: NivelCristales::Normal,
    aliento: true,
    escarcha_interior: false,
});

static ESTADO: Mutex<Option<EstadoCristales>> = Mutex::new(None);

pub fn config() -> CristalesConfig {
    CONFIG.lock().map(|g| *g).unwrap_or_default()
}

pub fn set_config(cfg: CristalesConfig) {
    if let Ok(mut g) = CONFIG.lock() {
        *g = cfg;
    }
}

/// Limpia el estado dinámico (sin mundo cargado o pantalla de título).
pub fn limpiar() {
    if let Ok(mut lock) = ESTADO.lock() {
        *lock = None;
    }
}

/// Calcula el objetivo de escarcha `[0, 1]` a partir de la temperatura efectiva (con altura),
/// si es de noche (`noche` en `0..1`) y si está nevando (`nieve` en `0..1`).
pub fn objetivo_escarcha(temp_efectiva: f32, noche: f32, nieve: f32) -> f32 {
    if temp_efectiva >= TEMP_UMBRAL_NIEVE {
        return 0.0;
    }
    let frio = ((TEMP_UMBRAL_NIEVE - temp_efectiva) / 0.45).clamp(0.0, 1.0);
    let base = 0.48 + 0.32 * frio;
    (base + 0.22 * noche.clamp(0.0, 1.0) + 0.25 * nieve.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// Observa el clima de lluvia, escarcha en el frío y viento (partes 1 y 2).
pub fn observar_lluvia_y_frio(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    temp_efectiva: f32,
    noche: f32,
    viento_xz: [f32; 2],
) {
    observar_lluvia_y_frio_en(
        fuerza_lluvia,
        fuerza_trueno,
        llueve_en_bioma,
        temp_efectiva,
        noche,
        viento_xz,
        Instant::now(),
    );
}

/// Conserva compatibilidad directa con la firma de parte 1.
pub fn observar_lluvia(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    viento_xz: [f32; 2],
) {
    observar_lluvia_y_frio(fuerza_lluvia, fuerza_trueno, llueve_en_bioma, 0.8, 0.0, viento_xz);
}

/// Variante determinista de parte 1 para pruebas unitarias.
pub fn observar_lluvia_en(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    viento_xz: [f32; 2],
    ahora: Instant,
) {
    observar_lluvia_y_frio_en(fuerza_lluvia, fuerza_trueno, llueve_en_bioma, 0.8, 0.0, viento_xz, ahora);
}

/// Variante determinista con marca de tiempo explícita para pruebas unitarias (partes 1 y 2).
pub fn observar_lluvia_y_frio_en(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    temp_efectiva: f32,
    noche: f32,
    viento_xz: [f32; 2],
    ahora: Instant,
) {
    let Ok(mut lock) = ESTADO.lock() else { return };
    let es_nuevo = lock.is_none();
    let lluvia = fuerza_lluvia.clamp(0.0, 1.0);
    let trueno = fuerza_trueno.clamp(0.0, 1.0);
    let frio = temp_efectiva < TEMP_UMBRAL_NIEVE;
    let nevando = if frio { lluvia } else { 0.0 };
    let obj_escarcha = objetivo_escarcha(temp_efectiva, noche, nevando);

    let mut st = lock.unwrap_or(EstadoCristales {
        lluvia_pico: 0.0,
        lluvia_secado_s: LLUVIA_SECADO_S,
        viento_xz: [0.0, 0.0],
        escarcha: if es_nuevo { obj_escarcha * 0.7 } else { 0.0 },
        copos: if es_nuevo { nevando * 0.7 } else { 0.0 },
        monton: if es_nuevo { nevando * 0.55 } else { 0.0 },
        arena: 0.0,
        arena_roja: false,
        espectador: false,
        bioma_seco: false,
        bioma_frio: frio,
        vaho_peso: 0.0,
        vaho_pos_mundo: [0.0; 3],
        tiene_vaho_pos: false,
        ultimo: ahora,
        ultimo_aliento: ahora,
        ultimo_arena: ahora,
    });

    let dt = ahora
        .saturating_duration_since(st.ultimo)
        .as_secs_f32()
        .min(MAX_DT_S);
    st.ultimo = ahora;

    let activa = llueve_en_bioma && lluvia > 0.01;
    if activa {
        // Conserva el pico de intensidad (con refuerzo de tormenta) para el secado escalonado.
        let intensidad = (lluvia * (0.72 + 0.28 * trueno)).clamp(0.0, 1.0);
        st.lluvia_pico = st.lluvia_pico.max(intensidad);
        st.lluvia_secado_s = 0.0;
    } else if st.lluvia_pico > 0.0 {
        st.lluvia_secado_s = (st.lluvia_secado_s + dt).min(LLUVIA_SECADO_S);
        if st.lluvia_secado_s >= LLUVIA_SECADO_S {
            st.lluvia_pico = 0.0;
        }
    }

    // Escarcha en el frío: crece más rápido mientras nieva y se retira despacio al calentarse.
    st.bioma_frio = frio;
    if st.escarcha < obj_escarcha {
        let ritmo = if nevando > 0.01 { ESCARCHA_CRECE_S * 0.55 } else { ESCARCHA_CRECE_S };
        st.escarcha = (st.escarcha + dt / ritmo).min(obj_escarcha);
    } else if st.escarcha > obj_escarcha {
        st.escarcha = (st.escarcha - dt / ESCARCHA_RETIRA_S).max(obj_escarcha);
    }

    // Copos pegados y montón inferior de nieve (parte 3): los copos se caen en ~8 s al parar;
    // el montón crece mientras nieva y se funde despacio (~45 s en frío, ~15 s al calentarse).
    if nevando > 0.01 {
        st.copos = (st.copos + dt / COPOS_CRECEN_S).min(nevando);
        st.monton = (st.monton + dt / MONTON_CRECE_S).min(nevando);
    } else {
        st.copos = (st.copos - dt / COPOS_CAEN_S).max(0.0);
        let funde_s = if frio { MONTON_FUNDE_S } else { MONTON_FUNDE_S * 0.35 };
        st.monton = (st.monton - dt / funde_s).max(0.0);
    }

    // Normaliza/acota el viento de 26 a [-1, 1] conservando su dirección.
    st.viento_xz = [
        viento_xz[0].clamp(-1.0, 1.0),
        viento_xz[1].clamp(-1.0, 1.0),
    ];
    *lock = Some(st);
}

/// Actualiza la memoria del vaho del aliento (parte 4): punto del cristal más cercano (< 1,5 bloques),
/// desvanecido en ~5 s al alejarse y desactivación inmediata en modo espectador.
pub fn observar_aliento(punto_cristal_cercano: Option<[f64; 3]>, espectador: bool) {
    observar_aliento_en(punto_cristal_cercano, espectador, Instant::now());
}

/// Variante determinista para pruebas unitarias (parte 4).
pub fn observar_aliento_en(
    punto_cristal_cercano: Option<[f64; 3]>,
    espectador: bool,
    ahora: Instant,
) {
    let Ok(mut lock) = ESTADO.lock() else { return };
    let Some(st) = lock.as_mut() else { return };

    let paso = ahora
        .saturating_duration_since(st.ultimo_aliento)
        .as_secs_f32()
        .clamp(0.0, MAX_DT_S);
    st.ultimo_aliento = ahora;

    st.espectador = espectador;
    if espectador || !config().aliento {
        st.vaho_peso = 0.0;
        st.tiene_vaho_pos = false;
        return;
    }

    if let Some(pt) = punto_cristal_cercano {
        if st.escarcha > 0.01 {
            if !st.tiene_vaho_pos || st.vaho_peso < 0.05 {
                st.vaho_pos_mundo = pt;
            } else {
                // Suaviza el centro de la mancha al desplazarse junto a un ventanal.
                let k = (paso * 6.0).clamp(0.05, 0.5) as f64;
                st.vaho_pos_mundo = [
                    st.vaho_pos_mundo[0] + (pt[0] - st.vaho_pos_mundo[0]) * k,
                    st.vaho_pos_mundo[1] + (pt[1] - st.vaho_pos_mundo[1]) * k,
                    st.vaho_pos_mundo[2] + (pt[2] - st.vaho_pos_mundo[2]) * k,
                ];
            }
            st.tiene_vaho_pos = true;
            st.vaho_peso = (st.vaho_peso + paso / VAHO_CRECE_S).min(1.0);
            return;
        }
    }

    if st.vaho_peso > 0.0 {
        st.vaho_peso = (st.vaho_peso - paso / VAHO_DESVANECE_S).max(0.0);
        if st.vaho_peso <= 0.0 {
            st.tiene_vaho_pos = false;
        }
    }
}

/// Observa la arena tras tormenta en bioma seco (parte 5): crece con lluvia/tormenta eléctrica y
/// el viento se la lleva en ~3 min tras despejar.
pub fn observar_arena(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    bioma_seco: bool,
    arena_roja: bool,
) {
    observar_arena_en(fuerza_lluvia, fuerza_trueno, bioma_seco, arena_roja, Instant::now());
}

/// Variante determinista para pruebas unitarias (parte 5).
pub fn observar_arena_en(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    bioma_seco: bool,
    arena_roja: bool,
    ahora: Instant,
) {
    let Ok(mut lock) = ESTADO.lock() else { return };
    let Some(st) = lock.as_mut() else { return };

    let dt = ahora
        .saturating_duration_since(st.ultimo_arena)
        .as_secs_f32()
        .clamp(0.0, MAX_DT_S);
    st.ultimo_arena = ahora;

    st.bioma_seco = bioma_seco;
    if bioma_seco {
        st.arena_roja = arena_roja;
    }

    let lluvia = fuerza_lluvia.clamp(0.0, 1.0);
    let trueno = fuerza_trueno.clamp(0.0, 1.0);
    if bioma_seco && lluvia > 0.01 {
        let objetivo = (0.68 * lluvia + 0.32 * trueno).clamp(0.0, 1.0);
        let ritmo = if trueno > 0.2 { ARENA_CRECE_S * 0.65 } else { ARENA_CRECE_S };
        st.arena = (st.arena + dt / ritmo).min(objetivo.max(st.arena));
    } else if st.arena > 0.0 {
        st.arena = (st.arena - dt / ARENA_LIMPIA_S).max(0.0);
    }
}
/// Empaqueta dos valores normalizados `[0, 1]` a 12 bits cada uno en un `f32` exacto (≤ 2²⁴ − 1).
#[inline]
pub fn empaquetar_par12(alto: f32, bajo: f32) -> f32 {
    let a = (alto.clamp(0.0, 1.0) * 4095.0).round() as u32;
    let b = (bajo.clamp(0.0, 1.0) * 4095.0).round() as u32;
    ((a.min(4095) * 4096) + b.min(4095)) as f32
}

/// Empaqueta un valor normalizado `[0, 1]` (12 bits altos) y una máscara entera (12 bits bajos).
#[inline]
pub fn empaquetar_valor12_flags12(alto: f32, flags12: u32) -> f32 {
    let a = (alto.clamp(0.0, 1.0) * 4095.0).round() as u32;
    ((a.min(4095) * 4096) + (flags12 & 0x0FFF)) as f32
}

/// Empaqueta tres valores normalizados `[0, 1]` a 8 bits cada uno en un `f32` exacto (24 bits).
#[inline]
pub fn empaquetar_triple8(a: f32, b: f32, c: f32) -> f32 {
    let qa = (a.clamp(0.0, 1.0) * 255.0).round() as u32;
    let qb = (b.clamp(0.0, 1.0) * 255.0).round() as u32;
    let qc = (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    (qa.min(255) + qb.min(255) * 256 + qc.min(255) * 65536) as f32
}

/// Devuelve los dos `vec4<f32>` (`mods[6]` y `mods[7]`) para el shader, o `None` si aún no hay mundo observado.
pub fn shader_params(camara: Option<[f64; 3]>) -> Option<([f32; 4], [f32; 4])> {
    let st = (*ESTADO.lock().ok()?)?;
    let cfg = config();

    let secado_frac = (st.lluvia_secado_s / LLUVIA_SECADO_S).clamp(0.0, 1.0);
    let m6_x = empaquetar_par12(st.lluvia_pico, secado_frac);
    let m6_y = empaquetar_triple8(st.escarcha, st.copos, st.monton);

    let mut flags12: u32 = cfg.lluvia.codigo()
        | (cfg.nieve.codigo() << 2)
        | (cfg.arena.codigo() << 4);
    if cfg.aliento {
        flags12 |= 1 << 6;
    }
    if cfg.escarcha_interior {
        flags12 |= 1 << 7;
    }
    if st.arena_roja {
        flags12 |= 1 << 8;
    }
    if st.espectador {
        flags12 |= 1 << 9;
    }
    if st.bioma_seco {
        flags12 |= 1 << 10;
    }
    if st.bioma_frio {
        flags12 |= 1 << 11;
    }
    // Solo aplica arena sobre los cristales del bioma seco (spec 27 §5).
    let arena_visible = if st.bioma_seco { st.arena } else { 0.0 };
    let m6_z = empaquetar_valor12_flags12(arena_visible, flags12);
    let m6_w = empaquetar_par12(st.viento_xz[0] * 0.5 + 0.5, st.viento_xz[1] * 0.5 + 0.5);

    let m7 = if st.tiene_vaho_pos && st.vaho_peso > 0.0 {
        if let Some(cam) = camara {
            [
                (st.vaho_pos_mundo[0] - cam[0]) as f32,
                (st.vaho_pos_mundo[1] - cam[1]) as f32,
                (st.vaho_pos_mundo[2] - cam[2]) as f32,
                st.vaho_peso.clamp(0.0, 1.0),
            ]
        } else {
            [0.0, 0.0, 0.0, 0.0]
        }
    } else {
        [0.0, 0.0, 0.0, 0.0]
    };

    Some(([m6_x, m6_y, m6_z, m6_w], m7))
}

/// Resumen legible para `/vulpe estado`.
pub fn resumen_estado() -> String {
    let cfg = config();
    let Some(st) = ESTADO.lock().ok().and_then(|g| *g) else {
        return "n/d".to_string();
    };
    format!(
        "lluvia={}({:.2}/sec={:.1}s) escarcha={:.2} copos={:.2} monton={:.2} vaho={:.2} arena={:.2}{} nieve={} arena_cfg={} aliento={}",
        cfg.lluvia.as_str(),
        st.lluvia_pico,
        st.lluvia_secado_s,
        st.escarcha,
        st.copos,
        st.monton,
        st.vaho_peso,
        st.arena,
        if st.arena_roja { "(roja)" } else { "" },
        cfg.nieve.as_str(),
        cfg.arena.as_str(),
        if cfg.aliento { "sí" } else { "no" },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn test_lock() -> std::sync::MutexGuard<'static, ()> {
        crate::GLOBAL_TEST_MUTEX.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn test_config_toml_roundtrip() {
        let _g = test_lock();
        let texto = "[clima]\nlluvia = false\n[cristales]\nlluvia = \"intensa\"\nnieve = \"ligera\"\narena = \"apagada\"\naliento = false\nescarcha_interior = true\n";
        let cfg = CristalesConfig::from_toml_str(texto);
        assert_eq!(cfg.lluvia, NivelCristales::Intensa);
        assert_eq!(cfg.nieve, NivelCristales::Ligera);
        assert_eq!(cfg.arena, NivelCristales::Apagada);
        assert!(!cfg.aliento);
        assert!(cfg.escarcha_interior);
        let roundtrip = CristalesConfig::from_toml_str(&cfg.to_toml_section());
        assert_eq!(roundtrip, cfg);
    }

    #[test]
    fn test_secado_30s_tras_lluvia() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        let t0 = Instant::now();

        // Lloviendo con tormenta
        observar_lluvia_en(1.0, 1.0, true, [-0.6, 0.2], t0);
        let (m6, _) = shader_params(None).unwrap();
        let code = m6[0] as u32;
        let pico = (code / 4096) as f32 / 4095.0;
        let secado = (code % 4096) as f32 / 4095.0;
        assert!((pico - 1.0).abs() < 1e-3);
        assert!(secado < 1e-3);

        // Para de llover: avanza 15 s -> mitad de secado, mantiene pico
        let mut t = t0;
        for _ in 0..15 {
            t += Duration::from_secs(1);
            observar_lluvia_en(0.0, 0.0, true, [-0.6, 0.2], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let code = m6[0] as u32;
        let pico = (code / 4096) as f32 / 4095.0;
        let secado = (code % 4096) as f32 / 4095.0;
        assert!((pico - 1.0).abs() < 1e-3);
        assert!((secado - 0.5).abs() < 0.02);

        // Otros 15 s (30 s en total) -> seco por completo (pico = 0)
        for _ in 0..15 {
            t += Duration::from_secs(1);
            observar_lluvia_en(0.0, 0.0, true, [-0.6, 0.2], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let code = m6[0] as u32;
        let pico = (code / 4096) as f32 / 4095.0;
        assert!(pico < 1e-3);

        limpiar();
    }

    #[test]
    fn test_empaquetado_exacto_f32() {
        let f = empaquetar_par12(0.75, 0.25);
        let u = f as u32;
        assert_eq!(u / 4096, 3071);
        assert_eq!(u % 4096, 1024);

        let t = empaquetar_triple8(1.0, 0.5, 0.25);
        let tu = t as u32;
        assert_eq!(tu & 255, 255);
        assert_eq!((tu >> 8) & 255, 128);
        assert_eq!((tu >> 16) & 255, 64);
    }

    #[test]
    fn test_escarcha_crece_en_frio_y_se_retira_al_calentar() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        let t0 = Instant::now();

        // Bioma nevado de noche: crece más que de día
        assert!(objetivo_escarcha(-0.5, 1.0, 0.0) > objetivo_escarcha(-0.5, 0.0, 0.0));
        assert!(objetivo_escarcha(0.0, 0.0, 1.0) > objetivo_escarcha(0.0, 0.0, 0.0));
        assert_eq!(objetivo_escarcha(0.8, 1.0, 0.0), 0.0);

        observar_lluvia_y_frio_en(0.0, 0.0, false, -0.5, 1.0, [0.0, 0.0], t0);
        let mut t = t0;
        for _ in 0..10 {
            t += Duration::from_secs(1);
            observar_lluvia_y_frio_en(1.0, 0.0, false, -0.5, 1.0, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let esc_frio = ((m6[1] as u32) & 255) as f32 / 255.0;
        assert!(esc_frio > 0.9, "escarcha llena en nevada nocturna: {esc_frio}");

        // Al calentarse (bioma templado), se retira gradualmente (no de golpe)
        t += Duration::from_secs(1);
        observar_lluvia_y_frio_en(0.0, 0.0, true, 0.8, 0.0, [0.0, 0.0], t);
        let (m6, _) = shader_params(None).unwrap();
        let esc_tras_1s = ((m6[1] as u32) & 255) as f32 / 255.0;
        assert!(esc_tras_1s > 0.8 && esc_tras_1s < esc_frio);

        for _ in 0..30 {
            t += Duration::from_secs(1);
            observar_lluvia_y_frio_en(0.0, 0.0, true, 0.8, 0.0, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let esc_fin = ((m6[1] as u32) & 255) as f32 / 255.0;
        assert!(esc_fin < 0.01);
        limpiar();
    }

    #[test]
    fn test_copos_y_monton_de_nieve() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        let t0 = Instant::now();

        observar_lluvia_y_frio_en(1.0, 0.0, false, -0.3, 0.0, [0.0, 0.0], t0);
        let mut t = t0;
        for _ in 0..12 {
            t += Duration::from_secs(1);
            observar_lluvia_y_frio_en(1.0, 0.0, false, -0.3, 0.0, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let packed = m6[1] as u32;
        let copos = ((packed >> 8) & 255) as f32 / 255.0;
        let monton = ((packed >> 16) & 255) as f32 / 255.0;
        assert!(copos > 0.95 && monton > 0.95, "copos={copos} monton={monton}");

        // Al parar de nevar: en 9 s los copos ya cayeron pero el montón sigue parcialmente ahí.
        for _ in 0..9 {
            t += Duration::from_secs(1);
            observar_lluvia_y_frio_en(0.0, 0.0, false, -0.3, 0.0, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let packed = m6[1] as u32;
        let copos_tras = ((packed >> 8) & 255) as f32 / 255.0;
        let monton_tras = ((packed >> 16) & 255) as f32 / 255.0;
        assert!(copos_tras < 0.01, "copos deben caer en ~8 s: {copos_tras}");
        assert!(monton_tras > 0.6, "montón se funde despacio: {monton_tras}");
        limpiar();
    }

    #[test]
    fn test_vaho_aliento_y_espectador() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        assert!(es_bloque_cristal("minecraft:glass"));
        assert!(es_bloque_cristal("minecraft:blue_stained_glass_pane"));
        assert!(es_bloque_cristal("minecraft:tinted_glass"));
        assert!(!es_bloque_cristal("minecraft:stone"));

        let t0 = Instant::now();
        // Bioma frío con escarcha
        observar_lluvia_y_frio_en(0.0, 0.0, false, -0.4, 1.0, [0.0, 0.0], t0);
        let mut t = t0;
        for _ in 0..6 {
            t += Duration::from_secs(1);
            observar_aliento_en(Some([10.0, 65.0, 20.0]), false, t);
        }
        let (_, m7) = shader_params(Some([10.0, 65.0, 21.0])).unwrap();
        assert!(m7[3] > 0.9, "vaho cargado tras 6 s cerca: {}", m7[3]);
        assert!((m7[2] - (-1.0)).abs() < 1e-3);

        // Al alejarse se desvanece en ~5 s manteniendo la posición
        t += Duration::from_secs(2);
        observar_aliento_en(None, false, t);
        let (_, m7_mid) = shader_params(Some([10.0, 65.0, 24.0])).unwrap();
        assert!(m7_mid[3] > 0.4 && m7_mid[3] < 0.85, "desvaneciendo: {}", m7_mid[3]);

        // En modo espectador se apaga de inmediato y marca bit 9
        t += Duration::from_secs(1);
        observar_aliento_en(Some([10.0, 65.0, 20.0]), true, t);
        let (m6, m7_spec) = shader_params(Some([10.0, 65.0, 21.0])).unwrap();
        assert_eq!(m7_spec[3], 0.0);
        assert_ne!((m6[2] as u32) & (1 << 9), 0);
        limpiar();
    }

    #[test]
    fn test_arena_tras_tormenta_y_mesa_roja() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        assert!(es_bioma_seco("minecraft:desert", false, 2.0));
        assert!(es_bioma_seco("minecraft:badlands", false, 2.0));
        assert!(!es_bioma_seco("minecraft:nether_wastes", false, 2.0));
        assert!(es_bioma_mesa("minecraft:wooded_badlands"));
        assert!(!es_bioma_mesa("minecraft:desert"));

        let t0 = Instant::now();
        observar_lluvia_y_frio_en(1.0, 1.0, false, 2.0, 0.0, [-0.8, 0.2], t0);
        let mut t = t0;
        for _ in 0..10 {
            t += Duration::from_secs(1);
            observar_arena_en(1.0, 1.0, true, true, t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let code = m6[2] as u32;
        let arena = (code / 4096) as f32 / 4095.0;
        assert!(arena > 0.9, "arena acumulada en tormenta eléctrica: {arena}");
        assert_ne!(code & (1 << 8), 0, "bandera arena roja en mesa");

        // Tras despejar 60 s: aún queda ~2/3 de arena (se va en ~3 min).
        for _ in 0..60 {
            t += Duration::from_secs(1);
            observar_arena_en(0.0, 0.0, true, true, t);
        }
        let (m6_tras, _) = shader_params(None).unwrap();
        let arena_tras = ((m6_tras[2] as u32) / 4096) as f32 / 4095.0;
        assert!(arena_tras > 0.5 && arena_tras < arena, "arena bajando: {arena_tras}");

        // Al salir a un bioma no seco no se dibuja arena en sus cristales.
        t += Duration::from_secs(1);
        observar_arena_en(0.0, 0.0, false, false, t);
        let (m6_fuera, _) = shader_params(None).unwrap();
        assert_eq!((m6_fuera[2] as u32) / 4096, 0);
        limpiar();
    }
}
