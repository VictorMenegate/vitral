---
category: Escolha
---
# Controle segmentado

`<ControleSegmentado>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

Escolha entre poucas opções numa pílula: o trilho é um poço e a opção escolhida é a ficha de metal azul, que desliza até a nova em 180 ms.

## Regras
- **O escolhido é sempre a ficha de metal** no segmentado neutro (o comum): o miolo do metal debaixo do `veu-de-placa`, com `sombra-elevado` e `aresta-de-ficha`. É o "escolhido" do app inteiro, a família do + e do enviar (dono: "o metal azul como escolhido"). Antes, as Configurações marcavam com uma pílula cinza lisa: o mesmo componente marcava o escolhido de dois jeitos opostos (relatório de design, D4).
- Só o segmentado **de outra cor** (um estado, um aviso) pinta o escolhido numa pílula tingida: `opacidade-selecao-de` da cor, fio `tinta-borda`, `aresta-de-tecla`.
- Trilho: `recuo` com `aresta-de-poco` e fio `borda`; `solto` tira o fio (o alto da lista); `no_flutuante` troca o recuo pelo `recuo-no-flutuante`, porque o escuro some no popover.
- Alturas: 28 (pequeno), 32 (médio), 36 (grande) ou sob medida (40 no alto da lista); canto `raio::de(altura)` no trilho e 2 a menos na ficha. Padding e vão `esp-2`.
- Segmentos: 13 px, 500 (600 no escolhido, em `forte`), `tenue` no resto com hover `hover`; padding 12 (8 só ícone, 4 compacto).
- Teclado: um ponto de Tab; setas, Home e End aplicam na hora. Desabilitado a `opacidade-desabilitado`.

## No código Rust (para quem implementa)
`ControleSegmentado::new(id, opcoes, atual, ao_mudar)` com `.solto(bool)`, `.cor(Hsla)`, `.largura_total(bool)`, `.so_icone(bool)`, `.compacto(bool)`, `.no_flutuante(bool)`, `.with_size(..)`, `.disabled(bool)`. Cada `Segmento` tem rótulo, ícone, dica e um ponto opcional.

Fonte: `src/ui/componentes/controles.rs` (`ControleSegmentado`), `src/ui/componentes/superficies.rs` (`ficha_do_escolhido`).

## Em React

`opcoes` como texto ou objetos com ícone; `valor` é o índice escolhido.

```jsx
<ControleSegmentado opcoes={['Nunca', '3 dias', '7 dias', '30 dias']} valor={2} onChange={setDias} />
<ControleSegmentado noFlutuante compacto largo opcoes={['Auto', 'Baixo', 'Médio', 'Alto', 'Extra', 'Máx']} valor={0} />
```
