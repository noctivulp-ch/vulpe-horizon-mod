# Decisiones de diseño — Mod nativo de Vulpe para Horizon (M-01)

Este documento registra las decisiones arquitectónicas y técnicas adoptadas en el primer corte del mod nativo (`vulpe-horizon-mod`).

## 1. Identificador del mod y anclaje a Nightly

- **ID del mod:** Se eligió `vulpe` (`[mod] id = "vulpe"` en `horizon-mixin.toml`). Esto coincide exactamente con el identificador del shader pack (`id = "vulpe"` en `vulpe-horizon/horizon-shaders.toml`). De este modo, la carpeta generada/empaquetada del pack en Horizon sigue la convención estándar `native-<id>` (`native-vulpe`), permitiendo que el cliente y el pack se reconozcan mutuamente sin desfases de nomenclatura.
- **Anclaje de versión (`horizon_revision`):** Se fijó en `[compatibility] horizon_revision = "fdc87aecdbd116bf1e08adcd062fcbaea8c2e2ca"`, correspondiente a la versión instalada `nightly-2026-11-08-fdc87ae`. Siguiendo la especificación de Horizon (`docs/native-mods.md`), este commit garantiza reproducibilidad estricta y compilación determinista.

## 2. Arquitectura Bundle y Crate Propio (`vulpe-engine`)

- **Estructura separada:** Siguiendo la documentación de Horizon, un mod nativo complejo divide su código entre:
  1. El *helper* inyectado (`shader_params.rs`), que es código pegamento compilado dentro de `horizon_client`.
  2. El crate del mod (`engine/`, paquete `vulpe-engine`), que se compila una sola vez como miembro del workspace y permanece en caché entre reconstrucciones incrementales.
- **Independencia de tipos en el crate:** `vulpe-engine` no depende de `horizon-client` (lo cual violaría las restricciones de Cargo y crearía ciclos de dependencias) ni de tipos internos del juego como `Game` o `NativeClock`. En su lugar, recibe tipos estándar de Rust (`Option<i64>`, `f32`, `&mut [[f32; 4]; 8]`). Esto permite:
  - Pruebas unitarias directas y aisladas con `cargo test`.
  - Cero dependencias externas en este primer corte.
  - Portabilidad y desacoplamiento limpio respecto al motor del juego.
- **Requisito de función única en el helper:** `horizon-mixins` exige de manera estricta que los archivos helper contengan **exactamente una función privada** (`ItemFn`), sin atributos (lo que prohíbe atributos de nivel de módulo como comentarios de documentación interna `//!`). Por tanto, `shader_params.rs` contiene una única función privada `vulpe_shader_params` que extrae `day_time` de `game.native_clock()` y delega el cálculo matemático al motor.

## 3. Formato `frame.mods` v1: Cabecera y Datos del Mundo

De acuerdo con `vulpe-horizon-spec/flexibilidad/mod-nativo.md` §5, se rellenan los 16 primeros floats (índices 0 a 3 de `[[f32; 4]; 8]`):

### `mods[0]` — Cabecera
- `x = 1001.0`: Firma mágica `1000 + versión`. Si vale 0.0 (o no finito), el shader sabe que no hay mod presente y usa sus valores por defecto.
- `y = 1.0`: Versión de la tabla unificada de parámetros (versión 1).
- `z = banderas`: Banderas binarias empaquetadas en un float entero exacto:
  - Bit 0 (`1`): Datos de tiempo válidos.
  - Bit 1 (`2`): Datos de rayo válidos (0 en este corte).
  - Bit 2 (`4`): Datos de estación válidos.
  - Bit 3 (`8`): Datos de estado del jugador válidos (0 en este corte).
  - Bit 4 (`16`): Canal fijo en uso (0 en este corte).
  - *Valor emitido en juego:* `5.0` (`1 | 4`) cuando hay un mundo cargado con reloj; `0.0` en pantalla de título o sin reloj.
- `w = 0.0`: Cantidad de ajustes en vivo activos en `mods[4..8]` (0 en este corte).

