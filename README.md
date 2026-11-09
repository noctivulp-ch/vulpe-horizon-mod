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

- Pasa al shader el día del mundo, la fase de la luna y la estación del año.
- Pasa la posición y edad del último rayo relativo a la cámara.
- Lee los ajustes finos del jugador desde un archivo de configuración (`vulpe-horizon.toml`).
- Pasa hasta 16 ajustes finos en vivo al shader sin recargar.
- Escribe el archivo de constantes del pack para el ajuste fino fijo.

## Puesta en marcha

Necesita Horizon con su lanzador. El mod se compila junto al juego:

```bash
horizon-launcher native-build --trust-native-code <ruta-del-repo>
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
