use std::fs;
use vulpe_engine::*;

#[test]
fn test_end_to_end_flow() {
    clear_lightning_strike();
    clear_live_adjustments();
    set_fixed_channel_active(false);

    let temp_dir = std::env::temp_dir().join(format!("vulpe_e2e_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // 1. Configuración del jugador
    let mut config = VulpeConfig::load_or_create(&temp_dir).unwrap();
    assert_eq!(config.formato, 1);
    config.set("cielo_brillo_manual", 3.0);
    config.set("sol_intensidad", 2.0);
    config.save(&temp_dir).unwrap();

    // 2. Canal fijo: escribir constantes WGSL en la carpeta del pack
    let pack_dir = temp_dir.join("shaderpack");
    fs::create_dir_all(&pack_dir).unwrap();
    let write_res = write_pack_constants(&pack_dir, &config);
    assert!(matches!(write_res, PackWriteResult::Written(_)));
    set_fixed_channel_active(true);

    // 3. Ajustes en vivo: el jugador mueve un ajuste fino en tiempo real
    assert!(set_live_adjustment_by_name("bruma_aerosoles_r", 2.5));

    // 4. Rayo en el mundo
    record_lightning_strike_pos([50.0, 70.0, -100.0]);

    // 5. Generar frame.mods en el fotograma actual
    let camera_pos = Some([50.0, 70.0, -90.0]); // A 10 bloques de distancia en Z
    let mut params = [[0.0; 4]; 8];
    populate_shader_params_full(Some(24000), 0.0, camera_pos, &mut params);

    // Cabecera:
    // firma = 1001.0, tabla = 1.0
    // banderas = 1 (tiempo) | 2 (rayo) | 4 (estación) | 16 (canal fijo) = 23.0
    // cantidad ajustes en vivo = 1.0
    assert_eq!(params[0][0], 1001.0);
    assert_eq!(params[0][1], 1.0);
    assert_eq!(params[0][2], 23.0);
    assert_eq!(params[0][3], 1.0);

    // Tiempo: día 1, luna menguante (1)
    assert_eq!(params[1][0], 1.0);
    assert_eq!(params[1][1], 1.0);

    // Rayo en mods[2]: rel_x = 0, rel_y = 0, rel_z = -10, edad >= 0
    assert!((params[2][0] - 0.0).abs() < 1e-4);
    assert!((params[2][1] - 0.0).abs() < 1e-4);
    assert!((params[2][2] - (-10.0)).abs() < 1e-4);
    assert!(params[2][3] >= 0.0 && params[2][3] < 1.0);

    // Ajuste en vivo en mods[4][0]: bruma_aerosoles_r (id 4)
    let (id, _) = table::decode_live_raw(params[4][0]);
    assert_eq!(id, 4);

    let _ = fs::remove_dir_all(&temp_dir);
}
