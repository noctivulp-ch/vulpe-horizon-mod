fn vulpe_frame(game: &mut crate::game::Game<'_>) {
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
            let shader_packs_dir = crate::game::Game::shader_packs_dir();
            let resolved =
                vulpe_engine::resolve_shader_pack_in_dir(&shader_packs_dir, "horizon:vulpe")
                    .unwrap_or_else(|| {
                        horizon_client::shader_pack_list::resolve(
                            &shader_packs_dir,
                            "horizon:vulpe",
                        )
                        .1
                    });
            game.native_shaders.select(&resolved.to_string_lossy());
        }
    }
}
