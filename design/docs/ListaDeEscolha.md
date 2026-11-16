---
category: Escolha
---
# Lista de escolha

`<ListaDeEscolha>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A lista com busca dos popovers (o seletor de modelo, o "/", o "@", o histórico do +): busca sempre com o foco, setas pulando os desabilitados, Enter escolhe, Esc em dois tempos; o escolhido vai na ficha de metal com o visto.

## Anatomia (seletor de modelo)
- Popover `flutuante` de 400 (320 na caixa estreita), padding 6, canto `raio-peca`, `sombra-popover`.
- Em cima, as abas **Claude | Codex** num `ControleSegmentado` de largura toda: trocar de aba só mostra os modelos do outro; quem responde muda ao escolher.
- A busca, com a lupa na coluna dos ícones e um fio cheio embaixo (o desenho do Raycast e do Linear).
- Itens de 48 com descrição (32 sem): título 14/20 em 500 (`texto`; `forte` no escolhido), descrição `metadado` em `tenue` (`texto` sobre a ficha), selos e o visto de 16 à direita.
- **Escolhido**: a ficha de metal sangra 8 para fora da linha, no canto `raio-controle` (o mesmo do realce do ponteiro e do teclado nas mesmas linhas), sem o realce claro por baixo.
- Grupos com rótulo de seção (mono 11 `apagado`, caixa alta, tracinho): "Conta do Claude", "Versões exatas".
- Rodapé: o fio de pontos, a linha "Claude e Codex juntos" com o interruptor (dono, 16/11/2026) e as teclas ("↑ ↓ escolher", "Enter usa").
- Altura máxima da lista: 440.

## No código Rust (para quem implementa)
`ListaDeEscolha::new(placeholder)` e `.definir(grupos, escolhido)`; cada `ItemDeEscolha` tem chave, símbolo, título, descrição, selos, motivo de desabilitado e termos extras da busca. `.ficha_no_escolhido(true)`, `.simbolo_do_grupo(..)`, `.filtro_proprio(..)`, `.rodape(..)`. Emite `EventoEscolha::Escolheu(chave)` ou `Fechou`.

Fonte: `src/ui/componentes/listas.rs` (`ListaDeEscolha`, `linha_de_escolha`), `src/ui/composer_seletores.rs` (`chip_modelo`, `grupos_de_modelo`).

## Em React

`itens` mistura itens e seções (`{ secao }`); `escolhido` é o índice em itens; o rodapé extra vai como filho.

```jsx
<ListaDeEscolha
  abas={['Claude', 'Codex']}
  busca="Buscar modelo"
  escolhido={0}
  itens={[
    { titulo: 'Padrão', descricao: 'O que o Claude Code escolher' },
    { titulo: 'Opus', descricao: 'Para trabalho complexo e o dia a dia' },
    { secao: 'Conta do Claude' },
    { titulo: 'Principal', descricao: 'semana 16%' },
  ]}
/>
```
