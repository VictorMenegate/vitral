#!/bin/bash
# restaurar-bancada.sh <pasta da bancada> <pasta do pristino>: devolve à bancada os dados e a
# configuração de uma migração já feita (os ids dos chats ficam os mesmos de uma rodada para a
# outra, o que os roteiros que citam um chat por id precisam).
set -eu
B=$1/vitral-bancada; P=$2
rm -rf "$B/data/vitral" "$B/config/vitral"
cp -a "$P/data" "$B/data/vitral"; cp -a "$P/config" "$B/config/vitral"
