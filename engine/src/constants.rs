//! Generación y gestión del canal fijo hacia el shader (`constantes_mod.wgsl`).
//!
//! Implementa únicamente la **Forma C** para cualquier pack (`canal_fijo = "c" | "apagado"`, C-06):
//! - Si el pack elegido es un archivo `.hshader`, se descomprime en `shaderpacks/vulpe-mod/`.
//! - Si el pack elegido es una carpeta (como `shaderpacks/vulpe-carpeta/`), se **copia** a
//!   `shaderpacks/vulpe-mod/`.
//! - En ambos casos se selecciona `shaderpacks/vulpe-mod/` (`horizon:vulpe_mod`) como pack activo
//!   y se escribe allí `constantes_mod.wgsl` sin tocar el pack original del jugador.
//! - La copia en `shaderpacks/vulpe-mod/` se regenera automáticamente cuando cambia el `.hshader`
//!   o la carpeta original mediante la huella guardada en `.vulpe-hshader-stamp`.
//! - Para encontrar el pack elegido cuando el juego guarda `horizon:<id>` en su configuración,
//!   `resolve_shader_pack_in_dir` recorre `shaderpacks/` y lee el `id` del manifiesto
//!   `horizon-shaders.toml` de cada carpeta o archivo `.hshader`, en vez de suponer que el nombre
//!   del archivo o carpeta coincide con el `id`.

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

/// Nombre de la carpeta propia en `shaderpacks/` donde la Forma C descomprime o copia el pack.
pub const UNPACKED_PACK_DIR_NAME: &str = "vulpe-mod";

/// Identificador de selección de pack en Horizon (`settings.horizon.shader_pack`) para `shaderpacks/vulpe-mod/`.
pub const UNPACKED_PACK_KEY: &str = "horizon:vulpe_mod";

/// Archivo oculto dentro de `shaderpacks/vulpe-mod/` que guarda la huella del pack de origen (`.hshader` o carpeta).
/// Como empieza por `.`, `horizon-shader-api` (`read_dir`) lo ignora al validar la carpeta.
pub const UNPACKED_STAMP_FILENAME: &str = ".vulpe-hshader-stamp";

/// Resultado de intentar escribir el archivo de constantes en el pack.
#[derive(Debug, PartialEq)]
pub enum PackWriteResult {
    /// Archivo escrito en la carpeta gestionada (`shaderpacks/vulpe-mod/`) tras descomprimir un `.hshader`
    /// o copiar una carpeta de pack (Forma C).
    /// Contiene `(ruta_constantes_wgsl, ruta_carpeta_desempaquetada)`.
    UnpackedAndWritten {
        constants_file: PathBuf,
        unpacked_dir: PathBuf,
    },
    /// El canal fijo está configurado en `"apagado"`.
    Disabled,
    /// La ruta especificada del pack no existe en el disco.
    NotFound(PathBuf),
    /// Error de entrada/salida, copia o descompresión durante la escritura.
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
    let active = modified_count > 0 && config.canal_fijo == FixedChannelMode::C;
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

/// Extrae el valor `id = "..."` de nivel superior de un manifiesto `horizon-shaders.toml`.
pub fn parse_manifest_id_str(manifest_content: &str) -> Option<String> {
    for line in manifest_content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if trimmed.starts_with('[') {
            // El id del pack siempre está en la raíz antes de cualquier sección [options], [[post]], etc.
            break;
        }
        if let Some((key, val)) = trimmed.split_once('=') {
            if key.trim() == "id" {
                let val_no_comment = match val.split_once('#') {
                    Some((v, _)) => v.trim(),
                    None => val.trim(),
                };
                let clean = val_no_comment.trim_matches('"').trim_matches('\'').trim();
                if !clean.is_empty() {
                    return Some(clean.to_string());
                }
            }
        }
    }
    None
}

