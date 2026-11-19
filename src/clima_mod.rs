// Clima con partículas sobre la API pública de Horizon (`mod_api::Hooks` + `ParticleProvider`).
// Port de Particle Rain (pigcart, MIT; ver NOTICE). Porqué: docs/decisiones.md §7–§9.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex, OnceLock};

use mod_api::{
    BiomeProperties, ClientHost, Heightmap, LevelRef, ModParticle, ParticleKey, ParticleOptions,
    ParticleProvider, ParticleProviders, Registrar,
};
use vulpe_engine::clima as cl;

/// Claves de partículas registradas en `Hooks::register`.
pub struct ClimaKeys {
    lluvia: ParticleKey,
    lluvia_fuerte: ParticleKey,
    nieve: ParticleKey,
    nieve_fuerte: ParticleKey,
    polvo: ParticleKey,
    bruma_polvo: ParticleKey,
    matorral: ParticleKey,
    reguero: ParticleKey,
    niebla: ParticleKey,
    vanilla_rain: ParticleKey,
    vanilla_smoke: ParticleKey,
}

static KEYS: OnceLock<ClimaKeys> = OnceLock::new();

pub fn registrar_claves(registrar: &mut dyn Registrar) {
    let keys = ClimaKeys {
        lluvia: registrar.particle_key("vulpe:lluvia"),
        lluvia_fuerte: registrar.particle_key("vulpe:lluvia_fuerte"),
        nieve: registrar.particle_key("vulpe:nieve"),
        nieve_fuerte: registrar.particle_key("vulpe:nieve_fuerte"),
        polvo: registrar.particle_key("vulpe:polvo"),
        bruma_polvo: registrar.particle_key("vulpe:bruma_polvo"),
        matorral: registrar.particle_key("vulpe:matorral"),
        reguero: registrar.particle_key("vulpe:reguero"),
        niebla: registrar.particle_key("vulpe:niebla"),
        vanilla_rain: registrar.particle_key("minecraft:rain"),
        vanilla_smoke: registrar.particle_key("minecraft:smoke"),
    };
    let _ = KEYS.set(keys);
}

fn clave_efecto(keys: &ClimaKeys, efecto: cl::Efecto) -> ParticleKey {
    match efecto {
        cl::Efecto::Lluvia => keys.lluvia,
        cl::Efecto::LluviaFuerte => keys.lluvia_fuerte,
        cl::Efecto::Nieve => keys.nieve,
        cl::Efecto::NieveFuerte => keys.nieve_fuerte,
        cl::Efecto::Polvo | cl::Efecto::PolvoRaso => keys.polvo,
        cl::Efecto::BrumaPolvo => keys.bruma_polvo,
        cl::Efecto::Matorral => keys.matorral,
        cl::Efecto::NieblaVertical | cl::Efecto::NieblaVerticalDespues => keys.niebla,
    }
}

#[derive(Clone, Copy)]
struct SpawnInfo {
    pos: [f64; 3],
    efecto: cl::Efecto,
    tamano_base: f32,
    distancia_max: f32,
    vida: i32,
    rgba: [f32; 4],
    frame: u32,
    tope_y: f64,
}

#[derive(Clone, Copy)]
struct RegueroSpawnInfo {
    pos: [f64; 3],
    rgba: [f32; 4],
    tope_suelo: f64,
    semilla: u64,
}

static COLA_PARTICULAS: Mutex<VecDeque<SpawnInfo>> = Mutex::new(VecDeque::new());
static COLA_REGUEROS: Mutex<VecDeque<RegueroSpawnInfo>> = Mutex::new(VecDeque::new());

fn encolar_particula(info: SpawnInfo) {
    if let Ok(mut q) = COLA_PARTICULAS.lock() {
        if q.len() > 4096 {
            q.pop_front();
        }
        q.push_back(info);
    }
}

