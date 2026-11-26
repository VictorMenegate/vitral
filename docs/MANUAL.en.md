# Vitral

> Português: [LEIA-ME.md](../LEIA-ME.md)

> The project entry point is [README.md](../README.md) (what Vitral is, how to install, how to verify downloads). This file is the inner manual, translated from [LEIA-ME.md](../LEIA-ME.md): how each part works, what has been decided, and the gotchas. The app interface is in Portuguese only for now, so menu and button names are quoted as they appear, with the English meaning in parentheses.

An environment for working with Claude Code in several folders at once: a list of chats, each one
working directly in a folder, like Claude Code in the VS Code extension, with the conversations
side by side in panes. The background is a sheet of blue liquid metal (see **Acabamento**
(Finish); it started as the navy glass of Stained Glass UI) and the organization comes from
Ratoeira (the "Esperando você" (Waiting for you) and "Rodando agora" (Running now) sections, chat
tabs). Built in Rust with GPUI (Zed's UI engine) and the GPUI Kit components.

## Como funciona (How it works)

- **Chats.** The list on the left has all the chats, one row each (the bubble, the title and the
  time): at the top, what is waiting for you, what is running and the pinned ones, then all the
  others, from the most recently touched to the oldest. `Motor::chats_da_lista` (in `src/motor.rs`)
  builds the list: everything the list shows or counts goes through it. Each chat stores the
  title, the folder (`cwd`), the Claude Code session, the model, the permission, the account,
  whether it is pinned or archived, and the messages.
- **Each chat works directly in its folder.** No copy, no merge and no Vitral commit: Claude Code
  runs in the folder, as in the terminal or in VS Code, and the session is the folder's (`--resume`
  from there). **Sem pasta** (No folder) gives the chat an empty folder of its own, in
  `~/.local/share/vitral/avulsos/<chat>`.
- **New chat.** The "+" and Ctrl+N create one right away, without asking, like the "new chat" in
  VS Code (`novo_chat_direto`, in `src/ui/dialogos.rs`): with a **work folder**
  (`Config.pasta_de_trabalho`, chosen on the welcome screen; **Pasta dos chats novos** (Folder for
  new chats), in Configurações (Settings), Chats, changes it), it is always there.
  Without a work folder: in the folder of the open chat (the Sem pasta chat opens another Sem
  pasta); with no chat open, in the last folder used (the one from the last chat created, stored
  as `ultimo_destino` in `interface.json`, or the one from the chat touched most recently); with
  none, the palette asks. **Novo chat em outra pasta…** (New chat in another folder), in the logo
  menu, opens the "Em qual pasta?" (Which folder?) palette: the folders that already have a chat
  (the least recently used first, with search on name and path), the **Worktrees**, **Outra
  pasta…** (Other folder…, the system picker) and **Sem pasta**. The Worktrees group
  (`src/ui/copias.rs`) brings the copies of the repositories in those folders: whatever
  `git worktree list --porcelain` returns for each one (`copias_das_pastas`, in `src/git.rs`),
  read in the background when the palette opens, with the folder name and, below it, the branch
  (or "cabeça solta em <commit>" (detached head at <commit>)) and the path; search matches on
  name, branch and path. A copy that already has a chat shows up only under Pastas (Folders), one
  whose folder was deleted does not show up, and Vitral only lists and opens: it does not create
  or delete worktrees. What creates a chat is `Motor::novo_chat(pasta:
  Option<&Path>, …)`; the folder arrives canonical. The title comes from the first message
  (`titulo_de`, in `src/motor.rs`): the first line with text, without the leading markdown marks
  ("## ", "> ", "- ", "- [ ] ", "1. "), up to 7 words. After a turn, this automatic title gives
  way to the one Claude Code generated for the session (the `ai-title` line of its file, read
  from the end of the file by `importar::titulos_da_sessao`), once only: `titulo_automatico`
  compares the title with the first message's, so what you renamed (F2), the branch's and the
  lane's titles stay as they are, and the replaced title is not replaced again. A session renamed
  in Claude Code (`/rename`, the `custom-title` line) and a Codex chat do not change either. In a
  `-p` run Claude Code does not generate this title by itself (measured on 2.1.328: it only does
  so in the terminal, in VS Code and through the SDK), so the turn asks for it through the input:
  `generate_session_title` with the first message and `persist` (`Processo::pedir_titulo`), up to
  twice per chat on each app launch, on Claude Code 2.1.323 or newer. It is one extra small
  Claude Code call per new chat.
- **Claude or Codex.** Each chat says who answers (`Chat.agente`, in `src/modelo.rs`): Claude
  Code or Codex, OpenAI's CLI. You choose in the model selector, which has the **Claude** and
  **Codex** tabs (switching tabs only shows the other one's models; who answers changes when you
  pick one; the Codex tab has its default and the models from `codex debug models`, reread on
  every launch, with the description in Portuguese when the catalog's sentence is recognized,
  `descricao_do_codex`); the default for new chats is `Config.agente_padrao` (Configurações and
  the welcome screen). Codex runs one `codex app-server` per turn, over the JSON-RPC protocol
  (`src/agente_codex/servidor.rs`): `thread/start` or `thread/resume`, `turn/start`, `turn/steer`
  for messages sent during the work, and `turn/interrupt`. The steer confirmation removes the
  message from the pending ones; a refusal returns only that one to the queue. The end waits for
  the confirmations, so messages are not lost in a race with shutdown. The `thread_id` is kept in
  `Chat.sessao`, and `thread/resume` receives that id. The schema came from the installed CLI
  itself, and initialize and thread/start were checked without inference on 11/16/2026.
  **Permission request in the middle of a turn** (11/24/2026): each mode is a sandbox + approval
  policy pair (`sandbox_de` and `aprovacao_de`, in `src/agente_codex.rs`), which goes in
  `thread/start` (or `thread/resume`) and again in `turn/start`:

  | Vitral mode | sandbox | approvalPolicy | What it asks about |
  | --- | --- | --- | --- |
  | Manual | `workspace-write` | `untrusted` | every edit and every command that is not read-only |
  | Aceitar edições (Accept edits) | `workspace-write` | `untrusted` | commands; edits inside the folder are allowed by Vitral itself (`modelo::modo_permite`) |
  | Planejar (Plan) | `read-only` | `on-request` | when Codex wants to write or use the network |
  | Automático (Automatic) | `workspace-write` | `on-request` | when Codex wants to leave the folder or use the network |
  | Sem perguntar (Don't ask) | `danger-full-access` | `never` | nothing |

  The `item/commandExecution/requestApproval` and `item/fileChange/requestApproval` requests
  become `Evento::Permissao` and land on the same card as Claude's: the whole command (without
  the `/bin/bash -lc` wrapper) or the file's mini-diff, which comes from the `fileChange`
  `item/started` (the edit request only carries the `itemId`; with several files, a single diff,
  with the path of the first one outside the folder). Permitir (Allow) is `accept`, Negar (Deny)
  is `decline` (the item comes back `declined` and the turn goes on), and a card that disappears
  with no answer is `cancel`. `serverRequest/resolved` closes the card's channel
  (`Evento::PermissaoRetirada`) and it goes away. "Permitir sempre" (Always allow) and the current
  mode apply to Codex as they do to Gemini, and the review by the other agent has everything
  denied. In "Negar e explicar" (Deny and explain), the decision goes back through the protocol
  and the explanation follows as a message in the turn; if the turn is already ending, it stays in
  the queue. The same goes for typing in the box while the request is open. Measured on 0.169.0
  with real turns: the request id is a number, the command's `availableDecisions` does not list
  `decline` (which works anyway) and `resolved` arrives right after each answer.
  **Slash commands** (11/24/2026): the ones the app-server serves. `/compact` goes as
  `thread/compact/start` (measured on 0.169.0: it answers `{}` and opens a turn with the
  `contextCompaction` item, which becomes the "Compactando…" (Compacting…) and the marker, as in
  Claude; the text after it is not sent, Codex does not accept focus); `/review` as `review/start`
  (without text, the uncommitted changes; with text, whatever it asks for; the text of the
  `exitedReviewMode` item is the answer, and 0.169.0 sends the same text in an agent message,
  which is not entered again); and each skill in the folder (`skills/list`, requested on every
  turn) is a "/name", which goes as the `skill` input with its file and the text "$name …", as
  Codex does. A message with a slash waits for the list; a list that fails does not stop the turn.
  `Inicio` carries the list (`compact`, `review` and the skills) to `Chat.comandos`, and the "/"
  menu shows Vitral's `/clear` plus that list (`comandos_fora_do_claude`); before the first turn,
  only the two. Compacting (the "…" in the box, the palette, the context popover) works on Codex
  too.
  **Context and tokens** (11/24/2026): `thread/tokenUsage/updated` arrives on every model call
  (measured on 0.169.0). `last.totalTokens` is the current context (`Evento::Contexto`, the ring
  in the box), `modelContextWindow` is the window (it goes in `Fim`; until the end of the first
  turn, the ring uses the default), and the turn's spending is the `total` at the end minus the
  one before its first call (`Evento::Gasto` before `Fim`: the tooltip of the "Trabalhou"
  (Worked) line, the popover's "Último turno" (Last turn) and the **Onde o uso foi** (Where the
  usage went) ledger). Only the usage of the running turn counts toward spending. Codex's context
  popover has no cost and no account.
  **Steps** (11/24/2026): `turn/plan/updated` (the whole list on every change, with `step` and
  `status`) becomes a TodoWrite call with the result right away (`todos_do_plano`), and the chat's
  step list (`motor/passos.rs`), the "1/3" pill and the trail lines ("Criou 3 passos" (Created 3
  steps), "Concluiu: … · Começou: …" (Finished: … · Started: …)) come out as in Claude. Checked
  only against the schema and the fake Codex: in the real turns on 0.169.0 (gpt-6-luna and
  gpt-6.2-sol) the model said `update_plan` was not available, and no plan arrived.
  **Checking against the real Codex:** `cargo-vez.sh test -- --ignored --nocapture
  codex_de_verdade` (in `src/claude.rs`) runs four short turns in a new folder in /tmp, through
  the same `Processo` as the chats, and spends the Codex plan: in Manual, the edit request
  (allowed) and the command request (denied); `/compact`; the copy before the first turn; and
  `/review`. It passed on 0.169.0 with gpt-6.2-sol on 11/24/2026.
  What still differs in a Codex chat: there is no account and no dollar cost, and stopping goes
  through `turn/interrupt` (signal as a fallback). Switching a chat's agent
  (`Motor::definir_agente`) starts a new session on the other one, with the latest messages in the
  handoff block (`MotivoPassagem::TrocouDeAgente`); with a turn running, it does not switch.
- **Gemini.** The third agent (`Agente::Gemini`, since 11/23/2026) is Google's Antigravity CLI
  (`agy`), the successor of Gemini CLI, which stopped serving personal accounts on 06/18/2026. It
  shows up in the **Gemini** tab of the selector when `agy` is installed
  (`agente_gemini::instalado`: the PATH or `~/.local/bin`). It runs one `agy acp` per turn, with
  ACP (Agent Client Protocol), in `src/agente_gemini.rs`: `initialize`, `session/new` (or
  `session/resume` with the id kept in `Chat.sessao`; a conversation agy no longer has opens a
  new one), the `model`, `thought_level` and `mode` options through `session/set_config_option`
  before the first `session/prompt`, and the system prompt in
  `_meta.antigravity.systemPrompt.append`. A message sent while the turn is running is another
  `session/prompt`, which agy absorbs into the same turn (the previous one answers `end_turn`
  right away; the end waits for the answer to the last one). The permission request
  (`session/request_permission`) becomes `Evento::Permissao` and lands on the same card as Claude
  Code's requests; the answer goes back to agy through a thread (with no answer, `cancelled`).
  Stopping is `session/cancel`. "Sem perguntar" is the process's
  `--dangerously-skip-permissions`; the other modes become agy's (`modo_de`). The models come from
  `agy models` (the level variants merge into one model; the level is the box's effort, up to
  Alto (High)) and from the session options. No usage ring (Antigravity does not expose the
  quota), no account, no slash commands or `/compact`, and none of Vitral's tools: agy only
  accepts a local MCP server. Install and sign in (`uso_plano::instalar_gemini`) opens the
  terminal with the official installer and `agy acp --login`. Checked on agy 1.5.15 with a
  personal account.
- **Claude and Codex in the same chat (the pair).** `Chat.parceiro` holds the session, the model
  and the usage of the other agent; `Chat::trocar_vez` swaps the two, so the rest of the turn
  (account, model, resuming the session) keeps reading `chat.agente` and `chat.sessao` as always.
  In `comecar_turno_com`, the user's message picks who works (`alvo_da_dupla`: "@claude" or
  "@codex"; without @, whoever has the turn); `o_que_o_outro_fez` builds, from the agent's last
  `Item::Vez`, what the user sent and what the other one said and changed, and it goes before the
  turn's text. The pair is created on its own, when you switch agents or call the other with @.
  Choosing in the selector a model from the other agent changes that one's model
  (`definir_agente`), without undoing the pair. The model chip shows the two brands overlapping
  (`Simbolo::Dupla`, the one from whoever worked last in front) at every width. The "Os dois em
  toda mensagem" (Both on every message) switch (the rounds in which one worked and the other
  reviewed) was removed on 11/24/2026: the `Chat.revisao_em_dupla` field is still read, so old
  data opens, and it no longer does anything.
