mod clima_mod;

pub use vulpe_engine::*;

use std::path::{Path, PathBuf};

use mod_api::{
    ClientHost, ClientTickPostEvent, Command, CommandEvent, Flow, Heightmap, Hooks,
    LightningStrikeEvent, RegisterClientCommandsEvent, RegisterParticleProvidersEvent,
    RegistrationContext, RenderFramePreEvent, ServerHost, ShaderParamsEvent, Text,
};

/// Lee `pickedShaderPack:` de `horizon-graphics.txt` en la carpeta de la instancia.
fn read_picked_shader_pack(instance_dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(instance_dir.join("horizon-graphics.txt")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("pickedShaderPack:") {
            let val = rest.trim();
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

/// Resuelve el pack activo en `shaderpacks/` a partir de `horizon-graphics.txt`.
fn resolve_active_pack(instance_dir: &Path) -> Option<PathBuf> {
    let picked = read_picked_shader_pack(instance_dir)?;
    let shader_packs_dir = instance_dir.join("shaderpacks");
    let direct = Path::new(&picked);
    if direct.exists() {
        return Some(direct.to_path_buf());
    }
    vulpe_engine::resolve_shader_pack_in_dir(&shader_packs_dir, &picked)
}

/// Deduce si el jugador local está ardiendo a partir de los bloques en pies y cabeza, agua y lluvia (N-07).
fn observe_player_fire(host: &dyn ClientHost) {
    let Some(level) = host.level(host.dimension()) else {
        vulpe_engine::jugador::limpiar_fuego();
        return;
    };
    let pos_y_altura = host
        .local_player()
        .and_then(|p| {
            let ent = p.entity();
            let ojos = ent.eye_position(host)?;
            let altura = ent.eye_height(host).unwrap_or(1.62);
            Some((ojos, altura))
        })
        .or_else(|| host.camera().map(|c| (c.position, f64::from(c.eye_height))));
    let Some((ojos, altura_ojos)) = pos_y_altura else {
        vulpe_engine::jugador::limpiar_fuego();
        return;
    };

    let cabeza = [
        ojos[0].floor() as i32,
        ojos[1].floor() as i32,
        ojos[2].floor() as i32,
    ];
    let pies = [
        ojos[0].floor() as i32,
        (ojos[1] - altura_ojos + 0.1).floor() as i32,
        ojos[2].floor() as i32,
    ];

    let bloque_pies = level
        .block(host, pies)
        .map(|k| k.id(host))
        .unwrap_or_default();
    let bloque_cabeza = level
        .block(host, cabeza)
        .map(|k| k.id(host))
        .unwrap_or_default();
    let agua_extra = level
        .block_property(host, pies, "waterlogged")
        .as_deref()
        == Some("true")
        || level
            .block_property(host, cabeza, "waterlogged")
            .as_deref()
            == Some("true");

    let (lluvia, _) = level.weather(host).unwrap_or((0.0, 0.0));
    let bajo_lluvia = if lluvia > 0.0 {
        let llueve_en_bioma = level
            .client_biome(host, cabeza)
            .and_then(|b| level.biome_properties(host, &b))
            .is_some_and(|props| {
                if !props.has_precipitation {
                    return false;
                }
                let temp = if cabeza[1] > 80 {
                    props.temperature - (cabeza[1] - 80) as f32 * 0.00125
                } else {
                    props.temperature
                };
                temp >= 0.15
            });
        if llueve_en_bioma {
            host.client_surface_heights(&[(cabeza[0], cabeza[2])], Heightmap::MotionBlocking)
                .ok()
                .and_then(|h| h.first().copied().flatten())
                .is_some_and(|tope_y| tope_y <= cabeza[1] + 1)
        } else {
            false
        }
    } else {
        false
    };

    vulpe_engine::jugador::observar_fuego(&bloque_pies, &bloque_cabeza, agua_extra, bajo_lluvia);
}

/// Factor de noche `[0, 1]` desde `day_time` en ticks (24 000 por día; noche entre ~12 500 y 23 500).
fn factor_noche_ticks(day_time: Option<i64>) -> f32 {
    let Some(t) = day_time else { return 0.0 };
    let h = t.rem_euclid(24_000) as f32;
    if h >= 13_000.0 && h <= 23_000.0 {
        1.0
    } else if h > 12_000.0 && h < 13_000.0 {
        (h - 12_000.0) / 1_000.0
    } else if h > 23_000.0 {
        (24_000.0 - h) / 1_000.0
    } else {
        0.0
    }
}

/// Observa el clima de los cristales (N-08, partes 1 y 2: lluvia/secado, escarcha en el frío y viento).
fn observe_glass_weather(host: &dyn ClientHost, camera_pos: Option<[f64; 3]>, day_time: Option<i64>) {
    let (Some(level), Some(cam)) = (host.level(host.dimension()), camera_pos) else {
        vulpe_engine::cristales::limpiar();
        return;
    };
    let pos = [
        cam[0].floor() as i32,
        cam[1].floor() as i32,
        cam[2].floor() as i32,
    ];
    let (lluvia, trueno) = level.weather(host).unwrap_or((0.0, 0.0));
    let mut temp_efectiva = 0.8_f32;
    let mut llueve_en_bioma = false;
    if let Some(props) = level
        .client_biome(host, pos)
        .and_then(|b| level.biome_properties(host, &b))
    {
        temp_efectiva = if pos[1] > 80 {
            props.temperature - (pos[1] - 80) as f32 * 0.00125
        } else {
            props.temperature
        };
        llueve_en_bioma = props.has_precipitation && temp_efectiva >= 0.15;
    }

    let cfg_clima = vulpe_engine::clima::config();
    let v_raw = vulpe_engine::clima::viento(
        cam[0],
        cam[1],
        cam[2],
        vulpe_engine::clima::tiempo(),
        &cfg_clima,
    );
    let escala = (cfg_clima.viento_fuerza * 2.0).max(0.03);
    let viento_norm = [(v_raw[0] / escala).clamp(-1.0, 1.0), (v_raw[1] / escala).clamp(-1.0, 1.0)];
    let noche = factor_noche_ticks(day_time);
    vulpe_engine::cristales::observar_lluvia_y_frio(
        lluvia,
        trueno,
        llueve_en_bioma,
        temp_efectiva,
        noche,
        viento_norm,
    );
}

#[derive(Default)]
struct VulpeMod;

#[mod_api::horizon_mod]
impl Hooks for VulpeMod {
    fn register(&self, registrar: &mut dyn mod_api::Registrar, _: &RegistrationContext) {
        clima_mod::registrar_claves(registrar);
    }

    fn register_particle_providers(
        &self,
        _: &RegistrationContext,
        event: &mut RegisterParticleProvidersEvent<'_>,
    ) {
        clima_mod::registrar_proveedores(event.providers);
    }

    fn register_client_commands(
        &self,
        _: &mut dyn ClientHost,
        event: &mut RegisterClientCommandsEvent<'_>,
    ) {
        event.commands.push(Command::new("vulpe"));
        event.commands.push(Command::new("vulpe estado"));
        event.commands.push(Command::new("vulpe modo [modo:word]"));
        event.commands.push(Command::new("vulpe ver <parametro:word>"));
        event
            .commands
            .push(Command::new("vulpe poner <parametro:word> <valor:number>"));
        event
            .commands
            .push(Command::new("vulpe vivo <parametro:word> <valor:number>"));
        event.commands.push(Command::new("vulpe restablecer"));
    }

    fn command(&self, host: &mut dyn ClientHost, event: &mut CommandEvent<'_>) -> Flow {
        let instance_dir = host.instance_dir();
        let config_dir = instance_dir.join("config");
        let active_pack = resolve_active_pack(&instance_dir);
        let Some(reply) = vulpe_engine::execute_client_command(
            event.command,
            Some(&config_dir),
            active_pack.as_deref(),
        ) else {
            return Flow::Continue;
        };
        for line in reply.lines() {
            let _ = host.chat_line(Text::literal(line));
        }
        Flow::Cancel
    }

    fn lightning_strike(&self, _: &mut dyn ServerHost, event: &mut LightningStrikeEvent<'_>) -> Flow {
        vulpe_engine::record_lightning_strike(event.pos);
        Flow::Continue
    }

    fn render_frame_pre(&self, host: &mut dyn ClientHost, _: &mut RenderFramePreEvent) {
        let day_time = host.clock().map(|c| c.day_time);
        let camera_pos = host.camera().map(|c| c.position);
        vulpe_engine::observe_frame_snapshot(day_time, camera_pos);
        let biome = camera_pos.and_then(|c| {
            let level = host.level(host.dimension())?;
            level.client_biome(host, [c[0].floor() as i32, c[1].floor() as i32, c[2].floor() as i32])
        });
        vulpe_engine::observe_camera_biome(biome.as_deref());
        observe_player_fire(host);
        observe_glass_weather(host, camera_pos, day_time);
        let instance_dir = host.instance_dir();
        let config_dir = instance_dir.join("config");
        let active_pack = resolve_active_pack(&instance_dir);
        vulpe_engine::sync_runtime_state(Some(&config_dir), active_pack.as_deref(), false);
    }

    fn shader_params(&self, host: &dyn ClientHost, event: &mut ShaderParamsEvent<'_>) {
        let (snap_time, snap_cam) = vulpe_engine::last_frame_snapshot();
        let day_time = host.clock().map(|c| c.day_time).or(snap_time);
        let camera_pos = host.camera().map(|c| c.position).or(snap_cam);
        vulpe_engine::populate_shader_params_full(
            day_time,
            event.partial,
            camera_pos,
            event.params,
        );
    }

    fn client_tick_post(&self, host: &mut dyn ClientHost, _: &mut ClientTickPostEvent) {
        clima_mod::tick_clima(host);
    }
}
