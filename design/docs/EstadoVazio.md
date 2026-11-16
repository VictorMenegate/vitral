---
category: Conversa
---
# Estado vazio

`<EstadoVazio>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O que aparece onde ainda não há nada: amplo, no painel sem conversa, e compacto, nas listas, painéis e popovers.

## Variantes
- **Amplo**: só o título em `titulo-tela` (18/24, 600, `forte`) e o texto em `corpo-ui` `tenue` até 420 de largura. Sem disco em volta de ícone.
- **Compacto**: o ícone de 20 solto em `apagado`, o título 14/20 em 600 na cor `texto` e o texto 13/18 `tenue` até 300.
- Centrado, vão `esp-8`, 24 em cima e embaixo; ações opcionais em fileira, centradas.

## Em discussão
O relatório de design (P11) propõe falar com quem responde ("Converse com o Claude", "Mensagem para o Codex…") em vez de "este chat", e (P7) centrar o amplo no painel em vez de colá-lo no alto.

Fonte: `src/ui/componentes/superficies.rs` (`EstadoVazio`), `src/ui/mensagens.rs` (`dica_inicial`).

## Em React

Amplo no painel sem conversa; compacto nas listas e popovers.

```jsx
<EstadoVazio titulo="Converse com este chat" descricao="Este chat mexe direto em ~/eita." />
<EstadoVazio compacto icone="search" titulo="Nada encontrado" descricao="Nada encontrado para “cozinha retrô”" />
```
