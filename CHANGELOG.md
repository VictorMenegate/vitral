# Vitral changelog

The five most recent versions, translated into English. Earlier versions are in
[NOVIDADES.md](NOVIDADES.md) (Portuguese), which is also the source of the release notes. Menu and
button names are quoted as they appear in the app, which is in Portuguese for now, with the English
meaning in parentheses on first use.

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

## 0.10.2

Codex is now almost as complete as Claude in Vitral: it asks for permission mid-turn through the
same card, has `/compact`, `/review` and skills in the "/" menu, shows context, tokens and steps,
and can fix a message or continue from here in another chat without a summary. The diff gained
per-line comments, the "/" menu shows everything, and there are new shortcuts to move between
messages, maximize the pane, move tabs and archive.

- **Shortcuts to move and organize.** Alt+Up and Alt+Down jump to your previous and next message;
  PgUp and PgDn scroll the conversation (in a box with text they still belong to the text).
  Ctrl+Shift+Enter maximizes the pane and goes back (Esc also goes back), Alt+PgUp and Alt+PgDn
  move the tab to the neighboring pane, Alt+Shift+PgUp and Alt+Shift+PgDn open a pane beside it
  with the tab, and Ctrl+Alt+A archives the pane's chat, with Ctrl+Z undoing while the notice is on
  screen. All of them are in Atalhos do teclado (Keyboard shortcuts) and in the palette.
- **Mark as unread.** The chat menu brings back the blue dot, and it stays until you open the chat
  again.
- **Search in the conversation no longer sits under the rings.** With the pane against the right
  edge of the window, the Ctrl+F bar leaves room for the usage rings, whether there are one, two,
  three or none.
- **Per-line comment in the diff.** In the agent's edit, click a line's margin and write the
  comment. They accumulate, stay marked on the lines, and **Enviar comentários (N)** (Send comments)
  sends all of them to the agent at once, with the file and line of each.
- **Pasting HTML becomes Markdown.** What you copy from a browser or document enters the box with
  links, lists, bold and code; Ctrl+Z goes back to plain text. On Linux with Wayland.
- **The "/" menu shows everything.** All commands and skills in a scrolling list, each with its
  description, argument hint, skill badge and origin; the icon (or Ctrl+O) opens the skill file on
  the right, and the best matches for what you typed come first.
- **"@" and "/" where the cursor is.** The menu opens for the word you are on, not only the last
  one, and completes there. A path with spaces goes in quotes.
- **Narrow box with the model name.** With three panes side by side, the chip shows "Opus" or
  "Fable", and mode and effort keep only the icon.
- **Unused pasted images clean themselves up.** On start, Vitral deletes pasted images that no
  conversation, queue or draft uses and that have been idle for more than a day.
- **Codex asks for permission.** In the Manual, Aceitar edições (Accept edits), Planejar (Plan) and
  Automático (Automatic) modes, Codex asks mid-turn through the same card as Claude, with the full
  command or the file diff, Negar (Deny) and Permitir (Allow). Before, those modes became only its
  sandbox. "Permitir sempre" (Always allow) applies to Codex too, and Accept edits lets edits inside
  the folder through.
- **Slash commands in Codex.** `/compact` summarizes the Codex conversation (also through Compactar),
  `/review` asks for a review of the changes (or of whatever is written after it), and the Codex
  skills in the folder show up in the "/" menu.
- **Codex context and tokens.** The context meter also appears in a Codex chat, with the model's
  window, and the "Trabalhou" (Worked) line says how many tokens the turn used. Codex does not report
  the cost in dollars, so the popover shows only context and tokens.
- **Codex steps.** The plan Codex keeps appears like Claude's steps: the "1/3" pill above the box
  and the lines in the trail.
- **Fix and continue from here in Codex, without a summary.** Fixing a message or continuing the
  conversation in another chat takes Codex exactly to the chosen point, as it already did with
  Claude, instead of starting a new conversation with a summary of the last messages.
