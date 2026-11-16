---
category: Conversa
---
# Pílula de arquivo

`<PilulaDeArquivo>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Um arquivo na conversa, como a extensão do Claude Code mostra: uma pílula de 28 com a miniatura (ou o ícone), o nome e as medidas da imagem (ou o tamanho do arquivo); o clique numa imagem abre o visor.

## Anatomia
- Altura 28, canto `raio-de-28`, padding 4 à esquerda com imagem e 8 sem, 8 à direita, vão `esp-6`, texto `metadado`.
- Miniatura de 20 de altura (até 44 de largura) no `preto`, canto `raio-controle`.
- Nome em `texto`, até 220 de largura, com reticências; medida em `tenue` ("601×263", "184 KB").
- Fora do balão: fundo `cartao-elevado` e fio `borda-forte`. **Dentro do seu balão**: o poço escuro (`recuo` com fio `borda`), da família do código do balão: o cinza opaco parecia colado de fora no azul.
- O que saiu do disco fica apagado: ImageOff ou FileX em `apagado`, nome em `tenue`, "sumiu".
- Hover `hover` (só nas imagens, que abrem). A dica tem o nome, a medida e o caminho.

## Onde aparece
Os anexos das suas mensagens, as imagens que a resposta aponta e a que uma chamada leu ou gravou (embaixo da linha dela). Decisão do dono, 16/11/2026: "faz as imagens que mando e a que você usa ficar igual fica aqui"; as imagens da conversa viraram pílulas.

Fonte: `src/ui/mensagens.rs` (`pilula_de_arquivo`), `src/ui/imagens.rs` (`ALTURA_CHIP`, `LARGURA_MAXIMA_CHIP`).

## Em React

Uma por arquivo; `noBalao` dentro do BalaoDoUsuario.

```jsx
<PilulaDeArquivo imagem={url} nome="colado-1c5399bf6e.png" medida="410×165" />
<PilulaDeArquivo nome="relatorio.pdf" medida="184 KB" />
<PilulaDeArquivo sumiu imagem={url} nome="captura-antiga.png" />
```
