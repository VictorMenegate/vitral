#!/usr/bin/env python3
"""Release falso do Vitral para a bancada da atualização (ferramentas/roteiros/dist-atualizacao.txt).

  python3 -I ferramentas/release_falso.py gerar <pasta>
      Uma chave só da bancada (Ed25519 pelo openssl, guardada em <pasta>/chave.pem; nunca a do
      release), o pacote 0.9.0 de uns 20 MB com um "vitral" de mentira que só responde ao
      --versao --json, o vitral-versao.json assinado no formato do minisign (pré-hash blake2b-512),
      a pública em <pasta>/bancada.pub e um manifesto adulterado.

  python3 -I ferramentas/release_falso.py publicar <pasta> <pacote.tar.gz> <saída>
      O vitral-versao.json de um pacote de verdade (a versão e a plataforma saem do nome, como no
      release), assinado com a mesma chave da bancada (<pasta>/chave.pem, gerada se faltar), e o
      pacote, em <saída>. É o release falso do aceite do atualizador no runner do Windows
      (ferramentas/atualizador-windows.ps1), que copia a <saída> para <pasta>/r na hora de publicar.

  python3 -I ferramentas/release_falso.py servir <pasta> <porta> [--direto]
      Serve em http://127.0.0.1:<porta>/r/. O manifesto responde, na ordem dos pedidos: 503 (sem
      conexão), o adulterado (a assinatura não confere) e depois o certo; com --direto, sempre o
      certo, 4 s depois do pedido (para o "Procurando…" aparecer). O pacote sai a 1 MB/s, para o
      "Baixando" aparecer. Os pedidos ficam em <pasta>/pedidos.log.

O app só aceita a chave da bancada fora do canal estável (o build do PC, o canal de teste do
runner), com VITRAL_ATUALIZACAO_BASE e VITRAL_ATUALIZACAO_CHAVE=<pasta>/bancada.pub.
"""
import re
import base64
import hashlib
import http.server
import io
import json
import os
import subprocess
import sys
import tarfile
import time

VERSAO = "0.9.0"


def assinador(pasta):
    """A função que assina no formato do minisign com a chave da bancada (gerada pelo openssl na
    primeira vez), e a pública dela em <pasta>/bancada.pub."""
    chave = os.path.join(pasta, "chave.pem")
    if not os.path.exists(chave):
        subprocess.run(["openssl", "genpkey", "-algorithm", "ed25519", "-out", chave], check=True)
    pk = subprocess.run(["openssl", "pkey", "-in", chave, "-pubout", "-outform", "DER"], check=True, capture_output=True).stdout[-32:]
    keyid = hashlib.sha256(pk).digest()[:8]

    def assinar(dados):
        caminho = os.path.join(pasta, "msg.bin")
        with open(caminho, "wb") as f:
            f.write(dados)
        saida = subprocess.run(["openssl", "pkeyutl", "-sign", "-inkey", chave, "-rawin", "-in", caminho], check=True, capture_output=True).stdout
        os.remove(caminho)
        assert len(saida) == 64
        return saida

    def minisig(dados, comentario):
        sig = assinar(hashlib.blake2b(dados, digest_size=64).digest())
        glob = assinar(sig + comentario.encode())
        linha = base64.b64encode(b"ED" + keyid + sig).decode()
        return f"untrusted comment: assinatura da bancada\n{linha}\ntrusted comment: {comentario}\n{base64.b64encode(glob).decode()}\n"

    with open(os.path.join(pasta, "bancada.pub"), "w") as f:
        f.write("untrusted comment: chave da bancada do Vitral (só testes)\n")
        f.write(base64.b64encode(b"Ed" + keyid + pk).decode() + "\n")
    return minisig


def gerar(pasta):
    pasta = os.path.abspath(pasta)
    r = os.path.join(pasta, "r")
    os.makedirs(r, exist_ok=True)
    minisig = assinador(pasta)

    prefixo = f"vitral-{VERSAO}-linux-x86_64"
    script = (
        "#!/bin/sh\n"
        f"printf '%s\\n' '{{\"versao\":\"{VERSAO}\",\"formato_dados\":1,\"canal\":\"estavel\",\"commit\":\"\",\"plataforma\":\"linux-x86_64\"}}'\n"
    ).encode()
    novidades = (
        "# Novidades do Vitral\n\n"
        f"## {VERSAO}\n\n"
        "- **Conversas que se lembram.** O Mestre guarda o que importa entre as conversas.\n"
        "- **Mais rápido ao abrir.** A lista aparece antes do motor terminar de ler as tarefas.\n"
        "- Versão de mentira da bancada: o binário só responde ao `--versao --json`.\n"
    ).encode()
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", compresslevel=1) as tar:
        info = tarfile.TarInfo(prefixo)
        info.type, info.mode = tarfile.DIRTYPE, 0o755
        tar.addfile(info)
        for nome, dados, modo in [("vitral", script, 0o755), ("NOVIDADES.md", novidades, 0o644), ("preenchimento.bin", os.urandom(20 * 1024 * 1024), 0o644)]:
            info = tarfile.TarInfo(f"{prefixo}/{nome}")
            info.size, info.mode = len(dados), modo
            tar.addfile(info, io.BytesIO(dados))
    pacote = buf.getvalue()
    nome = f"{prefixo}.tar.gz"
    with open(os.path.join(r, nome), "wb") as f:
        f.write(pacote)

    manifesto = {
        "formato": 1,
        "app": "vitral",
        "versao": VERSAO,
        "commit": "0" * 40,
        "data": "2026-11-01T15:00:00Z",
        "pagina": f"https://github.com/VictorMenegate/vitral/releases/tag/v{VERSAO}",
        "formato_dados": 1,
        "notas": novidades.decode().split(f"## {VERSAO}\n\n", 1)[1].strip(),
        "arquivos": {"linux-x86_64": {"nome": nome, "tamanho": len(pacote), "sha256": hashlib.sha256(pacote).hexdigest()}},
    }
    dados = json.dumps(manifesto, indent=2, ensure_ascii=False).encode()
    with open(os.path.join(r, "vitral-versao.json"), "wb") as f:
        f.write(dados)
    with open(os.path.join(r, "vitral-versao.json.minisig"), "w") as f:
        f.write(minisig(dados, f"vitral {VERSAO} vitral-versao.json"))
    with open(os.path.join(pasta, "adulterado.json"), "wb") as f:
        f.write(dados.replace(b'"formato_dados": 1', b'"formato_dados": 2'))
    print(f"{len(pacote)} bytes em {nome}; chave {os.path.join(pasta, 'bancada.pub')}")


