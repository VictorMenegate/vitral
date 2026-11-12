# Porta exata (para esta receita) do shader "Mesh drift" da 21st.dev, para ver como fica no tamanho
# do botão do menu do Vitral. Só a prévia: o Vitral desenha a mesma conta em Rust.
import math, sys
from PIL import Image, ImageDraw

CORES = [(0.090, 0.090, 0.090), (0.569, 0.773, 1.000), (0.227, 0.506, 0.965), (0.145, 0.388, 0.937), (0.102, 0.306, 0.855)]
ESCALA, INTENSIDADE = 1.26, 0.35
CONTRASTE, BRILHO, SATURACAO = 1.37, -0.02, 0.68
VINHETA, GRAO = 0.54, 0.22
SEMENTE, DERIVA = 1.0, 0.06
DESLOC = (-0.05, 0.0)
FORCA_PONTEIRO, RAIO_PONTEIRO = 0.50, 0.80

def fract(x): return x - math.floor(x)
def smoothstep(a, b, x):
    t = min(1.0, max(0.0, (x - a) / (b - a)))
    return t * t * (3 - 2 * t)

def grao(x, y):
    p3 = [fract(x * 0.1031), fract(y * 0.1031), fract(x * 0.1031)]
    d = p3[0] * (p3[1] + 33.33) + p3[1] * (p3[2] + 33.33) + p3[2] * (p3[0] + 33.33)
    p3 = [v + d for v in p3]
    return fract((p3[0] + p3[1]) * p3[2])

def pixel(fx, fy, w, h, t, ponteiro=None, com_grao=True, vinheta=VINHETA, cru=False):
    """fx, fy: centro do pixel em coordenadas do GL (origem embaixo à esquerda)."""
    m = min(w, h)
    px, py = (fx - 0.5 * w) / m, (fy - 0.5 * h) / m
    mascara = 0.0
    if ponteiro is not None:
        cx, cy = 0.5 * ponteiro[0] * w / m, 0.5 * ponteiro[1] * h / m
        mascara = 1.0 - smoothstep(0.0, RAIO_PONTEIRO, math.hypot(px - cx, py - cy))
    qx, qy = px * ESCALA + DESLOC[0], py * ESCALA + DESLOC[1]
    qx += DERIVA * math.sin(t * 0.31); qy += DERIVA * math.cos(t * 0.23)
    acc = [c * 0.15 for c in CORES[0]]; total = 0.15
    for i, cor in enumerate(CORES):
        ax = math.sin(t * (0.21 + i * 0.071) + i * 2.4 + SEMENTE) * (0.45 + INTENSIDADE * 0.35)
        ay = math.cos(t * (0.17 + i * 0.093) + i * 1.7) * (0.45 + INTENSIDADE * 0.35)
        wgt = math.exp(-((qx - ax) ** 2 + (qy - ay) ** 2) * 6.0)
        acc = [a + c * wgt for a, c in zip(acc, cor)]; total += wgt
    col = [a / total for a in acc]
    col = [(c - 0.5) * CONTRASTE + 0.5 for c in col]
    luma = 0.299 * col[0] + 0.587 * col[1] + 0.114 * col[2]
    col = [luma + (c - luma) * SATURACAO for c in col]
    col = [c + BRILHO for c in col]
    vd = math.hypot(fx / w - 0.5, fy / h - 0.5) * 1.41421356
    col = [c * (1.0 - vinheta * smoothstep(0.35, 1.0, vd)) for c in col]
    col = [c + (0.18 + c * 0.12) * mascara * FORCA_PONTEIRO for c in col]
    if com_grao:
        g = (grao(fx + SEMENTE * 17.0, fy + SEMENTE * 31.0) - 0.5) * GRAO
        col = [c + g for c in col]
    if cru:
        return tuple(min(1.0, max(0.0, c)) for c in col)
    return tuple(int(round(min(1.0, max(0.0, c)) * 255)) for c in col)

def quadro(n, t, ponteiro=None, **kw):
    im = Image.new("RGB", (n, n))
    for linha in range(n):
        for coluna in range(n):
            im.putpixel((coluna, linha), pixel(coluna + 0.5, n - linha - 0.5, n, n, t, ponteiro, **kw))
    return im

def botao(n, t, ponteiro=None, raio=0.30, v=True, **kw):
    """O botão: a malha recortada num quadrado de cantos bem arredondados, com o V por cima."""
    k = 8  # superamostragem do recorte e do V
    base = quadro(n, t, ponteiro, **kw).resize((n * k, n * k), Image.NEAREST)
    mascara = Image.new("L", (n * k, n * k), 0)
    ImageDraw.Draw(mascara).rounded_rectangle([0, 0, n * k - 1, n * k - 1], radius=int(raio * n * k), fill=255)
    if v:
        e = n * k / 128.0
        camada = Image.new("RGBA", base.size, (0, 0, 0, 0))
        d = ImageDraw.Draw(camada)
        d.polygon([(79 * e, 36 * e), (101 * e, 36 * e), (75 * e, 96 * e), (53 * e, 96 * e)], fill=(255, 255, 255, 140))
        d.polygon([(27 * e, 36 * e), (49 * e, 36 * e), (75 * e, 96 * e), (53 * e, 96 * e)], fill=(255, 255, 255, 255))
        base = Image.alpha_composite(base.convert("RGBA"), camada).convert("RGB")
    fundo = Image.new("RGB", base.size, (13, 15, 19))
    fundo.paste(base, (0, 0), mascara)
    return fundo

if __name__ == "__main__":
    saida = sys.argv[1]
    tempos = [0, 4, 8, 12, 16, 20, 24]
    lado = 272
    folha = Image.new("RGB", (len(tempos) * (lado + 8) + 8, 4 * (lado + 8) + 8), (13, 15, 19))
    for j, seg in enumerate(tempos):
        t = seg * 1.09
        x = 8 + j * (lado + 8)
        folha.paste(quadro(34, t).resize((lado, lado), Image.NEAREST), (x, 8))                    # 34 px cru
        folha.paste(botao(34, t).resize((lado, lado), Image.LANCZOS), (x, 8 + (lado + 8)))        # botão a 100%
        folha.paste(botao(68, t).resize((lado, lado), Image.LANCZOS), (x, 8 + 2 * (lado + 8)))    # botão a 200%
        pont = (-0.6 + 0.2 * j, 0.5 - 0.15 * j)
        folha.paste(botao(68, t, pont).resize((lado, lado), Image.LANCZOS), (x, 8 + 3 * (lado + 8)))  # com o ponteiro
    folha.save(saida)
    # No tamanho real, lado a lado, sobre a cor da lista.
    real = Image.new("RGB", (len(tempos) * 46 + 8, 50), (13, 15, 19))
    for j, seg in enumerate(tempos):
        real.paste(botao(34, seg * 1.09).resize((34, 34), Image.LANCZOS), (8 + j * 46, 8))
    real.save(saida.replace(".png", "-real.png"))
