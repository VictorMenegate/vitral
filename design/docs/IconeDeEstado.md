---
category: Lista
---
# Ícone de estado

`<IconeDeEstado>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O ícone de um chat é o estado dele, na aba, na linha da lista, no sino e no histórico: o balão igual em todos não dizia nada (dono, 13/11/2026: "tô achando esse ícone meio inútil").

## Os estados (na ordem de prioridade: o primeiro que valer ganha)
| Sinal | Desenho | Cor |
| --- | --- | --- |
| Erro | disco `estado-erro` com ponto | `vermelho` |
| Pedido de permissão, parada que pede Retomar | disco `estado-espera` com ponto | `espera` (azul-claro; dono, 13/11/2026: "por que tem umas coisas amarelas? tem que ser azul") |
| Resposta nova, não lida | disco `estado-nova` com ponto | `destaque` |
| Rodando | o anel girando | `destaque` |
| Parado e lido | o balão MessageCircle | `apagado` |

## Medidas
Caixa de 20 (16 na aba); disco e anel a 0,8 da caixa e o balão a 2 a menos: os três com 15 a 16 de desenho numa coluna só (relatório de aparência, V10). O ponto no meio do disco tem 0,45 da caixa (9 na lista).

## Pontos e pílula
- **Ponto de sinal**: 8 solto ou no canto, 6 dentro do texto; o de rodando é um aro de 2 que respira de 1 a 0,55 em 1,6 s.
- **Pílula de estado**: 22 de altura, padding 8, ponto de 6, rótulo `metadado` em 500, sempre minúsculo (é adjetivo), sobre a tinta da cor (`opacidade-tinta`); o detalhe depois de " · " em `tenue`. "rodando", "verificando", "esperando você", "na fila", "em revisão", "pronto", "falhou", "parada".
- A cor `espera` fica só para o que precisa de você; a revisão anda na família do rodando. O âmbar nunca marca "esperando".

Fonte: `src/ui/componentes/estado.rs` (`Sinal`, `IconeDeEstado`, `PontoDeSinal`, `PilulaDeEstado`, `sinal_do_chat`).

## Em React

`estado` é o sinal do chat; `forma` troca o ícone pelo ponto ou pela pílula.

```jsx
<IconeDeEstado estado="espera" />
<IconeDeEstado estado="rodando" tamanho={16} />
<IconeDeEstado forma="pilula" estado="rodando" detalhe="2 min 13 s" />
```
