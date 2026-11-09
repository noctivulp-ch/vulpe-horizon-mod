use std::fs;
use std::sync::Mutex;
use vulpe_engine::*;

static E2E_MUTEX: Mutex<()> = Mutex::new(());

#[test]
fn test_end_to_end_flow() {
    let _guard = E2E_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
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

#[test]
fn test_forms_b_and_c_end_to_end() {
    let _guard = E2E_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_lightning_strike();
    clear_live_adjustments();
    set_fixed_channel_active(false);
    let _ = take_pending_unpacked_pack_path();
    let _ = take_pending_pack_revert();

    let temp_dir = std::env::temp_dir().join(format!("vulpe_e2e_bc_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    let cfg_dir = temp_dir.join("config");
    let shaderpacks_dir = temp_dir.join("shaderpacks");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::create_dir_all(&shaderpacks_dir).unwrap();

    // Crear un .hshader válido en shaderpacks/
    let src_dir = temp_dir.join("src_pack");
    fs::create_dir_all(&src_dir).unwrap();
    let manifest_text = "schema = 1\nid = \"vulpe\"\nname = \"Vulpe\"\nversion = \"0.2.0-beta.1\"\napi = \"0.1\"\nshared = [\"constantes_mod.wgsl\"]\n";
    fs::write(src_dir.join(PACK_MANIFEST_FILENAME), manifest_text).unwrap();
    fs::write(
        src_dir.join(CONSTANTS_WGSL_FILENAME),
        generate_default_constants_wgsl(),
    )
    .unwrap();
    let hshader_path = shaderpacks_dir.join("vulpe-0.2.0-beta.1.hshader");
    let py = std::process::Command::new("python3")
        .arg("-c")
        .arg("import sys, os, zipfile\nsrc, dst = sys.argv[1], sys.argv[2]\nwith zipfile.ZipFile(dst, 'w') as z:\n    for root, _, files in os.walk(src):\n        for f in files:\n            p = os.path.join(root, f)\n            z.write(p, os.path.relpath(p, src))\n")
        .arg(&src_dir)
        .arg(&hshader_path)
        .status()
        .unwrap();
    assert!(py.success());

    // 1. En modo B sobre .hshader: no desempaqueta y mantiene canal fijo apagado (mods[0].z = 5.0)
    let _ = execute_client_command("vulpe modo b", Some(&cfg_dir), Some(&hshader_path));
    let _ = execute_client_command(
        "vulpe poner cielo_brillo_manual 2.5",
        Some(&cfg_dir),
        Some(&hshader_path),
    );
    assert!(!is_fixed_channel_active());
    assert!(!shaderpacks_dir.join(UNPACKED_PACK_DIR_NAME).exists());
    let mut params = [[0.0; 4]; 8];
    populate_shader_params_full(Some(0), 0.0, Some([0.0, 0.0, 0.0]), &mut params);
    assert_eq!(params[0][2], 5.0);
    assert_eq!(params[0][3], 1.0);

    // 2. Cambiar a modo C sobre .hshader: desempaqueta en shaderpacks/vulpe-mod/, solicita selección y prende bit 4 (mods[0].z = 21.0)
    let _ = execute_client_command("vulpe modo c", Some(&cfg_dir), Some(&hshader_path));
    assert!(is_fixed_channel_active());
    let pending = take_pending_unpacked_pack_path().expect("Debe solicitar seleccionar vulpe-mod/");
    assert_eq!(pending, shaderpacks_dir.join(UNPACKED_PACK_DIR_NAME));
    assert!(pending.join(CONSTANTS_WGSL_FILENAME).is_file());
    populate_shader_params_full(Some(0), 0.0, Some([0.0, 0.0, 0.0]), &mut params);
    assert_eq!(params[0][2], 21.0);

    let _ = fs::remove_dir_all(&temp_dir);
}
