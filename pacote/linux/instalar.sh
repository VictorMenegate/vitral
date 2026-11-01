#!/bin/sh
# Instala o Vitral publicado no GitHub só na pasta do usuário (sem sudo e sem compilar):
#   curl -fsSL https://github.com/VictorMenegate/vitral/releases/latest/download/instalar.sh | sh
# Depois disso o próprio Vitral se atualiza.
set -eu
REPO="VictorMenegate/vitral"
BASE="https://github.com/$REPO/releases/latest/download"
# O conteúdo de pacote/vitral.pub (a mesma chave embutida no app).
CHAVE="RWTYL8CpwbDBQDlQSSec/5qzypGrJ8CnO/aKN6XDMIuKEQajo8ZmyHoS"

[ "$(uname -s)-$(uname -m)" = "Linux-x86_64" ] || { echo "Por enquanto só há Vitral para Linux x86_64." >&2; exit 1; }
for c in curl tar sha256sum; do
    command -v "$c" >/dev/null || { echo "Falta o $c." >&2; exit 1; }
done
# Fora do /tmp: ele é tmpfs e o PC pode reiniciar no meio.
cache="${XDG_CACHE_HOME:-$HOME/.cache}"; mkdir -p "$cache"
trabalho=$(mktemp -d "$cache/vitral-instalar.XXXXXX")
trap 'rm -rf "$trabalho"' EXIT INT TERM
cd "$trabalho"

curl -fsSL -o SHA256SUMS "$BASE/SHA256SUMS"
curl -fsSL -o SHA256SUMS.minisig "$BASE/SHA256SUMS.minisig"
if command -v minisign >/dev/null; then
    minisign -Vq -P "$CHAVE" -m SHA256SUMS || { echo "A assinatura não confere: nada foi instalado." >&2; exit 1; }
else
    echo "Sem o minisign (sudo dnf install minisign), confiro só o SHA-256 desta vez; as atualizações conferem a assinatura."
fi
linha=$(grep -E '^[0-9a-f]{64}  vitral-[0-9][0-9A-Za-z.-]*-linux-x86_64\.tar\.gz$' SHA256SUMS | head -n 1)
[ -n "$linha" ] || { echo "Esta versão não tem pacote do Linux." >&2; exit 1; }
nome=${linha##* }
echo "Baixando $nome…"
curl -fSL --progress-bar -o "$nome" "$BASE/$nome"
printf '%s\n' "$linha" | sha256sum -c --quiet - || { echo "O arquivo veio diferente do publicado: nada foi instalado." >&2; exit 1; }
tar -xzf "$nome"
"./${nome%.tar.gz}/vitral" --instalar
