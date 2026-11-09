//! Gestión de la configuración del mod nativo (config/vulpe-horizon.toml).
//!
//! Lee y escribe archivos TOML sin dependencias externas, asegurando formato v1
//! y valores por defecto para todos los parámetros de la tabla.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::table::{PARAM_TABLE, find_param_by_name};

/// Nombre estándar del archivo de configuración del mod.
pub const CONFIG_FILE_NAME: &str = "vulpe-horizon.toml";

/// Configuración de ajustes finos del jugador para Vulpe.
#[derive(Clone, Debug, PartialEq)]
pub struct VulpeConfig {
    pub formato: u32,
    pub values: BTreeMap<String, f32>,
}

impl Default for VulpeConfig {
    fn default() -> Self {
        let mut values = BTreeMap::new();
        for param in PARAM_TABLE {
            values.insert(param.name.to_string(), param.default);
        }
        Self { formato: 1, values }
    }
}

impl VulpeConfig {
    /// Obtiene el valor configurado de un parámetro, o su valor por defecto si no existe.
    pub fn get(&self, name: &str) -> f32 {
        if let Some(param) = find_param_by_name(name) {
            self.values
                .get(name)
                .copied()
                .unwrap_or(param.default)
                .clamp(param.min, param.max)
        } else {
            0.0
        }
    }

    /// Modifica el valor de un parámetro, limitándolo al rango permitido.
    pub fn set(&mut self, name: &str, value: f32) -> bool {
        if let Some(param) = find_param_by_name(name) {
            let clamped = value.clamp(param.min, param.max);
            self.values.insert(name.to_string(), clamped);
            true
        } else {
            false
        }
    }

    /// Parsea una cadena de texto en formato TOML simple.
    pub fn from_toml_str(content: &str) -> Result<Self, String> {
        let mut config = Self::default();
        let mut _current_section = String::new();

        for (line_idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            // Ignorar líneas vacías o comentarios
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            // Encabezado de sección: [nombre]
            if trimmed.starts_with('[') && trimmed.ends_with(']') {
                _current_section = trimmed[1..trimmed.len() - 1].trim().to_string();
                continue;
            }

            // Par clave = valor
            if let Some((key, val_str)) = trimmed.split_once('=') {
                let key = key.trim();
                let val_str = val_str.trim();

                // Eliminar posible comentario al final del valor
                let clean_val = match val_str.split_once('#') {
                    Some((v, _)) => v.trim(),
                    None => val_str,
                };

                if key == "formato" {
                    let f: u32 = clean_val
                        .parse()
                        .map_err(|e| format!("Línea {}: formato inválido: {e}", line_idx + 1))?;
                    config.formato = f;
                    continue;
                }

                if let Some(param) = find_param_by_name(key) {
                    let num: f32 = clean_val.parse().map_err(|e| {
                        format!(
                            "Línea {}: valor numérico inválido para '{}': {e}",
                            line_idx + 1,
                            key
                        )
                    })?;
                    config.values.insert(key.to_string(), num.clamp(param.min, param.max));
                }
            }
        }

        Ok(config)
    }

    /// Serializa la configuración a una cadena TOML estructurada y comentada.
    pub fn to_toml_string(&self) -> String {
        let mut out = String::new();
        out.push_str("# Archivo de configuración del mod nativo de Vulpe para Horizon\n");
        out.push_str("# Ajustes finos que acompañan al pack de shaders vulpe-horizon\n");
        out.push_str(&format!("formato = {}\n\n", self.formato));

        // Agrupar por subsistema según el orden de la tabla
        let mut subsystems: Vec<&'static str> = Vec::new();
        for param in PARAM_TABLE {
            if !subsystems.contains(&param.subsystem) {
                subsystems.push(param.subsystem);
            }
        }

        for sub in subsystems {
            out.push_str(&format!("[{sub}]\n"));
            for param in PARAM_TABLE.iter().filter(|p| p.subsystem == sub) {
                let val = self.get(param.name);
                out.push_str(&format!(
                    "# {} (rango: {} .. {}, defecto: {})\n",
                    param.description, param.min, param.max, param.default
                ));
                out.push_str(&format!("{} = {:.4}\n", param.name, val));
            }
            out.push('\n');
        }

        out
    }

