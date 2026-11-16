---
category: Caixa
---
# Chip da caixa

`<ChipDaCaixa>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O chip é o controle da barra da caixa de mensagem: símbolo, rótulo e seta, sem fundo em repouso; serve de gatilho para o menu do modo e para os popovers do modelo e do esforço.

## Os três chips
- **Modo de permissão**: o ícone do modo (`icone_do_modo`): Manual (escudo cheio), Aceitar edições (FilePen), Planejar (Map), Automático (WandSparkles), todos em `tenue`; **Sem perguntar** (ShieldOff) em `ambar`, o único com cor, porque pede cuidado. Shift+Tab percorre os modos.
- **Modelo**: o glifo solto da marca (o coral do Claude, o degradê do Codex), sem o disco cheio (relatório de aparência, V9: o disco coral ao lado do escudo âmbar era a mancha mais forte da faixa). Na dupla, as duas marcas sobrepostas, deslocadas 6.
- **Esforço**: as barrinhas (5 barras de 2 com vão de 2, de 4 a 12 de altura) e o nível; no automático, "Auto · Extra". Ultrathink armado: barrinhas e rótulo em arco-íris. Ultracode: o ícone Workflow e o rótulo em `violeta`.

## Medidas
Altura 32 (`altura-controle`, Grande), 28 (Médio) ou 24 (Pequeno, rótulo 12); padding `esp-8`, canto `raio-de-32`, fio de 1 transparente; símbolo de 14; rótulo 13/18 em 500. Só ícone: quadrado da altura.

## Estados
- Rótulo em `tenue` no padrão da pasta e em `texto` quando mudado nesta conversa.
- Hover `hover`; **aberto**: `pressionado` com fio `borda` e a seta para cima.
- Desabilitado a `opacidade-desabilitado` com o motivo na dica ("Este modelo não usa esforço").
- Nos painéis de fundo, a `opacidade-em-repouso` (menos o modo Sem perguntar).

## No código Rust (para quem implementa)
`Chip::new(id, rotulo)` com `.simbolo(Simbolo)`, `.tamanho(TamanhoChip)`, `.mudado(bool)`, `.seta(bool)`, `.aberto(bool)`, `.so_icone(bool)`, `.alerta(cor)`, `.arco_iris()`, `.cor_rotulo(cor)`, `.dica(...)` ou `.dica_acao(...)`, `.desabilitado(motivo)`. Embrulhado em `GatilhoChip` quando abre menu.

Fonte: `src/ui/componentes/controles.rs` (`Chip`), `src/ui/composer_seletores.rs` (`chip_modo`, `chip_modelo`, `chip_esforco`).

## Em React

`modo`, `marca` ou `esforco` escolhem o símbolo; o rótulo é o filho; `soIcone` na faixa estreita.

```jsx
<ChipDaCaixa modo="sem-perguntar" seta />
<ChipDaCaixa marca="claude" seta>Opus 5.5</ChipDaCaixa>
<ChipDaCaixa esforco={4} seta>Auto · Extra</ChipDaCaixa>
<ChipDaCaixa ultrathink>Ultrathink</ChipDaCaixa>
```
