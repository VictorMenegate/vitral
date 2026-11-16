---
category: Painel
---
# Aba de painel

`<AbaDePainel>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A aba do chat no cabeçalho de um painel: 32 de altura, canto `raio-de-32`, um fio preto de 1 px (`preto`) e um miolo opaco; a aba à vista tem o material do campo da caixa de mensagem, as outras são lisas.

## Quando usar
- Uma aba por chat aberto no painel, na ordem em que abriram. A à vista para em 280 de largura quando há outras ao lado; a de fundo, em 180.
- Sem lugar para quatro letras e reticências, a aba de fundo vira **só o ícone**, num quadrado de 32 centrado, sem o lugar do ×: o título vai para a dica (dono, 16/11/2026: com o lugar do × reservado ela virava uma pílula de 80 com o ícone num canto). Abas que não cabem nem assim saem para o botão "+N".

## No código Rust (para quem implementa)
`Aba::new(id, titulo)` com `.simbolo(Simbolo)` (o ícone de estado do chat, nunca um balão igual para todos), `.numero(n, ativo)` (o número do painel, com vários painéis e a lista à vista), `.tom(TomAba)`, `.estado(Option<Sinal>)` (ponto de 8 no lugar do ×), `.fechar(dica, sempre, f)`, `.quadrada(bool)`, `.arrastando(bool)`, `.foco(&FocusHandle)`. A aba vai dentro de `casca_da_aba()`.

## Estados
- **À vista** (`TomAba::Ativa`): miolo `cartao-elevado` com a `linha-de-luz` no topo, texto `forte`.
- **De fundo** (`TomAba::Inativa`): miolo `aba-inativa`, texto `tenue`, que clareia para `texto` no hover.
- **Sob o ponteiro**: o aro de metal de 2 px acende em volta (o mesmo "é aqui" da caixa e da busca).
- **Número do painel** à direita, antes do × (dono, 13/11/2026): tecla de 18 no `recuo`; no painel ativo, fio `tinta-borda-destaque` e algarismo `destaque-texto`. Nunca o disco azul cheio, que é desenho de aviso.
- **×**: sempre à vista só na aba do painel ativo com vários painéis; nas outras, no hover e no foco. O clique do meio fecha.
- **Arrastando**: a aba de origem a `opacidade-desabilitado`, tracejada em `borda-forte`.

## Regras
- O contorno é o fio preto de 1 px, numa linha só (dono, 16/11/2026: "diminui as bordas"); sem casca dobrada em volta.
- O ícone da aba É o estado do chat (`icone_de_estado`): balão apagado, anel girando, disco `espera`, `destaque` ou `vermelho`.
- Rótulo em `rotulo` (13/500), truncado com reticências.

Fonte: `src/ui/componentes/paineis.rs` (`Aba`, `casca_da_aba`), `src/ui/conversa.rs` (`abas_do_painel`).

## Em React

Vai dentro do CabecalhoDoPainel. `estado` vira o ícone; `ativa` é a à vista; `quadrada` só o ícone; `painel` e `painelAtivo` o número.

```jsx
<CabecalhoDoPainel>
  <AbaDePainel quadrada titulo="Alertas de cupons" />
  <AbaDePainel quadrada titulo="WMS" estado="espera" />
  <AbaDePainel ativa titulo="Pesagem de caixas" painel={1} painelAtivo fechar />
</CabecalhoDoPainel>
```
