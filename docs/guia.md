# Guía técnica del mod nativo

Mod nativo de [Horizon](https://github.com/horizon-rust/horizon) que acompaña al shader
[vulpe-horizon](https://github.com/noctivulp-ch/vulpe-horizon). Le pasa al pack, por `frame.mods` (`array<vec4<f32>, 8>`),
el ajuste fino que no cabe en las 64 opciones del menú de Horizon y datos del mundo que la API básica de shaders no proporciona.

Este repositorio contiene la implementación del **segundo corte (M-02)**:
- Estructura bundle y crate del mod según `docs/native-mods.md` de Horizon.
- Ganchos `native_shader_params` (API 84) y `native_lightning_strike` (API 236).
- Formato v1 de `frame.mods`: cabecera (firma, tabla, banderas, contador en vivo), tiempo/estaciones y rayo relativo a la cámara.
- Configuración del jugador mediante archivo TOML (`config/vulpe-horizon.toml`).
- Codificación de hasta 16 ajustes en vivo simultáneos (`id × 4096 + q`).
- Segundo canal (fijo) mediante generación de `constantes_mod.wgsl` para recarga en caliente (hot reload cada 500 ms) en carpetas de pack, con fallback para `.hshader`.
- Pruebas unitarias e integración exhaustivas en `engine/`.

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
  - `w = 0.0`: 0 ajustes en vivo en uso (o el número de ajustes activos si se previsualizan).
- `frame.mods[1] = vec4(0.0, 0.0, 0.0, 0.0)`
- `frame.mods[2] = vec4(0.0, 0.0, 0.0, -1.0)` (edad negativa = sin rayo).

#### En un mundo cargado según el día, la hora y el clima

| Escenario | Comando | `frame.mods[0]` (Cabecera) | `frame.mods[1]` (Tiempo y Estación) | `frame.mods[2]` (Rayo) |
|---|---|---|---|---|
| **Día 0, amanecer** | `/time set 0` | `(1001.0, 1.0, 5.0, 0.0)` | `(0.0, 0.0, 0.000, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Día 0, noche** | `/time set 18000` | `(1001.0, 1.0, 5.0, 0.0)` | `(0.0, 0.0, 0.031, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Día 1, amanecer** | `/time set 24000` | `(1001.0, 1.0, 5.0, 0.0)` | `(1.0, 1.0, 0.042, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Rayo caído cerca** | `/summon lightning_bolt ~10 ~ ~20` o `/weather thunder` | `(1001.0, 1.0, 7.0, 0.0)` | `(día, luna, estación, 0.0)` | `(rel_x, rel_y, rel_z, edad)` |
| **Con canal fijo activo** | Al escribir `constantes_mod.wgsl` | `(1001.0, 1.0, 21.0 ó 23.0, N)` | `(día, luna, estación, 0.0)` | Según presencia de rayo |

#### Desglose de componentes:
- **Banderas (`mods[0].z`):** suma binaria de enteros exactos:
  - `+1.0` (Bit 0): Datos de tiempo válidos.
  - `+2.0` (Bit 1): Datos de rayo válidos (activo durante 10 segundos tras el impacto).
  - `+4.0` (Bit 2): Datos de estación válidos.
  - `+8.0` (Bit 3): Estado de jugador válido (0.0 en este corte: límite de API cliente).
  - `+16.0` (Bit 4): Canal fijo en uso (constantes WGSL presentes).
  - *Valores comunes:* `5.0` (tiempo + estación), `7.0` (tiempo + estación + rayo), `21.0` (tiempo + estación + canal fijo), `23.0` (tiempo + estación + rayo + canal fijo).
- **Ajustes en vivo (`mods[0].w`):** número de ajustes finos activos en `mods[4..7]` (de `0.0` a `16.0`).
- **Rayo (`mods[2]`):**
  - `x, y, z`: posición espacial en bloques **relativa a la cámara** `(pos_rayo - pos_camara)`.
  - `w`: edad en segundos desde el impacto (`0.0` a `10.0` s mientras esté activo; `-1.0` si no hay rayo o expiró).
- **Jugador (`mods[3]`):** `vec4(0.0, 0.0, 0.0, 0.0)` con bandera en 0 (límite de lectura de salud/efectos en cliente).
- **Ajustes en vivo (`mods[4..7]`):** hasta 16 slots con fórmula `id × 4096 + q`. Los slots que excedan `mods[0].w` valen `0.0`.

---

## Cómo provocar y comprobar el rayo en el juego

1. Entra a un mundo en Horizon con el shader Vulpe activo.
2. Abre la consola del chat y ejecuta:
   ```mcfunction
   /summon lightning_bolt ~10 ~ ~20
   ```
   (o bien `/weather thunder` y espera a que caiga un relámpago natural).
3. **Comprobación inmediata:**
   - La cabecera `frame.mods[0].z` pasa de `5.0` a `7.0` (enciende el bit 1 de rayo válido).
   - `frame.mods[2].xyz` muestra la posición del rayo relativa a tus ojos (por ejemplo `(10.0, 0.0, 20.0)` si invocaste a `~10 ~ ~20`).
   - `frame.mods[2].w` comienza en `0.0` y va aumentando con el tiempo transcurrido en segundos.
   - Tras 10 segundos, `frame.mods[2].w` vuelve a `-1.0` y la bandera `frame.mods[0].z` regresa a `5.0`.
