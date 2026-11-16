---
category: Base
---
# Interruptor

`<Interruptor>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O liga e desliga das preferências e das chaves da próxima mensagem: o `Switch` do kit, 36 x 20 com o pino de 16; a linha inteira alterna.

## Estados
- Desligado: trilho branco a 12% (`pressionado`), pino em `texto`.
- Ligado: trilho `destaque` chapado, pino em `texto` (3,8:1 no azul).
- Desabilitado: o interruptor e o título a `opacidade-desabilitado`, com o motivo legível em `apagado` embaixo.

## Onde
Na `LinhaDeOpcao` (Configurações, boas-vindas), no rodapé do seletor de modelo ("Claude e Codex juntos") e no popover do esforço (Ultrathink, Ultracode).

## Ainda em discussão
Hoje ele mede em px fixos e não cresce com o zoom, e é o único azul liso das Configurações. O relatório de aparência (V23) propõe medidas em rem e o ligado sobre a abertura de metal do "+" (desligado no `recuo` com `aresta-de-poco`); fica para quando se mexer no kit.

Fonte: `vendor/gpui-component/src/switch.rs`, `src/ui/componentes/listas.rs` (`LinhaDeOpcao::interruptor`), tema do kit (`switch.background`, `switch.thumb.background`).

## Em React

Controlado: `ligado` e `onChange(valorNovo)`. Mora na linha de opção (GrupoDeOpcoes).

```jsx
<Interruptor ligado={uso} onChange={setUso} rotulo="Mostrar o uso do plano" />
```
