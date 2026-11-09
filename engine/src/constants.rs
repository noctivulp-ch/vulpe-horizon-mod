//! Generación y gestión del canal fijo hacia el shader (`constantes_mod.wgsl`).
//!
//! Implementa las dos formas para que el gestor compare (C-05):
//! - **Forma B (archivo de serie):** el pack incluye de serie `constantes_mod.wgsl` en su raíz,
//!   ya registrado en `shared` de `horizon-shaders.toml` con `VULPE_FIXED_CHANNEL_ACTIVE = false`.
//!   El mod únicamente sobrescribe `constantes_mod.wgsl` sin tocar `horizon-shaders.toml`. Solo sirve
//!   cuando el pack activo es una carpeta.
//! - **Forma C (desempaquetar):** si el pack activo es un archivo `.hshader` (o si ya está seleccionada
//!   la carpeta desempaquetada), el mod lo descomprime una vez en una carpeta propia dentro de
//!   `shaderpacks/` (`vulpe-mod/`), la selecciona como pack activo y escribe allí `constantes_mod.wgsl`.
//!   Al actualizar el `.hshader`, la carpeta `vulpe-mod/` se regenera automáticamente.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{FixedChannelMode, VulpeConfig};
use crate::table::PARAM_TABLE;

/// Nombre estándar del archivo de constantes WGSL generado dentro del pack.
pub const CONSTANTS_WGSL_FILENAME: &str = "constantes_mod.wgsl";

/// Nombre del manifiesto de un pack nativo de Horizon.
pub const PACK_MANIFEST_FILENAME: &str = "horizon-shaders.toml";

/// Nombre de la carpeta propia en `shaderpacks/` donde la Forma C descomprime el `.hshader`.
pub const UNPACKED_PACK_DIR_NAME: &str = "vulpe-mod";

/// Identificador de selección de pack en Horizon (`settings.horizon.shader_pack`) para `shaderpacks/vulpe-mod/`.
pub const UNPACKED_PACK_KEY: &str = "horizon:vulpe_mod";

/// Archivo oculto dentro de `shaderpacks/vulpe-mod/` que guarda la huella del `.hshader` de origen.
/// Como empieza por `.`, `horizon-shader-api` (`read_dir`) lo ignora al validar la carpeta.
pub const UNPACKED_STAMP_FILENAME: &str = ".vulpe-hshader-stamp";

/// Resultado de intentar escribir el archivo de constantes en el pack.
#[derive(Debug, PartialEq)]
pub enum PackWriteResult {
    /// Archivo escrito exitosamente en la carpeta del pack (dispara recarga en caliente).
    Written(PathBuf),
    /// Archivo escrito en la carpeta desempaquetada (`shaderpacks/vulpe-mod/`) tras descomprimir un `.hshader` (Forma C).
    /// Contiene `(ruta_constantes_wgsl, ruta_carpeta_desempaquetada)`.
    UnpackedAndWritten {
        constants_file: PathBuf,
        unpacked_dir: PathBuf,
    },
    /// El pack es un archivo empaquetado `.hshader` y el modo elegido (`b`) no permite desempaquetarlo.
    PackedArchive(PathBuf),
    /// La ruta especificada del pack no existe en el disco.
    NotFound(PathBuf),
    /// Error de entrada/salida o de descompresión durante la escritura.
    IoError(String),
}

/// Genera el contenido WGSL del canal fijo con todos los parámetros de la tabla.
///
/// Como WGSL no tiene preprocesador (`#ifdef`), el archivo debe definir siempre las 38 constantes
/// `VULPE_<NOMBRE>` que referencian las etapas del shader pack, activando `VULPE_FIXED_CHANNEL_ACTIVE`
/// únicamente cuando existe al menos un parámetro modificado respecto a su valor por defecto.
pub fn generate_constants_wgsl(config: &VulpeConfig) -> String {
    let mut modified_count = 0;
    for param in PARAM_TABLE {
        let val = config.get(param.name);
        if (val - param.default).abs() > 1e-5 {
            modified_count += 1;
        }
    }
    let active = modified_count > 0;
    generate_constants_wgsl_with_state(config, active)
}

