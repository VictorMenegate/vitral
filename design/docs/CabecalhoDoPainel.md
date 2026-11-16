---
category: Painel
---
# Cabeçalho do painel

`<CabecalhoDoPainel>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A fileira de 44 no topo de cada painel de conversa: as abas, o modo foco (só quando ligado) e o "+", que abre um chat novo neste painel ou um do histórico.

## Quando usar
- Em todo painel de conversa. Ele arrasta a janela pelo vazio; as abas e os botões não.
- No último painel (com a direita fechada), depois do "+" vêm um traço de 1 x 16 (`traco`) e os botões da janela, um pouco afastados, para o × não parecer o de fechar o painel. O × da janela acende em vermelho e a dica diz "Fechar o Vitral".

## No código Rust (para quem implementa)
As abas (`AbaDePainel`) e as ações. Não há mais "⋯" no cabeçalho (dono, 16/11/2026: "tira o de 3 pontinhos e deixa só no botão direito"): o menu do painel abre no botão direito ou com Shift+F10.

## Regras
- Altura `altura-cabecalho` (44), padding e vão `esp-8`, abas separadas por `esp-6`.
- As ações ficam a `opacidade-em-repouso` (0,55) até o ponteiro chegar: o cabeçalho é o título da conversa, não uma fileira de ícones.
- O "+" é um botão só ícone de 28 (`altura-barra`) em `tenue`.
- Sem linha embaixo do cabeçalho com a conversa rolada (dono, 12/11/2026).
- **Anéis de uso** flutuam no canto de cima à direita da janela, logo **embaixo do ×**, um em cima do outro (14 da borda, 60 do topo), cada um no disco opaco da peça com fio e sombra curta: a conversa passa por baixo deles, sem folga (dono, 13/11 e 16/11/2026).
- O topo da lista tem a mesma altura de 44, para o logo e as abas da lista ficarem na linha das abas ao lado.

Fonte: `src/ui/conversa.rs` (`cabecalho_painel`, `acoes_do_painel`, `botao_historico`), `src/ui/uso_plano.rs` (`uso_no_canto`).

## Em React

O topo de cada peça de conversa: as abas como filhos; `fecharJanela` só no último painel.

```jsx
<Materiais material="vidro">
  <CabecalhoDoPainel fecharJanela>
    <AbaDePainel ativa titulo="Projeto Vitral" estado="espera" painel={3} />
  </CabecalhoDoPainel>
  {/* a conversa */}
</Materiais>
```
