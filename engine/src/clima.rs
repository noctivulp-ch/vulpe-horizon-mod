//! Clima con partículas: port de Particle Rain (pigcart, MIT; ver NOTICE).
//! Lógica pura, sin el juego: lo que lee el mundo y crea las partículas está en `client_tick.rs`.
//! Porqué: docs/decisiones.md §7–§8 y vulpe-horizon-spec/subsistemas/26.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Tope real de distancia de nacimiento de `ParticleEngine::controlled` en Horizon (32 bloques).
pub const MAX_DISTANCIA_CONTROLADA: f32 = 31.5;

/// Ajustes de la sección `[clima]` de `config/vulpe-horizon.toml` (valores por defecto de Particle Rain y del dueño).
#[derive(Clone, Debug, PartialEq)]
pub struct ClimaConfig {
    /// Lluvia como partículas; encendida, sustituye a la del pack (decisión del dueño, 2026-11-12).
    pub lluvia: bool,
    /// Nieve como partículas; igual que la lluvia.
    pub nieve: bool,
    /// Polvo de arena en el aire (`dust`).
    pub polvo: bool,
    /// Granos de arena que saltan a ras del suelo (`skittering_dust`).
    pub polvo_raso: bool,
    /// Bruma de arena que pasa junto a la cámara (`dust_haze`).
    pub bruma_polvo: bool,
    /// Matorrales secos que ruedan por el desierto (`shrub`).
    pub matorral: bool,
    /// Multiplicador de la densidad de todos los efectos.
    pub densidad: f32,
    /// Tope de partículas vivas (`perf.particleAmount`).
    pub tope: u32,
    /// Densidad con tormenta (`perf.particleStormDensity`).
    pub densidad_tormenta: f32,
    /// Radio de la esfera donde nacen las de cielo, en bloques (`perf.particleDistance`).
    pub distancia: f32,
    /// Alcance de los efectos de superficie (`perf.surfaceRange`; Horizon acota el nacimiento a 32 bloques).
    pub alcance_superficie: f32,
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
            polvo: true,
            polvo_raso: true,
            bruma_polvo: true,
            matorral: true,
            densidad: 1.0,
            tope: 1500,
            densidad_tormenta: 1.5,
            distancia: 16.0,
            alcance_superficie: 32.0,
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
                "polvo" => c.polvo = b,
                "polvo_raso" => c.polvo_raso = b,
                "bruma_polvo" => c.bruma_polvo = b,
                "matorral" => c.matorral = b,
                "densidad" => c.densidad = f.unwrap_or(c.densidad).clamp(0.0, 4.0),
                "tope" => c.tope = f.map(|x| x.clamp(0.0, 20000.0) as u32).unwrap_or(c.tope),
                "densidad_tormenta" => c.densidad_tormenta = f.unwrap_or(c.densidad_tormenta).clamp(0.0, 4.0),
                "distancia" => c.distancia = f.unwrap_or(c.distancia).clamp(4.0, 32.0),
                "alcance_superficie" => c.alcance_superficie = f.unwrap_or(c.alcance_superficie).clamp(8.0, 100.0),
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
             # Polvo de arena y matorrales rodantes en desiertos y mesas con lluvia/tormenta\npolvo = {}\npolvo_raso = {}\nbruma_polvo = {}\nmatorral = {}\n\
             # Densidad relativa (0 .. 4)\ndensidad = {:.2}\n# Tope de partículas vivas\ntope = {}\n\
             # Densidad con tormenta\ndensidad_tormenta = {:.2}\n# Radio donde nacen, bloques (4 .. 32)\ndistancia = {:.1}\n\
             # Alcance de efectos de superficie, bloques (8 .. 100; Horizon acota a 32)\nalcance_superficie = {:.1}\n\
             # Viento: fuerza, ráfagas, modulación y más viento con la altura\nviento_fuerza = {:.3}\nviento_rafagas = {:.3}\n\
             viento_modulacion = {:.3}\nviento_altura = {}\n",
            self.lluvia, self.nieve, self.polvo, self.polvo_raso, self.bruma_polvo, self.matorral,
            self.densidad, self.tope, self.densidad_tormenta, self.distancia, self.alcance_superficie,
            self.viento_fuerza, self.viento_rafagas, self.viento_modulacion, self.viento_altura
        )
    }
}