/// Genera el contenido WGSL con TODOS los parámetros de la tabla y `VULPE_FIXED_CHANNEL_ACTIVE = true`.
pub fn generate_full_constants_wgsl(config: &VulpeConfig) -> String {
    generate_constants_wgsl_with_state(config, true)
}

/// Genera el contenido WGSL de serie con todos los valores por defecto y `VULPE_FIXED_CHANNEL_ACTIVE = false`.
pub fn generate_default_constants_wgsl() -> String {
    generate_constants_wgsl_with_state(&VulpeConfig::default(), false)
}

fn generate_constants_wgsl_with_state(config: &VulpeConfig, active: bool) -> String {
    let mut out = String::new();
    out.push_str("// ==========================================================================\n");
    out.push_str("// Generado por vulpe-horizon-mod (Canal fijo de ajustes)\n");
    out.push_str("// Si la versión de tabla no coincide con la del pack, se descarta (§5).\n");
    out.push_str("// ==========================================================================\n\n");

    out.push_str(&format!(
        "const VULPE_PARAM_TABLE_VERSION: f32 = {:.1};\n",
        crate::PARAM_TABLE_VERSION
    ));
    out.push_str(&format!(
        "const VULPE_FIXED_CHANNEL_ACTIVE: bool = {};\n\n",
        if active { "true" } else { "false" }
    ));

    let mut modified_count = 0;
    for param in PARAM_TABLE {
        let val = config.get(param.name);
        if (val - param.default).abs() > 1e-5 {
            modified_count += 1;
        }
        let const_name = format!("VULPE_{}", param.name.to_uppercase());
        out.push_str(&format!(
            "// {} (rango: {} .. {}, defecto: {})\n",
            param.description, param.min, param.max, param.default
        ));
        out.push_str(&format!("const {}: f32 = {:.5};\n\n", const_name, val));
    }

    if modified_count == 0 {
        out.push_str("// Todos los parámetros conservan sus valores por defecto.\n");
    }

    out
}

/// En la carpeta extraída desde un `.hshader` (`shaderpacks/vulpe-mod/`), asigna el identificador
/// propio `id = "vulpe_mod"` para que el selector de Horizon (`shader_pack_list::resolve`) no colisione
/// por orden léxico con el `.hshader` original (`id = "vulpe"`), y asegura que `constantes_mod.wgsl`
/// esté registrado en `shared`. En la Forma B sobre carpetas de usuario nunca se invoca.
fn ensure_manifest_includes_constants(pack_path: &Path) -> Result<(), std::io::Error> {
    let manifest_path = pack_path.join(PACK_MANIFEST_FILENAME);
    if !manifest_path.is_file() {
        return Ok(());
    }

    let original = fs::read_to_string(&manifest_path)?;
    let mut content = original.replace("id = \"vulpe\"", "id = \"vulpe_mod\"");

    if !content.contains(CONSTANTS_WGSL_FILENAME) {
        content = if let Some(shared_pos) = content.find("shared") {
            if let Some(bracket_rel) = content[shared_pos..].find('[') {
                let insert_at = shared_pos + bracket_rel + 1;
                let mut out = String::with_capacity(content.len() + 40);
                out.push_str(&content[..insert_at]);
                out.push_str(&format!("\n    \"{CONSTANTS_WGSL_FILENAME}\","));
                out.push_str(&content[insert_at..]);
                out
            } else {
                content
            }
        } else {
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
    }

    if content != original {
        fs::write(&manifest_path, content)?;
    }
    Ok(())
}

/// Calcula una huella rápida (nombre, tamaño y FNV-1a/DefaultHasher de los bytes) de un archivo `.hshader`
/// para detectar cuándo se ha actualizado y regenerar `shaderpacks/vulpe-mod/`.
fn compute_hshader_stamp(hshader_path: &Path) -> Result<String, std::io::Error> {
    let bytes = fs::read(hshader_path)?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    if let Some(name) = hshader_path.file_name() {
        name.hash(&mut hasher);
    }
    bytes.len().hash(&mut hasher);
    bytes.hash(&mut hasher);
    let hash = hasher.finish();
    let file_name = hshader_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy();
    Ok(format!("{file_name}:{}:{hash:016x}", bytes.len()))
}

/// Busca el archivo `.hshader` más reciente en `shaderpacks/` cuando el pack activo ya es `vulpe-mod/`
/// para comprobar si el `.hshader` original fue actualizado.
pub fn find_source_hshader_in_dir(shaderpacks_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(shaderpacks_dir).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_file()
                && p.extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("hshader"))
        })
        .collect();
    candidates.sort();
    // Priorizar el que empiece por "vulpe" si hay varios
    if let Some(vulpe_pack) = candidates.iter().find(|p| {
        p.file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.to_ascii_lowercase().starts_with("vulpe"))
    }) {
        return Some(vulpe_pack.clone());
    }
    candidates.into_iter().next()
}

