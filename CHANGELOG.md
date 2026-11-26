# Vitral changelog

The five most recent versions, translated into English. Earlier versions are in
[NOVIDADES.md](NOVIDADES.md) (Portuguese), which is also the source of the release notes. Menu and
button names are quoted as they appear in the app, which is in Portuguese for now, with the English
meaning in parentheses on first use.

## 0.12.0

Six new agents, Vitral in English and Spanish, Tasks in place of the ideas board, and several Codex
accounts.

- **New agents.** GitHub Copilot CLI, Cursor Agent, OpenCode, Qwen Code, Kimi Code and Grok Build,
  over the same protocol as Gemini (ACP). Each one shows up in the model picker when installed, and
  is installed and signed in from Settings, Agents, or from the AIs step of the welcome screen.
- **Languages.** Portuguese, English and Spanish, in Settings, Appearance, and right on the welcome
  screen; with no choice, it follows the system language. The agent also answers in the chosen
  language.
- **Tasks.** The ideas board became Tasks, shown in place of the chats (Ctrl+I): the ideas and every
  chat, each in the lane of its state (waiting for you, working, to review, done), with search,
  filters, and stop, archive and allow right on the card.
- **Accounts.** Several Codex accounts, each with its own sign-in; `/conta` switches the account of
  a Claude or Codex chat; and an account picked by hand also moves to another one at the limit.
- **Review.** You can pick which AI reviews and with which model.
- **Settings and welcome.** The AIs together under Agents, each with Install (Claude Code too); the
  logo menu without the Install items; and the welcome screen with the AIs step and more options.
- **Menus, queue and motion.** Menus in the new design, a redesigned queue, dialogs without the
  light border, the selection that slides in the list and in Settings, and the prototype's timings.
- **Windows.** Pasting a screenshot works (it came in as BMP and was dropped), and backslash paths
  become links.
- **Fixes.** "Pensando" (Thinking) no longer shows twice, one action bar at a time, and a subagent's
  request no longer shrinks in the agents map.

## 0.11.2

Much smoother with a chat working, and branches and the agent pair are more solid.

- **Smoother.** The rings became images, the parts of the window that do not change are no longer
  redrawn every frame, and the animated background is computed separately. With a chat writing, the
  window went from about 22 to about 33 frames per second, with half the GPU work.
- **Branch with Claude.** A branch opened on Codex's turn no longer breaks when Claude comes in, and
  a turn's error now states the reason, not just "error_during_execution".
- **Agent pair.** A message with @ to the other agent waits for the current turn to finish;
  "Negar e explicar" (Deny and explain) reaches Codex and Gemini; and a folder that disappeared or
  an agent that is not installed leave the message in the queue instead of losing it.
- **List.** It narrows again down to quite small without collapsing, and its background is less
  transparent.
- **Details.** The metal also follows the intensity of the accent color (a gray accent makes the
  metal gray), the time in **Nesta conversa** (In this conversation) moved to the corner, and the
  message bar no longer passes over the box.

## 0.11.1

Smoother, with flat menus and a round of fixes in the new style.

- **Smoother.** Animations follow the screen, the mouse wheel scrolls smoothly as in a browser, and
  with a chat working the window no longer stutters: the animated background is much lighter. The
  spinners keep turning when you are in another window.
- **Flat menus.** No gradient and no light border, with groups separated only by space.
- **Fixed list.** It no longer collapses or gets narrower than in the design; focus mode is what
  hides it. Only the selected conversation has a background.
- **Tabs in a narrow pane.** The other tabs stay in the strip, shrinking.
- **Nesta conversa.** A small search above the list, which scrolls without dragging the panel along.
- **Details.** The focus ring inside the message box frame, the context ring identical to the one in
  the bottom bar, discreet scroll bars, and focus mode usage that drags without opening.

## 0.11.0

Vitral has a new look, flat and cleaner, in the style of Linear. The theme colors are now yours,
.md files open as a page you edit in place, and the first launch became a single card.

- **New look.** The list with search and "+" at the top and titled sections; the bottom bar with
  each agent's usage, the context, Revisão (Review), Arquivadas (Archived) and Avisos (Notices);
  tabs, message box, menus and Configurações (Settings) redesigned, with shorter texts. The default
  widths of the list and the right panel are smaller.
- **Focus mode (Ctrl+.).** Only the conversations on screen, with usage floating in a corner, which
  you can drag. Double-clicking a tab maximizes the pane.
- **Theme colors.** In Configurações › Aparência › Cores (Settings, Appearance, Colors): ready-made
  themes (Grafite, Noite, Violeta, Floresta, Brasa and Oceano), accent, background and contrast,
  custom list colors, thin borders, and Importar/Copiar (Import/Copy) the theme. The metal and the
  logo follow the accent color.
- **Animated background.** Smoke, Mesh drift or off, in the theme colors or the recipe's. It moves
  when a chat is working.
- **.md page.** A .md opens as a page of blocks: a click edits the block, "/" picks the type, "# "
  and "- " become heading and list, the handle adds a block below and drags, and the selection has
  bold, italic, code, Mencionar (Mention) and Copiar (Copy). It saves by itself, and whatever you
  did not touch goes back to the file as it was.
- **Right panel.** **Este chat** (This chat) in groups, the Arquivos (Files) tab with the folder
  tree and git marks, Mudanças | Arquivo (Changes | File) with equal lines folded and several files
  open in tabs.
- **Mentioning a snippet.** Select a piece of code and mention only that in the conversation.
- **Review with more options.** When to review, the minimum number of lines, skipping in Planejar
  (Plan), the reviewer's focus, your own instructions, and sending the finding to the agent by
  itself.
- **New first launch**, in a card with the steps at the top.
- **Removed:** agent Profiles and "Enquanto você esteve fora" (While you were away). Accounts now
  live in Configurações › Claude Code.
- **Fixes.** A review cut off by closing Vitral no longer comes back half done; the file editor's
  text is saved whole on close; the message action bar no longer ends up under the bottom text.

## 0.10.3

Codex and Gemini got the same care in Configurações (Settings) as Claude Code: you can install,
update, sign in and out, and see each one's MCP servers. The turn pills went back below the message
field.

- **Codex and Gemini in Settings.** Two new sections after Claude Code, in the same mold: the
  installed version with **Atualizar** (Update), or **Instalar…** (Install) if the program is not on
  the computer, the account with **Entrar…** (Sign in) (and **Sair** (Sign out) on Codex, with a
  confirmation), the list of MCP servers, and the config file with **Abrir** (Open). Update runs
  between turns, waits for the agent's chats to finish and shows what the command answered. The
  Codex entry that was under Agentes (Agents) moved to its own section.
- **Background commands, subagents, steps and changes are back inside the box.** The pills sit
  again under the message field, on the status line, and there is no loose line above the box
  anymore. They keep the click that lists and stops each command, the subagent list that opens each
  one's conversation, and the changes that open "Este chat".
