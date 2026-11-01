#!/bin/bash
# Roda o Vitral num GNOME Shell isolado (sem tocar na sessão nem no dconf de verdade), segue um
# roteiro de cliques/teclas e tira prints.
#
# Uso: ferramentas/bancada.sh <roteiro> <pasta_dos_prints> <comando...>
# O comando é o Vitral (./target/debug/vitral), com uma pasta opcional, que ele importa ou liga: no
# primeiro uso, uma pasta do Vitral antigo (com .vitral/) vem na migração; depois, a pasta abre o
# projeto que já a tem ou o Novo projeto com ela ligada. A migração de ponta a ponta é a
# RAIZ_SINTETICA (abaixo), nunca uma cópia de uma pasta de verdade.
# Roteiro, uma ação por linha:
#   espera S | clique X Y | mover X Y | arrastar X1 Y1 X2 Y2 | rolar X Y N | texto ... | tecla Return | print nome
# "rolar X Y N" gira a roda N entalhes com o ponteiro em X,Y (N negativo sobe).
# "roda <comando>" roda o comando no bash (sem comentário no fim da linha) e junta a saída em
# <pasta_dos_prints>/roda.log, com os XDG da bancada ($XDG_DATA_HOME/vitral são os dados dela).
# "reabre [comando]" derruba o Vitral com SIGKILL (uma queda), roda o comando, se houver, com ele
# fechado (ex.: mexer no estado.json) e abre de novo com os mesmos argumentos.
# CLAUDE_FALSO=1 põe na frente do PATH o ferramentas/claude-falso/claude: os turnos não chamam a API
# (ele anota a mensagem em notas-do-chat.md na pasta do chat e grava os argumentos em
# $XDG_STATE_HOME/claude-falso.log).
# PRINT_NO_ARRASTO=nome tira um print no meio do arrasto, antes de soltar.
# BANCADA=2 permite uma segunda bancada ao mesmo tempo; a pasta dela ($XDG_CACHE_HOME/vitral-bancada,
# com os dados, a configuração e a trava do Vitral) também precisa ser outra. Dois agentes ao mesmo
# tempo usam cada um o seu par, por exemplo XDG_CACHE_HOME=~/.cache/vitral-trabalho/bancada-l5
# BANCADA=5. Com o mesmo número (o Shell de uma recebe a janela da outra) ou a mesma pasta (o
# DADOS_NOVOS afastaria os dados da outra) já em uso, a bancada recusa começar.
# FUNDO=branco (fundo branco liso, o pior caso de contraste) ou FUNDO=<imagem> troca o papel de
# parede; sem ele, vale o da sessão.
# ZOOM=1.5 abre o Vitral com esse zoom (grava o interface.json da bancada antes de abrir).
# DADOS_NOVOS=1 abre como primeiro uso: afasta os dados que a bancada tinha (em $B/data, nunca os de
# verdade; o app/ que a atualização instalou fica), a configuração e o interface.json (a vista, o
# último chat e o último destino) para $B/afastados/<hora>.
# RAIZ_SINTETICA=1 é a migração de ponta a ponta: gera com o demo_vitral.py uma pasta do Vitral
# antigo dentro da bancada ($B/raiz-sintetica, com os cwd dentro dela e sessões novas), põe ela no
# recentes.json da bancada, abre como primeiro uso e, no fim, confere que os arquivos do .vitral
# antigo ficaram iguais (os hashes vão para <pasta_dos_prints>/hashes-antes.txt e -depois.txt).
# A atualização fica desligada; VITRAL_SEM_ATUALIZACAO=0 com VITRAL_ATUALIZACAO_BASE (e, no build de
# desenvolvimento, VITRAL_ATUALIZACAO_CHAVE) procura num release falso. O que ela instala vai para a
# pasta de dados da bancada, e o link do ~/.local/bin para o $B/bin.
# Pasta de demonstração: python3 -I ferramentas/demo_vitral.py <pasta> [--16]. Contraste nos
# prints: python3 -I ferramentas/medir_print.py <print.png> --pontos <arquivo>.
set -u
ROTEIRO=$(realpath "$1"); PRINTS=$(realpath -m "$2"); shift 2
N=${BANCADA:-1}
B=${XDG_CACHE_HOME:-$HOME/.cache}/vitral-bancada
RUN=/run/user/$(id -u)/vitral-bancada$N

