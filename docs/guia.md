# Guía técnica del mod nativo

Mod nativo de [Horizon](https://github.com/horizon-rust/horizon) que acompaña al shader
[vulpe-horizon](https://github.com/noctivulp-ch/vulpe-horizon). Le pasa al pack, por `frame.mods` (`array<vec4<f32>, 8>`),
el ajuste fino que no cabe en las 64 opciones del menú de Horizon y datos del mundo que la API básica de shaders no proporciona.

Este repositorio contiene la implementación del **segundo corte (M-02)** y las **correcciones en vivo (C-04)**:
- Estructura bundle y crate del mod según `docs/native-mods.md` de Horizon.
- Ganchos `native_shader_params` (API 84), `native_lightning_strike` (API 236) y comandos del cliente `native_commands` / `native_command` (API 35).
- Formato v1 de `frame.mods`: cabecera (firma, tabla, banderas, contador en vivo), tiempo/estaciones y rayo relativo a la cámara (`game.view.position` + `game.bolts`).
- Configuración del jugador mediante archivo TOML (`config/vulpe-horizon.toml`), sincronizada automáticamente cada 500 ms y mediante comandos `/vulpe`.
- Codificación de hasta 16 ajustes en vivo simultáneos (`id × 4096 + q`) en `frame.mods[4..7]`.
- Segundo canal (fijo) mediante generación de `constantes_mod.wgsl` y registro en `shared` de `horizon-shaders.toml` para recarga en caliente (hot reload cada 500 ms) en carpetas de pack, con fallback para `.hshader`.
- Pruebas unitarias e integración exhaustivas en `engine/`.

Licencia: GPL-3.0-or-later ([LICENSE](LICENSE)).

---

## Estructura del repositorio

```
vulpe-horizon-mod/
  ├── horizon-mixin.toml     # Manifiesto del bundle para horizon-mixins (id = "vulpe")
  ├── shader_params.rs       # Helper privado inyectado en native_shader_params
  ├── lightning_strike.rs    # Helper privado inyectado en native_lightning_strike
  ├── commands.rs            # Helper privado inyectado en native_commands (API 35)
  ├── command.rs             # Helper privado inyectado en native_command (API 35)
  ├── frame.rs               # Helper privado inyectado en native_frame (selección de pack en Forma C)
  ├── engine/                # Crate propio del mod (miembro del workspace de Horizon)
  │   ├── Cargo.toml         # Paquete vulpe-engine (edición 2024, sin dependencias externas)
  │   └── src/               # Motor de formato v1, tabla, configuración, constantes y pruebas
  ├── docs/
  │   ├── decisiones.md      # Registro detallado de decisiones de diseño
  │   └── guia.md            # Guía técnica y de verificación
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
`horizon-launcher native-build` compila el juego con el mod integrado en el directorio de datos del lanzador:
- **Caché y artefactos de compilación:** `<datos-del-lanzador>/native/target/`
- **Juego instalado para la instancia:** `<datos-del-lanzador>/versions/native-<id>/`
- La instancia seleccionada queda automáticamente configurada para jugar esta versión construida.

---

## Cómo comprobar `frame.mods` en el juego

El gestor puede verificar que el mod está activo e inyectando datos correctamente inspeccionando los componentes de `frame.mods` en el shader o ejecutando `/vulpe estado` en el chat.

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

| Escenario | Comando / Acción | `frame.mods[0]` (Cabecera) | `frame.mods[1]` (Tiempo y Estación) | `frame.mods[2]` (Rayo) |
|---|---|---|---|---|
| **En reposo (día 0, amanecer)** | `/time set 0` | `(1001.0, 1.0, 5.0, 0.0)` | `(0.0, 0.0, 0.000, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Día 0, noche** | `/time set 18000` | `(1001.0, 1.0, 5.0, 0.0)` | `(0.0, 0.0, 0.031, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Día 1, amanecer** | `/time set 24000` | `(1001.0, 1.0, 5.0, 0.0)` | `(1.0, 1.0, 0.042, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Rayo activo (< 10 s)** | `/summon lightning_bolt 180 90 186` o `/weather thunder` | `(1001.0, 1.0, 7.0, 0.0)` | `(día, luna, estación, 0.0)` | `(180 - cam_x, 90 - cam_y, 186 - cam_z, edad)` |
| **Rayo expirado (≥ 10 s)** | A los 14 s tras el rayo | `(1001.0, 1.0, 5.0, 0.0)` | `(día, luna, estación, 0.0)` | `(0.0, 0.0, 0.0, -1.0)` |
| **Canal fijo + ajuste en vivo** | `/vulpe poner cielo_brillo_manual 2.5` o editar `config/vulpe-horizon.toml` | `(1001.0, 1.0, 21.0, 1.0)` | `(día, luna, estación, 0.0)` | Según presencia de rayo (`23.0` si hay rayo) |

#### Desglose de componentes:
- **Banderas (`mods[0].z`):** suma binaria de enteros exactos:
  - `+1.0` (Bit 0): Datos de tiempo válidos.
  - `+2.0` (Bit 1): Datos de rayo válidos (activo durante 10 segundos tras el impacto).
  - `+4.0` (Bit 2): Datos de estación válidos.
  - `+8.0` (Bit 3): Estado de jugador válido (0.0 en este corte: límite de API cliente).
  - `+16.0` (Bit 4): Canal fijo en uso (`constantes_mod.wgsl` escrito en la carpeta del pack).
  - *Valores comunes:* `5.0` (tiempo + estación), `7.0` (tiempo + estación + rayo), `21.0` (tiempo + estación + canal fijo), `23.0` (tiempo + estación + rayo + canal fijo).
- **Ajustes en vivo (`mods[0].w`):** número de ajustes finos activos en `mods[4..7]` (de `0.0` a `16.0`).
- **Rayo (`mods[2]`):**
  - `x, y, z`: posición espacial en bloques **relativa a la cámara** `(pos_rayo - pos_camara)`.
  - `w`: edad en segundos desde el impacto (`0.0` a `10.0` s mientras esté activo; `-1.0` si no hay rayo o expiró).
- **Jugador (`mods[3]`):** `vec4(0.0, 0.0, 0.0, 0.0)` con bandera en 0 (límite de lectura de salud/efectos en cliente).
- **Ajustes en vivo (`mods[4..7]`):** hasta 16 slots con fórmula `id × 4096 + q`. Los slots que excedan `mods[0].w` valen `0.0`.

---

## 1. Cómo provocar y comprobar el rayo en el juego

1. Entra a un mundo en Horizon con el pack activo.
2. En reposo (sin rayo reciente y con la configuración por defecto):
   - `frame.mods[0].z = 5.0`
   - `frame.mods[2] = (0.0, 0.0, 0.0, -1.0)`
3. Abre el chat y ejecuta:
   ```mcfunction
   /summon lightning_bolt 180 90 186
   ```
   (o bien `/summon lightning_bolt ~10 ~ ~20`, o `/weather thunder`; nota: con `/weather thunder` el motor de Minecraft sortea `1/100 000` por chunk activo y tick, por lo que los rayos naturales caen de forma estocástica cada ~20-30 s de media, mientras que `/summon lightning_bolt` es instantáneo y determinista).
4. **Valores esperados:**
   - **Entre `0.0 s` y `10.0 s` (p. ej. a `1.5 s`):**
     - `frame.mods[0].z = 7.0` (o `23.0` si el canal fijo también está activo).
     - `frame.mods[2].xyz = (180.0 - cam_x, 90.0 - cam_y, 186.0 - cam_z)`.
     - `frame.mods[2].w` empieza en `0.0` y avanza en segundos (`~1.5` al cabo de `1.5 s`).
   - **Pasados `10.0 s` (p. ej. a `14.0 s`):**
     - `frame.mods[0].z` regresa a `5.0` (o `21.0` con canal fijo).
     - `frame.mods[2]` regresa a `(0.0, 0.0, 0.0, -1.0)`.

---

## 2. Cómo activar y comprobar el canal fijo (`constantes_mod.wgsl`, Forma C)

En `config/vulpe-horizon.toml` (o con `/vulpe modo <c|apagado>`) se controla el canal fijo:
- `canal_fijo = "c"` (**Por defecto, Forma C**): si el pack elegido en `shaderpacks/` es un archivo `.hshader`, lo descomprime en `shaderpacks/vulpe-mod/`; si es una carpeta (por ejemplo `shaderpacks/vulpe-carpeta/`), la **copia** a `shaderpacks/vulpe-mod/` sin tocar el pack original del jugador. En ambos casos selecciona `shaderpacks/vulpe-mod/` (`horizon:vulpe_mod`) como pack activo y escribe allí `constantes_mod.wgsl`. Si el `.hshader` o la carpeta original cambian en disco, `shaderpacks/vulpe-mod/` se regenera automáticamente.
- `canal_fijo = "apagado"`: desactiva el canal fijo (`mods[0].z = 5.0`) y deja solo los ajustes en vivo (`mods[4..7]`).

### Comprobación con el pack en carpeta (p. ej. `shaderpacks/vulpe-carpeta/`):
1. Coloca la carpeta del pack en `shaderpacks/vulpe-carpeta/` (con `id = "vulpe"` en su `horizon-shaders.toml`) y elígela en el menú de shaders del juego (queda guardada como `horizon:vulpe`).
2. Ejecuta en el chat:
   ```mcfunction
   /vulpe poner cielo_brillo_manual 2.5
   ```
3. **Qué ocurre y valores esperados:**
   - El mod resuelve `horizon:vulpe` leyendo los manifiestos de `shaderpacks/`, encuentra `shaderpacks/vulpe-carpeta/` y la **copia** a `shaderpacks/vulpe-mod/` con huella `.vulpe-hshader-stamp`.
   - La carpeta original `shaderpacks/vulpe-carpeta/` queda intacta.
   - Se escribe `shaderpacks/vulpe-mod/constantes_mod.wgsl` con `VULPE_FIXED_CHANNEL_ACTIVE = true` y `VULPE_CIELO_BRILLO_MANUAL = 2.50000`, y el juego cambia el pack activo a `horizon:vulpe_mod`.
   - `frame.mods[0].z` vale **`21.0`** (`1` tiempo + `4` estación + `16` canal fijo), o **`23.0`** con rayo activo.
   - Si modificas un archivo dentro de `shaderpacks/vulpe-carpeta/`, en el siguiente ciclo de sincronización (≤ 500 ms) `shaderpacks/vulpe-mod/` se regenera automáticamente y conserva `constantes_mod.wgsl`.

### Comprobación con el archivo `.hshader` de release:
1. Con el archivo `.hshader` en `shaderpacks/` seleccionado como pack activo, ejecuta en el chat:
   ```mcfunction
   /vulpe poner cielo_brillo_manual 2.5
   ```
2. **Qué archivo aparece y valores esperados:**
   - Aparece la carpeta desempaquetada `shaderpacks/vulpe-mod/` con la huella `.vulpe-hshader-stamp` y el archivo `shaderpacks/vulpe-mod/constantes_mod.wgsl` (`VULPE_FIXED_CHANNEL_ACTIVE = true`, `VULPE_CIELO_BRILLO_MANUAL = 2.50000`).
   - El mod selecciona automáticamente `shaderpacks/vulpe-mod/` (`horizon:vulpe_mod`) como pack activo en Horizon.
   - `frame.mods[0].z` vale **`21.0`** (`1` tiempo + `4` estación + `16` canal fijo), o **`23.0`** con rayo activo.
   - Si se reemplaza o actualiza el archivo `.hshader` en `shaderpacks/`, en el siguiente ciclo de sincronización (≤ 500 ms) `shaderpacks/vulpe-mod/` se regenera automáticamente conservando `constantes_mod.wgsl` actualizado.

### Editando `config/vulpe-horizon.toml` en disco:
1. Abre `config/vulpe-horizon.toml` en la carpeta `config/` de la instancia, verifica `canal_fijo = "c"` (o `"apagado"`) y cambia por ejemplo `cielo_brillo_manual = 2.5000`.
2. Guarda el archivo: en menos de 500 ms el mod detecta el cambio, actualiza `shaderpacks/vulpe-mod/constantes_mod.wgsl` y `frame.mods[0].z` pasa a **`21.0`**.

---

## 3. Cómo activar y comprobar los 16 ajustes en vivo (`frame.mods[4..7]`)

Cada ajuste en vivo se codifica como un float exacto `id × 4096 + q`, donde `q = round(((valor - min) / (max - min)) * 4095)`.

### Comprobación de 1 ajuste en vivo:
1. Ejecuta en el chat:
   ```mcfunction
   /vulpe vivo cielo_brillo_manual 2.5
   ```
   *(Nota: `/vulpe vivo` activa únicamente el canal en vivo dejando `mods[0].z = 5.0`; `/vulpe poner` o editar `config/vulpe-horizon.toml` activa a la vez el canal fijo `mods[0].z = 21.0` y el canal en vivo).*
2. **Valores esperados:**
   - `cielo_brillo_manual` tiene `id = 0`, `min = 0.0`, `max = 5.0` → para `2.5`, `t = 0.5`, `q = 2048` → código = `0 × 4096 + 2048 = 2048.0`.
   - `frame.mods[0].w = 1.0` (1 ajuste en vivo activo).
   - `frame.mods[4].x = 2048.0`, y el resto de componentes de `mods[4..7]` en `0.0`.

### Comprobación de varios ajustes o del tope de 16 ajustes en vivo:
- Si añades un segundo ajuste, por ejemplo:
  ```mcfunction
  /vulpe vivo sol_intensidad 2.5
  ```
  (`sol_intensidad` tiene `id = 14`, `min = 0.0`, `max = 5.0` → `q = 2048` → código = `14 × 4096 + 2048 = 59392.0`):
  - `frame.mods[0].w = 2.0`
  - `frame.mods[4].x = 2048.0`
  - `frame.mods[4].y = 59392.0`
- Si modificas 16 o más parámetros en `config/vulpe-horizon.toml` (o con `/vulpe poner` / `/vulpe vivo`), `frame.mods[0].w` llega al tope de **`16.0`** y las 16 componentes de `frame.mods[4]`, `frame.mods[5]`, `frame.mods[6]` y `frame.mods[7]` contienen los primeros 16 parámetros modificados codificados como `id × 4096 + q`.

### Cómo restablecer el estado de reposo:
Ejecuta en el chat:
```mcfunction
/vulpe restablecer
```
- `frame.mods[0]` vuelve a `(1001.0, 1.0, 5.0, 0.0)` y `frame.mods[4..7]` vuelve a `0.0`.
