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
  escolhida nas boas-vindas; **Pasta de trabalho: <nome>…**, no menu do logo, troca), é sempre nela.
  Sem pasta de trabalho: na pasta do chat aberto (o chat Sem pasta abre outro Sem pasta); sem chat
  aberto, na última pasta usada (a do último chat criado, guardada como
  `ultimo_destino` no `interface.json`, ou a do chat mexido por último); sem nenhuma, a paleta
  pergunta. **Novo chat em outra pasta…**, no menu do logo, abre a paleta "Em qual pasta?": as
  pastas que já têm chat (a usada há menos tempo primeiro, com a busca no nome e no caminho), **Outra
  pasta…** (o seletor do sistema) e **Sem pasta**. Quem cria é `Motor::novo_chat(pasta:
  Option<&Path>, …)`; a pasta chega canônica.
- **Claude ou Codex.** Cada chat diz quem responde (`Chat.agente`, em `src/modelo.rs`): o Claude
  Code ou o Codex, a CLI da OpenAI. Escolhe-se no seletor de modelo, que tem as abas **Claude** e **Codex**
  (trocar de aba só mostra os modelos do outro; quem responde muda ao escolher um; a aba do Codex tem
  o padrão dele e os modelos do `codex debug models`, relidos a cada abertura, com a descrição em
  português quando a frase do catálogo é reconhecida, `descricao_do_codex`); o padrão dos chats
  novos é `Config.agente_padrao` (Configurações e boas-vindas). O Codex roda um `codex exec --json`
  por turno, com o pedido pela entrada padrão, e a saída é traduzida para os mesmos eventos do
  Claude Code (`src/agente_codex.rs`, que documenta o formato conferido no 0.169.0): a conversa, a
  trilha e o motor não sabem quem respondeu. O `thread_id` dele fica em `Chat.sessao` e o turno
  seguinte usa `codex exec resume`. O que muda num chat do Codex: não há pedido de permissão no
  meio do turno (o modo vira o sandbox: Planejar é `read-only`, Sem perguntar é
  `danger-full-access`, os outros `workspace-write`), não há conta, comandos de barra nem
  `/compact` (fica o `/clear`, que é do Vitral), o custo e o contexto não são medidos, e parar é
  pelo SIGINT. Trocar o agente de um chat (`Motor::definir_agente`) começa uma sessão nova no
  outro, com as últimas mensagens no bloco de passagem (`MotivoPassagem::TrocouDeAgente`); com um
  turno rodando, não troca.
- **A mesma pergunta para os dois.** **Perguntar também ao Codex** (ou ao Claude), no "…" da caixa,
  manda a mensagem para o chat e para um chat novo do outro agente, na mesma pasta, aberto num
  painel ao lado (`Janela::enviar_aos_dois`, `Motor::abrir_raia`). O chat novo nasce em Planejar,
  que só lê: dois agentes mexendo nos mesmos arquivos se atropelam. O título dele leva o nome de
  quem responde na frente ("Codex · …").
- **Mapa de agentes.** `src/ui/mapa_agentes.rs`: um diálogo com a conversa à esquerda e, ligados
  por um fio, os subagentes que ela abriu (as chamadas Task e Agent, `agentes_do_chat`), com o
  estado (trabalhando, terminou, falhou, interrompido), o tempo e as ferramentas usadas (o
  `Subagente` de `Item::Ferramenta.sub`; as do histórico do Claude Code vêm sem esses números). O
  clique num agente abre os passos e o resultado. Abre pela pílula "N agentes" da linha de estado
  da caixa (só com agente vivo) e por **Agentes desta conversa**, no "…" da caixa. Um agente sem
  resultado só conta como vivo com o turno rodando e sem aviso depois dele na conversa.
- **Mensagem no meio do turno.** O Enviar com o turno rodando manda a mensagem para dentro dele
  (`Motor::dirigir_turno`). Quando o turno termina sem ter absorvido a mensagem, o Claude Code abre
  outro turno com ela no mesmo processo: nesse caso a entrada do processo fica aberta, para o
  turno seguinte também receber mensagens (o `Inicio` dele desfaz o fim adiado; se em 8 s ele não
  começa, `vigiar_turno_dirigido` fecha a entrada e o processo sai). A entrada só fecha no fim
  adiado quando há agentes em segundo plano. `Motor::turno_recebe_mensagem` diz se o Enviar entra
  no turno ou vai para a fila, e **Mandar agora**, na lista da fila
  (`Motor::mandar_da_fila_agora`), tira uma mensagem da fila e a põe no turno que roda.
- **Esforço automático.** No Auto, o seletor diz o nível que vale: o Vitral pergunta ao Claude
  Code (`get_settings`, o `applied.effort`), uma vez por conta e modelo, em segundo plano
  (`claude::atualizar_esforco_padrao`); no Codex vem do catálogo (`default_reasoning_level`).
- **Servidores MCP.** O `/mcp` da caixa e **Servidores MCP**, no menu do logo, abrem a lista dos
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
- **Custo de um redesenho.** A janela é uma vista só: qualquer batida de animação redesenha tudo,
  uns 20 a 30 ms de processador no release (medido em 13/11/2026 com `VITRAL_QUADROS=1` e o tempo
  da thread principal em `/proc`). Por isso o laço do metal e do anel girando bate junto com o
  compasso dos spinners, 12 vezes por segundo (`movimento::laco_a`), e não numa grade própria: as
  duas somavam até 28 redesenhos por segundo com um chat trabalhando. O tamanho da conversa pesa
  pouco nisso (três painéis longos custam 10% a mais que dois curtos); o que baixaria de verdade é
  cada painel virar uma vista própria com cache.
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
  plano; nada disso vai para os dados do Vitral): os 300 últimos itens, com **Carregar mais**, e a
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
  de permissão (`--permission-prompt-tool mcp__vitral__permissao`).
- **Enquanto você esteve fora.** Ao voltar à janela depois de 15 min fora (**Resumo ao voltar
  depois de**), ou ao abrir o Vitral, o topo da lista mostra o que terminou, o que precisa de você e
  o que falhou ou parou desde que você saiu, cada linha abrindo o chat (Ctrl+clique, ao lado), com
  o ×. Fica só na memória: a próxima volta troca, e nada entra no histórico.

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
  pastas dos chats Sem pasta), `exportadas/`, `anexos/`, `migracao.json` e o `formato` dos dados; ao
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
  acima da caixa diz "O último turno falhou" com **Tentar de novo**, e com a janela em segundo plano
  vem a notificação. Nada tenta de novo sozinho.
