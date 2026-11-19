//! Estado global, rampas y configuración del efecto «el tiempo en los cristales» (subsistema 27, versión 2 / N-09).
//!
//! Mantiene entre fotogramas los acumuladores lineales y el estado del aliento descritos en
//! `vulpe-horizon-spec/subsistemas/27-tiempo-en-los-cristales.md`:
//! - escarcha de fuera (sube 90 s, baja 150 s; objetivos 1,0 nevando / 0,7 noche / 0,45 día en bioma frío);
//! - repisa de nieve (sube 60 s, baja 240 s);
//! - arena en bioma seco (sube 60 s, baja 180 s; visible > 1/64; amarilla o roja en mesa);
//! - condensación de dentro (sube 90 s, baja 60 s; objetivo 1 de noche 12 542–23 460 o con tormenta);
//! - vaho del aliento (exhalaciones cada 2,8–3,8 s de 0,5 s y +0,45, hasta ×1,7 en 5 seguidas a < 0,3 bloques,
//!   mantiene 35 s y se va en 20 s; apagado en espectador).
//!
//! Todo viaja en `frame.mods[6]` y `frame.mods[7]` (formato v4, firma `1004.0`, bandera bit 8 `FLAG_GLASS_VALID`).

use std::sync::Mutex;
use std::time::Instant;

/// Umbral de intensidad de clima `lluvia * (1 + 0,4 * trueno)` para considerar precipitación activa (27 §Tiempo).
pub const CLIMA_UMBRAL: f32 = 0.05;

/// Umbral de temperatura de nieve en Minecraft (biomas fríos o altura con nieve).
pub const TEMP_UMBRAL_NIEVE: f32 = 0.15;

/// Rango de ticks del día considerado noche para escarcha y condensación (27 §Tiempo).
pub const NOCHE_INICIO_TICKS: i64 = 12_542;
pub const NOCHE_FIN_TICKS: i64 = 23_460;

/// Rampas lineales (segundos, 27 §Tiempo y estado).
pub const ESCARCHA_SUBE_S: f32 = 90.0;
pub const ESCARCHA_BAJA_S: f32 = 150.0;
pub const REPISA_SUBE_S: f32 = 60.0;
pub const REPISA_BAJA_S: f32 = 240.0;
pub const ARENA_SUBE_S: f32 = 60.0;
pub const ARENA_BAJA_S: f32 = 180.0;
pub const ARENA_VISIBLE_MIN: f32 = 1.0 / 64.0;
pub const CONDENSACION_SUBE_S: f32 = 90.0;
pub const CONDENSACION_BAJA_S: f32 = 60.0;

/// Parámetros del vaho del aliento (27 §Capa 6).
pub const VAHO_DISTANCIA_MAX: f64 = 1.5;
pub const VAHO_EXHALACION_MIN_S: f32 = 2.8;
pub const VAHO_EXHALACION_MAX_S: f32 = 3.8;
pub const VAHO_EXHALACION_DUR_S: f32 = 0.5;
pub const VAHO_EXHALACION_APORTE: f32 = 0.45;
pub const VAHO_MISMO_SITIO_DIST: f64 = 0.3;
pub const VAHO_REPETICIONES_MAX: u32 = 5;
pub const VAHO_ESCALA_MAX: f32 = 1.7;
pub const VAHO_MANTIENE_S: f32 = 35.0;
pub const VAHO_DESVANECE_S: f32 = 20.0;

/// Paso máximo de tiempo entre observaciones para no saltar temporizadores tras una pausa.
const MAX_DT_S: f32 = 1.0;

/// Cierto si la hora del día (ticks) cae en la ventana nocturna de la spec (12 542..=23 460).
pub fn es_noche_ticks(day_time: Option<i64>) -> bool {
    let Some(t) = day_time else { return false };
    let h = t.rem_euclid(24_000);
    (NOCHE_INICIO_TICKS..=NOCHE_FIN_TICKS).contains(&h)
}

/// Intensidad del clima según spec 27 §Tiempo: `lluvia * (1 + 0,4 * trueno)`.
#[inline]
pub fn intensidad_clima(lluvia: f32, trueno: f32) -> f32 {
    lluvia.clamp(0.0, 1.0) * (1.0 + 0.4 * trueno.clamp(0.0, 1.0))
}

/// Cierto si un bioma del Overworld es seco (sin precipitación, temperatura >= 1,0: desierto, sabana, mesas).
pub fn es_bioma_seco(bioma: &str, has_precipitation: bool, temperature: f32) -> bool {
    crate::clima::es_overworld(bioma) && !has_precipitation && temperature >= 1.0
}

/// Cierto si la arena del bioma es roja (mesas / badlands y variantes de mods).
pub fn es_bioma_mesa(bioma: &str) -> bool {
    let n = bioma.rsplit(':').next().unwrap_or(bioma);
    n.contains("badlands") || n.contains("mesa") || n.contains("red_desert")
}

/// Cierto si el identificador de bloque corresponde a un cristal o panel de cristal (27 §Bloques y caras).
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

/// Intensidad configurable por parte en `[cristales]` (27 §Opciones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NivelCristales {
    Apagada = 0,
    Poca = 1,
    Normal = 2,
    Mucha = 3,
}

