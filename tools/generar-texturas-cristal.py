#!/usr/bin/env python3
"""Genera texturas limpias de cristal transparente para Horizon (subsistema 27, N-09).

Bajo `[material] coverage = "engine"`, Horizon recorta en la pasada translúcida todo texel con
`albedo.a < 0.01` (`shaders-reference-0.2.md:105-106`), por lo que el interior con alfa 0 del
`glass.png` vanilla nunca llega a `fn material()`. Estas texturas propias mantienen el marco
opaco del bloque y del canto del panel, y fijan el interior transparente a alfa 6/255 (~0,0235),
suficiente para pasar el recorte del motor; el pack devuelve alfa 0 cuando no dibuja capas.

Uso: tools/generar-texturas-cristal.py (sin argumentos; determinista).
"""
import pathlib

from PIL import Image

RAIZ = pathlib.Path(__file__).resolve().parent.parent / "assets" / "vulpe" / "textures" / "block"
ALFA_INTERIOR = 6  # 6/255 ~= 0.0235 > 0.01 (invisible cuando el pack devuelve alfa 0)


def generar_glass() -> Image.Image:
    img = Image.new("RGBA", (16, 16), (255, 255, 255, ALFA_INTERIOR))
    marco_ext = (212, 232, 240, 255)
    marco_esq = (232, 246, 252, 255)
    brillo = (242, 250, 255, 255)

    for i in range(16):
        img.putpixel((i, 0), marco_ext)
        img.putpixel((i, 15), marco_ext)
        img.putpixel((0, i), marco_ext)
        img.putpixel((15, i), marco_ext)

    for x, y in [(0, 0), (1, 0), (0, 1), (15, 0), (14, 0), (15, 1), (0, 15), (15, 15)]:
        img.putpixel((x, y), marco_esq)

    # Dos destellos diagonales sutiles propios del cristal (sin copiar píxeles de Mojang).
    for x, y in [(3, 2), (2, 3), (4, 2), (12, 13), (13, 12)]:
        img.putpixel((x, y), brillo)

    return img


def generar_glass_pane_top() -> Image.Image:
    img = Image.new("RGBA", (16, 16), (255, 255, 255, ALFA_INTERIOR))
    borde = (205, 226, 236, 255)
    centro = (232, 244, 250, 255)
    for y in range(16):
        img.putpixel((7, y), borde if y in (0, 15) else centro)
        img.putpixel((8, y), borde if y in (0, 15) else centro)
    return img


def main() -> None:
    RAIZ.mkdir(parents=True, exist_ok=True)
    generar_glass().save(RAIZ / "glass.png")
    generar_glass_pane_top().save(RAIZ / "glass_pane_top.png")


if __name__ == "__main__":
    main()