- **Agentes em segundo plano.** Quando o Claude responde com um fluxo de agentes (ou um subagente)
  ainda rodando, o turno não acaba ali: o chat continua rodando, a conversa ganha o aviso "Os
  agentes seguem trabalhando em segundo plano; o turno termina quando eles acabarem." e o turno só
  termina, com a última resposta, depois que eles acabam (ou quando o processo do Claude Code sai). Se o processo sai com eles ainda rodando, com o teto da espera
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

## Opções da conversa (como na extensão do Claude Code)

A caixa de mensagem é uma moldura dupla (`caixa_mensagem`, em `src/ui/composer.rs`): por fora, uma
moldura preta de canto 20 (14 na Estreita); dentro dela, com 4 de folga, o campo opaco, com o texto
e os chips; embaixo do campo, ainda na moldura, a **linha de estado**, em mono 11. Com o foco, um
aro de metal de 2 px acende em volta da moldura (180 ms; apaga em 120), sem azul nem halo. A barra
de baixo do campo tem chips com ícone, nesta ordem: **modo de permissão** (Manual, com o escudo
cheio, e Planejar em verde; Aceitar edições e Automático em violeta; Sem perguntar em âmbar),
**modelo** (o logo do Claude e o que vale, como "Claude
Opus 5.5"), **esforço** (barrinhas em ciano, de "Auto" a "Máximo") e **conta** (o ícone na cor do
uso de 5 h; só com mais de uma conta). O que segue o padrão da pasta fica em tom tênue. À direita
ficam o anel do contexto (com a fenda embaixo e a porcentagem ao lado, "24%", em mono; âmbar a
70%, vermelho a 90%; na caixa estreita, só o anel; o clique abre o uso da conversa: em cima o
contexto e o custo, "Contexto 31% · US$ 1,84", embaixo o modelo, a conta e os tokens, e no rodapé
**Compactar a conversa**, que prepara o `/compact`), o clipe de anexar (arquivo ou imagem, arquivo
da pasta, conversa anterior e comandos
de barra), o **…** (comandos de barra, compactar, sessão nova e fixar) e o enviar, uma tecla de
metal (`VarianteRedonda::Metal`, o par do "+" da lista), apagado com a caixa vazia, que acende
quando ela ganha texto e dá um pulso claro a cada envio (o pulso fica no botão também quando ele já
virou o parar). Clicar em qualquer ponto vazio da caixa (o vão entre o texto e os chips, a margem,
o espaço entre os chips, a linha de estado) mantém o cursor nela. Com a conversa rodando, ele vira
o parar (o quadrado, com um anel girando em volta); com texto escrito, ficam os dois, e o enviar
entra na fila. A linha de estado, com o chat parado, mostra as teclas: à esquerda "/" comandos e,
com a caixa vazia, "↑" última; à direita "Enter" envia e, com
texto escrito, "Shift Enter" quebra a linha (na caixa estreita, só a direita). Com o chat
trabalhando, ela diz o que ele faz (ver **Faixa do turno**, abaixo). Na última conversa estreita (o
cartão com menos de 1000 px e a direita fechada), a caixa encolhe 32 à direita para os anéis de uso
do canto da janela caberem ao lado dela. Vazia, a caixa diz "Mensagem para este chat…" no chat
com pasta e "Mensagem…" no Sem pasta. **Shift+Tab** troca o modo. O menu do modo lista o que foi permitido nesta conversa: o clique numa regra tira só ela, e
**Voltar a perguntar sobre tudo** tira todas. O **esforço** abre um popover com o nível num
segmentado (←/→, Home e End mudam na hora; o nível escolhido fica na ficha do escolhido, a mesma
dos menus, sem repetir no cabeçalho), com uma linha curta sobre cada nível (Auto: "Usa o
padrão do modelo."), e a chave **Ultrathink**, em arco-íris (Espaço liga; a descrição diz "Só na
próxima mensagem"): vale só para a próxima mensagem, sobe o esforço dela para Extra (o Máximo
continua Máximo) e deixa o chip em arco-íris até enviar. Um comando de barra não a consome. Embaixo
dela fica a chave **Ultracode**, em violeta com o ícone de fluxo (U liga): vale só para a próxima
mensagem, como o Ultrathink, e faz o Claude orquestrar vários agentes em paralelo por um fluxo (a
ferramenta Workflow do Claude Code). A linha avisa em âmbar que gasta bem mais do plano e que roda
no esforço Extra (o Máximo vira Extra nessa mensagem). Ligada, o chip do esforço vira "Ultracode" em
violeta com o ícone de fluxo (com o Ultrathink junto, as barrinhas ficam em arco-íris; na caixa
estreita, só o ícone de fluxo). Começa sempre desligada, também ao abrir o Vitral. Fica
desabilitada, com o motivo curto no lugar da descrição, com um Claude Code anterior à 2.1.278 (a versão em que o Vitral conferiu o Ultracode; o limite de
agentes só existe desde a 2.1.269) e com um modelo sem o esforço Extra (o Haiku, os 4.x até o 4.6).
Numa conta em que um turno com Ultracode mostrou os fluxos de agentes desligados (em Dynamic
workflows no `/config`, por `disableWorkflows` ou pela organização), a linha continua ligável e
avisa em âmbar; esse turno ganha um aviso em vermelho, segue em Extra e a mensagem perde o selo.
Fora de Sem perguntar e Automático, a linha também avisa que comandos e edições dos agentes podem
ser recusados (eles não conseguem pedir permissão). Um Ultracode por vez no Vitral inteiro: com
outro rodando, a linha avisa ("O Ultracode roda em outra conversa: esta mensagem iria sem ele") e a
mensagem vai sem o Ultracode, com o motivo na conversa. Quando a conta chega ao limite e o turno
continua na próxima, a repetição vai sem ele; só o **Tentar de novo** o repete. Fora do Ultracode, a ferramenta
Workflow fica bloqueada: nem um chat nem um "Continue de onde parou" relançam um fluxo sozinhos. O
**modelo** abre um seletor com busca: Padrão (o das Configurações) e as famílias do Claude,
com a versão num selo à direita (a que está rodando, quando a conversa segue a família, ou a que o
apelido pega hoje no Claude Code instalado); o logo do Claude fica só no rótulo do grupo e
o check do escolhido à direita. Seguindo o Padrão, é ele que diz qual está em uso ("Padrão · Opus
5.5"); só uma família escolhida na conversa ganha o selo "em uso". Uma conversa antiga que
guardou o nome completo do modelo ganha a linha dele. Quando a família não é a que está em uso, a
linha dela diz a versão que ela é hoje ("Para os desafios mais difíceis · hoje, Fable 5.1"). Embaixo
das famílias vêm as **versões exatas** (Fable 5.2, Fable 5.1, Opus 5.5 e as outras), com a que está
rodando marcada, inclusive quando a conversa segue a família. A lista vem do Claude Code instalado:
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
primeiro. Mencionar arquivo da pasta (`@`, ou digite `@`) e anexar outra conversa como contexto.
Colar (Ctrl+V) imagem ou arquivos copiados, ou arrastar arquivos do gerenciador para a conversa,
vira anexo. `/` abre os comandos de barra (os do seu Claude Code, com `/clear` para sessão nova e
`/compact`). Passe o mouse numa mensagem para copiar (e, na sua, corrigir e mandar de novo): as ações aparecem numa barra flutuante, à esquerda do seu balão, na altura da última linha
(no canto de cima dele, no painel mínimo), e no canto de cima à direita da resposta; nada fica
reservado nem à vista sem o ponteiro, e nada se mexe quando ela aparece. Os blocos de código ficam
num preto quase puro, como as saídas de comando, e têm o próprio Copiar, numa coluna à direita que
nunca cobre o código; a linguagem fica na dica dele ("Copiar código python"). Modo foco (esconde as
ferramentas, vale por painel).

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
  de uma ferramenta), que abre ao chegar a vez dele, e **Esc** fecha. O
  índice só é refeito quando o termo ou o tamanho da conversa muda, e nada disso anima.
