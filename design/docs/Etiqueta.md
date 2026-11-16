---
category: Base
---
# Etiqueta

`<Etiqueta>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A etiqueta de fio: um fato curto em mono 11 numa pílula de contorno (a pasta, o ramo, "revisar", "saída", o relógio). Não é botão nem selo de contagem: é o que um aparelho escreveria do lado do mostrador.

## Anatomia
- 20 de altura (16 na `baixa`, para caber numa linha de lista), padding `esp-6`, vão `esp-4`, canto `raio-de-20`.
- Fio `borda-forte` e texto `tenue` em `etiqueta` (JetBrains Mono 11/14, tabular).
- Ícone de 11 em `apagado` antes do texto, opcional.
- **Tom**: o fio na tinta da cor e o texto na cor ("direto" em `ambar`).
- **Com fundo**: o `recuo`, para quem fica por cima de texto ou de pontilhado.

Fonte: `src/ui/componentes/estado.rs` (`Etiqueta`).

## Em React

Um fato curto em mono, com ícone de 11 opcional.

```jsx
<Etiqueta icone="folder">~/eita</Etiqueta>
<Etiqueta tom="ambar">direto</Etiqueta>
```
