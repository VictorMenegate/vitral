---
category: Lista
---
# Cartão de lista

`<CartaoDeLista>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A linha de um chat na lista corrida: o ícone de estado, o título e a meta à direita (o número do painel em que está aberto, ou há quanto tempo foi atualizado); no hover, as ações tomam o lugar da meta.

## Anatomia
- Altura `altura-linha-de-chat` (36; dono, 13/11/2026: "deixa esses chats um pouco maior"), padding 8 contando o fio, vão `esp-8`, canto `raio-controle`. Na lista, 2 entre os cartões.
- **Ícone** de 20: o estado do chat (`IconeDeEstado`). Sem o ponto no canto: o ícone já é o estado.
- **Título** 15/20 em 400 na cor `texto` (relatório de aparência, V8: em 500 a lista pesava mais que a conversa); não lido em 600 e `forte`; selecionado em `forte`.
- **Meta** (`metadado`, tabular): o chat aberto num painel mostra só o número do painel (tecla de 18; azul no painel ativo) e a hora vai para a dica do ícone; fora de painel, a hora ("3 h", "4 d") em `tenue` (`texto` no selecionado); rodando, há quanto tempo em `destaque-texto`. O alfinete (11, `apagado`) diz por que o chat está em cima.
- **Selecionado**: fundo `selecao` e a barra de 3 em `destaque` encostada à esquerda; aberto em outro painel, a barra a 40%.
- **Hover**: `hover`; as ações (abrir ao lado, o "⋯"), botões de 24, aparecem no lugar da meta. **Realce do teclado**: fio `destaque-borda`.
- Na busca que achou no texto, uma segunda linha volta (52 de altura) com a pasta e o trecho em forte.

## No código Rust (para quem implementa)
`CartaoDeLista::new(id, titulo)` com `.simbolo(..)`, `.sinal(..)`, `.titulo_rico(StyledText)`, `.subtitulo(..)`, `.meta(..)`, `.meta_inferior(..)`, `.acao(..)` (até 2), `.acao_fixa(..)`, `.selecionado(bool)`, `.aberto_em_outro(bool)`, `.nao_lido(bool)`, `.realce(bool)`, `.uma_linha()`, `.nivel(1)` e `.ramo(..)` na árvore da Equipe.

## Ainda em discussão
O relatório de aparência (V24) propõe trocar a barra azul e o fundo claro do selecionado pela ficha de metal, como no resto do app; ainda não está no código.

Fonte: `src/ui/componentes/listas.rs` (`CartaoDeLista`), `src/ui/lateral.rs` (`cartao_conversa`, `hora_do_cartao`).

## Em React

As linhas da lista de chats, que mora direto na base (Metal tipo="base"), 2 entre elas, recuo de 8.

```jsx
<Metal tipo="base" style={{ width: 312, padding: 8, display: 'flex', flexDirection: 'column', gap: 2 }}>
  <CartaoDeLista titulo="WMS" estado="rodando" selecionado painel={2} painelAtivo />
  <CartaoDeLista titulo="Alertas de cupons" estado="nova" naoLido hora="2 h" />
  <CartaoDeLista titulo="Pesagem de caixas" estado="erro" hora="3 d" />
</Metal>
```
