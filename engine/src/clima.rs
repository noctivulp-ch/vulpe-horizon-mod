//! Clima con partículas: port de Particle Rain (pigcart, MIT; ver NOTICE). Etapa 1: lluvia y nieve, viento y la
//! bandera para que el pack no dibuje su propia lluvia. Lógica pura, sin el juego: lo que lee el mundo y crea las
//! partículas está en `client_tick.rs`. Porqué: docs/decisiones.md §7 y vulpe-horizon-spec/subsistemas/26.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Ajustes de la sección `[clima]` de `config/vulpe-horizon.toml` (valores por defecto de Particle Rain y del dueño).
#[derive(Clone, Debug, PartialEq)]
pub struct ClimaConfig {
    /// Lluvia como partículas; encendida, sustituye a la del pack (decisión del dueño, 2026-11-12).
    pub lluvia: bool,
    /// Nieve como partículas; igual que la lluvia.
    pub nieve: bool,
    /// Multiplicador de la densidad de todos los efectos.
    pub densidad: f32,
    /// Tope de partículas vivas (`perf.particleAmount`).
    pub tope: u32,
    /// Densidad con tormenta (`perf.particleStormDensity`).
    pub densidad_tormenta: f32,
    /// Radio de la esfera donde nacen, en bloques (`perf.particleDistance`).
    pub distancia: f32,
    /// Viento: fuerza, frecuencia de las ráfagas y velocidad de modulación (`WindOptions`).
    pub viento_fuerza: f32,
    pub viento_rafagas: f32,
    pub viento_modulacion: f32,
    /// Más viento con la altura (`yLevelAdjustment`).
    pub viento_altura: bool,
}

impl Default for ClimaConfig {
    fn default() -> Self {
        Self {
            lluvia: true,
            nieve: true,
            densidad: 1.0,
            tope: 1500,
            densidad_tormenta: 1.5,
            distancia: 16.0,
            viento_fuerza: 0.03,
            viento_rafagas: 0.15,
            viento_modulacion: 0.1,
            viento_altura: true,
        }
    }
}

impl ClimaConfig {
    /// Lee la sección `[clima]` de un TOML simple; lo que falte queda por defecto.
    pub fn from_toml_str(content: &str) -> Self {
        let mut c = Self::default();
        let mut en_clima = false;
        for linea in content.lines() {
            let l = linea.split('#').next().unwrap_or("").trim();
            if l.starts_with('[') && l.ends_with(']') {
                en_clima = l[1..l.len() - 1].trim() == "clima";
                continue;
            }
            if !en_clima {
                continue;
            }
            let Some((k, v)) = l.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let b = matches!(v, "true" | "1" | "1.0");
            let f = v.parse::<f32>().ok();
            match k {
                "lluvia" => c.lluvia = b,
                "nieve" => c.nieve = b,
                "densidad" => c.densidad = f.unwrap_or(c.densidad).clamp(0.0, 4.0),
                "tope" => c.tope = f.map(|x| x.clamp(0.0, 20000.0) as u32).unwrap_or(c.tope),
                "densidad_tormenta" => c.densidad_tormenta = f.unwrap_or(c.densidad_tormenta).clamp(0.0, 4.0),
                "distancia" => c.distancia = f.unwrap_or(c.distancia).clamp(4.0, 32.0),
                "viento_fuerza" => c.viento_fuerza = f.unwrap_or(c.viento_fuerza).clamp(0.0, 0.5),
                "viento_rafagas" => c.viento_rafagas = f.unwrap_or(c.viento_rafagas).clamp(0.0, 2.0),
                "viento_modulacion" => c.viento_modulacion = f.unwrap_or(c.viento_modulacion).clamp(0.0, 2.0),
                "viento_altura" => c.viento_altura = b,
                _ => {}
            }
        }
        c
    }

