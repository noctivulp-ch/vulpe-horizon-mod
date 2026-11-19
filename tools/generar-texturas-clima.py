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


def polvo(img, rnd):
    """Granos irregulares de arena/polvo claros (se tiñen según el bloque en el juego)."""
    cx = LADO // 2 + rnd.randrange(-3, 4)
    cy = LADO // 2 + rnd.randrange(-3, 4)
    for _ in range(rnd.randrange(3, 7)):
        x = max(1, min(LADO - 2, cx + rnd.randrange(-4, 5)))
        y = max(1, min(LADO - 2, cy + rnd.randrange(-4, 5)))
        t = rnd.choice([1, 2, 2])
        a = rnd.randrange(175, 245)
        v = rnd.randrange(215, 255)
        for dx in range(t):
            for dy in range(t):
                if 0 <= x + dx < LADO and 0 <= y + dy < LADO:
                    img.putpixel((x + dx, y + dy), (v, v, v, a))


def bruma_polvo(img, rnd):
    """Nube suave con tramado ligero para la bruma de arena (se tiñe en el juego)."""
    centro = (LADO - 1) * 0.5
    radio = LADO * 0.48
    for y in range(LADO):
        for x in range(LADO):
            d = ((x - centro) ** 2 + (y - centro) ** 2) ** 0.5 / radio
            if d >= 1.0:
                continue
            peso = (1.0 - d * d) ** 2
            # Tramado suave para que se sienta arenosa y no un disco plano.
            ruido = 0.78 + 0.22 * rnd.random()
            a = int(min(255, max(0, peso * ruido * 210)))
            if a > 6:
                img.putpixel((x, y), (242, 240, 235, a))


def matorral(img, _rnd):
    """Un matorral seco con ramas en tono madera/arena oscura; luego se gira en 4 fotogramas."""
    # Ramas simples desde el centro hacia fuera (sin copiar dead_bush de Mojang).
    ramas = [
        [(16, 26), (16, 18), (11, 12), (8, 7)],
        [(16, 22), (21, 15), (25, 10)],
        [(16, 18), (17, 10), (15, 5)],
        [(12, 14), (7, 15), (5, 12)],
        [(20, 16), (25, 17), (27, 14)],
        [(16, 20), (10, 21), (7, 23)],
        [(16, 20), (22, 22), (25, 24)],
        [(14, 10), (19, 7)],
    ]
    paleta = [(118, 84, 48, 255), (138, 100, 58, 255), (96, 68, 38, 255)]
    for idx, pts in enumerate(ramas):
        col = paleta[idx % len(paleta)]
        for (x0, y0), (x1, y1) in zip(pts, pts[1:]):
            pasos = max(abs(x1 - x0), abs(y1 - y0), 1)
            for s in range(pasos + 1):
                x = round(x0 + (x1 - x0) * s / pasos)
                y = round(y0 + (y1 - y0) * s / pasos)
                if 0 <= x < LADO and 0 <= y < LADO:
                    img.putpixel((x, y), col)


def reguero(img, _rnd):
    """Gotita vertical fina que escurre por paredes y cristales (se tiñe con color de agua)."""
    cx = LADO // 2
    for y in range(6, 26):
        t = (y - 6) / 19.0
        radio = 1 if t < 0.7 else 2
        a = int(80 + 150 * t)
        for dx in range(-radio, radio + 1):
            borde = abs(dx) == radio and radio > 1
            img.putpixel((cx + dx, y), (235, 240, 250, a // 2 if borde else a))


def niebla(img, _rnd):
    """Banco de niebla suave y muy traslúcido (para no doblar la niebla del pack)."""
    cx = (LADO - 1) * 0.5
    cy = (LADO - 1) * 0.5
    rx = LADO * 0.48
    ry = LADO * 0.42
    for y in range(LADO):
        for x in range(LADO):
            dx = (x - cx) / rx
            dy = (y - cy) / ry
            d2 = dx * dx + dy * dy
            if d2 >= 1.0:
                continue
            peso = (1.0 - d2) ** 2
            a = int(peso * 85)
            if a > 3:
                img.putpixel((x, y), (245, 248, 255, a))


def juego_girado(nombre, dibujar, rnd):
    """Genera 4 fotogramas girados 0/90/180/270° para simular giro en Horizon."""
    base = Image.new("RGBA", (LADO, LADO), (0, 0, 0, 0))
    dibujar(base, rnd)
    texturas = []
    for i in range(4):
        rot = base.rotate(-90 * i)
        archivo = f"{nombre}_{i}"
        rot.save(TEX / f"{archivo}.png")
        texturas.append(f"vulpe:{archivo}")
    (PART / f"{nombre}.json").write_text(json.dumps({"textures": texturas}, indent=2) + "\n")


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
    juego("polvo", 3, polvo, 1, rnd)
    juego("bruma_polvo", 1, bruma_polvo, 1, rnd)
    juego_girado("matorral", matorral, rnd)
    juego("reguero", 1, reguero, 1, rnd)
    juego("niebla", 1, niebla, 1, rnd)


if __name__ == "__main__":
    main()
