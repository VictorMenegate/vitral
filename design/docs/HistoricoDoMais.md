---
category: Escolha
---
# Histórico do +

`<HistoricoDoMais>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O "+" do cabeçalho do painel abre esta lista: a primeira linha é o chat novo neste painel, já marcada (o Enter cria), e embaixo o histórico com busca; o escolhido aparece neste painel, como uma aba (dono, 16/11/2026: "faz esse botão ser o de + de criar mais uma conversa e ter o histórico também").

## Anatomia
- É a `ListaDeEscolha` num popover de 400 com padding 6, ancorado no canto de cima à direita do "+".
- "Novo chat neste painel" com o ícone Plus, na ficha de metal com o visto.
- Cada chat: o ícone de estado de 20 (o disco `espera`, o balão apagado), o título 14/500 e a linha de baixo em `metadado`: onde está ("no painel 3", "à vista neste painel"), há quanto tempo e a pasta.
- Escolher um chat que está à vista em outro painel ativa aquele painel (um chat só aparece num lugar).
- As abas que não couberam no cabeçalho abrem a mesma lista, só com elas, pelo botão "+N".

Fonte: `src/ui/conversa.rs` (`botao_historico`, `abrir_historico`, `escolheu_no_historico`, `botao_abas_fora`).

## Em React

Abre ancorado no canto de cima à direita do "+" do CabecalhoDoPainel.

```jsx
<HistoricoDoMais
  chats={[
    { titulo: 'Projeto Vitral', descricao: 'no painel 3 · há 35 min · eita', estado: 'espera' },
    { titulo: 'WMS', descricao: 'há 1 h · eita', estado: 'rodando' },
  ]}
/>
```
