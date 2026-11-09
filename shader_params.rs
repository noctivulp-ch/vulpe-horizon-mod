fn vulpe_shader_params(
    game: &crate::game::Game<'_>,
    partial: f32,
    params: &mut [[f32; 4]; 8],
) {
    let day_time = game.native_clock().map(|clock| clock.day_time);
    let camera_pos = game.native_camera().map(|cam| cam.position);
    vulpe_engine::populate_shader_params_full(day_time, partial, camera_pos, params);
}