    /// La sección `[clima]` para añadirla al archivo de configuración.
    pub fn to_toml_section(&self) -> String {
        format!(
            "\n[clima]\n# Lluvia y nieve como partículas del mod (sustituyen a las del pack)\nlluvia = {}\nnieve = {}\n\
             # Densidad relativa (0 .. 4)\ndensidad = {:.2}\n# Tope de partículas vivas\ntope = {}\n\
             # Densidad con tormenta\ndensidad_tormenta = {:.2}\n# Radio donde nacen, bloques (4 .. 32)\ndistancia = {:.1}\n\
             # Viento: fuerza, ráfagas, modulación y más viento con la altura\nviento_fuerza = {:.3}\nviento_rafagas = {:.3}\n\
             viento_modulacion = {:.3}\nviento_altura = {}\n",
            self.lluvia, self.nieve, self.densidad, self.tope, self.densidad_tormenta, self.distancia,
            self.viento_fuerza, self.viento_rafagas, self.viento_modulacion, self.viento_altura
        )
    }
}

/// Los cuatro efectos de esta etapa, con los datos de `particles.json` de Particle Rain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gota {
    Lluvia,
    LluviaFuerte,
    Nieve,
    NieveFuerte,
}

/// Datos de un efecto (`ParticleData`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DatosGota {
    /// Nombre del juego de sprites del mod (`assets/vulpe/particles/<nombre>.json`).
    pub sprites: &'static str,
    pub densidad: f32,
    pub gravedad: f32,
    pub viento: f32,
    pub viento_tormenta: f32,
    pub tamano: f32,
    /// Solo con tormenta (`ONLY_DURING_STORMY_WEATHER`).
    pub solo_tormenta: bool,
    /// Nieve (precipitación `SNOW`) o lluvia (`RAIN`).
    pub nieve: bool,
    /// Tinte del agua (`tintType: WATER`).
    pub tinte_agua: bool,
    pub vida: i32,
}

impl Gota {
    pub const TODAS: [Gota; 4] = [Gota::Lluvia, Gota::LluviaFuerte, Gota::Nieve, Gota::NieveFuerte];

    pub fn datos(self) -> DatosGota {
        match self {
            Gota::Lluvia => DatosGota {
                sprites: "vulpe:lluvia", densidad: 1.0, gravedad: 0.9, viento: 0.4, viento_tormenta: 0.8,
                tamano: 0.75, solo_tormenta: false, nieve: false, tinte_agua: true, vida: 3000,
            },
            Gota::LluviaFuerte => DatosGota {
                sprites: "vulpe:lluvia_fuerte", densidad: 1.0, gravedad: 1.0, viento: 0.4, viento_tormenta: 0.8,
                tamano: 0.75, solo_tormenta: true, nieve: false, tinte_agua: true, vida: 3000,
            },
            Gota::Nieve => DatosGota {
                sprites: "vulpe:nieve", densidad: 0.3, gravedad: 0.05, viento: 0.01, viento_tormenta: 0.1,
                tamano: 0.75, solo_tormenta: false, nieve: true, tinte_agua: false, vida: 3000,
            },
            Gota::NieveFuerte => DatosGota {
                sprites: "vulpe:nieve_fuerte", densidad: 0.4, gravedad: 0.1, viento: 0.01, viento_tormenta: 0.1,
                tamano: 0.75, solo_tormenta: true, nieve: true, tinte_agua: false, vida: 3000,
            },
        }
    }

    /// Si el efecto está encendido en la configuración.
    pub fn activa(self, c: &ClimaConfig) -> bool {
        if self.datos().nieve { c.nieve } else { c.lluvia }
    }
}