- **Escrever.** Com a caixa vazia, a **seta para cima** traz de volta a sua última mensagem para
  ajustar e mandar de novo, e a conversa volta a antes dela (`EditarUltima`, só no contexto
  "Vazia"; com texto escrito a seta continua movendo o cursor). O **rascunho** de cada conversa (o
  que você escreveu e não enviou) já ficava na memória da janela ao trocar de painel; agora também
  vai para o `interface.json` 1,2 s depois de você parar de digitar, e volta quando o Vitral abrir
  de novo, inclusive depois de uma queda (`src/ui/rascunhos.rs`). A **fila** do que você mandou com
  o turno rodando tem lista própria (`src/ui/fila.rs`), aberta pela pílula da fila (na linha de
  estado da caixa, com o chat trabalhando, ou na faixa acima dela): cada
  mensagem esperando numa linha, com o × que devolve o texto à caixa e, com o chat parado,
  **Entregar agora** no rodapé. **?**, com o foco fora de um campo de texto, abre os **Atalhos do
  teclado** (`src/ui/atalhos.rs`): a tela é montada da mesma tabela que registra as teclas, então
  nunca desencontra do que o teclado faz.
- **Faixa do turno.** O turno em andamento mora na linha de estado da caixa, dentro da moldura
  (`linha_de_estado`, em `src/ui/mensagens.rs`): à esquerda, o anel girando, o que o chat está
  fazendo ("Lendo motor.rs…", "Rodando: cargo test…", com o brilho que corre), o relógio em mm:ss,
  "ação N" e, quando há, o "dirigido" e a pílula da fila; à direita, a tecla **Esc** e "para parar".
  Com a resposta já dada e os agentes de um fluxo ainda rodando, ela diz "Agentes trabalhando em
  segundo plano…". Nas larguras menores saem o
  "ação N", o rótulo do Esc e, na mínima, o relógio. Acima da caixa (`faixa_atividade`), alinhada
  com o texto dela e, à direita, com o enviar, que entra com um fade e 8 px de baixo, só fica o que
  pede um botão seu: **Parada** com **Retomar** (âmbar quando foi o Vitral que parou), **O último
  turno falhou** com **Tentar de novo**, mensagens esperando com **Entregar agora** e a pasta do
  chat que sumiu do disco. Esperando você, ela some (o pedido está logo abaixo).
- **Planejar.** O plano aparece num cartão com a faixa âmbar em cima e o modo em que a conversa
  fica depois de aprovar ("Depois de aprovar: Aceitar edições"). **Aprovar e executar**
  sai do Planejar e volta ao modo de antes (sem modo anterior, o padrão da pasta; se também for
  Planejar, Aceitar edições); a seta ao lado aprova em outro modo (**Aprovar e aceitar edições** ou
  **Aprovar e perguntar antes**). O campo embaixo leva o que mudar: Enter (ou
  **Enviar mudanças**) devolve o plano com o comentário.
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
  antes de permitir.". No rodapé, **Negar e explicar…** (o ícone; o texto vai ao Claude),
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
- **Ferramentas.** Chamadas seguidas viram uma trilha calma, uma linha de 28 por chamada, coladas e
  ligadas por um traço fino: o ícone do tipo (em cor só quando falhou; na que ainda roda, o anel
  girando, `AnelGirando`, no lugar do spinner do kit), o verbo ("Leu", "Editou",
  "Rodou") e o detalhe; a falha tem fundo vermelho claro e "falhou" no fim. Um fluxo de agentes (a
  ferramenta `Workflow`) aparece como **Fluxo de agentes**. A partir de 4 a trilha começa recolhida
  ("6 ações · leu 3 · rodou 2", com "1 falhou" em vermelho); o cabeçalho entra no Tab. O clique numa
  chamada mostra primeiro o que ela pediu, numa caixa em mono: o que um Edit tirou em vermelho e o que
  pôs em verde, o começo do que um Write gravou e o comando inteiro de um Bash (até 60 linhas de cada
  lado ficam guardadas na conversa; as chamadas de antes disto não têm). Embaixo, o resultado (as
  últimas 40 linhas, com Copiar e Mostrar tudo); o de uma edição que deu certo é só "o arquivo foi
  atualizado" e fica de fora. No hover, a linha que leu ou mudou um arquivo que ainda está no disco
  ganha na ponta o ícone **Abrir o arquivo ao lado**. As ferramentas do Vitral que um chat usa
  (`mensagem_para_chat`, `abrir_chat`) têm o ícone e o verbo delas, e no hover a linha ganha
  **Abrir o chat** (o balão de conversa; Ctrl+clique abre ao lado). As das versões de antes (as do
  Mestre e dos ajudantes, nas conversas antigas) continuam com os verbos delas. Fora da largura
  larga, o verbo e a frase vão para a dica (o ícone já diz o que foi) e o título fica com o espaço.
