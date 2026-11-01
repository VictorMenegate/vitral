#!/bin/bash
# Só para desenvolvimento: compila este checkout em release e instala no layout do app
# (~/.local/share/vitral/app) como <próxima>-dev.<commit>, com o mesmo `vitral --instalar` do
# release. Pelo semver isso é menor que a <próxima>, então o release dela a substitui, e a versão
# que estava fica como anterior. Para o dono, instalar é só pelos releases (pacote/linux/instalar.sh
# ou o próprio app): nada de compilar no PC dele para instalar.
set -euo pipefail
cd "$(dirname "$0")/.."
# Enquanto os dados globais não estão ligados, o ramo está no meio da mudança do Mestre.
if ! grep -q 'DADOS_GLOBAIS: bool = true' src/dados.rs; then
    echo "Os dados globais ainda estão desligados (DADOS_GLOBAIS em src/dados.rs): não instalo um Vitral no meio da mudança." >&2
    exit 1
fi
versao=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)
base=${versao%%-*}
if [ "$base" = "$versao" ]; then
    IFS=. read -r maior menor _ <<< "$base"
    proxima="$maior.$((menor + 1)).0"
else
    proxima=$base
fi
commit=$(git rev-parse --short=10 HEAD)
# Pelo semver, um identificador só de dígitos não pode começar com zero.
case "$commit" in *[a-f]*) ;; *) commit="g$commit" ;; esac
[ -z "$(git status --porcelain --untracked-files=no)" ] || commit="$commit.modificado"
export VITRAL_VERSAO="$proxima-dev.$commit"
# Uma compilação por vez em toda a máquina, com nice e -j 4 (a fonte do PC não aguenta carga alta).
compilar="${COMPILAR:-$HOME/.cache/vitral-trabalho/compilar.sh}"
if [ -x "$compilar" ]; then
    "$compilar" build --release
    binario=target/release/vitral
else
    nice -n 10 flock "${XDG_CACHE_HOME:-$HOME/.cache}/vitral-compilando" cargo build --release -j 4
    binario="${CARGO_TARGET_DIR:-target}/release/vitral"
fi
test "$("$binario" --versao)" = "vitral $VITRAL_VERSAO"
"$binario" --instalar
