// Clima con partículas: port de Particle Rain (pigcart, MIT; ver NOTICE).
// Lógica en engine/src/clima.rs; aquí se lee el mundo y se crean las partículas. Porqué: docs/decisiones.md §7–§8.

fn vulpe_client_tick(game: &mut crate::game::Game<'_>) {
    /// Una partícula de clima: Horizon no le aplica movimiento propio; este controlador la mueve en cada tick.
    #[derive(Clone, Debug)]
    struct VulpeParticula {
        efecto: vulpe_engine::clima::Efecto,
        tamano_base: f32,
        distancia_max: f32,
        rebotes_borde: u8,
        giro: f32,
    }

    impl horizon_client::particles::ParticleController for VulpeParticula {
        fn tick(
            &mut self,
            p: &mut horizon_client::particles::Particle,
            level: &horizon_client::particles::ParticleLevel<'_>,
        ) -> horizon_client::particles::ParticleUpdate {
            use vulpe_engine::clima as cl;
            if !p.advance_age() {
                cl::muere();
                return Default::default();
            }
            let d = self.efecto.datos();
            let cfg = cl::config();
            let viento = cl::viento(p.position[0], p.position[1], p.position[2], cl::tiempo(), &cfg);
            let mult = if cl::tormenta() { d.viento_tormenta } else { d.viento };
            p.velocity = cl::paso(p.velocity, d.gravedad, viento, mult);

            let sig = [
                p.position[0] + p.velocity[0],
                p.position[1] + p.velocity[1],
                p.position[2] + p.velocity[2],
            ];
            let estado = level.world.block(
                sig[0].floor() as i32,
                sig[1].floor() as i32,
                sig[2].floor() as i32,
            );
            let f = level.facts.get(estado);
            if f.fluid != horizon_client::blocks::Fluid::None {
                cl::muere();
                p.remove();
                return Default::default();
            }
            if f.full_collision {
                if d.rebote > 0.0 {
                    // Determinar la normal de choque comparando eje a eje contra la posición anterior.
                    let (bx0, by0, bz0) = (
                        p.position[0].floor() as i32,
                        p.position[1].floor() as i32,
                        p.position[2].floor() as i32,
                    );
                    let (bx1, by1, bz1) = (
                        sig[0].floor() as i32,
                        sig[1].floor() as i32,
                        sig[2].floor() as i32,
                    );
                    let normal = if by1 != by0 {
                        [0.0, if by1 < by0 { 1.0 } else { -1.0 }, 0.0]
                    } else if bx1 != bx0 {
                        [if bx1 < bx0 { 1.0 } else { -1.0 }, 0.0, 0.0]
                    } else if bz1 != bz0 {
                        [0.0, 0.0, if bz1 < bz0 { 1.0 } else { -1.0 }]
                    } else {
                        [0.0, 1.0, 0.0]
                    };
                    if let Some(nueva_vel) = cl::rebotar(p.velocity, normal, d.rebote, &mut self.rebotes_borde) {
                        p.velocity = nueva_vel;
                        // Empujar ligeramente fuera de la cara para no quedarse dentro del bloque.
                        for i in 0..3 {
                            p.position[i] += normal[i] * 0.05;
                        }
                    } else {
                        cl::muere();
                        p.remove();
                        return Default::default();
                    }
                } else {
                    cl::muere();
                    p.remove();
                    return Default::default();
                }
            } else {
                p.position = sig;
            }

            let cam = cl::camara();
            let dist = ((p.position[0] - cam[0]).powi(2)
                + (p.position[1] - cam[1]).powi(2)
                + (p.position[2] - cam[2]).powi(2))
                .sqrt() as f32;
            if dist > self.distancia_max + 0.5 {
                cl::muere();
                p.remove();
                return Default::default();
            }
            if d.escala_distancia > 0.0 {
                p.size = cl::tamano_por_distancia(self.tamano_base, dist, d.escala_distancia);
            }
            let nivel_clima = match self.efecto {
                cl::Efecto::NieblaVertical | cl::Efecto::NieblaVerticalDespues => 1.0,
                _ => cl::lluvia(),
            };
            let mut alfa = cl::alfa_efecto(
                d.desvanecido,
                dist,
                self.distancia_max,
                p.age,
                p.lifetime,
                d.opacidad,
                nivel_clima,
            );
            if matches!(self.efecto, cl::Efecto::NieblaVertical | cl::Efecto::NieblaVerticalDespues) {
                alfa = alfa.min(cl::atenuacion_vertical(p.position, cam));
            }
            p.rgba[3] = alfa;
            let frame = if self.efecto == cl::Efecto::Matorral {
                let rapidez = (p.velocity[0] * p.velocity[0] + p.velocity[1] * p.velocity[1] + p.velocity[2] * p.velocity[2])
                    .sqrt() as f32;
                self.giro += rapidez;
                Some(((self.giro * 2.0) as usize) % d.num_sprites)
            } else {
                None
            };
            horizon_client::particles::ParticleUpdate {
                frame,
                ..Default::default()
            }
        }
    }

    /// Reguero de agua que baja por una pared o cristal (`StreakParticle`).
    #[derive(Clone, Debug)]
    struct VulpeReguero {
        normal: [i8; 2],
        gravedad: f32,
        desvaneciendo: bool,
        azar: vulpe_engine::clima::Azar,
    }

    impl horizon_client::particles::ParticleController for VulpeReguero {
        fn tick(
            &mut self,
            p: &mut horizon_client::particles::Particle,
            level: &horizon_client::particles::ParticleLevel<'_>,
        ) -> horizon_client::particles::ParticleUpdate {
            use vulpe_engine::clima as cl;
            if !p.advance_age() {
                cl::muere();
                return Default::default();
            }
            let mut spawned = Vec::new();
            if self.desvaneciendo {
                p.velocity = [0.0; 3];
                p.rgba[3] = (p.rgba[3] - 0.1).max(0.0);
                if p.rgba[3] <= 0.0 {
                    cl::muere();
                    p.remove();
                }
                return Default::default();
            }
            if p.age % 10 == 0 {
                self.gravedad = if self.azar.siguiente() < 0.5 {
                    self.azar.siguiente() * 0.1
                } else {
                    0.0
                };
            }
            p.velocity[1] = (p.velocity[1] - 0.04 * f64::from(self.gravedad)) * 0.98;
            let sig_y = p.position[1] + p.velocity[1];
            let bx = p.position[0].floor() as i32;
            let by = sig_y.floor() as i32;
            let bz = p.position[2].floor() as i32;
            let f_actual = level.facts.get(level.world.block(bx, by, bz));
            if f_actual.full_collision || f_actual.fluid != horizon_client::blocks::Fluid::None {
                self.desvaneciendo = true;
            } else {
                p.position[1] = sig_y;
            }
            let detras_x = (p.position[0] - f64::from(self.normal[0]) * 0.12).floor() as i32;
            let detras_z = (p.position[2] - f64::from(self.normal[1]) * 0.12).floor() as i32;
            let f_detras = level.facts.get(level.world.block(detras_x, p.position[1].floor() as i32, detras_z));
            if !f_detras.full_collision {
                spawned.push((
                    horizon_client::particles::ParticleOptions::simple(
                        horizon_client::particles::ParticleType::DrippingWater,
                    ),
                    [p.position[0], p.position[1] - 0.05, p.position[2]],
                    [0.0; 3],
                ));
                self.desvaneciendo = true;
            } else if !cl::es_bloque_reguero(&f_detras.name) {
                self.desvaneciendo = true;
            }
            p.rgba[3] = (0.9 * cl::lluvia()).clamp(0.0, 0.9);
            horizon_client::particles::ParticleUpdate {
                frame: None,
                spawned,
            }
        }
    }

    use vulpe_engine::clima as cl;
    // El gancho `tick` corre también en el servidor integrado: solo la ventana tiene partículas.
    if !game.native_window() {
        return;
    }
    let cfg = cl::config();
    let Some(reloj) = game.native_clock() else {
        cl::set_particulas_activas(false);
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
    // La cámara del fotograma, o la vista del juego si el gancho no la da (igual que shader_params.rs).
    let camara = game.native_camera().map(|c| c.position).unwrap_or(game.view.position);
    let (lluvia, trueno) = game.weather_levels(1.0);
    let tormenta = trueno > 0.5;
    let velocidad = cl::observar(camara, reloj.game_time.max(0) as u64, lluvia, tormenta);
    let tras_lluvia = cl::ticks_despues_lluvia() > 0;
    if reloj.game_time % 100 == 0 {
        cl::recortar_vivas(game.particles.len() as u64);
    }
    let puede_niebla = (cfg.niebla_vertical) || (cfg.niebla_vertical_despues && tras_lluvia);
    if (!hay_lluvia_fx && !hay_niebla_fx)
        || (lluvia <= 0.0 && !puede_niebla)
        || cl::vivas() >= u64::from(cfg.tope)
    {
        cl::contar_tick(0);
        cl::contar_superficie(0);
        return;
    }

    let mut azar = cl::Azar::new(reloj.game_time as u64 ^ 0x9E37_79B9_7F4A_7C15);
    let world = &game.world;
    let properties = &game.pack.assets.properties;
    let facts = game.res.facts();
    let biomes = world_biomes!(game);
    let particulas = &mut game.particles;

    // 1) Efectos de cielo (`tickSkyFX`): lluvia, nieve y polvo en el aire (`dust`).
    let mut creadas_cielo = 0u32;
    let (mut sin_cielo, mut sin_sprite) = (0u32, 0u32);
    if lluvia > 0.0 && (lluvia_nieve_activas || cfg.polvo) {
        let n_cielo = cl::densidad_cielo(&cfg, trueno, velocidad, cl::inactivo());
        for _ in 0..n_cielo {
        if cl::vivas() >= u64::from(cfg.tope) {
            break;
        }
        let o = cl::punto_nacimiento(azar.siguiente(), azar.siguiente(), azar.siguiente(), velocidad >= 0.8, cfg.distancia);
        let pos = [camara[0] + f64::from(o[0]), camara[1] + f64::from(o[1]), camara[2] + f64::from(o[2])];
        let (x, y, z) = (pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32);
        let Some(chunk) = world.chunks.get(&(x >> 4, z >> 4)) else {
            continue;
        };
        let tope_y = super::render_tick::motion_blocking_height(world, properties, x, z);
        if tope_y >= y {
            sin_cielo += 1;
            continue;
        }
        let bioma = biomes.get(chunk.biome(x, y, z));
        let precip = match bioma.snows_at(x, y, z) {
            Some(true) => cl::Precipitacion::Nieve,
            Some(false) => cl::Precipitacion::Lluvia,
            None => cl::Precipitacion::Ninguna,
        };
        let probabilidad = azar.siguiente();
        let bloque_suelo = if precip == cl::Precipitacion::Ninguna {
            Some(facts.get(world.block(x, tope_y - 1, z)).name.as_ref())
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
            if d.solo_overworld && !cl::es_overworld(&bioma.name) {
                continue;
            }
            if let Some(nombre_bloque) = bloque_suelo {
                if !cl::es_bloque_arena(nombre_bloque) {
                    continue;
                }
            }
            let ctrl = VulpeParticula {
                efecto,
                tamano_base: d.tamano,
                distancia_max: cfg.distancia,
                rebotes_borde: 0,
                giro: 0.0,
            };
            let Some(mut p) = particulas.controlled(d.sprites, pos, d.translucida, ctrl) else {
                sin_sprite += 1;
                continue;
            };
            let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
            p.velocity = cl::velocidad_inicial_efecto(
                d.origen,
                d.gravedad,
                d.rebote,
                viento,
                if tormenta { d.viento_tormenta } else { d.viento },
            );
            p.lifetime = d.vida;
            p.size = d.tamano;
            let tinte = match d.tinte {
                cl::Tinte::Agua => cl::tinte_agua(),
                cl::Tinte::Mapa => cl::tinte_bloque_arena(bloque_suelo.unwrap_or("sand"), azar.siguiente()),
                cl::Tinte::Niebla => cl::tinte_niebla(bioma.fog, lluvia),
                cl::Tinte::Ninguno => [1.0, 1.0, 1.0],
            };
            p.rgba = [tinte[0], tinte[1], tinte[2], 0.0];
            if d.num_sprites > 1 {
                particulas.set_sprite_frame(&mut p, (azar.siguiente() * d.num_sprites as f32) as usize);
            }
            particulas.push(p);
            cl::nace();
            creadas_cielo += 1;
        }
    }
    }
    cl::contar_tick(creadas_cielo);

    // 2) Efectos de superficie (`tickSurfaceFX`): bruma de polvo (`dust_haze`), matorrales (`shrub`) y nieblas.
    let mut creadas_sup = 0u32;
    if ((lluvia > 0.0 && (cfg.bruma_polvo || cfg.matorral)) || puede_niebla) && cl::vivas() < u64::from(cfg.tope) {
        let n_sup = cl::densidad_superficie(&cfg, trueno, cl::inactivo_superficie());
        let alcance = cfg.alcance_superficie.min(cl::MAX_DISTANCIA_CONTROLADA);
        for _ in 0..n_sup {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
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
            let Some(chunk) = world.chunks.get(&(x >> 4, z >> 4)) else {
                continue;
            };
            let tope_y = super::render_tick::motion_blocking_height(world, properties, x, z);
            let py = f64::from(tope_y) + f64::from(azar.siguiente()) + 0.1;
            let dist = ((px - camara[0]).powi(2) + (py - camara[1]).powi(2) + (pz - camara[2]).powi(2)).sqrt() as f32;
            if dist > cl::MAX_DISTANCIA_CONTROLADA {
                continue;
            }
            let bioma = biomes.get(chunk.biome(x, tope_y - 1, z));
            if !cl::es_overworld(&bioma.name) {
                continue;
            }
            let sin_precip = bioma.snows_at(x, tope_y - 1, z).is_none();
            let nombre_bloque = facts.get(world.block(x, tope_y - 1, z)).name.as_ref();
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
                        lluvia > 0.0 && sin_precip && cl::es_bloque_arena(nombre_bloque)
                    }
                    cl::Efecto::NieblaVertical => {
                        cl::es_bioma_niebla_siempre(&bioma.name) && cl::es_bloque_niebla(nombre_bloque)
                    }
                    cl::Efecto::NieblaVerticalDespues => {
                        tras_lluvia
                            && !cl::es_bioma_excluido_niebla_despues(&bioma.name)
                            && cl::es_bloque_niebla(nombre_bloque)
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
                let ctrl = VulpeParticula {
                    efecto,
                    tamano_base: d.tamano,
                    distancia_max: alcance,
                    rebotes_borde: 0,
                    giro: 0.0,
                };
                let Some(mut p) = particulas.controlled(d.sprites, pos, d.translucida, ctrl) else {
                    sin_sprite += 1;
                    continue;
                };
                let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                p.velocity = cl::velocidad_inicial_efecto(
                    d.origen,
                    d.gravedad,
                    d.rebote,
                    viento,
                    if tormenta { d.viento_tormenta } else { d.viento },
                );
                p.lifetime = d.vida;
                p.size = cl::tamano_por_distancia(d.tamano, dist, d.escala_distancia);
                let tinte = match d.tinte {
                    cl::Tinte::Mapa => cl::tinte_bloque_arena(nombre_bloque, azar.siguiente()),
                    cl::Tinte::Agua => cl::tinte_agua(),
                    cl::Tinte::Niebla => cl::tinte_niebla(bioma.fog, lluvia),
                    cl::Tinte::Ninguno => [1.0, 1.0, 1.0],
                };
                p.rgba = [tinte[0], tinte[1], tinte[2], 0.0];
                particulas.push(p);
                cl::nace();
                creadas_sup += 1;
            }
        }
    }
    cl::contar_superficie(creadas_sup);

    // 3) Efectos sobre bloques (`tickBlockFX`): polvo raso (`skittering_dust`), salpicadura (`splatter`) y humo (`smoke`).
    let mut creadas_bloque = 0u32;
    if lluvia > 0.0 && (cfg.polvo_raso || cfg.salpicadura || cfg.humo) && cl::vivas() < u64::from(cfg.tope) {
        let intentos_bloque = ((24.0 * cfg.densidad) as u32).max(1);
        let r = cfg.distancia;
        for _ in 0..intentos_bloque {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
            let dx = (azar.siguiente() * 2.0 - 1.0) * r;
            let dz = (azar.siguiente() * 2.0 - 1.0) * r;
            let px = camara[0] + f64::from(dx);
            let pz = camara[2] + f64::from(dz);
            let (x, z) = (px.floor() as i32, pz.floor() as i32);
            let Some(chunk) = world.chunks.get(&(x >> 4, z >> 4)) else {
                continue;
            };
            let tope_y = super::render_tick::motion_blocking_height(world, properties, x, z);
            let estado_suelo = world.block(x, tope_y - 1, z);
            let f_suelo = facts.get(estado_suelo);
            let es_lava = f_suelo.fluid == horizon_client::blocks::Fluid::Lava;
            let altura_fluido = if f_suelo.fluid != horizon_client::blocks::Fluid::None {
                f64::from(f_suelo.fluid_level) / 9.0
            } else {
                1.0
            };
            let py = f64::from(tope_y - 1) + altura_fluido + 0.05;
            let dist = ((px - camara[0]).powi(2) + (py - camara[1]).powi(2) + (pz - camara[2]).powi(2)).sqrt() as f32;
            if dist > r {
                continue;
            }
            let bioma = biomes.get(chunk.biome(x, tope_y - 1, z));
            let nombre_bloque = f_suelo.name.as_ref();
            match bioma.snows_at(x, tope_y, z) {
                None => {
                    if !cfg.polvo_raso || !cl::es_overworld(&bioma.name) || !cl::es_bloque_arena(nombre_bloque) {
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
                        let ctrl = VulpeParticula {
                            efecto,
                            tamano_base: d.tamano,
                            distancia_max: cfg.distancia,
                            rebotes_borde: 0,
                            giro: 0.0,
                        };
                        let Some(mut p) = particulas.controlled(d.sprites, pos, d.translucida, ctrl) else {
                            sin_sprite += 1;
                            continue;
                        };
                        let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                        p.velocity = cl::velocidad_inicial_efecto(
                            d.origen,
                            d.gravedad,
                            d.rebote,
                            viento,
                            if tormenta { d.viento_tormenta } else { d.viento },
                        );
                        p.lifetime = d.vida;
                        p.size = d.tamano;
                        let tinte = cl::tinte_bloque_arena(nombre_bloque, azar.siguiente());
                        p.rgba = [tinte[0], tinte[1], tinte[2], 0.0];
                        if d.num_sprites > 1 {
                            particulas.set_sprite_frame(&mut p, (azar.siguiente() * d.num_sprites as f32) as usize);
                        }
                        particulas.push(p);
                        cl::nace();
                        creadas_bloque += 1;
                    }
                }
                Some(false) => {
                    // Lluvia sobre bloques: humo en bloques calientes (`smoke`) y salpicadura con viento (`splatter`).
                    let pos = [px, py, pz];
                    if cfg.humo && cl::es_bloque_caliente(nombre_bloque, es_lava) {
                        let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                        particulas.add(
                            &horizon_client::particles::ParticleOptions::simple(
                                horizon_client::particles::ParticleType::Smoke,
                            ),
                            pos,
                            [f64::from(viento[0] * 0.5), 0.04, f64::from(viento[1] * 0.5)],
                        );
                        creadas_bloque += 1;
                    } else if cfg.salpicadura && !es_lava && azar.siguiente() < 0.35 * cfg.densidad {
                        let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
                        let mult = if tormenta { 0.2 } else { 0.1 };
                        particulas.add(
                            &horizon_client::particles::ParticleOptions::simple(
                                horizon_client::particles::ParticleType::Rain,
                            ),
                            pos,
                            [f64::from(viento[0] * mult * 10.0), 0.0, f64::from(viento[1] * mult * 10.0)],
                        );
                        creadas_bloque += 1;
                    }
                }
                Some(true) => {}
            }
        }
    }

    // 4) Regueros en paredes y cristales (`streak`, `BLOCK_SIDES`).
    let mut creadas_pared = 0u32;
    if lluvia > 0.0 && cfg.reguero && cl::vivas() < u64::from(cfg.tope) {
        let intentos_pared = ((16.0 * cfg.densidad) as u32).max(1);
        let r = cfg.distancia;
        const DIRS: [[i8; 2]; 4] = [[1, 0], [-1, 0], [0, 1], [0, -1]];
        for _ in 0..intentos_pared {
            if cl::vivas() >= u64::from(cfg.tope) {
                break;
            }
            let x = (camara[0] + f64::from((azar.siguiente() * 2.0 - 1.0) * r)).floor() as i32;
            let y = (camara[1] + f64::from((azar.siguiente() * 2.0 - 1.0) * 6.0)).floor() as i32;
            let z = (camara[2] + f64::from((azar.siguiente() * 2.0 - 1.0) * r)).floor() as i32;
            let Some(chunk) = world.chunks.get(&(x >> 4, z >> 4)) else {
                continue;
            };
            let f_aire = facts.get(world.block(x, y, z));
            if f_aire.full_collision || f_aire.fluid != horizon_client::blocks::Fluid::None {
                continue;
            }
            let tope_y = super::render_tick::motion_blocking_height(world, properties, x, z);
            if tope_y > y {
                continue;
            }
            let bioma = biomes.get(chunk.biome(x, y, z));
            if bioma.snows_at(x, y, z) != Some(false) {
                continue;
            }
            let dir = DIRS[((azar.siguiente() * 4.0) as usize).min(3)];
            let pared_x = x - i32::from(dir[0]);
            let pared_z = z - i32::from(dir[1]);
            let f_pared = facts.get(world.block(pared_x, y, pared_z));
            if !f_pared.full_collision || !cl::es_bloque_reguero(&f_pared.name) {
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
            let ctrl = VulpeReguero {
                normal: dir,
                gravedad: 0.0,
                desvaneciendo: false,
                azar: cl::Azar::new((reloj.game_time as u64).wrapping_add(creadas_pared as u64 * 31 + 7)),
            };
            let Some(mut p) = particulas.controlled("vulpe:reguero", [px, py, pz], true, ctrl) else {
                sin_sprite += 1;
                continue;
            };
            p.velocity = [0.0, -0.1, 0.0];
            p.lifetime = 200;
            p.size = 0.14;
            let tinte = cl::tinte_agua();
            p.rgba = [tinte[0], tinte[1], tinte[2], 0.9 * lluvia];
            particulas.push(p);
            cl::nace();
            creadas_pared += 1;
        }
    }

    if reloj.game_time % 100 == 0 {
        log::debug!(
            "Vulpe clima: lluvia {lluvia:.2}, cielo {creadas_cielo}, sup {creadas_sup}, bloque {creadas_bloque}, pared {creadas_pared}, sin cielo {sin_cielo}, sin sprite {sin_sprite}, propias {}, total {}",
            cl::vivas(),
            particulas.len()
        );
    }
}