- **Review of the turn by the other agent.** `src/motor/revisao_do_par.rs` (owner's decision on
  11/23/2026). In `fim_de_turno` with no error, a turn that changed `LINHAS_PARA_REVISAR` (20)
  lines or more (the count from the turn's Mudanças (Changes), `linhas_mudadas_no_turno`) is
  reviewed by the other agent (Codex in a Claude chat, Claude in the others; `Agente::outro`), if
  the `Config.revisar_edicoes` option ("Revisão automática pelo outro agente" (Automatic review by
  the other agent), in Configurações, Agentes; on by default) is on. It does not run with the
  other one unavailable (not installed or at the plan limit), with the chat in Planejar, or on a
  turn that came from a requested review (no loops; `motivo_sem_revisao`). The reviewer enters as
  the pair's partner (`abrir`, which creates the partner if the chat had none), runs a single turn
  in Planejar (on Codex, the `read-only` sandbox; its permission request is denied without
  asking), and the chat goes back to who it was (`fechar`): what it did leaves the conversation
  and the result goes in, the line "Codex revisou: nada a acrescentar" (Codex reviewed: nothing to
  add) or the `Item::Revisao`, the card with the finding and **Mandar ao Claude** (Send to Claude)
  (`Motor::mandar_revisao`, which delivers the text to the chat's agent as a "Revisão do Codex"
  message). While it runs, the status line says "Codex revisando o turno…" (Codex reviewing the
  turn…) and the chat counts as idle; a message from you cuts the review off, leaving nothing
  behind. On the "Trabalhou…" line of the last finished turn, **Revisar com o Codex** (Review
  with Codex) (or "com o Claude"; `ui/mensagens/revisao.rs`, `Motor::pode_revisar`) asks for the
  same review of any turn. On the test bench, `[ferramenta EditGrande x.py]` makes a 30-line turn
  and `[revisor-acha]` in the message makes the fake reviewer find something.
- **Background subagent.** Since Claude Code 2.1.3xx the Agent call returns right away with
  `tool_use_result.status = "async_launched"`: this becomes `Evento::SubLancado`, not a result.
  `system/task_progress` counts the tools (`SubProgresso`) and `system/task_notification` closes
  the call with the `summary` (`SubFim`). `ferramentas/claude-falso/claude` stages this in
  `[ferramenta Agent …]`; `Task` stays the old way, which ends in its own result.
  **Who they are and what they do** (`src/motor/fundo.rs`, measured with the real CLI, 2.1.328,
  Haiku, on 11/23/2026, with one agent and with two): `background_tasks_changed` brings the whole
  list on every change, with the `task_id` (the agent's id) and the `description` of each
  `local_agent` (`Evento::AgentesNoFundo`, extracted by `claude::agentes_do_fundo`, next to
  `interpretar`), and `task_started` says which call opened each one (`Evento::AgenteAbriu`). Its
  file is `<session>/subagents/agent-<id>.jsonl`, with `agent-<id>.meta.json` beside it
  (`description`, `toolUseId`, `model` and, at the end, `finished`). The file starts at about 230
  KB (the attachments and the API request) and its last line is only the agent's call while it
  runs: between one call and another, the end is the result, attachments and the
  `api-request-blob` and `api-request` lines. The last action lives on the call's card
  (`Subagente::atividade`): it arrives through the stream (the agent's calls keep coming after the
  `result`, attached to the call that opened it) and through the end of the file, which brings
  what it says between one call and another. The end is read at most once per second
  (`Motor::conferir_segundo_plano`, on the Janela's one-second beat), only if the file grew, only
  the new part and never more than 256 KB (`fundo::ler_o_fim`, with the previous read's position
  kept), in the background; what it finds only counts if it is newer than what the stream already
  delivered (`fundo::anotar_acao`), and only that chat's panes redraw (`TextoChegando`), not the
  window. With more than one agent, Claude Code opens a turn for each agent that finishes (another
  `init`, one sentence) and **holds the `result` of those turns until the last one**, releasing
  them all together: the first closes the turn in Vitral, with what the agent said last, and the
  following ones only enter the spending ledger (before, each one closed the turn again: one
  "Terminou" (Finished) per agent).
- **Fixing any message.** `motor::editaveis_desde` says from where the user's messages can be
  fixed (after the last delivery from another chat, card or unknown item; Vitral's own delivery
  does not lock) and `motor::editavel` checks one; `retirar_mensagem` removes the message and what
  came after it. In the pair, the partner's session restarts too. **The session goes back to the
  point, without a summary:** so Claude does not remember what was removed, the next turn runs in
  a new session, which is a copy of the earlier one up to the point before the fixed message
  (`--resume <session> --fork-session --session-id <new> --resume-session-at <uuid>`, the same
  cut as "Continuar daqui em outro chat" (Continue from here in another chat);
  `Bifurcacao::correcao`, `bifurcar::sessao_depois_da_correcao`). The agent keeps the exact
  context of what came before, and the start of the conversation is the same one Claude Code had
  already sent, instead of a ten-message summary reread from scratch. On Codex the cut is the
  conversation's `thread/fork` with the `beforeTurnId` of the turn the message opened (see
  "Continuar daqui em outro chat"). When the point is not found (on Claude, in the session file:
  a conversation from before the marks, the session's first message, a message addressed to the
  turn; on Codex, a message without the turn id), the earlier path applies: an empty session with
  the handoff block (`MotivoPassagem::Reenvio`); if the agent refuses the cut, the turn is
  repeated that way, with a notice in the conversation. Fixing twice in a row, before sending,
  cuts again from the origin session. A session's copy brings the original's accumulated cost
  (Claude Code restores its `cost-state`, read from the 2.1.325 binary): the chat carries the last
  total seen over to the new session, and `somar_custo` adds only what grows. The question before
  fixing only says "sessão nova, com um resumo" (new session, with a summary) when that is what
  will happen. The cut was measured without sending a message (see "Continuar daqui em outro
  chat"); cache reads on the next turn were not measured, because that would need a paid turn.
- **Continuar daqui em outro chat (fork).** In the actions of one of your messages and of a reply
  (the branch icon; `src/ui/bifurcar.rs`) and in the command palette (from the last reply): a new
  chat is born in the same folder, "title (branch)", with the conversation's items up to that
  point, the same agent, model, mode and permission rules, opened in a pane beside it (with no room, a tab
  of the same one); the original does not change. The cut is always at a turn boundary: on a
  reply, its whole turn goes; on a message of yours, everything that came before, and its text
  (with the attachments) goes into the new chat's box. On Claude with a recorded session, the new
  chat's first turn runs on a copy of the origin session, through the path of imported
  conversations (`--resume <origin> --fork-session --session-id <new>`, `bifurcar` in
  `src/claude.rs`), cut at the point with `--resume-session-at <uuid>` (`Opcoes::cortar_em`): the
  agent remembers what came before without a summary. The uuid is the one from the last agent or
  user entry before the message that opens the next turn, found in the session file by walking up
  the `parentUuid` chain from its mark (`motor/bifurcar.rs`, `ponto_de_corte`); the point stays in
  `Chat.bifurcado_de` until the new session is born. On Codex (11/24/2026) the cut is exact too:
  the first turn opens with `thread/fork` of the origin conversation and the `beforeTurnId` of
  the turn that the point's message opened (`bifurcar::ponto_no_codex`,
  `agente_codex::abertura`), which is experimental on 0.169.0 (`initialize` turns on
  `experimentalApi`, as Paseo does). The message of yours that opens a Codex turn keeps its id in
  the mark (`modelo::marca_do_codex`, "codex:<id>"): it enters the conversation with a
  provisional uuid, and `turn/started` (`Evento::TurnoDoAgente`) swaps it for the id. The fork
  does not inherit the configuration: the folder, model, instructions, sandbox and policy go
  again, with `excludeTurns` (the answer does not bring the whole history). Measured on 0.169.0:
  the copy is born with another id and `forkedFromId`; in a real turn, the copy made before the
  first turn answered "Não." to "was any file asked for before?"; a turn that is not in the
  conversation gives `-32600 turn not found`, and then the turn is repeated in a new conversation
  with the handoff block (`servidor::FORK_RECUSADO`, `consertar_bifurcacao`). On Gemini, with no
  session, no mark or no point, the new chat starts in a new session with the handoff block
  (`MotivoPassagem::Bifurcou`). The `--resume-session-at` flag is in Claude Code 2.1.325 but not
  in `--help`. Measured on 11/18/2026 on a real session, opening the process without sending a
  message (it does not spend the plan, like `rewind_files`): it accepts the uuid of any entry in
  the chain (agent, yours, attachment or system), the whole resumed session had 40,388 tokens of
  messages and, cut at the first message, 39 (`get_context_usage`), and a uuid that is not in the
  session comes out with "No message found with message.uuid of". In that case (and with the
  origin session deleted) Vitral repeats the turn in a new session with the handoff block
  (`consertar_bifurcacao`). A real turn after the cut was not measured. With the last turn still
  running, forking on the last reply waits.
- **Token savings without touching the answer.** Three things, none of which cuts output, lowers
  effort or switches models on its own. (1) `claude::ambiente` turns off the background spending
  that Vitral does not show (`CLAUDE_CODE_ENABLE_PROMPT_SUGGESTION=false`,
  `CLAUDE_CODE_ENABLE_AWAY_SUMMARY=0`, `CLAUDE_CODE_DISABLE_TERMINAL_TITLE=1`). (2) `Chat::fria`
  (in `src/modelo.rs`): a Claude conversation with a session, `CONTEXTO_GRANDE` (100 thousand
  tokens) or more and idle for `DESCONTO_DURA` (one hour, the cache lifetime on the subscription)
  becomes `Situacao::Fria` (Cold) in the strip above the box, with "Compactar" (Compact). The
  "Compactar" (the strip's, the ring's, the "…" menu's and the error card's) sends `/compact`
  straight through the normal send, without touching the box (the draft stays); with the turn
  running, it goes into the queue. To compact with a focus, you type "/compact <instruction>" in
  the box, as the hint reminds you (`compactar_conversa`, usage round, item 6.3). A running turn,
  stopped, failure, missing folder, message queue and "Você parou" (You stopped; item 6.2) come
  before it. What the last turn changed ("3 arquivos mudados · +40 −12" (3 files changed), which
  used to come last in that strip) has been, since 0.10.0, the changes pill in the box's status
  line (see **Pílulas da caixa** (Box pills)): the click opens the right side on "Este chat"
  (This chat), in the Mudanças section (usage round, item 5.1). (3) `Chat::contexto_grande` puts a
  line in the model selector, in the Claude tab, saying how much the switch makes it reread. The
  conversation gauge (context, cost, model, account and Compactar) already existed, in the box's
  ring.
- **Compaction marker.** While Claude Code summarizes the conversation (`/compact`, or on its own
  when the context is full), the turn's live line says "Compactando…" in place of "Pensando…"
  (Thinking…), in the conversation and in the box's status line
  (`mensagens::rotulo_do_turno_calado`, `Motor::compactando`). When it finishes, the marker
  "Contexto compactado (automático, 168 mil tokens antes)" (Context compacted (automatic, 168
  thousand tokens before)) enters the conversation (or "manual", for `/compact`;
  `motor::marco_da_compactacao`), a conversation note like the "Turno interrompido" (Turn
  interrupted), which stays visible even with the turn collapsed into "Trabalhou…", and the
  context ring drops to what the conversation takes up after the summary. Measured on Claude Code
  2.1.328 on 11/23/2026 with a `/compact`: the output brings `system/status` with `status:
  "compacting"`, then `status: null` with `compact_result: "success"`, another `init` and
  `system/compact_boundary` with `compact_metadata` (`trigger` "manual" or "auto", `pre_tokens`,
  `post_tokens`); `claude::interpretar` turns them into `Evento::Compactando` and
  `Evento::Compactou` (plus `Evento::Contexto` with `post_tokens`). The `status` from a mode
  switch, which only brings `permissionMode`, says nothing. The marker is an `Item::Aviso` that
  starts with `motor::CONTEXTO_COMPACTADO`; a conversation imported from Claude Code goes on with
  "A conversa foi resumida aqui" (The conversation was summarized here), which comes from its
  file (`src/importar.rs`).
- **Claude account and lost login.** With more than one account, the choice lives in the model
  selector, in a line at the foot of the Claude tab, "Conta: tecnologia · semana 44%" (Account:
  tecnologia · week 44%), which opens the accounts in place (`pe_da_conta`, in
  `src/ui/composer_seletores.rs`; the keys are `conta:<id>`). When a turn ends with a login error
  ("OAuth token revoked", "OAuth session expired and could not be refreshed";
  `motor::falha_de_login`), the account goes into `Motor.sem_login`, leaves the selection
  (`conta_da_escolha` now skips even the account chosen by hand) and the turn is repeated on
  another account with a login, with a notice saying which (`Motor::login_perdido`). With no
  other, the notice says where to sign in again. The account comes back when a turn on it
  succeeds or when the person signs in again (`adicionar_conta`).
- **The same question for both.** **Perguntar também ao Codex** (Also ask Codex) (or Claude), in
  the box's "…" menu, sends the message to the chat and to a new chat of the other agent, in the
  same folder, opened in a pane beside it (`Janela::enviar_aos_dois`, `Motor::abrir_raia`). The
  new chat is born in Planejar, which only reads: two agents changing the same files trample each
  other. Its title carries the name of who answers up front ("Codex · …").
- **Agent map.** `src/ui/mapa_agentes.rs`: a wide dialog with the conversation on top (the model,
  the context and the account of how many are working, finished and failed) and, below, the list
  of all the subagents it opened (the Task and Agent calls, `agentes_do_chat`) on the left, with
  its own scroll, and the chosen one on the right (`painel_do_agente`): the state (working,
  finished, failed, interrupted), the time, the tokens and the tools used and, below, the **agent's
  conversation like a normal chat's** (`conversa_do_agente`): the request in a bubble, what it
  answered in Markdown, the consecutive calls in a collapsible group ("Leu 1 arquivo · fez 1
  busca" (Read 1 file · ran 1 search), the click opens the lines), the images the calls brought
  back as thumbnails (the click opens the viewer) and the report, which is the last answer. The
  items come from the subagent's conversation that the engine reads from Claude Code's file
  (`Motor::abrir_subconversa_no_mapa`, the same one as the tab's, kept in memory while the dialog
  is open and released on close); without the file, the card (the steps and the report). **Abrir
  em aba** (Open in tab) closes the dialog and opens that conversation in a tab. The list's
  **order** (`ordenar`): those working on top, then those waiting for your answer (a permission
  request from the subagent), those that failed, and the finished and interrupted ones, from most
  recent to oldest in each group; it opens on the first. It used to be a tree with the agent open
  inside it, and with 10 agents only two fit (owner, 11/23/2026). It opens through **Agentes
  desta conversa** (Agents in this conversation), in the box's "…" menu, and through the palette
  (the "N agentes" pill in the status line, which used to open it, became the subagents pill of
  the box pills, which lists the turn's). An agent with no result only counts as alive with the
  turn running and no notice after it in the conversation, **unless its file says otherwise**
  (`importar::subagente::sinal_das_linhas`, read every 2 s with the dialog open): ending on a
  text-only reply, it finished; recorded less than 2 min ago, it is working (the conversation's
  turn may have dropped and come back, with `SendMessage`, and the subagent kept going on the
  side: that was the "Interrompido" (Interrupted) of an agent that was working); idle for longer,
  it stays interrupted.
- **Box pills** (`src/ui/pilulas.rs`). What happens in that chat, in the status line of each
  pane's box (inside the frame, below the field; `Janela::linha_de_estado` builds it, and
  `Janela::pilulas_da_linha` draws the pills), **after** what the turn is doing now, the clock,
  the directed message and the queue: a pill that appears or disappears moves nothing to its
  left, and what yields, if width runs short, is the turn's text, which shortens with an
  ellipsis. The line exists with the turn alive, during the memory review underneath, or with some
  pill (a command that keeps running in the background and the turn's changes stay with the turn
  already stopped, and then the box grows by the line's 22 px; with no pill and no turn it
  disappears, and the keys and the Parou, Relê and Compactar strip stay above the box). Above the
  box and at the foot of the message list there is no place at all for them (`pe_da_lista`; the
  back-to-the-end pill does not move up because of them). The pills, in this order
  (`pilulas::montar`, pure and tested): **background commands** (Bash's `run_in_background` and
  the command that ran past its time; "2 comandos no fundo" (2 commands in the background), and
  the click lists each one with **Parar** (Stop), its `stop_task`, and **Parar todos** (Stop all);
  before, the click stopped them all at once; a command you stop is not marked as failed, and the
  end notice of a background command no longer puts the subagent count on its line, the "0
  ferramentas · falhou" (0 tools · failed)); **subagents by state**, each state with its own mark
  (the ring for those working, the dot for those waiting: "2 trabalhando" (2 working), "1
  esperando você" (1 waiting for you), and "1 falhou" (1 failed) while the turn runs; the click
  lists this turn's subagents, with what each one is doing, and the click on one opens its
  conversation in a tab; background agents keep counting after the turn's answer, and the list
  gives the last action of whoever is working in the words of its line in the conversation,
  `meta_na_lista`); **the agent's steps** ("3/5", TodoWrite or the task tools; the whole list in
  the tooltip and, on click, with what it is doing now highlighted); and **the turn's changes** ("2
  arquivos +40 −12" (2 files +40 −12), from the edit calls since your last message; the click
  opens the list on the right, on "Este chat"). The first three only with the turn running; the
  changes one stays after it, until the next message. In a narrow pane, the icon and the number
  stay. "Esperando você" is the subagent that made a permission request still unanswered: each
  subagent call arrives with its id (`Evento::SubFerramenta.id`), the engine keeps which subagent
  it belongs to (`Motor.chamadas_dos_subs`) and the permission request carries the id of the call
  that made it (`Motor::subagentes_esperando`). Where each one came from: they lived in the status
  line (the background commands, the steps' "3/7" and "N agentes"), went to a floating line above
  the box on 11/23 (0.10.2, `d01160e`) and came back on 11/24, at the owner's request ("it was
  better there"); the "N arquivos mudados" left a strip above the box that pushed the
  conversation. The turn's state is not a pill: it stays as the line's text.
  Reference: Paseo's "tracks" (`composer/tracks.tsx`), 11/23/2026.
- **Agent steps** (`src/motor/passos.rs`). Each chat's step list (`Motor.passos`, in memory only)
  comes from two families of Claude Code tools. `TodoWrite` sends the whole list on every call.
  `TaskCreate`, `TaskUpdate` and `TaskList` touch one step at a time, by id, and only change the
  list when the result arrives (the created step's id comes in it: "Task #4 created
  successfully"; the "Task not found" does not come marked as an error; `TaskList` answers "#4
  [in_progress] step x" per line, and rebuilds the whole list). Measured on Claude Code 2.1.328
  on 11/23/2026, with Haiku, Sonnet and Opus: in `-p`, with nothing in the environment, it offers
  neither family (the `init` tool list only has `Task` and `TaskStop`); with
  `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` it offers `TaskCreate`, `TaskGet`, `TaskList` and
  `TaskUpdate`, and not `TodoWrite`, which only shows up in sessions from earlier versions
  (2.1.219 to 2.1.228, through the VS Code extension). Vitral does not set this variable: with
  today's Claude Code, the pill and the Passos (Steps) section only show up for those who turn it
  on themselves. Claude Code's list belongs to the session and carries on from one turn to the
  next (it lives in `tasks/<session>/<id>.json`, in the config folder); Vitral's is lost on
  close, and comes back on the next `TaskList` (a previous step the agent touches without it
  shows up as "Concluiu o passo 4" (Finished step 4)). A `TaskCreate` that returns id 1 is a new
  list: the earlier one goes away. **In the trail, each call says what changed**
  (`Passos::chamada`, kept in the call's summary: "TodoWrite · Começou: Rodar os testes"
  (Started: Run the tests)): "Criou 5 passos", "Começou: …", "Concluiu: …", "Tirou: …" (Removed:
  …), with several together given as a number ("Concluiu 2 passos") and more than one change in
  the same call separated by a middle dot ("Concluiu: Ler o código · Começou: Corrigir"). In
  `TodoWrite` the steps are matched by text with those of the earlier list. A call that changed
  none of this, the ones from before this version and `TaskList` stay with "Atualizou os passos"
  (Updated the steps) and "Conferiu os passos" (Checked the steps), without detail. The history
  imported from Claude Code goes through the same logic (`importar.rs`). In the header of a group
  of calls they become "atualizou os passos", instead of counting as "usou N ferramentas" (used N
  tools).
- **Subagent conversation in a tab** (`src/motor/subconversa.rs`, `src/importar/subagente.rs`).
  Clicking a subagent's card (the Task or Agent call's line) or on it in the subagents pill opens
  in a pane tab its WHOLE conversation, read-only: the request it received, the reasoning, each
  call with its result and what it answered, drawn like the normal conversation. The card stays in
  the conversation of whoever opened it, as the marker of when it left, and its chevron (now a
  separate button, as in the edit) shows the steps and the report right there. **Where it comes
  from.** From the file Claude Code writes for each subagent:
  `projects/<folder>/<session>/subagents/agent-<id>.jsonl`, with `agent-<id>.meta.json` saying
  which call opened it (`toolUseId`). Measured on 2.1.328 with the real CLI (Haiku, 11/23/2026):
  the stream-json delivers from the subagent only the calls and their results (the text and
  reasoning only with `--forward-subagent-text`) and nothing more once the process exits; the
  file has everything, is written bit by bit while it works and stays there afterward. That is why
  the conversation always comes from the file, live and after reopening Vitral, without Claude
  Code's new option: `importar::subagente::transcript` finds the file by the call (first in the
  chat's session, then in the other sessions of the same project folder, for a chat that carried
  on in another session) and `importar::subagente::conversa` reads it with the imported history's
  reader (`itens_do_fio`, which now serves both threads). The request enters as a message delivered
  by the chat that opened it (you can go back to it from there). In automatic mode (2.1.271
  onward) the subagent delivers the report through the `SubagentHandback` call and the Task call's
  result only says "This agent's report was delivered…": in its conversation the handoff becomes
  the answer, and the card keeps the real report (`Evento::SubRelatorio`,
  `subconversa::relatorio_do_cartao`; before, the card showed the notice in English). The card's
  tool count was also fixed: `task_progress` arrives before the call and already counts it, and
  adding one more on the call gave one too many (now it comes from the calls seen, by id). **How
  it lives in the engine.** The open conversation is a fake `Chat` (`Subconversa`, id
  `sub:<chat>:<call>`) kept outside the state: it is not saved, does not enter the list and leaves
  memory when the tab closes (`Janela::soltar_subconversas`). `Motor::chat` finds both (it is what
  the interface draws); inside the engine everything uses `chat_gravado`, which only finds the
  real ones, so nothing acts on it (send, rename, archive and save do not reach it). Its execution
  is the subagent's (`Motor::execucao`: the call in progress spins, the tab turns blue). **Live.**
  The read goes in the background and is redone when an event from that subagent arrives
  (`subconversa_andou`) and, once per second with some chat working, when the file grew
  (`conferir_subconversas`: this is what brings the text between one call and another); only the
  panes that show it redraw (`TextoChegando`). **Without the file** (Claude Code deletes old
  sessions; the test bench without `CLAUDE_CONFIG_DIR`), the tab shows what the card kept, saying
  that this is all there is (`conversa_do_cartao`). At the foot, in place of the box, a line says
  "Conversa do subagente, só leitura" (Subagent conversation, read-only) and how far along it is
  (what it is doing now, or how long it took); if it is waiting on a permission from you, the
  request shows up there too. The tab is not remembered when Vitral closes (the pane goes back to
  the conversation of whoever opened it), and a subagent's subagent does not open: Claude Code
  does not record its conversation separately. Reference: Paseo's subagent panel
  (`provider-subagent-panel.tsx`, `sidechain-tracker.ts`), 11/23/2026.
- **Message in the middle of a turn.** Send with the turn running pushes the message into it
  (`Motor::dirigir_turno`). An @ to another agent waits in the queue, without entering the current
  process or answering its requests; requests to different agents enter in separate turns. When
  the turn ends without having absorbed the message, Claude Code opens another turn with it in the
  same process: in that case the process's input stays open, so the next turn also receives
  messages (its `Inicio` undoes the deferred end; if it does not start within 8 s,
  `vigiar_turno_dirigido` closes the input and the process exits). The input only closes at the
  deferred end when there are background agents. `Motor::turno_recebe_mensagem` says whether Send
  goes into the turn or into the queue, and **Mandar agora** (Send now), in the queue list
  (`Motor::mandar_da_fila_agora`), takes a message out of the queue and puts it in the running
  turn. Each directed message goes with a `uuid` (`Processo::dirigir`): **Parar removes it from
  Claude Code's internal queue** before the `interrupt` (the `cancel_async_message` control
  request, which is not in the SDK's public type; `Processo::retirar_dirigidas`), instead of
  killing the process, which was the way to keep it from opening another turn with it after the
  Parar. Measured on Claude Code 2.1.328 on 11/23/2026: the request answers `{"cancelled":
  true}`, the `interrupt` starts answering `still_queued: []`, the turn closes with the error
  `result`, the process exits on its own less than 1 s after the input closes, and the resumed
  session does not have the message (it goes back to Vitral's queue, as before). If the
  `interrupt` answer still brings some message (`Evento::FicouNaFila`), or on a Claude Code
  earlier than 2.1.323 (`VERSAO_RETIRAR_DIRIGIDA`), or on Codex, the old way applies: SIGTERM on
  the process (`motor::como_parar`).
- **Answering a permission request by typing.** With a request open in the chat, Enter in the box
  denies the request (all the open ones in that chat) and hands the text to the model as the
  explanation of the refusal, through the same path as the card's **Negar e explicar…** (Deny and
  explain…) (`Motor::responder_pedido_escrevendo`, `motor::recusa_explicada`; on a request to
  approve a plan, the text is the request for changes; on a question with options the text denies
  nothing, it is the answer to it). The message enters the conversation as yours, before the
  call's denied result. On Codex and Gemini, the refusal only carries the decision: the text
  follows as a message, with confirmation or back to the queue if the turn has already ended.
  With an attached image, which the refusal does not carry, the requests are denied with "O
  usuário respondeu com uma mensagem…" (The user answered with a message…) and the message enters
  the turn through the input. What applies to the whole turn (slash command, Ultrathink,
  Ultracode) still goes to the queue, denying nothing, and so does **Ctrl+Enter** (Mandar depois,
  Send later). The Send tooltip says what Enter will do (`composer::dica_do_enviar`).
- **Mode, model and effort on the running turn.** Switching the mode (Shift+Tab or the selector),
  the model or the effort with the turn running takes effect in it, not only in the next one:
  Vitral sends through the process's input the `set_permission_mode`, `set_model` and
  `apply_flag_settings` control requests (with `effortLevel`), as it already did with `interrupt`
  and `stop_task` (`Processo::acertar`, with what the process has in `claude::AoVivo`). Measured on
  2.1.328, in the middle of a command: the mode answers `{"mode": …}` and the next Write of the
  same turn went through without asking; the model answers `{"model": <id>}` and the next model
  call already goes out on it (the conversation starts showing it in use, `Evento::ModeloEmUso`);
  the effort answers success and `get_settings` reports the new level (`null` goes back to
  automatic). The switch only goes out after 0.6 s idle (`Motor::acertar_turno_depois`): Shift+Tab
  passes through the modes on the way, and none of them ("Sem perguntar" between Automático and
  Manual, for example) may take effect in the turn even for an instant. So that "Sem perguntar"
  can enter in the middle of a turn, every turn opens with `--allow-dangerously-skip-permissions`
  (without it the request comes back `bypass_not_launched`), except with Vitral running as root,
  where Claude Code does not even open with the option. A turn with Ultrathink or Ultracode keeps
  its effort to the end. With the input already closed, on Codex, or if Claude Code refuses the
  request, nothing shows on screen: the choice is saved in the conversation and takes effect on
  the next turn, as it always did.
- **Sign of life and reasoning.** While a tool runs, Claude Code sends a `tool_progress` with
  `heartbeat` every 30 s (measured on 2.1.328 with a 75 s command): it counts as a sign of life
  (`Evento::Vivo`), and the status line's "sem sinal há N min" (no signal for N min)
  (`Motor::silencio`, 10 minutes silent) only shows up when the process has really stopped
  talking, not on a long build. The reasoning text only comes in the output with
  `--thinking-display summarized` (not in `--help`; without it the block arrives empty on
  2.1.328): Vitral passes the option from Claude Code 2.1.323 on (`VERSAO_RACIOCINIO_RESUMIDO`,
  the oldest one where it was checked; on one that did not know it, it would break the turn), and
  the conversation's "Pensou por…" (Thought for…) opens with the summary.
- **Automatic effort.** On Auto, the selector says which level applies: Vitral asks Claude Code
  (`get_settings`, the `applied.effort`), once per account and model, in the background
  (`claude::atualizar_esforco_padrao`); on Codex it comes from the catalog
  (`default_reasoning_level`).
- **MCP servers.** The box's `/mcp`, **Lista dos servidores MCP** (MCP server list)
  (Configurações, Claude Code) and the palette open the list of the active pane's chat's servers
  (`src/ui/mcp_servidores.rs`): by origin (from your Claude Code, from the folder, account
  connectors), one card each, with the state, the error of any that went down and, on click, the
  tools. While the dialog is open there is a Claude Code just for it (`claude::SessaoMcp`: no
  message and no hooks, in the chat's folder and account), in a thread that serves the screen with
  the protocol's `mcp_status` and `mcp_reconnect` control requests: **Atualizar** (Refresh)
  rereads the list and each card's button turns the server on again and shows whether it came
  back. Reconnecting applies to that query session (each chat turn opens Claude Code again and
  connects the servers on its own); turning a server off and signing in to a connector stay in
  Claude Code. An extra account keeps the server list from when it was created
  (`contas::preparar_pasta` copies `mcpServers` from `~/.claude.json`): a server added later does
  not show up in it.
- **Cost of a redraw.** The Janela is the root and is remounted every frame; what is heavy lives
  in cached views, which only redo themselves when they themselves are notified: each pane's
  messages and the list (`src/ui/vista_mensagens.rs`) and what animates on its own, the metal and
  the rings (`componentes/vista_viva.rs`). The metal and spinning-ring loop beats together with
  the spinners' tick, 30 times per second (`movimento::laco_a`), not on a grid of its own. These
  things hold the cost down, measured on 11/23/2026 on the optimized binary, on the test bench,
  with three 450-item conversations side by side at 150% (`VITRAL_QUADROS=1` and the main
  thread's time in `/proc`; the machine was busy, so the difference is what counts). Per frame,
  before to after: scrolling a pane, from 35-58 ms to 28-30; scrolling with text arriving, from
  76-90 to 36-59; text arriving, from 41-55 to 36; each key in the message box, from about 69 ms
  to about 14; idle, 0% in both. With a single chat running a tool (ring and metal beating,
  nothing arriving) nothing changed: about 21 ms per frame, which is the shell.
  - **Only what is in view is mounted** (`src/ui/mensagens/a_vista.rs`). The conversation is
    still a single column inside a scrolling area (the reading anchor, the pinned message, the
    search and the summary count in pixels), but a block far from view enters as a spacer of the
    height it measured last time, without mounting its markdown, layout or painting. The
    conversation is planned first (`Planejado`: what each block is and how to mount it), then
    `a_vista::blocos_a_montar` picks those in view and in the margin around it (half a view on
    each side, 300 px at least), and only those are mounted. The stored height is valid for the
    same block showing the same thing (`ChaveDoBloco`: the type, the items and a content mark,
    which changes when a call's result arrives or the message is fixed), at the same width and the
    same zoom (`Alturas`); without a stored height the block is mounted, and the count always errs
    on the safe side. Always mounted: everything, with the conversation search open, with text
    selected in the window or in the drawing that opens the chat; the error card and the text that
    is arriving. Text over 3 KB has its own rule (`texto_que_chega_depois`): the kit reads it in
    the background, and the first drawing comes out without it; its height only counts after
    staying the same for 400 ms (`Alturas::assentada`) and, when it returns to view after being a
    spacer, the box stays at the stored height until the reread text arrives (`Alturas::segurar`).
    When a mounted block measures a height different from the one used in the count (it collapsed
    an output, the text arrived), the conversation asks for one more draw: those after it moved,
    and a spacer may have entered the view. GPUI's `list()` did not work: it scrolls by item, and
    the anchor, the pinned message, the "open at the start of the reply" and the scroll to the
    search hit count in pixels from the top of the conversation. The pages of 60 items continue
    (`ITENS_NA_TELA`): a block only becomes a spacer after being measured once.
  - **Arriving text redraws only its pane.** The engine batches the pieces that arrive within 60
    ms (`motor::JANELA_DO_TEXTO`) and, when the batch only made the text grow, emits
    `TextoChegando` instead of `notify` (`motor::aviso_do_lote`): the Janela notifies only the
    messages of that chat's panes (`Janela::texto_chegando`), and the list and the other panes
    come out of the cache. The first piece of a reply still redraws everything (the turn's strip
    switches to "escrevendo" (writing)). The markdown of growing text is not reread whole: the kit
    receives only what was added (`TextViewState::set_text` becomes `push_str` when the new text
    starts with the old).
  - **The shell is a view of its own, with no cache** (`vista_mensagens::VistaDaCasca`):
    everything the Janela draws outside the cached views is mounted by it
    (`Janela::montar_casca`). GPUI notifies the view an element is in on every hover, tooltip,
    transition frame (`movimento::pedir_quadro`) and stored state that changes (`use_keyed_state`);
    when that view was the Janela, each such notice went through `Janela::avisar_vistas` and
    redid the messages of the three panes and the list (13 to 25 ms per pane). Now only what
    Vitral's code changes notifies the Janela (`cx.notify()` in a `Context<Janela>`), and then the
    views all redo themselves, because their state lives in it. Three notices became narrower: the
    key in the message box redraws only the shell (`Janela::redesenhar_a_casca`: the box's text
    does not appear in the messages or the list), the redraw that the conversation's prepaint asks
    for (the pinned message that changes on each wheel step) notifies only the pane's view, and
    scrolling only notifies the Janela through the summary when the summary is on screen (with the
    right side closed, each wheel step in the active pane used to redo the whole window). **Anyone
    who writes a listener that changes the Janela's state needs `cx.notify()`**: before, the
    click's hover redid everything as a side effect.
  - **The running call's clock ticks on its own, once per second.** The spinners' tick redraws the
    shell and the rings, not the messages, and the Janela's 1 s clock goes quiet while the tick
    beats: since the messages became a cached view, the "00:42" on a call's line (and the
    subagent's) stopped at the value of the last draw. Now, with the tick beating, that clock
    notifies only the messages of the panes whose chat has a call with no result
    (`Janela::andar_os_relogios_das_chamadas`): one draw per second of the working pane, and the
    Janela, the list and the other panes stay in the cache.
  - **The pills are mounted with the box's status line**, on every frame of each pane: that is why
    `pilulas::montar` only counts (the subagents with no result, the turn's edits), copying
    nothing from the conversation. Everything that changes a pill arrives through an engine event
    that is not text, and those notify the Janela; a growing subagent conversation notifies only
    the panes that show it (`TextoChegando`, emitted by `Motor::subconversa_lida`).
  - **The fade of what is cached lives inside the view.** GPUI reuses the painting of a cached
    view while it is not notified, and the cache key is the box, the clip and the text style, not
    the opacity: a fade put by the shell around it only moves if it redraws every frame (in the
    pane opened beside, it redraws itself in the first frames, measuring the blocks, and the card's
    fade works). That is why the conversation that switches in the pane enters through
    `movimento::entrada` from inside `Janela::corpo_do_painel` (`Painel.troca` and `troca_vista`):
    the view redraws on the fade's frames, and only it. What animates in the shell redraws the
    shell; what animates in the messages, only that pane's view. The entries from the 11/24/2026
    round (see **O que entra com fade** (What enters with a fade)) did not change the cost,
    measured on the same test bench (three 450-item conversations at 150%, optimized binary, the
    main thread in % of one core, average of 3 runs before and 2 after, with the machine busy with
    other sessions and up to 10 points of difference between identical runs): idle, 0 and 0;
    thinking, 49 and 46; running a tool, 48 and 48; writing, 56 and 56; scrolling idle, 33 and 33;
    scrolling with text arriving, 55 and 54. Switching tabs, opening a group and switching between
    Conversas (Conversations) and Arquivos (Files) make 6 to 8 redraws in the second of the
    action, before and after, with the same cost per frame: the fade's frames fall on the ones the
    action was already asking for (measuring the blocks, going to the anchor).
  - **What still costs** is the shell, redone every frame: with three panes, about 10 ms of layout
    and 7 of painting (the headers, the message boxes, the strips), 30 times per second with a
    chat working. What would lower that is making each pane's header and box cached views,
    siblings of the messages one (nested ones do not work: the view that redoes itself redoes
    those inside), with their spinners in live views. And a Janela notice (`cx.notify()`) still
    redoes the messages of all the panes: knowing which pane changed needs a signature of what
    each one draws.
- **Welcome.** On the first launch (with no work folder and no chat at all;
  `VITRAL_BOAS_VINDAS=1` forces it), `src/ui/boas_vindas.rs` shows four steps: the introduction,
  the work folder (the folders where Claude Code already has conversations come as a suggestion)
  and the basics (permissions, who answers when Codex is installed, Claude's model and bringing
  in the conversations that already exist) and the optional connection to the team memory.
- **Team memory.** On first use, in the logo menu and in Configurações, each person enters the
  address of an ai-memory server and their key. **Testar e ligar** (Test and connect) checks the
  server before saving the connection; **Consultar memória** (Query memory) searches among the
  accessible projects and opens the text of the pages, without calling a model. Claude and Codex
  receive the MCP on every turn, including resumed sessions. Claude receives a private file
  through `--mcp-config`; Codex, a configuration override for that process only and the key
  through the environment. The agents' personal configuration does not change. With no
  connection, Vitral keeps working. The key stays in `~/.config/vitral/memoria-equipe.chave` (0600
  on Unix), outside `config.json`; **Desligar** (Turn off) removes the key and the MCP copy in the
  cache. Owner's decision, 11/13/2026: share the team's knowledge through Vitral, with a server on
  the VPS and one key per person. Only the lessons the agent records are sent: Vitral does not
  install hooks that capture prompts or tool outputs and does not import the local database.
  There is no need to install ai-memory on each person's computer. The prompt tells the agents to
  query before working and to use the owner and the repository name as workspace and project, so
  that each computer's local path does not split the team's memory.
- **Memory review when done** (Configurações, **Rever a memória ao terminar** (Review memory when
  done)). When a turn that touched a file ends, once per session, Vitral asks the agent to review
  what is worth keeping in memory and in the instructions. The work turn counts as finished
  immediately: the "Terminou", the bell, the list and the notification come out without waiting
  for the review, which runs underneath, with a discreet "Revendo a memória…" (Reviewing the
  memory…) in the status line and without notifying again (it still counts as work for closing
  and Reiniciar (Restart)). A message from you during the review never enters it: if the review
  has not yet used a write tool, it is cut off and the message opens a normal turn (the review is
  asked for again at the next end); if it has already written, the message waits in the queue. In
  the conversation, the review becomes a collapsed line: "Memória revista · nada a guardar"
  (Memory reviewed · nothing to keep) or "Memória: editou AGENTS.md" (Memory: edited AGENTS.md),
  with the icon that opens the file; the click shows what it did. The criterion is the tool used,
  not the text of the answer.
- **Claude Code conversations.** On the first launch (along with the migration), Vitral brings in
  the conversations from the VS Code extension and the terminal
  (`~/.claude/projects/<folder>/<session>.jsonl`, only read: nothing there is changed). After
  that a **watcher** (`src/motor/importacao.rs`) looks again on each launch and every 2 minutes
  and brings in the new conversations on its own: it only opens files touched since the previous
  pass, with 30 s of slack (`Filtro.desde`, in `src/importar.rs`), stays quiet when there is
  nothing and, when it brings something, says so in the window. The pass's mark is the time the
  read started, never gets ahead of the clock and only advances to it if all the folders opened
  (with one that failed, it stays at most a day behind). Someone who was already using Vitral
  before the import existed has no record: for them the watcher starts from now, and the whole
  history only comes through **Importar do Claude Code…** (Import from Claude Code…). This is
  what lets someone who uses VS Code move to Vitral without importing by hand. Each one becomes a
  chat directly in its folder, as over there (Claude Code only resumes a session from its folder).
  The title is the one you gave (`/rename`), Claude Code's summary or the start of the first
  message; the dates and the order in the list are the conversation's. Left out: the empty ones
  (only a `/clear`), the test ones (folder in `/tmp`), those from Vitral's own chats, those from
  programs (`claude -p`) and those whose folder no longer exists (an unmounted disk: they enter
  when it is mounted and you ask again). The notice reports the result ("40 conversas do Claude
  Code importadas · 3 abertas agora no Claude Code" (40 Claude Code conversations imported · 3
  open right now in Claude Code)). The history shows in the chat, read from the session file when
  it opens (in the background; none of this goes into Vitral's data): it starts with the last 60
  items and shows the earlier ones automatically on scrolling up, preserving the reading
  position, and the line "Trazida do Claude Code (VS Code) em 01/11" (Brought from Claude Code
  (VS Code) on 11/01) before what comes in Vitral. The first message continues on a copy of the
  conversation (`--resume <id> --fork-session`): the original stays as it is and can go on in VS
  Code; the following ones resume the copy. A conversation that was open in Claude Code when it
  was brought in gets a notice saying so. **Importar do Claude Code…** (Vitral menu) looks at
  everything again, not only what changed, and always answers; neither it nor the watcher repeats
  what has already come (nor the conversations of chats deleted afterward).
- **One chat opens another** (`abrir_chat`, from Vitral's MCP server): when you ask ("open a chat
  for this"), the chat opens a new chat in the same folder (the Sem pasta one, a Sem pasta), in
  view in the list, with the request already sent; you carry on the conversation there and
  nothing comes back on its own to whoever opened it. Only in a turn where only you spoke: text
  from another chat does not open chats.
- **Chats talk to each other** (`listar_chats` and `mensagem_para_chat`); messages that arrive
  while the chat is busy wait in the queue. The sender signs "chat <id> “title”", and the message
  arrives with `[de: …]` in front; Vitral's own, with `[Vitral]`. **Loop brake:** after 6
  consecutive automatic messages between two chats with you not in between, the next one is held
  and both warn "muitas mensagens seguidas entre X e Y; esperando você" (too many consecutive
  messages between X and Y; waiting for you); writing to either one resets it. The MCP server
  (`src/mcp.rs`) has only these three tools and `permissao`, the permission-request card's
  (`--permission-prompt-tool mcp__vitral__permissao`, in all modes).
- **Every list says what the chat answered** (`src/ui/falas.rs`, usage round, item 3.3). In the
  bell, the new reply shows the first sentence of the last answer, without markdown and cut at
  the sentence, and below it what the turn did ("3 editados · 12 comandos" (3 edited · 12
  commands)), counted on the spot from the items. In the "+" history, the second line says the
  state and the start of the last utterance ("Permissão · Bash · npm test" (Permission · Bash ·
  npm test), "Terminou · Pronto, mandei as duas…" (Finished · Done, I sent both…)), with "há N
  min" (N min ago); the folder only shows when it differs from the chat of the pane. The
  background tab's tooltip has the title and, on the second line, the state with when and the
  start of the last utterance. The system notification continues with the first

## Primeira abertura, dados e linha de comando (First launch, data and command line)

- **Data from versions with projects.** Data from the versions with projects (the `estado.json` with
  `"versao"` 2: projects, requests, Mestre (Master), helpers, copies, cards, routines) opens without losing
  any conversation. On load (`dados::normalizar`), the Mestre and the helpers leave the
  list (their message file stays in `chats/`); a chat that worked on an isolated copy now works
  directly in the folder the copy came from, with the notice "Este chat trabalhava numa
  cópia isolada, que continua em <caminho>; agora ele mexe direto em <origem>." (This chat used to work on an isolated
  copy, which is still at <path>; now it works directly in <origin>.) (the copy is not
  deleted) and, if the Claude Code session had been created in the copy, it starts a new session with a
  summary of the conversation; an archived project sends its chats to the archived ones, and the project's
  model passes to the chats that followed the default; the conversation of a coordinator from the old Vitral
  becomes a chat like any other. Before the first write in the new format, the `estado.json` as it came
  is kept in `estado.versao-2.json`, once only. Nothing in the users' folders is deleted:
  `projetos/<nome>` (the own folders), `copias/`, `notas/` and `avulsos/` stay as they are.
  `FORMATO_DADOS` and `VERSAO_ESTADO` are 3: an earlier version refuses to open this data, and the
  updater does not go back to it.
- **Migration from the old Vitral.** On first launch (when there is no
  `~/.local/share/vitral/estado.json` yet), before the window opens, Vitral brings in what was in the
  old Vitral's folders (those in `~/.config/vitral/recentes.json`, most recent first, and the one from
  `vitral <pasta>`): the conversations and the queues, with the same `cwd` and the same session, each one becoming
  a chat directly in its folder; the requests are only used to find the origin folder of whoever worked
  on a copy. A subfolder marked as archived sends its conversations to the archived ones. The notes
  stay in the old `.vitral` (the report says where). A request with a number repeated across two folders
  gets another one, and its chat says so ("Na importação, este chat passou de #3 para chat 12." (On import, this chat went from #3 to chat 12.)). Nothing
  in the old folders is changed, moved or deleted (`<raiz>/.vitral/` stays intact, including the old copies
  in `.vitral/worktrees`); `estado.json` is the last file written, so a PC that
  shuts down midway redoes everything on the next launch. The report appears in the window ("Trouxe 2
  conversas do Vitral antigo de ~/Projetos." (Brought 2 conversations from the old Vitral in ~/Projetos.)) and is kept in `~/.local/share/vitral/migracao.json`. It
  runs only once: afterwards, `vitral <pasta antiga>` opens the list, and another folder with an old `.vitral`
  gets a notice that this version only brings in those conversations on first launch.
- **The old Vitral open.** With a Vitral from before this version open on first launch, nothing is
  written and the screen says "O Vitral antigo está aberto (pid N). Feche-o para eu trazer as conversas de
  ~/Projetos." (The old Vitral is open (pid N). Close it so I can bring in the conversations from ~/Projetos.), with **Tentar de novo** (Try again) and **Sair** (Quit).
- **Going back to 0.2.0** is turned off after migrating: it does not read the conversations in the new format
  (Settings say "A 0.2.0-rc.2 não lê as conversas do formato novo" (0.2.0-rc.2 does not read conversations in the new format)). The `.vitral/` in
  each folder is still there, as 0.2.0 left it.
- **Where everything lives.** The data in `~/.local/share/vitral`: `estado.json` (with `.anterior` and,
  coming from an earlier version, `estado.versao-2.json`), `chats/<id>.json`, `avulsos/<chat>/` (the
  folders of the Sem pasta (No folder) chats), `exportadas/`, `anexos/`, `imagens/<chat>/` (the images the
  tools returned, see **Ferramentas** (Tools)), `migracao.json` and the data `formato`; next to
  them, in `app/`, the app installed by the updater (the versions and the `atual` and
  `anterior` links). The preferences in `~/.config/vitral` (`config.json`, `interface.json`, the accounts,
  `atualizacoes.json`; the old `recentes.json`, which only the migration reads) and each turn's prompt in
  `~/.cache/vitral/prompts/`. A corrupted file goes to `<nome>.ilegivel-<hora>.json` and Vitral
  opens with the `.anterior`, with a notice. The keys in `interface.json` that this version no longer uses
  (`vista`, those of the issues, `abas_por_chat`) are ignored; so is the old `ultimo_destino` (a project
  id).
- **Only one Vitral.** The lock is `~/.local/share/vitral.trava` (outside the data folder) and the socket is
  `$XDG_RUNTIME_DIR/vitral/vitral.sock`. Opening again (from the menu, `vitral <pasta>` or `--nova`)
  hands the request to the one that is open (also while it is migrating) and does not open another window; with it
  frozen, the second launch waits up to 10 s and says "O Vitral já está aberto, mas não respondeu" (Vitral is already open, but did not respond).
- **Command line.** `vitral` opens the app. `vitral <pasta>` (the `%f` of Nautilus) opens the list if
  the folder is one of the migrated ones, the chat last touched in that folder if there is one, and otherwise a new chat
  in it. `vitral --nova <pasta> [mensagem]` creates a new chat in the folder (relative to the folder you
  called from), with the message already sent; without it, the cursor in the box; anything that is not a folder only
  shows a notice.
- **When the data does not open.** Unreadable state (and the `.anterior` too), data from a newer
  version, or a migration that was postponed or failed: the recovery screen, writing nothing over the data, with
  **Abrir a pasta de dados** (Open the data folder), **Tentar de novo** (Try again), **Sair** (Quit) and **Começar do zero** (Start from scratch), which asks first,
  moves the data to `~/.local/share/vitral.descartado-<hora>` (except `app/`, which stays: the menu
  keeps opening Vitral) and opens as on the first launch, bringing in the old Vitral's folders and
  the Claude Code conversations again.
- **When it does not save.** If the disk refuses (full, no permission), the red banner "Não
  estou conseguindo gravar (…). Nada novo começa até voltar a gravar." (I can't save right now (…). Nothing new starts until saving works again.) appears and Vitral retries on
  its own; messages sent in the meantime wait in the queue and go in when it recovers.
- **Claude Code transcripts.** Claude Code deletes conversations with no activity for more than 30 days
  (`cleanupPeriodDays`), and a chat idle that long loses its session: Vitral continues in a new one, with
  a summary, and says so ("A sessão antiga do Claude Code expirou; continuei numa nova com um resumo." (The old Claude Code session expired; I continued in a new one with a summary.)).
  To keep more, put a high value in `~/.claude/settings.json`, such as `"cleanupPeriodDays":
  3650`.

## Quando algo para no meio (When something stops midway)

- **New version.** When installed from the release, Vitral looks for a new version on GitHub, checks the
  signature, downloads and installs it on its own, and notifies once: "Versão 0.3.0 pronta" (Version 0.3.0 ready), with **Reiniciar** (Restart) and
  the **Novidades** (What's new) link (the version's Markdown and "Ver no GitHub" (View on GitHub)). It never restarts by itself; closing
  the notice loses nothing (the blue dot on the logo and the menu item remain). Reiniciar asks the same
  question as closing ("2 conversas estão trabalhando" (2 conversations are working), **Reiniciar mesmo assim** (Restart anyway) / **Continuar
  aberto** (Keep open), Enter on Continuar), and whatever was cut off stays stopped, with Retomar (Resume). After
  restarting, once: "Atualizado para a 0.3.0 · Ver novidades" (Updated to 0.3.0 · See what's new). In **Só avisar** (Only notify), the notice is "Há a
  0.3.0 no GitHub" (0.3.0 is available on GitHub), with **Instalar** (Install).
- **New version that does not open.** If it crashes twice on launch in the same PC session, the third
  attempt opens the previous one and says: "A 0.3.0 não abriu duas vezes; voltei para a 0.2.0" (0.3.0 failed to open twice; I went back to 0.2.0) (it stays until
  closed). 0.3.0 is not installed again on its own. A power loss between attempts does not
  count, and the rollback only goes to a version that reads the conversations on disk.

- **Stopped request.** If Vitral closes, the PC goes down or you press Parar (Stop), the chat's request shows up
  as **parada** (stopped) in amber, with a pause icon, moves up to "Esperando você" (Waiting for you) (except when it was you
  who stopped it) and gets **Retomar** (Resume) on the list card, on the right and at the end of the conversation. A chat
  cut off in the middle of a turn too. The notice on reopening appears only once.
- **A chat that fails** (process that dies or does not start, API error): the turn ends with an error, the banner
  above the box says in a few words what it was ("Limite de uso do Codex · volta às 12:30" (Codex usage limit · back at 12:30), "Sem
  login no Claude Code" (Not logged in to Claude Code), "Sem conexão" (No connection), "Serviço sobrecarregado" (Service overloaded), "Conversa longa demais para o
  modelo" (Conversation too long for the model) or, otherwise, the first line of the error), with the full text in the tooltip and **Tentar de
  novo** (Try again), and with the window in the background comes the notification "Parou com erro: <primeira linha>" (Stopped with an error: <first line>)
  (urgent: it stays on the GNOME screen until clicked). Two causes get their own way out in the banner.
  On the limit of an account chosen by hand, if another account is logged in and has headroom, the banner says "Limite da
  <conta> · volta 15:42" (<account> limit · back at 15:42) and offers **Continuar na <outra>** (Continue on <other>): the chat's account becomes the other one
  (the model picker shows it, and that is where you go back) and the same turn runs again, without
  Ultracode. With no other account with headroom, it stays "Limite do plano · volta às 15:42" (Plan limit · back at 15:42) with Tentar de
  novo. On a lost login, the banner says "Login da <conta> caiu" (<account> login dropped) and offers **Entrar de novo** (Sign in again), which
  opens the Contas do Claude (Claude accounts) already redoing that account's login. Nothing retries by itself. In the conversation, the error becomes the **cartão de erro** (error card)
  (`src/ui/mensagens/erros.rs` reads the text; `CartaoDeErro` draws it): the icon in the color of the subject
  (red when the turn could not continue, light blue when it needs you, amber when it went on
  with a caveat, neutral for network and overloaded service) on a 6% veil, the title in Portuguese,
  the sentence about what to do ("Volta às 12:30." (Back at 12:30.)), the Copiar (Copy) in the corner (it takes the agent's text as it came) and
  the agent's text behind **Detalhes** (Details), in a dark well in mono, with URLs shortened to
  the domain and clickable (open when Vitral does not know the error). On the live error (the last one, with no
  message from you after it and with the turn stopped) up to two actions appear: **Ver o uso** (See usage) and **Continuar no
  Claude** (Continue on Claude) at the Codex limit; **Contas do Claude** and **Continuar no Codex** (Continue on Codex) at the Claude limit;
  **Entrar de novo** (which opens the Contas already on the login of the account that dropped) or **Entrar no Codex** (Sign in to Codex) on a
  lost login; **Instalar o Codex** (Install Codex); **Compactar** (Compact)
  on a conversation that is too long. Continuar switches who answers and repeats the turn; in the pair (Claude and
  Codex in the same chat), it hands the turn to the other. **Tentar de novo** stays only in the banner. The plan
  limit with another free account still switches accounts on its own, with no card. A lost login with no
  other account and all accounts at the limit, which used to come out as two notices in a row (Vitral's and the
  agent's), are now a single card, also in conversations already saved; Copiar takes both texts. When Claude Code sends the error also as a
  reply, it is saved but not drawn nor counted by the search: only the error card appears
  (`resposta_repete_o_erro`, in `src/ui/mensagens.rs`).
- **Background agents.** When Claude replies with a flow of agents (or a subagent)
  still running, the turn does not end there: the chat keeps running, the conversation gets the notice "Os
  agentes seguem trabalhando em segundo plano; o turno termina quando eles acabarem." (The agents keep working in the background; the turn ends when they finish.) and the turn only
  ends, with the last reply, after they finish (or when the Claude Code process exits).
  During that wait the process's input stays open: the message you send goes into Claude Code right
  away (`Motor::dirigir_turno`, `agentes_no_fundo_com_entrada`), instead of waiting in the queue until the
  agents finish, and the input closes when the last one ends (`Evento::Fundo(0)`) with no pending
  message (owner, 11/23/2026: a message sat 53 min in the queue).
  This notice does not close what came before it (`Item::aviso_que_fecha`): the other notices mark the
  turn that stopped, and calls without a result from before them count as interrupted; with this one,
  the agents card showed up as "interrompida" (interrupted), the pill reset to zero and the subagent tab stopped
  following exactly when they were still working. **One line per agent**
  (`src/ui/mensagens/no_fundo.rs`): while the reply has already come and they keep going, the end of the conversation
  has one line for each, in place of the sentence "Agentes trabalhando em segundo plano…" (Agents working in the background…): the ring, the
  description, the last action in the gerund ("Rodando cargo test…" (Running cargo test…), "Lendo views.py…" (Reading views.py…); "Começando…" (Starting…)
  before the first; `acao_ao_vivo`, the same one as in the pill's list and the card's live line) and the
  clock since launch, in mono. Clicking opens its conversation in a tab, and the tooltip also says
  the model. The lines stay while Claude comments on the agent that just finished (the
  text arrives above them). **The end of each one** is a conversation item (`Item::FimDoAgente`, placed
  by `fundo::anotar_fim` on the `SubFim` of the call that came back with "Async agent launched"): the line
  "Terminou: Mapear o carrinho · 25 min" (Finished: Map the cart · 25 min), or "Falhou: …" (Failed: …) with the word in red, at the point
  where it ends; clicking opens the report, which lives in the call's card. In a collapsed turn
  these lines stay visible, with the line "Trabalhou…" (Worked…). A Vitral from before this keeps the item
  as it came. If the process exits with them still running, with the wait ceiling
  expired or with an error code, the turn ends with an error and the report says the agents did not
  finish. Parar still works and, during that wait, kills the process along with the agents. A turn
  that ends with an error or stopped while there are background agents also kills the process, instead of
  leaving them running off screen. Claude Code waits for the agents **with no ceiling**
  (`CLAUDE_CODE_PRINT_BG_WAIT_CEILING_MS=0`) on every turn: its default is to cut at 10 minutes
  ("Background tasks still running after 600s; terminating"), and a one-hour batch used to die midway,
  with the turn marked as failed. The one who holds the wait is the owner, through Parar. On every turn, each flow runs at most 3 agents at the same time
  (`CLAUDE_CODE_WORKFLOW_MAX_CONCURRENT_AGENTS=3`; the default would be 6 on this PC, and the power supply cannot hold
  the load); the limit is per flow, so a turn with Ultracode asks Claude for one flow at a time,
  with no other agents alongside.
- **Claude Code session.** The session id is saved as soon as it is created, and each turn uses
  `--resume` if the conversation already exists. If Claude Code refuses ("already in use", "No
  conversation found"), Vitral fixes the mark and repeats the turn once; in the second case it opens
  a new session and says the memory was lost.
- **Saving.** The messages of the turn in progress go to disk every 5 s, on a separate thread
  and with fsync, to survive a power loss. State and configuration go through the same
  queue, off the window thread.
- **Closing with things running.** The window's x, Alt+F4, the Panorama's x and the dash's Sair ask
  for confirmation when conversations are working ("1 conversa está trabalhando" (1 conversation is working), "3 conversas estão
  trabalhando" (3 conversations are working); "Fechar mesmo assim" (Close anyway) / "Continuar aberto"); so does the update's Reiniciar. Enter
  stays on **Continuar aberto**: closing takes a click.
- **Processes.** Each claude process runs in a user systemd scope (`vitral-<pid>-...scope`, in `vitral.slice`). Stopping one kills everything it
  opened, even servers and shells detached with `setsid`; closing the app sends SIGTERM to all of them without
  freezing the screen; on launch, Vitral stops the scopes of an instance that crashed. Without systemd, a
  process group is used.
- **Confirmations.** Every question before acting has the same design: the icon in a circle in the tone's color,
  the title, what happens and, in a box below, what goes away. On the ones that delete or undo
  (**Desconectar** (Disconnect) an account, **Começar do zero**), Enter only closes, without acting. Archiving a chat does not
  ask: it acts right away, and the notice has **Desfazer** (Undo).
- With agents running, the window redraws at most 12 to 20 times per second (and not on every
  frame of the 165 Hz monitor). Effects only move in response to an action (opening, focusing, clicking,
  the pointer entering a round button, new data arriving), last 120 to 240 ms and redraw at
  most 60 times per second; when idle, nothing moves. With GNOME animations off, with
  **Animações: Desligadas** (Animations: Off) in Configurações (Settings) (or `VITRAL_SEM_ANIMACAO=1`), everything appears directly in the
  final state and the spinner stays still. With the window visible but unfocused, the spinners, the rings that
  spin, the metal and the status line's glow stop (1 redraw per second, the clock's, instead
  of 12); text arriving
  in a conversation that no panel shows does not redraw the window; and the scrollbar appears and
  disappears without a fade (the fade asked for frames at the monitor's rate on every size change, like Ctrl+B).
- **What comes in with a fade** (owner, 11/24/2026: "tem bastante coisa que parece meio seco" (a lot of things feel kind of dry)). Everything
  at 160 ms, once, with a 6 to 8 px slide or just the fade, never a scale (GPUI has no scale on
  a div): the conversation that switches in the panel (tab, list, history);
  each permission request, question and plan above the box (from below); the open queue (from below);
  the turn banner when it appears and when it changes state (Parada, Falhou, Fila, Você parou (Stopped, Failed, Queue, You stopped)); each
  pill that appears with the line already in view (from below); the box's status line when the turn
  starts (fade only: it opens its own space and the box grows); the call that arrives in a trail already in
  view (from below) and the group header, which is born with the second one; what a click opens in the
  conversation (a trail group, "Trabalhou…", "Pensou" (Thought), a call's output, an agent's report, the answered
  plan, an error's "Detalhes", the memory review), from above, with the
  chevron rotating 90° in 120 ms; the pinned message when it appears (the text swap while scrolling
  follows the scroll, with no fade); the Settings section, sliding from the side the user came from in the
  column (like the welcome step); Conversas ↔ Arquivos (Conversations ↔ Files) in the list; the docked right panel
  on opening (Ctrl+J) and on switching between the file and Este chat (This chat); the "Comece um chat" (Start a chat) of the empty panel; the
  corner notices; the banner for when saving fails; and the attachment island. Leaving is dry (the kit
  unmounts on the frame where the state changes), collapsing too (automatic height does not animate), and none
  of this repeats on scroll, on switching panels or with another panel opening alongside. Left without a
  color transition, on purpose, are the hover of list rows, tabs and menu items:
  moving the pointer over a row would become continuous animation. The theme switch still has no
  transition.

## Opções da conversa (como na extensão do Claude Code) (Chat options, as in the Claude Code extension)

The message box is a double frame (`caixa_mensagem`, in `src/ui/composer.rs`): outside, a black frame with a 20 corner radius (14 in Estreita, the narrow layout); inside it, with 2 of clearance (it was 4; owner, 11/16/2026: "diminui as bordas", make the borders smaller), the opaque field, holding the text
and the chips; below the field, still inside the frame, the **linha de estado** (status line), in mono 11. With focus, a
2 px metal ring lights up around the frame (180 ms; fades in 120), with no blue and no halo; it
cuts out the middle of the field (`Encaixe::Miolo`, with the light grain), so the ribbon is continuous on all four
sides, instead of blue on top and gray on the sides (the same in the search box, in the tab under the pointer and in the frame
of the request waiting for you). In the background panels, the box recedes: the model, the effort, the paperclip, the
**…** and the mode (except "Sem perguntar", which is a warning) sit at 0.55 (`opacidade::EM_REPOUSO`) and
light up when the pointer is over the box and when the panel is activated; the field, the frame, the ring with the
percentage and the stop button always stay lit (appearance report, 11/16/2026, V2). The bar
below the field has chips with an icon, in this order: **modo de permissão** (permission mode: Manual, with the filled shield, and Planejar in green; Aceitar edições and Automático in violet; Sem perguntar in amber),
**modelo** (model: the Claude glyph, loose, without the filled disc, and whatever applies, such as "Claude
Opus 5.5"; on chips the brand is always the glyph, `Marca::glifo`, and the disc stays in lists, popovers
and the usage ring), **esforço** (effort: small bars in cyan, from "Auto" to "Máximo", sized like an icon: bars of 2 with
a gap of 2, from 4 to 12 tall) and **conta** (account: the icon in the color of the
5 h usage; only with more than one account). What follows the folder default is shown in a faint tone. On the right
are the context ring (with the gap at the bottom and the percentage beside it, "24%", in mono; amber at
70%, red at 90%; also in the narrow box, tighter, because the ring alone looked like the spinning ring
of "running"; a click opens the chat usage: on top the
context and the cost, "Contexto 31% · US$ 1,84", below the model, the account and the tokens, and in the footer
**Compactar a conversa** (Compact the conversation), which prepares `/compact`), the attach paperclip (file or image, file
from the folder, previous conversation and slash
commands), the **…** and the send button, a metal key
(`VarianteRedonda::Metal`, the pair of the list's "+"), dimmed with an empty box, which lights up
when the box gets text and gives a clear pulse on each send (the pulse stays on the button even when it has already
become the stop button). The **…** has at most three groups, with a divider between them (`grupos_do_mais`,
owner, 11/24/2026: it was 11 items and 5 dividers): in the narrow box, **attach** first (**Arquivo
ou imagem…** (File or image), **Arquivo da pasta…** (File from the folder) and **Conversa anterior como contexto…** (Previous conversation as context), which leave the paperclip);
with text in the box, **send** (**Mandar depois, na fila** (Send later, in the queue), with the turn running and Enter going into
it; **Mandar quando as 5 h renovarem** (Send when the 5 h renew); **Mandar às…** (Send at…); **Perguntar também ao Codex** (Also ask Codex), except
for a slash command); and **the conversation** (**Comandos de barra** (Slash commands), **Compactar a conversa** in a
Claude or Codex conversation that already has a session, **Agentes desta conversa** (Agents of this conversation) when it has opened any, **Sessão
nova** (New session) with the chat idle, **Fixar** (Pin)). What does not apply right now is removed, instead of being grayed out. In the
medium box, the **…** does not appear, as before. Clicking any empty spot of the box (the gap between the text and the chips, the margin,
the space between chips, the status line) keeps the cursor in it. With the conversation running, it becomes
the stop button (the square, with a ring spinning around it); with text written, both stay, and send
goes into the queue. The status line, with the chat idle, shows the keys: on the left "/" commands and,
with an empty box, "↑" last; on the right "Enter" sends and, with
text written, "Shift Enter" breaks the line (in the narrow box, only the right side). With the chat
working, it says what the chat is doing (see **Faixa do turno** (Turn strip), below). The messages, the queue, the turn strip
and the box reach the same edge in every panel: the usage rings float in the corner,
on an opaque disc, and nothing reserves room for them. Empty, the box says "Mensagem para este chat…" (Message for this chat…) in the chat
with a folder and "Mensagem…" in Sem pasta (No folder). **Shift+Tab** switches the mode, also in the turn that is
running (see **Modo, modelo e esforço no turno que roda** (Mode, model and effort in the running turn)). In the narrow box, where the
chips are icons only, switching the mode with Shift+Tab or choosing a model shows a short notice
("Modo: Manual", "Modelo: Claude Fable 5.2", `avisar_na_estreita`). The mode menu lists what was allowed in this conversation: a click on a rule removes only that rule, and
**Voltar a perguntar sobre tudo** (Go back to asking about everything) removes all of them. The **effort** chip opens a popover with the level in a
segmented control (←/→, Home and End change it right away; the chosen level goes on the chosen item's card, the same one
used by the menus, without repeating it in the header), with a short line about each level (Auto: "Usa o
padrão do modelo." (Uses the model default.)), and the **Ultrathink** switch, in rainbow (Space turns it on; the description says "Só na
próxima mensagem" (Only on the next message)): it applies only to the next message, raises its effort to Extra (Máximo
stays Máximo) and keeps the chip rainbow until sent. A slash command does not consume it. Below
it is the **Ultracode** switch, in violet with the flow icon (U turns it on): it applies only to the next
message, like Ultrathink, and makes Claude orchestrate several agents in parallel through a flow (the
Workflow tool of Claude Code). Under the title, a single line: "Agentes em paralelo, só na
próxima · gasta bem mais" (Agents in parallel, only on the next one · spends a lot more); the (i) beside the name says the rest: that it spends much more of the plan and runs at
Extra effort (Máximo becomes Extra on that message). Amber is kept for what gets in the way right now
(`avisos_do_ultracode`). When on, the effort chip becomes "Ultracode" in
violet with the flow icon (with Ultrathink as well, the small bars turn rainbow; in the narrow
box, only the flow icon). It always starts off, also when Vitral opens. It is
disabled, with the short reason in place of the description (only the title and the switch dim; the reason
stays legible), with a Claude Code older than 2.1.278 (the version in which Vitral checked Ultracode; the agent
limit only exists since 2.1.269), with a model without the Extra effort (Haiku, the 4.x up to 4.6) and
in the chat where Codex is the one answering ("Só no Claude Code" (Only in Claude Code), also with both together).
On an account where an Ultracode turn showed agent flows turned off (in Dynamic
workflows in `/config`, by `disableWorkflows` or by the organization), the line stays switchable and
warns in amber; that turn gets a red notice, stays at Extra and the message loses its seal.
Outside Sem perguntar and Automático, the line also warns ("Neste modo, os agentes podem ter ações
recusadas" (In this mode, agents may have actions refused): they cannot ask for permission). One Ultracode at a time in the whole of Vitral: with
another one running, the line warns ("Roda em outra conversa: esta iria sem ele" (Runs in another conversation: this one would go without it)) and the message goes without
Ultracode, with the reason in the conversation. Each of these warnings is a short amber line; the why goes in the
(i) tooltip. When the account hits the limit and the turn
continues in the next window, the retry goes without it; only **Tentar de novo** (Try again) repeats it. Outside Ultracode, the Workflow
tool stays blocked: neither a chat nor a "Continue de onde parou" (Continue where you left off) relaunches a flow on its own. The
**model** chip opens a picker with search: Padrão (Default, the one from Configurações (Settings)) and the Claude families,
with the version in a badge on the right (the one running, when the conversation follows the family, or the one the
alias resolves to today in the installed Claude Code); the Claude logo stays only on the group label and
the check of the chosen item on the right. When following Padrão, it is the one that says which is in use ("Padrão · Opus
5.5"); only a family chosen in the conversation gets the "em uso" (in use) badge. An old conversation that
saved the full model name gets its own line. When the family is not the one in use, its
line says which version it is today ("Para os desafios mais difíceis · hoje, Fable 5.1" (For the hardest challenges · today, Fable 5.1)). Below
the families, the **Outras versões ▸** (Other versions) line opens the **exact versions** in place (Fable 5.2, Fable 5.1,
Opus 5.5 and the others), with the running one marked, including when the conversation follows the family.
At the bottom, with more than one account, the account line ("Conta: tecnologia · semana 44%" (Account: tecnologia · week 44%)) opens the account
choice in place. Both start collapsed on each open (only a chosen exact version opens
the versions), so the picker fits without scrolling at 150%: what is open is `Abertos`, kept in the
per-panel Janela, and what stays in view comes from `grupos_a_vista`, which also does the search (the
list's `filtro_proprio`): with something typed, the matching versions and accounts appear directly and the
open-rows go away. In a Claude conversation above 100 thousand tokens (`Chat::contexto_grande`),
a short line below the list says "Trocar agora relê 751 mil tokens" (Switching now rereads 751 thousand tokens) (`aviso_de_troca`). The list comes from the installed Claude Code:
when the app opens and when its version changes, Vitral makes the `initialize` request of the
Agent SDK protocol (the same one the VS Code extension makes on open), sending no message and spending no usage, in an
empty folder owned only by the user inside the Vitral cache (`perguntas`), with only the user
settings, no MCP server at all and hooks turned off (`claude::atualizar_modelos_oferecidos`,
with a 12 s deadline; at the end the process and what it opened are shut down), and the picker shows each
model it listed and what each alias resolves to, once
only, by family and from newest to oldest (`versoes_de_modelo`, in `src/ui/composer.rs`).
A new model appears when Claude Code updates, with no new Vitral version. With no answer
(no Claude Code, or the fake `claude` of the test bench), the fixed list `VERSOES_DE_MODELO` applies,
which is only the fallback. A version the conversation had chosen and that Claude Code stopped
offering stays in view, as "escolhido antes" (chosen before). On a Claude Code older than 2.1.280 (`VERSAO_MODELOS_EXATOS`), the exact
versions do not appear and a line says why. Outside the narrow box, the effort
and model popovers end in a footer with the keys they accept, in the palette's key design: **← →**
level, **Espaço** (Space) Ultrathink, **U** Ultracode, **Enter** closes; in the model one, **↑ ↓** choose,
**Enter** uses. With the medium box, the arrows and the "Claude " of the model go away, and the account keeps only the
icon. With the narrow box, the chips keep only the icon (the name goes in the tooltip), and attach and
account go to the **…**. No text is cut in the middle: what does not fit disappears whole, the account
first. Attaching another conversation as context and mentioning a folder file through the paperclip
(**Arquivo da pasta…**). Typing `@` at the start of the message or after a space opens the **"@"
menu** in the box itself, the same design as the `/` one, filtering as you type (up to 8 rows): with
the pair turned on and at the start of the message, **@claude** and **@codex** first (the "@" of an email,
"victor@", opens nothing); then the files and folders of the chat, with the file open on the right
at the top. Tab or Enter complete "@path ", and Esc closes without erasing. The completed file (or folder)
becomes a **pill** inside the box (the kit field's `InlineToken`, in
`composer::ficha_da_mencao`): the type icon, the short name and the lines, if any
("fila.rs:120-134"); a click opens the file on the right and Backspace erases the whole mention. The
text that goes to the agent stays the full path, with the lines in the format of Claude
Code mentions ("@src/ui/fila.rs#L120-134"). The saved draft comes back with the pills; the text that returns to the
box through Corrigir (Fix) or through the queue returns as text, and becomes a pill in the bubble the same way. The folder is read in the
background and again on each new "@"; while it reads, the line "Lendo a pasta…" (Reading the folder…). The list of files of the
folder (the one for "@", the one in the Arquivos tab and the one in Mencionar) descends into the repositories inside it: each
subfolder with its own git that the parent ignores or does not know enters with its prefix
(`central/src/…`), up to 3 levels and within the cap of 20 thousand files. Only a `.git` that is a
folder counts (a worktree, with `.git` as a file, would duplicate everything). In ~/eita, whose root is a whitelist,
this is what makes "@" find the projects' code.
Pasting (Ctrl+V) an image or copied files, or dragging files from the file manager into the conversation,
makes an attachment. `/` opens the slash commands (those of your Claude Code, with `/clear` for a new session and
`/compact`), in a menu above the box: ↑/↓ move through the menu (the field stays in the "Menu" context only
while it is in view), Tab or Enter complete the marked item (Enter with the exact name of a
command sends it), and Esc closes the menu without erasing what was written; the next letter opens it again. Hover over a message to copy (and, on yours, to fix and send again): the actions appear in a floating bar at the top corner of your bubble, on the right, 16 from the edge
(12 in the minimum panel: on the straight stretch, in the text column, and not on the corner curve), and at the foot of the answer, on the right (on top it covered the
first line); nothing is
reserved or visible without the pointer, and nothing moves when it appears. The message hover
reaches the whole bar, which sits half outside it (`com_alcance_da_barra`): arriving from
above, the pencil does not vanish before the click. Code blocks are
almost pure black, like command outputs, and have their own Copiar (Copy), in a column on the right that
never covers the code; the language goes in its tooltip ("Copiar código python"). Each block
quote in the answer ("> ...", usually the message ready to send to another person; in the color of the
text, with the rule and the indent marking the quote) has its own Copiar
in the top right corner, which takes the text without the ">" and without the markdown marks (bold,
italic, code, heading; a link becomes "text (address)"), with the lines and paragraphs. The kit
has no hook for the quote's actions: an answer with a quote is split into stretches
(`trechos_com_citacoes`, in `src/ui/mensagens.rs`) and each quote is drawn separately, with the
same style; search counts occurrences stretch by stretch (`Realce::trecho`), and selection holds
within each stretch. Modo foco (Focus mode; applies to all chats: `Config::modo_foco`, in the panel menu and
in the palette, with no button in the header; owner, 11/18/2026): like the Claude Code extension in VS Code,
each turn becomes a single line with what the agent did (`turnos_do_foco`): done, "Trabalhou 16 s ·
1 comando" (Worked 16 s · 1 command), with the final answer below; running, live, "Buscando cupom · 2 chamadas" (Looking up coupon · 2 calls), with the
ring spinning. A click opens the whole turn, and search (Ctrl+F) finds and opens what is inside
(owner, 11/23/2026).

- **Ler a conversa** (Reading the conversation). The **time** of each message appears on hover, in the same floating bar as
  Copiar (`hora_na_barra`; the bar is a raised key, opaque, with a light rule and light on top, without
  the blue ring of the floating ones, in `barra_de_acoes`), and a **day separator** ("Hoje" (Today), "Ontem" (Yesterday),
  "22 de março" (March 22)) separates the turns (`separador_de_dia`): in a conversation that spans days you can tell when
  each thing happened. Conversations from before 0.4.0 did not save the time and show nothing. **Copiar** works on
  any message, not just the last answer of each turn. **Ctrl+F** searches inside the open
  conversation (`src/ui/busca_conversa.rs`): the bar floats at the top of the panel, with the counter ("3 de
  12" (3 of 12)), **F3** and **Shift+F3** to move and amber highlighting; the search ignores case and
  accents ("codigo" finds "código"), also counts what is inside a closed block (the output
  of a tool, also that of a command, which then appears whole and scrolls to the match), which
  opens when its turn comes (also in the trail with reasoning in the middle: the
  search groups the calls with the same rules as the conversation), and **Esc** closes. The match stops 48 px
  from the top and 88 from the bottom, away from the pill that returns to the end. Outside the wide panel the counter is short
  ("2/7", with the sentence in the tooltip) and the field does not go below 128. The
  index is only rebuilt when the term or the size of the conversation changes, and none of this animates.
- **Escrever** (Writing). With an empty box, the **up arrow** brings back your last message to
  adjust and send again, and the conversation goes back to before it (`EditarUltima`, only in the
  "Vazia" context; with text written the arrow keeps moving the cursor). The **draft** of each conversation (what
  you wrote and did not send) already stayed in window memory when switching panels; now it also
  goes to `interface.json` 1.2 s after you stop typing, and comes back when Vitral opens
  again, even after a crash (`src/ui/rascunhos.rs`). The draft is whole: the text,
  the attachments (the pasted screenshots) and the quote; a draft with only an attachment is saved too, and the old
  format (text only) still opens. Ultrathink and Ultracode still start off.
  In the list and in the background tabs, a chat with a draft gets the small pencil and, on the list card, the
  second line "Rascunho: começo do texto" (Draft: start of the text) (or "Rascunho: 2 anexos" (Draft: 2 attachments)) in place of the time; on the tab, the
  start goes in the pencil's tooltip. The panel where the chat is open does not carry the mark (usage round,
  item 1.4). The text Vitral puts in the box
  (the fix, a completed command, a file mention) goes in through `composer::por_texto`, which
  keeps the undo (Ctrl+Z brings back what was written), scrolls the box to the cursor, at the
  end, and counts as typing for the draft; the corrected message, which already left the conversation, goes
  to disk right away. Pasting a long text also scrolls the box to the cursor, in the frame after
  the paste, when it has already grown. On send, the empty box also goes to disk right away, otherwise a crash
  soon after brought the already sent message back as a draft. Corrigir with something written in the box always asks first, and the question says
  that the box text leaves it. The **queue** of what you sent while
  the turn was running has its own list (`src/ui/fila.rs`), opened by the queue pill (in the box's status
  line, with the chat working, or in the strip above it), inside the panel and above the
  message box: each
  waiting message on a line, with the × that returns the whole message to the box (the text, the
  attachments and its Ultrathink or Ultracode; with something written in the box, the text joins at the end,
  after a blank line, the attachments add to the pills and Ctrl+Z undoes the join), the pencil
  that edits right there, in the line (Ctrl+Enter saves, Esc gives up; if the queue moved meanwhile,
  nothing is replaced) and, with the chat idle, **Entregar agora** (Deliver now) in the footer (the strip above the box does not
  repeat the button with the queue open). With the turn running, the box empty and something of yours in the queue, the up
  arrow brings back the last one in the queue, with attachments and boost, without touching the turn; Ctrl+Enter
  puts it back in the queue. With no queue, the arrow warns as before (usage round, item 1.3).
  **Mensagem com hora marcada** (Message with a set time; `src/motor/agendadas.rs` and `src/ui/agendar.rs`): in the box's **…**,
  with text in it, **Mandar quando as 5 h renovarem** (the renewal of the account the chat runs on, or that
  of Codex, plus 30 s of slack; the item disappears without that time read) and **Mandar às…** (a dialog with
  the time: "15:10", "15h", "830"; one that already passed today applies to tomorrow) send what is in the box
  to the chat queue with the time (`Entrada::agendada`, saved in the state: survives a restart).
  The queue opens with its card, which carries the badge "sai às 15:10" (goes out at 15:10); the ×, the pencil, the order and
  **Mandar agora** (Send now) work as on the others, and the idle chat's **Entregar agora** sends everything, with a
  time or without. Until the time, it does not enter any turn, even with the chat free: whoever takes
  messages from the queue for a turn goes through `Motor::tirar_prontas`. At the time, the engine's 15 s routine
  (`entregar_agendadas`) opens the free chat's turn, together with whatever else was waiting in the queue;
  with the chat busy, the message enters when the turn ends. It only goes out with Vitral open: a time
  that passed with it closed goes out right on opening. The
  list shows about three messages before scrolling and keeps on the right the place of the scrollbar,
  which used to hold the click of the ×; what the queue does is in the title tooltip. Outside the wide panel, each row's
  **Mandar agora** is only the arrow, so the row of buttons fits. **?**, with focus outside a text field, opens the **Atalhos do
  teclado** (Keyboard shortcuts; `src/ui/atalhos.rs`): the screen is built from the same table that registers the keys, so
  it never drifts from what the keyboard does.
- **Paleta de comandos (Ctrl+Shift+P)** (Command palette; `src/ui/paleta.rs`; also in the logo menu). A single
  list, with accent-insensitive search by words, of everything Vitral does. It has two sources: the rows
  with a label in the table of `src/ui/atalhos.rs` (with the keys on the right; Enter triggers the same
  action as the key, from the active panel's box, through the `atalhos::acoes()` that the table now
  generates) and the commands that used to exist only in menus (`paleta::Direto`: from the logo menu, the box's "…"
  and the conversation menu, such as Compactar, Agentes desta conversa, Perguntar também ao Codex,
  Sessão nova, Renomear (Rename), Fixar, Arquivar (Archive), Modo foco, Servidores MCP (MCP servers), Contas (Accounts), Consultar memória (Query memory)),
  which call the menu item's method. The sections are those of the shortcuts dialog. What does not apply
  now is dimmed, at the end of the section, with the reason below ("A conversa ainda não tem sessão" (The conversation has no session yet),
  "Só com o menu “/” da caixa à vista" (Only with the box's “/” menu in view)): the list is built from a snapshot of the moment
  (`paleta::Momento`). Esc returns focus to the box. A test guarantees that every row with a label
  in the shortcuts table is in the palette; a new shortcut enters on its own, and a new menu command enters
  in `Direto::TODOS`. **With something written**, the palette also finds conversations by title (all but the one in the
  active panel, with the folder below; Enter opens it in the active panel) and the files of the active
  chat's folder by name (Enter opens it on the right), in two sections after the commands, **Ir para a
  conversa** (Go to the conversation) and **Abrir arquivo** (Open file), with up to 6 rows each (`grupos_com_achados`); with nothing
  written, only the commands, as before.
- **Abrir arquivo pelo nome (Ctrl+P)** (Open file by name; `paleta::abrir_arquivo_pelo_nome`; also in the palette). The
  list of the files in the active panel's chat folder, in the same choice list as "@"
  (`seletor_arquivos::grupos_de_arquivos`: the name on top, the folder below, a name match ranking before
  a folder match and the shorter first), and Enter opens the file in the right panel. The list is the one
  the Arquivos tab and "@" already read (`cache_arquivos`), so it opens right away; the first time for the
  folder, it is read in the background ("Lendo os arquivos…" (Reading the files…)). File search, here, in "@" and in
  Mencionar (Mention), ignores accents and case ("relatorio" finds "relatório.md"), with the piece in
  `acentos.rs`. With no conversation in the active panel, a notice asks for one.
- **Mensagens já enviadas (Ctrl+R)** (Messages already sent). The list of your messages from all chats (the
  `Item::Usuario` the engine holds in memory; the history of a conversation brought from Claude Code
  stays out), in the choice palette, with the accent-insensitive search by words from `acentos.rs`.
  First those of the open chat, then those of the other chats in the same folder, then the rest; in each
  group, from newest to oldest, and the same text appears only once. Each row has the
  start of the message and, below, the chat, the folder and how long ago. **Enter** puts the text in the
  box of the active panel, without sending: with something written, it joins at the end after a blank line
  (Ctrl+Z undoes it, as with the queue's ×). Shows 60 at a time; search narrows the rest. All in
  `src/ui/enviadas.rs`.
- **Faixa do turno** (Turn strip). The turn in progress lives in the box's status line, inside the frame
  (`linha_de_estado`, in `src/ui/mensagens.rs`): on the left, the spinning ring, what the chat is
  doing ("Lendo motor.rs…" (Reading motor.rs…), "Rodando: cargo test…" (Running: cargo test…), with the running shimmer), the clock in mm:ss,
  "ação N" (action N) and, when present, "dirigido" (steered) and the queue pill; on the right, the **Esc** key and "para parar" (to stop)
  (the background commands, the steps and the agents are back in it, in **Pílulas da caixa** (Box pills)).
  With the answer already given and the agents still running, it says the agent and what it is doing ("Mapear o
  carrinho: Rodando cargo test…" (Map the cart: Running cargo test…)) or, with more than one, "3 agentes trabalhando em segundo plano…" (3 agents working in the background…)
  (the box line only says what the chat does with the conversation scrolled up; at the bottom, the agents'
  own lines do the talking). **Memória do chat** (Chat memory): on Linux, the clock tooltip says how much the turn occupies
  right now, "Memória deste chat: 3,1 GB" (Memory of this chat: 3.1 GB) (the agent and everything it opened: commands, tests,
  servers). The number is the `memory.current` of the cgroup of the systemd scope the chat runs in
  (`Escopo::memoria`, in `src/processos/unix.rs`, found through the process's `/proc/<pid>/cgroup` and
  only accepted if the cgroup is the scope's), read when the line asks and cached for 3 s
  (`src/motor/memoria.rs`); an idle chat is not read. On Windows, without systemd or without the memory controller
  in the user cgroup, the tooltip does not appear. There is no warning for a machine at its limit (the owner
  did not want a new warning). **Esc que não custa caro** (An Esc that doesn't cost much; usage round, item 6.2): when stopping would take down the
  whole process (background agents, the wait for them, an Ultracode turn), the
  first Esc only arms, and the line says "Esc de novo para parar os N agentes" (Esc again to stop the N agents) for 3 s; the Parar (Stop) button
  still stops directly. After an Esc (or Parar) that stopped the turn, the strip above the box
  shows "Você parou" (You stopped) with **Continuar** (Continue), and the box background says "O que fazer em vez disso? Enter
  continua" (What to do instead? Enter continues): Continuar, or Enter with an empty box, sends the same sentence as Retomar (Resume). This does not
  go to the bell and disappears on the first send, on switching chat or after 10 minutes. At the smaller widths the
  "ação N", the Esc label and, at the minimum, the clock go away. Above the box (`faixa_atividade`), aligned
  with its text and, on the right, with the send button, which enters with a fade and 8 px from below, only what
  asks for a button of yours stays: **Parada** (Stopped) with **Retomar** (amber when it was Vitral that stopped; the reason in the
  format of the list and the bell, "Parada · Vitral fechou" (Stopped · Vitral closed), on a single line, with an ellipsis when
  tight, and the whole reason in the tooltip; the action on the right is never cut), what failed (**Limite
  do plano · volta 15:00** (Plan limit · back at 15:00), **Sem conexão** (No connection), **O último turno falhou** (The last turn failed)) with **Tentar de novo**, waiting messages with **Entregar agora** and the
  chat folder that vanished from disk. Waiting for you, it goes away (the request is right below).
- **Planejar** (Plan). The plan appears in a card with an amber strip on top and the mode the conversation
  stays in after approval ("Depois de aprovar: Aceitar edições" (After approving: Accept edits)). **Aprovar e executar** (Approve and run)
  leaves Planejar and goes back to the previous mode (with no previous mode, the folder default; if that is also
  Planejar, Aceitar edições); the arrow beside it approves in another mode (**Aprovar e aceitar edições** (Approve and accept edits) or
  **Aprovar e perguntar antes** (Approve and ask first)). The field below takes what to change: Enter (or
  **Enviar mudanças** (Send changes)) returns the plan with the comment. Once answered, the plan **stays in the
  conversation**, at the point where it was proposed: a collapsed card with what you answered
  ("Plano aprovado" (Plan approved), with the green check, or "Plano recusado" (Plan rejected)) and the plan title, which opens the
  whole text in markdown, with Copiar and, for a rejected one, "Você pediu: …" (You asked: …) with the comment
  (`Janela::plano_respondido`, `mensagens::plano_na_conversa`). The card stays in view with the
  turn collapsed into "Trabalhou…" (Worked…), splits the call trail and Ctrl+F finds text in it; if
  the turn stopped before the answer, it says "Plano proposto" (Plan proposed). While the request waits, only the
  request appears. Measured on Claude Code 2.1.328 (11/23/2026, Haiku in `plan` mode): the
  `ExitPlanMode` call comes **empty** in the output; the plan only comes in the permission request (`input.plan`,
  with the `planFilePath` and the call's `tool_use_id`) and, once approved, in the result ("User has
  approved your plan…" and, after "## Approved Plan:", the plan). The engine stores the request text
  in the call's `detalhe` (`motor::guardar_plano`, up to `claude::MAX_PLANO`, 60 thousand
  characters; since the request comes through MCP and the call through the output, whichever arrives first waits for the
  other) and, lacking it, takes it from the approved result (`claude::plano_do_resultado`), which is
  also where the plan of a conversation brought from Claude Code comes from. A call without the text (the
  conversations from before this, the rejected plan of a brought conversation) stays as a trail
  line, "Propôs um plano" (Proposed a plan). In focus mode, which hides the tools, the card goes away too.
- **Pedidos de permissão** (Permission requests) in a double-frame card (`cartao_de_pedido`, in
  `src/ui/mensagens.rs`; `moldura_dupla`, in `src/ui/componentes/controles.rs`): outside, the
  frame with a 20 corner radius; inside, with 4 of clearance, the opaque card. On the open request that waits for you, the
  outer frame is metal (the 4 px ring); on a dangerous one, red; on a collapsed one, black. The header
  has the 28 tile with the tool icon, the affirmative title ("Rodar um comando" (Run a command), "Editar
  views.py" (Edit views.py), "Gravar notas.md" (Write notas.md), "Abrir docs.rs" (Open docs.rs), "Pesquisar na web" (Search the web)) and the tool in mono
  ("bash", "edit", the MCP server); the "fora da pasta" (outside the folder) in amber, when the path falls outside the
  chat folder, appears at any width. The body shows the whole command in a recessed box,
  with "$" in front, a mini-diff in Edit and the first lines in Write; the reason (the command
  description, the path) goes below. The command box shows about 11 lines and scrolls: with more than
  that (or more than 900 characters), the card warns "Comando comprido: role a caixa para ler tudo
  antes de permitir." (Long command: scroll the box to read it all before allowing.). Typing in the message box and pressing Enter denies the request and sends the text
  to Claude (see **Responder a um pedido de permissão escrevendo** (Answering a permission request by typing)). In the footer, **Negar e
  explicar…** (Deny and explain…; the icon; the text goes to Claude),
  **Negar** (Deny) and **Permitir** (Allow), and on the left the checkbox **Não perguntar de novo: edições nesta
  conversa** (Don't ask again: edits in this conversation) (or "este comando" (this command), or the tool name): checked, Permitir saves the rule, which
  applies already in the running turn (`permitir_sempre`); unchecked, it allows just this once. With a
  risky stretch in the command (`rm -rf`, `sudo`, `--force`, `| sh`...), the stretch shows in
  red, a warning explains, the checkbox goes away and the button is **Segure para permitir** (Hold to allow)
  (`BotaoDeSegurar`, in `src/ui/componentes/controles.rs`, with the `Segurar` of
  `src/ui/movimento.rs`): it only confirms after 0.9 s held down, filling from left to
  right, and releasing or leaving with the pointer cancels. By keyboard, the first Enter arms ("Enter de
  novo para permitir" (Enter again to allow), for 3 s) and the second confirms; key repeat from a held key and an Enter
  stuck to the first (under 250 ms) do not count. It works the same with animations off. With
  several requests at once, they stay under "Pedidos" (Requests) with the number, and only one shows the body (click
  the header of another to open it; the closed ones have Negar and Permitir for this time only right there, and the
  dangerous one, the hold).
- **O que um hook seu manda perguntar** (What a hook of yours orders asked). A Claude Code `PreToolUse` hook can return
  `permissionDecision: "ask"` (a deploy lock, for example): the call becomes the same card,
  in **every mode, even Sem perguntar**, with one extra line below the command, with the
  shield in amber: "Um hook pediu a sua confirmação: <o motivo que o hook deu>" (A hook asked for your confirmation: <the reason the hook gave>), whole and with
  line wrapping (up to 600 characters). That card does not have the **Não perguntar de novo** box, and
  a rule already saved in the conversation does not apply to it: whoever put the hook there wants to be asked every
  time. The same card (without the line) appears for an `ask` rule in the Claude Code settings.
  How it works, measured on Claude Code 2.1.328 on 11/23/2026 with a test hook: with "ask",
  it calls the `--permission-prompt-tool` tool in any `--permission-mode`, but only
  with `tool_name`, `input` and `tool_use_id`; without that tool it refuses the call and returns the
  hook's reason to the model as an error. That was the case in Sem perguntar, which opened without it: the card
  never appeared and the agent kept insisting. In `bypassPermissions` only what the
  user themselves ordered asked reaches the tool (hook or `ask` rule): writing to `.git`, `.claude`, `.vscode`
  or outside the folder and commands with `$(...)` go straight through, as before. The reason comes by another
  path, the output's `hook_response` (`--include-hook-events`), which does not carry the call id,
  only the tool name, and comes out right before the request: `claude::interpretar` turns it into
  `Evento::HookPediu`, the engine stores it (`src/motor/pedidos_de_hook.rs`) and the next request for the
  same tool, in the same chat, takes it. Without the id, pairing is by order: two calls of the same
  tool at the same time, with different reasons, can come out with the reason swapped; a reason that
  nobody came to fetch (another hook denied, the turn stopped) expires in 60 s and disappears when the process
  exits. The events option only goes from Claude Code 2.1.323 on (`VERSAO_EVENTOS_DE_HOOK`, the
  oldest in which it was checked; on one that did not know it, it would take the turn down): below
  that, the card appears as an ordinary request, without the hook line and with the box (in a turn that
  starts before the version is read, the version from the native installer link applies,
  `claude::versao_pelo_link`). `--permission-prompt-tool stdio` would bring the reason in the request itself (`decision_reason`,
  `decision_reason_type`), but the answer would have to go through the process input, which Vitral closes
  while the background agents are still working.
- **Pergunta com opções (AskUserQuestion)** (Question with options). When the agent asks something with alternatives, the
  question appears in a requests card (`Janela::cartao_pergunta`, in `src/ui/pergunta.rs`),
  in the double frame: each question with the short header in mono, the text and the options to click,
  one per line, with a round marker (single choice) or square one ("Dá para marcar mais de
  uma." (You can mark more than one.), the `multiSelect`), the label and the description below; last, **Outra…** (Other…), which opens a
  free text field. In the footer, **Não responder** (Don't answer) and **Enviar** (Send), which only turns on with all
  questions answered (Enter in the "Outra…" field also sends; with several questions, the
  footer says "1 de 2 respondidas" (1 of 2 answered), and the questions scroll inside the card when there is not enough height).
  **Não responder** returns the question unanswered (the model reads that nobody answered), without
  marking the call as a failure. Typing in the message box and pressing Enter answers without choosing an
  option: the text counts as the "Outra…" of each question (`motor::respostas_escritas`; the
  Enviar tooltip says "Responde à pergunta com isto, sem escolher opção" (Answers the question with this, without choosing an option)). In the bell, in the corner
  notice and in the list it appears as "Pergunta · <texto>" (Question · <text>), with **Ver pergunta** (See question), and it never has
  "Não perguntar de novo" (`modelo::regra_de_permissao`). Once answered, it stays in the trail as
  "Perguntou <a primeira pergunta>" (Asked <the first question>), which opens the questions, the options and the answer. Measured on
  Claude Code 2.1.328 on 11/23/2026 (Haiku, in a folder in /tmp): in `-p`, AskUserQuestion (and
  also `EnterPlanMode` and `ExitPlanMode`) only enters the `init` tool list **with
  `--permission-prompt-tool`**; without it (or with `--permission-prompts none`) it disappears, and
  `--permission-prompts host` alone does not bring it. That is why the sessions opened by
  Vitral in "Sem perguntar" never called it: until the hook "ask" commit, that mode opened without
  the permission tool. Now it exists in every mode, and the request reaches
  `mcp__vitral__permissao` even in `bypassPermissions`, with `tool_name`, `input` (the `questions`:
  `question`, `header`, `options` with `label` and `description`, `multiSelect`) and `tool_use_id`. The
  answer returns by the same path as permission, as `allow` with the `updatedInput` equal to the
  input plus `answers`, an object with the **whole question text** as key and the answer
  as value: the option label, the labels separated by ", " when there are several, or the free
  text (`motor::resposta_da_pergunta`, `Motor::responder_pergunta`; `pergunta::respostas_de`).
  The model receives "Your questions have been answered: "pergunta"="resposta"…". With the
  `updatedInput` without `answers` (that was the generic card's Permitir) it receives "The user did not
  answer the questions."; with only the `answers`, without the `questions`, Claude Code refuses the
  `updatedInput` for not matching the schema; when denied, the message returns as the call's error.
- **Ferramentas** (Tools). Consecutive calls become a trail, one 32 line per call, stuck together and
  linked by branches drawn in a single path: the type icon (in color only when it failed; on one that is still running, the spinning
  ring, `AnelGirando`, in place of the kit spinner), the verb ("Leu" (Read), "Editou" (Edited),
  "Rodou" (Ran)) and the detail; a failure has a light red background and "falhou" (failed) at the end. A detail that is code
  (command, file, Grep and Glob pattern, WebFetch address, MCP tool) goes in mono
  in a chip; what is prose (the Agent task, the web search, the task list) goes in Inter, without a
  chip. **Quanto a chamada levou** (How long the call took). One that is still running shows the clock at the end of the line after
  3 s ("Rodando cargo test 00:42" (Running cargo test 00:42)); when finished, one that took 10 s or more
  (`mensagens::CHAMADA_DEMORADA`) keeps the duration in the same place ("Rodou cargo test · 2 min
  14 s", `demora_da_chamada`; in Inter and with the dot, so it does not look like a command argument). The number is the `durou` of `Item::Ferramenta`, in seconds, noted
  when the result arrives (`motor::anotar_resultado` and `duracao_da_chamada`): from the request to the
  result of each call, by its id, so those running in parallel each get their own (Claude
  Code sends the result of each one when it finishes, out of order; measured on
  2.1.328). One that waited for a permission counts from when you allowed (`Motor.liberadas`, by the
  request's `tool_use_id`). One that went to the background, asked that way or because it passed its
  time, gets no duration: the result carries `tool_use_result.backgroundTaskId`
  (`ResultadoFerramenta.no_fundo`) and the command keeps running. The subagent keeps its own
  count (tokens, tools and clock). Calls from before this have no number. The clock
  of a running call lives in the messages, which are a cached view: once a second the Janela notifies
  only the messages of the panels whose chat has a call without a result
  (`Janela::andar_os_relogios_das_chamadas`, `motor::tem_chamada_rodando`). An agent flow (the
  `Workflow` tool) appears as **Fluxo de agentes** (Agent flow). Groups of two or more show the
  summary ("Rodou 2 comandos · editou views.py · leu 3 arquivos" (Ran 2 commands · edited views.py · read 3 files), with the names of edited files
  up to two and the count above that; outside the wide panel, the short form, "2 comandos · editou
  views.py · 3 lidos" (2 commands · edited views.py · 3 read), so the cut does not take the last number),
  with the trail icon on the
  left, from which the branches descend, and the chevron at the end, as on the calls and on "Pensou" (Thought); they open
  while working and collapse when finished, respecting a manual choice. Inside the group, the verb
  has a 56 column so the details line up. A loose call (alone between two
  utterances) starts at the group header column, with no branch, and with the detail right after the verb.
  Collapsed, the group shows the images its calls read or wrote: a single one, in its
  pill; more than one, on a single line (`pilha_de_imagens`): the stack of up to 4 square thumbnails of
  22, overlapping like avatars, and "9 imagens" (9 images), with the names in the tooltip (up to 10); a click opens the
  viewer on the first, and the arrows go through the others. Open, each call shows its own pill.
  **Imagem que a ferramenta devolve** (Image the tool returns; `src/imagens_recebidas.rs`; the idea came from Paseo). The
  screenshot from a browser MCP (chrome-devtools, playwright), the generated image and the Read of an
  image arrive in base64 inside the `tool_result`; the conversation only showed "(image/png)". Now the
  reader splits the result into text and images (`partes_do_resultado`, which reads the Claude API
  block, `source.data`, and the MCP one, `data` and `mimeType`), the images go out in a separate event
  (`Evento::ImagensDoResultado`, right after the `ResultadoFerramenta`) and the engine saves them in
  `~/.local/share/vitral/imagens/<chat>/imagem-<hash>.<ext>` (`guardar_na_chamada`): the
  `Item::Ferramenta` keeps only the paths (`imagens`), and the base64 never goes to
  `chats/<id>.json` or to `estado.json`. The name is the start of the content's SHA-256, so the
  same image again does not save another file; the extension follows the byte signature (PNG,
  JPEG, GIF, WebP), and what the conversation cannot show stays as the mark in the text
  ("(image/svg+xml)"). The chat folder belongs only to the owner (0700, files 0600). The call shows the
  thumbnail of each image (`imagens::da_chamada`; more than one, side by side), which enters the stack
  of the collapsed group and the viewer queue along with the attached ones, always as a thumbnail that opens in the
  viewer, never as a large image in the middle of the text (owner, 11/17/2026). On the Read of an image,
  the one on disk applies, whole, while it is there; deleted (a screenshot in /tmp), the saved one stays,
  which is the one the model saw. **Tetos e limpeza** (Caps and cleanup): 10 MB per image (`TETO_POR_IMAGEM`; a larger one
  is not saved and the result says "passa do teto de 10 MB e não foi guardada" (exceeds the 10 MB cap and was not saved)), 100 MB per chat
  (`TETO_POR_CHAT`; past it, the oldest in that folder go, and their thumbnail disappears) and 1 GB
  for all conversations together (`TETO_TOTAL`, checked when Vitral opens, removing the oldest from
  any chat). A chat that leaves Vitral takes its folder with it (`Dados::apagar_chat`),
  and on open the folders of chats that are not in the state and no longer have `chats/<id>.json` are removed
  (`varrer`). Conversations imported from Claude Code and those from before this version do not have the
  images (the importer reads only the result text). **In Codex** (checked on the 0.169.0 app-server
  with a real turn, 11/23/2026): `view_image` does not return base64, but the
  `imageView` item with the file path, which becomes its reading ("Leu a.png", with the thumbnail from
  disk); the result of an MCP tool (`mcpToolCall.result.content`) carries the MCP
  blocks, and their image is saved like Claude's (before, the whole result JSON went to the conversation,
  with the start of the base64); and the generated image (`imageGeneration`, from the schema only, with no real
  turn) points to the `savedPath` or, without it, is saved from the `result`.
  **Turno pronto** (Finished turn). When the turn ends, everything that came before the final answer collapses into a
  single line, "Trabalhou 6 min 12 s · 13 comandos · editou carrinho.py e motor.rs" (Worked 6 min 12 s · 13 commands · edited carrinho.py and motor.rs) (the names of
  edited files up to two; above that, the count), with the answer right below; a click on the
  line opens and collapses the rest. It only collapses a turn with at least one trail and one intermediate
  text; intermediate text that ends in a question, the error notice and the failure card
  stay in view, and the images of the hidden calls stay below the line, in the stack.
  The duration is the time of the last item minus that of the message (no number in conversations from before
  0.4.0). **Checagens do turno** (Turn checks; `src/ui/checagens.rs`): next to the commands, the line says how
  each check the turn ran ended, "tsc passou · vitest falhou 2×, passou · cargo test
  passou" (tsc passed · vitest failed 2×, passed · cargo test passed): the last run of each one counts, and earlier failures become "falhou N×" (failed N×) (the
  "falhou" in the red of the call that failed; the rest in the line's tone; the whole list also
  in the tooltip, because in the narrow panel the part that does not fit disappears whole). They count `cargo test`,
  `check`, `clippy`, `build` and `nextest`, `tsc`, `vitest`, `jest`, `pytest`, `go
  test`, `build` and `vet`, and the `test`, `build`, `typecheck` and `lint` scripts of npm, pnpm, yarn
  and bun (with `test:unit` and similar), behind variables, `cd x &&`, `timeout`, `npx` and
  relatives. The result is the `erro` of the Bash call when the exit code is that of the check
  (nothing after it besides `&&`). With a `|` after it (`cargo test 2>&1 | tail -40`), a `;` or
  a `||`, the exit code belongs to another command: what the output says counts ("test result:" from
  cargo, "error TS" from tsc, "Test Files" from vitest, "Tests:" from jest, "N passed in" from pytest,
  "ok"/"FAIL" from go), and with no signal at all the check is left out, instead of appearing as
  "passou" (passed) without having passed (npm scripts behind a pipe never appear). In a chain of
  `&&` that failed, the last check that left a signal in the output ran, so the earlier ones
  passed and those after a failure do not appear; with no signal from any, all of them show as a
  failure. A custom wrapper (`./checar.sh`) is not recognized. In the pair, each agent has its own
  collapsed one. Ctrl+F opens the line when it finds something inside it. The history imported from Claude Code does not collapse. The line's tooltip says the tokens and the
  cost of the turn, "182 mil tokens · US$ 0,84" (182 thousand tokens · US$ 0.84), taken from the spending ledger (`Livro::do_turno`, in
  `src/motor/gasto.rs`, by the message that opened the turn: its index and time), and splits the
  tokens into three numbers: the new input (the undiscounted plus what was written to the cache), what was read
  from the cache and the output. Only in the tooltip: the line stays short (owner, 11/18/2026). Below, in the same
  tooltip (`onde_uso::dica_do_turno`, with `Livro::estatistica`): how much of the input came from the cache
  ("96% da entrada veio do cache" (96% of the input came from the cache): what was read from the cache over the new input, the written and the read), the
  model with the round trips to the model ("Opus 5.5 · 12 passos" (12 steps)), the times ("6 min 12 s no total · 4 min
  3 s de modelo · 1,8 s até a primeira palavra" (6 min 12 s total · 4 min 3 s model · 1.8 s to the first word)) and how much it cost to start Claude Code ("1,6 s para
  abrir o Claude Code" (1.6 s to open Claude Code)). Each line only appears with its data; see **Estatística do turno** (Turn statistics), in
  **Contas e uso dos planos** (Accounts and plan usage). A turn with no line in the ledger (those from before this, and those
  from more than two months ago, which opening does not read) stays as it was. A click on a
  call first shows what it asked for, in a mono box: the diff of an Edit, the start of what a
  Write saved and the whole command of a Bash (up to 60 lines on each side are kept in the
  conversation). The diff shows only what changed (`edicao::diff_da_trilha`): the lines that left in
  red and those that came in in green, with 3 equal lines around each change, "… 9 linhas
  iguais" (… 9 equal lines) for what was left out between two changes, and the changed word with a stronger background
  inside the line. Before, the old side came entirely in red and the new entirely in green: one
  word changed in 10 lines meant 20 colored lines. Lines are matched by the common
  subsequence that already laid the edit over the file (`edicao::casar`, with the same cap of 250 thousand
  comparisons: above it, the equal start and end stay, and the middle goes out and comes in). The word is
  marked when the same number of consecutive lines left and came in (the first with the first),
  comparing words, whitespace and punctuation; lines with fewer than half their words in common, or
  that add up to more than 1024 bytes, get no mark, because the whole line already says it changed. The
  diff's Copiar takes what the screen shows. The permission request of an Edit or MultiEdit uses the
  same diff, with 2 lines of context (`diff_da_troca`). In a swap with more than 60 lines on
  one side, the equal start of both sides is not kept (3 lines remain before the first
  difference, `claude::sem_o_comeco_igual`): without that, a change at line 150 fell after the
  cut and the diff opened with no change at all. **Ver a edição** (See the edit; `src/ui/edicao.rs`): on an edit that
  succeeded (Edit, MultiEdit, Write, NotebookEdit; Codex's `fileChange`), a click on the line opens the
  file in the right panel already at the edit, and the chevron at the end, alone, shows the diff right
  there. Codex delivers one file per line, each with its own diff (the `changes[].diff` of the
  app-server's `fileChange`, checked in the Codex 0.169.0 schema: the unified diff in
  `update`, the content in `add`, which becomes a Write, and what was there in `delete`), read at the end of the
  item, when the patch arrived whole. The old Codex record, which grouped several files
  in one call without the diff, appears with one line per file (in place of the box with the list of
  "mudou …" (changed …)), and a click opens the file as it is now, with a discreet notice. Collapsed, the
  turn ("Trabalhou…") and the call group show below them one line per edited
  file, with the +N −M and "novo" (new) (up to 5; above that, "e mais N arquivos" (and N more files), which opens the
  collapsed one): a click opens that file's edit, adding up the turn's calls. Commands have command and output in the same box. The result shows the
  last 40 lines, with Copiar and Mostrar tudo (Show all); that of an edit that succeeded is just "o arquivo foi
  atualizado" (the file was updated) and stays out. On hover, a line that read or changed a file that is still on disk
  gets at the end the icon **Abrir o arquivo ao lado** (Open the file beside). The Vitral tools a chat uses
  (`mensagem_para_chat`, `abrir_chat`) have their own icon and verb, and on hover the line gets
  **Abrir o chat** (Open the chat; the speech bubble; Ctrl+click opens beside). Those from earlier versions (those of the
  Mestre and the helpers, in old conversations) keep their verbs. Outside the wide width, the verb and the sentence go to the tooltip (the icon already says what happened) and the title gets the space.
- **Mensagens** (Messages). What you write sits in a low blue bubble across the full width of the column (radius 16,
  tip at the bottom right corner, text at 14/22, one step below the answer), with the pills
  of the attached images and files in the dark well above the text and the badges
  **Ultrathink** (in rainbow) and **Ultracode** (in violet, with the flow icon) when the message
  went with them. Paths that exist in the chat folder become links and open in the right panel,
  as in the answer; inline code and code blocks go in the dark well. A lone command
  ("/compact") becomes a blue pill in mono. Text over 14 lines shows 12, with the last one
  fading into a veil, and **Mostrar tudo** with the chevron. The line breaks
  you typed stay in the bubble (`com_quebras_duras`: each line followed by another gets the markdown hard
  break, except in fenced blocks and tables), and Copiar takes the text as it was written. Messages another chat
  (or Vitral) delivered show who sent them (the other chat's bubble or the Vitral logo; in old
  conversations, the circle of the Mestre, of the coordinator or a helper's wrench) and a
  preview; those from another chat have **Abrir o chat**. The preview ends with an ellipsis before the arrow
  column, never cut in the middle of a letter; a click opens the whole text. Vitral's notices are
  centered markers ("Sessão nova" (New session), "Turno interrompido" (Turn interrupted), "O Vitral fechou no meio do turno" (Vitral closed in the middle of the turn),
  which is how the saved notice of the cut turn appears, without the instruction to use Retomar, because the
  strip right below already has the button; when it is the last item and the strip says "Parada · Vitral
  fechou", it does not even appear, so the same fact does not show twice, except with search open),
  faint lines ("Tentando de novo." (Trying again.))
  or, when it is an error, the error card (title in Portuguese, what to do, Copiar in the corner, the
  agent's text in **Detalhes** (Details); see **Chat que cai** (Chat that drops)). The swap of the old Claude Code session for a
  new one with a summary (the one that vanished and the one that expired) is the marker "Sessão nova, com um resumo da anterior" (New session, with a summary of the previous one),
  and the account-switch or agent-switch lines carry the swap icon. What arrives with the conversation open in the panel (your
  message, the answer, an event, a notice, a call arriving in a trail already in view) enters with
  a fade and 8 px from below, once; switching conversation in the panel makes the whole conversation enter
  with a fade, and switching panel or scrolling animates nothing. Between turns there are 32 px before your bubble
  (and before the day separator), 12 after it and 12 between the blocks of a turn (`esp::ENTRE_TURNOS`
  and `NO_TURNO`): with the outside gap more than 2 times the inside one, each question stays tied to its answer
  and turns read like paragraphs (appearance report, 11/16/2026, V3).
- **Rolagem** (Scrolling). A scrollbar visible per panel when the content overflows, with its own identity and
  drawn over the fixed area, outside the scrolled content. Earlier items appear in pages
  of 60 when nearing the top; compensation for the added height keeps the reading in place.
  Of the items that have already entered, only the blocks in view are mounted (see **Custo de um redesenho** (Cost of a redraw)):
  scrolling a conversation that has already been scrolled up a lot costs the same as scrolling a freshly opened one.
  The imported history widens the file reading window when necessary. The old smoothing
  was removed on 11/16/2026: it restored the offset before detecting the upward scroll,
  keeping the panel stuck to the bottom. At the bottom, the conversation stays glued to the bottom also
  when the panel changes width (opening beside, Ctrl+B, zoom, dragging the window edge), when
  the lines wrap again. Any upward scroll with the wheel, the touchpad or the bar releases the bottom; then
  streaming does not pull the screen back and the conversation does not count as read; the pill with the arrow (and
  "Novas mensagens" (New messages), with a blue dot, when something arrived), which enters with a fade, returns to the bottom,
  as does **Ctrl+End** (scrolling down, the last 48 px already latch again). Away from the bottom, the foot of the
  messages area fades out over 24 px down to the card, so the last line is not cut off abruptly above
  the strip or the box (at the top, no). Opening a block at the end of the
  conversation (a tool's output) does not jump to the end of it: the
  panel stays where it is.
- **Onde a conversa abre** (Where the conversation opens). A chat with an answer you have not seen opens at the start of it when it
  does not fit on screen (with the "Trabalhou…" line right above and the "Novas mensagens" pill below);
  when it fits, it opens at the bottom, as always. A "Novo" (New) separator, in blue, marks the first item
  you have not seen (not counting your own messages). Opening does not mark as read: the chat only leaves
  "Terminou" (Finished) and the blue dot with the end in view (scrolling there or with Ctrl+End). When switching tab
  in the middle of reading, each panel remembers where you were in each chat (the item at the top and
  how much of it had already scrolled), and returning to the tab goes back to that point; this holds while Vitral is
  open. If Corrigir removed the item, the conversation opens at the bottom.
- Messages, requests and the box sit in a centered reading column (up to 860 px, with a 16 margin).
  In a short window, the plan and the requests shrink and scroll, and the box grows less. Code blocks
  have syntax coloring. **Copiar tabela** (Copy table): each table in a message has the Copiar of the code
  blocks (`BotaoCopiar`, faint) at the right corner, just below it: a click copies the table as
  Markdown, and with Shift it copies tab-separated, which a spreadsheet pastes into columns
  (`mensagens::tabela_para_planilha`; a tab and a line break inside a cell become a space).
  The copied Markdown comes out without the internal links that file paths get in the answer
  (`links_arquivo::sem_links_do_vitral`: the path goes back to being inline code).
  It is the kit's `table_actions` hook, which draws what it receives in a row below the table, and
  not on top of it as in the code block: that is why the button sits below, and the text after
  a table moves down about 28 px.


**A caixa de mensagem: menu "/", "@" e chip do modelo** (The message box: "/" menu, "@" and model chip) (`src/ui/composer.rs`, `src/claude.rs`).
The "/" lists all commands and skills, in a list that scrolls (the arrows carry the marked item along): each
row has the description, the argument hint, the "skill" badge, the origin ("projeto" (project), "pessoal" (personal),
"plugin <nome>") and an icon that opens the `SKILL.md` on the right (Ctrl+O opens the marked one's). The order is
by match quality (`qualidade_do_casamento`: exact name, starts with, a part of the
name starts with, contains, description only); "@" and "#" keep the cap of 8. The catalog comes from Claude Code's
`initialize`, per folder (`claude::catalogo_da_pasta`): the origin is the mark it puts
in the description ("(user)", "(project)", "(plugin) …", `builtin`) and the file is found in
`.claude/skills` of the folder and the folders above, in the account (`CLAUDE_CONFIG_DIR` or `~/.claude`) and in
the plugins of `installed_plugins.json`. It lasts 10 minutes (`precisa_perguntar`) and is redone when the chat's
agent changes. The menu opens for the word under the cursor, not only for the last one (`gatilho_em`): the
`/` only on the first word, `@` and `#` on any; choosing replaces the whole word
(`trocar_palavra`) and goes past the space that already existed. A path with a space goes in quotes
(`@"minha pasta/a b.rs"`, also in `mencao.rs`). In the narrow box (3 panels at 150%) the model
chip keeps the family name ("Opus", "Fable"); mode and effort keep only the icon, and the name
drops to just the brand when it does not fit (`decidir_modelo`, measured like the effort). On opening, in the
background, the `colado-*` files that no conversation, queue or draft cites are removed from `anexos/`
(`src/limpeza_anexos.rs`; idle for more than a day; an unreadable data file cancels everything). In

## O que fica à vista (What stays in view)

The default is the minimum, and the rest shows up when needed:

- **The message pinned at the top** (`src/ui/mensagens.rs`, decided in the list's prepaint): your
  most recent message that has already scrolled above the top of the conversation sticks there, in
  two lines, in a strip with the card's layers; clicking it takes you to the message. The next
  message, when it reaches the top, pushes the strip up (`Painel::empurrao_da_presa`, with the
  strip's height measured when it is drawn), like the "sticky" in VS Code: before, it passed
  underneath, and the upper one covered the lower one (owner, 2026-11-18: "it looks kind of buggy
  when it's like this").
- **The top of the list is a single row**: the logo (the Vitral menu) and the tabs **Conversas |
  Arquivos** (Chats | Files), at the same height (40), in a full-width segmented control
  (`topo_enxuto`, in `src/ui/trilho.rs`). The row has the height of the panels' header (44), so the
  logo and the list's tabs line up with the conversation tabs next to them. Arquivos shows the
  folder of the open chat.
- **The search and the "+" live at the foot of the list**, below Arquivadas (Archived), the running
  agents and the bell, also at a height of 40. Collapsing the list (Ctrl+B) lives in the logo menu,
  **Recolher a lista** (Collapse the list).
- **The list is continuous**, with no title or section headings: what is waiting for you, what is
  running, the pinned ones (with the pin) and all the others, in that order. Each chat is a single
  row of 32 (the neutral bubble, the title and the time; the folder goes in the bubble's tooltip);
  the second row comes back in the search that matched the text, with the folder name in front, and
  the search result uses the same small bubble as the other rows.
- **New chat asks nothing.** The "+" and Ctrl+N open a chat right away, in the folder of the open
  chat (see **Chat novo**, in **Como funciona**). Choosing the folder lives in the logo menu,
  **Novo chat em outra pasta…** (New chat in another folder): the folders that already have a chat
  (with the path under the name), **Outra pasta…** (Other folder…), which picks one on disk, and
  **Sem pasta** (No folder).
- **The usage rings** float in the top right corner of the window, just below the ×, one above the
  other (`uso_no_canto`, in `src/ui/uso_plano.rs`), each on the piece's opaque disc, with a 1 px
  hairline and a short shadow, like a floating button: the conversation reaches the edge underneath
  them, with no gap (owner, 2026-11-16: they went through the header and came back the same day,
  "the usage one has to stay where I said, below the ×"): each one only with that service's account
  connected (0.10.0; `aneis_a_mostrar`): the Claude one with an account whose login is saved and
  not lost (`Motor::claude_conectado`, from `busca::tem_login`, read on open and on every usage
  lookup), the Codex one with its `auth.json` and a usage reading. Without an account, the ring
  disappears, the right panel's row does not reserve space (`reserva_dos_aneis`) and "Entrar"
  (Sign in) and "Instalar o Codex…" (Install Codex…) remain in the logo menu. **Mostrar o uso do
  plano no canto da janela** (Show plan usage in the window corner), in Configurações > Aparência
  (Settings > Appearance) (`Config.uso_no_canto`, on by default), hides them; the usage stays in
  Contas do Claude (Claude accounts).
- **The conversation header has little in it:** the tabs and the **+** (new chat or history). The
  rest of the panel, including opening and closing the right side, lives in the right-click menu on
  the header (or Shift+F10).
- **The account chip**, in the input box, only appears with more than one account.
- **The × in the corner is the window's.** With the right panel open, its header has the button
  that closes the panel (the collapse icon), a divider and then the window buttons; the × lights up
  red and the tooltip says "Fechar o Vitral" (Close Vitral). With the right panel closed, the window
  buttons sit in the header of the last conversation, also after a divider and a little apart from
  the **+**, so the × does not look like the one that closes the panel.

## Acabamento (Finish)

The finish in use is **Grafite** (Graphite) (`superficie::GRAFITE`, in `src/tema.rs`), from
2026-11-12; the earlier navy blue is still in `superficie::MARINHO`, and changing `ATUAL` brings it
back.

- **Color.** A cool near-black, in steps (the base, the conversation, what is elevated), and a
  single accent, full blue (`#2B63FF`): links, the text of what is running and, very lightly, the
  chip of the selected item. The "+" and the send button are metal keys, not flat blue. Selection,
  hover and focus ring are neutral. The icons of the input box chips (mode, effort) are neutral;
  color is kept for what needs care ("Sem perguntar", No asking).
- **Liquid metal: the plate and the two layers.** Behind everything there is a plate of liquid
  metal. The plate is the "Mesh drift" recipe that the owner built on 21st.dev and pasted on
  2026-11-12 ("try to focus on this liquid metal instead of glass"): five blobs (one almost black
  and four blue) drifting, with film grain and a light that follows the pointer (only while the
  metal is moving). It is opaque: the wallpaper is gone (the glass that let you see it is gone).
  On top of it the app has **two layers**. The **base** is the whole window: the plate under a dark
  blue veil (`vidro_leitura` of Grafite, #0A1533 at 0xE3: 11% of the metal shows through, and the
  dark stays a deep blue that shifts position, lighter where a blob passes). The list lives
  directly on the base, with no card, background or edge of its own (in the drawer, the surface is
  the drawer's). On top float the **cards**, the conversation and the right panels (Este chat, the
  open file), with the floating shadow (`sombra_flutuante`) and almost opaque (5% of the metal
  shows through). Window margin and gap between cards: 8 screen px, both equal and not growing with
  zoom (the gap between the list and the 1st panel, between panels and between the last and the
  right edge is half on each side of the divider; owner, 2026-11-16: "the one between the chats has
  to be the same as the edges"); the divider's grab strip is wider than the gap and goes over the
  pieces' edge, invisible. Card corners: 16 (`margem`, `vao` and `raio_peca` of the variant). Menus
  and popovers are opaque and one step lighter than the piece they come from (`flutuante` #1F222B,
  with the deep shadow dropping 24; lighter than that, the muted text on the floating panel goes
  below the 4.5:1 floor). Text never goes on raw metal: the alphas come from the contrast
  calculation against the lightest color the plate delivers (`TETO_DO_METAL`, #ACD9FF, the light
  blob with the light on top), and the test `contraste_dos_tokens_no_pior_caso` runs against it
  (0xE0 is the minimum for the list). For what goes on top of the metal there are two veils:
  `Cores::veu_de_placa()` (black at 56%, under `forte` and `texto`) and `Cores::veu_de_botao()`
  (black at 36%, under a white icon).
- **The metal engine** (`src/ui/componentes/metal.rs`). The shader's calculation runs in Rust on a
  grid of 1 point per 8 px of the window (about 20 thousand points, 2 ms in the debug binary),
  becomes a small image (a hand-built BMP, which GPUI's `image` already knows how to read: no new
  dependency) and the video card stretches it with a linear filter; the grain, which is per pixel,
  goes on top in a fixed 256 px tile. One plate per window: `metal::fundo` (the first child of the
  Janela and of the showcase) makes the frame and keeps it; each `metal::abertura` paints a cutout
  of the same image, with no new calculation (`Encaixe::Continuo` shows the piece of the plate that
  is behind; `Encaixe::Cobrir` shrinks the whole plate until it covers the box, for buttons and
  plates), and `metal::aro_de_metal` is a 2 to 4 px opening around an opaque core. The test
  `metal_so_pelo_motor` forbids `paint_image(` and `drop_image(` in the rest of `src/ui`. No
  opening inside a view with `.cached()` (the kit's tooltip uses it): the texture changes at every
  tick. What lives in a cached view (your bubbles and the conversation's pinned strip, through
  `metal::fundo_da_lista`) uses `metal::abertura_parada`, a still frame of the field that is only
  redone when the window changes size: with the plate moving, each bubble stayed on the frame in
  which it was drawn and changed when the pointer passed over it. On Windows, GPUI's renderer
  (Direct3D) samples the sprite with a linear filter and returns the atlas space when the image is
  released, like the Linux one (read in the source; still to be seen running there).
- **When the metal moves.** It is the exception to "nothing animates at rest", with a measured
  cost. Each tick of the loop redraws the whole Janela, and in the debug binary that costs about 35
  ms of CPU: at 8 frames per second at rest the app used 30% of a core (bench, idle chat open,
  pointer still on the window, 30 s; the limit was 8%), and the metal itself was the smallest part
  (with the field frozen, 28%). So the policy is: **at rest the metal stays still and nothing
  redraws** (0% measured); it **moves at 20 frames per second only with a chat in view that is
  working** (a chat in an open panel; what runs in the background does not wake it), and only then
  does the light follow the pointer. Moving the pointer does **not** wake the metal: it used to, and
  each movement redrew the whole window 20 times per second, which choked scrolling (owner,
  2026-11-12). The exception is the logo button, which animates with the pointer over it or with
  the menu open (see **Botão do menu**). With the window unfocused or reduced motion (GNOME without
  animations, Animações (Animations) turned off in Settings, `VITRAL_SEM_ANIMACAO=1`) it stays
  still, at the initial phase and without the light. While moving, debug measures 15 to 18 frames
  per second and 52% to 62% of a core (the Janela's redraw, not the metal). The resting rate is a
  single constant, `movimento::QPS_EM_REPOUSO` (today 0; with 8 it animates at rest again), and
  `movimento::laco_a(window, cx, qps)` is the entry point: it keeps the phase in `f64` and the time
  of the next tick per window, on the beat (drawing time is not added to the interval). What
  decides the plate's rhythm is `metal::fundo(trabalhando, …)`, called in the Janela's `render`.
  `VITRAL_QUADROS=1` shows the frames and the field's time.
- **Background transparency and blur.** In Settings, **Transparência dos fundos** (Background
  transparency) goes from Opaco (Opaque, the default) to Leve (Light), Média (Medium) and Forte
  (Strong) (`tema::NIVEIS_DE_TRANSPARENCIA`): the metal loses opacity, the base veil and the card
  let through what is behind the window, and a grain on top makes it read as frosted. At the
  transparent levels the window asks the system for the blurred background (`Blurred`, in
  `fundo_da_janela`, in `src/main.rs`), with the same code on every system, and what is behind
  appears truly blurred: on Windows (acrylic), on macOS, on KDE and on GNOME 51 and later. On
  Wayland GPUI only knew the KDE protocol; the standard one, ext-background-effect-v1 (the one GNOME
  51 brought), comes from `vendor/gpui-pre-linux` (see `Cargo.toml`). With both in the compositor
  the standard one wins, and in both the blur covers only the window's frame, without the shadow
  margin. **On GNOME 50 or earlier** (and on X11) there is no blur: the window is only transparent,
  with the veil and the grain on top (on GNOME, the Blur my Shell extension can blur from outside).
  On Opaco the plate covers everything and the request is only for transparent, so as not to spend
  the blur; changing the level changes the request immediately. `VITRAL_FUNDO=mica`, `transparente`
  or `opaco` forces a background at any level, for comparison (Mica only exists on Windows 11).
- **Three materials.** Everything is made only of light and 1 px shadow on the inside
  (`src/tema.rs`): **glass** on the cards (`aresta_de_vidro`: a 1 px light hairline on the sides,
  light on the top corner itself, a 1 px shadow at the base and a 3% glow that enters about 40 px
  from the top; the glow stays at 3% because at 5% the muted text over the list fails contrast),
  **key** on what is elevated and selected (`aresta_de_tecla`: the selected pill of the segmented
  control, the shortcut keys) and **well** on what is carved in (`aresta_de_poco`: the Search, the
  segmented control's track, the counters). The pieces' edge lives in a separate layer, the last
  child of the piece (`camada_de_aresta`, or `.aresta_de_vidro(raio)` after the children), because
  the inner shadow paints before the children; and the piece cannot have a `border`, which pushes
  the layer inward and makes the light fall on the second row of pixels (that was the case of the
  conversation, with the 1.5 border). Each side is a shadow offset by 1 px, not a ring: the ring
  also went across the base, and its light with the shadow on top gave the background's tone. Key
  only on what is selected, never on every list row.
- **No halo; the metal says "it's here".** No gradient, no blue halo and no blue border. The "it's
  here" is always a piece of the metal: the 2 px **metal ring** (`metal::aro_de_metal`, around an
  opaque core) on the message box and on the search when focused and on the tab under the pointer;
  the 4 px ring on the frame of the permission request waiting for you; and, on the selected panel,
  the **light from below** (the dock, below). The blue corner marks and the animated dotted line are
  gone (the `cantoneiras` still live in `superficies.rs` and in the kit, `cantoneira` in
  `vendor/gpui-component/src/styled.rs`, and only the showcase uses them).
- **The dock (the light from below).** The foot of every conversation card is an opening through
  which the metal shows, with the message box resting on it (`src/ui/conversa.rs`). Only the active
  panel lights it (180 ms; fades in 120): the whole metal field shrunk to cover the dock, with 20%
  of the accent blue on top and the light grain (`metal::abertura_lisa`), and the card's dark
  slowly fading from top to bottom (`tema::gradiente_da_doca`, in two layers), like a light coming
  from below. On the other panels the foot stays dark. In the empty conversation the dock is tall
  (172 px above the box, or 56 in a window under 640 high); otherwise, flush with the box. It is in
  the flow: the messages end where it begins, and no text sits on top of the metal.
- **Reticle.** A single texture, in the shader's own pattern, one quad per use: the **weave**
  (`trama`, a 2 px checkerboard in one color, the rest transparent), the **dot line**
  (`fio de pontos`, the checkerboard in a single row, "2 lit, 2 off", in place of every solid line:
  between the lines of the Settings groups, indented 12, above the key footers of the input box
  popovers, of Novo chat and of the popover body, in the menu separators and next to the top label
  of the choice popovers) and the diagonal **hatching** (`hachura`, for what has gone away). The
  side is measured in device pixels and rounded (`lado_da_trama`), otherwise the checkerboard blurs
  at 125%. The drawing lives in the kit (`trama`, `fio_de_pontos`, `fio_de_pontos_em_pe`) and the
  app goes through `ui/componentes/graficos.rs` (`trama`, `fio_de_pontos`, `hachura`); the test
  `reticula_so_pelos_graficos` forbids `checkerboard(` and `pattern_slash(` in the rest of
  `src/ui`. Both renderers have the patterns (Linux's `shaders.wgsl` and Windows's `shaders.hlsl`,
  cases 2 and 3), so there is no fallback to flat.
- **Corners.** A single standard for controls: `raio::de(altura)`, in `src/tema.rs`, 45% of the
  control's height (16 on a 36 button, 14 on a 32, 9 on a 20 chip): well rounded, never round. It
  is the logo's shape at scale. Buttons, chips, tabs, the search, badges and menu items use it; only
  dots, thin bars and rings stay round. The pieces and large boxes follow the tokens: 16 on the
  window's cards, 20 on the input box frame, on popovers and on menus, 14 on inner cards, 10 on
  rows.
- **Rings.** Every ring has a 60° gap at the bottom and round ends (`pintar_arco`, `Anel` and
  `AnelGirando`, in `src/ui/componentes/graficos.rs`): it goes from 7 o'clock to 5 o'clock, with
  the dark track and the fraction's arc on top. `AnelGirando` is the app's "loading": the same ring,
  open, making a turn every 1.2 s in the 20 frames per second loop (still, with the gap at the
  bottom, under reduced motion); it replaces the kit's spinner in the tool rows, in the input box's
  status line and in the stop button. The usage ring (`AnelDeUso`) is one per service and shows the
  tighter of the two windows (5 h or week): there used to be two, one inside the other, and with
  the gap they read as "two bars".
- **The chip of the selected item.** The selected item (in a choice menu or popover, the selected
  tab at the top of the list, the effort level) goes on a light blue chip, with a hairline of the
  same blue and the key relief, in the corner of the controls (`ficha_do_escolhido` and
  `ficha_de_metal`, in `src/ui/componentes/superficies.rs`). It used to be a cutout of metal under a
  veil; on a long rectangle it came out black on one side and speckled blue on the other. The name
  `ficha_de_metal` stayed.
- **Menus and popovers.** The logo menu and the menus with an object header start on a **metal
  plate** (`placa_de_metal`, `cabecalho_objeto`: the cutout of the plate under the plate veil, 52
  high, with the symbol, the name in white and the detail in mono). Shortcuts come out as drawn
  keys, separators are the dot line, the item under the pointer (or the keyboard) gets the key
  relief, with the corner `raio::de` of its height, and the footer is a dark strip with the
  technical line in mono (in the logo menu, the version and the commit). The outline lost the blue
  ring. In the choice popovers (permission mode, account, model, effort), each item's icon goes on
  a 28 tile (`azulejo_de_escolha`) and the top label gets the dot line out to the edge.
- **Type, in two voices.** Inter for text. The second voice is the tiny JetBrains Mono, like a
  measuring instrument: in code, in keys, in `etiqueta()` (mono 11/14, for facts and measures; the
  `Etiqueta` component, a hairline pill with icon, tone, `baixa` and `fundo`, lives next to
  `Selo`) and in section labels, which come out in mono 11 muted, in capitals and with a small 8 by
  1 dash in front ("PASTAS", "CHATS"). The capitals are only in the drawing (`etiqueta_de_secao`,
  `CabecalhoDeSecao`, the title of `GrupoDeOpcoes` and the group label of the kit's list with
  search); the data stays "Pastas", "Hoje". It stays at 11: GPUI has no letter spacing and 10 in
  capitals is cramped. Every small number (`metadado`, `micro`, `numero`) uses equal-width digits
  (`tabular`, Inter's `tnum`), and running time does not make the column move. The empty screen's
  title has its own size, `titulo_heroi` (28/34).
- **Keys.** A single drawing across the whole app (`desenho_de_teclas`, in
  `vendor/gpui-component/src/kbd.rs`, which the kit's tooltip and the app's `desenho_teclas` call):
  18 high, at least 18 wide, no border, a light background at 8% in relief (light on top, shadow
  below) and the letter in mono 10. The list's search shows "Ctrl" "K" on the right while it is
  empty and unfocused, with the list from 276 px.
- **Logo.** The owner's V (a drop on the left, joined by a thin neck to the thick leg on the right),
  in white, on a liquid metal square with 45% corners (the same `raio::de`). It is the same
  everywhere: the menu button, the Vitral menu header, the Vitral messages in the conversation, the
  recovery screen and the app icon (the PNGs, the `.ico` and the two SVGs). The files come from
  `ferramentas/logo/`: `rasterizar.py` extracts the V from `v-do-dono.png` (the dark shape on a
  white background) and writes `assets/vitral-v.png` (256 px, the white V with the dark copy offset
  underneath, to stay legible over the light blob); `icones.py` paints the metal still at the menu
  button's phase, puts the V on top, cuts the corner and writes the `assets/vitral-*.png` (48 to
  256), `vitral.ico` (16 to 256) and the two SVGs, `vitral.svg` and `vitral-pequeno.svg`, each with
  the PNG embedded (the metal has grain and cannot be described in vector); `previa.py` is the
  metal's calculation in Python, which `icones.py` imports. All three need Pillow. Usage:
  `python3 -I ferramentas/logo/rasterizar.py ferramentas/logo/v-do-dono.png assets/vitral-v.png`
  and `python3 -I ferramentas/logo/icones.py ferramentas/logo assets/vitral-v.png assets
  [folha.png]` (the sheet with all sizes side by side, for checking).
- **Menu button.** At the top of the list, the logo square is liquid metal: the live mesh
  (`malha_viva`, in `ui/componentes/malha.rs`, the same recipe calculated point by point, one
  square per pixel of the button, with the exact grain) and the V on top (`assets/vitral-v.png`,
  the `LOGO_V` of `src/ui/trilho.rs`). With the pointer over it or the menu open the mesh moves at
  20 frames per second and the light follows the pointer; otherwise it follows the window's metal
  phase (still at rest).

## Painel da direita (Ctrl+J) (Right panel)

It starts closed, so the conversation keeps the full width. It opens and closes with Ctrl+J or with
the item in the **⋯** of the last panel (**Mostrar Este chat** (Show This chat) or **Mostrar o
arquivo aberto** (Show the open file), checked when it is open); when open beside it, its header
has its own close button (the collapse icon). What you chose is remembered (`painel` in
`interface.json`). It also opens by itself when you click a file cited in the conversation,
without changing that choice.

**Per-line comment in the diff** (`src/ui/comentarios_diff.rs`; idea from Paseo). In the agent's
edit, clicking the sign margin (+, − or the space) of a line opens the comment field below the
panel; Enter saves and Esc closes. Comments accumulate per chat, the commented line gets a dot in
the margin (clicking again edits, and empty or the trash icon deletes) and the discreet button
**Enviar comentários (N)** (Send comments (N)) sends them all in a single message
(`comentarios_diff::mensagem`: the file, the line and the snippet of each; a line that was removed
carries its text in place of the number). The message enters the turn or waits in the queue like
the input box's Enter (`enter_manda_depois`). Only in an edit: an open file without a diff has no
comment margin (the number still selects lines to mention).

**Paste HTML** (`src/ui/colar_html.rs`). GPUI on Linux only read `text/plain`; the
`vendor/gpui-pre-linux` (Wayland, `clipboard.rs`) also reads `text/html` and keeps it in the
`ClipboardItem`'s `metadata`. In the input box's `on_paste`, `markdown_da_colagem` converts links,
lists, bold, italic, code, headings, quotes, tables and images to Markdown, only when the HTML says
more than the plain text (the styled `<span>` from an editor pastes as usual). The paste is done in
two edits (the plain text and, on top, the Markdown): Ctrl+Z goes back to the plain text. Wayland
only; on X11 and on Windows pasting stays plain text. Not verified end to end on the bench (Mutter
only lets a focused client offer the clipboard, and `wl-copy` only offers one type at a time).

**Mentions** (`src/ui/mencao.rs`). In your bubble, the mention of a file or folder that exists
("@src/ui/fila.rs#L120-134", "@central/src/") becomes a pill in the middle of the text, in the
bubble's dark well: the type icon, the short name in mono and the lines in a faint tone, with the
full path in the tooltip. Clicking opens the file on the right, with the chosen lines (you can
mention it again); the folder opens in the file manager. It is a plugin of the kit's text
(`PilulaDeMencao`) that picks up the `vitral:mencao/<linhas>/<caminho>` links that `ligar_mencoes`
puts in place of the mentions.

**Cited files** (`src/ui/links_arquivo.rs`). In the assistant's reply, a file that is on disk
becomes a link: `produtos/views.py`, `src/motor.rs:120` in backticks, the markdown link to a path
(`[a janela](src/ui/mod.rs#L120)`) and, in running text, a word with a slash and an extension.
Clicking opens the file here, scrolled to the line, which is highlighted; an image opens in the
viewer. A relative path starts from the chat's folder. It only opens what is in a folder Vitral
knows (some chat's); the rest warns and is left to VS Code. Lines come from `arquivo:12`,
`arquivo:12:5` (the column is left out), `arquivo:12-20`, `arquivo#L10` and `arquivo#L10-L20`: with
two, the file opens with them highlighted, like the mention of a snippet
(`vitral:arquivo/10-20/<caminho>`). The `file:///caminho` of a file that exists also becomes a link
(the address's `%20` is undone). A path that does not exist as written is searched by its tail
among the files in the chat's folder (the "@" list, read in the background when needed):
`mensagens.rs:120` opens `src/ui/mensagens.rs` if only one file ends that way; with two (`mod.rs`),
it stays as text (`Ligador` and `pelo_fim`). **In a command's output** (the box in
`componentes/codigo.rs`, in Bash and in the result of the other tools), `src/x.rs:120:5` (cargo,
vitest, go), `src/x.ts(12,5)` (tsc) and `"x.py", line 12` (Python) open the file at the line, and
the pointer resting on the link shows the full path; without the line, only the path with a slash
and an extension (the `git status`). Only the last 40 lines of the output are checked (the ones the
box shows when closed), and the answer is kept for 5 s (`links_da_saida`, with a test). **The slash
command cited** in inline code (`/compact`), when it is in the chat's command list (the one in the
"/" menu), becomes a link (`vitral:comando/<chat>/<nome>`): clicking puts the command in the box
of the panel the conversation is in, with the draft that was there as the argument, and you are the
one who sends it. In the reply text the kit has no way to show a tooltip when the pointer rests on
a link: the tooltip with the full path only exists in command output.

**Este chat** (This chat) (`src/ui/este_chat.rs`) opens with the **Mudanças** (Changes) section:
the files the chat edited or created, split into **Neste turno** (In this turn) (since your last
message) and **Antes** (Before), each with the type icon, the name, the folder in a faint tone, the
+N −M and the **novo** (new) badge (the Write that created the file). Clicking opens the file here,
already in the edit (all the calls that changed the file, added together). The +N −M is counted
when the call arrives, with the full text, and is stored in the item (`Mudanca`); calls from before
that are counted from the stored detail, which is truncated. The list comes from the edit calls:
what a command changed is left out, and the note at the foot says so. Below, the card of where the
chat works. It says whether it works directly in the folder, whether it is a Sem pasta chat (the
files it creates stay in its own folder) or, in a conversation brought from Claude Code, where it
came from, the dates and how many messages it had; it shows the folder (the full path in the
tooltip, with copy) and **Abrir pasta** (Open folder). With the folder gone from disk (unmounted
disk, renamed folder), a red alert in place of Abrir. In the tab's tooltip, the folder and the panel
number. The other tab is the open file's (see **Arquivos citados**, above, and the Arquivos tab of
the list).

**Na pasta (In the folder) and the git branch** (`src/ui/git_da_pasta.rs`, `src/ui/ramo.rs`). After
Mudanças, the **Na pasta** section lists what git sees as changed since the last commit in the
chat's repository (what a command changed too), reread every 4 s while in view. Its header shows
the **branch** the folder is on: the name in mono, the short commit and "cabeça solta" (detached
head) when there is no branch, and the **worktree** badge when the folder is a copy of another
repository. The branch also goes in the tooltip of the chat's row in the list (together with the
folder) and in the tab's tooltip, on an extra line ("No ramo feat/x, num worktree"), read when the
tooltip appears.

- The branch comes from git's files (`git::ramo_no_disco`: the `.git` of the folder or of one above
  and its `HEAD`; in a worktree, `.git` is a file that points to the real folder, and the
  `commondir` inside says it is a copy). It does not spawn a process, so it works for the tooltip
  and for checking every few seconds. `retrato_da_pasta` uses the same reading and falls back to
  `git symbolic-ref` only in a repository the files do not describe (the reftable format).
- There is no notice when the folder switches branch (owner, 2026-11-18: no new notices).
- **Ignorar espaço em branco** (Ignore whitespace). With the "Na pasta" diff open, the file's **…**
  has **Ignorar espaço em branco** (checked when on): the diff is redone with git's `-w`
  (`git::diff_do_arquivo`), and what only changed indentation or trailing whitespace drops out. The
  strip says "sem espaço em branco" (a funnel, with the phrase in the tooltip, when it does not
  fit) and, if nothing is left, "Só mudou espaço em branco". It starts off and holds until Vitral
  is closed (`Janela::diff_sem_espaco`).

**Nesta conversa** (In this conversation) (`src/ui/sumario.rs`). Este chat opens with the
conversation's summary (with two or more messages from you): one row per message of yours, with
the time and the start of the text in up to two lines, and below it what that turn did ("3
editados · 12 comandos", the calculation of `falas::o_que_o_turno_fez`). In a conversation that
spans several days, the day enters as a small heading where it changes. The list is about 230 px
high and scrolls inside.

- **Clicking takes you to the message.** The panel starts drawing up to it (the panel's `mostrar`
  grows: it is the same as scrolling up to the top does little by little) and the opening at the
  anchor (`AlvoDeAbertura::Ancora`, in `mensagens.rs`) puts it at the top, below the place of the
  pinned message; it stays highlighted for 1.6 s, with the highlight's frame and tint.
- **The message in view is marked**: the last one of yours that starts up to 120 px below the top
  of the conversation (the one you are reading, or the one the click just brought). The
  conversation's scrolling only redraws the window when the marked one changes
  (`acompanhar_sumario`), and the summary list scrolls to its row.
- What came from the history of a conversation brought from Claude Code is not in the chat's items
  and stays out of the summary.

## Contas e uso dos planos (Accounts and plan usage)

In the top right corner of the window, just below the ×, a 30 ring floats for each service, one
above the other, each on a piece's opaque disc (`uso_no_canto`, in `src/ui/uso_plano.rs`): each one
only with that service's account connected (the Codex one, also with a usage reading). Each is the
brand's disc with **one** ring around it (with the gap at the bottom), which shows the tighter of
the two windows (5 h or week), in the service's color up to 69%, amber from 70 to 89% and red from
there up; at the limit, the ring is full and red. Both percentages are in the tooltip and in the
popover. With more than one account, the Claude one carries the initial of the account in use.
**Uso do plano no canto da janela** (Plan usage in the window corner), in Configurações >
Aparência (`Config.uso_no_canto`, on by default), hides the rings; the usage stays in **Contas do
Claude**. Without Codex installed, its ring does not appear: **Instalar o Codex…**, in the logo
menu, opens the terminal with the official installer and the login. Clicking a ring opens the
details popover, upward, right-aligned: the plan, the two windows, each with a 44 ring that fills on
opening, the percentage in the middle (in amber or red from 70%) and, beside it, the label and the
time it renews ("renova em 2 h 13 min · 15:59"; a window without a reading keeps only the label and
"sem dado", with no ring), the weekly limits per model, the extra credits for the month and, in the
footer, when the number was read, with **Atualizar** (Refresh) (and, discreetly, "não deu para
atualizar" below when the last lookup brought no number). Claude's usage updates by itself, directly
from Anthropic, with the same request Claude Code makes for `/usage` and each account's login: right
when the panel opens and every minute while it is open, every 5 min while it is closed (the corner
rings also show it), every 15 min with the window unfocused for more than 15 min (30 min after 2 h
away) and immediately with **Atualizar**. The token is read from the account's `.credentials.json`
on every lookup and only goes to `https://api.anthropic.com`; expired or refused, Vitral does not
renew it (Claude Code renews it) and waits, and lack of network and too many requests make it wait
longer before trying again, with no notice. What Claude Code and Codex keep on disk also counts
(reread every 30 s), as do the limit warnings during turns; Codex only through those two paths.

**Forecast of when the limit runs out** (`src/contas/previsao.rs`). The engine keeps in memory, per
account, the raw usage readings (`Motor::historico_uso`, fed in `juntar_uso_da_conta` and in
`Evento::Limite`), and the slope between the oldest of the recent stretch and the last says when the
window reaches 100%. In Claude's popover, below "renova em…", each window gets a line: "No ritmo de
agora, chega a 100% às 15:10" (At the current pace, reaches 100% at 15:10), in amber, when that
falls before the renewal, or "Dá até a renovação" (Lasts until the renewal). When the 5 h forecast
falls before the renewal, the corner ring turns amber even below 70% and its tooltip gives the
time. The stretch is 30 min for the 5 h (with at least 5 min between readings) and 6 h for the week
(at least 30 min): half an hour of heavy work stretched over days would always say the week
overflows, which is why the week also does not paint the ring. Without enough readings, with the
last reading older than the stretch, right after the window renews (the previous window's readings
do not count) or at the limit, the line does not appear. The readings do not go to disk: after
opening Vitral the forecast takes a few minutes to appear.

**Where the usage went** (`src/motor/gasto.rs` and `src/ui/onde_uso.rs`). Each finished turn
becomes a row in a ledger outside the state, `gastos/AAAA-MM.jsonl` in the data folder (one file
per month, written whole by `dados.rs`'s writer): the time, the chat, the folder, the model, the
account, the cost and the tokens of new input, cache read, cache created and output. The cost is
what `Chat::somar_custo` added up at that end of turn; the tokens come from Claude Code's `result`
(`Evento::Gasto`, from `tokens_do_resultado`: the `model_usage_since_last_result`, which counts
subagents, or, without it, the turn's `usage`; never `modelUsage`, which is the session total) and
from Codex (no cost): in the app-server, the `total` of the turn's last `thread/tokenUsage/updated`
minus the one from before its first call; in the old `exec`, the `turn.completed`. A turn with
several `result`s (background agents) sums the tokens of all of them. In the **Contas do Claude**
dialog, below the accounts, the **Onde o uso foi** section has the selectors **Hoje** (Today) and
**7 dias** (7 days) (today and the 6 previous days): one bar per folder, from the one that spent the
most to the one that spent the least, with "182 mil tokens · US$ 0,84"; clicking the folder opens
the five chats that spent the most in it, and clicking a chat closes the dialog and opens the
conversation. The bars measure cost; in a Codex-only period, tokens. On startup Vitral reads the
current month and the previous one (what the 7 days and the recent conversations need). Only what
ran after this version enters: what came before has no row in the ledger.

**Último turno** (Last turn), in the input box's context popover (`ultimo_turno`, in
`src/ui/onde_uso.rs`): after the context tokens, the three numbers of the conversation's last
recorded turn (new input, cache reads and output), one below the other. Without a recorded turn,
the line does not appear. Below them, the **Cache** line, the cache cooling down: "quente por mais
3 min" (warm for 3 more min) (the minutes round up; in the last minute, the seconds) or "frio"
(cold) (`linha_do_cache` and `estado_do_cache`, which give the whole sentence, "cache quente por
mais 3 min"; the word "cache" goes in the labels column because the sentence did not fit in the
values one). The calculation is the end of the last turn plus the prompt cache's validity minus now
(`gasto::cache_restante`), done when the popover opens: no clock and no color on the ring. The
validity comes from the `result`: `usage.cache_creation` separates what the turn wrote with 5 min
(`ephemeral_5m_input_tokens`) from what it wrote with 1 h (`ephemeral_1h_input_tokens`), and the
one that wrote the most wins (on the subscription measured on 2026-11-23, everything with 1 h). A
turn that wrote nothing uses the last validity the chat saw, or the last one from the same account;
with none, the line does not appear. It also does not appear while the chat is working (the cache
is in use), on Codex and on turns recorded before this. What the line does not know: if the next
turn goes through another account or another model, the cache is of no use, warm or not.

**Turn statistics** (`Tempos`, in `src/motor/gasto.rs`). The ledger row also keeps the timings,
taken from the same `result` (`Evento::Medida`, from `medida_do_resultado`; names checked against
the real output of Claude Code 2.1.328): `total_ms` is the `duration_ms`, `modelo_ms` the
`duration_api_ms`, `ttft_ms` the `ttft_ms`, `passos` the `num_turns` and `cache_ttl` the cache's
validity in seconds. `duration_ms`, `ttft_ms` and `num_turns` are per `result`; `duration_api_ms`
is the process total, like `total_cost_usd` (two `result`s in the same process gave 1518 and then
1943), so the ledger adds the difference from the previous `result` (`Livro::somar_medida`), or the
whole value when it drops (the turn changed process). In a turn with several `result`s, the total
and the steps add up and the first word is the first one's. `duration_ms` does not count the
process starting up nor the wait for background agents between two `result`s: so it can be smaller
than the row's "Trabalhou" (Worked), which goes by the time of the items. `espera_ms` is measured
here: from the instant `Processo::iniciar` is about to open the process until the first line of the
response (`claude::primeira_palavra`: the start of a block or a piece of it;
`Evento::PrimeiraPalavra`, once per process, which redraws nothing). The wait minus `ttft_ms` is
what starting Claude Code cost (`Tempos::subida_ms`): in a real turn, 2967 ms of wait for 1405 of
`ttft_ms`, 1.5 s of startup; in the second `result` of the same process, 9 ms. It is the number
that measures a future improvement of keeping the process open. Zero does not go to the file, and
rows from before this are read without the timings. Codex reports none of them. Known limit: a turn
that ends with the process exiting after a postponed end (the agents finished without another
`result`) does not go through `registrar_gasto`; its tokens are left for the next turn's row, as
before, and its timings are lost (`Livro::abrir`).

## Organização (Organization)

- **The top of the list** (the empty area drags the window, a double click maximizes it and a right
  click opens the window menu): the logo, on the metal square (click it or press F10 for the **menu
  do Vitral** (Vitral menu), which opens to the right, also in collapsed mode, 12 from the top
  margin: at the top, the metal plate with the logo, "Vitral" and, below, the version, the short
  commit (or the channel in the build made on the PC) and how many chats there are, "0.7.0 · a1b2c3d
  · 16 chats", with the origin and the data folder in the tooltip; with a new version ready, just
  below, the highlighted line **Versão nova pronta · Reiniciar** (New version ready · Restart) (the
  version in the tooltip); then three groups by topic, with a divider between them
  (`grupos_do_menu`): **Recolher a lista** (Collapse the list) (Ctrl+B; when collapsed, **Mostrar a
  lista** (Show the list) and **O que precisa de você** (What needs you), with the bell badge),
  **Novo chat** (New chat), **Novo chat em outra pasta…** (New chat in another folder) and
  **Importar do Claude Code…** (Import from Claude Code); **Quadro de ideias** (Idea board),
  **Resumo de hoje** (Today's summary) and **Arranjos ▸** (Layouts); **Contas do Claude** (Claude
  accounts), **Instalar o Codex…** (Install Codex) and **Instalar o Gemini…** (Install Gemini) when
  they are missing, **Configurações** (Settings), **Paleta de comandos** (Command palette) and
  **Atalhos do teclado** (Keyboard shortcuts); with each one's shortcut keys. It fits without
  scrolling at 150% in a window 880 tall, also collapsed and with the ready version. Until
  24/11/2026 it also had MCP Servers, Team memory, the working folder and Open the data folder: they
  now live in Settings (Claude Code, Memory, Chats and Updates) and in the palette. With a new
  version ready, an 8 px blue dot on the corner of the logo, with the version in the tooltip. The
  ready version is read again from the `atual` link when the menu opens and when you return to the
  window (`Atualizacoes::versao_pronta`): the "Reiniciar para a 0.10.0-dev .766bc7804b" (Restart to
  0.10.0-dev .766bc7804b) in the 24/11/2026 screenshot was the one stored when the app opened, with
  a newer one already installed by another launch, and Restart already went to the new one. Beside
  the logo, the tabs **Conversas | Arquivos** (Chats | Files) (see **O que fica à vista** (What
  stays in view)).
- **The foot of the list:** **Arquivadas** (Archived), the running agents and the bell for what
  needs you and, below them, the search and the "+" (the usage rings stay in the corner of the
  window; see **Contas e uso dos planos** (Accounts and plan usage)). The bell number is the same as
  "Esperando você" (Waiting for you) (requests, errors, stops that ask for Retomar (Resume) and the
  pairs of chats stopped at the cycle brake), red if something failed and amber in the other cases;
  with new replies only, a blue dot. When the number goes up (a new request, stop or error), the
  bell pulses once with an amber ring (red if something failed); opening the app, the number going
  down or marking as read do not pulse, and with animations off nothing pulses. The click opens
  **O que precisa de você**: each row with the chat bubble and, below, the same sentence and the same
  signal as the list preview (the dot in color and the label: "Permissão · Bash · npm test"
  (Permission), "Plano para aprovar" (Plan to approve), "Parada · Vitral fechou" (Stopped · Vitral
  closed), "Erro · …" (Error)): the requests (with Negar (Deny) and Permitir (Allow); a command with
  a risky snippet, or too long for the little box here, more than 2 lines or 160 characters, is not
  approved from the bell: the button is **Ver o comando perigoso** (See the dangerous command) or
  **Ver o comando inteiro** (See the whole command) and takes you to the conversation, where the card
  shows everything and the dangerous one asks you to hold the button; a plan opens the conversation
  to be read first), the stops (Retomar), the errors (Tentar de novo (Try again)) and the new
  replies (mark as read), with the panel number when the conversation is open; clicking a row opens
  the conversation (Ctrl+click, beside). In the footer, the buttons **Marcar tudo como lido** (Mark
  all as read) and **Ver na lista** (See in the list) (goes back to the list, without archived, and
  shows the section); header, sections, rows and the footer icon stay on the same guide, 16 from the
  edge. Ctrl+G goes to the pending item after the one of the open chat, found by chat (and not by
  position), and wraps around after the last; Ctrl+Shift+G goes backwards. On arrival, focus goes to
  the useful place: the **Retomar** of the strip in the stopped chat, the **Tentar de novo** (or the
  way out of the failure) in the error, the plan comment in the plan, the box in the other cases. The
  pill says where you are ("2 de 3 · Parada · WMS" (2 of 3 · Stopped · WMS)), or "Nada esperando
  você" (Nothing waiting for you). The **Paradas** (Stops) header of the bell gets **Retomar tudo**
  (Resume all) with two or more (usage round, item 3.4).
- **Ctrl+B collapsed** (or **Recolher a lista**, in the logo menu). The list disappears and the
  conversation goes to the edge. In its place stays the logo, at the start of the tabs of the 1st
  panel, with the bell badge (the number or the dot, which pulses like the bell); the usage rings
  stay in the corner of the window. The logo menu swaps Recolher for, at the top, **Mostrar a lista**
  and **O que precisa de você** (opens the same box as the bell). Ctrl+B again brings everything
  back.
- **Chat tabs.** Each panel keeps tabs in the header (`Painel.abas`, in `src/ui/paineis.rs`), in the
  order they were opened: clicking a chat in the list opens a tab in the active panel and the
  previous ones stay, up to 8 (`MAX_ABAS`; past that, the oldest one that is not in view goes out).
  Clicking a tab switches the panel's chat (`abrir_aba`); the × or the middle click closes the tab
  (`fechar_aba`; the conversation stays in the list), and the panel shows the neighbor from before
  (without it, the one after); closing the last one closes the panel. Ctrl+W closes the tab in view
  and, on the last one, the panel. On each draw, `acertar_abas` puts the chat in view among the tabs
  and removes those of chats that are gone. A chat appears in only one place at a time: the neighbor
  that is already in view in another panel does not enter. The tab in view is the full one (up to 280
  wide, and never less than the symbol, the number, the × and the start of the title); every tab has
  a single outline, the 1 px black line (the 1 px black shell of slack around it was removed, owner,
  16/11/2026: "diminui as bordas" (reduce the borders));
  the others are compact (up to 180), with the title and the path in the tooltip, and they shrink
  before it. A background tab that would have no room for 4 letters and the ellipsis keeps only the
  icon, in a 32 square (`Aba::quadrada`); if even so the tab in view lacks the start of the title,
  the background ones leave the strip, the oldest first, and a **+N** at the end of the strip opens
  the list of them (the history one, with only them; `jeito_das_abas`, in `src/ui/conversa.rs`, with
  the strip width measured in the previous frame and the natural width of each tab, counts the room
  for the +N). The strip never goes over the **+** and does not scroll: the mouse wheel over it does
  nothing. Every tab has a background, and with the pointer over it gets a metal ring. Tabs have no
  icon (owner, 18/11/2026: "tira os ícones daqui também" (remove the icons here too)): the chat state
  goes in the tab's black line (`Aba::borda`, in `src/ui/componentes/paineis.rs`): a 2 px line in the
  state color over the black, with a 6% veil of the same color in the middle, no halo. Working,
  blue, breathing slowly (from 0.5 to 1 in 2.4 s, in its own view: the beat redraws only the line);
  finished and you have not read it, green; waiting for you, amber; error, red; stopped and read, the
  usual black line. The owner asked for discreet, but the dimmed 1 px line could not be seen ("dá pra
  ver nada" (can't see anything), 18/11/2026). Only the background tab that became a square (no room
  for the title) keeps the state icon. At minimum width, only the tab in view. Tabs are not
  remembered when the app closes.
- **Saved panel layouts** (`src/ui/arranjo/salvos.rs`). What the window already saves on close
  (`Arranjo`: the chats of each panel, the tabs, the tab in view, the split of the conversations,
  the widths and the collapsed list) can be stored under a name, one per front of work ("Central",
  "Vitral"), in the `arranjos` key of interface.json (up to 12). In the logo menu, **Arranjos**: the
  list (the one in use with the check mark), **Salvar o arranjo atual…** (Save the current layout)
  (Ctrl+Alt+S; the same name saves over it) and an **Apagar** (Delete) for each. Switching first
  stores the layout in use, as the window is, so the way back is from where you stopped, closes the
  current panels (each box's draft is kept, and the tabs do not go to Ctrl+Shift+T) and puts back
  those of the chosen one through the same `reabrir_arranjo` as on startup; the window size does not
  change. **Ctrl+Alt+PageDown** and **Ctrl+Alt+PageUp** walk through the layouts, wrapping around,
  and the command palette has an "Arranjo: nome" (Layout: name) for each. An archived or deleted chat
  disappears from the layout without error (`Arranjo::reabrivel`); if none is left, the switch warns
  and does not happen. Chats that leave the screen keep running. After closing and opening, the
  window comes back as it was (the usual `arranjo` key) and the "in use" one stays the same.
- **Conversations side by side** (up to 4, depending on the width): drag a conversation from the
  list, or any tab of a panel, as in VS Code: dropped in the **middle** of another panel, it becomes
  one more tab of it ("Abrir aqui, como aba" (Open here, as a tab); it leaves the source panel, which
  closes if it was the only tab); dropped on the left or right **edge** of a panel, it opens beside.
  A tab that leaves a panel with other tabs, dropped on the edge of any panel (even its own), becomes
  a new panel on that side and leaves the others in the source one; the only tab of a panel, dropped
  on the edge of another, moves the panel ("Mover para cá" (Move here)). The logic is `efeito_de`, in
  `src/ui/paineis.rs`, with a test. The shape says what will happen: half a panel in blue for "Abrir
  ao lado" (Open beside), a bar on the edge for "Mover para cá" and the whole panel for the tab (with
  "Já há 4 lado a lado" (There are already 4 side by side) when another does not fit); the tab you
  drag is dimmed and dashed. It also works with Ctrl+click or the "open beside" button on the card.
  The panel that opens beside enters with a fade (the width does not animate). With several panels,
  each one has a number in the form of a key (on the tab, in the list and on the right), Alt+1…4 goes
  to it, and the selected one is marked by the **bottom light** (the metal dock at the foot of the
  card, see **Acabamento** (Finish)), which lights softly (180 ms; goes out in 120); clicking the tab
  (or any part of the panel) activates the panel and puts the cursor in its box. The chat state goes
  in the tab outline (blue breathing running, amber request or stop, red error, green news). The ×
  appears over the end of the tab, without reserving room, on hover and on focus (holding the tab
  already gives focus); the tab with only the symbol does not show it (a click on it would switch
  tabs while closing), and it closes with the middle click. Closing with the × or the middle click
  puts the cursor in the box of the panel that remains, like Ctrl+W. Text selection stays in the
  panel where it started: each one has its own scope (`Painel.escopo_selecao`), which the click
  activates (`conversa::aplicar_escopo_do_clique`, with the `definir_escopo_de_selecao` of the
  vendored gpui-component). The tab enters the Tab order: Enter goes to the box, Delete closes and Shift+F10 opens the menu.
  The dividers have no fixed line: the indicator appears when the mouse passes over, and you just
  drag to resize; **two clicks** on the divider between two conversations leave all of them with the
  same width (`igualar_conversas`, in `src/ui/mod.rs`). The list on the left collapses through the
  logo menu (**Recolher a lista**) or Ctrl+B, and when dragged goes from 200 to 560 px
  (`FAIXA_LISTA`). Ctrl+PgUp/PgDn go to the neighbor panel and Ctrl+Shift+PgUp/PgDn move the panel to
  another place. Keyboard and organization (24/11/2026), all in the `atalhos.rs` table ("?" screen
  and palette): `paineis_a_mostrar` and `alternar_maximizado` (`paineis.rs`) maximize the active panel
  with Ctrl+Shift+Enter, and the panel menu does too (the other panel does not close, it just leaves
  the drawing; `Janela.maximizado` follows the active panel, Esc and repeating the shortcut give it
  back); `destino_da_aba` and `divisao_da_aba` make Alt+PgUp/PgDn (tab to the neighbor panel, at the
  end a new panel) and Alt+Shift+PgUp/PgDn (new panel beside) the same as dropping the tab with the
  mouse, through `soltar`; Alt+↑/↓ (`pular_mensagem` in `sumario.rs`, uses the summary anchor) and
  PgUp/PgDn (`offset_da_pagina`, 90% of the screen; in the empty box, because with text PgUp belongs
  to the cursor); Ctrl+Alt+A archives the panel's chat through the menu path and Ctrl+Z, only with
  focus outside a field (`Vitral && !Input`), undoes it through `ArquivadasPendentes` (`lateral.rs`:
  a global with the token of each notice, which leaves on Desfazer (Undo), on the × or on the
  switch); "Marcar como não lido" (Mark as unread) (`Chat.retido_nao_lido`, memory only) holds the
  blue dot against the `ler_ate_agora` of each frame until `Motor::visitar`, called when opening the
  chat from the list or activating its panel. The Ctrl+F bar (`recuo_da_direita`,
  `busca_conversa.rs`) retreats by what `reserva_dos_aneis` dictates in the panel that touches the
  right of the window (`painel_na_borda_da_janela`).
- **Panel header.** One line only (44): the tabs and the **+** (owner,
  16/11/2026: "faz esse botão ser o de + de criar mais uma conversa e ter o histórico também, e
  tira o de 3 pontinhos e deixa só no botão direito" (make this button the + one that creates another
  conversation and also has the history, and remove the 3-dot one and leave it only on the right
  button)). The **+** (`botao_historico`, in `src/ui/conversa.rs`, tooltip "Novo chat ou histórico"
  (New chat or history)) opens a list with search ("Buscar chat…" (Search chat), ↑/↓ and Enter): the
  first row, already marked, is **Novo chat neste painel** (New chat in this panel) (Enter creates
  the new chat here, as a tab, in the same folder the list's "+" would choose); below, all chats,
  from the most recently touched to the oldest, each with the state icon, the time and the folder,
  and the archived ones in a group at the end. Typing filters the list (without accents and by loose
  words, like the list search: see below), and the new chat row stays only if it matches its text
  ("novo"). The chat in view says "à vista neste painel" (in view in this panel); choosing another
  shows it in this panel, as a tab (one already in view in another panel says "no painel 2" (in
  panel 2) and only activates that panel). The button is also there in the narrow panel. The **⋯** of
  the header is gone: the **right click** on the header (on the tab strip and on the empty space of
  it) opens the menu of the panel where the mouse is, and on a background tab the same menu, with the
  conversation actions applying to that chat; **Shift+F10** and the **Menu** key open the same menu
  of the active panel, anchored where the **⋯** used to be (`menu_pelo_teclado`). The menu has
  **Modo foco** (Focus mode), in the last panel the item on the right (**Mostrar Este chat** (Show
  This chat) or **Mostrar o arquivo aberto** (Show the open file), checked when it is open, with
  Ctrl+J), **Nova conversa ao lado** (New conversation beside) and the conversation menu (rename,
  pin, archive). On the tab, the chat bubble, with the folder in the tooltip ("Direto em
  ~/Projetos/loja" (Directly in), "Sem pasta: um chat numa pasta vazia só dele" (No folder: a chat in
  an empty folder of its own)). The cost ("US$ 0,42") is in the context ring of the box. There is no
  line between the header and the messages, even with the conversation scrolled. In a narrow panel
  the focus mode goes to the panel menu; in the narrowest, with several panels, the number takes the
  place of the tab symbol. No text is cut in the middle.
- **Panel without a conversation:** "Comece um chat" (Start a chat) with **Novo chat** (in the last
  folder used) and **Em outra pasta…** (In another folder); you can drop there a conversation dragged
  from the list.
- **Zoom:** Ctrl + mouse wheel (or touchpad), Ctrl+= / Ctrl+- and Ctrl+0 (back to 100%) enlarge and
  shrink the whole interface in steps of 10%, from 70% to 200%, as in a browser; the percentage shows
  at the top of the window for a moment. Everything grows together, including the widths of the
  parts: at 150% the normal list is 468 px, the right one 570 and each conversation needs at least
  420, so in a window that does not hold the right one and then the list, they become drawers, as in
  a narrow window. What you dragged on the dividers is saved at 100% and follows the zoom (a list of
  240 at 100% becomes 360 at 150%, and the split between conversations is kept). It applies to the
  whole app and is saved in `~/.config/vitral/interface.json`.
- **Look:** the background is the liquid metal plate (see **Acabamento**), opaque: the wallpaper no
  longer shows. The whole window is the base, with the plate under a dark blue veil, and the list
  lives directly on it; the conversation and the panels on the right are cards that float above, with
  shadow and the glass edge (a line on the sides, light on the top corner, shadow at the base and the
  shine of the top); your bubble and the cards on the right and of delivery, the line of light at the
  top. Dialogs and drawers are opaque; black enters what goes on top and inside, to give depth:
  popovers, menus, tooltips and notices are almost black and opaque (`flutuante`, with the thin light
  border and the line of light at the top); code blocks, command outputs and the frame of the message
  box stay in a deeper black (#08090c), with the same syntax color; and the usage rings sit on the
  opaque disc of the piece, so that the color of each brand pops. Inside a popover, what on the glass
  is a dark inset (the track of the effort segmented control) becomes a light one. The logo (the V on
  the metal square; see **Acabamento**) comes in full size (`assets/vitral.svg` and the PNGs from 64
  to 256 px) in the applications menu and on the recovery screen, and in the small one
  (`assets/vitral-pequeno.svg`, declared at 32 px, and the 48 PNG) in small icons. Popovers and menus
  enter with a fade and 6 px coming from the side of the button that opened them, and all floating
  elements have a 1 px line of light at the top, like the reflection of a glass; the list and right
  drawers enter sliding 24 px from the edge, in 180 ms, with the veil fading in together, and close
  abruptly. The dialogs' veil covers only the window, not the shadow margin. The pill of the
  segmented control (Conversas · Arquivos, the effort) slides to the new option; in the neutral
  segmented control it is always the metal token, also in Settings and in the welcome screen (the
  tinted pill is kept for the segmented control of another color); the round buttons (send, +, bell)
  light up softly with the pointer over them; the usage and context rings slide to the new value; the
  bell pulses when something that needs you arrives; and the "running" dot breathes while the beat of
  the spinners and of the shimmering text (the one in the status line) ticks. Lists, menus and cards
  keep the abrupt hover. Amber belongs only to what needs you. With the pointer outside the window,
  nothing is in hover or has a tooltip open.
- **Narrow window:** when width is lacking, the right panel and then the list leave the side and open
  on top, as a drawer (Ctrl+J, Ctrl+B, Ctrl+K and the usual buttons); Esc or a click outside closes
  them. The drawer enters sliding from its edge, with the background darkening together; it closes
  abruptly. The window buttons follow the GNOME layout (on Fedora, only the ×).
- **List**, directly on the base of the window (no card of its own): the running list of **O que fica
  à vista** (what waits for you, what runs, the pinned and all the others, one line per chat). The
  search (Ctrl+K; finds in the title, in the text of the messages and in the folder name, also in
  archived conversations) and the **+** stay at the foot of the list. An amber dot on Conversas warns,
  from inside Arquivos, that something is waiting for you. Everything in the list starts 16 px from
  the edge of the piece (the bubble, the search magnifier and the first footer icon). The search says
  only "Buscar" (Search); what it finds and Ctrl+K stay in the field tooltip (which does not appear
  while typing in it). In the search, when the term is not in the title, the line below shows the
  folder and the snippet that matched ("loja · Você: valide o **frete** grátis…" (store · You:
  validate the free **shipping**…), with the term in strong), and the archived ones that match
  appear at the end, in a section **Arquivadas** (with the number) and with the faint card. Writing
  to an archived chat (or the **Entregar agora** (Deliver now) of its queue) takes it out of
  Arquivadas, and the message goes in at once; before, it waited in the queue until it was unarchived
  by hand (owner, 19/11/2026). Outside the search, they stay only in Arquivadas, which has the date
  band titles (**Recentes** (Recent), up to 7 days, and **Mais antigas** (Older), collapsed at the
  start). As the list narrows (or with high zoom, because the math is the same), it gives up what
  is least important before cutting the title: below about 290 px (at 100%), on hover only the **…**
  remains; below 275, the tabs keep only the icon and the preview keeps the short label of the
  reason; below 260, the file search says only "Buscar" and **Retomar tudo** keeps only the icon. The
  times stay in view. The scrollbar is thin, flush with the edge of the piece, does not cover the
  cards and appears and disappears without a fade, 1.2 s after scrolling.
- **Esperando você** (the number is the same as the bell's: requests, errors and stops; new replies
  do not count, they become a dot) starts with the pairs of chats stopped at the cycle brake, each
  with **Ver** (See); requests, stops and errors come before new replies, and with two stops or more
  **Retomar tudo** appears. The cards are neutral like the others (selected in blue); amber stays on
  the bubble dot and on the preview one. The **Arquivar** (Archive) in the chat menu shows
  "Conversa arquivada" (Conversation archived) with **Desfazer** (a chat that never ran leaves
  Vitral: "Chat vazio apagado" (Empty chat deleted)); unarchiving does not notify.
- **Conversation card**: one line of 32 (the 20 bubble with the state dot in the corner, the title,
  at 400, the time, the pin of the pinned one, the unread dot and the panel number; the folder in the
  bubble tooltip; unread goes to 600, two steps up); the line below, with the preview, comes back
  only in a search that matched in the text. The time comes out as a faint metadata with equal digits
  ("3 min", "5 d", with a thin non-breaking space), light on the selected line. A chat open in a panel
  shows only the panel number, always in the same column, and the time goes to the bubble tooltip,
  together with the folder: with three panels the list is narrow and the title needs the room. The
  number is a key in an outlined inset; the one of the active panel has the border and the numeral in
  blue (the full blue disc is only for notices, on the bell and on the logo). The dot: red error,
  amber request or stop, blue new reply, blue ring running. On hover, the time gives way to two
  actions: **Abrir ao lado** (or Retomar) and **…**; outside hover, the title uses their place, and
  only the card under the pointer (or highlighted by the keyboard) cuts the title to show them. The
  right click or the **…** open the chat menu: open beside, rename, pin and archive. A simple click
  opens the chat in a tab of the active panel; a double click renames; Ctrl+click or the middle click
  open beside.
- **Keyboard in the list**: ↓ in the search enters the list; ↑/↓, Home and End walk through the
  cards; Enter opens (and, with a request, puts the cursor in the box), Ctrl+Enter opens beside and
  puts the cursor in the box of the new panel, F2 renames and Esc goes back to the search. A letter
  typed with the list in focus (after clicking a chat) goes to the box of the active panel, which
  gets the cursor. The same holds with focus on the conversation (after opening a trail, selecting to
  copy or clicking on the "Hoje" (Today)) and on a tab: the letter goes into the active panel's box,
  and Ctrl+V pastes into it (a copied screenshot becomes a pill without clicking first). Space and
  Enter remain with the focused button, and "?" opens the shortcuts. In the search, with text, the
  first result is already marked, even with the cursor in the field; ↑/↓ move the mark without taking
  the cursor away (refining the term is still typing), Enter opens the marked one (Ctrl+Enter,
  beside) and puts the cursor in its box, and F2 renames it. With the field empty, ↓ leads to the
  list. Esc clears and, when empty, goes back to the panel's box. Ctrl+K selects the previous term,
  so the new search replaces it. With the list collapsed (Ctrl+B), Ctrl+K opens it as a drawer on top
  of the panels, with the cursor in the search: opening a chat collapses it by itself, and Esc closes
  it (the term stays). It does not come back pinned nor squeeze the panels, and Ctrl+B stays the same
  (usage round, item 4.1). In the "+" history, what waits for you (request, stop, error) comes in a
  "Esperando você" group at the top. If the list leaves view with focus on it (a new panel pushed it,
  Ctrl+B, the window narrowed), focus goes to the active panel's box.
- **Search without accents and by words** (`src/ui/acentos.rs`, usage round, item 4.3). In the list
  and in the "+" history, the search ignores accents and case on both sides and splits the term into
  words: it accepts the chat in which each word appears in the title, in the folder or in a message,
  in any order ("integracao" finds "integração"; "vip cupons" finds "Cupons para grupos VIP"). First
  come those that match in the title, then those that match only in the text, and inside each group
  the order is by date. The highlight marks each word in the original text. The prepared text of each
  chat is kept and only redone when the items change. The piece that removes the accent is the same
  as the Ctrl+F of the conversation.
- **Footer**, only when there is something to show: **Arquivadas** (the icon, with the number in the
  tooltip; turns the archived view on and off; the search finds both) and, with something running,
  how many agents are running in the whole Vitral ("2 rodando" (2 running); the click goes back to
  the top of the list). In the Arquivos tab, how many files the folder has. A line on top and 4 of
  breathing room separate it from the last line of the list.
- **Arquivos** tab (the @ and the **…** of each file only take room on the row's hover, like on the
  chat card, and the name goes to the end outside it): the files of the open chat's folder in a tree
  (folders collapsed, except the path to the open file), with its path on top and the ↻ to reread.
  With a search, a flat list with no limit, with the snippet found highlighted and the folder cut in
  the middle. Clicking opens in the right panel: the tab with the name and the type icon, and a bar
  with the folder, the number of lines and the language, **Mencionar** (Mention), **VS Code** and the
  **…** (copy the content or the path, open outside, show in the folder). The **file header**
  (`cabecalho_do_arquivo`) has the icon and the name, the lines and the language ("59 linhas ·
  JavaScript" (59 lines)), the folder in a faint tone cut at the start and, on the right, the
  actions. Nothing goes under anything: the computation (`o_que_cabe`) is by the panel width and by
  the size of the texts, and what does not fit goes out whole (first the action labels, which become
  just an icon with a tooltip, then the lines and the language); the name is the last to shrink. With
  the usage rings on, the top rows of the panel (the header and the edit strip) reserve their place
  at the right end (`reserva_dos_aneis`); the content does not. **Mencionar** puts the file mention
  in the box, as a pill; with lines chosen, theirs ("@x.rs#L12-18"); in the edit, that of the change
  in view (the **…** has **Mencionar o arquivo inteiro** (Mention the whole file)). A click on the
  line number chooses the line (the second undoes it) and Shift+click extends the choice; the chosen
  ones stay in blue, and the header says which ones ("linhas 12 a 18" (lines 12 to 18)), with the ×
  that undoes it. Opened from the edit (see **Ver a edição** (See the edit), in Ferramentas (Tools)),
  the file gets the edit strip below the header: "Edição" (Edit), the +N −M and "2 de 5" (2 of 5),
  the arrows that go from one change to another and the × that goes back to the file without the
  marks. **Reviewing the turn in sequence:** opened from the Mudanças (Changes) of a chat (or from an
  edit in the conversation) with more than one file in the list, the strip also says "arquivo 2 de 8"
  (file 2 of 8) ("2/8" when the sentence does not fit in the strip, as in the panel at minimum width
  with the usage rings in the corner; the sentence goes in the tooltip), and the arrow, on the last
  change, moves to the next file of the list it is in ("Neste turno" (In this turn) or "Antes"
  (Before)), in its order; the up one, on the first change, goes back to the previous file, to its
  last change. At the ends of the list, it wraps around. The arrow tooltip says whether the next step
  is another change or another file (`passo_da_revisao` and `edicao::lugar_na_revisao`, with a test).
  Outside a list (the diff in "Na pasta" (In the folder)) or with a single file, the arrows wrap
  around within the file, as before; with a single change and no other file, the strip has no arrows.
  The lines that came in get the green background, the green strip on the border and the "+"; those
  that went out appear among them, with no number, in red, with the "−" (markdown comes out in lines
  in this mode). Each piece of the edit is found in the current file by its new side (the context and
  what came in; in the Codex diff, near the line it says); what can no longer be found (the file
  changed afterwards) appears loose at the end, with a note. The panel opens scrolled to the first
  change. The old record, without the diff, opens the file as it is, with the notice "Este registro
  não guardou a edição" (This record did not keep the edit) in the strip.
  **Edit in the panel** (`src/ui/editor_arquivo.rs`). A click on the file text opens the editor over
  the reading view, with the cursor on the line and column of the click and the same scroll; the line
  number still chooses lines to mention. It is the kit's code editor (`EditorState`, with syntax
  color, undo and the indentation the file uses: `recuo_do_texto`), without line wrapping, like the
  reading view. The text **saves itself 800 ms after the last key** (`Agenda`) and **Ctrl+S** saves
  at once (context `Arquivo`). In the header, the pencil takes the place of the file symbol,
  "editando" (editing) that of the lines and the language, an amber dot marks what has not yet gone
  to disk (red if saving failed, with the error in the tooltip) and the × (or **Esc**) goes back to
  reading, saving what is missing. Opening another file, closing the tab, the .md Preview and closing
  Vitral also save first. Ctrl+F remains that of the panel bar: it saves, goes back to reading and
  searches (the kit's editor has no search or replace here). In the agent's edit (the "Edição"
  strip), a click on a file line leaves it and opens the editor there. Only UTF-8 text up to 400 KB
  enters the editor (`Sessao::abrir`): a file in another encoding warns and stays read-only
  (saving from here would swap its letters), tabs and the Windows line ending go back to disk as they
  came, and the **empty file** has the **Escrever nele** (Write in it) button. The editor text stays
  where the reading text was (the panel compensates the margins of the kit's field, which do not
  grow with the zoom); the line numbers stay 10 px closer to the text. The gutter of the numbers is
  painted over the text that scrolls sideways, and Vitral's theme has it transparent: the editor
  receives the opaque color of the surface it is on (`gutter_background`, a change marked in the kit
  under `vendor/`).
  **Changed on disk.** With the editor open, the file is checked every second (the date and the size;
  the content only when they change: `Sessao::conferir`). If the agent writes the file and you have
  nothing left to save, the editor starts showing the new text, with the cursor and the scroll where
  they were. With text of yours left to save, the amber strip **"O arquivo mudou no disco."** (The
  file changed on disk.) appears, and nothing is saved until you choose: **Recarregar** (Reload)
  keeps the disk one (yours is lost) and **Manter o meu** (Keep mine) keeps yours, which overwrites
  on the next save (the usual 800 ms, or Ctrl+S). Saving also checks the disk before writing, in case
  the agent wrote between two checks. A **deleted file** while you edit keeps the text in the editor,
  with the strip "O arquivo foi apagado. O seu texto continua aqui." (The file was deleted. Your text
  is still here.) and the Manter o meu, which creates the file again. The conflict goes away by
  itself if the disk becomes equal to what you wrote. In the narrow panel the strip buttons drop to
  the line below. Leaving the editor with an unanswered conflict (or with saving failing) saves
  nothing: what you wrote goes to the clipboard, and the notice "Texto não salvo" (Text not saved)
  says so.
  **Large file.** Above 400 KB the file opens **read-only**, up to 5 MB (`TETO_DE_LEITURA`), with the
  discreet strip "Só leitura: arquivo grande (612 KB)" (Read-only: large file) below the header: in
  lines, without syntax color (coloring a few MB would take tens of seconds), without the markdown
  preview and with lines of more than 2 thousand letters cut at drawing (minified JSON). Line
  choice and Ctrl+F continue (the search counts lines with ASCII only without folding the text:
  `achados_na_linha`). Measured on the bench, in the debug binary, with a 5 MB log of 99.8 thousand
  lines: a 0.5 s frame on opening, scrolling at no cost and up to 0.34 s per search keystroke; the
  release binary is several times faster. Above 5 MB the notice remains.
  **Prévia | Fonte** (Preview | Source). The .md and the .svg have in the header the **Prévia |
  Fonte** selector (the eye and the braces when the labels do not fit): Prévia is the rendered
  markdown or the drawn SVG, and Fonte is the text in lines, which a click edits. Another file opens
  in Prévia. In the .md preview, the YAML **frontmatter** at the top becomes a small key and value
  table (the name and description of a SKILL.md; `previa_markdown::frontmatter` reads the first-level
  keys, folded and literal text, the inner list and map, and what is not a list of keys stays as
  text), and **links to files** open the file in the panel, on the line when the link carries one
  (`[o carrinho](../loja/carrinho.py#L18)`): the relative path starts from the .md's own folder and,
  if it does not exist there, from the chat's folder (`previa_markdown::ligar_links`, which uses the
  same linker as the conversation). The preview is built once per read of the file. A web link opens
  in the browser; the rest (`file://`, a slash command) does not open.
  The markdown comes out rendered; code, with color and line number, scrolls sideways. The **SVG**
  opens drawn, like the other images (GPUI's `img` draws it; `claude::imagem_de_ver`), and the click
  takes it to the viewer, at the size the `<svg>` tag says (`imagens::medidas_svg`: the `width` and
  `height` or the `viewBox`; GPUI draws the SVG at its source size, and stretched to the area it
  blurred) and over a mid gray instead of black (`imagens::fundo_da_imagem`: the dark stroke without
  background of an icon vanished into the black); in the edit it comes out in lines, and attached it
  follows as a text file (the `tipo_imagem` remains without it: the API does not accept SVG as an
  image). The header's **VS Code** opens at the line in view (`code --goto arquivo:linha`,
  `sistema::argumentos_do_editor`): the first of the chosen ones, that of the change in view in the
  edit or the one the conversation link marked; the button tooltip says which. A file that cannot be
  shown has a notice in its place, with the way out that fits (`saidas_do_aviso`): the **binary**
  (PDF, spreadsheet, zip; the notice says the type by the extension) has **Abrir fora** (Open
  outside), which opens in the system's default application (`sistema::abrir_fora`); the **too
  large** (over 5 MB) has **Abrir no VS Code** and **Abrir fora**; the one that vanished, **Fechar**
  (Close). A large file with a null byte at the start is binary, not "too large" (before, a 2 MB PDF
  only offered VS Code). The header's **…** also has **Abrir fora**, for any file (the .html in the
  browser, the .csv in the spreadsheet).
  **Search in the file (Ctrl+F).** A click on the file panel takes focus to it (`foco_arquivo`,
  context `Arquivo` in the keymap), and then Ctrl+F searches inside the file, not in the
  conversation; with the file in the drawer, over the conversations, Ctrl+F is already its own. It is
  also in the header's **…** (**Buscar no arquivo** (Search in the file)) and in the palette. The bar
  floats at the top of the content, like the conversation's: the field, "3 de 12" ("3/12" in the
  narrow panel; "nenhum achado" (no match) in amber), the arrows and the ×. Enter and F3 go to the
  next match, Shift with them to the previous, wrapping around, and Esc closes (with the bar open,
  before collapsing the drawer or stopping the turn). The search ignores accents and case
  (`acentos.rs`), marks all matches in amber (the current one stronger) over the syntax colors and
  scrolls to the current one, in the middle of the screen. The bar reserves the place of the usage
  rings, and the first lines move down so as not to be behind it. It works by lines: with the bar
  open, markdown and SVG come out in lines, and appear rendered again on closing;
  in the edit, it finds in the file lines (not in those that went out). The index
  (`IndiceDoArquivo`: the line and position of each match) is only rebuilt when the term or the file
  changes. Typing with focus on the panel still writes in the box; opening another file closes the
  bar. It does not scroll sideways to a match on a very long line. In the list, on hover, **@**
  mentions the file in the conversation and **…** (or the right click) opens in VS Code, copies the
  path or shows it in the folder. The list is reread every 20 s in the background, without freezing
  the screen.
  **Mostrar ignorados** (Show ignored). Beside the ↻, the eye (closed by default; open and
  highlighted when on) puts in the tree what git leaves out, in a faint tone: this is how `arquivo/`,
  `dados/` and `tarefas/` (screenshots, exports) appear, which the workspace keeps out of git. An
  ignored folder enters with its name only, and what is inside is read one level at a time, when it
  is opened in the tree ("Lendo…" (Reading…) until it arrives; "Pasta vazia" (Empty folder) if there
  is nothing): `node_modules` and `target` are never scanned without asking. The list comes from
  `seletor_arquivos::listar_com_ignorados` (`git ls-files --others --ignored --exclude-standard
  --directory`, descending into inner repositories like the normal list; without git, what the scan
  skips: hidden and dependency folders), with the same cap of 20 thousand entries counting
  everything together. Open folders stay in `ArquivosLidos::ignoradas_abertas`, and the 20 s reread
  and the ↻ reread them too. The tab's search finds the ignored items that are already in the tree;
  the "@", Ctrl+P and the links keep only what git tracks. It lasts until Vitral closes.
- **Configurações** (Ctrl+,), saving at once, apply to all chats (the path of `config.json` is in the
  subtitle tooltip). Since 23/11/2026 they are a wide dialog (`LARGURA_DAS_CONFIGURACOES`) with a
  column on the left of the parts (`Parte`: Chats, Agentes (Agents), Revisão (Review), Notificações
  (Notifications), Memória (Memory), Aparência (Appearance), Atalhos (Shortcuts), Claude Code, Codex,
  Gemini and Atualizações (Updates); the Claude accounts live in Claude Code; in the narrow dialog,
  only the icons) and, on the right, the options of the chosen part, which scroll by themselves at a
  height that never exceeds the window's (`altura_do_corpo`: 520 at 100%, or the window minus the
  header slack; the navigation and the header stay still and only the column scrolls). What lives in
  each part (`src/ui/configuracoes.rs`):
  - **Chats:** **Pasta dos chats novos** (Folder for new chats) (the working folder, with
    **Trocar…** (Change), which opens the system picker; `trocar_pasta_de_trabalho`), **Avisar dentro
    do app** (Notify inside the app), **Notificação do sistema** (System notification), with
    **Testar** (Test), which sends a notification at once (without `notify-send`, the line says what
    is missing), **Arquivar chats parados depois de** (Archive idle chats after) (never, 3, 7 or 30
    days; only chats already read, not pinned and without a queue), **Sugerir a próxima mensagem**
    (Suggest the next message) and **Título do chat pelo Claude Code** (Chat title by Claude Code)
    (on; off, the title stays that of the first words and not even the title request goes out,
    `opcoes::titulo_pelo_claude_vale`). Three more groups, all saving at once in `Config`: **Abas**
    (Tabs) (**Chat chamado por outro abre numa aba** (Chat called by another opens in a tab), on, and
    **Fechar a aba para o chat** (Closing the tab stops the chat), on), **Caixa de mensagem** (Message
    box) (**Seta ↑ na caixa vazia** (Up arrow in the empty box): Corrigir a última (Fix the last), as
    it was, or Percorrer as enviadas (Walk through the sent ones); **Enter com o turno rodando**
    (Enter with the turn running): Entra no turno (Joins the turn), as it was, or Vai para a fila
    (Goes to the queue)) and **Arquivos** (Files) (**Lembrar o que a aba Arquivos mostra**
    (Remember what the Arquivos tab shows), off). The default of each option is the one from before
    it existed, except the two for tabs.
  - **Agentes:** the **Modo de permissão padrão** (Default permission mode), **Quem responde nos
    chats novos** (Who answers in new chats), the **Modelo dos chats do Claude** (Model of the Claude
    chats) and the **Revisão automática pelo outro agente** (Automatic review by the other agent)
    (on; see **A revisão do turno pelo outro agente** (The turn review by the other agent)); the
    **Instruções para todos os chats** (Instructions for all chats) (a text of yours, up to 4,000
    characters, that goes at the end of the system prompt of each chat, Claude's and Codex's, after
    what Vitral sends; it applies from the next message; `prompts::com_instrucoes`); and **Versão
    nova** (New version) (**Consultar a versão nova dos agentes** (Check for new agent versions):
    Sozinho (Automatic), as it was, or Só pelo botão (Button only), which brings **Consultar agora**
    (Check now)).
  - **Memória:** **Memória da equipe** (Team memory) and **Rever a memória ao terminar** (Review the
    memory when done).
  - **Aparência:** **Zoom da interface** (the same as Ctrl+wheel), **Letra da conversa**
    (Conversation font) (from 80% to 150% of the rest: only the messages grow, with their text, code
    and cards; the list, the box and the menus follow the zoom; **Letra da conversa também na
    caixa** (Conversation font in the box too), off, puts the message box in the same font),
    **Mostrar o uso do plano no canto da janela** (Show plan usage in the window corner), on by
    default, with a switch per ring (**Anel do Claude, do Codex e do Gemini**, all on; each one still
    depends on the connected account, `opcoes::aneis_escolhidos`), **Números nas abas e na lista**
    (Numbers on tabs and in the list) (Sempre (Always), as it was, or Só segurando Ctrl (Only while
    holding Ctrl): the badge appears after 150 ms with Ctrl pressed and goes away on release, keeping
    its place so the tab does not change width; `Janela.ctrl_desde`, which resets on each draw with
    Ctrl actually released: the `Ctrl+,` that opens Settings released Ctrl with focus in the dialog,
    and the number stayed in view), **Animações** (Animations) (Seguir o GNOME (Follow GNOME) or
    Desligadas (Off); it applies to the whole app and changes at once; following a GNOME without
    animations, the line says so) and **Transparência dos fundos** (Background transparency).
  - **Claude Code:** the installed version, the one the turns open, and **Atualizar** (Update), which
    runs `claude update` outside any turn, waits for the chats that are working or have a message in
    the queue and shows what it answered, in amber if it fails (the new version applies on the next
    turn, without restarting Vitral), **Servidores MCP do Claude Code** (Claude Code MCP servers),
    those of your Claude Code in the chats, and the **Lista dos servidores MCP** (MCP server list),
    which opens the list of each server and its state.
  - **Codex** and **Gemini** (`src/ui/configuracoes/agentes_fora.rs`, the same mold as the Claude Code
    card; what runs a process lives in `src/cli_agentes.rs` and `src/motor/cli_fora.rs`): the
    **Versão instalada** (Installed version) (`codex --version`, `agy --version`) with **Atualizar**,
    which runs `codex update` or `agy update` outside any turn, in the background, with a spinner,
    waits for the chats of that agent (as agent or as partner of the pair) that are working or have a
    message in the queue and shows what the command answered, in amber if it fails (`agy update`
    answers a one-line JSON, and Vitral shows only the `message`); without the program, only that
    line, with **Instalar…** (the official installer and the login in the terminal,
    `uso_plano::instalar_*`). With it: the **Conta** (Account) (Codex: the translated `codex login
    status`, "Entrou com a conta do ChatGPT" (Signed in with the ChatGPT account), "Entrou com uma
    chave de API" (Signed in with an API key) or "Não entrou" (Not signed in), never showing the key;
    Gemini: "Conta Google conectada" (Google account connected) when `agy -p /usage` returns the
    quota, which is the only login signal `agy` gives) with **Entrar…** (Sign in) (the terminal with
    `codex login` or `agy acp --login`, and the engine checks from time to time, for a few minutes,
    whether the account signed in) and, in Codex, **Sair** (Sign out) (`codex logout`, with
    confirmation first, because it drops the Codex chats until signing in again); the **Lista dos
    servidores MCP**, read-only (`codex mcp list --json`, `agy mcp list`; the `agy` format with
    servers has not been seen, the reading is tolerant); and the **Configuração do Codex** (Codex
    configuration) (`$CODEX_HOME/config.toml` or `~/.codex/config.toml`) or **do Gemini** (Gemini's)
    (`~/.gemini/antigravity-cli/settings.json`), with **Abrir** (Open) in the system application, only
    when the file exists. The new Codex version comes from the npm query (below); Gemini has no query,
    because `agy` does not publish a version that can be read without installing.
    When the Claude Code or Codex package announces a version newer than the installed one, the line
    says so ("Há a 2.1.330. O Atualizar instala." (2.1.330 is available. Atualizar installs it.));
    up to date, it says nothing. The query is the package tags of each one on npm (`stable` for those
    who chose that channel in Claude Code), with none of your data, made only when the Agentes part,
    the Claude Code one or the Codex one opens (or by the button, with **Só pelo botão**) and cached
    for an hour (`src/versoes_agentes.rs`).
  - **Atualizações:** the Vitral updates, the **Pasta de dados** (Data folder) (with **Abrir**) and
    the **Diagnóstico para pedir ajuda** (Diagnostics for asking for help): **Copiar** (Copy) puts on
    the clipboard a text with the versions (Vitral, Claude Code, Codex, Gemini), the system, the data
    folders, the options turned on and the last 40 lines of `queda.log`. Anything that looks like a
    key or token is masked, the home folder becomes `~`, your instructions enter only by size, the
    team memory address only as "ligada" (on), and no conversation enters (`src/diagnostico.rs`).
  The organization is that of the 0.9.6 parts. The branch that drew Settings in six sections (Chats,
  Agentes, Perfis (Profiles), Aparência, Avisos (Notices) and Vitral, with the list in
  `Secao::grupos`) brought the new options, which went inside the parts; its sections did not stay.
- **Chat called by another one in a tab, and closing the tab stops the chat** (Configurações > Chats
  > Abas). When `mensagem_para_chat` delivers the message (not when the cycle brake keeps it as a
  note) or `abrir_chat` opens a chat, the engine puts the id in `Motor.chats_trazidos`; the Janela
  consumes the queue in the same observer as `pedido_de_abrir` and calls `trazer_aba`, which follows
  `onde_entra_a_aba`: a chat already in view or already a tab of some panel does nothing, with no
  panel it opens like any chat, and in the other cases it enters as a background tab in the active
  panel, without changing what is in view and without `activate_window` (the tab outline shows the
  state). The tab's × and Ctrl+W (`fechar_aba_do_usuario`) stop the turn of the chat that is actually
  running (not the one waiting for the user's answer) through `Motor::parar`, unless it remains in
  view in another panel or tab (`ao_fechar_a_aba`); if stopping is costly (`parar_custa_caro`:
  background agents, Ultracode), Vitral asks first. Dropping the tab in another panel and archiving
  do not go through here.
- **Up arrow walks through the sent ones.** With the option (`Config.seta_acima_percorre`), the arrow
  in the empty box brings the text of the messages the user sent in this chat
  (`recordar::enviadas_do_chat`, from newest to oldest, without immediate repeats), one at a time;
  while the box has exactly the brought text, its context gets "Recordando" (Recalling) and ↑/↓ keep
  walking (↓ goes back to the empty box). Editing the text ends the walk (`Percurso::ativo`).
  Corrigir stays on the bubble's pencil and on Ctrl+↑.
- **Agent profiles.** Removed in 0.11.0 (without the part in Settings and without the group in the
  model selector). Those that existed stay in `config.json` (`Config.perfis`, `src/perfis.rs`).
  **Atualizações** also applies to the whole app: the mode (**Automáticas** (Automatic), the default,
  **Só avisar** (Notify only) or **Desligadas**), the **Versão** ("0.3.0 · procurada hoje às 14:02"
  (checked today at 14:02), "Procurando…" (Checking…), "Baixando 0.3.1 · 12 de 21 MB" (Downloading
  0.3.1 · 12 of 21 MB), "é a mais nova" (is the newest), "A 0.3.1 está pronta; vale quando reiniciar"
  (0.3.1 is ready; it applies when you restart)) with the button of the moment (**Procurar agora**
  (Check now), **Instalar** (Install), **Novidades** (What's new) and **Reiniciar**), the error in
  one line (amber without connection, with the time of the next attempt; red when the signature or
  the file do not check out) and, on the installed version, **Voltar para a 0.2.0…** (Go back to
  0.2.0), which asks first (Enter only closes), refuses the current version and restarts on the
  previous one; if the previous one does not read the conversation format (like 0.2.0 after the
  migration), the line is dimmed and says so. In the build made on the PC: "Compilado no PC · sem
  atualização automática" (Built on the PC · no automatic update), with **Instalar a 0.3.1 como app**
  (Install 0.3.1 as an app). The menus show the icon and color of the mode, or the Claude mark and a
  line about each model; deadlines are side-by-side choices; the (i) explains the rest. With the
  dialog narrow (high zoom or small window), the controls drop below the text. Each row has a
  one-sentence description; the detail lives in the (i).
- **Dialogs** opaque, with 20 corners, the large title and the × with a tooltip; the footer is always
  in view and only the middle scrolls. **Mencionar arquivo** (Mention file) and **Conversa como
  contexto** (Conversation as context) are lists with search and keyboard (the files are read in the
  background, with the type icon and the folder below; the conversations, from newest to oldest, with
  when they spoke, how many messages they have and the folder). **Renomear** (Rename) already opens
  with the title selected and does not save empty.
- System notification, only with the window in the background, when a chat finishes, when a chat asks
  for permission or when a chat crashes: with the Vitral icon, the chat title and a single line
  ("Precisa da sua permissão: Bash, $ cargo test" (Needs your permission), "Terminou: …" (Finished),
  "Parou com erro: ..." (Stopped with an error)). The "Terminou" carries the start of what the agent
  said, without the markdown marks (heading, list, bold, backticks, link, code fence) and with up to
  220 letters, cut at a word (`previa_da_resposta`, in `src/motor.rs`; before, the raw first line
  went, and the notice came out as "Terminou: ## Resumo"). Clicking it opens the conversation in the
  active panel and brings the window to the front (or GNOME warns that Vitral is ready). A new
  notification from the same chat replaces the previous one instead of stacking. Permission is
  answered inside the app, never in the notification. The permission request and the crash with an
  error go out with high urgency: in GNOME they stay on screen until clicked (on Windows they go out
  like the others); the "Terminou" disappears by itself, as always.
- **Notice inside the app.** With the window in front, a chat out of view (in no panel, or in a
  background tab) that needs you gets a notice in the bottom right corner, above the box of the last
  panel: the permission request with **Negar** and **Permitir** (a dangerous or long command becomes
  **Ver o comando…**, the bell's rule), the plan with **Ver plano** (See plan) and the failure with
  **Abrir** (Open) and **Tentar de novo**. The click on the title opens the chat. The notice stays
  until answered and disappears by itself when answered elsewhere; the × dismisses it. It never takes
  focus, and the box's Enter never lands on a button of it. Three or more at once become a single
  notice, "3 chats precisam de você · Ver" (3 chats need you · See), which opens the bell. A turn
  started by another chat never warns, and the "Terminou" stays in the bell. Turn it off in
  Configurações, **Avisar dentro do app**.

Shortcuts: `Ctrl+N` new chat (at once, in the open chat's folder), `Ctrl+K` search (`↓` enters the
list; in it, `Enter` opens, `Ctrl+Enter` opens beside and `F2` renames), `Ctrl+J` right panel,
`Ctrl+B` collapses or shows the list, `Alt+1…4` panel N, `F10` Vitral menu, `Ctrl+,`
settings, `Ctrl+G` next pending item (`Ctrl+Shift+G` the previous), `Ctrl+W` closes the tab in view (on the last one, the panel),
`Ctrl+PgUp/PgDn` neighbor panel, `Ctrl+Shift+PgUp/PgDn` moves the panel to another place (on the tab: `Enter` goes
to the box, `Delete` closes, `Shift+F10` opens the menu), `Ctrl+End` goes to the end of the conversation, `Ctrl+F`
search in the conversation (`F3`/`Shift+F3` walk through), `Shift+Tab` switches the mode, `Esc` stops the active panel's turn,
also with focus outside the box, after a click on the conversation (or closes the drawer), `Enter` sends, `Shift+Enter` breaks the line, `Ctrl+wheel`, `Ctrl+=` and `Ctrl+-` zoom,
`Ctrl+0` normal zoom. `Ctrl+Shift+M` (model) and `Ctrl+E` (effort) open and close the popovers of the
active panel's box (in the effort one, ←/→, Home and End change the level, Space turns on Ultrathink, U
turns on Ultracode and Enter closes (the keys are in the popover footer); Esc closes without stopping the
conversation). `?` opens the **Atalhos do teclado**. One floating element stays open at a time: opening the menu of the
logo, the bell, a box popover, a panel's menu or **+**, by mouse or by shortcut,
closes the one that was open (`Janela::so_um_flutuante`).

## Resumo do dia (Daily summary)

**Resumo de hoje** (Today's summary), in the logo menu, opens a dialog with what the chats did during the day, by folder
(`src/ui/resumo_do_dia.rs`). It is built only from what the app already has, without calling any model: the items
of the conversations (each with its time) and the state of each chat. A selector switches to **Ontem** (Yesterday) and **7
dias** (7 days), and **Copiar como Markdown** (Copy as Markdown) puts the text on the clipboard (a title, one section
per folder and, in each chat, the sentence, the time, the pending item and the files).

- **Who is included**: the chat in which the agent replied or used a tool during the period (archived
  ones too). Your message alone, with the agent having done nothing, does not count. The folder touched
  last comes first and, within each one, the chat touched last.
- **For each chat**: the title (clicking it opens the chat), the first sentence of the last reply of the
  period (`falas::primeira_frase`), the files that the edit calls of the period changed, with
  the summed +N −M and the "novo" (new) flag (the same source as **Mudanças** (Changes); what a command changed is left out), the
  time worked with the number of turns, and what is still pending: the state of the chat right now
  (`falas::estado_do_chat`: running, stopped, error, permission or plan waiting for you). A reply
  you have not read yet is not a pending item.
- **The time worked** is the sum of the intervals between one item and the next within each turn,
  only the part that falls inside the period. An interval of more than 10 minutes with nothing happening counts only
  up to that cap: it is waiting (a permission request left standing, Vitral closed), not work. A
  command that runs by itself for more than 10 minutes also counts only up to the cap.
- The periods follow local midnight: today, from the start of the day until now; yesterday, the whole day; 7
  days, from the start of the day six days ago until now. The summary is built when it opens and when the
  period is switched, not on every frame.

## Quadro de ideias (Ctrl+I) (Ideas board)

The list of what you want to do in the future, as a kanban (`src/ideias.rs`, `src/motor/ideias.rs` and
`src/ui/quadro_ideias.rs`). Only you add notes: no chat puts an idea on the board by itself (owner,
11/18/2026: "these ideas are meant to be future things that I keep adding so I don't forget"). It opens
with Ctrl+I, from **Quadro de ideias** (Ideas board) in the logo menu, and from **Ideias deste chat** (Ideas of this chat) in the menu of a
conversation (right click on the list or on the header, or Shift+F10), which says how many it has.

The dialog takes up almost the whole window, with a 16 margin. At the top, a single line: "Ideias", the chip of the chat
when the board belongs to one (with the × that shows all of them), the state of Linear (with the ⟳ and the plug) and the
close button. Below, the four columns, **Ideias** (Ideas), **Próximas** (Next), **Fazendo** (Doing) and **Feitas** (Done): the name
with the number of cards and, at the bottom of the list (the metal, the blue veil and the hole, like the conversation
bubbles), the cards, which scroll per column. The field at the top of Ideias adds a note with Enter; the idea
goes to the top of the column, in the folder of the chat in view (with no chat, in the working folder) or, on the board
of a chat, already belonging to it.

- **Card:** like a row of the list, a light veil that lights up under the pointer. The title (13, up to 3
  lines), the reason or the start of the request (2 lines) and a line of facts, cut at the edge without
  ellipsis: the idea's chat with its state (or the folder), the issue code and the assignee.
  With the pointer over it, the send button.
- **An idea of a chat:** the **Chat** choice in the editor links the idea to a conversation (the 40 touched
  last) or leaves it for **Um chat novo** (A new chat). **Mandar no chat** (Send to chat) puts the request in the queue of its
  conversation, without cutting the turn that is running, and opens the conversation; with no chat, **Abrir chat** (Open chat) creates
  one in the card's folder, with the request as the first message, and links the card to it. In both
  cases the card goes to Fazendo. The right click on a card with a chat has both.
- **Dragging:** dropped on a column, the card goes to the end of it; dropped on top of another, it goes in
  just before it (the order of a column is the `ordem` of each card, ascending).
- **Click:** opens the editor, with the back arrow: title, **Pedido para o chat** (Request for the chat) (empty, the
  title is sent), **Chat**, **Pasta** (Folder) (only for the new chat), **Coluna** (Column), **Apagar** (Delete) (with Undo in the
  notice), the Linear code, **Ir ao chat** (Go to chat) and **Mandar no chat** or **Abrir chat**. The text
  is saved by itself (the engine marks the board and saves on the 5 s pass, and on close).
- **Right click:** send (to the chat or to a new one), edit, open in Linear, move to each of
  the other columns and delete.

It lives in `ideias.json`, in the data folder (`{"ideias": [...]}`; fields this version does not
know are written back as they came). If it is unreadable, the file is copied alongside and the board opens empty,
with a notice.

**With Linear** (`src/linear.rs`; owner, 11/18/2026: the board is Linear's, in the EFI team,
Eficiência Operacional). Turned on by **Ligar o Linear** (Connect Linear) in the header (the personal key, from
Settings > Security & access > Personal API keys, tested against the team before being saved, in
`linear.chave` in the configuration, 0600), the ideas are the team's issues with the **Ideia** label
(open ones and ones completed in the last 30 days, without the canceled ones): the team's tasks are left out.
The column is the one for the state type (triage and backlog in Ideias, "unstarted" in Próximas, "started" in
Fazendo, "completed" in Feitas), in the order of `sortOrder`. The chat, the folder and the reason remain only
Vitral's, kept in the card.

- **What goes up:** the new idea (it becomes an issue in the backlog state, with the Ideia label, which the
  board creates in the team if it does not exist yet), the title and the request (1.5 s after typing
  stops), the column (the first state of its type in the team's workflow) and the order (the `sortOrder`
  between the neighbors), the send (goes to "started") and the Apagar (archives the issue; Desfazer (Undo)
  unarchives it). The card changes at once; the call goes out in the background.
- **What comes down:** when the board opens, every minute while it is open, and on the ⟳. While Linear
  does not answer, the header shows the spinning ring and "Lendo o Linear" (Reading Linear), and the empty column shows the ring
  in place of its text (owner, 11/18/2026: "make a loading"). A card with a change
  on its way up stays as it is until the next read; if Linear refuses a change, the
  header shows the error in red and the board rereads.
- **The Vitral-only ones:** the cards from before connecting (and the ones from an error on creation) stay on the board, without
  the issue code, and do not go up (owner, 11/18/2026: "there's no need to send it to Linear").
- **The filter:** in the header, **Todas** (All), **Linear** and **Vitral**, each with how many there are (owner,
  11/18/2026: "select the Linear ones and the Vitral ones as a filter"). With **Vitral**, the new idea
  also stays only in Vitral (the field says "Nova ideia só no Vitral" (New idea only in Vitral)). The board opens on the last filter
  chosen, until the app closes. With Linear not connected, the filter does not appear.
- **Disconnect** (the plug icon) erases the key; the cards of the issues leave the board.

## Compilar e rodar (Build and run)

For day-to-day use, install from the release, without compiling: `curl -fsSL
https://github.com/VictorMenegate/vitral/releases/latest/download/instalar.sh | sh` (afterwards it
updates itself; see **Versões e releases** (Versions and releases)).

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

`rust-toolchain.toml` pins Rust to 1.99.1, the same as GitHub Actions. With rustup, the first
build in the folder installs that toolchain (minimal profile) if it is not there yet; as long as the PC's
`stable` is the same 1.99.1, `RUSTUP_TOOLCHAIN=stable` avoids the download.

`vitral --instalar` puts the full logo at the scalable size and from 64 to 256 px, and the small one
(`vitral-pequeno.svg`, made for small sizes) at 16, 24 and 32 px and in the 48 PNG, which
notifications and alt+tab use. The icons and the `.desktop` file go inside the binary (`include_bytes!`).

`DADOS_GLOBAIS` (in `src/dados.rs`) is the switch for the global data, on since 0.3.0. During a large change to the data,
when it is off, Vitral only opens in the test bench (`DENTRO_DA_BANCADA=1`, which isolates the XDG directories),
`vitral --instalar` and `ferramentas/instalar-dev.sh` refuse, and a stable release does not pass the
tests: a build from the middle of the change never writes to the real data. No test touches
`~/.local/share` or `~/.config` (`Dados::padrao()` panics in the tests; they use `Dados::em`).
To check the migration with real data without touching it, copy to a fake folder only the
`.vitral/` of the old folders and the `recentes.json` (pointing to the copies) and run
`VITRAL_COPIA_DA_MIGRACAO=<casa> target/debug/deps/vitral-<hash> --ignored --nocapture --exact
migracao::testes::migrar_uma_copia` with `HOME` and the `XDG_*` variables in the fake folder (the tests binary
comes from `cargo test --no-run`): it migrates to `<casa>/.local/share/vitral`, opens it the way the
engine would open it and writes out what became what; with `VITRAL_COPIA_PROC_REAL=1`, it checks that the
old Vitral, if open, defers without writing anything.

Bench: the command is Vitral with an optional folder, which it imports (on first use, an old
Vitral folder) or opens (afterwards, its chat or a new one in it). `FUNDO=branco` (or `FUNDO=<imagem>`)
changes the wallpaper (with the metal opaque, it only shows outside the window), `ZOOM=1.5` opens at
that zoom and `TELA=2400x1300` changes the size of the fake monitor (the default is 1600x1000; at 150%,
three conversations side by side only fit on a wider screen). `DADOS_NOVOS=1` opens as first use
(it moves the bench's data, configuration and `interface.json` aside to `$B/afastados/<hora>`; the `app/`
that the bench's update installed stays) and `RAIZ_SINTETICA=1` is the end-to-end migration: it generates
with `demo_vitral.py` an old Vitral folder inside the bench (with the `cwd` values inside it and
new sessions, never a copy of a real folder), puts it in the bench's `recentes.json`, opens
as first use and checks at the end that the hashes of its `.vitral` did not change
(`hashes-antes.txt` and `hashes-depois.txt` in the prints folder, and "raiz sintética intacta" (synthetic root intact) on
stderr). `CLAUDE_CONFIG_DIR=<pasta de mentira>` gives the bench a fake Claude Code to
import from; without it, inside the bench, nothing is imported (the real `~/.claude` never enters
a bench, not even through the extra accounts). `CLAUDE_FALSO=1` puts
`ferramentas/claude-falso/claude` at the front of the PATH, which does not call the API: it writes the message to `notas-do-chat.md`
in the chat folder, before the tools; it takes 10 s with
`[demora]`; with `[escreve 20]` the reply arrives in pieces during those seconds, about 40 per
second, like the text from the real Claude Code (this is how the conversation with text arriving is measured); `CLAUDE_FALSO_SOBE=<segundos>` makes it wait that long before reading the input, like the
real Claude Code while it starts up (this is how to check that the window does not freeze when sending a
screenshot); with `[ferramenta <Nome> <detalhe>]` (Read, Edit, Write, Bash, Grep, Glob, WebFetch,
Task…) it pretends to use the tool, sending the `tool_use` (Edit and Write with a fake snippet,
so the tool line has something to show when opened), waiting 4 s (or the seconds in
`[ferramenta … | 8]`) and sending the `tool_result`, so the turn line can be seen ("Lendo
motor.rs…", "Rodando: cargo test…") without spending the API; `[ferramenta Pensa <texto> | 3]` stages a
reasoning block that lasts the requested seconds, and `Task` (or `Agent`, which goes to the background)
stages a subagent like the real one (2.1.328): the `task_started`, the request and each call
with its result tied to its call (`parent_tool_use_id` and `agent_id`), the text and the reasoning
only in the file (in the stream, only with `--forward-subagent-text`) and, with a fake `CLAUDE_CONFIG_DIR`,
its file written bit by bit in `projects/<pasta>/<sessão>/subagents/` with the
`.meta.json` (it is what the subagent's tab reads); with "(pede)" in the description, its command asks
Vitral for permission (the subagent that "waits for you"), and with "(entrega)" the report goes through the
`SubagentHandback` call, as in automatic mode; with "(segue)" in the description of an `Agent`, it
keeps going after the `result` like the real `run_in_background` (the `background_tasks_changed`
list, its calls tied to the call that opened it, the file growing
with the lines of the API request between the calls, the `task_notification` at the end and a new turn opened
by the notice, with the `result` held until the last agent), and with "(falha)" the end notice comes
as `failed` (both marks are removed from the description); with `[juntas]` and only `Task` calls, the
subagents work at the same time; `[ferramenta Fundo <comando> | 20]` is a background command
that no longer holds the turn (several run together, and `stop_task` stops only its own), and the id of each
call carries a mark of the process, so it does not repeat from one turn to the next; `[ferramenta Diz
<texto>]` is a text in the middle of the turn (with it and a call, the finished turn collapses into the line
"Trabalhou…" (Worked…)); with `[juntas]` ordinary calls go out in a single reply and each result arrives
when its call ends, like real parallel calls; `[ferramenta Estourou <comando>
| 12]` is the Bash that ran past its time and went to the background (`backgroundTaskId` and
`timedOutAfterMs` in the result); the `result` carries `duration_ms`, `duration_api_ms`,
`ttft_ms`, `num_turns` and `usage.cache_creation` like the real one (1 h cache, or 5 min
with `[cache 5m]`), and `[ttft 1.8]` waits that many seconds before the first word (with
`CLAUDE_FALSO_SOBE`, the turn hint shows the "para abrir o Claude Code" (to open Claude Code)); `[ferramenta Edit
<arquivo> (palavra)]` swaps one word in a 10-line snippet (a one-line diff), and with
"(pede)" the edit asks for permission first; `[ferramenta Falha <comando>]` is the Bash that exited with an
error (`is_error` and "Exit code 101"), for the checks of the "Trabalhou…" line; `[ferramenta
TodoWrite fap]` sends the list with one step per letter (f done, a in progress, p pending), and
`TaskCreate <assunto>`, `TaskUpdate <id> <estado>` and `TaskList x` are the Claude Code task tools, with the text and the `tool_use_result` measured on 2.1.328 and the list kept
per session; with `[tabela]` the reply carries a Markdown table; `[ferramenta Print
<LxA> <nome>]` stages the screenshot of a browser MCP (the call
`mcp__chrome-devtools__take_screenshot` and a `tool_result` with a text block and a PNG generated
on the spot in base64; `Prints` returns two images in the same result) and `[ferramenta Read
<arquivo>]` of an image that exists returns only its image block, like the real Read (the
script is `ferramentas/roteiros/imagens-de-ferramenta.txt`, which also checks with `grep` that
the base64 did not go into the conversation or the state); with `[hook <Nome>
<detalhe> | <motivo>]` it stages a call that a `PreToolUse` hook sent to ask, like Claude
Code 2.1.328: the `tool_use`, the `hook_response` with the reason (only with `--include-hook-events`) and the
request to Vitral's `permissao`, without the reason (without `--permission-prompt-tool` in the arguments, the
call comes back denied with the reason as an error, without asking anyone; for the reason to show on the
card, the bench needs `CLAUDE_FALSO_VERSAO=2.1.328`, because Vitral only asks for hook events
from 2.1.323 on and the default of the fake claude is 2.1.300, the same version it reports in the
`init`; the script is `ferramentas/roteiros/ask-de-hook.txt`); through the input, in the middle of the turn,
it answers like 2.1.328 to `set_permission_mode`, `set_model` and `apply_flag_settings` and
ends the reply with "no meio do turno: modo acceptEdits, …" (mid-turn: acceptEdits mode, …), so the bench can see that the switch
arrived; a message that arrives while a tool is running stays in its queue until the tool returns, 
`cancel_async_message` removes it from there and `interrupt` answers with `still_queued` and, if a
message is left over, opens another turn with it; a slow tool sends `tool_progress` with
`heartbeat` every 30 s (`CLAUDE_FALSO_BATIDA=<segundos>` shortens it); and the text of `Pensa` only comes
with `--thinking-display summarized` in the arguments, which Vitral only passes from 2.1.323 on
(with the default 2.1.300 the block arrives empty, as in the real one without the option; the scripts are
`ferramentas/roteiros/responder-pedido.txt` and `ferramentas/roteiros/troca-no-turno.txt`); with
`[pergunta <Cabeçalho> | <pergunta> | <opção>: <descrição>; <opção>]` (and `[pergunta+ …]` for the
one that accepts several) it stages AskUserQuestion like 2.1.328, all the questions of the message in a
single call, and the turn's reply ends with "respostas: …" (answers: …); with `[plano <texto>]` (a written `\n`
becomes a line break) it stages the plan, with the ExitPlanMode call empty, the plan in the permission
request and, once approved, in the result; with `[compacta <auto|manual> <tokens antes> | <segundos>]`
it stages a compaction in the middle of the turn, and the message "/compact" does the manual one and ends without text,
like the real one (`CLAUDE_FALSO_COMPACTA=<segundos>`; the script for the three is
`ferramentas/roteiros/pergunta-plano-compactacao.txt`); with `[responde <arquivo>]` the turn's
reply is the content of that file in the chat folder (markdown, quoted files and commands) and with
`[saida <arquivo>]` the result of the tools staged in the message is the content of the file (the
output of a build with an error); to `generate_session_title` it answers with the title from `[titulo <nome>]`
in the message (or the one from `CLAUDE_FALSO_TITULO`, or "Sobre" and the first three words) and, with `persist` and a fake
`CLAUDE_CONFIG_DIR`, writes the `ai-title` line in the session, which is all that file
has (the script for the three is `ferramentas/roteiros/titulo-aviso-links.txt`); alongside it lives
`ferramentas/claude-falso/codex`, the fake Codex (it speaks `exec --json` and the `app-server` JSON-RPC, with `turn/steer`, and understands the same
`[ferramenta …]`, plus `Ver <arquivo>`, which is `view_image` (the `imageView` item), and `Print`, an MCP tool that returns an image in base64; with the `untrusted` policy (Manual and Aceitar edições (Accept edits)), each Bash, Edit and Write asks for approval like 0.169.0 and waits for the answer, and with `on-request` only the command with "curl" asks; `thread/compact/start`, `review/start` and `skills/list` (the fake skill "revisar-pr") answer like the app-server, and usage comes in a `thread/tokenUsage/updated` after each item; `[passos A; B; C]` stages the plan (`turn/plan/updated`), one step per tool; conversations are kept in `$XDG_STATE_HOME/codex-falso-conversas.json`, the reply says which earlier messages it remembers and `thread/fork` cuts at `beforeTurnId` (a turn that does not exist gives `turn not found`); `[falha]` ends the turn with an error, `codex debug models` gives a small catalog and
the arguments are kept in `$XDG_STATE_HOME/codex-falso.log`; `codex --version` reports `CODEX_FALSO_VERSAO`, 0.169.0 by default; `codex login status` (`CODEX_FALSO_LOGIN`: chatgpt, chave or fora; the fake `logout` writes "fora"), `codex update` (`CODEX_FALSO_UPDATE_SEGUNDOS`, `CODEX_FALSO_UPDATE_FALHA=1`; it installs 0.171.0) and `codex mcp list [--json]` (`CODEX_FALSO_MCP=1` adds two servers); in `agy`, `update` (`AGY_FALSO_UPDATE_*`, with the one-line JSON) and `mcp list` (`AGY_FALSO_MCP=1`); `SO_CLAUDE_FALSO=1` on the bench takes both out of the PATH to see the **Instalar** (Install) button) and `ferramentas/claude-falso/agy`, the fake
Antigravity CLI (Gemini) (it speaks the ACP of `agy acp`; `[demora]`, `[permissao]` asks to
run `ls -la` and says what Vitral answered, `[ferramenta Edit]` stages an edit, `[falha]`
ends with an error, a `session/prompt` with the turn running joins it, `agy models` gives the list, and the
requests are kept in `$XDG_STATE_HOME/agy-falso.log`); it executes the `[mcp <nome> <json>]` in the message
(outside the lines quoted with "> ") as real calls to Vitral's MCP; on `--fork-session`, it copies the original session to the new one beside it (only in that `CLAUDE_CONFIG_DIR`); and
writes the arguments, the environment and the calls to `$XDG_STATE_HOME/claude-falso.log`. In the script,
`roda <comando>` (output in `roda.log`, no comment on the line; the XDG directories are the bench's) and `reabre
[comando]` (kills Vitral with SIGKILL, runs the command with it closed and opens it again).
`ferramentas/conferir-snapshot.sh` is the manual check of the per-file prompt (it calls Claude: two
short turns). Two benches at the same time need different `BANCADA=N` values and different folders
(`XDG_CACHE_HOME=~/.cache/vitral-trabalho/bancada-x BANCADA=5`; the bench lives in
`$XDG_CACHE_HOME/vitral-bancada`); with the number or the folder already in use, it refuses to start. `python3 -I
ferramentas/demo_vitral.py <pasta> [--16]` creates a demo old Vitral folder (4
subfolders, 11 conversations, 8 requests in every state, routines; with `--16`, 12 more subfolders), without
running any process, which the bench migrates on first use; generate it again and run with `DADOS_NOVOS=1`
before each round. `python3 -I ferramentas/medir_print.py
<print.png> X Y` (or `--pontos <arquivo>`) measures the contrast of a text in the screenshot. The scripts for the visual
language live in `ferramentas/roteiros/` (`4c-antes.txt` takes all the screens;
`4c-zoom-larguras.txt` goes from 100% to 150% with the list dragged, the Claude and bell popovers,
the narrow window and the drawers, and `4c-zoom-lista-estreita.txt` narrows the list). To see all
menus, popovers and dialogs at once: `ferramentas/galeria-de-menus.sh <pasta> [binário]` runs the
script `galeria-menus.txt` (zoom 1.6; the logo menu, the bell, usage, the chat menu, the chips of the
box, Nova conversa (New conversation), the parts of Configurações (Settings), Atalhos (Shortcuts), Paleta (Palette), Contas (Accounts), Memória da equipe (Team memory),
Quadro de ideias, Novo chat (New chat) and Importar (Import)), saves a full screenshot of each one and assembles the `folha.png` with
all of them as thumbnails, with the name; it serves design reviews (owner, 11/23/2026: "make something to
be able to see all of them and analyze what can be improved"). The clicks are fixed positions: if the
layout changed, check the sheet before trusting it. In the middle of a
script, `tecla Control_L+equal` changes the zoom, and `rolar X Y N` turns the wheel N notches with the pointer
at X, Y (negative N scrolls up; it serves to see the end of a dialog that scrolls). The bench uses the
Wayland socket that the Shell announces in the log (a stale lock from a round that crashed leads it to `wayland-1`) and
puts `XDG_BIN_HOME` in its own folder. Updating is off; with `VITRAL_SEM_ATUALIZACAO=0`,
`VITRAL_ATUALIZACAO_BASE` and `VITRAL_ATUALIZACAO_CHAVE`, the development build looks in a
local fake release and installs in the bench's data folder: `ferramentas/release_falso.py gerar`
makes the fake 0.9.0 (20 MB, signed with a bench-only key through `openssl`) and `servir` serves
the delivery with the manifest answering 503, tampered with and correct, in that order, and the package at 1 MB/s; the
script `ferramentas/roteiros/dist-atualizacao.txt` (with the command at the top) photographs the showcase and the
whole path in the app, from "ainda não procurou" (has not checked yet) to the dot on the logo. The showcase opens on the section
**Atualizações** (Updates): the group in Configurações in each state (stopped with the previous one, checking, up to
date, downloading, ready, notify only, offline, signature refused, built on the PC and new format
in the folder) and 200 buttons for the notices, **Novidades** (What's new) and the **Reiniciar** (Restart) question with 2 conversations
(the "mesmo assim" (anyway) leaves a notice, to check that Enter does not trigger it).

`VITRAL_AGENTES_PUBLICADOS="claude=2.1.400,codex=0.200.0"` answers in place of the lookup of the published
versions of the agents (on the bench, with updating off, nothing is looked up): this is how to see the
"há a X" (X is available) in the Claude Code and Codex rows. The script
`ferramentas/roteiros/config-partes.txt` (with the command at the top) goes through the seven parts of
Configurações.

`VITRAL_QUADROS=1` writes "quadros/s: N" to stderr every second in which the window redrew
(nothing at rest): it is the check on the cost of the effects. When the metal rebuilt the field in that
second, the line carries the average time of one field: "quadros/s: 18 · campo: 2,1 ms". Then come the
per-frame averages of the shell assembly, of the time from the start of drawing to the end of layout and to
the end of painting (what is left of the window thread's time is GPUI closing the scene) and, if
any conversation was rebuilt, how many, how many blocks each one assembled and how long it took: "montagem:
média 2.4 ms, pior 4.1 ms · até o layout: 12.0 ms · até a pintura: 19.0 ms · conversas: 12 ·
blocos: 14 de 28 · 9.5 ms cada". With the conversations cached, the number of conversations per frame
says who is redoing what: 1 per frame is just the touched panel; 3 per frame with three panels
is the notified window.
`VITRAL_SEM_ANIMACAO=1` opens as if the GNOME animations were turned off (the metal stays
still in the initial phase, the same on every machine: this is how captures are taken). The showcase (`--vitrine`) gained the sections **Movimento**
(Motion) (one button for each effect) and **Espaçamento** (Spacing) (the scale and the guides of the list, the right side and the
conversation).

It needs `claude` (Claude Code) in the PATH or in `~/.local/bin`, `git`, and on Fedora
`libxkbcommon-devel libxkbcommon-x11-devel wayland-devel vulkan-loader-devel fontconfig-devel
freetype-devel libxcb-devel` to compile. Of the Rust dependencies, `chrono` provides the dates in the local
time zone; it was already in `Cargo.lock` through the vendor.

**On Windows** (10 1809 or newer, x64): it needs **Git for Windows** (Claude Code runs the
commands in its Git Bash) and **native Claude Code** (`irm https://claude.ai/install.ps1 | iex`, the
`claude.exe` in `%USERPROFILE%\.local\bin`); whatever is missing shows up as a warning in the window, and so does a chat folder
inside OneDrive. The data lives in `%LOCALAPPDATA%\vitral` (the cache in `cache\` inside
it) and the configuration in `%APPDATA%\vitral`. The fonts go inside the binary on both systems.
At the transparent levels, the blur is the system's acrylic (see **Transparência dos fundos e o
desfoque** (Background transparency and blur), in **Acabamento** (Finish); on Windows 10 it can be heavy when dragging); the
dragging, the double click, Snap and the minimize, □ and × buttons are Windows' own. Notices go out as
Windows notifications (AppUserModelID `VictorMenegate.Vitral`), Parar (Stop) sends the `interrupt` through the
Claude Code input (on both systems) and the real Stop is the Job Object, which kills the
whole tree. Not yet: extra accounts and "Instalar o Codex" (Install Codex).

To install on Windows, use the release's `vitral-<versão>-windows-x86_64-instalador.exe`: per-user only,
without administrator (with no signing certificate, SmartScreen warns the first time: Mais
informações › Executar assim mesmo (More info › Run anyway)). It puts Vitral in `%LOCALAPPDATA%\Programs\Vitral` and the shortcut
in the Start Menu, with the app's AppUserModelID (the taskbar pin and the notifications go to it). The
`Vitral.exe` at the root is the **launcher** (`lancador/`, std only): it reads `versoes.txt` (`atual=` and
`anterior=`) and opens the `app-<versão>\vitral.exe` of the current one, or the previous one, or the highest `app-*`
that has one, with the same arguments and `VITRAL_LANCADOR` in the environment, and exits; with none,
it explains in a message box. Each version stays in its own folder because an open `.exe` cannot be
overwritten. Uninstalling (in Installed apps) removes only the program and whatever the updater leaves
in its folder; the data and the configuration stay. The `.zip` is the portable version: open `vitral.exe`
directly (the launcher that comes with it only works inside the installation); it only warns about new versions,
with **Baixar** (Download), which opens the release page.

Once installed, Vitral updates itself on Windows as on Linux (see **Versões e
releases**), starting from the first version with the Windows updater: 0.3.0-rc.6 installed through the
installer does not check yet, and moves to the next one through that one's installer.

## Versões e releases (Versions and releases)

Vitral ships ready-made from GitHub (`VictorMenegate/vitral`, MIT license): the PC does not need to
compile anything to install it. Each version has its own section in `NOVIDADES.md` (`## 0.2.0`, short
sentences, what you notice while using it), which becomes the Release notes and, later, the
**Novidades** (What's new) screen of the app; a prerelease (`0.2.0-rc.1`) uses the section of the
version without the suffix.

- **CI** (`.github/workflows/ci.yml`): on every PR and every push to `main`, `cargo test --locked` on
  `ubuntu-24.04`. There is no user systemd and no git identity there: processes fall into their own
  group and the commits made by the tests go out with Vitral's identity, as on a PC without those.
  Windows stays off until it compiles (the repository variable `WINDOWS_CI=true` turns it on without
  failing the build; `WINDOWS_OBRIGATORIO=true` makes it fail). On `main`, one job compiles in release
  to keep the tag's cache warm. `dependabot.yml` only updates the actions (`Swatinem/rust-cache` is
  pinned by commit).
- **Release** (`.github/workflows/release.yml`): goes out with a `v<version>` tag equal to the `version`
  in `Cargo.toml` (`git tag v0.2.0 && git push origin v0.2.0`), or by hand in Actions › Release
  (without a tag, it only compiles and keeps the packages for 14 days). It tests and compiles in
  release with `VITRAL_CANAL=estavel` and the commit embedded, checks the package's `vitral --versao`
  and `--versao --json`, and generates `vitral-<version>-linux-x86_64.tar.gz` (the binary, LEIA-ME,
  NOVIDADES and LICENSE), `vitral-versao.json` (`pacote/manifesto.py`: version, commit, page, data
  format, notes, and the size and SHA-256 of each package) and `SHA256SUMS`. The publish job runs in
  the `release` environment (with the owner's approval): it signs the manifest and the sums with
  minisign, checks them against `pacote/vitral.pub` and publishes as a draft before releasing, so
  that "latest" is never left half done. A version with a suffix goes out as a prerelease, which
  `releases/latest` ignores. On `windows-2025` it produces `vitral-<version>-windows-x86_64.zip`
  (portable: `vitral.exe`, `vitral-lancador.exe` and the texts), the `.tar.gz` with the same contents
  (what the Windows updater will download; made with GNU tar and the Linux options) and the
  `-instalador.exe` (Inno Setup, `pacote/windows/vitral.iss`, installed through `choco` if the image
  does not have it), also in `SHA256SUMS` and in the manifest (each system reads only its own
  package). While `WINDOWS_OBRIGATORIO` is not on, a Windows failure does not hold back the release:
  it goes out with Linux only and a warning. On the `windows` branch, Actions › Prints do Windows
  (Windows screenshots; the installer job) silently installs the package from a release run, checks
  the folder, the shortcut, the AppUserModelID and the launcher choice, opens the app through the
  shortcut and the zip directly, and uninstalls while checking what is left over
  (`ferramentas/instalador-windows.ps1`).
- **Signing**: minisign (Ed25519). The main key and the spare key, without a password, live in
  `~/.config/vitral/chaves` (folder 700, files 600): the main one also lives in the
  `MINISIGN_SECRET_KEY` secret of the `release` environment; the spare one never goes to GitHub. Keep
  a copy of both `.key` files outside the PC: losing both means reinstalling by hand on every PC.
  The public keys live in `pacote/` and enter the app through `include_str!`
  (`src/atualizacao/assinatura.rs`), which only accepts the prehash format; if the main key leaks or
  is lost, the spare key signs a version that swaps the keys. The tests use a test key
  (`src/atualizacao/teste/`), never the main one.
- **Where it came from**: `VITRAL_CANAL` and `VITRAL_COMMIT` are read at compile time
  (`option_env!`); without them the binary calls itself "pc" (compiled on the PC), and only the stable
  channel updates itself. `FORMATO_DADOS` (in `modelo.rs`) is the data format (1, the `.vitral/` of
  each folder of the old Vitral; 2, the global data of the versions with projects; 3, chats only, each
  one directly in a folder), so that going back to an older version does not open data it does not
  understand.
- **Installing in one go**: `pacote/linux/instalar.sh` ships as a Release file (`curl -fsSL
  …/releases/latest/download/instalar.sh | sh`), with the release tag recorded in it by
  `release.yml`: the one from a prerelease, which "latest" does not show, installs the prerelease
  (`VITRAL_TAG=v…` chooses by hand). It checks `SHA256SUMS` (and the signature, if minisign is
  installed), downloads the package to `~/.cache` (outside `/tmp`), checks that the binary opens and
  calls `vitral --instalar`.
- **Installation** (`src/atualizacao/linux.rs`): each version in its own folder,
  `~/.local/share/vitral/app/<version>/`, and two links: `atual` (current), the one the menu opens
  (`vitral.desktop` has `Exec=…/app/atual/vitral %f`), and `anterior` (previous), the one that was
  there before. `vitral --instalar` copies its own binary (and the LEIA-ME, NOVIDADES and LICENSE that
  sit next to it) and writes the same `vitral.desktop` as before (the dash favorite keeps working),
  the icons and the `~/.local/bin/vitral` link, if that folder exists (a `vitral` that is not a link
  stays where it is); it only writes what changed. Run from inside the app folder, it only checks the
  menu. The order survives a power failure at any point: it opens in a `.novo-*` folder, tests the new
  binary (`--versao --json`, 10 s), puts it in place with a rename (the same version again swaps places
  with the old one in a `RENAME_EXCHANGE`), points `anterior` and then `atual` (temporary link and
  rename), writes the `estado.json` (the version to confirm, the refused ones and the data format of
  each version) and removes the other versions through `.lixo-*`. `atual` always points to a complete
  version, and the next launch deletes what was left half done. `app/.trava` prevents two
  installations at once. In the installed version, the GPUI restart goes to `app/atual/vitral`
  (without that it would open the old version, because `/proc/self/exe` resolves the link), and every
  launch of the current version checks the menu.
- **Installation on Windows** (`src/atualizacao/windows.rs`): the installer layout, in
  `%LOCALAPPDATA%\Programs\Vitral`: each version in `app-<version>\` (the whole package, as the
  Windows `.tar.gz` brings it), `versoes.txt` (`atual=` and `anterior=`, CRLF) and the launcher
  `Vitral.exe`. The installation opens the package in a `.novo-*` folder, tests the new `vitral.exe`
  (`--versao --json`, 10 s, no window), puts it in place with a rename (retries for 2 s, since the
  antivirus holds the new `.exe` for a moment), swaps the launcher at the root if the one in the
  package is different (by checksum; if it is open at that moment, it waits for the next launch of the
  current version), writes the `versoes.txt` next to it and swaps in one go (`MoveFileExW` with
  `MOVEFILE_WRITE_THROUGH`; the `estado.json` and the updater's other writes too), writes the
  `estado.json` and removes the `app-*` folders that are not the current one, the previous one or the
  one that is running, through `.lixo-*`. The folder of a `.exe` in use does not go away: it stays
  whole for the cleanup of the next launch (which only removes versions when there is a `versoes.txt`
  that points to a whole one; without it, the launcher chooses among the folders). A failure at any
  step leaves the launcher opening a whole version (a test with the failure points, which runs on both
  systems: the Windows layout compiles everywhere, and `versoes.txt` is read by the same
  `lancador/src/escolha.rs`). Reiniciar (Restart) goes through the root launcher (GPUI waits for the
  process to exit with PowerShell and opens `Vitral.exe`, which opens the new version); the health
  rollback opens the previous one directly and exits. The PC session, for health purposes, is the boot
  time (now minus `GetTickCount64`, to the minute), and the update thread runs in background mode
  (`THREAD_MODE_BACKGROUND_BEGIN`). `--instalar` is Linux only; on Windows, the build compiled on the
  PC and the portable zip only warn, with the **Baixar** (Download) button (release page), because the
  shortcut, the uninstaller and the registry belong to the installer.
- **Automatic update** (`src/atualizacao/`): the `vitral-atualizacao` thread (`nice 10`) checks 60 s
  after opening and every 6 h (plus a slack of up to 10 min), never before 1 h after the last
  successful check, and 1 h after a network error. It reads the `vitral-versao.json` from
  `releases/latest` (no API and no token; with the ETag, a 304 is enough), checks the signature before
  reading the JSON (the trusted comment has to be "vitral <version> vitral-versao.json"), validates
  the manifest and only keeps a higher version without a suffix. In the **Automáticas** (Automatic)
  mode, the default, a release installed in the app folder downloads the package to `app/.baixando`
  (up to 200 MB, with 3 times the size free on disk, size and SHA-256 checked along the way), installs
  as above and becomes **pronto** (ready): it takes effect on restart, and Vitral never restarts by
  itself. **Só avisar** (Notify only), the build compiled on the PC (channel "pc", also the one from
  `instalar-dev.sh`) and a package opened outside the app folder only warn; "Instalar" (Install) (on
  the build compiled on the PC, "Instalar como app", Install as app) downloads and installs the same
  way. Three failures in a row for the same version (different file, binary that does not open) give
  up until another one comes out; with no published release (404), it is up to date.
  `~/.config/vitral/atualizacoes.json` keeps the mode (`automaticas`, `so_avisar` or `desligadas`), the
  last check, the ETag and the failures. `VITRAL_SEM_ATUALIZACAO=1` turns the thread off (the test
  bench uses it). Outside the stable channel (compiled on the PC, the test channel or a prerelease),
  `VITRAL_ATUALIZACAO_BASE=https://github.com/VictorMenegate/vitral/releases/download/v0.3.0-rc.2`
  looks in that release and accepts a prerelease; outside the stable channel (the channel embedded at
  compile time; on stable the check always refuses it), `VITRAL_ATUALIZACAO_CHAVE=<.pub file>` adds a
  key to the embedded ones, so the test bench can serve a fake release signed with a key of its own.
  The **teste** (test) channel (`VITRAL_CANAL=teste`) is the one for acceptance on the Windows runner:
  it installs by itself in the app folder like the stable one and, only in it,
  `VITRAL_TESTE_CAI=<version>` makes that version crash after counting the launch and before the first
  frame (the real automatic rollback). The interface (`src/ui/atualizacao.rs`) reads the `Global`
  `Atualizacoes` (state, mode, permission, last and next check, the launch notice and the possible
  rollback) and redraws with `observe_global`: the Janela (Window) only when the ready version changes
  (the dot and the menu item), the Configurações (Settings) on every change, and the download progress
  goes at most 2 times per second and only while the Settings are open (`acompanhar_progresso` returns
  a guard that they hold). The sentences come from pure functions of the state (`linha_da_versao`,
  `texto_do_erro`, `megas`, `botoes`, `texto_do_aviso`), with tests; the version notices use the same
  key and replace each other, and the Window remembers which one it already showed (one per version).
  The release notes come from the manifest's `notas`, from the `NOVIDADES.md` that came with the
  version in the app folder or, for the running version, from the binary itself (`include_str!`).
  Reiniciar (`fechar::reiniciar_pela_janela`) goes through `pode_parar_tudo` with `Parar::Reiniciar`;
  the text of the question is `confirmacao_de_parar`, with a plural test.
- **Health and rollback** (`src/atualizacao/saude.rs`): at the start of `main`, before GPUI (and after
  `--versao` and `--instalar`), `saude::ao_abrir` looks at the `estado.json` of the managed version
  that is running. The installation writes `confirmar = { versao, aberturas: [] }` there; each launch
  of the version behind the `atual` link without confirmation gets in with the PC session
  (`/proc/sys/kernel/random/boot_id`; on Windows, the boot time) and the time, and the ones from
  another session drop out (a power failure does not count). With two unconfirmed launches in the same
  session, the third puts the version in `recusadas` (the check does not offer it again), writes
  `voltou` and only then points `atual` to the previous one (which has to answer `--versao --json` and
  be able to read the format on disk); `main` does an `exec` of it with the same arguments, and it
  reports the rollback once. A rollback cut off between writing the state and swapping the link
  finishes on the next launch. The version is confirmed 10 s after the first frame
  (`atualizacao::primeiro_quadro`) or on a normal close (`on_app_quit`). The first launch after an
  update shows "Atualizado" (Updated). All of it with the installation lock (another Vitral
  installing: the launch does not count) and atomic writing (temporary + fsync + rename). The manual
  rollback (`saude::voltar`) does the same, with `automatica: false`, and the caller restarts. The
  format rule: `dados::formato_em` reads the `formato` of the data folder
  (`~/.local/share/vitral/formato`; without it, 2 if there is already an `estado.json` and 1 if there
  is not), which the migration writes right after the `estado.json` and each launch checks
  (`Dados::marcar_formato`, which never lowers the number); the rollback only goes to a version whose
  `formato_dados` (from the app's `estado.json`) is greater than or equal to it. The copy from before
  is the `.vitral/` of the old folders themselves, which the migration only reads. Health runs after
  the lock (a launch that only hands the request to the open Vitral does not count) and before the
  migration (one that crashes while migrating counts). `app/` lives inside the data folder
  (`dados::PASTA_DO_APP`), and nothing in the data touches it: **Começar do zero** (Start over) moves
  everything aside except `app/`, and the app's `estado.json` is not the data one.
- **Development**: `ferramentas/instalar-dev.sh` compiles this checkout in release (through
  `compilar.sh`) and installs it in the same layout as `<next>-dev.<commit>` (`VITRAL_VERSAO` at
  compile time; `.modificado` at the end with uncommitted changes). By semver that is lower than the
  next one, and its release replaces it. For the owner, installing is only through releases.

## Arquivos (Files)

| Arquivo | O que faz |
| --- | --- |
| `src/motor.rs` | The chats (create with `novo_chat(pasta)`, the list in `chats_da_lista`, rename, pin, archive, what is waiting for you), the turns and the queue, accounts, resumptions, Ultracode and background agents, the permission requests and the session handover |
| `src/motor/pedidos_de_hook.rs` | What the user's `PreToolUse` hooks asked to be asked about: the reason for each one, from the hook event to the call's permission request, and the rule that this request never goes through "Não perguntar de novo" (Do not ask again) |
| `src/motor/conversas.rs` | What links conversations to each other and to the user: `listar_chats`, `mensagem_para_chat` and `abrir_chat`, the loop brake and the time you left the window |
| `src/motor/passos.rs` | The list of agent steps of each chat (`TodoWrite` and the Claude Code task tools) and the sentence of what each call changed in it, for the trail |
| `src/motor/fundo.rs` | The background agents of each chat: the list from `background_tasks_changed`, the call that opened each one, the end of each one's file read little by little (the last action), and the item that marks their end in the conversation |
| `src/motor/revisao_do_par.rs` | The turn review by the other agent: when it runs by itself (20 lines or more, the option on, the other one at hand), the reviewer that joins as a partner and leaves at the end, the result (the line or the card) and the delivery of the review to the chat's agent |
| `src/ui/mensagens/revisao.rs` | The card of the other agent's review, with "Mandar ao Claude" (Send to Claude), and the "Revisar com o Codex" (Review with Codex) of the "Trabalhou…" (Worked…) line |
| `src/motor/subconversa.rs` | A subagent's conversation opened in a tab: the fake chat, read-only, outside the state; finding and rereading its file in the background while it works; which subagent is waiting for a permission from you and the delivered report that goes to the card |
| `src/motor/importacao.rs` | The import in the engine: a chat directly in the folder of each Claude Code conversation, the one on first launch, the one from the menu, the watcher (on every launch and every 2 minutes, only what changed since the previous pass) and the mark of when it continues in Vitral |
| `src/dados.rs` | Global data folder (`~/.local/share/vitral`, the configuration in `~/.config/vitral`): paths, writing that survives a power failure (temporary with fsync, `.anterior` by hard link, rename and fsync of the folder), the background writer (which keeps a failed write to try again), `carregar` with fallback to `.anterior` and the guard of `estado.versao-N.json`, the `normalizar` (the data of the versions with projects: chats only, each with its folder), the data `formato` and `DADOS_GLOBAIS` |
| `src/migracao.rs` | Migration from the old Vitral (the `.vitral/` of each work folder) to the global data: once only, on first use, writing nothing in the old folders, with the `estado.json` written last; postponed while the old Vitral is open |
| `src/instancia.rs` | One Vitral per user: the lock (`flock` on `~/.local/share/vitral.trava`), the socket (`$XDG_RUNTIME_DIR/vitral/vitral.sock`, folder 0700) through which a second launch hands over the request and exits, the `Abertura` (`vitral <folder>`, `--nova <folder>`) and the re-execution of the recovery screen |
| `src/imagens_recebidas.rs` | The images a tool returns in its result: the base64 block (the Claude API one and the MCP one) becomes an `ImagemRecebida`, which the engine writes to `imagens/<chat>/` with the content hash in the name (`guardar`, `guardar_na_chamada`); the caps per image, per chat and overall (`podar`), and the cleanup when Vitral opens (`varrer`) |
| `src/importar.rs` | Reading of Claude Code conversations (read-only): the listing with title, dates, messages and whether it is open (the watcher's `Filtro.desde` skips what did not change), the history as conversation items and the record of what has already come in; `importar/subagente.rs` finds and reads the file Claude Code writes for each subagent (`subagents/agent-<id>.jsonl`) |
| `src/claude.rs` | Claude Code process in `stream-json` (turn arguments and environment), reading of events, Claude Code version, session check, the models it offers (the `initialize`, in `atualizar_modelos_oferecidos`) and the detail of each tool call (`detalhe_ferramenta`: the Edit diff, the start of the Write, the Bash command, up to 60 lines on each side) |
| `src/mcp.rs` | Local MCP server with Vitral's tools (`listar_chats`, `mensagem_para_chat`, `abrir_chat` and `permissao`), one route per chat with the session token |
| `src/prompts.rs` | The text Vitral puts in each chat's system prompt: where it works (directly in the folder, Sem pasta (No folder) or continuing a Claude Code conversation) and the `[de: …]` and `[Vitral]` conventions; at the end of it, the user's instructions (`com_instrucoes`) |
| `src/perfis.rs` | The saved agent profiles (`Config.perfis`); their screen and selector were removed in 0.11.0 |
| `src/versoes_agentes.rs` | The newest published Claude Code and Codex (the package tags on npm, kept for one hour), for the "há a X" (X is available) of the Settings |
| `src/cli_agentes.rs` | Codex and Gemini as installed programs: version, `login status`, MCP lists, `update`, `logout`, the terminal command for login and the configuration file of each one (reading what they print is pure, with tests) |
| `src/diagnostico.rs` | The diagnostic text for asking for help and the covering of anything that looks like a secret (`cobrir`) |
| `src/processos/` | Every process Vitral opens (`novo_comando`, no console window on Windows) and the ones it stops entirely: systemd scopes or process groups (`unix.rs`), Job Objects (`windows.rs`); `entrada.rs` writes to the input of claude and codex on a thread of its own, because the pipe fills at 64 KiB and the window cannot wait for the process to read |
| `src/sistema/` | What opens outside Vitral, on both systems: editor, terminal, file manager, the bash for commands (on Windows, the one from Git for Windows, never the WSL one) and the notice of what is missing to install (`linux.rs`, `windows.rs`) |
| `src/contas.rs`, `src/contas/` | Extra accounts (`CLAUDE_CONFIG_DIR`), login and usage and plan (what Claude Code keeps in `.claude.json` and the direct lookup at Anthropic, `busca.rs`) |
| `src/codex.rs` | Codex plan usage, read from the sessions in `~/.codex/sessions` |
| `src/agente_gemini.rs` | Gemini as a chat's agent, through `agy acp` (ACP): the input channel, the translation of the output into Claude Code events, the permission request, the modes and the model catalog |
| `src/agente_codex.rs` | Codex as a chat's agent: the process arguments and the JSON-RPC protocol in `src/agente_codex/servidor.rs`, the translation of the output into Claude Code events, the sandbox of each mode and the model catalog |
| `src/git.rs` | The git command (`novo_git`, the only way to open git), the relative path as text and where an isolated copy from earlier versions came from (so the load puts the chat back at its origin) |
| `src/modelo.rs` | The types of what goes to disk (chats, occurrences, queues and the global configuration, with the old names as aliases), the types that are only read from the versions with projects (`ProjetoAntigo`, `TarefaAntiga`, `PapelAntigo`) and `FORMATO_DADOS` and `VERSAO_ESTADO` |
| `src/atualizacao/` | Embedded version and the windowless commands (`--versao [--json]`, `--instalar`), the update state for the interface (`mod.rs`), the thread and the cycle (`procura.rs`), the manifest (`manifesto.rs`), the signature (`assinatura.rs`), the download (`rede.rs`), the package and the check of the new binary (`pacote.rs`), the per-version installation and the menu on Linux (`linux.rs`), the per-version installation, the `versoes.txt` and the launcher on Windows (`windows.rs`), the `atualizacoes.json` (`config.rs`) and the health of the new version with the rollback to the previous one (`saude.rs`); the fixtures live in `teste/` (`teste/ponta/` is the fake release of the end-to-end tests, signed with another test key) |
| `src/tema.rs` | Tokens of the visual language (glass and black-part colors, measures in rem with `r()` and `tam()`, typography, shadows and gradients), GPUI Kit theme, syntax colors and extra Lucide icons |
| `src/ui/` | The top of the list (`trilho.rs`: the logo, the Vitral menu and the bell), the list (`lateral.rs`), the panels (`conversa.rs` is the shell: the tabs, the header, the panel menu, the dock and the conversation usage popover; `mensagens.rs` what goes inside, with the permission request card, the card of the plan left in the conversation and the composer status line, `pergunta.rs` the card of the question with options (AskUserQuestion) and its answers, and `mensagens/historico.rs` the history of conversations brought from Claude Code, read in the background, with automatic pagination when scrolling up, and `mensagens/a_vista.rs` the computation of which conversation blocks are mounted and the stored heights of the others, and `mensagens/no_fundo.rs` the lines of the background agents and the one for the end of each; `vista_mensagens.rs` the messages of each panel and the list as cached views; `pilulas.rs` the pills of the composer status line; `checagens.rs` the checks (test, typecheck, lint, build) that the turn ran, for the "Trabalhou…" line; `paineis.rs` the side-by-side panels, the tabs of each one and the drop; `arranjo.rs` how the window was when it closed), `links_arquivo.rs` (the file paths in the answer that become links and open in the right panel, at the cited line), `composer.rs` (the message box) and `composer_seletores.rs` (its chips, menus and popovers), `menus.rs` (the conversation menu, the same everywhere), `dialogos.rs` (Novo chat (New chat) and the folder palette, rename, accounts, settings, mention a file and a conversation as context), `este_chat.rs` (the right side: where the chat works), `abas_direita.rs` (the right tabs), `painel_arquivo.rs` (the open file on the right, with editing, line selection and Ctrl+F inside the file), `editor_arquivo.rs` (the open file's editor: the comparison with disk, the automatic saving and the conflict when the agent writes the same file), `previa_markdown.rs` (the frontmatter and the links of a .md preview), `edicao.rs` (a call's edit placed over the current file: the two diff formats, the found fragments and the rows), `mencao.rs` (file mentions as text and as a pill, in the bubble and in the box), `caixa_entrada.rs` (the bell popover and Ctrl+G), `resumo.rs` (the loop brake in "Esperando você" (Waiting for you)), `abertura.rs` (what to do with the folder and the `--nova` of a launch, on opening and through the socket), `recuperacao.rs` (the screen for when the data opens read-only, with "Começar do zero" and "Tentar de novo" (Try again)), `uso_plano.rs` (the usage rings in the corner of the window and their popover), `atualizacao.rs` (the update: notices, Novidades, the Settings group and the texts of each state), `fechar.rs` (the question before closing or restarting), `busca_conversa.rs` (Ctrl+F inside the conversation: the bar, the index of the occurrences and the highlight), `fila.rs` (the list of messages waiting for the turn, with "Entregar agora" (Deliver now)), `reenvio.rs` (fix and resend the last message), `rascunhos.rs` (what you wrote and did not send, kept per conversation in `interface.json`), `imagens.rs` and `visor_imagem.rs` (the chat's images and the viewer), `seletor_arquivos.rs` (the folder's files to mention with `@` and the ignored ones of the Arquivos (Files) tab), `atalhos.rs` (the single table of keys, which becomes the `KeyBinding`s and the "Atalhos do teclado" (Keyboard shortcuts) dialog of `?`), `zoom.rs`, `movimento.rs` (the single animation gateway: durations, curves, 60 frames per second cap, entrances and transitions, the metal loop and the `Segurar` (hold) of the dangerous request) |
| `src/ui/componentes/` | The components of the visual language (chip, buttons, segmented control, popover, menus, alerts and notices, list cards, options, choice list, badges, tabs, dialogs, code box), the slotted rings and the spinning ring (`graficos.rs`), the double frame, the tile and the hold button (`controles.rs`), the metal plate, the object header, the chosen-item card and the footer strip of the menus (`superficies.rs`), the liquid metal (`metal.rs`, the engine: the window sheet, the openings and the metal rim; `malha.rs`, the point-by-point recipe computation and the mesh of the menu button), the single state (each chat's signal) and the showcase (`--vitrine`) |
| `pacote/` | `manifesto.py` (the release's `vitral-versao.json`), the public keys (`vitral.pub` and `vitral-reserva.pub`), `linux/instalar.sh` and `windows/vitral.iss` (the Windows installer, Inno Setup 6) |
| `lancador/` | The Windows launcher (`vitral-lancador`, std only, the `Vitral.exe` at the installation root): `escolha.rs` reads `versoes.txt` and chooses the `app-<version>`; `build.rs` sets the icon, the version and the manifest. Outside Windows, `main` only warns and exits with 2 |
| `ferramentas/logo/` | The logo: `v-do-dono.png` (the V the owner sent), `rasterizar.py` (extracts the V from it and writes `assets/vitral-v.png`), `icones.py` (the metal with the V on top: the `assets/vitral-*.png`, the `vitral.ico` and the two SVGs with the PNG embedded) and `previa.py` (the metal computation in Python, which `icones.py` uses); see **Logo**, in **Acabamento** (Finishing) |
| `ferramentas/instalar-dev.sh` | Development only: compiles and installs in the app layout as `<next>-dev.<commit>` |
| `.github/` | `workflows/ci.yml`, `workflows/release.yml`, `workflows/windows-prints.yml` (by hand: screenshots on a real Windows, through `ferramentas/prints-windows.ps1`; the installer from a release run, through `ferramentas/instalador-windows.ps1`; and the updater, from the published version to three before the branch on a fake release, through `ferramentas/atualizador-windows.ps1`) and `dependabot.yml` |
| `build.rs` | On Windows, the icon and the version in `vitral.exe` (the manifest comes from GPUI); elsewhere, nothing |
| `tests/versao.rs` | `--versao` and `--versao --json` without a screen (the real binary, with no DISPLAY or WAYLAND_DISPLAY) |
| `tests/instalar.rs` | `--instalar` with the real binary in a temporary HOME in `target/tmp`: the layout, the link, the `.desktop`, `~/.local/bin` and reinstalling without duplicating |
| `README.md`, `docs/imagens/` | The GitHub front door (what it is, installing, checking the download, limits) and its screenshots, taken on the test bench with `RAIZ_SINTETICA=1` and `CLAUDE_FALSO=1`, cropped and with a 256-color palette |
| `assets/` | Full and small logo (SVG with the PNG embedded), PNGs from 48 to 256 px, `vitral.ico` (16 to 256 px, for Windows), all generated by `ferramentas/logo/icones.py`, `vitral-v.png` (the 256 px white V of the menu button, generated by `rasterizar.py`), Claude and Codex logos (and the colored discs in `marcas/`), the full shield of Manual mode and the embedded fonts (`fontes/`: Inter 4.1 and JetBrains Mono 2.304, only the weights used, OFL 1.1 in `fontes/OFL.txt`, which goes in the package as `OFL-fontes.txt`) |

Lucide icons that are not among the GPUI Kit ones have to be added to `icon_assets!` in
`src/tema.rs`, otherwise they come out blank (a test checks it). The Kit components' texts come out
in Portuguese (`set_locale("pt-BR")` in `src/main.rs`).

Measures and zoom: everything is measured at 100% and grows with the zoom (`r()` gives rem; `tam()`
gives Pixels for the APIs that only accept Pixels); radii, shadows and strokes stay fixed on purpose.
So do the widths of the parts: `orcamento(util, escala, …)` in `src/ui/mod.rs` multiplies `LATERAL`,
`DIREITA`, `MIN_CONVERSA`, `FAIXA_LISTA`, `FAIXA_DIREITA` and the others by the zoom; `Larguras` keeps
what the drag left, at 100%, and `em_px` gives the frame's widths. The state of the dividers
(`estado_partes`) belongs to the Window and starts over when the composition or the zoom changes, from
the stored widths; the drawer uses the width stored at the current zoom. In the list,
`Aperto::de(width at 100%)` decides what drops out, and the search text is swapped in `lateral()`
(`dica_da_busca`). The width that decides the `Aperto` is stored at 100% (the `largura-da-lista`
state), so a zoom change does not produce a frame with the previous scale's computation. The list's
search is `componentes::campo_de_busca_de` at a height of 40, the same as the logo and the "+" (the
magnifier 8 from its edge, which on the 8 piece falls on the 16 guide), with "Ctrl" "K" on the right
only on the Conversas (Conversations) tab and outside the narrow list, and the hint on top
(`campo_da_lista`). The thin scrollbar is `componentes::barra_de_rolagem` (no fade: the vendor's
`SCROLLBAR_*` stays at zero and the `barra_de_rolagem_sem_fade` test, in `tema.rs`, checks it);
`BotaoCopiar` gives the kit's Copiar (Copy) a 24 that grows with the zoom; the kit's text field has
its side margin in rem (`composer::recuo_do_texto`, where the box's text starts and by which the turn
strip is aligned, relies on that); and the notices have their width (`tema::LARGURA_AVISO`)
multiplied by the zoom in `zoom.rs`.

Spacing only on the scale 2, 4, 6, 8, 12, 16, 24 and 32 (`tema::esp`, with names by context: `PECA`,
`CAIXA`, `FLUTUANTE`, `GUIA_LISTA`, `GUIA_EQUIPE`, `NO_TURNO`, `ENTRE_TURNOS`, `ANTES_DE_SECAO`);
anything with a 1 px outline uses `dentro_da_borda(12.)` so the content sits 12 from the outer edge.
Tailwind shortcuts (`gap_2()`, `p_3()`) measure in rem and, with 1 rem = 15 px, land off the grid (7.5
and 11.25). The `espacos_na_escala` test blocks both cases across all of `src/ui`, with no
exception, and `movimento_com_teto` blocks animation made outside `src/ui/movimento.rs`.
`metal_so_pelo_motor` blocks `paint_image(` and `drop_image(` outside
`src/ui/componentes/metal.rs`. `dica_pela_gerenciada` blocks GPUI's bare `.tooltip(closure)`: every
tooltip goes through the kit's managed one (`dica_no_ponteiro` for a wide row, `dica_com_hover` for
whoever already listens to hover, or the buttons' own), which waits only for the first and, with one
open, shows the next one immediately (owner, 23/11/2026).

Radii (`tema::raio`, in fixed Pixels): on controls, a single standard, `raio::de(altura)`, 45% of the
height (16 on a 36 button, 14 on a 32 one, 9 on a 20 chip; well rounded, never round, the logo's
design scaled), on buttons (the `BotaoRedondo` included), chips, tabs, the search, the segmented
control, badges, the highlight of menu items and the keys; on pieces, the tokens: the variant's
`raio_da_peca()` (16 in Grafite) on the window cards (the conversation, the right panels, the open
file) and on the core of the double frames; `PECA` 20 on drawers, popovers, menus, dialogs, notices,
on the message box frame and the requests' one, and on full screen; `CARTAO` 14 on what goes inside
them (right and conversation cards, bubble, two-line list card, code blocks and chat tables,
annotation, option groups, alerts, the box in Estreita (Narrow) and the tooltip); `CONTROLE` 10 on
fields, rows (single-line ones in the list, tree and log), blocks inside a card (code), the tile and
thumbnails; and `PILULA` (the `rounded_full()`, or `raio::PILULA` where the API only accepts Pixels)
only on what stays round: dots, rings, thin bars, the disc of the usage rings and the "Segure para
permitir" (Hold to allow). Inside something with padding p, the radius is max(outer radius − p,
`CONTROLE`), so the corners are concentric: the 40 spot of the rail has 14 (20 − 6) and the 32 square
inside it, 10
(14 − 4); the menus have margin 8 so the 10 items fit in the 20 menu; the model popover, 6 plus the 4
of the list. Where the math does not close, the token within 2 of the exact one applies (concentric by
eye) or the rounder inner corner (the message box keeps 20 inside the conversation); to avoid
flattening more than that, increase the padding. Layers inside (the reflection, the box's glow, the
tinted header of the cards) take the piece's radius minus the border. The kit theme follows (`radius`
10, `radius.lg` 20) and the `raios_na_escala` test blocks, across all of `src/ui`, a loose radius
(`rounded(px(..))`, `rounded(r(..))`, a number), a Tailwind shortcut (`rounded_md()`) and
`ButtonRounded`, with no exception.

Surfaces (in Grafite): behind everything sits the metal sheet (`componentes/metal.rs`, the
`metal::fundo`, first child of the Janela); `Cores::vidro` (#0d0f13, opaque) is only the fallback under
it and what the kit root paints; `vidro_leitura` (#0a1533e3) is the dark blue veil of the base, which
the Janela paints over the sheet across the whole window (the list has no background of its own), and
`cartao` (#14161df2) is the one for the conversation and the panels, with the `sombra_flutuante`;
`dialogo` and
`gaveta`, the opaque for what covers the conversation without being a floating element; `flutuante`
(#171a23f7, equal to the theme's `popover.background`, which the kit uses in menus, popovers, tooltips
and notices) is the near-black of what floats; `cartao_elevado` is the core of the double frames;
`preto` (#08090c) is the inner one (code, command output, diff, the message box frame and the disc of
the usage rings); and `caixa_quente` and `caixa_preta` are the box's field, today the same opaque gray
(no gradient). The dark `recuo` vanishes into the near-black: inside a popover, `recuo_no_flutuante`
applies (in the segmented control, `.no_flutuante(true)`). The tests in `tema.rs` check the text
contrast on all of them, the syntax at 4.5:1 on black (also on the lines that entered and left the
diff) and that hover and selection stay visible on black. The `BotaoRedondo`, like the logo and the
rings, does not show the tooltip while its popover is open.

Motion: `src/ui/movimento.rs` has `COR` 120, `ENTRAR` 160, `MOVER` 180 and `ENCHER` 240 ms; `Curva`
(Entrar, Mover, Sair, no bounce); `animacao(d, c)` for `with_animation`; `entrar(el, id, De)` for what
appears; `valor`/`valor_desde` for a value that moves to the target when it changes (mounting does not
animate); `disparo` for an effect that plays when a key changes; `Hover` for the pointer on a loose
control; `JaVistos` for what just appeared in a part of the screen (what the panel shell showed in the
previous drawing, `Painel.ja_vistos`, flipped at the start of `Janela::conversa` and forgotten when
the conversation changes: the requests, the queue, each pill, the strip state, the composer status
line, the empty panel); `AbertosAgora` for what a click just opened in the conversation
(`Janela.abertos_agora`, marked by `alternar` and by the trail clicks; it lasts 400 ms, so the opened
content enters once and a later remount shows up ready); `lado_da_navegacao` (the parts of the
Settings); and, in `mensagens.rs`, `chevron` (the open and collapse one, which turns 90° through
`valor`), `entrada_da_chamada` and `entrada_do_cabecalho` (with a test). An `entrada` that stops being
mounted midway loses its state: whoever decides to enter keeps the wrapper (the `Some(false)` of
`entrada_do_bloco`) while the element stays visible; `abertura(id)` and
`deslizar_gaveta(el, De::Esquerda | De::Direita, t)` for the drawer (24 px in `MOVER`, with the veil at
0.5·t; `Janela.geracao_gaveta` goes up on each opening, and closing is abrupt). All on the 60 fps clock
per window, and the last beat already lands on the target. What moves without stopping (the metal)
goes through `laco_a(window, cx, qps)`: the phase in `f64`, one beat marked per window (a faster
request marks another, earlier; the old one, when it arrives, does not count), `anima` (motion on and
window in the foreground) and `laco_agendado`; with `qps` 0 it returns the phase without moving or
requesting a frame. `tema::aplicar` calls `movimento::iniciar`, which reads GNOME's `enable-animations`
once and the choice in the Settings (`"animacoes": "desligadas"` in `interface.json`, written by
`zoom::gravar_na_interface` without losing the zoom) and turns on `set_reduce_motion`;
`movimento::escolher_animacoes` switches immediately and, going back to Seguir o GNOME (Follow GNOME),
hands control back to the kit, which follows the system. The
`VITRAL_QUADROS` counter lives in `src/ui/mod.rs` (`contar_quadro`, called in the `render` of the
Janela and of the showcase). In the conversation, `movimento::com_estado` lets whoever only has the
Janela's `&Context` use the stored state (the box's metal rim, the dock's light and height, the
entrances), and `movimento::entrada` does the fade with slide of something that just appeared: `nova`
only on the first frame, so a remount (another panel opened, the list collapsed) repeats nothing and
redraws nothing. The blocks use `Painel.base_animacao` (the conversation's size when it opened in the
panel) and `Painel.vistos` (`entrada_do_bloco`, with a test). The send pulse goes up with
`Painel.envios`, in `paineis::enviar_do_painel` (Enter and the click). The bell pulse is
`BotaoRedondo::pulso_cor` with `anel_do_pulso`, and the generation only goes up when the number
increases (`sino_numero`, `sino_geracao`, zeroed when the folder changes).

Pointer outside and window without focus: with the pointer outside the window and no drag, the Janela
puts the `ponteiro-fora` layer on top, which removes the stuck hover (Wayland does not move the stored
position when the pointer leaves); the vendor's tooltip does not open that way either. Without focus,
the spinners' beat schedules no tick and returns the phase stopped, `observe_window_activation`
redraws on return and the 1 s clock does not redraw when the beat is already ticking.

Engine and redraw: `Motor.visiveis` (the conversations some panel shows) is updated in the Janela's
render without notify; `tratar` returns whether the batch asks for a redraw, and partial text from a
conversation out of view only redraws on the first chunk (`juntar_parcial`, with a test). In an open
conversation, the first chunk redraws the window and the following ones only its panels
(`pedaco_de_texto` and `aviso_do_lote`, with a test; see **Custo de um redesenho** (Cost of a redraw)).
`salvar_estado` and `salvar_config` serialize on the spot and hand the bytes to the writer's queue.
The notification is `Motor::notificar`: `notify-send -a Vitral -i vitral -A default=Abrir -p`, on a
thread that keeps the id per conversation for the next `--replace-id`; the click sends `Sinal::Abrir`,
and the Janela's observer selects the conversation and calls `activate_window`.

Chats, archiving and forgetting: the interface only talks to the chats through the engine's API:
`chat(id)`, `chats_da_lista()` (everything that builds or counts the list goes through it),
`novo_chat(pasta, criado_por)` (with no folder, an empty folder in `dados/avulsos/<id>`),
`pastas_dos_chats()` (the Novo chat palette), `sem_pasta(chat)`, `renomear`, `alternar_fixado`,
`alternar_arquivado`, `arquivar_chat` (a chat that never ran leaves Vitral instead of going to
Arquivadas (Archived)) and `arquivar_varios`. `esquecer` clears the engine's maps of the conversations
that left and denies their permission requests. The writer deletes files in its turn
(`Gravador::apagar`, which removes a pending write of the same file from the queue): the chats'
messages go after the new state.

Data and normalization: `Dados` (in `dados.rs`) has the writer: state and configuration with
`.anterior` by hard link (the link goes in an `.anterior.tmp` and enters by rename, so there is never
a moment without `.anterior`), the chats without. A write that fails stays in `falhas` until the same
file writes; `Motor::gravar_sujos` (every 5 s) turns on the failure strip, puts the failures back in
the queue and, when it recovers, delivers the queues that waited. `carregar` reads each
`chats/<id>.json` item by item: what does not convert becomes `Item::Desconhecido` and is rewritten as
it came; `Chat` and `Estado` have `resto` (flatten), and a `versao` higher than the supported one opens
read-only. A state from an earlier version (`versao` 1 or 2) is kept as it came in
`estado.versao-N.json` before the first write (`guardar_estado_antigo`, once only). `salvar_config`
rereads `config.json` as a `Value`, removes the legacy
keys (`modelo_coordenador` becomes `modelo_mestre`, `modelo_trabalhador` becomes `modelo_chats`; with
both, the new one wins) and keeps the unknown ones. What the versions with projects wrote and this one
only reads lives in the `#[serde(skip_serializing)]` fields: `Estado.projetos` (`ProjetoAntigo`: id,
folders, archived, model), `Estado.tarefas` (`TarefaAntiga`: the form, the origin and the copy),
`Chat.projeto_antigo`, `Chat.papel_antigo` (`PapelAntigo`), `Chat.pai` and `Chat.tarefa`.
`normalizar(estado, dados, consultas, agora)` runs in `Motor::new`, before anything else, with the
`Consultas` trait (the real `NoDisco`, a fake one in the tests): it removes the Mestre (Master),
converts the old coordinator's workers into chats (the one from a closed request goes to the archived
ones; the one from the others gets the notice), removes the helpers, puts back at the origin whoever
worked on a copy (`voltar_para_a_origem`: the recorded origin, the copy's git one or the project's main
folder; a session born in the copy becomes a new one, with the `SaiuDaCopia` handover), passes to the
chats what belonged to the project (archived, model), prunes the old occurrences and sweeps up empty
chats older than 24 h; at the end, the tasks, the projects and the role and project marks of the chats
go away, and `Motor::new` writes everything before any turn. The test
`estado_com_projetos_abre_so_com_chats_de_pasta` runs the whole path with a real `estado.json` of
version 2. The system prompt goes by file
(`~/.cache/vitral/prompts/<chat>.md`, rewritten every turn) with `--append-system-prompt-file` and
`--system-prompt-snapshot off` from Claude Code 2.1.278
(`claude::VERSAO_PROMPT_POR_ARQUIVO`).

Migration: `migracao::migrar(dados, raizes, proc, pid, na_bancada, quando)` returns `NaoPrecisa`,
`SemRaizes`, `Migrou(Relatorio)`, `Adiada { raizes, pids }` or `Falhou { raiz, erro }` (the last two
become the recovery screen). It works on `serde_json::Value` (what this version does not know passes
through as it came): `ler_raiz` reads without writing (the chats' bytes, the notes' text, the mtime and
size of the `estado.json` and the folder of each subfolder, from which the origin of the requests
comes), `planejar` is pure and treats the roots together (task ids renumbered in two passes, the most
recent root first; only chats and requests, which the load normalizes), and `aplicar` writes directly
to the final folder, with the `estado.json` last and the `formato` right after it. `vitral_antigo_vivo`
scans `/proc/*/comm == "vitral"`, minus the process itself, its children and, depending on the side,
the test bench's (`DENTRO_DA_BANCADA=1`; inside it, only the one from the same bench, through
`XDG_DATA_HOME`). `migracao.json` keeps, per root, the mtime and size of the old `estado.json` (the
notice that the old Vitral changed something afterwards) and the renumbered tasks. The order in `main`:
`--versao`/`--instalar`, data, lock (or the handover to the open Vitral), version health, socket,
migration, load, `processos::limpar_sobras`, window.

Texts and pending items: `textos::duracao_curta` is the single time format ("19 s", "2 min 13 s",
"1 h 05 min", with two digits after the first so that the running time does not change width), used by
the list and by the turn clock. `lateral::Causa` (Permissao, Plano, Parada, Erro; `da_pendencia` for
the bell, `da_espera` for the list) and `frase_da_pendencia` give the same sentence and the same signal
in the list and in the bell, drawn by the same `previa_elemento` (test
`a_mesma_pendencia_diz_o_mesmo_na_lista_e_no_sino`). In the conversation, the panel at the end asks
for `scroll_to_bottom()` on every drawing; `acompanhar_rolagem` releases the end on any upward scroll
(1 px) and grabs it again in the last 48 going down, and `Painel.manter_lugar`, turned on by
`alternar()`, holds the place when a block at the end opens.

Ultracode and background agents: in `src/claude.rs`, the turn's arguments and environment are pure
functions (`argumentos`, `ambiente`, `configuracoes_do_turno`), with tests. Ultracode goes as
`--effort ultracode` (the way that turns it on before and after 2.1.284; before it, Ultracode only
counts at Extra) and, in the same `--settings`, `"enableWorkflows": true` (on the Pro plan, workflows
start off) and `"ultracode": true`. Outside Ultracode, `Workflow` goes in `--disallowedTools`, along
with `EnterWorktree` and `ExitWorktree` (the chat stays in its folder: the session is recorded by
folder). The agent limit goes in every turn. The
`Saiu` carries the exit code (read without reaping the process, in `processos::codigo_de_saida`). The
Claude Code version comes from the installer's `versions/<version>` link or from `claude --version`
and, at each init, from `claude_code_version`, together with the `Workflow` tool in the list (or not).
`background_tasks_changed` counts live workflows and subagents (Claude Code itself ends background
shells). In `src/motor.rs`: `Entrada.ultracode` and `Item::Usuario.ultracode` (written only when on;
the queue keeps it and survives a restart), `Reforco { ultrathink, ultracode }` on sending,
`motivo_sem_ultracode`, `aceita_ultracode`, one Ultracode at a time (`ultracode_rodando`), the end of
the turn postponed while there are background agents (`fim_adiado`; `agentes_cortados` when the
process exits before them) and `entrada_para_repetir` in the automatic retry of the account switch.

One chat, one process (0.10.0): there are never two agent processes in the same conversation. On
23/11/2026 a chat ended up with three `claude -p --resume` of the same session, each thinking it was
the only one. What triggered it, measured on Claude Code 2.1.328: when resuming a session whose
previous process was killed with a background agent (the Parar (Stop) of a chat waiting for the
agents), it sends the notice of the cut agent, puts the message in the queue (`command_lifecycle`
`queued`), sends an `init` and an empty successful `result`, with `num_turns` 0, and only then starts
the message (`started`). The engine took that `result` for the end of the turn: the chat looked
stopped with the process in the middle of the turn, and the next message (or the memory review, or the
Continuar (Continue)) opened another process, leaving the first running outside, with agents, out of
the Parar's reach. The message that enters the live process with background agents is from commit
`d4691c3` (the input stays open while waiting; measured here on 2.1.328, with Haiku: the `init` of its
turn comes out 0.15 s later, with the agent still running). Here there are three fixes:
`claude::VezDaMensagem` (on the thread that reads the output) drops the `result` that arrives before
the turn of the message the process opened with; `motor/saidas.rs` keeps PER CHAT the process that
already finished the turn and is exiting (the one waiting for the suggestion stays in `sugerindo`), and
`comecar_turno_com`, the only place that opens a chat process, does not open another while one of them
lives (`esperar_o_anterior_sair`: with the chat's process still in the map, the one about to open goes
back to the front of the queue instead of swapping the process for another; with one exiting, the
messages stay in `a_abrir`, the chat counts as running and
`abrir_quando_sair` opens the turn when the previous one exits, which takes 0.5 to 1.6 s on Claude
Code, or about 7 s with a background command); and whoever does not exit in time is killed
(`PRAZO_DE_SAIDA`, 12 s; 5 s for one that was stopped or failed; SIGKILL 5 s after SIGTERM). The
process that had already finished and sends the `init` of another turn (a background task notice opens
one by itself) becomes the chat's process again (`Motor::acordou`), except the one that was stopped or
the one with a new turn waiting for it, which is killed. Parar undoes the waiting turn
(`desistir_de_abrir`: what you wrote goes back to the queue) and kills the one still exiting; closing
Vitral kills them all (`encerrar_tudo`). Continuar, "Tentar de novo" and "Continuar na outra conta"
(Continue on the other account) do not step on a running turn or one waiting to open (`com_turno`). In
the fake claude, `[ferramenta AgenteFundo <description> | <seconds>]` is the agent that continues
after the `result` (the process stays alive for it, a message that arrives with the turn stopped
opens another turn in the same process, and SIGTERM with an agent running leaves the mark that makes
the next resumption release the empty `result`, like the real one;
`CLAUDE_FALSO_AGENTE_CORTADO=1` stages it without the cut); the script is
`ferramentas/roteiros/um-chat-um-processo.txt`, which counts the live processes per session at each
step (with 0.9.5, step 06 gives 2).

Cost: the `result`'s `total_cost_usd` is the session total up to that point, not the turn's. It grows
with each `result` of the same process and, on a resumption (`--resume`), already comes with that of
the previous turns, which Claude Code writes in the session transcript. `Chat::somar_custo` keeps the
last total with the session it refers to (`custo_da_sessao`) and adds only the difference to
`custo_usd`; a smaller total (the session without the total recorded, after a crash) counts in full,
and the zeroed one of a process that crashed on opening does not count. Conversations from before
this kept the cost counted too high (each turn's total summed up).

Ultracode has not yet run in a real turn. The first time, check: with `ps -o args` on the child
`claude`, the `--effort ultracode` and the `--settings` with `enableWorkflows`; that the init brings
`Workflow` and the workflow shows up as "Fluxo de agentes" (Agent workflow); that the turn stays
running until the workflow ends and only then finishes; and, in Manual mode (Vitral asks for
permission for `Workflow`), whether the workflow's agents, which run in the background, have been
refusing what would need permission, as the 2.1.278 binary suggests (the `avoid_prompts` layer). If
confirmed, the Ultracode line goes from an amber warning to disabled outside Sem perguntar (Do not ask)
and Automático (Automatic); in Sem perguntar everything runs directly.

`vendor/gpui-component` is a copy of gpui-component 0.8.1 with few changes, all marked with
"Vitral:" in the code: the window's border and shadow follow the theme (`src/window_border.rs`; the
original draws a fixed gray outline); menus, popovers and notices have the pieces' radius
(`radius.lg`, `src/styled.rs`, `src/notification.rs`, and the popover's arrow avoids that corner,
`src/popover.rs`), the tooltip 14 and the keys, the time fields and the focus ring of the check and the
radio have the control one (`src/tooltip.rs`, `src/kbd.rs`, `src/time/time_field.rs`,
`src/checkbox.rs`, `src/radio.rs`, `src/input/group.rs`), and the button comes out as a pill by default
(`src/button/button.rs`); the menus have margin 8 and items with radius 10, measured in rem (they grow
with the zoom), dangerous item, external item, fixed header row and footer outside the keyboard and
edge to edge (the metal plate at the top and the footer strip touch the shell), check on the right, the
separator on the light dot line, indented up to the icon column (and hidden when stuck to the plate or
the strip), the section label in mono 11, the item highlight as a raised key in the `raio::de` corner
of its height (`realce_de_tecla`), a layer under the item for the chosen one's card, disabled at 0.45,
the shortcut in separate keys also on the element item and
the ↓ with nothing highlighted going to the first navigable item, not to the fixed row
(`src/menu/popup_menu.rs`, `src/menu/menu_item.rs`); menus, popovers and notices have the shell
outline (the light, neutral ring, without the earlier blue, the 2 black line outside and the glass edge
inside) and Vitral's floating shadow, and the dialogs their own shadow (`src/styled.rs`,
`src/dialog/dialog.rs`); the tooltip is opaque, with separate keys and "title\ndetail"
(`src/tooltip.rs`); the notice has the icon in the line, the × always visible and a shorter entrance
(`src/notification.rs`); `Kbd` writes Super instead of Win and gives the keys separately
(`src/kbd.rs`); the changed word of the diff is more discreet (`src/diff/mod.rs`); the spinners and the
shimmering text move on a single beat, at 12 frames per second all together
(`src/spinner.rs`, `src/shimmer.rs`); the syntax highlight of a loose snippet is public
(`src/highlighter`); the diff gives the text of a line and opens a line's fold without scrolling
(`src/diff/document.rs`, `src/diff/state.rs`); the radio marks with a small dot (`src/radio.rs`); the
scrollbar appears and disappears abruptly and disappears 1.2 s after last use (`src/theme/mod.rs`,
`SCROLLBAR_*`; `scrollbar_motion` is public for the theme test); the dialogs' veil covers only the
window frame, without the shadow margin (`src/dialog/dialog.rs`); the tooltip does not reopen with the
pointer outside the window (`src/tooltip.rs`); with the window unfocused, the beat does not tick and
returns the phase stopped (`src/spinner.rs`); the resizable dividers do not draw the fixed line, only
the indicator on hover (`src/resizable.rs`); popovers and menus enter with a fade and 6 px from the
trigger's side, in 160 ms at most 60 frames per second (`src/popover.rs`, `entrada_flutuante`, and
`src/menu/context_menu.rs`), and floating elements have the light line at the top (`src/styled.rs`);
the notices animate at most 60 frames per second (`src/notification.rs`); the spinners' beat says
whether it is ticking, so the "running" dot breathes along (`src/spinner.rs`, `compasso_recente`); the
searchable list has the 24 group label with a symbol before it and the row without vertical margin
(`src/command`, `label_prefix`), in mono 11 uppercase with the dash (`heading_row`); the grid
(`trama`, `fio_de_pontos`, `fio_de_pontos_em_pe`, `lado_da_trama`) and the `cantoneira` (corner
bracket) live in `src/styled.rs`; the single key drawing is `desenho_de_teclas` in `src/kbd.rs`, which
the tooltip also uses; and the text field's side margin grows with the zoom (`com_zoom` in
`src/sizing.rs`, used by `Input` and by the field group in `src/input`). When updating gpui-kit, redo
these changes on the new version.
