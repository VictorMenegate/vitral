# Vitral: como desenhar com o vitral-ui

O Vitral é um app de desktop (Rust + GPUI) para conversar com o Claude Code e o Codex em vários painéis lado a lado. O vitral-ui recria em React cada peça do app, com os valores exatos de `src/tema.rs`. Um design feito aqui é uma proposta para o app: use os componentes como estão e respeite as decisões do dono abaixo. Escreva toda a interface em português do Brasil.

## Montagem
- Toda tela começa em `<RaizVitral>`: ela liga a Inter 14/20, o texto em `--texto`, o tema escuro e o fundo. Sem ela, o texto solto sai na fonte do navegador. Só existe tema escuro.
- A janela inteira é `<RaizVitral fundo="base">` (o metal parado sob o véu azul-escuro), com margem de 8 e vão de 8 entre as peças (`--margem-da-janela`, `--vao-entre-pecas`, nunca maiores). A lista de chats (312, `--largura-lateral`) mora direto na base, sem cartão. Cada conversa é uma `<Materiais material="vidro">` (mínimo 280) com `CabecalhoDoPainel` no topo, a coluna de leitura (até 806, 16 de margem) e a `CaixaDeMensagem` no pé. Menus e popovers ficam sobre `<Materiais material="casca">` ou são `ListaDeEscolha`, `MenuComPlaca`, `PopoverDeEsforco`.
- Os anéis de uso (`AnelDeUso`) ficam no canto de cima à direita da janela, embaixo do ×, um em cima do outro.

## Estilo: tokens `var(--*)`, sem utilitários
Não há Tailwind nem classes utilitárias. O seu layout usa `style` ou CSS próprio, só com os tokens:
- Superfícies, do fundo para cima: `--vidro-leitura`, `--cartao-opaco`, `--cartao-elevado`, `--flutuante`; `--preto` (código, moldura da caixa), `--recuo` (poço), `--balao`.
- Texto: `--forte`, `--texto`, `--tenue`, `--apagado`, `--codigo`. Um destaque só: `--destaque` (fundo e ícone) e `--destaque-texto` (texto e link; `--destaque` como texto pequeno reprova no contraste).
- Estado, sempre com ícone ou palavra: `--espera` (precisa de você, azul-claro), `--vermelho`, `--ambar` (aviso de verdade), `--verde`, `--violeta`; tintas `--tinta-vermelho`, `--tinta-leve-vermelho`, `--tinta-leve-espera`, `--tinta-ambar`.
- Fios e estados: `--borda`, `--borda-forte`, `--linha`, `--fio`, `--hover`, `--selecao`, `--pressionado`.
- Espaço só em `--esp-2`, `--esp-4`, `--esp-6`, `--esp-8`, `--esp-12`, `--esp-16`, `--esp-24`, `--esp-32`. Cantos: `--raio-controle` (10), `--raio-cartao` (14), `--raio-da-peca` (16), `--raio-peca` (20); nos controles, `--raio-de-24`, `--raio-de-28`, `--raio-de-32`, `--raio-de-36` (45% da altura). Alturas: `--altura-acao-linha`, `--altura-barra`, `--altura-controle`, `--altura-cabecalho`. Sombras e arestas: `--sombra-flutuante`, `--sombra-popover`, `--aresta-de-vidro`, `--aresta-de-tecla`, `--aresta-de-poco`.
- Tipografia por classe: Inter em `vt-tipo-titulo-heroi`, `vt-tipo-titulo-tela`, `vt-tipo-titulo-cartao`, `vt-tipo-corpo` (15/23, a resposta), `vt-tipo-fala`, `vt-tipo-corpo-ui`, `vt-tipo-rotulo`, `vt-tipo-metadado`, `vt-tipo-micro`, `vt-tipo-numero`; a segunda voz em JetBrains Mono: `vt-tipo-rotulo-secao`, `vt-tipo-etiqueta`, `vt-tipo-mono`, `vt-tipo-mono-pequeno`, `vt-tipo-tecla`.
- Ícones só com `<Icone nome="search" tamanho={14} />` (os nomes estão no Icone.prompt.md). Separador é `<Materiais material="fio" />`, nunca linha cheia. As outras classes `vt-*` são internas dos componentes: não invente nem monte peças à mão quando há componente.