/// Lee el `id` del manifiesto `horizon-shaders.toml` dentro de una carpeta o archivo `.hshader` (ZIP).
pub fn read_pack_manifest_id(pack_path: &Path) -> Option<String> {
    if pack_path.is_dir() {
        let manifest_path = pack_path.join(PACK_MANIFEST_FILENAME);
        let content = fs::read_to_string(manifest_path).ok()?;
        parse_manifest_id_str(&content)
    } else if pack_path.is_file() && is_hshader_path(pack_path) {
        let content = read_manifest_from_hshader_zip(pack_path)?;
        parse_manifest_id_str(&content)
    } else {
        None
    }
}

/// Lee `horizon-shaders.toml` desde el interior de un archivo `.hshader` (ZIP) sin extraerlo a disco.
fn read_manifest_from_hshader_zip(hshader_path: &Path) -> Option<String> {
    if let Ok(out) = Command::new("unzip")
        .arg("-p")
        .arg(hshader_path)
        .arg(PACK_MANIFEST_FILENAME)
        .output()
    {
        if out.status.success() && !out.stdout.is_empty() {
            if let Ok(s) = String::from_utf8(out.stdout) {
                return Some(s);
            }
        }
    }

    let py_out = Command::new("python3")
        .arg("-c")
        .arg("import sys, zipfile\nwith zipfile.ZipFile(sys.argv[1]) as z:\n    sys.stdout.write(z.read(sys.argv[2]).decode('utf-8'))\n")
        .arg(hshader_path)
        .arg(PACK_MANIFEST_FILENAME)
        .output()
        .ok()?;
    if py_out.status.success() && !py_out.stdout.is_empty() {
        String::from_utf8(py_out.stdout).ok()
    } else {
        None
    }
}

/// Resuelve el selector del pack elegido (`"horizon:<id>"`, `"<id>"` o una ruta/nombre) al archivo
/// `.hshader` o carpeta real dentro de `shaderpacks_dir` recorriendo `shaderpacks/` y leyendo el
/// manifiesto `horizon-shaders.toml` de cada candidato, en vez de suponer que el nombre de archivo
/// o carpeta coincide con el `id`.
pub fn resolve_shader_pack_in_dir(shaderpacks_dir: &Path, pack_selector: &str) -> Option<PathBuf> {
    let trimmed = pack_selector.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("off") {
        return None;
    }

    // Si ya es una ruta existente en el sistema de archivos, devolverla directamente
    let direct_path = Path::new(trimmed);
    if direct_path.exists() {
        return Some(direct_path.to_path_buf());
    }

    let target_id = trimmed.strip_prefix("horizon:").unwrap_or(trimmed).trim();
    if target_id.is_empty() {
        return None;
    }

    if target_id == "vulpe_mod" || target_id == UNPACKED_PACK_DIR_NAME {
        let unpacked = shaderpacks_dir.join(UNPACKED_PACK_DIR_NAME);
        if unpacked.is_dir() {
            return Some(unpacked);
        }
    }

    let entries = fs::read_dir(shaderpacks_dir).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                return false;
            }
            p.is_dir() || (p.is_file() && is_hshader_path(p))
        })
        .collect();
    candidates.sort();

    // 1. Buscar por el `id` declarado en `horizon-shaders.toml` (ignorando `vulpe-mod` salvo que se pida `vulpe_mod`)
    for candidate in &candidates {
        let is_unpacked = candidate
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == UNPACKED_PACK_DIR_NAME);
        if is_unpacked && target_id != "vulpe_mod" {
            continue;
        }
        if let Some(manifest_id) = read_pack_manifest_id(candidate) {
            if manifest_id == target_id {
                return Some(candidate.clone());
            }
        }
    }

    // 2. Respaldo por nombre de carpeta o archivo si no tenía manifiesto legible
    for candidate in candidates {
        let stem = if candidate.is_dir() {
            candidate.file_name().and_then(|n| n.to_str())
        } else {
            candidate.file_stem().and_then(|n| n.to_str())
        };
        if stem == Some(target_id) {
            return Some(candidate);
        }
    }

    None
}

