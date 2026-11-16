---
category: Base
---
# Ícone

`<Icone>` no vitral-ui: os ícones Lucide (ISC) que o app embute (gpui-kit-assets 0.8.1), em traço de 2 no quadro de 24, pintados por máscara na cor do texto em volta. Mais o escudo cheio do modo Manual (`escudo-cheio`).

## Regras
- Tamanhos: 12 (chevrons, marcos), 14 (linhas, botões de linha, chips), 16 (botões de barra e controle, menus), 20 (estado vazio, símbolos de lista).
- Cor `tenue` por padrão (herdada de quem envolve); `cor` só quando a cor é o sinal: o escudo do Sem perguntar em `ambar`, a ferramenta que falhou em `vermelho`.
- Ícones de ferramenta: book-open ler, file-pen-line editar, file-plus gravar, terminal comando, text-search buscar, folder-search listar, globe web, bot subagente, workflow fluxo, list-checks passos, brain pensou.
- Modos: escudo-cheio (Manual), file-pen (Aceitar edições), map (Planejar), wand-sparkles (Automático), shield-off (Sem perguntar).
- Sem emoji. O parar é um quadrado, não um ícone (BotaoRedondo `parar`).

## Os nomes que existem
Só estes têm a classe `vt-i-<nome>` no CSS; outro nome desenha um quadrado vazio.

`activity`, `archive-restore`, `archive`, `arrow-down`, `arrow-left`, `arrow-right`, `arrow-up`, `asterisk`, `at-sign`, `bell-off`, `bell`, `binary`, `book-open`, `bot`, `brain`, `check-check`, `check`, `chevron-down`, `chevron-left`, `chevron-right`, `chevron-up`, `circle-arrow-up`, `circle-check`, `circle-pause`, `circle-user-round`, `circle-user`, `circle-x`, `clock`, `code-xml`, `columns-2`, `compass`, `container`, `copy`, `download`, `ellipsis`, `eraser`, `escudo-cheio`, `external-link`, `eye`, `file-code`, `file-diff`, `file-image`, `file-pen-line`, `file-pen`, `file-plus`, `files`, `file`, `file-text`, `file-x`, `focus`, `folder-input`, `folder-open`, `folder-plus`, `folder-search`, `folder`, `folder-x`, `gauge`, `git-branch`, `git-fork`, `github`, `globe`, `image-off`, `image`, `info`, `keyboard`, `key-round`, `list-checks`, `list-tree`, `map`, `maximize`, `message-circle-question-mark`, `message-circle`, `message-square-plus`, `message-square-quote`, `message-square`, `messages-square`, `milestone`, `minimize`, `minus`, `move-down`, `move-up`, `network`, `notebook-pen`, `orbit`, `panel-left-close`, `panel-left-open`, `panel-left`, `panel-right-close`, `panel-right`, `paperclip`, `pause`, `pencil`, `pin-off`, `pin`, `play`, `plus`, `refresh-cw`, `rotate-ccw`, `rotate-cw`, `rows-3`, `scroll-text`, `search`, `settings`, `shield-alert`, `shield-check`, `shield-off`, `shield-question-mark`, `shuffle`, `sparkles`, `square-check`, `square-slash`, `square`, `terminal`, `text-cursor-input`, `text-search`, `trash`, `triangle-alert`, `undo-2`, `unplug`, `wand-sparkles`, `wifi-off`, `workflow`, `wrench`, `x`, `zap`, `zoom-in`, `zoom-out`.

## Em React

```jsx
<Icone nome="terminal" tamanho={14} />
<Icone nome="shield-off" tamanho={14} cor="var(--ambar)" />
```

Fonte: `src/tema.rs` (`Lucide`), `src/ui/componentes/base.rs` (`icone`), `assets/escudo-cheio.svg`.
