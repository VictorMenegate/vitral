#!/usr/bin/env python3
"""Release falso do Vitral para a bancada da atualização (ferramentas/roteiros/dist-atualizacao.txt).

  python3 -I ferramentas/release_falso.py gerar <pasta>
      Uma chave só da bancada (Ed25519 pelo openssl, guardada em <pasta>/chave.pem; nunca a do
      release), o pacote 0.9.0 de uns 20 MB com um "vitral" de mentira que só responde ao
      --versao --json, o vitral-versao.json assinado no formato do minisign (pré-hash blake2b-512),
      a pública em <pasta>/bancada.pub e um manifesto adulterado.

  python3 -I ferramentas/release_falso.py servir <pasta> <porta>
      Serve em http://127.0.0.1:<porta>/r/. O manifesto responde, na ordem dos pedidos: 503 (sem
      conexão), o adulterado (a assinatura não confere) e depois o certo. O pacote sai a 1 MB/s,
      para o "Baixando" aparecer. Os pedidos ficam em <pasta>/pedidos.log.

O app só aceita a chave da bancada no build de desenvolvimento, com VITRAL_ATUALIZACAO_BASE e
VITRAL_ATUALIZACAO_CHAVE=<pasta>/bancada.pub.
"""
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


def gerar(pasta):
    pasta = os.path.abspath(pasta)
    r = os.path.join(pasta, "r")
    os.makedirs(r, exist_ok=True)
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


def servir(pasta, porta):
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
            if nome == "vitral-versao.json":
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
    elif len(sys.argv) == 4 and sys.argv[1] == "servir":
        servir(os.path.abspath(sys.argv[2]), int(sys.argv[3]))
    else:
        sys.exit(__doc__)