    /// Carga la configuración desde un archivo o crea uno con los valores por defecto si no existe.
    pub fn load_or_create(config_dir: &Path) -> Result<Self, std::io::Error> {
        let file_path = config_dir.join(CONFIG_FILE_NAME);

        if file_path.exists() {
            let content = fs::read_to_string(&file_path)?;
            match Self::from_toml_str(&content) {
                Ok(cfg) => Ok(cfg),
                Err(_) => {
                    // Si el archivo está corrupto, devolver configuración por defecto
                    Ok(Self::default())
                }
            }
        } else {
            let default_cfg = Self::default();
            // Asegurar que el directorio padre existe
            if let Some(parent) = file_path.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&file_path, default_cfg.to_toml_string())?;
            Ok(default_cfg)
        }
    }

    /// Guarda la configuración en disco en la ruta indicada.
    pub fn save(&self, config_dir: &Path) -> Result<(), std::io::Error> {
        let file_path = config_dir.join(CONFIG_FILE_NAME);
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&file_path, self.to_toml_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config_has_format_1_and_defaults() {
        let cfg = VulpeConfig::default();
        assert_eq!(cfg.formato, 1);
        assert_eq!(cfg.get("cielo_brillo_manual"), 1.0);
        assert_eq!(cfg.get("aurora_radio"), 300.0);
        assert_eq!(cfg.get("balance_blancos_k"), 6500.0);
    }

    #[test]
    fn test_toml_roundtrip() {
        let mut cfg = VulpeConfig::default();
        cfg.set("cielo_brillo_manual", 2.5);
        cfg.set("aurora_radio", 500.0);

        let toml_str = cfg.to_toml_string();
        let parsed = VulpeConfig::from_toml_str(&toml_str).expect("Debe parsear");

        assert_eq!(parsed.formato, 1);
        assert!((parsed.get("cielo_brillo_manual") - 2.5).abs() < 1e-4);
        assert!((parsed.get("aurora_radio") - 500.0).abs() < 1e-4);
        assert_eq!(parsed.get("balance_blancos_k"), 6500.0); // No modificado, debe conservar default
    }

    #[test]
    fn test_parse_partial_toml() {
        let toml = r#"
        formato = 1

        [cielo]
        cielo_brillo_manual = 3.2 # Ajuste personalizado

        [aurora]
        aurora_velo = 2.0
        "#;

        let parsed = VulpeConfig::from_toml_str(toml).expect("Debe parsear");
        assert_eq!(parsed.formato, 1);
        assert!((parsed.get("cielo_brillo_manual") - 3.2).abs() < 1e-4);
        assert!((parsed.get("aurora_velo") - 2.0).abs() < 1e-4);
        assert_eq!(parsed.get("aurora_radio"), 300.0); // Omitido, usa default
    }

    #[test]
    fn test_clamping_to_param_range() {
        let mut cfg = VulpeConfig::default();
        // cielo_brillo_manual tiene rango 0.0 .. 5.0
        cfg.set("cielo_brillo_manual", 99.0);
        assert_eq!(cfg.get("cielo_brillo_manual"), 5.0);

        cfg.set("cielo_brillo_manual", -10.0);
        assert_eq!(cfg.get("cielo_brillo_manual"), 0.0);
    }

    #[test]
    fn test_load_or_create_file() {
        let temp_dir = std::env::temp_dir().join(format!("vulpe_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&temp_dir);

        // Archivo no existe: debe crear y retornar default
        let loaded = VulpeConfig::load_or_create(&temp_dir).expect("Debe crear");
        assert_eq!(loaded.get("cielo_brillo_manual"), 1.0);
        assert!(temp_dir.join(CONFIG_FILE_NAME).exists());

        // Modificar y guardar
        let mut modified = loaded;
        modified.set("cielo_brillo_manual", 2.2);
        modified.save(&temp_dir).expect("Debe guardar");

        // Cargar nuevamente: debe leer el valor guardado
        let reloaded = VulpeConfig::load_or_create(&temp_dir).expect("Debe cargar");
        assert!((reloaded.get("cielo_brillo_manual") - 2.2).abs() < 1e-4);

        let _ = fs::remove_dir_all(&temp_dir);
    }
}