/// En la carpeta gestionada `shaderpacks/vulpe-mod/`, asigna el identificador propio `id = "vulpe_mod"`
/// para que el selector de Horizon (`shader_pack_list::resolve`) no colisione con el pack original
/// (`id = "vulpe"`), y asegura que `constantes_mod.wgsl` esté registrado en `shared`.
fn ensure_manifest_includes_constants(pack_path: &Path) -> Result<(), std::io::Error> {
    let manifest_path = pack_path.join(PACK_MANIFEST_FILENAME);
    if !manifest_path.is_file() {
        return Ok(());
    }

    let original = fs::read_to_string(&manifest_path)?;
    let mut content = replace_manifest_id_with_vulpe_mod(&original);

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

fn replace_manifest_id_with_vulpe_mod(manifest_content: &str) -> String {
    let mut out = String::with_capacity(manifest_content.len() + 16);
    let mut in_root = true;
    let mut replaced = false;

    for line in manifest_content.split_inclusive('\n') {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_root = false;
        }
        if in_root && !replaced && !trimmed.starts_with('#') {
            if let Some((key, _)) = trimmed.split_once('=') {
                if key.trim() == "id" {
                    let newline = if line.ends_with("\r\n") {
                        "\r\n"
                    } else if line.ends_with('\n') {
                        "\n"
                    } else {
                        ""
                    };
                    out.push_str(&format!("id = \"vulpe_mod\"{newline}"));
                    replaced = true;
                    continue;
                }
            }
        }
        out.push_str(line);
    }
    out
}

/// Calcula una huella rápida de un archivo `.hshader` o de una carpeta de pack para detectar
/// cuándo ha cambiado el original y regenerar `shaderpacks/vulpe-mod/`.
pub fn compute_pack_source_stamp(source_path: &Path) -> Result<String, std::io::Error> {
    if source_path.is_file() {
        let bytes = fs::read(source_path)?;
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(name) = source_path.file_name() {
            name.hash(&mut hasher);
        }
        bytes.len().hash(&mut hasher);
        bytes.hash(&mut hasher);
        let hash = hasher.finish();
        let file_name = source_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        Ok(format!("file:{file_name}:{}:{hash:016x}", bytes.len()))
    } else if source_path.is_dir() {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        if let Some(name) = source_path.file_name() {
            name.hash(&mut hasher);
        }
        let mut file_count = 0usize;
        hash_dir_recursive(source_path, source_path, &mut hasher, &mut file_count)?;
        let hash = hasher.finish();
        let dir_name = source_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy();
        Ok(format!("dir:{dir_name}:{file_count}:{hash:016x}"))
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("No existe el pack de origen: {}", source_path.display()),
        ))
    }
}

fn hash_dir_recursive(
    root: &Path,
    current: &Path,
    hasher: &mut std::collections::hash_map::DefaultHasher,
    file_count: &mut usize,
) -> Result<(), std::io::Error> {
    let mut entries: Vec<PathBuf> = fs::read_dir(current)?
        .flatten()
        .map(|e| e.path())
        .collect();
    entries.sort();

    for path in entries {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') {
            continue;
        }
        let rel = path.strip_prefix(root).unwrap_or(&path);
        rel.to_string_lossy().hash(hasher);
        if path.is_dir() {
            hash_dir_recursive(root, &path, hasher, file_count)?;
        } else if path.is_file() {
            let bytes = fs::read(&path)?;
            bytes.len().hash(hasher);
            bytes.hash(hasher);
            *file_count += 1;
        }
    }
    Ok(())
}

