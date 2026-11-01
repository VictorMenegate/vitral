#!/bin/bash
# Compila em modo release e põe o Vitral no menu de aplicativos (só na pasta do usuário).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
icones=~/.local/share/icons/hicolor
mkdir -p ~/.local/share/applications "$icones/scalable/apps"
install -m 644 assets/vitral.svg "$icones/scalable/apps/vitral.svg"
# PNGs para quem não usa o SVG; o de 48 px sai do logo pequeno, que se lê melhor nesse tamanho.
for n in 48 64 128 256; do
    install -D -m 644 "assets/vitral-$n.png" "$icones/${n}x${n}/apps/vitral.png"
done
# De 16 a 32 px (notificações, alt+tab, barra), o logo pequeno, feito para esses tamanhos.
for n in 16 24 32; do
    install -D -m 644 assets/vitral-pequeno.svg "$icones/${n}x${n}/apps/vitral.svg"
done
sed "s#@BINARIO@#$PWD/target/release/vitral#" assets/vitral.desktop > ~/.local/share/applications/vitral.desktop
update-desktop-database ~/.local/share/applications 2>/dev/null || true
gtk-update-icon-cache -q -t ~/.local/share/icons/hicolor 2>/dev/null || true
echo "Vitral instalado no menu de aplicativos."
