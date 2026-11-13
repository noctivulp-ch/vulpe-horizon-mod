// Clima con partículas: port de Particle Rain (pigcart, MIT; ver NOTICE). Etapa 1: lluvia y nieve.
// Lógica en engine/src/clima.rs; aquí se lee el mundo y se crean las partículas. Porqué: docs/decisiones.md §7.

fn vulpe_client_tick(game: &mut crate::game::Game<'_>) {
    /// Una gota o un copo: Horizon no le aplica movimiento propio; este controlador lo mueve en cada tick.
    #[derive(Clone, Debug)]
    struct VulpeGota {
        gota: vulpe_engine::clima::Gota,
    }

    impl horizon_client::particles::ParticleController for VulpeGota {
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
            let d = self.gota.datos();
            let cfg = cl::config();
            let viento = cl::viento(p.position[0], p.position[1], p.position[2], cl::tiempo(), &cfg);
            let mult = if cl::tormenta() { d.viento_tormenta } else { d.viento };
            p.velocity = cl::paso(p.velocity, d.gravedad, viento, mult);
            for i in 0..3 {
                p.position[i] += p.velocity[i];
            }
            let cam = cl::camara();
            let dist = ((p.position[0] - cam[0]).powi(2) + (p.position[1] - cam[1]).powi(2) + (p.position[2] - cam[2]).powi(2))
                .sqrt() as f32;
            if dist > cfg.distancia + 0.5 {
                cl::muere();
                p.remove();
                return Default::default();
            }
            // Se acaba al entrar en un bloque sólido o en un líquido (`onPositionUpdate`).
            let estado = level.world.block(
                p.position[0].floor() as i32,
                p.position[1].floor() as i32,
                p.position[2].floor() as i32,
            );
            let f = level.facts.get(estado);
            if f.full_collision || f.fluid != horizon_client::blocks::Fluid::None {
                cl::muere();
                p.remove();
                return Default::default();
            }
            p.rgba[3] = cl::alfa(dist, cfg.distancia, cl::lluvia());
            Default::default()
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
    let activas = cfg.lluvia && cfg.nieve;
    cl::set_particulas_activas(activas);
    // La cámara del fotograma, o la vista del juego si el gancho no la da (igual que shader_params.rs).
    let camara = game.native_camera().map(|c| c.position).unwrap_or(game.view.position);
    let (lluvia, trueno) = game.weather_levels(1.0);
    let tormenta = trueno > 0.5;
    let velocidad = cl::observar(camara, reloj.game_time.max(0) as u64, lluvia, tormenta);
    if reloj.game_time % 100 == 0 {
        cl::recortar_vivas(game.particles.len() as u64);
    }
    if !activas || lluvia <= 0.0 || cl::vivas() >= u64::from(cfg.tope) {
        cl::contar_tick(0);
        return;
    }
    let n = cl::densidad_cielo(&cfg, trueno, velocidad, cl::inactivo());
    let mut azar = cl::Azar::new(reloj.game_time as u64 ^ 0x9E37_79B9_7F4A_7C15);
    let world = &game.world;
    let properties = &game.pack.assets.properties;
    let biomes = world_biomes!(game);
    let particulas = &mut game.particles;
    let mut creadas = 0u32;
    let (mut sin_cielo, mut sin_precip, mut sin_sprite) = (0u32, 0u32, 0u32);
    for _ in 0..n {
        let o = cl::punto_nacimiento(azar.siguiente(), azar.siguiente(), azar.siguiente(), velocidad >= 0.8, cfg.distancia);
        let pos = [camara[0] + f64::from(o[0]), camara[1] + f64::from(o[1]), camara[2] + f64::from(o[2])];
        let (x, y, z) = (pos[0].floor() as i32, pos[1].floor() as i32, pos[2].floor() as i32);
        let Some(chunk) = world.chunks.get(&(x >> 4, z >> 4)) else {
            continue;
        };
        // Solo donde llega el cielo: por encima del primer bloque que frena el movimiento.
        if super::render_tick::motion_blocking_height(world, properties, x, z) >= y {
            sin_cielo += 1;
            continue;
        }
        let Some(nieva) = biomes.get(chunk.biome(x, y, z)).snows_at(x, y, z) else {
            sin_precip += 1;
            continue;
        };
        let probabilidad = azar.siguiente();
        for gota in cl::Gota::TODAS {
            let d = gota.datos();
            if d.nieve != nieva || (d.solo_tormenta && !tormenta) || d.densidad * cfg.densidad <= probabilidad {
                continue;
            }
            let Some(mut p) = particulas.controlled(d.sprites, pos, true, VulpeGota { gota }) else {
                sin_sprite += 1;
                continue;
            };
            let viento = cl::viento(pos[0], pos[1], pos[2], cl::tiempo(), &cfg);
            p.velocity = cl::velocidad_inicial(d.gravedad, viento, if tormenta { d.viento_tormenta } else { d.viento });
            p.lifetime = d.vida;
            p.size = d.tamano;
            let tinte = if d.tinte_agua { cl::tinte_agua() } else { [1.0, 1.0, 1.0] };
            p.rgba = [tinte[0], tinte[1], tinte[2], 0.0];
            particulas.set_sprite_frame(&mut p, (azar.siguiente() * 16.0) as usize);
            particulas.push(p);
            cl::nace();
            creadas += 1;
        }
    }
    cl::contar_tick(creadas);
    if reloj.game_time % 100 == 0 {
        log::debug!(
            "Vulpe clima: lluvia {lluvia:.2}, intentos {n}, nacen {creadas}, sin cielo {sin_cielo}, sin precipitación {sin_precip}, sin sprite {sin_sprite}, propias {}, total {}",
            cl::vivas(),
            particulas.len()
        );
    }
}