- **Mensagens.** O que você escreve fica num balão neutro à direita (sem avatar, com o reflexo do
  vidro), com as miniaturas das imagens anexadas, o nome e o tamanho dos outros arquivos e os selos
  **Ultrathink** (em arco-íris) e **Ultracode** (em violeta, com o ícone de fluxo) quando a mensagem
  foi com eles; texto com mais de 14 linhas mostra 12 e **Mostrar tudo**. Mensagens que outro chat
  (ou o Vitral) entregou mostram quem mandou (o balão de outro chat ou o logo do Vitral; nas
  conversas antigas, o círculo do Mestre, do coordenador ou a chave inglesa de um ajudante) e uma
  prévia; as de outro chat têm **Abrir o chat**. A prévia termina com reticências antes da coluna da
  seta, nunca cortada no meio da letra; o clique abre o texto inteiro. Os avisos do Vitral são
  marcos centralizados ("Sessão nova", "Turno interrompido"), linhas tênues ("Tentando de novo.")
  ou, quando é erro, um alerta vermelho com Copiar. O que chega com a conversa aberta no painel (a sua
  mensagem, a resposta, um evento, um aviso) entra com um fade e 8 px de baixo, uma vez; abrir a
  conversa, trocar de painel ou rolar não anima nada. Entre turnos ficam 24 px antes do seu balão,
  16 depois dele e 12 entre os blocos de um turno.
- **Rolagem.** Barra de rolagem por painel. No fim, a conversa continua colada no fim também
  quando o painel muda de largura (abrir ao lado, Ctrl+B, zoom, puxar a borda da janela), em que
  as linhas quebram de novo. Qualquer subida com a roda, o touchpad ou a barra solta o fim; aí o
  streaming não puxa a tela de volta e a conversa não conta como lida; a pílula com a seta (e
  "Novas mensagens", com um ponto azul, quando chegou algo), que entra com um fade, volta ao fim,
  como o **Ctrl+End** (descendo, os últimos 48 px já prendem de novo). Abrir um bloco no fim da
  conversa (a saída de uma ferramenta) não pula para o fim dele: o
  painel fica onde está.
- Mensagens, pedidos e caixa ficam numa coluna de leitura centrada (até 860 px, com 16 de margem).
  Em janela baixa, o plano e os pedidos encolhem e rolam, e a caixa cresce menos. Blocos de código
  têm cor de sintaxe.


## O que fica à vista

O padrão é o mínimo, e o resto aparece quando faz falta:

- **O alto da lista é uma linha só**: o logo (o menu do Vitral) e as abas **Conversas | Arquivos**,
  na mesma altura (40), num segmentado de largura toda (`topo_enxuto`, em `src/ui/trilho.rs`).
  Arquivos mostra a pasta do chat aberto.
- **A busca e o "+" moram no pé da lista**, embaixo das Arquivadas, dos agentes rodando e do sino,
  também na altura de 40. O recolher da lista (Ctrl+B) mora no menu do logo, **Recolher a lista**.
- **A lista é corrida**, sem título nem os das seções: o que espera você, o que roda, as fixadas
  (com o alfinete) e todos os outros, nessa ordem. Cada chat é uma linha só de 32 (o balão neutro, o
  título e a hora em mono; a pasta vai na dica do balão); a linha de baixo volta na busca que achou
  no texto, com o nome da pasta na frente, e o resultado da busca usa o mesmo balão pequeno das
  outras linhas.
- **Novo chat não pergunta nada.** O "+" e o Ctrl+N abrem um chat na hora, na pasta do chat aberto
  (ver **Chat novo**, em **Como funciona**). Escolher a pasta fica no menu do logo, **Novo chat em
  outra pasta…**: as pastas que já têm chat (com o caminho embaixo do nome), **Outra pasta…**, que
  escolhe uma no disco, e **Sem pasta**.
- **Os anéis de uso** flutuam no canto de baixo, à direita, da janela, um em cima do outro, sem
  cápsula atrás (`uso_no_canto`, em `src/ui/uso_plano.rs`): o do Claude sempre e o do Codex
  quando ele está instalado e tem uso lido. **Mostrar o uso do plano no canto da janela**, em
  Configurações > Chats
  (`Config.uso_no_canto`, ligada por padrão), os esconde; o uso continua em Contas do Claude.
- **O cabeçalho da conversa tem pouca coisa:** as abas, o modo foco (só ligado) e o **⋯**. Abrir e fechar o painel da direita mora no **⋯** do último painel,
  com o número do que ele espera no canto do **⋯**.
