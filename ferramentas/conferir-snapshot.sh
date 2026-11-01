#!/bin/bash
# Conferência manual (chama o Claude de verdade: gasta dois turnos curtos; não é do cargo test).
# Confere que, com o prompt de sistema por arquivo e --system-prompt-snapshot off, uma sessão
# retomada vê o prompt NOVO (um marcador trocado entre os turnos) e uma ferramenta MCP que só
# apareceu no segundo turno. É o que o Vitral faz em todo turno desde o Mestre L2.
#
# Uso: ferramentas/conferir-snapshot.sh            (com o claude do PATH ou de ~/.local/bin)
# Tudo fica numa pasta temporária em ~/.cache; nada do Vitral é lido nem gravado.
set -u
CLAUDE=$(command -v claude || echo "$HOME/.local/bin/claude")
T=$(mktemp -d "${XDG_CACHE_HOME:-$HOME/.cache}/vitral-snapshot.XXXXXX")
mkdir -p "$T/pasta"
S=$(python3 -c 'import uuid; print(uuid.uuid4())')
A="MARCADOR-A-$RANDOM$RANDOM"
B="MARCADOR-B-$RANDOM$RANDOM"
echo "O marcador deste chat é $A." > "$T/a.md"
echo "O marcador deste chat é $B." > "$T/b.md"

cat > "$T/servidor.py" <<'PY'
import json, sys
nomes = sys.argv[1:]
for linha in sys.stdin:
    pedido = json.loads(linha)
    if "id" not in pedido:
        continue
    metodo = pedido.get("method")
    if metodo == "initialize":
        versao = pedido.get("params", {}).get("protocolVersion", "2025-06-18")
        resultado = {"protocolVersion": versao, "capabilities": {"tools": {}},
                     "serverInfo": {"name": "teste", "version": "1"}}
    elif metodo == "tools/list":
        resultado = {"tools": [{"name": n, "description": "ferramenta de teste " + n,
                                "inputSchema": {"type": "object", "properties": {}}} for n in nomes]}
    elif metodo == "tools/call":
        resultado = {"content": [{"type": "text", "text": "ok"}]}
    else:
        resultado = {}
    print(json.dumps({"jsonrpc": "2.0", "id": pedido["id"], "result": resultado}), flush=True)
PY
mcp() {
    python3 -c 'import json,sys; print(json.dumps({"mcpServers": {"teste": {"type": "stdio", "command": "python3", "args": ["-I", sys.argv[1]] + sys.argv[2:]}}}))' "$T/servidor.py" "$@"
}

cd "$T/pasta" || exit 1
"$CLAUDE" -p --session-id "$S" --append-system-prompt-file "$T/a.md" --system-prompt-snapshot off \
    --mcp-config "$(mcp ferramenta_velha)" --strict-mcp-config --allowedTools mcp__teste \
    "Responda só OK." > "$T/turno1.txt" 2>&1
SAIDA=$("$CLAUDE" -p --resume "$S" --append-system-prompt-file "$T/b.md" --system-prompt-snapshot off \
    --mcp-config "$(mcp ferramenta_velha ferramenta_nova)" --strict-mcp-config --allowedTools mcp__teste \
    "Numa linha só: qual é o marcador deste chat no seu prompt de sistema, e quais ferramentas mcp__teste você tem?" 2>&1)
echo "$SAIDA" > "$T/turno2.txt"
echo "$SAIDA"
ok=0
if grep -q "$B" <<< "$SAIDA"; then echo "prompt novo: visto"; else echo "PROMPT NOVO NÃO VISTO"; ok=1; fi
if grep -q "$A" <<< "$SAIDA"; then echo "ATENÇÃO: o marcador antigo ainda aparece"; ok=1; fi
if grep -q "ferramenta_nova" <<< "$SAIDA"; then
    echo "ferramenta nova: vista"
else
    echo "FERRAMENTA NOVA NÃO VISTA: a passagem de sessão precisa disparar quando as ferramentas do papel mudam"
    ok=1
fi
echo "(arquivos em $T)"
exit $ok
