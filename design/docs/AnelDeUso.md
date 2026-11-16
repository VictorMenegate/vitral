---
category: Painel
---
# Anel de uso

`<AnelDeUso>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O uso do plano de cada serviço (Claude, Codex): o disco da marca com UM anel em volta, que mostra a janela mais apertada das duas (5 h ou semana). Eram dois anéis, um dentro do outro, e com a fenda eles liam como "duas barras" (dono, 12/11/2026).

## Anatomia
- Botão de 30 no canto (36 no trilho e nas Contas, 20 no menu): o disco opaco da peça (`cartao-opaco`) com fio `borda` e `sombra-elevado`, para flutuar sobre a conversa sem sumir no texto (dono, 16/11/2026).
- O anel com fenda (3 de espessura) na cor da marca (`claude`, `codex`) até 69%, `ambar` de 70% a 89%, `vermelho` a partir de 90%; no limite, cheio em `vermelho`. Serviço ausente: disco cinza e esmaecido, sem arco.
- O disco da marca no meio (diâmetro menos 12).
- Com várias contas, a inicial da conta em uso num disquinho de 15 no canto de baixo (`flutuante`, fio `borda-forte`, 9 px em 600).
- O clique abre o popover do uso, para baixo.

## Regras
- **Moram no canto de cima à direita da janela, logo embaixo do ×**, um em cima do outro, 14 da borda direita e 60 do topo (dono, 13/11/2026; voltaram para lá no mesmo dia em que foram para o cabeçalho, 16/11/2026: "o de uso tem que ficar onde eu tinha falado, embaixo do ×"). A conversa passa por baixo deles, sem folga.
- O do Claude sempre; o do Codex só instalado e com uso lido.
- A opção "Mostrar o uso do plano no canto da janela" (Configurações > Chats) os esconde.

Fonte: `src/ui/uso_plano.rs` (`uso_no_canto`, `BotaoUso`, `inicial_da_conta`), `src/ui/componentes/graficos.rs` (`AnelDeUso`).

## Em React

Posicione no canto de cima à direita da janela, embaixo do ×, um em cima do outro (right 14, top 60 da janela).

```jsx
<div style={{ position: 'absolute', right: 14, top: 60, display: 'flex', flexDirection: 'column', gap: 6 }}>
  <AnelDeUso marca="claude" fracao={0.42} inicial="P" />
  <AnelDeUso marca="codex" fracao={0.12} />
</div>
```
