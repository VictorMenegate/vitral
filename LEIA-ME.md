# Vitral

> A porta de entrada do projeto é o [README.md](README.md) (o que o Vitral é, como instalar, como
> conferir o que baixou). Este arquivo é o manual de dentro: como cada parte funciona, o que já foi
> decidido e as pegadinhas.

Ambiente para trabalhar com o Claude Code em vários projetos ao mesmo tempo: um **Mestre** que
entende o pedido e o manda para o projeto certo, e **chats de projeto** que fazem o trabalho, cada
um na sua cópia da pasta. Tem o visual de vidro azul-marinho da Stained Glass UI e a organização do
Ratoeira (trilho de ícones, seções "Esperando você" e "Rodando agora", abas em pílula). Feito em
Rust com GPUI (o motor de interface do Zed) e os componentes do GPUI Kit.

## Como funciona

- **Mestre.** Uma conversa só, global, que vê todos os projetos: o círculo com a órbita logo abaixo
  do logo, Ctrl+M ou **Falar com o Mestre** no menu do Vitral. Ele não edita nada (só lê, com Read,
  Grep e Glob, nas pastas dos projetos e nas cópias): entende o pedido, responde o que dá (como estão
  os chats, perguntas rápidas sobre o código) e manda o trabalho para os chats de projeto. Roda sem
  perguntar permissão (nada do que ele tem escreve ou roda comando), sem Ultracode e fora da conta
  de vagas. `#nome` cita um projeto e `@12` (ou `@"Título"`) um chat; na caixa dele, `#` abre a lista
  dos projetos e `@` a dos chats, as duas filtrando enquanto você digita (Enter ou o clique troca só a
  última palavra; Esc fecha a lista sem apagar). O Vitral resolve as citações e avisa do ambíguo ou
  do não achado; sem citação, o Mestre escolhe pela descrição do projeto (ou pelo começo do README da
  pasta principal) e pela memória. O Vitral abre no último chat aberto e, sem ele, no Mestre. Vazio,
  o Mestre convida ("Peça qualquer coisa ao Mestre", com três sugestões que só preenchem a caixa).
