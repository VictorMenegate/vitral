O Vitral é um app de desktop (Rust e GPUI) para conversar com o Claude Code e o Codex em vários painéis lado a lado. É escuro, de um grafite frio em degraus, com um destaque só, o azul, e com uma chapa de metal líquido azul atrás de tudo: é ela que aparece no "+", no enviar, no escolhido e no "é aqui". Este sistema é a fonte para redesenhar o app; o código é a verdade, e cada valor daqui saiu de `src/tema.rs` e dos componentes em `src/ui/componentes/`.

## As regras do dono

Decisões que o Victor já tomou e que o código registra ("dono, data"). Valem acima de qualquer gosto; mudar uma delas é decisão dele.

1. **Abas separadas, cada uma com moldura própria e um fio preto de 1 px** (`preto`), numa linha só, sem casca dobrada em volta (13/11 e 16/11/2026: "diminui as bordas"). A aba à vista tem o material do campo da caixa de mensagem (12/11 e 13/11/2026). Ver `AbaDePainel`.
2. **Azul, não amarelo, para "esperando"**: o que espera você (pedido de permissão, parada que pede Retomar, o número do sino) é o `espera` #8ECBFF (13/11/2026: "por que tem umas coisas amarelas? tem que ser azul"). O `ambar` fica para aviso de verdade: o modo Sem perguntar, o uso do plano alto, o custo do Ultracode.
3. **Raio 16 nas peças**: os cartões da janela (a conversa, os painéis) e o balão usam `raio-da-peca` (16). Foram a 24 ("mais arredondados ainda") e voltaram no mesmo dia, porque a curva grande destoava das abas (13/11/2026).
4. **Balão do usuário na largura toda** da coluna (16/11/2026: "faz o campo das minhas mensagens ficar na largura toda"), em `balao` com a ponta embaixo à direita.
5. **Luz azul embaixo da caixa ativa**: a marca do painel selecionado é a luz de baixo (o metal com 20% de destaque, sumindo para o escuro em cima e nos lados), não borda nem cantoneira (12/11/2026; 13/11: "menos nos lados pra ficar mais no meio só").
6. **Anel de contexto com a porcentagem ao lado**, em mono, também na caixa estreita: sem ela o anel parecia o "carregando".
7. **Pílulas de imagem**: as imagens que você manda e as que o agente usa ficam em pílulas com a miniatura, o nome e as medidas, como na extensão do Claude Code (16/11/2026); várias de uma vez viram a pilha.
8. **O metal azul é o "escolhido"**: a ficha de metal marca o item escolhido em toda lista, segmentado e popover de escolha (voltou em 13/11/2026; o miolo do metal com véu marinho desde 16/11).
9. **Anéis de uso embaixo do × da janela**, no canto de cima à direita, um em cima do outro, flutuando sobre a conversa (13/11/2026; voltaram para lá em 16/11: "o de uso tem que ficar onde eu tinha falado, embaixo do ×").
10. **Vão entre painéis igual à margem da janela**: 8 px de tela, os dois iguais e sem crescer com o zoom (16/11/2026: "a do meio dos chats tem que ficar igual das bordas").
11. **Mais moderno e com menos cara de IA** (12/11/2026): o acabamento Grafite, chapado, sem degradê, sem reflexo e sem halo; depois, "partes transparentes e bordas estilizadas" e "tenta focar em vez de glass esse metal líquido". O metal é a assinatura; o vidro azul-marinho de antes continua no código (`superficie::MARINHO`), fora de uso.
12. **Sem a bordinha de cima das peças** (13/11/2026): a aresta de vidro tem o fio só nos lados.
13. **O cabeçalho do painel tem pouca coisa**: as abas, o modo foco (só ligado) e o "+", que cria um chat ou abre o histórico (16/11/2026); sem o "⋯", o menu do painel mora no botão direito.
14. **O ícone do chat é o estado dele** (13/11/2026: "tô achando esse ícone meio inútil").
15. **O número do painel fica à direita da aba, antes do ×** (13/11/2026), e some com a lista recolhida.
16. **Cantos bem arredondados nos controles**: o dono viu a versão de cantos curtos e preferiu assim (12/11/2026); os raios crescem com o zoom (13/11/2026).
17. **Os chats da lista um pouco maiores**: linha de 36 e título 15 (13/11/2026).
18. **A moldura da caixa com 2 de folga**, não 4 (16/11/2026), e sem a linha das teclas embaixo quando parada (13/11/2026).
19. **Os fundos opacos por padrão**; a transparência (Leve, Média, Forte) é escolha das Configurações, sem desfoque (13/11/2026: "se não for, deixa sem mesmo").
20. **Nada anima em repouso**: o metal fica parado e só anda com um chat à vista trabalhando (12/11/2026: a rolagem engasgava).

