---
category: Escolha
---
# Menu com placa

`<MenuComPlaca>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Os menus com cabeçalho de objeto (o do logo, o "…" de um chat) começam numa placa de metal com o símbolo, o nome em branco e o detalhe em mono; os itens têm ícone, rótulo e as teclas do atalho, e o pé é uma faixa escura com a linha técnica.

## Anatomia
- Casca: `flutuante` opaco, canto `raio-peca`, `sombra-popover` (o anel claro de 1 colado, o fio preto de 2 por fora, a luz no topo). Sem anel azul.
- **Placa** de 52 (`altura-linha-2`), padding 16, colada no topo e nos lados, com os cantos de cima do menu: o miolo do metal debaixo do `veu-de-placa` e a `aresta-de-ficha`. Em cima dela só `forte` (o nome, 14/20 em 600) e `texto` (o detalhe em `etiqueta` mono). O símbolo de 20 cai na coluna dos ícones.
- **Itens** de 32, margem 8 do menu (os cantos de 10 ficam concêntricos com o de 20), 2 entre eles, padding 10, ícone de 16 em `tenue` e 10 até o rótulo 14/20. Item sob o ponteiro ou o teclado: `hover` com o relevo de tecla. Perigoso: rótulo `vermelho`, realce `perigo-hover`. O que abre fora do app leva o ExternalLink à direita.
- **Teclas** do atalho à direita, uma por tecla (`Tecla`): "Ctrl" "B".
- **Separador**: o fio de pontos claro a 14%, recuado até a coluna dos ícones, 6 em cima e embaixo. Nunca linha cheia.
- **Rodapé**: faixa de 32 no `preto` a 45% com `aresta-de-poco`, os cantos de baixo do menu, em `etiqueta` `apagado` ("Vitral 0.7.0 · pc").
- Largura de 220 a 360.

## No código Rust (para quem implementa)
`cabecalho_objeto(simbolo, nome, detalhe)`, os `PopupMenuItem` com ícone e ação (o atalho sai do keymap), `rodape_menu(texto, atalho)` ou `rodape_menu_com_dica(texto, dica)`.

Fonte: `src/ui/componentes/superficies.rs` (`placa_de_metal`, `placa_de_cabecalho`, `cabecalho_objeto`, `rodape_menu`), `vendor/gpui-component/src/menu/popup_menu.rs` (medidas marcadas "Vitral:"), `src/ui/trilho.rs` (menu do logo).

## Em React

`itens` com ícone, rótulo e teclas; a string "separador" põe o fio de pontos.

```jsx
<MenuComPlaca
  titulo="Vitral"
  detalhe="25 chats"
  rodape="Vitral 0.7.0 · pc"
  itens={[
    { icone: 'message-square-plus', rotulo: 'Novo chat', teclas: ['Ctrl', 'N'] },
    { icone: 'folder-open', rotulo: 'Novo chat em outra pasta…' },
    'separador',
    { icone: 'settings', rotulo: 'Configurações', teclas: ['Ctrl', ','] },
  ]}
/>
```