- **"Vou dividir assim".** Todo trabalho vira uma divisão: uma ou mais partes, cada uma para um chat
  de projeto (um novo, ou um que já cuida do assunto e está em conversa), com o pedido, os critérios
  e a verificação. O cartão mostra o resumo e uma linha por parte (o projeto, "→ chat novo “título”"
  ou "→ chat 7 “Docs”", o pedido em duas linhas, os critérios e a verificação; o clique abre o pedido
  inteiro). O × tira a parte (riscada, com Desfazer); **Ajustar** põe "Ajuste na divisão 3: " na
  caixa; **Descartar** avisa o Mestre e nada começa; **Começar** cria os chats e manda os pedidos,
  numa gravação só. Decidido, o cartão vira uma linha ("Divisão 3 começada · 2 partes", "1 de 2
  terminaram", "Mandado direto ao chat 7"), que mostra cada chat com o estado. A divisão começa
  direto, sem cartão, com **Divisão: Começar direto** (o chip no lugar do modo, na caixa do Mestre),
  numa rotina ou quando há uma parte só e ela vai para o projeto (ou o chat) que você citou. Propor
  de novo no mesmo turno, ou a mesma divisão em 2 h, não cria nada. A parte que já não vale (projeto
  arquivado, chat ocupado) fica com o motivo e as outras seguem.
- **O pedido no chat.** Chega como "Pedido do Mestre (chat 12, rodada 1) · Ninee", com os critérios
  numerados, a verificação, as outras partes da divisão e a mensagem original. O chat faz, verifica
  e entrega (`entregar_tarefa`, com o estado de cada critério); sem a entrega, ganha um lembrete por
  rodada. Uma dúvida vai ao Mestre. O chat que o Mestre criou (ou com um pedido dele aberto) leva a
  órbita pequena no cartão da lista e, no cabeçalho, o selo "do Mestre ↩"; em Este chat, "Pedido do
  Mestre (divisão 3, rodada 2)" com o botão **Mestre**.
- **Junta sozinho.** Com **Juntar sozinho quando a verificação e o revisor aprovarem** (o padrão), a
  entrega toda verde (verificação passou ou não existe, revisor aprovou sem problema alto nem médio,
  todos os critérios feitos, ninguém mexendo nos mesmos arquivos) entra na pasta sem acordar o
  Mestre: ele só recebe uma nota. O resto (vermelho, conflito, pasta suja, sem revisor, pasta sem
  git) o acorda com um relatório curto, e ele junta (`aprovar_tarefa`) ou pede ajuste. Quando todas
  as partes que começaram terminam, o Mestre é acordado uma vez para o resumo, que ganha o cartão
  azul **Resumo do Mestre · Divisão 3**.
- **Criar o projeto “X”?** Quando o pedido não cabe em nenhum projeto, o Mestre sugere um: o
  motivo, a descrição e as pastas, com **Agora não** e **Criar projeto…**, que abre o Novo projeto
  preenchido ("Sugerido pelo Mestre; confira antes de criar"); criar responde ao cartão e o Mestre
  continua o pedido.
- **Memória do Mestre** (`~/.local/share/vitral/mestre/memoria.md`, até 6000 caracteres). Ele grava
  direto só quando o turno inteiro é seu; com um aviso, um relatório ou uma rotina no meio, vira o
  cartão "O Mestre quer lembrar" (**Não** e **Lembrar**). **Memória do Mestre…** (menu do Vitral,
  menu do Mestre, Editar do Distribuído) edita o texto, com a contagem e **Esquecer tudo**; se o
  Mestre lembrou algo com o diálogo aberto, o Salvar traz as linhas novas para o fim.
- **Projetos.** Ficam nos dados do Vitral, cada um com nome, ícone, cor, descrição ("o Mestre usa
  isto para saber o que mandar para este projeto"), instruções para os chats (até 8000 caracteres) e
  as pastas dele: nenhuma, uma ou várias ligadas (a primeira é a principal) ou, sem nenhuma, uma
  pasta própria com git em `~/.local/share/vitral/projetos/<nome>`. **Novo projeto** (Ctrl+Shift+N,
  o **+** do trilho) e **Editar projeto** são o mesmo formulário: o nome (Enter cria ou salva; "Já
  existe um projeto com esse nome", "Nome reservado"), o ícone e a cor (o chip abre a escolha, com
  Automático: o ícone pelo conteúdo da primeira pasta, a cor pelo nome), a descrição, as pastas
  ("Adicionar pasta…" escolhe várias; ligar uma que já é de outro projeto, ou que fica dentro ou em
  volta de uma ligada, pergunta antes; tirar uma em que chats trabalham avisa que eles continuam lá)
  e as instruções; na edição, também as **decisões anotadas pelos chats** (o que um chat anotar com o
  formulário aberto entra no fim). O **Avançado** (recolhido) tem, por pasta, a preparação de cada
  cópia, os arquivos que vão junto e o comando de rodar (e Ativar git na pasta sem git), e a
  permissão, o modelo e a conta dos chats novos do projeto. Nada grava antes do Criar ou do Salvar.
  Uma pasta que sumiu (disco desmontado, renomeada) aparece como "não encontrada", com **Localizar
  pasta…**, que também religa as cópias dos chats àquela pasta (`git worktree repair`; com cópias,
  só aceita uma pasta do mesmo repositório e avisa da que não voltou). `vitral <pasta>` abre o projeto
  que já tem essa pasta ou o Novo projeto com ela ligada.
- **Chat de projeto.** Cada chat trabalha numa pasta do projeto (a principal, ou a escolhida no chip
  **Pasta: x** antes da primeira mensagem), de um de três jeitos, decidido quando ele nasce, com a
  pasta presente, e guardado: numa **cópia** só dele (`git worktree` em
  `~/.local/share/vitral/copias/<projeto>/t<N>`, branch `vitral/t<N>-…`) quando a pasta é a raiz de
  um repositório com commit; **direto na pasta própria** do projeto, com um commit no fim de cada vez
  ("Vitral: <título>") e Voltar no tempo; ou **direto numa pasta sem git** (sem cópia, sem diff, sem
  Voltar no tempo). Uma pasta dentro de um repositório (sem ser a raiz) vai direto, com o aviso para
  ligar a raiz. As outras pastas do projeto entram só para leitura. O chat conversa com você como
  uma conversa do Claude Code e recebe os pedidos do Mestre; o número dele é o que você vê ("chat 12").
  Ctrl+N cria um na vista do projeto (com mais de uma pasta, pergunta **Em qual pasta?**); na visão
  geral, abre a paleta com os projetos, **Avulsos** e **Novo projeto…**.
- **A cópia nasce na primeira mensagem**, não quando o chat é criado: a mensagem espera ("Preparando
  a cópia de ~/Projetos/loja; a mensagem entra quando ela ficar pronta.") e entra quando a cópia fica
  pronta. A cópia recebe os arquivos locais fora do git (`.env*` e `CLAUDE.local.md` por padrão), roda
  o script de preparação da pasta e ganha dez portas próprias (`$VITRAL_PORT`). Se não der ("Não deu
  para preparar a cópia (…)", com **Tentar preparar de novo**), a mensagem fica guardada e a próxima
  tenta de novo. O Vitral fechado no meio refaz a preparação ao abrir, uma vez. A pasta original fica
  bloqueada para escrita do chat.
- **Juntar.** Acima da caixa, quando nada roda: "4 arquivos mudados na cópia (+12 −3)" com
  **Revisar** e **Juntar**. As mudanças da cópia entram na pasta original e a cópia continua, em dia
  com ela: a próxima junção leva só o que é novo. O chat ganha "As mudanças entraram em ~/x." (e o
  Claude fica sabendo na mensagem seguinte). Com a pasta original em outro branch que o de quando a
  cópia nasceu, pergunta antes ("Juntar em outro branch?"). Pasta original com mudanças sem commit,
  arquivos soltos no caminho ou outro git segurando a pasta (um `index.lock`): nada entra e o aviso
  diz o que fazer. Conflito: nada entra e o próprio chat recebe o pedido de resolver na cópia (`git
  merge`). Uma junção cortada por uma queda é conferida ao abrir: o merge parado é desfeito; se já
  tinha entrado, conta como feita; senão, "nada entrou; junte de novo". Depois de juntar, as outras
  cópias do mesmo repositório sem nada só delas andam junto (as com trabalho andam antes do próximo
  turno delas). As junções e criações de cópia de um mesmo repositório entram uma de cada vez.
- **Um escritor por pasta.** Dois chats diretos não mexem ao mesmo tempo na mesma pasta, nem numa de
  dentro da outra: o segundo espera ("Outro chat está mexendo em ~/x; esta mensagem entra quando ele
  terminar."). A pasta pessoal não trava as outras (as conversas trazidas do Claude Code em `~`).
- **Vagas e trabalho pesado.** Chats de projeto e ajudantes dividem as vagas (**Chats trabalhando ao
  mesmo tempo**); a mensagem que você escreveu tem uma vaga a mais e passa na frente da fila.
  Preparar uma cópia, conferir uma entrega (verificação e revisor) e o parecer que um chat pede
  (`revisar_minhas_mudancas`) entram numa fila única do Vitral inteiro, um de cada vez; o que você
  pediu vai na frente.
- **Ajudantes.** Um chat de projeto com git (numa cópia ou na pasta própria) pode dividir um pedido
  grande em até 3 ajudantes (**Ajudantes por chat**; 0 desliga), cada um num chat próprio, numa
  cópia da pasta do chat feita a partir do que ela tem agora (antes, o Vitral põe num commit o que o
  chat ainda não tinha posto, "Vitral: ponto antes do ajudante #M"). O ajudante trabalha só na cópia
  dele, pergunta ao chat que o criou e, no fim de cada vez, manda o relatório a ele (com a
  verificação e o parecer do revisor). O chat junta (na pasta dele, mesmo com mudanças sem commit:
  antes vai um commit "Vitral: ponto antes de juntar o ajudante #M"; a cópia do ajudante sai e o chat
  dele vai para as arquivadas), pede ajuste ou cancela. O **Juntar** do chat leva tudo à pasta
  original; num pedido do Mestre, o chat só entrega depois que os ajudantes terminam. Ajudante não
  cria ajudante, entra na conta de vagas e vai junto quando o chat muda de projeto. Pasta sem git e
  avulso não têm ajudantes.
- **Avulsos.** Chats sem projeto, cada um numa pasta só dele em `~/.local/share/vitral/avulsos/<chat>`,
  direto (sem cópia). Ficam no atalho **Avulsos** do trilho (o balão), com **Novo chat avulso**.
- **Conversas do Claude Code.** Na primeira abertura (junto da migração), o Vitral traz as conversas
  da extensão do VS Code e do terminal (`~/.claude/projects/<pasta>/<sessão>.jsonl`, só lidos: nada lá
  é mudado). Cada uma vira um chat: no projeto cuja pasta ligada contém a pasta da conversa (a mais
  funda), senão em Avulsos. O chat mexe direto nessa pasta, como lá (o Claude Code só retoma uma
  sessão a partir da pasta dela): sem cópia nem Juntar, e sem commit (só a pasta própria de um projeto
  ganha o commit de cada vez). O título é o que você deu (`/rename`), o resumo do Claude Code ou o
  começo da primeira mensagem; as datas e a ordem na lista são as da conversa. Ficam de fora as vazias
  (só um `/clear`), as de testes (pasta em `/tmp`), as dos chats do próprio Vitral, as de programas
  (`claude -p`) e as de pasta que não existe mais (um disco desmontado: entram quando ele estiver
  montado e você pedir de novo). O aviso diz o resultado ("40 conversas do Claude Code importadas ·
  38 em Avulsos · 2 em jcm-climatizacao · 3 abertas agora no Claude Code"), também na conversa do
  Mestre. O histórico aparece no chat, lido do arquivo da sessão quando ele abre (em segundo plano;
  nada disso vai para os dados do Vitral): os 300 últimos itens, com **Carregar mais**, e a linha
  "Trazida do Claude Code (VS Code) em 01/11" antes do que vem no Vitral. A primeira mensagem
  continua numa cópia da conversa (`--resume <id> --fork-session`): a original fica como está e pode
  seguir no VS Code; as seguintes retomam a cópia. A conversa que estava aberta no Claude Code quando
  foi trazida ganha o aviso disso. **Importar do Claude Code…** (menu do Vitral) traz as novas desde a
  última vez, sem repetir (nem as de chats apagados depois).
- **Conversas do Vitral antigo.** O coordenador que conversou fica só para leitura: no lugar da caixa,
  "Conversa do Vitral antigo, só para leitura." e **Continuar num chat novo**, que abre um chat no
  mesmo projeto com a conversa anexada e "Continuando a conversa “…” (anexada): " na caixa. Os
  trabalhadores dele viram chats de projeto, sem dono: o que estava rodando ou em revisão fica em
  conversa com o aviso "Este chat nasceu de uma conversa do Vitral antigo; o pedido agora é seu:
  continue a conversa ou use Juntar." (a barra de juntar já aparece ao abrir); o que já tinha
  terminado vai para as arquivadas como está. O coordenador que nunca conversou vira chat de projeto.
- **Verificação em três camadas.** Se o pedido tem comando de verificação, um hook roda os testes
  quando o chat para e, se falharem, ele continua sozinho. Depois o Vitral roda a verificação de novo
  e um **revisor independente** (sessão nova, só de leitura, pode ser outro modelo) confere os
  critérios e aponta problemas numerados, com arquivo e linha. Tudo vai para quem pediu (o Mestre, o
  chat de um ajudante ou você), que junta ou pede ajuste. A saída da verificação e da preparação é
  lida enquanto o comando roda (fica só o fim), então um `cargo test` ou `npm install` barulhento não
  trava; as duas mostram quanto tempo levaram ("PASSOU em 12 s"). Num turno com Ultracode o hook não
  vai: ele rodaria já na primeira resposta, com os agentes do fluxo ainda mexendo nos arquivos. A
  verificação do Vitral e o revisor continuam, quando o turno termina de verdade.
- **Portão.** Quem pediu (o Mestre ou o chat de um ajudante) só junta uma entrega cujo relatório
  desta rodada já saiu. Com a verificação falhando ou um problema de gravidade alta do revisor, o
  Vitral recusa e lista o que está vermelho; juntar mesmo assim exige `forcar` com um motivo, que vai
  para o commit ("Aprovada apesar de: ..."), aparece em vermelho na conversa e numa linha âmbar no
  cartão. Você, pelo Revisar, junta sem o portão; com a verificação vermelha, um apontamento de
  gravidade alta ou comentários seus ainda não enviados, ele pergunta antes ("Juntar com
  pendências?"), e o que estava vermelho vai como ressalva no commit.
- **Ajustes.** `pedir_ajuste` anexa sozinho os problemas do revisor e as últimas 40 linhas da
  verificação quando eles apontaram algo; quem pediu escolhe o que vai com `incluir` e tira falsos
  positivos pelo número com `ignorar_problemas`. `ver_diff` deixa os gerados (lockfiles, `.min.js`,
  `.map`, `.snap`) numa linha à parte e resume um diff grande (o `--stat`, os arquivos que cabem
  inteiros e o que falta); `arquivo` (no Mestre) ou `arquivos: [...]` (no chat, para um ajudante)
  traz o diff completo só desses. Uma mensagem sua
  direto a um chat em revisão abre uma nova rodada (verificação, revisor e relatório) e avisa quem
  pediu.
- **Chats conversam entre si** (`mensagem_para_chat`, `perguntar_coordenador` para quem pediu);
  mensagens que chegam com o chat ocupado esperam na fila. **Freio de ciclo:** depois de 6 mensagens
  automáticas seguidas entre dois chats (o Mestre inclusive) sem você no meio, a próxima fica guardada
  e os dois avisam "muitas mensagens seguidas entre X e Y; esperando você"; escrever para um dos dois
  zera.
- **Enquanto você esteve fora.** Ao voltar à janela depois de 15 min fora (**Resumo ao voltar
  depois de**), ou ao abrir o Vitral, a visão geral (no topo) e o Mestre (acima da caixa) mostram o
  que terminou, o que precisa de você e o que falhou ou parou desde que você saiu, cada linha abrindo
  o chat (Ctrl+clique, ao lado), com **Pedir ao Mestre um resumo** e o ×. Fica só na memória: a
  próxima volta troca, e nada entra no histórico.
- **Sessão do Mestre.** Com o contexto acima de 70% no fim de um turno, o seguinte começa numa sessão
  nova ("Sessão nova (contexto cheio).") com os pedidos abertos dele, os cartões abertos e as últimas
  5 mensagens suas; o mesmo bloco vai quando a sessão antiga expirou.
- **Rotinas.** Um pedido que o Vitral manda sozinho num horário e dias da semana, para o Mestre
  (que começa a divisão direto) ou para um chat novo num projeto; a de projeto arquivado não roda.

## Primeira abertura, dados e linha de comando

- **A migração.** Na primeira abertura desta versão (quando ainda não há
  `~/.local/share/vitral/estado.json`), antes de a janela abrir, o Vitral traz o que havia nas
  pastas do Vitral antigo (as de `~/.config/vitral/recentes.json`, a mais recente primeiro, e a do
  `vitral <pasta>`): cada subpasta vira um projeto (o id é o nome da subpasta), com o ícone, a cor, a
  configuração e a marca de arquivado de antes; as conversas, as tarefas, as filas, as rotinas e as
  notas vão junto, com o mesmo `cwd` e a mesma sessão. Uma tarefa com o número repetido entre duas
  pastas ganha outro, e o chat dela avisa ("Na importação, este chat passou de #3 para chat 12.").
  Nada nas pastas antigas é mudado, movido ou apagado (`<raiz>/.vitral/` fica intacto, inclusive as
  cópias antigas em `.vitral/worktrees`, que continuam valendo: os caminhos são absolutos); o
  `estado.json` é o último arquivo gravado, então um PC que desliga no meio refaz tudo na abertura
  seguinte. O relatório fica na conversa do Mestre ("Trouxe 6 projetos de ~/Projetos: … Os 2 chats
  foram junto."), com o pedido de escrever a descrição dos projetos que não têm descrição nem README,
  e em `~/.local/share/vitral/migracao.json`. Ela roda uma vez só: depois, `vitral <pasta antiga>`
  só avisa no Mestre que essa pasta tem conversas do Vitral antigo.
- **O Vitral antigo aberto.** Com um Vitral de antes desta versão aberto na primeira abertura, nada é
  gravado e a tela diz "O Vitral antigo está aberto (pid N). Feche-o para eu trazer as conversas de
  ~/Projetos.", com **Tentar de novo** e **Sair**. Se ele mexer numa pasta antiga depois da migração,
  o Mestre avisa uma vez por mudança ("O Vitral antigo mexeu em ~/Projetos depois da importação …
  O que foi feito lá não aparece aqui.").
- **Não apague `<raiz>/.vitral`** enquanto houver chats com cópia lá (as tarefas do Vitral antigo
  que ainda não foram juntadas): a cópia mora em `.vitral/worktrees/<projeto>/t<N>`.
- **Voltar para a 0.2.0** fica desligado depois de migrar: ela não lê as conversas do formato novo
  (as Configurações dizem "A 0.2.0-rc.2 não lê as conversas do formato novo"). O `.vitral/` de
  cada pasta continua lá, como a 0.2.0 o deixou.
- **Onde fica cada coisa.** Os dados em `~/.local/share/vitral`: `estado.json` (com `.anterior`),
  `chats/<id>.json`, `notas/<projeto>.md`, `mestre/` (a pasta do Mestre e a `memoria.md`),
  `avulsos/<chat>/`, `projetos/<nome>/` (as pastas próprias), `copias/<projeto>/t<N>/`, `exportadas/`,
  `anexos/`, `migracao.json` e o `formato` dos dados; ao lado deles, em `app/`, o app instalado pelo
  atualizador (as versões e os links `atual` e `anterior`). As preferências em `~/.config/vitral`
  (`config.json`, `interface.json`, as contas, `atualizacoes.json`; o `recentes.json` antigo, que só
  a migração lê) e o prompt de cada turno em `~/.cache/vitral/prompts/`. Um arquivo estragado vai
  para `<nome>.ilegivel-<hora>.json` e o Vitral abre com o `.anterior`, avisando.
- **Um Vitral só.** A trava é `~/.local/share/vitral.trava` (fora da pasta de dados) e o socket,
  `$XDG_RUNTIME_DIR/vitral/vitral.sock`. Abrir de novo (pelo menu, `vitral <pasta>` ou `--nova`)
  entrega o pedido ao que está aberto (também enquanto ele migra) e não abre outra janela; com ele
  travado, a segunda abertura espera até 10 s e diz "O Vitral já está aberto, mas não respondeu".
- **Linha de comando.** `vitral` abre o app. `vitral <pasta>` (o `%f` do Nautilus) abre a visão
  geral se a pasta é uma das migradas, o projeto se ela já está ligada a um, e senão o Novo projeto
  com ela ligada (avisando se contém uma pasta ligada ou está dentro de uma). `vitral --nova mestre
  [mensagem]` abre o Mestre (com a mensagem, já enviada; sem ela, o cursor na caixa); `--nova
  <projeto> [mensagem]` cria um chat no projeto (id, nome ou chave sem ambiguidade); com um destino
  que não é um projeto (ou é mais de um), a mensagem vai ao Mestre como `#<destino> <mensagem>`, e sem
  mensagem o Mestre abre com `#<destino> ` na caixa.
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
- **Verificação, revisor ou preparação cortados** rodam de novo sozinhos ao reabrir, um de cada
  vez. Se cair de novo nessa mesma refeita, o pedido fica parado com Retomar, para não entrar num
  ciclo de quedas.
- **Chat que cai** (processo que morre ou não abre) ganha uma nova tentativa depois de 10 s; um
  erro passageiro da API (sobrecarga, 5xx, tempo esgotado, conexão), depois de 60 s. Se falhar de
  novo, o pedido volta a quem pediu com ERRO e o motivo (o ajudante, ao chat dele), com notificação
  se a janela estiver em segundo plano. Mandar uma mensagem ou Parar cancela a nova tentativa.
- **Agentes em segundo plano.** Quando o Claude responde com um fluxo de agentes (ou um subagente)
  ainda rodando, o turno não acaba ali: o chat continua rodando, a conversa ganha o aviso "Os
  agentes seguem trabalhando em segundo plano; o turno termina quando eles acabarem." e o pedido só
  vai para a verificação e o revisor com a última resposta, depois que eles acabam (ou quando o
  processo do Claude Code sai). Se o processo sai com eles ainda rodando, com o teto da espera
  vencido ou com um código de erro, o turno termina com erro e o relatório diz que os agentes não
  terminaram. Parar continua valendo e, nessa espera, derruba o processo com os agentes. Um turno
  que termina com erro ou parado enquanto há agentes em segundo plano também derruba o processo, em
  vez de deixá-los rodando fora da tela. No Ultracode, o Claude Code espera os agentes sem o teto
  de 10 minutos. Em todo turno, cada fluxo roda no máximo 3 agentes ao mesmo tempo
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
- **Processos.** O claude, a verificação e o revisor rodam cada um num escopo do systemd do
  usuário (`vitral-<pid>-...scope`, em `vitral.slice`). Parar um deles derruba tudo o que ele
  abriu, até servidores e shells separados com `setsid`; fechar o app manda SIGTERM a tudo sem
  travar a tela; ao abrir, o Vitral para os escopos de uma instância que caiu. Sem systemd, vale
  um grupo de processos.
- **Confirmações.** Toda pergunta antes de agir tem o mesmo desenho: o ícone num círculo na cor
  do tom, o título, o que acontece e, numa caixa embaixo, o que some (o tamanho, o branch, os
  pontos de volta). Nas que apagam ou desfazem (**Apagar a cópia**, **Voltar os arquivos**,
  **Descartar**, **Desconectar**, **Apagar projeto**, **Começar do zero**) e nas que param o que trabalha
  (**Parar e arquivar**), o Enter só fecha, sem agir; em **Ativar git**, o Enter confirma.
  Arquivar (chat ou projeto) não pergunta: age na hora, e o aviso tem **Desfazer** (com chats
  trabalhando ou pedidos do Mestre abertos no projeto, arquivar o projeto pergunta antes).
- Com agentes rodando, a janela redesenha no máximo 12 a 20 vezes por segundo (e não a cada
  quadro do monitor de 165 Hz). Os efeitos só andam em resposta a uma ação (abrir, focar, clicar,
  o ponteiro entrar num botão redondo, chegar dado novo), duram de 120 a 240 ms e redesenham no
  máximo 60 vezes por segundo; parado, nada se mexe. Com as animações do GNOME desligadas, com
  **Animações: Desligadas** nas Configurações (ou `VITRAL_SEM_ANIMACAO=1`), tudo aparece direto no
  estado final e o spinner fica parado. Com a janela visível mas sem foco, os spinners e o brilho
  da faixa do turno param (1 redesenho por segundo, o do relógio, em vez de 12); texto chegando
  numa conversa que nenhum painel mostra não redesenha a janela; e a barra de rolagem aparece e
  some sem fade (o fade pedia quadros no ritmo do monitor a cada mudança de tamanho, como o Ctrl+B).

## Opções da conversa (como na extensão do Claude Code)

Na caixa de mensagem (cantos de 20, 14 na Estreita; gradiente azul indo para o preto; com o foco ou um
popover dela aberto, a borda fica azul e um halo azul suave acende em volta, em 180 ms), a barra de
baixo tem chips com ícone, nesta ordem: **modo de permissão** (Manual, com o escudo cheio, e
Planejar em verde; Aceitar edições e Automático em violeta; Sem perguntar em âmbar e, sem
cópia isolada, o chip inteiro em vermelho), **modelo** (o logo do Claude e o que vale, como "Claude
Opus 5.5"), **esforço** (barrinhas em ciano, de "Auto" a "Máximo") e **conta** (o ícone na cor do
uso de 5 h). O que segue o padrão da pasta fica em tom tênue. À direita ficam o anel do contexto (a
porcentagem no meio; âmbar a 70%, vermelho a 90%; o clique abre o uso da conversa: em cima o
contexto e o custo, "Contexto 31% · US$ 1,84", embaixo o modelo, a conta, os tokens e, na
conversa do Vitral antigo, o custo com os ajudantes, e no rodapé **Compactar a conversa**, que prepara o
`/compact`), o clipe de anexar (arquivo ou imagem, arquivo do projeto, conversa anterior e comandos
de barra), o **…** (comandos de barra, compactar, sessão nova e fixar) e o botão redondo de enviar,
apagado com a caixa vazia, que acende quando ela ganha texto e dá um pulso claro a cada envio (o
pulso fica no botão também quando ele já virou o parar). Clicar em qualquer ponto vazio da caixa
(o vão entre o texto e os chips, a margem, o espaço entre os chips) mantém o cursor nela. Com a
conversa rodando, ele vira o parar (o
quadrado); com texto escrito, ficam os dois, e o enviar entra na fila. Vazia, a caixa diz "Peça ao
Mestre… (#projeto, @chat)" no Mestre, "Mensagem para este chat…" no chat de projeto, "Mensagem…" no
avulso e "Mensagem direta para este ajudante…" no ajudante ("Sem cópia: fale com o chat dele" e
"Cópia removida: fale com o chat dele" quando não dá); num chat em revisão, a dica do enviar diz
que a mensagem abre uma nova rodada de ajuste, com verificação e revisor. No Mestre, no lugar do
modo de permissão fica o chip **Divisão: Mostrar antes | Começar direto**, e o clipe tem "Mandar
para um projeto…" (#), "Falar de um chat…" (@) e "Arquivo do computador…". **Shift+Tab** troca o
modo. O menu do modo lista o que foi permitido nesta conversa: o clique numa regra tira só ela, e
**Voltar a perguntar sobre tudo** tira todas. O **esforço** abre um popover com o nível num
segmentado (←/→, Home e End mudam na hora; o nível fica só na pílula do segmentado, sem repetir no
cabeçalho), com uma linha curta sobre cada nível (Auto: "Usa o
padrão do modelo."), e a chave **Ultrathink**, em arco-íris (Espaço liga; a descrição diz "Só na
próxima mensagem"): vale só para a próxima mensagem, sobe o esforço dela para Extra (o Máximo
continua Máximo) e deixa o chip em arco-íris até enviar. Um comando de barra não a consome. Embaixo
dela fica a chave **Ultracode**, em violeta com o ícone de fluxo (U liga): vale só para a próxima
mensagem, como o Ultrathink, e faz o Claude orquestrar vários agentes em paralelo por um fluxo (a
ferramenta Workflow do Claude Code). A linha avisa em âmbar que gasta bem mais do plano e que roda
no esforço Extra (o Máximo vira Extra nessa mensagem). Ligada, o chip do esforço vira "Ultracode" em
violeta com o ícone de fluxo (com o Ultrathink junto, as barrinhas ficam em arco-íris; na caixa
estreita, só o ícone de fluxo). Começa sempre desligada, também ao abrir o Vitral. Fica
desabilitada, com o motivo curto no lugar da descrição, no Mestre ("só nos chats de projeto"; a
explicação, que ele não edita e manda o trabalho aos chats, vai na dica da linha), na conversa do
Vitral antigo, com
um Claude Code anterior à 2.1.278 (a versão em que o Vitral conferiu o Ultracode; o limite de
agentes só existe desde a 2.1.269) e com um modelo sem o esforço Extra (o Haiku, os 4.x até o 4.6).
Numa conta em que um turno com Ultracode mostrou os fluxos de agentes desligados (em Dynamic
workflows no `/config`, por `disableWorkflows` ou pela organização), a linha continua ligável e
avisa em âmbar; esse turno ganha um aviso em vermelho, segue em Extra e a mensagem perde o selo.
Fora de Sem perguntar e Automático, a linha também avisa que comandos e edições dos agentes podem
ser recusados (eles não conseguem pedir permissão). Um Ultracode por vez no Vitral inteiro: com
outro rodando, a linha avisa ("Ligado, espera o Ultracode da outra conversa terminar") e a mensagem
espera a vez na fila de vagas, com um aviso só. As novas tentativas automáticas (queda, erro da API,
troca de conta) vão sem o Ultracode, com o aviso "A nova tentativa vai sem o Ultracode; ligue de
novo se quiser."; só o **Tentar de novo** o repete. Fora do Ultracode, a ferramenta Workflow fica
bloqueada: nem um chat nem um "Continue de onde parou" relançam um fluxo sozinhos (o Mestre nunca
a tem). O **modelo** abre um seletor com busca: Padrão (o do projeto, ou o das Configurações) e as famílias do Claude,
com a versão quando o modelo em uso é daquela família; o logo do Claude fica só no rótulo do grupo e
o check do escolhido à direita. Seguindo o Padrão, é ele que diz qual está em uso ("Padrão · Opus
5.5"); só uma família escolhida na conversa ganha o selo "em uso". Uma conversa antiga que
guardou o nome completo do modelo ganha a linha dele. Fora da caixa estreita, os popovers de esforço
e de modelo terminam num rodapé com as teclas que aceitam, no desenho de tecla da paleta: **← →**
nível, **Espaço** Ultrathink, **U** Ultracode, **Enter** fecha; no de modelo, **↑ ↓** escolher,
**Enter** usa. Com a caixa média, as setas e o "Claude " do modelo saem, e a conta fica só com o
ícone. Com a caixa estreita, os chips ficam só com o ícone (o nome vai na dica), e o anexar e a
conta vão para o **…**. Nenhum texto é cortado no meio: o que não cabe some inteiro, a conta
primeiro. Mencionar arquivo do projeto (`@`, ou digite `@`) e anexar outra conversa como contexto.
Colar (Ctrl+V) imagem ou arquivos copiados, ou arrastar arquivos do gerenciador para a conversa,
vira anexo. `/` abre os comandos de barra (os do seu Claude Code, com `/clear` para sessão nova e
`/compact`). Passe o mouse numa mensagem para copiar (e, na sua mensagem, voltar os arquivos àquele
ponto): as ações aparecem numa barra flutuante, à esquerda do seu balão, na altura da última linha
(no canto de cima dele, no painel mínimo), e no canto de cima à direita da resposta; nada fica
reservado nem à vista sem o ponteiro, e nada se mexe quando ela aparece. Os blocos de código ficam
num preto quase puro, como as saídas de comando, e têm o próprio Copiar, numa coluna à direita que
nunca cobre o código; a linguagem fica na dica dele ("Copiar código python"). Modo foco (esconde as
ferramentas, vale por painel).

- **Ler a conversa.** A **hora** de cada mensagem aparece no hover, na mesma barra flutuante do
  Copiar (`hora_na_barra`), e um **separador de dia** ("Hoje", "Ontem", "22 de março") separa os
  turnos (`separador_de_dia`): numa conversa de dias dá para saber quando cada coisa aconteceu. As
  conversas de antes da 0.4.0 não guardaram a hora e não mostram nada. O **Copiar** vale em
  qualquer mensagem, não só na última resposta de cada turno. **Ctrl+F** busca dentro da conversa
  aberta (`src/ui/busca_conversa.rs`): a barra flutua no alto do painel, com o contador ("3 de
  12"), **F3** e **Shift+F3** para andar e realce em âmbar; a busca não liga para maiúscula nem
  para acento ("codigo" acha "código"), conta também o que está dentro de um bloco fechado (a saída
  de uma ferramenta, o relatório de um ajudante), que abre ao chegar a vez dele, e **Esc** fecha. O
  índice só é refeito quando o termo ou o tamanho da conversa muda, e nada disso anima.
- **Escrever.** Com a caixa vazia, a **seta para cima** traz de volta a sua última mensagem para
  ajustar e mandar de novo, e a conversa volta a antes dela (`EditarUltima`, só no contexto
  "Vazia"; com texto escrito a seta continua movendo o cursor). O **rascunho** de cada conversa (o
  que você escreveu e não enviou) já ficava na memória da janela ao trocar de painel; agora também
  vai para o `interface.json` 1,2 s depois de você parar de digitar, e volta quando o Vitral abrir
  de novo, inclusive depois de uma queda (`src/ui/rascunhos.rs`). A **fila** do que você mandou com
  o turno rodando tem lista própria (`src/ui/fila.rs`), aberta pela pílula da faixa do turno: cada
  mensagem esperando numa linha, com o × que devolve o texto à caixa e, com o chat parado,
  **Entregar agora** no rodapé. **?**, com o foco fora de um campo de texto, abre os **Atalhos do
  teclado** (`src/ui/atalhos.rs`): a tela é montada da mesma tabela que registra as teclas, então
  nunca desencontra do que o teclado faz.
- **Faixa do turno.** Uma linha fixa acima da caixa, alinhada com o texto dela (e, à direita, com o
  enviar), que entra com um fade e 8 px de baixo quando aparece, diz o que o chat está fazendo
  ("Lendo motor.rs…", "Rodando: cargo test…"), há quanto tempo, quantas mensagens esperam na fila e
  lembra que **Esc** para o turno. Com a resposta já dada e os agentes de um fluxo ainda rodando,
  ela diz "Agentes trabalhando em segundo plano…". Quando nada roda, ela diz por que parou e dá a
  ação: na fila esperando vaga (Esc tira da fila), "Caiu; tentando de novo em 8 s", **Parada** com
  **Retomar** (âmbar quando foi o Vitral que parou), **O último turno falhou** com **Tentar de
  novo** e mensagens esperando com **Entregar agora**. Esperando você, ela some (o pedido está logo
  abaixo).
- **Planejar.** O plano aparece num cartão com a faixa âmbar em cima e o modo em que a conversa
  fica depois de aprovar ("Depois de aprovar: Aceitar edições"). **Aprovar e executar**
  sai do Planejar e volta ao modo de antes (sem modo anterior, o padrão da pasta; se também for
  Planejar, Aceitar edições); a seta ao lado aprova em outro modo (**Aprovar e aceitar edições** ou
  **Aprovar e perguntar antes**). O campo embaixo leva o que mudar: Enter (ou
  **Enviar mudanças**) devolve o plano com o comentário.
- **Pedidos de permissão** num cartão com a faixa âmbar em cima e o título do que vai acontecer
  ("Rodar este comando?", "Editar cupom.rs?", "Abrir docs.rs?"), com a descrição ou o caminho
  embaixo. O corpo mostra o comando inteiro, um mini-diff no Edit, as primeiras linhas no Write;
  o caminho fora da pasta do chat leva "fora da pasta" em âmbar. Com um trecho arriscado no
  comando (`rm -rf`, `sudo`, `--force`, `| sh`...), o cartão fica vermelho, o trecho aparece em
  vermelho, o botão vira **Permitir mesmo assim** e o "permitir sempre" some. Além de Permitir e
  Negar: **Negar e explicar…** (o texto vai ao Claude) e **Permitir edições nesta conversa** /
  **Sempre permitir este comando** / **Sempre permitir X**, que valem já no turno em andamento.
  Com vários pedidos de uma vez, eles ficam sob "Pedidos" com o número, e só um mostra o corpo
  (clique no cabeçalho de outro para abri-lo; os fechados têm Negar e Permitir ali mesmo).
- **Ferramentas.** Chamadas seguidas viram uma trilha calma, uma linha de 28 por chamada, coladas e
  ligadas por um traço fino: o ícone do tipo (em cor só quando falhou), o verbo ("Leu", "Editou",
  "Rodou") e o detalhe; a falha tem fundo vermelho claro e "falhou" no fim. Um fluxo de agentes (a
  ferramenta `Workflow`) aparece como **Fluxo de agentes**. A partir de 4 a trilha começa recolhida
  ("6 ações · leu 3 · rodou 2", com "1 falhou" em vermelho); o cabeçalho entra no Tab. O clique numa
  chamada mostra o resultado (as últimas 40 linhas, com Copiar e Mostrar tudo). O que um chat faz
  com os ajudantes (e o Mestre com os chats) aparece como evento ("Criou o ajudante #2 …", "Pediu
  ajuste em", "Aprovou e juntou", "Criou 3 ajudantes"), com a pílula do estado na última menção de
  cada um (só a palavra, "em revisão"; o detalhe e a explicação na dica). As ferramentas do Mestre
  têm os ícones e os verbos delas ("Propôs a divisão · 3 partes: Ninee, Vitral", "Sugeriu o
  projeto", "Lembrou"). Fora da largura larga, o verbo e a frase vão para a dica (o ícone já diz o
  que foi) e o título fica com o espaço. O clique mostra o detalhe e, no hover, a linha ganha
  **Abrir o chat** (o balão de conversa; Ctrl+clique abre ao lado) e Copiar. Em repouso, a pílula do estado encosta na seta de abrir; no hover, só a linha
  sob o ponteiro abre espaço para as ações. A
  última entrega de um ajudante em revisão vira um cartão com "#4 título" e a pílula, a rodada e o
  que a verificação e o revisor acharam ("Rodada 1 · verificação falhou · 4 observações do revisor";
  a frase do Vitral, o tempo da verificação e o parecer na dica) e **Revisar** (em tinta, o
  azul-claro dos cartões da direita; o azul cheio fica só no primeiro cartão da direita que pede
  ação), **Abrir conversa** (o balão; a chave inglesa fica só no título, onde quer dizer
  ajudante) e o relatório. O cartão tem os cantos dos cartões da direita, e o ícone dos botões cai
  na coluna da linha de cima; na largura mínima, os dois botões ficam só com o ícone.
- **Mensagens.** O que você escreve fica num balão neutro à direita (sem avatar, com o reflexo do
  vidro), com as miniaturas das imagens anexadas, o nome e o tamanho dos outros arquivos e os selos
  **Ultrathink** (em arco-íris) e **Ultracode** (em violeta, com o ícone de fluxo) quando a mensagem
  foi com eles; texto com mais de 14 linhas mostra 12 e **Mostrar tudo**. Mensagens que outro chat
  (ou o Vitral) entregou mostram quem mandou (o círculo do Mestre, o quadrado do projeto de outro
  chat, a chave inglesa de um ajudante ou o logo do Vitral) e uma prévia; as dos chats ao Mestre (e
  o aviso do Vitral que cita um chat) têm **Abrir o chat**. A prévia termina com reticências antes da coluna da seta, nunca
  cortada no meio da letra; o clique abre o texto inteiro. Os avisos do Vitral são marcos
  centralizados ("Sessão nova", "Turno interrompido"), linhas tênues ("Tentando de novo.") ou,
  quando é erro, um alerta vermelho com Copiar. A resposta do Mestre ao fim de uma divisão fica no
  cartão **Resumo do Mestre · Divisão 3** (na conversa do Vitral antigo, o resumo final é o **Resumo
  da conversa**, com a contagem: "3 ajudantes aprovados · 1 cancelado"). O que chega com a conversa aberta no painel (a sua
  mensagem, a resposta, um evento, um aviso) entra com um fade e 8 px de baixo, uma vez; abrir a
  conversa, trocar de painel ou rolar não anima nada. Entre turnos ficam 24 px antes do seu balão,
  16 depois dele e 12 entre os blocos de um turno.
- **Rolagem.** Barra de rolagem por painel. No fim, a conversa continua colada no fim também
  quando o painel muda de largura (abrir ao lado, Ctrl+B, zoom, puxar a borda da janela), em que
  as linhas quebram de novo. Qualquer subida com a roda, o touchpad ou a barra solta o fim; aí o
  streaming não puxa a tela de volta e a conversa não conta como lida; a pílula com a seta (e
  "Novas mensagens", com um ponto azul, quando chegou algo), que entra com um fade, volta ao fim,
  como o **Ctrl+End** (descendo, os últimos 48 px já prendem de novo). Abrir um bloco no fim da
  conversa (o relatório de uma entrega, a saída de uma ferramenta) não pula para o fim dele: o
  painel fica onde está.
- Mensagens, pedidos e caixa ficam numa coluna de leitura centrada (até 860 px, com 16 de margem).
  Em janela baixa, o plano e os pedidos encolhem e rolam, e a caixa cresce menos. Blocos de código
  têm cor de sintaxe.

**Voltar no tempo:** antes de cada turno de um chat com git (numa cópia ou na pasta própria) o
Vitral tira uma foto dos arquivos;
passe o mouse na sua mensagem e use ↶, na barra que aparece à esquerda do balão, para voltar os
arquivos àquele ponto (o botão vermelho da confirmação é o que volta). A resposta diz quantos
voltaram e quais não. Com a cópia apagada, o ↶ some.

## Painel da direita (Ctrl+J)

O que ele mostra depende do chat aberto: o **Distribuído** no Mestre, **Este chat** num chat de
projeto, **Ajudantes** num ajudante (os do chat dele, com o seu em destaque) e **Esta conversa**
numa conversa do Vitral antigo (as tarefas dela que já tinham terminado). Na dica da aba, o projeto
e o número do painel. Com a lista rolada, uma linha separa o cabeçalho, como na conversa; a lista não
rola sozinha, e ao trocar para um chat sem cartão destacado ela volta ao topo.

- **Distribuído** (o Mestre): por projeto (o quadrado, o nome, quantos chats e a barra do estado
  deles), cada chat das divisões que ainda andam ou terminaram nas últimas 24 h, com o estado, "+N
  −M" ou o andamento e **Revisar** quando ele entregou; o clique abre ao lado (Ctrl+clique, no
  lugar). "Ver anteriores (N)" mostra as mais antigas. Embaixo, a **Memória do Mestre** com Editar.
- **Este chat**: na cópia, a origem, o branch (→ o da origem), "N à frente · N atrás", mudanças sem
  commit e o que falta juntar, com **Revisar e juntar** (em tinta quando há o que juntar) e **Pedir
  parecer do revisor**; na pasta própria, "cada vez vira um commit (Voltar no tempo desfaz)" e Abrir
  pasta; na pasta sem git, **Ativar git em x…** (os próximos chats ganham cópia) e Abrir pasta; no
  avulso, a pasta dele; numa conversa trazida do Claude Code, de onde ela veio, as datas e quantas
  mensagens tinha; **Localizar pasta…** quando ela sumiu. Embaixo, o cartão do pedido em aberto (só
  com um pedido, ou numa conferência) e os ajudantes, sob **Chats (n)**.
- **Chats (n)**: uma linha só com a barra fina da proporção de cada estado (cada parte diz na dica
  quais são: "1 em revisão: #2", com as mesmas palavras e cores da pílula) e os atalhos, só com o
  ícone (com o rótulo quando o painel está largo, de uns 450 px para cima): num ajudante, **Chat**
  abre ao lado o chat que o criou; com ajudantes em andamento, **Lado a lado** monta o chat e eles,
  quantos couberem na largura.
- Cada cartão tem "#2 título", a pílula do estado ("● em revisão · com o Mestre", "com você" ou "com
  o chat dele"; a parada é âmbar quando precisa de Retomar e neutra quando foi você que parou) e a
  trilha de pontos (Trabalho, Verificação, Revisor, Decisão, Juntada; o detalhe de cada um na dica).
  Embaixo, numa linha só, o motivo principal: a preparação ou a verificação que falhou ("Verificação
  falhou · 19 s", com o comando na dica), o revisor que pede ajustes ("Revisor pede ajustes · 2
  importantes"), os mesmos arquivos de outro chat ou o andamento (o resumo do revisor e o andamento
  terminam em reticências quando não cabem). O clique no motivo, ou nos pontos da verificação e do
  revisor, abre a saída do comando (o fim dela, com **Mostrar tudo** e copiar) ou o parecer do
  revisor, com a gravidade em cada problema; cada problema abre o Revisar já na linha. Selos
  pequenos só para o que não virou motivo: ressalva, mesmos arquivos (#1) e "sem cópia".
- Uma ação principal por cartão, e um só botão azul cheio no painel: o do cartão destacado, se ele
  pede ação, ou o do primeiro que pede; nos outros, o mesmo botão fica azul-claro, em tinta. As
  ações: **Ver pedido** (o chat espera permissão: abre a conversa dele ao lado com o cursor na
  caixa), **Retomar** (parado) ou **Revisar** (em revisão). Ao lado, **Abrir ao lado** (ou **Ir à
  conversa**, se já está aberta; **Ver erro** se o processo caiu), **Mudanças** e, no ajudante
  aberto, **Rodar** com o comando de rodar da pasta. O **…** tem rodar na cópia (com o
  `$VITRAL_PORT` dela), terminal, VS Code, copiar o caminho, **Abrir neste painel**, configurar o
  projeto e, quando a cópia já não vai ser juntada, **Apagar a cópia…** em vermelho. A seta no fim
  mostra o objetivo, os critérios, o comando de verificação, a pasta e o branch da cópia (cada um com
  copiar) e a ressalva da aprovação. O detalhe abre e fecha deslizando.
- Clicar no cartão (ou Enter, com ele em foco pelo Tab) abre o chat dele à direita dos painéis;
  Espaço mostra ou esconde o detalhe; também dá para arrastar o cartão para um painel. Com um
  ajudante aberto, o cartão dele fica com borda e barra azuis e o detalhe aberto, e o painel rola até
  ele ao trocar de conversa. Quem pede permissão fica com o cartão âmbar.
- Os encerrados ficam em **Concluídas (n)**, recolhidas enquanto houver abertos, numa linha cada
  (aprovado, cancelado ou falhou no ícone, a ressalva à direita e o **…** no hover). As seções abrem
  e fecham deslizando, e a pílula pisca mais forte por um instante quando o estado muda com o painel
  aberto. As decisões anotadas pelos chats ficam em **Editar projeto**.
- Num painel estreito (abaixo de uns 330 px, ou a 150% de zoom), os botões ficam só com o ícone e o
  detalhe da pílula e o tempo do motivo vão para a dica.
- Na gaveta (janela estreita), o painel é opaco por cima das conversas, com o fundo escurecido; ele
  entra deslizando da borda direita, e o botão do canto (ou Esc) o recolhe.
- No ajudante, o ↰ no lugar do símbolo da aba leva ao chat dele (Ctrl+clique abre ao lado), e o ⋯
  do painel tem **Cópia t2**, com o menu da cópia: rodar, terminal e VS Code na cópia, não na pasta
  original.

## Revisão das mudanças

**Revisar** (na barra acima da caixa do chat, em Este chat, no Distribuído ou no cartão de um
ajudante) abre os arquivos mudados no diff do GPUI Kit, num diálogo do
tamanho da janela: o diff num fundo preto, com cor de sintaxe, número antigo e novo em cada linha, o
que não mudou dobrado, **Unificado** ou **Lado a lado** num segmentado com a pílula que desliza
(Lado a lado apagado, com o motivo na dica, quando falta largura), **Arquivo
inteiro**, **Quebrar linhas** e as setas de mudança anterior e próxima. Em cima, "Revisar e juntar
· chat 12 título" (numa linha, inteiro na dica) e, embaixo, o projeto, a pasta, a rodada, os
arquivos e o tamanho do diff ("loja · site · rodada 2 · 3 arquivos · +11 −1"); num ajudante, o
projeto, "ajudante #4", a rodada e a pílula do estado.

- **Conferência em chips.** Na barra, **Verificação passou** (ou falhou, em vermelho) e **Revisor
  aprovaria** (ou **pede ajustes**, com o número de apontamentos na cor da maior gravidade). O
  clique abre a saída do comando (o fim dela, com **Mostrar tudo** e copiar) ou o parecer do
  revisor, com cada apontamento ("alta", "média", "baixa"): o clique vai à linha, e **Usar** o
  transforma em comentário. Enquanto a conferência roda, os chips mostram o spinner; da rodada
  anterior (o chat ajustando), ficam em tom tênue e dizem isso.
- **Arquivos à esquerda**, por pasta (a raiz primeiro, os gerados como lockfiles e `.min.js` no
  fim, recolhidos e com o selo "gerado"), cada um com o ponto do apontamento mais grave, os seus
  comentários e o +N −M. Com mais de 8, um campo filtra. Embaixo, **Seus comentários**: o clique
  vai à linha, e no hover cada um tem Editar e apagar (os de um arquivo que não está no diff
  aparecem como "fora do diff"). Numa janela estreita, ou no lado a lado sem espaço, a lista vira
  o chip **Arquivos**, com busca.
- **Comentar.** O clique numa linha a escolhe (arraste nos números para pegar várias); **C**,
  **Enter** ou o **+** abrem o comentário ali. Enter salva, Shift+Enter quebra a linha e Esc
  cancela. Os apontamentos do revisor aparecem como cartão na própria linha, com **Usar como
  comentário**; os seus, num cartão neutro. **Visto**, no cabeçalho de cada arquivo, o recolhe, e
  o rodapé conta quantos já foram vistos. Ctrl+PgUp e Ctrl+PgDn vão ao arquivo anterior e ao
  seguinte; F7 e Shift+F7, à mudança. Ctrl+Enter, fora de um campo, aciona o botão azul do rodapé.
- **Pedir ajuste** mostra o que vai junto (os comentários por arquivo), tem um recado geral
  (Enter envia) e, quando sobram apontamentos do revisor, um interruptor para incluí-los. Vira uma
  rodada de ajuste do chat.
- **O botão azul segue o que está vermelho.** Sem pendências, o azul é **Juntar**. Com a
  verificação vermelha nesta rodada, apontamento de gravidade alta, o diff que não deu para ler ou
  comentário por enviar, o azul passa para **Pedir ajuste** (com o selo do que vai junto: os
  comentários e, com o interruptor ligado, os apontamentos que você não usou), e **Juntar** fica
  fantasma, com o ícone de junção. **Ctrl+Enter** aciona o botão azul do momento (abre
  o Pedir ajuste ou aprova), e a dica dele mostra o atalho; num campo de texto, com um comentário
  aberto ou com um popover aberto, o Ctrl+Enter fica com eles.
- **Juntar** fica desabilitado com o motivo quando o chat não pode ser juntado agora (conferindo,
  rodando, já juntado); com pendências, pergunta antes ("Juntar com pendências?"); com a pasta
  original em outro branch, "Juntar em outro branch?". Um conflito na
  junção aparece num aviso dentro do diálogo, que continua aberto. **Abrir a cópia** tem rodar,
  terminal, VS Code e copiar o caminho, na cópia.
- **Fechar não perde nada**: Esc, o × e o clique fora fecham, e ao reabrir os comentários, o
  rascunho e os vistos estão lá. Se chegou uma entrega nova, o diff é relido (com o diálogo aberto,
  um aviso oferece **Recarregar**).
- Sem cópia (apagada, ou pasta sem git), com erro do git ou sem mudanças, o diálogo diz isso no
  lugar do diff. Juntado ou cancelado, a revisão é só de leitura.
- Dá para descartar as mudanças de um arquivo (com confirmação): um renomeado volta ao nome antigo
  e, se não der para voltar, aparece "Não deu" e o arquivo fica na lista. Nomes com acento, espaço
  ou aspas aparecem certos aqui, em Arquivos e no `@`.

## Cópias dos chats

Com `node_modules`, `.venv` e `target/`, cada cópia pode passar de 1 GB. A cópia de um chat de
projeto continua depois de juntar; ela fica **parada** quando o chat é arquivado sem nada para
juntar, e a de um ajudante quando ele termina. Em Configurações, **Cópias dos chats** mostra o espaço
ocupado ("Medindo…", com o spinner, enquanto mede) e quantas estão paradas, tem **Limpar N** (com o
número; desligado quando não há nenhuma) e a escolha de apagar sozinho depois de nunca, 7 ou 30
dias. O aviso no fim diz quantas saíram e, se alguma não deu, qual e por quê. A limpeza apaga a
pasta e os pontos de volta; o que não tinha sido juntado ganha antes um commit e o branch fica, e a
próxima mensagem ao chat refaz a cópia no mesmo lugar, a partir dele ("A cópia foi recriada a partir
de …"). Sem a pasta original à mão, nada é apagado. No **…** do cartão de um ajudante encerrado,
**Apagar a cópia…** mede o espaço e pergunta antes; arquivar um chat com ajudantes parados oferece
apagar as cópias deles. As cópias que os agentes de um fluxo criam dentro da cópia
(`.claude/worktrees/`) não entram no commit.

## GitHub e Linear

As issues e os PRs ficam ao lado do chat. Nada é obrigatório: sem conexão, o Vitral junta na pasta
como sempre.

- **Conectar.** Em Configurações › **Conexões**. O GitHub é o login do `gh` ("Conectado como
  VictorMenegate · gist, read:org, repo, workflow"; sem login, **Entrar no GitHub** abre o terminal
  com `gh auth login --web`); o Vitral nunca guarda nem lê o token, quem fala com o GitHub é o `gh`.
  O Linear pede uma chave pessoal de API (Linear › Configurações › Conta › Segurança e acesso, com
  Leitura, Escrita e Criar comentários, limitada aos times que você usa): **Conectar…**, colar e
  **Testar e salvar**. **Testar** confere a chave, **Desconectar** a tira do chaveiro, e
  "Mostrar o botão do Linear nas conversas" esconde ou mostra o botão. Embaixo,
  "Isolamento dos chats: ativo" (ou "indisponível", em âmbar, quando o `bwrap` não funciona).
- **A chave no chaveiro.** A chave do Linear vai para o chaveiro do sistema (Senhas e chaves, item
  "Vitral · chave do Linear"; `secret-tool lookup application vitral servico linear tipo
  chave-api`), nunca para arquivo, e só fica na memória do Vitral enquanto ele está aberto. Abrir o
  Vitral com o chaveiro trancado não pede senha: o Linear aparece como "chaveiro trancado", com
  **Destravar**, e só o que você pede (abrir as issues do Linear, Testar, Iniciar agente) destrava.
  Um efeito automático que precisou da chave trancada vira "Não consegui mover a ENG-123" em
  Esperando você, com **Destravar e tentar**. Outros programas da sua conta conseguem ler o chaveiro
  destravado: use uma chave só com essas permissões.
- **Por pasta.** No editor do projeto, a seção **GitHub e Linear** mostra o repositório de cada
  pasta (o remote do GitHub, com a permissão da sua conta) e **Ao aprovar: Juntar aqui | Abrir
  PR**. Em **Mais opções**, o ramo de destino, o prefixo dos ramos (com o sugerido pelos seus PRs
  anteriores), abrir como rascunho, o remote e, num fork, abrir os PRs no repositório original. O
  Linear é por projeto: o time (e, em Mais opções, para que estado a issue vai ao iniciar um agente
  e ao mesclar; "Automático" é o primeiro "em andamento" e o primeiro "concluído" do time).
- **Issues.** O botão do Linear (e o do GitHub, quando a pasta tem issues) no cabeçalho do chat abre
  a lista: Minhas, Do time ou Ciclo atual (no GitHub, Minhas ou Todas), abertas ou fechadas, um
  filtro (Enter com 3 letras ou mais busca no servidor) e o detalhe de cada uma, com o popover do
  estado. **Iniciar agente** cria o chat "ENG-123 · título", com o ramo que o Linear sugere (ou
  `<prefixo>eng-123-…`), e manda a issue como primeiro pedido; no Linear, a issue vai para "Em
  andamento", ganha **um** comentário ("Em andamento no Vitral: ramo …") e fica com você se estava
  sem ninguém. **Só a cópia** cria o chat sem mexer na issue, com ela anexada na caixa. Até 3 de
  uma vez (uma cópia preparada por vez). **Anexar a esta conversa** põe a issue num chat que já
  existe (o chip na caixa e na bolha) e pergunta se liga a issue ao chat. O Mestre consulta com
  `listar_issues` e `ver_issue` e divide o trabalho por issue ("pega a ENG-7 e a ENG-8"). A lista
  não se atualiza sozinha: ao abrir, ao voltar à janela e no ⟳.
- **Entregar por PR.** Com **Abrir PR**, aprovar é publicar: **Revisar e abrir PR** (em Este chat e
  na barra acima da caixa) abre a revisão de sempre com o formulário do PR embaixo do diff (título e
  descrição gerados, com o que muda, como foi verificado e o `Closes` da issue; **Pedir ao chat uma
  descrição**; os arquivos novos; rascunho; **Mesclar quando ficar pronto**), e o botão azul vira
  **Abrir PR** (depois, **Atualizar PR**). O Vitral faz o commit (sem os arquivos copiados da
  origem), confere os segredos e as mudanças em `.github/`, envia o ramo e abre o PR; se o PR já
  existe, atualiza. O chat também abre o PR quando você pede na conversa ("abre o PR e mescla
  quando passar"), pela ferramenta `abrir_pr`.
- **Watch e Ship.** Todo PR aberto pelo Vitral é acompanhado (Watch): a linha de acompanhamento no
  chat ("PR #12 · título · 15/41 checks · acompanhando"), a aba **PR #12** à direita (Visão geral
  com a descrição e a linha do tempo, Arquivos, Checks com o log de cada falha, e o rodapé com o que
  falta para mesclar e o **Mesclar** pelo método padrão do repositório) e, em Esperando você, o
  que pede ação: o CI que falhou (**Mandar ao chat**), a revisão de quem tem acesso ao repositório,
  o comentário de quem não tem (só você decide mandar), o pronto para mesclar e o commit que chegou
  de fora. **Mesclar quando ficar pronto** (Ship) faz o resto sozinho: a falha do CI e a revisão vão
  direto ao chat, a rodada que corrige atualiza o PR sozinha (até 3 rodadas, sem `.github/`, segredo
  nem commit de fora) e, com os checks passando, sem pedido de mudança nem conversa aberta e 90 s
  depois do último envio (os bots de revisão comentam logo), o Vitral mescla o head que ele enviou.
  Um commit de fora, a fila de merge que some ou checks parados há um dia desligam o Ship. O vigia
  consulta um repositório por vez, mais vezes logo depois de um envio e com a aba PR à vista, e
  para sozinho sem rede (a faixa "sem conexão"), voltando quando a rede volta.
- **Depois de mesclar** (pelo Vitral ou por qualquer pessoa): o PR vai para os anteriores (‹ 1/2 ›
  na aba), o ramo remoto sai quando foi o Vitral que mesclou, a cópia recomeça num ramo novo a
  partir da base (com as mudanças sem commit de volta, sem `git stash`), a pasta original limpa anda
  por fast-forward e a issue vai para Concluída (no GitHub, pelo `Closes`; sem ele, a linha oferece
  **Fechar a issue #45**). Fechado sem mesclar, a cópia e o ramo ficam; arquivar um chat com o PR
  aberto pergunta se fecha o PR ou deixa aberto.
- **O que pede confirmação.** As ferramentas do PR (`abrir_pr`, `ver_pr`, `mesclar_pr`) não pedem
  permissão ao Claude Code, então o Vitral decide: o que você pediu na conversa, num turno sem texto
  de terceiros, roda; mesclar agora, mudança em `.github/` e o que veio de um aviso do Vitral, de
  outro chat, de uma issue ou de um comentário viram o cartão **Confirmar** no chat (e em Esperando
  você), com **Confirmar** e **Agora não**. O primeiro PR pedido pelo Mestre também, a menos que você
  tenha pedido a ele na conversa. "Tentar de novo" (e a troca de conta) refaz o turno sem repetir no
  GitHub o que ele já fez.
- **Isolamento.** Os chats que leem texto de terceiros (uma issue anexada, um aviso do PR) e todos
  os chats de uma pasta que entrega por PR rodam isolados, assim como os ajudantes deles, a
  verificação, a preparação e o revisor nas cópias deles: o `claude` roda dentro do `bwrap`, sem o
  barramento de sessão (e com ele o chaveiro e o `secret-tool`), sem o agente ssh, o `~/.ssh`, o
  login do `gh` e as credenciais do git, sem ver o processo do Vitral (que tem a chave do Linear na
  memória) e sem os sockets do X11. O `.git` do repositório (a configuração, os ganchos e o `info/`)
  e a configuração global do git ficam só leitura lá dentro, e o `.git` não pode ser trocado por
  outro; o git do Vitral nessas pastas roda sem `core.fsmonitor` e sem ganchos. Quem envia o ramo e
  fala com o GitHub é o Vitral, fora do isolamento.
- **O que o isolamento não cobre.** Ele protege as credenciais, não os arquivos: o chat isolado
  continua lendo e gravando no seu HOME como você (um `~/.bashrc`, uma pasta acima da cópia), e a
  rede continua aberta (só o que pede login fica de fora). **Rodar**, que abre o app com a sua
  sessão, roda fora dele. Sem o `bwrap` funcionando (ou fora do Linux), os chats "isolados" rodam
  só com o ambiente limpo e com `git push`, `gh` e `secret-tool` bloqueados nas regras: o aviso em
  Conexões diz isso, e o diálogo do Linear avisa que um chat conseguiria ler a chave.

## Contas e uso dos planos

No pé do trilho fica um anel por serviço, o do Claude e o do Codex, com o disco da marca no
meio: o anel de fora é a janela de 5 h e o de dentro, mais fino, a semana, na cor do serviço
até 69%, âmbar de 70 a 89% e vermelho daí para cima; no limite, o anel de fora fica cheio em
vermelho. Com mais de uma conta, o do Claude leva a inicial da conta em uso. Sem o Codex
instalado, o botão dele fica apagado (cinza), e o painel dele oferece **Instalar e entrar**,
que abre o terminal com o instalador oficial e o login (também no menu do logo, **Instalar o
Codex…**). O clique abre o painel de detalhes, à direita do trilho: o plano, as duas janelas
com a hora em que renovam ("renova em 2 h 13 min · 15:59"; uma janela sem leitura fica só com o
rótulo e "sem dado", sem a barra), os limites semanais por modelo, os
créditos extras do mês e, no rodapé, quando o número foi lido, com **Atualizar** (e, discreto,
"não deu para atualizar" embaixo quando a última busca não trouxe número). O uso do Claude se
atualiza sozinho, direto da Anthropic, com o mesmo pedido que o Claude Code faz para o `/usage` e o
login de cada conta: logo ao abrir o painel e a cada minuto com ele aberto, a cada 5 min com ele
fechado (os anéis do trilho também mostram), a cada 15 min com a janela sem foco há mais de 15 min
(30 min depois de 2 h fora) e na hora com **Atualizar**. O token é lido do `.credentials.json` da
conta a cada busca e só vai para `https://api.anthropic.com`; vencido ou recusado, o Vitral não o
renova (quem renova é o Claude Code) e espera, e a falta de rede e o excesso de pedidos fazem
esperar mais antes de tentar de novo, sem aviso. Contam também o que o Claude Code e o Codex
guardam no disco (relido a cada 30 s) e os avisos de limite durante os turnos; o Codex só por
esses dois caminhos.

No painel do Claude também se escolhe a conta padrão, em **Conta padrão**: clicar numa
linha (**Automática**, "a de menor uso em cada turno", ou uma conta, com o disco da marca e os
dois anéis, como no trilho (5 h por fora, a semana por dentro), o e-mail inteiro em cima e o uso
embaixo, "5 h 7% · semana 38%", sem a janela que ainda não foi lida) a torna a padrão, e o check
fica nela; **Entrar com outra conta…** conecta uma nova e o **…** de uma conta extra tem
**Desconectar…**. **Contas…**, no rodapé, abre o diálogo das contas (também no menu do logo,
**Contas do Claude**, ou em **Gerenciar contas…**, no chip da conta na caixa de mensagem). Ele
repete o seletor do popover, maior: em **Conta padrão**, a Automática e uma linha por conta com o
mesmo anel do trilho (5 h por fora, a semana por dentro), o login e o plano, o uso ("5 h 23% ·
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

- Trilho à esquerda, numa peça de vidro própria (a área vazia arrasta a janela, o clique duplo
  maximiza e o clique direito abre o menu da janela), de cima para baixo: o logo (clique ou F10 para
  o **menu do Vitral**, que abre à direita: no cabeçalho, "Vitral" e quantos projetos há, com a pasta
  de dados na dica; depois **Falar com o Mestre**, **Novo chat…**, **Novo projeto…**, **Rotinas** e
  **Importar do Claude Code…**; os **Projetos arquivados**, quando há, cada um com o quadrado apagado e
  **Desarquivar**; **Contas do Claude**, **Instalar o Codex…** quando ele falta, **Configurações** e
  **Memória do Mestre…**; **Abrir a pasta de dados** ↗; com os atalhos de cada um; no rodapé, a
  versão em tênue, "Vitral 0.3.0-rc.1", com o commit na dica ("Compilado no PC" no que não veio do
  GitHub); com uma versão nova pronta, um ponto azul de 8 no canto do logo, a versão na dica e
  **Reiniciar para a 0.3.0** logo acima das Configurações). Logo abaixo, o **Mestre**: o círculo com a
  órbita (Ctrl+M) abre o Mestre no painel ativo, sem mudar a lista; aberto no painel ativo, fica na
  seleção com o aro azul; o número âmbar no canto conta os cartões que esperam você (e um a mais se o
  Mestre parou ou deu erro, em vermelho no erro); sem isso, um ponto azul é resposta nova que você
  não leu e, rodando, o ponto vazado. Depois, os **projetos**, numa área que rola, na ordem que você
  der: arraste um quadrado para cima ou para baixo (uma linha azul mostra se ele vai antes ou depois;
  perto da borda, a área rola; a ordem fica). O clique num projeto abre a lista dele; clicar de novo
  volta à **Visão geral**. O quadrado do projeto em que está o chat aberto ganha um aro fraco; o do
  projeto aberto na lista, o aro inteiro; o ponteiro em cima acende o quadrado suave; um ponto mostra
  erro em vermelho, pedido ou parada em âmbar, resposta nova em azul e, vazado, algo rodando. Soltar
  um chat (da lista ou a aba de um painel) num projeto, ou em Avulsos, o move para lá: enquanto ele
  está por cima, uma faixa diz "Mover para X"; quando há algo a dizer (a cópia continua ligada à
  pasta de antes), pergunta antes; a recusa (o chat está trabalhando, o projeto está arquivado) vira
  aviso. O clique direito num projeto abre o menu dele. Embaixo deles, fixos, **Avulsos** (o balão;
  no clique direito, **Novo chat avulso** e **Projetos arquivados…**) e o **+** do projeto novo.
  No pé, o sino do que precisa de você e os anéis de uso do Claude e do Codex (o logo e os anéis
  ficam sobre um disco preto, que clareia com o ponteiro em cima). O número do sino é o mesmo de
  "Esperando você" (pedidos, erros, paradas que pedem Retomar e os cartões do Mestre), em vermelho se
  algo falhou e âmbar nos outros casos; só com respostas novas, um ponto azul. Quando o número aumenta
  (um pedido, uma parada ou um erro novo), o sino pulsa uma vez com um anel âmbar (vermelho se algo
  falhou); abrir o app, o número cair ou marcar como lido não pulsam, e com as animações desligadas
  nada pulsa. O clique abre **O que precisa de você**: cada linha com o quadrado do projeto e,
  embaixo, a mesma frase e o mesmo sinal da prévia da lista (o ponto na cor e o rótulo: "Permissão ·
  Bash · npm test", "Plano para aprovar", "Parada · Vitral fechou", "Erro · …"): a seção **Do Mestre**
  (os cartões que esperam você e os pares de chats parados no freio de ciclo), os pedidos (com Negar e
  Permitir, ou **Permitir mesmo assim** em vermelho quando o comando tem um trecho arriscado; um plano
  abre a conversa para ler antes), as paradas (Retomar), os erros (Tentar de novo) e as respostas
  novas (marcar como lida), com o número do painel quando a conversa está aberta; clicar numa linha
  abre a conversa (Ctrl+clique, ao lado). No rodapé, os botões **Marcar tudo como lido** e **Ver na
  lista** (volta à visão geral, sem busca e sem arquivadas, e mostra a seção); cabeçalho, seções,
  linhas e o ícone do rodapé ficam na mesma guia, a 16 da borda. Ctrl+G vai direto à próxima
  pendência (sem outra, ao primeiro cartão aberto do Mestre).
- **Ctrl+B recolhido.** A lista e o trilho somem e a conversa vai até a borda. No lugar deles ficam
  o logo, no começo das abas do 1º painel, com o selo do sino (o número ou o ponto, que pulsa como
  o sino), e os anéis de uso, de 24, no cabeçalho do 1º painel, logo depois do logo (o do Codex só
  quando está instalado e tem uso lido; os popovers abrem para baixo). O menu do logo ganha, em
  cima, **Mostrar a lista**, **O que precisa de você** (abre a mesma caixa do sino) e os projetos
  (até 9, com Ctrl+1…9, e **Todos os projetos…**); escolher um projeto abre a vista dele e traz a
  lista de volta. Ctrl+B de novo volta tudo.
- **Ícone e cor do projeto.** Cada projeto tem um ícone do Lucide e uma cor. Sem escolha, o ícone
  sai do conteúdo da primeira pasta (um `Cargo.toml` vira chip, um `manage.py` ou `Dockerfile` vira
  servidor, um `next.config` vira site, só `.md` vira documento) e a cor, do nome, numa paleta sem
  as cores de estado. Para trocar, o chip do ícone em **Editar projeto** (também pelo **Ícone e
  cor…**), que abre a prévia, a busca de ícone pelo nome ("loja", "banco"), 60 ícones e 8 cores, com
  o escolhido na cor do projeto, e **Automático**, que volta ao palpite. A escolha fica no projeto e
  vale no trilho, na lista, nos diálogos e em Arquivos.
- **Conversas lado a lado** (até 4, conforme a largura): arraste uma conversa da lista (ou a aba de
  um painel) para a borda esquerda ou direita de um painel para abrir ao lado, ou para o meio para
  trocar. A forma diz o que vai acontecer: meio painel em azul para "Abrir ao lado", uma barra na
  borda para "Mover para cá" e o painel inteiro para "Trocar a conversa" (com "Já há 4 lado a lado"
  quando não cabe outro); a aba que você arrasta fica apagada e tracejada. Também dá com Ctrl+clique
  ou o botão "abrir ao lado" no cartão. O painel que abre ao lado entra com um fade (a largura não
  anima). Com vários painéis, cada um tem um número em forma de tecla (na aba, na lista e na
  direita), Alt+1…4 vai para ele, e o ativo tem a aba azul e a borda e um halo azuis, que acendem
  suave (180 ms; apagam em 120); clicar na aba (ou em qualquer parte do painel) ativa o painel e põe
  o cursor na caixa dele. O × fica dentro da aba, no lugar do ponto de estado (azul rodando, âmbar
  pedido ou parada, vermelho erro): aparece no hover, no foco e sempre no painel ativo; o clique do
  meio na aba também fecha; fechar pelo × ou pelo clique do meio põe o cursor na caixa do painel
  que fica, como o Ctrl+W. A aba entra no Tab: Enter vai para a caixa, Delete fecha e Shift+F10
  abre o menu. As divisórias não têm linha fixa: o indicador aparece ao passar o mouse, e é só
  arrastar para redimensionar. A lista da esquerda recolhe no botão ao lado das abas (ou Ctrl+B, que
  esconde o trilho junto). Ctrl+W fecha o painel ativo (e o cursor vai para a caixa do que fica),
  Ctrl+PgUp/PgDn vão para o vizinho e Ctrl+Shift+PgUp/PgDn trocam o painel de lugar.
- **Cabeçalho do painel.** Uma linha só (44): a aba e os botões, modo foco, **⋯** (**Mais opções da
  conversa**, o mesmo menu do clique direito na aba, como diz a dica) e, no último painel, o da
  direita, com a dica do que ela mostra (**Distribuído**, **Este chat**, **Ajudantes**, **Esta
  conversa** ou **Arquivo aberto**) e o Ctrl+J. Com a direita fora da vista (fechada, ou a gaveta
  dela fechada) e chats que pedem você, o botão leva no canto um selo com o número, âmbar, ou
  vermelho se um caiu, e a dica diz quais ("1 em revisão, 1 parada"; também "pedindo permissão" e
  "com erro"). Quando o botão vai para o **⋯**, o item vira **Mostrar Este chat** (ou **Mostrar o
  arquivo aberto**, **Mostrar os ajudantes**…), com o mesmo número tingido. Ligados, ficam num cinza
  mais claro, nunca azul. No chat de projeto e na conversa antiga, o quadrado do projeto na aba abre
  o menu do projeto; no avulso, fica o balão; no ajudante, o ↰ que abre o chat dele; no Mestre, a
  órbita. A dica da aba diz onde o chat está ("Chat 12 · loja · site · cópia t12", "· direto", "Avulso
  · chat 3", "Ajudante de “Checkout”"). O chat que veio do Mestre leva, no cabeçalho, o selo "do
  Mestre ↩", que abre o Mestre. O **⋯** tem **Nova conversa ao lado**, **Mover para a
  esquerda/direita**, **Fechar os outros painéis**, **Fechar este painel**, o menu da conversa
  (renomear, fixar, mover para projeto, arquivar, abrir a pasta fora do app) e, embaixo, o que a aba
  não mostra: **Cópia t2**, **Projeto loja** quando o quadrado não está na aba e **Abrir o chat**
  quando o ↰ não está. O custo ("US$ 0,42") está no anel do contexto da caixa. Uma linha separa o
  cabeçalho das mensagens só quando há mensagens roladas por baixo dele. Em painel estreito o modo
  foco e depois o painel da direita vão para o **⋯**; no mais estreito, com vários painéis, o número
  toma o lugar do símbolo da aba. Nenhum texto é cortado no meio.
- **Painel sem conversa:** "Fale com o Mestre" com **Abrir o Mestre**, **Novo chat** e **Novo
  projeto**; dá para soltar ali uma conversa arrastada da lista.
- **Zoom:** Ctrl + roda do mouse (ou do touchpad), Ctrl+= / Ctrl+- e Ctrl+0 (volta a 100%)
  aumentam e diminuem a interface inteira de 10 em 10%, de 70% a 200%, como num navegador; a
  porcentagem aparece no alto da janela por um instante. Tudo cresce junto, inclusive as larguras
  das partes: a 150% a lista normal tem 468 px, a direita 570 e cada conversa pede no mínimo 420,
  então numa janela que não comporta a direita e depois a lista viram gaveta, como numa janela
  estreita. O que você arrastou nas divisórias fica guardado a 100% e acompanha o zoom (a lista
  de 240 a 100% vira 360 a 150%, e a divisão entre as conversas se mantém). Vale para o app todo
  e fica guardado em `~/.config/vitral/interface.json`.
- **Visual:** o fundo é um vidro fosco, mais opaco, em que o papel de parede aparece apagado atrás
  do trilho e da lista; nas frestas entre as peças e na margem ele fica num chumbo azulado escuro, e
  as peças ficam mais claras que o fundo, para se destacarem. O trilho, a lista, os painéis, o
  seu balão e os cartões da direita e de entrega têm o reflexo do vidro (a linha de luz e o brilho no
  topo). O vidro azul-marinho fica no fundo (trilho, lista, painéis, diálogos e as gavetas); o
  preto entra no que vai por cima e por dentro, para dar profundidade: popovers, menus, dicas, avisos
  e a barra de ações das mensagens são quase pretos (#0c0e13, opacos, com a borda clara fina e a
  linha de luz no topo); os blocos de código, as saídas de comando, o comando da verificação e o
  diff do Revisar ficam num preto mais fundo (#08090c), com a mesma cor de sintaxe; a caixa de
  mensagem vai do azul, à esquerda, ao preto; e o logo e os anéis de uso do trilho ficam sobre um
  disco preto, para a cor de cada marca saltar. Dentro de um popover, o que no vidro é um recuo
  escuro (o trilho do segmentado do esforço, o recado do Pedir ajuste) vira um claro leve. O
  logo é um vitral redondo: o completo (`assets/vitral.svg` e PNGs de 64 a 256 px) no menu de
  aplicativos e na tela de recuperação, o pequeno (`assets/vitral-pequeno.svg`, feito para 16 a 32
  px, e o PNG de 48) no trilho e nos ícones pequenos. Popovers e menus entram com um fade e 6 px
  vindos do lado do botão que os abriu, e todos os flutuantes têm uma linha de luz de 1 px no topo,
  como o reflexo de um vidro; as gavetas da lista e da direita entram deslizando 24 px da borda, em
  180 ms, com o véu em fade junto, e fecham secas. O véu dos diálogos cobre só a janela, não a
  margem da sombra. A pílula do segmentado (Conversas · Arquivos, o esforço) desliza até a
  opção nova; os botões redondos (enviar, +, sino) acendem suave com o ponteiro em cima; os anéis de
  uso e de contexto deslizam até o valor novo; o sino pulsa quando chega algo que precisa de você;
  e o ponto de "rodando" respira enquanto um spinner gira. Listas, menus e cartões continuam com o
  hover seco. "Em revisão" fica no azul do rodando (a pílula, o ponto e a parte da barra da direita);
  o âmbar é só do que precisa de você. Com o ponteiro fora da janela, nada fica em hover nem com a
  dica aberta. Na direita tudo começa na mesma coluna, a
  24 da borda do painel (o texto de Chats, das seções e dos cartões e o ícone das concluídas), e o
  espaço é medido no que se vê: 8 entre cartões, 16 antes de cada seção
  e, dentro do cartão, 8 entre as linhas e 12 da borda ao título e ao botão.
- **Janela estreita:** quando falta largura, o painel da direita e depois a lista saem do lado e
  abrem por cima, como gaveta (Ctrl+J, Ctrl+B, Ctrl+K e os botões de sempre); Esc ou um clique
  fora fecham. A gaveta entra deslizando da borda dela, com o fundo escurecendo junto; fecha seca.
  Os botões de janela seguem o layout do GNOME (no Fedora, só o ×).
- **Lista** numa peça de vidro própria, em três vistas, lembradas ao reabrir (com o último chat
  aberto): a **Visão geral** ("Esperando você", "Rodando agora", "Fixadas" e os 15 **Recentes** de
  todos os projetos, com o nome do projeto antes da prévia; na busca, todos os que casam), um
  **projeto** (o cabeçalho com o quadrado, o nome, a descrição numa linha, ou o começo do README em
  itálico, o **…** com o menu e um chip por pasta ligada; as abas **Conversas · Arquivos** num
  segmentado; as faixas Recentes e Mais antigas) e os **Avulsos**. Fora da visão geral, "← Visão
  geral" no topo e, quando outro projeto espera você, o "+N" âmbar que leva de volta a ela. O chip de
  uma pasta que sumiu fica em vermelho ("não encontrada") e o clique abre **Localizar pasta…**; o de
  uma que existe abre os arquivos dela. A lista vazia de um projeto diz "Comece um chat aqui, ou peça
  ao Mestre com #chave", com **Pedir ao Mestre** (abre o Mestre com `#chave ` na caixa). Em cima, a
  busca (Ctrl+K; acha também no texto das mensagens e nas conversas arquivadas) e o **+ Novo chat**,
  que faz o mesmo que o Ctrl+N. Um ponto âmbar em Conversas avisa, de dentro de Arquivos, que algo
  espera você. Tudo na lista começa a 16 px da borda da peça (o texto dos cabeçalhos de seção, o
  quadrado do projeto, a lupa da busca e o primeiro ícone do rodapé); a seta das seções fica à
  esquerda dessa guia. A busca diz só "Buscar"; o que ela acha e o Ctrl+K ficam na dica do campo
  (que não aparece enquanto se digita nele). Na busca, quando o termo não está no título, a prévia do
  cartão mostra o trecho que casou ("Você: valide o **frete** grátis…", com o termo em forte; de um
  ajudante, "#2: …"), e as arquivadas que casam aparecem no fim, numa seção **Arquivadas** (com o
  número) e com o cartão tênue. Fora da busca, elas continuam só em Arquivadas. Estreitando a lista
  (ou com o zoom alto, porque a conta é a mesma), ela abre mão do que é menos importante antes de
  cortar o título: abaixo de uns 290 px (a 100%), os ajudantes perdem o selo de pedido ou erro (o
  ponto já diz) e, no hover, fica só o **…**; abaixo de 275, as abas ficam só com o ícone e a prévia
  fica com o rótulo curto do motivo: o do próprio chat sai (o ponto diz qual é), e o de um ajudante
  fica só com o número e o "(+N)" ("#5 (+1) · Vitral fechou"); abaixo de 260, a busca de arquivos diz
  só "Buscar" e **Retomar tudo** e **Arquivar estas** ficam só com o ícone. Os horários continuam à
  vista. A barra de rolagem é fina, colada na borda da peça, não cobre os cartões e aparece e some
  sem fade, 1,2 s depois de rolar.
- Seções, cada uma com o próprio cabeçalho: **Esperando você** (o número é o mesmo do sino:
  pedidos, erros e paradas; respostas novas não contam, viram ponto; na visão geral, ela começa
  pelas linhas do Mestre, uma por cartão aberto, "Divisão 3 espera Começar (2 partes)", "Criar o
  projeto Receitas?", "Quer lembrar: …", e uma por par de chats parado no freio de ciclo, cada uma
  com **Ver**, que abre o Mestre rolado até o cartão; pedidos, paradas e erros vêm antes das
  respostas novas, e com duas paradas ou mais aparece **Retomar tudo**), **Rodando agora** (o Mestre
  também, quando roda), **Fixadas** e as faixas de data (**Recentes**, até 7 dias, e **Mais
  antigas**; o tempo de cada conversa já está no cartão). Em Esperando você os cartões são neutros
  como os outros (selecionado em azul); o âmbar fica no ponto do quadrado, no ponto da prévia e no
  número da seção. Fixadas e as faixas recolhem no clique do cabeçalho (a seta fica à esquerda do
  nome, e o hover clareia os dois); Mais antigas começa recolhida e tem **Arquivar estas**, que
  arquiva na hora e mostra "3 conversas arquivadas" com **Desfazer** (que devolve só as que ela
  arquivou). O **Arquivar** do menu do chat também mostra "Conversa arquivada" com Desfazer (um chat
  de projeto que nunca rodou sai do Vitral: "Chat vazio apagado"); desarquivar não avisa.
- **Cartão de conversa**: o quadrado do projeto (ícone e cor dele; o balão no avulso) com o ponto
  do estado da família no canto (vermelho erro, âmbar pedido ou parada, azul resposta nova, aro azul
  rodando; a dica diz "loja · chat 12"), o título, a prévia com o motivo no ponto do sinal e no
  rótulo ("Permissão · Bash · npm test", "Plano para aprovar", "Parada · Vitral fechou", "Erro · …";
  quando é um ajudante, "#5 parada · Vitral fechou"; a mesma frase do sino; parado, o estado do
  chat: "Pronto para revisar" com o olho âmbar, "Para juntar: +12 −3", "Juntado", "A cópia não foi
  preparada", "Na fila para preparar a cópia", "2 ajudantes rodando"; a resposta nova só com o
  texto, porque o ponto azul e o título em forte já dizem; rodando, "#3 · Edit · README.md"; as
  prévias começam com maiúscula, "Pensando…", "Na fila, esperando vaga") e, à direita, o horário (ou
  há quanto tempo roda), o número do painel e o ponto de não lido (só quando o ponto do quadrado não
  é já o azul da resposta nova). O chat que o Mestre criou (ou com um pedido dele aberto) leva a
  órbita pequena (a dica diz a parte e a divisão). No hover, o horário dá lugar a duas ações:
  **Abrir ao lado** (ou Ver o pedido do ajudante, ou Retomar) e **…**; fora do hover, o título usa o
  lugar delas, e só o cartão sob o ponteiro (ou realçado pelo teclado) corta o título para
  mostrá-las. O clique direito ou o **…** abrem o menu do chat: abrir ao lado, renomear, fixar,
  **Mover para projeto…** (os projetos e Avulsos), arquivar e abrir a pasta no VS Code ou no
  terminal. Clique duplo também renomeia; Ctrl+clique ou o clique do meio abrem ao lado.
- **Ajudantes** embaixo do chat, numa árvore: um traço fino sai do centro do quadrado do pai e
  passa por um ponto de 8 por ajudante, na cor do estado (o sinal quando roda, pede, falhou ou parou
  e pede Retomar; o aro de quem roda fica solto, com o traço parando antes dele); "#2 título" fica na
  coluna do título do pai, e a palavra do estado vai para a dica do ponto ("Em revisão" e a
  explicação). À direita só o que pede ação (o selo "permissão", "plano" ou "erro"), a duração de
  quem roda e o número do painel. Fora do chat aberto, os aprovados e cancelados ficam em **Mais N
  concluídas**, no fim do ramo (o traço termina na seta).
- **Teclado na lista**: ↓ na busca entra na lista; ↑/↓, Home e End andam pelos cartões; → mostra
  as concluídas da família (e os ajudantes, nas faixas de data), ← esconde ou volta ao chat pai;
  Enter abre (e, com pedido, põe o cursor na caixa), Ctrl+Enter abre ao lado, F2 renomeia e Esc
  volta à busca. Na busca, Esc limpa e, vazia, volta à caixa do painel.
- **Rodapé**, só quando há o que mostrar: **Arquivadas** (o ícone com o número; liga e desliga a
  vista das arquivadas, que mostra também os chats dos projetos arquivados; a busca acha as duas) e,
  com algo rodando, quantos agentes rodam no Vitral inteiro ("2 rodando · 1 na fila"; o clique
  volta ao topo da lista). Na aba Arquivos, quantos arquivos a pasta tem.
- Aba **Arquivos**: os arquivos de uma pasta numa árvore (pastas recolhidas, menos o caminho até o
  arquivo aberto); com mais de uma pasta (ou com o chat aberto numa cópia), um seletor em cima ("site
  | site-docs | Cópia deste chat"); com uma só, o caminho dela e o ↻ para reler. Com busca, uma lista
  plana sem limite, com o trecho achado em destaque e a pasta cortada no meio. Clicar abre no painel
  da direita: a aba com o nome e o ícone do tipo, e uma barra com a pasta, o número de linhas e a
  linguagem, **Mencionar**, **VS Code** e o **…** (copiar o conteúdo ou o caminho, mostrar na pasta).
  O markdown sai renderizado; o código, com cor e número de linha, rola para os lados. Arquivo grande
  (mais de 400 KB), binário, vazio ou que sumiu dá um aviso no lugar, com **Abrir no VS Code** ou
  **Fechar**. Na lista, no hover, **@** menciona o arquivo na conversa (fora do Mestre) e **…** (ou o
  clique direito) abre no VS Code, copia o caminho ou mostra na pasta. A lista é relida a cada 20 s
  em segundo plano, sem travar a tela.
- Menu do projeto (clique direito no trilho, **…** do cabeçalho do projeto, quadrado da aba): com o
  quadrado, o nome e o caminho em cima, **Editar projeto…**, **Novo chat neste projeto**; por pasta
  (com o nome dela quando há várias), **Rodar** (ou **Definir como rodar…**), VS Code, terminal e
  gerenciador, ou **Localizar pasta…** na que sumiu; e, no fim, **Arquivar projeto…** (**Desarquivar
  projeto** num arquivado) e **Apagar projeto…**, em vermelho. **Arquivar** age na hora, com
  **Desfazer** ("Projeto site arquivado · 3 conversas em Arquivadas"): o projeto sai do trilho, do
  Ctrl+1…9, do Novo chat e das rotinas (que não rodam enquanto ele está arquivado; o dia passa
  marcado, então desarquivar à tarde não dispara a das 8h), e os chats dele vão para **Arquivadas**
  sem perder nada. Com chats trabalhando, pergunta antes ("Parar e arquivar loja?", o Enter fica com
  **Continuar trabalhando**); com pedidos do Mestre abertos nos chats dele, ou com uma divisão
  esperando o Começar com parte nele, também pergunta: os pedidos são cancelados (a conversa e a
  cópia ficam; o Mestre recebe uma nota) e a parte sai da divisão; o Desfazer só desarquiva. Com um
  chat sendo conferido, juntado ou preparado, não arquiva e diz qual esperar. Ele volta pelo menu do
  Vitral (**Projetos arquivados**, com **Desarquivar**), pelo **Projetos arquivados…** de Avulsos,
  por **Desarquivar o projeto loja** no menu de um chat dele em Arquivadas ou por um chat novo com
  `--nova`. **Apagar projeto…** pergunta antes, em vermelho, o que sai: o projeto e os chats dele saem
  do Vitral de vez (com as mensagens, as filas, as rotinas e as notas), as cópias vão para a Lixeira
  do GNOME (`gio trash`, nunca `rm`; com os branches no repositório), e as pastas ligadas ficam como
  estão; a pasta própria vai para a Lixeira só quando o projeto não tem pasta ligada. Se a lixeira
  recusa, nada sai ("Nada foi apagado: …"); com um chat do projeto trabalhando, não deixa. O aviso do
  fim diz o que saiu, com **Abrir a Lixeira** quando algo foi para lá.
- **Rotinas:** um pedido que o Vitral manda sozinho num horário e dias da semana, ao Mestre ou a um
  chat novo num projeto (com o Vitral aberto; se estava fechado, roda ao abrir). O diálogo abre na
  lista (o quadrado do projeto, ou a órbita do Mestre, o pedido, "loja · Seg a Sex às 08:00 ·
  próxima: amanhã" e a chave de pausar; no hover, **Rodar agora**, **Editar** e **Apagar**, que tem
  Desfazer). A de um projeto arquivado diz "projeto arquivado, não roda", e o Rodar agora fica
  apagado. **Nova rotina** (ou o clique numa rotina) troca para o formulário: **Para quem** (o
  **Mestre** primeiro e os projetos com o quadrado e a descrição), **O que pedir** em várias linhas
  (Enter salva, Shift+Enter quebra), a hora num campo de hora e minuto (↑/↓ em cada parte) e os dias
  em chips com **Dias úteis** e **Todo dia**. O que falta aparece no campo, e o **Criar rotina** diz
  na dica o que falta; a seta volta à lista. Sem rotinas, **Usar exemplo** preenche uma.
- **Configurações** (Ctrl+,), gravando na hora, valem para todos os projetos e chats (o caminho do
  `config.json` fica na dica do subtítulo), em sete grupos: **Mestre** (o modelo, **Mostrar a
  divisão antes de começar**, **Juntar sozinho quando a verificação e o revisor aprovarem** e
  **Resumo ao voltar depois de**), **Chats** (o modo de permissão padrão, o modelo dos chats e
  **Arquivar chats parados depois de**: nunca, 3, 7 ou 30 dias, só os chats já lidos e não
  fixados, sem fila, sem pedido aberto e sem nada para juntar), **Trabalho** (chats trabalhando ao
  mesmo tempo, ajudantes por chat, rodadas de ajuste antes de chamar você e os servidores MCP do seu
  Claude Code), **Revisão** (o revisor independente e o modelo dele), **Cópias dos chats**,
  **Interface** e **Atualizações**. Em Interface, **Animações** (Seguir o GNOME ou Desligadas) vale
  para o app todo e muda na hora; seguindo um GNOME sem animações, a linha diz isso.
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
  conversas, por projeto, com quando falaram e quantas mensagens têm). **Renomear** já abre com o
  título selecionado e não salva vazio.
- Notificação do sistema, só com a janela em segundo plano, quando um chat termina, quando um chat
  pede permissão, quando o Mestre propõe uma divisão ou quando um chat cai e o pedido volta a quem
  pediu: com o ícone do Vitral, "projeto · chat" no título e uma linha só ("Precisa da sua permissão:
  Bash, $ cargo test", "Vou dividir assim: …", "Terminou: …", "Parou com erro: ..."). Clicar nela abre
  a conversa no painel ativo e traz a janela para a frente (ou o GNOME avisa que o Vitral está
  pronto). Uma notificação nova do mesmo chat substitui a anterior em vez de empilhar. A permissão se
  responde dentro do app, nunca na notificação.

Atalhos: `Ctrl+M` o Mestre, `Ctrl+N` novo chat (na vista do projeto, nele; na visão geral, a
paleta), `Ctrl+Shift+N` novo projeto, `Ctrl+K` buscar (`↓` entra na lista; nela, `Enter` abre,
`Ctrl+Enter` abre ao lado e `F2` renomeia), `Ctrl+J` painel da direita, `Ctrl+B` lista e trilho,
`Alt+1…4` painel N, `Ctrl+1…9` abre o projeto N do trilho, `F10` menu do Vitral, `Ctrl+,`
configurações, `Ctrl+G` próxima pendência, `Ctrl+W` fecha o painel, `Ctrl+PgUp/PgDn` painel vizinho,
`Ctrl+Shift+PgUp/PgDn` muda o painel de lugar (na aba: `Enter` vai para a caixa, `Delete` fecha,
`Shift+F10` abre o menu), `Ctrl+End` vai ao fim da conversa, `F7`/`Shift+F7` próxima e anterior
mudança na revisão (nela, `Ctrl+PgUp/PgDn` vão ao arquivo anterior e ao seguinte, e `C` ou `Enter`
comentam as linhas escolhidas), `Shift+Tab` troca o modo, `Esc` para o turno (ou fecha a gaveta),
`Enter` envia, `Shift+Enter` quebra a linha, `Ctrl+roda`, `Ctrl+=` e `Ctrl+-` zoom, `Ctrl+0` zoom
normal. `Ctrl+Shift+M` (modelo) e `Ctrl+E` (esforço) abrem e fecham os popovers da caixa do painel
ativo (no do esforço, ←/→, Home e End mudam o nível, Espaço liga o Ultrathink, U liga o Ultracode e
Enter fecha (as teclas estão no rodapé do popover); Esc fecha sem parar a conversa); no Revisar,
`Ctrl+Enter` aciona o botão azul do rodapé.

## Compilar e rodar

Para usar no dia a dia, instale pelo release, sem compilar: `curl -fsSL
https://github.com/VictorMenegate/vitral/releases/latest/download/instalar.sh | sh` (depois ele se
atualiza sozinho; ver **Versões e releases**).

```sh
vitral                                  # abre o app (no último chat, ou no Mestre)
vitral ~/Projetos/loja                  # o projeto dessa pasta, ou o Novo projeto com ela ligada
vitral --nova loja "o que fazer"        # um chat novo no projeto loja, já com a mensagem
vitral --nova mestre "como estão os projetos?"
cargo test                              # testes (git, motor, revisor, processos, dados e migração, importação, atualização, interface, --instalar, lançador do Windows)
ferramentas/bancada.sh roteiro.txt prints/ ./target/debug/vitral [pasta]   # teste visual isolado
AMOSTRA=prs ferramentas/bancada.sh roteiro.txt prints/ ./target/debug/vitral # a bancada com um PR em cada estado, sem rede
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
Vitral antigo) ou liga (depois, abre o Novo projeto com ela). `FUNDO=branco` (ou `FUNDO=<imagem>`)
troca o papel de parede e `ZOOM=1.5` abre com esse zoom. `DADOS_NOVOS=1` abre como primeiro uso
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
na pasta do chat (o ajudante, em `notas-do-t<N>.md`), antes das ferramentas; leva 10 s com
`[demora]`; com `[ferramenta <Nome> <detalhe>]` (Read, Edit, Write, Bash, Grep, Glob, WebFetch,
Task…) finge usar a ferramenta, mandando o `tool_use`, esperando 4 s (ou os segundos de
`[ferramenta … | 8]`) e mandando o `tool_result`, para dar de ver a linha do turno ("Lendo
motor.rs…", "Rodando: cargo test…") sem gastar a API; executa os `[mcp <nome> <json>]` da mensagem
(fora das linhas citadas com "> ") como chamadas de verdade ao MCP do Vitral; faz o revisor (com `--json-schema` devolve "aprovar", ou
"ajustar" com `[revisor-ajustar]`); entrega sozinho o pedido do Mestre num chat de projeto
(`[nao-entregar]` desliga); aprova sozinho o relatório de um ajudante (`[nao-aprovar]` desliga;
`[ajuste-uma-vez]` pede ajuste na rodada 1); responde a "Pergunta do ajudante #N"; no
`--fork-session`, copia a sessão original para a nova ao lado dela (só nesse `CLAUDE_CONFIG_DIR`); e
grava os argumentos, o ambiente e as chamadas em `$XDG_STATE_HOME/claude-falso.log`. No roteiro,
`roda <comando>` (saída em `roda.log`, sem comentário na linha; os XDG são os da bancada) e `reabre
[comando]` (derruba o Vitral com SIGKILL, roda o comando com ele fechado e abre de novo).
`ferramentas/conferir-snapshot.sh` é a conferência manual do prompt por arquivo (chama o Claude: dois
turnos curtos). Duas bancadas ao mesmo tempo precisam de `BANCADA=N` diferentes e de pastas diferentes
(`XDG_CACHE_HOME=~/.cache/vitral-trabalho/bancada-x BANCADA=5`; a bancada fica em
`$XDG_CACHE_HOME/vitral-bancada`); com o número ou a pasta já em uso, ela recusa começar. `python3 -I
ferramentas/demo_vitral.py <pasta> [--16]` cria uma pasta do Vitral antigo de demonstração (4
projetos, 11 conversas, 8 tarefas em todos os estados, rotinas; com `--16`, mais 12 projetos), sem
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
(nada em repouso): é a conferência do custo dos efeitos. `VITRAL_SEM_ANIMACAO=1` abre como se as
animações do GNOME estivessem desligadas. A vitrine (`--vitrine`) ganhou as seções **Movimento**
(um botão para cada efeito) e **Espaçamento** (a escala e as guias da lista, da direita e da
conversa).

Precisa do `claude` (Claude Code) no PATH ou em `~/.local/bin`, do `git`, e no Fedora de
`libxkbcommon-devel libxkbcommon-x11-devel wayland-devel vulkan-loader-devel fontconfig-devel
freetype-devel libxcb-devel` para compilar. Das dependências em Rust, o `chrono` entrou para a hora
das Rotinas (o `TimeField` do kit); já vinha no `Cargo.lock` pelo vendor.

**No Windows** (10 1809 ou mais novo, x64): precisa do **Git para Windows**
(as cópias, a verificação e o hook rodam no Git Bash dele, achado como o Claude Code acha) e do
**Claude Code nativo** (`irm https://claude.ai/install.ps1 | iex`, o `claude.exe` em
`%USERPROFILE%\.local\bin`); o que faltar aparece como aviso no Mestre, e uma pasta de projeto
dentro do OneDrive também. Os dados ficam em `%LOCALAPPDATA%\vitral` (o cache em `cache\` dentro
dela) e a configuração em `%APPDATA%\vitral`. As fontes vão dentro do binário nos dois sistemas. A
janela tem o fundo acrílico do sistema (`VITRAL_FUNDO=mica`, `transparente` ou `opaco` troca), e o
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
  antigo; 2, os dados globais do Mestre), para a volta de versão não abrir dados que não entende.
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
  `app/` mora dentro da pasta de dados do Mestre (`dados::PASTA_DO_APP`), e nada dos dados mexe
  nela: o Começar do zero afasta tudo menos `app/`, e o `estado.json` do app não é o dos dados.
- **Desenvolvimento**: o `ferramentas/instalar-dev.sh` compila este checkout em release (pelo
  `compilar.sh`) e instala no mesmo layout como `<próxima>-dev.<commit>` (`VITRAL_VERSAO` na
  compilação; `.modificado` no fim com mudança sem commit). Pelo semver isso é menor que a
  próxima, e o release dela o substitui. Para o dono, instalar é só pelos releases.

## Arquivos

| Arquivo | O que faz |
| --- | --- |
| `src/motor.rs` | Chats, projetos (a API deles: criar, mudar, arquivar, apagar, reordenar, localizar pasta, mover chat), os pedidos de cada chat (as tarefas), turnos, fila, vagas e trabalho pesado, contas, rotinas, retomadas e novas tentativas, Ultracode e agentes em segundo plano, a cópia na primeira mensagem, a junção, os ajudantes e a revisão |
| `src/motor/mestre.rs` | O Mestre no motor: o turno dele (prompt, catálogo, referências, fila com o usuário primeiro, passagem de sessão), as ferramentas que só ele tem, os cartões (divisão, sugestão de projeto, memória), a entrega dos chats de projeto, a junção sozinha das entregas verdes, o freio de ciclo e o resumo ao voltar |
| `src/motor/importacao.rs` | A importação no motor: um chat por conversa do Claude Code, a da primeira abertura, o menu e a marca de quando ela continua no Vitral |
| `src/dados.rs` | Pasta de dados global (`~/.local/share/vitral`, a configuração em `~/.config/vitral`): caminhos, gravação que aguenta queda de energia (temporário com fsync, `.anterior` por link duro, rename e fsync da pasta), o gravador em segundo plano (que guarda a gravação que falhou para tentar de novo), `carregar` com volta ao `.anterior`, as notas e a memória, a `normalizar`, a pasta própria dos projetos (criada com git, atômica), a chave do nome de um projeto, o `formato` dos dados e a `DADOS_GLOBAIS` |
| `src/migracao.rs` | Migração do Vitral antigo (o `.vitral/` de cada pasta de trabalho) para os dados globais: uma vez só, no primeiro uso, sem escrever nada nas pastas antigas, com o `estado.json` gravado por último; adiada com o Vitral antigo aberto |
| `src/instancia.rs` | Um Vitral só por usuário: a trava (`flock` em `~/.local/share/vitral.trava`), o socket (`$XDG_RUNTIME_DIR/vitral/vitral.sock`, pasta 0700) por onde uma segunda abertura entrega o pedido e sai, a `Abertura` (`vitral <pasta>`, `--nova`) e a reexecução da tela de recuperação |
| `src/importar.rs` | Leitura das conversas do Claude Code (só leitura): a listagem com título, datas, mensagens e se está aberta, o histórico como itens da conversa, o projeto de destino e o registro do que já veio |
| `src/claude.rs` | Processo do Claude Code em `stream-json` (argumentos e ambiente do turno), leitura dos eventos, versão do Claude Code e conferência da sessão |
| `src/mcp.rs` | Servidor MCP local com as ferramentas do Vitral, as rotas por papel e a tabela de quem pode cada ferramenta (com a regra do dono) |
| `src/revisor.rs` | Revisor independente e o hook que roda os testes quando o chat para (no Linux, um script do bash; no Windows, o próprio Vitral com `--hook-verificacao`, em exec form) |
| `src/processos/` | Todo processo que o Vitral abre (`novo_comando`, sem janela de console no Windows) e os que ele para por inteiro: escopos do systemd ou grupos de processos (`unix.rs`), Job Objects (`windows.rs`) |
| `src/sistema/` | O que abre fora do Vitral, nos dois sistemas: editor, terminal, Lixeira, gerenciador de arquivos, o bash dos comandos (no Windows, o do Git para Windows, nunca o do WSL) e o aviso do que falta instalar (`linux.rs`, `windows.rs`) |
| `src/contas.rs` | Contas extras (`CLAUDE_CONFIG_DIR`), login e o uso e o plano que o Claude Code guarda em `.claude.json` |
| `src/codex.rs` | Uso do plano do Codex, lido das sessões em `~/.codex/sessions` |
| `src/git.rs` | Cópia isolada (a do chat de projeto, refeita no mesmo caminho, juntada sem ser apagada e em dia com a origem), diff, fotos/volta, o commit de cada vez na pasta própria, junção (uma por vez por repositório, e a cortada conferida ao abrir), o ponto (checkpoint) antes de um ajudante nascer ou ser juntado, religar as cópias de uma origem que mudou de lugar, verificação, limpeza das cópias |
| `src/motor/conexoes.rs` | As conexões no motor: o login do `gh`, a chave do Linear (lida sem destravar o chaveiro), o teste do isolamento, o repositório de cada pasta e os métodos de mesclar, com cache |
| `src/motor/issues.rs` | As issues no motor: listas e detalhes em cache, o chat de uma issue (Iniciar agente, Só a cópia, em lote), anexar e ligar, os efeitos no Linear e no GitHub, e quem roda isolado (`roda_isolado`, as pastas que o git do Vitral trata como isoladas) |
| `src/motor/prs.rs` | A entrega por PR (seção 6 da especificação): publicar, a API da aba PR, CI e revisão no chat, mesclar e o depois; as regras puras em `prs/regras.rs`, o que bloqueia (pelo `Executor`) em `prs/trabalho.rs` e o laço do vigia em `prs/laco.rs` |
| `src/motor/guarda.rs` | A guarda das ações com efeito fora do Vitral (executar, cartão Confirmar, recusar ou devolver o que o turno já fez) e as ferramentas `abrir_pr`, `ver_pr` e `mesclar_pr` |
| `src/github.rs`, `src/linear.rs` | O `gh` como subprocesso (sem token no Vitral) e a GraphQL do Linear pelo `curl` (a chave só no stdin), com as amostras reais nos testes |
| `src/issues.rs`, `src/vigia.rs`, `src/remoto.rs` | Os tipos comuns das issues e as funções puras (ramo da tarefa, título e corpo do PR, confiança no autor), os intervalos do vigia e a execução remota (erros, recuo, segredos tirados do texto) |
| `src/segredos.rs` | A chave do Linear no chaveiro do sistema (Secret Service), nunca em arquivo |
| `src/isolamento.rs` | Os chats isolados: o `bwrap` (com o `.git` do repositório só leitura lá dentro, `travas_do_git`), o ambiente sem o barramento nem as credenciais e o escopo do Landlock que corta os sockets abstratos |
| `src/ui/painel_pr.rs`, `src/ui/abas_direita.rs`, `src/ui/popover_issues.rs`, `src/ui/conexoes.rs` | A aba PR (e as funções puras dela: as ações, o que falta para mesclar, Esperando você), as abas da direita, a lista de issues e o diálogo do Linear |
| `src/prompts.rs` | Instruções do Mestre, do chat de projeto, do ajudante e o pedido do Mestre a um chat |
| `src/modelo.rs` | Os tipos do que vai para o disco (projetos, chats, tarefas, cartões, ocorrências e a configuração global, com os nomes antigos como alias) e o `FORMATO_DADOS` |
| `src/atualizacao/` | Versão embutida e os comandos sem janela (`--versao [--json]`, `--instalar`), o estado da atualização para a interface (`mod.rs`), a thread e o ciclo (`procura.rs`), o manifesto (`manifesto.rs`), a assinatura (`assinatura.rs`), o download (`rede.rs`), o pacote e a conferência do binário novo (`pacote.rs`), a instalação por versão e o menu no Linux (`linux.rs`), a instalação por versão, o `versoes.txt` e o lançador no Windows (`windows.rs`), o `atualizacoes.json` (`config.rs`) e a saúde da versão nova com a volta para a anterior (`saude.rs`); as fixtures ficam em `teste/` (`teste/ponta/` é o release falso dos testes de ponta a ponta, assinado com outra chave de teste) |
| `src/tema.rs` | Tokens da linguagem visual (cores do vidro e das partes pretas, medidas em rem com `r()` e `tam()`, tipografia, sombras e gradientes), tema do GPUI Kit, cores de sintaxe e ícones extras do Lucide |
| `src/ui/` | Trilho (com o Mestre, os projetos que rolam e se reordenam, Avulsos e o +), lista, painéis (`conversa.rs` é a casca: aba, cabeçalho, menu do painel e popover de uso; `mensagens.rs` o que vai dentro, e `mensagens/historico.rs` o histórico das conversas trazidas do Claude Code, lido em segundo plano, com Carregar mais; `paineis.rs` os painéis lado a lado e a soltura), caixa de mensagem, a direita, revisão, gavetas e diálogos; `lateral.rs` (a lista), `visao_geral.rs` (o topo da lista em cada vista e os Recentes), `composer.rs` (a caixa de mensagem e o menu de `#` e `@` do Mestre) e `composer_seletores.rs` (os chips, menus e popovers dela), `menus.rs` (os menus do projeto, da cópia e da conversa, iguais em todo lugar), `equipe.rs` (a direita: Este chat, os ajudantes e a conversa antiga), `distribuido.rs` (a direita do Mestre), `cartoes.rs` (os cartões do Mestre: divisão, sugestão de projeto, memória; as linhas do que ele espera de você, o Resumo do Mestre e o Enquanto você esteve fora), `projeto.rs` (Novo e Editar projeto), `config_projeto.rs` (a configuração de uma pasta, no Avançado de Editar projeto), `painel_arquivo.rs` (o arquivo aberto à direita), `revisao.rs` (o Revisar: diff, comentários, pedir ajuste e juntar), `caixa_entrada.rs` (o popover do sino e o Ctrl+G), `abertura.rs` (o que fazer com a pasta e o `--nova` de uma abertura, ao abrir e pelo socket), `recuperacao.rs` (a tela de quando os dados abrem só para leitura, com Começar do zero e Tentar de novo), `uso_plano.rs` (anéis e painel de uso, também no modo recolhido), `icone_projeto.rs` (ícone e cor dos projetos), `tirar_projeto.rs` (arquivar e apagar projetos: a pergunta, os avisos e o que a janela esquece), `atualizacao.rs` (a atualização: avisos, Novidades, o grupo das Configurações e os textos de cada estado), `fechar.rs` (a pergunta antes de fechar ou reiniciar), `busca_conversa.rs` (o Ctrl+F dentro da conversa: a barra, o índice das ocorrências e o realce), `fila.rs` (a lista das mensagens que esperam o turno, com o Entregar agora), `rascunhos.rs` (o que você escreveu e não enviou, guardado por conversa no `interface.json`), `atalhos.rs` (a tabela única das teclas, que vira os `KeyBinding` e o diálogo "Atalhos do teclado" do `?`), `zoom.rs`, `movimento.rs` (a única porta de animação: durações, curvas, teto de 60 quadros por segundo, entradas e transições) |
| `src/ui/componentes/` | Os componentes da linguagem visual (chip, botões, segmentado, popover, menus, alertas e avisos, cartões de lista, opções, lista de escolha, selos, anéis, abas, diálogos, caixa de código), o estado único (sinal de cada chat e projeto, estado visual do pedido de cada chat) e a vitrine (`--vitrine`) |
| `pacote/` | `manifesto.py` (o `vitral-versao.json` do release), as chaves públicas (`vitral.pub` e `vitral-reserva.pub`), o `linux/instalar.sh` e o `windows/vitral.iss` (o instalador do Windows, Inno Setup 6) |
| `lancador/` | O lançador do Windows (`vitral-lancador`, só a std, o `Vitral.exe` da raiz da instalação): `escolha.rs` lê o `versoes.txt` e escolhe a `app-<versão>`; o `build.rs` põe o ícone, a versão e o manifesto. Fora do Windows o `main` só avisa e sai com 2 |
| `ferramentas/amostra-prs/` | A amostra de PRs da bancada (`AMOSTRA=prs`): o `preparar.py` grava um projeto em modo PR com um PR em cada estado e as issues, e o `gh` responde o que o GitHub diria, coerente com o estado, sem rede e sem escrever |
| `ferramentas/instalar-dev.sh` | Só desenvolvimento: compila e instala no layout do app como `<próxima>-dev.<commit>` |
| `.github/` | `workflows/ci.yml`, `workflows/release.yml`, `workflows/windows-prints.yml` (à mão: prints num Windows de verdade, pelo `ferramentas/prints-windows.ps1`; o instalador de uma rodada do release, pelo `ferramentas/instalador-windows.ps1`; e o atualizador, da versão publicada a três do ramo num release falso, pelo `ferramentas/atualizador-windows.ps1`) e `dependabot.yml` |
| `build.rs` | No Windows, o ícone e a versão no `vitral.exe` (o manifesto vem do GPUI); fora dele, nada |
| `tests/versao.rs` | `--versao` e `--versao --json` sem tela (o binário de verdade, sem DISPLAY nem WAYLAND_DISPLAY) |
| `tests/hook.rs` | `--hook-verificacao`: código 2 e o fim da saída no stderr quando a verificação falha, 0 na segunda parada seguida |
| `tests/instalar.rs` | `--instalar` com o binário de verdade num HOME temporário em `target/tmp`: o layout, o link, o `.desktop`, o `~/.local/bin` e reinstalar sem duplicar |
| `README.md`, `docs/imagens/` | A porta de entrada do GitHub (o que é, instalar, conferir o download, limites) e os prints dela, tirados na bancada com `RAIZ_SINTETICA=1`/`AMOSTRA=prs` e `CLAUDE_FALSO=1`, recortados e com paleta de 256 cores |
| `assets/` | Logo completo e pequeno, PNGs de 48 a 256 px, `vitral.ico` (16 a 256 px, para o Windows), logos do Claude e do Codex (e os discos coloridos em `marcas/`), o escudo cheio do modo Manual, os ícones de projeto (`icones/`, Lucide, ISC) e as fontes embutidas (`fontes/`: Inter 4.1 e JetBrains Mono 2.304, só os pesos usados, OFL 1.1 em `fontes/OFL.txt`, que vai no pacote como `OFL-fontes.txt`) |

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
dá um quadro com a conta da escala anterior. A busca da lista é o `componentes::campo_de_busca`
sem o atalho (a lupa a 8 da borda dele, que na peça de 8 cai na guia de 16), com a dica por cima
(`campo_da_lista`). Os ajudantes vão no nível 1 do `CartaoDeLista`: o `.sinal()` e, sem sinal,
um ponto de 8 na cor do estado como símbolo, com a palavra do estado em `.dica_do_ponto()`; o traço
da árvore conta com os 2 em cima de todo cartão da lista (`esp::E2`) e para a 2 do aro de rodando.
A barra de rolagem fina é `componentes::barra_de_rolagem` (sem fade: o `SCROLLBAR_*` do vendor
fica em zero e o teste `barra_de_rolagem_sem_fade`, em `tema.rs`, confere); o `BotaoCopiar` dá ao
Copiar do kit 24 crescendo com o zoom; `icone_projeto::desenho` mede em rem; o campo de texto do kit
tem a margem do lado em rem (o `composer::recuo_do_texto`, onde o texto da caixa começa e por onde a
faixa do turno se alinha, conta com isso); e os avisos têm a largura (`tema::LARGURA_AVISO`)
multiplicada pelo zoom em `zoom.rs`.

Espaços só na escala 2, 4, 6, 8, 12, 16, 24 e 32 (`tema::esp`, com os nomes por contexto: `PECA`,
`CAIXA`, `FLUTUANTE`, `GUIA_LISTA`, `GUIA_EQUIPE`, `NO_TURNO`, `ENTRE_TURNOS`, `ANTES_DE_SECAO`);
quem tem contorno de 1 px usa `dentro_da_borda(12.)` para o conteúdo ficar a 12 da borda de fora. Os
atalhos do Tailwind (`gap_2()`, `p_3()`) medem em rem e, com 1 rem = 15 px, saem fora da grade (7,5
e 11,25). O teste `espacos_na_escala` barra os dois casos em todo o `src/ui`, sem exceção, e o
`movimento_com_teto` barra animação feita fora de `src/ui/movimento.rs`.

Raios só três e a pílula (`tema::raio`, em Pixels fixos): `PECA` 20 nas peças (trilho, lista,
conversa, direita, arquivo aberto, gavetas, popovers, menus, diálogos, avisos, a caixa de mensagem e
a tela cheia); `CARTAO` 14 no que vai dentro delas (cartões da direita e da conversa, balão,
cartão de lista de duas linhas, blocos de código e tabelas do chat, diff do Revisar, anotação,
grupos de opções, alertas, a caixa na Estreita e a dica); `CONTROLE` 10 em campos, linhas (de uma
linha na lista, item de menu, árvore, log e listas do Revisar), teclas, blocos dentro de cartão
(código, verificação, ressalva) e miniaturas; e `PILULA` (o `rounded_full()`, ou o `raio::PILULA`
onde a API só aceita Pixels) em botões, chips, abas, segmentado, selos, contadores, pontos, anéis e
barras. Dentro de quem tem padding p, o raio é max(raio de fora − p, `CONTROLE`), para os cantos
ficarem concêntricos: o lugar de 40 do trilho tem 14 (20 − 6) e o quadrado de 32 dentro dele, 10
(14 − 4); os menus têm margem 8 para os itens de 10 caberem no menu de 20; o popover do modelo,
6 mais os 4 da lista. Onde a conta não fecha, vale o token a até 2 do exato (concêntrico a olho)
ou o canto de dentro mais redondo (a caixa de mensagem fica com 20 dentro da conversa); para não
achatar mais que isso, aumenta o padding. Camadas por dentro (o reflexo, o
brilho da caixa, a faixa do Mestre, o cabeçalho tingido dos cartões) levam o raio da peça menos
a borda. O quadrado do projeto é um ícone: `raio::do_icone(lado)`, o 32 com o `CONTROLE` e os
outros lados em escala. O tema do kit acompanha (`radius` 10, `radius.lg` 20) e o teste
`raios_na_escala` barra em todo o `src/ui` raio solto (`rounded(px(..))`, `rounded(r(..))`, número),
atalho do Tailwind (`rounded_md()`) e `ButtonRounded`, sem exceção.

Superfícies: o fundo da janela e o `Cores::vidro` são o chumbo das frestas (#060810d4);
`vidro_leitura` (#151b2cbf, trilho e lista) e `cartao` (#141a2ae6) são o azul das peças; `dialogo` e
`gaveta`, o azul opaco do que cobre a conversa sem ser um flutuante; `flutuante` (#0c0e13, igual ao
`popover.background` do tema, que o kit usa nos menus, popovers, dicas e avisos) é o quase preto do
que flutua; `preto` (#08090c) é o de dentro (código, saída de comando, diff e os discos do trilho);
e `caixa_preta` é o fim do gradiente da caixa. O `recuo` escuro some no quase preto: dentro de um
popover vale o `recuo_no_flutuante` (no segmentado, `.no_flutuante(true)`). Os testes de `tema.rs`
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
alvo. `tema::aplicar` chama `movimento::iniciar`, que lê o `enable-animations` do GNOME uma vez e a
escolha das Configurações (`"animacoes": "desligadas"` no `interface.json`, gravado pelo
`zoom::gravar_na_interface` sem perder o zoom) e liga o `set_reduce_motion`;
`movimento::escolher_animacoes` troca na hora e, voltando a Seguir o GNOME, devolve o controle ao
kit, que acompanha o sistema. O
contador do `VITRAL_QUADROS` fica em `src/ui/mod.rs` (`contar_quadro`, chamado no `render` da
Janela e da vitrine). Na conversa, `movimento::com_estado` deixa quem só tem o `&Context` da Janela
usar o estado guardado (o halo da caixa e do painel, as entradas), e `movimento::entrada` faz o fade
com deslize de algo que acabou de aparecer: `nova` só no primeiro quadro, então um remonte (outro
painel abriu, a lista recolheu) não repete nada nem redesenha. Os blocos usam `Painel.base_animacao`
(o tamanho da conversa quando ela abriu no painel) e `Painel.vistos` (`entrada_do_bloco`, com
teste). O pulso do enviar sobe com `Painel.envios`, em `paineis::enviar_do_painel` (o Enter e o
clique). Na direita, o `Revelar` abre e fecha a altura em 180 ms pelo `movimento::valor`, medindo a
altura do conteúdo no próprio desenho (`on_prepaint`); não usa o `MotionReveal` do kit, que faz o
layout do conteúdo à parte e, sem a altura guardada, desenha um quadro vazio na montagem. O
`Realce` é um `movimento::disparo` com a chave do estado da pílula (palavra e cor). O pulso do sino
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
`salvar_config` serializam na hora e entregam os bytes à fila do gravador; as rotinas só gravam a
configuração quando alguma rodou. `usa_git` responde "sem git" até a consulta em segundo plano
(`consultar_git`, no fim do render) voltar, num mapa à parte (`git_consultas`). A notificação é
`Motor::notificar`: `notify-send -a Vitral -i vitral -A default=Abrir -p`, numa thread que guarda
o id por conversa para o `--replace-id` seguinte; o clique manda `Sinal::Abrir`, e o observer da
Janela seleciona a conversa e chama `activate_window`.

Projetos, arquivar e apagar: os projetos são `Estado.projetos` (no `estado.json`, um ponto de
gravação só), com `id` opaco (o nome da subpasta antiga, num migrado; `novo_id()` num novo), e a
interface só fala com eles pela API do motor: `projetos_visiveis()` (sem os arquivados, na ordem do
trilho) e `projetos_arquivados()`, `projeto(id)`, `nome_do_projeto(id)` ("Avulsos" para `""`),
`projeto_por_nome(texto) -> Resolucao` (id, nome exato ou chave; o `#` da frente sai), `pastas_do`,
`pasta_principal`, `projeto_da_pasta`, `criar_projeto(NovoProjeto)` e `mudar_projeto(id,
MudancaProjeto)` (um patch; as pastas novas entram canônicas), `arquivar_projeto`,
`parar_e_arquivar` (com os pedidos do Mestre cancelados), `reordenar_projeto`, `localizar_pasta`
(em segundo plano, com o `git worktree repair`), `pode_mover`/`mover_chat` (o chat, os descendentes
e os pedidos deles; o `cwd` não muda) e `novo_chat`. Um chat conta como arquivado pela
`Motor::arquivada` (`conversa_arquivada`: ele mesmo ou o projeto), usada no sino, no Ctrl+G, nos
sinais, na lista e no `arquivadas_que_casam`; o `c.arquivado` cru fica para o menu do chat e os
ajudantes. `ocupacao_do_projeto` diz o que roda (chats: turno, vaga, nova tentativa; pedidos:
conferência, junção, preparação ou retomada na fila). `apagar_projeto` recusa com algo rodando,
manda a pasta própria para a Lixeira só quando o projeto não tem pasta ligada
(`pasta_propria_para_lixeira`: direto em `dados/projetos`, sem ponto na frente; com teste), roda o
`gio trash` (`sistema::para_a_lixeira`, que confere que a pasta saiu) e, só depois, as cópias para a
Lixeira, o `git worktree prune` e os pontos de volta apagados em cada origem, e o `tirar_do_estado`
(puro, com teste: chats do projeto, ajudantes, pedidos, filas, rotinas e as partes dos cartões); o
`esquecer` limpa os mapas do motor e nega pedidos de permissão. O gravador apaga arquivos na vez
deles (`Gravador::apagar`, que tira da fila uma gravação pendente do mesmo arquivo): as mensagens
dos chats e as notas saem depois do estado novo.

Dados e normalização: `Dados` (em `dados.rs`) tem o gravador: estado e configuração com
`.anterior` por link duro (o link vai num `.anterior.tmp` e entra por rename, então nunca há um
momento sem `.anterior`), os chats sem. Uma gravação que falha fica em `falhas` até o mesmo arquivo
gravar; `Motor::gravar_sujos` (a cada 5 s) liga a faixa de falha, põe as falhas de novo na fila e,
quando volta, entrega as filas que esperaram. `carregar` lê cada `chats/<id>.json` item a item: o que
não converte vira `Item::Desconhecido` e é regravado como veio; `Chat`, `Tarefa`, `Projeto`,
`Cartao` e `Estado` têm `resto` (flatten), e uma `versao` maior que a suportada abre só para
leitura. `salvar_config` relê o `config.json` como `Value`, tira as chaves legadas
(`modelo_coordenador` vira `modelo_mestre`, `modelo_trabalhador` vira `modelo_chats`; com as duas,
vale a nova) e mantém as desconhecidas. A `normalizar(estado, dados, consultas, agora)` roda no
`Motor::new`, antes de tudo, com o trait `Consultas` (o `NoDisco` de verdade, um falso nos testes):
garante o Mestre (id `"mestre"`, cwd `dados/mestre`), recria como provisório o projeto que só é
citado, acha a origem dos pedidos antigos, converte o coordenador vazio em chat de projeto e os
trabalhadores do Vitral antigo em chats sem dono (a conversa com histórico fica só para leitura),
dá uma faixa de portas a cada pedido aberto, põe na conversa do Mestre o cartão aberto que não
estava nela, poda os cartões e as ocorrências velhas, recolhe os chats vazios com mais de 24 h e
acerta a `proxima_tarefa` acima de todo `t<N>` de `dados/copias`; o `Motor::new` grava tudo antes de
qualquer turno. O prompt de sistema vai por arquivo (`~/.cache/vitral/prompts/<chat>.md`, regravado
a cada turno) com `--append-system-prompt-file` e `--system-prompt-snapshot off` a partir do Claude
Code 2.1.278 (`claude::VERSAO_PROMPT_POR_ARQUIVO`).

Migração: `migracao::migrar(dados, raizes, proc, pid, na_bancada, quando)` devolve `NaoPrecisa`,
`SemRaizes`, `Migrou(Relatorio)`, `Adiada { raizes, pids }` ou `Falhou { raiz, erro }` (as duas
últimas viram a tela de recuperação). Ela trabalha sobre `serde_json::Value` (o que esta versão não
conhece passa como veio): `ler_raiz` lê sem escrever (os bytes dos chats, o texto das notas, o mtime
e o tamanho do `estado.json`), `planejar` é pura e trata as raízes juntas (ids das tarefas
renumerados em duas passadas, a raiz mais recente primeiro), e `aplicar` grava direto na pasta
final, com o `estado.json` por último e o `formato` logo depois dele. `vitral_antigo_vivo` varre
`/proc/*/comm == "vitral"`, menos o próprio processo, os filhos dele e, conforme o lado, os da
bancada (`DENTRO_DA_BANCADA=1`; dentro dela, só o da mesma bancada, pelo `XDG_DATA_HOME`). O
`migracao.json` guarda, por raiz, o mtime e o tamanho do `estado.json` antigo (o aviso de que o
Vitral antigo mexeu depois), os ids dos projetos e as tarefas renumeradas. A ordem do `main`:
`--versao`/`--instalar`, dados, trava (ou a entrega ao Vitral aberto), saúde da versão, socket,
migração, carregar, `processos::limpar_sobras`, janela.

Textos e pendências: `textos::duracao_curta` é o formato único do tempo ("19 s", "2 min 13 s",
"1 h 05 min", com dois dígitos depois do primeiro para o tempo que anda não mudar de largura), usado
pela pílula, pela lista, pela verificação e pela preparação. `lateral::Causa` (Permissao, Plano,
Parada, Erro; `da_pendencia` para o sino, `da_espera` para a lista) e `frase_da_pendencia` dão a
mesma frase e o mesmo sinal na lista e no sino, desenhados pelo mesmo `previa_elemento` (teste
`a_mesma_pendencia_diz_o_mesmo_na_lista_e_no_sino`). No Revisar, `pendencias_de` decide o botão
azul, e a ação `AcaoPrincipal` (Ctrl+Enter no contexto "Revisao") deixa o evento seguir quando há um
campo focado, um rascunho ou um popover aberto. Na conversa, o painel que está no fim pede
`scroll_to_bottom()` a cada desenho; `acompanhar_rolagem` solta o fim em qualquer subida (1 px) e
prende de novo nos últimos 48 descendo, e `Painel.manter_lugar`, ligado por `alternar()`, segura o
lugar quando um bloco no fim abre. O `coordenador_da_equipe` e o `pedidos_da_equipe` de `equipe.rs`
dão o selo do botão da direita.

Ritmo da direita (`equipe.rs`): o painel é uma pilha de `Bloco` (Caixa, Resumo, Secao, Linha), sem `gap`; o
`respiro(antes, agora)` de `equipe.rs` dá a margem de cima de cada um, medida na tinta (16 do texto
de uma seção até a borda do cartão, o texto da seção de cima ou a última linha). Os cabeçalhos de
seção usam `.respiro(4.)` para ficarem com 4 em cima e 4 embaixo (o texto no meio de uma faixa de
32); o resto vem do `respiro` do painel, e o teste `secoes_ficam_a_16_na_tinta` faz a conta. Dentro
do cartão, o que não tem fundo sangra para o vão: a linha do motivo tem 20 com `my(-4)`, a fileira
de botões sem o principal `my(-4)`, o andamento `my(-2)`. O título tem entrelinha de 18 e o cartão
`pt` 8 e `pb` 12 (com a borda), para a tinta do título ficar a 12 da borda como a base do botão.
`primaria()` escolhe o cartão do botão sólido; os outros principais usam `variante_tinta`.

Ultracode e agentes em segundo plano: em `src/claude.rs`, os argumentos e o ambiente do turno são
funções puras (`argumentos`, `ambiente`, `configuracoes_do_turno`), com teste. O Ultracode vai como
`--effort ultracode` (o jeito que liga antes e depois da 2.1.284; antes dela o Ultracode só vale em
Extra) e, no mesmo `--settings`, `"enableWorkflows": true` (no plano Pro os fluxos começam
desligados) e `"ultracode": true`. Fora do Ultracode, `Workflow` vai em `--disallowedTools`; o
Mestre roda só com `--tools Read,Grep,Glob` e o MCP do Vitral (com `--disallowedTools` de segunda
camada), em `bypassPermissions`. O limite de agentes vai em todo turno. O
`Saiu` traz o código de saída (lido sem colher o processo, em `processos::codigo_de_saida`). A
versão do Claude Code vem do link `versions/<versão>` do instalador ou de `claude --version` e, a
cada init, de `claude_code_version`, junto com a ferramenta `Workflow` na lista (ou não). O
`background_tasks_changed` conta os fluxos e subagentes vivos (os shells em segundo plano o próprio
Claude Code encerra). Em `src/motor.rs`: `Entrada.ultracode` e `Item::Usuario.ultracode` (gravados
só quando ligados; a fila guarda e sobrevive a um reinício), `Reforco { ultrathink, ultracode }` no
envio, `motivo_sem_ultracode`, `aceita_ultracode`, um Ultracode por vez (`ultracode_rodando`), o
fim do turno adiado enquanto há agentes em segundo plano (`fim_adiado`; `agentes_cortados` quando o
processo sai antes deles) e `entrada_para_repetir` nas novas tentativas automáticas.

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
só então o pedido vai para a verificação; e, no modo Manual (o Vitral pede permissão para o
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
os menus têm margem 8 e itens de raio 10, medidas em rem (crescem com o zoom), item perigoso, item externo, linha fixa de cabeçalho e rodapé fora do teclado, check à
direita, separador de borda a borda, desabilitado a 0,45, o atalho também no item de elemento e
o ↓ sem nada realçado indo ao primeiro item navegável, não à linha fixa
(`src/menu/popup_menu.rs`, `src/menu/menu_item.rs`); menus, popovers e avisos têm o anel a 12% e a
sombra flutuante do Vitral, e os diálogos a sombra própria (`src/styled.rs`,
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
(`src/command`, `label_prefix`); e a margem do lado do campo de texto cresce com o zoom (`com_zoom`
em `src/sizing.rs`, usada pelo `Input` e pelo grupo de campo em `src/input`). Ao atualizar o
gpui-kit, refazer essas mudanças na versão nova.