## Conteúdo e voz

- Tudo em português do Brasil, frases curtas, sem travessão nem meia-risca. Dois pontos ou vírgula no lugar.
- **"Chat" é o objeto, "conversa" é o conteúdo**: "Novo chat", "Fixar chat", "Buscar na conversa". O relatório de design (D3) pede para acabar com "Nova conversa" como título de chat.
- O que o app diz no meio da conversa é impessoal e sem instrução: "O Vitral fechou no meio do turno", "Sessão nova, com um resumo da anterior". Os erros do app fora da conversa ficam na primeira pessoa, a voz do app ("Não estou conseguindo gravar").
- Erros dizem o que aconteceu e o que fazer, em português, com o texto cru do agente atrás de "Detalhes": "O Codex chegou ao limite de uso" / "Volta às 12:30."
- Estado é adjetivo e minúsculo: "rodando", "esperando você", "parada", "falhou".
- Verbos de ação: **Enviar** (nunca "Mandar"), **Remover** para tirar de uma lista, **Apagar** para destruir, **Retomar**, **Tentar de novo**, **Compactar**.
- Números e medidas com espaço e dígitos tabulares: "3 d", "22 min", "2 min 13 s", "601×263", "52%"; o relógio do turno em mono, "00:03".
- Verbos de ferramenta no passado e ao vivo: "Leu", "Lendo"; "Rodou", "Rodando"; "Editou", "Editando". O nome real vai na dica.
- Nada de emoji na interface. As reticências ainda não têm regra ("Buscar pasta…" e "Buscar" convivem; relatório de design, D16).

## Fundamentos visuais

### Cor
- **Grafite em degraus**: `vidro-leitura` (a base, sobre o metal), `cartao` e `cartao-opaco` (a conversa e os painéis), `cartao-elevado` (aba, barra de ações, miolo da caixa), `flutuante` (menus e popovers, um degrau acima da peça de onde saem), `preto` (código, moldura da caixa). O que separa as peças é o degrau de tom.
- **Um destaque só**: `destaque` #2B63FF ("um azul mais azul"), no + e no enviar (debaixo do metal), no interruptor ligado, no anel. Como texto, `destaque-texto` #7C9DFF (links, o que roda, o número do painel ativo). Seleção, hover e foco do kit são neutros (`selecao`, `hover`).
- **Texto**: `forte` (títulos, escolhido), `texto` (corpo), `tenue` (segunda voz), `apagado` (metadados), `fantasma` (só decorativo), `codigo`.
- **Estado**: `espera` (precisa de você), `vermelho` (erro), `ambar` (aviso de verdade), `verde` (pronto, linha que entrou), `violeta` (automação, Ultracode). Cor de estado vem sempre com ícone ou palavra.
- **Marcas**: `claude` e `codex` só no logo delas e no arco do anel de uso.
- **Tintas**: a cor de estado entra no fundo a 12% (`tinta-*`, pílulas e selos) ou a 6% com fio a 18% (`tinta-leve-*`, cartão de erro e alerta). Misturada à base, opaca: `balao`, `estado-*`, `aba-inativa`, `moldura-perigo`.
- **Contraste**: o teste `contraste_dos_tokens_no_pior_caso` roda contra a cor mais clara do metal (`metal-teto`). Em tudo que é opaco (o que flutua, a caixa, o balão), todo texto passa de 4,5:1. Nos fundos translúcidos (a base, o cartão nos níveis transparentes, o cartão de erro), `forte` e `texto` passam de 4,5:1 e as vozes baixas e as cores de estado ficam em 3:1, decisão registrada no código (13/11/2026). `destaque` como texto pequeno não passa: use `destaque-texto`.

