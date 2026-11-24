# Vitral

> A porta de entrada do projeto é o [README.md](README.md) (o que o Vitral é, como instalar, como
> conferir o que baixou). Este arquivo é o manual de dentro: como cada parte funciona, o que já foi
> decidido e as pegadinhas.

Ambiente para trabalhar com o Claude Code em várias pastas ao mesmo tempo: uma lista de chats, cada
um trabalhando direto numa pasta, como o Claude Code na extensão do VS Code, com as conversas lado
a lado em painéis. O fundo é uma chapa de metal líquido azul (ver **Acabamento**; começou com o
vidro azul-marinho da Stained Glass UI) e a organização vem do Ratoeira (seções "Esperando você" e
"Rodando agora", abas de chat). Feito em Rust com GPUI (o motor de interface do Zed) e os
componentes do GPUI Kit.

## Como funciona

- **Chats.** A lista da esquerda tem todos os chats, uma linha cada (o balão, o título e a hora):
  em cima o que espera você, o que está rodando e as fixadas, depois todos os outros, do mexido por
  último ao mais antigo. Quem monta a lista é `Motor::chats_da_lista` (em `src/motor.rs`): tudo o
  que mostra ou conta a lista passa por ali. Cada chat guarda o título, a pasta (`cwd`), a sessão do
  Claude Code, o modelo, a permissão, a conta, se está fixado ou arquivado e as mensagens.
- **Cada chat mexe direto na pasta dele.** Sem cópia, sem juntar e sem commit do Vitral: o Claude
  Code roda na pasta, como no terminal ou no VS Code, e a sessão é a da pasta (`--resume` a partir
  dela). **Sem pasta** dá ao chat uma pasta vazia só dele, em `~/.local/share/vitral/avulsos/<chat>`.
- **Chat novo.** O "+" e o Ctrl+N criam um na hora, sem perguntar, como o "novo chat" do VS Code
  (`novo_chat_direto`, em `src/ui/dialogos.rs`): com **pasta de trabalho** (`Config.pasta_de_trabalho`,
  escolhida nas boas-vindas; **Pasta dos chats novos**, em Configurações, Chats, troca), é sempre nela.
  Sem pasta de trabalho: na pasta do chat aberto (o chat Sem pasta abre outro Sem pasta); sem chat
  aberto, na última pasta usada (a do último chat criado, guardada como
  `ultimo_destino` no `interface.json`, ou a do chat mexido por último); sem nenhuma, a paleta
  pergunta. **Novo chat em outra pasta…**, no menu do logo, abre a paleta "Em qual pasta?": as
  pastas que já têm chat (a usada há menos tempo primeiro, com a busca no nome e no caminho), os
  **Worktrees**, **Outra pasta…** (o seletor do sistema) e **Sem pasta**. O grupo Worktrees
  (`src/ui/copias.rs`) traz as cópias dos repositórios dessas pastas: o que o `git worktree list
  --porcelain` de cada uma devolve (`copias_das_pastas`, em `src/git.rs`), lido em segundo plano
  ao abrir a paleta, com o nome da pasta e, embaixo, o ramo (ou "cabeça solta em <commit>") e o
  caminho; a busca acha pelo nome, pelo ramo e pelo caminho. A cópia que já tem chat fica só em
  Pastas, a que teve a pasta apagada não aparece, e o Vitral só lista e abre: não cria nem apaga
  worktree. Quem cria é `Motor::novo_chat(pasta:
  Option<&Path>, …)`; a pasta chega canônica. O título vem da primeira mensagem (`titulo_de`, em
  `src/motor.rs`): a primeira linha com texto, sem as marcas de markdown do começo ("## ", "> ",
  "- ", "- [ ] ", "1. "), até 7 palavras. Depois de um turno, esse título automático dá lugar ao
  que o Claude Code gerou para a sessão (a linha `ai-title` do arquivo dela, lida do fim dele por
  `importar::titulos_da_sessao`), uma vez só: `titulo_automatico` compara o título com o da
  primeira fala, então o que você renomeou (F2), o do ramo e o da raia ficam como estão, e o
  título trocado não troca de novo. A sessão renomeada no Claude Code (`/rename`, a linha
  `custom-title`) e o chat do Codex também não mudam. Num `-p` o Claude Code não gera esse título
  sozinho (medido no 2.1.328: ele só o faz no terminal, no VS Code e pelo SDK), então o turno o
  pede pela entrada: o `generate_session_title` com a primeira fala e o `persist`
  (`Processo::pedir_titulo`), até duas vezes por chat em cada abertura do app, num Claude Code 2.1.323
  ou mais novo. É uma chamada pequena a mais do Claude Code por chat novo.
- **Claude ou Codex.** Cada chat diz quem responde (`Chat.agente`, em `src/modelo.rs`): o Claude
  Code ou o Codex, a CLI da OpenAI. Escolhe-se no seletor de modelo, que tem as abas **Claude** e **Codex**
  (trocar de aba só mostra os modelos do outro; quem responde muda ao escolher um; a aba do Codex tem
  o padrão dele e os modelos do `codex debug models`, relidos a cada abertura, com a descrição em
  português quando a frase do catálogo é reconhecida, `descricao_do_codex`); o padrão dos chats
  novos é `Config.agente_padrao` (Configurações e boas-vindas). O Codex roda um `codex app-server`
  por turno, com o protocolo JSON-RPC (`src/agente_codex/servidor.rs`): `thread/start` ou
  `thread/resume`, `turn/start`, `turn/steer` para mensagens durante o trabalho e `turn/interrupt`.
  A confirmação do steer tira a mensagem das pendentes; uma recusa devolve só ela à fila. O fim
  espera as confirmações, para não perder mensagens na corrida com o encerramento. O `thread_id`
  fica em `Chat.sessao`, e `thread/resume` recebe esse id. O schema veio da própria
  CLI instalada, e o initialize e thread/start foram conferidos sem inferência em 16/11/2026.
  **Pedido de permissão no meio do turno** (24/11/2026): cada modo é um par sandbox + política de
  aprovação (`sandbox_de` e `aprovacao_de`, em `src/agente_codex.rs`), que vai no `thread/start`
  (ou `thread/resume`) e de novo no `turn/start`:

  | Modo do Vitral | sandbox | approvalPolicy | O que pergunta |
  | --- | --- | --- | --- |
  | Manual | `workspace-write` | `untrusted` | toda edição e todo comando que não seja só de leitura |
  | Aceitar edições | `workspace-write` | `untrusted` | os comandos; a edição dentro da pasta o Vitral permite sozinho (`modelo::modo_permite`) |
  | Planejar | `read-only` | `on-request` | quando o Codex quer escrever ou usar a rede |
  | Automático | `workspace-write` | `on-request` | quando o Codex quer sair da pasta ou usar a rede |
  | Sem perguntar | `danger-full-access` | `never` | nada |

  Os pedidos `item/commandExecution/requestApproval` e `item/fileChange/requestApproval` viram
  `Evento::Permissao` e caem no mesmo cartão do Claude: o comando inteiro (sem o embrulho
  `/bin/bash -lc`) ou o mini-diff do arquivo, que vem do `item/started` do `fileChange` (o
  pedido de edição só traz o `itemId`; com vários arquivos, um diff só, com o caminho do primeiro
  de fora da pasta). Permitir é `accept`, Negar é `decline` (o item volta `declined` e o turno
  segue), e o cartão que some sem resposta é `cancel`. O `serverRequest/resolved` fecha o canal do
  cartão (`Evento::PermissaoRetirada`) e ele sai. O "Permitir sempre" e o modo de agora valem para
  o Codex como para o Gemini, e a revisão pelo outro agente tem tudo negado. O "Negar e explicar"
  manda só o `decline`: a explicação não chega ao Codex. Medido no 0.169.0 com turnos de verdade:
  o id do pedido é um número, o `availableDecisions` do comando não lista o `decline` (que mesmo
  assim vale) e o `resolved` chega logo depois de cada resposta.
  **Comandos de barra** (24/11/2026): os que o app-server atende. O `/compact` vai como
  `thread/compact/start` (medido no 0.169.0: responde `{}` e abre um turno com o item
  `contextCompaction`, que vira o "Compactando…" e o marco, como no Claude; o texto depois dele
  não vai, o Codex não aceita foco); o `/review` como `review/start` (sem texto, as mudanças sem
  commit; com texto, o que ele pede; o texto do item `exitedReviewMode` é a resposta); e cada
  skill da pasta (o `skills/list`, pedido em todo turno) é um "/nome", que vai como a entrada
  `skill` com o arquivo dela e o texto "$nome …", como o Codex faz. A mensagem com barra espera a
  lista; a lista que falha não para o turno. O `Inicio` leva a lista (`compact`, `review` e as
  skills) para `Chat.comandos`, e o menu "/" mostra o `/clear` do Vitral mais ela
  (`comandos_fora_do_claude`); antes do primeiro turno, só os dois. Compactar (o "…" da caixa, a
  paleta, o popover do contexto) vale no Codex também.
  **Contexto e tokens** (24/11/2026): o `thread/tokenUsage/updated` chega a cada chamada ao modelo
  (medido no 0.169.0). O `last.totalTokens` é o contexto de agora (`Evento::Contexto`, o anel da
  caixa), o `modelContextWindow` é a janela (vai no `Fim`; até o fim do primeiro turno, o anel usa
  a padrão), e o gasto do turno é o `total` do fim menos o de antes da primeira chamada dele
  (`Evento::Gasto` antes do `Fim`: a dica da linha "Trabalhou", o "Último turno" do popover e o
  livro do **Onde o uso foi**). Só o uso do turno que roda conta para o gasto. O popover do
  contexto do Codex não tem custo nem conta.
  **Passos** (24/11/2026): o `turn/plan/updated` (a lista inteira a cada mudança, com `step` e
  `status`) vira uma chamada TodoWrite com o resultado na hora (`todos_do_plano`), e a lista de
  passos do chat (`motor/passos.rs`), a pílula "1/3" e as linhas da trilha ("Criou 3 passos",
  "Concluiu: … · Começou: …") saem como no Claude. Conferido só pelo schema e pelo Codex de
  mentira: nos turnos de verdade do 0.169.0 (gpt-6-luna e gpt-6.2-sol) o modelo disse que o
  `update_plan` não estava disponível, e nenhum plano chegou.
  O que ainda muda num chat do Codex: não há conta nem custo em dólar, e parar é pelo
  `turn/interrupt` (sinal como reserva). Trocar o agente de um chat (`Motor::definir_agente`) começa uma sessão nova no
  outro, com as últimas mensagens no bloco de passagem (`MotivoPassagem::TrocouDeAgente`); com um
  turno rodando, não troca.
- **Gemini.** O terceiro agente (`Agente::Gemini`, desde 23/11/2026) é o Antigravity CLI do Google
  (`agy`), o sucessor do Gemini CLI, que parou de atender as contas pessoais em 18/06/2026. Ele
  aparece na aba **Gemini** do seletor quando o `agy` está instalado (`agente_gemini::instalado`:
  o PATH ou `~/.local/bin`). Roda um `agy acp` por turno, com o ACP (Agent Client Protocol), em
  `src/agente_gemini.rs`: `initialize`, `session/new` (ou `session/resume` com o id guardado em
  `Chat.sessao`; uma conversa que o agy não tem mais abre uma nova), as opções `model`,
  `thought_level` e `mode` por `session/set_config_option` antes do primeiro `session/prompt`, e o
  prompt de sistema em `_meta.antigravity.systemPrompt.append`. Uma mensagem com o turno rodando é
  outro `session/prompt`, que o agy absorve no mesmo turno (o anterior responde `end_turn` na hora;
  o fim espera a resposta do último). O pedido de permissão (`session/request_permission`) vira
  `Evento::Permissao` e cai no mesmo cartão dos pedidos do Claude Code; a resposta volta ao agy por
  uma thread (sem resposta, `cancelled`). Parar é o `session/cancel`. O "Sem perguntar" é o
  `--dangerously-skip-permissions` do processo; os outros modos viram os do agy (`modo_de`). Os
  modelos vêm do `agy models` (as variantes de nível juntam num modelo só; o nível é o esforço da
  caixa, até Alto) e das opções da sessão. Sem anel de uso (o Antigravity não expõe a cota), sem
  conta, sem comandos de barra nem `/compact`, e sem as ferramentas do Vitral: o agy só aceita
  servidor MCP local. Instalar e entrar (`uso_plano::instalar_gemini`) abre o terminal com o
  instalador oficial e o `agy acp --login`. Conferido no agy 1.5.15 com uma conta pessoal.
- **Claude e Codex no mesmo chat (a dupla).** `Chat.parceiro` guarda a sessão, o modelo e o uso do
  outro agente; `Chat::trocar_vez` troca os dois de lugar, então todo o resto do turno (conta,
  modelo, retomar a sessão) continua lendo `chat.agente` e `chat.sessao` como sempre. Em
  `comecar_turno_com`, a mensagem do usuário escolhe quem trabalha (`alvo_da_dupla`: "@claude" ou
  "@codex"; sem @, quem está na vez); `o_que_o_outro_fez` monta, a partir da última `Item::Vez` do
  agente, o que o usuário mandou e o que o outro disse e mexeu, e vai antes do texto do turno. A
  dupla nasce sozinha, ao trocar de agente ou chamar o outro com @. Escolher no seletor um modelo
  do outro agente muda o modelo dele (`definir_agente`), sem desfazer a dupla. O chip do modelo
  mostra as duas marcas sobrepostas (`Simbolo::Dupla`, a de quem trabalhou por último na frente) em
  toda largura. A chave "Os dois em toda mensagem" (as rodadas em que um trabalhava e o outro
  revisava) saiu em 24/11/2026: o campo `Chat.revisao_em_dupla` ainda é lido, para os dados
  antigos abrirem, e não vale mais nada.
- **A revisão do turno pelo outro agente.** `src/motor/revisao_do_par.rs` (decisão do dono em
  23/11/2026). No `fim_de_turno` sem erro, o turno que mudou `LINHAS_PARA_REVISAR` (20) linhas ou
  mais (a conta das Mudanças do turno, `linhas_mudadas_no_turno`) é revisto pelo outro agente (o
  Codex num chat do Claude, o Claude nos outros; `Agente::outro`), se a opção
  `Config.revisar_edicoes` ("Revisão automática pelo outro agente", em Configurações, Agentes;
  ligada de fábrica) estiver ligada. Não roda com o outro fora (não instalado ou no limite do
  plano), com o chat em Planejar, nem no turno que veio de uma revisão mandada (nada de laço;
  `motivo_sem_revisao`). O revisor entra como o parceiro da dupla (`abrir`, que cria o parceiro se o
  chat não tinha), roda um turno só em Planejar (no Codex, o sandbox `read-only`; o pedido de
  permissão dele é negado sem perguntar) e o chat volta a quem era (`fechar`): o que ele fez sai da
  conversa e entra o resultado, a linha "Codex revisou: nada a acrescentar" ou o `Item::Revisao`, o
  cartão com o achado e **Mandar ao Claude** (`Motor::mandar_revisao`, que entrega o texto ao agente
  do chat como uma mensagem de "Revisão do Codex"). Enquanto roda, a linha de estado diz "Codex
  revisando o turno…" e o chat conta como parado; uma mensagem sua corta a revisão, sem deixar
  nada. Na linha "Trabalhou…" do último turno pronto, **Revisar com o Codex** (ou "com o Claude";
  `ui/mensagens/revisao.rs`, `Motor::pode_revisar`) pede a mesma revisão para qualquer turno. Na
  bancada, `[ferramenta EditGrande x.py]` faz um turno de 30 linhas e `[revisor-acha]` na mensagem
  faz o revisor de mentira achar algo.
- **Subagente em segundo plano.** Desde o Claude Code 2.1.3xx a chamada Agent volta na hora com
  `tool_use_result.status = "async_launched"`: isso vira `Evento::SubLancado`, e não resultado. O
  `system/task_progress` conta as ferramentas (`SubProgresso`) e o `system/task_notification`
  fecha a chamada com o `summary` (`SubFim`). O `ferramentas/claude-falso/claude` encena isso no
  `[ferramenta Agent …]`; o `Task` segue no jeito antigo, que termina no próprio resultado.
  **Quem são e o que fazem** (`src/motor/fundo.rs`, medido com a CLI de verdade, 2.1.328, Haiku,
  em 23/11/2026, com um agente e com dois): o `background_tasks_changed` traz a lista inteira a
  cada mudança, com o `task_id` (o id do agente) e a `description` de cada `local_agent`
  (`Evento::AgentesNoFundo`, tirado pela `claude::agentes_do_fundo`, ao lado do `interpretar`), e
  o `task_started` diz qual chamada abriu cada um (`Evento::AgenteAbriu`). O arquivo dele é
  `<sessão>/subagents/agent-<id>.jsonl`, com o `agent-<id>.meta.json` ao lado (`description`,
  `toolUseId`, `model` e, no fim, `finished`). O arquivo nasce com uns 230 KB (os anexos e o
  pedido à API) e a última linha só é a chamada do agente enquanto ela roda: entre uma chamada e
  outra, o fim são o resultado, anexos e as linhas `api-request-blob` e `api-request`. A última
  ação mora no cartão da chamada (`Subagente::atividade`): chega pelo stream (as chamadas do
  agente continuam vindo depois do `result`, presas à chamada que o abriu) e pelo fim do arquivo,
  que traz o que ele diz entre uma chamada e outra. O fim é lido no máximo uma vez por segundo
  (`Motor::conferir_segundo_plano`, no compasso de um segundo da Janela), só se o arquivo cresceu,
  só o pedaço novo e nunca mais que 256 KB (`fundo::ler_o_fim`, com o ponto da leitura anterior
  guardado), em segundo plano; o que ele acha só entra se for mais novo que o que o stream já
  entregou (`fundo::anotar_acao`), e quem redesenha são só os painéis daquele chat
  (`TextoChegando`), não a janela. Com mais de um agente, o Claude Code abre um turno a cada
  agente que termina (outro `init`, uma frase) e **segura o `result` desses turnos até o último**,
  soltando todos juntos: o primeiro fecha o turno no Vitral, com o que o agente disse por último,
  e os seguintes só entram no livro dos gastos (antes, cada um fechava o turno de novo: um
  "Terminou" por agente).
- **Corrigir qualquer mensagem.** `motor::editaveis_desde` diz desde onde as mensagens do usuário
  dão para corrigir (depois da última entrega de outro chat, cartão ou item desconhecido; a entrega
  do próprio Vitral não tranca) e `motor::editavel` confere uma; `retirar_mensagem` tira a mensagem
  e o que veio depois. Na dupla, a sessão do parceiro também recomeça. **A sessão volta ao ponto,
  sem resumo:** para o Claude não lembrar do que saiu, o próximo turno roda numa sessão nova, que
  é uma cópia da de antes até o ponto anterior à mensagem corrigida (`--resume <sessão>
  --fork-session --session-id <nova> --resume-session-at <uuid>`, o mesmo corte do "Continuar
  daqui em outro chat"; `Bifurcacao::correcao`, `bifurcar::sessao_depois_da_correcao`). O agente
  fica com o contexto exato do que veio antes, e o começo da conversa é o mesmo que o Claude Code
  já tinha mandado, em vez de um resumo de dez mensagens relido do zero. Quando o ponto não é
  achado no arquivo da sessão (Codex, conversa de antes das marcas, a primeira mensagem da
  sessão, mensagem dirigida ao turno), vale o caminho de antes: sessão vazia com o bloco de
  passagem (`MotivoPassagem::Reenvio`); se o Claude Code recusar o corte, o turno é repetido
  assim, com um aviso na conversa. Corrigir duas vezes seguidas, antes de mandar, corta de novo
  na sessão de origem. A cópia de uma sessão traz o custo acumulado da original (o Claude Code
  restaura o `cost-state` dela, lido no binário da 2.1.325): o chat leva o último total visto
  para a sessão nova, e o `somar_custo` soma só o que crescer. A pergunta antes de corrigir só
  fala em "sessão nova, com um resumo" quando é isso que vai acontecer. O corte foi medido sem
  mandar mensagem (ver o "Continuar daqui em outro chat"); a leitura de cache no turno seguinte
  não foi medida, porque pediria um turno pago.
- **Continuar daqui em outro chat (bifurcar).** Nas ações de uma mensagem sua e de uma resposta
  (o ícone do ramo; `src/ui/bifurcar.rs`) e na paleta de comandos (a partir da última resposta):
  nasce um chat novo na mesma pasta, "título (ramo)", com os itens da conversa até aquele ponto, o
  mesmo agente, modelo, modo e regras de permissão, aberto num painel ao lado (sem lugar, numa aba
  do mesmo); o original não muda. O corte é sempre na borda de um turno: numa resposta, vai o
  turno dela inteiro; numa mensagem sua, tudo o que veio antes, e o texto dela (com os anexos)
  vai para a caixa do chat novo. No Claude com sessão gravada, o primeiro turno do chat novo sai
  numa cópia da sessão de origem, pelo caminho das conversas trazidas (`--resume <origem>
  --fork-session --session-id <nova>`, o `bifurcar` de `src/claude.rs`), cortada no ponto com
  `--resume-session-at <uuid>` (`Opcoes::cortar_em`): o agente lembra do que veio antes sem
  resumo. O uuid é o da última entrada do agente ou do usuário antes da mensagem que abre o turno
  seguinte, achado no arquivo da sessão subindo a cadeia de `parentUuid` a partir da marca dela
  (`motor/bifurcar.rs`, `ponto_de_corte`); o ponto fica em `Chat.bifurcado_de` até a sessão nova
  nascer. No Codex, sem sessão, sem marca (conversa antiga, mensagem dirigida ao turno) ou sem o
  ponto no arquivo, o chat novo começa numa sessão nova com o bloco de passagem
  (`MotivoPassagem::Bifurcou`). A flag `--resume-session-at` está no Claude Code 2.1.325 fora do
  `--help`. Medido em 18/11/2026 numa sessão de verdade, abrindo o processo sem mandar mensagem
  (não gasta o plano, como o `rewind_files`): ela aceita o uuid de qualquer entrada da cadeia (do
  agente, sua, de anexo ou de sistema), a sessão retomada inteira tinha 40.388 tokens de
  mensagens e, cortada na primeira mensagem, 39 (o `get_context_usage`), e um uuid que não está
  na sessão sai com "No message found with message.uuid of". Nesse caso (e com a sessão de origem
  apagada) o Vitral repete o turno numa sessão nova com o bloco de passagem
  (`consertar_bifurcacao`). Não foi medido um turno de verdade depois do corte. Com o último
  turno ainda rodando, bifurcar na última resposta espera.
- **Economia de tokens sem mexer na resposta.** Três coisas, nenhuma corta saída, baixa esforço ou
  troca modelo sozinha. (1) `claude::ambiente` desliga o gasto de fundo que o Vitral não mostra
  (`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`, `CLAUDE_CODE_ENABLE_AWAY_SUMMARY=0`,
  `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`). (2) `Chat::fria` (em `src/modelo.rs`): conversa do Claude
  com sessão, `CONTEXTO_GRANDE` (100 mil tokens) ou mais e parada há `DESCONTO_DURA` (uma hora, o
  tempo do cache na assinatura) vira `Situacao::Fria` na faixa acima da caixa, com "Compactar".
  O "Compactar" (o da faixa, o do anel, o do "…" e o do cartão de erro) manda o `/compact` direto
  pelo envio normal, sem tocar na caixa (o rascunho fica); com o turno rodando, ele entra na fila.
  Para compactar com foco, escreve-se "/compact <instrução>" na caixa, como a dica lembra
  (`compactar_conversa`, rodada de uso, item 6.3). Turno rodando, parada, falha, pasta sumida, fila de
  mensagens e o "Você parou" (item 6.2) vêm antes dela. O que o último turno mudou ("3 arquivos
  mudados · +40 −12", que ficava por último nessa faixa) é desde a 0.10.0 a pílula das mudanças, na
  linha de pílulas (ver **Linha de pílulas**): o clique abre a direita em "Este chat", na seção
  Mudanças (rodada de uso, item 5.1). (3) `Chat::contexto_grande` põe uma linha no seletor
  de modelo, na aba do Claude, dizendo quanto a troca faz reler. O medidor da conversa (contexto,
  custo, modelo, conta e Compactar) já existia, no anel da caixa.
- **Marco de compactação.** Enquanto o Claude Code resume a conversa (o `/compact`, ou sozinho com
  o contexto cheio), a linha viva do turno diz "Compactando…" no lugar do "Pensando…", na conversa
  e na linha de estado da caixa (`mensagens::rotulo_do_turno_calado`, `Motor::compactando`). Ao
  terminar, entra na conversa o marco "Contexto compactado (automático, 168 mil tokens antes)"
  (ou "manual", no `/compact`; `motor::marco_da_compactacao`), uma nota da conversa como o
  "Turno interrompido", que fica à vista também com o turno recolhido em "Trabalhou…", e o anel
  do contexto desce para o que a conversa ocupa depois do resumo. Medido no Claude Code 2.1.328
  em 23/11/2026 com um `/compact`: a saída traz `system/status` com `status: "compacting"`,
  depois `status: null` com `compact_result: "success"`, um `init` de novo e o
  `system/compact_boundary` com `compact_metadata` (`trigger` "manual" ou "auto", `pre_tokens`,
  `post_tokens`); o `claude::interpretar` os vira `Evento::Compactando` e `Evento::Compactou`
  (mais o `Evento::Contexto` com o `post_tokens`). O `status` da troca de modo, que só traz o
  `permissionMode`, não diz nada. O marco é um `Item::Aviso` que começa com
  `motor::CONTEXTO_COMPACTADO`; a conversa trazida do Claude Code segue com "A conversa foi
  resumida aqui", que vem do arquivo dela (`src/importar.rs`).
- **Conta do Claude e login perdido.** Com mais de uma conta, a escolha fica no seletor de modelo,
  numa linha no pé da aba do Claude, "Conta: tecnologia · semana 44%", que abre as contas no lugar
  (`pe_da_conta`, em `src/ui/composer_seletores.rs`; as chaves são `conta:<id>`). Quando um turno termina com erro de login ("OAuth token revoked",
  "OAuth session expired and could not be refreshed"; `motor::falha_de_login`), a conta entra em
  `Motor.sem_login`, sai da escolha (`conta_da_escolha` passa a pular até a conta escolhida à mão)
  e o turno é repetido em outra conta com login, com o aviso de qual (`Motor::login_perdido`). Sem
  outra, o aviso diz onde entrar de novo. A conta volta quando um turno nela dá certo ou quando a
  pessoa entra de novo (`adicionar_conta`).
- **A mesma pergunta para os dois.** **Perguntar também ao Codex** (ou ao Claude), no "…" da caixa,
  manda a mensagem para o chat e para um chat novo do outro agente, na mesma pasta, aberto num
  painel ao lado (`Janela::enviar_aos_dois`, `Motor::abrir_raia`). O chat novo nasce em Planejar,
  que só lê: dois agentes mexendo nos mesmos arquivos se atropelam. O título dele leva o nome de
  quem responde na frente ("Codex · …").
- **Mapa de agentes.** `src/ui/mapa_agentes.rs`: um diálogo largo com a conversa em cima (o
  modelo, o contexto e a conta de quantos trabalham, terminaram e falharam) e, embaixo, a lista de
  todos os subagentes que ela abriu (as chamadas Task e Agent, `agentes_do_chat`) à esquerda, com
  a sua rolagem, e o escolhido à direita (`painel_do_agente`): o estado (trabalhando, terminou,
  falhou, interrompido), o tempo, os tokens e as ferramentas usadas e, embaixo, a **conversa do
  agente como a de um chat normal** (`conversa_do_agente`): o pedido num balão, o que ele
  respondeu em Markdown, as chamadas seguidas num grupo recolhível ("Leu 1 arquivo · fez 1 busca",
  o clique abre as linhas), as imagens que as chamadas trouxeram em miniatura (o clique abre o
  visor) e o relatório, que é a última resposta. Os itens vêm da conversa do subagente que o motor
  lê do arquivo do Claude Code (`Motor::abrir_subconversa_no_mapa`, a mesma da aba, mantida em
  memória enquanto o diálogo está aberto e solta ao fechar); sem o arquivo, o cartão (os passos e
  o relatório). O **Abrir em aba** fecha o diálogo e abre essa conversa numa aba. A **ordem** da
  lista (`ordenar`): quem trabalha em cima, depois quem espera resposta sua (um pedido de
  permissão do subagente), quem falhou, e os terminados e interrompidos, do mais recente ao mais
  antigo em cada grupo; abre no primeiro. Era uma árvore com o agente aberto dentro dela, e com 10
  agentes só dois cabiam (dono, 23/11/2026). Abre por **Agentes desta conversa**, no "…" da
  caixa, e pela paleta (a pílula "N agentes" da linha de estado, que o abria, virou a pílula dos
  subagentes da linha de pílulas, que lista os do turno). Um agente sem resultado só conta como
  vivo com o turno rodando e sem aviso depois dele na conversa, **a não ser que o arquivo dele
  diga outra coisa** (`importar::subagente::sinal_das_linhas`, lido a cada 2 s com o diálogo
  aberto): terminando numa resposta só em texto, terminou; gravado há menos de 2 min, trabalha
  (o turno da conversa pode ter caído e voltado, com o `SendMessage`, e o subagente seguiu por
  fora: era o "Interrompido" de um agente que trabalhava); parado há mais, segue interrompido.
- **Linha de pílulas** (`src/ui/pilulas.rs`). Uma linha só, logo acima da caixa de cada painel,
  com o que acontece naquele chat. Só existe quando há pílula, flutua sobre o pé da conversa
  (`Janela::conversa` a põe por cima da área das mensagens, fora do fluxo) e não empurra a caixa;
  enquanto ela existe, a lista de mensagens guarda 32 px a mais no fim (`ALTURA_DA_LINHA`), para a
  última linha (a chamada que roda) não ficar atrás dela, e a pílula de voltar ao fim sobe o
  mesmo tanto. As pílulas, nesta ordem (`pilulas::montar`, pura e com teste): **os comandos no
  fundo** (o `run_in_background` do Bash e o comando que passou do tempo; "2 comandos no fundo",
  e o clique lista cada um com **Parar**, o `stop_task` dele, e **Parar todos**; antes o clique
  parava todos de uma vez; o comando que você para não fica marcado como falho, e o aviso de fim
  de um comando de fundo não põe mais a conta de subagente na linha dele, o "0 ferramentas ·
  falhou"); **os subagentes por estado**, cada estado com a marca dele (o anel de quem trabalha, o
  ponto de quem espera: "2 trabalhando", "1 esperando você", e "1 falhou" enquanto o turno roda;
  o clique lista os subagentes deste turno, com o que cada um faz, e o clique num deles abre a
  conversa dele numa aba; os agentes em segundo plano continuam contando depois da resposta do
  turno, e a lista diz a última ação de quem trabalha com as palavras da linha dele na conversa,
  `meta_na_lista`); **os passos do
  agente** ("3/5", o TodoWrite ou as ferramentas de tarefa; a lista inteira na dica e, no clique,
  com o que ele faz agora em destaque); e **as mudanças do turno** ("2 arquivos +40 −12", pelas chamadas de edição desde a
  sua última mensagem; o clique abre a lista à direita, em "Este chat"). As três primeiras só
  com o turno rodando; a das mudanças fica depois dele, até a próxima mensagem. No painel
  estreito ficam o ícone e o número. "Esperando você" é o subagente que fez um pedido de
  permissão ainda sem resposta: cada chamada de subagente chega com o id dela
  (`Evento::SubFerramenta.id`), o motor guarda de qual subagente ela é
  (`Motor.chamadas_dos_subs`) e o pedido de permissão traz o id da chamada que o fez
  (`Motor::subagentes_esperando`). De onde cada uma saiu: os comandos no fundo, o "3/7" dos
  passos e a pílula "N agentes" estavam na linha de estado da caixa (só com o turno rodando),
  e o "N arquivos mudados" numa faixa acima da caixa (que empurrava a conversa ao aparecer).
  Referência: as "tracks" do Paseo (`composer/tracks.tsx`), 23/11/2026.
- **Passos do agente** (`src/motor/passos.rs`). A lista de passos de cada chat (`Motor.passos`,
  só na memória) vem de duas famílias de ferramenta do Claude Code. O `TodoWrite` manda a lista
  inteira a cada chamada. O `TaskCreate`, o `TaskUpdate` e o `TaskList` mexem num passo por vez,
  pelo id, e só mudam a lista quando o resultado chega (o id do passo criado vem nele: "Task #4
  created successfully"; o "Task not found" não vem marcado como erro; o `TaskList` responde
  "#4 [in_progress] passo x" por linha, e refaz a lista inteira). Medido no Claude Code 2.1.328
  em 23/11/2026, com Haiku, Sonnet e Opus: no `-p`, sem nada no ambiente, ele não oferece
  nenhuma das duas famílias (a lista de ferramentas do `init` só tem `Task` e `TaskStop`); com
  `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` oferece `TaskCreate`, `TaskGet`, `TaskList` e `TaskUpdate`,
  e não o `TodoWrite`, que só aparece nas sessões de versões de antes (2.1.219 a 2.1.228, pela
  extensão do VS Code). O Vitral não põe essa variável: com o Claude Code de hoje, a pílula e a
  seção Passos só aparecem em quem a ligar por conta própria. A lista do Claude Code é da
  sessão e continua de um turno para o outro (ela mora em `tasks/<sessão>/<id>.json`, na pasta
  de configuração); a do Vitral se perde ao fechar, e volta no próximo `TaskList` (o passo de
  antes que o agente mexer sem ela aparece como "Concluiu o passo 4"). Um `TaskCreate` que
  devolve o id 1 é uma lista nova: a de antes sai. **Na trilha, cada chamada diz o que mudou**
  (`Passos::chamada`, guardado no resumo da chamada: "TodoWrite · Começou: Rodar os testes"):
  "Criou 5 passos", "Começou: …", "Concluiu: …", "Tirou: …", com vários juntos em número
  ("Concluiu 2 passos") e mais de uma mudança na mesma chamada separadas por ponto ("Concluiu:
  Ler o código · Começou: Corrigir"). No `TodoWrite` os passos são casados pelo texto com os da
  lista de antes. A chamada que não mudou nada disso, as de antes desta versão e o `TaskList`
  ficam com "Atualizou os passos" e "Conferiu os passos", sem detalhe. O histórico trazido do
  Claude Code passa pela mesma conta (`importar.rs`). No cabeçalho de um grupo de chamadas
  elas viram "atualizou os passos", em vez de contar como "usou N ferramentas".
- **Conversa do subagente em aba** (`src/motor/subconversa.rs`, `src/importar/subagente.rs`). O
  clique no cartão de um subagente (a linha da chamada Task ou Agent) ou nele na pílula dos
  subagentes abre numa aba do painel a conversa INTEIRA dele, só para ler: o pedido que ele
  recebeu, o raciocínio, cada chamada com o resultado e o que ele respondeu, com o desenho da
  conversa normal. O cartão continua na conversa de quem o abriu, como o marco de quando ele
  saiu, e o chevron dele (agora um botão à parte, como na edição) mostra os passos e o relatório
  ali mesmo. **De onde vem.** Do arquivo que o Claude Code grava para cada subagente:
  `projects/<pasta>/<sessão>/subagents/agent-<id>.jsonl`, com o `agent-<id>.meta.json` dizendo
  qual chamada o abriu (`toolUseId`). Medido na 2.1.328 com a CLI de verdade (Haiku, 23/11/2026):
  o stream-json entrega do subagente só as chamadas e os resultados delas (o texto e o raciocínio
  só com `--forward-subagent-text`) e mais nada depois que o processo sai; o arquivo tem tudo, é
  gravado aos poucos enquanto ele trabalha e continua lá depois. Por isso a conversa vem sempre
  do arquivo, ao vivo e depois de reabrir o Vitral, sem a opção nova do Claude Code:
  `importar::subagente::transcript` acha o arquivo pela chamada (primeiro na sessão do chat,
  depois nas outras sessões da mesma pasta de projeto, para o chat que continuou noutra sessão)
  e `importar::subagente::conversa` o lê com o leitor do histórico trazido (`itens_do_fio`, que
  agora serve aos dois fios). O pedido entra como mensagem entregue pelo chat que o abriu (dá
  para voltar a ele por ali). No modo automático (2.1.271 em diante) o subagente entrega o
  relatório pela chamada `SubagentHandback` e o resultado da chamada Task só diz "This agent's
  report was delivered…": na conversa dele a entrega vira a resposta, e o cartão fica com o
  relatório de verdade (`Evento::SubRelatorio`, `subconversa::relatorio_do_cartao`; antes o
  cartão mostrava o aviso em inglês). A conta de ferramentas do cartão também foi acertada: o
  `task_progress` chega antes da chamada e já a conta, e somar mais uma na chamada dava uma a
  mais (agora ela sai das chamadas vistas, pelo id). **Como mora no motor.** A conversa aberta é um `Chat` de
  mentira (`Subconversa`, id `sub:<chat>:<chamada>`) guardado fora do estado: não é gravado, não
  entra na lista e sai da memória quando a aba fecha (`Janela::soltar_subconversas`).
  `Motor::chat` acha os dois (é o que a interface desenha); dentro do motor tudo usa
  `chat_gravado`, que só acha os de verdade, então nada age sobre ela (enviar, renomear,
  arquivar e gravar não a alcançam). A execução dela é a do subagente (`Motor::execucao`: a
  chamada em curso gira, a aba fica azul). **Ao vivo.** A leitura vai em segundo plano e é
  refeita quando chega um evento daquele subagente (`subconversa_andou`) e, uma vez por segundo
  com algum chat trabalhando, quando o arquivo cresceu (`conferir_subconversas`: é o que traz o
  texto entre uma chamada e outra); quem redesenha são só os painéis que a mostram
  (`TextoChegando`). **Sem o arquivo** (o Claude Code apaga as sessões velhas; a bancada sem
  `CLAUDE_CONFIG_DIR`), a aba mostra o que o cartão guardou, dizendo que é só isso
  (`conversa_do_cartao`). No pé, no lugar da caixa, uma linha diz "Conversa do subagente, só
  leitura" e em que pé ele está (o que faz agora, ou quanto levou); se ele espera uma permissão sua, o pedido aparece ali também. A aba não é
  lembrada ao fechar o Vitral (o painel volta na conversa de quem o abriu), e o subagente de um
  subagente não abre: o Claude Code não grava a conversa dele à parte. Referência: o painel de
  subagente do Paseo (`provider-subagent-panel.tsx`, `sidechain-tracker.ts`), 23/11/2026.
- **Mensagem no meio do turno.** O Enviar com o turno rodando manda a mensagem para dentro dele
  (`Motor::dirigir_turno`). Quando o turno termina sem ter absorvido a mensagem, o Claude Code abre
  outro turno com ela no mesmo processo: nesse caso a entrada do processo fica aberta, para o
  turno seguinte também receber mensagens (o `Inicio` dele desfaz o fim adiado; se em 8 s ele não
  começa, `vigiar_turno_dirigido` fecha a entrada e o processo sai). A entrada só fecha no fim
  adiado quando há agentes em segundo plano. `Motor::turno_recebe_mensagem` diz se o Enviar entra
  no turno ou vai para a fila, e **Mandar agora**, na lista da fila
  (`Motor::mandar_da_fila_agora`), tira uma mensagem da fila e a põe no turno que roda. Cada
  mensagem dirigida vai com um `uuid` (`Processo::dirigir`): **o Parar a tira da fila interna do
  Claude Code** antes do `interrupt` (o pedido de controle `cancel_async_message`, que não está no
  tipo público do SDK; `Processo::retirar_dirigidas`), em vez de matar o processo, que era o
  jeito de ele não abrir outro turno com ela depois do Parar. Medido no Claude Code 2.1.328 em
  23/11/2026: o pedido responde `{"cancelled": true}`, o `interrupt` passa a responder
  `still_queued: []`, o turno fecha com o `result` de erro, o processo sai sozinho menos de 1 s
  depois de a entrada fechar e a sessão retomada não tem a mensagem (ela volta para a fila do
  Vitral, como antes). Se a resposta do `interrupt` ainda trouxer alguma mensagem
  (`Evento::FicouNaFila`), ou num Claude Code anterior ao 2.1.323 (`VERSAO_RETIRAR_DIRIGIDA`),
  ou no Codex, vale o de antes: SIGTERM no processo (`motor::como_parar`).
- **Responder a um pedido de permissão escrevendo.** Com um pedido aberto no chat, o Enter na
  caixa nega o pedido (todos os abertos daquele chat) e entrega o texto ao modelo como a
  explicação da recusa, pelo mesmo caminho do **Negar e explicar…** do cartão
  (`Motor::responder_pedido_escrevendo`, `motor::recusa_explicada`; no pedido de aprovar um
  plano, o texto é o pedido de mudanças; na pergunta com opções o texto não nega nada, é a
  resposta dela). A mensagem entra na conversa como sua, antes do
  resultado negado da chamada. Com imagem anexada, que a recusa não leva, os pedidos são negados
  com "O usuário respondeu com uma mensagem…" e a mensagem entra no turno pela entrada. O que
  vale para o turno inteiro (comando de barra, Ultrathink, Ultracode) continua indo para a fila,
  sem negar nada, e o **Ctrl+Enter** (Mandar depois) também. A dica do Enviar diz o que o Enter
  vai fazer (`composer::dica_do_enviar`).
- **Modo, modelo e esforço no turno que roda.** Trocar o modo (Shift+Tab ou o seletor), o modelo
  ou o esforço com o turno rodando vale nele, e não só no seguinte: o Vitral manda pela entrada
  do processo os pedidos de controle `set_permission_mode`, `set_model` e `apply_flag_settings`
  (com o `effortLevel`), como já mandava o `interrupt` e o `stop_task` (`Processo::acertar`, com
  o que o processo tem em `claude::AoVivo`). Medido no 2.1.328, no meio de um comando: o modo
  responde `{"mode": …}` e o Write seguinte do mesmo turno passou sem perguntar; o modelo
  responde `{"model": <id>}` e a chamada seguinte ao modelo já sai nele (a conversa passa a
  mostrá-lo em uso, `Evento::ModeloEmUso`); o esforço responde sucesso e o `get_settings` diz o
  nível novo (`null` volta ao automático). A troca só vai depois de 0,6 s parada
  (`Motor::acertar_turno_depois`): o Shift+Tab passa pelos modos do caminho, e nenhum deles (o
  "Sem perguntar" entre o Automático e o Manual, por exemplo) pode valer no turno nem por um
  instante. Para o "Sem perguntar" poder entrar no meio de um turno, todo turno abre com
  `--allow-dangerously-skip-permissions` (sem ela o pedido volta `bypass_not_launched`), menos
  com o Vitral rodando como root, em que o Claude Code nem abre com a opção. O turno com
  Ultrathink ou Ultracode fica com o esforço dele até o fim. Com a entrada já fechada, no Codex,
  ou se o Claude Code recusar o pedido, nada aparece na tela: a escolha fica gravada na conversa
  e vale no turno seguinte, como sempre valeu.
- **Sinal de vida e raciocínio.** Enquanto uma ferramenta roda, o Claude Code manda a cada 30 s
  um `tool_progress` com `heartbeat` (medido no 2.1.328 com um comando de 75 s): ele conta como
  sinal de vida (`Evento::Vivo`), e o "sem sinal há N min" da linha de estado (`Motor::silencio`,
  10 minutos calado) só aparece quando o processo parou mesmo de falar, não num build longo. O
  texto do raciocínio só vem na saída com `--thinking-display summarized` (fora do `--help`; sem
  ela o bloco chega vazio no 2.1.328): o Vitral passa a opção a partir do Claude Code 2.1.323
  (`VERSAO_RACIOCINIO_RESUMIDO`, a mais antiga em que foi conferida; num que não a conhecesse
  ela derrubaria o turno), e o "Pensou por…" da conversa abre com o resumo.
- **Esforço automático.** No Auto, o seletor diz o nível que vale: o Vitral pergunta ao Claude
  Code (`get_settings`, o `applied.effort`), uma vez por conta e modelo, em segundo plano
  (`claude::atualizar_esforco_padrao`); no Codex vem do catálogo (`default_reasoning_level`).
- **Servidores MCP.** O `/mcp` da caixa, **Lista dos servidores MCP** (Configurações, Claude Code) e
  a paleta abrem a lista dos
  servidores do chat do painel ativo (`src/ui/mcp_servidores.rs`): por origem (do seu Claude Code,
  da pasta, conectores da conta), um cartão cada, com o estado, o erro de quem caiu e, no clique,
  as ferramentas. Enquanto o diálogo está aberto há um Claude Code só dele (`claude::SessaoMcp`:
  sem mensagem e sem hooks, na pasta e na conta do chat), numa thread que atende a tela com os
  pedidos `mcp_status` e `mcp_reconnect` do protocolo de controle: **Atualizar** relê a lista e o
  botão de cada cartão liga o servidor de novo e mostra se ele voltou. O reconectar vale para essa
  sessão de consulta (cada turno de um chat abre o Claude Code de novo e liga os servidores
  sozinho); desligar um servidor e entrar num conector continuam no Claude Code. Uma conta extra
  guarda a lista de servidores de quando foi criada (`contas::preparar_pasta` copia o `mcpServers`
  do `~/.claude.json`): servidor acrescentado depois não aparece nela.
- **Custo de um redesenho.** A Janela é a raiz e é remontada em todo quadro; o que pesa fica em
  vistas com cache, que só se refazem quando elas mesmas são avisadas: as mensagens de cada painel
  e a lista (`src/ui/vista_mensagens.rs`) e o que anima sozinho, o metal e os anéis
  (`componentes/vista_viva.rs`). O laço do metal e do anel girando bate junto com o compasso dos
  spinners, 30 vezes por segundo (`movimento::laco_a`), e não numa grade própria. Estas coisas
  seguram o custo, medidas em 23/11/2026 no binário otimizado, na bancada, com três conversas de
  450 itens lado a lado a 150% (`VITRAL_QUADROS=1` e o tempo da thread principal em `/proc`; a
  máquina estava ocupada, então vale a diferença). Por quadro, de antes para depois: rolar um
  painel, de 35 a 58 ms para 28 a 30; rolar com o texto chegando, de 76 a 90 para 36 a 59; o
  texto chegando, de 41 a 55 para 36; cada tecla na caixa de mensagem, de uns 69 ms para uns 14;
  parado, 0% nos dois. Com um chat só rodando ferramenta (o anel e o metal batendo, nada
  chegando) nada mudou: uns 21 ms por quadro, que é a casca.
  - **Só o que está à vista é montado** (`src/ui/mensagens/a_vista.rs`). A conversa continua uma
    coluna só dentro de uma área que rola (a âncora de leitura, a mensagem presa no alto, a busca
    e o sumário contam em pixels), mas o bloco longe da vista entra como um espaço da altura que
    mediu da última vez, sem montar o markdown, o layout nem a pintura dele. A conversa é
    planejada primeiro (`Planejado`: o que cada bloco é e como montá-lo), depois
    `a_vista::blocos_a_montar` escolhe os que estão na vista e na sobra em volta dela (meia vista
    para cada lado, 300 px no mínimo), e só esses são montados. A altura guardada vale para o
    mesmo bloco mostrando a mesma coisa (`ChaveDoBloco`: o tipo, os itens e uma marca do conteúdo,
    que muda quando o resultado de uma chamada chega ou a mensagem é corrigida), na mesma largura
    e no mesmo zoom (`Alturas`); sem altura guardada o bloco é montado, e a conta é sempre para o
    lado seguro. Ficam sempre montados: tudo, com a busca da conversa aberta, com texto
    selecionado na janela ou no desenho que abre o chat; o cartão de erro e o texto que está
    chegando. O texto de mais de 3 KB tem regra própria (`texto_que_chega_depois`): o kit o lê em
    segundo plano, e o primeiro desenho sai sem ele; a altura dele só vale depois de ficar igual
    por 400 ms (`Alturas::assentada`) e, quando ele volta à vista depois de ter sido espaço, a
    caixa fica na altura guardada até o texto relido chegar (`Alturas::segurar`). Quando um bloco montado
    mede outra altura que a usada na conta (recolheu uma saída, o texto chegou), a conversa pede
    mais um desenho: os de depois dele saíram do lugar, e um espaço pode ter entrado na vista. O `list()` do GPUI não serviu: ele rola por item, e a âncora, a
    mensagem presa, o "abrir no começo da resposta" e a rolagem até o achado da busca contam em
    pixels a partir do alto da conversa. As páginas de 60 itens continuam (`ITENS_NA_TELA`):
    um bloco só vira espaço depois de medido uma vez.
  - **O texto chegando redesenha só o painel dele.** O motor junta os pedaços que chegam em 60 ms
    (`motor::JANELA_DO_TEXTO`) e, quando o lote só fez o texto crescer, emite `TextoChegando` em
    vez do `notify` (`motor::aviso_do_lote`): a Janela avisa só as mensagens dos painéis desse
    chat (`Janela::texto_chegando`), e a lista e os outros painéis saem do cache. O primeiro
    pedaço de uma resposta ainda redesenha tudo (a faixa do turno passa a "escrevendo"). O
    markdown do texto que cresce não é relido inteiro: o kit recebe só o que foi acrescentado
    (`TextViewState::set_text` vira `push_str` quando o texto novo começa pelo antigo).
  - **A casca é uma vista própria, sem cache** (`vista_mensagens::VistaDaCasca`): tudo o que a
    Janela desenha fora das vistas em cache é montado por ela (`Janela::montar_casca`). O GPUI
    avisa a vista em que o elemento está a cada hover, dica, quadro de transição
    (`movimento::pedir_quadro`) e estado guardado que muda (`use_keyed_state`); quando essa vista
    era a Janela, cada aviso desses passava pelo `Janela::avisar_vistas` e refazia as mensagens
    dos três painéis e a lista (de 13 a 25 ms por painel). Agora só o que o código do Vitral muda
    avisa a Janela (`cx.notify()` num `Context<Janela>`), e aí as vistas se refazem todas, porque
    o estado delas mora nela. Três avisos ficaram mais estreitos: a tecla na caixa de mensagem
    redesenha só a casca (`Janela::redesenhar_a_casca`: o texto da caixa não aparece nas
    mensagens nem na lista), o redesenho que o prepaint da conversa pede (a mensagem presa que
    troca a cada passo da roda) avisa só a vista do painel, e a rolagem só avisa a Janela pelo
    sumário quando o sumário está na tela (com a direita fechada, cada passo da roda no painel
    ativo refazia a janela inteira). **Quem escreve um listener que muda o estado da Janela
    precisa do `cx.notify()`**: antes o hover do clique refazia tudo de carona.
  - **O relógio da chamada que roda anda sozinho, uma vez por segundo.** O compasso dos spinners
    redesenha a casca e os anéis, não as mensagens, e o relógio de 1 s da Janela se cala enquanto
    o compasso bate: desde que as mensagens viraram vista em cache, o "00:42" da linha de uma
    chamada (e o do subagente) parava no valor do último desenho. Agora, com o compasso batendo,
    esse relógio avisa só as mensagens dos painéis cujo chat tem chamada sem resultado
    (`Janela::andar_os_relogios_das_chamadas`): um desenho por segundo do painel que trabalha, e
    a Janela, a lista e os outros painéis ficam no cache.
  - **A linha de pílulas é montada com a casca**, em todo quadro de cada painel: por isso
    `pilulas::montar` só conta (os subagentes sem resultado, as edições do turno), sem copiar
    nada da conversa. Tudo o que muda uma pílula chega por um evento do motor que não é texto,
    e esses avisam a Janela; a conversa de um subagente que cresce avisa só os painéis que a
    mostram (`TextoChegando`, emitido por `Motor::subconversa_lida`).
  - **O fade de quem está em cache mora dentro da vista.** O GPUI reaproveita a pintura de uma
    vista em cache enquanto ela não é avisada, e a chave do cache é a caixa, o recorte e o estilo
    do texto, não a opacidade: um fade posto pela casca em volta dela só anda se ela se
    redesenhar a cada quadro (no painel aberto ao lado, ela se redesenha sozinha nos primeiros
    quadros, medindo os blocos, e o fade do cartão funciona). Por isso a conversa que troca no
    painel entra pelo `movimento::entrada` de dentro de `Janela::corpo_do_painel` (`Painel.troca`
    e `troca_vista`): a vista se redesenha nos quadros do fade, e só ela. O que anima na casca
    redesenha a casca; o que anima nas mensagens, só a vista daquele painel. As entradas da rodada
    de 24/11/2026 (ver **O que entra com fade**) não mudaram o custo, medido com a mesma bancada
    (três conversas de 450 itens a 150%, binário otimizado, a thread principal em % de um núcleo,
    média de 3 rodadas antes e 2 depois, com a máquina ocupada por outras sessões e até 10 pontos
    de diferença entre rodadas iguais): parado, 0 e 0; pensando, 49 e 46; rodando ferramenta, 48
    e 48; escrevendo, 56 e 56; rolando parado, 33 e 33; rolando com texto chegando, 55 e 54.
    Trocar de aba, abrir um grupo e trocar Conversas e Arquivos fazem de 6 a 8 redesenhos no
    segundo da ação, antes e depois, com o mesmo custo por quadro: os quadros do fade caem nos que
    a ação já pedia (medir os blocos, ir à âncora).
  - **O que ainda custa** é a casca, refeita em todo quadro: com três painéis, uns 10 ms de
    layout e 7 de pintura (os cabeçalhos, as caixas de mensagem, as faixas), 30 vezes por segundo
    com um chat trabalhando. O que baixaria isso é o cabeçalho e a caixa de cada painel virarem
    vistas com cache, irmãs da das mensagens (aninhadas não servem: a vista que se refaz refaz as
    de dentro), com os spinners deles em vistas vivas. E um aviso da Janela (`cx.notify()`)
    ainda refaz as mensagens de todos os painéis: saber qual painel mudou pede uma assinatura do
    que cada um desenha.
- **Boas-vindas.** Na primeira abertura (sem pasta de trabalho e sem chat nenhum; `VITRAL_BOAS_VINDAS=1`
  força), `src/ui/boas_vindas.rs` mostra quatro passos: a apresentação, a pasta de trabalho (as
  pastas em que o Claude Code já tem conversas vêm como sugestão) e o básico (permissões, quem
  responde quando o Codex está instalado, modelo do Claude e trazer as conversas que já existem)
  e a conexão opcional à memória da equipe.
- **Memória da equipe.** No primeiro uso, no menu do logo e nas Configurações, cada pessoa informa
  o endereço de um servidor ai-memory e sua chave. **Testar e ligar** confere o servidor antes de
  guardar a conexão; **Consultar memória** busca entre os projetos acessíveis e abre o texto das
  páginas, sem chamar um modelo. Claude e Codex recebem o MCP em cada turno, inclusive nas sessões
  retomadas. O Claude recebe um arquivo privado por `--mcp-config`; o Codex, uma substituição de
  configuração só daquele processo e a chave pelo ambiente. A configuração pessoal dos agentes
  não muda. Sem conexão, o Vitral continua funcionando.
  A chave fica em `~/.config/vitral/memoria-equipe.chave` (0600 no Unix), fora do `config.json`;
  **Desligar** remove a chave e a cópia do MCP no cache. Decisão do dono, 13/11/2026: compartilhar
  o conhecimento da equipe pelo Vitral, com um servidor na VPS e uma chave por pessoa. Só os
  aprendizados registrados pelo agente são enviados: o Vitral não instala hooks de captura de
  prompts ou saídas de ferramenta e não importa o banco local. Não é preciso instalar o
  ai-memory no computador de cada pessoa. O prompt orienta os agentes a consultar antes de
  trabalhar e a usar o dono e o nome do repositório como workspace e projeto, para o caminho local
  de cada computador não dividir a memória da equipe.
- **Revisão da memória ao terminar** (Configurações, **Rever a memória ao terminar**). Quando um
  turno que mexeu em arquivo termina, uma vez por sessão, o Vitral pede ao agente para rever o que
  vale guardar na memória e nas instruções. O turno de trabalho conta como terminado na hora: o
  "Terminou", o sino, a lista e a notificação saem sem esperar a revisão, que roda por baixo, com
  "Revendo a memória…" discreto na linha de estado e sem notificar de novo (ela continua contando
  como trabalho para o fechar e o Reiniciar). Uma mensagem sua durante a revisão nunca entra nela:
  se a revisão ainda não usou ferramenta de escrita, ela é cortada e a mensagem abre um turno
  normal (a revisão volta a ser pedida no próximo fim); se já escreveu, a mensagem espera na fila.
  Na conversa, a revisão vira uma linha recolhida: "Memória revista · nada a guardar" ou "Memória:
  editou AGENTS.md", com o ícone que abre o arquivo; o clique mostra o que ela fez. O critério é
  a ferramenta usada, não o texto da resposta.
- **Conversas do Claude Code.** Na primeira abertura (junto da migração), o Vitral traz as conversas
  da extensão do VS Code e do terminal (`~/.claude/projects/<pasta>/<sessão>.jsonl`, só lidos: nada lá
  é mudado). Depois disso uma **vigia** (`src/motor/importacao.rs`) olha de novo a cada abertura e a
  cada 2 minutos e traz as conversas novas sozinha: só abre os arquivos mexidos desde a passada
  anterior, com 30 s de folga (`Filtro.desde`, em `src/importar.rs`), fica calada quando não há
  nada e, quando traz, avisa na janela. A marca da passada é a hora em que a leitura começou, nunca
  fica à frente do relógio e só anda até ali se todas as pastas abriram (com uma que falhou, fica no
  máximo um dia para trás). Quem já usava o Vitral antes de existir a importação não tem registro:
  para ele a vigia começa de agora, e o histórico inteiro só vem pelo **Importar do Claude Code…**.
  É o que deixa quem usa o VS Code passar para o Vitral sem importar à mão. Cada uma vira um chat
  direto na pasta dela, como lá (o Claude Code só retoma uma sessão a partir da pasta dela). O
  título é o que você deu (`/rename`), o resumo do Claude Code ou o começo da primeira mensagem; as
  datas e a ordem na lista são as da conversa. Ficam de fora as vazias (só um `/clear`), as de
  testes (pasta em `/tmp`), as dos chats do próprio Vitral, as de programas (`claude -p`) e as de
  pasta que não existe mais (um disco desmontado: entram quando ele estiver montado e você pedir de
  novo). O aviso diz o resultado ("40 conversas do Claude Code importadas · 3 abertas agora no
  Claude Code"). O histórico aparece no chat, lido do arquivo da sessão quando ele abre (em segundo
  plano; nada disso vai para os dados do Vitral): começa pelos 60 últimos itens e mostra os
  anteriores automaticamente ao subir, preservando a posição de leitura, e a
  linha "Trazida do Claude Code (VS Code) em 01/11" antes do que vem no Vitral. A primeira mensagem
  continua numa cópia da conversa (`--resume <id> --fork-session`): a original fica como está e pode
  seguir no VS Code; as seguintes retomam a cópia. A conversa que estava aberta no Claude Code quando
  foi trazida ganha o aviso disso. **Importar do Claude Code…** (menu do Vitral) olha tudo de novo,
  não só o que mudou, e sempre responde; nem ele nem a vigia repetem o que já veio (nem as conversas
  de chats apagados depois).
- **Um chat abre outro** (`abrir_chat`, do servidor MCP do Vitral): quando você pede ("abre um
  chat para isso"), o chat abre um chat novo na mesma pasta (o Sem pasta, um Sem pasta), à vista na
  lista, já com o pedido enviado; você continua a conversa por lá e nada volta sozinho para quem
  abriu. Só num turno em que só você falou: o texto de outro chat não abre chats.
- **Chats conversam entre si** (`listar_chats` e `mensagem_para_chat`); mensagens que chegam com o
  chat ocupado esperam na fila. Quem manda assina "chat <id> “título”", e a mensagem chega com
  `[de: …]` na frente; as do próprio Vitral, com `[Vitral]`. **Freio de ciclo:** depois de 6
  mensagens automáticas seguidas entre dois chats sem você no meio, a próxima fica guardada e os dois
  avisam "muitas mensagens seguidas entre X e Y; esperando você"; escrever para um dos dois zera. O
  servidor MCP (`src/mcp.rs`) só tem essas três ferramentas e a `permissao`, a do cartão de pedido
  de permissão (`--permission-prompt-tool mcp__vitral__permissao`, em todos os modos).
- **Enquanto você esteve fora.** Ao voltar à janela depois de 15 min fora (**Resumo ao voltar
  depois de**), ou ao abrir o Vitral, o topo da lista mostra o que terminou, o que precisa de você e
  o que falhou ou parou desde que você saiu, cada linha abrindo o chat (Ctrl+clique, ao lado), com
  o ×. Na lista estreita o título quebra em duas linhas, sem cortar a palavra. Fica só na memória:
  a próxima volta troca, e nada entra no histórico. Em cada linha, o título do chat vira um rótulo
  curto e a frase (a primeira da resposta, sem markdown) fica ao lado.
- **Toda lista diz o que o chat respondeu** (`src/ui/falas.rs`, rodada de uso, item 3.3). No sino,
  a resposta nova mostra a primeira frase da última resposta, sem markdown e cortada na frase, e
  embaixo o que o turno fez ("3 editados · 12 comandos"), contado na hora a partir dos itens. No
  histórico do "+", a segunda linha diz o estado e o começo da última fala ("Permissão · Bash ·
  npm test", "Terminou · Pronto, mandei as duas…"), com "há N min"; a pasta só aparece quando é
  outra que a do chat do painel. A dica da aba de fundo tem o título e, na segunda linha, o
  estado com quando e o começo da última fala. A notificação do sistema continua com a primeira
  linha.

## Primeira abertura, dados e linha de comando

- **Os dados das versões com projetos.** Os dados das versões com projetos (o `estado.json` com
  `"versao"` 2: projetos, pedidos, Mestre, ajudantes, cópias, cartões, rotinas) abrem sem perder
  conversa. Na carga (`dados::normalizar`), o Mestre e os ajudantes saem
  da lista (o arquivo das mensagens deles fica em `chats/`); o chat que trabalhava numa cópia
  isolada passa a mexer direto na pasta de onde ela saiu, com o aviso "Este chat trabalhava numa
  cópia isolada, que continua em <caminho>; agora ele mexe direto em <origem>." (a cópia não é
  apagada) e, se a sessão do Claude Code tinha nascido na cópia, começa uma sessão nova com um
  resumo da conversa; o projeto arquivado leva os chats dele para as arquivadas, e o modelo do
  projeto passa para os chats que seguiam o padrão; a conversa de um coordenador do Vitral antigo
  vira um chat como os outros. Antes da primeira gravação no formato novo, o `estado.json` como veio
  fica guardado em `estado.versao-2.json`, uma vez só. Nada nas pastas dos usuários é apagado:
  `projetos/<nome>` (as pastas próprias), `copias/`, `notas/` e `avulsos/` ficam como estão.
  `FORMATO_DADOS` e `VERSAO_ESTADO` são 3: uma versão de antes recusa abrir estes dados, e o
  atualizador não volta para ela.
- **A migração do Vitral antigo.** Na primeira abertura (quando ainda não há
  `~/.local/share/vitral/estado.json`), antes de a janela abrir, o Vitral traz o que havia nas
  pastas do Vitral antigo (as de `~/.config/vitral/recentes.json`, a mais recente primeiro, e a do
  `vitral <pasta>`): as conversas e as filas, com o mesmo `cwd` e a mesma sessão, cada uma virando
  um chat direto na pasta dela; os pedidos só servem para achar a pasta de origem de quem trabalhava
  numa cópia. A subpasta marcada como arquivada leva as conversas dela para as arquivadas. As notas
  ficam no `.vitral` antigo (o relatório diz onde). Um pedido com o número repetido entre duas pastas
  ganha outro, e o chat dele avisa ("Na importação, este chat passou de #3 para chat 12."). Nada nas
  pastas antigas é mudado, movido ou apagado (`<raiz>/.vitral/` fica intacto, inclusive as cópias
  antigas em `.vitral/worktrees`); o `estado.json` é o último arquivo gravado, então um PC que
  desliga no meio refaz tudo na abertura seguinte. O relatório aparece na janela ("Trouxe 2
  conversas do Vitral antigo de ~/Projetos.") e fica em `~/.local/share/vitral/migracao.json`. Ela
  roda uma vez só: depois, `vitral <pasta antiga>` abre a lista, e outra pasta com um `.vitral`
  antigo ganha o aviso de que esta versão só traz essas conversas na primeira abertura.
- **O Vitral antigo aberto.** Com um Vitral de antes desta versão aberto na primeira abertura, nada é
  gravado e a tela diz "O Vitral antigo está aberto (pid N). Feche-o para eu trazer as conversas de
  ~/Projetos.", com **Tentar de novo** e **Sair**.
- **Voltar para a 0.2.0** fica desligado depois de migrar: ela não lê as conversas do formato novo
  (as Configurações dizem "A 0.2.0-rc.2 não lê as conversas do formato novo"). O `.vitral/` de
  cada pasta continua lá, como a 0.2.0 o deixou.
- **Onde fica cada coisa.** Os dados em `~/.local/share/vitral`: `estado.json` (com `.anterior` e,
  vindo de uma versão de antes, `estado.versao-2.json`), `chats/<id>.json`, `avulsos/<chat>/` (as
  pastas dos chats Sem pasta), `exportadas/`, `anexos/`, `imagens/<chat>/` (as imagens que as
  ferramentas devolveram, ver **Ferramentas**), `migracao.json` e o `formato` dos dados; ao
  lado deles, em `app/`, o app instalado pelo atualizador (as versões e os links `atual` e
  `anterior`). As preferências em `~/.config/vitral` (`config.json`, `interface.json`, as contas,
  `atualizacoes.json`; o `recentes.json` antigo, que só a migração lê) e o prompt de cada turno em
  `~/.cache/vitral/prompts/`. Um arquivo estragado vai para `<nome>.ilegivel-<hora>.json` e o Vitral
  abre com o `.anterior`, avisando. As chaves que esta versão não usa mais no `interface.json`
  (`vista`, as das issues, `abas_por_chat`) são ignoradas; o `ultimo_destino` de antes (um id de
  projeto) também.
- **Um Vitral só.** A trava é `~/.local/share/vitral.trava` (fora da pasta de dados) e o socket,
  `$XDG_RUNTIME_DIR/vitral/vitral.sock`. Abrir de novo (pelo menu, `vitral <pasta>` ou `--nova`)
  entrega o pedido ao que está aberto (também enquanto ele migra) e não abre outra janela; com ele
  travado, a segunda abertura espera até 10 s e diz "O Vitral já está aberto, mas não respondeu".
- **Linha de comando.** `vitral` abre o app. `vitral <pasta>` (o `%f` do Nautilus) abre a lista se
  a pasta é uma das migradas, o chat mexido por último nessa pasta se há um, e senão um chat novo
  nela. `vitral --nova <pasta> [mensagem]` cria um chat novo na pasta (relativa à pasta de onde
  você chamou), já com a mensagem enviada; sem ela, o cursor na caixa; o que não é uma pasta só
  avisa.
- **Quando os dados não abrem.** Estado ilegível (e o `.anterior` também), dados de uma versão mais
  nova, ou a migração adiada ou que falhou: a tela de recuperação, sem gravar nada por cima, com
  **Abrir a pasta de dados**, **Tentar de novo**, **Sair** e **Começar do zero**, que pergunta antes,
  afasta os dados para `~/.local/share/vitral.descartado-<hora>` (menos o `app/`, que fica: o menu
  continua abrindo o Vitral) e abre como na primeira vez, trazendo de novo as pastas do Vitral antigo e
  as conversas do Claude Code.
- **Quando não grava.** Se o disco recusa (cheio, sem permissão), aparece a faixa vermelha "Não
  estou conseguindo gravar (…). Nada novo começa até voltar a gravar." e o Vitral tenta de novo
  sozinho; as mensagens enviadas nesse meio-tempo esperam na fila e entram quando voltar.
- **Transcrições do Claude Code.** O Claude Code apaga as conversas sem atividade há mais de 30 dias
  (`cleanupPeriodDays`), e um chat parado esse tempo perde a sessão: o Vitral continua numa nova, com
  um resumo, e avisa ("A sessão antiga do Claude Code expirou; continuei numa nova com um resumo.").
  Para guardar mais, ponha um valor alto em `~/.claude/settings.json`, como `"cleanupPeriodDays":
  3650`.

## Quando algo para no meio

- **Versão nova.** Instalado pelo release, o Vitral procura versão nova no GitHub, confere a
  assinatura, baixa e instala sozinho, e avisa uma vez: "Versão 0.3.0 pronta", com **Reiniciar** e
  o link das **Novidades** (o Markdown da versão e "Ver no GitHub"). Nunca reinicia sozinho; fechar
  o aviso não perde nada (fica o ponto azul no logo e o item no menu). O Reiniciar faz a mesma
  pergunta do fechar ("2 conversas estão trabalhando", **Reiniciar mesmo assim** / **Continuar
  aberto**, o Enter no Continuar), e o que foi cortado fica parado, com Retomar. Depois de
  reiniciar, uma vez: "Atualizado para a 0.3.0 · Ver novidades". No **Só avisar**, o aviso é "Há a
  0.3.0 no GitHub", com **Instalar**.
- **Versão nova que não abre.** Se ela cai duas vezes ao abrir na mesma sessão do PC, a terceira
  tentativa abre a anterior e avisa: "A 0.3.0 não abriu duas vezes; voltei para a 0.2.0" (fica até
  fechar). A 0.3.0 não volta a ser instalada sozinha. Uma queda de energia entre as tentativas não
  conta, e a volta só vai para uma versão que leia as conversas do disco.

- **Pedido parado.** Se o Vitral fecha, o PC cai ou você aperta Parar, o pedido do chat aparece
  como **parada** em âmbar, com ícone de pausa, sobe para "Esperando você" (menos quando foi você
  que parou) e ganha **Retomar** no cartão da lista, na direita e no fim da conversa. Um chat
  cortado no meio do turno também. O aviso ao reabrir sai uma vez só.
- **Chat que cai** (processo que morre ou não abre, erro da API): o turno termina com erro, a faixa
  acima da caixa diz em poucas palavras o que foi ("Limite de uso do Codex · volta às 12:30", "Sem
  login no Claude Code", "Sem conexão", "Serviço sobrecarregado", "Conversa longa demais para o
  modelo" ou, no resto, a primeira linha do erro), com o texto inteiro na dica e **Tentar de
  novo**, e com a janela em segundo plano vem a notificação "Parou com erro: <primeira linha>"
  (com urgência: fica na tela do GNOME até o clique). Duas causas ganham saída própria na faixa.
  No limite de uma conta escolhida à mão, se outra conta tem login e folga, a faixa diz "Limite da
  <conta> · volta 15:42" e oferece **Continuar na <outra>**: a conta do chat passa a ser a outra
  (o seletor de modelo mostra, e é por lá que se volta) e o mesmo turno roda de novo, sem o
  Ultracode. Sem outra conta com folga, fica "Limite do plano · volta às 15:42" com Tentar de
  novo. No login perdido, a faixa diz "Login da <conta> caiu" e oferece **Entrar de novo**, que
  abre as Contas do Claude já refazendo o login dessa conta. Nada tenta de novo sozinho. Na conversa, o erro vira o **cartão de erro**
  (`src/ui/mensagens/erros.rs` lê o texto; `CartaoDeErro` desenha): o ícone na cor do assunto
  (vermelho quando o turno não pôde seguir, azul-claro quando precisa de você, âmbar quando seguiu
  com ressalva, neutro na rede e no serviço sobrecarregado) num véu de 6%, o título em português,
  a frase do que fazer ("Volta às 12:30."), o Copiar no canto (leva o texto do agente como veio) e
  o texto do agente atrás de **Detalhes**, num poço escuro em mono, com as URLs encurtadas no
  domínio e clicáveis (aberto quando o Vitral não conhece o erro). No erro vivo (o último, sem
  mensagem sua depois e com o turno parado) entram até duas ações: **Ver o uso** e **Continuar no
  Claude** no limite do Codex; **Contas do Claude** e **Continuar no Codex** no limite do Claude;
  **Entrar de novo** (que abre as Contas já no login da conta que caiu) ou **Entrar no Codex** no
  login perdido; **Instalar o Codex**; **Compactar**
  na conversa longa demais. O Continuar troca quem responde e repete o turno; na dupla (o Claude e
  o Codex no mesmo chat), passa a vez ao outro. O **Tentar de novo** fica só na faixa. O limite do
  plano com outra conta livre continua trocando de conta sozinho, sem cartão. O login perdido sem
  outra conta e todas as contas no limite, que saíam em dois avisos seguidos (o do Vitral e o do
  agente), são um cartão só, também nas conversas já gravadas; o Copiar leva os dois textos. Quando o Claude Code manda o erro também como
  resposta, ela fica gravada mas não é desenhada nem contada pela busca: aparece só o cartão do erro
  (`resposta_repete_o_erro`, em `src/ui/mensagens.rs`).
- **Agentes em segundo plano.** Quando o Claude responde com um fluxo de agentes (ou um subagente)
  ainda rodando, o turno não acaba ali: o chat continua rodando, a conversa ganha o aviso "Os
  agentes seguem trabalhando em segundo plano; o turno termina quando eles acabarem." e o turno só
  termina, com a última resposta, depois que eles acabam (ou quando o processo do Claude Code sai).
  Nessa espera a entrada do processo fica aberta: a mensagem que você manda entra no Claude Code na
  hora (`Motor::dirigir_turno`, `agentes_no_fundo_com_entrada`), em vez de esperar na fila até os
  agentes acabarem, e a entrada fecha quando o último termina (`Evento::Fundo(0)`) sem mensagem
  pendente (dono, 23/11/2026: uma mensagem ficou 53 min na fila).
  Esse aviso não fecha o que veio antes dele (`Item::aviso_que_fecha`): os outros avisos marcam o
  turno que parou, e as chamadas sem resultado de antes deles contam como interrompidas; com este,
  o cartão dos agentes aparecia como "interrompida", a pílula zerava e a aba do subagente parava de
  acompanhar justo quando eles seguiam trabalhando. **Uma linha por agente**
  (`src/ui/mensagens/no_fundo.rs`): enquanto a resposta já veio e eles seguem, o fim da conversa
  tem uma linha para cada um, no lugar da frase "Agentes trabalhando em segundo plano…": o anel, a
  descrição, a última ação no gerúndio ("Rodando cargo test…", "Lendo views.py…"; "Começando…"
  antes da primeira; `acao_ao_vivo`, a mesma da lista da pílula e da linha viva do cartão) e o
  relógio desde o lançamento, em mono. O clique abre a conversa dele numa aba, e a dica diz
  também o modelo. As linhas ficam enquanto o Claude comenta o agente que acabou de terminar (o
  texto chega acima delas). **O fim de cada um** é um item da conversa (`Item::FimDoAgente`, posto
  pelo `fundo::anotar_fim` no `SubFim` da chamada que voltou com "Async agent launched"): a linha
  "Terminou: Mapear o carrinho · 25 min", ou "Falhou: …" com a palavra em vermelho, no ponto em
  que ele termina; o clique abre o relatório, que mora no cartão da chamada. No turno recolhido
  essas linhas ficam à vista, com a linha "Trabalhou…". Um Vitral de antes disto guarda o item
  como veio. Se o processo sai com eles ainda rodando, com o teto da espera
  vencido ou com um código de erro, o turno termina com erro e o relatório diz que os agentes não
  terminaram. Parar continua valendo e, nessa espera, derruba o processo com os agentes. Um turno
  que termina com erro ou parado enquanto há agentes em segundo plano também derruba o processo, em
  vez de deixá-los rodando fora da tela. O Claude Code espera os agentes **sem teto**
  (`CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0`) em todo turno: o padrão dele é cortar em 10 minutos
  ("Background tasks still running after 600s; terminating"), e um lote de uma hora morria no meio,
  com o turno marcado como falhado. Quem segura a espera é o dono, pelo Parar. Em todo turno, cada fluxo roda no máximo 3 agentes ao mesmo tempo
  (`CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS=3`; o padrão seria 6 neste PC, e a fonte não segura
  a carga); o limite vale por fluxo, então o turno com Ultracode pede ao Claude um fluxo por vez,
  sem outros agentes junto.
- **Sessão do Claude Code.** O id da sessão é gravado assim que ela nasce, e cada turno usa
  `--resume` se a conversa já existe. Se o Claude Code recusar ("already in use", "No
  conversation found"), o Vitral acerta a marca e repete o turno uma vez; no segundo caso abre
  uma sessão nova e avisa que a memória se perdeu.
- **Gravação.** As mensagens do turno em andamento vão para o disco a cada 5 s, numa thread à
  parte e com fsync, para sobreviver a queda de energia. O estado e a configuração vão pela mesma
  fila, fora da thread da janela.
- **Fechar com coisa rodando.** O × da janela, Alt+F4, o × do Panorama e o Sair do dash pedem
  confirmação quando há conversas trabalhando ("1 conversa está trabalhando", "3 conversas estão
  trabalhando"; "Fechar mesmo assim" / "Continuar aberto"); o Reiniciar da atualização também. O
  Enter fica com **Continuar aberto**: fechar pede o clique.
- **Processos.** Cada processo do claude roda num escopo do systemd do usuário (`vitral-<pid>-...scope`, em `vitral.slice`). Parar um deles derruba tudo o que ele
  abriu, até servidores e shells separados com `setsid`; fechar o app manda SIGTERM a tudo sem
  travar a tela; ao abrir, o Vitral para os escopos de uma instância que caiu. Sem systemd, vale
  um grupo de processos.
- **Confirmações.** Toda pergunta antes de agir tem o mesmo desenho: o ícone num círculo na cor
  do tom, o título, o que acontece e, numa caixa embaixo, o que some. Nas que apagam ou desfazem
  (**Desconectar** uma conta, **Começar do zero**), o Enter só fecha, sem agir. Arquivar um chat não
  pergunta: age na hora, e o aviso tem **Desfazer**.
- Com agentes rodando, a janela redesenha no máximo 12 a 20 vezes por segundo (e não a cada
  quadro do monitor de 165 Hz). Os efeitos só andam em resposta a uma ação (abrir, focar, clicar,
  o ponteiro entrar num botão redondo, chegar dado novo), duram de 120 a 240 ms e redesenham no
  máximo 60 vezes por segundo; parado, nada se mexe. Com as animações do GNOME desligadas, com
  **Animações: Desligadas** nas Configurações (ou `VITRAL_SEM_ANIMACAO=1`), tudo aparece direto no
  estado final e o spinner fica parado. Com a janela visível mas sem foco, os spinners, os anéis que
  giram, o metal e o brilho da linha de estado param (1 redesenho por segundo, o do relógio, em vez
  de 12); texto chegando
  numa conversa que nenhum painel mostra não redesenha a janela; e a barra de rolagem aparece e
  some sem fade (o fade pedia quadros no ritmo do monitor a cada mudança de tamanho, como o Ctrl+B).
- **O que entra com fade** (dono, 24/11/2026: "tem bastante coisa que parece meio seco"). Tudo em
  160 ms, uma vez, com 6 a 8 px de deslize ou só o fade, nunca de escala (o GPUI não tem escala em
  div): a conversa que troca no painel (aba, lista, histórico);
  cada pedido de permissão, pergunta e plano acima da caixa (de baixo); a fila aberta (de baixo);
  a faixa do turno quando aparece e quando troca de estado (Parada, Falhou, Fila, Você parou); cada
  pílula que aparece com a linha já à vista (de baixo); a linha de estado da caixa quando o turno
  começa (só o fade: ela abre o lugar dela e a caixa cresce); a chamada que chega numa trilha já à
  vista (de baixo) e o cabeçalho do grupo, que nasce com a segunda; o que um clique abre na
  conversa (um grupo da trilha, "Trabalhou…", "Pensou", a saída de uma chamada, o relatório de um
  agente, o plano respondido, os "Detalhes" de um erro, a revisão da memória), de cima, com o
  chevron girando 90° em 120 ms; a mensagem presa quando aparece (a troca de texto ao rolar
  segue a rolagem, sem fade); a parte das Configurações, deslizando do lado para onde se foi na
  coluna (como o passo das boas-vindas); Conversas ↔ Arquivos na lista; a direita encaixada ao
  abrir (Ctrl+J) e ao trocar entre o arquivo e Este chat; o "Comece um chat" do painel vazio; os
  avisos do canto; a faixa de quando a gravação falha; e a ilha do anexo. Sair é seco (o kit
  desmonta no quadro em que o estado muda), recolher também (altura automática não anima), e nada
  disso repete ao rolar, ao trocar de painel ou com outro painel abrindo ao lado. Ficam sem
  transição de cor, de propósito, o hover das linhas de lista, das abas e dos itens de menu:
  passar o ponteiro por uma fileira viraria animação contínua. A troca de tema continua sem
  transição.

## Opções da conversa (como na extensão do Claude Code)

A caixa de mensagem é uma moldura dupla (`caixa_mensagem`, em `src/ui/composer.rs`): por fora, uma
moldura preta de canto 20 (14 na Estreita); dentro dela, com 2 de folga (eram 4; dono, 16/11/2026:
"diminui as bordas"), o campo opaco, com o texto
e os chips; embaixo do campo, ainda na moldura, a **linha de estado**, em mono 11. Com o foco, um
aro de metal de 2 px acende em volta da moldura (180 ms; apaga em 120), sem azul nem halo; ele
recorta o miolo do campo (`Encaixe::Miolo`, com o grão leve), para a fita ser contínua nos quatro
lados, e não azul em cima e cinza nos lados (o mesmo na busca, na aba sob o ponteiro e na moldura
do pedido que espera você). Nos painéis de fundo, a caixa recua: o modelo, o esforço, o clipe, o
**…** e o modo (menos o "Sem perguntar", que é aviso) ficam a 0,55 (`opacidade::EM_REPOUSO`) e
acendem com o ponteiro na caixa e ao ativar o painel; o campo, a moldura, o anel com a
porcentagem e o parar ficam sempre acesos (relatório de aparência, 16/11/2026, V2). A barra
de baixo do campo tem chips com ícone, nesta ordem: **modo de permissão** (Manual, com o escudo
cheio, e Planejar em verde; Aceitar edições e Automático em violeta; Sem perguntar em âmbar),
**modelo** (o glifo do Claude, solto, sem o disco cheio, e o que vale, como "Claude
Opus 5.5"; nos chips a marca é sempre o glifo, `Marca::glifo`, e o disco fica nas listas, nos
popovers e no anel de uso), **esforço** (barrinhas em ciano, de "Auto" a "Máximo", na medida de um ícone: barras de 2 com
vão de 2, de 4 a 12 de altura) e **conta** (o ícone na cor do
uso de 5 h; só com mais de uma conta). O que segue o padrão da pasta fica em tom tênue. À direita
ficam o anel do contexto (com a fenda embaixo e a porcentagem ao lado, "24%", em mono; âmbar a
70%, vermelho a 90%; também na caixa estreita, mais justa, porque o anel sozinho parecia o anel
girando de "rodando"; o clique abre o uso da conversa: em cima o
contexto e o custo, "Contexto 31% · US$ 1,84", embaixo o modelo, a conta e os tokens, e no rodapé
**Compactar a conversa**, que prepara o `/compact`), o clipe de anexar (arquivo ou imagem, arquivo
da pasta, conversa anterior e comandos
de barra), o **…** e o enviar, uma tecla de
metal (`VarianteRedonda::Metal`, o par do "+" da lista), apagado com a caixa vazia, que acende
quando ela ganha texto e dá um pulso claro a cada envio (o pulso fica no botão também quando ele já
virou o parar). O **…** tem no máximo três grupos, com uma divisória entre eles (`grupos_do_mais`,
dono, 24/11/2026: eram 11 itens e 5 divisórias): na caixa estreita, **anexar** primeiro (**Arquivo
ou imagem…**, **Arquivo da pasta…** e **Conversa anterior como contexto…**, que saem do clipe);
com texto na caixa, **mandar** (**Mandar depois, na fila**, com o turno rodando e o Enter entrando
nele; **Mandar quando as 5 h renovarem**; **Mandar às…**; **Perguntar também ao Codex**, menos
para um comando de barra); e **a conversa** (**Comandos de barra**, **Compactar a conversa** numa
conversa do Claude que já tem sessão, **Agentes desta conversa** quando ela abriu algum, **Sessão
nova** com o chat parado, **Fixar**). O que não se aplica agora sai, em vez de ficar cinza. Na
caixa média, o **…** não aparece, como antes. Clicar em qualquer ponto vazio da caixa (o vão entre o texto e os chips, a margem,
o espaço entre os chips, a linha de estado) mantém o cursor nela. Com a conversa rodando, ele vira
o parar (o quadrado, com um anel girando em volta); com texto escrito, ficam os dois, e o enviar
entra na fila. A linha de estado, com o chat parado, mostra as teclas: à esquerda "/" comandos e,
com a caixa vazia, "↑" última; à direita "Enter" envia e, com
texto escrito, "Shift Enter" quebra a linha (na caixa estreita, só a direita). Com o chat
trabalhando, ela diz o que ele faz (ver **Faixa do turno**, abaixo). As mensagens, a fila, a faixa
do turno e a caixa vão até a mesma borda em todos os painéis: os anéis de uso flutuam no canto,
num disco opaco, e nada guarda folga para eles. Vazia, a caixa diz "Mensagem para este chat…" no chat
com pasta e "Mensagem…" no Sem pasta. **Shift+Tab** troca o modo, também no turno que está
rodando (ver **Modo, modelo e esforço no turno que roda**). Na caixa estreita, em que os
chips são só ícones, trocar o modo pelo Shift+Tab ou escolher um modelo mostra um aviso curto
("Modo: Manual", "Modelo: Claude Fable 5.2", `avisar_na_estreita`). O menu do modo lista o que foi permitido nesta conversa: o clique numa regra tira só ela, e
**Voltar a perguntar sobre tudo** tira todas. O **esforço** abre um popover com o nível num
segmentado (←/→, Home e End mudam na hora; o nível escolhido fica na ficha do escolhido, a mesma
dos menus, sem repetir no cabeçalho), com uma linha curta sobre cada nível (Auto: "Usa o
padrão do modelo."), e a chave **Ultrathink**, em arco-íris (Espaço liga; a descrição diz "Só na
próxima mensagem"): vale só para a próxima mensagem, sobe o esforço dela para Extra (o Máximo
continua Máximo) e deixa o chip em arco-íris até enviar. Um comando de barra não a consome. Embaixo
dela fica a chave **Ultracode**, em violeta com o ícone de fluxo (U liga): vale só para a próxima
mensagem, como o Ultrathink, e faz o Claude orquestrar vários agentes em paralelo por um fluxo (a
ferramenta Workflow do Claude Code). Embaixo do título, uma linha só: "Agentes em paralelo, só na
próxima · gasta bem mais"; o (i) ao lado do nome diz o resto, que gasta bem mais do plano e roda no
esforço Extra (o Máximo vira Extra nessa mensagem). O âmbar fica para o que atrapalha agora
(`avisos_do_ultracode`). Ligada, o chip do esforço vira "Ultracode" em
violeta com o ícone de fluxo (com o Ultrathink junto, as barrinhas ficam em arco-íris; na caixa
estreita, só o ícone de fluxo). Começa sempre desligada, também ao abrir o Vitral. Fica
desabilitada, com o motivo curto no lugar da descrição (só o título e a chave apagam; o motivo
fica legível), com um Claude Code anterior à 2.1.278 (a versão em que o Vitral conferiu o Ultracode; o limite de
agentes só existe desde a 2.1.269), com um modelo sem o esforço Extra (o Haiku, os 4.x até o 4.6) e
no chat em que quem responde é o Codex ("Só no Claude Code", também com os dois juntos).
Numa conta em que um turno com Ultracode mostrou os fluxos de agentes desligados (em Dynamic
workflows no `/config`, por `disableWorkflows` ou pela organização), a linha continua ligável e
avisa em âmbar; esse turno ganha um aviso em vermelho, segue em Extra e a mensagem perde o selo.
Fora de Sem perguntar e Automático, a linha também avisa ("Neste modo, os agentes podem ter ações
recusadas": eles não conseguem pedir permissão). Um Ultracode por vez no Vitral inteiro: com
outro rodando, a linha avisa ("Roda em outra conversa: esta iria sem ele") e a mensagem vai sem o
Ultracode, com o motivo na conversa. Cada aviso desses é uma linha curta em âmbar; o porquê vai na
dica do (i). Quando a conta chega ao limite e o turno
continua na próxima, a repetição vai sem ele; só o **Tentar de novo** o repete. Fora do Ultracode, a ferramenta
Workflow fica bloqueada: nem um chat nem um "Continue de onde parou" relançam um fluxo sozinhos. O
**modelo** abre um seletor com busca: no alto, os seus perfis de agente, se houver (grupo **Perfis**;
ver **Perfis de agente**, em Organização); depois, Padrão (o das Configurações) e as famílias do Claude,
com a versão num selo à direita (a que está rodando, quando a conversa segue a família, ou a que o
apelido pega hoje no Claude Code instalado); o logo do Claude fica só no rótulo do grupo e
o check do escolhido à direita. Seguindo o Padrão, é ele que diz qual está em uso ("Padrão · Opus
5.5"); só uma família escolhida na conversa ganha o selo "em uso". Uma conversa antiga que
guardou o nome completo do modelo ganha a linha dele. Quando a família não é a que está em uso, a
linha dela diz a versão que ela é hoje ("Para os desafios mais difíceis · hoje, Fable 5.1"). Embaixo
das famílias, a linha **Outras versões ▸** abre no lugar as **versões exatas** (Fable 5.2, Fable 5.1,
Opus 5.5 e as outras), com a que está rodando marcada, inclusive quando a conversa segue a família.
No pé, com mais de uma conta, a linha da conta ("Conta: tecnologia · semana 44%") abre a escolha
dela no lugar. As duas começam recolhidas a cada abertura (só uma versão exata escolhida abre as
versões), para o seletor caber sem rolagem a 150%: o que está aberto é `Abertos`, guardado na
Janela por painel, e o que fica à vista sai de `grupos_a_vista`, que também faz a busca (o
`filtro_proprio` da lista): com algo escrito, as versões e as contas que casam aparecem direto e as
linhas de abrir saem. Numa conversa do Claude acima de 100 mil tokens (`Chat::contexto_grande`),
uma linha curta embaixo da lista diz "Trocar agora relê 751 mil tokens" (`aviso_de_troca`). A lista vem do Claude Code instalado:
ao abrir o app e quando a versão dele muda, o Vitral faz o pedido `initialize` do protocolo do
Agent SDK (o mesmo da extensão do VS Code ao abrir), sem mandar mensagem e sem gastar uso, numa
pasta vazia e só do dono dentro do cache do Vitral (`perguntas`), só com as configurações do
usuário, sem servidor MCP nenhum e com os hooks desligados (`claude::atualizar_modelos_oferecidos`,
com prazo de 12 s; no fim o processo e o que ele abriu são encerrados), e o seletor mostra cada
modelo que ele listou e o que cada apelido resolve, uma vez
só, por família e da mais nova para a mais antiga (`versoes_de_modelo`, em `src/ui/composer.rs`).
Modelo novo aparece quando o Claude Code se atualiza, sem versão nova do Vitral. Sem resposta
(nenhum Claude Code, ou o `claude` de mentira da bancada), vale a lista fixa `VERSOES_DE_MODELO`,
que é só a reserva. Uma versão que a conversa tinha escolhido e que o Claude Code deixou de
oferecer continua à vista, como "escolhido antes". Num Claude Code anterior ao 2.1.280 (`VERSAO_MODELOS_EXATOS`), as versões
exatas não aparecem e uma linha diz por quê. Fora da caixa estreita, os popovers de esforço
e de modelo terminam num rodapé com as teclas que aceitam, no desenho de tecla da paleta: **← →**
nível, **Espaço** Ultrathink, **U** Ultracode, **Enter** fecha; no de modelo, **↑ ↓** escolher,
**Enter** usa. Com a caixa média, as setas e o "Claude " do modelo saem, e a conta fica só com o
ícone. Com a caixa estreita, os chips ficam só com o ícone (o nome vai na dica), e o anexar e a
conta vão para o **…**. Nenhum texto é cortado no meio: o que não cabe some inteiro, a conta
primeiro. Anexar outra conversa como contexto e mencionar arquivo da pasta pelo clipe
(**Arquivo da pasta…**). Digitar `@` no começo da mensagem ou depois de um espaço abre o **menu
"@"** na própria caixa, o mesmo desenho do `/`, filtrando enquanto se digita (até 8 linhas): com
a dupla ligada e no começo da mensagem, **@claude** e **@codex** primeiro (o "@" de um e-mail,
"victor@", não abre nada); depois os arquivos e as pastas do chat, com o arquivo aberto à direita
no topo. Tab ou Enter completam "@caminho ", e Esc fecha sem apagar. O arquivo (ou a pasta)
completado vira uma **pílula** dentro da caixa (o `InlineToken` do campo do kit, em
`composer::ficha_da_mencao`): o ícone do tipo, o nome curto e as linhas, se houver
("fila.rs:120-134"); o clique abre o arquivo à direita e o Backspace apaga a menção inteira. O
texto que vai ao agente continua o caminho inteiro, com as linhas no formato das menções do Claude
Code ("@src/ui/fila.rs#L120-134"). O rascunho guardado volta com as pílulas; o texto que volta à
caixa por Corrigir ou pela fila volta como texto, e vira pílula no balão do mesmo jeito. A pasta é lida em segundo
plano e de novo a cada "@" novo; enquanto lê, a linha "Lendo a pasta…". A lista de arquivos da
pasta (a do "@", a da aba Arquivos e a do Mencionar) desce nos repositórios de dentro: cada
subpasta com git próprio que o de cima ignora ou não conhece entra com o prefixo dela
(`central/src/…`), até 3 níveis e dentro do teto de 20 mil arquivos. Só conta o `.git` que é
pasta (o worktree, com `.git` em arquivo, duplicaria tudo). Em ~/eita, cuja raiz é lista-branca,
é isso que faz o "@" achar o código dos projetos.
Colar (Ctrl+V) imagem ou arquivos copiados, ou arrastar arquivos do gerenciador para a conversa,
vira anexo. `/` abre os comandos de barra (os do seu Claude Code, com `/clear` para sessão nova e
`/compact`), num menu acima da caixa: ↑/↓ andam pelo menu (o campo fica no contexto "Menu" só
enquanto ele está à vista), Tab ou Enter completam o item marcado (o Enter com o nome exato de um
comando envia), e Esc fecha o menu sem apagar o que foi escrito; a próxima letra o abre de novo. Passe o mouse numa mensagem para copiar (e, na sua, corrigir e mandar de novo): as ações aparecem numa barra flutuante no canto de cima do seu balão, à direita, a 16 da borda
(12 no painel mínimo: no trecho reto, na coluna do texto, e não na curva do canto), e no pé da resposta, à direita (em cima ela cobria a
primeira linha); nada fica
reservado nem à vista sem o ponteiro, e nada se mexe quando ela aparece. O hover da mensagem
alcança a barra inteira, que fica metade para fora dela (`com_alcance_da_barra`): chegando por
cima, o lápis não some antes do clique. Os blocos de código ficam
num preto quase puro, como as saídas de comando, e têm o próprio Copiar, numa coluna à direita que
nunca cobre o código; a linguagem fica na dica dele ("Copiar código python"). Cada citação em
bloco da resposta ("> ...", em geral a mensagem pronta para mandar a outra pessoa; na cor do
texto, com o fio e o recuo marcando a citação) tem o Copiar
dela no canto de cima à direita, que leva o texto sem os ">" e sem as marcas de markdown (negrito,
itálico, código, título; o link vira "texto (endereço)"), com as linhas e os parágrafos. O kit não
tem gancho para as ações da citação: a resposta com citação é partida em trechos
(`trechos_com_citacoes`, em `src/ui/mensagens.rs`) e cada citação é desenhada à parte, com o
mesmo estilo; a busca conta as ocorrências trecho a trecho (`Realce::trecho`), e a seleção vale
dentro de cada trecho. Modo foco (vale para todos os chats: `Config::modo_foco`, no menu do painel e
na paleta, sem botão no cabeçalho; dono, 18/11/2026): como a extensão do Claude Code no VS Code,
cada turno vira uma linha só com o que o agente fez (`turnos_do_foco`): pronto, "Trabalhou 16 s ·
1 comando", com a resposta final embaixo; rodando, ao vivo, "Buscando cupom · 2 chamadas", com o
anel girando. O clique abre o turno inteiro, e a busca (Ctrl+F) acha e abre o que está lá dentro
(dono, 23/11/2026).

- **Ler a conversa.** A **hora** de cada mensagem aparece no hover, na mesma barra flutuante do
  Copiar (`hora_na_barra`; a barra é uma tecla em relevo, opaca, com fio claro e a luz em cima, sem
  o anel azul dos flutuantes, em `barra_de_acoes`), e um **separador de dia** ("Hoje", "Ontem",
  "22 de março") separa os turnos (`separador_de_dia`): numa conversa de dias dá para saber quando
  cada coisa aconteceu. As
  conversas de antes da 0.4.0 não guardaram a hora e não mostram nada. O **Copiar** vale em
  qualquer mensagem, não só na última resposta de cada turno. **Ctrl+F** busca dentro da conversa
  aberta (`src/ui/busca_conversa.rs`): a barra flutua no alto do painel, com o contador ("3 de
  12"), **F3** e **Shift+F3** para andar e realce em âmbar; a busca não liga para maiúscula nem
  para acento ("codigo" acha "código"), conta também o que está dentro de um bloco fechado (a saída
  de uma ferramenta, também a de um comando, que então aparece inteira e rola até o achado), que
  abre ao chegar a vez dele (também na trilha com raciocínio no meio: a
  busca agrupa as chamadas com as mesmas regras da conversa), e **Esc** fecha. O achado para a 48 px
  do alto e a 88 do pé, longe da pílula de voltar ao fim. Fora do painel largo o contador é curto
  ("2/7", com a frase na dica) e o campo não desce de 128. O
  índice só é refeito quando o termo ou o tamanho da conversa muda, e nada disso anima.
- **Escrever.** Com a caixa vazia, a **seta para cima** traz de volta a sua última mensagem para
  ajustar e mandar de novo, e a conversa volta a antes dela (`EditarUltima`, só no contexto
  "Vazia"; com texto escrito a seta continua movendo o cursor). O **rascunho** de cada conversa (o
  que você escreveu e não enviou) já ficava na memória da janela ao trocar de painel; agora também
  vai para o `interface.json` 1,2 s depois de você parar de digitar, e volta quando o Vitral abrir
  de novo, inclusive depois de uma queda (`src/ui/rascunhos.rs`). O rascunho é inteiro: o texto,
  os anexos (os prints colados) e a citação; um rascunho só com um anexo também vai, e o formato
  antigo (só o texto) continua abrindo. O Ultrathink e o Ultracode continuam começando desligados.
  Na lista e nas abas de fundo, o chat com rascunho ganha o lápis pequeno e, no cartão da lista, a
  segunda linha "Rascunho: começo do texto" (ou "Rascunho: 2 anexos") no lugar da hora; na aba, o
  começo vai na dica do lápis. O painel em que o chat está aberto não leva a marca (rodada de uso,
  item 1.4). O texto que o Vitral põe na caixa
  (a correção, um comando completado, a menção de um arquivo) entra por `composer::por_texto`, que
  guarda o desfazer (o Ctrl+Z traz de volta o que estava escrito), rola a caixa até o cursor, no
  fim, e conta como digitação para o rascunho; a mensagem corrigida, que já saiu da conversa, vai
  para o disco na hora. Colar um texto longo também rola a caixa até o cursor, no quadro seguinte
  ao da colagem, quando ela já cresceu. Ao enviar, a caixa vazia também vai para o disco na hora, senão uma queda
  logo depois trazia de volta, como rascunho, a mensagem já enviada. Corrigir com algo escrito na caixa sempre pergunta antes, e a pergunta diz
  que o texto da caixa sai dela. A **fila** do que você mandou com
  o turno rodando tem lista própria (`src/ui/fila.rs`), aberta pela pílula da fila (na linha de
  estado da caixa, com o chat trabalhando, ou na faixa acima dela), dentro do painel e acima da
  caixa de mensagem: cada
  mensagem esperando numa linha, com o × que devolve a mensagem inteira à caixa (o texto, os
  anexos e o Ultrathink ou Ultracode dela; com algo escrito na caixa, o texto se junta ao fim,
  depois de uma linha em branco, os anexos se somam às pílulas e o Ctrl+Z desfaz a junção), o lápis
  que edita ali mesmo, na linha (Ctrl+Enter salva, Esc desiste; se a fila andou enquanto isso,
  nada é trocado) e, com o chat parado, **Entregar agora** no rodapé (a faixa acima da caixa não
  repete o botão com a fila aberta). Com o turno rodando, a caixa vazia e algo seu na fila, a seta
  para cima traz de volta a última da fila, com anexos e reforço, sem tocar no turno; o Ctrl+Enter
  a devolve à fila. Sem fila, a seta avisa como antes (rodada de uso, item 1.3).
  **Mensagem com hora marcada** (`src/motor/agendadas.rs` e `src/ui/agendar.rs`): no **…** da
  caixa, com texto nela, **Mandar quando as 5 h renovarem** (a renovação da conta em que o chat roda, ou a
  do Codex, mais 30 s de folga; o item some sem essa hora lida) e **Mandar às…** (um diálogo com
  a hora: "15:10", "15h", "830"; a que já passou hoje vale para amanhã) mandam o que está na caixa
  para a fila do chat com a hora (`Entrada::agendada`, gravada no estado: sobrevive ao reinício).
  A fila abre com o cartão dela, que leva o selo "sai às 15:10"; o ×, o lápis, a ordem e o
  **Mandar agora** valem como nas outras, e o **Entregar agora** do chat parado manda tudo, com
  hora ou sem. Até a hora, ela não entra em turno nenhum, mesmo com o chat livre: quem tira
  mensagens da fila para um turno passa por `Motor::tirar_prontas`. Na hora, a rotina de 15 s do
  motor (`entregar_agendadas`) abre o turno do chat livre, junto com o que mais esperava na fila;
  com o chat ocupado, a mensagem entra quando o turno acabar. Só sai com o Vitral aberto: a hora
  que passou com ele fechado sai logo na abertura. A
  lista mostra umas três mensagens antes de rolar e guarda à direita o lugar da barra de rolagem,
  que segurava o clique do ×; o que a fila faz fica na dica do título. Fora do painel largo, o
  **Mandar agora** de cada linha é só a seta, para a fileira de botões caber. **?**, com o foco fora de um campo de texto, abre os **Atalhos do
  teclado** (`src/ui/atalhos.rs`): a tela é montada da mesma tabela que registra as teclas, então
  nunca desencontra do que o teclado faz.
- **Paleta de comandos (Ctrl+Shift+P)** (`src/ui/paleta.rs`; também no menu do logo). Uma lista
  só, com busca sem acento e por palavras, de tudo o que o Vitral faz. Tem duas fontes: as linhas
  com rótulo da tabela de `src/ui/atalhos.rs` (com as teclas à direita; o Enter dispara a mesma
  ação da tecla, a partir da caixa do painel ativo, pelo `atalhos::acoes()` que a tabela passou a
  gerar) e os comandos que só existiam em menu (`paleta::Direto`: do menu do logo, do "…" da
  caixa e do menu da conversa, como Compactar, Agentes desta conversa, Perguntar também ao Codex,
  Sessão nova, Renomear, Fixar, Arquivar, Modo foco, Servidores MCP, Contas, Consultar memória),
  que chamam o método do item do menu. As seções são as do diálogo de atalhos. O que não vale
  agora fica apagado, no fim da seção, com o motivo embaixo ("A conversa ainda não tem sessão",
  "Só com o menu “/” da caixa à vista"): a lista é montada de uma foto do momento
  (`paleta::Momento`). O Esc devolve o foco à caixa. Um teste garante que toda linha com rótulo
  da tabela de atalhos está na paleta; atalho novo entra sozinho, e comando novo de menu entra em
  `Direto::TODOS`. **Com algo escrito**, a paleta acha também as conversas pelo título (todas menos a do
  painel ativo, com a pasta embaixo; o Enter a abre no painel ativo) e os arquivos da pasta do
  chat ativo pelo nome (o Enter abre à direita), em duas seções depois dos comandos, **Ir para a
  conversa** e **Abrir arquivo**, com até 6 linhas cada (`grupos_com_achados`); sem nada
  escrito, só os comandos, como antes.
- **Abrir arquivo pelo nome (Ctrl+P)** (`paleta::abrir_arquivo_pelo_nome`; também na paleta). A
  lista dos arquivos da pasta do chat do painel ativo, na mesma lista de escolha do "@"
  (`seletor_arquivos::grupos_de_arquivos`: o nome em cima, a pasta embaixo, o nome batendo antes
  da pasta e o mais curto antes), e o Enter abre o arquivo no painel da direita. A lista é a que
  a aba Arquivos e o "@" já leram (`cache_arquivos`), então abre na hora; na primeira vez da
  pasta, é lida em segundo plano ("Lendo os arquivos…"). A busca de arquivo, aqui, no "@" e no
  Mencionar, não liga para acento nem para caixa ("relatorio" acha "relatório.md"), com a peça de
  `acentos.rs`. Sem conversa no painel ativo, um aviso pede uma.
- **Mensagens já enviadas (Ctrl+R).** A lista das suas mensagens de todos os chats (os
  `Item::Usuario` que o motor tem na memória; o histórico de uma conversa trazida do Claude Code
  fica de fora), na paleta de escolha, com a busca sem acento e por palavras de `acentos.rs`.
  Primeiro as do chat aberto, depois as dos outros chats da mesma pasta, depois o resto; em cada
  grupo, da mais recente à mais antiga, e o mesmo texto aparece uma vez só. Cada linha tem o
  começo da mensagem e, embaixo, o chat, a pasta e há quanto tempo. O **Enter** põe o texto na
  caixa do painel ativo, sem enviar: com algo escrito, junta ao fim depois de uma linha em branco
  (o Ctrl+Z desfaz, como no × da fila). Mostra 60 por vez; a busca estreita o resto. Tudo em
  `src/ui/enviadas.rs`.
- **Faixa do turno.** O turno em andamento mora na linha de estado da caixa, dentro da moldura
  (`linha_de_estado`, em `src/ui/mensagens.rs`): à esquerda, o anel girando, o que o chat está
  fazendo ("Lendo motor.rs…", "Rodando: cargo test…", com o brilho que corre), o relógio em mm:ss,
  "ação N" e, quando há, o "dirigido" e a pílula da fila; à direita, a tecla **Esc** e "para parar"
  (os comandos no fundo, os passos e os agentes saíram dela para a **Linha de pílulas**).
  Com a resposta já dada e os agentes ainda rodando, ela diz o agente e o que ele faz ("Mapear o
  carrinho: Rodando cargo test…") ou, com mais de um, "3 agentes trabalhando em segundo plano…"
  (a linha da caixa só diz o que o chat faz com a conversa rolada para cima; no fim, quem diz são
  as linhas dos agentes). **Memória do chat**: no Linux, a dica do relógio diz quanto o turno ocupa
  agora, "Memória deste chat: 3,1 GB" (o agente e tudo o que ele abriu: comandos, testes,
  servidores). O número é o `memory.current` do cgroup do escopo do systemd em que o chat roda
  (`Escopo::memoria`, em `src/processos/unix.rs`, achado pelo `/proc/<pid>/cgroup` do processo e
  só aceito se o cgroup é o do escopo), lido quando a linha pergunta e guardado por 3 s
  (`src/motor/memoria.rs`); chat parado não é lido. No Windows, sem systemd ou sem o controlador
  de memória no cgroup do usuário, a dica não aparece. Não há aviso de máquina no limite (o dono
  não quis aviso novo). **Esc que não custa caro** (rodada de uso, item 6.2): quando parar derrubaria o
  processo inteiro (agentes em segundo plano, a espera por eles, um turno com Ultracode), o
  primeiro Esc só arma, e a linha diz "Esc de novo para parar os N agentes" por 3 s; o botão Parar
  continua parando direto. Depois de um Esc (ou do Parar) que parou o turno, a faixa acima da caixa
  mostra "Você parou" com **Continuar**, e o fundo da caixa diz "O que fazer em vez disso? Enter
  continua": o Continuar, ou o Enter com a caixa vazia, manda a mesma frase do Retomar. Isso não
  entra no sino e some no primeiro envio, ao trocar de chat ou depois de 10 minutos. Nas larguras menores saem o
  "ação N", o rótulo do Esc e, na mínima, o relógio. Acima da caixa (`faixa_atividade`), alinhada
  com o texto dela e, à direita, com o enviar, que entra com um fade e 8 px de baixo, só fica o que
  pede um botão seu: **Parada** com **Retomar** (âmbar quando foi o Vitral que parou; o motivo no
  formato da lista e do sino, "Parada · Vitral fechou", numa linha só, com reticências quando
  aperta, e o motivo inteiro na dica; a ação à direita nunca é cortada), o que falhou (**Limite
  do plano · volta 15:00**, **Sem conexão**, **O último turno falhou**) com **Tentar de novo**, mensagens esperando com **Entregar agora** e a pasta do
  chat que sumiu do disco. Esperando você, ela some (o pedido está logo abaixo).
- **Planejar.** O plano aparece num cartão com a faixa âmbar em cima e o modo em que a conversa
  fica depois de aprovar ("Depois de aprovar: Aceitar edições"). **Aprovar e executar**
  sai do Planejar e volta ao modo de antes (sem modo anterior, o padrão da pasta; se também for
  Planejar, Aceitar edições); a seta ao lado aprova em outro modo (**Aprovar e aceitar edições** ou
  **Aprovar e perguntar antes**). O campo embaixo leva o que mudar: Enter (ou
  **Enviar mudanças**) devolve o plano com o comentário. Respondido, o plano **continua na
  conversa**, no ponto em que foi proposto: um cartão recolhido com o que você respondeu
  ("Plano aprovado", com o visto verde, ou "Plano recusado") e o título do plano, que abre o
  texto inteiro em markdown, com o Copiar, e, no recusado, o "Você pediu: …" com o comentário
  (`Janela::plano_respondido`, `mensagens::plano_na_conversa`). O cartão fica à vista com o
  turno recolhido em "Trabalhou…", parte a trilha das chamadas e o Ctrl+F acha no texto dele; se
  o turno parou antes da resposta, diz "Plano proposto". Enquanto o pedido espera, quem aparece é
  só o pedido. Medido no Claude Code 2.1.328 (23/11/2026, Haiku em modo `plan`): a chamada
  `ExitPlanMode` vem **vazia** na saída; o plano só vem no pedido de permissão (`input.plan`,
  com o `planFilePath` e o `tool_use_id` da chamada) e, aprovado, no resultado ("User has
  approved your plan…" e, depois de "## Approved Plan:", o plano). O motor guarda o texto do
  pedido no `detalhe` da chamada (`motor::guardar_plano`, até `claude::MAX_PLANO`, 60 mil
  caracteres; como o pedido vem pelo MCP e a chamada pela saída, o que chegar primeiro espera o
  outro) e, na falta dele, tira do resultado aprovado (`claude::plano_do_resultado`), que é
  também de onde sai o plano da conversa trazida do Claude Code. A chamada sem o texto (as
  conversas de antes disto, o plano recusado de uma conversa trazida) segue como linha da
  trilha, "Propôs um plano". No modo foco, que tira as ferramentas, o cartão também sai.
- **Pedidos de permissão** num cartão de moldura dupla (`cartao_de_pedido`, em
  `src/ui/mensagens.rs`; `moldura_dupla`, em `src/ui/componentes/controles.rs`): por fora, a
  moldura de canto 20; dentro, com 4 de folga, o cartão opaco. No pedido aberto que espera você, a
  moldura de fora é metal (o aro de 4 px); no perigoso, vermelha; no recolhido, preta. O cabeçalho
  tem o azulejo de 28 com o ícone da ferramenta, o título afirmativo ("Rodar um comando", "Editar
  views.py", "Gravar notas.md", "Abrir docs.rs", "Pesquisar na web") e a ferramenta em mono
  ("bash", "edit", o servidor do MCP); o "fora da pasta" em âmbar, quando o caminho fica fora da
  pasta do chat, aparece em qualquer largura. O corpo mostra o comando inteiro numa caixa cavada,
  com "$" na frente, um mini-diff no Edit e as primeiras linhas no Write; o motivo (a descrição do
  comando, o caminho) vai embaixo. A caixa do comando mostra umas 11 linhas e rola: com mais que
  isso (ou mais de 900 caracteres), o cartão avisa "Comando comprido: role a caixa para ler tudo
  antes de permitir.". Escrever na caixa de mensagem e dar Enter nega o pedido e manda o texto
  ao Claude (ver **Responder a um pedido de permissão escrevendo**). No rodapé, **Negar e
  explicar…** (o ícone; o texto vai ao Claude),
  **Negar** e **Permitir**, e à esquerda a caixa de marcar **Não perguntar de novo: edições nesta
  conversa** (ou "este comando", ou o nome da ferramenta): marcada, o Permitir grava a regra, que
  vale já no turno em andamento (`permitir_sempre`); desmarcada, permite só esta vez. Com um
  trecho arriscado no comando (`rm -rf`, `sudo`, `--force`, `| sh`...), o trecho aparece em
  vermelho, um aviso explica, a caixa de marcar some e o botão é **Segure para permitir**
  (`BotaoDeSegurar`, em `src/ui/componentes/controles.rs`, com o `Segurar` de
  `src/ui/movimento.rs`): só confirma depois de 0,9 s apertado, enchendo da esquerda para a
  direita, e soltar ou sair com o ponteiro cancela. Pelo teclado, o primeiro Enter arma ("Enter de
  novo para permitir", por 3 s) e o segundo confirma; a repetição da tecla segurada e um Enter
  colado no primeiro (menos de 250 ms) não contam. Vale igual com as animações desligadas. Com
  vários pedidos de uma vez, eles ficam sob "Pedidos" com o número, e só um mostra o corpo (clique
  no cabeçalho de outro para abri-lo; os fechados têm Negar e Permitir só desta vez ali mesmo, e o
  perigoso, o segurar).
- **O que um hook seu manda perguntar.** Um hook `PreToolUse` do Claude Code pode devolver
  `permissionDecision: "ask"` (a trava de um deploy, por exemplo): a chamada vira o mesmo cartão,
  em **todos os modos, até em Sem perguntar**, com uma linha a mais embaixo do comando, com o
  escudo em âmbar: "Um hook pediu a sua confirmação: <o motivo que o hook deu>", inteira e com
  quebra de linha (até 600 caracteres). Esse cartão não tem a caixa **Não perguntar de novo**, e
  uma regra já gravada na conversa não vale para ele: quem pôs o hook quer ser perguntado toda
  vez. O mesmo cartão (sem a linha) aparece para uma regra `ask` das configurações do Claude Code.
  Como funciona, medido no Claude Code 2.1.328 em 23/11/2026 com um hook de teste: com o "ask",
  ele chama a ferramenta do `--permission-prompt-tool` em qualquer `--permission-mode`, mas só
  com `tool_name`, `input` e `tool_use_id`; sem essa ferramenta ele recusa a chamada e devolve o
  motivo do hook ao modelo como erro. Era o caso do Sem perguntar, que abria sem ela: o cartão
  nunca aparecia e o agente insistia. Em `bypassPermissions` só chega à ferramenta o que o
  próprio usuário mandou perguntar (hook ou regra `ask`): gravar em `.git`, `.claude`, `.vscode`
  ou fora da pasta e comandos com `$(...)` passam direto, como antes. O motivo vem por outro
  caminho, o `hook_response` da saída (`--include-hook-events`), que não traz o id da chamada,
  só o nome da ferramenta, e sai logo antes do pedido: o `claude::interpretar` o vira
  `Evento::HookPediu`, o motor guarda (`src/motor/pedidos_de_hook.rs`) e o pedido seguinte da
  mesma ferramenta, no mesmo chat, o leva. Sem o id, o par é pela ordem: duas chamadas da mesma
  ferramenta ao mesmo tempo, com motivos diferentes, podem sair com o motivo trocado; o motivo que
  ninguém veio buscar (outro hook negou, o turno parou) vence em 60 s e some quando o processo
  sai. A opção dos eventos só vai a partir do Claude Code 2.1.323 (`VERSAO_EVENTOS_DE_HOOK`, a
  mais antiga em que foi conferida; num que não a conhecesse ela derrubaria o turno): abaixo
  disso o cartão aparece como um pedido comum, sem a linha do hook e com a caixa (no turno que
  começa antes de a versão ser lida, vale a do link do instalador nativo,
  `claude::versao_pelo_link`). O `--permission-prompt-tool stdio` traria o motivo no próprio pedido (`decision_reason`,
  `decision_reason_type`), mas a resposta teria de ir pela entrada do processo, que o Vitral fecha
  enquanto os agentes em segundo plano ainda trabalham.
- **Pergunta com opções (AskUserQuestion).** Quando o agente pergunta algo com alternativas, a
  pergunta aparece num cartão dos pedidos (`Janela::cartao_pergunta`, em `src/ui/pergunta.rs`),
  na moldura dupla: cada pergunta com o cabeçalho curto em mono, o texto e as opções para clicar,
  uma por linha, com o marcador redondo (escolha única) ou quadrado ("Dá para marcar mais de
  uma.", o `multiSelect`), o rótulo e a descrição embaixo; por último a **Outra…**, que abre um
  campo de texto livre. No rodapé, **Não responder** e **Enviar**, que só liga com todas as
  perguntas respondidas (o Enter no campo da "Outra…" também envia; com várias perguntas, o
  rodapé diz "1 de 2 respondidas", e as perguntas rolam dentro do cartão quando falta altura).
  O **Não responder** devolve a pergunta sem resposta (o modelo lê que ninguém respondeu), sem
  marcar a chamada como falha. Escrever na caixa de mensagem e dar Enter responde sem escolher
  opção: o texto vale como a "Outra…" de cada pergunta (`motor::respostas_escritas`; a dica do
  Enviar diz "Responde à pergunta com isto, sem escolher opção"). No sino, no aviso do
  canto e na lista ela aparece como "Pergunta · <texto>", com **Ver pergunta**, e nunca tem
  "Não perguntar de novo" (`modelo::regra_de_permissao`). Respondida, fica na trilha como
  "Perguntou <a primeira pergunta>", que abre as perguntas, as opções e a resposta. Medido no
  Claude Code 2.1.328 em 23/11/2026 (Haiku, numa pasta em /tmp): em `-p`, a AskUserQuestion (e
  também `EnterPlanMode` e `ExitPlanMode`) só entra na lista de ferramentas do `init` **com o
  `--permission-prompt-tool`**; sem ele (ou com `--permission-prompts none`) ela some, e o
  `--permission-prompts host` sozinho não a traz. Era por isso que as sessões abertas pelo
  Vitral em "Sem perguntar" nunca a chamavam: até o commit do "ask" de hook, esse modo abria sem
  a ferramenta de permissão. Agora ela existe em todos os modos, e o pedido chega à
  `mcp__vitral__permissao` até em `bypassPermissions`, com `tool_name`, `input` (as `questions`:
  `question`, `header`, `options` com `label` e `description`, `multiSelect`) e `tool_use_id`. A
  resposta volta pelo mesmo caminho da permissão, como `allow` com o `updatedInput` igual à
  entrada mais `answers`, um objeto com o **texto inteiro da pergunta** como chave e a resposta
  como valor: o rótulo da opção, os rótulos separados por ", " quando são várias, ou o texto
  livre (`motor::resposta_da_pergunta`, `Motor::responder_pergunta`; `pergunta::respostas_de`).
  O modelo recebe "Your questions have been answered: "pergunta"="resposta"…". Com o
  `updatedInput` sem `answers` (era o Permitir do cartão genérico) ele recebe "The user did not
  answer the questions."; só com as `answers`, sem as `questions`, o Claude Code recusa o
  `updatedInput` por não bater com o esquema; negado, a mensagem volta como erro da chamada.
- **Ferramentas.** Chamadas seguidas viram uma trilha, uma linha de 32 por chamada, coladas e
  ligadas por ramificações desenhadas num só path: o ícone do tipo (em cor só quando falhou; na que ainda roda, o anel
  girando, `AnelGirando`, no lugar do spinner do kit), o verbo ("Leu", "Editou",
  "Rodou") e o detalhe; a falha tem fundo vermelho claro e "falhou" no fim. O detalhe que é código
  (comando, arquivo, padrão do Grep e do Glob, endereço do WebFetch, ferramenta MCP) vai em mono
  num chip; o que é prosa (a tarefa do Agent, a busca na web, a lista de tarefas) vai em Inter, sem
  chip. **Quanto a chamada levou.** A que ainda roda mostra o relógio na ponta da linha depois
  de 3 s ("Rodando cargo test 00:42"); terminada, a que levou 10 s ou mais
  (`mensagens::CHAMADA_DEMORADA`) fica com a duração no mesmo lugar ("Rodou cargo test · 2 min
  14 s", `demora_da_chamada`; em Inter e com o ponto, para não parecer argumento do comando). O número é o `durou` de `Item::Ferramenta`, em segundos, anotado
  quando o resultado chega (`motor::anotar_resultado` e `duracao_da_chamada`): do pedido ao
  resultado de cada chamada, pelo id dela, então as que rodam em paralelo têm cada uma o seu (o
  Claude Code manda o resultado de cada uma quando ela termina, fora de ordem; medido na
  2.1.328). A que esperou uma permissão conta de quando você permitiu (`Motor.liberadas`, pelo
  `tool_use_id` do pedido). A que foi para o segundo plano, pedida assim ou porque passou do
  tempo dela, não ganha duração: o resultado traz o `tool_use_result.backgroundTaskId`
  (`ResultadoFerramenta.no_fundo`) e o comando segue rodando. O subagente continua com a conta
  dele (tokens, ferramentas e relógio). As chamadas de antes disto não têm o número. O relógio
  da que roda mora nas mensagens, que são uma vista em cache: uma vez por segundo a Janela avisa
  só as mensagens dos painéis cujo chat tem chamada sem resultado
  (`Janela::andar_os_relogios_das_chamadas`, `motor::tem_chamada_rodando`). Um fluxo de agentes (a
  ferramenta `Workflow`) aparece como **Fluxo de agentes**. Grupos de duas ou mais mostram o
  resumo ("Rodou 2 comandos · editou views.py · leu 3 arquivos", com o nome dos arquivos editados
  até dois e a contagem acima disso; fora do painel largo, a forma curta, "2 comandos · editou
  views.py · 3 lidos", para o corte não levar o último número),
  com o ícone da trilha à
  esquerda, de onde descem os ramos, e o chevron no fim, como nas chamadas e no "Pensou"; abrem
  enquanto trabalham e recolhem ao terminar, respeitando a escolha manual. Dentro do grupo, o verbo
  tem uma coluna de 56 para os detalhes ficarem alinhados. A chamada solta (sozinha entre duas
  falas) começa na coluna do cabeçalho de grupo, sem ramo, e com o detalhe logo depois do verbo.
  Recolhido, o grupo mostra as imagens que as chamadas dele leram ou gravaram: uma só, na pílula
  dela; mais de uma, numa linha só (`pilha_de_imagens`): a pilha de até 4 miniaturas quadradas de
  22, sobrepostas como avatares, e "9 imagens", com os nomes na dica (até 10); o clique abre o
  visor na primeira, e as setas passam pelas outras. Aberto, cada chamada mostra a pílula dela.
  **Imagem que a ferramenta devolve** (`src/imagens_recebidas.rs`; a ideia veio do Paseo). O
  print de um MCP de navegador (chrome-devtools, playwright), a imagem gerada e o Read de uma
  imagem chegam em base64 dentro do `tool_result`; a conversa mostrava só "(image/png)". Agora o
  leitor separa o resultado em texto e imagens (`partes_do_resultado`, que lê o bloco da API do
  Claude, `source.data`, e o do MCP, `data` e `mimeType`), as imagens saem num evento à parte
  (`Evento::ImagensDoResultado`, logo depois do `ResultadoFerramenta`) e o motor as grava em
  `~/.local/share/vitral/imagens/<chat>/imagem-<hash>.<ext>` (`guardar_na_chamada`): a
  `Item::Ferramenta` guarda só os caminhos (`imagens`), e o base64 nunca vai para o
  `chats/<id>.json` nem para o `estado.json`. O nome é o começo do SHA-256 do conteúdo, então a
  mesma imagem de novo não grava outro arquivo; a extensão segue a assinatura dos bytes (PNG,
  JPEG, GIF, WebP), e o que a conversa não sabe mostrar continua como a marca no texto
  ("(image/svg+xml)"). A pasta do chat é só do dono (0700, os arquivos 0600). A chamada mostra a
  miniatura de cada imagem (`imagens::da_chamada`; mais de uma, lado a lado), que entra na pilha
  do grupo recolhido e na fila do visor junto com as anexadas, sempre como miniatura que abre no
  visor, nunca como imagem grande no meio do texto (dono, 17/11/2026). No Read de uma imagem
  vale a do disco, inteira, enquanto ela está lá; apagada (um print em /tmp), fica a guardada,
  que é a que o modelo viu. **Tetos e limpeza:** 10 MB por imagem (`TETO_POR_IMAGEM`; a maior não
  é guardada e o resultado diz "passa do teto de 10 MB e não foi guardada"), 100 MB por chat
  (`TETO_POR_CHAT`; passou, saem as mais antigas daquela pasta, e a miniatura delas some) e 1 GB
  de todas as conversas juntas (`TETO_TOTAL`, conferido ao abrir o Vitral, tirando as mais
  antigas de qualquer chat). O chat que sai do Vitral leva a pasta dele (`Dados::apagar_chat`),
  e ao abrir saem as pastas de chats que não estão no estado nem têm mais o `chats/<id>.json`
  (`varrer`). As conversas importadas do Claude Code e as de antes desta versão não têm as
  imagens (o importador lê só o texto do resultado). **No Codex** (conferido no app-server do
  0.169.0 com um turno de verdade, 23/11/2026): o `view_image` não devolve base64, e sim o item
  `imageView` com o caminho do arquivo, que vira a leitura dele ("Leu a.png", com a miniatura do
  disco); o resultado de uma ferramenta MCP (`mcpToolCall.result.content`) traz os blocos do
  MCP, e a imagem deles é guardada como a do Claude (antes ia para a conversa o JSON inteiro do
  resultado, com o começo do base64); e a imagem gerada (`imageGeneration`, só pelo schema, sem
  turno de verdade) aponta para o `savedPath` ou, sem ele, é guardada a partir do `result`.
  **Turno pronto.** Quando o turno termina, tudo o que veio antes da resposta final recolhe numa
  linha só, "Trabalhou 6 min 12 s · 13 comandos · editou carrinho.py e motor.rs" (o nome dos
  arquivos editados até dois; acima disso, a contagem), com a resposta logo embaixo; o clique na
  linha abre e recolhe o resto. Só recolhe o turno com pelo menos uma trilha e um texto
  intermediário; o texto intermediário que termina em pergunta, o aviso de erro e o cartão de
  falha ficam à vista, e as imagens das chamadas escondidas continuam embaixo da linha, na pilha.
  A duração é a hora do último item menos a da mensagem (sem número nas conversas de antes da
  0.4.0). **Checagens do turno** (`src/ui/checagens.rs`): ao lado dos comandos, a linha diz como
  terminou cada checagem que o turno rodou, "tsc passou · vitest falhou 2×, passou · cargo test
  passou": vale a última execução de cada uma, e as falhas de antes viram "falhou N×" (o
  "falhou" no vermelho da chamada que falhou; o resto no tom da linha; a lista inteira também
  na dica, porque no painel estreito a parte que não cabe some inteira). Contam o `cargo test`,
  `check`, `clippy`, `build` e `nextest`, o `tsc`, o `vitest`, o `jest`, o `pytest`, o `go
  test`, `build` e `vet`, e os scripts `test`, `build`, `typecheck` e `lint` do npm, pnpm, yarn
  e bun (com `test:unit` e parecidos), atrás de variáveis, `cd x &&`, `timeout`, `npx` e
  parentes. O resultado é o `erro` da chamada de Bash quando o código de saída é o da checagem
  (nada depois dela além de `&&`). Com um `|` depois (`cargo test 2>&1 | tail -40`), um `;` ou
  um `||`, o código de saída é de outro comando: vale o que a saída diz ("test result:" do
  cargo, "error TS" do tsc, "Test Files" do vitest, "Tests:" do jest, "N passed in" do pytest,
  "ok"/"FAIL" do go), e sem sinal nenhum a checagem fica de fora, em vez de aparecer como
  "passou" sem ter passado (os scripts do npm atrás de um pipe nunca aparecem). Numa cadeia de
  `&&` que falhou, a última checagem que deixou sinal na saída rodou, então as de antes
  passaram e as de depois de uma falha não aparecem; sem sinal de nenhuma, todas ficam como
  falha. Um wrapper próprio (`./checar.sh`) não é reconhecido. Na dupla, cada agente tem o seu
  recolhido. O Ctrl+F abre a linha ao achar algo lá
  dentro. O histórico importado do Claude Code não recolhe. A dica da linha diz os tokens e o
  custo do turno, "182 mil tokens · US$ 0,84", tirados do livro dos gastos (`Livro::do_turno`, em
  `src/motor/gasto.rs`, pela mensagem que abriu o turno: o índice e a hora dela), e abre os
  tokens em três números: a entrada nova (a sem desconto mais a gravada no cache), o que foi lido
  do cache e a saída. Só na dica: a linha continua curta (dono, 18/11/2026). Embaixo, na mesma
  dica (`onde_uso::dica_do_turno`, com o `Livro::estatistica`): quanto da entrada veio do cache
  ("96% da entrada veio do cache": o lido do cache sobre a entrada nova, a gravada e a lida), o
  modelo com as idas ao modelo ("Opus 5.5 · 12 passos"), os tempos ("6 min 12 s no total · 4 min
  3 s de modelo · 1,8 s até a primeira palavra") e quanto custou subir o Claude Code ("1,6 s para
  abrir o Claude Code"). Cada linha só aparece com o dado dela; ver **Estatística do turno**, em
  **Contas e uso dos planos**. O turno sem linha no livro (os de antes disto, e os
  de mais de dois meses atrás, que a abertura não lê) fica como era. O clique numa
  chamada mostra primeiro o que ela pediu, numa caixa em mono: o diff de um Edit, o começo do que um
  Write gravou e o comando inteiro de um Bash (até 60 linhas de cada lado ficam guardadas na
  conversa). O diff mostra só o que mudou (`edicao::diff_da_trilha`): as linhas que saíram em
  vermelho e as que entraram em verde, com 3 linhas iguais em volta de cada mudança, "… 9 linhas
  iguais" no que ficou de fora entre duas mudanças, e a palavra trocada com o fundo mais forte
  dentro da linha. Antes o lado antigo vinha inteiro em vermelho e o novo inteiro em verde: uma
  palavra trocada em 10 linhas eram 20 linhas coloridas. As linhas são casadas pela subsequência
  comum que já punha a edição sobre o arquivo (`edicao::casar`, com o mesmo teto de 250 mil
  comparações: acima dele ficam o começo e o fim iguais, e o meio sai e entra). A palavra é
  marcada quando saíram e entraram o mesmo número de linhas seguidas (a primeira com a primeira),
  comparando palavras, brancos e sinais; linhas com menos da metade das palavras em comum, ou
  que somam mais de 1024 bytes, ficam sem marca, porque a linha inteira já diz que mudou. O
  Copiar do diff leva o que a tela mostra. O pedido de permissão de um Edit ou MultiEdit usa o
  mesmo diff, com 2 linhas de contexto (`diff_da_troca`). Numa troca com mais de 60 linhas de
  um lado, o começo igual dos dois lados não é guardado (ficam 3 linhas antes da primeira
  diferença, `claude::sem_o_comeco_igual`): sem isso a mudança da linha 150 ficava depois do
  corte e o diff abria sem mudança nenhuma. **Ver a edição** (`src/ui/edicao.rs`): numa edição que deu
  certo (Edit, MultiEdit, Write, NotebookEdit; o `fileChange` do Codex), o clique na linha abre o
  arquivo no painel da direita já na edição, e o chevron do fim, sozinho, mostra o diff ali
  mesmo. O Codex entrega um arquivo por linha, cada um com o diff dele (o `changes[].diff` do
  `fileChange` do app-server, conferido no schema do Codex 0.169.0: o diff unificado no
  `update`, o conteúdo no `add`, que vira um Write, e o que tinha no `delete`), lido no fim do
  item, quando o patch chegou inteiro. O registro antigo do Codex, que juntava vários arquivos
  numa chamada sem o diff, aparece com uma linha por arquivo (no lugar da caixa com a lista de
  "mudou …"), e o clique abre o arquivo como está agora, com um aviso discreto. Recolhidos, o
  turno ("Trabalhou…") e o grupo de chamadas mostram embaixo deles uma linha por arquivo
  editado, com o +N −M e o "novo" (até 5; acima disso, "e mais N arquivos", que abre o
  recolhido): o clique abre a edição daquele arquivo, somando as chamadas do turno. Comandos têm comando e saída na mesma caixa. O resultado mostra as
  últimas 40 linhas, com Copiar e Mostrar tudo); o de uma edição que deu certo é só "o arquivo foi
  atualizado" e fica de fora. No hover, a linha que leu ou mudou um arquivo que ainda está no disco
  ganha na ponta o ícone **Abrir o arquivo ao lado**. As ferramentas do Vitral que um chat usa
  (`mensagem_para_chat`, `abrir_chat`) têm o ícone e o verbo delas, e no hover a linha ganha
  **Abrir o chat** (o balão de conversa; Ctrl+clique abre ao lado). As das versões de antes (as do
  Mestre e dos ajudantes, nas conversas antigas) continuam com os verbos delas. Fora da largura
  larga, o verbo e a frase vão para a dica (o ícone já diz o que foi) e o título fica com o espaço.
- **Mensagens.** O que você escreve fica num balão azul baixo na largura toda da coluna (raio 16,
  ponta no canto de baixo à direita, texto em 14/22, um degrau abaixo da resposta), com as pílulas
  das imagens e arquivos anexados no poço escuro em cima do texto e os selos
  **Ultrathink** (em arco-íris) e **Ultracode** (em violeta, com o ícone de fluxo) quando a mensagem
  foi com eles. Os caminhos que existem na pasta do chat viram link e abrem no painel da direita,
  como na resposta; o código em linha e os blocos de código vão no poço escuro. Um comando sozinho
  ("/compact") vira uma pílula azul em mono. Texto com mais de 14 linhas mostra 12, com a última
  sumindo num véu, e **Mostrar tudo** com o chevron. As quebras de linha que
  você digitou ficam no balão (`com_quebras_duras`: cada linha seguida de outra ganha a quebra dura
  do markdown, menos nos blocos cercados e nas tabelas), e o Copiar leva o texto como foi escrito. Mensagens que outro chat
  (ou o Vitral) entregou mostram quem mandou (o balão de outro chat ou o logo do Vitral; nas
  conversas antigas, o círculo do Mestre, do coordenador ou a chave inglesa de um ajudante) e uma
  prévia; as de outro chat têm **Abrir o chat**. A prévia termina com reticências antes da coluna da
  seta, nunca cortada no meio da letra; o clique abre o texto inteiro. Os avisos do Vitral são
  marcos centralizados ("Sessão nova", "Turno interrompido", "O Vitral fechou no meio do turno",
  que é como aparece o aviso gravado do turno cortado, sem a instrução de usar Retomar, porque a
  faixa logo abaixo já tem o botão; quando ele é o último item e a faixa diz "Parada · Vitral
  fechou", ele nem aparece, para o mesmo fato não vir duas vezes, menos com a busca aberta),
  linhas tênues ("Tentando de novo.")
  ou, quando é erro, o cartão de erro (título em português, o que fazer, Copiar no canto, o texto do
  agente em **Detalhes**; ver **Chat que cai**). A troca da sessão antiga do Claude Code por uma
  nova com resumo (a que sumiu e a que expirou) é o marco "Sessão nova, com um resumo da anterior",
  e as linhas de troca de conta ou de agente levam o ícone de troca. O que chega com a conversa aberta no painel (a sua
  mensagem, a resposta, um evento, um aviso, a chamada que chega numa trilha já à vista) entra com
  um fade e 8 px de baixo, uma vez; trocar de conversa no painel faz a conversa inteira entrar
  com um fade, e trocar de painel ou rolar não anima nada. Entre turnos ficam 32 px antes do seu balão
  (e antes do separador de dia), 12 depois dele e 12 entre os blocos de um turno (`esp::ENTRE_TURNOS`
  e `NO_TURNO`): com o vão de fora mais de 2 vezes o de dentro, cada pergunta fica presa à resposta
  e os turnos se leem como parágrafos (relatório de aparência, 16/11/2026, V3).
- **Rolagem.** Barra visível por painel quando o conteúdo transborda, com identidade própria e
  desenhada sobre a área fixa, fora do conteúdo rolado. Os itens anteriores aparecem em páginas
  de 60 ao chegar perto do topo; a compensação da altura acrescentada mantém a leitura no lugar.
  Dos itens que já entraram, só os blocos à vista são montados (ver **Custo de um redesenho**):
  rolar uma conversa em que já se subiu muito custa o mesmo que rolar uma recém-aberta.
  O histórico importado amplia a janela de leitura do arquivo quando necessário. A suavização
  antiga foi retirada em 16/11/2026: ela restaurava o deslocamento antes de detectar a subida,
  mantendo o painel preso ao fim. No fim, a conversa continua colada no fim também
  quando o painel muda de largura (abrir ao lado, Ctrl+B, zoom, puxar a borda da janela), em que
  as linhas quebram de novo. Qualquer subida com a roda, o touchpad ou a barra solta o fim; aí o
  streaming não puxa a tela de volta e a conversa não conta como lida; a pílula com a seta (e
  "Novas mensagens", com um ponto azul, quando chegou algo), que entra com um fade, volta ao fim,
  como o **Ctrl+End** (descendo, os últimos 48 px já prendem de novo). Fora do fim, o pé da área
  das mensagens esmaece em 24 px até o cartão, para a última linha não ficar cortada a seco acima
  da faixa ou da caixa (no topo, não). Abrir um bloco no fim da
  conversa (a saída de uma ferramenta) não pula para o fim dele: o
  painel fica onde está.
- **Onde a conversa abre.** Um chat com resposta que você não viu abre no começo dela quando ela
  não cabe na tela (com a linha "Trabalhou…" logo acima e a pílula "Novas mensagens" embaixo);
  quando cabe, abre no fim, como sempre. Um separador "Novo", em azul, marca o primeiro item que
  você não viu (sem contar as suas mensagens). Abrir não marca como lido: o chat só sai de
  "Terminou" e do ponto azul com o fim à vista (rolando até lá ou com o Ctrl+End). Ao trocar de aba
  no meio de uma leitura, cada painel guarda onde você estava em cada chat (o item no alto e
  quanto dele já subiu), e voltar à aba volta a esse ponto; isso vale enquanto o Vitral está
  aberto. Se o Corrigir tirou o item, a conversa abre no fim.
- Mensagens, pedidos e caixa ficam numa coluna de leitura centrada (até 860 px, com 16 de margem).
  Em janela baixa, o plano e os pedidos encolhem e rolam, e a caixa cresce menos. Blocos de código
  têm cor de sintaxe. **Copiar tabela:** cada tabela de uma mensagem tem o Copiar dos blocos de
  código (`BotaoCopiar`, tênue) no canto direito, logo abaixo dela: o clique copia a tabela em
  Markdown, e com Shift copia separada por tabulação, que uma planilha cola em colunas
  (`mensagens::tabela_para_planilha`; a tabulação e a quebra dentro de uma célula viram espaço).
  O Markdown copiado sai sem os links internos que os caminhos de arquivo ganham na resposta
  (`links_arquivo::sem_links_do_vitral`: o caminho volta a ser código em linha).
  É o gancho `table_actions` do kit, que desenha o que recebe numa fileira abaixo da tabela, e
  não por cima dela como no bloco de código: por isso o botão fica embaixo, e o texto depois de
  uma tabela desce uns 28 px.


## O que fica à vista

O padrão é o mínimo, e o resto aparece quando faz falta:

- **A mensagem presa no alto** (`src/ui/mensagens.rs`, decidida no prepaint da lista): a sua
  mensagem mais recente que já subiu além do alto da conversa gruda lá, em duas linhas, numa faixa
  com as camadas do cartão; o clique leva até ela. A mensagem seguinte, chegando no alto, empurra
  a faixa para cima (`Painel::empurrao_da_presa`, com a altura da faixa medida no desenho dela),
  como no "sticky" do VS Code: antes ela passava por baixo, e a de cima cobria a de baixo (dono,
  18/11/2026: "fica meio bugado quando tá assim").
- **O alto da lista é uma linha só**: o logo (o menu do Vitral) e as abas **Conversas | Arquivos**,
  na mesma altura (40), num segmentado de largura toda (`topo_enxuto`, em `src/ui/trilho.rs`).
  A fileira tem a altura do cabeçalho dos painéis (44), para o logo e as abas da lista ficarem na
  linha das abas das conversas ao lado. Arquivos mostra a pasta do chat aberto.
- **A busca e o "+" moram no pé da lista**, embaixo das Arquivadas, dos agentes rodando e do sino,
  também na altura de 40. O recolher da lista (Ctrl+B) mora no menu do logo, **Recolher a lista**.
- **A lista é corrida**, sem título nem os das seções: o que espera você, o que roda, as fixadas
  (com o alfinete) e todos os outros, nessa ordem. Cada chat é uma linha só de 32 (o balão neutro, o
  título e a hora; a pasta vai na dica do balão); a linha de baixo volta na busca que achou
  no texto, com o nome da pasta na frente, e o resultado da busca usa o mesmo balão pequeno das
  outras linhas.
- **Novo chat não pergunta nada.** O "+" e o Ctrl+N abrem um chat na hora, na pasta do chat aberto
  (ver **Chat novo**, em **Como funciona**). Escolher a pasta fica no menu do logo, **Novo chat em
  outra pasta…**: as pastas que já têm chat (com o caminho embaixo do nome), **Outra pasta…**, que
  escolhe uma no disco, e **Sem pasta**.
- **Os anéis de uso** flutuam no canto de cima, à direita, da janela, logo abaixo do ×, um em cima
  do outro (`uso_no_canto`, em `src/ui/uso_plano.rs`), cada um no disco opaco da peça, com o fio de
  1 px e uma sombra curta, como um botão flutuante: a conversa vai até a borda por baixo deles, sem
  folga (dono, 16/11/2026: eles passaram pelo cabeçalho e voltaram no mesmo dia, "o de uso tem que
  ficar onde eu tinha falado, embaixo do ×"): cada um só com a conta
  daquele serviço conectada (0.10.0; `aneis_a_mostrar`): o do Claude com uma conta com o login
  guardado e não perdido (`Motor::claude_conectado`, de `busca::tem_login`, lido ao abrir e a cada
  busca do uso), o do Codex com o `auth.json` dele e uso lido. Sem conta, o anel some, a fileira
  do painel da direita não guarda lugar (`reserva_dos_aneis`) e o "Entrar" e o "Instalar o
  Codex…" seguem no menu do logo. **Mostrar o uso do plano no canto da janela**, em
  Configurações > Aparência
  (`Config.uso_no_canto`, ligada por padrão), os esconde; o uso continua em Contas do Claude.
- **O cabeçalho da conversa tem pouca coisa:** as abas e o **+** (novo
  chat ou histórico). O resto do painel, inclusive abrir e fechar a direita, mora no menu do botão
  direito no cabeçalho (ou Shift+F10).
- **O chip da conta**, na caixa, só aparece com mais de uma conta.
- **O × do canto é o da janela.** Com o painel da direita aberto, o cabeçalho dele tem o botão de
  fechar o painel (o ícone de recolher), um traço e então os botões da janela; o × acende em
  vermelho e a dica diz "Fechar o Vitral". Com a direita fechada, os botões da janela ficam no
  cabeçalho da última conversa, também depois de um traço e um pouco afastados do **+**, para o ×
  não parecer o de fechar o painel.

## Acabamento

O acabamento em uso é o **Grafite** (`superficie::GRAFITE`, em `src/tema.rs`), de 12/11/2026; o
azul-marinho de antes continua em `superficie::MARINHO`, e trocar o `ATUAL` volta a ele.

- **Cor.** Um quase preto frio, em degraus (a base, a conversa, o que é elevado), e um destaque
  só, azul cheio (`#2B63FF`): os links, o texto do que roda e, bem leve, a ficha do escolhido. O
  "+" e o enviar são teclas de metal, não azul chapado. Seleção, hover e anel de foco são neutros.
  Os ícones dos chips da caixa (modo, esforço) são neutros; a cor fica para o que pede cuidado (o
  "Sem perguntar").
- **Metal líquido: a chapa e as duas camadas.** Atrás de tudo há uma chapa de metal líquido. A
  chapa é a receita "Mesh drift" que o dono montou na 21st.dev e colou em 12/11/2026 ("tenta focar
  em vez de glass esse metal líquido"): cinco bolhas (uma quase preta e quatro azuis) derivando,
  com grão de filme e uma luz que segue o ponteiro (só com o metal andando). Ela é opaca: o papel de
  parede saiu de cena (o vidro que deixava vê-lo acabou). Por cima dela o app tem **duas camadas**.
  A **base** é a janela inteira: a chapa debaixo de um véu azul-escuro (`vidro_leitura` do Grafite,
  #0A1533 a 0xE3: 11% do metal passa, e o escuro fica de um azul fundo que muda de lugar, mais
  claro onde passa uma bolha). A lista mora direto na base, sem cartão, fundo nem aresta próprios
  (na gaveta, a superfície é a da gaveta). Por cima flutuam os **cartões**, a conversa e os painéis
  da direita (Este chat, o arquivo aberto), com a sombra dos flutuantes (`sombra_flutuante`)
  e quase opacos (5% do metal passa). Margem da janela e vão entre os cartões: 8 px de tela, os
  dois iguais e sem crescer com o zoom (o vão entre a lista e o 1º painel, entre os painéis e
  entre o último e a direita é metade de cada lado da divisória; dono, 16/11/2026: "a do meio dos
  chats tem que ficar igual das bordas"); a faixa de pegar da divisória é mais larga que o vão e
  passa por cima da borda das peças, invisível. Canto dos
  cartões: 16 (`margem`, `vao` e `raio_peca` da variante). Menus e popovers são opacos e um
  degrau mais claros que a peça de onde saem (`flutuante` #1F222B, com a sombra funda descendo
  24; mais claro que isso, o apagado no recuo do flutuante passa do piso de 4,5:1). Texto nunca vai no metal cru: os alfas
  saem da conta de contraste contra a cor mais clara que a chapa entrega (`TETO_DO_METAL`,
  #ACD9FF, a bolha clara com a luz em cima), e o teste `contraste_dos_tokens_no_pior_caso` roda
  contra ela (0xE0 é o mínimo da lista). Para o que vier por cima do metal há dois véus:
  `Cores::veu_de_placa()` (preto a 56%, debaixo de `forte` e `texto`) e `Cores::veu_de_botao()`
  (preto a 36%, debaixo de ícone branco).
- **O motor do metal** (`src/ui/componentes/metal.rs`). A conta do shader roda em Rust numa grade
  de 1 ponto a cada 8 px da janela (uns 20 mil pontos, 2 ms no binário de debug), vira uma imagem
  pequena (um BMP montado à mão, que o `image` do GPUI já sabe ler: nenhuma dependência nova) e a
  placa de vídeo a estica com filtro linear; o grão, que é por pixel, vai por cima num ladrilho
  fixo de 256 px. Uma chapa só por janela: o `metal::fundo` (o primeiro filho da Janela e da
  vitrine) faz o quadro e o guarda; cada `metal::abertura` pinta um recorte da mesma imagem, sem
  conta nova (`Encaixe::Continuo` mostra o pedaço da chapa que está atrás; `Encaixe::Cobrir`
  encolhe a chapa inteira até cobrir a caixa, para botões e placas), e o `metal::aro_de_metal` é
  uma abertura de 2 a 4 px em volta de um miolo opaco. O teste `metal_so_pelo_motor` barra
  `paint_image(` e `drop_image(` no resto do `src/ui`. Nenhuma abertura dentro de view com
  `.cached()` (a dica do kit usa): a textura troca a cada batida. O que mora numa view guardada (os
  balões seus e a faixa presa da conversa, pelo `metal::fundo_da_lista`) usa a
  `metal::abertura_parada`, um quadro parado do campo que só é refeito quando a janela muda de
  tamanho: com a chapa que anda, cada balão ficava no quadro em que foi desenhado e trocava quando
  o ponteiro passava por ele. No Windows o renderizador do
  GPUI (Direct3D) amostra o sprite com filtro linear e devolve o espaço do atlas ao soltar a
  imagem, como o do Linux (lido no fonte; falta ver rodando lá).
- **Quando o metal anda.** É a exceção ao "nada anima em repouso", com custo medido. Cada batida do
  laço redesenha a Janela inteira, e no binário de debug isso custa uns 35 ms de processador: a 8
  quadros por segundo em repouso o app gastava 30% de um núcleo (bancada, chat parado aberto,
  ponteiro parado na janela, 30 s; o limite era 8%), e o metal em si era o de menos (com o campo
  congelado, 28%). Por isso a política é: **em repouso o metal fica parado e nada redesenha** (0%
  medido); ele **anda a 20 quadros por segundo só com um chat à vista trabalhando** (um chat de um
  painel aberto; o que roda em segundo plano não o acorda), e só então a luz segue o ponteiro. O
  ponteiro mexendo **não** acorda o metal: acordava, e cada movimento redesenhava a janela inteira
  20 vezes por segundo, o que engasgava a rolagem (dono, 12/11/2026). A exceção é o botão do logo,
  que anima com o ponteiro em cima ou com o menu aberto (ver **Botão do menu**). Com a janela sem
  foco ou o movimento reduzido (GNOME sem animações, Animações desligadas nas Configurações,
  `VITRAL_SEM_ANIMACAO=1`) fica parado, na fase inicial e sem a luz. Andando, o debug mede de 15 a
  18 quadros por segundo e de 52% a 62% de um núcleo (o redesenho da Janela, não o metal). O ritmo
  de repouso é uma constante só, `movimento::QPS_EM_REPOUSO` (hoje 0; com 8 volta a animar em
  repouso), e o `movimento::laco_a(window, cx, qps)` é a porta: guarda a fase em `f64` e a hora da
  próxima batida por janela, no compasso (o tempo do desenho não se soma ao intervalo). Quem decide
  o ritmo da chapa é o `metal::fundo(trabalhando, …)`, chamado no `render` da Janela.
  `VITRAL_QUADROS=1` mostra os quadros e o tempo do campo.
- **Transparência dos fundos e o desfoque.** Nas Configurações, **Transparência dos fundos** vai do
  Opaco (o padrão) ao Leve, Média e Forte (`tema::NIVEIS_DE_TRANSPARENCIA`): o metal perde
  opacidade, o véu da base e o cartão deixam passar o que está atrás da janela, e um grão por cima
  faz ler como fosco. Nos níveis transparentes a janela pede ao sistema o fundo desfocado
  (`Blurred`, em `fundo_da_janela`, no `src/main.rs`), com o mesmo código em todo sistema, e o que
  está atrás aparece desfocado de verdade: no Windows (o acrílico), no macOS, no KDE e no GNOME 51
  em diante. No Wayland o GPUI só conhecia o protocolo do KDE; o padrão, o ext-background-effect-v1
  (o que o GNOME 51 trouxe), vem do `vendor/gpui-pre-linux` (ver o `Cargo.toml`). Com os dois no
  compositor vale o padrão, e nos dois o desfoque cobre só o quadro da janela, sem a margem da
  sombra. **No GNOME 50 ou anterior** (e no X11) não há desfoque: a janela fica só transparente,
  com o véu e o grão por cima (no GNOME, a extensão Blur my Shell pode desfocar por fora). No Opaco
  a chapa cobre tudo e o pedido é só transparente, para não gastar o desfoque; trocar o nível troca
  o pedido na hora. `VITRAL_FUNDO=mica`, `transparente` ou `opaco` força um fundo em qualquer
  nível, para comparar (o Mica só existe no Windows 11).
- **Três materiais.** Tudo é feito só de luz e sombra de 1 px por dentro (`src/tema.rs`): **vidro**
  nos cartões (`aresta_de_vidro`: fio claro de 1 px nos lados, luz na própria quina de cima, sombra
  de 1 px na base e um brilho de 3% que entra uns 40 px pelo topo; o brilho fica em 3% porque com
  5% o apagado sobre a lista reprova no contraste), **tecla** no que é elevado e escolhido
  (`aresta_de_tecla`: a pílula escolhida do segmentado, as teclas de atalho) e **poço** no que é
  cavado (`aresta_de_poco`: o Buscar, o trilho do segmentado, os contadores). A aresta das peças
  mora numa camada à parte, o último filho da peça (`camada_de_aresta`, ou `.aresta_de_vidro(raio)`
  depois dos filhos), porque a sombra interna pinta antes dos filhos; e a peça não pode ter
  `border`, que empurra a camada para dentro e faz a luz cair na segunda fileira de pixels (era o
  caso da conversa, com a borda de 1,5). Cada lado é uma sombra deslocada 1 px, não um anel: o anel
  passava também pela base, e o claro dele com a sombra por cima dava o tom do fundo. Tecla só no
  que está escolhido, nunca em cada linha de lista.
- **Sem halo; o metal diz "é aqui".** Sem degradê, sem halo azul e sem borda azul. O "é aqui" é
  sempre um pedaço do metal: o **aro de metal** de 2 px (`metal::aro_de_metal`, em volta de um miolo
  opaco) na caixa de mensagem e na busca com o foco e na aba sob o ponteiro; o aro de 4 px na
  moldura do pedido de permissão que espera você; e, no painel selecionado, a **luz de baixo** (a
  doca, abaixo). As cantoneiras azuis e o pontilhado animado saíram (as `cantoneiras` ainda moram
  em `superficies.rs` e no kit, `cantoneira` em `vendor/gpui-component/src/styled.rs`, e só a
  vitrine as usa).
- **A doca (a luz de baixo).** O pé de todo cartão de conversa é uma abertura por onde se vê o
  metal, com a caixa de mensagem pousada nela (`src/ui/conversa.rs`). Só o painel ativo a acende
  (180 ms; apaga em 120): o campo inteiro do metal encolhido para cobrir a doca, com 20% do azul do
  destaque por cima e o grão leve (`metal::abertura_lisa`), e o escuro do cartão sumindo devagar de
  cima para baixo (`tema::gradiente_da_doca`, em duas camadas), como uma luz que vem de baixo. Nos
  outros painéis o pé fica escuro. Na conversa vazia a doca é alta (172 px acima da caixa, ou 56
  numa janela com menos de 640 de altura); no resto, rente à caixa. Ela está no fluxo: as
  mensagens terminam onde ela começa, e nenhum texto fica por cima do metal.
- **Retícula.** Uma textura só, no padrão do próprio shader, um quad por uso: a **trama** (xadrez de
  2 px numa cor, o resto transparente), o **fio de pontos** (o xadrez numa fileira só, "2 aceso, 2
  apagado", no lugar de toda linha cheia: entre as linhas dos grupos das Configurações, recuado 12,
  em cima dos rodapés de teclas dos popovers da caixa, do Novo chat e do corpo de popover, nos
  separadores dos menus e ao lado da etiqueta de topo dos popovers de escolha) e a
  **hachura** diagonal (para o que saiu). O lado é medido em pixel do aparelho e arredondado
  (`lado_da_trama`), senão o xadrez borra em 125%. O desenho mora no kit (`trama`, `fio_de_pontos`,
  `fio_de_pontos_em_pe`) e o app passa por `ui/componentes/graficos.rs` (`trama`, `fio_de_pontos`,
  `hachura`); o teste `reticula_so_pelos_graficos` barra `checkerboard(` e `pattern_slash(` no resto
  do `src/ui`. Os dois renderizadores têm os padrões (o `shaders.wgsl` do Linux e o `shaders.hlsl`
  do Windows, casos 2 e 3), então não há queda para o liso.
- **Cantos.** Um padrão só nos controles: `raio::de(altura)`, em `src/tema.rs`, 45% da altura do
  controle (16 num botão de 36, 14 num de 32, 9 num chip de 20): bem arredondado, nunca redondo. É
  o desenho do logo em escala. Botões, chips, abas, a busca, selos e os itens de menu usam ele; só
  ponto, barra fina e anel continuam redondos. As peças e as caixas grandes seguem nos tokens: 16
  nos cartões da janela, 20 na moldura da caixa, nos popovers e nos menus, 14 nos cartões de
  dentro, 10 nas linhas.
- **Anéis.** Todo anel tem uma fenda de 60° embaixo e pontas redondas (`pintar_arco`, `Anel` e
  `AnelGirando`, em `src/ui/componentes/graficos.rs`): vai das 7 às 5 horas, com o trilho escuro e o
  arco da fração por cima. O `AnelGirando` é o "carregando" do app: o mesmo anel, aberto, dando uma
  volta a cada 1,2 s no laço de 20 quadros por segundo (parado, com a fenda embaixo, no movimento
  reduzido); ele substitui o spinner do kit nas linhas de ferramenta, na linha de estado da caixa e
  no parar. O anel de uso (`AnelDeUso`) é um só por serviço e mostra a janela mais apertada das
  duas (5 h ou semana): eram dois, um dentro do outro, e com a fenda eles liam como "duas barras".
- **A ficha do escolhido.** O item escolhido (num menu ou popover de escolha, a aba escolhida do
  alto da lista, o nível de esforço) vai numa ficha azul leve, com um fio do mesmo azul e o relevo
  de tecla, no canto dos controles (`ficha_do_escolhido` e `ficha_de_metal`, em
  `src/ui/componentes/superficies.rs`). Já foi um recorte de metal debaixo de um véu; num retângulo
  comprido ficava preto de um lado e azul chuviscado do outro. O nome `ficha_de_metal` ficou.
- **Menus e popovers.** O menu do logo e os menus com cabeçalho de objeto começam numa **placa de metal** (`placa_de_metal`, `cabecalho_objeto`: o recorte da
  chapa debaixo do véu de placa, 52 de altura, com o símbolo, o nome em branco e o detalhe em
  mono). Os atalhos saem em teclas desenhadas, os separadores são o fio de pontos, o item sob o
  ponteiro (ou o teclado) ganha o relevo de tecla, no canto `raio::de` da altura dele, e o rodapé é
  uma faixa escura com a linha técnica em mono (no menu do logo, a versão e o commit). O contorno
  perdeu o anel azul. Nos popovers de escolha (modo de permissão, conta, modelo, esforço),
  o ícone de cada item vai num azulejo de 28 (`azulejo_de_escolha`) e a etiqueta do topo ganha o
  fio de pontos até a borda.
- **Letra, em duas vozes.** Inter no texto. A segunda voz é a JetBrains Mono miúda, de aparelho de
  medição: no código, nas teclas, na `etiqueta()` (mono 11/14, dos fatos e
  medidas; o componente `Etiqueta`, uma pílula de fio com ícone, tom, `baixa` e `fundo`, mora ao
  lado do `Selo`) e nos rótulos de seção, que saem em mono 11 apagado, em caixa alta e com um
  tracinho de 8 por 1 na frente ("PASTAS", "CHATS"). A caixa alta é só do
  desenho (`etiqueta_de_secao`, `CabecalhoDeSecao`, o título do `GrupoDeOpcoes` e o rótulo de grupo
  da lista com busca do kit); o dado continua "Pastas", "Hoje". Fica em 11: o GPUI não tem
  espaçamento entre letras e 10 em caixa alta aperta. Todo número pequeno (`metadado`, `micro`,
  `numero`) usa dígitos de largura igual (`tabular`, o `tnum` da Inter), e o tempo que corre não
  faz a coluna andar. O título da tela vazia tem um tamanho próprio, `titulo_heroi` (28/34).
- **Teclas.** Um desenho só em todo o app (`desenho_de_teclas`, em `vendor/gpui-component/src/kbd.rs`,
  que a dica do kit e o `desenho_teclas` do app chamam): 18 de altura, no mínimo 18 de largura, sem
  borda, fundo claro a 8% em relevo (luz em cima, sombra embaixo) e a letra em mono 10. A busca da
  lista mostra "Ctrl" "K" à direita enquanto está vazia e sem foco, com a lista a partir de 276 px.
- **Logo.** O V do dono (uma gota à esquerda, ligada por um pescoço fino à perna grossa da
  direita), em branco, sobre um quadrado de metal líquido de cantos 45% (o mesmo `raio::de`). É o
  mesmo em todo lugar: o botão do menu, o cabeçalho do menu do Vitral, as mensagens do Vitral na
  conversa, a tela de recuperação e o ícone do app (os PNGs, o `.ico` e os dois SVG). Os arquivos
  saem de `ferramentas/logo/`: o `rasterizar.py` tira o V de `v-do-dono.png` (a forma escura num
  fundo branco) e grava o `assets/vitral-v.png` (256 px, o V branco com a cópia escura deslocada
  por baixo, para continuar legível sobre a bolha clara); o `icones.py` pinta o metal parado na
  fase do botão do menu, põe o V por cima, recorta o canto e grava os `assets/vitral-*.png` (48 a
  256), o `vitral.ico` (16 a 256) e os dois SVG, `vitral.svg` e `vitral-pequeno.svg`, cada um com o
  PNG embutido (o metal tem grão e não se descreve em vetor); o `previa.py` é a conta do metal em
  Python, que o `icones.py` importa. Os três precisam do Pillow. Uso: `python3 -I
  ferramentas/logo/rasterizar.py ferramentas/logo/v-do-dono.png assets/vitral-v.png` e `python3 -I
  ferramentas/logo/icones.py ferramentas/logo assets/vitral-v.png assets [folha.png]` (a folha com
  todos os tamanhos lado a lado, para conferir).
- **Botão do menu.** No alto da lista, o quadrado do logo é metal líquido: a malha viva
  (`malha_viva`, em `ui/componentes/malha.rs`, a mesma receita calculada ponto a ponto, um
  quadrado por pixel do botão, com o grão exato) e o V por cima (`assets/vitral-v.png`, o
  `LOGO_V` de `src/ui/trilho.rs`). Com o ponteiro em cima ou o menu aberto a malha anda a 20
  quadros por segundo e a luz segue o ponteiro; fora disso acompanha a fase do metal da janela
  (parada em repouso).

## Painel da direita (Ctrl+J)

Começa fechado, para a conversa ficar com a largura toda. Abre e fecha pelo Ctrl+J ou pelo item do
**⋯** do último painel (**Mostrar Este chat** ou **Mostrar o arquivo aberto**, marcado quando ele
está aberto); aberto ao lado, o
cabeçalho dele tem o próprio botão de fechar (o ícone de recolher). O que você escolheu fica
lembrado (`painel` no `interface.json`). Ele também abre sozinho quando você clica num arquivo
citado na conversa, sem mudar essa escolha.

**Menções** (`src/ui/mencao.rs`). No seu balão, a menção de um arquivo ou pasta que existe
("@src/ui/fila.rs#L120-134", "@central/src/") vira uma pílula no meio do texto, no poço escuro do
balão: o ícone do tipo, o nome curto em mono e as linhas em tom tênue, com o caminho inteiro na
dica. O clique abre o arquivo à direita, com as linhas escolhidas (dá para mencionar de novo); a
pasta abre no gerenciador de arquivos. É um plugin do texto do kit (`PilulaDeMencao`) que pega os
links `vitral:mencao/<linhas>/<caminho>` que `ligar_mencoes` põe no lugar das menções.

**Arquivos citados** (`src/ui/links_arquivo.rs`). Na resposta do assistente, o arquivo que está no
disco vira link: `produtos/views.py`, `src/motor.rs:120` entre crases, o link do markdown para um
caminho
(`[a janela](src/ui/mod.rs#L120)`) e, no texto corrido, a palavra com barra e extensão. O clique
abre o arquivo aqui, rolado até a linha, que fica marcada; uma imagem abre no visor. O caminho
relativo parte da pasta do chat. Só abre o que está numa pasta que o Vitral conhece (a de algum
chat); o resto avisa e fica para o VS Code. As linhas vêm de `arquivo:12`, `arquivo:12:5` (a
coluna fica de fora), `arquivo:12-20`, `arquivo#L10` e `arquivo#L10-L20`: com duas, o arquivo abre
com elas marcadas, como a menção de um trecho (`vitral:arquivo/10-20/<caminho>`). O
`file:///caminho` de um arquivo que existe também vira link (o `%20` do endereço é desfeito). O
caminho que não existe como foi escrito é procurado pelo fim dele entre os arquivos da pasta do
chat (a lista do "@", lida em segundo plano quando faz falta): `mensagens.rs:120` abre o
`src/ui/mensagens.rs` se só um arquivo termina assim; com dois (`mod.rs`), fica como texto
(`Ligador` e `pelo_fim`). **Na saída de um comando** (a caixa de `componentes/codigo.rs`, no Bash e
no resultado das outras ferramentas), `src/x.rs:120:5` (cargo, vitest, go), `src/x.ts(12,5)` (tsc)
e `"x.py", line 12` (Python) abrem o arquivo na linha, e o ponteiro parado no link mostra o
caminho inteiro; sem a linha, só o caminho com barra e extensão (o `git status`). Só as últimas 40
linhas da saída são conferidas (as que a caixa mostra fechada), e a resposta fica guardada por 5 s
(`links_da_saida`, com teste). **O comando de barra citado** em código em linha (`/compact`), quando
está na lista de comandos do chat (a do menu "/"), vira link (`vitral:comando/<chat>/<nome>`): o
clique põe o comando na caixa do painel em que a conversa está, com o rascunho que estava lá como
argumento, e quem envia é você. No texto da resposta o kit não tem como mostrar uma dica ao parar o ponteiro num link: a
dica com o caminho inteiro só existe na saída de comando.

**Este chat** (`src/ui/este_chat.rs`) abre com a seção **Mudanças**: os arquivos que o chat editou
ou criou, separados em **Neste turno** (desde a sua última mensagem) e **Antes**, cada um com o
ícone do tipo, o nome, a pasta em tom tênue, o +N −M e o selo **novo** (o Write que criou o
arquivo). O clique abre o arquivo aqui, já na edição (todas as chamadas que mudaram o arquivo,
somadas). O +N −M é contado quando a chamada chega, com o texto
inteiro, e fica gravado no item (`Mudanca`); as chamadas de antes disso contam pelo detalhe
gravado, que é cortado. A lista sai das chamadas de edição: o que um comando mudou fica de fora,
e a nota no pé diz isso. Embaixo, o cartão de onde o chat trabalha. Diz se ele mexe direto na pasta, se é
um chat Sem pasta (os arquivos que ele criar ficam na pasta só dele) ou, numa conversa trazida do
Claude Code, de onde ela veio, as datas e quantas mensagens tinha; mostra a pasta (o caminho
inteiro na dica, com copiar) e **Abrir pasta**. Com a pasta sumida do disco (disco desmontado,
pasta renomeada), um alerta vermelho no lugar do Abrir. Na dica da aba, a pasta e o número do
painel. A outra aba é a do arquivo aberto (ver **Arquivos citados**, acima, e a aba Arquivos da
lista).

**Na pasta e o ramo do git** (`src/ui/git_da_pasta.rs`, `src/ui/ramo.rs`). Depois das Mudanças, a
seção **Na pasta** lista o que o git vê mudado desde o último commit no repositório do chat (o que
um comando mudou também), relida a cada 4 s enquanto está à vista. O cabeçalho dela mostra o
**ramo** em que a pasta está: o nome em mono, o commit curto e "cabeça solta" quando não há ramo, e
o selo **worktree** quando a pasta é uma cópia de outro repositório. O ramo também vai na dica da
linha do chat na lista (junto da pasta) e na dica da aba, numa linha a mais ("No ramo feat/x, num
worktree"), lido quando a dica aparece.

- O ramo sai dos arquivos do git (`git::ramo_no_disco`: o `.git` da pasta ou de uma acima e o
  `HEAD` dele; num worktree, o `.git` é um arquivo que aponta a pasta de verdade, e o `commondir`
  lá dentro diz que é cópia). Não abre processo, então serve para a dica e para conferir a cada
  poucos segundos. O `retrato_da_pasta` usa a mesma leitura e cai no `git symbolic-ref` só no
  repositório que os arquivos não contam (o formato reftable).
- Não há aviso quando a pasta troca de ramo (dono, 18/11/2026: nada de avisos novos).
- **Ignorar espaço em branco.** Com o diff de "Na pasta" aberto, o **…** do arquivo tem
  **Ignorar espaço em branco** (marcado quando ligado): o diff é refeito com o `-w` do git
  (`git::diff_do_arquivo`), e o que só mudou de recuo ou de espaço no fim da linha sai. A faixa
  diz "sem espaço em branco" (um funil, com a frase na dica, quando ela não cabe) e, se não
  sobrou nada, "Só mudou espaço em branco". Começa
  desligado e vale até fechar o Vitral (`Janela::diff_sem_espaco`).

**Nesta conversa** (`src/ui/sumario.rs`). O Este chat abre com o sumário da conversa (com duas
mensagens suas ou mais): uma linha por mensagem sua, com a hora e o começo do texto em até duas
linhas, e embaixo o que o turno dela fez ("3 editados · 12 comandos", a conta de
`falas::o_que_o_turno_fez`). Numa conversa de vários dias, o dia entra como um título pequeno
onde ele muda. A lista tem altura de uns 230 px e rola por dentro.

- **O clique leva à mensagem.** O painel passa a desenhar até ela (o `mostrar` do painel cresce:
  é o mesmo que o subir até o alto faz aos poucos) e a abertura na âncora
  (`AlvoDeAbertura::Ancora`, em `mensagens.rs`) a põe no alto, abaixo do lugar da mensagem presa;
  ela fica realçada por 1,6 s, com a moldura e a tinta do destaque.
- **A mensagem à vista fica marcada**: a última sua que começa até 120 px abaixo do alto da
  conversa (a que você está lendo, ou a que o clique acabou de trazer). A rolagem da conversa só
  redesenha a janela quando a marcada muda (`acompanhar_sumario`), e a lista do sumário rola até
  a linha dela.
- O que veio do histórico de uma conversa trazida do Claude Code não está nos itens do chat e
  fica de fora do sumário.

## Contas e uso dos planos

No canto de cima, à direita, da janela, logo abaixo do ×, flutua um anel de 30 por serviço, um em cima do
outro, cada um num disco opaco da peça (`uso_no_canto`, em `src/ui/uso_plano.rs`): cada um só com a conta daquele serviço conectada (o do Codex,
também com uso lido). Cada um é o disco da marca com **um** anel em volta (com a fenda
embaixo), que mostra a janela mais apertada das duas (5 h ou semana), na cor do serviço até 69%,
âmbar de 70 a 89% e vermelho daí para cima; no limite, o anel fica cheio em vermelho. As duas
porcentagens ficam na dica e no popover. Com mais de uma conta, o do Claude leva a inicial da
conta em uso. **Uso do plano no canto da janela**, em Configurações > Aparência
(`Config.uso_no_canto`, ligada por padrão), esconde os anéis; o uso continua em **Contas do
Claude**. Sem o Codex instalado, o anel dele não aparece: **Instalar o Codex…**, no menu do logo,
abre o terminal com o instalador oficial e o login. O clique num anel abre o popover de detalhes,
para cima, alinhado à direita: o plano, as duas janelas, cada uma com um anel de 44 que enche ao
abrir, a porcentagem no meio (em âmbar ou vermelho a partir de 70%) e, ao lado, o rótulo e a hora
em que renova ("renova em 2 h 13 min · 15:59"; uma janela sem leitura fica só com o rótulo e "sem
dado", sem o anel), os limites semanais por modelo, os
créditos extras do mês e, no rodapé, quando o número foi lido, com **Atualizar** (e, discreto,
"não deu para atualizar" embaixo quando a última busca não trouxe número). O uso do Claude se
atualiza sozinho, direto da Anthropic, com o mesmo pedido que o Claude Code faz para o `/usage` e o
login de cada conta: logo ao abrir o painel e a cada minuto com ele aberto, a cada 5 min com ele
fechado (os anéis do canto também mostram), a cada 15 min com a janela sem foco há mais de 15 min
(30 min depois de 2 h fora) e na hora com **Atualizar**. O token é lido do `.credentials.json` da
conta a cada busca e só vai para `https://api.anthropic.com`; vencido ou recusado, o Vitral não o
renova (quem renova é o Claude Code) e espera, e a falta de rede e o excesso de pedidos fazem
esperar mais antes de tentar de novo, sem aviso. Contam também o que o Claude Code e o Codex
guardam no disco (relido a cada 30 s) e os avisos de limite durante os turnos; o Codex só por
esses dois caminhos.

**Previsão de quando o limite acaba** (`src/contas/previsao.rs`). O motor guarda na memória, por
conta, as leituras cruas do uso (`Motor::historico_uso`, alimentado em `juntar_uso_da_conta` e no
`Evento::Limite`), e a inclinação entre a mais velha do trecho recente e a última diz quando a
janela chega a 100%. No popover do Claude, embaixo de "renova em…", cada janela ganha uma linha:
"No ritmo de agora, chega a 100% às 15:10", em âmbar, quando isso cai antes da renovação, ou "Dá
até a renovação". Quando a previsão das 5 h cai antes da renovação, o anel do canto fica âmbar
mesmo abaixo de 70% e a dica dele diz a hora. O trecho é de 30 min nas 5 h (com pelo menos 5 min
entre as leituras) e de 6 h na semana (pelo menos 30 min): meia hora de trabalho pesado esticada
por dias diria sempre que a semana estoura, e por isso a semana também não pinta o anel. Sem
leituras bastantes, com a última leitura mais velha que o trecho, logo depois de a janela renovar
(as leituras da janela anterior não contam) ou no limite, a linha não aparece. As leituras não vão
para o disco: depois de abrir o Vitral a previsão leva alguns minutos para aparecer.

**Onde o uso foi** (`src/motor/gasto.rs` e `src/ui/onde_uso.rs`). Cada turno terminado vira uma
linha num livro fora do estado, `gastos/AAAA-MM.jsonl` na pasta de dados (um arquivo por mês,
gravado inteiro pelo gravador do `dados.rs`): a hora, o chat, a pasta, o modelo, a conta, o custo
e os tokens de entrada nova, de cache lido, de cache criado e de saída. O custo é o que o
`Chat::somar_custo` somou naquele fim de turno; os tokens vêm do `result` do Claude Code
(`Evento::Gasto`, de `tokens_do_resultado`: o `model_usage_since_last_result`, que conta os
subagentes, ou, sem ele, o `usage` do turno; nunca o `modelUsage`, que é o total da sessão) e do
Codex (sem custo): no app-server, o `total` do último `thread/tokenUsage/updated` do turno menos o
de antes da primeira chamada dele; no `exec` antigo, o `turn.completed`. Um turno com vários `result` (agentes em segundo plano)
soma os tokens de todos. No diálogo **Contas do Claude**, embaixo das contas, a seção **Onde o uso
foi** tem os seletores **Hoje** e **7 dias** (hoje e os 6 dias anteriores): uma barra por pasta,
da que mais gastou para a que menos, com "182 mil tokens · US$ 0,84"; o clique na pasta abre os
cinco chats que mais gastaram nela, e o clique num chat fecha o diálogo e abre a conversa. As
barras medem o custo; num período só de Codex, os tokens. Na abertura o Vitral lê o mês de agora e
o anterior (o que os 7 dias e as conversas recentes precisam). Só entra o que rodou depois desta
versão: o que veio antes não tem linha no livro.

**Último turno**, no popover do contexto da caixa (`ultimo_turno`, em `src/ui/onde_uso.rs`):
depois dos tokens do contexto, os três números do último turno anotado da conversa (entrada nova,
lidos do cache e saída), um embaixo do outro. Sem turno anotado, a linha não aparece. Embaixo
deles, a linha **Cache**, o cache esfriando: "quente por mais 3 min" (os minutos arredondam para
cima; no último minuto, os segundos) ou "frio" (`linha_do_cache` e `estado_do_cache`, que dão a
frase inteira, "cache quente por mais 3 min"; a palavra "cache" vai na coluna dos rótulos porque
a frase não cabia na dos valores). A conta é o fim do último turno mais a validade do cache do prompt menos
agora (`gasto::cache_restante`), feita quando o popover abre: nada de relógio nem de cor no anel.
A validade vem do `result`: o `usage.cache_creation` separa o que o turno gravou com 5 min
(`ephemeral_5m_input_tokens`) do que gravou com 1 h (`ephemeral_1h_input_tokens`), e vale a do
que mais gravou (na assinatura medida em 23/11/2026, tudo com 1 h). O turno que não gravou nada
usa a última validade que o chat viu, ou a última da mesma conta; sem nenhuma, a linha não
aparece. Também não aparece com o chat trabalhando (o cache está em uso), no Codex e nos turnos
anotados antes disto. O que a linha não sabe: se o próximo turno for por outra conta ou outro
modelo, o cache não serve, quente ou não.

**Estatística do turno** (`Tempos`, em `src/motor/gasto.rs`). A linha do livro guarda também os
tempos, tirados do mesmo `result` (`Evento::Medida`, de `medida_do_resultado`; nomes conferidos
na saída de verdade do Claude Code 2.1.328): `total_ms` é o `duration_ms`, `modelo_ms` o
`duration_api_ms`, `ttft_ms` o `ttft_ms`, `passos` o `num_turns` e `cache_ttl` a validade do
cache em segundos. O `duration_ms`, o `ttft_ms` e o `num_turns` são de cada `result`; o
`duration_api_ms` é o total do processo, como o `total_cost_usd` (dois `result` no mesmo
processo deram 1518 e depois 1943), então o livro soma a diferença para o `result` anterior
(`Livro::somar_medida`), ou o valor inteiro quando ele baixa (o turno trocou de processo). Num
turno com vários `result`, o total e os passos somam e a primeira palavra é a do primeiro. O
`duration_ms` não conta o processo subindo nem a espera pelos agentes em segundo plano entre dois
`result`: por isso pode ser menor que o "Trabalhou" da linha, que é pela hora dos itens.
`espera_ms` é medido aqui: do instante em que o `Processo::iniciar` vai abrir o processo até a
primeira linha da resposta (`claude::primeira_palavra`: o começo de um bloco ou um pedaço dele;
`Evento::PrimeiraPalavra`, uma vez por processo, que não redesenha nada). A espera menos o
`ttft_ms` é quanto custou subir o Claude Code (`Tempos::subida_ms`): num turno de verdade, 2967
ms de espera para 1405 de `ttft_ms`, 1,5 s de subida; no segundo `result` do mesmo processo, 9
ms. É o número que mede uma futura melhoria de manter o processo aberto. Zero não vai para o
arquivo, e as linhas de antes disto são lidas sem os tempos. O Codex não informa nenhum deles.
Limite conhecido: o turno que termina com o processo saindo depois de um fim adiado (os agentes
acabaram sem outro `result`) não passa pelo `registrar_gasto`; os tokens dele ficam para a linha
do turno seguinte, como já era, e os tempos dele se perdem (`Livro::abrir`).

## Organização

- **O alto da lista** (a área vazia arrasta a janela, o clique duplo maximiza e o clique direito
  abre o menu da janela): o logo, no quadrado de metal (clique ou F10 para o **menu do Vitral**, que
  abre à direita, também no modo recolhido, a 12 da margem de cima: no topo, a placa de metal com
  o logo, "Vitral" e, embaixo, a versão, o commit curto (ou o canal no compilado no PC) e quantos
  chats há, "0.7.0 · a1b2c3d · 16 chats", com a origem e a pasta de dados na dica; com uma versão
  nova pronta, logo abaixo, a linha de destaque **Versão nova pronta · Reiniciar** (a versão na
  dica); depois três grupos por assunto, com uma divisória entre eles (`grupos_do_menu`): **Recolher
  a lista** (Ctrl+B; no recolhido, **Mostrar a lista** e **O que precisa de você**, com o selo do
  sino), **Novo chat**, **Novo chat em outra pasta…** e **Importar do Claude Code…**; **Quadro de
  ideias**, **Resumo de hoje** e **Arranjos ▸**; **Contas do Claude**, **Instalar o Codex…** e
  **Instalar o Gemini…** quando faltam, **Configurações**, **Paleta de comandos** e **Atalhos do
  teclado**; com os atalhos de cada um em teclas. Cabe sem rolagem a 150% numa janela de 880 de
  altura, também recolhido e com a versão pronta. Até 24/11/2026 ele também tinha Servidores MCP,
  Memória da equipe, a pasta de trabalho e Abrir a pasta de dados: eles moram nas Configurações
  (Claude Code, Memória, Chats e Atualizações) e na paleta. Com uma versão nova pronta, um ponto
  azul de 8 no canto do logo, com a versão na dica. A versão pronta é relida no link `atual` ao
  abrir o menu e ao voltar à janela (`Atualizacoes::versao_pronta`): o "Reiniciar para a 0.10.0-dev
  .766bc7804b" do print de 24/11/2026 era a pronta guardada ao abrir o app, com outra mais nova já
  instalada por outra abertura, e o Reiniciar já ia para a nova. Ao lado do logo, as abas
  **Conversas | Arquivos** (ver **O que fica à vista**).
- **O pé da lista:** as **Arquivadas**, os agentes rodando e o sino do que precisa de você e,
  embaixo deles, a busca e o "+" (os anéis de uso ficam no canto da janela; ver **Contas e uso dos
  planos**). O número do sino é o mesmo de "Esperando você" (pedidos, erros, paradas que pedem
  Retomar e os pares de chats parados no freio de ciclo), em vermelho se
  algo falhou e âmbar nos outros casos; só com respostas novas, um ponto azul. Quando o número aumenta
  (um pedido, uma parada ou um erro novo), o sino pulsa uma vez com um anel âmbar (vermelho se algo
  falhou); abrir o app, o número cair ou marcar como lido não pulsam, e com as animações desligadas
  nada pulsa. O clique abre **O que precisa de você**: cada linha com o balão do chat e, embaixo, a
  mesma frase e o mesmo sinal da prévia da lista (o ponto na cor e o rótulo: "Permissão · Bash · npm
  test", "Plano para aprovar", "Parada · Vitral fechou", "Erro · …"): os pedidos (com Negar e
  Permitir; o comando com um trecho arriscado, ou comprido demais para a caixinha daqui, mais de 2
  linhas ou 160 caracteres, não se aprova pelo sino: o botão é **Ver o comando perigoso** ou **Ver o
  comando inteiro** e leva à conversa, onde o cartão mostra tudo e o perigoso pede o segurar; um
  plano abre a conversa para ler antes), as paradas (Retomar), os erros (Tentar de novo) e as respostas
  novas (marcar como lida), com o número do painel quando a conversa está aberta; clicar numa linha
  abre a conversa (Ctrl+clique, ao lado). No rodapé, os botões **Marcar tudo como lido** e **Ver na
  lista** (volta à lista, sem arquivadas, e mostra a seção); cabeçalho, seções, linhas e o ícone do
  rodapé ficam na mesma guia, a 16 da borda. Ctrl+G vai à pendência seguinte à do chat
  aberto, achada pelo chat (e não pela posição), e dá a volta depois da última; Ctrl+Shift+G anda
  para trás. Ao chegar, o foco vai para o lugar útil: o **Retomar** da faixa no chat parado, o
  **Tentar de novo** (ou a saída da falha) no erro, o comentário do plano no plano, a caixa nos
  outros casos. A pílula diz onde está ("2 de 3 · Parada · WMS"), ou "Nada esperando você". O
  cabeçalho **Paradas** do sino ganha **Retomar tudo** com duas ou mais (rodada de uso, item 3.4).
- **Ctrl+B recolhido** (ou **Recolher a lista**, no menu do logo). A lista some e a conversa vai
  até a borda. No lugar dela fica o logo, no começo das abas do 1º painel, com o selo do sino (o
  número ou o ponto, que pulsa como o sino); os anéis de uso continuam no canto da janela. O menu
  do logo troca o Recolher por, em cima, **Mostrar a lista** e **O que precisa de você** (abre a
  mesma caixa do sino). Ctrl+B de novo volta tudo.
- **Abas de chat.** Cada painel guarda abas no cabeçalho (`Painel.abas`, em `src/ui/paineis.rs`),
  na ordem em que abriram: clicar num chat da lista abre uma aba no painel ativo e as anteriores
  ficam, até 8 (`MAX_ABAS`; passou disso, sai a mais antiga que não está à vista). Clicar numa aba
  troca o chat do painel (`abrir_aba`); o × ou o clique do meio fecha a aba (`fechar_aba`; a conversa
  continua na lista), e o painel passa a mostrar a vizinha de antes (sem ela, a de depois); fechar a
  última fecha o painel. O Ctrl+W fecha a aba à vista e, na última, o painel. A cada desenho,
  `acertar_abas` põe o chat à vista entre as abas e tira as de chats que sumiram. Um chat só aparece
  num lugar por vez: a vizinha que já está à vista noutro painel não entra. A aba à vista é a
  completa (até 280 de largura, e nunca menos que o símbolo, o número, o × e o começo do título);
  toda aba tem um contorno só, o fio preto de 1 px (a casca preta de 1 px de folga em volta dela
  saiu, dono, 16/11/2026: "diminui as bordas");
  as outras são compactas (até 180), com o título e o caminho na dica, e encolhem antes dela. A de
  fundo que não teria lugar para 4 letras e as reticências fica só com o ícone, num quadrado de
  32 (`Aba::quadrada`); se nem assim a aba à vista tiver o começo do título, as de fundo saem da
  faixa, a mais antiga primeiro, e um **+N** no fim da faixa abre a lista delas (a do histórico,
  só com elas; `jeito_das_abas`, em `src/ui/conversa.rs`, com a largura da faixa medida no quadro
  anterior e a natural de cada aba, conta o lugar do +N). A faixa nunca passa por cima do **+** e
  não rola: a roda do mouse em cima dela não faz nada. Toda
  aba tem fundo, e com o ponteiro em cima ganha um aro de metal. As abas não têm ícone (dono,
  18/11/2026: "tira os ícones daqui também"): o estado do chat vai no fio preto da aba (`Aba::borda`,
  em `src/ui/componentes/paineis.rs`): um fio de 2 px na cor do estado por cima do preto, com um
  véu de 6% da mesma cor no miolo, sem halo. Trabalhando, azul, respirando devagar (de 0,5 a 1 em
  2,4 s, numa vista própria: a batida redesenha só o fio); terminou e você não leu, verde; esperando
  você, âmbar; erro, vermelho; parado e lido, o fio preto de sempre. O dono pediu discreto, mas o
  fio de 1 px apagado não se via ("dá pra ver nada", 18/11/2026). Só a aba de fundo que virou um
  quadrado (sem lugar para o título) continua com o ícone do estado. Na largura mínima, só a aba à
  vista. As abas não são lembradas ao fechar o app.
- **Arranjos de painéis salvos** (`src/ui/arranjo/salvos.rs`). O que a janela já grava ao fechar
  (`Arranjo`: os chats de cada painel, as abas, a aba à vista, a divisão das conversas, as
  larguras e a lista recolhida) pode ser guardado com um nome, um por frente de trabalho
  ("Central", "Vitral"), na chave `arranjos` do interface.json (até 12). No menu do logo,
  **Arranjos**: a lista (o em uso com o visto), **Salvar o arranjo atual…** (Ctrl+Alt+S; o mesmo
  nome salva por cima) e um **Apagar** para cada um. Trocar guarda antes o arranjo em uso, como a
  janela está, para a volta ser de onde parou, fecha os painéis de agora (o rascunho de cada caixa
  fica guardado, e as abas não vão para o Ctrl+Shift+T) e recoloca os do escolhido pelo mesmo
  `reabrir_arranjo` da abertura; o tamanho da janela não muda. **Ctrl+Alt+PageDown** e
  **Ctrl+Alt+PageUp** andam pelos arranjos, dando a volta, e a paleta de comandos tem um "Arranjo:
  nome" para cada. Chat arquivado ou apagado some do arranjo sem erro (`Arranjo::reabrivel`); se
  não sobrou nenhum, a troca avisa e não acontece. Os chats que saem da tela continuam rodando.
  Depois de fechar e abrir, a janela volta como estava (a chave `arranjo` de sempre) e o "em uso"
  continua o mesmo.
- **Conversas lado a lado** (até 4, conforme a largura): arraste uma conversa da lista, ou qualquer
  aba de um painel, como no VS Code: solta no **meio** de outro painel, ela vira mais uma aba dele
  ("Abrir aqui, como aba"; sai do painel de origem, que fecha se era a única aba); solta na
  **borda** esquerda ou direita de um painel, abre ao lado. A aba que sai de um painel com outras
  abas, solta na borda de qualquer painel (até do próprio), vira um painel novo daquele lado e
  deixa as outras no de origem; a única aba de um painel, solta na borda de outro, muda o painel de
  lugar ("Mover para cá"). A lógica é a `efeito_de`, em `src/ui/paineis.rs`, com teste. A forma diz
  o que vai acontecer: meio painel em azul para "Abrir ao lado", uma barra na borda para "Mover para
  cá" e o painel inteiro para a aba (com "Já há 4 lado a lado" quando não cabe outro); a aba que
  você arrasta fica apagada e tracejada. Também dá com Ctrl+clique ou o botão "abrir ao lado" no
  cartão. O painel que abre ao lado entra com um fade (a largura não anima). Com vários painéis,
  cada um tem um número em forma de tecla (na aba, na lista e na direita), Alt+1…4 vai para ele, e
  o selecionado é marcado pela **luz de baixo** (a doca de metal no pé do cartão, ver
  **Acabamento**), que acende suave (180 ms; apaga em 120); clicar na aba (ou em qualquer parte do
  painel) ativa o painel e põe o cursor na caixa dele. O estado do chat vai no contorno da aba (azul
  respirando rodando, âmbar pedido ou parada, vermelho erro, verde novidade). O × aparece por cima
  do fim da aba, sem guardar lugar, no hover e no foco (segurar a aba já dá o foco); a aba só com o
  símbolo não o mostra (o clique nela trocaria de aba fechando), e fecha pelo clique do meio. Fechar
  pelo × ou pelo clique do meio põe o cursor na caixa do painel que fica, como o Ctrl+W. A seleção
  de texto fica no painel em que começou: cada um tem o seu escopo (`Painel.escopo_selecao`), que o
  clique ativa (`conversa::aplicar_escopo_do_clique`, com o `definir_escopo_de_selecao` do
  gpui-component vendorizado). A aba entra no Tab: Enter vai para a caixa, Delete fecha e Shift+F10 abre o menu.
  As divisórias não têm linha fixa: o indicador aparece ao passar o mouse, e é só arrastar para
  redimensionar; **dois cliques** na divisória entre duas conversas deixam todas com a mesma
  largura (`igualar_conversas`, em `src/ui/mod.rs`). A lista da esquerda recolhe pelo menu do logo
  (**Recolher a lista**) ou pelo Ctrl+B, e no arrasto vai de 200 a 560 px (`FAIXA_LISTA`).
  Ctrl+PgUp/PgDn vão para o painel vizinho e Ctrl+Shift+PgUp/PgDn trocam o painel de lugar.
- **Cabeçalho do painel.** Uma linha só (44): as abas e o **+** (dono,
  16/11/2026: "faz esse botão ser o de + de criar mais uma conversa e ter o histórico também, e
  tira o de 3 pontinhos e deixa só no botão direito"). O **+** (`botao_historico`, em
  `src/ui/conversa.rs`, dica "Novo chat ou histórico") abre uma lista com busca ("Buscar chat…",
  ↑/↓ e Enter): a primeira linha, já marcada, é **Novo chat neste painel** (o Enter cria o chat
  novo aqui, como aba, na mesma pasta que o "+" da lista escolheria); embaixo, todos os chats, do
  mexido por último ao mais antigo, cada um com o ícone do estado, a hora e a pasta, e os
  arquivados num grupo no fim. Digitar filtra a lista (sem acento e por palavras soltas, como a
  busca da lista: ver abaixo), e a linha do chat novo só fica se casar com o texto dela ("novo"). O chat à vista diz "à vista neste painel"; escolher outro mostra ele
  neste painel, como uma aba (o que já está à vista noutro painel diz "no painel 2" e só ativa
  aquele painel). O botão fica também no painel estreito. O **⋯** do cabeçalho saiu: o **botão
  direito** no cabeçalho (na faixa das abas e no vazio dela) abre o menu do painel onde o mouse
  está, e na aba de fundo o mesmo menu, com as ações da conversa valendo para aquele chat;
  **Shift+F10** e a tecla **Menu** abrem o mesmo menu do painel ativo, ancorado onde ficava o
  **⋯** (`menu_pelo_teclado`). O menu tem **Modo foco**, no último painel o item da direita
  (**Mostrar Este chat** ou **Mostrar o arquivo aberto**, marcado quando ela está aberta, com o
  Ctrl+J), **Nova conversa ao lado** e o menu da conversa (renomear, fixar, arquivar). Na aba, o
  balão do chat, com a pasta na dica ("Direto em ~/Projetos/loja", "Sem pasta: um chat numa pasta
  vazia só dele"). O custo ("US$ 0,42") está no anel do contexto da caixa. Não há linha entre o
  cabeçalho e as mensagens, nem com a conversa rolada. Em painel estreito o modo foco vai para o
  menu do painel; no mais estreito, com vários painéis, o número toma o lugar do símbolo da aba.
  Nenhum texto é cortado no meio.
- **Painel sem conversa:** "Comece um chat" com **Novo chat** (na última pasta usada) e **Em outra
  pasta…**; dá para soltar ali uma conversa arrastada da lista.
- **Zoom:** Ctrl + roda do mouse (ou do touchpad), Ctrl+= / Ctrl+- e Ctrl+0 (volta a 100%)
  aumentam e diminuem a interface inteira de 10 em 10%, de 70% a 200%, como num navegador; a
  porcentagem aparece no alto da janela por um instante. Tudo cresce junto, inclusive as larguras
  das partes: a 150% a lista normal tem 468 px, a direita 570 e cada conversa pede no mínimo 420,
  então numa janela que não comporta a direita e depois a lista viram gaveta, como numa janela
  estreita. O que você arrastou nas divisórias fica guardado a 100% e acompanha o zoom (a lista
  de 240 a 100% vira 360 a 150%, e a divisão entre as conversas se mantém). Vale para o app todo
  e fica guardado em `~/.config/vitral/interface.json`.
- **Visual:** o fundo é a chapa de metal líquido (ver **Acabamento**), opaca: o papel de parede não
  aparece mais. A janela inteira é a base, com a chapa debaixo de um véu azul-escuro, e a lista
  mora direto nela; a conversa e os painéis da direita são cartões que flutuam por cima, com
  sombra e a aresta de vidro (fio nos lados, luz na quina de cima, sombra na base e o brilho do
  topo); o seu balão e os cartões da direita e de entrega, a linha de luz no topo. Os diálogos e as
  gavetas são opacos; o preto entra no que vai por cima e por dentro, para dar profundidade:
  popovers, menus, dicas e avisos são quase pretos e opacos (`flutuante`, com a borda clara fina e
  a linha de luz no topo); os blocos de código, as saídas de comando e a
  moldura da caixa de mensagem ficam num preto mais fundo (#08090c), com a
  mesma cor de sintaxe; e os anéis de uso ficam sobre o disco opaco da peça, para a cor de cada marca
  saltar. Dentro de um popover, o que no vidro é um recuo escuro (o trilho do segmentado do
  esforço) vira um claro leve. O logo (o V sobre o quadrado de metal; ver
  **Acabamento**) vai no tamanho completo (`assets/vitral.svg` e os PNGs de 64 a 256 px) no menu de
  aplicativos e na tela de recuperação, e no pequeno (`assets/vitral-pequeno.svg`, declarado em 32
  px, e o PNG de 48) nos ícones pequenos. Popovers e menus entram com um fade e 6 px
  vindos do lado do botão que os abriu, e todos os flutuantes têm uma linha de luz de 1 px no topo,
  como o reflexo de um vidro; as gavetas da lista e da direita entram deslizando 24 px da borda, em
  180 ms, com o véu em fade junto, e fecham secas. O véu dos diálogos cobre só a janela, não a
  margem da sombra. A pílula do segmentado (Conversas · Arquivos, o esforço) desliza até a
  opção nova; no segmentado neutro ela é sempre a ficha de metal, também nas Configurações e no
  boas-vindas (a pílula tingida fica para o segmentado de outra cor); os botões redondos (enviar, +, sino) acendem suave com o ponteiro em cima; os anéis de
  uso e de contexto deslizam até o valor novo; o sino pulsa quando chega algo que precisa de você;
  e o ponto de "rodando" respira enquanto o compasso dos spinners e do texto com brilho (o da
  linha de estado) bate. Listas, menus e cartões continuam com o
  hover seco. O âmbar é só do que precisa de você. Com o ponteiro fora da janela, nada fica em
  hover nem com a dica aberta.
- **Janela estreita:** quando falta largura, o painel da direita e depois a lista saem do lado e
  abrem por cima, como gaveta (Ctrl+J, Ctrl+B, Ctrl+K e os botões de sempre); Esc ou um clique
  fora fecham. A gaveta entra deslizando da borda dela, com o fundo escurecendo junto; fecha seca.
  Os botões de janela seguem o layout do GNOME (no Fedora, só o ×).
- **Lista**, direto na base da janela (sem cartão próprio): a lista corrida de **O que fica à
  vista** (o que espera você, o que roda, as fixadas e todos os outros, uma linha por chat). A busca
  (Ctrl+K; acha no título, no texto das mensagens e no nome da pasta, também nas conversas
  arquivadas) e o **+** ficam no pé da lista. Um ponto âmbar em Conversas avisa, de dentro de
  Arquivos, que algo espera você. Tudo na lista começa a 16 px da borda da peça (o balão, a lupa da
  busca e o primeiro ícone do rodapé). A busca diz só "Buscar"; o que ela acha e o Ctrl+K ficam na
  dica do campo (que não aparece enquanto se digita nele). Na busca, quando o termo não está no
  título, a linha de baixo mostra a pasta e o trecho que casou ("loja · Você: valide o **frete**
  grátis…", com o termo em forte), e as arquivadas que casam aparecem no fim, numa seção
  **Arquivadas** (com o número) e com o cartão tênue. Escrever num chat arquivado (ou o **Entregar
  agora** da fila dele) o tira das Arquivadas, e a mensagem entra na hora; antes ela esperava na
  fila até desarquivar à mão (dono, 19/11/2026). Fora da busca, elas continuam só em
  Arquivadas, que tem os títulos das faixas de data (**Recentes**, até 7 dias, e **Mais antigas**,
  recolhida no começo). Estreitando a lista (ou com o zoom alto, porque a conta é a mesma), ela abre
  mão do que é menos importante antes de cortar o título: abaixo de uns 290 px (a 100%), no hover
  fica só o **…**; abaixo de 275, as abas ficam só com o ícone e a prévia fica com o rótulo curto do
  motivo; abaixo de 260, a busca de arquivos diz só "Buscar" e **Retomar tudo** fica só com o
  ícone. Os horários continuam à vista. A barra de rolagem é fina, colada na borda da peça, não
  cobre os cartões e aparece e some sem fade, 1,2 s depois de rolar.
- **Esperando você** (o número é o mesmo do sino: pedidos, erros e paradas; respostas novas não
  contam, viram ponto) começa pelos pares de chats parados no freio de ciclo, cada um com **Ver**;
  pedidos, paradas e erros vêm antes das respostas novas, e com duas paradas ou mais aparece
  **Retomar tudo**. Os cartões são neutros como os outros (selecionado em azul); o âmbar fica no
  ponto do balão e no da prévia. O **Arquivar** do menu do chat mostra "Conversa arquivada" com
  **Desfazer** (um chat que nunca rodou sai do Vitral: "Chat vazio apagado"); desarquivar não avisa.
- **Cartão de conversa**: uma linha só de 32 (o balão de 20 com o ponto do estado no canto, o
  título, em 400, a hora, o alfinete da fixada, o ponto de não lido e o número do painel; a pasta
  na dica do balão; o não lido sai em 600, dois degraus acima); a linha de baixo, com a prévia, volta só na busca que achou no texto. A hora sai em
  metadado tênue de dígitos iguais ("3 min", "5 d", com espaço fino que não quebra), clara na linha
  selecionada. Chat aberto num painel mostra só o número do painel, sempre na mesma coluna, e a hora
  vai para a dica do balão, junto da pasta: com três painéis a lista fica estreita e o título precisa
  do lugar. O número é uma tecla no recuo com borda; a do painel ativo tem a borda e o algarismo em
  azul (o disco azul cheio é só dos avisos, no sino e no logo). O ponto:
  vermelho erro, âmbar pedido ou parada, azul resposta nova, aro azul rodando. No hover, o horário
  dá lugar a duas ações: **Abrir ao lado** (ou Retomar) e **…**; fora do hover, o título usa o lugar
  delas, e só o cartão sob o ponteiro (ou realçado pelo teclado) corta o título para mostrá-las. O
  clique direito ou o **…** abrem o menu do chat: abrir ao lado, renomear, fixar e arquivar. O
  clique simples abre o chat numa aba do painel ativo; clique duplo renomeia; Ctrl+clique ou o
  clique do meio abrem ao lado.
- **Teclado na lista**: ↓ na busca entra na lista; ↑/↓, Home e End andam pelos cartões; Enter abre
  (e, com pedido, põe o cursor na caixa), Ctrl+Enter abre ao lado e põe o cursor na caixa do painel
  novo, F2 renomeia e Esc volta à busca. Uma letra digitada com a lista em foco (depois de clicar
  num chat) vai para a caixa do painel ativo, que fica com o cursor. O mesmo vale com o foco na
  conversa (depois de abrir uma trilha, selecionar para copiar ou clicar no "Hoje") e numa aba:
  a letra entra na caixa do painel ativo, e o Ctrl+V cola nela (um print copiado vira pílula sem
  clicar antes). O Espaço e o Enter continuam do botão focado, e o "?" abre os atalhos. Na busca, com texto, o
  primeiro resultado já vem marcado, mesmo com o cursor no campo; ↑/↓ movem a marca sem tirar o
  cursor (refinar o termo continua sendo digitar), Enter abre o marcado (Ctrl+Enter, ao lado) e
  põe o cursor na caixa dele, e F2 o renomeia. Com o campo vazio, ↓ leva à lista. Esc limpa e,
  vazia, volta à caixa do painel. O Ctrl+K seleciona o termo anterior, para a busca nova o trocar.
  Com a lista recolhida (Ctrl+B), o Ctrl+K a abre como gaveta por cima dos painéis, com o cursor
  na busca: abrir um chat a recolhe sozinha, e Esc a fecha (o termo fica). Ela não volta fixa nem
  espreme os painéis, e o Ctrl+B continua igual (rodada de uso, item 4.1). No histórico do "+", o
  que espera você (pedido, parada, erro) vem num grupo "Esperando você" no alto. Se a lista sai de
  vista com o foco nela (um painel novo a empurrou, Ctrl+B, a janela estreitou), o foco vai para a
  caixa do painel ativo.
- **Busca sem acento e por palavras** (`src/ui/acentos.rs`, rodada de uso, item 4.3). Na lista e no
  histórico do "+", a busca ignora acento e maiúsculas dos dois lados e separa o termo em palavras:
  aceita o chat em que cada palavra aparece no título, na pasta ou numa fala, em qualquer ordem
  ("integracao" acha "integração"; "vip cupons" acha "Cupons para grupos VIP"). Primeiro vêm os que
  casam no título, depois os que casam só no texto, e dentro de cada grupo a ordem é por data. O
  realce marca cada palavra no texto original. O texto preparado de cada chat fica guardado e só é
  refeito quando os itens mudam. A peça que tira o acento é a mesma do Ctrl+F da conversa.
- **Rodapé**, só quando há o que mostrar: **Arquivadas** (o ícone, com o número na dica; liga e desliga a
  vista das arquivadas; a busca acha as duas) e, com algo rodando, quantos agentes rodam no Vitral
  inteiro ("2 rodando"; o clique volta ao topo da lista). Na aba Arquivos, quantos arquivos a pasta tem.
  Um fio em cima e 4 de respiro o separam da última linha da lista.
- Aba **Arquivos** (o @ e o **…** de cada arquivo só ocupam lugar no hover da linha, como no
  cartão de chat, e o nome vai até o fim fora dele): os arquivos da pasta do chat aberto numa árvore (pastas recolhidas, menos o
  caminho até o arquivo aberto), com o caminho dela em cima e o ↻ para reler. Com busca, uma lista
  plana sem limite, com o trecho achado em destaque e a pasta cortada no meio. Clicar abre no painel
  da direita: a aba com o nome e o ícone do tipo, e uma barra com a pasta, o número de linhas e a
  linguagem, **Mencionar**, **VS Code** e o **…** (copiar o conteúdo ou o caminho, abrir fora, mostrar na pasta).
  O **cabeçalho do arquivo** (`cabecalho_do_arquivo`) tem o ícone e o nome, as linhas e a
  linguagem ("59 linhas · JavaScript"), a pasta em tom tênue cortada no começo e, à direita, as
  ações. Nada passa por baixo de nada: a conta (`o_que_cabe`) é pela largura do painel e pelo
  tamanho dos textos, e o que não cabe sai inteiro (primeiro os rótulos das ações, que ficam só
  ícone com a dica, depois as linhas e a linguagem); o nome é o último a encolher. Com os anéis de
  uso ligados, as fileiras de cima do painel (o cabeçalho e a faixa da edição) guardam o lugar
  deles na ponta direita (`reserva_dos_aneis`); o conteúdo não. **Mencionar** põe na caixa a
  menção do arquivo, como pílula; com linhas escolhidas, a delas ("@x.rs#L12-18"); na edição, a
  da mudança à vista (o **…** tem **Mencionar o arquivo inteiro**). Um clique no número da linha
  escolhe a linha (o segundo a desfaz) e Shift+clique estende a escolha; as escolhidas ficam em
  azul, e o cabeçalho diz quais são ("linhas 12 a 18"), com o × que desfaz. Aberto pela edição
  (ver **Ver a edição**, em Ferramentas), o arquivo ganha a faixa da edição embaixo do cabeçalho:
  "Edição", o +N −M e "2 de 5", as setas que passam de uma mudança à outra e o ×
  que volta ao arquivo sem as marcas. **Rever o turno em sequência:** aberto pelas Mudanças de um
  chat (ou por uma edição da conversa) com mais de um arquivo na lista, a faixa diz também
  "arquivo 2 de 8" ("2/8" quando a frase não cabe na faixa, como no painel na largura mínima com
  os anéis de uso no canto; a frase vai na dica), e a seta, na última mudança, passa ao arquivo
  seguinte da lista em que ele está ("Neste turno" ou "Antes"), na ordem dela; a de cima, na
  primeira mudança, volta ao arquivo anterior, na última mudança dele. Nas pontas da lista, dá a
  volta. A dica da seta diz se o próximo passo é outra mudança ou outro arquivo
  (`passo_da_revisao` e `edicao::lugar_na_revisao`, com teste). Fora de uma lista (o diff de "Na
  pasta") ou com um arquivo só, as setas dão a volta dentro do arquivo, como antes; com uma
  mudança só e sem outro arquivo, a faixa não tem setas. As linhas que entraram ficam com o fundo verde, a faixa
  verde na borda e o "+"; as que saíram aparecem entre elas, sem número, em vermelho, com o "−"
  (o markdown sai em linhas nesse modo). Cada trecho da edição é achado no arquivo de agora pelo
  lado novo (o contexto e o que entrou; no diff do Codex, perto da linha que ele diz); o que não
  se acha mais (o arquivo mudou depois) aparece solto no fim, com uma nota. O painel abre rolado
  até a primeira mudança. O registro antigo, sem o diff, abre o arquivo como está, com o aviso
  "Este registro não guardou a edição" na faixa.
  **Editar no painel** (`src/ui/editor_arquivo.rs`). Um clique no texto do arquivo abre o editor
  por cima da leitura, com o cursor na linha e na coluna do clique e a mesma rolagem; o número da
  linha continua escolhendo linhas para mencionar. É o editor de código do kit (`EditorState`, com
  a cor da sintaxe, o desfazer e o recuo que o arquivo usa: `recuo_do_texto`), sem quebra de linha,
  como a leitura. O texto **se grava sozinho 800 ms depois da última tecla** (`Agenda`) e o
  **Ctrl+S** grava na hora (contexto `Arquivo`). No cabeçalho, o lápis toma o lugar do símbolo do
  arquivo, "editando" o das linhas e da linguagem, um ponto âmbar marca o que ainda não foi para
  o disco (vermelho se a gravação falhou, com o erro na dica) e o × (ou o **Esc**) volta à
  leitura, gravando o que falta. Abrir outro arquivo, fechar a aba, a Prévia do .md e fechar o
  Vitral também gravam antes. O Ctrl+F continua o da barra do painel: ele grava, volta à leitura
  e busca (o editor do kit não tem busca nem substituir aqui). Na edição do agente (a faixa
  "Edição"), o clique numa linha do arquivo sai dela e abre o editor ali. Só entra no editor o
  texto em UTF-8 de até 400 KB (`Sessao::abrir`): o arquivo em outra codificação avisa e fica só
  para ler (gravar daqui trocaria as letras dele), a tabulação e o fim de linha do Windows voltam
  ao disco como vieram, e o **arquivo vazio** tem o botão **Escrever nele**. O texto do editor
  fica onde estava o da leitura (o painel compensa as margens do campo do kit, que não crescem
  com o zoom); os números de linha ficam 10 px mais perto do texto. A calha dos números é pintada
  por cima do texto que rola para o lado, e o tema do Vitral a tem transparente: o editor recebe
  a cor opaca da superfície em que está (`gutter_background`, mudança marcada no kit de `vendor/`).
  **Mudou no disco.** Com o editor aberto, o arquivo é conferido a cada segundo (a data e o
  tamanho; o conteúdo só quando eles mudam: `Sessao::conferir`). Se o agente grava o arquivo e
  você não tem nada por gravar, o editor passa a mostrar o texto novo, com o cursor e a rolagem
  onde estavam. Com texto seu por gravar, aparece a faixa âmbar **"O arquivo mudou no disco."**,
  e nada é gravado até você escolher: **Recarregar** fica com o do disco (o seu se perde) e
  **Manter o meu** fica com o seu, que passa por cima na gravação seguinte (os 800 ms de sempre,
  ou o Ctrl+S). A gravação também confere o disco antes de escrever, para o caso de o agente ter
  gravado entre duas conferências. O **arquivo apagado** enquanto você edita mantém o texto no
  editor, com a faixa "O arquivo foi apagado. O seu texto continua aqui." e o Manter o meu, que
  cria o arquivo de novo. O conflito some sozinho se o disco fica igual ao que você escreveu. No
  painel estreito os botões da faixa descem para a linha de baixo. Sair do editor com um conflito
  sem resposta (ou com a gravação falhando) não grava nada: o que você escreveu vai para a área
  de transferência, e o aviso "Texto não salvo" diz isso.
  **Arquivo grande.** Acima de 400 KB o arquivo abre **só para ler**, até 5 MB
  (`TETO_DE_LEITURA`), com a faixa discreta "Só leitura: arquivo grande (612 KB)" embaixo do
  cabeçalho: em linhas, sem a cor da sintaxe (colorir alguns MB levaria dezenas de segundos), sem
  a prévia do markdown e com a linha de mais de 2 mil letras cortada no desenho (o JSON
  minificado). A escolha de linhas e o Ctrl+F continuam (a busca conta as linhas só com ASCII
  sem dobrar o texto: `achados_na_linha`). Medido na bancada, no binário de depuração, com um log
  de 5 MB e 99,8 mil linhas: um quadro de 0,5 s ao abrir, a rolagem sem custo e até 0,34 s por
  tecla da busca; o binário de release é várias vezes mais rápido. Acima de 5 MB continua o aviso.
  **Prévia | Fonte.** O .md e o .svg têm no cabeçalho o seletor **Prévia | Fonte** (o olho e as
  chaves quando os rótulos não cabem): a Prévia é o markdown renderizado ou o SVG desenhado, e a
  Fonte, o texto em linhas, que um clique edita. Outro arquivo abre na Prévia. Na prévia do .md,
  o **frontmatter** YAML do alto vira uma tabelinha de chave e valor (o nome e a descrição de um
  SKILL.md; `previa_markdown::frontmatter` lê as chaves do primeiro nível, o texto dobrado e o
  literal, a lista e o mapa de dentro, e o que não é uma lista de chaves fica no texto), e os
  **links para arquivos** abrem o arquivo no painel, na linha quando o link a traz
  (`[o carrinho](../loja/carrinho.py#L18)`): o caminho relativo parte da pasta do próprio .md e,
  se não existe ali, da pasta do chat (`previa_markdown::ligar_links`, que usa o mesmo ligador da
  conversa). A prévia é montada uma vez por leitura do arquivo. O link da web abre no navegador; o
  resto (`file://`, um comando de barra) não abre.
  O markdown sai renderizado; o código, com cor e número de linha, rola para os lados. O **SVG**
  abre desenhado, como as outras imagens (o `img` do GPUI o desenha; `claude::imagem_de_ver`), e o
  clique o leva ao visor, no tamanho que a tag `<svg>` diz (`imagens::medidas_svg`: o `width` e o
  `height` ou o `viewBox`; o GPUI desenha o SVG no tamanho de origem, e esticado até a área ele
  borrava) e sobre um cinza médio em vez do preto (`imagens::fundo_da_imagem`: o traço escuro sem
  fundo de um ícone sumia no preto); na edição ele sai em linhas, e anexado segue como arquivo de texto (o
  `tipo_imagem` continua sem ele: a API não aceita SVG como imagem). O **VS Code** do cabeçalho
  abre na linha que está em vista (`code --goto arquivo:linha`, `sistema::argumentos_do_editor`):
  a primeira das escolhidas, a da mudança à vista na edição ou a que o link da conversa marcou; a
  dica do botão diz qual. Arquivo que não dá para mostrar tem um aviso no lugar, com a saída que
  serve (`saidas_do_aviso`): o **binário** (PDF, planilha, zip; o aviso diz o tipo pela extensão)
  tem o **Abrir fora**, que abre no aplicativo padrão do sistema (`sistema::abrir_fora`); o
  **grande demais** (mais de 5 MB) tem **Abrir no VS Code** e **Abrir fora**; o que sumiu,
  **Fechar**. O arquivo grande com um byte nulo no começo é binário, e não "grande demais" (antes
  um PDF de 2 MB só oferecia o VS Code). O **…** do cabeçalho também tem o **Abrir fora**, para
  qualquer arquivo (o .html no navegador, o .csv na planilha).
  **Buscar no arquivo (Ctrl+F).** Um clique no painel do arquivo leva o foco para ele
  (`foco_arquivo`, contexto `Arquivo` no keymap), e aí o Ctrl+F busca dentro do arquivo, e não na
  conversa; com o arquivo na gaveta, por cima das conversas, o Ctrl+F já é dele. Também está no
  **…** do cabeçalho (**Buscar no arquivo**) e na paleta. A barra flutua no alto do conteúdo, como
  a da conversa: o campo, "3 de 12" ("3/12" no painel estreito; "nenhum achado" em âmbar), as
  setas e o ×. O Enter e o F3 vão ao achado seguinte, o Shift com eles ao anterior, dando a volta,
  e o Esc fecha (com a barra aberta, antes de recolher a gaveta ou parar o turno). A busca não
  liga para acento nem para caixa (`acentos.rs`), marca todos os achados em âmbar (o atual, mais
  forte) por cima das cores da sintaxe e rola até o atual, no meio da tela. A barra guarda o lugar
  dos anéis de uso, e as primeiras linhas descem para não ficar atrás dela. Ela é pelas linhas:
  com a barra aberta, o markdown e o SVG saem em linhas, e voltam a aparecer prontos ao fechar;
  na edição, acha nas linhas do arquivo (não nas que saíram). O índice (`IndiceDoArquivo`: a
  linha e a posição de cada achado) só é refeito quando o termo ou o arquivo mudam. Digitar com
  o foco no painel continua escrevendo na caixa; abrir outro arquivo fecha a barra. Não rola
  para o lado até um achado numa linha muito comprida. Na lista, no hover, **@** menciona o arquivo na conversa e **…** (ou o
  clique direito) abre no VS Code, copia o caminho ou mostra na pasta. A lista é relida a cada 20 s
  em segundo plano, sem travar a tela.
  **Mostrar ignorados.** Ao lado do ↻, o olho (fechado por padrão; aberto e em destaque quando
  ligado) põe na árvore o que o git deixa de fora, em tom tênue: é como aparecem `arquivo/`,
  `dados/` e `tarefas/` (prints, exports), que o workspace mantém fora do git. Uma pasta ignorada
  entra só com o nome, e o que há dentro é lido um nível por vez, quando ela é aberta na árvore
  ("Lendo…" até chegar; "Pasta vazia" se não tem nada): `node_modules` e `target` nunca são
  varridos sem pedir. A lista vem de `seletor_arquivos::listar_com_ignorados` (`git ls-files
  --others --ignored --exclude-standard --directory`, descendo nos repositórios de dentro como a
  lista normal; sem git, o que a varredura pula: as pastas ocultas e de dependências), com o
  mesmo teto de 20 mil entradas somando tudo. As pastas abertas ficam em
  `ArquivosLidos::ignoradas_abertas`, e a releitura de 20 s e o ↻ as releem também. A busca da
  aba acha os ignorados que já estão na árvore; o "@", o Ctrl+P e os links continuam só com o que
  o git acompanha. Vale até fechar o Vitral.
- **Configurações** (Ctrl+,), gravando na hora, valem para todos os chats (o caminho do
  `config.json` fica na dica do subtítulo). Desde 23/11/2026 são um diálogo largo
  (`LARGURA_DAS_CONFIGURACOES`) com uma coluna à esquerda das partes (`Parte`: Chats, Agentes,
  Perfis, Memória, Aparência, Claude Code e Atualizações; no diálogo estreito, só os ícones) e, à
  direita, as opções da parte escolhida, que rolam sozinhas numa altura que nunca passa da da
  janela (`altura_do_corpo`: 520 a 100%, ou a janela menos a folga do cabeçalho; a navegação e o
  cabeçalho ficam parados e só a coluna rola). O que mora em cada
  parte (`src/ui/configuracoes.rs`):
  - **Chats:** **Pasta dos chats novos** (a pasta de trabalho, com **Trocar…**, que abre o seletor
    do sistema; `trocar_pasta_de_trabalho`), **Resumo ao voltar depois de**, **Avisar dentro do app**, **Notificação do
    sistema**, com o **Testar**, que manda uma notificação na hora (sem o `notify-send`, a linha diz
    o que falta), **Arquivar chats parados depois de** (nunca, 3, 7 ou 30 dias; só os chats já
    lidos, não fixados e sem fila), **Sugerir a próxima mensagem** e **Título do chat pelo Claude
    Code** (ligado; desligado, o título fica o das primeiras palavras e nem o pedido do título sai,
    `opcoes::titulo_pelo_claude_vale`). Mais três grupos, todos gravando na hora em `Config`:
    **Abas** (**Chat chamado por outro abre numa aba**, ligada, e **Fechar a aba para o chat**,
    ligada), **Caixa de mensagem** (**Seta ↑ na caixa vazia**: Corrigir a última, como era, ou
    Percorrer as enviadas; **Enter com o turno rodando**: Entra no turno, como era, ou Vai para a
    fila) e **Arquivos** (**Lembrar o que a aba Arquivos mostra**, desligada). O padrão de cada
    opção é o de antes dela, menos as duas das abas.
  - **Agentes:** o **Modo de permissão padrão**, **Quem responde nos chats novos**, o **Modelo
    dos chats do Claude** e a **Revisão automática pelo outro agente** (ligada; ver **A revisão do
    turno pelo outro agente**); as **Instruções para todos os chats** (um texto seu, de até 4.000
    caracteres, que entra no fim do prompt de sistema de cada chat, do Claude e do Codex, depois do
    que o Vitral manda; vale a partir da próxima mensagem; `prompts::com_instrucoes`); e o
    **Codex** (a versão instalada, ou **Instalar…** sem ele); e **Versão nova** (**Consultar a
    versão nova dos agentes**: Sozinho, como era, ou Só pelo botão, que traz o **Consultar agora**).
  - **Perfis:** os perfis de agente (ver **Perfis de agente**, abaixo).
  - **Memória:** **Memória da equipe** e **Rever a memória ao terminar**.
  - **Aparência:** **Zoom da interface** (o mesmo do Ctrl+roda), **Letra da conversa** (de 80% a
    150% do resto: só as mensagens crescem, com o texto, o código e os cartões delas; a lista, a
    caixa e os menus seguem o zoom; **Letra da conversa também na caixa**, desligada, põe a caixa
    de mensagem na mesma letra), **Mostrar o uso do plano no canto da janela**, ligada por
    padrão, com um interruptor por anel (**Anel do Claude, do Codex e do Gemini**, todos ligados;
    cada um ainda depende da conta conectada, `opcoes::aneis_escolhidos`), **Números nas abas e na
    lista** (Sempre, como era, ou Só segurando Ctrl: o selo aparece depois de 150 ms com o Ctrl
    apertado e some ao soltar, guardando o lugar para a aba não mudar de largura; `Janela.ctrl_desde`, que
    zera a cada desenho com o Ctrl de fato solto: o `Ctrl+,` que abre as Configurações soltava o Ctrl
    com o foco no diálogo, e o número ficava à vista),
    **Animações** (Seguir o GNOME ou Desligadas; vale para o app todo e muda na hora;
    seguindo um GNOME sem animações, a linha diz isso) e **Transparência dos fundos**.
  - **Claude Code:** a versão instalada, a que os turnos abrem, e **Atualizar**, que roda `claude
    update` fora de qualquer turno, espera os chats que estão trabalhando ou com mensagem na fila
    e mostra o que ele respondeu, em âmbar se falhar (a versão nova vale no turno seguinte, sem
    reiniciar o Vitral), **Servidores MCP do Claude Code**, os do seu Claude Code nos chats, e a
    **Lista dos servidores MCP**, que abre a lista de cada servidor e o estado dele.
    Quando o pacote do Claude Code ou do Codex anuncia versão mais nova que a instalada, a linha
    diz ("Há a 2.1.330. O Atualizar instala.", "0.169.0 · há a 0.171.0"); em dia, não diz nada. A
    consulta são as etiquetas do pacote de cada um no npm (a `stable` para quem escolheu esse
    canal no Claude Code), sem nenhum dado seu, feita só quando a parte Agentes ou a do Claude
    Code abre (ou pelo botão, com **Só pelo botão**) e guardada por uma hora (`src/versoes_agentes.rs`).
  - **Atualizações:** as atualizações do Vitral, a **Pasta de dados** (com **Abrir**) e o
    **Diagnóstico para pedir ajuda**: o **Copiar**
    põe na área de transferência um texto com as versões (Vitral, Claude Code, Codex), o sistema,
    as pastas dos dados, as opções ligadas e as últimas 40 linhas do `queda.log`. O que tem cara de
    chave ou token é coberto, a pasta pessoal vira `~`, as suas instruções entram só pelo tamanho,
    o endereço da memória da equipe só como "ligada", e nenhuma conversa entra
    (`src/diagnostico.rs`).
  A organização é a das partes da 0.9.6. O ramo que desenhou as Configurações em seis seções
  (Chats, Agentes, Perfis, Aparência, Avisos e Vitral, com a lista em `Secao::grupos`) trouxe as
  opções novas, que entraram dentro das partes; as seções dele não ficaram.
- **Chat chamado por outro numa aba, e fechar a aba para o chat** (Configurações > Chats > Abas).
  Quando `mensagem_para_chat` entrega a mensagem (não quando o freio de ciclo a guarda como nota)
  ou `abrir_chat` abre um chat, o motor põe o id em `Motor.chats_trazidos`; a Janela consome a fila
  no mesmo observador do `pedido_de_abrir` e chama `trazer_aba`, que segue `onde_entra_a_aba`: o
  chat já à vista ou já aba de algum painel não faz nada, sem painel abre como qualquer chat, e nos
  outros casos entra como aba de fundo no painel ativo, sem trocar o que está à vista e sem
  `activate_window` (o contorno da aba mostra o estado). O × da aba e o Ctrl+W
  (`fechar_aba_do_usuario`) param o turno do chat que está de fato rodando (não o que espera a
  resposta do usuário) pelo `Motor::parar`, a menos que ele continue à vista noutro painel ou aba
  (`ao_fechar_a_aba`); se parar custa caro (`parar_custa_caro`: agentes em segundo plano,
  Ultracode), o Vitral pergunta antes. Soltar a aba em outro painel e arquivar não passam por aí.
- **Seta ↑ percorrer as enviadas.** Com a opção (`Config.seta_acima_percorre`), a seta na caixa
  vazia traz o texto das mensagens que o usuário enviou neste chat (`recordar::enviadas_do_chat`,
  da mais nova para a mais antiga, sem repetir em seguida), uma por vez; enquanto a caixa tem
  exatamente o texto trazido, o contexto dela ganha "Recordando" e ↑/↓ continuam andando (↓ volta
  até a caixa vazia). Editar o texto encerra o percurso (`Percurso::ativo`). O Corrigir fica no
  lápis do balão e no Ctrl+↑.
- **Perfis de agente.** Um perfil guarda, com um nome, quem responde, o modelo, o modo de
  permissão e o esforço, mais uma nota de quando usar. Cria-se em Configurações > Perfis (**Novo
  perfil…**; **Editar…** troca ou exclui, e as setas mudam a ordem). Os perfis de quem responde na
  aba aparecem no alto do seletor de modelo da caixa, no grupo **Perfis**, com a nota (ou o resumo,
  "Claude · Opus · Planejar") embaixo do nome e o selo "em uso" no que bate com o chat. Escolher um
  deixa o chat como ele diz, de uma vez: o que o perfil deixa no padrão volta ao padrão no chat. A
  troca de agente segue a regra de sempre (com um turno rodando, espera). Ficam no `config.json`
  (`Config.perfis`, `src/perfis.rs`); não há perfil padrão.
  **Atualizações** também vale para o app todo: o modo (**Automáticas**, o padrão, **Só avisar** ou
  **Desligadas**), a **Versão** ("0.3.0 · procurada hoje às 14:02", "Procurando…", "Baixando 0.3.1 ·
  12 de 21 MB", "é a mais nova", "A 0.3.1 está pronta; vale quando reiniciar") com o botão do
  momento (**Procurar agora**, **Instalar**, **Novidades** e **Reiniciar**), o erro numa linha (âmbar
  sem conexão, com a hora da próxima tentativa; vermelho quando a assinatura ou o arquivo não
  conferem) e, na versão instalada, **Voltar para a 0.2.0…**, que pergunta antes (o Enter só fecha),
  recusa a versão de agora e reinicia na anterior; se a anterior não lê o formato das conversas (como
  a 0.2.0 depois da migração), a linha fica apagada e diz isso. No compilado no PC: "Compilado no PC ·
  sem atualização automática", com **Instalar a 0.3.1 como app**. Os menus mostram o ícone e a cor do
  modo, ou a marca do Claude e uma linha sobre cada modelo; os prazos são escolhas lado a lado; o (i)
  explica o resto. Com o diálogo estreito (zoom alto ou janela pequena), os controles descem para
  baixo do texto. Cada linha tem uma descrição de uma frase; o detalhe mora no (i).
- **Diálogos** opacos, com cantos de 20, o título grande e o × com a dica; o rodapé fica sempre à
  vista e só o meio rola. **Mencionar arquivo** e **Conversa como contexto** são listas com busca
  e teclado (os arquivos são lidos em segundo plano, com o ícone do tipo e a pasta embaixo; as
  conversas, da mais recente para a mais antiga, com quando falaram, quantas mensagens têm e a
  pasta). **Renomear** já abre com o
  título selecionado e não salva vazio.
- Notificação do sistema, só com a janela em segundo plano, quando um chat termina, quando um chat
  pede permissão ou quando um chat cai: com o ícone do Vitral, o título do chat e uma linha só
  ("Precisa da sua permissão: Bash, $ cargo test", "Terminou: …", "Parou com erro: ..."). O
  "Terminou" leva o começo do que o agente disse, sem as marcas de markdown (título, lista,
  negrito, crases, link, cerca de código) e com até 220 letras, cortado numa palavra
  (`previa_da_resposta`, em `src/motor.rs`; antes ia a primeira linha crua, e o aviso saía
  "Terminou: ## Resumo"); a linha do "Enquanto você esteve fora" sai da mesma prévia. Clicar nela abre
  a conversa no painel ativo e traz a janela para a frente (ou o GNOME avisa que o Vitral está
  pronto). Uma notificação nova do mesmo chat substitui a anterior em vez de empilhar. A permissão se
  responde dentro do app, nunca na notificação. O pedido de permissão e a queda com erro saem com
  urgência alta: no GNOME ficam na tela até o clique (no Windows saem como as outras); o "Terminou"
  some sozinho, como sempre.
- **Aviso dentro do app.** Com a janela na frente, o chat fora de vista (em nenhum painel, ou numa
  aba de fundo) que precisa de você ganha um aviso no canto de baixo, à direita, acima da caixa do
  último painel: o pedido de permissão com **Negar** e **Permitir** (o comando perigoso ou comprido
  vira **Ver o comando…**, a regra do sino), o plano com **Ver plano** e a falha com **Abrir** e
  **Tentar de novo**. O clique no título abre o chat. O aviso fica até ser respondido e some
  sozinho quando é respondido em outro lugar; o × o dispensa. Ele nunca pega o foco, e o Enter da
  caixa nunca cai num botão dele. Três ou mais de uma vez viram um aviso só, "3 chats precisam de
  você · Ver", que abre o sino. O turno começado por outro chat nunca avisa, e o "Terminou" fica
  no sino. Desliga em Configurações, **Avisar dentro do app**.

Atalhos: `Ctrl+N` novo chat (na hora, na pasta do chat aberto), `Ctrl+K` buscar (`↓` entra na
lista; nela, `Enter` abre, `Ctrl+Enter` abre ao lado e `F2` renomeia), `Ctrl+J` painel da direita,
`Ctrl+B` recolhe ou mostra a lista, `Alt+1…4` painel N, `F10` menu do Vitral, `Ctrl+,`
configurações, `Ctrl+G` próxima pendência (`Ctrl+Shift+G` a anterior), `Ctrl+W` fecha a aba à vista (na última, o painel),
`Ctrl+PgUp/PgDn` painel vizinho, `Ctrl+Shift+PgUp/PgDn` muda o painel de lugar (na aba: `Enter` vai
para a caixa, `Delete` fecha, `Shift+F10` abre o menu), `Ctrl+End` vai ao fim da conversa, `Ctrl+F`
busca na conversa (`F3`/`Shift+F3` andam), `Shift+Tab` troca o modo, `Esc` para o turno do painel ativo,
também com o foco fora da caixa, depois de um clique na conversa (ou fecha a gaveta), `Enter` envia, `Shift+Enter` quebra a linha, `Ctrl+roda`, `Ctrl+=` e `Ctrl+-` zoom,
`Ctrl+0` zoom normal. `Ctrl+Shift+M` (modelo) e `Ctrl+E` (esforço) abrem e fecham os popovers da
caixa do painel ativo (no do esforço, ←/→, Home e End mudam o nível, Espaço liga o Ultrathink, U
liga o Ultracode e Enter fecha (as teclas estão no rodapé do popover); Esc fecha sem parar a
conversa). `?` abre os **Atalhos do teclado**. Fica um flutuante aberto por vez: abrir o menu do
logo, o sino, um popover da caixa, o menu ou o **+** de um painel, pelo mouse ou pelo atalho,
fecha o que estava aberto (`Janela::so_um_flutuante`).

## Resumo do dia

**Resumo de hoje**, no menu do logo, abre um diálogo com o que os chats fizeram no dia, por pasta
(`src/ui/resumo_do_dia.rs`). É montado só do que o app já tem, sem chamar modelo nenhum: os itens
das conversas (cada um com a hora) e o estado de cada chat. Um seletor troca para **Ontem** e **7
dias**, e **Copiar como Markdown** leva o texto para a área de transferência (um título, uma seção
por pasta e, em cada chat, a frase, o tempo, a pendência e os arquivos).

- **Quem entra**: o chat em que o agente respondeu ou usou uma ferramenta no período (as
  arquivadas também). Só a sua mensagem, sem o agente ter feito nada, não conta. A pasta mexida por
  último vem primeiro e, em cada uma, o chat mexido por último.
- **De cada chat**: o título (o clique abre o chat), a primeira frase da última resposta do
  período (`falas::primeira_frase`), os arquivos que as chamadas de edição do período mudaram, com
  o +N −M somado e o "novo" (a mesma fonte das Mudanças; o que um comando mudou fica de fora), o
  tempo trabalhado com o número de turnos e o que ficou pendente: o estado do chat agora
  (`falas::estado_do_chat`: rodando, parada, erro, permissão ou plano esperando você). A resposta
  que você ainda não leu não é pendência.
- **O tempo trabalhado** é a soma dos intervalos entre um item e o seguinte dentro de cada turno,
  só o pedaço que cai no período. Um intervalo de mais de 10 minutos sem nada acontecer conta só
  até esse teto: é espera (um pedido de permissão parado, o Vitral fechado), não trabalho. Um
  comando que roda sozinho por mais de 10 minutos também entra só até o teto.
- Os períodos vão pela meia-noite local: hoje, do começo do dia até agora; ontem, o dia inteiro; 7
  dias, do começo do dia de seis dias atrás até agora. O resumo é montado ao abrir e ao trocar de
  período, não a cada quadro.

## Quadro de ideias (Ctrl+I)

A lista do que você quer fazer no futuro, num kanban (`src/ideias.rs`, `src/motor/ideias.rs` e
`src/ui/quadro_ideias.rs`). Só você anota: nenhum chat põe ideia no quadro sozinho (dono,
18/11/2026: "essas ideias é pra ser coisas do futuro que vou colocando pra não esquecer"). Abre
pelo Ctrl+I, pelo **Quadro de ideias** do menu do logo e por **Ideias deste chat** no menu de uma
conversa (o botão direito na lista ou no cabeçalho, ou Shift+F10), que diz quantas ela tem.

O diálogo ocupa quase a janela, com 16 de margem. Em cima, uma linha só: "Ideias", o chip do chat
quando o quadro é de um (com o × que mostra todas), como está o Linear (com o ⟳ e a tomada) e o
fechar. Embaixo, as quatro colunas, **Ideias**, **Próximas**, **Fazendo** e **Feitas**: o nome
com o número de cartões e, no fundo da lista (o metal, o véu azul e o buraco, como os balões da
conversa), os cartões, que rolam por coluna. O campo do alto de Ideias anota com o Enter; a ideia
entra no alto da coluna, na pasta do chat à vista (sem chat, na pasta de trabalho), ou, no quadro
de um chat, já dele.

- **Cartão:** como uma linha da lista, um véu claro que acende com o ponteiro. O título (13, até 3
  linhas), o porquê ou o começo do pedido (2 linhas) e uma linha de fatos, cortada na borda sem
  reticências: o chat da ideia com o estado dele (ou a pasta), o código da issue e o responsável.
  Com o ponteiro em cima, o botão de mandar.
- **Ideia de um chat:** a escolha **Chat** da edição liga a ideia a uma conversa (as 40 mexidas por
  último) ou a deixa para **Um chat novo**. **Mandar no chat** põe o pedido na fila da conversa
  dela, sem cortar o turno que estiver rodando, e abre a conversa; sem chat, **Abrir chat** cria
  um na pasta do cartão, com o pedido como primeira mensagem, e liga o cartão a ele. Nos dois
  casos o cartão vai para Fazendo. O botão direito de um cartão com chat tem os dois.
- **Arrastar:** solto numa coluna, o cartão vai para o fim dela; solto em cima de outro, entra
  logo antes dele (a ordem de uma coluna é a `ordem` de cada cartão, crescente).
- **Clique:** abre a edição, com a seta de voltar: título, **Pedido para o chat** (vazio, vai o
  título), **Chat**, **Pasta** (só para o chat novo), **Coluna**, **Apagar** (com Desfazer no
  aviso), o código no Linear, **Ir ao chat** e **Mandar no chat** ou **Abrir chat**. O texto se
  grava sozinho (o motor marca o quadro e grava na passada de 5 s, e ao fechar).
- **Botão direito:** mandar (no chat ou num novo), editar, abrir no Linear, mover para cada uma
  das outras colunas e apagar.

Fica em `ideias.json`, na pasta de dados (`{"ideias": [...]}`; os campos que esta versão não
conhece são regravados como vieram). Ilegível, o arquivo é copiado ao lado e o quadro abre vazio,
com o aviso.

**Com o Linear** (`src/linear.rs`; dono, 18/11/2026: o quadro é o do Linear, no time EFI,
Eficiência Operacional). Ligado pelo **Ligar o Linear** do cabeçalho (a chave pessoal, de
Settings > Security & access > Personal API keys, testada contra o time antes de gravar, em
`linear.chave` na configuração, 0600), as ideias são as issues do time com a etiqueta **Ideia**
(abertas e concluídas nos últimos 30 dias, sem as canceladas): as tarefas do time ficam de fora.
A coluna é a do tipo do estado (triagem e backlog em Ideias, "unstarted" em Próximas, "started" em
Fazendo, "completed" em Feitas), na ordem do `sortOrder`. O chat, a pasta e o porquê continuam só
do Vitral, guardados no cartão.

- **O que sobe:** a ideia nova (vira issue no estado de backlog, com a etiqueta Ideia, que o
  quadro cria no time se ainda não existir), o título e o pedido (1,5 s depois de parar de
  digitar), a coluna (o primeiro estado do tipo dela no fluxo do time) e a ordem (o `sortOrder`
  no meio dos vizinhos), o mandar (vai para "started") e o Apagar (arquiva a issue; o Desfazer
  desarquiva). O cartão muda na hora; a chamada vai em segundo plano.
- **O que desce:** ao abrir o quadro, a cada minuto com ele aberto e no ⟳. Enquanto o Linear não
  responde, o cabeçalho mostra o anel girando e "Lendo o Linear", e a coluna vazia mostra o anel
  no lugar do texto dela (dono, 18/11/2026: "faz um loading"). Um cartão com mudança
  daqui a caminho fica como está até a próxima leitura; se o Linear recusar uma mudança, o
  cabeçalho mostra o erro em vermelho e o quadro relê.
- **Os só do Vitral:** os cartões de antes de ligar (e os de um erro ao criar) ficam no quadro, sem
  o código de issue, e não sobem (dono, 18/11/2026: "não precisa ter isso de mandar pro Linear").
- **O filtro:** no cabeçalho, **Todas**, **Linear** e **Vitral**, cada um com quantas há (dono,
  18/11/2026: "selecionar as do Linear e as do Vitral como um filtro"). Com **Vitral**, a ideia nova
  também fica só no Vitral (o campo diz "Nova ideia só no Vitral"). O quadro abre no último filtro
  escolhido, até fechar o app. Sem o Linear ligado, o filtro não aparece.
- **Desligar** (o ícone da tomada) apaga a chave; os cartões das issues saem do quadro.

## Compilar e rodar

Para usar no dia a dia, instale pelo release, sem compilar: `curl -fsSL
https://github.com/VictorMenegate/vitral/releases/latest/download/instalar.sh | sh` (depois ele se
atualiza sozinho; ver **Versões e releases**).

```sh
vitral                                  # abre o app (no último chat aberto)
vitral ~/Projetos/loja                  # o chat dessa pasta mexido por último, ou um chat novo nela
vitral --nova ~/Projetos/loja "o que fazer"   # um chat novo nessa pasta, já com a mensagem
cargo test                              # testes (motor, processos, dados e migração, importação, atualização, interface, --instalar, lançador do Windows)
ferramentas/bancada.sh roteiro.txt prints/ ./target/debug/vitral [pasta]   # teste visual isolado
./target/debug/vitral --vitrine         # todos os componentes da linguagem visual, sem pasta e sem motor
./target/debug/vitral --versao [--json] # a versão (e canal, commit, formato dos dados), sem janela
ferramentas/instalar-dev.sh             # só desenvolvimento: compila e instala no layout do app como <próxima>-dev.<commit>
```

O `rust-toolchain.toml` fixa o Rust em 1.99.1, o mesmo do GitHub Actions. Com o rustup, a primeira
compilação na pasta instala esse toolchain (perfil mínimo) se ele ainda não estiver lá; enquanto o
`stable` do PC for o mesmo 1.99.1, `RUSTUP_TOOLCHAIN=stable` evita o download.

O `vitral --instalar` põe o logo completo no tamanho escalável e de 64 a 256 px, e o pequeno
(`vitral-pequeno.svg`, feito para tamanhos pequenos) em 16, 24 e 32 px e no PNG de 48, o que as
notificações e o alt+tab usam. Os ícones e o `.desktop` vão dentro do binário (`include_bytes!`).

`DADOS_GLOBAIS` (em `src/dados.rs`) é a chave dos dados globais, ligada desde a 0.3.0. Numa mudança
grande dos dados, desligada, o Vitral só abre na bancada (`DENTRO_DA_BANCADA=1`, que isola os XDG),
o `vitral --instalar` e o `ferramentas/instalar-dev.sh` recusam, e um release estável não passa nos
testes: um build do meio da mudança nunca grava nos dados de verdade. Nenhum teste toca
`~/.local/share` nem `~/.config` (`Dados::padrao()` dá panic nos testes; eles usam `Dados::em`).
Para conferir a migração com dados de verdade sem tocar neles, copie para uma pasta de mentira só os
`.vitral/` das pastas antigas e o `recentes.json` (apontando para as cópias) e rode
`VITRAL_COPIA_DA_MIGRACAO=<casa> target/debug/deps/vitral-<hash> --ignored --nocapture --exact
migracao::testes::migrar_uma_copia` com `HOME` e os `XDG_*` na pasta de mentira (o binário dos
testes sai do `cargo test --no-run`): ele migra para `<casa>/.local/share/vitral`, abre como o
motor abriria e escreve o que virou o quê; com `VITRAL_COPIA_PROC_REAL=1`, confere que o Vitral
antigo aberto adia sem gravar nada.

Bancada: o comando é o Vitral com uma pasta opcional, que ele importa (no primeiro uso, uma pasta do
Vitral antigo) ou abre (depois, o chat dela ou um novo nela). `FUNDO=branco` (ou `FUNDO=<imagem>`)
troca o papel de parede (com o metal opaco, ele só aparece fora da janela), `ZOOM=1.5` abre com
esse zoom e `TELA=2400x1300` troca o tamanho do monitor de mentira (o padrão é 1600x1000; a 150%,
três conversas lado a lado só cabem numa tela mais larga). `DADOS_NOVOS=1` abre como primeiro uso
(afasta para `$B/afastados/<hora>` os dados, a configuração e o `interface.json` da bancada; o `app/`
que a atualização da bancada instalou fica) e `RAIZ_SINTETICA=1` é a migração de ponta a ponta: gera
com o `demo_vitral.py` uma pasta do Vitral antigo dentro da bancada (com os `cwd` dentro dela e
sessões novas, nunca uma cópia de uma pasta de verdade), põe ela no `recentes.json` da bancada, abre
como primeiro uso e confere no fim que os hashes do `.vitral` dela não mudaram
(`hashes-antes.txt` e `hashes-depois.txt` na pasta dos prints, e "raiz sintética intacta" no
stderr). `CLAUDE_CONFIG_DIR=<pasta de mentira>` dá à bancada um Claude Code de mentira para
importar; sem ele, dentro da bancada, nada é importado (o `~/.claude` de verdade nunca entra numa
bancada, nem pelas contas extras). `CLAUDE_FALSO=1` põe na frente do PATH o
`ferramentas/claude-falso/claude`, que não chama a API: ele anota a mensagem em `notas-do-chat.md`
na pasta do chat, antes das ferramentas; leva 10 s com
`[demora]`; com `[escreve 20]` a resposta chega aos pedaços durante esses segundos, uns 40 por
segundo, como o texto do Claude Code de verdade (é como se mede a conversa com o texto chegando); `CLAUDE_FALSO_SOBE=<segundos>` o faz esperar esse tempo antes de ler a entrada, como o
Claude Code de verdade enquanto sobe (é como se confere que a janela não trava no envio de um
print); com `[ferramenta <Nome> <detalhe>]` (Read, Edit, Write, Bash, Grep, Glob, WebFetch,
Task…) finge usar a ferramenta, mandando o `tool_use` (o Edit e o Write com um trecho de mentira,
para a linha da ferramenta ter o que mostrar ao abrir), esperando 4 s (ou os segundos de
`[ferramenta … | 8]`) e mandando o `tool_result`, para dar de ver a linha do turno ("Lendo
motor.rs…", "Rodando: cargo test…") sem gastar a API; o `[ferramenta Pensa <texto> | 3]` encena um
bloco de raciocínio que dura os segundos pedidos, e o `Task` (ou `Agent`, que vai para o segundo
plano) encena um subagente como o de verdade (2.1.328): o `task_started`, o pedido e cada chamada
com o resultado presos à chamada dele (`parent_tool_use_id` e `agent_id`), o texto e o raciocínio
só no arquivo (no stream, só com `--forward-subagent-text`) e, com um `CLAUDE_CONFIG_DIR` de
mentira, o arquivo dele gravado aos poucos em `projects/<pasta>/<sessão>/subagents/` com o
`.meta.json` (é o que a aba do subagente lê); com "(pede)" na descrição, o comando dele pede
permissão ao Vitral (o subagente que "espera você"), e com "(entrega)" o relatório vai pela
chamada `SubagentHandback`, como no modo automático; com "(segue)" na descrição de um `Agent`, ele
continua depois do `result` como o `run_in_background` de verdade (a lista do
`background_tasks_changed`, as chamadas dele presas à chamada que o abriu, o arquivo crescendo com
as linhas do pedido à API entre as chamadas, o `task_notification` no fim e um turno novo aberto
pelo aviso, com o `result` guardado até o último agente), e com "(falha)" o aviso de fim vem
`failed` (as duas marcas saem da descrição); com `[juntas]` e só chamadas `Task`, os
subagentes trabalham ao mesmo tempo; o `[ferramenta Fundo <comando> | 20]` é um comando de fundo
que não segura mais o turno (vários rodam juntos, e o `stop_task` para só o dele), e o id de cada
chamada leva uma marca do processo, para não se repetir de um turno para o outro; o `[ferramenta Diz
<texto>]` é um texto no meio do turno (com ele e uma chamada, o turno pronto recolhe na linha
"Trabalhou…"); com `[juntas]` as chamadas comuns saem numa resposta só e cada resultado chega
quando a dela termina, como as chamadas em paralelo de verdade; o `[ferramenta Estourou <comando>
| 12]` é o Bash que passou do tempo e foi para o segundo plano (`backgroundTaskId` e
`timedOutAfterMs` no resultado); o `result` leva o `duration_ms`, o `duration_api_ms`, o
`ttft_ms`, o `num_turns` e o `usage.cache_creation` como o de verdade (cache de 1 h, ou de 5 min
com `[cache 5m]`), e `[ttft 1.8]` espera esses segundos antes da primeira palavra (com o
`CLAUDE_FALSO_SOBE`, a dica do turno mostra o "para abrir o Claude Code"); o `[ferramenta Edit
<arquivo> (palavra)]` troca uma palavra num trecho de 10 linhas (o diff de uma linha só), e com
"(pede)" a edição pede permissão antes; o `[ferramenta Falha <comando>]` é o Bash que saiu com
erro (`is_error` e "Exit code 101"), para as checagens da linha "Trabalhou…"; o `[ferramenta
TodoWrite fap]` manda a lista com um passo por letra (f feito, a em andamento, p pendente), e o
`TaskCreate <assunto>`, o `TaskUpdate <id> <estado>` e o `TaskList x` são as ferramentas de
tarefas do Claude Code, com o texto e o `tool_use_result` medidos na 2.1.328 e a lista guardada
por sessão; com `[tabela]` a resposta leva uma tabela em Markdown; o `[ferramenta Print
<LxA> <nome>]` encena o print de um MCP de navegador (a chamada
`mcp__chrome-devtools__take_screenshot` e um `tool_result` com um bloco de texto e um PNG gerado
na hora em base64; `Prints` devolve duas imagens no mesmo resultado) e o `[ferramenta Read
<arquivo>]` de uma imagem que existe devolve só o bloco de imagem dela, como o Read de verdade (o
roteiro é o `ferramentas/roteiros/imagens-de-ferramenta.txt`, que também confere com o `grep` que
o base64 não foi para a conversa nem para o estado); com `[hook <Nome>
<detalhe> | <motivo>]` encena uma chamada que um hook `PreToolUse` mandou perguntar, como o Claude
Code 2.1.328: o `tool_use`, o `hook_response` com o motivo (só com `--include-hook-events`) e o
pedido à `permissao` do Vitral, sem o motivo (sem o `--permission-prompt-tool` nos argumentos, a
chamada volta recusada com o motivo como erro, sem perguntar a ninguém; para o motivo aparecer no
cartão, a bancada precisa de `CLAUDE_FALSO_VERSAO=2.1.328`, porque o Vitral só pede os eventos de
hook a partir do 2.1.323 e o padrão do claude de mentira é 2.1.300, a mesma versão que ele diz no
`init`; o roteiro é o `ferramentas/roteiros/ask-de-hook.txt`); pela entrada, no meio do turno,
responde como o 2.1.328 ao `set_permission_mode`, ao `set_model` e ao `apply_flag_settings` e
termina a resposta com "no meio do turno: modo acceptEdits, …", para a bancada ver que a troca
chegou; a mensagem que chega com uma ferramenta rodando fica na fila dele até ela voltar, o
`cancel_async_message` a tira de lá e o `interrupt` responde com o `still_queued` e, se sobrou
mensagem, abre outro turno com ela; a ferramenta que demora manda o `tool_progress` com
`heartbeat` a cada 30 s (`CLAUDE_FALSO_BATIDA=<segundos>` encurta); e o texto do `Pensa` só vem
com `--thinking-display summarized` nos argumentos, que o Vitral só passa a partir do 2.1.323
(com o padrão 2.1.300 o bloco chega vazio, como no de verdade sem a opção; os roteiros são o
`ferramentas/roteiros/responder-pedido.txt` e o `ferramentas/roteiros/troca-no-turno.txt`); com
`[pergunta <Cabeçalho> | <pergunta> | <opção>: <descrição>; <opção>]` (e `[pergunta+ …]` para a
que aceita várias) encena a AskUserQuestion como o 2.1.328, todas as perguntas da mensagem numa
chamada só, e a resposta do turno termina com "respostas: …"; com `[plano <texto>]` (um `\n`
escrito vira quebra de linha) encena o plano, com a chamada ExitPlanMode vazia, o plano no pedido
de permissão e, aprovado, no resultado; com `[compacta <auto|manual> <tokens antes> | <segundos>]`
encena uma compactação no meio do turno, e a mensagem "/compact" faz a manual e termina sem texto,
como o de verdade (`CLAUDE_FALSO_COMPACTA=<segundos>`; o roteiro dos três é o
`ferramentas/roteiros/pergunta-plano-compactacao.txt`); com `[responde <arquivo>]` a resposta do
turno é o conteúdo desse arquivo da pasta do chat (markdown, arquivos e comandos citados) e com
`[saida <arquivo>]` o resultado das ferramentas encenadas na mensagem é o conteúdo do arquivo (a
saída de um build com erro); ao `generate_session_title` responde com o título do `[titulo <nome>]`
da mensagem (ou o de `CLAUDE_FALSO_TITULO`, ou "Sobre" e as três primeiras palavras) e, com o `persist` e um
`CLAUDE_CONFIG_DIR` de mentira, grava a linha `ai-title` na sessão, que é só o que esse arquivo
tem (o roteiro dos três é o `ferramentas/roteiros/titulo-aviso-links.txt`); ao lado dele mora o
`ferramentas/claude-falso/codex`, o Codex de mentira (fala `exec --json` e o JSON-RPC do `app-server`, com `turn/steer`, e entende o mesmo
`[ferramenta …]`, mais o `Ver <arquivo>`, que é o `view_image` (o item `imageView`), e o `Print`, uma ferramenta MCP que devolve uma imagem em base64; com a política `untrusted` (Manual e Aceitar edições), cada Bash, Edit e Write pede aprovação como o 0.169.0 e espera a resposta, e com `on-request` só o comando com "curl" pede; o `thread/compact/start`, o `review/start` e o `skills/list` (a skill falsa "revisar-pr") respondem como o app-server, e o uso vem num `thread/tokenUsage/updated` depois de cada item; `[passos A; B; C]` encena o plano (`turn/plan/updated`), um passo a cada ferramenta; `[falha]` termina o turno com erro, `codex debug models` dá um catálogo pequeno e
os argumentos ficam em `$XDG_STATE_HOME/codex-falso.log`; o `codex --version` diz a `CODEX_FALSO_VERSAO`, por padrão 0.169.0) e o `ferramentas/claude-falso/agy`, o
Antigravity CLI (o Gemini) de mentira (fala o ACP do `agy acp`; `[demora]`, `[permissao]` pede para
rodar `ls -la` e diz o que o Vitral respondeu, `[ferramenta Edit]` encena uma edição, `[falha]`
termina com erro, um `session/prompt` com o turno rodando entra nele, `agy models` dá a lista, e os
pedidos ficam em `$XDG_STATE_HOME/agy-falso.log`); executa os `[mcp <nome> <json>]` da mensagem
(fora das linhas citadas com "> ") como chamadas de verdade ao MCP do Vitral; no `--fork-session`, copia a sessão original para a nova ao lado dela (só nesse `CLAUDE_CONFIG_DIR`); e
grava os argumentos, o ambiente e as chamadas em `$XDG_STATE_HOME/claude-falso.log`. No roteiro,
`roda <comando>` (saída em `roda.log`, sem comentário na linha; os XDG são os da bancada) e `reabre
[comando]` (derruba o Vitral com SIGKILL, roda o comando com ele fechado e abre de novo).
`ferramentas/conferir-snapshot.sh` é a conferência manual do prompt por arquivo (chama o Claude: dois
turnos curtos). Duas bancadas ao mesmo tempo precisam de `BANCADA=N` diferentes e de pastas diferentes
(`XDG_CACHE_HOME=~/.cache/vitral-trabalho/bancada-x BANCADA=5`; a bancada fica em
`$XDG_CACHE_HOME/vitral-bancada`); com o número ou a pasta já em uso, ela recusa começar. `python3 -I
ferramentas/demo_vitral.py <pasta> [--16]` cria uma pasta do Vitral antigo de demonstração (4
subpastas, 11 conversas, 8 pedidos em todos os estados, rotinas; com `--16`, mais 12 subpastas), sem
rodar nenhum processo, que a bancada migra no primeiro uso; gere de novo e rode com `DADOS_NOVOS=1`
antes de cada rodada. `python3 -I ferramentas/medir_print.py
<print.png> X Y` (ou `--pontos <arquivo>`) mede o contraste de um texto no print. Os roteiros da
linguagem visual ficam em `ferramentas/roteiros/` (`4c-antes.txt` tira todas as telas;
`4c-zoom-larguras.txt` vai de 100% a 150% com a lista arrastada, os popovers do Claude e do sino, a
janela estreita e as gavetas, e `4c-zoom-lista-estreita.txt` estreita a lista). Para ver todos os
menus, popovers e diálogos de uma vez: `ferramentas/galeria-de-menus.sh <pasta> [binário]` roda o
roteiro `galeria-menus.txt` (zoom 1,6; o menu do logo, o sino, o uso, o menu do chat, os chips da
caixa, Nova conversa, as seis partes das Configurações, Atalhos, Paleta, Contas, Memória da equipe,
Quadro de ideias, Novo chat e Importar), grava um print inteiro de cada e monta a `folha.png` com
todos em miniatura, com o nome; serve para as revisões de design (dono, 23/11/2026: "faz algo pra
conseguir ver todos e analisar o que dá pra melhorar"). Os cliques são posições fixas: mudou o
layout, confira a folha antes de confiar nela. No meio de um
roteiro, `tecla Control_L+equal` muda o zoom, e `rolar X Y N` gira a roda N entalhes com o ponteiro
em X, Y (N negativo sobe; serve para ver o fim de um diálogo que rola). A bancada usa o socket do
Wayland que o Shell anuncia no log (um lock velho de uma rodada que caiu o leva ao `wayland-1`) e
põe o `XDG_BIN_HOME` na pasta dela. A atualização fica desligada; com `VITRAL_SEM_ATUALIZACAO=0`,
`VITRAL_ATUALIZACAO_BASE` e `VITRAL_ATUALIZACAO_CHAVE`, o build de desenvolvimento procura num
release falso local e instala na pasta de dados da bancada: `ferramentas/release_falso.py gerar`
faz a 0.9.0 de mentira (20 MB, assinada com uma chave só da bancada pelo `openssl`) e `servir` a
entrega com o manifesto respondendo 503, adulterado e certo, nessa ordem, e o pacote a 1 MB/s; o
roteiro `ferramentas/roteiros/dist-atualizacao.txt` (com o comando no topo) fotografa a vitrine e o
caminho inteiro no app, do "ainda não procurou" ao ponto no logo. A vitrine abre na seção
**Atualizações**: o grupo das Configurações em cada estado (parado com a anterior, procurando, em
dia, baixando, pronta, só avisar, sem conexão, assinatura recusada, compilado no PC e formato novo
na pasta) e botões de 200 para os avisos, as Novidades e a pergunta do Reiniciar com 2 conversas
(o "mesmo assim" deixa um aviso, para conferir que o Enter não o aciona).

`VITRAL_AGENTES_PUBLICADOS="claude=2.1.400,codex=0.200.0"` responde no lugar da consulta das versões
publicadas dos agentes (na bancada, com a atualização desligada, nada é consultado): é como se vê o
"há a X" das linhas do Claude Code e do Codex. O roteiro
`ferramentas/roteiros/config-partes.txt` (com o comando no topo) passa pelas sete partes das
Configurações.

`VITRAL_QUADROS=1` escreve "quadros/s: N" no stderr a cada segundo em que a janela redesenhou
(nada em repouso): é a conferência do custo dos efeitos. Quando o metal refez o campo nesse
segundo, a linha leva o tempo médio de um campo: "quadros/s: 18 · campo: 2,1 ms". Depois vêm as
médias por quadro da montagem da casca, do tempo do começo do desenho até o fim do layout e até
o fim da pintura (o que sobra do tempo da thread da janela é o GPUI fechando a cena) e, se
alguma conversa foi refeita, quantas, quantos blocos cada uma montou e quanto levou: "montagem:
média 2.4 ms, pior 4.1 ms · até o layout: 12.0 ms · até a pintura: 19.0 ms · conversas: 12 ·
blocos: 14 de 28 · 9.5 ms cada". Com as conversas em cache, o número de conversas por quadro
diz quem está refazendo o quê: 1 por quadro é só o painel mexido; 3 por quadro com três painéis
é a Janela avisada.
`VITRAL_SEM_ANIMACAO=1` abre como se as animações do GNOME estivessem desligadas (o metal fica
parado na fase inicial, igual em toda máquina: é como se tiram as capturas). A vitrine (`--vitrine`) ganhou as seções **Movimento**
(um botão para cada efeito) e **Espaçamento** (a escala e as guias da lista, da direita e da
conversa).

Precisa do `claude` (Claude Code) no PATH ou em `~/.local/bin`, do `git`, e no Fedora de
`libxkbcommon-devel libxkbcommon-x11-devel wayland-devel vulkan-loader-devel fontconfig-devel
freetype-devel libxcb-devel` para compilar. Das dependências em Rust, o `chrono` dá as datas no fuso
local; já vinha no `Cargo.lock` pelo vendor.

**No Windows** (10 1809 ou mais novo, x64): precisa do **Git para Windows** (o Claude Code roda os
comandos no Git Bash dele) e do **Claude Code nativo** (`irm https://claude.ai/install.ps1 | iex`, o
`claude.exe` em `%USERPROFILE%\.local\bin`); o que faltar aparece como aviso na janela, e uma pasta
de chat dentro do OneDrive também. Os dados ficam em `%LOCALAPPDATA%\vitral` (o cache em `cache\` dentro
dela) e a configuração em `%APPDATA%\vitral`. As fontes vão dentro do binário nos dois sistemas.
Nos níveis transparentes, o desfoque é o acrílico do sistema (ver **Transparência dos fundos e o
desfoque**, em **Acabamento**; no Windows 10 ele pode pesar ao arrastar); o
arrastar, o clique duplo, o Snap e os botões –, □ e × são do próprio Windows. Os avisos saem como
notificações do Windows (AppUserModelID `VictorMenegate.Vitral`), o Parar manda o `interrupt` pela
entrada do Claude Code (nos dois sistemas) e o Parar de verdade é o Job Object, que derruba a
árvore inteira. Ainda não: contas extras e "Instalar o Codex".

Para instalar no Windows, o `vitral-<versão>-windows-x86_64-instalador.exe` do release: só para o
usuário, sem administrador (sem certificado de assinatura, o SmartScreen avisa na primeira vez: Mais
informações › Executar assim mesmo). Ele põe o Vitral em `%LOCALAPPDATA%\Programs\Vitral` e o atalho
no Menu Iniciar, com o AppUserModelID do app (o pino da barra e as notificações vão para ele). O
`Vitral.exe` da raiz é o **lançador** (`lancador/`, só a std): lê o `versoes.txt` (`atual=` e
`anterior=`) e abre o `app-<versão>\vitral.exe` da atual, ou o da anterior, ou o da maior `app-*`
que tiver um, com os mesmos argumentos e o `VITRAL_LANCADOR` no ambiente, e sai; sem nenhum,
explica numa caixa de mensagem. Cada versão fica na pasta dela porque o `.exe` aberto não pode ser
sobrescrito. Desinstalar (em Aplicativos instalados) tira só o programa e o que o atualizador deixar
na pasta dele; os dados e a configuração ficam. O `.zip` é a versão portátil: abra o `vitral.exe`
direto (o lançador que vem junto só serve dentro da instalação); ele só avisa das versões novas,
com o **Baixar**, que abre a página do release.

Depois de instalado, o Vitral se atualiza sozinho no Windows como no Linux (ver **Versões e
releases**), a partir da primeira versão com o atualizador do Windows: a 0.3.0-rc.6 instalada pelo
instalador ainda não procura, e passa para a seguinte pelo instalador dela.

## Versões e releases

O Vitral sai pronto do GitHub (`VictorMenegate/vitral`, licença MIT): o PC não precisa compilar
para instalar. Cada versão tem a seção dela no `NOVIDADES.md` (`## 0.2.0`, frases curtas, o que se
nota usando), que vira as notas do Release e, depois, o **Novidades** do app; um pré-lançamento
(`0.2.0-rc.1`) usa a seção da versão sem o sufixo.

- **CI** (`.github/workflows/ci.yml`): em todo PR e push no `main`, `cargo test --locked` no
  `ubuntu-24.04`. Lá não há systemd do usuário nem identidade do git: os processos caem no grupo
  próprio e os commits dos testes saem com a identidade do Vitral, como num PC sem isso. O Windows
  fica desligado até compilar (variável do repositório `WINDOWS_CI=true` liga sem reprovar;
  `WINDOWS_OBRIGATORIO=true` faz reprovar). No `main`, um job compila em release para deixar o
  cache do tag aquecido. O `dependabot.yml` só atualiza as actions (o `Swatinem/rust-cache` vai
  fixado pelo commit).
- **Release** (`.github/workflows/release.yml`): sai com um tag `v<versão>` igual ao `version` do
  `Cargo.toml` (`git tag v0.2.0 && git push origin v0.2.0`), ou à mão em Actions › Release (sem
  tag, só compila e guarda os pacotes por 14 dias). Testa e compila em release com
  `VITRAL_CANAL=estavel` e o commit embutidos, confere o `vitral --versao` e o `--versao --json` do
  pacote, gera o `vitral-<versão>-linux-x86_64.tar.gz` (o binário, LEIA-ME, NOVIDADES e LICENSE), o
  `vitral-versao.json` (`pacote/manifesto.py`: versão, commit, página, formato dos dados, notas e o
  tamanho e o SHA-256 de cada pacote) e o `SHA256SUMS`. O job de publicar roda no environment
  `release` (com aprovação do dono): assina o manifesto e as somas com o minisign, confere com o
  `pacote/vitral.pub` e publica como rascunho antes de soltar, para o "latest" nunca ficar pela
  metade. Versão com sufixo sai como pré-lançamento, que o `releases/latest` ignora. No
  `windows-2025` saem o `vitral-<versão>-windows-x86_64.zip` (portátil: o `vitral.exe`, o
  `vitral-lancador.exe` e os textos), o `.tar.gz` com o mesmo conteúdo (o que o atualizador do
  Windows vai baixar; feito com o GNU tar e as opções do Linux) e o `-instalador.exe` (Inno Setup,
  `pacote/windows/vitral.iss`, instalado pelo `choco` se a imagem não tiver), também no
  `SHA256SUMS` e no manifesto (cada sistema lê só o pacote dele). Enquanto o `WINDOWS_OBRIGATORIO`
  não liga, uma falha do Windows não segura o release: ele sai só com o Linux e um aviso. No ramo
  `windows`, o Actions › Prints do Windows (job do instalador) instala o pacote de uma rodada do
  release em silêncio, confere a pasta, o atalho, o AppUserModelID e a escolha do lançador, abre o
  app pelo atalho e o zip direto, e desinstala conferindo o que sobrou
  (`ferramentas/instalador-windows.ps1`).
- **Assinatura**: minisign (Ed25519). A chave principal e a reserva, sem senha, ficam em
  `~/.config/vitral/chaves` (pasta 700, arquivos 600): a principal também vive no secret
  `MINISIGN_SECRET_KEY` do environment `release`; a reserva nunca vai ao GitHub. Guarde uma cópia
  das duas `.key` fora do PC: perder as duas é ter de reinstalar à mão em todo PC. As públicas ficam
  em `pacote/` e entram no app por `include_str!` (`src/atualizacao/assinatura.rs`), que só aceita o
  formato com pré-hash; se a principal vazar ou se perder, a reserva assina uma versão que troca as
  chaves. Os testes usam uma chave de teste (`src/atualizacao/teste/`), nunca a principal.
- **De onde veio**: `VITRAL_CANAL` e `VITRAL_COMMIT` são lidos na compilação (`option_env!`); sem
  eles o binário se diz "pc" (compilado no PC), e só o canal estável se atualiza sozinho.
  `FORMATO_DADOS` (em `modelo.rs`) é o formato dos dados (1, o `.vitral/` de cada pasta do Vitral
  antigo; 2, os dados globais das versões com projetos; 3, só chats, cada um direto numa pasta),
  para a volta de versão não abrir dados que não entende.
- **Instalar de uma vez**: `pacote/linux/instalar.sh` vai como arquivo do Release (`curl -fsSL
  …/releases/latest/download/instalar.sh | sh`), com o tag do release gravado nele pelo
  `release.yml`: o de um pré-lançamento, que o "latest" não mostra, instala o pré-lançamento
  (`VITRAL_TAG=v…` escolhe à mão). Confere o `SHA256SUMS` (e a assinatura, com o minisign
  instalado), baixa o pacote para `~/.cache` (fora do `/tmp`), confere que o binário abre e chama
  `vitral --instalar`.
- **Instalação** (`src/atualizacao/linux.rs`): cada versão numa pasta,
  `~/.local/share/vitral/app/<versão>/`, e dois links: `atual`, o que o menu abre (o
  `vitral.desktop` tem `Exec=…/app/atual/vitral %f`), e `anterior`, a que estava. O `vitral
  --instalar` copia o próprio binário (e o LEIA-ME, o NOVIDADES e a LICENSE que estiverem ao lado)
  e escreve o mesmo `vitral.desktop` de antes (o favorito do dash continua), os ícones e o link
  `~/.local/bin/vitral`, se essa pasta existir (um `vitral` que não é link fica onde está); só grava
  o que mudou. Rodado de dentro da pasta do app, só confere o menu. A ordem aguenta a queda de
  energia em qualquer ponto: abre numa `.novo-*`, testa o binário novo (`--versao --json`, 10 s),
  põe no lugar com um rename (a mesma versão de novo troca de lugar com a velha num
  `RENAME_EXCHANGE`), aponta `anterior` e depois `atual` (link temporário e rename), grava o
  `estado.json` (a versão a confirmar, as recusadas e o formato dos dados de cada versão) e tira as
  outras versões por `.lixo-*`. O `atual` sempre aponta para uma versão completa, e a abertura
  seguinte apaga o que sobrou pela metade. A `app/.trava` impede duas instalações juntas. Na
  versão instalada, o reinício do GPUI vai para `app/atual/vitral` (sem isso abriria a versão
  velha, porque o `/proc/self/exe` resolve o link), e cada abertura da versão atual confere o menu.
- **Instalação no Windows** (`src/atualizacao/windows.rs`): o layout do instalador, em
  `%LOCALAPPDATA%\Programs\Vitral`: cada versão em `app-<versão>\` (o pacote inteiro, como o
  `.tar.gz` do Windows traz), o `versoes.txt` (`atual=` e `anterior=`, CRLF) e o lançador
  `Vitral.exe`. A instalação abre o pacote numa `.novo-*`, testa o `vitral.exe` novo
  (`--versao --json`, 10 s, sem janela), põe no lugar com um rename (tenta por 2 s, o antivírus
  segura o `.exe` novo por um instante), troca o lançador da raiz se o do pacote é outro (pela
  soma; aberto naquele instante, fica para a próxima abertura da versão atual), grava o
  `versoes.txt` ao lado e troca de uma vez (`MoveFileExW` com `MOVEFILE_WRITE_THROUGH`; o
  `estado.json` e as outras gravações do atualizador também), grava o `estado.json` e tira as
  `app-*` que não são a atual, a anterior nem a que roda, por `.lixo-*`. A pasta de um `.exe` em
  uso não sai: fica inteira para a limpeza da abertura seguinte (que só tira versões com um
  `versoes.txt` que aponta para uma inteira; sem ele, o lançador escolhe entre as pastas). Uma
  queda em qualquer passo deixa o lançador abrindo uma versão inteira (teste com os pontos de
  falha, que roda nos dois sistemas: o layout do Windows compila em todo lugar, e o `versoes.txt`
  é lido pelo mesmo `lancador/src/escolha.rs`). O Reiniciar vai pelo lançador da raiz (o GPUI
  espera o processo sair com o PowerShell e abre o `Vitral.exe`, que abre a versão nova); a volta
  da saúde abre a anterior direto e sai. A sessão do PC, para a saúde, é a hora do boot (agora
  menos o `GetTickCount64`, no minuto), e a thread da atualização roda no modo de fundo
  (`THREAD_MODE_BACKGROUND_BEGIN`). O `--instalar` é só do Linux; no Windows, o compilado no PC e o
  zip portátil só avisam, com o **Baixar** (página do release), porque o atalho, o desinstalador e
  o registro são do instalador.
- **Atualização automática** (`src/atualizacao/`): a thread `vitral-atualizacao` (`nice 10`)
  procura 60 s depois de abrir e a cada 6 h (mais uma folga de até 10 min), nunca antes de 1 h da
  última procura que deu certo, e 1 h depois de um erro de rede. Lê o `vitral-versao.json` do
  `releases/latest` (sem API e sem token; com o ETag, um 304 basta), confere a assinatura antes de
  ler o JSON (o comentário confiável tem de ser "vitral <versão> vitral-versao.json"), valida o
  manifesto e só fica com versão maior e sem sufixo. No modo **Automáticas**, o padrão, o release
  instalado na pasta do app baixa o pacote para `app/.baixando` (até 200 MB, com 3 vezes o tamanho
  livre no disco, tamanho e SHA-256 conferidos no caminho), instala como acima e fica **pronto**:
  vale quando reiniciar, e o Vitral nunca reinicia sozinho. **Só avisar**, o compilado no PC (canal
  "pc", também o do `instalar-dev.sh`) e o pacote aberto fora da pasta do app só avisam; o
  "Instalar" (no compilado no PC, o "Instalar como app") baixa e instala do mesmo jeito. Três falhas
  seguidas da mesma versão (arquivo diferente, binário que não abre) desistem até sair outra; sem
  release publicado (404), está em dia. O `~/.config/vitral/atualizacoes.json` guarda o modo
  (`automaticas`, `so_avisar` ou `desligadas`), a última procura, o ETag e as falhas.
  `VITRAL_SEM_ATUALIZACAO=1` desliga a thread (a bancada usa). Fora do canal estável (compilado no
  PC, o canal de teste ou pré-lançamento), `VITRAL_ATUALIZACAO_BASE=https://github.com/VictorMenegate/vitral/releases/download/v0.3.0-rc.2`
  procura nesse release e aceita pré-lançamento; fora do canal estável (o canal embutido na
  compilação; no estável a conferência recusa sempre), `VITRAL_ATUALIZACAO_CHAVE=<arquivo .pub>`
  junta uma chave às embutidas, para a bancada servir um release falso assinado com uma chave dela.
  O canal **teste** (`VITRAL_CANAL=teste`) é o do aceite no runner do Windows: instala sozinho na
  pasta do app como o estável e, só nele, `VITRAL_TESTE_CAI=<versão>` faz essa versão cair depois
  de contar a abertura e antes do primeiro quadro (a volta automática de verdade). A interface (`src/ui/atualizacao.rs`)
  lê a `Global` `Atualizacoes` (estado, modo, permissão, última e próxima procura, o aviso da
  abertura e a volta possível) e redesenha com `observe_global`: a Janela só quando a versão pronta
  muda (o ponto e o item do menu), as Configurações a cada mudança, e o progresso do download vai
  no máximo 2 vezes por segundo e só enquanto as Configurações estão abertas (o
  `acompanhar_progresso` devolve uma guarda que elas seguram). As frases saem de funções puras do
  estado (`linha_da_versao`, `texto_do_erro`, `megas`, `botoes`, `texto_do_aviso`), com teste; os
  avisos de versão usam a mesma chave e se substituem, e a Janela guarda qual já mostrou (um por
  versão). As novidades vêm do `notas` do manifesto, do `NOVIDADES.md` que veio com a versão na
  pasta do app ou, para a versão que roda, do próprio binário (`include_str!`). O Reiniciar
  (`fechar::reiniciar_pela_janela`) passa pelo `pode_parar_tudo` com `Parar::Reiniciar`; o texto da
  pergunta é `confirmacao_de_parar`, com teste do plural.
- **Saúde e volta** (`src/atualizacao/saude.rs`): no começo do `main`, antes do GPUI (e depois do
  `--versao` e do `--instalar`), `saude::ao_abrir` olha o `estado.json` da versão gerenciada que
  roda. A instalação grava ali `confirmar = { versao, aberturas: [] }`; cada abertura da versão do
  link `atual` sem confirmar entra com a sessão do PC (`/proc/sys/kernel/random/boot_id`; no
  Windows, a hora do boot) e a hora,
  e as de outra sessão saem (uma queda de energia não conta). Com duas aberturas sem confirmar na
  mesma sessão, a terceira põe a versão em `recusadas` (a procura não a oferece de novo), grava
  `voltou` e só então aponta `atual` para a anterior (que precisa responder ao `--versao --json` e
  ler o formato do disco); o `main` faz `exec` dela com os mesmos argumentos, e ela conta a volta
  uma vez. Uma volta cortada entre gravar o estado e trocar o link termina na abertura seguinte. A
  versão fica confirmada 10 s depois do primeiro quadro (`atualizacao::primeiro_quadro`) ou ao
  fechar normalmente (`on_app_quit`). A primeira abertura depois de uma atualização conta
  "Atualizado". Tudo com a trava das instalações (outro Vitral instalando: a abertura não conta) e
  a gravação atômica (temporário + fsync + rename). A volta à mão (`saude::voltar`) faz o mesmo,
  com `automatica: false`, e quem chama reinicia. A regra do formato: `dados::formato_em` lê o
  `formato` da pasta de dados (`~/.local/share/vitral/formato`; sem ele, 2 se já há `estado.json`
  e 1 se não há), que a migração grava logo depois do `estado.json` e cada abertura confere
  (`Dados::marcar_formato`, que nunca abaixa o número); a volta só vai para uma versão cujo
  `formato_dados` (do `estado.json` do app) seja maior ou igual a ele. A cópia de antes é o próprio
  `.vitral/` das pastas antigas, que a migração só lê. A saúde roda depois da trava (a abertura que só
  entrega o pedido ao Vitral aberto não conta) e antes da migração (a que cai migrando conta).
  `app/` mora dentro da pasta de dados (`dados::PASTA_DO_APP`), e nada dos dados mexe
  nela: o Começar do zero afasta tudo menos `app/`, e o `estado.json` do app não é o dos dados.
- **Desenvolvimento**: o `ferramentas/instalar-dev.sh` compila este checkout em release (pelo
  `compilar.sh`) e instala no mesmo layout como `<próxima>-dev.<commit>` (`VITRAL_VERSAO` na
  compilação; `.modificado` no fim com mudança sem commit). Pelo semver isso é menor que a
  próxima, e o release dela o substitui. Para o dono, instalar é só pelos releases.

## Arquivos

| Arquivo | O que faz |
| --- | --- |
| `src/motor.rs` | Os chats (criar com `novo_chat(pasta)`, a lista em `chats_da_lista`, renomear, fixar, arquivar, o que espera você), os turnos e a fila, contas, retomadas, Ultracode e agentes em segundo plano, os pedidos de permissão e a passagem de sessão |
| `src/motor/pedidos_de_hook.rs` | O que os hooks `PreToolUse` do usuário mandaram perguntar: o motivo de cada um, do evento do hook até o pedido de permissão da chamada, e a regra de que esse pedido nunca passa pelo "Não perguntar de novo" |
| `src/motor/conversas.rs` | O que liga as conversas umas às outras e ao usuário: `listar_chats`, `mensagem_para_chat` e `abrir_chat`, o freio de ciclo e o resumo do que aconteceu enquanto você esteve fora |
| `src/motor/passos.rs` | A lista de passos do agente de cada chat (o `TodoWrite` e as ferramentas de tarefa do Claude Code) e a frase do que cada chamada mudou nela, para a trilha |
| `src/motor/fundo.rs` | Os agentes em segundo plano de cada chat: a lista do `background_tasks_changed`, a chamada que abriu cada um, o fim do arquivo de cada um lido aos poucos (a última ação), e o item que marca o fim deles na conversa |
| `src/motor/revisao_do_par.rs` | A revisão do turno pelo outro agente: quando roda sozinha (20 linhas ou mais, a opção ligada, o outro à mão), o revisor que entra como parceiro e sai no fim, o resultado (a linha ou o cartão) e a entrega da revisão ao agente do chat |
| `src/ui/mensagens/revisao.rs` | O cartão da revisão do outro agente, com "Mandar ao Claude", e o "Revisar com o Codex" da linha "Trabalhou…" |
| `src/motor/subconversa.rs` | A conversa de um subagente aberta numa aba: o chat de mentira, só de leitura, fora do estado; achar e reler o arquivo dele em segundo plano enquanto ele trabalha; qual subagente espera uma permissão sua e o relatório entregue que vai para o cartão |
| `src/motor/importacao.rs` | A importação no motor: um chat direto na pasta de cada conversa do Claude Code, a da primeira abertura, a do menu, a vigia (a cada abertura e a cada 2 minutos, só o que mudou desde a passada anterior) e a marca de quando ela continua no Vitral |
| `src/dados.rs` | Pasta de dados global (`~/.local/share/vitral`, a configuração em `~/.config/vitral`): caminhos, gravação que aguenta queda de energia (temporário com fsync, `.anterior` por link duro, rename e fsync da pasta), o gravador em segundo plano (que guarda a gravação que falhou para tentar de novo), `carregar` com volta ao `.anterior` e a guarda do `estado.versao-N.json`, a `normalizar` (os dados das versões com projetos: só chats, cada um com a pasta dele), o `formato` dos dados e a `DADOS_GLOBAIS` |
| `src/migracao.rs` | Migração do Vitral antigo (o `.vitral/` de cada pasta de trabalho) para os dados globais: uma vez só, no primeiro uso, sem escrever nada nas pastas antigas, com o `estado.json` gravado por último; adiada com o Vitral antigo aberto |
| `src/instancia.rs` | Um Vitral só por usuário: a trava (`flock` em `~/.local/share/vitral.trava`), o socket (`$XDG_RUNTIME_DIR/vitral/vitral.sock`, pasta 0700) por onde uma segunda abertura entrega o pedido e sai, a `Abertura` (`vitral <pasta>`, `--nova <pasta>`) e a reexecução da tela de recuperação |
| `src/imagens_recebidas.rs` | As imagens que uma ferramenta devolve no resultado: o bloco em base64 (o da API do Claude e o do MCP) vira `ImagemRecebida`, que o motor grava em `imagens/<chat>/` com o hash do conteúdo no nome (`guardar`, `guardar_na_chamada`); os tetos por imagem, por chat e de todas (`podar`), e a faxina de quando o Vitral abre (`varrer`) |
| `src/importar.rs` | Leitura das conversas do Claude Code (só leitura): a listagem com título, datas, mensagens e se está aberta (o `Filtro.desde` da vigia pula o que não mudou), o histórico como itens da conversa e o registro do que já veio; `importar/subagente.rs` acha e lê o arquivo que o Claude Code grava para cada subagente (`subagents/agent-<id>.jsonl`) |
| `src/claude.rs` | Processo do Claude Code em `stream-json` (argumentos e ambiente do turno), leitura dos eventos, versão do Claude Code, conferência da sessão, os modelos que ele oferece (o `initialize`, em `atualizar_modelos_oferecidos`) e o detalhe de cada chamada de ferramenta (`detalhe_ferramenta`: o diff do Edit, o começo do Write, o comando do Bash, até 60 linhas de cada lado) |
| `src/mcp.rs` | Servidor MCP local com as ferramentas do Vitral (`listar_chats`, `mensagem_para_chat`, `abrir_chat` e `permissao`), uma rota por chat com o token da sessão |
| `src/prompts.rs` | O texto que o Vitral põe no prompt de sistema de cada chat: onde ele trabalha (direto na pasta, Sem pasta ou continuando uma conversa do Claude Code) e as convenções `[de: …]` e `[Vitral]`; no fim dele, as instruções de quem usa (`com_instrucoes`) |
| `src/perfis.rs` | Os perfis de agente (`Config.perfis`): o que cada um guarda, guardar, remover e mudar de ordem, o que ele põe no chat e quando ele bate com o chat |
| `src/versoes_agentes.rs` | A versão do Codex instalado e a mais nova publicada do Claude Code e do Codex (as etiquetas do pacote no npm, guardadas por uma hora), para o grupo Instalados das Configurações |
| `src/diagnostico.rs` | O texto do diagnóstico para pedir ajuda e a cobertura do que tem cara de segredo (`cobrir`) |
| `src/processos/` | Todo processo que o Vitral abre (`novo_comando`, sem janela de console no Windows) e os que ele para por inteiro: escopos do systemd ou grupos de processos (`unix.rs`), Job Objects (`windows.rs`); `entrada.rs` escreve na entrada do claude e do codex numa thread própria, porque o pipe enche em 64 KiB e a janela não pode esperar o processo ler |
| `src/sistema/` | O que abre fora do Vitral, nos dois sistemas: editor, terminal, gerenciador de arquivos, o bash dos comandos (no Windows, o do Git para Windows, nunca o do WSL) e o aviso do que falta instalar (`linux.rs`, `windows.rs`) |
| `src/contas.rs`, `src/contas/` | Contas extras (`CLAUDE_CONFIG_DIR`), login e o uso e o plano (o que o Claude Code guarda em `.claude.json` e a busca direta na Anthropic, `busca.rs`) |
| `src/codex.rs` | Uso do plano do Codex, lido das sessões em `~/.codex/sessions` |
| `src/agente_gemini.rs` | O Gemini como agente de um chat, pelo `agy acp` (ACP): o canal da entrada, a tradução da saída para os eventos do Claude Code, o pedido de permissão, os modos e o catálogo de modelos |
| `src/agente_codex.rs` | O Codex como agente de um chat: os argumentos do processo e o protocolo JSON-RPC em `src/agente_codex/servidor.rs`, a tradução da saída para os eventos do Claude Code, o sandbox de cada modo e o catálogo de modelos |
| `src/git.rs` | O comando do git (`novo_git`, o único jeito de abrir o git), o caminho relativo em texto e de onde saiu uma cópia isolada das versões de antes (para a carga pôr o chat de volta na origem) |
| `src/modelo.rs` | Os tipos do que vai para o disco (chats, ocorrências, filas e a configuração global, com os nomes antigos como alias), os tipos só lidos das versões com projetos (`ProjetoAntigo`, `TarefaAntiga`, `PapelAntigo`) e o `FORMATO_DADOS` e o `VERSAO_ESTADO` |
| `src/atualizacao/` | Versão embutida e os comandos sem janela (`--versao [--json]`, `--instalar`), o estado da atualização para a interface (`mod.rs`), a thread e o ciclo (`procura.rs`), o manifesto (`manifesto.rs`), a assinatura (`assinatura.rs`), o download (`rede.rs`), o pacote e a conferência do binário novo (`pacote.rs`), a instalação por versão e o menu no Linux (`linux.rs`), a instalação por versão, o `versoes.txt` e o lançador no Windows (`windows.rs`), o `atualizacoes.json` (`config.rs`) e a saúde da versão nova com a volta para a anterior (`saude.rs`); as fixtures ficam em `teste/` (`teste/ponta/` é o release falso dos testes de ponta a ponta, assinado com outra chave de teste) |
| `src/tema.rs` | Tokens da linguagem visual (cores do vidro e das partes pretas, medidas em rem com `r()` e `tam()`, tipografia, sombras e gradientes), tema do GPUI Kit, cores de sintaxe e ícones extras do Lucide |
| `src/ui/` | O alto da lista (`trilho.rs`: o logo, o menu do Vitral e o sino), a lista (`lateral.rs`), os painéis (`conversa.rs` é a casca: as abas, o cabeçalho, o menu do painel, a doca e o popover de uso da conversa; `mensagens.rs` o que vai dentro, com o cartão de pedido de permissão, o do plano que ficou na conversa e a linha de estado da caixa, `pergunta.rs` o cartão da pergunta com opções (AskUserQuestion) e as respostas dele, e `mensagens/historico.rs` o histórico das conversas trazidas do Claude Code, lido em segundo plano, com paginação automática ao subir, e `mensagens/a_vista.rs` a conta de quais blocos da conversa são montados e as alturas guardadas dos outros, e `mensagens/no_fundo.rs` as linhas dos agentes em segundo plano e a do fim de cada um; `vista_mensagens.rs` as mensagens de cada painel e a lista como vistas com cache; `pilulas.rs` a linha de pílulas acima da caixa; `checagens.rs` as checagens (teste, typecheck, lint, build) que o turno rodou, para a linha "Trabalhou…"; `paineis.rs` os painéis lado a lado, as abas de cada um e a soltura; `arranjo.rs` como a janela estava ao fechar), `links_arquivo.rs` (os caminhos de arquivo na resposta que viram links e abrem no painel da direita, na linha citada), `composer.rs` (a caixa de mensagem) e `composer_seletores.rs` (os chips, menus e popovers dela), `menus.rs` (o menu da conversa, igual em todo lugar), `dialogos.rs` (Novo chat e a paleta das pastas, renomear, contas, configurações, mencionar arquivo e conversa como contexto), `este_chat.rs` (a direita: onde o chat trabalha), `abas_direita.rs` (as abas da direita), `painel_arquivo.rs` (o arquivo aberto à direita, com a edição, a escolha de linhas e o Ctrl+F dentro do arquivo), `editor_arquivo.rs` (o editor do arquivo aberto: a conta com o disco, a gravação sozinha e o conflito quando o agente grava o mesmo arquivo), `previa_markdown.rs` (o frontmatter e os links da prévia de um .md), `edicao.rs` (a edição de uma chamada posta sobre o arquivo de agora: os dois formatos de diff, os trechos achados e as fileiras), `mencao.rs` (as menções de arquivo como texto e como pílula, no balão e na caixa), `caixa_entrada.rs` (o popover do sino e o Ctrl+G), `resumo.rs` (o freio de ciclo em Esperando você e o Enquanto você esteve fora), `abertura.rs` (o que fazer com a pasta e o `--nova` de uma abertura, ao abrir e pelo socket), `recuperacao.rs` (a tela de quando os dados abrem só para leitura, com Começar do zero e Tentar de novo), `uso_plano.rs` (os anéis de uso no canto da janela e o popover deles), `atualizacao.rs` (a atualização: avisos, Novidades, o grupo das Configurações e os textos de cada estado), `fechar.rs` (a pergunta antes de fechar ou reiniciar), `busca_conversa.rs` (o Ctrl+F dentro da conversa: a barra, o índice das ocorrências e o realce), `fila.rs` (a lista das mensagens que esperam o turno, com o Entregar agora), `reenvio.rs` (corrigir e mandar de novo a última mensagem), `rascunhos.rs` (o que você escreveu e não enviou, guardado por conversa no `interface.json`), `imagens.rs` e `visor_imagem.rs` (as imagens do chat e o visor), `seletor_arquivos.rs` (os arquivos da pasta para mencionar com `@` e os ignorados da aba Arquivos), `atalhos.rs` (a tabela única das teclas, que vira os `KeyBinding` e o diálogo "Atalhos do teclado" do `?`), `zoom.rs`, `movimento.rs` (a única porta de animação: durações, curvas, teto de 60 quadros por segundo, entradas e transições, o laço do metal e o `Segurar` do pedido perigoso) |
| `src/ui/componentes/` | Os componentes da linguagem visual (chip, botões, segmentado, popover, menus, alertas e avisos, cartões de lista, opções, lista de escolha, selos, abas, diálogos, caixa de código), os anéis com fenda e o anel que gira (`graficos.rs`), a moldura dupla, o azulejo e o botão de segurar (`controles.rs`), a placa de metal, o cabeçalho de objeto, a ficha do escolhido e a faixa do pé dos menus (`superficies.rs`), o metal líquido (`metal.rs`, o motor: a chapa da janela, as aberturas e o aro de metal; `malha.rs`, a conta da receita ponto a ponto e a malha do botão do menu), o estado único (o sinal de cada chat) e a vitrine (`--vitrine`) |
| `pacote/` | `manifesto.py` (o `vitral-versao.json` do release), as chaves públicas (`vitral.pub` e `vitral-reserva.pub`), o `linux/instalar.sh` e o `windows/vitral.iss` (o instalador do Windows, Inno Setup 6) |
| `lancador/` | O lançador do Windows (`vitral-lancador`, só a std, o `Vitral.exe` da raiz da instalação): `escolha.rs` lê o `versoes.txt` e escolhe a `app-<versão>`; o `build.rs` põe o ícone, a versão e o manifesto. Fora do Windows o `main` só avisa e sai com 2 |
| `ferramentas/logo/` | O logo: `v-do-dono.png` (o V que o dono mandou), `rasterizar.py` (tira o V dele e grava o `assets/vitral-v.png`), `icones.py` (o metal com o V por cima: os `assets/vitral-*.png`, o `vitral.ico` e os dois SVG com o PNG embutido) e `previa.py` (a conta do metal em Python, que o `icones.py` usa); ver **Logo**, em **Acabamento** |
| `ferramentas/instalar-dev.sh` | Só desenvolvimento: compila e instala no layout do app como `<próxima>-dev.<commit>` |
| `.github/` | `workflows/ci.yml`, `workflows/release.yml`, `workflows/windows-prints.yml` (à mão: prints num Windows de verdade, pelo `ferramentas/prints-windows.ps1`; o instalador de uma rodada do release, pelo `ferramentas/instalador-windows.ps1`; e o atualizador, da versão publicada a três do ramo num release falso, pelo `ferramentas/atualizador-windows.ps1`) e `dependabot.yml` |
| `build.rs` | No Windows, o ícone e a versão no `vitral.exe` (o manifesto vem do GPUI); fora dele, nada |
| `tests/versao.rs` | `--versao` e `--versao --json` sem tela (o binário de verdade, sem DISPLAY nem WAYLAND_DISPLAY) |
| `tests/instalar.rs` | `--instalar` com o binário de verdade num HOME temporário em `target/tmp`: o layout, o link, o `.desktop`, o `~/.local/bin` e reinstalar sem duplicar |
| `README.md`, `docs/imagens/` | A porta de entrada do GitHub (o que é, instalar, conferir o download, limites) e os prints dela, tirados na bancada com `RAIZ_SINTETICA=1` e `CLAUDE_FALSO=1`, recortados e com paleta de 256 cores |
| `assets/` | Logo completo e pequeno (SVG com o PNG embutido), PNGs de 48 a 256 px, `vitral.ico` (16 a 256 px, para o Windows), todos gerados pelo `ferramentas/logo/icones.py`, `vitral-v.png` (o V branco de 256 px do botão do menu, gerado pelo `rasterizar.py`), logos do Claude e do Codex (e os discos coloridos em `marcas/`), o escudo cheio do modo Manual e as fontes embutidas (`fontes/`: Inter 4.1 e JetBrains Mono 2.304, só os pesos usados, OFL 1.1 em `fontes/OFL.txt`, que vai no pacote como `OFL-fontes.txt`) |

Ícones do Lucide que não estão entre os do GPUI Kit precisam entrar em `icon_assets!` em
`src/tema.rs`, senão saem em branco (um teste confere). Os textos dos componentes do Kit saem em
português (`set_locale("pt-BR")` em `src/main.rs`).

Medidas e zoom: tudo é medido a 100% e cresce com o zoom (`r()` dá rem; `tam()` dá Pixels para as
APIs que só aceitam Pixels); os raios, as sombras e os traços ficam fixos de propósito. As larguras
das partes também: `orcamento(util, escala, …)` em `src/ui/mod.rs` multiplica `LATERAL`,
`DIREITA`, `MIN_CONVERSA`, `FAIXA_LISTA`, `FAIXA_DIREITA` e as outras pelo zoom; `Larguras` guarda
o que o arrasto deixou, a 100%, e `em_px` dá as larguras do quadro. O estado das divisórias
(`estado_partes`) é da Janela e recomeça quando muda a composição ou o zoom, a partir das larguras
guardadas; a gaveta usa a largura guardada no zoom atual. Na lista, `Aperto::de(largura a 100%)`
decide o que sai, e o texto da busca é trocado em `lateral()` (`dica_da_busca`). A largura que
decide o `Aperto` fica guardada a 100% (o estado `largura-da-lista`), e assim a troca de zoom não
dá um quadro com a conta da escala anterior. A busca da lista é o `componentes::campo_de_busca_de`
na altura de 40, a do logo e a do "+" (a lupa a 8 da borda dele, que na peça de 8 cai na guia de
16), com o "Ctrl" "K" à direita só na aba Conversas e fora da lista estreita, e a dica por cima
(`campo_da_lista`). A barra de rolagem fina é `componentes::barra_de_rolagem` (sem fade: o `SCROLLBAR_*` do vendor
fica em zero e o teste `barra_de_rolagem_sem_fade`, em `tema.rs`, confere); o `BotaoCopiar` dá ao
Copiar do kit 24 crescendo com o zoom; o campo de texto do kit
tem a margem do lado em rem (o `composer::recuo_do_texto`, onde o texto da caixa começa e por onde a
faixa do turno se alinha, conta com isso); e os avisos têm a largura (`tema::LARGURA_AVISO`)
multiplicada pelo zoom em `zoom.rs`.

Espaços só na escala 2, 4, 6, 8, 12, 16, 24 e 32 (`tema::esp`, com os nomes por contexto: `PECA`,
`CAIXA`, `FLUTUANTE`, `GUIA_LISTA`, `GUIA_EQUIPE`, `NO_TURNO`, `ENTRE_TURNOS`, `ANTES_DE_SECAO`);
quem tem contorno de 1 px usa `dentro_da_borda(12.)` para o conteúdo ficar a 12 da borda de fora. Os
atalhos do Tailwind (`gap_2()`, `p_3()`) medem em rem e, com 1 rem = 15 px, saem fora da grade (7,5
e 11,25). O teste `espacos_na_escala` barra os dois casos em todo o `src/ui`, sem exceção, e o
`movimento_com_teto` barra animação feita fora de `src/ui/movimento.rs`. O `metal_so_pelo_motor`
barra `paint_image(` e `drop_image(` fora de `src/ui/componentes/metal.rs`. O `dica_pela_gerenciada`
barra o `.tooltip(closure)` puro do GPUI: toda dica vai pela gerenciada do kit (`dica_no_ponteiro`
para linha larga, `dica_com_hover` para quem já ouve o hover, ou a dos botões), que espera só na
primeira e, com uma aberta, mostra a próxima na hora (dono, 23/11/2026).

Raios (`tema::raio`, em Pixels fixos): nos controles, um padrão só, `raio::de(altura)`, 45% da
altura (16 num botão de 36, 14 num de 32, 9 num chip de 20; bem arredondado, nunca redondo, o
desenho do logo em escala), em botões (o `BotaoRedondo` inclusive), chips, abas, a busca, o
segmentado, selos, o realce dos itens de menu e as teclas; nas peças, os tokens: o
`raio_da_peca()` da variante (16 no Grafite) nos cartões da janela (a conversa, os painéis da
direita, o arquivo aberto) e no miolo das molduras duplas; `PECA` 20 nas gavetas, popovers, menus,
diálogos, avisos, na moldura da caixa de mensagem e na dos pedidos, e na tela cheia; `CARTAO` 14
no que vai dentro delas (cartões da direita e da conversa, balão,
cartão de lista de duas linhas, blocos de código e tabelas do chat, anotação,
grupos de opções, alertas, a caixa na Estreita e a dica); `CONTROLE` 10 em campos, linhas (de uma
linha na lista, árvore e log), blocos dentro de cartão (código), o azulejo e miniaturas; e `PILULA` (o `rounded_full()`, ou o `raio::PILULA` onde a API
só aceita Pixels) só no que continua redondo: pontos, anéis, barras finas, o disco dos anéis de
uso e o "Segure para permitir". Dentro de quem tem padding p, o raio é max(raio de fora − p,
`CONTROLE`), para os cantos ficarem concêntricos: o lugar de 40 do trilho tem 14 (20 − 6) e o
quadrado de 32 dentro dele, 10
(14 − 4); os menus têm margem 8 para os itens de 10 caberem no menu de 20; o popover do modelo,
6 mais os 4 da lista. Onde a conta não fecha, vale o token a até 2 do exato (concêntrico a olho)
ou o canto de dentro mais redondo (a caixa de mensagem fica com 20 dentro da conversa); para não
achatar mais que isso, aumenta o padding. Camadas por dentro (o reflexo, o
brilho da caixa, o cabeçalho tingido dos cartões) levam o raio da peça menos a borda. O tema do kit acompanha (`radius` 10, `radius.lg` 20) e o teste
`raios_na_escala` barra em todo o `src/ui` raio solto (`rounded(px(..))`, `rounded(r(..))`, número),
atalho do Tailwind (`rounded_md()`) e `ButtonRounded`, sem exceção.

Superfícies (no Grafite): atrás de tudo fica a chapa de metal (`componentes/metal.rs`, o
`metal::fundo`, primeiro filho da Janela); o `Cores::vidro` (#0d0f13, opaco) é só a reserva
debaixo dela e o que a raiz do kit pinta; `vidro_leitura` (#0a1533e3) é o véu azul-escuro da base,
que a Janela pinta por cima da chapa na janela inteira (a lista não tem fundo próprio), e `cartao`
(#14161df2) o da conversa e dos painéis, com a `sombra_flutuante`; `dialogo` e
`gaveta`, o opaco do que cobre a conversa sem ser um flutuante; `flutuante` (#171a23f7, igual ao
`popover.background` do tema, que o kit usa nos menus, popovers, dicas e avisos) é o quase preto do
que flutua; `cartao_elevado` é o miolo das molduras duplas; `preto` (#08090c) é o de dentro
(código, saída de comando, diff, a moldura da caixa e o disco dos anéis de uso); e `caixa_quente`
e `caixa_preta` são o campo da caixa, hoje o mesmo cinza opaco (sem gradiente). O `recuo` escuro
some no quase preto: dentro de um popover vale o `recuo_no_flutuante` (no segmentado,
`.no_flutuante(true)`). Os testes de `tema.rs`
conferem o contraste do texto em todas elas, a sintaxe a 4,5:1 no preto (também nas linhas que
entraram e saíram do diff) e que o hover e a seleção continuam à vista no preto. O `BotaoRedondo`,
como o logo e os anéis, não mostra a dica com o popover dele aberto.

Movimento: `src/ui/movimento.rs` tem `COR` 120, `ENTRAR` 160, `MOVER` 180 e `ENCHER` 240 ms; `Curva`
(Entrar, Mover, Sair, sem ricochete); `animacao(d, c)` para o `with_animation`; `entrar(el, id, De)`
para o que aparece; `valor`/`valor_desde` para um valor que anda até o alvo quando ele muda (a
montagem não anima); `disparo` para um efeito que toca quando uma chave muda; `Hover` para o
ponteiro num controle solto; `JaVistos` para o que acabou de aparecer numa parte da tela (o que a
casca do painel mostrou no desenho anterior, `Painel.ja_vistos`, virado no começo de
`Janela::conversa` e esquecido ao trocar de conversa: os pedidos, a fila, cada pílula, o estado da
faixa, a linha de estado da caixa, o painel vazio); `AbertosAgora` para o que um clique acabou de
abrir na conversa (`Janela.abertos_agora`, marcado pelo `alternar` e pelos cliques da trilha;
vale 400 ms, então o conteúdo aberto entra uma vez e uma remontagem depois aparece pronta);
`lado_da_navegacao` (as partes das Configurações); e, em `mensagens.rs`, `chevron` (o de abrir e
recolher, que gira 90° pelo `valor`), `entrada_da_chamada` e `entrada_do_cabecalho` (com teste).
Uma `entrada` que deixa de ser montada no meio perde o estado: quem decide entrar mantém o
invólucro (o `Some(false)` do `entrada_do_bloco`) enquanto o elemento fica à vista; `abertura(id)` e `deslizar_gaveta(el, De::Esquerda | De::Direita, t)`
para a gaveta (24 px em `MOVER`, com o véu a 0,5·t; a `Janela.geracao_gaveta` sobe em cada
abertura, e fechar é seco). Tudo no relógio de 60 qps por janela, e a última batida já pousa no
alvo. O que anda sem parar (o metal) passa pelo `laco_a(window, cx, qps)`: a fase em `f64`, uma
batida marcada por janela (um pedido mais rápido marca outra, mais cedo; a velha, ao chegar, não
vale), `anima` (movimento ligado e janela em primeiro plano) e `laco_agendado`; com `qps` 0 ele
devolve a fase sem andar nem pedir quadro. `tema::aplicar` chama `movimento::iniciar`, que lê o `enable-animations` do GNOME uma vez e a
escolha das Configurações (`"animacoes": "desligadas"` no `interface.json`, gravado pelo
`zoom::gravar_na_interface` sem perder o zoom) e liga o `set_reduce_motion`;
`movimento::escolher_animacoes` troca na hora e, voltando a Seguir o GNOME, devolve o controle ao
kit, que acompanha o sistema. O
contador do `VITRAL_QUADROS` fica em `src/ui/mod.rs` (`contar_quadro`, chamado no `render` da
Janela e da vitrine). Na conversa, `movimento::com_estado` deixa quem só tem o `&Context` da Janela
usar o estado guardado (o aro de metal da caixa, a luz e a altura da doca, as entradas), e
`movimento::entrada` faz o fade com deslize de algo que acabou de aparecer: `nova` só no primeiro
quadro, então um remonte (outro
painel abriu, a lista recolheu) não repete nada nem redesenha. Os blocos usam `Painel.base_animacao`
(o tamanho da conversa quando ela abriu no painel) e `Painel.vistos` (`entrada_do_bloco`, com
teste). O pulso do enviar sobe com `Painel.envios`, em `paineis::enviar_do_painel` (o Enter e o
clique). O pulso do sino
é o `BotaoRedondo::pulso_cor` com o `anel_do_pulso`, e a geração sobe só quando o número aumenta
(`sino_numero`, `sino_geracao`, zerados ao trocar de pasta).

Ponteiro fora e janela sem foco: com o ponteiro fora da janela e sem arrasto, a Janela põe por cima
a camada `ponteiro-fora`, que tira o hover preso (o Wayland não move a posição guardada quando o
ponteiro sai); a dica do vendor também não abre assim. Sem foco, o compasso dos spinners não agenda
batida e devolve a fase parada, o `observe_window_activation` redesenha na volta e o relógio de 1 s
não redesenha quando o compasso já está batendo.

Motor e redesenho: o `Motor.visiveis` (as conversas que algum painel mostra) é atualizado no render
da Janela sem notify; `tratar` devolve se o lote pede redesenho, e texto parcial de uma conversa
fora da vista só redesenha no primeiro pedaço (`juntar_parcial`, com teste). Numa conversa aberta,
o primeiro pedaço redesenha a janela e os seguintes só os painéis dela (`pedaco_de_texto` e
`aviso_do_lote`, com teste; ver **Custo de um redesenho**). `salvar_estado` e
`salvar_config` serializam na hora e entregam os bytes à fila do gravador. A notificação é
`Motor::notificar`: `notify-send -a Vitral -i vitral -A default=Abrir -p`, numa thread que guarda
o id por conversa para o `--replace-id` seguinte; o clique manda `Sinal::Abrir`, e o observer da
Janela seleciona a conversa e chama `activate_window`.

Chats, arquivar e esquecer: a interface só fala com os chats pela API do motor: `chat(id)`,
`chats_da_lista()` (tudo o que monta ou conta a lista passa por ela), `novo_chat(pasta, criado_por)`
(sem pasta, uma pasta vazia em `dados/avulsos/<id>`), `pastas_dos_chats()` (a paleta do Novo chat),
`sem_pasta(chat)`, `renomear`, `alternar_fixado`, `alternar_arquivado`, `arquivar_chat` (o chat que
nunca rodou sai do Vitral em vez de ir para Arquivadas) e `arquivar_varios`. O `esquecer` limpa os
mapas do motor das conversas que saíram e nega os pedidos de permissão delas. O gravador apaga
arquivos na vez deles (`Gravador::apagar`, que tira da fila uma gravação pendente do mesmo arquivo):
as mensagens dos chats saem depois do estado novo.

Dados e normalização: `Dados` (em `dados.rs`) tem o gravador: estado e configuração com
`.anterior` por link duro (o link vai num `.anterior.tmp` e entra por rename, então nunca há um
momento sem `.anterior`), os chats sem. Uma gravação que falha fica em `falhas` até o mesmo arquivo
gravar; `Motor::gravar_sujos` (a cada 5 s) liga a faixa de falha, põe as falhas de novo na fila e,
quando volta, entrega as filas que esperaram. `carregar` lê cada `chats/<id>.json` item a item: o que
não converte vira `Item::Desconhecido` e é regravado como veio; `Chat` e `Estado` têm `resto`
(flatten), e uma `versao` maior que a suportada abre só para leitura. Um estado de versão anterior
(`versao` 1 ou 2) é guardado como veio em `estado.versao-N.json` antes da primeira gravação
(`guardar_estado_antigo`, uma vez só). `salvar_config` relê o `config.json` como `Value`, tira as
chaves legadas (`modelo_coordenador` vira `modelo_mestre`, `modelo_trabalhador` vira
`modelo_chats`; com as duas, vale a nova) e mantém as desconhecidas. O que as versões com projetos
gravavam e esta só lê fica nos campos `#[serde(skip_serializing)]`: `Estado.projetos`
(`ProjetoAntigo`: id, pastas, arquivado, modelo), `Estado.tarefas` (`TarefaAntiga`: a forma, a
origem e a cópia), `Chat.projeto_antigo`, `Chat.papel_antigo` (`PapelAntigo`), `Chat.pai` e
`Chat.tarefa`. A `normalizar(estado, dados, consultas, agora)` roda no `Motor::new`, antes de tudo,
com o trait `Consultas` (o `NoDisco` de verdade, um falso nos testes): tira o Mestre, converte os
trabalhadores do coordenador antigo em chats (o do pedido encerrado vai para as arquivadas; o dos
outros ganha o aviso), tira os ajudantes, põe de volta na origem quem trabalhava numa cópia
(`voltar_para_a_origem`: a origem gravada, a do git da cópia ou a pasta principal do projeto; a
sessão nascida na cópia vira uma nova, com a passagem `SaiuDaCopia`), passa aos chats o que era do
projeto (arquivado, modelo), poda as ocorrências velhas e recolhe os chats vazios com mais de 24 h;
no fim, as tarefas, os projetos e as marcas de papel e projeto dos chats saem, e o `Motor::new`
grava tudo antes de qualquer turno. O teste `estado_com_projetos_abre_so_com_chats_de_pasta` faz o
caminho inteiro com um `estado.json` de verdade da versão 2. O prompt de sistema vai por arquivo
(`~/.cache/vitral/prompts/<chat>.md`, regravado a cada turno) com `--append-system-prompt-file` e
`--system-prompt-snapshot off` a partir do Claude Code 2.1.278
(`claude::VERSAO_PROMPT_POR_ARQUIVO`).

Migração: `migracao::migrar(dados, raizes, proc, pid, na_bancada, quando)` devolve `NaoPrecisa`,
`SemRaizes`, `Migrou(Relatorio)`, `Adiada { raizes, pids }` ou `Falhou { raiz, erro }` (as duas
últimas viram a tela de recuperação). Ela trabalha sobre `serde_json::Value` (o que esta versão não
conhece passa como veio): `ler_raiz` lê sem escrever (os bytes dos chats, o texto das notas, o mtime
e o tamanho do `estado.json` e a pasta de cada subpasta, de onde sai a origem dos pedidos),
`planejar` é pura e trata as raízes juntas (ids das tarefas renumerados em duas passadas, a raiz
mais recente primeiro; só chats e pedidos, que a carga normaliza), e `aplicar` grava direto na pasta
final, com o `estado.json` por último e o `formato` logo depois dele. `vitral_antigo_vivo` varre
`/proc/*/comm == "vitral"`, menos o próprio processo, os filhos dele e, conforme o lado, os da
bancada (`DENTRO_DA_BANCADA=1`; dentro dela, só o da mesma bancada, pelo `XDG_DATA_HOME`). O
`migracao.json` guarda, por raiz, o mtime e o tamanho do `estado.json` antigo (o aviso de que o
Vitral antigo mexeu depois) e as tarefas renumeradas. A ordem do `main`:
`--versao`/`--instalar`, dados, trava (ou a entrega ao Vitral aberto), saúde da versão, socket,
migração, carregar, `processos::limpar_sobras`, janela.

Textos e pendências: `textos::duracao_curta` é o formato único do tempo ("19 s", "2 min 13 s",
"1 h 05 min", com dois dígitos depois do primeiro para o tempo que anda não mudar de largura), usado
pela lista e pelo relógio do turno. `lateral::Causa` (Permissao, Plano,
Parada, Erro; `da_pendencia` para o sino, `da_espera` para a lista) e `frase_da_pendencia` dão a
mesma frase e o mesmo sinal na lista e no sino, desenhados pelo mesmo `previa_elemento` (teste
`a_mesma_pendencia_diz_o_mesmo_na_lista_e_no_sino`). Na conversa, o painel que está no fim pede
`scroll_to_bottom()` a cada desenho; `acompanhar_rolagem` solta o fim em qualquer subida (1 px) e
prende de novo nos últimos 48 descendo, e `Painel.manter_lugar`, ligado por `alternar()`, segura o
lugar quando um bloco no fim abre.

Ultracode e agentes em segundo plano: em `src/claude.rs`, os argumentos e o ambiente do turno são
funções puras (`argumentos`, `ambiente`, `configuracoes_do_turno`), com teste. O Ultracode vai como
`--effort ultracode` (o jeito que liga antes e depois da 2.1.284; antes dela o Ultracode só vale em
Extra) e, no mesmo `--settings`, `"enableWorkflows": true` (no plano Pro os fluxos começam
desligados) e `"ultracode": true`. Fora do Ultracode, `Workflow` vai em `--disallowedTools`, junto
com o `EnterWorktree` e o `ExitWorktree` (o chat fica na pasta dele: a sessão é gravada pela pasta).
O limite de agentes vai em todo turno. O
`Saiu` traz o código de saída (lido sem colher o processo, em `processos::codigo_de_saida`). A
versão do Claude Code vem do link `versions/<versão>` do instalador ou de `claude --version` e, a
cada init, de `claude_code_version`, junto com a ferramenta `Workflow` na lista (ou não). O
`background_tasks_changed` conta os fluxos e subagentes vivos (os shells em segundo plano o próprio
Claude Code encerra). Em `src/motor.rs`: `Entrada.ultracode` e `Item::Usuario.ultracode` (gravados
só quando ligados; a fila guarda e sobrevive a um reinício), `Reforco { ultrathink, ultracode }` no
envio, `motivo_sem_ultracode`, `aceita_ultracode`, um Ultracode por vez (`ultracode_rodando`), o
fim do turno adiado enquanto há agentes em segundo plano (`fim_adiado`; `agentes_cortados` quando o
processo sai antes deles) e `entrada_para_repetir` na repetição automática da troca de conta.

Um chat, um processo (0.10.0): nunca há dois processos do agente na mesma conversa. Em 23/11/2026
um chat chegou a ter três `claude -p --resume` da mesma sessão, cada um achando que era o único.
O que disparou, medido no Claude Code 2.1.328: ao retomar uma sessão cujo processo anterior foi
derrubado com um agente em segundo plano (o Parar de quem esperava os agentes), ele manda o aviso
do agente cortado, põe a mensagem na fila (`command_lifecycle` `queued`), manda um `init` e um
`result` de sucesso vazio, com `num_turns` 0, e só depois começa a mensagem (`started`). O motor
tomava esse `result` pelo fim do turno: o chat parecia parado com o processo no meio do turno, e a
mensagem seguinte (ou a revisão da memória, ou o Continuar) abria outro processo, deixando o
primeiro rodando por fora, com agentes, sem que o Parar o alcançasse. A mensagem que entra no
processo vivo com agentes em segundo plano é do commit `d4691c3` (a entrada fica aberta na
espera; medido aqui no 2.1.328, com o Haiku: o `init` do turno dela sai 0,15 s depois, com o
agente ainda rodando). Aqui são três consertos:
`claude::VezDaMensagem` (na thread que lê a saída) tira o `result` que chega antes da vez da
mensagem com que o processo abriu; `motor/saidas.rs` guarda POR CHAT o processo que já terminou o
turno e está saindo (o que espera a sugestão continua em `sugerindo`), e `comecar_turno_com`,
o único lugar que abre processo de chat, não abre outro enquanto um deles vive
(`esperar_o_anterior_sair`: com o processo do chat ainda no mapa, o que ia abrir volta para a
frente da fila em vez de trocar o processo por outro; com um saindo, as mensagens ficam em
`a_abrir`, o chat conta como rodando e
`abrir_quando_sair` abre o turno quando o anterior sai, o que leva de 0,5 a 1,6 s no Claude Code,
ou uns 7 s com um comando de fundo); e quem não sai no prazo é derrubado (`PRAZO_DE_SAIDA`, 12 s;
5 s para o que foi parado ou deu erro; SIGKILL 5 s depois do SIGTERM). O processo que já tinha
terminado e manda o `init` de outro turno (um aviso de tarefa de fundo abre um sozinho) volta a
ser o processo do chat (`Motor::acordou`), menos o que foi parado ou o que tem turno novo
esperando por ele, que é derrubado. O Parar desfaz o turno que esperava (`desistir_de_abrir`: o
que você escreveu volta para a fila) e derruba o que ainda saía; fechar o Vitral derruba todos
(`encerrar_tudo`). O Continuar, o Tentar de novo e o "Continuar na outra conta" não entram por
cima de um turno rodando nem de um que espera para abrir (`com_turno`). No claude de mentira, `[ferramenta AgenteFundo <descrição> | <segundos>]` é o
agente que segue depois do `result` (o processo fica vivo por ele, a mensagem que chega com o
turno parado abre outro turno no mesmo processo, e o SIGTERM com agente rodando deixa a marca que
faz a retomada seguinte soltar o `result` vazio, como o de verdade;
`CLAUDE_FALSO_AGENTE_CORTADO=1` encena sem o corte); o roteiro é o
`ferramentas/roteiros/um-chat-um-processo.txt`, que conta os processos vivos por sessão a cada
passo (com o 0.9.5 o passo 06 dá 2).

Custo: o `total_cost_usd` do `result` é o total da sessão até ali, não o do turno. Ele cresce a
cada `result` do mesmo processo e, numa retomada (`--resume`), já vem com o dos turnos anteriores,
que o Claude Code grava no transcript da sessão. `Chat::somar_custo` guarda o último total com a
sessão a que ele se refere (`custo_da_sessao`) e soma ao `custo_usd` só a diferença; um total menor
(a sessão sem o total gravado, depois de uma queda) conta inteiro, e o zerado de um processo que
caiu ao abrir não conta. As conversas de antes disso guardaram o custo contado a mais (o total de
cada turno somado).

O Ultracode ainda não rodou num turno de verdade. Na primeira vez, conferir: com `ps -o args` no
`claude` filho, o `--effort ultracode` e o `--settings` com `enableWorkflows`; que o init traz
`Workflow` e o fluxo aparece como "Fluxo de agentes"; que o turno fica rodando até o fluxo acabar e
só então termina; e, no modo Manual (o Vitral pede permissão para o
`Workflow`), se os agentes do fluxo, que rodam em segundo plano, têm recusado o que precisaria de
permissão, como o binário 2.1.278 sugere (a camada `avoid_prompts`). Se confirmar, a linha do
Ultracode passa de aviso em âmbar a desabilitada fora de Sem perguntar e Automático; em Sem
perguntar tudo roda direto.

`vendor/gpui-component` é uma cópia do gpui-component 0.8.1 com poucas mudanças, todas marcadas
com "Vitral:" no código: a borda e a sombra da janela seguem o tema (`src/window_border.rs`; a
original desenha um contorno cinza fixo); menus, popovers e avisos têm o raio das peças
(`radius.lg`, `src/styled.rs`, `src/notification.rs`, e a seta do popover foge desse canto,
`src/popover.rs`), a dica 14 e as teclas, os campos de hora e o anel de foco do check e do rádio o
de controle (`src/tooltip.rs`, `src/kbd.rs`, `src/time/time_field.rs`, `src/checkbox.rs`,
`src/radio.rs`, `src/input/group.rs`), e o botão sai em pílula por padrão (`src/button/button.rs`);
os menus têm margem 8 e itens de raio 10, medidas em rem (crescem com o zoom), item perigoso, item
externo, linha fixa de cabeçalho e rodapé fora do teclado e de borda a borda (a placa de metal do
topo e a faixa do pé encostam na casca), check à direita, o separador no fio de pontos claro,
recuado até a coluna dos ícones (e escondido colado
na placa ou na faixa), o rótulo de seção em mono 11, o realce do item como tecla em relevo no
canto `raio::de` da altura dele (`realce_de_tecla`), uma camada por baixo do item para a ficha do
escolhido, desabilitado a 0,45, o atalho em teclas separadas também no item de elemento e
o ↓ sem nada realçado indo ao primeiro item navegável, não à linha fixa
(`src/menu/popup_menu.rs`, `src/menu/menu_item.rs`); menus, popovers e avisos têm o contorno de
casca (o anel claro e neutro, sem o azul de antes, o fio preto de 2 por fora e a aresta de vidro
por dentro) e a sombra flutuante do Vitral, e os diálogos a sombra própria (`src/styled.rs`,
`src/dialog/dialog.rs`); a dica é opaca, com as teclas separadas e "título\ndetalhe"
(`src/tooltip.rs`); o aviso tem o ícone na linha, o × sempre visível e entrada mais curta
(`src/notification.rs`); o `Kbd` escreve Super no lugar de Win e dá as teclas separadas
(`src/kbd.rs`); a palavra mudada do diff fica mais discreta (`src/diff/mod.rs`); os spinners e o
texto com brilho andam num compasso só, a 12 quadros por segundo para todos juntos
(`src/spinner.rs`, `src/shimmer.rs`); o realce de sintaxe de um trecho solto é público
(`src/highlighter`); o diff dá o texto de uma linha e abre a dobra de uma linha sem rolar
(`src/diff/document.rs`, `src/diff/state.rs`); o rádio marca com uma bolinha (`src/radio.rs`); a
barra de rolagem aparece e some seca e some 1,2 s depois do último uso (`src/theme/mod.rs`,
`SCROLLBAR_*`; `scrollbar_motion` é pública para o teste do tema); o véu dos diálogos cobre só o
quadro da janela, sem a margem da sombra (`src/dialog/dialog.rs`); a dica não reabre com o ponteiro
fora da janela (`src/tooltip.rs`); com a janela sem foco, o compasso não bate e devolve a fase
parada (`src/spinner.rs`); as divisórias redimensionáveis não desenham a linha fixa, só o indicador
no hover (`src/resizable.rs`); popovers e menus entram com fade e 6 px do lado do gatilho, em 160 ms
a no máximo 60 quadros por segundo (`src/popover.rs`, `entrada_flutuante`, e
`src/menu/context_menu.rs`), e os flutuantes têm a linha de luz no topo (`src/styled.rs`); os avisos
animam a no máximo 60 quadros por segundo (`src/notification.rs`); o compasso dos spinners diz se
está batendo, para o ponto de "rodando" respirar de carona (`src/spinner.rs`, `compasso_recente`); a
lista com busca tem o rótulo de grupo de 24 com um símbolo antes e a linha sem margem vertical
(`src/command`, `label_prefix`), em mono 11 caixa alta com o tracinho (`heading_row`); a
retícula (`trama`, `fio_de_pontos`, `fio_de_pontos_em_pe`, `lado_da_trama`) e a `cantoneira`
moram em `src/styled.rs`; o desenho único de tecla é o `desenho_de_teclas` de `src/kbd.rs`, que a
dica também usa; e a margem do lado do campo de texto cresce com o zoom (`com_zoom`
em `src/sizing.rs`, usada pelo `Input` e pelo grupo de campo em `src/input`). Ao atualizar o
gpui-kit, refazer essas mudanças na versão nova.
