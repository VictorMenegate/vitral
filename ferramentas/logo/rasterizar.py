"""Tira o V da imagem que o dono mandou (forma escura em fundo branco) e gera o PNG do logo:
a forma em branco, com transparência nas bordas, e a cópia escura deslocada por baixo (para o V
continuar legível sobre a bolha clara do metal)."""
import sys
from PIL import Image

origem, saida = sys.argv[1], sys.argv[2]
LADO = 256
FRACAO = 0.60          # quanto do quadro a forma ocupa no lado maior
im = Image.open(origem).convert('L')
# alfa pelo quanto o ponto é escuro: o fundo branco some, a borda fica suave e o miolo (tudo o
# que é mais escuro que 75) sai inteiro
alfa = im.point(lambda v: max(0, min(255, int((255 - v) * 255 / 180))))
caixa = alfa.point(lambda v: 255 if v > 24 else 0).getbbox()
alfa = alfa.crop(caixa)
k = FRACAO * LADO / max(alfa.size)
novo = (max(1, round(alfa.width * k)), max(1, round(alfa.height * k)))
alfa = alfa.resize(novo, Image.LANCZOS)
mascara = Image.new('L', (LADO, LADO), 0)
mascara.paste(alfa, ((LADO - novo[0]) // 2, (LADO - novo[1]) // 2))
sombra = Image.new('RGBA', (LADO, LADO), (11, 27, 77, 0))
sombra.putalpha(mascara.point(lambda v: int(v * 0.42)))
fundo = Image.new('RGBA', (LADO, LADO), (0, 0, 0, 0))
fundo.paste(sombra, (3, 5), sombra)
branco = Image.new('RGBA', (LADO, LADO), (255, 255, 255, 0))
branco.putalpha(mascara)
Image.alpha_composite(fundo, branco).save(saida)
print('forma na origem:', caixa, '-> no quadro:', novo)
