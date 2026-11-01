#!/usr/bin/env python3
"""Mede o contraste de um texto (ou ícone) num print da bancada.

Uso:
  python3 -I ferramentas/medir_print.py <print.png> X Y [X Y ...]
  python3 -I ferramentas/medir_print.py <print.png> --pontos <arquivo>

No arquivo de pontos, uma medida por linha: "nome X Y [raio] [mínimo]" (o resto depois de # é
comentário). O raio (padrão 6) é a metade do quadrado olhado em volta do ponto; o mínimo
(padrão 4.5, texto abaixo de 18 px; use 3 para ícones e anéis) decide o "ok".

No quadrado, o fundo é a cor que mais aparece e o texto é o pixel que mais contrasta com ela
(o miolo da letra, sem a borda suavizada). O contraste é o da WCAG 2.
"""

import sys
from collections import Counter

from PIL import Image


def luminancia(rgb):
    def canal(c):
        c = c / 255
        return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4

    r, g, b = (canal(c) for c in rgb[:3])
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contraste(a, b):
    la, lb = sorted((luminancia(a), luminancia(b)), reverse=True)
    return (la + 0.05) / (lb + 0.05)


def medir(imagem, x, y, raio):
    largura, altura = imagem.size
    caixa = (max(x - raio, 0), max(y - raio, 0), min(x + raio + 1, largura), min(y + raio + 1, altura))
    dados = imagem.crop(caixa).tobytes()
    pixels = [tuple(dados[i:i + 3]) for i in range(0, len(dados), 3)]
    if not pixels:
        raise ValueError(f"ponto ({x}, {y}) fora do print de {largura}x{altura}")
    fundo = Counter(pixels).most_common(1)[0][0]
    texto = max(pixels, key=lambda p: contraste(p, fundo))
    return contraste(texto, fundo), texto, fundo


def hexa(rgb):
    return "#{:02x}{:02x}{:02x}".format(*rgb)


def pontos_do_arquivo(caminho):
    for numero, linha in enumerate(open(caminho, encoding="utf-8"), 1):
        linha = linha.split("#", 1)[0].split()
        if not linha:
            continue
        if len(linha) < 3:
            sys.exit(f"{caminho}:{numero}: esperava 'nome X Y [raio] [mínimo]'")
        nome, x, y = linha[0], int(linha[1]), int(linha[2])
        raio = int(linha[3]) if len(linha) > 3 else 6
        minimo = float(linha[4]) if len(linha) > 4 else 4.5
        yield nome, x, y, raio, minimo


def main():
    args = sys.argv[1:]
    if len(args) < 3:
        sys.exit(__doc__)
    imagem = Image.open(args[0]).convert("RGB")
    if args[1] == "--pontos":
        pontos = list(pontos_do_arquivo(args[2]))
    else:
        numeros = [int(a) for a in args[1:]]
        if len(numeros) % 2:
            sys.exit("os pontos vão em pares: X Y")
        pontos = [(f"{x},{y}", x, y, 6, 4.5) for x, y in zip(numeros[::2], numeros[1::2])]
    abaixo = 0
    for nome, x, y, raio, minimo in pontos:
        valor, texto, fundo = medir(imagem, x, y, raio)
        ok = valor >= minimo
        abaixo += not ok
        print(f"{nome}: {valor:.2f}:1 ({hexa(texto)} sobre {hexa(fundo)}) {'ok' if ok else f'abaixo de {minimo}'}")
    sys.exit(1 if abaixo else 0)


if __name__ == "__main__":
    main()