### O metal
A chapa (receita "Mesh drift": cinco bolhas, grão e vinheta) fica atrás de tudo; a base é ela debaixo do `vidro-leitura`. Os recortes dela são o vocabulário do app: a **ficha do escolhido** (o miolo com o `veu-de-placa`), a **placa** dos menus, a **tecla de metal** (+, enviar, parar, com o destaque a 60%), o **aro de 2 px** ("é aqui": caixa e busca com foco, aba sob o ponteiro) e o de 4 (o pedido que espera você), e a **doca** (a luz azul do painel ativo). Texto nunca no metal cru. Sem halo, sem borda azul, sem cantoneira. Ver `Metal`.

### Materiais
Vidro nas peças (`aresta-de-vidro`), tecla no que está em relevo e escolhido (`aresta-de-tecla`, `aresta-de-ficha`), poço no que é cavado (`aresta-de-poco`). Tudo luz e sombra de 1 px por dentro. A casca dos flutuantes é a `sombra-popover`. Ver `Materiais`.

### Tipografia
Duas vozes, com as fontes embutidas no binário (OFL, em `fonts/`):
- **Inter**, a que conversa: `corpo` 15/23 (a resposta), `fala` 14/22 (o seu balão, um degrau abaixo), `corpo-ui` 14/20, `rotulo` 13/18 em 500 (botões, abas), `metadado` 12/16, `micro` 11/14 em 600 (selos), `titulo-tela` 18/24, `titulo-heroi` 28/34, `titulo-cartao` 15/20, `numero` 20/24. Números pequenos sempre tabulares. Markdown: títulos de 18, 16 e 15.
- **JetBrains Mono**, a segunda voz, de aparelho de medição: `rotulo-secao` (11, `apagado`, caixa alta no desenho, com o tracinho de 8 x 1), `etiqueta` 11/14 (fatos e medidas), `mono` 13/20 (código), `mono-pequeno` 12/16, `tecla` 10.
- O app mede em rem de 15 px: tudo cresce com o zoom (100%, 125%, 150%). Os valores aqui são a 100%.

### Espaço e composição
- Grade de espaços: só 2, 4, 6, 8, 12, 16, 24 e 32 (`esp-*`); o teste `espacos_na_escala` barra o resto.
- A janela: margem de 8 (`margem-da-janela`) e vão de 8 entre as peças (`vao-entre-pecas`). A lista (312) mora direto na base; as conversas (no mínimo 280) e o painel da direita (380) são cartões de canto 16 com `sombra-flutuante`.
- Coluna de leitura de 806 com 16 de margem: mensagens, faixa e caixa alinham pelas mesmas bordas.
- Ritmo da conversa: 32 entre turnos (`esp-entre-turnos`), 12 dentro do turno (`esp-no-turno`), 16 em volta de marco, linhas de log coladas no passo de 32.
- Cabeçalho do painel e topo da lista: 44 (`altura-cabecalho`), na mesma linha.

### Cantos
Três raios e a pílula: `raio-controle` 10 (linhas, campos, miniaturas), `raio-cartao` 14 (cartões de dentro, código, erro, alerta), `raio-peca` 20 (caixa, popovers, menus, diálogos) e `raio-da-peca` 16 (os cartões da janela e o balão). Nos controles, `raio::de(altura)` = 45% da altura (`raio-de-*`): bem arredondado, nunca redondo, o desenho do logo em escala. Só ponto, barra fina e anel são redondos. Dentro de quem tem padding p, o raio é max(raio de fora menos p, 10).

### Alturas
Selo 20, botão de linha 24, botão de barra 28, controle 32, grande 36, cabeçalho 44, item com descrição 48, cartão de duas linhas 52, linha de chat 36, linha de log 32 (`altura-*`).

