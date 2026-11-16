---
category: Conversa
---
# Trilha de ferramentas

`<TrilhaDeFerramentas>` no vitral-ui. O que está abaixo vale para o app de verdade (Rust + GPUI); o componente React recria o mesmo desenho.

As chamadas que o agente fez num turno, ligadas por ramos a um resumo: uma linha de 32 por chamada, o ícone do tipo, o verbo em português e o detalhe.

## Anatomia da linha
- Altura `altura-do-log` (32), padding 12 (32 dentro de um grupo), vão `esp-8`, canto `raio-controle`, texto `metadado` (12/16).
- **Ícone** de 14 do tipo (BookOpen ler, FilePenLine editar, FilePlus gravar, Terminal comando, TextSearch e FolderSearch buscar, Globe web, Bot subagente, Workflow fluxo, ListChecks passos), em `tenue`; `vermelho` na falha; `fantasma` na interrompida; o anel girando enquanto roda.
- **Verbo** em 500, cor `texto`, no passado ou ao vivo ("Leu", "Lendo"; "Rodou", "Rodando"). Dentro de um grupo ocupa uma coluna de 56; solta, não.
- **Detalhe**: o que é código (comando, caminho, padrão, endereço, ferramenta MCP) vai em mono dentro de um chip de 22 no `recuo`; a prosa (a tarefa do subagente, a busca na web) vai em Inter, sem chip, em `texto`.
- **Meta** em mono `apagado`: "por 9,8 s", "00:12", "14 ferramentas · 01:20"; numa edição, "+5" em `verde` e "−2" em `vermelho`.
- O chevron (12, `apagado`) sempre no fim da linha.

## O grupo
- O cabeçalho: ListTree, o resumo ("Rodou 13 comandos · editou 1 arquivo · leu 3 arquivos" no painel largo; "13 comandos · 1 editado · 3 lidos" fora dele, para nunca cortar um número), um selo "1 falhou" e o chevron.
- O ramo de 1 px em `traco` sai do centro do ícone (x 19), dobra com canto de 6 e chega a 28.
- Abre durante o turno e recolhe quando termina; a escolha manual prevalece. Recolhido, mostra a pilha das imagens que as chamadas leram.
- Falha: a linha ganha o fundo `diff-sai` e a palavra "falhou".

## No código Rust (para quem implementa)
As chamadas do turno (nome, resumo, resultado, erro, arquivo, subagente), se o turno está ao vivo e a largura do painel.

Fonte: `src/ui/mensagens.rs` (`trilha`, `chamada`, `raciocinio`, `resumo_das_chamadas`, `trilho_do_log`, `icone_da_ferramenta`, `verbo_da_ferramenta`).

## Em React

As chamadas como dados em `chamadas`; com `resumo` vira o grupo com ramos.

```jsx
<TrilhaDeFerramentas
  resumo="Rodou 2 comandos · editou 1 arquivo · leu 1 arquivo"
  falhas={1}
  chamadas={[
    { icone: 'book-open', verbo: 'Leu', alvo: 'tema.rs' },
    { icone: 'file-pen-line', verbo: 'Editou', alvo: 'superficies.rs', mais: 5, menos: 2 },
    { icone: 'terminal', verbo: 'Rodou', alvo: 'npm run build', falhou: true },
    { verbo: 'Rodando', alvo: 'sleep 40', meta: '00:12', rodando: true },
  ]}
/>
```
