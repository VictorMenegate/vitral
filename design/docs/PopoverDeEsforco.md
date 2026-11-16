---
category: Escolha
---
# Popover do esforço

`<PopoverDeEsforco>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O popover do chip do esforço: o rótulo de seção com o fio de pontos até a borda, o segmentado dos seis níveis, a frase do nível e, no rodapé, as duas chaves da próxima mensagem (Ultrathink e Ultracode) com as teclas.

## Anatomia
- Popover de 400 (320 na caixa estreita), padding 16, vão `esp-12`; `flutuante`, `sombra-popover`.
- Cabeçalho de escolha: as barrinhas (12) do nível, o rótulo "ESFORÇO" (mono 11 `apagado`, tracinho) e o `fio` de pontos até a borda.
- Segmentado compacto no flutuante, largura toda: Auto, Baixo, Médio, Alto, Extra, Máx ("Máximo" não cabe na faixa estreita). O escolhido na ficha de metal.
- A frase do nível em `metadado` `tenue` ("Usa o padrão do modelo, que agora é o Extra.").
- **Ultrathink**: o título letra a letra no arco-íris (`arco-iris-1` a `arco-iris-8`, clareados para passar de 4,5:1 no flutuante), "Só na próxima mensagem" e o interruptor.
- **Ultracode**: o ícone Workflow e o título em `violeta`, a descrição e o aviso de custo em `ambar` ("Gasta bem mais do plano e roda no esforço Extra"); desabilitado, o título e o interruptor a 0,45 e o motivo legível em `apagado`.
- Rodapé de teclas: Espaço Ultrathink, U Ultracode, Enter fecha.

Fonte: `src/ui/composer_seletores.rs` (`corpo_do_esforco`, `linha_ultracode`), `src/ui/componentes/superficies.rs` (`CorpoPopover`, `CabecalhoPopover::Escolha`), `src/ui/componentes/listas.rs` (`LinhaDeOpcao`).

## Em React

Abre do ChipDaCaixa do esforço. `nivel` 0 a 5 (0 é Auto).

```jsx
<PopoverDeEsforco nivel={3} frase="Pensa mais antes de responder." ultrathink />
```