/// Dónde nace un efecto (`ParticleData.SpawnPos`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origen {
    Cielo,
    Superficie,
    ArribaBloque,
}

/// Precipitación del bioma requerida (`Biome.Precipitation`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Precipitacion {
    Lluvia,
    Nieve,
    Ninguna,
}

/// Cómo cambia la opacidad de la partícula (`ParticleData.FadeType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Desvanecido {
    Distancia,
    EntradaSalida,
    Salida,
}

/// Cómo se tiñe la partícula (`ParticleData.TintType`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tinte {
    Ninguno,
    Agua,
    Mapa,
}

/// Efectos de partículas controladas con los datos de `particles.json` de Particle Rain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Efecto {
    Lluvia,
    LluviaFuerte,
    Nieve,
    NieveFuerte,
    Polvo,
    PolvoRaso,
    BrumaPolvo,
    Matorral,
}

/// Datos de un efecto (`ParticleData`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DatosEfecto {
    /// Nombre del juego de sprites del mod (`assets/vulpe/particles/<nombre>.json`).
    pub sprites: &'static str,
    pub num_sprites: usize,
    pub origen: Origen,
    pub precipitacion: Precipitacion,
    /// Solo con tormenta (`ONLY_DURING_STORMY_WEATHER`).
    pub solo_tormenta: bool,
    pub solo_overworld: bool,
    pub densidad: f32,
    pub gravedad: f32,
    pub viento: f32,
    pub viento_tormenta: f32,
    pub rebote: f32,
    pub tamano: f32,
    pub escala_distancia: f32,
    pub opacidad: f32,
    pub desvanecido: Desvanecido,
    pub tinte: Tinte,
    pub translucida: bool,
    pub vida: i32,
}

impl Efecto {
    pub const CIELO: [Efecto; 5] = [
        Efecto::Lluvia,
        Efecto::LluviaFuerte,
        Efecto::Nieve,
        Efecto::NieveFuerte,
        Efecto::Polvo,
    ];
    pub const SUPERFICIE: [Efecto; 2] = [Efecto::BrumaPolvo, Efecto::Matorral];
    pub const ARRIBA_BLOQUE: [Efecto; 1] = [Efecto::PolvoRaso];

