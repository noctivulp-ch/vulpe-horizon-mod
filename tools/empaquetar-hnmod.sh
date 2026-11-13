#!/usr/bin/env bash
# Empaqueta el mod como .hnmod (el formato de los mods nativos instalados en una instancia).
# Uso: tools/empaquetar-hnmod.sh [--instalar [CARPETA_DE_LA_INSTANCIA]]
#   Sin opciones deja dist/vulpe.hnmod. Con --instalar lo copia también a
#   <instancia>/horizon-native-mods/ (por defecto ~/.local/share/horizon) y lo activa en horizon-mods.txt.
# Después hay que recompilar la instancia: horizon-launcher native-build --trust-native-code
# Detalle: docs/guia.md §«Instalarlo en la instancia».
set -euo pipefail
raiz="$(cd "$(dirname "$0")/.." && pwd)"
salida="$raiz/dist/vulpe.hnmod"
mkdir -p "$raiz/dist"

python3 -I - "$raiz" "$salida" <<'PY'
import json, re, subprocess, sys, tomllib, zipfile
raiz, salida = sys.argv[1], sys.argv[2]
toml = tomllib.load(open(f"{raiz}/horizon-mixin.toml", "rb"))
mod = toml["mod"]
manifiesto = {
    "game": "26.3",
    "horizon": toml["compatibility"]["horizon_revision"],
    "id": mod["id"],
    # horizon-mixins con el que se compiló el formato 2; el lanzador usa el más nuevo de los que pidan los mods.
    "mixins": "e8cd0aa38cffef49cb2e088ef716baa107148c02",
    "schema": 1,
    "version": mod["version"],
}
# Solo lo que acepta el registro: .rs, .toml y los textos de una crate (README, LICENSE, NOTICE…).
texto = re.compile(r"(^|/)(README|LICENSE|LICENCE|COPYING|NOTICE)[^/]*$")
archivos = subprocess.run(["git", "-C", raiz, "ls-files"], capture_output=True, text=True, check=True).stdout.split()
dentro = sorted(f for f in archivos if f.endswith((".rs", ".toml")) or texto.search(f))
with zipfile.ZipFile(salida, "w", zipfile.ZIP_DEFLATED) as z:
    fecha = (2026, 1, 1, 0, 0, 0)
    z.writestr(zipfile.ZipInfo("horizon-native.json", fecha), json.dumps(manifiesto, indent=2, sort_keys=True))
    for f in dentro:
        z.writestr(zipfile.ZipInfo(f"mixins/{mod['id']}/{f}", fecha), open(f"{raiz}/{f}", "rb").read())
print(f"{salida}: {len(dentro)} archivos")
PY

if [[ "${1:-}" == --instalar ]]; then
    instancia="${2:-$HOME/.local/share/horizon}"
    mkdir -p "$instancia/horizon-native-mods"
    cp "$salida" "$instancia/horizon-native-mods/vulpe.hnmod"
    lista="$instancia/horizon-mods.txt"
    touch "$lista"
    grep -q '^mod\.vulpe:' "$lista" || echo 'mod.vulpe:true' >>"$lista"
    sed -i 's/^mod\.vulpe:.*/mod.vulpe:true/' "$lista"
    echo "Instalado en $instancia/horizon-native-mods/vulpe.hnmod (activado en horizon-mods.txt)"
fi
