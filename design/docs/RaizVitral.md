---
category: Materiais
---
# Raiz do Vitral

`<RaizVitral>` no vitral-ui: a raiz de toda tela. Liga o tema do app (só escuro), a Inter 14/20, o texto em `texto`, o antialiasing e o fundo. Sem ela, o texto solto cai na fonte e na cor do navegador.

## Fundos
- `cartao` (padrão): o `cartao-opaco` das peças.
- `base`: a base da janela, o metal parado debaixo do véu azul-escuro (`vidro-leitura`). Use para montar a janela inteira: a lista mora direto nela e as peças (`Materiais material="vidro"`) flutuam por cima, com margem de 8 e vão de 8 (`margem-da-janela`, `vao-entre-pecas`).
- `nenhum`: só a tipografia.

## Em React

```jsx
<RaizVitral fundo="base" style={{ display: 'flex', gap: 8, padding: 8, height: '100vh' }}>
  <aside style={{ width: 312 }}>{/* CartaoDeLista... */}</aside>
  <Materiais material="vidro" style={{ flex: 1, minWidth: 280 }}>{/* a conversa */}</Materiais>
</RaizVitral>
```

Não existe no app como componente: no GPUI o tema e a fonte são globais da janela (`src/tema.rs`, `src/ui/componentes/metal.rs`).