/// Descomprime un archivo `.hshader` (formato ZIP estándar) dentro de `target_dir`.
fn extract_zip_archive(hshader_path: &Path, target_dir: &Path) -> Result<(), String> {
    if target_dir.exists() {
        fs::remove_dir_all(target_dir).map_err(|e| format!("Error limpiando carpeta destino: {e}"))?;
    }
    fs::create_dir_all(target_dir).map_err(|e| format!("Error creando carpeta destino: {e}"))?;

    let status = Command::new("unzip")
        .arg("-q")
        .arg("-o")
        .arg(hshader_path)
        .arg("-d")
        .arg(target_dir)
        .status();

    match status {
        Ok(st) if st.success() => Ok(()),
        _ => {
            // Fallback con python3 zipfile si `unzip` no estuviera disponible
            let py_status = Command::new("python3")
                .arg("-c")
                .arg("import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])")
                .arg(hshader_path)
                .arg(target_dir)
                .status()
                .map_err(|e| format!("No se pudo ejecutar unzip ni python3 para descomprimir: {e}"))?;
            if py_status.success() {
                Ok(())
            } else {
                Err("El archivo .hshader no es un archivo ZIP válido o falló la extracción".to_string())
            }
        }
    }
}

/// Asegura que `shaderpacks/vulpe-mod/` contenga la extracción actualizada de `hshader_path`.
/// Si ya existe y la huella `.vulpe-hshader-stamp` coincide con la del `.hshader`, no vuelve a descomprimir.
pub fn ensure_unpacked_hshader(hshader_path: &Path) -> Result<PathBuf, String> {
    let parent_dir = hshader_path
        .parent()
        .unwrap_or_else(|| Path::new("."));
    let unpacked_dir = parent_dir.join(UNPACKED_PACK_DIR_NAME);
    let stamp_file = unpacked_dir.join(UNPACKED_STAMP_FILENAME);

    let current_stamp = compute_hshader_stamp(hshader_path)
        .map_err(|e| format!("No se pudo leer {}: {e}", hshader_path.display()))?;

    let needs_extract = if unpacked_dir.is_dir() && unpacked_dir.join(PACK_MANIFEST_FILENAME).is_file() {
        match fs::read_to_string(&stamp_file) {
            Ok(saved) => saved.trim() != current_stamp,
            Err(_) => true,
        }
    } else {
        true
    };

    if needs_extract {
        extract_zip_archive(hshader_path, &unpacked_dir)?;
        fs::write(&stamp_file, &current_stamp)
            .map_err(|e| format!("Error guardando huella en {}: {e}", stamp_file.display()))?;
    }

    Ok(unpacked_dir)
}

/// Comprueba si `pack_path` es un archivo `.hshader`.
pub fn is_hshader_path(pack_path: &Path) -> bool {
    pack_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("hshader"))
}

