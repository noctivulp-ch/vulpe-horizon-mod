//! Generación y gestión del canal fijo hacia el shader (archivo de constantes WGSL).
//!
//! Escribe en la carpeta del shader pack un archivo WGSL con las constantes de los ajustes
//! finos que difieren de su valor por defecto. Horizon detecta cambios cada 500 ms (hot reload)
//! y reconstruye el pack automáticamente.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::VulpeConfig;
use crate::table::PARAM_TABLE;

/// Nombre estándar del archivo de constantes WGSL generado dentro del pack.
pub const CONSTANTS_WGSL_FILENAME: &str = "constantes_mod.wgsl";

/// Resultado de intentar escribir el archivo de constantes en el pack.
#[derive(Debug, PartialEq)]
pub enum PackWriteResult {
    /// Archivo escrito exitosamente en la carpeta del pack (dispara recarga en caliente).
    Written(PathBuf),
    /// El pack es un archivo empaquetado `.hshader` (comprimido en zip), no escribible directamente.
    PackedArchive(PathBuf),
    /// La ruta especificada del pack no existe en el disco.
    NotFound(PathBuf),
    /// Error de entrada/salida durante la escritura.
    IoError(String),
}

/// Genera el contenido WGSL del canal fijo con las constantes modificadas respecto al por defecto.
pub fn generate_constants_wgsl(config: &VulpeConfig) -> String {
    let mut out = String::new();
    out.push_str("// ==========================================================================\n");
    out.push_str("// Generado automáticamente por vulpe-horizon-mod (Canal fijo de ajustes)\n");
    out.push_str("// Si la versión de tabla no coincide con la del pack, se descarta (§5).\n");
    out.push_str("// ==========================================================================\n\n");

    out.push_str(&format!(
        "const VULPE_PARAM_TABLE_VERSION: f32 = {:.1};\n",
        crate::PARAM_TABLE_VERSION
    ));
    out.push_str("const VULPE_FIXED_CHANNEL_ACTIVE: bool = true;\n\n");

    let mut modified_count = 0;
    for param in PARAM_TABLE {
        let val = config.get(param.name);
        // Escribir solo aquellos parámetros que difieran de su valor por defecto
        if (val - param.default).abs() > 1e-5 {
            let const_name = format!("VULPE_{}", param.name.to_uppercase());
            out.push_str(&format!(
                "// {} (defecto: {})\n",
                param.description, param.default
            ));
            out.push_str(&format!("const {}: f32 = {:.5};\n\n", const_name, val));
            modified_count += 1;
        }
    }

    if modified_count == 0 {
        out.push_str("// Todos los parámetros conservan sus valores por defecto.\n");
    }

    out
}

/// Genera el contenido WGSL con TODOS los parámetros de la tabla (útil como plantilla de defaults).
pub fn generate_full_constants_wgsl(config: &VulpeConfig) -> String {
    let mut out = String::new();
    out.push_str("// ==========================================================================\n");
    out.push_str("// Constantes completas de vulpe-horizon (Canal fijo)\n");
    out.push_str("// ==========================================================================\n\n");

    out.push_str(&format!(
        "const VULPE_PARAM_TABLE_VERSION: f32 = {:.1};\n",
        crate::PARAM_TABLE_VERSION
    ));
    out.push_str("const VULPE_FIXED_CHANNEL_ACTIVE: bool = true;\n\n");

    for param in PARAM_TABLE {
        let val = config.get(param.name);
        let const_name = format!("VULPE_{}", param.name.to_uppercase());
        out.push_str(&format!(
            "// {} (rango: {} .. {}, defecto: {})\n",
            param.description, param.min, param.max, param.default
        ));
        out.push_str(&format!("const {}: f32 = {:.5};\n\n", const_name, val));
    }

    out
}

/// Nombre del manifiesto de un pack nativo de Horizon.
pub const PACK_MANIFEST_FILENAME: &str = "horizon-shaders.toml";

