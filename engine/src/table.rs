//! Tabla unificada de parámetros finos para el mod nativo y shaders de Vulpe.
//!
//! Cada parámetro tiene un id único en el rango 0..=4095, nombre, subsistema,
//! rango [mín, máx] y valor por defecto.

/// Definición de un parámetro en la tabla unificada.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamDef {
    pub id: u32,
    pub name: &'static str,
    pub subsystem: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub description: &'static str,
}

impl ParamDef {
    /// Cuantiza un valor continuo en el rango [min, max] a un entero en 0..=4095.
    pub fn quantize(&self, value: f32) -> u32 {
        let clamped = value.clamp(self.min, self.max);
        if (self.max - self.min).abs() < 1e-6 {
            return 0;
        }
        let t = (clamped - self.min) / (self.max - self.min);
        let q = (t * 4095.0).round() as u32;
        q.min(4095)
    }

    /// Decodifica un valor cuantizado q (0..=4095) al valor continuo [min, max].
    pub fn dequantize(&self, q: u32) -> f32 {
        let q_clamped = q.min(4095) as f32;
        self.min + (self.max - self.min) * (q_clamped / 4095.0)
    }

    /// Codifica el ajuste en vivo según formato v1: id × 4096 + q.
    pub fn encode_live(&self, value: f32) -> f32 {
        let q = self.quantize(value);
        ((self.id * 4096) + q) as f32
    }
}

/// Decodifica un float de ajuste en vivo (id × 4096 + q) retornando (id, q).
pub fn decode_live_raw(encoded: f32) -> (u32, u32) {
    if !encoded.is_finite() || encoded < 0.0 {
        return (0, 0);
    }
    let code = encoded as u32;
    let id = code / 4096;
    let q = code % 4096;
    (id, q)
}

