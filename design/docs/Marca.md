---
category: Marca
---
# Marca

`<Marca>` no vitral-ui: a marca de um serviço (Claude, Codex, Linear) pelo arquivo do app, sempre por imagem e nunca recolorida.

## Regras
- **Disco** cheio (`claude.svg`, `codex.svg`, `linear.svg`): nas listas, nos popovers, na dupla e no anel de uso.
- **Glifo** solto (`claude-glifo.svg`, `codex-glifo.svg`): nos chips da caixa e das Configurações. O disco coral ao lado do escudo âmbar era a mancha mais forte da faixa (relatório de aparência, V9).
- As cores `claude` e `codex` só aparecem no logo delas e no arco do anel de uso.

## Em React

```jsx
<Marca marca="claude" tamanho={20} />
<Marca marca="codex" forma="glifo" tamanho={14} />
```

Fonte: `assets/marcas/`, `src/ui/componentes/graficos.rs` (`AnelDeUso`).
