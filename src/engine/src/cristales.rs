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

/// Observa el clima de lluvia y el viento para actualizar el estado de mojado y secado (parte 1).
pub fn observar_lluvia(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    viento_xz: [f32; 2],
) {
    observar_lluvia_en(
        fuerza_lluvia,
        fuerza_trueno,
        llueve_en_bioma,
        viento_xz,
        Instant::now(),
    );
}

/// Variante determinista con marca de tiempo explícita para pruebas unitarias.
pub fn observar_lluvia_en(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    viento_xz: [f32; 2],
    ahora: Instant,
) {
    let Ok(mut lock) = ESTADO.lock() else { return };
    let mut st = lock.unwrap_or(EstadoCristales {
        lluvia_pico: 0.0,
        lluvia_secado_s: LLUVIA_SECADO_S,
        viento_xz: [0.0, 0.0],
        escarcha: 0.0,
        copos: 0.0,
        monton: 0.0,
        arena: 0.0,
        arena_roja: false,
        espectador: false,
        bioma_seco: false,
        bioma_frio: false,
        vaho_peso: 0.0,
        vaho_pos_mundo: [0.0; 3],
        tiene_vaho_pos: false,
        ultimo: ahora,
    });

    let dt = ahora
        .saturating_duration_since(st.ultimo)
        .as_secs_f32()
        .min(MAX_DT_S);
    st.ultimo = ahora;

    let lluvia = fuerza_lluvia.clamp(0.0, 1.0);
    let trueno = fuerza_trueno.clamp(0.0, 1.0);
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

    // Normaliza/acota el viento de 26 a [-1, 1] conservando su dirección.
    st.viento_xz = [
        viento_xz[0].clamp(-1.0, 1.0),
        viento_xz[1].clamp(-1.0, 1.0),
    ];
    *lock = Some(st);
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
    let m6_z = empaquetar_valor12_flags12(st.arena, flags12);
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
        "lluvia={}({:.2}/sec={:.1}s) nieve={} arena={} aliento={}",
        cfg.lluvia.as_str(),
        st.lluvia_pico,
        st.lluvia_secado_s,
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
}