/// Busca el pack original (`.hshader` o carpeta con `id = "vulpe"`) en `shaderpacks/` cuando el
/// pack activo ya es `shaderpacks/vulpe-mod/`, para comprobar si el original fue actualizado.
pub fn find_source_pack_in_dir(shaderpacks_dir: &Path) -> Option<PathBuf> {
    // 1. Si existe `shaderpacks/vulpe-mod/.vulpe-hshader-stamp`, intentar localizar primero el origen
    // cuyo tipo y nombre coincidan con el guardado en la huella (`dir:<nombre>:...` o `file:<nombre>:...`).
    let stamp_path = shaderpacks_dir
        .join(UNPACKED_PACK_DIR_NAME)
        .join(UNPACKED_STAMP_FILENAME);
    if let Ok(saved_stamp) = fs::read_to_string(&stamp_path) {
        let mut parts = saved_stamp.trim().splitn(3, ':');
        if let (Some(kind), Some(name)) = (parts.next(), parts.next()) {
            let candidate = shaderpacks_dir.join(name);
            if kind == "dir" && candidate.is_dir() && name != UNPACKED_PACK_DIR_NAME {
                return Some(candidate);
            } else if kind == "file" && candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    // 2. Buscar por manifiesto con `id = "vulpe"` en `shaderpacks/` (excluyendo `vulpe-mod`)
    if let Some(found) = resolve_shader_pack_in_dir(shaderpacks_dir, "horizon:vulpe") {
        return Some(found);
    }

    // 3. Buscar cualquier `.hshader` disponible en `shaderpacks/`
    find_source_hshader_in_dir(shaderpacks_dir)
}

/// Busca el archivo `.hshader` más reciente en `shaderpacks/` cuando el pack activo ya es `vulpe-mod/`.
pub fn find_source_hshader_in_dir(shaderpacks_dir: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(shaderpacks_dir).ok()?;
    let mut candidates: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_hshader_path(p))
        .collect();
    candidates.sort();
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
        fs::remove_dir_all(target_dir)
            .map_err(|e| format!("Error limpiando carpeta destino: {e}"))?;
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
            let py_status = Command::new("python3")
                .arg("-c")
                .arg("import sys, zipfile; zipfile.ZipFile(sys.argv[1]).extractall(sys.argv[2])")
                .arg(hshader_path)
                .arg(target_dir)
                .status()
                .map_err(|e| {
                    format!("No se pudo ejecutar unzip ni python3 para descomprimir: {e}")
                })?;
            if py_status.success() {
                Ok(())
            } else {
                Err(
                    "El archivo .hshader no es un archivo ZIP válido o falló la extracción"
                        .to_string(),
                )
            }
        }
    }
}

/// Copia recursivamente una carpeta de pack (`source_dir`) a `target_dir`.
fn copy_pack_directory(source_dir: &Path, target_dir: &Path) -> Result<(), String> {
    if target_dir.exists() {
        fs::remove_dir_all(target_dir)
            .map_err(|e| format!("Error limpiando carpeta destino: {e}"))?;
    }
    copy_dir_recursive_inner(source_dir, target_dir)
        .map_err(|e| format!("Error copiando carpeta del pack: {e}"))
}

fn copy_dir_recursive_inner(src: &Path, dst: &Path) -> Result<(), std::io::Error> {
    fs::create_dir_all(dst)?;
    let mut entries: Vec<PathBuf> = fs::read_dir(src)?.flatten().map(|e| e.path()).collect();
    entries.sort();

    for path in entries {
        let Some(name) = path.file_name() else {
            continue;
        };
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let dst_path = dst.join(name);
        if path.is_dir() {
            copy_dir_recursive_inner(&path, &dst_path)?;
        } else if path.is_file() {
            fs::copy(&path, &dst_path)?;
        }
    }
    Ok(())
}

/// Asegura que `shaderpacks/vulpe-mod/` contenga la extracción o copia actualizada de `source_path`
/// (que puede ser un `.hshader` o una carpeta de pack). Si ya existe y la huella `.vulpe-hshader-stamp`
/// coincide con la del original, no vuelve a descomprimir ni copiar.
pub fn ensure_unpacked_pack(source_path: &Path) -> Result<PathBuf, String> {
    let parent_dir = source_path.parent().unwrap_or_else(|| Path::new("."));
    let unpacked_dir = parent_dir.join(UNPACKED_PACK_DIR_NAME);
    let stamp_file = unpacked_dir.join(UNPACKED_STAMP_FILENAME);

    let current_stamp = compute_pack_source_stamp(source_path)
        .map_err(|e| format!("No se pudo calcular la huella de {}: {e}", source_path.display()))?;

    let needs_sync =
        if unpacked_dir.is_dir() && unpacked_dir.join(PACK_MANIFEST_FILENAME).is_file() {
            match fs::read_to_string(&stamp_file) {
                Ok(saved) => saved.trim() != current_stamp,
                Err(_) => true,
            }
        } else {
            true
        };

    if needs_sync {
        if source_path.is_file() {
            extract_zip_archive(source_path, &unpacked_dir)?;
        } else if source_path.is_dir() {
            copy_pack_directory(source_path, &unpacked_dir)?;
        } else {
            return Err(format!(
                "Ruta de pack inválida: {}",
                source_path.display()
            ));
        }
        fs::write(&stamp_file, &current_stamp)
            .map_err(|e| format!("Error guardando huella en {}: {e}", stamp_file.display()))?;
    }

    Ok(unpacked_dir)
}