/// Viento en un punto (`ParticleRain.getWind`): dos senos por eje que dan ráfagas, desplazados con el tiempo del
/// mundo, y nulo bajo Y 50 si se ajusta por altura (de 0 en Y 50 a 1 en Y 90).
pub fn viento(x: f64, y: f64, z: f64, tiempo_juego: f64, c: &ClimaConfig) -> [f32; 2] {
    let desfase = (tiempo_juego * f64::from(c.viento_modulacion)) as f32;
    let altura = if c.viento_altura { ((y as f32 - 50.0) / 40.0).clamp(0.0, 1.0) } else { 0.0 };
    let uno = |p: f32| -> f32 {
        let f = c.viento_rafagas;
        -((p * f + desfase).sin() * (p * f * 0.5 + desfase * 0.5).sin() * c.viento_fuerza + c.viento_fuerza)
    };
    [uno(x as f32) * altura, uno(z as f32) * altura]
}

/// Cuántas intenta crear por tick en el cielo (`getSkyParticleDensity`): tope/25, ×densidad de tormenta con el trueno,
/// × (1 + 2·velocidad de la cámara); 4 si lleva más de 100 ticks sin crear ninguna (inactivo).
pub fn densidad_cielo(c: &ClimaConfig, trueno: f32, velocidad: f32, inactivo: bool) -> u32 {
    if inactivo {
        return 4;
    }
    let base = (c.tope / 25) as f32;
    let tormenta = base + (base * c.densidad_tormenta - base) * trueno.clamp(0.0, 1.0);
    (tormenta * (1.0 + velocidad * 2.0) * c.densidad).max(0.0) as u32
}

/// Desplazamiento desde la cámara de un punto de nacimiento en la esfera de radio `r` (`tickSkyFX`). `u1..u3` son
/// números al azar en 0..1. Parado, se concentran por encima de la cámara (también nacen algunas por debajo, como en
/// el original); deprisa, reparto uniforme.
pub fn punto_nacimiento(u1: f32, u2: f32, u3: f32, rapido: bool, r: f32) -> [f32; 3] {
    let altura = if rapido { u1 } else { 1.0 - (u1 * u1 - u2 * u2).abs() };
    let theta = std::f32::consts::TAU * u3;
    let phi = (2.0 * altura - 1.0).clamp(-1.0, 1.0).acos();
    [r * phi.sin() * theta.cos(), r * phi.cos(), r * phi.sin() * theta.sin()]
}

/// Un tick del movimiento (`Particle.tick` de vanilla con el viento de `WindManager.applyWind`): el viento se suma a
/// la velocidad horizontal, la gravedad tira a 0,04 por tick y todo se frena un 2 %.
pub fn paso(vel: [f64; 3], gravedad: f32, viento: [f32; 2], mult_viento: f32) -> [f64; 3] {
    let mut v = vel;
    v[0] += f64::from(viento[0] * mult_viento);
    v[2] += f64::from(viento[1] * mult_viento);
    v[1] -= 0.04 * f64::from(gravedad);
    [v[0] * 0.98, v[1] * 0.98, v[2] * 0.98]
}

/// Velocidad inicial (`CustomParticle`): cae a `gravedad` bloques por tick con el viento de nacimiento (×10).
pub fn velocidad_inicial(gravedad: f32, viento: [f32; 2], mult_viento: f32) -> [f64; 3] {
    [f64::from(viento[0] * mult_viento * 10.0), -f64::from(gravedad), f64::from(viento[1] * mult_viento * 10.0)]
}

/// Opacidad por la distancia a la cámara (`FadeType.DISTANCE`): entera hasta 2/3 del radio, nula en el radio; por la
/// fuerza de la lluvia del momento (sube y baja con ella).
pub fn alfa(distancia: f32, radio: f32, lluvia: f32) -> f32 {
    let caida = radio / 3.0;
    let t = (((distancia + caida) / caida) - 3.0).clamp(0.0, 1.0);
    (1.0 - t) * lluvia.clamp(0.0, 1.0)
}