/// Escribe el archivo de constantes según el modo elegido en `config.canal_fijo` (`b`, `c` o `auto`).
///
/// - **Forma B (`FixedChannelMode::B`):** solo escribe `constantes_mod.wgsl` si `pack_path` es una carpeta
///   (y no es la carpeta desempaquetada `vulpe-mod` a menos que sea carpeta directa), **sin modificar nunca**
///   `horizon-shaders.toml`. Si `pack_path` es un `.hshader`, devuelve `PackWriteResult::PackedArchive`.
/// - **Forma C (`FixedChannelMode::C`):** si `pack_path` es un `.hshader`, lo descomprime en
///   `shaderpacks/vulpe-mod/` (regenerándolo si el `.hshader` cambió), asegura que `constantes_mod.wgsl` esté
///   registrado en el manifiesto desempaquetado y escribe allí `constantes_mod.wgsl`, devolviendo
///   `PackWriteResult::UnpackedAndWritten`. Si `pack_path` ya es `shaderpacks/vulpe-mod/`, comprueba si el
///   `.hshader` vecino en `shaderpacks/` se actualizó para regenerar la carpeta antes de escribir.
/// - **Auto (`FixedChannelMode::Auto`):** elige Forma C si `pack_path` es un `.hshader` (o `vulpe-mod/`)
///   y Forma B si es cualquier otra carpeta de pack.
pub fn write_pack_constants_with_mode(
    pack_path: &Path,
    config: &VulpeConfig,
    mode: FixedChannelMode,
) -> PackWriteResult {
    if !pack_path.exists() {
        return PackWriteResult::NotFound(pack_path.to_path_buf());
    }

    let is_hshader = pack_path.is_file() || is_hshader_path(pack_path);
    let is_unpacked_dir = pack_path.is_dir()
        && pack_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == UNPACKED_PACK_DIR_NAME);

    let effective_mode = match mode {
        FixedChannelMode::Auto => {
            if is_hshader || is_unpacked_dir {
                FixedChannelMode::C
            } else {
                FixedChannelMode::B
            }
        }
        other => other,
    };

    match effective_mode {
        FixedChannelMode::B => {
            if is_hshader
                || !pack_path.is_dir()
                || (is_unpacked_dir && pack_path.join(UNPACKED_STAMP_FILENAME).is_file())
            {
                return PackWriteResult::PackedArchive(pack_path.to_path_buf());
            }
            // Forma B: escribir únicamente constantes_mod.wgsl en la carpeta del pack,
            // SIN tocar horizon-shaders.toml (el pack ya lo trae registrado de serie).
            let target_file = pack_path.join(CONSTANTS_WGSL_FILENAME);
            let wgsl_content = generate_constants_wgsl(config);
            match fs::write(&target_file, wgsl_content) {
                Ok(()) => PackWriteResult::Written(target_file),
                Err(e) => PackWriteResult::IoError(e.to_string()),
            }
        }
        FixedChannelMode::C => {
            if is_hshader {
                let unpacked_dir = match ensure_unpacked_hshader(pack_path) {
                    Ok(dir) => dir,
                    Err(e) => return PackWriteResult::IoError(e),
                };
                if let Err(e) = ensure_manifest_includes_constants(&unpacked_dir) {
                    return PackWriteResult::IoError(e.to_string());
                }
                let target_file = unpacked_dir.join(CONSTANTS_WGSL_FILENAME);
                let wgsl_content = generate_constants_wgsl(config);
                match fs::write(&target_file, wgsl_content) {
                    Ok(()) => PackWriteResult::UnpackedAndWritten {
                        constants_file: target_file,
                        unpacked_dir,
                    },
                    Err(e) => PackWriteResult::IoError(e.to_string()),
                }
            } else if pack_path.is_dir() {
                // Si ya estamos en `shaderpacks/vulpe-mod/`, verificar si el `.hshader` de origen
                // en `shaderpacks/` cambió para regenerar la carpeta automáticamente.
                if is_unpacked_dir {
                    if let Some(shaderpacks_dir) = pack_path.parent() {
                        if let Some(src_hshader) = find_source_hshader_in_dir(shaderpacks_dir) {
                            let _ = ensure_unpacked_hshader(&src_hshader);
                        }
                    }
                    if let Err(e) = ensure_manifest_includes_constants(pack_path) {
                        return PackWriteResult::IoError(e.to_string());
                    }
                    let target_file = pack_path.join(CONSTANTS_WGSL_FILENAME);
                    let wgsl_content = generate_constants_wgsl(config);
                    return match fs::write(&target_file, wgsl_content) {
                        Ok(()) => PackWriteResult::UnpackedAndWritten {
                            constants_file: target_file,
                            unpacked_dir: pack_path.to_path_buf(),
                        },
                        Err(e) => PackWriteResult::IoError(e.to_string()),
                    };
                }

                // Si el usuario puso `canal_fijo = "c"` pero seleccionó una carpeta normal,
                // escribimos en la carpeta sin tocar su manifiesto (igual que B).
                let target_file = pack_path.join(CONSTANTS_WGSL_FILENAME);
                let wgsl_content = generate_constants_wgsl(config);
                match fs::write(&target_file, wgsl_content) {
                    Ok(()) => PackWriteResult::Written(target_file),
                    Err(e) => PackWriteResult::IoError(e.to_string()),
                }
            } else {
                PackWriteResult::PackedArchive(pack_path.to_path_buf())
            }
        }
        FixedChannelMode::Auto => unreachable!(),
    }
}