### `mods[1]` — Tiempo y Ciclos
- `x = día mod 2²⁴`: Día del mundo calculado mediante división euclídea `ticks.div_euclid(24_000)` y módulo `16_777_216`. Funciona de manera robusta incluso con valores negativos originados por comandos `/time set`.
- `y = fase de la luna`: `día.rem_euclid(8)` (0 a 7, donde 0 es luna llena, coincidiendo con la progresión de Minecraft).
- `z = estación continua`: Número en el rango `[0.0, 4.0)`:
  - `0.0`: Inicio de primavera.
  - `1.0`: Inicio de verano.
  - `2.0`: Inicio de otoño.
  - `3.0`: Inicio de invierno.
  - La parte decimal representa el avance continuo intradía, calculada con la duración por defecto de 24 días por estación (año de 96 días = 2 304 000 ticks), sumando el tick parcial (`partial`) interpolado.
- `w = 0.0`: Reservado para bioma futuro.

### `mods[2]` — Rayo (implementado en M-02, corregido en C-04)
- Coordenadas relativas a la cámara `[rel_x, rel_y, rel_z, edad]` capturadas combinando la lista de rayos activos del cliente (`game.bolts`, alimentada por `spawn_bolt` en un jugador y `spawn_remote_bolt` en servidores remotos) y el gancho `native_lightning_strike` (API 236), junto con la posición de cámara del fotograma actual (`game.view.position` / `native_camera()`). Si no hay rayo o si su edad supera los 10.0 segundos, emite `[0.0, 0.0, 0.0, -1.0]` y apaga el bit 1 de las banderas.

### `mods[3]` — Estado del Jugador (límite de API documentado)
- `[0.0, 0.0, 0.0, 0.0]`: La API cliente de Horizon (`native_hooks.rs` y `Game::native_player`) no expone salud, tiempo desde daño, fuego ni humedad en el hilo de renderizado (`shader_params`). Los ganchos de servidor (`native_acting_player`) operan en otro hilo y no aplican en clientes remotos sin mensajes de mod. Consecuentemente, el bit 3 (8) permanece en 0 y el shader descarta estos valores.

### `mods[4]` a `mods[7]` — Ajustes en vivo (implementado en M-02, conectado en C-04)
- Hasta 16 ajustes en vivo codificados como `id × 4096 + q`. La cabecera `mods[0].w` indica cuántos ajustes válidos viajan en cada fotograma.

---

## 4. Segundo corte (M-02) y corrección C-04: Rayos, Configuración y Canales

### 4.1 Diagnóstico y corrección de la captura del rayo y de la cámara (C-04)
En el segundo corte (`M-02`), el mod nunca encendía la bandera del rayo (`mods[0].z` permanecía en `5.0` y `mods[2].w` en `-1.0`) por dos causas combinadas:
1. **`game.native_camera()` devuelve `None` en bundles exclusivos de mixins:** En `crates/horizon-client/src/game.rs`, `self.native_camera_frame` únicamente se rellena cuando `!horizon_client::mod_api::mods().is_empty()` (es decir, cuando existe un crate de mod registrado mediante el trait `[api]`). Como `vulpe` se inyecta mediante `[[injection]]` de `horizon-mixins`, `game.native_camera()` devolvía siempre `None`. Al ser `camera_pos == None`, `populate_shader_params_full` abortaba siempre el cálculo del rayo relativo.
   - **Solución:** En `shader_params.rs` se obtiene la posición de la cámara de `game.view.position` (actualizada en `self.camera_view` antes de `shader_pack_inputs` en cada fotograma del mundo), manteniendo `game.native_camera().map(|c| c.position)` como primera opción y `day_time.map(|_| game.view.position)` cuando hay mundo cargado.
2. **Alcance del gancho `native_lightning_strike` frente a `game.bolts`:** El gancho `native_lightning_strike` (API 236) se ejecuta únicamente en el servidor autoritativo (`lightning_strike_allowed`) con coordenadas de bloque truncadas `[i32; 3]`, y no se dispara al jugar conectado a un servidor remoto (`spawn_remote_bolt`) ni con rayos visuales de caballos trampa. En cambio, tanto `spawn_bolt` (un jugador: `/summon lightning_bolt`, `/weather thunder`, tridentes, trampas) como `spawn_remote_bolt` (multijugador: paquete `AddEntity` de `minecraft:lightning_bolt`) insertan cada rayo en `game.bolts` con un identificador de entidad único `id: u32` y posición exacta `at: [f64; 3]`.
   - **Solución:** En `shader_params.rs` se observa `game.bolts.last()` y se invoca `vulpe_engine::observe_client_bolt(bolt.id, bolt.at)`, deduplicando por `bolt.id` para registrar el instante exacto del impacto (`edad = 0.0`) una sola vez por rayo y mantenerlo activo durante 10.0 segundos.