/// Si existe `horizon-shaders.toml` en la carpeta del pack, asegura que `constantes_mod.wgsl`
/// esté declarado en la lista `shared` para que el validador de Horizon (`Pack::from_files`)
/// acepte el archivo durante la recarga en caliente y lo inyecte en todas las etapas.
fn ensure_manifest_includes_constants(pack_path: &Path) -> Result<(), std::io::Error> {
    let manifest_path = pack_path.join(PACK_MANIFEST_FILENAME);
    if !manifest_path.is_file() {
        return Ok(());
    }

    let content = fs::read_to_string(&manifest_path)?;
    if content.contains(CONSTANTS_WGSL_FILENAME) {
        return Ok(());
    }

    let updated = if let Some(shared_pos) = content.find("shared") {
        if let Some(bracket_rel) = content[shared_pos..].find('[') {
            let insert_at = shared_pos + bracket_rel + 1;
            let mut out = String::with_capacity(content.len() + 40);
            out.push_str(&content[..insert_at]);
            out.push_str(&format!("\n    \"{CONSTANTS_WGSL_FILENAME}\","));
            out.push_str(&content[insert_at..]);
            out
        } else {
            return Ok(());
        }
    } else {
        // Insertar `shared = ["constantes_mod.wgsl"]` antes de la primera sección `[`
        let mut insert_offset = content.len();
        let mut offset = 0;
        for line in content.split_inclusive('\n') {
            if line.trim_start().starts_with('[') {
                insert_offset = offset;
                break;
            }
            offset += line.len();
        }
        let mut out = String::with_capacity(content.len() + 48);
        out.push_str(&content[..insert_offset]);
        out.push_str(&format!("shared = [\"{CONSTANTS_WGSL_FILENAME}\"]\n\n"));
        out.push_str(&content[insert_offset..]);
        out
    };

    if updated != content {
        fs::write(&manifest_path, updated)?;
    }
    Ok(())
}

