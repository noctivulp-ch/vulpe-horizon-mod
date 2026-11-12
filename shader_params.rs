fn vulpe_shader_params(
    game: &crate::game::Game<'_>,
    partial: f32,
    params: &mut [[f32; 4]; 8],
) {
    let day_time = game.native_clock().map(|clock| clock.day_time);
    let camera_pos = game
        .native_camera()
        .map(|cam| cam.position)
        .or_else(|| day_time.map(|_| game.view.position));
    // Formato 2: el bioma en la cámara (subsistema 21, niebla local).
    let biome = camera_pos.and_then(|c| {
        game.native_biome([c[0].floor() as i32, c[1].floor() as i32, c[2].floor() as i32])
    });
    vulpe_engine::observe_camera_biome(biome.as_deref());
    if let Some(bolt) = game.bolts.last() {
        vulpe_engine::observe_client_bolt(bolt.id, bolt.at);
    }
    let config_dir = crate::game::Game::native_config_dir().ok();
    let shader_packs_dir = crate::game::Game::shader_packs_dir();
    let picked_path = std::path::Path::new(&game.native_shaders.picked);
    let resolved_pack = if !game.native_shaders.picked.is_empty() && picked_path.exists() {
        picked_path.to_path_buf()
    } else if !game.settings.horizon.shader_pack.is_empty() {
        vulpe_engine::resolve_shader_pack_in_dir(
            &shader_packs_dir,
            &game.settings.horizon.shader_pack,
        )
        .unwrap_or_else(|| {
            horizon_client::shader_pack_list::resolve(
                &shader_packs_dir,
                &game.settings.horizon.shader_pack,
            )
            .1
        })
    } else if !game.native_shaders.picked.is_empty() {
        vulpe_engine::resolve_shader_pack_in_dir(&shader_packs_dir, &game.native_shaders.picked)
            .unwrap_or_else(|| picked_path.to_path_buf())
    } else {
        std::path::PathBuf::new()
    };
    let pack_path = (!resolved_pack.as_os_str().is_empty()).then_some(resolved_pack.as_path());
    vulpe_engine::sync_runtime_state(config_dir.as_deref(), pack_path, false);
    vulpe_engine::populate_shader_params_full(day_time, partial, camera_pos, params);
}