/// Lista completa de parámetros de la tabla v1 (mapa-de-ajustes.md §3).
pub static PARAM_TABLE: &[ParamDef] = &[
    // 01 Cielo y atmósfera
    ParamDef {
        id: 0,
        name: "cielo_brillo_manual",
        subsystem: "cielo",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Brillo del cielo manual",
    },
    ParamDef {
        id: 1,
        name: "aire_color_r",
        subsystem: "cielo",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        description: "Multiplicador de color del aire (canal rojo)",
    },
    ParamDef {
        id: 2,
        name: "aire_color_g",
        subsystem: "cielo",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        description: "Multiplicador de color del aire (canal verde)",
    },
    ParamDef {
        id: 3,
        name: "aire_color_b",
        subsystem: "cielo",
        min: 0.0,
        max: 2.0,
        default: 1.0,
        description: "Multiplicador de color del aire (canal azul)",
    },
    ParamDef {
        id: 4,
        name: "bruma_aerosoles_r",
        subsystem: "cielo",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Bruma de aerosoles (canal rojo)",
    },
    ParamDef {
        id: 5,
        name: "bruma_aerosoles_g",
        subsystem: "cielo",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Bruma de aerosoles (canal verde)",
    },
    ParamDef {
        id: 6,
        name: "bruma_aerosoles_b",
        subsystem: "cielo",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Bruma de aerosoles (canal azul)",
    },
    ParamDef {
        id: 7,
        name: "halo_asimetria",
        subsystem: "cielo",
        min: 0.0,
        max: 1.0,
        default: 0.8,
        description: "Asimetría de fase del halo solar",
    },
    ParamDef {
        id: 8,
        name: "sol_intensidad",
        subsystem: "cielo",
        min: 0.0,
        max: 10.0,
        default: 1.0,
        description: "Intensidad luminosa del sol",
    },
    ParamDef {
        id: 9,
        name: "luna_intensidad",
        subsystem: "cielo",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Intensidad luminosa de la luna",
    },
    // 02 Cielo fractal
    ParamDef {
        id: 10,
        name: "fractal_manual",
        subsystem: "fractal",
        min: 0.0,
        max: 1.0,
        default: 0.0,
        description: "Modo manual del cielo fractal (0: auto, 1: manual)",
    },
    ParamDef {
        id: 11,
        name: "fractal_semilla",
        subsystem: "fractal",
        min: 0.0,
        max: 65535.0,
        default: 0.0,
        description: "Semilla base del dibujo fractal manual",
    },
    ParamDef {
        id: 12,
        name: "fractal_saturacion",
        subsystem: "fractal",
        min: 0.0,
        max: 3.0,
        default: 1.0,
        description: "Saturación de color del cielo fractal",
    },
    ParamDef {
        id: 13,
        name: "fractal_contraste",
        subsystem: "fractal",
        min: 0.0,
        max: 3.0,
        default: 1.0,
        description: "Contraste tonal del cielo fractal",
    },
    ParamDef {
        id: 14,
        name: "fractal_ciclos_paleta",
        subsystem: "fractal",
        min: 0.5,
        max: 8.0,
        default: 1.0,
        description: "Frecuencia de repetición de la paleta fractal",
    },
    // 03 Luz de superficie
    ParamDef {
        id: 15,
        name: "luz_ambiente_brillo",
        subsystem: "luz_superficie",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Brillo multiplicador de la luz ambiente",
    },
    ParamDef {
        id: 16,
        name: "luz_minima_exterior",
        subsystem: "luz_superficie",
        min: 0.0,
        max: 1.0,
        default: 0.05,
        description: "Iluminación mínima al aire libre",
    },
    ParamDef {
        id: 17,
        name: "luz_bloque_intensidad",
        subsystem: "luz_superficie",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Intensidad de la luz de bloques (antorchas, etc.)",
    },
    ParamDef {
        id: 18,
        name: "estaciones_activa",
        subsystem: "luz_superficie",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        description: "Habilita la modulación estacional del entorno",
    },
    ParamDef {
        id: 19,
        name: "estacion_duracion_dias",
        subsystem: "luz_superficie",
        min: 1.0,
        max: 120.0,
        default: 24.0,
        description: "Duración de cada estación en días de juego",
    },
    // 04 Sombras
    ParamDef {
        id: 20,
        name: "sombras_profundidad_busqueda",
        subsystem: "sombras",
        min: 1.0,
        max: 64.0,
        default: 16.0,
        description: "Pasos de búsqueda para penumbra de sombras",
    },
    ParamDef {
        id: 21,
        name: "sombras_tenido_translucidos",
        subsystem: "sombras",
        min: 0.0,
        max: 1.0,
        default: 1.0,
        description: "Teñido de sombras a través de vidrios de color",
    },
    // 05 Niebla y nubes
    ParamDef {
        id: 22,
        name: "niebla_altura_caida",
        subsystem: "niebla_nubes",
        min: 0.0,
        max: 1.0,
        default: 0.1,
        description: "Velocidad de disminución de la niebla con la altura",
    },
    ParamDef {
        id: 23,
        name: "nubes_altura_capa1",
        subsystem: "niebla_nubes",
        min: 50.0,
        max: 400.0,
        default: 192.0,
        description: "Altitud en bloques de la capa principal de nubes",
    },
    ParamDef {
        id: 24,
        name: "nubes_espesor_capa1",
        subsystem: "niebla_nubes",
        min: 10.0,
        max: 100.0,
        default: 32.0,
        description: "Grosor volumétrico de la capa principal de nubes",
    },
    // 06 Aurora
    ParamDef {
        id: 25,
        name: "aurora_radio",
        subsystem: "aurora",
        min: 50.0,
        max: 1000.0,
        default: 300.0,
        description: "Radio de alcance de las cortinas de aurora",
    },
    ParamDef {
        id: 26,
        name: "aurora_velo",
        subsystem: "aurora",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Opacidad y fuerza del velo de aurora",
    },
    ParamDef {
        id: 27,
        name: "aurora_cortina",
        subsystem: "aurora",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Longitud vertical de la cortina de aurora",
    },
    // 07 Agua
    ParamDef {
        id: 28,
        name: "agua_absorcion_r",
        subsystem: "agua",
        min: 0.0,
        max: 1.0,
        default: 0.35,
        description: "Absorción espectral en agua (rojo)",
    },
    ParamDef {
        id: 29,
        name: "agua_absorcion_g",
        subsystem: "agua",
        min: 0.0,
        max: 1.0,
        default: 0.15,
        description: "Absorción espectral en agua (verde)",
    },
    ParamDef {
        id: 30,
        name: "agua_absorcion_b",
        subsystem: "agua",
        min: 0.0,
        max: 1.0,
        default: 0.05,
        description: "Absorción espectral en agua (azul)",
    },
    ParamDef {
        id: 31,
        name: "agua_turbidez",
        subsystem: "agua",
        min: 0.0,
        max: 5.0,
        default: 1.0,
        description: "Turbidez y dispersión de partículas en agua",
    },
    // 11 Exposición y color
    ParamDef {
        id: 32,
        name: "exposicion_manual",
        subsystem: "color",
        min: 0.05,
        max: 10.0,
        default: 1.0,
        description: "Valor fijo de exposición cuando auto está apagado",
    },
    ParamDef {
        id: 33,
        name: "adaptacion_velocidad",
        subsystem: "color",
        min: 0.1,
        max: 10.0,
        default: 1.0,
        description: "Velocidad de adaptación de la exposición automática",
    },
    ParamDef {
        id: 34,
        name: "balance_blancos_k",
        subsystem: "color",
        min: 2000.0,
        max: 12000.0,
        default: 6500.0,
        description: "Temperatura de color del balance de blancos (Kelvin)",
    },
    // 14 Rayos del fractal
    ParamDef {
        id: 35,
        name: "rayos_foco",
        subsystem: "rayos",
        min: 0.1,
        max: 10.0,
        default: 1.0,
        description: "Concentración angular de los rayos crepusculares",
    },
    ParamDef {
        id: 36,
        name: "rayos_longitud",
        subsystem: "rayos",
        min: 0.1,
        max: 5.0,
        default: 1.0,
        description: "Alcance espacial de los haces crepusculares",
    },
    ParamDef {
        id: 37,
        name: "rayos_caida",
        subsystem: "rayos",
        min: 0.1,
        max: 5.0,
        default: 1.0,
        description: "Atenuación con la distancia de los haces",
    },
];

/// Busca un parámetro por su identificador numérico único.
pub fn find_param_by_id(id: u32) -> Option<&'static ParamDef> {
    PARAM_TABLE.iter().find(|p| p.id == id)
}

/// Busca un parámetro por su nombre textual.
pub fn find_param_by_name(name: &str) -> Option<&'static ParamDef> {
    PARAM_TABLE.iter().find(|p| p.name == name)
}
