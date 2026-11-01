#!/usr/bin/env python3
"""Gera <dist>/vitral-versao.json: o manifesto que o Vitral lê para saber da versão nova.

Uso (no release.yml): manifesto.py <dist> <versão> <tag> <dono/repo> <commit> <notas.md> <src/modelo.rs>
"""
import datetime, hashlib, json, os, re, sys

PLATAFORMAS = ("linux-x86_64", "windows-x86_64")


def soma(caminho):
    h = hashlib.sha256()
    with open(caminho, "rb") as f:
        for bloco in iter(lambda: f.read(1 << 20), b""):
            h.update(bloco)
    return h.hexdigest()


def main():
    if len(sys.argv) != 8:
        sys.exit(__doc__.strip())
    dist, versao, tag, repo, commit, notas, modelo = sys.argv[1:8]
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?", versao):
        sys.exit(f"versão inválida: {versao}")
    if tag != f"v{versao}":
        sys.exit(f"o tag {tag} não bate com a versão {versao}")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        sys.exit(f"commit inválido: {commit}")
    arquivos = {}
    for plataforma in PLATAFORMAS:
        nome = f"vitral-{versao}-{plataforma}.tar.gz"
        caminho = os.path.join(dist, nome)
        if os.path.exists(caminho):
            arquivos[plataforma] = {"nome": nome, "tamanho": os.path.getsize(caminho), "sha256": soma(caminho)}
    if "linux-x86_64" not in arquivos:
        sys.exit("falta o pacote do Linux")
    with open(modelo, encoding="utf-8") as f:
        achado = re.search(r"^pub const FORMATO_DADOS: u32 = (\d+);", f.read(), re.M)
    if not achado:
        sys.exit("não achei FORMATO_DADOS em " + modelo)
    with open(notas, encoding="utf-8") as f:
        texto = f.read().strip()
    if not texto:
        sys.exit("as notas da versão estão vazias")
    manifesto = {
        "formato": 1,
        "app": "vitral",
        "versao": versao,
        "commit": commit,
        "data": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "pagina": f"https://github.com/{repo}/releases/tag/{tag}",
        "formato_dados": int(achado.group(1)),
        "notas": texto,
        "arquivos": arquivos,
    }
    with open(os.path.join(dist, "vitral-versao.json"), "w", encoding="utf-8") as f:
        json.dump(manifesto, f, ensure_ascii=False, indent=2)
        f.write("\n")


if __name__ == "__main__":
    main()
