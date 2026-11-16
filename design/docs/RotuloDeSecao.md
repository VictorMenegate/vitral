---
category: Lista
---
# Rótulo de seção

`<RotuloDeSecao>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

A segunda voz do app em cima de um grupo: o tracinho de 8 x 1 em `fio` e o título em mono 11 `apagado`, em caixa alta só no desenho ("PASTAS", "CONTA DO CLAUDE"); o dado continua "Pastas".

## Anatomia
- Linha de 24, `rotulo-secao` (JetBrains Mono 11/16, 400, `apagado`), vão `esp-6`.
- 16 de respiro em cima (`esp-antes-de-secao`; 4 na primeira seção) e 4 embaixo.
- Recuo de 8 (cai na guia de 16 da lista); 12 na Equipe (guia de 24).
- **Contagem** num selo de 18 (`traco` com `tenue`, ou na tinta da cor).
- **Recolhível**: o chevron de 12 à esquerda do texto; o hover clareia os dois para `texto`.
- **Ação** à direita (um botão de linha).
- Nos popovers de escolha, o rótulo ganha o fio de pontos até a borda (ou até as teclas do atalho, à direita).

## Regras
- Fica em 11: o GPUI não tem espaçamento entre letras, e 10 em caixa alta aperta.
- O "Hoje" da conversa não é rótulo de seção: é o separador de dia (`MarcoDaConversa`), de propósito.

Fonte: `src/ui/componentes/listas.rs` (`CabecalhoDeSecao`), `src/ui/componentes/textos.rs` (`etiqueta_de_secao`, `tracinho_de_secao`), `src/ui/componentes/superficies.rs` (`linha_escolha`, `cabecalho_escolha`).

## Em React

Escreva o título normal; a caixa alta é do desenho. `fio` para os popovers de escolha.

```jsx
<RotuloDeSecao contagem={12} acao="plus" dicaAcao="Nova pasta">Pastas</RotuloDeSecao>
<RotuloDeSecao fio teclas={['Shift', 'Tab']} rotuloTeclas="troca">Conta do Claude</RotuloDeSecao>
```
