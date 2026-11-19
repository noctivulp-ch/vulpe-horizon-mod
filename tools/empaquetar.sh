#!/usr/bin/env bash
# Empaqueta el mod nativo de Vulpe (código + texturas de clima) como un único .hmod.
# Uso: tools/empaquetar.sh [--instalar [CARPETA_DE_LA_INSTANCIA]]
#   Sin opciones deja dist/vulpe.hmod.
#   Con --instalar lo copia también a <instancia>/horizon-native-mods/vulpe.hmod
#   (por defecto ~/.local/share/horizon) y activa mod.vulpe:true en horizon-mods.txt.
# Después hay que recompilar la instancia:
#   horizon-launcher native-build --instance <nombre> --trust-native-code
# Detalle: docs/guia.md §«Instalarlo en la instancia».
set -euo pipefail
raiz="$(cd "$(dirname "$0")/.." && pwd)"
salida="$raiz/dist/vulpe.hmod"
mkdir -p "$raiz/dist"

python3 -I - "$raiz" "$salida" <<'PY'
import pathlib, re, subprocess, sys, zipfile

raiz = pathlib.Path(sys.argv[1])
salida = sys.argv[2]

texto_raiz = re.compile(r"^(README|LICENSE|LICENCE|COPYING|NOTICE)[^/]*$")

def incluir(rel: str) -> bool:
    if rel in ("horizon-mod.toml", "Cargo.toml", "Cargo.lock", "icon.png"):
        return True
    if texto_raiz.match(rel):
        return True
    if rel.startswith("src/") and rel.endswith((".rs", ".toml")):
        return True
    if rel.startswith(("assets/", "data/")):
        return True
    return False

archivos = subprocess.run(
    ["git", "-C", str(raiz), "ls-files"],
    capture_output=True,
    text=True,
    check=True,
).stdout.split()

# Incluye también archivos nuevos aún sin añadir al índice si ya existen en el árbol
# (solo los que cumplan las mismas reglas de paquete).
candidatos = set(archivos)
for p in raiz.rglob("*"):
    if not p.is_file():
        continue
    rel = p.relative_to(raiz).as_posix()
    if rel.startswith((".git/", ".horizon/", "dist/", "target/", "src/engine/target/")):
        continue
    if rel.endswith("Cargo.lock") and rel != "Cargo.lock":
        continue
    if incluir(rel):
        candidatos.add(rel)

dentro = sorted(f for f in candidatos if incluir(f) and (raiz / f).is_file())
fecha = (2026, 1, 1, 0, 0, 0)
with zipfile.ZipFile(salida, "w", zipfile.ZIP_DEFLATED) as z:
    for f in dentro:
        zi = zipfile.ZipInfo(f, fecha)
        zi.compress_type = zipfile.ZIP_DEFLATED
        z.writestr(zi, (raiz / f).read_bytes())
print(f"{salida}: {len(dentro)} archivos")
PY

if [[ "${1:-}" == --instalar ]]; then
    instancia="${2:-$HOME/.local/share/horizon}"
    mkdir -p "$instancia/horizon-native-mods"
    cp "$salida" "$instancia/horizon-native-mods/vulpe.hmod"
    lista="$instancia/horizon-mods.txt"
    touch "$lista"
    grep -q '^mod\.vulpe:' "$lista" || echo 'mod.vulpe:true' >>"$lista"
    sed -i 's/^mod\.vulpe:.*/mod.vulpe:true/' "$lista"
    echo "Instalado en $instancia/horizon-native-mods/vulpe.hmod (activado en horizon-mods.txt)"
fi
