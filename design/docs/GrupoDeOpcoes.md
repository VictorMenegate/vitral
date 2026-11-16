---
category: Escolha
---
# Grupo de opções

`<GrupoDeOpcoes>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

As preferências no estilo do libadwaita: um rótulo de seção e as linhas numa caixa, separadas por um fio de pontos; cada linha tem título, descrição e o controle à direita.

## Anatomia
- Caixa: `grupo` com fio `borda`, canto `raio-cartao`; o rótulo de seção em cima, recuado 4, com 6 de vão.
- Entre as linhas, o fio de pontos recuado 12 dos dois lados, nunca uma linha cheia.
- **Linha** (`LinhaDeOpcao`): padding 12, vão 16 até o controle; título 14/20 em 500 `texto` (com o (i) da dica longa, opcional); descrição `metadado` `tenue`; aviso de custo `metadado` `ambar`.
- **Controles**: o interruptor (a linha inteira alterna), o contador (− número +, num poço de 28), o chip de menu, o segmentado (com a ficha de metal) ou um campo embaixo do título.
- **Desabilitada**: só o título e o controle a `opacidade-desabilitado`; o motivo em `apagado` cheio, legível (com a linha inteira a 0,45 o motivo dava 2,45:1).

## Em discussão
O relatório de design (D8) propõe dar ao chip de menu e ao "Ligar…" o fundo de poço do contador, para a borda direita da coluna não ficar dentada; (D13) "15 min" no contador; (P12) navegação por seções nas Configurações.

Fonte: `src/ui/componentes/listas.rs` (`LinhaDeOpcao`, `GrupoDeOpcoes`), `src/ui/componentes/controles.rs` (`Contador`), `src/ui/configuracoes.rs`.

## Em React

As linhas como dados; o controle de cada uma em `controle` (ou `contador`).

```jsx
<GrupoDeOpcoes
  titulo="Chats"
  opcoes={[
    { titulo: 'Resumo ao voltar depois de', descricao: 'Minutos fora de foco.', contador: 15 },
    { titulo: 'Mostrar o uso do plano no canto da janela', descricao: 'Os anéis ficam embaixo do ×.', controle: <Interruptor ligado /> },
    { titulo: 'Arquivar chats parados depois de', controle: <ControleSegmentado opcoes={['Nunca', '7 dias']} valor={1} /> },
  ]}
/>
```