### 4.2 Límite arquitectónico del estado del jugador
- Se investigó exhaustivamente la superficie de API de Horizon (`docs/native-mods.md` y `crates/horizon-client/src/game/native_hooks.rs`). En el hilo cliente, `native_player()` únicamente provee `(x, y, z, yaw, pitch, dimension)`. El helper `native_health_hud_state()` manipula la geometría del HUD pero no expone la salud numérica ni los efectos de fuego/agua de forma estable para el pipeline de shaders. De acuerdo con las instrucciones de la tarea, esta limitación se documenta formalmente y se mantiene el vector en `[0.0, 0.0, 0.0, 0.0]` con su bandera desactivada.

### 4.3 Tabla de parámetros v1 y configuración en TOML
- **Tabla unificada:** Definida en `engine/src/table.rs`, asigna identificadores inmutables de 0 a 4095 a los 38 parámetros finos de Vulpe distribuidos en los subsistemas del mapa de ajustes (`cielo`, `fractal`, `luz_superficie`, `sombras`, `niebla_nubes`, `aurora`, `agua`, `color`, `rayos`).
- **Archivo de configuración (`config/vulpe-horizon.toml`):** Se implementó un analizador y generador TOML nativo sin dependencias externas en `engine/src/config.rs`. Es tolerante a claves ausentes (completando con valores por defecto), acota los valores a los rangos válidos del parámetro y genera comentarios explicativos legibles por el usuario si el archivo no existe.

### 4.4 Activación real en el juego del canal fijo y de los 16 ajustes en vivo (C-04)
En `M-02`, las funciones `VulpeConfig::load_or_create`, `write_pack_constants`, `set_fixed_channel_active` y `set_live_adjustment` existían en `vulpe-engine` pero nunca eran invocadas desde `shader_params.rs` ni desde ningún otro gancho del juego. Además, el cargador de packs de Horizon (`crates/horizon-shader-api/src/package.rs`) rechaza cualquier archivo `.wgsl` en la carpeta del pack que no esté listado en `horizon-shaders.toml` (`shared` o `files`).
- **Sincronización en tiempo de ejecución (`sync_runtime_state`):** Desde `shader_params.rs` se consulta cada 500 ms (o al cambiar de pack) la carpeta de configuración `Game::native_config_dir()` (API 26) y la ruta del pack activo (`game.native_shaders.picked`):
  1. En el primer arranque crea automáticamente `config/vulpe-horizon.toml` con los 38 valores por defecto. Mientras todos los parámetros conserven su valor por defecto, el mod permanece en reposo (`mods[0].z = 5.0` sin rayo, `mods[0].w = 0.0`).
  2. Cuando el jugador edita `config/vulpe-horizon.toml` o usa el comando `/vulpe`:
     - **Canal en vivo (`mods[0].w` y `mods[4..7]`):** Los parámetros modificados respecto a su valor por defecto (hasta los primeros 16 por orden de tabla) se codifican inmediatamente como `id × 4096 + q` en `frame.mods[4..7]`, y `frame.mods[0].w` refleja su cantidad (`1.0` a `16.0`).
     - **Canal fijo (`constantes_mod.wgsl` y Bit 4 en `mods[0].z`):** Si el pack activo reside en una carpeta escribible, `write_pack_constants` escribe `constantes_mod.wgsl`, asegura que `"constantes_mod.wgsl"` figure en la lista `shared` de `horizon-shaders.toml` del pack para que el validador de Horizon lo compile y recargue en caliente en 500 ms, y enciende el Bit 4 (`+16.0`) en `frame.mods[0].z` (`21.0` en reposo o `23.0` con rayo activo). Si el pack es un archivo comprimido `.hshader`, el Bit 4 permanece apagado y los cambios actúan por el canal en vivo.
- **Comandos del cliente `/vulpe` (API 35: `native_commands` y `native_command`):** Se inyectaron los ayudantes `commands.rs` y `command.rs` para permitir inspeccionar y activar ambos canales directamente desde el chat del juego (`/vulpe estado`, `/vulpe ver <parámetro>`, `/vulpe poner <parámetro> <valor>`, `/vulpe vivo <parámetro> <valor>`, `/vulpe restablecer`).