if [ -z "${DENTRO_DA_BANCADA:-}" ]; then
    mkdir -p "$B"/{config/glib-2.0/settings,data/gnome-shell/extensions/inseguro@bancada,cache,state$N} "$RUN" "$PRINTS"
    chmod 700 "$RUN"
    # As travas ficam com o script até o fim (o Shell e o Vitral abrem sem elas). Uma bancada de antes
    # delas aparece pelo wayland-0 que atende e pela trava do Vitral nos dados.
    atende() { [ -S "$1" ] && python3 -I -c 'import socket, sys; socket.socket(socket.AF_UNIX).connect(sys.argv[1])' "$1" 2>/dev/null; }
    exec 7>"$RUN/bancada.trava" 8>"$B/bancada.trava"
    if ! flock -n 7 || atende "$RUN/wayland-0"; then
        echo "BANCADA=$N já está em uso ($RUN): escolha outro número." >&2
        exit 1
    fi
    if ! flock -n 8 || { [ -e "$B/data/vitral.trava" ] && ! flock -n "$B/data/vitral.trava" true; }; then
        echo "A pasta da bancada $B já está em uso: use outro XDG_CACHE_HOME." >&2
        exit 1
    fi
    cat > "$B/data/gnome-shell/extensions/inseguro@bancada/metadata.json" <<'J'
{"uuid":"inseguro@bancada","name":"bancada","description":"unsafe mode para testes","shell-version":["48","49","50","51"]}
J
    cat > "$B/data/gnome-shell/extensions/inseguro@bancada/extension.js" <<'J'
import * as Main from 'resource:///org/gnome/shell/ui/main.js';
export default class Inseguro {
    enable() { global.context.unsafe_mode = true; Main.overview.hide(); }
    disable() {}
}
J
    LISO=""
    case "${FUNDO:-}" in
        "") PAPEL=$(gsettings get org.gnome.desktop.background picture-uri-dark 2>/dev/null) ;;
        branco) PAPEL="''"; LISO=$'picture-options=\'none\'\nprimary-color=\'#ffffff\'' ;;
        *) PAPEL="'file://$(realpath "$FUNDO")'" ;;
    esac
    if [ -n "${DADOS_NOVOS:-}${RAIZ_SINTETICA:-}" ]; then
        AFASTAR="$B/afastados/$(date +%Y%m%d-%H%M%S)"
        # Os dados saem, o app que a atualização da bancada instalou ($B/data/vitral/app) fica.
        for d in "$B/data/vitral"/* "$B/data/vitral"/.[!.]*; do
            [ -e "$d" ] && [ "$(basename "$d")" != app ] && mkdir -p "$AFASTAR/vitral" && mv "$d" "$AFASTAR/vitral/"
        done
        for d in "$B/data/vitral.trava" "$B/config/vitral/config.json" "$B/config/vitral/recentes.json" \
            "$B/config/vitral/interface.json"; do
            [ -e "$d" ] && mkdir -p "$AFASTAR" && mv "$d" "$AFASTAR/"
        done
    fi
    if [ -n "${RAIZ_SINTETICA:-}" ]; then
        python3 -I "$(dirname "$(realpath "$0")")/demo_vitral.py" "$B/raiz-sintetica" || exit 1
        mkdir -p "$B/config/vitral"
        printf '["%s"]\n' "$B/raiz-sintetica" > "$B/config/vitral/recentes.json"
        (cd "$B/raiz-sintetica" && find .vitral -type f -exec sha256sum {} + | sort -k2) > "$PRINTS/hashes-antes.txt"
    fi
    if [ -n "${ZOOM:-}" ]; then
        mkdir -p "$B/config/vitral"
        echo "{\"zoom\": $ZOOM}" > "$B/config/vitral/interface.json"
    fi
    cat > "$B/config/glib-2.0/settings/keyfile" <<J
[org/gnome/shell]
enabled-extensions=['inseguro@bancada']
disable-user-extensions=false
welcome-dialog-last-shown-version='999'

[org/gnome/desktop/interface]
color-scheme='prefer-dark'
enable-hot-corners=false

[org/gnome/desktop/background]
picture-uri=$PAPEL
picture-uri-dark=$PAPEL
$LISO

[org/gnome/desktop/notifications]
show-banners=false
J
    [ -n "${CLAUDE_FALSO:-}" ] && export PATH="$(dirname "$(realpath "$0")")/claude-falso:$PATH"
    export XDG_CONFIG_HOME=$B/config XDG_DATA_HOME=$B/data XDG_CACHE_HOME=$B/cache XDG_BIN_HOME=$B/bin \
        XDG_STATE_HOME=$B/state$N XDG_RUNTIME_DIR=$RUN GSETTINGS_BACKEND=keyfile DENTRO_DA_BANCADA=1 \
        VITRAL_SEM_ATUALIZACAO=${VITRAL_SEM_ATUALIZACAO:-1}
    unset WAYLAND_DISPLAY DISPLAY DBUS_SESSION_BUS_ADDRESS
    exec dbus-run-session -- "$0" "$ROTEIRO" "$PRINTS" "$@"
fi

gnome-shell --headless --wayland --virtual-monitor 1600x1000 --no-x11 > "$XDG_STATE_HOME/shell.log" 2>&1 7>&- 8>&- &
SHELL_PID=$!
# O nome do socket sai no log: o lock velho de uma rodada que caiu faz o Shell ir para o wayland-1, e
# o wayland-0 que sobrou não tem ninguém atrás (o app abria sem tela).
for i in $(seq 1 40); do
    SOCKET=$(sed -n "s/.*Using Wayland display name '\([^']*\)'.*/\1/p" "$XDG_STATE_HOME/shell.log" | head -1)
    [ -n "$SOCKET" ] && [ -S "$XDG_RUNTIME_DIR/$SOCKET" ] && break
    sleep 0.25