/// Tinte del agua sobre el sprite gris (`TintType.WATER` con `tintMix` 0,6): el color del agua por defecto de vanilla.
pub fn tinte_agua() -> [f32; 3] {
    let agua = [0x3F as f32 / 255.0, 0x76 as f32 / 255.0, 0xE4 as f32 / 255.0];
    let m = 0.6;
    [1.0 + (agua[0] - 1.0) * m, 1.0 + (agua[1] - 1.0) * m, 1.0 + (agua[2] - 1.0) * m]
}

// Estado compartido entre el tick del cliente y los controladores de las partículas (que no ven el juego).
static CAMARA: Mutex<[f64; 3]> = Mutex::new([0.0; 3]);
static TIEMPO: AtomicU64 = AtomicU64::new(0);
static TRUENO: AtomicBool = AtomicBool::new(false);
static PARTICULAS_ACTIVAS: AtomicBool = AtomicBool::new(false);
static LLUVIA: AtomicU64 = AtomicU64::new(0);
static TICKS_SIN_CREAR: AtomicU64 = AtomicU64::new(1000);
static VIVAS: AtomicU64 = AtomicU64::new(0);
static CONFIG: Mutex<Option<ClimaConfig>> = Mutex::new(None);

/// El tick del cliente apunta dónde está la cámara, el tiempo del mundo, la fuerza de la lluvia y si hay tormenta.
/// Devuelve lo que se movió la cámara desde el tick anterior (bloques por tick), para la densidad.
pub fn observar(camara: [f64; 3], tiempo_juego: u64, lluvia: f32, tormenta: bool) -> f32 {
    let mut velocidad = 0.0;
    if let Ok(mut c) = CAMARA.lock() {
        let d = [camara[0] - c[0], camara[1] - c[1], camara[2] - c[2]];
        velocidad = ((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt() as f32).min(10.0);
        *c = camara;
    }
    TIEMPO.store(tiempo_juego, Ordering::Relaxed);
    LLUVIA.store(u64::from(lluvia.clamp(0.0, 1.0).to_bits()), Ordering::Relaxed);
    TRUENO.store(tormenta, Ordering::Relaxed);
    velocidad
}

pub fn lluvia() -> f32 {
    f32::from_bits(LLUVIA.load(Ordering::Relaxed) as u32)
}

/// Partículas propias vivas (`ParticleSpawner.particleCount`): el tope se compara con estas, no con el total del motor.
pub fn vivas() -> u64 {
    VIVAS.load(Ordering::Relaxed)
}

pub fn nace() {
    VIVAS.fetch_add(1, Ordering::Relaxed);
}

pub fn muere() {
    let _ = VIVAS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some(v.saturating_sub(1)));
}

/// El motor también quita partículas por sus límites sin pasar por el controlador: la cuenta no puede pasar de su total.
pub fn recortar_vivas(total_motor: u64) {
    let _ = VIVAS.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |v| Some(v.min(total_motor)));
}

/// Si lleva más de 100 ticks sin crear ninguna (`ticksUntilSkyFXIdle`): entonces solo prueba 4 por tick.
pub fn inactivo() -> bool {
    TICKS_SIN_CREAR.load(Ordering::Relaxed) > 100
}

/// Cuenta un tick: `creadas` las que nacieron en él.
pub fn contar_tick(creadas: u32) {
    if creadas > 0 {
        TICKS_SIN_CREAR.store(0, Ordering::Relaxed);
    } else {
        TICKS_SIN_CREAR.fetch_add(1, Ordering::Relaxed);
    }
}

pub fn camara() -> [f64; 3] {
    CAMARA.lock().map(|c| *c).unwrap_or([0.0; 3])
}

pub fn tiempo() -> f64 {
    TIEMPO.load(Ordering::Relaxed) as f64
}

pub fn tormenta() -> bool {
    TRUENO.load(Ordering::Relaxed)
}

pub fn config() -> ClimaConfig {
    CONFIG.lock().ok().and_then(|c| c.clone()).unwrap_or_default()
}

pub fn set_config(c: ClimaConfig) {
    if let Ok(mut g) = CONFIG.lock() {
        *g = Some(c);
    }
}