def publicar(pasta, pacote, saida):
    pasta, saida = os.path.abspath(pasta), os.path.abspath(saida)
    os.makedirs(saida, exist_ok=True)
    nome = os.path.basename(pacote)
    achado = re.fullmatch(r"vitral-([0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.]+)?)-([a-z0-9_]+-[a-z0-9_]+)\.tar\.gz", nome)
    if not achado:
        sys.exit(f"nome fora do padrão do release: {nome}")
    versao, plataforma = achado.groups()
    with open(pacote, "rb") as f:
        dados_do_pacote = f.read()
    with open(os.path.join(saida, nome), "wb") as f:
        f.write(dados_do_pacote)
    minisig = assinador(pasta)
    manifesto = {
        "formato": 1,
        "app": "vitral",
        "versao": versao,
        "commit": "0" * 40,
        "data": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "pagina": f"https://github.com/VictorMenegate/vitral/releases/tag/v{versao}",
        "formato_dados": 2,
        "notas": f"- **Versão de teste {versao}.** Publicada por um release falso no runner, assinada com uma chave de teste.",
        "arquivos": {plataforma: {"nome": nome, "tamanho": len(dados_do_pacote), "sha256": hashlib.sha256(dados_do_pacote).hexdigest()}},
    }
    dados = json.dumps(manifesto, indent=2, ensure_ascii=False).encode()
    with open(os.path.join(saida, "vitral-versao.json"), "wb") as f:
        f.write(dados)
    with open(os.path.join(saida, "vitral-versao.json.minisig"), "w") as f:
        f.write(minisig(dados, f"vitral {versao} vitral-versao.json"))
    print(f"{versao} ({plataforma}): {len(dados_do_pacote)} bytes em {saida}; chave {os.path.join(pasta, 'bancada.pub')}")


def servir(pasta, porta, direto=False):
    pedidos = {"manifesto": 0}
    registro = open(os.path.join(pasta, "pedidos.log"), "a", buffering=1)

    class Release(http.server.BaseHTTPRequestHandler):
        def log_message(self, *args):
            pass

        def vazio(self, codigo):
            self.send_response(codigo)
            self.send_header("Content-Length", "0")
            self.end_headers()

        def enviar(self, dados, lento=False):
            self.send_response(200)
            self.send_header("Content-Length", str(len(dados)))
            self.end_headers()
            bloco = 256 * 1024 if lento else len(dados) or 1
            for i in range(0, len(dados), bloco):
                self.wfile.write(dados[i : i + bloco])
                self.wfile.flush()
                if lento:
                    time.sleep(0.25)

        def do_GET(self):
            nome = self.path.removeprefix("/r/")
            registro.write(f"{time.time():.1f} {nome}\n")
            if nome == "vitral-versao.json" and direto:
                time.sleep(4)
                arquivo = os.path.join(pasta, "r", nome)
            elif nome == "vitral-versao.json":
                pedidos["manifesto"] += 1
                if pedidos["manifesto"] == 1:
                    return self.vazio(503)
                arquivo = os.path.join(pasta, "adulterado.json" if pedidos["manifesto"] == 2 else "r/vitral-versao.json")
            else:
                arquivo = os.path.join(pasta, "r", nome)
            if "/" in nome or not os.path.isfile(arquivo):
                return self.vazio(404)
            with open(arquivo, "rb") as f:
                self.enviar(f.read(), lento=nome.endswith(".tar.gz"))

    http.server.ThreadingHTTPServer(("127.0.0.1", porta), Release).serve_forever()


if __name__ == "__main__":
    if len(sys.argv) == 3 and sys.argv[1] == "gerar":
        gerar(sys.argv[2])
    elif len(sys.argv) == 5 and sys.argv[1] == "publicar":
        publicar(sys.argv[2], sys.argv[3], sys.argv[4])
    elif len(sys.argv) in (4, 5) and sys.argv[1] == "servir" and sys.argv[4:] in ([], ["--direto"]):
        servir(os.path.abspath(sys.argv[2]), int(sys.argv[3]), direto=sys.argv[4:] == ["--direto"])
    else:
        sys.exit(__doc__)