impl NivelCristales {
    pub fn codigo(self) -> u32 {
        self as u32
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apagada => "apagada",
            Self::Poca => "poca",
            Self::Normal => "normal",
            Self::Mucha => "mucha",
        }
    }

    pub fn escala_lluvia_nieve(self) -> f32 {
        match self {
            Self::Apagada => 0.0,
            Self::Poca => 0.5,
            Self::Normal => 1.0,
            Self::Mucha => 1.7,
        }
    }

    pub fn escala_arena(self) -> f32 {
        match self {
            Self::Apagada => 0.0,
            Self::Poca => 0.5,
            Self::Normal => 1.0,
            Self::Mucha => 1.4,
        }
    }

    pub fn from_toml_value(raw: &str) -> Option<Self> {
        let limpio = raw.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase();
        match limpio.as_str() {
            "apagada" | "apagado" | "off" | "0" | "false" => Some(Self::Apagada),
            "poca" | "ligera" | "baja" | "1" => Some(Self::Poca),
            "normal" | "media" | "2" | "true" => Some(Self::Normal),
            "mucha" | "intensa" | "alta" | "3" => Some(Self::Mucha),
            _ => None,
        }
    }
}

/// Estilo de lluvia en los cristales (27 §Opciones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstiloLluvia {
    Gotas = 0,
    Regueros = 1,
}

impl EstiloLluvia {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Gotas => "gotas",
            Self::Regueros => "regueros",
        }
    }

    pub fn from_toml_value(raw: &str) -> Option<Self> {
        match raw.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase().as_str() {
            "gotas" | "drops" | "0" => Some(Self::Gotas),
            "regueros" | "streaks" | "1" => Some(Self::Regueros),
            _ => None,
        }
    }
}

/// Resolución en texeles por bloque (16, 32 por defecto, 64).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolucionCristales {
    R16 = 0,
    R32 = 1,
    R64 = 2,
}

impl ResolucionCristales {
    pub fn texeles(self) -> u32 {
        match self {
            Self::R16 => 16,
            Self::R32 => 32,
            Self::R64 => 64,
        }
    }

    pub fn from_toml_value(raw: &str) -> Option<Self> {
        match raw.trim().trim_matches('"').trim_matches('\'') {
            "16" => Some(Self::R16),
            "32" => Some(Self::R32),
            "64" => Some(Self::R64),
            _ => None,
        }
    }
}

/// Estilo del borde helado interior (27 §Capa 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EstiloBordeHelado {
    Apagado = 0,
    Banda = 1,
    Cristales = 2,
}

impl EstiloBordeHelado {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apagado => "apagado",
            Self::Banda => "banda",
            Self::Cristales => "cristales",
        }
    }

    pub fn from_toml_value(raw: &str) -> Option<Self> {
        match raw.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase().as_str() {
            "apagado" | "off" | "0" => Some(Self::Apagado),
            "banda" | "band" | "1" => Some(Self::Banda),
            "cristales" | "crystals" | "2" => Some(Self::Cristales),
            _ => None,
        }
    }
}

/// Nivel del desenfoque detrás del cristal empañado (27 §Capa 7).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NivelDesenfoque {
    Apagado = 0,
    Suave = 1,
    Normal = 2,
    Fuerte = 3,
}

impl NivelDesenfoque {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Apagado => "apagado",
            Self::Suave => "suave",
            Self::Normal => "normal",
            Self::Fuerte => "fuerte",
        }
    }

    pub fn from_toml_value(raw: &str) -> Option<Self> {
        match raw.trim().trim_matches('"').trim_matches('\'').to_ascii_lowercase().as_str() {
            "apagado" | "off" | "0" => Some(Self::Apagado),
            "suave" | "soft" | "1" => Some(Self::Suave),
            "normal" | "medio" | "2" => Some(Self::Normal),
            "fuerte" | "strong" | "3" => Some(Self::Fuerte),
            _ => None,
        }
    }
}

/// Ajustes de la sección `[cristales]` en `config/vulpe-horizon.toml` (spec 27 v2 §Opciones).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CristalesConfig {
    pub lluvia: NivelCristales,
    pub nieve: NivelCristales,
    pub arena: NivelCristales,
    pub estilo_lluvia: EstiloLluvia,
    pub resolucion: ResolucionCristales,
    pub seguir_viento: bool,
    pub interior: bool,
    pub condensacion_pct: u16,          // 0 (apagado) o 50..=250 (por defecto 100)
    pub blancura_condensacion_pct: u8,  // 0..=100 (por defecto 0)
    pub borde_helado: EstiloBordeHelado,
    pub desenfoque: NivelDesenfoque,
    pub aliento_pct: u16,               // 0 (apagado) o 50..=250 (por defecto 150)
    pub blancura_aliento_pct: u8,       // 0..=100 (por defecto 100)
}

impl Default for CristalesConfig {
    fn default() -> Self {
        Self {
            lluvia: NivelCristales::Normal,
            nieve: NivelCristales::Normal,
            arena: NivelCristales::Normal,
            estilo_lluvia: EstiloLluvia::Gotas,
            resolucion: ResolucionCristales::R32,
            seguir_viento: true,
            interior: true,
            condensacion_pct: 100,
            blancura_condensacion_pct: 0,
            borde_helado: EstiloBordeHelado::Banda,
            desenfoque: NivelDesenfoque::Suave,
            aliento_pct: 150,
            blancura_aliento_pct: 100,
        }
    }
}

