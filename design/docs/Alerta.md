---
category: Base
---
# Alerta

`<Alerta>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O alerta em linha, para o que precisa ficar na tela (aviso de painel, validação de formulário): o ícone na cor do tom e o texto, no mesmo material leve do cartão de erro.

## Tons
| Tom | Ícone | Fundo e fio |
| --- | --- | --- |
| Info | Info em `tenue` | `hover` e `borda` |
| Atenção | TriangleAlert em `ambar` | `tinta-leve-ambar` e `tinta-borda-leve-ambar` |
| Erro | CircleX em `vermelho` | `tinta-leve-vermelho` e `tinta-borda-leve-vermelho` |
| Pronto | CircleCheck em `verde` | a tinta leve do verde |

## Anatomia
Padding 12 contando o fio, canto `raio-cartao`, vão `esp-8`; ícone de 16 numa linha de 20; título opcional 14/20 em 600 `texto`; texto 13/18 `tenue`; ação embaixo (6 de respiro) e o × de 20 à direita. **Compacto**: numa linha, sem caixa, o ícone de 14 na cor e o texto `metadado` `tenue`.

A cor fica no ícone e num véu de 6%, como no cartão de erro (16/11/2026; era a tinta de 12%).

Os **avisos** (toasts) usam o mesmo `Tom`: no topo e no centro, até 3, 380 de largura; o de erro fica até fechar.

Fonte: `src/ui/componentes/superficies.rs` (`Alerta`, `Tom`, `aviso`, `aviso_com_acao`).

## Em React

`tom` (info, atencao, erro, pronto), `titulo` opcional e o texto como filho; `compacto` tira a caixa.

```jsx
<Alerta tom="erro" titulo="Pasta não encontrada" onFechar={fechar}>
  Não estou conseguindo gravar. Nada novo começa até voltar a gravar.
</Alerta>
<Alerta tom="atencao" compacto>O uso do plano passou de 70% na janela de 5 h.</Alerta>
```