/// Si la lluvia y la nieve del mod están encendidas: el pack lo lee por `frame.mods` (bit 7) y no dibuja las suyas.
/// Con una de las dos apagada, el mod no dibuja ninguna y el pack dibuja las dos (no se mezclan).
pub fn set_particulas_activas(v: bool) {
    PARTICULAS_ACTIVAS.store(v, Ordering::Relaxed);
}

pub fn particulas_activas() -> bool {
    PARTICULAS_ACTIVAS.load(Ordering::Relaxed)
}

/// Generador propio y barato (xorshift) para los números al azar del tick, sin asignaciones.
#[derive(Clone, Debug)]
pub struct Azar(u64);

impl Azar {
    pub fn new(semilla: u64) -> Self {
        Self(semilla | 1)
    }
    pub fn siguiente(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        ((x >> 40) as f32) / ((1u64 << 24) as f32)
    }
}

#[cfg(test)]
mod tests {
    //! Por qué se prueba: si el viento, la densidad o el movimiento se salen de los valores de Particle Rain, la lluvia
    //! cae recta o no cae, o llena la escena de partículas y hunde los fps del dueño sin que se note en el código.
    use super::*;

    #[test]
    fn viento_nulo_abajo_y_entero_arriba() {
        let c = ClimaConfig::default();
        assert_eq!(viento(10.0, 40.0, 10.0, 100.0, &c), [0.0, 0.0]);
        let v = viento(10.0, 120.0, 10.0, 100.0, &c);
        // −(sin·sin·f + f) está entre −2f y 0.
        assert!(v[0] <= 0.0 && v[0] >= -2.0 * c.viento_fuerza - 1e-6);
    }

    #[test]
    fn densidad_con_tormenta_y_velocidad() {
        let c = ClimaConfig::default();
        assert_eq!(densidad_cielo(&c, 0.0, 0.0, false), 60);
        assert_eq!(densidad_cielo(&c, 1.0, 0.0, false), 90);
        assert_eq!(densidad_cielo(&c, 0.0, 0.5, false), 120);
        assert_eq!(densidad_cielo(&c, 1.0, 0.0, true), 4);
    }

    #[test]
    fn nacen_en_la_esfera() {
        let mut a = Azar::new(7);
        let mut media_y = 0.0;
        for _ in 0..400 {
            let p = punto_nacimiento(a.siguiente(), a.siguiente(), a.siguiente(), false, 16.0);
            let r = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
            assert!((r - 16.0).abs() < 1e-3, "{r}");
            media_y += p[1] / 400.0;
        }
        // Parado se concentran por encima de la cámara (media de y claramente positiva).
        assert!(media_y > 2.0, "{media_y}");
    }

    #[test]
    fn la_lluvia_cae_y_se_frena() {
        let mut v = velocidad_inicial(0.9, [0.0, 0.0], 0.4);
        assert!((v[1] + 0.9).abs() < 1e-6);
        for _ in 0..1000 {
            v = paso(v, 0.9, [0.0, 0.0], 0.4);
        }
        // Velocidad límite: 0,04·0,9·0,98/0,02.
        assert!((v[1] + 1.764).abs() < 0.01, "{}", v[1]);
    }

    #[test]
    fn alfa_por_distancia_y_lluvia() {
        assert_eq!(alfa(5.0, 16.0, 1.0), 1.0);
        assert_eq!(alfa(16.0, 16.0, 1.0), 0.0);
        assert!((alfa(5.0, 16.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn config_lee_la_seccion_clima() {
        let c = ClimaConfig::from_toml_str("[cielo]\nlluvia = false\n[clima]\nlluvia = false\ntope = 800\n");
        assert!(!c.lluvia && c.nieve);
        assert_eq!(c.tope, 800);
        let ida = ClimaConfig::from_toml_str(&ClimaConfig::default().to_toml_section());
        assert_eq!(ida, ClimaConfig::default());
    }
}
