#!/usr/bin/env python3
"""Dibuja las texturas de la lluvia y la nieve como partículas (clima, etapa 1).

Particle Rain recorta las hojas de lluvia y nieve de vanilla en tiempo de ejecución; esas hojas son de Mojang y no se
pueden repartir con el mod, así que aquí se dibujan unas propias con la misma idea: trazos finos y grises (el tinte
del agua se aplica en el juego) para la lluvia, copos pequeños para la nieve, y versiones «fuertes» que juntan varias.
Escribe en contenido/assets/vulpe/textures/particle/ y los JSON de sprites en contenido/assets/vulpe/particles/.
Uso: tools/generar-texturas-clima.py  (sin argumentos; determinista).
"""
import json
import pathlib
import random

from PIL import Image

RAIZ = pathlib.Path(__file__).resolve().parent.parent / "contenido" / "assets" / "vulpe"
TEX = RAIZ / "textures" / "particle"
PART = RAIZ / "particles"
LADO = 32


def gota(img, rnd):
    """Un trazo vertical de 1 px con la opacidad que sube hacia abajo, en una columna y altura al azar."""
    x = rnd.randrange(4, LADO - 4)
    largo = rnd.randrange(10, 26)
    y0 = rnd.randrange(0, LADO - largo)
    for i in range(largo):
        a = int(110 + 120 * i / largo)
        img.putpixel((x, y0 + i), (230, 230, 230, a))


def copo(img, rnd):
    """Un copo de 2×2 a 3×3 con el centro más opaco."""
    t = rnd.choice([2, 2, 3])
    x = rnd.randrange(2, LADO - 2 - t)
    y = rnd.randrange(2, LADO - 2 - t)
    for dx in range(t):
        for dy in range(t):
            borde = dx in (0, t - 1) and dy in (0, t - 1) and t == 3
            img.putpixel((x + dx, y + dy), (250, 250, 255, 150 if borde else 235))


def juego(nombre, cuantas, dibujar, por_sprite, rnd):
    texturas = []
    for i in range(cuantas):
        img = Image.new("RGBA", (LADO, LADO), (0, 0, 0, 0))
        for _ in range(por_sprite):
            dibujar(img, rnd)
        archivo = f"{nombre}_{i}"
        img.save(TEX / f"{archivo}.png")
        texturas.append(f"vulpe:{archivo}")
    (PART / f"{nombre}.json").write_text(json.dumps({"textures": texturas}, indent=2) + "\n")


def main():
    TEX.mkdir(parents=True, exist_ok=True)
    PART.mkdir(parents=True, exist_ok=True)
    rnd = random.Random(26)
    juego("lluvia", 16, gota, 1, rnd)
    juego("lluvia_fuerte", 4, gota, 4, rnd)
    juego("nieve", 16, copo, 1, rnd)
    juego("nieve_fuerte", 4, copo, 4, rnd)


if __name__ == "__main__":
    main()
