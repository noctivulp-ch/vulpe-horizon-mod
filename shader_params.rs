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
    let pack_path = (!game.native_shaders.picked.is_empty())
        .then(|| std::path::Path::new(game.native_shaders.picked.as_str()));
    vulpe_engine::sync_runtime_state(config_dir.as_deref(), pack_path, false);
    vulpe_engine::populate_shader_params_full(day_time, partial, camera_pos, params);
}
