---
category: Base
---
# Botão redondo

`<BotaoRedondo>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O botão só com ícone de canto `raio::de(diâmetro)`, sempre com dica: o "+" da lista e o enviar são teclas de metal; o resto é neutro ou plano.

## Variantes
- **Metal** (o + do pé da lista, o enviar, o parar): o metal líquido encolhido para cobrir o botão, com o `destaque` a 60% por cima (`opacidade-veu-do-botao-de-metal`), a `sombra-elevado` que o solta da base, a `aresta-de-tecla` e um fio claro a 16% (55% com o ponteiro). Ícone branco (`sobre-destaque`, 6,0:1). Apertado, 0,82. É o par do logo. Não é azul chapado.
- **Vazio** (o enviar sem texto): só o fio `borda` e a seta em `apagado`; com texto, o fundo acende e o ícone escurece em 120 ms.
- **Neutro**: `hover` em repouso (o parar ao lado do enviar, o trilho).
- **Plano**: sem fundo em repouso, ícone `tenue` (o − e o + do contador, o sino).
- **Perigo**: a tinta `vermelho`.
- **Primário** e **Claro** existem no código e quase não aparecem.

## Medidas
24, 28, 32 ou 36 (ícone de 14 até 24, 16 acima). O parar é um quadrado de 10 com canto 2,6, sem anel girando em volta (dono, 13/11/2026: "já tem nos outros lugares").

## Detalhes
- O fundo segue o ponteiro em 120 ms; o `pulso` dá um anel claro que cresce 6 px e some em 240 ms a cada envio (o sino pulsa em âmbar, ou vermelho com erro).
- O selo por cima (o sino) é o contador de 16 recortado do botão por um aro de 2 no escuro da base (`aro-de-recorte`), em `top -2, right -2`.

Fonte: `src/ui/componentes/controles.rs` (`BotaoRedondo`, `VarianteRedonda`, `anel_do_pulso`), `src/ui/composer.rs` (`botoes_de_envio`).

## Em React

`tipo="metal"` para o +, o enviar e o parar; `parar` desenha o quadrado; `selo` põe a contagem no canto.

```jsx
<BotaoRedondo tipo="metal" tamanho={36} icone="arrow-up" dica="Enviar" />
<BotaoRedondo tipo="metal" tamanho={36} parar dica="Parar (Esc)" />
<BotaoRedondo tipo="plano" tamanho={36} icone="bell" selo={2} dica="2 pedidos esperando você" />
```
