fn vulpe_command(game: &mut crate::game::Game<'_>, command: &str, ran: &mut bool) {
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
    if let Some(reply) =
        vulpe_engine::execute_client_command(command, config_dir.as_deref(), pack_path)
    {
        *ran = true;
        if let Some(unpacked_dir) = vulpe_engine::take_pending_unpacked_pack_path() {
            let unpacked_str = unpacked_dir.to_string_lossy().into_owned();
            if game.settings.horizon.shader_pack != vulpe_engine::UNPACKED_PACK_KEY {
                game.settings.horizon.shader_pack = vulpe_engine::UNPACKED_PACK_KEY.to_string();
                let _ = game.settings.save(&crate::game::Game::options_path());
            }
            if game.native_shaders.picked != unpacked_str {
                game.native_shaders.select(&unpacked_str);
            }
        } else if vulpe_engine::take_pending_pack_revert() {
            if game.settings.horizon.shader_pack == vulpe_engine::UNPACKED_PACK_KEY {
                game.settings.horizon.shader_pack = "horizon:vulpe".to_string();
                let _ = game.settings.save(&crate::game::Game::options_path());
                let (_, resolved) = horizon_client::shader_pack_list::resolve(
                    &crate::game::Game::shader_packs_dir(),
                    "horizon:vulpe",
                );
                game.native_shaders.select(&resolved.to_string_lossy());
            }
        }
        for line in reply.lines() {
            game.native_chat(line);
        }
    }
}
