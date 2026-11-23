#!/bin/bash
# Abre cada menu, popover e diálogo do Vitral na bancada (o roteiro roteiros/galeria-menus.txt),
# tira um print de cada e monta uma folha com todos, para ver e analisar de uma vez.
# Uso: ferramentas/galeria-de-menus.sh <pasta de saída> [binário]
# Saída: <pasta>/NN-nome.png (cada um inteiro) e <pasta>/folha.png (todos em miniatura, com o nome).
set -euo pipefail
cd "$(dirname "$(realpath "$0")")/.."
SAIDA=${1:?uso: ferramentas/galeria-de-menus.sh <pasta de saída> [binário]}
BIN=${2:-./target/release/vitral}
ZOOM=${ZOOM:-1.6} BANCADA=${BANCADA:-6} CLAUDE_FALSO=1 ferramentas/bancada.sh ferramentas/roteiros/galeria-menus.txt "$SAIDA" "$BIN"
python3 -I - "$SAIDA" <<'PY'
import glob, os, sys
from PIL import Image, ImageDraw
pasta = sys.argv[1]
arquivos = sorted(a for a in glob.glob(os.path.join(pasta, "*.png")) if not a.endswith("folha.png"))
colunas, largura, altura = 4, 480, 300
linhas = (len(arquivos) + colunas - 1) // colunas
folha = Image.new("RGB", (colunas * (largura + 8), linhas * (altura + 28)), "white")
desenho = ImageDraw.Draw(folha)
for i, arquivo in enumerate(arquivos):
    x, y = (i % colunas) * (largura + 8), (i // colunas) * (altura + 28)
    folha.paste(Image.open(arquivo).convert("RGB").resize((largura, altura)), (x, y + 24))
    desenho.text((x + 4, y + 6), os.path.basename(arquivo)[:-4], fill="black")
folha.save(os.path.join(pasta, "folha.png"))
print(f"{len(arquivos)} telas em {os.path.join(pasta, 'folha.png')}")
PY
rm -f "$HOME/eita/notas-do-chat.md"
