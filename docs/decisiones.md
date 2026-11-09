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

### `mods[2]` — Rayo (implementado en M-02)
- Coordenadas relativas a la cámara `[rel_x, rel_y, rel_z, edad]` capturadas mediante el gancho `native_lightning_strike` (API 236) y la cámara del render thread `native_camera()`. Si no hay rayo o si su edad supera los 10.0 segundos, emite `[0.0, 0.0, 0.0, -1.0]` y apaga el bit 1 de las banderas.

### `mods[3]` — Estado del Jugador (límite de API documentado)
- `[0.0, 0.0, 0.0, 0.0]`: La API cliente de Horizon (`native_hooks.rs` y `Game::native_player`) no expone salud, tiempo desde daño, fuego ni humedad en el hilo de renderizado (`shader_params`). Los ganchos de servidor (`native_acting_player`) operan en otro hilo y no aplican en clientes remotos sin mensajes de mod. Consecuentemente, el bit 3 (8) permanece en 0 y el shader descarta estos valores.

### `mods[4]` a `mods[7]` — Ajustes en vivo (implementado en M-02)
- Hasta 16 ajustes en vivo codificados como `id × 4096 + q`. La cabecera `mods[0].w` indica cuántos ajustes válidos viajan en cada fotograma.

---

## 4. Segundo corte (M-02): Ajustes finos, Configuración y Canales

### 4.1 Captura del rayo y relatividad a la cámara
- **Gancho del servidor integrado (`native_lightning_strike`, API 236):** Escucha la creación de rayos por clima (`/weather thunder`), tridentes de canalización o comandos (`/summon lightning_bolt`), almacenando la posición del impacto y la estampa temporal en un mutex thread-safe en memoria.
- **Relatividad espacial:** En cada fotograma de `shader_params` se consulta `game.native_camera()`. Si hay un rayo registrado con menos de 10 segundos de antigüedad, se calculan las coordenadas relativas `(pos_rayo - pos_camara)` y se suministra la edad en segundos, encendiendo el Bit 1 (`2.0`) en las banderas de la cabecera.

### 4.2 Límite arquitectónico del estado del jugador
- Se investigó exhaustivamente la superficie de API de Horizon (`docs/native-mods.md` y `crates/horizon-client/src/game/native_hooks.rs`). En el hilo cliente, `native_player()` únicamente provee `(x, y, z, yaw, pitch, dimension)`. El helper `native_health_hud_state()` manipula la geometría del HUD pero no expone la salud numérica ni los efectos de fuego/agua de forma estable para el pipeline de shaders. De acuerdo con las instrucciones de la tarea, esta limitación se documenta formalmente y se mantiene el vector en `[0.0, 0.0, 0.0, 0.0]` con su bandera desactivada.

### 4.3 Tabla de parámetros v1 y configuración en TOML
- **Tabla unificada:** Definida en `engine/src/table.rs`, asigna identificadores inmutables de 0 a 4095 a los 38 parámetros finos de Vulpe distribuidos en los subsistemas del mapa de ajustes (`cielo`, `fractal`, `luz_superficie`, `sombras`, `niebla_nubes`, `aurora`, `agua`, `color`, `rayos`).
- **Archivo de configuración (`config/vulpe-horizon.toml`):** Se implementó un analizador y generador TOML nativo sin dependencias externas en `engine/src/config.rs`. Es tolerante a claves ausentes (completando con valores por defecto), acota los valores a los rangos válidos del parámetro y genera comentarios explicativos legibles por el usuario si el archivo no existe.

### 4.4 Canal en vivo: codificación `id × 4096 + q`
- Cada ajuste viaja como un `f32` exacto que empaqueta 12 bits de id (`0..4095`) y 12 bits de valor cuantizado `q` (`0..4095`). El rango total (`id × 4096 + q <= 16 777 215`) es inferior a $2^{24}$, garantizando cero pérdida de precisión al decodificar en WGSL con operaciones enteras y división de coma flotante.

### 4.5 Segundo canal (fijo) y recarga en caliente en Horizon
- **Comportamiento de recarga:** La documentación de Horizon (`docs/api/shaders.md`) y la implementación en `crates/horizon-client/src/game/native_shaders.rs` (`tick_native_shader_pack`) establecen que el motor vigila las carpetas de los packs cada 500 ms (`LOOK_EVERY = 500 ms`) mediante un fingerprinting de archivos. Al detectar un archivo nuevo o modificado, Horizon revalida con naga y reconstruye la tubería gráfica sin reiniciar el juego.
- **Generación de `constantes_mod.wgsl`:** El mod escribe este archivo en la carpeta del pack activo conteniendo únicamente constantes WGSL para aquellos parámetros cuyos valores difieran de su valor por defecto, activando el Bit 4 (`16.0`) en la cabecera `mods[0].z`.
- **Manejo de paquetes `.hshader`:** Si el shader pack se encuentra instalado como archivo empaquetado `.hshader` (archivo ZIP comprimido de solo lectura), no es posible escribir dentro de él en caliente. El mod detecta este escenario mediante `write_pack_constants()` y documenta la degradación elegante: en lugar de fallar, los ajustes se transmiten por el canal en vivo a través de los 16 slots dinámicos de `frame.mods[4..7]`.