fn desencolar_particula(pos: [f64; 3], defecto: cl::Efecto) -> SpawnInfo {
    if let Ok(mut q) = COLA_PARTICULAS.lock() {
        if let Some(idx) = q.iter().position(|s| {
            (s.pos[0] - pos[0]).abs() < 0.05
                && (s.pos[1] - pos[1]).abs() < 0.05
                && (s.pos[2] - pos[2]).abs() < 0.05
        }) {
            if let Some(found) = q.remove(idx) {
                return found;
            }
        }
        if let Some(front) = q.pop_front() {
            return front;
        }
    }
    let d = defecto.datos();
    SpawnInfo {
        pos,
        efecto: defecto,
        tamano_base: d.tamano,
        distancia_max: 16.0,
        vida: d.vida,
        rgba: [1.0, 1.0, 1.0, 0.0],
        frame: 0,
        tope_y: pos[1] - 32.0,
    }
}

fn encolar_reguero(info: RegueroSpawnInfo) {
    if let Ok(mut q) = COLA_REGUEROS.lock() {
        if q.len() > 1024 {
            q.pop_front();
        }
        q.push_back(info);
    }
}

fn desencolar_reguero(pos: [f64; 3]) -> RegueroSpawnInfo {
    if let Ok(mut q) = COLA_REGUEROS.lock() {
        if let Some(idx) = q.iter().position(|s| {
            (s.pos[0] - pos[0]).abs() < 0.05
                && (s.pos[1] - pos[1]).abs() < 0.05
                && (s.pos[2] - pos[2]).abs() < 0.05
        }) {
            if let Some(found) = q.remove(idx) {
                return found;
            }
        }
        if let Some(front) = q.pop_front() {
            return front;
        }
    }
    RegueroSpawnInfo {
        pos,
        rgba: [0.3, 0.5, 0.9, 0.8],
        tope_suelo: pos[1] - 4.0,
        semilla: 1,
    }
}

fn efecto_a_codigo(efecto: cl::Efecto) -> u32 {
    match efecto {
        cl::Efecto::Lluvia => 0,
        cl::Efecto::LluviaFuerte => 1,
        cl::Efecto::Nieve => 2,
        cl::Efecto::NieveFuerte => 3,
        cl::Efecto::Polvo => 4,
        cl::Efecto::PolvoRaso => 5,
        cl::Efecto::BrumaPolvo => 6,
        cl::Efecto::Matorral => 7,
        cl::Efecto::NieblaVertical => 8,
        cl::Efecto::NieblaVerticalDespues => 9,
    }
}

fn codigo_a_efecto(c: u32) -> cl::Efecto {
    match c {
        0 => cl::Efecto::Lluvia,
        1 => cl::Efecto::LluviaFuerte,
        2 => cl::Efecto::Nieve,
        3 => cl::Efecto::NieveFuerte,
        4 => cl::Efecto::Polvo,
        5 => cl::Efecto::PolvoRaso,
        6 => cl::Efecto::BrumaPolvo,
        7 => cl::Efecto::Matorral,
        8 => cl::Efecto::NieblaVertical,
        _ => cl::Efecto::NieblaVerticalDespues,
    }
}

struct VulpeParticulaProvider {
    defecto: cl::Efecto,
}