    pub fn datos(self) -> DatosEfecto {
        match self {
            Efecto::Lluvia => DatosEfecto {
                sprites: "vulpe:lluvia", num_sprites: 16, origen: Origen::Cielo,
                precipitacion: Precipitacion::Lluvia, solo_tormenta: false, solo_overworld: false,
                densidad: 1.0, gravedad: 0.9, viento: 0.4, viento_tormenta: 0.8, rebote: 0.0,
                tamano: 0.75, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Agua, translucida: true, vida: 3000,
            },
            Efecto::LluviaFuerte => DatosEfecto {
                sprites: "vulpe:lluvia_fuerte", num_sprites: 4, origen: Origen::Cielo,
                precipitacion: Precipitacion::Lluvia, solo_tormenta: true, solo_overworld: false,
                densidad: 1.0, gravedad: 1.0, viento: 0.4, viento_tormenta: 0.8, rebote: 0.0,
                tamano: 0.75, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Agua, translucida: true, vida: 3000,
            },
            Efecto::Nieve => DatosEfecto {
                sprites: "vulpe:nieve", num_sprites: 16, origen: Origen::Cielo,
                precipitacion: Precipitacion::Nieve, solo_tormenta: false, solo_overworld: false,
                densidad: 0.3, gravedad: 0.05, viento: 0.01, viento_tormenta: 0.1, rebote: 0.0,
                tamano: 0.75, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Ninguno, translucida: true, vida: 3000,
            },
            Efecto::NieveFuerte => DatosEfecto {
                sprites: "vulpe:nieve_fuerte", num_sprites: 4, origen: Origen::Cielo,
                precipitacion: Precipitacion::Nieve, solo_tormenta: true, solo_overworld: false,
                densidad: 0.4, gravedad: 0.1, viento: 0.01, viento_tormenta: 0.1, rebote: 0.0,
                tamano: 0.75, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Ninguno, translucida: true, vida: 3000,
            },
            Efecto::Polvo => DatosEfecto {
                sprites: "vulpe:polvo", num_sprites: 3, origen: Origen::Cielo,
                precipitacion: Precipitacion::Ninguna, solo_tormenta: false, solo_overworld: true,
                densidad: 1.0, gravedad: 0.1, viento: 0.7, viento_tormenta: 1.0, rebote: 0.0,
                tamano: 0.25, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Mapa, translucida: true, vida: 3000,
            },
            Efecto::PolvoRaso => DatosEfecto {
                sprites: "vulpe:polvo", num_sprites: 3, origen: Origen::ArribaBloque,
                precipitacion: Precipitacion::Ninguna, solo_tormenta: false, solo_overworld: true,
                densidad: 0.2, gravedad: 0.5, viento: 0.8, viento_tormenta: 1.0, rebote: 0.15,
                tamano: 0.2, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Mapa, translucida: false, vida: 60,
            },
            Efecto::BrumaPolvo => DatosEfecto {
                sprites: "vulpe:bruma_polvo", num_sprites: 1, origen: Origen::Superficie,
                precipitacion: Precipitacion::Ninguna, solo_tormenta: false, solo_overworld: true,
                densidad: 1.0, gravedad: 0.1, viento: 0.5, viento_tormenta: 0.8, rebote: 0.15,
                tamano: 0.4, escala_distancia: 1.0, opacidad: 0.4, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Mapa, translucida: true, vida: 100,
            },
            Efecto::Matorral => DatosEfecto {
                sprites: "vulpe:matorral", num_sprites: 4, origen: Origen::Superficie,
                precipitacion: Precipitacion::Ninguna, solo_tormenta: false, solo_overworld: true,
                densidad: 0.01, gravedad: 0.5, viento: 0.4, viento_tormenta: 0.5, rebote: 0.15,
                tamano: 0.55, escala_distancia: 0.0, opacidad: 1.0, desvanecido: Desvanecido::Distancia,
                tinte: Tinte::Ninguno, translucida: false, vida: 3000,
            },
        }
    }

    /// Si el efecto está encendido en la configuración.
    pub fn activa(self, c: &ClimaConfig) -> bool {
        match self {
            Efecto::Lluvia | Efecto::LluviaFuerte => c.lluvia && c.nieve,
            Efecto::Nieve | Efecto::NieveFuerte => c.lluvia && c.nieve,
            Efecto::Polvo => c.polvo,
            Efecto::PolvoRaso => c.polvo_raso,
            Efecto::BrumaPolvo => c.bruma_polvo,
            Efecto::Matorral => c.matorral,
        }
    }
}

/// Si el nombre del bloque (con o sin prefijo `minecraft:`) pertenece a la lista arenosa de `dust` / `skittering_dust` /
/// `dust_haze` en `particles.json` (`#minecraft:sand`, `#minecraft:terracotta`, `#c:sandstone_blocks`, `coarse_dirt`).
pub fn es_bloque_arena(nombre: &str) -> bool {
    let n = nombre.rsplit(':').next().unwrap_or(nombre);
    matches!(n, "sand" | "red_sand" | "suspicious_sand" | "terracotta" | "coarse_dirt")
        || n.ends_with("_terracotta")
        || n.ends_with("sandstone")
}