/// Escribe el archivo de constantes en el pack usando el modo configurado en `config.canal_fijo`.
pub fn write_pack_constants(pack_path: &Path, config: &VulpeConfig) -> PackWriteResult {
    write_pack_constants_with_mode(pack_path, config, config.canal_fijo)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_hshader_zip(zip_path: &Path, manifest_content: &str, post_wgsl: &str) {
        let src_dir = zip_path.parent().unwrap().join("zip_src_tmp");
        let _ = fs::remove_dir_all(&src_dir);
        fs::create_dir_all(&src_dir).unwrap();
        fs::write(src_dir.join(PACK_MANIFEST_FILENAME), manifest_content).unwrap();
        fs::write(src_dir.join("post.wgsl"), post_wgsl).unwrap();
        fs::write(
            src_dir.join(CONSTANTS_WGSL_FILENAME),
            generate_default_constants_wgsl(),
        )
        .unwrap();

        let py_status = Command::new("python3")
            .arg("-c")
            .arg("import sys, os, zipfile\nwith zipfile.ZipFile(sys.argv[1], 'w', zipfile.ZIP_DEFLATED) as zf:\n    for root, _, files in os.walk(sys.argv[2]):\n        for f in sorted(files):\n            full = os.path.join(root, f)\n            rel = os.path.relpath(full, sys.argv[2])\n            zf.write(full, rel)\n")
            .arg(zip_path)
            .arg(&src_dir)
            .status()
            .expect("Crear archivo .hshader de prueba con python3");
        assert!(py_status.success());
        let _ = fs::remove_dir_all(&src_dir);
    }

    #[test]
    fn test_generate_constants_wgsl_defaults() {
        let config = VulpeConfig::default();
        let wgsl = generate_constants_wgsl(&config);
        assert!(wgsl.contains("const VULPE_PARAM_TABLE_VERSION: f32 = 1.0;"));
        assert!(wgsl.contains("const VULPE_FIXED_CHANNEL_ACTIVE: bool = false;"));
        assert!(wgsl.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 1.00000;"));
        assert!(wgsl.contains("Todos los parámetros conservan sus valores por defecto"));
    }

    #[test]
    fn test_generate_constants_wgsl_with_overrides() {
        let mut config = VulpeConfig::default();
        config.set("cielo_brillo_manual", 3.5);
        config.set("aurora_radio", 800.0);

        let wgsl = generate_constants_wgsl(&config);
        assert!(wgsl.contains("const VULPE_FIXED_CHANNEL_ACTIVE: bool = true;"));
        assert!(wgsl.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 3.50000;"));
        assert!(wgsl.contains("const VULPE_AURORA_RADIO: f32 = 800.00000;"));
        // También incluye el resto de constantes en sus valores por defecto para que WGSL compile siempre
        assert!(wgsl.contains("const VULPE_LUNA_INTENSIDAD: f32 = 1.00000;"));
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
    fn test_mode_b_writes_constants_without_touching_manifest() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_mode_b_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let manifest_path = temp_dir.join(PACK_MANIFEST_FILENAME);
        let original_manifest = "schema = 1\nid = \"vulpe\"\nname = \"Vulpe\"\nversion = \"0.2.0-beta.1\"\napi = \"0.1\"\nshared = [\n    \"constantes_mod.wgsl\",\n]\n\n[[post]]\nfile = \"post.wgsl\"\n";
        fs::write(&manifest_path, original_manifest).unwrap();

        let mut config = VulpeConfig::default();
        config.canal_fijo = FixedChannelMode::B;
        config.set("cielo_brillo_manual", 2.0);

        let res = write_pack_constants(&temp_dir, &config);
        let expected_file = temp_dir.join(CONSTANTS_WGSL_FILENAME);
        assert_eq!(res, PackWriteResult::Written(expected_file.clone()));
        assert!(expected_file.exists());

        // Verificar que horizon-shaders.toml NO fue modificado
        let after_manifest = fs::read_to_string(&manifest_path).unwrap();
        assert_eq!(after_manifest, original_manifest);

        let read_back = fs::read_to_string(&expected_file).expect("Leer archivo escrito");
        assert!(read_back.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 2.00000;"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_mode_b_refuses_hshader() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_mode_b_hshader_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let fake_hshader = temp_dir.join("vulpe.hshader");
        fs::write(&fake_hshader, b"fake zip content").expect("Crear fake file");

        let mut config = VulpeConfig::default();
        config.canal_fijo = FixedChannelMode::B;
        let res = write_pack_constants(&fake_hshader, &config);
        assert_eq!(res, PackWriteResult::PackedArchive(fake_hshader));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_mode_c_unpacks_hshader_and_regenerates_on_update() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_mode_c_hshader_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let hshader_path = temp_dir.join("vulpe-0.2.0-beta.1.hshader");
        let manifest_v1 = "schema = 1\nid = \"vulpe\"\nname = \"Vulpe\"\nversion = \"0.2.0-beta.1\"\napi = \"0.1\"\nshared = [\"constantes_mod.wgsl\"]\n[[post]]\nfile = \"post.wgsl\"\n";
        create_test_hshader_zip(&hshader_path, manifest_v1, "// v1 post");

        let mut config = VulpeConfig::default();
        config.canal_fijo = FixedChannelMode::C;
        config.set("cielo_brillo_manual", 2.5);

        // 1. Primera ejecución con .hshader: descomprime en vulpe-mod/ y escribe constantes_mod.wgsl
        let res = write_pack_constants(&hshader_path, &config);
        let expected_unpacked = temp_dir.join(UNPACKED_PACK_DIR_NAME);
        let expected_constants = expected_unpacked.join(CONSTANTS_WGSL_FILENAME);
        assert_eq!(
            res,
            PackWriteResult::UnpackedAndWritten {
                constants_file: expected_constants.clone(),
                unpacked_dir: expected_unpacked.clone(),
            }
        );
        assert!(expected_constants.is_file());
        assert_eq!(
            fs::read_to_string(expected_unpacked.join("post.wgsl")).unwrap(),
            "// v1 post"
        );
        let wgsl = fs::read_to_string(&expected_constants).unwrap();
        assert!(wgsl.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 2.50000;"));

        // 2. Actualizar el .hshader con contenido nuevo (v2): al volver a sincronizar (incluso apuntando ya a vulpe-mod/),
        // la carpeta se regenera automáticamente
        create_test_hshader_zip(&hshader_path, manifest_v1, "// v2 post actualizado");
        config.set("cielo_brillo_manual", 3.25);
        let res2 = write_pack_constants(&expected_unpacked, &config);
        assert!(matches!(res2, PackWriteResult::UnpackedAndWritten { .. }));
        assert_eq!(
            fs::read_to_string(expected_unpacked.join("post.wgsl")).unwrap(),
            "// v2 post actualizado"
        );
        let wgsl2 = fs::read_to_string(&expected_constants).unwrap();
        assert!(wgsl2.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 3.25000;"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_write_pack_constants_not_found() {
        let non_existent = Path::new("/path/that/does/not/exist/surely");
        let config = VulpeConfig::default();
        let res = write_pack_constants(non_existent, &config);
        assert_eq!(res, PackWriteResult::NotFound(non_existent.to_path_buf()));
    }
}
