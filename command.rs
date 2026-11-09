fn vulpe_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
    let config_dir = crate::game::Game::native_config_dir().ok();
    let picked = game.native_shaders.picked.clone();
    let pack_path = (!picked.is_empty()).then(|| std::path::Path::new(picked.as_str()));
    if let Some(reply) =
        vulpe_engine::execute_client_command(command, config_dir.as_deref(), pack_path)
    {
        *ran = true;
        for line in reply.lines() {
            game.native_chat(line);
        }
    }
}