impl ParticleProvider for VulpeParticulaProvider {
    fn tick(&self, p: &mut ModParticle) {
        // state[0] = tope_y (altura de suelo para choque/rebote)
        // state[1] = distancia_max
        // state[2] = tamano_base
        // state[3] = empaquetado: bits bajos = codigo_efecto (u8) + rebotes*16, parte fraccional = giro
        if p.age == 1 {
            let info = desencolar_particula(p.position, self.defecto);
            p.lifetime = info.vida;
            p.size = info.tamano_base;
            p.rgba = info.rgba;
            p.frame = info.frame;
            p.state[0] = info.tope_y;
            p.state[1] = f64::from(info.distancia_max);
            p.state[2] = f64::from(info.tamano_base);
            p.state[3] = f64::from(efecto_a_codigo(info.efecto));
        }

        if p.age >= p.lifetime {
            cl::muere();
            p.removed = true;
            return;
        }

        let tope_y = p.state[0];
        let distancia_max = p.state[1] as f32;
        let tamano_base = p.state[2] as f32;
        let packed = p.state[3];
        let codigo = (packed.floor() as u32) & 0xF;
        let mut rebotes = ((packed.floor() as u32) >> 4) as u8;
        let mut giro = (packed - packed.floor()) as f32;
        let efecto = codigo_a_efecto(codigo);
        let d = efecto.datos();
        let cfg = cl::config();

        let viento = cl::viento(p.position[0], p.position[1], p.position[2], cl::tiempo(), &cfg);
        let mult = if cl::tormenta() {
            d.viento_tormenta
        } else {
            d.viento
        };
        p.velocity = cl::paso(p.velocity, d.gravedad, viento, mult);

        let sig = [
            p.position[0] + p.velocity[0],
            p.position[1] + p.velocity[1],
            p.position[2] + p.velocity[2],
        ];
        if sig[1] <= tope_y {
            if d.rebote > 0.0 {
                if let Some(nueva_vel) = cl::rebotar(p.velocity, [0.0, 1.0, 0.0], d.rebote, &mut rebotes) {
                    p.velocity = nueva_vel;
                    p.position = [sig[0], tope_y + 0.05, sig[2]];
                } else {
                    cl::muere();
                    p.removed = true;
                    return;
                }
            } else {
                cl::muere();
                p.removed = true;
                return;
            }
        } else {
            p.position = sig;
        }

        let cam = cl::camara();
        let dist = ((p.position[0] - cam[0]).powi(2)
            + (p.position[1] - cam[1]).powi(2)
            + (p.position[2] - cam[2]).powi(2))
        .sqrt() as f32;
        if dist > distancia_max + 0.5 {
            cl::muere();
            p.removed = true;
            return;
        }
        if d.escala_distancia > 0.0 {
            p.size = if matches!(
                efecto,
                cl::Efecto::NieblaVertical | cl::Efecto::NieblaVerticalDespues
            ) {
                cl::tamano_niebla(tamano_base, dist, d.escala_distancia)
            } else {
                cl::tamano_por_distancia(tamano_base, dist, d.escala_distancia)
            };
        }
        let nivel_clima = match efecto {
            cl::Efecto::NieblaVertical | cl::Efecto::NieblaVerticalDespues => 1.0,
            _ => cl::lluvia(),
        };
        let mut alfa = cl::alfa_efecto(
            d.desvanecido,
            dist,
            distancia_max,
            p.age,
            p.lifetime,
            d.opacidad,
            nivel_clima,
        );
        if matches!(
            efecto,
            cl::Efecto::NieblaVertical | cl::Efecto::NieblaVerticalDespues
        ) {
            alfa = alfa.min(cl::atenuacion_vertical(p.position, cam))
                * cl::atenuacion_cercania_niebla(dist);
        }
        p.rgba[3] = alfa;

        if efecto == cl::Efecto::Matorral {
            let rapidez = (p.velocity[0] * p.velocity[0]
                + p.velocity[1] * p.velocity[1]
                + p.velocity[2] * p.velocity[2])
            .sqrt() as f32;
            giro = (giro + rapidez * 0.25).fract();
            p.frame = ((giro * d.num_sprites as f32) as u32) % (d.num_sprites as u32);
        }
        p.state[3] = f64::from(codigo | (u32::from(rebotes) << 4)) + f64::from(giro.clamp(0.0, 0.999));
    }
}

struct VulpeRegueroProvider;

impl ParticleProvider for VulpeRegueroProvider {
    fn tick(&self, p: &mut ModParticle) {
        // state[0] = tope_suelo, state[1] = gravedad, state[2] = desvaneciendo (0/1), state[3] = semilla
        if p.age == 1 {
            let info = desencolar_reguero(p.position);
            p.lifetime = 200;
            p.size = 0.14;
            p.rgba = info.rgba;
            p.state[0] = info.tope_suelo;
            p.state[1] = 0.0;
            p.state[2] = 0.0;
            p.state[3] = info.semilla as f64;
        }
        if p.age >= p.lifetime {
            cl::muere();
            p.removed = true;
            return;
        }
        let mut gravedad = p.state[1] as f32;
        let mut desvaneciendo = p.state[2] > 0.5;
        let mut azar = cl::Azar::new((p.state[3] as u64).wrapping_add(p.age as u64 * 17));
        if azar.siguiente() < 0.04 {
            gravedad = if gravedad == 0.0 { 0.05 } else { 0.0 };
        }
        p.velocity[1] -= 0.04 * f64::from(gravedad);
        p.velocity[1] *= 0.92;
        let sig_y = p.position[1] + p.velocity[1];
        if sig_y <= p.state[0] {
            desvaneciendo = true;
            p.velocity[1] = 0.0;
        } else {
            p.position[1] = sig_y;
        }
        if desvaneciendo {
            p.rgba[3] -= 0.08;
            if p.rgba[3] <= 0.02 {
                cl::muere();
                p.removed = true;
                return;
            }
        }
        p.state[1] = f64::from(gravedad);
        p.state[2] = if desvaneciendo { 1.0 } else { 0.0 };
        p.state[3] = (azar.siguiente() * 1_000_000.0) as f64;
    }
}

