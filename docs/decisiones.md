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

### `mods[2]` — Rayo
- `[0.0, 0.0, 0.0, -1.0]`: Edad negativa (-1.0 segundos) para indicar que no hay ningún rayo reciente en memoria, con la bandera correspondiente apagada en la cabecera.

### `mods[3]` — Estado del Jugador
- `[0.0, 0.0, 0.0, 0.0]`: La API actual del cliente no expone salud ni efectos sin sincronización con servidor. La bandera permanece apagada en la cabecera para que el shader descarte estos valores.

### `mods[4]` a `mods[7]` — Ajustes en vivo
- Rellenos con `0.0` (quedan reservados para el corte M-02 de ajustes en vivo).
