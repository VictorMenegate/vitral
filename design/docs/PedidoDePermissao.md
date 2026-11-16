---
category: Conversa
---
# Pedido de permissão

`<PedidoDePermissao>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O que espera a sua decisão antes de o agente agir: um cartão de raio 16 dentro de uma moldura de raio 20 com 4 de folga (a moldura dupla, base AI Tool Approval).

## Anatomia
- **Moldura** (`raio-peca`, folga `esp-4`): esperando você, ela é o metal vivo (o aro de 4 px, o miolo do metal); no comando perigoso, `moldura-perigo` (vermelho a 35% misturado, nunca metal); recolhida (há outro pedido aberto), `preto` com fio `borda`.
- **Miolo**: `cartao-elevado`, canto 16, opaco: é ele que deixa só o aro de fora à vista.
- Cabeçalho: o azulejo de 28 (`hover`, canto `raio-controle`) com o ícone de 14 da ferramenta, o título em `titulo-cartao` `forte` e a etiqueta mono da ferramenta em `apagado`; "1 de 3" quando há vários.
- O pedido (comando, diff) no preto, com o trecho perigoso marcado em `vermelho`.
- Botões: Negar (fantasma) e **Permitir** no botão claro (`claro` com texto `sobre-cor`), para o azul chapado não disputar com a moldura. No perigoso, "Segure para permitir": só confirma depois de 900 ms apertado, enchendo da esquerda para a direita; pelo teclado, Enter duas vezes.

## Regras
- O "espera você" é sempre metal (o aro de 4), nunca halo azul nem borda azul.
- A cor `espera` (azul-claro) marca o chat que espera na lista, na aba e no sino.

Fonte: `src/ui/componentes/controles.rs` (`moldura_dupla`, `miolo_da_moldura`, `azulejo`, `BotaoDeSegurar`, `botao_claro`), `src/ui/mensagens.rs` (`cartao_de_pedido`).

## Em React

O pedido como filho (vai no preto, em mono); `perigoso` troca o metal pela moldura vermelha.

```jsx
<PedidoDePermissao titulo="Rodar um comando" ferramenta="Bash">$ cargo test tema</PedidoDePermissao>
<PedidoDePermissao perigoso titulo="Rodar um comando" ferramenta="Bash">
  $ rm -rf <span style={{ color: 'var(--vermelho)', background: 'var(--diff-sai)' }}>~/eita/dados</span>
</PedidoDePermissao>
```
