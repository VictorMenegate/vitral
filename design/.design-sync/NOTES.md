# Notas do design-sync do Vitral

- **O app não é React.** O Vitral é Rust + GPUI; o `design/` é um pacote novo (`vitral-ui`) que recria cada peça em React com as classes `vt-*` e os tokens de `src/tema.rs`. A fonte da recriação foi o design system em HTML e CSS montado em 16/11/2026 (38 componentes) e o código em `src/ui/componentes/`. Quando o app mudar, o pacote não acompanha sozinho: alguém tem de levar a mudança para `src/` e `tokens/tokens.json` daqui.
- **Build**: `npm install` e `npm run build` dentro de `design/`. O `build.mjs` gera `tokens/tokens.css` do `tokens/tokens.json`, junta `src/estilos/vitral.css` com as fontes e os ícones em `dist/vitral-ui.css`, empacota `src/index.js` com o esbuild e tira os `.d.ts` do JSDoc com o tsc. As fontes e as marcas vêm de `../assets/` (do app), não são copiadas para cá.
- **Tokens num pacote à parte** (`tokens/`, ligado por `file:./tokens` como `vitral-ui-tokens`), para o conversor publicar `tokens/tokens.css` separado do CSS das peças (`cfg.tokensPkg`).
- **Fontes**: o `@font-face` usa `url(./fonts/X.ttf)` sem aspas. Com aspas, o `rewriteBundleFontFaces` do conversor lê a URL como morta e derruba os 8 blocos do `_ds_bundle.css`.
- **Provider**: `RaizVitral` (fundo `cartao-opaco`, Inter, texto). O cartão do Claude Design tem fundo branco; sem a raiz, os componentes escuros ficam sem palco.
- **Previews**: `.design-sync/previews/<Nome>.tsx`, todos autorados; `_capturas.ts` tem miniaturas JPEG de capturas reais do app (só para os cartões de imagem).
- **Grupos**: vêm da pasta em `src/<grupo>/`; `Marca` e `Materiais` (pasta com o nome do próprio componente) pegam o `category` do `docs/<Nome>.md`.
- **guidelinesGlob** aponta só para `docs/guias/*.md`: o padrão (`docs/*.md`) copiaria os 41 docs de componente como guias.
- **Ligaduras**: o JetBrains Mono junta `->` e `::` nos blocos de código dos cartões. Não se sabe se o GPUI do app liga as ligaduras; ficou como está.
- **Upload desta primeira sincronização**: a sessão que montou o pacote (um subagente) não tinha a ferramenta DesignSync; o `ds-bundle/` ficou verificado e pronto para a sessão principal enviar.

## Known render warns
- `[GRID_OVERFLOW]` resolvido com `cardMode: column` nos 12 componentes largos (caixa, listas, popovers, menus, a janela).

## Re-sync risks
- `tokens/tokens.json` é cópia do `project/tokens.json` do design system de 16/11/2026, que saiu de `src/tema.rs` naquele dia. Se o tema do app mudar, este arquivo fica velho sem aviso.
- `src/estilos/vitral.css` é a montagem em CSS das peças; medidas novas no Rust não chegam aqui.
- Os 127 ícones em `src/icones/` foram copiados do crate `gpui-kit-assets` 0.8.1 (registro do cargo) mais o `escudo-cheio.svg` do app. Ícone novo no app precisa ser copiado à mão.
- O conversor e o playwright: `.ds-sync/` usa `playwright@1.61.1`, que casa com o chromium headless 1228 do cache desta máquina.
- As notas de grade (`.cache/review/`) não vão para o git; o que vale entre máquinas é o `_ds_sync.json` enviado ao projeto.
