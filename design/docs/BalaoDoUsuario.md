---
category: Conversa
---
# Balão do usuário

`<BalaoDoUsuario>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

O que você escreveu: um balão azul baixo, na largura toda da coluna, com a ponta no canto de baixo à direita; as ações flutuam no canto de cima à direita, só no hover.

## Anatomia
- Largura toda da coluna de leitura (dono, 16/11/2026: "faz o campo das minhas mensagens ficar na largura toda").
- Fundo `balao` (o destaque a 16% misturado à base, opaco), fio de 1 px `balao-borda`.
- Canto `raio-da-peca` (16) e a ponta, embaixo à direita, em `raio-controle` (10). Com 20, a mensagem de uma linha virava pílula e parecia um campo esperando texto.
- Padding 16 nos lados (12 no painel estreito) e 12 em cima e embaixo, contando o fio; vão de `esp-8` entre pílulas, texto e "Mostrar tudo".
- Texto em `fala` (14/22) na cor `texto`: um degrau abaixo da resposta, que é 15/23.

## Conteúdo
- **Pílulas** dos anexos em cima do texto, no poço escuro (`recuo` com fio `borda`): ver `PilulaDeArquivo`.
- **Caminhos** que existem no disco viram link em `destaque-texto` e abrem no painel da direita; código em linha vai no `recuo`.
- **Comando de barra sozinho** ("/compact") vira uma pílula de 32 no material do balão, em `mono` na cor `codigo`.
- **Longo** (mais de 14 linhas): mostra 12, com um véu de 24 na cor do balão por cima da última linha e o botão "Mostrar tudo (N linhas)" com o chevron, na coluna do texto.
- Selos Ultrathink e Ultracode no pé, à direita.

## Ações
Barra de 28 (`cartao-elevado`, fio `borda-forte`, `aresta-de-tecla` e `sombra-elevado`): a hora, Editar (o texto volta para a caixa) e Copiar (o texto original). Fica a 14 acima do balão e a 16 da borda direita, no trecho reto da borda; aparece seca no hover, sem fade.

## Espaço
32 antes do balão (`esp-entre-turnos`) e 12 até a resposta (`esp-no-turno`).

Fonte: `src/ui/mensagens.rs` (`balao_usuario`, `estilo_do_balao`, `barra_de_acoes`), especificação `arquivo/balao-erro-spec-2026-11-16.md`.

## Em React

Na coluna de leitura (até 806, 16 de margem), largura toda. Anexos em `anexos` com PilulaDeArquivo `noBalao`.

```jsx
<BalaoDoUsuario acoes hora="12:51"
  anexos={<PilulaDeArquivo noBalao imagem={url} nome="colado-b51d81ef3e.png" medida="601×263" />}>
  Confere o <a href="#">src/claude.rs</a>, a função <code>ler_erro</code> está devolvendo o tipo errado.
</BalaoDoUsuario>
<BalaoDoUsuario comando="/compact" />
```