/// Tinte tipo `MAP` de Particle Rain para bloques arenosos, con variación aleatoria 0,8..1,0 (`TintType.MAP`).
pub fn tinte_bloque_arena(nombre: &str, u: f32) -> [f32; 3] {
    let n = nombre.rsplit(':').next().unwrap_or(nombre);
    let base = match n {
        "red_sand" | "red_sandstone" | "chiseled_red_sandstone" | "cut_red_sandstone" | "smooth_red_sandstone" => {
            [0.85, 0.50, 0.20]
        }
        "coarse_dirt" => [0.59, 0.43, 0.30],
        "white_terracotta" => [0.82, 0.69, 0.63],
        "orange_terracotta" => [0.62, 0.33, 0.15],
        "yellow_terracotta" => [0.73, 0.52, 0.14],
        "red_terracotta" => [0.56, 0.24, 0.18],
        "brown_terracotta" => [0.30, 0.20, 0.14],
        "light_gray_terracotta" => [0.53, 0.42, 0.38],
        "terracotta" => [0.60, 0.36, 0.25],
        _ if n.ends_with("_terracotta") => [0.55, 0.35, 0.28],
        _ => [0.86, 0.81, 0.64], // sand / sandstone (MapColor.SAND = 0xDBD3A0)
    };
    let var = 0.8 + 0.2 * u.clamp(0.0, 1.0);
    [base[0] * var, base[1] * var, base[2] * var]
}

/// Si el bioma pertenece al Overworld (`#c:in_overworld`).
pub fn es_overworld(bioma: &str) -> bool {
    let n = bioma.rsplit(':').next().unwrap_or(bioma);
    !matches!(
        n,
        "nether_wastes"
            | "crimson_forest"
            | "warped_forest"
            | "soul_sand_valley"
            | "basalt_deltas"
            | "the_end"
            | "end_highlands"
            | "end_midlands"
            | "small_end_islands"
            | "end_barrens"
            | "the_void"
    )
}