/// Alias por compatibilidad para descomprimir o copiar un pack hacia `shaderpacks/vulpe-mod/`.
pub fn ensure_unpacked_hshader(hshader_path: &Path) -> Result<PathBuf, String> {
    ensure_unpacked_pack(hshader_path)
}

/// Comprueba si `pack_path` tiene extensión `.hshader`.
pub fn is_hshader_path(pack_path: &Path) -> bool {
    pack_path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("hshader"))
}

/// Escribe el archivo de constantes según el modo elegido en `config.canal_fijo` (`"c"` o `"apagado"`).
///
/// - **Forma C (`FixedChannelMode::C`):**
///   - Si `pack_path` es un archivo `.hshader`, lo descomprime en `shaderpacks/vulpe-mod/`.
///   - Si `pack_path` es una carpeta de pack (p. ej. `shaderpacks/vulpe-carpeta/`), la **copia** a
///     `shaderpacks/vulpe-mod/`.
///   - Si `pack_path` ya es `shaderpacks/vulpe-mod/`, comprueba si el pack original en `shaderpacks/`
///     cambió (comparando `.vulpe-hshader-stamp`) y regenera la copia si es necesario.
///   - En todos los casos asegura que `horizon-shaders.toml` en `vulpe-mod/` tenga `id = "vulpe_mod"`
///     y `"constantes_mod.wgsl"` en `shared`, escribe allí `constantes_mod.wgsl` y devuelve
///     `PackWriteResult::UnpackedAndWritten`.
/// - **Apagado (`FixedChannelMode::Apagado`):** no escribe ni modifica nada y devuelve `PackWriteResult::Disabled`.
pub fn write_pack_constants_with_mode(
    pack_path: &Path,
    config: &VulpeConfig,
    mode: FixedChannelMode,
) -> PackWriteResult {
    if mode == FixedChannelMode::Apagado {
        return PackWriteResult::Disabled;
    }

    if !pack_path.exists() {
        return PackWriteResult::NotFound(pack_path.to_path_buf());
    }

    let is_unpacked_dir = pack_path.is_dir()
        && pack_path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n == UNPACKED_PACK_DIR_NAME);

    let unpacked_dir = if is_unpacked_dir {
        if let Some(shaderpacks_dir) = pack_path.parent() {
            if let Some(src_pack) = find_source_pack_in_dir(shaderpacks_dir) {
                let _ = ensure_unpacked_pack(&src_pack);
            }
        }
        pack_path.to_path_buf()
    } else if pack_path.is_file() || pack_path.is_dir() {
        match ensure_unpacked_pack(pack_path) {
            Ok(dir) => dir,
            Err(e) => return PackWriteResult::IoError(e),
        }
    } else {
        return PackWriteResult::NotFound(pack_path.to_path_buf());
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
    fn test_mode_c_copies_folder_pack_without_touching_original_and_regenerates_on_change() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_mode_c_folder_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        let shaderpacks_dir = temp_dir.join("shaderpacks");
        let folder_pack = shaderpacks_dir.join("vulpe-carpeta");
        fs::create_dir_all(&folder_pack).expect("Crear carpeta de pack");

        let manifest_path = folder_pack.join(PACK_MANIFEST_FILENAME);
        let original_manifest = "schema = 1\nid = \"vulpe\"\nname = \"Vulpe\"\nversion = \"0.2.0-beta.1\"\napi = \"0.1\"\nshared = [\n    \"constantes_mod.wgsl\",\n]\n\n[[post]]\nfile = \"post.wgsl\"\n";
        let original_constants = generate_default_constants_wgsl();
        fs::write(&manifest_path, original_manifest).unwrap();
        fs::write(folder_pack.join(CONSTANTS_WGSL_FILENAME), &original_constants).unwrap();
        fs::write(folder_pack.join("post.wgsl"), "// v1 carpeta").unwrap();

        // Verificar que resolve_shader_pack_in_dir encuentra `vulpe-carpeta` a partir de `horizon:vulpe`
        let resolved = resolve_shader_pack_in_dir(&shaderpacks_dir, "horizon:vulpe")
            .expect("Debe resolver horizon:vulpe leyendo el manifiesto de vulpe-carpeta");
        assert_eq!(resolved, folder_pack);

        let mut config = VulpeConfig::default();
        config.canal_fijo = FixedChannelMode::C;
        config.set("cielo_brillo_manual", 2.5);

        // 1. Escribir constantes: debe copiar vulpe-carpeta a shaderpacks/vulpe-mod/ y escribir allí
        let res = write_pack_constants(&resolved, &config);
        let expected_unpacked = shaderpacks_dir.join(UNPACKED_PACK_DIR_NAME);
        let expected_constants = expected_unpacked.join(CONSTANTS_WGSL_FILENAME);
        assert_eq!(
            res,
            PackWriteResult::UnpackedAndWritten {
                constants_file: expected_constants.clone(),
                unpacked_dir: expected_unpacked.clone(),
            }
        );

        // El pack original del jugador (`vulpe-carpeta`) permanece intacto
        assert_eq!(fs::read_to_string(&manifest_path).unwrap(), original_manifest);
        assert_eq!(
            fs::read_to_string(folder_pack.join(CONSTANTS_WGSL_FILENAME)).unwrap(),
            original_constants
        );

        // La copia en `vulpe-mod` tiene id = "vulpe_mod" y las constantes nuevas
        let copied_manifest = fs::read_to_string(expected_unpacked.join(PACK_MANIFEST_FILENAME)).unwrap();
        assert!(copied_manifest.contains("id = \"vulpe_mod\""));
        let read_back = fs::read_to_string(&expected_constants).unwrap();
        assert!(read_back.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 2.50000;"));

        // 2. Modificar un archivo en la carpeta original `vulpe-carpeta`: al volver a sincronizar
        // (incluso apuntando ya a `vulpe-mod`), regenera la copia automáticamente
        fs::write(folder_pack.join("post.wgsl"), "// v2 carpeta modificada").unwrap();
        config.set("cielo_brillo_manual", 3.75);
        let res2 = write_pack_constants(&expected_unpacked, &config);
        assert!(matches!(res2, PackWriteResult::UnpackedAndWritten { .. }));
        assert_eq!(
            fs::read_to_string(expected_unpacked.join("post.wgsl")).unwrap(),
            "// v2 carpeta modificada"
        );
        let read_back2 = fs::read_to_string(&expected_constants).unwrap();
        assert!(read_back2.contains("const VULPE_CIELO_BRILLO_MANUAL: f32 = 3.75000;"));

        let _ = fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn test_mode_apagado_does_not_write() {
        let temp_dir =
            std::env::temp_dir().join(format!("vulpe_mode_apagado_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);
        fs::create_dir_all(&temp_dir).expect("Crear dir");

        let fake_hshader = temp_dir.join("vulpe.hshader");
        fs::write(&fake_hshader, b"fake zip content").expect("Crear fake file");

        let mut config = VulpeConfig::default();
        config.canal_fijo = FixedChannelMode::Apagado;
        config.set("cielo_brillo_manual", 2.5);
        let res = write_pack_constants(&fake_hshader, &config);
        assert_eq!(res, PackWriteResult::Disabled);
        assert!(!temp_dir.join(UNPACKED_PACK_DIR_NAME).exists());

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

        // Verificar que resolve_shader_pack_in_dir encuentra el .hshader por su id "vulpe"
        let resolved = resolve_shader_pack_in_dir(&temp_dir, "horizon:vulpe")
            .expect("Debe resolver horizon:vulpe al archivo .hshader");
        assert_eq!(resolved, hshader_path);

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