impl CristalesConfig {
    /// Lee la sección `[cristales]` de un archivo TOML simple; lo que falte queda por defecto.
    /// Mantiene compatibilidad hacia atrás con claves de N-08 (`ligera`, `intensa`, `aliento = true/false`, `escarcha_interior`).
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
            let limpio = v.trim_matches('"').trim_matches('\'').to_ascii_lowercase();
            let as_bool = matches!(limpio.as_str(), "true" | "1" | "1.0" | "si" | "sí");
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
                "estilo_lluvia" => {
                    if let Some(e) = EstiloLluvia::from_toml_value(v) {
                        c.estilo_lluvia = e;
                    }
                }
                "resolucion" => {
                    if let Some(r) = ResolucionCristales::from_toml_value(v) {
                        c.resolucion = r;
                    }
                }
                "seguir_viento" => c.seguir_viento = as_bool,
                "interior" => c.interior = as_bool,
                "escarcha_interior" => {
                    // Compatibilidad N-08: si el usuario lo activaba explícitamente, asegura interior activo.
                    if as_bool {
                        c.interior = true;
                    }
                }
                "condensacion" => {
                    if limpio == "apagado" || limpio == "apagada" || limpio == "false" || limpio == "off" {
                        c.condensacion_pct = 0;
                    } else if let Ok(p) = limpio.trim_end_matches('%').parse::<u16>() {
                        c.condensacion_pct = p.min(250);
                    }
                }
                "blancura_condensacion" => {
                    if let Ok(p) = limpio.trim_end_matches('%').parse::<u8>() {
                        c.blancura_condensacion_pct = p.min(100);
                    }
                }
                "borde_helado" => {
                    if let Some(b) = EstiloBordeHelado::from_toml_value(v) {
                        c.borde_helado = b;
                    }
                }
                "desenfoque" => {
                    if let Some(d) = NivelDesenfoque::from_toml_value(v) {
                        c.desenfoque = d;
                    }
                }
                "aliento" => {
                    if limpio == "false" || limpio == "apagado" || limpio == "apagada" || limpio == "off" {
                        c.aliento_pct = 0;
                    } else if limpio == "true" || limpio == "si" || limpio == "sí" {
                        c.aliento_pct = 150;
                    } else if let Ok(p) = limpio.trim_end_matches('%').parse::<u16>() {
                        c.aliento_pct = p.min(250);
                    }
                }
                "blancura_aliento" => {
                    if let Ok(p) = limpio.trim_end_matches('%').parse::<u8>() {
                        c.blancura_aliento_pct = p.min(100);
                    }
                }
                _ => {}
            }
        }
        c
    }

    /// Sección `[cristales]` para añadir a `config/vulpe-horizon.toml`.
    pub fn to_toml_section(&self) -> String {
        format!(
            "\n[cristales]\n\
             # Intensidad por parte: apagada | poca | normal | mucha\n\
             lluvia = \"{}\"\n\
             nieve = \"{}\"\n\
             arena = \"{}\"\n\
             # Estilo de lluvia: gotas | regueros\n\
             estilo_lluvia = \"{}\"\n\
             # Resolución en texeles por bloque: 16 | 32 | 64\n\
             resolucion = {}\n\
             # Seguir el viento de Particle Rain (26)\n\
             seguir_viento = {}\n\
             # Lado de dentro (condensación, borde helado, aliento y desenfoque)\n\
             interior = {}\n\
             # Condensación interior (0 = apagado, 50..250 %)\n\
             condensacion = {}\n\
             # Blancura de la condensación (0..100 %)\n\
             blancura_condensacion = {}\n\
             # Borde helado interior: banda | cristales | apagado\n\
             borde_helado = \"{}\"\n\
             # Desenfoque detrás del cristal empañado: apagado | suave | normal | fuerte\n\
             desenfoque = \"{}\"\n\
             # Aliento cerca del cristal (0 = apagado, 50..250 %)\n\
             aliento = {}\n\
             # Blancura del aliento (0..100 %)\n\
             blancura_aliento = {}\n",
            self.lluvia.as_str(),
            self.nieve.as_str(),
            self.arena.as_str(),
            self.estilo_lluvia.as_str(),
            self.resolucion.texeles(),
            self.seguir_viento,
            self.interior,
            self.condensacion_pct,
            self.blancura_condensacion_pct,
            self.borde_helado.as_str(),
            self.desenfoque.as_str(),
            self.aliento_pct,
            self.blancura_aliento_pct,
        )
    }
}

/// Estado dinámico observado cada fotograma para los cristales (27 v2).
#[derive(Clone, Copy, Debug)]
struct EstadoCristales {
    escarcha: f32,
    repisa: f32,
    arena: f32,
    condensacion: f32,
    viento_xz: [f32; 2],
    arena_roja: bool,
    espectador: bool,
    bioma_seco: bool,
    bioma_frio: bool,
    llueve_en_bioma: bool,

