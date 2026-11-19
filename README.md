<div align="center">

# Vulpe para Horizon — mod nativo

**Le da al shader Vulpe datos del mundo y ajustes que el menú de Horizon no tiene sitio para mostrar.**

![Licencia](https://img.shields.io/badge/licencia-GPL--3.0--or--later-FF6B00?style=flat-square&labelColor=1A1A1A)
![Motor](https://img.shields.io/badge/motor-Horizon-3A3A3A?style=flat-square&labelColor=1A1A1A)
![Estado](https://img.shields.io/badge/estado-en%20desarrollo-FF6B00?style=flat-square&labelColor=1A1A1A)

</div>

---

## Qué es

Un mod nativo de [Horizon](https://github.com/horizon-rust/horizon) que acompaña al shader
[vulpe-horizon](https://github.com/noctivulp-ch/vulpe-horizon). Horizon permite como mucho 64 opciones por pack y no
da al shader algunos datos del mundo; el mod se los pasa por `frame.mods`.

## Qué hace hoy

- Pasa al shader el día del mundo, la fase de la luna, la estación del año y la categoría de bioma en la cámara (formato 2 de `frame.mods`).
- Pasa la posición y edad del último rayo relativo a la cámara.
- Lee los ajustes finos del jugador desde un archivo de configuración (`config/vulpe-horizon.toml`).
- Pasa hasta 16 ajustes finos en vivo al shader sin recargar (`/vulpe vivo`, `/vulpe poner`, `/vulpe estado`).
- Genera el archivo de constantes del pack (`constantes_mod.wgsl`) mediante la Forma C (`shaderpacks/vulpe-mod/`).
- Añade clima con partículas (lluvia, nieve, polvo del desierto, matorrales, salpicaduras, humo, regueros y niebla vertical; port de Particle Rain) con sus texturas empaquetadas dentro del mismo `.hmod`.

## Puesta en marcha

Necesita Horizon (`nightly-2026-11-18-dd67b76` o compatible) con su lanzador (0.2.81+). El mod se empaqueta como un único `.hmod` (código + `assets/`) y se compila contra el SDK precompilado del lanzador:

```bash
tools/empaquetar.sh --instalar                         # genera dist/vulpe.hmod e instala en horizon-native-mods/
horizon-launcher native-build --trust-native-code      # compila los mods activos de la instancia
```

También se puede compilar directamente desde la carpeta del repo para pruebas:

```bash
horizon-launcher native-build --instance <instancia> --trust-native-code <ruta-del-repo>
```

Detalle, perfiles de compilación y cómo comprobar que funciona: [`docs/guia.md`](docs/guia.md). Decisiones de
diseño: [`docs/decisiones.md`](docs/decisiones.md).

## Licencia

Código bajo **GPL-3.0-or-later** ([`LICENSE`](LICENSE)). Los componentes de terceros y sus licencias
están en [`NOTICE`](NOTICE). El nombre y la identidad visual no forman parte de la licencia: ver
[`TRADEMARKS.md`](TRADEMARKS.md). Para aportar, [`CONTRIBUTING.md`](CONTRIBUTING.md).

## Apoyo

Este proyecto forma parte del trabajo de **NoctiVulp**: herramientas para Linux y Android,
publicadas como software libre. Las compilaciones oficiales y las novedades salen en Patreon.

<p align="center">
  <a href="https://www.patreon.com/cw/NoctiVulp"><img src="https://img.shields.io/badge/Patreon-NoctiVulp-FF6B00?style=for-the-badge&logo=patreon&logoColor=white&labelColor=1A1A1A" alt="NoctiVulp en Patreon"></a>
</p>

---

<p align="center"><sub>© 2026 NoctiVulp · <b>Powered by AI</b> — desarrollado con la asistencia de Claude, de Anthropic.</sub></p>

## Créditos

Los efectos de clima con partículas son un port de Particle Rain (pigcart, MIT). Ver [`NOTICE`](NOTICE).