done
SOCKET=${SOCKET:-wayland-0}
sleep 2
# O socket do Wayland sai antes do Shell terminar de subir; com a máquina ocupada, o objeto do Shell
# no D-Bus (e o Eval, que a extensão libera) pode levar 10 s ou mais. Sem esperar por ele, o
# "esconder o Panorama" abaixo se perdia e o roteiro inteiro rodava com o Panorama aberto. A
# expressão não pode ser um texto entre aspas sozinho: o gdbus o lê como GVariant e tira as aspas.
for i in $(seq 1 120); do
    gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell \
        --method org.gnome.Shell.Eval 'Main.layoutManager._startingUp ? "subindo" : "pronto"' \
        2>/dev/null | grep -q '^(true, .*pronto' && break
    sleep 0.5
done
ev() { gdbus call --session --dest org.gnome.Shell --object-path /org/gnome/Shell --method org.gnome.Shell.Eval "$1" >> "$XDG_STATE_HOME/eval.log" 2>&1; }
PREP='const C = imports.gi.Clutter; const G = imports.gi.GLib; const seat = global.stage.context.get_backend().get_default_seat(); if (!global._vp) global._vp = seat.create_virtual_device(C.InputDeviceType.POINTER_DEVICE); if (!global._vk) global._vk = seat.create_virtual_device(C.InputDeviceType.KEYBOARD_DEVICE);'
ev "$PREP global._vp.notify_absolute_motion(G.get_monotonic_time(), 800, 500); 'ok'"
sleep 0.5; ev 'Main.overview.hide(); "ok"'; sleep 0.5
WAYLAND_DISPLAY=$XDG_RUNTIME_DIR/$SOCKET "$@" > "$XDG_STATE_HOME/app.log" 2>&1 7>&- 8>&- &
APP_PID=$!
# A animação de entrada do Shell ainda pode abrir o Panorama: esconde de novo com a janela já no ar.
(sleep 4; ev 'Main.overview.hide(); "ok"') &
while read -r cmd resto; do
    read -r a b _ <<< "$resto"
    case "$cmd" in
        espera) sleep "$a" ;;
        mover) ev "$PREP global._vp.notify_absolute_motion(G.get_monotonic_time(), $a, $b); 'ok'"; sleep 0.4 ;;
        clique)
            ev "$PREP global._vp.notify_absolute_motion(G.get_monotonic_time(), $a, $b); 'ok'"; sleep 0.3
            ev "$PREP global._vp.notify_button(G.get_monotonic_time(), C.BUTTON_PRIMARY, C.ButtonState.PRESSED); 'ok'"; sleep 0.08
            ev "$PREP global._vp.notify_button(G.get_monotonic_time(), C.BUTTON_PRIMARY, C.ButtonState.RELEASED); 'ok'"; sleep 0.4 ;;
        arrastar)
            read -r x1 y1 x2 y2 _ <<< "$resto"
            ev "$PREP global._vp.notify_absolute_motion(G.get_monotonic_time(), $x1, $y1); 'ok'"; sleep 0.3
            ev "$PREP global._vp.notify_button(G.get_monotonic_time(), C.BUTTON_PRIMARY, C.ButtonState.PRESSED); 'ok'"; sleep 0.2
            for passo in 1 2 3 4 5 6 7 8; do
                ev "$PREP global._vp.notify_absolute_motion(G.get_monotonic_time(), $x1 + ($x2 - $x1) * $passo / 8, $y1 + ($y2 - $y1) * $passo / 8); 'ok'"; sleep 0.08
            done
            sleep 0.3
            [ -n "${PRINT_NO_ARRASTO:-}" ] && gdbus call --session --dest org.gnome.Shell.Screenshot --object-path /org/gnome/Shell/Screenshot --method org.gnome.Shell.Screenshot.Screenshot false false "$PRINTS/$PRINT_NO_ARRASTO.png" > /dev/null
            ev "$PREP global._vp.notify_button(G.get_monotonic_time(), C.BUTTON_PRIMARY, C.ButtonState.RELEASED); 'ok'"; sleep 0.5 ;;
        rolar)
            ev "$PREP global._vp.notify_absolute_motion(G.get_monotonic_time(), $a, $b); 'ok'"; sleep 0.3
            read -r _ _ n _ <<< "$resto"; sentido=DOWN
            [ "$n" -lt 0 ] && { sentido=UP; n=$(( -n )); }
            for ((i=0; i<n; i++)); do
                ev "$PREP global._vp.notify_discrete_scroll(G.get_monotonic_time(), C.ScrollDirection.$sentido, C.ScrollSource.WHEEL); 'ok'"; sleep 0.05
            done
            sleep 0.4 ;;
        texto)
            T=$(python3 -c 'import json,sys;print(json.dumps(sys.argv[1]))' "$resto")
            ev "$PREP for (const ch of $T) { const k = C.unicode_to_keysym(ch.codePointAt(0)); global._vk.notify_keyval(G.get_monotonic_time(), k, C.KeyState.PRESSED); global._vk.notify_keyval(G.get_monotonic_time()+500, k, C.KeyState.RELEASED); } 'ok'"; sleep 0.4 ;;
        tecla)
            # "tecla Control_L+b" aperta os modificadores, a tecla e solta na ordem inversa.
            IFS='+' read -ra partes <<< "$a"; js="$PREP const t = G.get_monotonic_time();"; n=0
            for k in "${partes[@]}"; do js="$js global._vk.notify_keyval(t+$n, C.KEY_$k, C.KeyState.PRESSED);"; n=$((n+100)); done
            for ((i=${#partes[@]}-1; i>=0; i--)); do js="$js global._vk.notify_keyval(t+$n, C.KEY_${partes[i]}, C.KeyState.RELEASED);"; n=$((n+100)); done
            ev "$js 'ok'"; sleep 0.4 ;;
        print) gdbus call --session --dest org.gnome.Shell.Screenshot --object-path /org/gnome/Shell/Screenshot --method org.gnome.Shell.Screenshot.Screenshot false false "$PRINTS/$a.png" > /dev/null ;;
        roda) { echo "\$ $resto"; bash -c "$resto"; echo; } >> "$PRINTS/roda.log" 2>&1 ;;
        reabre)
            kill -KILL $APP_PID 2>/dev/null; wait $APP_PID 2>/dev/null; sleep 1
            [ -n "$resto" ] && { echo "\$ (fechado) $resto"; bash -c "$resto"; echo; } >> "$PRINTS/roda.log" 2>&1
            WAYLAND_DISPLAY=$XDG_RUNTIME_DIR/$SOCKET "$@" >> "$XDG_STATE_HOME/app.log" 2>&1 7>&- 8>&- &
            APP_PID=$!
            (sleep 4; ev 'Main.overview.hide(); "ok"') & ;;
    esac
done < "$ROTEIRO"
kill $APP_PID 2>/dev/null; sleep 1; kill $SHELL_PID 2>/dev/null; wait 2>/dev/null
if [ -n "${RAIZ_SINTETICA:-}" ]; then
    B_RAIZ=$(dirname "$XDG_CACHE_HOME")/raiz-sintetica
    (cd "$B_RAIZ" && find .vitral -type f -exec sha256sum {} + | sort -k2) > "$PRINTS/hashes-depois.txt"
    if cmp -s "$PRINTS/hashes-antes.txt" "$PRINTS/hashes-depois.txt"; then
        echo "raiz sintética intacta" >&2
    else
        echo "A RAIZ SINTÉTICA MUDOU: diff $PRINTS/hashes-antes.txt $PRINTS/hashes-depois.txt" >&2
    fi
fi
