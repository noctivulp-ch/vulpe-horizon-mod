mod clima_mod;

pub use vulpe_engine::*;

use std::path::{Path, PathBuf};

use mod_api::{
    ClientHost, ClientTickPostEvent, Command, CommandEvent, Flow, Hooks, LightningStrikeEvent,
    RegisterClientCommandsEvent, RegisterParticleProvidersEvent, RegistrationContext,
    RenderFramePreEvent, ServerHost, ShaderParamsEvent, Text,
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
