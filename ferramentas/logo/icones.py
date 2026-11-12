"""Gera os arquivos do logo do Vitral "com o shader": um quadrado de cantos bem arredondados cheio
da malha do metal líquido (a mesma conta do botão do menu, parada na fase inicial dele, com o
grão) e, por cima, o V que o dono mandou (branco, com a sombra escura por baixo).
Uso: icones.py <pasta_da_previa_py> <v.png de 256> <pasta assets>"""
import base64, io, sys
from PIL import Image, ImageDraw

sys.path.insert(0, sys.argv[1])
import previa

V = Image.open(sys.argv[2]).convert('RGBA')
ASSETS = sys.argv[3]
FASE = 4.0          # FASE_INICIAL_DA_MALHA do botão do menu
RAIO = 0.45         # o padrão de canto do app: 45% do lado
K = 4               # superamostragem do recorte e do V

def icone(n):
    malha = previa.quadro(n, FASE).resize((n * K, n * K), Image.NEAREST).convert('RGBA')
    v = V.resize((n * K, n * K), Image.LANCZOS)
    malha = Image.alpha_composite(malha, v)
    mascara = Image.new('L', (n * K, n * K), 0)
    ImageDraw.Draw(mascara).rounded_rectangle([0, 0, n * K - 1, n * K - 1], radius=int(RAIO * n * K), fill=255)
    fora = Image.new('RGBA', (n * K, n * K), (0, 0, 0, 0))
    fora.paste(malha, (0, 0), mascara)
    return fora.resize((n, n), Image.LANCZOS)

def svg_com(png, lado_declarado):
    buf = io.BytesIO(); png.save(buf, format='PNG')
    dados = base64.b64encode(buf.getvalue()).decode()
    n = png.width
    return (f'<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" '
            f'width="{lado_declarado}" height="{lado_declarado}" viewBox="0 0 {n} {n}">\n<title>Vitral</title>\n'
            f'<!-- O logo com o metal líquido e o V: um PNG de {n} px embutido (o metal tem grão e não se '
            f'descreve em vetor). Gerado por ferramentas/logo/icones.py. -->\n'
            f'<image width="{n}" height="{n}" xlink:href="data:image/png;base64,{dados}"/>\n</svg>\n')

imgs = {n: icone(n) for n in (256, 128, 64, 48, 32, 24, 16)}
for n in (256, 128, 64, 48):
    imgs[n].save(f'{ASSETS}/vitral-{n}.png')
imgs[256].save(f'{ASSETS}/vitral.ico', format='ICO', append_images=[imgs[n] for n in (128, 64, 48, 32, 24, 16)],
               sizes=[(256, 256), (128, 128), (64, 64), (48, 48), (32, 32), (24, 24), (16, 16)])
open(f'{ASSETS}/vitral.svg', 'w', encoding='utf-8').write(svg_com(imgs[256], 256))
open(f'{ASSETS}/vitral-pequeno.svg', 'w', encoding='utf-8').write(svg_com(imgs[64], 32))
folha = Image.new('RGB', (256 + 128 + 64 + 48 + 32 + 24 + 16 + 80, 276), (16, 18, 26))
x = 10
for n in (256, 128, 64, 48, 32, 24, 16):
    folha.paste(imgs[n], (x, 10), imgs[n]); x += n + 10
folha.save(sys.argv[4] if len(sys.argv) > 4 else 'folha-icones.png')
