# Vulpe para Horizon — mod nativo

Mod nativo de [Horizon](https://github.com/horizon-rust/horizon) que acompaña al shader
[vulpe-horizon](https://github.com/noctivulp-ch/vulpe-horizon). Le pasa al pack, por `frame.mods` (`array<vec4<f32>, 8>`),
el ajuste fino que no cabe en las 64 opciones del menú de Horizon y datos del mundo que la API básica de shaders no proporciona.

Este repositorio contiene la implementación del **primer corte (M-01)**:
- Estructura bundle y crate del mod según `docs/native-mods.md` de Horizon.
- Gancho `native_shader_params` (API 84) conectado a `frame.mods`.
- Formato v1 de `frame.mods`: cabecera (firma, tabla, banderas) y datos del mundo (día, fase de luna, estación continua).
- Pruebas unitarias exhaustivas del motor de cálculo en `engine/`.

Licencia: GPL-3.0-or-later ([LICENSE](LICENSE)).

---

## Estructura del repositorio

```
vulpe-horizon-mod/
  ├── horizon-mixin.toml     # Manifiesto del bundle para horizon-mixins (id = "vulpe")
  ├── shader_params.rs       # Helper privado inyectado en native_shader_params
  ├── engine/                # Crate propio del mod (miembro del workspace de Horizon)
  │   ├── Cargo.toml         # Paquete vulpe-engine (edición 2024, sin dependencias externas)
  │   └── src/lib.rs         # Motor de formato v1 y pruebas unitarias
  ├── docs/
  │   └── decisiones.md      # Registro detallado de decisiones de diseño
  └── README.md
```

Las decisiones técnicas y el diseño detallado se encuentran explicados en [docs/decisiones.md](docs/decisiones.md).

---

## Cómo compilar e instalar

La compilación se realiza a través de `horizon-launcher` fijado al commit de la versión instalada (`nightly-2026-11-08-fdc87ae`):

### 1. Pruebas unitarias del crate
Se pueden ejecutar las pruebas del motor de cálculo directamente con Cargo:
```bash
cd engine
cargo test
```

### 2. Compilación del mod nativo con Horizon Launcher
```bash
horizon-launcher native-build \
  --trust-native-code \
  <ruta-del-repo>
```

Opcionalmente, se puede especificar el perfil de compilación:
- `--profile performance` (por defecto): compilación incremental optimizada para jugar.
- `--profile dev`: iteración más rápida de desarrollo.
- `--profile release`: optimización máxima con LTO delgado.

### 3. Dónde queda el artefacto
`horizon-launcher native-build` compila el juego con el mod integrado en el directorio de datos del launcher:
- **Caché y artefactos de compilación:** `~/.local/share/horizon-launcher/native/target/`
- **Juego instalado para la instancia:** `~/.local/share/horizon-launcher/versions/native-<id>/`
- La instancia seleccionada queda automáticamente configurada para jugar esta versión construida.

---

## Cómo comprobar `frame.mods` en el juego

El gestor puede verificar que el mod está activo e inyectando datos correctamente inspeccionando los componentes de `frame.mods` en el shader.

### Valores esperados de `frame.mods` (Formato v1)

#### En la pantalla de título (sin mundo cargado)
- `frame.mods[0] = vec4(1001.0, 1.0, 0.0, 0.0)`
  - `x = 1001.0`: Firma de formato v1 presente.
  - `y = 1.0`: Versión de tabla de parámetros 1.
  - `z = 0.0`: Banderas en 0 (sin reloj disponible).
  - `w = 0.0`: 0 ajustes en vivo en uso.
- `frame.mods[1] = vec4(0.0, 0.0, 0.0, 0.0)`
- `frame.mods[2] = vec4(0.0, 0.0, 0.0, -1.0)` (edad negativa = sin rayo).

#### En un mundo cargado según el día y la hora

| Escenario | Comando | `frame.mods[0]` (Cabecera) | `frame.mods[1]` (Tiempo y Estación) |
|---|---|---|---|
| **Día 0, amanecer** | `/time set 0` | `(1001.0, 1.0, 5.0, 0.0)` | `(0.0, 0.0, 0.000, 0.0)` — Día 0, Luna Llena (0), Inicio Primavera |
| **Día 0, noche** | `/time set 18000` | `(1001.0, 1.0, 5.0, 0.0)` | `(0.0, 0.0, 0.031, 0.0)` — Día 0, Luna Llena (0), Primavera avanzando |
| **Día 1, amanecer** | `/time set 24000` | `(1001.0, 1.0, 5.0, 0.0)` | `(1.0, 1.0, 0.042, 0.0)` — Día 1, Gibosa Menguante (1) |
| **Día 4, amanecer** | `/time set 96000` | `(1001.0, 1.0, 5.0, 0.0)` | `(4.0, 4.0, 0.167, 0.0)` — Día 4, Luna Nueva (4) |
| **Día 24, amanecer** | `/time set 576000` | `(1001.0, 1.0, 5.0, 0.0)` | `(24.0, 0.0, 1.000, 0.0)` — Día 24, Luna Llena (0), Inicio Verano (1.0) |
| **Día 48, amanecer** | `/time set 1152000` | `(1001.0, 1.0, 5.0, 0.0)` | `(48.0, 0.0, 2.000, 0.0)` — Día 48, Luna Llena (0), Inicio Otoño (2.0) |
| **Día 72, amanecer** | `/time set 1728000` | `(1001.0, 1.0, 5.0, 0.0)` | `(72.0, 0.0, 3.000, 0.0)` — Día 72, Luna Llena (0), Inicio Invierno (3.0) |
| **Día 96, amanecer** | `/time set 2304000` | `(1001.0, 1.0, 5.0, 0.0)` | `(96.0, 0.0, 0.000, 0.0)` — Día 96, Ciclo completo, Vuelve a Primavera (0.0) |

- **Banderas (`mods[0].z = 5.0`):** `1` (Bit 0: datos de tiempo válidos) + `4` (Bit 2: estación válida) = `5.0`.
- **Rayo (`mods[2]`):** `vec4(0.0, 0.0, 0.0, -1.0)` constante en este corte.
- **Jugador (`mods[3]`):** `vec4(0.0, 0.0, 0.0, 0.0)` con bandera en 0 (el shader no lo lee).
- **Ajustes en vivo (`mods[4..7]`):** Todos en `0.0`.
