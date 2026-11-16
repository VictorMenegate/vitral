---
category: Base
---
# Teclas

`<Teclas>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Um desenho só de tecla de atalho em todo o app (dicas, menus, rodapés de popover, a busca): cada tecla separada, 18 de altura e no mínimo 18 de largura, sem borda, com fundo claro a 8% (`hover`) em relevo (`aresta-de-tecla`) e a letra em mono 10 `tenue` (estilo `tecla`).

## Regras
- O atalho sai do keymap, nunca escrito à mão: "Ctrl" "B", "Shift" "Tab", "Esc", "Page Up".
- Canto `raio-de-18` (8). O relatório de design (D14) achou três raios diferentes entre o menu, a dica e o app: o certo é este.
- Ao lado de um texto curto em `metadado` `tenue` ("Shift Tab troca", "Enter usa"), com 6 entre as teclas e o texto.
- O "/" e o "@" do menu "⋯" não são teclas desenhadas: não são atalho.

Fonte: `vendor/gpui-component/src/kbd.rs` (`desenho_de_teclas`), `src/ui/componentes/superficies.rs` (`desenho_teclas`, `teclas`).

## Em React

Uma tecla por item; `rotulo` diz o que o atalho faz.

```jsx
<Teclas teclas={['Ctrl', 'K']} />
<Teclas teclas={['Shift', 'Tab']} rotulo="troca" />
```