/// Viento en un punto (`ParticleRain.getWind`): dos senos por eje que dan ráfagas, desplazados con el tiempo del
/// mundo, y nulo bajo Y 50 si se ajusta por altura (de 0 en Y 50 a 1 en Y 90).
pub fn viento(x: f64, y: f64, z: f64, tiempo_juego: f64, c: &ClimaConfig) -> [f32; 2] {
    let desfase = (tiempo_juego * f64::from(c.viento_modulacion)) as f32;
    let altura = if c.viento_altura { ((y as f32 - 50.0) / 40.0).clamp(0.0, 1.0) } else { 1.0 };
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

/// Cuántas intenta crear por tick en superficie (`tickSurfaceFX`): tope/200, o 1 si inactivo.
pub fn densidad_superficie(c: &ClimaConfig, trueno: f32, inactivo: bool) -> u32 {
    if inactivo {
        return 1;
    }
    let base = (c.tope / 200).max(1) as f32;
    let tormenta = base + (base * c.densidad_tormenta - base) * trueno.clamp(0.0, 1.0);
    (tormenta * c.densidad).max(1.0) as u32
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

/// Desplazamiento horizontal triangular (`RANDOM.triangle`) para `tickSurfaceFX`, acotado al límite de Horizon (31,5).
pub fn punto_superficie(u1: f32, u2: f32, u3: f32, u4: f32, alcance: f32) -> [f32; 2] {
    let r = alcance.min(MAX_DISTANCIA_CONTROLADA);
    let mut dx = (u1 - u2) * r;
    let mut dz = (u3 - u4) * r;
    let len = (dx * dx + dz * dz).sqrt();
    if len > MAX_DISTANCIA_CONTROLADA {
        let s = MAX_DISTANCIA_CONTROLADA / len;
        dx *= s;
        dz *= s;
    }
    [dx, dz]
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

/// Velocidad inicial (`CustomParticle`): cae a `gravedad` bloques por tick si nace en el cielo, o sube a `rebote` si
/// nace sobre un bloque/superficie, con el viento de nacimiento (×10).
pub fn velocidad_inicial_efecto(
    origen: Origen,
    gravedad: f32,
    rebote: f32,
    viento: [f32; 2],
    mult_viento: f32,
) -> [f64; 3] {
    let yd = if origen == Origen::Cielo { -f64::from(gravedad) } else { f64::from(rebote) };
    [f64::from(viento[0] * mult_viento * 10.0), yd, f64::from(viento[1] * mult_viento * 10.0)]
}

/// Velocidad inicial para lluvia/nieve (mantiene compatibilidad con pruebas previas).
pub fn velocidad_inicial(gravedad: f32, viento: [f32; 2], mult_viento: f32) -> [f64; 3] {
    velocidad_inicial_efecto(Origen::Cielo, gravedad, 0.0, viento, mult_viento)
}

/// Rebote contra una cara sólida (`CustomParticle.onCollision`): devuelve la nueva velocidad o `None` si excede los
/// 3 rebotes laterales (evita atascos en paredes).
pub fn rebotar(vel: [f64; 3], normal: [f64; 3], rebote: f32, rebotes_borde: &mut u8) -> Option<[f64; 3]> {
    if rebote <= 0.0 {
        return None;
    }
    let rapidez = (vel[0] * vel[0] + vel[1] * vel[1] + vel[2] * vel[2]).sqrt();
    let impulso = f64::from(rebote) * rapidez * 6.0;
    let mut nueva = vel;
    if normal[1] == 0.0 {
        if *rebotes_borde >= 3 {
            return None;
        }
        *rebotes_borde += 1;
        nueva[1] += 0.5 * impulso;
    }
    nueva[0] += normal[0] * impulso;
    nueva[1] += normal[1] * impulso;
    nueva[2] += normal[2] * impulso;
    Some(nueva)
}

/// Tamaño escalado por distancia (`CustomParticle.getDistanceSize`).
pub fn tamano_por_distancia(base: f32, distancia: f32, escala: f32) -> f32 {
    if escala <= 0.0 {
        return base;
    }
    base + base * distancia * escala
}

/// Opacidad por distancia, vida o salida (`CustomParticle.tickFading`), multiplicada por `opacidad` y `nivel_clima`.
pub fn alfa_efecto(
    desvanecido: Desvanecido,
    distancia: f32,
    radio: f32,
    edad: i32,
    vida: i32,
    opacidad: f32,
    nivel_clima: f32,
) -> f32 {
    let base = match desvanecido {
        Desvanecido::Distancia => {
            let caida = (radio / 3.0).max(0.1);
            let t = (((distancia + caida) / caida) - 3.0).clamp(0.0, 1.0);
            (1.0 - t) * opacidad
        }
        Desvanecido::EntradaSalida => {
            let mitad = (vida / 2).max(1) as f32;
            let e = edad.max(0) as f32;
            let f = if e < mitad { e / mitad } else { ((vida as f32) - e) / mitad };
            f.clamp(0.0, 1.0) * opacidad
        }
        Desvanecido::Salida => {
            let mitad = (vida / 2).max(1) as f32;
            (((vida - edad) as f32) / mitad).clamp(0.0, 1.0) * opacidad
        }
    };
    (base * nivel_clima.clamp(0.0, 1.0)).clamp(0.0, 1.0)
}

/// Opacidad por la distancia a la cámara (`FadeType.DISTANCE`, opacidad 1,0).
pub fn alfa(distancia: f32, radio: f32, lluvia: f32) -> f32 {
    alfa_efecto(Desvanecido::Distancia, distancia, radio, 0, 1, 1.0, lluvia)
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
static TICKS_SIN_SUPERFICIE: AtomicU64 = AtomicU64::new(1000);
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

/// Si lleva más de 100 ticks sin crear ninguna de cielo (`ticksUntilSkyFXIdle`): entonces solo prueba 4 por tick.
pub fn inactivo() -> bool {
    TICKS_SIN_CREAR.load(Ordering::Relaxed) > 100
}

/// Cuenta un tick de cielo: `creadas` las que nacieron en él.
pub fn contar_tick(creadas: u32) {
    if creadas > 0 {
        TICKS_SIN_CREAR.store(0, Ordering::Relaxed);
    } else {
        TICKS_SIN_CREAR.fetch_add(1, Ordering::Relaxed);
    }
}

/// Si lleva más de 100 ticks sin crear ninguna de superficie (`ticksUntilSurfaceFXIdle`).
pub fn inactivo_superficie() -> bool {
    TICKS_SIN_SUPERFICIE.load(Ordering::Relaxed) > 100
}

/// Cuenta un tick de superficie.
pub fn contar_superficie(creadas: u32) {
    if creadas > 0 {
        TICKS_SIN_SUPERFICIE.store(0, Ordering::Relaxed);
    } else {
        TICKS_SIN_SUPERFICIE.fetch_add(1, Ordering::Relaxed);
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
    //! Por qué se prueba: si el viento, la densidad, el rebote o las listas de bloques se salen de los valores de
    //! Particle Rain, el polvo no nace en el desierto o llena la escena de partículas y hunde los fps del dueño.
    use super::*;

    #[test]
    fn viento_nulo_abajo_y_entero_arriba() {
        let c = ClimaConfig::default();
        assert_eq!(viento(10.0, 40.0, 10.0, 100.0, &c), [0.0, 0.0]);
        let v = viento(10.0, 120.0, 10.0, 100.0, &c);
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
        assert!(media_y > 2.0, "{media_y}");
    }

    #[test]
    fn la_lluvia_cae_y_se_frena() {
        let mut v = velocidad_inicial(0.9, [0.0, 0.0], 0.4);
        assert!((v[1] + 0.9).abs() < 1e-6);
        for _ in 0..1000 {
            v = paso(v, 0.9, [0.0, 0.0], 0.4);
        }
        assert!((v[1] + 1.764).abs() < 0.01, "{}", v[1]);
    }

    #[test]
    fn alfa_por_distancia_y_lluvia() {
        assert_eq!(alfa(5.0, 16.0, 1.0), 1.0);
        assert_eq!(alfa(16.0, 16.0, 1.0), 0.0);
        assert!((alfa(5.0, 16.0, 0.5) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn bloques_y_tinte_de_arena() {
        assert!(es_bloque_arena("minecraft:sand"));
        assert!(es_bloque_arena("minecraft:red_sand"));
        assert!(es_bloque_arena("minecraft:smooth_sandstone"));
        assert!(es_bloque_arena("minecraft:orange_terracotta"));
        assert!(es_bloque_arena("minecraft:coarse_dirt"));
        assert!(!es_bloque_arena("minecraft:grass_block"));
        assert!(!es_bloque_arena("minecraft:stone"));
        let t_arena = tinte_bloque_arena("minecraft:sand", 1.0);
        let t_roja = tinte_bloque_arena("minecraft:red_sand", 1.0);
        assert!(t_arena[0] > 0.8 && t_arena[2] > 0.5);
        assert!(t_roja[0] > t_roja[2] * 2.0);
        assert!(es_overworld("minecraft:desert"));
        assert!(!es_overworld("minecraft:nether_wastes"));
        assert!(!es_overworld("minecraft:the_end"));
    }

    #[test]
    fn rebote_en_suelo_y_pared_con_limite() {
        let mut bordes = 0u8;
        let v = rebotar([-0.2, -0.3, 0.0], [0.0, 1.0, 0.0], 0.15, &mut bordes).unwrap();
        assert!(v[1] > -0.3);
        assert_eq!(bordes, 0);
        // Tres rebotes laterales pasan; el cuarto mata la partícula.
        for _ in 0..3 {
            let vb = rebotar([-0.2, 0.0, 0.0], [1.0, 0.0, 0.0], 0.15, &mut bordes).unwrap();
            assert!(vb[1] > 0.0);
        }
        assert!(rebotar([-0.2, 0.0, 0.0], [1.0, 0.0, 0.0], 0.15, &mut bordes).is_none());
    }

    #[test]
    fn punto_superficie_respeta_limite_horizon() {
        let mut a = Azar::new(42);
        for _ in 0..200 {
            let p = punto_superficie(a.siguiente(), a.siguiente(), a.siguiente(), a.siguiente(), 100.0);
            let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
            assert!(r <= MAX_DISTANCIA_CONTROLADA + 1e-4, "{r}");
        }
    }

    #[test]
    fn config_lee_la_seccion_clima() {
        let c = ClimaConfig::from_toml_str(
            "[cielo]\nlluvia = false\n[clima]\nlluvia = false\npolvo = false\ntope = 800\n",
        );
        assert!(!c.lluvia && c.nieve && !c.polvo && c.polvo_raso && c.bruma_polvo);
        assert_eq!(c.tope, 800);
        let ida = ClimaConfig::from_toml_str(&ClimaConfig::default().to_toml_section());
        assert_eq!(ida, ClimaConfig::default());
    }
}