### Movimento
- Durações: 120 ms (hover, cor), 160 (popover, menu, mensagem nova), 180 (a pílula do segmentado, o foco), 240 (anéis, pulso do enviar). Curvas sem ricochete: entrar `cubic-bezier(0.16, 1, 0.3, 1)`, mover `cubic-bezier(0.2, 0, 0, 1)`, sair ease-out cúbica.
- Teto de 60 quadros por segundo; o metal anda a 20 só com um chat à vista trabalhando; o anel girando dá uma volta a cada 1,2 s.
- A barra de ações das mensagens e a barra de rolagem aparecem secas, sem fade. Com o movimento reduzido (GNOME, Configurações ou `VITRAL_SEM_ANIMACAO=1`), tudo direto no estado final.

### Estados de interação
Hover `hover` (neutro); escolhido em lista e segmentado na ficha de metal; pressionado `pressionado`; foco pelo teclado `halo-foco` (o "é aqui" do ponteiro e da caixa é o aro de metal); desabilitado a 0,45 com o motivo na dica; em repouso a 0,55 (nunca abaixo, para não ler como desabilitado).

## Iconografia
- **Lucide** (ISC), os SVG que o kit embute (gpui-kit-assets 0.8.1), em traço de 2 no quadro de 24. Tamanhos: 12 (chevrons, marcos), 14 (linhas, botões de linha, chips), 16 (botões de barra e controle, menus), 20 (estado vazio, símbolos de lista). Cor `tenue` por padrão; a cor só quando ela é o sinal (o escudo do Sem perguntar em `ambar`, a ferramenta que falhou em `vermelho`).
- Ícones de ferramenta: BookOpen ler, FilePenLine editar, FilePlus gravar, Terminal comando, TextSearch buscar, FolderSearch listar, Globe web, Bot subagente, Workflow fluxo, ListChecks passos, Brain pensou.
- Modos: escudo cheio (Manual, `escudo-cheio.svg`), FilePen (Aceitar edições), Map (Planejar), WandSparkles (Automático), ShieldOff (Sem perguntar).
- **Marcas**: o disco cheio (`claude.svg`, `codex.svg`, `linear.svg`) nas listas, popovers, na dupla e no anel de uso; o glifo solto (`claude-glifo.svg`, `codex-glifo.svg`) nos chips da caixa e das Configurações. Sempre por imagem, nunca recoloridas.
- O parar é um quadrado de 10 com canto 2,6, não um ícone.
- Sem emoji. Os previews deste sistema usam os mesmos SVG do Lucide como máscara na cor do texto (`.vt-i-<nome>` em `components/bundle.css`).

## Como usar este sistema
- Os tokens têm os nomes das funções de `src/tema.rs` (`Cores::tenue()` é `--tenue`, `raio::CONTROLE` é `--raio-controle`, `esp::E12` é `--esp-12`, `altura::BARRA` é `--altura-barra`). Mudou um valor aqui, ele volta para o mesmo lugar no código.
- Os componentes do vitral-ui são recriações em React do que o app desenha em Rust: servem para desenhar, comparar e propor. O `.prompt.md` de cada um diz a anatomia, as regras, o jeito de usar em React e o arquivo de origem em Rust.
- `tokens/tokens.css` tem as variáveis (com o uso de cada uma em comentário) e as classes de tipografia `vt-tipo-*`; `_ds_bundle.css` monta as peças só com `var(--token)`, nas classes `vt-*`.
- Os relatórios de 16/11/2026 do repositório do app (`arquivo/design-relatorio`, `aparencia-relatorio`, `balao-erro-spec`) têm o vocabulário e a lista do que já foi feito e do que ficou para depois.

## Propostas que ainda não estão no código
Para não confundir o que é decisão com o que é proposta: a ficha de metal também no chat aberto da lista, no lugar da barra azul (V24); o interruptor em rem e com o metal (V23); o boas-vindas com a peça dos diálogos (V18); a gaveta encaixando no vão entre painéis (V20); a rodada 2 de vocabulário e componentes (D3, D6 a D18, P11, P15 a P19) e a rodada 3 (P2 completo, P7, P12, o fim do D18).

## Fora deste sistema
O acabamento azul-marinho (`superficie::MARINHO`, fora de uso), os níveis de transparência como temas, o ciano do esforço (`Cores::ciano()` hoje é igual a `texto`), o tema claro (o app não tem), o shader exato do metal e das animações, o diálogo, o painel da direita, o boas-vindas e o "Enquanto você esteve fora" como componentes próprios.