- **O chip da conta**, na caixa, só aparece com mais de uma conta.
- **O × do canto é o da janela.** Com o painel da direita aberto, o cabeçalho dele tem o botão de
  fechar o painel (o ícone de recolher), um traço e então os botões da janela; o × acende em
  vermelho e a dica diz "Fechar o Vitral".

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
  e quase opacos (5% do metal passa). Margem da janela e vão entre os cartões: 8; canto dos
  cartões: 16 (`margem`, `vao` e `raio_peca` da variante). Menus e popovers são praticamente
  opacos. Texto nunca vai no metal cru: os alfas
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
  `.cached()` (a dica do kit usa): a textura troca a cada batida. No Windows o renderizador do
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
  medição: no código, nas horas da lista, nas teclas, na `etiqueta()` (mono 11/14, dos fatos e
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

**Arquivos citados** (`src/ui/links_arquivo.rs`). Na resposta do assistente, o arquivo que está no
disco vira link: `produtos/views.py`, `src/motor.rs:120` entre crases, o link do markdown para um
caminho
(`[a janela](src/ui/mod.rs#L120)`) e, no texto corrido, a palavra com barra e extensão. O clique
abre o arquivo aqui, rolado até a linha, que fica marcada; uma imagem abre no visor. O caminho
relativo parte da pasta do chat. Só abre o que está numa pasta que o Vitral conhece (a de algum
chat); o resto avisa e fica para o VS Code.

**Este chat** (`src/ui/este_chat.rs`): onde o chat trabalha. Diz se ele mexe direto na pasta, se é
um chat Sem pasta (os arquivos que ele criar ficam na pasta só dele) ou, numa conversa trazida do
Claude Code, de onde ela veio, as datas e quantas mensagens tinha; mostra a pasta (o caminho
inteiro na dica, com copiar) e **Abrir pasta**. Com a pasta sumida do disco (disco desmontado,
pasta renomeada), um alerta vermelho no lugar do Abrir. Na dica da aba, a pasta e o número do
painel. A outra aba é a do arquivo aberto (ver **Arquivos citados**, acima, e a aba Arquivos da
lista).

- Num painel estreito (abaixo de uns 330 px, ou a 150% de zoom), os botões ficam só com o ícone.
- Na gaveta (janela estreita), o painel é opaco por cima das conversas, com o fundo escurecido; ele
  entra deslizando da borda direita, e o botão do canto (ou Esc) o recolhe.

## Contas e uso dos planos

No canto de baixo, à direita, da janela flutua um anel de 30 por serviço, um em cima do outro
(`uso_no_canto`, em `src/ui/uso_plano.rs`): o do Claude sempre e o do Codex quando ele está
instalado e tem uso lido. Cada um é o disco da marca com **um** anel em volta (com a fenda
embaixo), que mostra a janela mais apertada das duas (5 h ou semana), na cor do serviço até 69%,
âmbar de 70 a 89% e vermelho daí para cima; no limite, o anel fica cheio em vermelho. As duas
porcentagens ficam na dica e no popover. Com mais de uma conta, o do Claude leva a inicial da
conta em uso. **Mostrar o uso do plano no canto da janela**, em Configurações > Chats
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

No painel do Claude também se escolhe a conta padrão, em **Conta padrão**: clicar numa
linha (**Automática**, "a de menor uso em cada turno", ou uma conta, com o disco da marca e o
anel da janela mais apertada, como no canto, o e-mail inteiro em cima e o uso
embaixo, "5 h 7% · semana 38%", sem a janela que ainda não foi lida) a torna a padrão, e o check
fica nela; **Entrar com outra conta…** conecta uma nova e o **…** de uma conta extra tem
**Desconectar…**. **Contas…**, no rodapé, abre o diálogo das contas (também no menu do logo,
**Contas do Claude**, ou em **Gerenciar contas…**, no chip da conta na caixa de mensagem). Ele
repete o seletor do popover, maior: em **Conta padrão**, a Automática e uma linha por conta com o
mesmo anel do canto (a janela mais apertada), o login e o plano, o uso ("5 h 23% ·
semana 41%", com a hora de renovar na dica, ou "no limite · volta às 14:30") e o check na padrão; o
clique numa linha a torna a padrão. O **…** de cada conta tem **Usar como padrão**,
**Copiar o e-mail** e, nas extras, **Desconectar…**. **Entrar com outra conta…** mostra dois passos:
o navegador (com **Abrir de novo** e **Copiar endereço**, que vira "Copiado" por um instante) e o
código da página, com **Entrar**; se falhar, o erro aparece ali com **Tentar de novo**.

Cada conta extra tem uma pasta própria só com o login; CLAUDE.md, configurações, skills, plugins
e o histórico das conversas são compartilhados com o seu Claude Code, então uma conversa continua
em qualquer conta. Na escolha **Automática**, cada turno vai para a conta com mais folga e, se uma
chegar ao limite, o mesmo turno roda de novo em outra. A conta no limite volta sozinha na hora em
que o plano renova (o painel de uso e o de contas dizem quando: "limite atingido, volta às
15:42"); uma janela que já renovou volta a 0%. Se todas estiverem no limite, o turno vai para a
que volta primeiro, e a conversa avisa a hora. Fora do painel, o uso aparece igual em todo lugar:
"7% do limite de 5 h".

## Organização

- **O alto da lista** (a área vazia arrasta a janela, o clique duplo maximiza e o clique direito
  abre o menu da janela): o logo, no quadrado de metal (clique ou F10 para o **menu do Vitral**, que
  abre à direita: no topo, a placa de metal com o logo, "Vitral" e quantos chats há, com a pasta de
  dados na dica; depois **Recolher a lista** (Ctrl+B), **Novo chat**, **Novo chat em outra pasta…**
  e **Importar do Claude Code…**; **Contas do Claude**, **Instalar o Codex…** quando ele falta,
  **Configurações** e **Atalhos do teclado**; **Abrir a pasta de dados** ↗; com os atalhos de cada
  um em teclas; no rodapé, a faixa escura com a linha técnica em mono, "Vitral 0.7.0 · a1b2c3d" (a
  versão e o commit curto, ou o canal no compilado no PC), e a origem na dica; com uma versão nova
  pronta, um ponto azul de 8 no canto do logo, a versão na dica e **Reiniciar para a 0.3.0** logo
  acima das Configurações). Ao lado do logo, as abas **Conversas | Arquivos** (ver **O que fica à
  vista**).
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
  rodapé ficam na mesma guia, a 16 da borda. Ctrl+G vai direto à próxima pendência.
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
  completa; as outras são compactas (até 180 de largura), com o título e o caminho na dica. Toda
  aba tem fundo, e com o ponteiro em cima ganha um aro de metal. Na largura mínima, só a aba à
  vista. As abas não são lembradas ao fechar o app.
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
  painel) ativa o painel e põe o cursor na caixa dele. O × fica dentro da aba, no lugar do ponto de
  estado (azul rodando, âmbar pedido ou parada, vermelho erro): aparece no hover, no foco e sempre
  no painel ativo; fechar pelo × ou pelo clique do meio põe o cursor na caixa do painel que fica,
  como o Ctrl+W. A aba entra no Tab: Enter vai para a caixa, Delete fecha e Shift+F10 abre o menu.
  As divisórias não têm linha fixa: o indicador aparece ao passar o mouse, e é só arrastar para
  redimensionar; **dois cliques** na divisória entre duas conversas deixam todas com a mesma
  largura (`igualar_conversas`, em `src/ui/mod.rs`). A lista da esquerda recolhe pelo menu do logo
  (**Recolher a lista**) ou pelo Ctrl+B, e no arrasto vai de 200 a 560 px (`FAIXA_LISTA`).
  Ctrl+PgUp/PgDn vão para o painel vizinho e Ctrl+Shift+PgUp/PgDn trocam o painel de lugar.
- **Cabeçalho do painel.** Uma linha só (44): as abas, o modo foco (só ligado) e o **⋯** (**Mais
  opções da conversa**, o mesmo menu do clique direito na aba, como diz a dica). O botão da direita
  saiu do cabeçalho: no último painel, abrir e fechar a direita é o item do **⋯**, **Mostrar Este
  chat** (ou **Mostrar o arquivo aberto**), marcado quando ela está aberta, com o Ctrl+J. Com a
  direita fora da vista (fechada, ou a gaveta dela fechada) e chats que pedem você, o **⋯** leva no
  canto um selo com o número, âmbar, ou vermelho se um caiu, e o item mostra o mesmo número tingido.
  Ligados, ficam num cinza mais claro, nunca azul. Na aba, o balão do chat, com a pasta na dica
  ("Direto em ~/Projetos/loja", "Sem pasta: um chat numa pasta vazia só dele"). O **⋯** tem **Modo
  foco**, o item da direita (no último painel), **Nova conversa ao lado**, **Mover para a
  esquerda/direita**, **Fechar os outros painéis**, **Fechar este painel** e o menu da conversa
  (renomear, fixar, arquivar; o "Abrir no VS Code" e o "Abrir no terminal" saíram dele). O custo ("US$ 0,42") está no anel do contexto
  da caixa. Não há linha entre o cabeçalho e as mensagens, nem com a conversa rolada. Em painel
  estreito o modo foco vai para o **⋯**; no mais estreito, com vários painéis, o número toma o
  lugar do símbolo da aba. Nenhum texto é cortado no meio.
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
  mesma cor de sintaxe; e os anéis de uso ficam sobre um disco preto, para a cor de cada marca
  saltar. Dentro de um popover, o que no vidro é um recuo escuro (o trilho do segmentado do
  esforço) vira um claro leve. O logo (o V sobre o quadrado de metal; ver
  **Acabamento**) vai no tamanho completo (`assets/vitral.svg` e os PNGs de 64 a 256 px) no menu de
  aplicativos e na tela de recuperação, e no pequeno (`assets/vitral-pequeno.svg`, declarado em 32
  px, e o PNG de 48) nos ícones pequenos. Popovers e menus entram com um fade e 6 px
  vindos do lado do botão que os abriu, e todos os flutuantes têm uma linha de luz de 1 px no topo,
  como o reflexo de um vidro; as gavetas da lista e da direita entram deslizando 24 px da borda, em
  180 ms, com o véu em fade junto, e fecham secas. O véu dos diálogos cobre só a janela, não a
  margem da sombra. A pílula do segmentado (Conversas · Arquivos, o esforço) desliza até a
  opção nova; os botões redondos (enviar, +, sino) acendem suave com o ponteiro em cima; os anéis de
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
  **Arquivadas** (com o número) e com o cartão tênue. Fora da busca, elas continuam só em
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
  título, a hora em mono, o alfinete da fixada, o número do painel e o ponto de não lido; a pasta na
  dica do balão); a linha de baixo, com a prévia, volta só na busca que achou no texto. O ponto:
  vermelho erro, âmbar pedido ou parada, azul resposta nova, aro azul rodando. No hover, o horário
  dá lugar a duas ações: **Abrir ao lado** (ou Retomar) e **…**; fora do hover, o título usa o lugar
  delas, e só o cartão sob o ponteiro (ou realçado pelo teclado) corta o título para mostrá-las. O
  clique direito ou o **…** abrem o menu do chat: abrir ao lado, renomear, fixar e arquivar. O
  clique simples abre o chat numa aba do painel ativo; clique duplo renomeia; Ctrl+clique ou o
  clique do meio abrem ao lado.
- **Teclado na lista**: ↓ na busca entra na lista; ↑/↓, Home e End andam pelos cartões; Enter abre
  (e, com pedido, põe o cursor na caixa), Ctrl+Enter abre ao lado, F2 renomeia e Esc volta à busca.
  Na busca, Esc limpa e, vazia, volta à caixa do painel.
- **Rodapé**, só quando há o que mostrar: **Arquivadas** (o ícone com o número; liga e desliga a
  vista das arquivadas; a busca acha as duas) e, com algo rodando, quantos agentes rodam no Vitral
  inteiro ("2 rodando"; o clique volta ao topo da lista). Na aba Arquivos, quantos arquivos a pasta tem.
- Aba **Arquivos**: os arquivos da pasta do chat aberto numa árvore (pastas recolhidas, menos o
  caminho até o arquivo aberto), com o caminho dela em cima e o ↻ para reler. Com busca, uma lista
  plana sem limite, com o trecho achado em destaque e a pasta cortada no meio. Clicar abre no painel
  da direita: a aba com o nome e o ícone do tipo, e uma barra com a pasta, o número de linhas e a
  linguagem, **Mencionar**, **VS Code** e o **…** (copiar o conteúdo ou o caminho, mostrar na pasta).
  O markdown sai renderizado; o código, com cor e número de linha, rola para os lados. Arquivo grande
  (mais de 400 KB), binário, vazio ou que sumiu dá um aviso no lugar, com **Abrir no VS Code** ou
  **Fechar**. Na lista, no hover, **@** menciona o arquivo na conversa e **…** (ou o
  clique direito) abre no VS Code, copia o caminho ou mostra na pasta. A lista é relida a cada 20 s
  em segundo plano, sem travar a tela.
- **Configurações** (Ctrl+,), gravando na hora, valem para todos os chats (o caminho do
  `config.json` fica na dica do subtítulo), em quatro grupos: **Chats** (**Resumo ao voltar depois
  de**, **Mostrar o uso do plano no canto da janela**, ligada por padrão, o modo de permissão
  padrão, o modelo dos chats, **Arquivar chats parados depois de**: nunca, 3, 7 ou 30 dias, só os
  chats já lidos e não fixados e sem fila, e **Servidores MCP do Claude Code**, os do seu Claude
  Code nos chats), **Interface**, **Claude Code** (a versão instalada, a que os turnos abrem, e
  **Atualizar**, que roda `claude update` fora de qualquer turno, espera os chats que estão
  trabalhando ou com mensagem na fila e mostra o que ele respondeu, em âmbar se falhar; a versão
  nova vale no turno seguinte, sem reiniciar o Vitral) e **Atualizações**. Em Interface,
  **Animações** (Seguir o GNOME ou Desligadas) vale para o app todo e muda na hora; seguindo um
  GNOME sem animações, a linha diz isso.
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
  baixo do texto.
- **Diálogos** opacos, com cantos de 20, o título grande e o × com a dica; o rodapé fica sempre à
  vista e só o meio rola. **Mencionar arquivo** e **Conversa como contexto** são listas com busca
  e teclado (os arquivos são lidos em segundo plano, com o ícone do tipo e a pasta embaixo; as
  conversas, da mais recente para a mais antiga, com quando falaram, quantas mensagens têm e a
  pasta). **Renomear** já abre com o
  título selecionado e não salva vazio.
- Notificação do sistema, só com a janela em segundo plano, quando um chat termina, quando um chat
  pede permissão ou quando um chat cai: com o ícone do Vitral, o título do chat e uma linha só
  ("Precisa da sua permissão: Bash, $ cargo test", "Terminou: …", "Parou com erro: ..."). Clicar nela abre
  a conversa no painel ativo e traz a janela para a frente (ou o GNOME avisa que o Vitral está
  pronto). Uma notificação nova do mesmo chat substitui a anterior em vez de empilhar. A permissão se
  responde dentro do app, nunca na notificação.

Atalhos: `Ctrl+N` novo chat (na hora, na pasta do chat aberto), `Ctrl+K` buscar (`↓` entra na
lista; nela, `Enter` abre, `Ctrl+Enter` abre ao lado e `F2` renomeia), `Ctrl+J` painel da direita,
`Ctrl+B` recolhe ou mostra a lista, `Alt+1…4` painel N, `F10` menu do Vitral, `Ctrl+,`
configurações, `Ctrl+G` próxima pendência, `Ctrl+W` fecha a aba à vista (na última, o painel),
`Ctrl+PgUp/PgDn` painel vizinho, `Ctrl+Shift+PgUp/PgDn` muda o painel de lugar (na aba: `Enter` vai
para a caixa, `Delete` fecha, `Shift+F10` abre o menu), `Ctrl+End` vai ao fim da conversa, `Ctrl+F`
busca na conversa (`F3`/`Shift+F3` andam), `Shift+Tab` troca o modo, `Esc` para o turno (ou fecha a
gaveta), `Enter` envia, `Shift+Enter` quebra a linha, `Ctrl+roda`, `Ctrl+=` e `Ctrl+-` zoom,
`Ctrl+0` zoom normal. `Ctrl+Shift+M` (modelo) e `Ctrl+E` (esforço) abrem e fecham os popovers da
caixa do painel ativo (no do esforço, ←/→, Home e End mudam o nível, Espaço liga o Ultrathink, U
liga o Ultracode e Enter fecha (as teclas estão no rodapé do popover); Esc fecha sem parar a
conversa). `?` abre os **Atalhos do teclado**.

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
troca o papel de parede (com o metal opaco, ele só aparece fora da janela) e `ZOOM=1.5` abre com
esse zoom. `DADOS_NOVOS=1` abre como primeiro uso
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
`[demora]`; com `[ferramenta <Nome> <detalhe>]` (Read, Edit, Write, Bash, Grep, Glob, WebFetch,
Task…) finge usar a ferramenta, mandando o `tool_use` (o Edit e o Write com um trecho de mentira,
para a linha da ferramenta ter o que mostrar ao abrir), esperando 4 s (ou os segundos de
`[ferramenta … | 8]`) e mandando o `tool_result`, para dar de ver a linha do turno ("Lendo
motor.rs…", "Rodando: cargo test…") sem gastar a API; o `[ferramenta Pensa <texto> | 3]` encena um
bloco de raciocínio que dura os segundos pedidos, e o `Task` (ou `Agent`) encena um subagente, com
três ferramentas e uma frase presas à chamada dele (`parent_tool_use_id`); ao lado dele mora o
`ferramentas/claude-falso/codex`, o Codex de mentira (fala o `codex exec --json`, entende o mesmo
`[ferramenta …]`, `[falha]` termina o turno com erro, `codex debug models` dá um catálogo pequeno e
os argumentos ficam em `$XDG_STATE_HOME/codex-falso.log`); executa os `[mcp <nome> <json>]` da mensagem
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
janela estreita e as gavetas, e `4c-zoom-lista-estreita.txt` estreita a lista); no meio de um
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

`VITRAL_QUADROS=1` escreve "quadros/s: N" no stderr a cada segundo em que a janela redesenhou
(nada em repouso): é a conferência do custo dos efeitos. Quando o metal refez o campo nesse
segundo, a linha leva o tempo médio de um campo: "quadros/s: 18 · campo: 2,1 ms".
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
dela) e a configuração em `%APPDATA%\vitral`. As fontes vão dentro do binário nos dois sistemas. A
janela pede o fundo acrílico do sistema (`VITRAL_FUNDO=mica`, `transparente` ou `opaco` troca), mas
ele fica debaixo da chapa de metal, que é opaca, e não aparece mais; o
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
| `src/motor/conversas.rs` | O que liga as conversas umas às outras e ao usuário: `listar_chats`, `mensagem_para_chat` e `abrir_chat`, o freio de ciclo e o resumo do que aconteceu enquanto você esteve fora |
| `src/motor/importacao.rs` | A importação no motor: um chat direto na pasta de cada conversa do Claude Code, a da primeira abertura, a do menu, a vigia (a cada abertura e a cada 2 minutos, só o que mudou desde a passada anterior) e a marca de quando ela continua no Vitral |
| `src/dados.rs` | Pasta de dados global (`~/.local/share/vitral`, a configuração em `~/.config/vitral`): caminhos, gravação que aguenta queda de energia (temporário com fsync, `.anterior` por link duro, rename e fsync da pasta), o gravador em segundo plano (que guarda a gravação que falhou para tentar de novo), `carregar` com volta ao `.anterior` e a guarda do `estado.versao-N.json`, a `normalizar` (os dados das versões com projetos: só chats, cada um com a pasta dele), o `formato` dos dados e a `DADOS_GLOBAIS` |
| `src/migracao.rs` | Migração do Vitral antigo (o `.vitral/` de cada pasta de trabalho) para os dados globais: uma vez só, no primeiro uso, sem escrever nada nas pastas antigas, com o `estado.json` gravado por último; adiada com o Vitral antigo aberto |
| `src/instancia.rs` | Um Vitral só por usuário: a trava (`flock` em `~/.local/share/vitral.trava`), o socket (`$XDG_RUNTIME_DIR/vitral/vitral.sock`, pasta 0700) por onde uma segunda abertura entrega o pedido e sai, a `Abertura` (`vitral <pasta>`, `--nova <pasta>`) e a reexecução da tela de recuperação |
| `src/importar.rs` | Leitura das conversas do Claude Code (só leitura): a listagem com título, datas, mensagens e se está aberta (o `Filtro.desde` da vigia pula o que não mudou), o histórico como itens da conversa e o registro do que já veio |
| `src/claude.rs` | Processo do Claude Code em `stream-json` (argumentos e ambiente do turno), leitura dos eventos, versão do Claude Code, conferência da sessão, os modelos que ele oferece (o `initialize`, em `atualizar_modelos_oferecidos`) e o detalhe de cada chamada de ferramenta (`detalhe_ferramenta`: o diff do Edit, o começo do Write, o comando do Bash, até 60 linhas de cada lado) |
| `src/mcp.rs` | Servidor MCP local com as ferramentas do Vitral (`listar_chats`, `mensagem_para_chat`, `abrir_chat` e `permissao`), uma rota por chat com o token da sessão |
| `src/prompts.rs` | O texto que o Vitral põe no prompt de sistema de cada chat: onde ele trabalha (direto na pasta, Sem pasta ou continuando uma conversa do Claude Code) e as convenções `[de: …]` e `[Vitral]` |
| `src/processos/` | Todo processo que o Vitral abre (`novo_comando`, sem janela de console no Windows) e os que ele para por inteiro: escopos do systemd ou grupos de processos (`unix.rs`), Job Objects (`windows.rs`) |
| `src/sistema/` | O que abre fora do Vitral, nos dois sistemas: editor, terminal, gerenciador de arquivos, o bash dos comandos (no Windows, o do Git para Windows, nunca o do WSL) e o aviso do que falta instalar (`linux.rs`, `windows.rs`) |
| `src/contas.rs`, `src/contas/` | Contas extras (`CLAUDE_CONFIG_DIR`), login e o uso e o plano (o que o Claude Code guarda em `.claude.json` e a busca direta na Anthropic, `busca.rs`) |
| `src/codex.rs` | Uso do plano do Codex, lido das sessões em `~/.codex/sessions` |
| `src/agente_codex.rs` | O Codex como agente de um chat: os argumentos do `codex exec`, a tradução da saída para os eventos do Claude Code, o sandbox de cada modo e o catálogo de modelos |
| `src/git.rs` | O comando do git (`novo_git`, o único jeito de abrir o git), o caminho relativo em texto e de onde saiu uma cópia isolada das versões de antes (para a carga pôr o chat de volta na origem) |
| `src/modelo.rs` | Os tipos do que vai para o disco (chats, ocorrências, filas e a configuração global, com os nomes antigos como alias), os tipos só lidos das versões com projetos (`ProjetoAntigo`, `TarefaAntiga`, `PapelAntigo`) e o `FORMATO_DADOS` e o `VERSAO_ESTADO` |
| `src/atualizacao/` | Versão embutida e os comandos sem janela (`--versao [--json]`, `--instalar`), o estado da atualização para a interface (`mod.rs`), a thread e o ciclo (`procura.rs`), o manifesto (`manifesto.rs`), a assinatura (`assinatura.rs`), o download (`rede.rs`), o pacote e a conferência do binário novo (`pacote.rs`), a instalação por versão e o menu no Linux (`linux.rs`), a instalação por versão, o `versoes.txt` e o lançador no Windows (`windows.rs`), o `atualizacoes.json` (`config.rs`) e a saúde da versão nova com a volta para a anterior (`saude.rs`); as fixtures ficam em `teste/` (`teste/ponta/` é o release falso dos testes de ponta a ponta, assinado com outra chave de teste) |
| `src/tema.rs` | Tokens da linguagem visual (cores do vidro e das partes pretas, medidas em rem com `r()` e `tam()`, tipografia, sombras e gradientes), tema do GPUI Kit, cores de sintaxe e ícones extras do Lucide |
| `src/ui/` | O alto da lista (`trilho.rs`: o logo, o menu do Vitral e o sino), a lista (`lateral.rs`), os painéis (`conversa.rs` é a casca: as abas, o cabeçalho, o menu do painel, a doca e o popover de uso da conversa; `mensagens.rs` o que vai dentro, com o cartão de pedido de permissão e a linha de estado da caixa, e `mensagens/historico.rs` o histórico das conversas trazidas do Claude Code, lido em segundo plano, com Carregar mais; `paineis.rs` os painéis lado a lado, as abas de cada um e a soltura; `arranjo.rs` como a janela estava ao fechar), `links_arquivo.rs` (os caminhos de arquivo na resposta que viram links e abrem no painel da direita, na linha citada), `composer.rs` (a caixa de mensagem) e `composer_seletores.rs` (os chips, menus e popovers dela), `menus.rs` (o menu da conversa, igual em todo lugar), `dialogos.rs` (Novo chat e a paleta das pastas, renomear, contas, configurações, mencionar arquivo e conversa como contexto), `este_chat.rs` (a direita: onde o chat trabalha), `abas_direita.rs` (as abas da direita), `painel_arquivo.rs` (o arquivo aberto à direita), `caixa_entrada.rs` (o popover do sino e o Ctrl+G), `resumo.rs` (o freio de ciclo em Esperando você e o Enquanto você esteve fora), `abertura.rs` (o que fazer com a pasta e o `--nova` de uma abertura, ao abrir e pelo socket), `recuperacao.rs` (a tela de quando os dados abrem só para leitura, com Começar do zero e Tentar de novo), `uso_plano.rs` (os anéis de uso no canto da janela e o popover deles), `atualizacao.rs` (a atualização: avisos, Novidades, o grupo das Configurações e os textos de cada estado), `fechar.rs` (a pergunta antes de fechar ou reiniciar), `busca_conversa.rs` (o Ctrl+F dentro da conversa: a barra, o índice das ocorrências e o realce), `fila.rs` (a lista das mensagens que esperam o turno, com o Entregar agora), `reenvio.rs` (corrigir e mandar de novo a última mensagem), `rascunhos.rs` (o que você escreveu e não enviou, guardado por conversa no `interface.json`), `imagens.rs` e `visor_imagem.rs` (as imagens do chat e o visor), `seletor_arquivos.rs` (os arquivos da pasta para mencionar com `@`), `atalhos.rs` (a tabela única das teclas, que vira os `KeyBinding` e o diálogo "Atalhos do teclado" do `?`), `zoom.rs`, `movimento.rs` (a única porta de animação: durações, curvas, teto de 60 quadros por segundo, entradas e transições, o laço do metal e o `Segurar` do pedido perigoso) |
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
barra `paint_image(` e `drop_image(` fora de `src/ui/componentes/metal.rs`.

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
ponteiro num controle solto; `abertura(id)` e `deslizar_gaveta(el, De::Esquerda | De::Direita, t)`
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
fora da vista só redesenha no primeiro pedaço (`juntar_parcial`, com teste). `salvar_estado` e
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
