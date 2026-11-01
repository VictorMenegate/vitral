#!/bin/bash
# Roda o Vitral num GNOME Shell isolado (sem tocar na sessão nem no dconf de verdade), segue um
# roteiro de cliques/teclas e tira prints.
#
# Uso: ferramentas/bancada.sh <roteiro> <pasta_dos_prints> <comando...>
# Roteiro, uma ação por linha:
#   espera S | clique X Y | mover X Y | arrastar X1 Y1 X2 Y2 | rolar X Y N | texto ... | tecla Return | print nome
# "rolar X Y N" gira a roda N entalhes com o ponteiro em X,Y (N negativo sobe).
# PRINT_NO_ARRASTO=nome tira um print no meio do arrasto, antes de soltar.
# BANCADA=2 permite uma segunda bancada ao mesmo tempo.
# FUNDO=branco (fundo branco liso, o pior caso de contraste) ou FUNDO=<imagem> troca o papel de
# parede; sem ele, vale o da sessão.
# ZOOM=1.5 abre o Vitral com esse zoom (grava o interface.json da bancada antes de abrir).
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
    export XDG_CONFIG_HOME=$B/config XDG_DATA_HOME=$B/data XDG_CACHE_HOME=$B/cache \
        XDG_STATE_HOME=$B/state$N XDG_RUNTIME_DIR=$RUN GSETTINGS_BACKEND=keyfile DENTRO_DA_BANCADA=1
    unset WAYLAND_DISPLAY DISPLAY DBUS_SESSION_BUS_ADDRESS
    exec dbus-run-session -- "$0" "$ROTEIRO" "$PRINTS" "$@"
fi

gnome-shell --headless --wayland --virtual-monitor 1600x1000 --no-x11 > "$XDG_STATE_HOME/shell.log" 2>&1 &
SHELL_PID=$!
for i in $(seq 1 40); do [ -S "$XDG_RUNTIME_DIR/wayland-0" ] && break; sleep 0.25; done
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
WAYLAND_DISPLAY=$XDG_RUNTIME_DIR/wayland-0 "$@" > "$XDG_STATE_HOME/app.log" 2>&1 &
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
    esac
done < "$ROTEIRO"
kill $APP_PID 2>/dev/null; sleep 1; kill $SHELL_PID 2>/dev/null; wait 2>/dev/null
