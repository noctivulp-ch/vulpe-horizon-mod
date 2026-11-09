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
    if let Some(bolt) = game.bolts.last() {
        vulpe_engine::observe_client_bolt(bolt.id, bolt.at);
    }
    let config_dir = crate::game::Game::native_config_dir().ok();
    let resolved_pack = if !game.native_shaders.picked.is_empty() {
        std::path::PathBuf::from(&game.native_shaders.picked)
    } else if !game.settings.horizon.shader_pack.is_empty() {
        horizon_client::shader_pack_list::resolve(
            &crate::game::Game::shader_packs_dir(),
            &game.settings.horizon.shader_pack,
        )
        .1
    } else {
        std::path::PathBuf::new()
    };
    let pack_path = (!resolved_pack.as_os_str().is_empty()).then_some(resolved_pack.as_path());
    vulpe_engine::sync_runtime_state(config_dir.as_deref(), pack_path, false);
    vulpe_engine::populate_shader_params_full(day_time, partial, camera_pos, params);
}