pub fn registrar_proveedores(providers: &mut dyn ParticleProviders) {
    let pares: [(&str, cl::Efecto); 8] = [
        ("vulpe:lluvia", cl::Efecto::Lluvia),
        ("vulpe:lluvia_fuerte", cl::Efecto::LluviaFuerte),
        ("vulpe:nieve", cl::Efecto::Nieve),
        ("vulpe:nieve_fuerte", cl::Efecto::NieveFuerte),
        ("vulpe:polvo", cl::Efecto::Polvo),
        ("vulpe:bruma_polvo", cl::Efecto::BrumaPolvo),
        ("vulpe:matorral", cl::Efecto::Matorral),
        ("vulpe:niebla", cl::Efecto::NieblaVertical),
    ];
    for (nombre, defecto) in pares {
        providers.register(
            nombre.to_string(),
            Arc::new(VulpeParticulaProvider { defecto }),
        );
    }
    providers.register("vulpe:reguero".to_string(), Arc::new(VulpeRegueroProvider));
}

fn precipitacion_en(props: &BiomeProperties, y: i32) -> cl::Precipitacion {
    if !props.has_precipitation {
        return cl::Precipitacion::Ninguna;
    }
    let temp = if y > 80 {
        props.temperature - (y - 80) as f32 * 0.00125
    } else {
        props.temperature
    };
    if temp < 0.15 {
        cl::Precipitacion::Nieve
    } else {
        cl::Precipitacion::Lluvia
    }
}

fn propiedades_bioma(
    host: &dyn ClientHost,
    level: LevelRef,
    bioma: &str,
    cache: &mut HashMap<String, BiomeProperties>,
) -> Option<BiomeProperties> {
    if let Some(p) = cache.get(bioma) {
        return Some(p.clone());
    }
    let p = level.biome_properties(host, bioma)?;
    cache.insert(bioma.to_string(), p.clone());
    Some(p)
}

fn nombre_bloque(host: &dyn ClientHost, level: LevelRef, pos: [i32; 3]) -> String {
    level
        .block(host, pos)
        .map(|k| k.id(host))
        .unwrap_or_default()
}

