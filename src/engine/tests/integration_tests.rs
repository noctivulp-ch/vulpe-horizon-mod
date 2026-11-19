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

    // 2. Canal fijo: copiar carpeta del pack a vulpe-mod/ y escribir constantes WGSL
    let pack_dir = temp_dir.join("shaderpack");
    fs::create_dir_all(&pack_dir).unwrap();
    let write_res = write_pack_constants(&pack_dir, &config);
    assert!(matches!(write_res, PackWriteResult::UnpackedAndWritten { .. }));
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
    // firma = 1002.0, tabla = 1.0
    // banderas = 1 (tiempo) | 2 (rayo) | 4 (estación) | 16 (canal fijo) = 23.0
    // cantidad ajustes en vivo = 1.0
    assert_eq!(params[0][0], 1002.0);
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
fn test_form_c_with_hshader_and_folder_pack_end_to_end() {
    let _guard = E2E_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    clear_lightning_strike();
    clear_live_adjustments();
    set_fixed_channel_active(false);
    let _ = take_pending_unpacked_pack_path();
    let _ = take_pending_pack_revert();

    let temp_dir = std::env::temp_dir().join(format!("vulpe_e2e_c06_{}", std::process::id()));
    let _ = fs::remove_dir_all(&temp_dir);
    let cfg_dir = temp_dir.join("config");
    let shaderpacks_dir = temp_dir.join("shaderpacks");
    fs::create_dir_all(&cfg_dir).unwrap();
    fs::create_dir_all(&shaderpacks_dir).unwrap();

    // A) Probar con pack en carpeta cuyo nombre (`vulpe-carpeta`) difiere del id (`vulpe`)
    let folder_pack = shaderpacks_dir.join("vulpe-carpeta");
    fs::create_dir_all(&folder_pack).unwrap();
    let manifest_text = "schema = 1\nid = \"vulpe\"\nname = \"Vulpe\"\nversion = \"0.2.0-beta.1\"\napi = \"0.1\"\nshared = [\"constantes_mod.wgsl\"]\n[[post]]\nfile = \"post.wgsl\"\n";
    fs::write(folder_pack.join(PACK_MANIFEST_FILENAME), manifest_text).unwrap();
    fs::write(
        folder_pack.join(CONSTANTS_WGSL_FILENAME),
        generate_default_constants_wgsl(),
    )
    .unwrap();
    fs::write(folder_pack.join("post.wgsl"), "// carpeta original v1").unwrap();

    // Resolver `horizon:vulpe` a `shaderpacks/vulpe-carpeta` leyendo los manifiestos
    let resolved_folder = resolve_shader_pack_in_dir(&shaderpacks_dir, "horizon:vulpe")
        .expect("Debe encontrar vulpe-carpeta a partir de horizon:vulpe");
    assert_eq!(resolved_folder, folder_pack);

    // Ejecutar `/vulpe poner`: en modo C (por defecto), copia la carpeta a `shaderpacks/vulpe-mod/`
    let _ = execute_client_command(
        "vulpe poner cielo_brillo_manual 2.5",
        Some(&cfg_dir),
        Some(&resolved_folder),
    );
    assert!(is_fixed_channel_active());
    let pending = take_pending_unpacked_pack_path().expect("Debe seleccionar vulpe-mod/");
    assert_eq!(pending, shaderpacks_dir.join(UNPACKED_PACK_DIR_NAME));
    assert!(pending.join(CONSTANTS_WGSL_FILENAME).is_file());
    // La carpeta original del jugador no se tocó
    assert!(
        fs::read_to_string(folder_pack.join(CONSTANTS_WGSL_FILENAME))
            .unwrap()
            .contains("const VULPE_FIXED_CHANNEL_ACTIVE: bool = false;")
    );
    let mut params = [[0.0; 4]; 8];
    populate_shader_params_full(Some(0), 0.0, Some([0.0, 0.0, 0.0]), &mut params);
    assert_eq!(params[0][2], 21.0);
    assert_eq!(params[0][3], 1.0);

    // Si el canal fijo se pone en "apagado", desactiva el bit 4 y solicita revertir vulpe-mod
    let _ = execute_client_command("vulpe modo apagado", Some(&cfg_dir), Some(&pending));
    assert!(!is_fixed_channel_active());
    assert!(take_pending_pack_revert());
    populate_shader_params_full(Some(0), 0.0, Some([0.0, 0.0, 0.0]), &mut params);
    assert_eq!(params[0][2], 5.0);

    // Volver a activar modo "c"
    let _ = execute_client_command("vulpe modo c", Some(&cfg_dir), Some(&resolved_folder));
    assert!(is_fixed_channel_active());
    populate_shader_params_full(Some(0), 0.0, Some([0.0, 0.0, 0.0]), &mut params);
    assert_eq!(params[0][2], 21.0);

    let _ = fs::remove_dir_all(&temp_dir);
}