---

## 5. Canal fijo exclusivamente con la Forma C (C-05 y C-06)

En `C-05` se construyeron y compararon en el juego dos formas para el canal fijo:
- **Forma B (archivo de serie):** escribía `constantes_mod.wgsl` directamente dentro de la carpeta del pack del jugador sin modificar `horizon-shaders.toml`.
- **Forma C (`shaderpacks/vulpe-mod/`):** operaba sobre una carpeta propia gestionada por el mod (`shaderpacks/vulpe-mod/`, seleccionada como `horizon:vulpe_mod`).

### 5.1 Por qué se descarta la Forma B y se adopta solo la Forma C (C-06)
Tras probar ambas formas en el juego real (con el pack elegido en la interfaz y sin `HORIZON_SHADER_PACK`):
1. **La Forma B dependía del nombre de la carpeta:** cuando la carpeta del pack se llamaba `shaderpacks/vulpe-carpeta/`, el juego guardó el pack elegido como `horizon:vulpe` (el `id` del manifiesto `horizon-shaders.toml`, no el nombre de la carpeta). Al buscar la carpeta asumiendo que su nombre coincidía con el `id`, `/vulpe poner` no encontraba el directorio, dejaba `canal_fijo = false` (banderas `5.0`) y las constantes de serie sin cambiar.
2. **La Forma B tocaba el pack del jugador:** sobrescribía archivos dentro de la carpeta original del usuario en vez de mantener el pack original intacto.
3. **La Forma C funcionó a la primera con el `.hshader`:** desempaquetó en `shaderpacks/vulpe-mod/`, cambió el pack activo a `horizon:vulpe_mod`, escribió `constantes_mod.wgsl`, Horizon recargó automáticamente en caliente y el cielo pasó de `42/255` a `96/255` con `cielo_brillo_manual = 2.5` (banderas `21.0`).

Por decisión del gestor, **se elimina la Forma B** (su opción y su código) y se usa **únicamente la Forma C** para cualquier pack (`canal_fijo = "c" | "apagado"`, por defecto `"c"`):
- **Si el pack elegido es un `.hshader`:** se descomprime en `shaderpacks/vulpe-mod/`.
- **Si el pack elegido es una carpeta (como `shaderpacks/vulpe-carpeta/`):** se **copia** a `shaderpacks/vulpe-mod/` sin tocar nunca la carpeta original del jugador.
- **En ambos casos:** el mod asigna `id = "vulpe_mod"` en `shaderpacks/vulpe-mod/horizon-shaders.toml`, selecciona `horizon:vulpe_mod` como pack activo en el cliente y escribe allí `constantes_mod.wgsl`.
- **Resolución del pack elegido por manifiesto (`resolve_shader_pack_in_dir`):** para resolver `horizon:<id>` al archivo `.hshader` o carpeta real, el mod recorre `shaderpacks/` y lee el `id` declarado en el `horizon-shaders.toml` de cada carpeta o archivo `.hshader` (en vez de suponer que el nombre del archivo o carpeta coincide con el `id`).
- **Huella y regeneración automática (`.vulpe-hshader-stamp`):** el mod calcula una huella tanto cuando el original es un `.hshader` (nombre, tamaño y hash de contenido) como cuando es una carpeta (nombre y hash recursivo de sus archivos). Si el `.hshader` o la carpeta original cambian en disco, `shaderpacks/vulpe-mod/` se regenera automáticamente y se vuelve a escribir `constantes_mod.wgsl`.
- **Cómo se selecciona el pack activo desde el mod (`command.rs` y `frame.rs`):**
  1. En la copia `shaderpacks/vulpe-mod/horizon-shaders.toml`, el mod asigna `id = "vulpe_mod"` para evitar colisiones con el pack original (`id = "vulpe"`).
  2. Actualiza `game.settings.horizon.shader_pack = "horizon:vulpe_mod"`, persiste la elección con `game.settings.save(&Game::options_path())` y llama a `game.native_shaders.select(...)` apuntando a `shaderpacks/vulpe-mod`.
  3. Si el jugador cambia `canal_fijo = "apagado"` (o ejecuta `/vulpe modo apagado`), el canal fijo se desactiva (`mods[0].z = 5.0`) y, si estaba en `vulpe-mod`, vuelve a seleccionar el pack original `horizon:vulpe`.