    // Estado del vaho del aliento (27 §Capa 6)
    vaho_peso: f32,
    vaho_pos_mundo: [f64; 3],
    tiene_vaho_pos: bool,
    vaho_repeticiones: u32,       // 1..=5 exhalaciones en el mismo sitio (< 0,3 bloques)
    vaho_desde_ultima_exh_s: f32, // segundos transcurridos desde el fin de la última exhalación
    vaho_prox_exh_s: f32,         // cuenta atrás hasta la siguiente exhalación (2,8..3,8 s)
    vaho_exh_restante_s: f32,     // > 0 mientras dura la exhalación de 0,5 s
    vaho_semilla: u32,            // PRNG ligero para elegir el siguiente intervalo 2,8..3,8 s

    ultimo: Instant,
    ultimo_aliento: Instant,
}

static CONFIG: Mutex<CristalesConfig> = Mutex::new(CristalesConfig {
    lluvia: NivelCristales::Normal,
    nieve: NivelCristales::Normal,
    arena: NivelCristales::Normal,
    estilo_lluvia: EstiloLluvia::Gotas,
    resolucion: ResolucionCristales::R32,
    seguir_viento: true,
    interior: true,
    condensacion_pct: 100,
    blancura_condensacion_pct: 0,
    borde_helado: EstiloBordeHelado::Banda,
    desenfoque: NivelDesenfoque::Suave,
    aliento_pct: 150,
    blancura_aliento_pct: 100,
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

/// Limpia el estado dinámico (sin mundo cargado o cambio de mundo).
pub fn limpiar() {
    if let Ok(mut lock) = ESTADO.lock() {
        *lock = None;
    }
}

#[inline]
fn avanzar_rampa(actual: f32, objetivo: f32, dt: f32, sube_s: f32, baja_s: f32) -> f32 {
    if actual < objetivo {
        (actual + dt / sube_s.max(1e-3)).min(objetivo)
    } else if actual > objetivo {
        (actual - dt / baja_s.max(1e-3)).max(objetivo)
    } else {
        objetivo
    }
}

/// Objetivo de escarcha de fuera según spec 27 §Tiempo y estado:
/// 1,0 nevando; 0,7 de noche sin nevar; 0,45 de día sin nevar (solo en bioma frío).
pub fn objetivo_escarcha(bioma_frio: bool, es_noche: bool, nevando: bool) -> f32 {
    if !bioma_frio {
        return 0.0;
    }
    if nevando {
        1.0
    } else if es_noche {
        0.7
    } else {
        0.45
    }
}

/// Objetivo de arena según spec 27 §Tiempo y estado:
/// con lluvia en bioma seco: `min(1, (1 si trueno > 0,5 else 0,5) * escala_arena)`; 0 sin lluvia.
pub fn objetivo_arena(bioma_seco: bool, clima: f32, trueno: f32, nivel: NivelCristales) -> f32 {
    if !bioma_seco || clima <= CLIMA_UMBRAL || nivel == NivelCristales::Apagada {
        return 0.0;
    }
    let base = if trueno > 0.5 { 1.0 } else { 0.5 };
    (base * nivel.escala_arena()).min(1.0)
}

/// Observa el clima global (escarcha, repisa, arena, condensación y viento) según spec 27 v2.
#[allow(clippy::too_many_arguments)]
pub fn observar_clima(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    temp_efectiva: f32,
    bioma_seco: bool,
    arena_roja: bool,
    es_noche: bool,
    viento_xz: [f32; 2],
) {
    observar_clima_en(
        fuerza_lluvia,
        fuerza_trueno,
        llueve_en_bioma,
        temp_efectiva,
        bioma_seco,
        arena_roja,
        es_noche,
        viento_xz,
        Instant::now(),
    );
}

/// Variante determinista para pruebas unitarias.
#[allow(clippy::too_many_arguments)]
pub fn observar_clima_en(
    fuerza_lluvia: f32,
    fuerza_trueno: f32,
    llueve_en_bioma: bool,
    temp_efectiva: f32,
    bioma_seco: bool,
    arena_roja: bool,
    es_noche: bool,
    viento_xz: [f32; 2],
    ahora: Instant,
) {
    let Ok(mut lock) = ESTADO.lock() else { return };
    let cfg = config();
    let lluvia = fuerza_lluvia.clamp(0.0, 1.0);
    let trueno = fuerza_trueno.clamp(0.0, 1.0);
    let clima = intensidad_clima(lluvia, trueno);
    let precip_activa = clima > CLIMA_UMBRAL;
    let bioma_frio = temp_efectiva < TEMP_UMBRAL_NIEVE && !bioma_seco;
    let nevando = bioma_frio && precip_activa;
    let tormenta = precip_activa && trueno > 0.5;

    let obj_escarcha = objetivo_escarcha(bioma_frio, es_noche, nevando);
    let obj_repisa = if nevando { 1.0 } else { 0.0 };
    let obj_arena = objetivo_arena(bioma_seco, clima, trueno, cfg.arena);
    let obj_condensacion = if es_noche || tormenta { 1.0 } else { 0.0 };

    let viento_acotado = [viento_xz[0].clamp(-1.0, 1.0), viento_xz[1].clamp(-1.0, 1.0)];

    // Al entrar al mundo los valores saltan a su objetivo sin rampa (27 §Tiempo y estado).
    let Some(st) = lock.as_mut() else {
        *lock = Some(EstadoCristales {
            escarcha: obj_escarcha,
            repisa: obj_repisa,
            arena: obj_arena,
            condensacion: obj_condensacion,
            viento_xz: viento_acotado,
            arena_roja: bioma_seco && arena_roja,
            espectador: false,
            bioma_seco,
            bioma_frio,
            llueve_en_bioma,
            vaho_peso: 0.0,
            vaho_pos_mundo: [0.0; 3],
            tiene_vaho_pos: false,
            vaho_repeticiones: 1,
            vaho_desde_ultima_exh_s: 0.0,
            vaho_prox_exh_s: 0.0, // exhala al acercarse por primera vez
            vaho_exh_restante_s: 0.0,
            vaho_semilla: 0x2709_A511,
            ultimo: ahora,
            ultimo_aliento: ahora,
        });
        return;
    };

    let dt = ahora
        .saturating_duration_since(st.ultimo)
        .as_secs_f32()
        .clamp(0.0, MAX_DT_S);
    st.ultimo = ahora;

    st.escarcha = avanzar_rampa(st.escarcha, obj_escarcha, dt, ESCARCHA_SUBE_S, ESCARCHA_BAJA_S);
    st.repisa = avanzar_rampa(st.repisa, obj_repisa, dt, REPISA_SUBE_S, REPISA_BAJA_S);
    st.arena = avanzar_rampa(st.arena, obj_arena, dt, ARENA_SUBE_S, ARENA_BAJA_S);
    st.condensacion = avanzar_rampa(
        st.condensacion,
        obj_condensacion,
        dt,
        CONDENSACION_SUBE_S,
        CONDENSACION_BAJA_S,
    );
    st.viento_xz = viento_acotado;
    st.bioma_seco = bioma_seco;
    if bioma_seco {
        st.arena_roja = arena_roja;
    }
    st.bioma_frio = bioma_frio;
    st.llueve_en_bioma = llueve_en_bioma;
}

fn siguiente_intervalo_exhalacion(semilla: &mut u32) -> f32 {
    // LCG determinista simple para repartir el ritmo de respiración entre 2,8 y 3,8 s.
    *semilla = semilla.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
    let u = ((*semilla >> 8) & 0xFFFF) as f32 / 65_535.0;
    VAHO_EXHALACION_MIN_S + u * (VAHO_EXHALACION_MAX_S - VAHO_EXHALACION_MIN_S)
}

/// Actualiza el vaho del aliento (27 §Capa 6):
/// - `punto_mirado`: punto en el mundo sobre el cristal al que mira el jugador a <= 1,5 bloques;
/// - exhalaciones cada 2,8–3,8 s, de 0,5 s (+0,45 cada una); hasta 5 seguidas a < 0,3 bloques ensanchan
///   la elipse hasta ×1,7; se mantiene 35 s tras la última exhalación y se va en 20 s; apagado en espectador.
pub fn observar_aliento(punto_mirado: Option<[f64; 3]>, espectador: bool) {
    observar_aliento_en(punto_mirado, espectador, Instant::now());
}

/// Variante determinista para pruebas unitarias.
pub fn observar_aliento_en(punto_mirado: Option<[f64; 3]>, espectador: bool, ahora: Instant) {
    let Ok(mut lock) = ESTADO.lock() else { return };
    let Some(st) = lock.as_mut() else { return };

    let dt = ahora
        .saturating_duration_since(st.ultimo_aliento)
        .as_secs_f32()
        .clamp(0.0, MAX_DT_S);
    st.ultimo_aliento = ahora;

    let cfg = config();
    st.espectador = espectador;
    if espectador || !cfg.interior || cfg.aliento_pct == 0 {
        st.vaho_peso = 0.0;
        st.tiene_vaho_pos = false;
        st.vaho_repeticiones = 1;
        st.vaho_exh_restante_s = 0.0;
        st.vaho_prox_exh_s = 0.0;
        st.vaho_desde_ultima_exh_s = 0.0;
        return;
    }

    // Si estamos en medio de una exhalación (0,5 s), añade en total +0,45 de vaho.
    if st.vaho_exh_restante_s > 0.0 {
        let paso_exh = dt.min(st.vaho_exh_restante_s);
        st.vaho_exh_restante_s = (st.vaho_exh_restante_s - paso_exh).max(0.0);
        let aporte = VAHO_EXHALACION_APORTE * (paso_exh / VAHO_EXHALACION_DUR_S);
        st.vaho_peso = (st.vaho_peso + aporte).min(1.0);
        st.vaho_desde_ultima_exh_s = 0.0;
    }

    if let Some(pt) = punto_mirado {
        // Condición de la spec: solo exhala si la condensación global aún deja sitio (< 0,5) o hay frío.
        if st.vaho_exh_restante_s <= 0.0 {
            st.vaho_prox_exh_s = (st.vaho_prox_exh_s - dt).max(0.0);
            if st.vaho_prox_exh_s <= 0.0 {
                if st.tiene_vaho_pos && st.vaho_peso > 0.01 {
                    let dx = pt[0] - st.vaho_pos_mundo[0];
                    let dy = pt[1] - st.vaho_pos_mundo[1];
                    let dz = pt[2] - st.vaho_pos_mundo[2];
                    let dist = (dx * dx + dy * dy + dz * dz).sqrt();
                    if dist < VAHO_MISMO_SITIO_DIST {
                        st.vaho_repeticiones = (st.vaho_repeticiones + 1).min(VAHO_REPETICIONES_MAX);
                        st.vaho_pos_mundo = [
                            st.vaho_pos_mundo[0] * 0.5 + pt[0] * 0.5,
                            st.vaho_pos_mundo[1] * 0.5 + pt[1] * 0.5,
                            st.vaho_pos_mundo[2] * 0.5 + pt[2] * 0.5,
                        ];
                    } else {
                        st.vaho_repeticiones = 1;
                        st.vaho_pos_mundo = pt;
                        st.vaho_peso = 0.0;
                    }
                } else {
                    st.vaho_repeticiones = 1;
                    st.vaho_pos_mundo = pt;
                }
                st.tiene_vaho_pos = true;
                st.vaho_exh_restante_s = VAHO_EXHALACION_DUR_S;
                st.vaho_prox_exh_s = siguiente_intervalo_exhalacion(&mut st.vaho_semilla);
                st.vaho_desde_ultima_exh_s = 0.0;
            }
        }
    }

    // Mantiene 35 s tras la última exhalación y luego se va en 20 s (27 §Capa 6).
    if st.vaho_exh_restante_s <= 0.0 && st.vaho_peso > 0.0 {
        st.vaho_desde_ultima_exh_s += dt;
        if st.vaho_desde_ultima_exh_s > VAHO_MANTIENE_S {
            st.vaho_peso = (st.vaho_peso - dt / VAHO_DESVANECE_S).max(0.0);
            if st.vaho_peso <= 0.0 {
                st.tiene_vaho_pos = false;
                st.vaho_repeticiones = 1;
            }
        }
    }
}

/// Empaqueta tres valores normalizados `[0, 1]` a 8 bits cada uno en un `f32` exacto (24 bits).
#[inline]
pub fn empaquetar_triple8(a: f32, b: f32, c: f32) -> f32 {
    let qa = (a.clamp(0.0, 1.0) * 255.0).round() as u32;
    let qb = (b.clamp(0.0, 1.0) * 255.0).round() as u32;
    let qc = (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    (qa.min(255) + qb.min(255) * 256 + qc.min(255) * 65536) as f32
}

/// Empaqueta cuatro valores normalizados `[0, 1]` a 6 bits cada uno (0..63) en un `f32` exacto (24 bits).
#[inline]
pub fn empaquetar_cuarteto6(a: f32, b: f32, c: f32, d: f32) -> f32 {
    let qa = (a.clamp(0.0, 1.0) * 63.0).round() as u32;
    let qb = (b.clamp(0.0, 1.0) * 63.0).round() as u32;
    let qc = (c.clamp(0.0, 1.0) * 63.0).round() as u32;
    let qd = (d.clamp(0.0, 1.0) * 63.0).round() as u32;
    (qa.min(63) | (qb.min(63) << 6) | (qc.min(63) << 12) | (qd.min(63) << 18)) as f32
}

/// Devuelve los dos `vec4<f32>` (`mods[6]` y `mods[7]`) en formato v4 (`1004.0`), o `None` si no hay mundo observado.
pub fn shader_params(camara: Option<[f64; 3]>) -> Option<([f32; 4], [f32; 4])> {
    let st = (*ESTADO.lock().ok()?)?;
    let cfg = config();

    let m6_x = empaquetar_triple8(st.escarcha, st.repisa, st.condensacion);

    // La arena solo se ve mientras sea > 1/64 y en bioma seco (27 §Tiempo y estado).
    let arena_vis = if st.bioma_seco && st.arena > ARENA_VISIBLE_MIN {
        st.arena
    } else {
        0.0
    };
    let m6_y = empaquetar_triple8(
        arena_vis,
        st.viento_xz[0] * 0.5 + 0.5,
        st.viento_xz[1] * 0.5 + 0.5,
    );

    let rep = st.vaho_repeticiones.clamp(1, VAHO_REPETICIONES_MAX);
    let vaho_escala_frac = (rep - 1) as f32 / (VAHO_REPETICIONES_MAX - 1) as f32;
    let vaho_escala_q = ((vaho_escala_frac * 15.0).round() as u32).min(15);

    let mut flags24: u32 = cfg.lluvia.codigo()
        | (cfg.nieve.codigo() << 2)
        | (cfg.arena.codigo() << 4)
        | ((cfg.estilo_lluvia as u32) << 6)
        | ((cfg.resolucion as u32) << 7)
        | ((u32::from(cfg.seguir_viento)) << 9)
        | ((u32::from(cfg.interior)) << 10)
        | ((cfg.borde_helado as u32) << 11)
        | ((cfg.desenfoque as u32) << 13)
        | ((u32::from(st.arena_roja)) << 15)
        | ((u32::from(st.espectador)) << 16)
        | ((u32::from(st.bioma_seco)) << 17)
        | ((u32::from(st.bioma_frio)) << 18)
        | ((u32::from(st.llueve_en_bioma)) << 19)
        | (vaho_escala_q << 20);
    flags24 &= 0x00FF_FFFF;
    let m6_z = flags24 as f32;

    let m6_w = empaquetar_cuarteto6(
        f32::from(cfg.condensacion_pct) / 250.0,
        f32::from(cfg.blancura_condensacion_pct) / 100.0,
        f32::from(cfg.aliento_pct) / 250.0,
        f32::from(cfg.blancura_aliento_pct) / 100.0,
    );

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
        "v2 res={} lluvia={}({}) escarcha={:.2} repisa={:.2} cond={:.2} arena={:.2}{} vaho={:.2}(x{:.2}) borde={} blur={}",
        cfg.resolucion.texeles(),
        cfg.lluvia.as_str(),
        cfg.estilo_lluvia.as_str(),
        st.escarcha,
        st.repisa,
        st.condensacion,
        st.arena,
        if st.arena_roja { "(roja)" } else { "" },
        st.vaho_peso,
        1.0 + (VAHO_ESCALA_MAX - 1.0)
            * ((st.vaho_repeticiones.saturating_sub(1)) as f32 / (VAHO_REPETICIONES_MAX - 1) as f32),
        cfg.borde_helado.as_str(),
        cfg.desenfoque.as_str(),
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
    fn test_config_toml_roundtrip_v2() {
        let _g = test_lock();
        let texto = "\
[clima]
lluvia = false
[cristales]
lluvia = \"mucha\"
nieve = \"poca\"
arena = \"apagada\"
estilo_lluvia = \"regueros\"
resolucion = 64
seguir_viento = false
interior = true
condensacion = 180
blancura_condensacion = 40
borde_helado = \"cristales\"
desenfoque = \"fuerte\"
aliento = 200
blancura_aliento = 75
";
        let cfg = CristalesConfig::from_toml_str(texto);
        assert_eq!(cfg.lluvia, NivelCristales::Mucha);
        assert_eq!(cfg.nieve, NivelCristales::Poca);
        assert_eq!(cfg.arena, NivelCristales::Apagada);
        assert_eq!(cfg.estilo_lluvia, EstiloLluvia::Regueros);
        assert_eq!(cfg.resolucion, ResolucionCristales::R64);
        assert!(!cfg.seguir_viento);
        assert!(cfg.interior);
        assert_eq!(cfg.condensacion_pct, 180);
        assert_eq!(cfg.blancura_condensacion_pct, 40);
        assert_eq!(cfg.borde_helado, EstiloBordeHelado::Cristales);
        assert_eq!(cfg.desenfoque, NivelDesenfoque::Fuerte);
        assert_eq!(cfg.aliento_pct, 200);
        assert_eq!(cfg.blancura_aliento_pct, 75);
        let roundtrip = CristalesConfig::from_toml_str(&cfg.to_toml_section());
        assert_eq!(roundtrip, cfg);
    }

    #[test]
    fn test_rampas_v2_escarcha_repisa_condensacion_y_salto_inicial() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        let t0 = Instant::now();

        // Al entrar al mundo en bioma frío de día sin nevar: salta directo a 0,45 (sin rampa).
        observar_clima_en(0.0, 0.0, false, -0.3, false, false, false, [0.0, 0.0], t0);
        let (m6, _) = shader_params(None).unwrap();
        let p = m6[0] as u32;
        let esc0 = (p & 255) as f32 / 255.0;
        let rep0 = ((p >> 8) & 255) as f32 / 255.0;
        let cond0 = ((p >> 16) & 255) as f32 / 255.0;
        assert!((esc0 - 0.45).abs() < 0.02, "escarcha inicial día frío: {esc0}");
        assert!(rep0 < 0.01);
        assert!(cond0 < 0.01);

        // Empieza a nevar de noche -> sube escarcha (90 s), repisa (60 s) y condensación (90 s).
        let mut t = t0;
        for _ in 0..45 {
            t += Duration::from_secs(1);
            observar_clima_en(1.0, 0.0, false, -0.3, false, false, true, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let p = m6[0] as u32;
        let esc45 = (p & 255) as f32 / 255.0;
        let rep45 = ((p >> 8) & 255) as f32 / 255.0;
        let cond45 = ((p >> 16) & 255) as f32 / 255.0;
        assert!(esc45 > 0.9, "escarcha tras 45 s nevando desde 0,45: {esc45}");
        assert!((rep45 - 0.75).abs() < 0.03, "repisa tras 45 s / 60 s: {rep45}");
        assert!((cond45 - 0.50).abs() < 0.03, "condensación tras 45 s / 90 s: {cond45}");

        // Para de nevar de día: la repisa tarda 240 s en fundirse, condensación 60 s en bajar.
        for _ in 0..30 {
            t += Duration::from_secs(1);
            observar_clima_en(0.0, 0.0, false, -0.3, false, false, false, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let p = m6[0] as u32;
        let rep_tras = ((p >> 8) & 255) as f32 / 255.0;
        let cond_tras = ((p >> 16) & 255) as f32 / 255.0;
        assert!(rep_tras > 0.55, "repisa baja en 240 s: {rep_tras}");
        assert!(cond_tras < 0.05, "condensación baja en 60 s: {cond_tras}");
        limpiar();
    }

    #[test]
    fn test_arena_v2_tormenta_y_umbral_visible() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        let t0 = Instant::now();

        // Entra al desierto despejado -> arena 0
        observar_clima_en(0.0, 0.0, false, 2.0, true, false, false, [0.0, 0.0], t0);
        let mut t = t0;
        // Lluvia sin trueno: objetivo 0,5 en 60 s
        for _ in 0..60 {
            t += Duration::from_secs(1);
            observar_clima_en(1.0, 0.0, false, 2.0, true, false, false, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let arena_lluvia = ((m6[1] as u32) & 255) as f32 / 255.0;
        assert!((arena_lluvia - 0.5).abs() < 0.03, "arena sin trueno: {arena_lluvia}");

        // Tormenta con trueno > 0,5 en mesa roja: sube a 1,0
        for _ in 0..60 {
            t += Duration::from_secs(1);
            observar_clima_en(1.0, 1.0, false, 2.0, true, true, false, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let arena_tormenta = ((m6[1] as u32) & 255) as f32 / 255.0;
        assert!(arena_tormenta > 0.98, "arena con tormenta: {arena_tormenta}");
        assert_ne!((m6[2] as u32) & (1 << 15), 0, "bandera arena roja");

        // Despeja 90 s -> baja a ~0,5 (180 s total)
        for _ in 0..90 {
            t += Duration::from_secs(1);
            observar_clima_en(0.0, 0.0, false, 2.0, true, true, false, [0.0, 0.0], t);
        }
        let (m6, _) = shader_params(None).unwrap();
        let arena_mitad = ((m6[1] as u32) & 255) as f32 / 255.0;
        assert!((arena_mitad - 0.5).abs() < 0.03, "arena a 90 s de 180 s: {arena_mitad}");
        limpiar();
    }

    #[test]
    fn test_vaho_exhalaciones_35s_hold_y_20s_fade() {
        let _g = test_lock();
        limpiar();
        set_config(CristalesConfig::default());
        let t0 = Instant::now();
        observar_clima_en(0.0, 0.0, false, -0.4, false, false, false, [0.0, 0.0], t0);

        // Primera exhalación al mirar un cristal: 0,5 s añade 0,45
        let mut t = t0;
        observar_aliento_en(Some([10.0, 65.0, 20.0]), false, t);
        t += Duration::from_millis(500);
        observar_aliento_en(Some([10.0, 65.0, 20.0]), false, t);
        let (_, m7) = shader_params(Some([10.0, 65.0, 21.0])).unwrap();
        assert!((m7[3] - 0.45).abs() < 0.03, "primera exhalación +0,45: {}", m7[3]);

        // 4 exhalaciones más en el mismo sitio (< 0,3 bloques) -> escala crece hacia x1,7
        for _ in 0..16 {
            t += Duration::from_secs(1);
            observar_aliento_en(Some([10.05, 65.0, 20.0]), false, t);
        }
        let (m6_rep, m7_rep) = shader_params(Some([10.0, 65.0, 21.0])).unwrap();
        let escala_q = ((m6_rep[2] as u32) >> 20) & 15;
        assert!(m7_rep[3] > 0.85, "vaho acumulado tras varias exhalaciones: {}", m7_rep[3]);
        assert!(escala_q >= 10, "elipse ensanchada tras repeticiones: q={escala_q}");

        // Se aleja: durante 30 s sigue intacto (mantiene 35 s)
        let peso_inicial = m7_rep[3];
        for _ in 0..30 {
            t += Duration::from_secs(1);
            observar_aliento_en(None, false, t);
        }
        let (_, m7_hold) = shader_params(Some([10.0, 65.0, 24.0])).unwrap();
        assert!((m7_hold[3] - peso_inicial).abs() < 0.01, "mantiene 35 s: {}", m7_hold[3]);

        // 15 s más (total 45 s = 10 s de fade sobre 20 s) -> mitad
        for _ in 0..15 {
            t += Duration::from_secs(1);
            observar_aliento_en(None, false, t);
        }
        let (_, m7_fade) = shader_params(Some([10.0, 65.0, 24.0])).unwrap();
        assert!(m7_fade[3] > 0.25 && m7_fade[3] < peso_inicial - 0.2, "bajando en 20 s: {}", m7_fade[3]);

        // En espectador se apaga de inmediato
        t += Duration::from_secs(1);
        observar_aliento_en(Some([10.0, 65.0, 20.0]), true, t);
        let (m6_spec, m7_spec) = shader_params(Some([10.0, 65.0, 21.0])).unwrap();
        assert_eq!(m7_spec[3], 0.0);
        assert_ne!((m6_spec[2] as u32) & (1 << 16), 0);
        limpiar();
    }

    #[test]
    fn test_empaquetado_v4_exacto_f32() {
        let t = empaquetar_triple8(1.0, 0.5, 0.25);
        let tu = t as u32;
        assert_eq!(tu & 255, 255);
        assert_eq!((tu >> 8) & 255, 128);
        assert_eq!((tu >> 16) & 255, 64);

        let q = empaquetar_cuarteto6(1.0, 0.0, 0.6, 1.0);
        let qu = q as u32;
        assert_eq!(qu & 63, 63);
        assert_eq!((qu >> 6) & 63, 0);
        assert_eq!((qu >> 12) & 63, 38);
        assert_eq!((qu >> 18) & 63, 63);
    }
}
