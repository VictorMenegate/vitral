---
category: Marca
---
# Logo

`<Logo>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O V do dono (uma gota à esquerda ligada por um pescoço fino à perna grossa da direita), em branco, sobre um quadrado de metal líquido de cantos 45%, o mesmo `raio::de` dos controles. É o mesmo em todo lugar.

## Onde aparece
- **Botão do menu**, no alto da lista (44 de lado): a malha viva (a receita do metal calculada ponto a ponto, com o grão exato) com o V por cima (`vitral-v.png`). Com o ponteiro em cima ou o menu aberto, a malha anda a 20 quadros por segundo e a luz segue o ponteiro; ganha um aro de 2 em `borda` no hover e `borda-forte` aberto. Uma versão nova pronta põe um ponto azul de 8 embaixo; no modo recolhido, leva o selo do sino.
- O cabeçalho do menu do Vitral (20), as mensagens do Vitral na conversa (20), a tela de recuperação e o ícone do app.

## Arquivos
Os PNGs de 48 a 256 (`vitral-*.png`) e o V sozinho (`vitral-v.png`, 256, o V branco com uma cópia escura deslocada por baixo, para continuar legível sobre a bolha clara) estão no grupo **Logo**. Saem de `ferramentas/logo/` (`rasterizar.py` e `icones.py`, com o Pillow). O metal tem grão e não se descreve em vetor: os dois SVG do app (`vitral.svg`, `vitral-pequeno.svg`) são só um PNG embutido.

## Regras
- Nunca redesenhar o V: usar os arquivos.
- O último exemplo do preview recria o botão com o metal parado em CSS e o V de verdade por cima; no app o metal é vivo.

Fonte: `src/ui/trilho.rs` (`BotaoLogo`, `LOGO`, `LOGO_V`), `src/ui/componentes/malha.rs`, `assets/vitral-*.png`.

## Em React

O PNG pronto em qualquer tamanho; `forma="metal"` para o botão do menu.

```jsx
<Logo tamanho={44} />
<Logo forma="metal" tamanho={64} />
```