## Onde ler
`tokens/tokens.css` traz cada variável com o uso dela em comentário. `guidelines/docs/guias/vitral.md` tem os fundamentos, a voz e as regras do dono completas. Cada componente tem o `components/<grupo>/<Nome>/<Nome>.prompt.md` (anatomia, regras e exemplo em React).

## As regras do dono (valem acima de qualquer gosto)
1. Abas separadas, cada uma com moldura própria e fio preto de 1 px, numa linha só; a à vista com o material do campo da caixa.
2. Azul-claro (`--espera`), não amarelo, para "esperando você"; âmbar só para aviso de verdade.
3. Raio 16 nas peças da janela e no balão.
4. O balão do usuário na largura toda da coluna, com a ponta embaixo à direita.
5. O painel ativo se marca pela luz azul embaixo da caixa (`CaixaDeMensagem ativa`), não por borda nem cantoneira.
6. O anel de contexto sempre com a porcentagem ao lado, em mono.
7. Imagens em pílulas com miniatura, nome e medida; várias viram a pilha.
8. O metal azul é o "escolhido" em toda lista, segmentado e popover (a ficha de metal).
9. Vão entre painéis igual à margem da janela: 8.
10. Grafite chapado, com menos cara de IA: sem degradê, sem reflexo, sem halo, sem borda azul; o metal é a assinatura.
11. Sem a bordinha de cima das peças.
12. Cabeçalho do painel com pouca coisa: abas, modo foco (só ligado) e o "+"; sem "⋯".
13. O ícone do chat é o estado dele (`IconeDeEstado`).
14. O número do painel à direita da aba, antes do ×.
15. Cantos bem arredondados nos controles; chats da lista com linha de 36 e título 15.
16. Moldura da caixa com 2 de folga; fundos opacos por padrão; nada anima em repouso.

## Voz
Frases curtas, sem travessão nem meia-risca. "Chat" é o objeto ("Novo chat"), "conversa" é o conteúdo. Enviar (nunca "Mandar"), Remover, Apagar, Retomar, Tentar de novo, Compactar. Estado é adjetivo minúsculo ("rodando", "esperando você"). Números com espaço: "3 d", "22 min", "52%". Sem emoji.

## Exemplo
```jsx
<RaizVitral fundo="base" style={{ display: 'flex', gap: 'var(--vao-entre-pecas)', padding: 'var(--margem-da-janela)', height: '100vh' }}>
  <aside style={{ width: 'var(--largura-lateral)', display: 'flex', flexDirection: 'column', gap: 2 }}>
    <RotuloDeSecao contagem={2} tomContagem="espera">Paradas</RotuloDeSecao>
    <CartaoDeLista titulo="WMS" estado="rodando" selecionado painel={1} painelAtivo />
    <CartaoDeLista titulo="Pesagem de caixas" estado="espera" hora="3 h" />
  </aside>
  <Materiais material="vidro" style={{ flex: 1, minWidth: 280, display: 'flex', flexDirection: 'column' }}>
    <CabecalhoDoPainel>
      <AbaDePainel ativa titulo="WMS" estado="rodando" painel={1} painelAtivo />
    </CabecalhoDoPainel>
    <div style={{ flex: 1, padding: 'var(--esp-16)', display: 'flex', flexDirection: 'column', gap: 'var(--esp-12)' }}>
      <BalaoDoUsuario>Confere o estoque do lote 418</BalaoDoUsuario>
      <RespostaDoAgente><p>Conferi: estão todos no lugar.</p></RespostaDoAgente>
    </div>
    <CaixaDeMensagem ativa contexto={0.12} chips={<ChipDaCaixa marca="claude" seta>Opus 5.5</ChipDaCaixa>} />
  </Materiais>
</RaizVitral>
```
