---
category: Base
---
# Campo de busca

`<CampoDeBusca>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A busca da lista e dos painéis: um poço de 32 de canto `raio-de-32`, sem fio, desenhado pela sombra que cai de cima para dentro e por 1 px de luz na quina de baixo (`aresta-de-poco`).

## Estados
- **Vazio e sem foco**: a lupa de 14 em `fantasma`, o placeholder em `tenue` e, à direita, as teclas do atalho do keymap ("Ctrl" "K"), com a lista a partir de 276 px.
- **Com foco** ("é aqui"): um aro de metal de 2 px em volta de um miolo opaco `preto`, sem azul chapado nem halo. O mesmo aro da caixa de mensagem e da aba sob o ponteiro.
- Com texto: 14 px em `texto`, com o limpar do kit.

## Medidas
32 de altura (40 no pé da lista, a do "+" de metal ao lado), padding 8 à esquerda e 6 à direita: a lupa fica a 8 da borda de fora e, numa peça de 8, cai na guia de 16 da lista.

## Variante plana
Dentro de uma lista de escolha, a busca é plana: 44 de altura, padding 16, sem fundo, com a lupa de 16 em `tenue` e um fio cheio `borda` embaixo.

Fonte: `src/ui/componentes/controles.rs` (`campo_de_busca`, `campo_de_busca_de`, `campo_de_busca_plano`), `src/ui/lateral.rs` (`linha_da_busca`).

## Em React

Sobre a base (a lista) ou dentro de um painel; `focada` mostra o aro de metal; `plana` é a de dentro dos popovers. Com `onChange` vira input.

```jsx
<Metal tipo="base" style={{ padding: 12 }}>
  <CampoDeBusca style={{ width: 260 }} />
  <CampoDeBusca focada valor="pesagem" style={{ width: 260 }} />
</Metal>
```