/// Escribe el archivo de constantes en el directorio del pack.
///
/// Si el pack es un archivo `.hshader`, detecta que está empaquetado y no permite
/// la escritura en caliente directa, documentando el fallback a ajustes en vivo.
pub fn write_pack_constants(pack_path: &Path, config: &VulpeConfig) -> PackWriteResult {
    // 1. Comprobar si es un archivo comprimido .hshader
    let is_hshader = pack_path.is_file()
        || pack_path
            .extension()
            .and_then(|ext| ext.to_str())
            .map_or(false, |ext| ext.eq_ignore_ascii_case("hshader"));

    if is_hshader {
        return PackWriteResult::PackedArchive(pack_path.to_path_buf());
    }

    // 2. Comprobar si existe la ruta
    if !pack_path.exists() {
        return PackWriteResult::NotFound(pack_path.to_path_buf());
    }

    // 3. Debe ser un directorio
    if !pack_path.is_dir() {
        return PackWriteResult::PackedArchive(pack_path.to_path_buf());
    }

    let target_file = pack_path.join(CONSTANTS_WGSL_FILENAME);
    let wgsl_content = generate_constants_wgsl(config);

    match fs::write(&target_file, wgsl_content) {
        Ok(()) => {
            if let Err(e) = ensure_manifest_includes_constants(pack_path) {
                return PackWriteResult::IoError(e.to_string());
            }
            PackWriteResult::Written(target_file)
        }
        Err(e) => PackWriteResult::IoError(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_constants_wgsl_defaults() {
        let config = VulpeConfig::default();
        let wgsl = generate_constants_wgsl(&config);
        assert!(wgsl.contains("const VULPE_PARAM_TABLE_VERSION: f32 = 1.0;"));
        assert!(wgsl.contains("const VULPE_FIXED_CHANNEL_ACTIVE: bool = true;"));
        assert!(wgsl.contains("Todos los parámetros conservan sus valores por defecto"));
    }

    #[test]
    fn test_generate_constants_wgsl_with_overrides() {
        let mut config = VulpeConfig::default();
        config.set("cielo_brillo_manual", 3.5);
        config.set("aurora_radio", 800.0);

        let wgsl = generate_constants_wgsl(&config);
        assert!(wgsl.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 3.50000;"));
        assert!(wgsl.contains("const VULPE_AURORA_RADIO: f32 = 800.00000;"));
        // No debe contener constantes para los que no cambiaron
        assert!(!wgsl.contains("VULPE_LUNA_INTENSIDAD"));
    }

    #[test]
    fn test_generate_full_constants_wgsl() {
        let config = VulpeConfig::default();
        let wgsl = generate_full_constants_wgsl(&config);
        assert!(wgsl.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 1.00000;"));
        assert!(wgsl.contains("const VULPE_AURORA_RADIO: f32 = 300.00000;"));
        assert!(wgsl.contains("const VULPE_BALANCE_BLANCOS_K: f32 = 6500.00000;"));
    }

    #[test]
    fn test_write_pack_constants_to_directory() {
        let temp_dir = std::env::temp_dir().join(format!("vulpe_pack_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let mut config = VulpeConfig::default();
        config.set("cielo_brillo_manual", 2.0);

        let res = write_pack_constants(&temp_dir, &config);
        let expected_file = temp_dir.join(CONSTANTS_WGSL_FILENAME);
        assert_eq!(res, PackWriteResult::Written(expected_file.clone()));
        assert!(expected_file.exists());

        let read_back = fs::read_to_string(&expected_file).expect("Leer archivo escrito");
        assert!(read_back.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 2.00000;"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_write_pack_constants_to_hshader() {
        let temp_dir = std::env::temp_dir().join(format!("vulpe_hshader_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let fake_hshader = temp_dir.join("vulpe.hshader");
        fs::write(&fake_hshader, b"fake zip content").expect("Crear fake file");

        let config = VulpeConfig::default();
        let res = write_pack_constants(&fake_hshader, &config);
        assert_eq!(res, PackWriteResult::PackedArchive(fake_hshader));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_write_pack_constants_not_found() {
        let non_existent = Path::new("/path/that/does/not/exist/surely");
        let config = VulpeConfig::default();
        let res = write_pack_constants(non_existent, &config);
        assert_eq!(res, PackWriteResult::NotFound(non_existent.to_path_buf()));
    }

    #[test]
    fn test_write_pack_constants_updates_manifest_shared() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_manifest_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let manifest_path = temp_dir.join(PACK_MANIFEST_FILENAME);
        let initial_manifest = "schema = 1\nid = \"vulpe\"\nname = \"Vulpe\"\nversion = \"0.2.0\"\napi = \"0.1\"\nshared = [\n    \"nucleo/matematica/constantes.wgsl\",\n]\n\n[[post]]\nfile = \"post.wgsl\"\n";
        fs::write(&manifest_path, initial_manifest).unwrap();

        let mut config = VulpeConfig::default();
        config.set("cielo_brillo_manual", 2.5);

        let res = write_pack_constants(&temp_dir, &config);
        assert!(matches!(res, PackWriteResult::Written(_)));

        let updated_manifest = fs::read_to_string(&manifest_path).unwrap();
        assert!(updated_manifest.contains("\"constantes_mod.wgsl\""));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_write_pack_constants_inserts_shared_when_absent_in_manifest() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_manifest_noshared_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let manifest_path = temp_dir.join(PACK_MANIFEST_FILENAME);
        let initial_manifest = "schema = 1\nid = \"sonda\"\nname = \"Sonda\"\nversion = \"0.1.0\"\napi = \"0.1\"\n\n[[post]]\nfile = \"post.wgsl\"\n";
        fs::write(&manifest_path, initial_manifest).unwrap();

        let mut config = VulpeConfig::default();
        config.set("cielo_brillo_manual", 2.5);

        let res = write_pack_constants(&temp_dir, &config);
        assert!(matches!(res, PackWriteResult::Written(_)));

        let updated_manifest = fs::read_to_string(&manifest_path).unwrap();
        assert!(updated_manifest.contains("shared = [\"constantes_mod.wgsl\"]"));
        let shared_idx = updated_manifest.find("shared =").unwrap();
        let post_idx = updated_manifest.find("[[post]]").unwrap();
        assert!(shared_idx < post_idx);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