pub fn tick_clima(host: &mut dyn ClientHost) {
    let Some(keys) = KEYS.get() else {
        return;
    };
    let cfg = cl::config();
    let Some(reloj) = host.clock() else {
        cl::set_particulas_activas(false);
        return;
    };
    let Some(cam) = host.camera() else {
        return;
    };
    let Some(level) = host.level(host.dimension()) else {
        return;
    };
    let lluvia_nieve_activas = cfg.lluvia && cfg.nieve;
    cl::set_particulas_activas(lluvia_nieve_activas);
    let hay_lluvia_fx = lluvia_nieve_activas
        || cfg.polvo
        || cfg.polvo_raso
        || cfg.bruma_polvo
        || cfg.matorral
        || cfg.salpicadura
        || cfg.humo
        || cfg.reguero;
    let hay_niebla_fx = cfg.niebla_vertical || cfg.niebla_vertical_despues;

    let camara = cam.position;
    let (lluvia, trueno) = level.weather(host).unwrap_or((0.0, 0.0));
    let tormenta = trueno > 0.5;
    let velocidad = cl::observar(camara, reloj.game_time.max(0) as u64, lluvia, tormenta);
    let tras_lluvia = cl::ticks_despues_lluvia() > 0;
    if reloj.game_time % 100 == 0 {
        cl::recortar_vivas(u64::from(host.particle_count()));
    }
    let puede_niebla = cfg.niebla_vertical || (cfg.niebla_vertical_despues && tras_lluvia);
    if (!hay_lluvia_fx && !hay_niebla_fx)
        || (lluvia <= 0.0 && !puede_niebla)
        || cl::vivas() >= u64::from(cfg.tope)
    {
        cl::contar_tick(0);
        cl::contar_superficie(0);
        return;
    }

    let mut azar = cl::Azar::new(reloj.game_time as u64 ^ 0x9E37_79B9_7F4A_7C15);
    let mut cache_biomas: HashMap<String, BiomeProperties> = HashMap::new();

    // 1) Efectos de cielo (`tickSkyFX`): lluvia, nieve y polvo en el aire (`dust`).
    let mut creadas_cielo = 0u32;
    if lluvia > 0.0 && (lluvia_nieve_activas || cfg.polvo) {
        let n_cielo = cl::densidad_cielo(&cfg, trueno, velocidad, cl::inactivo()) as usize;
        let mut puntos = Vec::with_capacity(n_cielo);
        let mut cols = Vec::with_capacity(n_cielo);
        for _ in 0..n_cielo {
            let o = cl::punto_nacimiento(
                azar.siguiente(),
                azar.siguiente(),
                azar.siguiente(),
                velocidad >= 0.8,
                cfg.distancia,
            );
            let pos = [
                camara[0] + f64::from(o[0]),
                camara[1] + f64::from(o[1]),
                camara[2] + f64::from(o[2]),
            ];
            let (x, y, z) = (
                pos[0].floor() as i32,
                pos[1].floor() as i32,
                pos[2].floor() as i32,
            );
            puntos.push((pos, x, y, z));
            cols.push((x, z));
        }
        let alturas = host
            .client_surface_heights(&cols, Heightmap::MotionBlocking)
            .unwrap_or_default();
        for (idx, (pos, x, y, z)) in puntos.into_iter().enumerate() {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
            let Some(Some(tope_y)) = alturas.get(idx).copied() else {
                continue;
            };
            if tope_y >= y {
                continue;
            }
            let Some(bioma) = level.client_biome(host, [x, y, z]) else {
                continue;
            };
            let Some(props) = propiedades_bioma(host, level, &bioma, &mut cache_biomas) else {
                continue;
            };
            let precip = precipitacion_en(&props, y);
            let probabilidad = azar.siguiente();
            let bloque_suelo = if precip == cl::Precipitacion::Ninguna {
                Some(nombre_bloque(host, level, [x, tope_y - 1, z]))
            } else {
                None
            };
            for efecto in cl::Efecto::CIELO {
                if !efecto.activa(&cfg) {
                    continue;
                }
                let d = efecto.datos();
                if d.precipitacion != precip
                    || (d.solo_tormenta && !tormenta)
                    || d.densidad * cfg.densidad <= probabilidad
                {
                    continue;
                }
                if d.solo_overworld && !cl::es_overworld(&bioma) {
                    continue;
                }
                if let Some(ref nombre) = bloque_suelo {
                    if !cl::es_bloque_arena(nombre) {
                        continue;
                    }
                }
                let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                let vel = cl::velocidad_inicial_efecto(
                    d.origen,
                    d.gravedad,
                    d.rebote,
                    viento,
                    if tormenta { d.viento_tormenta } else { d.viento },
                );
                let tinte = match d.tinte {
                    cl::Tinte::Agua => cl::tinte_agua(),
                    cl::Tinte::Mapa => cl::tinte_bloque_arena(
                        bloque_suelo.as_deref().unwrap_or("sand"),
                        azar.siguiente(),
                    ),
                    cl::Tinte::Niebla => cl::tinte_niebla(None, lluvia),
                    cl::Tinte::Ninguno => [1.0, 1.0, 1.0],
                };
                let frame = if d.num_sprites > 1 {
                    (azar.siguiente() * d.num_sprites as f32) as u32
                } else {
                    0
                };
                encolar_particula(SpawnInfo {
                    pos,
                    efecto,
                    tamano_base: d.tamano,
                    distancia_max: cfg.distancia,
                    vida: d.vida as i32,
                    rgba: [tinte[0], tinte[1], tinte[2], 0.0],
                    frame,
                    tope_y: f64::from(tope_y),
                });
                let opts = ParticleOptions::simple(clave_efecto(keys, efecto));
                if host.add_particle(&opts, pos, vel) {
                    cl::nace();
                    creadas_cielo += 1;
                }
            }
        }
    }
    cl::contar_tick(creadas_cielo);

    // 2) Efectos de superficie (`tickSurfaceFX`): bruma de polvo, matorrales y nieblas verticales.
    let mut creadas_sup = 0u32;
    if ((lluvia > 0.0 && (cfg.bruma_polvo || cfg.matorral)) || puede_niebla)
        && cl::vivas() < u64::from(cfg.tope)
    {
        let n_sup = cl::densidad_superficie(&cfg, trueno, cl::inactivo_superficie()) as usize;
        let alcance = cfg.alcance_superficie.min(cl::MAX_DISTANCIA_CONTROLADA);
        let mut puntos = Vec::with_capacity(n_sup);
        let mut cols = Vec::with_capacity(n_sup);
        for _ in 0..n_sup {
            let off = cl::punto_superficie(
                azar.siguiente(),
                azar.siguiente(),
                azar.siguiente(),
                azar.siguiente(),
                alcance,
            );
            let px = camara[0] + f64::from(off[0]);
            let pz = camara[2] + f64::from(off[1]);
            let (x, z) = (px.floor() as i32, pz.floor() as i32);
            puntos.push((px, pz, x, z));
            cols.push((x, z));
        }
        let alturas = host
            .client_surface_heights(&cols, Heightmap::MotionBlocking)
            .unwrap_or_default();
        for (idx, (px, pz, x, z)) in puntos.into_iter().enumerate() {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
            let Some(Some(tope_y)) = alturas.get(idx).copied() else {
                continue;
            };
            let py = f64::from(tope_y) + f64::from(azar.siguiente()) + 0.1;
            let dist = ((px - camara[0]).powi(2)
                + (py - camara[1]).powi(2)
                + (pz - camara[2]).powi(2))
            .sqrt() as f32;
            if dist > cl::MAX_DISTANCIA_CONTROLADA {
                continue;
            }
            let Some(bioma) = level.client_biome(host, [x, tope_y - 1, z]) else {
                continue;
            };
            if !cl::es_overworld(&bioma) {
                continue;
            }
            let Some(props) = propiedades_bioma(host, level, &bioma, &mut cache_biomas) else {
                continue;
            };
            let sin_precip = !props.has_precipitation;
            let nombre = nombre_bloque(host, level, [x, tope_y - 1, z]);
            let probabilidad = azar.siguiente();
            let candidatos = [
                cl::Efecto::BrumaPolvo,
                cl::Efecto::Matorral,
                cl::Efecto::NieblaVertical,
                cl::Efecto::NieblaVerticalDespues,
            ];
            for efecto in candidatos {
                if !efecto.activa(&cfg) {
                    continue;
                }
                let ok = match efecto {
                    cl::Efecto::BrumaPolvo | cl::Efecto::Matorral => {
                        lluvia > 0.0 && sin_precip && cl::es_bloque_arena(&nombre)
                    }
                    cl::Efecto::NieblaVertical => {
                        dist >= cl::MIN_DISTANCIA_NIEBLA
                            && cl::es_bioma_niebla_siempre(&bioma)
                            && cl::es_bloque_niebla(&nombre)
                    }
                    cl::Efecto::NieblaVerticalDespues => {
                        tras_lluvia
                            && dist >= cl::MIN_DISTANCIA_NIEBLA
                            && !cl::es_bioma_excluido_niebla_despues(&bioma)
                            && cl::es_bloque_niebla(&nombre)
                    }
                    _ => false,
                };
                if !ok {
                    continue;
                }
                let d = efecto.datos();
                if d.densidad * cfg.densidad <= probabilidad {
                    continue;
                }
                let pos = [px, py, pz];
                let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                let vel = cl::velocidad_inicial_efecto(
                    d.origen,
                    d.gravedad,
                    d.rebote,
                    viento,
                    if tormenta { d.viento_tormenta } else { d.viento },
                );
                let tinte = match d.tinte {
                    cl::Tinte::Mapa => cl::tinte_bloque_arena(&nombre, azar.siguiente()),
                    cl::Tinte::Agua => cl::tinte_agua(),
                    cl::Tinte::Niebla => cl::tinte_niebla(None, lluvia),
                    cl::Tinte::Ninguno => [1.0, 1.0, 1.0],
                };
                encolar_particula(SpawnInfo {
                    pos,
                    efecto,
                    tamano_base: d.tamano,
                    distancia_max: alcance,
                    vida: d.vida as i32,
                    rgba: [tinte[0], tinte[1], tinte[2], 0.0],
                    frame: 0,
                    tope_y: f64::from(tope_y),
                });
                let opts = ParticleOptions::simple(clave_efecto(keys, efecto));
                if host.add_particle(&opts, pos, vel) {
                    cl::nace();
                    creadas_sup += 1;
                }
            }
        }
    }
    cl::contar_superficie(creadas_sup);

    // 3) Efectos sobre bloques (`tickBlockFX`): polvo raso, salpicadura y humo.
    if lluvia > 0.0
        && (cfg.polvo_raso || cfg.salpicadura || cfg.humo)
        && cl::vivas() < u64::from(cfg.tope)
    {
        let intentos_bloque = ((24.0 * cfg.densidad) as usize).max(1);
        let r = cfg.distancia;
        let mut puntos = Vec::with_capacity(intentos_bloque);
        let mut cols = Vec::with_capacity(intentos_bloque);
        for _ in 0..intentos_bloque {
            let dx = (azar.siguiente() * 2.0 - 1.0) * r;
            let dz = (azar.siguiente() * 2.0 - 1.0) * r;
            let px = camara[0] + f64::from(dx);
            let pz = camara[2] + f64::from(dz);
            let (x, z) = (px.floor() as i32, pz.floor() as i32);
            puntos.push((px, pz, x, z));
            cols.push((x, z));
        }
        let alturas = host
            .client_surface_heights(&cols, Heightmap::MotionBlocking)
            .unwrap_or_default();
        for (idx, (px, pz, x, z)) in puntos.into_iter().enumerate() {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
            let Some(Some(tope_y)) = alturas.get(idx).copied() else {
                continue;
            };
            let nombre = nombre_bloque(host, level, [x, tope_y - 1, z]);
            let es_lava = nombre.ends_with("lava");
            let py = f64::from(tope_y) + 0.05;
            let dist = ((px - camara[0]).powi(2)
                + (py - camara[1]).powi(2)
                + (pz - camara[2]).powi(2))
            .sqrt() as f32;
            if dist > r {
                continue;
            }
            let Some(bioma) = level.client_biome(host, [x, tope_y - 1, z]) else {
                continue;
            };
            let Some(props) = propiedades_bioma(host, level, &bioma, &mut cache_biomas) else {
                continue;
            };
            match precipitacion_en(&props, tope_y) {
                cl::Precipitacion::Ninguna => {
                    if !cfg.polvo_raso || !cl::es_overworld(&bioma) || !cl::es_bloque_arena(&nombre)
                    {
                        continue;
                    }
                    let probabilidad = azar.siguiente();
                    for efecto in cl::Efecto::ARRIBA_BLOQUE {
                        if !efecto.activa(&cfg) {
                            continue;
                        }
                        let d = efecto.datos();
                        if d.densidad * cfg.densidad <= probabilidad {
                            continue;
                        }
                        let pos = [px, py, pz];
                        let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                        let vel = cl::velocidad_inicial_efecto(
                            d.origen,
                            d.gravedad,
                            d.rebote,
                            viento,
                            if tormenta { d.viento_tormenta } else { d.viento },
                        );
                        let tinte = cl::tinte_bloque_arena(&nombre, azar.siguiente());
                        let frame = if d.num_sprites > 1 {
                            (azar.siguiente() * d.num_sprites as f32) as u32
                        } else {
                            0
                        };
                        encolar_particula(SpawnInfo {
                            pos,
                            efecto,
                            tamano_base: d.tamano,
                            distancia_max: cfg.distancia,
                            vida: d.vida as i32,
                            rgba: [tinte[0], tinte[1], tinte[2], 0.0],
                            frame,
                            tope_y: f64::from(tope_y),
                        });
                        let opts = ParticleOptions::simple(clave_efecto(keys, efecto));
                        if host.add_particle(&opts, pos, vel) {
                            cl::nace();
                        }
                    }
                }
                cl::Precipitacion::Lluvia => {
                    let pos = [px, py, pz];
                    if cfg.humo && cl::es_bloque_caliente(&nombre, es_lava) {
                        let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                        let _ = host.add_particle(
                            &ParticleOptions::simple(keys.vanilla_smoke),
                            pos,
                            [f64::from(viento[0] * 0.5), 0.04, f64::from(viento[1] * 0.5)],
                        );
                    } else if cfg.salpicadura && !es_lava && azar.siguiente() < 0.35 * cfg.densidad {
                        let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                        let mult = if tormenta { 0.2 } else { 0.1 };
                        let _ = host.add_particle(
                            &ParticleOptions::simple(keys.vanilla_rain),
                            pos,
                            [
                                f64::from(viento[0] * mult * 10.0),
                                0.0,
                                f64::from(viento[1] * mult * 10.0),
                            ],
                        );
                    }
                }
                cl::Precipitacion::Nieve => {}
            }
        }
    }

    // 4) Regueros en paredes y cristales (`streak`, `BLOCK_SIDES`).
    if lluvia > 0.0 && cfg.reguero && cl::vivas() < u64::from(cfg.tope) {
        let intentos_pared = ((16.0 * cfg.densidad) as usize).max(1);
        let r = cfg.distancia;
        const DIRS: [[i8; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];
        let mut puntos = Vec::with_capacity(intentos_pared);
        let mut cols = Vec::with_capacity(intentos_pared);
        for _ in 0..intentos_pared {
            let x = (camara[0] + f64::from((azar.siguiente() * 2.0 - 1.0) * r)).floor() as i32;
            let y = (camara[1] + f64::from((azar.siguiente() * 2.0 - 1.0) * 6.0)).floor() as i32;
            let z = (camara[2] + f64::from((azar.siguiente() * 2.0 - 1.0) * r)).floor() as i32;
            puntos.push((x, y, z));
            cols.push((x, z));
        }
        let alturas = host
            .client_surface_heights(&cols, Heightmap::MotionBlocking)
            .unwrap_or_default();
        for (idx, (x, y, z)) in puntos.into_iter().enumerate() {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
            let Some(Some(tope_y)) = alturas.get(idx).copied() else {
                continue;
            };
            if tope_y > y {
                continue;
            }
            if !host.block_collision([x, y, z]).is_empty() {
                continue;
            }
            let Some(bioma) = level.client_biome(host, [x, y, z]) else {
                continue;
            };
            let Some(props) = propiedades_bioma(host, level, &bioma, &mut cache_biomas) else {
                continue;
            };
            if precipitacion_en(&props, y) != cl::Precipitacion::Lluvia {
                continue;
            }
            let dir = DIRS[((azar.siguiente() * 4.0) as usize).min(3)];
            let pared_x = x - i32::from(dir[0]);
            let pared_z = z - i32::from(dir[1]);
            let nombre_pared = nombre_bloque(host, level, [pared_x, y, pared_z]);
            if host.block_collision([pared_x, y, pared_z]).is_empty()
                || !cl::es_bloque_reguero(&nombre_pared)
            {
                continue;
            }
            if 0.2 * cfg.densidad <= azar.siguiente() {
                continue;
            }
            let px = match dir[0] {
                1 => f64::from(x) + 0.06,
                -1 => f64::from(x) + 0.94,
                _ => f64::from(x) + f64::from(azar.siguiente()),
            };
            let pz = match dir[1] {
                1 => f64::from(z) + 0.06,
                -1 => f64::from(z) + 0.94,
                _ => f64::from(z) + f64::from(azar.siguiente()),
            };
            let py = f64::from(y) + f64::from(azar.siguiente());
            let tinte = cl::tinte_agua();
            let pos = [px, py, pz];
            encolar_reguero(RegueroSpawnInfo {
                pos,
                rgba: [tinte[0], tinte[1], tinte[2], 0.9 * lluvia],
                tope_suelo: f64::from(tope_y),
                semilla: (reloj.game_time as u64).wrapping_add(idx as u64 * 31 + 7),
            });
            if host.add_particle(&ParticleOptions::simple(keys.reguero), pos, [0.0, -0.1, 0.0]) {
                cl::nace();
            }
        }
    }
}
