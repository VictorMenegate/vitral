---
category: Base
---
# Selo

`<Selo>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O selo pequeno: uma contagem, um rótulo de uma ou duas palavras ou o número do painel. Texto `micro` (11/14, 600, tabular), canto `raio::de` da altura.

## Tipos
- **Contagem** (18, `min_w` 18, padding 4): neutra (`traco` com `tenue`), tingida (a tinta da cor com o número na cor) ou sólida (a cor cheia com `sobre-cor`, só quando fica por cima de algo). Acima de 99, "99+".
- **Recortado** (o sino, o logo recolhido): 16, com o aro de 2 no escuro da base (`aro-de-recorte`): do tamanho de um contador, separado do botão por um vão escuro (relatório de aparência, V16).
- **Cor** (20, padding 6): "1 falhou" na tinta `vermelho`, "rápido", "beta".
- **Novo**: o destaque a 18% com `destaque-texto`.
- **Neutro**: contorno `borda`, `tenue` ("1M de contexto", "padrão").
- **Capacidades**: ícones de 14 separados por divisória, cada um com a dica.
- **Ultrathink** (arco-íris) e **Ultracode** (`violeta`, Workflow 12) no `grupo`: a mensagem foi com o reforço.
- **Número do painel** (Alt+1…4): tecla de 18 no `recuo` com fio `borda` e algarismo `tenue`; no painel ativo, fio `tinta-borda-destaque` e `destaque-texto`. Nunca o disco azul cheio: "Nova conversa 1" se lia como "1 não lida" (relatório de design, D1).

## Regras
- O disco cheio fica só para aviso (o sino, o logo). Contagem que repete o cabeçalho não aparece.

Fonte: `src/ui/componentes/estado.rs` (`Selo`, `numero_painel`), `src/ui/mensagens.rs` (`selo_ultrathink`, `selo_ultracode`).

## Em React

`tipo` escolhe contagem, rótulo ou número do painel; `tom` a cor (espera é o azul-claro do que precisa de você).

```jsx
<Selo tipo="contagem" tom="espera">2</Selo>
<Selo tom="vermelho">1 falhou</Selo>
<Selo tom="ultracode" />
<Selo tipo="painel" ativo>1</Selo>
```
