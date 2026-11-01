# Vitral

Ambiente para trabalhar com vários chats do Claude Code ao mesmo tempo, com o visual de vidro
azul-marinho da Stained Glass UI e a organização do Ratoeira (trilho de ícones, seções
"Esperando você" e "Rodando agora", abas em pílula). Feito em Rust com GPUI (o motor de
interface do Zed) e os componentes do GPUI Kit.

## Como funciona

- **Pasta de trabalho.** No começo você escolhe uma pasta (por exemplo `~/Projetos`). Cada
  subpasta é um projeto. As conversas ficam em `.vitral/` dentro da pasta escolhida. Dá para
  criar um projeto pelo app: **Novo projeto…** no menu do logo, no fim de Nova conversa ou no
  botão da tela vazia (com 0 projetos, Ctrl+N abre esse diálogo). O diálogo mostra a prévia do
  quadrado do projeto enquanto você escreve, diz em que pasta ele vai ficar, avisa na hora se o
  nome não serve (ou se a pasta já existe) e tem a chave **Usar git**, ligada por padrão.
- **Arquivar e apagar projetos.** No fim do menu do projeto, separados, ficam **Arquivar projeto**
  e **Mover para a lixeira…** (em vermelho). Arquivar age na hora, com **Desfazer** no aviso
  ("Projeto site arquivado · 3 conversas em Arquivadas"): o projeto sai do trilho, do filtro, do
  Ctrl+1…9, do Nova conversa e das rotinas (que não rodam enquanto ele está arquivado; o dia passa
  marcado, então desarquivar à tarde não dispara a das 8h), e as conversas dele vão para
  **Arquivadas** sem perder nada. A marca fica na configuração da pasta. Ele volta pelo menu do
  Vitral (seção **Projetos arquivados**, com **Desarquivar**), pelo grupo **Arquivados** de
  **Todos os projetos** (escolher desarquiva e filtra), por **Desarquivar o projeto loja** no menu
  de uma conversa dele em Arquivadas ou por uma conversa nova com `--nova`. Com conversa
  trabalhando, pergunta antes ("Parar e arquivar loja?", o Enter fica com **Continuar
  trabalhando**); com uma tarefa sendo conferida, juntada ou preparada, não arquiva e diz qual
  esperar. **Mover para a lixeira…** mede a pasta (até 100 mil arquivos) e pergunta, com o caminho,
  quantos arquivos e quanto ocupa, quantas conversas e trabalhadores saem do Vitral e quantas rotinas
  são apagadas (em vermelho: isso sai de vez), e quantas cópias de tarefa vão junto para a Lixeira (os
  branches ficam no repositório). O botão é vermelho e o Enter só fecha. A pasta vai para a Lixeira
  do GNOME com `gio trash` (nunca `rm`; volta pelo Arquivos, em Lixeira › Restaurar) e, só se ela
  foi, as cópias vão atrás e saem do Vitral as conversas (com as mensagens), as tarefas, as filas,
  as rotinas, as notas e a configuração do projeto; os painéis com conversas dele fecham. Se a
  lixeira recusa, nada sai ("Nada foi apagado: …"). Com uma conversa ou tarefa do projeto
  trabalhando, não deixa ("loja tem 2 conversas trabalhando; pare-as antes de mover para a
  lixeira."). O aviso do fim diz o que saiu ("loja na Lixeira · 5 conversas apagadas") e tem
  **Abrir a Lixeira**.
- **Nova conversa** (Ctrl+N). Com um projeto só, ou com um projeto escolhido no trilho, a
  conversa nasce direto nele. Nos outros casos abre uma paleta com busca: cada projeto com o
  quadrado dele, quantas conversas tem e quando foi a última ("atual" no da conversa aberta, "sem
  git" quando se sabe); ↑/↓ e Enter escolhem, o primeiro Esc limpa a busca e o segundo fecha. No
  fim fica **Novo projeto…** (ou, quando a busca não acha nada, **Criar o projeto “ops”**, que
  abre o diálogo com o nome já escrito).
- **Coordenador.** Cada conversa que você abre é um coordenador. Ele não edita código:
  planeja, divide o pedido em tarefas com critérios de aceitação, abre uma conversa de
  trabalhador para cada uma, revisa as entregas e pede ajustes até ficar bom. No fim manda um
  resumo, que aparece destacado.
- **Trabalhadores.** Cada tarefa roda num chat próprio, numa cópia isolada do projeto
  (`git worktree`, branch `vitral/t<N>-...`). A pasta original fica bloqueada para escrita
  deles. A cópia recebe os arquivos locais fora do git (`.env*` por padrão), roda o script de
  preparação do projeto e ganha dez portas próprias (`$VITRAL_PORT`). Num projeto git o
  trabalhador nunca roda na pasta original: enquanto a cópia é preparada, as mensagens para ele
  esperam e entram junto com o pedido; se a cópia não puder ser criada, a tarefa fica como falhou
  e o coordenador recebe o erro. O pedido da tarefa aparece no chat do trabalhador como mensagem
  do coordenador.
- **Projeto sem git.** Os trabalhadores mexem direto na pasta, e a Equipe avisa. **Ativar git**
  cria um `.gitignore` quando não há (`.env`, lixo de testes, `target`, `dist`/`build`, `.venv` e
  outros ambientes virtuais, conforme a pilha) e diz quantos arquivos e MB entraram no commit
  inicial.
- **Verificação em três camadas.** Se a tarefa tem comando de verificação, um hook roda os
  testes quando o trabalhador para e, se falharem, ele continua sozinho. Depois o Vitral roda a
  verificação de novo e um **revisor independente** (sessão nova, só de leitura, pode ser outro
  modelo) confere os critérios e aponta problemas numerados, com arquivo e linha. Tudo vai para o
  coordenador, que aprova (o Vitral faz o commit e junta) ou pede ajuste. A saída da verificação
  e da preparação é lida enquanto o comando roda (fica só o fim), então um `cargo test` ou
  `npm install` barulhento não trava; as duas mostram quanto tempo levaram ("PASSOU em 12 s"). Num
  turno com Ultracode o hook não vai: ele rodaria já na primeira resposta, com os agentes do fluxo
  ainda mexendo nos arquivos. A verificação do Vitral e o revisor continuam, quando o turno termina
  de verdade.
- **Portão de aprovação.** O coordenador só aprova uma tarefa em revisão cujo relatório desta
  rodada já saiu. Com a verificação falhando ou um problema de gravidade alta do revisor, o
  Vitral recusa e lista o que está vermelho; aprovar mesmo assim exige `forcar` com um motivo, que
  vai para o commit ("Aprovada apesar de: ..."), aparece em vermelho na conversa e numa linha
  âmbar no cartão da tarefa. Ninguém aprova uma tarefa já aprovada, cancelada, em verificação,
  sendo juntada ou com o trabalhador ainda rodando. Você, pelo Revisar, aprova sem o portão; com a
  verificação vermelha, um apontamento de gravidade alta do revisor ou comentários seus ainda não
  enviados, ele pergunta antes ("Aprovar com pendências?"), e o que estava vermelho vai como
  ressalva no commit.
- **Ajustes.** `pedir_ajuste` anexa sozinho os problemas do revisor e as últimas 40 linhas da
  verificação quando eles apontaram algo; o coordenador escolhe o que vai com `incluir` e tira
  falsos positivos pelo número com `ignorar_problemas`. `ver_diff` deixa os gerados (lockfiles,
  `.min.js`, `.map`, `.snap`) numa linha à parte e resume um diff grande (o `--stat`, os arquivos
  que cabem inteiros e o que falta); `arquivos: [...]` traz o diff completo só desses.
- **Junção e conflitos.** Antes de juntar, o Vitral traz o que já entrou no projeto para a cópia;
  se der conflito, ele fica na cópia. Depois disso, diff, Revisar, Descartar e revisor comparam
  com o que veio do projeto, então o trabalho de outras tarefas não aparece como desta. As
  junções e criações de cópia de um mesmo projeto entram uma de cada vez. Outro git segurando a
  pasta (um `index.lock`) não vira conflito: a resposta diz que dá para aprovar de novo. Arquivos
  soltos fora do git no projeto (`__pycache__`, `.coverage`) não travam a junção; se a tarefa cria
  um arquivo que já existe solto ali, o coordenador recebe um aviso próprio para falar com você.
  Tarefas em andamento que mexem nos mesmos arquivos são avisadas.
- **Chats conversam entre si** (`mensagem_para_chat`, `perguntar_coordenador`); mensagens que
  chegam com o chat ocupado esperam na fila. Uma mensagem sua direto a um trabalhador em revisão
  abre uma nova rodada (verificação, revisor e relatório) e avisa o coordenador.

## Quando algo para no meio

- **Tarefa parada.** Se o Vitral fecha, o PC cai ou você aperta Parar, a tarefa aparece como
  **parada** em âmbar, com ícone de pausa, sobe para "Esperando você" (menos quando foi você que
  parou) e ganha **Retomar** no cartão da lista, na Equipe e no fim da conversa. Um coordenador
  cortado no meio do turno também. O aviso ao reabrir sai uma vez só.
- **Verificação, revisor ou preparação cortados** rodam de novo sozinhos ao reabrir, um de cada
  vez. Se cair de novo nessa mesma refeita, a tarefa fica parada com Retomar, para não entrar num
  ciclo de quedas.
- **Trabalhador que cai** (processo que morre ou não abre) ganha uma nova tentativa depois de
  10 s; um erro passageiro da API (sobrecarga, 5xx, tempo esgotado, conexão), depois de 60 s. Se
  falhar de novo, a tarefa volta ao coordenador com ERRO e o motivo, com notificação se a janela
  estiver em segundo plano. Mandar uma mensagem ou Parar cancela a nova tentativa.
- **Agentes em segundo plano.** Quando o Claude responde com um fluxo de agentes (ou um subagente)
  ainda rodando, o turno não acaba ali: o chat continua rodando, a conversa ganha o aviso "Os
  agentes seguem trabalhando em segundo plano; o turno termina quando eles acabarem." e a tarefa só
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
  trabalhando"; "Fechar mesmo assim" / "Continuar aberto"); trocar a pasta de trabalho também. O
  Enter fica com **Continuar aberto**: fechar pede o clique.
- **Processos.** O claude, a verificação e o revisor rodam cada um num escopo do systemd do
  usuário (`vitral-<pid>-...scope`, em `vitral.slice`). Parar um deles derruba tudo o que ele
  abriu, até servidores e shells separados com `setsid`; fechar o app manda SIGTERM a tudo sem
  travar a tela; ao abrir, o Vitral para os escopos de uma instância que caiu. Sem systemd, vale
  um grupo de processos.
- **Confirmações.** Toda pergunta antes de agir tem o mesmo desenho: o ícone num círculo na cor
  do tom, o título, o que acontece e, numa caixa embaixo, o que some (o tamanho, o branch, os
  pontos de volta). Nas que apagam ou desfazem (**Apagar a cópia**, **Voltar os arquivos**,
  **Descartar**, **Desconectar**, **Mover para a lixeira**) e nas que param o que trabalha
  (**Parar e arquivar**), o Enter só fecha, sem agir; em **Ativar git**, o Enter confirma.
  Arquivar (conversa ou projeto) não pergunta: age na hora, e o aviso tem **Desfazer**.
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
contexto e o custo, "Contexto 31% · US$ 1,84", embaixo o modelo, a conta, os tokens e, no
coordenador, o custo com os trabalhadores, e no rodapé **Compactar a conversa**, que prepara o
`/compact`), o clipe de anexar (arquivo ou imagem, arquivo do projeto, conversa anterior e comandos
de barra), o **…** (comandos de barra, compactar, sessão nova e fixar) e o botão redondo de enviar,
apagado com a caixa vazia, que acende quando ela ganha texto e dá um pulso claro a cada envio (o
pulso fica no botão também quando ele já virou o parar). Clicar em qualquer ponto vazio da caixa
(o vão entre o texto e os chips, a margem, o espaço entre os chips) mantém o cursor nela. Com a
conversa rodando, ele vira o parar (o
quadrado); com texto escrito, ficam os dois, e o enviar entra na fila. Vazia, a caixa diz "Peça ao
coordenador…" e, no trabalhador, "Mensagem ao trabalhador…" ("Sem cópia: fale com o coordenador" e
"Cópia removida: fale com o coordenador" quando não dá); numa tarefa em revisão, a dica do enviar
diz que a mensagem abre uma nova rodada de ajuste, com verificação e revisor. **Shift+Tab** troca o
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
desabilitada, com o motivo curto no lugar da descrição, no coordenador ("só nos trabalhadores"; a
explicação, que ele divide o trabalho em tarefas para os trabalhadores, vai na dica da linha), com
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
novo se quiser."; só o **Tentar de novo** o repete. Fora do Ultracode, e sempre no coordenador, a
ferramenta Workflow fica bloqueada: nem o coordenador nem um "Continue de onde parou" relançam um
fluxo sozinhos. O **modelo** abre um seletor com busca: Padrão (o da pasta) e as famílias do Claude,
com a versão quando o modelo em uso é daquela família; o logo do Claude fica só no rótulo do grupo e
o check do escolhido à direita. Seguindo o Padrão, é ele que diz qual está em uso ("Padrão da pasta
· Opus 5.5"); só uma família escolhida na conversa ganha o selo "em uso". Uma conversa antiga que
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
  chamada mostra o resultado (as últimas 40 linhas, com Copiar e Mostrar tudo). O que o coordenador
  faz com as tarefas aparece como evento ("Criou #2 ...", "Pediu ajuste em", "Aprovou e juntou",
  "Distribuiu 3 tarefas"), com a pílula do estado na última menção de cada tarefa (só a palavra, "em
  revisão"; o detalhe e a explicação na dica). Fora da largura larga, o verbo e a frase vão para a
  dica (o ícone já diz o que foi) e o título da tarefa fica com o espaço. O clique mostra o detalhe
  e, no hover, a linha ganha **Abrir a conversa da tarefa** (o balão de conversa; Ctrl+clique abre
  ao lado) e Copiar. Em repouso, a pílula do estado encosta na seta de abrir; no hover, só a linha
  sob o ponteiro abre espaço para as ações. A
  última entrega de uma tarefa em revisão vira um cartão com "#4 título" e a pílula, a rodada e o
  que a verificação e o revisor acharam ("Rodada 1 · verificação falhou · 4 observações do revisor";
  a frase do Vitral, o tempo da verificação e o parecer na dica) e **Revisar** (em tinta, o
  azul-claro dos cartões da Equipe; o azul cheio fica só no primeiro cartão da Equipe que pede
  ação), **Abrir conversa** (o balão; a chave inglesa fica só no título, onde quer dizer
  trabalhador) e o relatório. O cartão tem os cantos dos cartões da Equipe, e o ícone dos botões cai
  na coluna da linha de cima; na largura mínima, os dois botões ficam só com o ícone.
- **Mensagens.** O que você escreve fica num balão neutro à direita (sem avatar, com o reflexo do
  vidro), com as miniaturas das imagens anexadas, o nome e o tamanho dos outros arquivos e os selos
  **Ultrathink** (em arco-íris) e **Ultracode** (em violeta, com o ícone de fluxo) quando a mensagem
  foi com eles; texto com mais de 14 linhas mostra 12 e **Mostrar tudo**. Mensagens que outra
  conversa (ou o Vitral) entregou mostram quem mandou (coordenador, trabalhador, outra conversa ou
  o logo do Vitral) e uma prévia, que termina com reticências antes da coluna da seta, nunca
  cortada no meio da letra; o clique abre o texto inteiro. Os avisos do Vitral são marcos
  centralizados ("Sessão nova", "Turno interrompido"), linhas tênues ("Tentando de novo.") ou,
  quando é erro, um alerta vermelho com Copiar. O resumo final do coordenador fica num cartão com a
  contagem das tarefas ("3 tarefas aprovadas · 1 cancelada"). Conversa nova de coordenador sugere
  dois começos que só preenchem a caixa. O que chega com a conversa aberta no painel (a sua
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

**Voltar no tempo:** antes de cada turno de um trabalhador o Vitral tira uma foto dos arquivos;
passe o mouse na sua mensagem e use ↶, na barra que aparece à esquerda do balão, para voltar os
arquivos àquele ponto (o botão vermelho da confirmação é o que volta). A resposta diz quantos
voltaram e quais não. Com a cópia apagada, o ↶ some.

## Equipe (painel da direita)

- No topo, a aba **Equipe** (na dica, o projeto, de qual coordenador é a equipe e o número do
  painel). Com a lista rolada, uma linha separa o cabeçalho, como na conversa; a lista não rola
  sozinha, e ao trocar para uma conversa sem cartão destacado ela volta ao topo. Logo embaixo, uma
  linha só: **Tarefas (n)**, uma barra fina com a proporção de cada
  estado (cada parte diz na dica quais tarefas são: "1 em revisão: #2", com as mesmas palavras e
  cores da pílula) e os atalhos da equipe, só com o ícone (com o rótulo quando o painel está largo,
  de uns 450 px para cima): com um trabalhador aberto, **Coordenador** abre o coordenador ao lado;
  com tarefas abertas, **Lado a lado** monta o coordenador e as tarefas em andamento, quantas
  couberem na largura.
- Cada cartão tem "#2 título", a pílula do estado ("● em revisão · com o coordenador"; a parada é
  âmbar quando precisa de Retomar e neutra quando foi você que parou) e a trilha de pontos
  (Trabalho, Verificação, Revisor, Coordenador, Juntada; o detalhe de cada um na dica). Embaixo,
  numa linha só, o motivo principal: a preparação ou a verificação que falhou ("Verificação falhou ·
  19 s", com o comando na dica), o revisor que pede ajustes ("Revisor pede ajustes · 2 importantes"),
  os mesmos arquivos de outra tarefa ou o andamento (o resumo do revisor e o andamento terminam
  em reticências quando não cabem). O clique no motivo, ou nos pontos da
  verificação e do revisor, abre a saída do comando (o fim dela, com **Mostrar tudo** e copiar) ou o
  parecer do revisor, com a gravidade em cada problema; cada problema abre o Revisar já na linha.
  Selos pequenos só para o que não virou motivo: ressalva, mesmos arquivos (#1) e "sem cópia".
- Uma ação principal por cartão, e um só botão azul cheio no painel: o do cartão destacado, se ele
  pede ação, ou o do primeiro que pede; nos outros, o mesmo botão fica azul-claro, em tinta (sem
  nenhum, o azul cheio é o **Ativar git** do aviso). As ações: **Ver pedido** (o trabalhador espera
  permissão: abre a conversa dele ao lado com o cursor na caixa), **Retomar** (parada) ou **Revisar**
  (em revisão). Ao lado, **Abrir ao lado** (ou **Ir à conversa**, se já está aberta; **Ver erro** se
  o processo caiu), **Mudanças** e, na tarefa do trabalhador aberto, **Rodar** com o comando de rodar
  do projeto. O **…** tem rodar na cópia (com o `$VITRAL_PORT` da tarefa), terminal, VS Code, copiar
  o caminho, **Abrir neste painel**, configurar o projeto e, quando a cópia já não vai ser juntada,
  **Apagar a cópia…** em vermelho. A seta no fim mostra o objetivo, os critérios, o comando de
  verificação, a pasta e o branch da cópia (cada um com copiar) e a ressalva da aprovação. O detalhe
  abre e fecha deslizando.
- Clicar no cartão (ou Enter, com ele em foco pelo Tab) abre o trabalhador à direita dos painéis
  da equipe; Espaço mostra ou esconde o detalhe; também dá para arrastar o cartão para um painel.
  Com um trabalhador aberto, a tarefa dele fica com borda e barra azuis e o detalhe aberto, e o
  painel rola até ela ao trocar de conversa. Quem pede permissão fica com o cartão âmbar.
- As concluídas ficam em **Concluídas (n)**, recolhidas enquanto houver abertas, numa linha cada
  (aprovada, cancelada ou falhou no ícone, a ressalva à direita e o **…** no hover). Embaixo,
  **Decisões**, que também recolhe. Sem tarefas, o painel diz que elas aparecem quando o coordenador
  dividir o trabalho. As duas seções abrem e fecham deslizando, e a pílula de uma tarefa pisca mais
  forte por um instante quando ela muda de estado com o painel aberto.
- Num painel estreito (abaixo de uns 330 px, ou a 150% de zoom), os botões ficam só com o ícone e o
  detalhe da pílula e o tempo do motivo vão para a dica.
- Na gaveta (janela estreita), o painel é opaco por cima das conversas, com o fundo escurecido; ele
  entra deslizando da borda direita, e o botão do canto (ou Esc) o recolhe.
- No trabalhador, o ↰ no lugar do símbolo da aba leva ao coordenador (Ctrl+clique abre ao lado), e
  o ⋯ do painel tem **Cópia t2**, com o menu da cópia: rodar, terminal e VS Code na cópia, não na
  pasta original. Num projeto sem git, um ponto âmbar no ↰ avisa que ele mexe na pasta original (a
  dica da aba diz isso).

## Revisão das mudanças

No cartão da tarefa, **Revisar** abre os arquivos mudados no diff do GPUI Kit, num diálogo do
tamanho da janela: o diff num fundo preto, com cor de sintaxe, número antigo e novo em cada linha, o
que não mudou dobrado, **Unificado** ou **Lado a lado** num segmentado com a pílula que desliza
(Lado a lado apagado, com o motivo na dica, quando falta largura), **Arquivo
inteiro**, **Quebrar linhas** e as setas de mudança anterior e próxima. Em cima, o título da tarefa
(numa linha, inteiro na dica) e, embaixo dele, o projeto, a tarefa, a rodada, a pílula do estado e
o tamanho do diff ("3 arquivos · +11 −1").

- **Conferência em chips.** Na barra, **Verificação passou** (ou falhou, em vermelho) e **Revisor
  aprovaria** (ou **pede ajustes**, com o número de apontamentos na cor da maior gravidade). O
  clique abre a saída do comando (o fim dela, com **Mostrar tudo** e copiar) ou o parecer do
  revisor, com cada apontamento ("alta", "média", "baixa"): o clique vai à linha, e **Usar** o
  transforma em comentário. Enquanto a conferência roda, os chips mostram o spinner; da rodada
  anterior (o trabalhador ajustando), ficam em tom tênue e dizem isso.
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
  rodada de ajuste do trabalhador.
- **O botão azul segue o que está vermelho.** Sem pendências, o azul é **Aprovar e juntar**. Com a
  verificação vermelha nesta rodada, apontamento de gravidade alta, o diff que não deu para ler ou
  comentário por enviar, o azul passa para **Pedir ajuste** (com o selo do que vai junto: os
  comentários e, com o interruptor ligado, os apontamentos que você não usou), e **Aprovar e
  juntar** fica fantasma, com o ícone de junção. **Ctrl+Enter** aciona o botão azul do momento (abre
  o Pedir ajuste ou aprova), e a dica dele mostra o atalho; num campo de texto, com um comentário
  aberto ou com um popover aberto, o Ctrl+Enter fica com eles.
- **Aprovar e juntar** fica desabilitado com o motivo quando a tarefa não pode ser aprovada
  (conferindo, trabalhador rodando, já juntada); com pendências, pergunta antes. Um conflito na
  junção aparece num aviso dentro do diálogo, que continua aberto. **Abrir a cópia** tem rodar,
  terminal, VS Code e copiar o caminho, na cópia.
- **Fechar não perde nada**: Esc, o × e o clique fora fecham, e ao reabrir os comentários, o
  rascunho e os vistos estão lá. Se chegou uma entrega nova, o diff é relido (com o diálogo aberto,
  um aviso oferece **Recarregar**).
- Sem cópia (apagada, ou projeto sem git), com erro do git ou sem mudanças, o diálogo diz isso no
  lugar do diff. Aprovada ou cancelada, a revisão é só de leitura.
- Dá para descartar as mudanças de um arquivo (com confirmação): um renomeado volta ao nome antigo
  e, se não der para voltar, aparece "Não deu" e o arquivo fica na lista. Nomes com acento, espaço
  ou aspas aparecem certos aqui, em Arquivos e no `@`.

## Cópias das tarefas

Com `node_modules`, `.venv` e `target/`, cada cópia pode passar de 1 GB. No **…** do cartão de
uma tarefa encerrada (ou de um coordenador arquivado), **Apagar a cópia…** mede o espaço e
pergunta antes. Arquivar um coordenador parado oferece apagar as cópias que sobraram. Em
Configurações, **Cópias das tarefas** mostra o espaço ocupado ("Medindo…", com o spinner, enquanto
mede) e quantas são de tarefas encerradas, tem **Limpar N** (com o número; desligado quando não há
nenhuma) e a escolha de apagar sozinho depois de nunca, 7 ou 30 dias. O aviso no fim diz quantas
saíram e, se alguma não deu, qual e por quê. A limpeza apaga a pasta, o branch e os pontos de volta
da tarefa; a aprovação também. A conversa de um trabalhador sem cópia continua para leitura, mas
não roda mais. As cópias que os agentes de um fluxo criam dentro da cópia da tarefa
(`.claude/worktrees/`) não entram no commit da tarefa.

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
créditos extras do mês e, no rodapé, quando o número foi lido, com **Atualizar**. O uso vem do
que o Claude Code e o Codex guardam no disco (relido a cada 30 s) e dos avisos de limite
durante os turnos; nada roda e nenhum token é lido.

No painel do Claude também se escolhe a conta padrão da pasta, em **Conta padrão**: clicar numa
linha (**Automática**, "a de menor uso em cada turno", ou uma conta, com o disco da marca e os
dois anéis, como no trilho (5 h por fora, a semana por dentro), o e-mail inteiro em cima e o uso
embaixo, "5 h 7% · semana 38%", sem a janela que ainda não foi lida) a torna a padrão, e o check
fica nela; **Entrar com outra conta…** conecta uma nova e o **…** de uma conta extra tem
**Desconectar…**. **Contas…**, no rodapé, abre o diálogo das contas (também no menu do logo,
**Contas do Claude**, ou em **Gerenciar contas…**, no chip da conta na caixa de mensagem). Ele
repete o seletor do popover, maior: em **Conta padrão**, a Automática e uma linha por conta com o
mesmo anel do trilho (5 h por fora, a semana por dentro), o login e o plano, o uso ("5 h 23% ·
semana 41%", com a hora de renovar na dica, ou "no limite · volta às 14:30") e o check na padrão; o
clique numa linha a torna a padrão. O **…** de cada conta tem **Usar como padrão nesta pasta**,
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
  maximiza e o clique direito abre o menu da janela): o logo (clique ou F10 para o **menu do
  Vitral**, que abre à direita: no cabeçalho, "Vitral" e o nome da pasta de trabalho, com o caminho
  inteiro na dica; depois três grupos, Nova conversa, Novo projeto… e Rotinas; Contas do Claude,
  Instalar o Codex… quando ele falta, e Configurações; Trocar a pasta… e Abrir a pasta ↗; com os
  atalhos de cada um; com projeto arquivado, entre o primeiro e o segundo grupo, **Projetos
  arquivados**, cada um com o quadrado apagado e **Desarquivar**), um atalho por projeto (filtra tudo por ele; o ativo ganha um aro na cor do
  projeto; o ponteiro em cima acende o quadrado suave, e o aro entra na mesma transição; um ponto
  mostra erro em vermelho, pedido ou parada em âmbar, resposta nova em azul e, vazado, algo
  rodando), o sino do que precisa de você e os anéis de uso do Claude e do Codex (o logo e os anéis
  ficam sobre um disco preto, que clareia com o ponteiro em cima). Quando os projetos
  não cabem na altura, o último lugar vira **+N**, que abre **Todos os projetos** com busca (os
  arquivados num grupo à parte, "Escolher desarquiva", e Novo projeto… no rodapé); o projeto filtrado sempre fica à vista. Com todos cabendo, o último lugar é
  **Novo projeto**. Os dois também acendem suave. O número do sino é o mesmo de "Esperando você"
  (pedidos, erros e paradas que pedem Retomar), em vermelho se algo falhou e âmbar nos outros casos;
  só com respostas novas, um ponto azul. Quando o número aumenta (um pedido, uma parada ou um erro
  novo), o sino pulsa uma vez com um anel âmbar (vermelho se algo falhou); abrir o app, o número
  cair ou marcar como lido não pulsam, e com as animações desligadas nada pulsa. O clique abre **O
  que precisa de você**: cada linha com o quadrado do projeto e, embaixo, a mesma frase e o mesmo
  sinal da prévia da lista (o ponto na cor e o rótulo: "Permissão · Bash · npm test", "Plano para
  aprovar", "Parada · Vitral fechou", "Erro · …"): os pedidos (com Negar e Permitir, ou **Permitir
  mesmo assim** em vermelho quando o comando tem um trecho arriscado; um plano abre a conversa para
  ler antes), as paradas (Retomar), os erros (Tentar de novo) e as respostas novas (marcar como
  lida), com o número do painel quando a conversa está aberta; clicar numa linha abre a conversa
  (Ctrl+clique, ao lado). No rodapé, os botões **Marcar tudo como lido** e **Ver na lista** (tira
  filtro, busca e arquivadas e mostra a seção); cabeçalho, seções, linhas e o ícone do rodapé ficam
  na mesma guia, a 16 da borda.
  Ctrl+G vai direto à próxima pendência.
- **Ctrl+B recolhido.** A lista e o trilho somem e a conversa vai até a borda. No lugar deles ficam
  o logo, no começo das abas do 1º painel, com o selo do sino (o número ou o ponto, que pulsa como
  o sino), e os anéis de
  uso, de 24, no cabeçalho do 1º painel, logo depois do logo (o do Codex só quando está instalado e
  tem uso lido; os popovers abrem para baixo). O menu do logo ganha, em cima, **Mostrar a lista**,
  **O que precisa de você** (abre a mesma caixa do sino) e os projetos (até 9, com Ctrl+1…9, e
  **Todos os projetos…**); escolher um projeto liga o filtro e traz a lista de volta. Ctrl+B de novo
  volta tudo.
- **Ícone e cor do projeto.** Cada atalho tem um ícone do Lucide e uma cor. Sem escolha, o ícone
  sai do conteúdo da pasta (um `Cargo.toml` vira chip, um `manage.py` ou `Dockerfile` vira
  servidor, um `next.config` vira site, só `.md` vira documento) e a cor, do nome, numa paleta
  sem as cores de estado. Para trocar: clique direito no atalho ou **Ícone e cor…** no menu
  **…** do projeto, que abre a prévia, a busca de ícone pelo nome ("loja", "banco"), 60 ícones e
  8 cores, com o escolhido na cor do projeto; **Usar automático** (também no clique direito,
  quando há escolha gravada) volta ao palpite, e **Salvar** (ou Enter) grava. Em **Configurar
  projeto…** a mesma escolha fica em **Aparência**, num popover, e só grava no Salvar. A escolha
  fica na configuração da pasta e vale também na lista, nos diálogos e em Arquivos.
- **Conversas lado a lado** (até 4, conforme a largura): arraste uma conversa da lista (ou a aba de
  um painel) para a borda esquerda ou direita de um painel para abrir ao lado, ou para o meio para
  trocar. A forma diz o que vai acontecer: meio painel em azul para "Abrir ao lado", uma barra na
  borda para "Mover para cá" e o painel inteiro para "Trocar a conversa" (com "Já há 4 lado a lado"
  quando não cabe outro); a aba que você arrasta fica apagada e tracejada. Também dá com Ctrl+clique
  ou o botão "abrir ao lado" no cartão. O painel que abre ao lado entra com um fade (a largura não
  anima). Com vários painéis, cada um tem um número em forma de tecla (na aba, na lista e na
  Equipe), Alt+1…4 vai para ele, e o ativo tem a aba azul e a borda e um halo azuis, que acendem
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
  direita, com a dica **Equipe** (ou **Arquivo aberto**, quando é o arquivo que ela mostra) e o
  Ctrl+J. Com a Equipe fora da vista (a direita fechada, ou a gaveta dela fechada) e tarefas que
  pedem você, o botão leva no canto um selo com o número, âmbar, ou vermelho se uma caiu, e a dica
  diz quais ("1 em revisão, 1 parada"; também "pedindo permissão" e "com erro"). Quando o botão vai
  para o **⋯**, o item vira **Mostrar a Equipe** (ou **Mostrar o arquivo aberto**), com o mesmo
  número tingido. Ligados, ficam num cinza mais
  claro, nunca azul. No coordenador, o quadrado do projeto na aba abre o menu do projeto (com
  **Mostrar só este projeto**; a dica diz o projeto e a pasta); no trabalhador, no lugar dele fica o
  ↰ que abre o coordenador. A dica da aba diz onde a conversa está ("loja › Carrinho… · cópia t4").
  O **⋯** tem **Nova conversa ao lado**, **Mover para a esquerda/direita**, **Fechar os outros
  painéis**, **Fechar este painel**, o menu da conversa (renomear, fixar, arquivar, abrir a pasta
  fora do app) e, embaixo, o que a aba não mostra: **Cópia t2** no trabalhador, **Projeto loja**
  quando o quadrado não está na aba e **Abrir o coordenador** quando o ↰ não está. O custo ("US$
  0,42") está no anel do contexto da caixa. Uma linha separa o cabeçalho das mensagens só quando há
  mensagens roladas por baixo dele. Em painel estreito o modo foco e depois o painel da direita vão
  para o **⋯**; no mais estreito, com vários painéis, o número toma o lugar do símbolo da aba.
  Nenhum texto é cortado no meio.
- **Painel sem conversa:** "Comece uma conversa" com **Nova conversa** (ou, sem projetos, "Nenhum
  projeto ainda" com **Novo projeto…**); dá para soltar ali uma conversa arrastada da lista.
- **Zoom:** Ctrl + roda do mouse (ou do touchpad), Ctrl+= / Ctrl+- e Ctrl+0 (volta a 100%)
  aumentam e diminuem a interface inteira de 10 em 10%, de 70% a 200%, como num navegador; a
  porcentagem aparece no alto da janela por um instante. Tudo cresce junto, inclusive as larguras
  das partes: a 150% a lista normal tem 468 px, a Equipe 570 e cada conversa pede no mínimo 420,
  então numa janela que não comporta a Equipe e depois a lista viram gaveta, como numa janela
  estreita. O que você arrastou nas divisórias fica guardado a 100% e acompanha o zoom (a lista
  de 240 a 100% vira 360 a 150%, e a divisão entre as conversas se mantém). Vale para o app todo
  e fica guardado em `~/.config/vitral/interface.json`.
- **Visual:** o fundo é um vidro fosco, mais opaco, em que o papel de parede aparece apagado atrás
  do trilho e da lista; nas frestas entre as peças e na margem ele fica num chumbo azulado escuro, e
  as peças ficam mais claras que o fundo, para se destacarem. O trilho, a lista, os painéis, o
  seu balão e os cartões da Equipe e de entrega têm o reflexo do vidro (a linha de luz e o brilho no
  topo). O vidro azul-marinho fica no fundo (trilho, lista, painéis, diálogos e as gavetas); o
  preto entra no que vai por cima e por dentro, para dar profundidade: popovers, menus, dicas, avisos
  e a barra de ações das mensagens são quase pretos (#0c0e13, opacos, com a borda clara fina e a
  linha de luz no topo); os blocos de código, as saídas de comando, o comando da verificação e o
  diff do Revisar ficam num preto mais fundo (#08090c), com a mesma cor de sintaxe; a caixa de
  mensagem vai do azul, à esquerda, ao preto; e o logo e os anéis de uso do trilho ficam sobre um
  disco preto, para a cor de cada marca saltar. Dentro de um popover, o que no vidro é um recuo
  escuro (o trilho do segmentado do esforço, o recado do Pedir ajuste) vira um claro leve. O
  logo é um vitral redondo: o completo (`assets/vitral.svg` e PNGs de 64 a 256 px) no menu de
  aplicativos e na tela de boas-vindas, o pequeno (`assets/vitral-pequeno.svg`, feito para 16 a 32
  px, e o PNG de 48) no trilho e nos ícones pequenos. Popovers e menus entram com um fade e 6 px
  vindos do lado do botão que os abriu, e todos os flutuantes têm uma linha de luz de 1 px no topo,
  como o reflexo de um vidro; as gavetas da lista e da Equipe entram deslizando 24 px da borda, em
  180 ms, com o véu em fade junto, e fecham secas. O véu dos diálogos cobre só a janela, não a
  margem da sombra. A pílula do segmentado (Conversas · Arquivos, o esforço) desliza até a
  opção nova; os botões redondos (enviar, +, sino) acendem suave com o ponteiro em cima; os anéis de
  uso e de contexto deslizam até o valor novo; o sino pulsa quando chega algo que precisa de você;
  e o ponto de "rodando" respira enquanto um spinner gira. Listas, menus e cartões continuam com o
  hover seco. "Em revisão" fica no azul do rodando (a pílula, o ponto e a parte da barra da Equipe);
  o âmbar é só do que precisa de você. Com o ponteiro fora da janela, nada fica em hover nem com a
  dica aberta. Na Equipe tudo começa na mesma coluna, a
  24 da borda do painel (o texto de Tarefas, das seções e dos cartões, o ícone das concluídas e o
  marcador das decisões), e o espaço é medido no que se vê: 8 entre cartões, 16 antes de cada seção
  e, dentro do cartão, 8 entre as linhas e 12 da borda ao título e ao botão.
- **Janela estreita:** quando falta largura, o painel da direita e depois a lista saem do lado e
  abrem por cima, como gaveta (Ctrl+J, Ctrl+B, Ctrl+K e os botões de sempre); Esc ou um clique
  fora fecham. A gaveta entra deslizando da borda dela, com o fundo escurecendo junto; fecha seca.
  Os botões de janela seguem o layout do GNOME (no Fedora, só o ×).
- **Lista** numa peça de vidro própria, com as abas **Conversas · Arquivos** num segmentado (um
  ponto âmbar em Conversas avisa, de dentro de Arquivos, que algo espera você), a busca (Ctrl+K;
  acha também no texto das mensagens e nas conversas arquivadas) e o **+** de nova conversa: com um
  projeto no escopo, ele cria a conversa direto nele. Tudo na lista começa a 16 px da borda da peça
  (o texto dos cabeçalhos de seção, o quadrado do projeto, a lupa da busca e o primeiro ícone do
  rodapé); a seta das seções fica à esquerda dessa guia. A busca diz só "Buscar"; o que ela acha e o
  Ctrl+K ficam na dica do campo (que não aparece enquanto se digita nele). Na busca, quando o termo
  não está no título, a prévia do cartão mostra o trecho que casou ("Você: valide o **frete**
  grátis…", com o termo em forte; de um trabalhador, "#2: …"), e as arquivadas que casam aparecem no
  fim, numa seção **Arquivadas** (com o número) e com o cartão tênue. Fora da busca, elas continuam
  só em Arquivadas. Estreitando a lista (ou com o zoom alto, porque a conta é a mesma), ela abre mão
  do que é menos importante antes de cortar o título: abaixo de uns 290 px (a 100%), os
  trabalhadores perdem o selo de pedido ou erro (o ponto já diz) e, no hover, fica só o **…**;
  abaixo de 275, as abas ficam só com o ícone e a prévia fica com o rótulo curto do motivo: o do
  próprio coordenador sai (o ponto diz qual é), e o de um trabalhador fica só com o número e o
  "(+N)" ("#5 (+1) · Vitral fechou"); abaixo de 260, a busca de arquivos diz só "Buscar" e **Retomar
  tudo** e **Arquivar estas** ficam só com o ícone. Os horários continuam à vista. A barra de
  rolagem é fina, colada na borda da peça, não cobre os cartões e aparece e some sem fade, 1,2 s
  depois de rolar.
- **Linha de escopo** embaixo da busca quando algo restringe a lista: o projeto escolhido no
  trilho (com o **…** do projeto, o **×** que mostra todos e um "+N" âmbar quando há conversas
  esperando você em outros projetos; o clique tira o filtro), as Arquivadas, ou o projeto da aba
  Arquivos (com **↻** para reler os arquivos).
- Seções, cada uma com o próprio cabeçalho: **Esperando você** (o número é o mesmo do sino: pedidos,
  erros e paradas; respostas novas não contam, viram ponto; pedidos, paradas e erros vêm antes das
  respostas novas, e com duas paradas ou mais aparece **Retomar tudo**), **Rodando agora**,
  **Fixadas** e as faixas de data (**Recentes**, até 7 dias, e **Mais antigas**; o tempo de cada
  conversa já está no cartão). Em Esperando você os cartões são neutros como os outros (selecionado
  em azul); o âmbar fica no ponto do quadrado, no ponto da prévia e no número da seção. Fixadas e as
  faixas recolhem no clique do cabeçalho (a seta fica à esquerda do nome, e o hover clareia os
  dois); Mais antigas começa recolhida e tem **Arquivar estas**, que arquiva na hora e
  mostra "3 conversas arquivadas" com **Desfazer** (que devolve só as que ela arquivou). O
  **Arquivar** do menu da conversa também mostra "Conversa arquivada" com Desfazer; desarquivar não
  avisa.
- **Cartão de conversa**: o quadrado do projeto (ícone e cor dele) com o ponto do estado da família
  no canto (vermelho erro, âmbar pedido ou parada, azul resposta nova, aro azul rodando), o título,
  a prévia com o motivo no ponto do sinal e no rótulo ("Permissão · Bash · npm test", "Plano para
  aprovar", "Parada · Vitral fechou", "Erro · …"; quando é um trabalhador, "#5 parada · Vitral
  fechou"; a mesma frase do sino; a resposta nova só com o texto, porque o ponto azul e o título em
  forte já dizem; rodando, "#3 · Edit · README.md"; as prévias começam com maiúscula, "Pensando…",
  "Na fila, esperando vaga") e, à direita, o horário (ou há quanto tempo roda), o número do painel e
  o ponto de não lido (só quando o ponto do quadrado não é já o azul da resposta nova). No hover, o
  horário dá lugar a duas ações: **Abrir ao lado** (ou Ver o pedido da tarefa, ou Retomar) e **…**;
  fora do hover, o título usa o lugar delas, e só o cartão sob o ponteiro (ou realçado pelo
  teclado) corta o título para mostrá-las. O clique
  direito ou o **…** abrem o menu da conversa: abrir ao lado, renomear, fixar, arquivar e abrir a
  pasta no VS Code ou no terminal. Clique duplo também renomeia; Ctrl+clique ou o clique do meio
  abrem ao lado.
- **Trabalhadores** embaixo do coordenador, numa árvore: um traço fino sai do centro do quadrado
  do pai e passa por um ponto de 8 por trabalhador, na cor do estado da tarefa (o sinal quando
  roda, pede, falhou ou parou e pede Retomar; o aro de quem roda fica solto, com o traço parando
  antes dele); "#2 título" fica na coluna do título do pai, e a palavra do estado vai para a dica
  do ponto ("Em revisão" e a explicação). À direita só o que pede ação (o selo "permissão",
  "plano" ou "erro"), a duração de quem roda e o número do painel. Fora da conversa aberta, as
  tarefas aprovadas e canceladas ficam em **Mais N concluídas**, no fim do ramo (o traço termina na
  seta).
- **Teclado na lista**: ↓ na busca entra na lista; ↑/↓, Home e End andam pelos cartões; → mostra
  as concluídas da família (e os trabalhadores, nas faixas de data), ← esconde ou volta ao
  coordenador; Enter abre (e, com pedido, põe o cursor na caixa), Ctrl+Enter abre ao lado, F2
  renomeia e Esc volta à busca. Na busca, Esc limpa e, vazia, volta à caixa do painel.
- **Rodapé**, só quando há o que mostrar: **Arquivadas** (o ícone com o número; liga e desliga a
  vista das arquivadas, que mostra também as conversas dos projetos arquivados; a busca acha as
  duas) e, com algo rodando, quantos agentes rodam em toda a pasta ("2 rodando · 1
  na fila"; o clique volta ao topo da lista). Na aba Arquivos, quantos arquivos o projeto tem.
- Aba **Arquivos**: os arquivos do projeto numa árvore (pastas recolhidas, menos o caminho até o
  arquivo aberto); com busca, uma lista plana sem limite, com o trecho achado em destaque e a
  pasta cortada no meio. Clicar abre no painel da direita: a aba com o nome e o ícone do tipo, e
  uma barra com a pasta, o número de linhas e a linguagem, **Mencionar**, **VS Code** e o **…**
  (copiar o conteúdo ou o caminho, mostrar na pasta). O markdown sai renderizado; o código, com
  cor e número de linha, rola para os lados. Arquivo grande (mais de 400 KB), binário, vazio ou
  que sumiu dá um aviso no lugar, com **Abrir no VS Code** ou **Fechar**. Na lista, no hover,
  **@** menciona o arquivo na conversa e **…** (ou o clique direito) abre no VS Code, copia o
  caminho ou mostra na pasta. A lista é relida a cada
  20 s em segundo plano, sem travar a tela.
- Menu **…** do projeto, o mesmo no clique direito num atalho do trilho (com o quadrado, o nome
  e o caminho em cima): rodar o projeto num terminal, abrir no VS Code, no terminal e no
  gerenciador, Ícone e cor… (e Usar automático, quando há escolha gravada) e Configurar
  projeto… (com o quadrado e o caminho do projeto em cima: ícone e cor, preparação de cada cópia,
  arquivos que vão junto e comando de rodar, os comandos em fonte mono; sem git, um aviso com
  **Ativar git…**; o Salvar só acende quando algo mudou, e o Enter num campo salva) e, no fim,
  separados, **Arquivar projeto** (**Desarquivar projeto** num arquivado) e **Mover para a
  lixeira…**, em vermelho (ver "Arquivar e apagar projetos").
- **Rotinas:** um pedido que o Vitral manda sozinho num horário e dias da semana, abrindo um
  coordenador novo no projeto (com o Vitral aberto; se estava fechado, roda ao abrir). O diálogo
  abre na lista (o quadrado do projeto, o pedido, "loja · Seg a Sex às 08:00 · próxima: amanhã" e
  a chave de pausar; no hover, **Rodar agora**, **Editar** e **Apagar**, que tem Desfazer). A de
  um projeto arquivado diz "projeto arquivado, não roda", e o Rodar agora fica apagado. **Nova
  rotina** (ou o clique numa rotina) troca para o formulário: o pedido em várias linhas (Enter
  salva, Shift+Enter quebra), a hora num campo de hora e minuto (↑/↓ em cada parte), os dias em
  chips com **Dias úteis** e **Todo dia**, e o projeto num menu. O que falta aparece no campo, e o
  **Criar rotina** diz na dica o que falta; a seta volta à lista. Sem rotinas, **Usar exemplo**
  preenche um.
- **Configurações** (Ctrl+,) em cinco grupos: Conversas, Trabalhadores, Revisão, Cópias das
  tarefas e Interface, gravando na hora. Em Interface, **Animações** (Seguir o GNOME ou
  Desligadas) vale para o app todo, em todas as pastas, e muda na hora; seguindo um GNOME sem
  animações, a linha diz isso. Os menus mostram o ícone e a cor do modo, ou a marca do Claude e uma
  linha sobre cada modelo; os prazos são escolhas lado a lado; o (i) explica o resto. Em
  Conversas, **Arquivar conversas paradas** (nunca, 3, 7 ou 30 dias) arquiva só coordenadores já
  lidos e não fixados, sem nada rodando, pedindo ou parado e com as tarefas encerradas. Com o
  diálogo estreito (zoom alto ou janela pequena), os controles descem para baixo do texto.
- **Diálogos** opacos, com cantos de 20, o título grande e o × com a dica; o rodapé fica sempre à
  vista e só o meio rola. **Mencionar arquivo** e **Conversa como contexto** são listas com busca
  e teclado (os arquivos são lidos em segundo plano, com o ícone do tipo e a pasta embaixo; as
  conversas, por projeto, com quando falaram e quantas mensagens têm). **Renomear** já abre com o
  título selecionado e não salva vazio.
- Notificação do sistema, só com a janela em segundo plano, quando um coordenador termina tudo,
  quando uma conversa pede permissão ou quando um trabalhador cai e a tarefa volta ao coordenador:
  com o ícone do Vitral, "projeto · conversa" no título e uma linha só ("Precisa da sua permissão:
  Bash, $ cargo test", "Terminou: 3 tarefas aprovadas", "Parou com erro: ..."). Clicar nela abre a
  conversa no painel ativo e traz a janela para a frente (ou o GNOME avisa que o Vitral está
  pronto). Uma notificação nova da mesma conversa substitui a anterior em vez de empilhar. A
  permissão se responde dentro do app, nunca na notificação.

Atalhos: `Ctrl+N` nova conversa, `Ctrl+K` buscar (`↓` entra na lista; nela, `Enter` abre,
`Ctrl+Enter` abre ao lado e `F2` renomeia), `Ctrl+J` painel da direita, `Ctrl+B` lista e
trilho, `Alt+1…4` painel N, `Ctrl+1…9` filtra pelo projeto N do trilho, `F10` menu do Vitral,
`Ctrl+,` configurações, `Ctrl+G` próxima pendência, `Ctrl+W` fecha o painel, `Ctrl+PgUp/PgDn`
painel vizinho, `Ctrl+Shift+PgUp/PgDn` muda o painel de lugar (na aba: `Enter` vai para a caixa,
`Delete` fecha, `Shift+F10` abre o menu), `Ctrl+End` vai ao fim da conversa, `F7`/`Shift+F7`
próxima e anterior mudança na revisão (nela, `Ctrl+PgUp/PgDn` vão ao arquivo anterior e ao
seguinte, e `C` ou `Enter` comentam as linhas escolhidas), `Shift+Tab` troca o modo, `Esc` para o
turno (ou fecha a gaveta), `Enter` envia, `Shift+Enter` quebra a linha, `Ctrl+roda`, `Ctrl+=` e
`Ctrl+-` zoom, `Ctrl+0` zoom normal. `Ctrl+Shift+M` (modelo) e `Ctrl+E` (esforço) abrem e fecham
os popovers da caixa do painel ativo (no do esforço, ←/→, Home e End mudam o nível, Espaço liga o
Ultrathink, U liga o Ultracode e Enter fecha (as teclas estão no rodapé do popover); Esc fecha sem
parar a conversa); no Revisar, `Ctrl+Enter` aciona o botão azul do rodapé; `Ctrl+M` fica reservado
para o Mestre.

## Compilar e rodar

```sh
./instalar.sh                           # compila e põe no menu de aplicativos (com o ícone)
./target/release/vitral ~/Projetos      # abre nessa pasta
./target/release/vitral ~/Projetos --nova meu-projeto "o que fazer"
cargo test                              # testes de unidade (git, motor, revisor, processos, interface)
ferramentas/bancada.sh roteiro.txt prints/ ./target/debug/vitral ~/pasta   # teste visual isolado
./target/debug/vitral --vitrine         # todos os componentes da linguagem visual, sem pasta e sem motor
```

Sem a pasta, o Vitral abre nas boas-vindas ("Boas-vindas ao Vitral"): **Escolher pasta…**, que fica
carregando enquanto o seletor do sistema está aberto, e até 5 recentes, com o caminho cortado no
meio (inteiro na dica).

`./instalar.sh` põe o logo completo no tamanho escalável e de 64 a 256 px, e o pequeno
(`vitral-pequeno.svg`, feito para tamanhos pequenos) em 16, 24 e 32 px e no PNG de 48, o que as
notificações e o alt+tab usam.

Bancada: `FUNDO=branco` (ou `FUNDO=<imagem>`) troca o papel de parede e `ZOOM=1.5` abre com esse
zoom. `python3 -I ferramentas/demo_vitral.py <pasta> [--16]` cria a pasta de demonstração (4
projetos, 11 conversas, 8 tarefas em todos os estados, rotinas; com `--16`, mais 12 projetos), sem
rodar nenhum processo; gere de novo antes de cada rodada. `python3 -I ferramentas/medir_print.py
<print.png> X Y` (ou `--pontos <arquivo>`) mede o contraste de um texto no print. Os roteiros da
linguagem visual ficam em `ferramentas/roteiros/` (`4c-antes.txt` tira todas as telas;
`4c-zoom-larguras.txt` vai de 100% a 150% com a lista arrastada, os popovers do Claude e do sino, a
janela estreita e as gavetas, e `4c-zoom-lista-estreita.txt` estreita a lista); no meio de um
roteiro, `tecla Control_L+equal` muda o zoom, e `rolar X Y N` gira a roda N entalhes com o ponteiro
em X, Y (N negativo sobe; serve para ver o fim de um diálogo que rola).

`VITRAL_QUADROS=1` escreve "quadros/s: N" no stderr a cada segundo em que a janela redesenhou
(nada em repouso): é a conferência do custo dos efeitos. `VITRAL_SEM_ANIMACAO=1` abre como se as
animações do GNOME estivessem desligadas. A vitrine (`--vitrine`) ganhou as seções **Movimento**
(um botão para cada efeito) e **Espaçamento** (a escala e as guias da lista, da Equipe e da
conversa).

Precisa do `claude` (Claude Code) no PATH ou em `~/.local/bin`, do `git`, e no Fedora de
`libxkbcommon-devel libxkbcommon-x11-devel wayland-devel vulkan-loader-devel fontconfig-devel
freetype-devel libxcb-devel` para compilar. Das dependências em Rust, o `chrono` entrou para a hora
das Rotinas (o `TimeField` do kit); já vinha no `Cargo.lock` pelo vendor.

## Arquivos

| Arquivo | O que faz |
| --- | --- |
| `src/motor.rs` | Chats, tarefas, turnos, fila, contas, rotinas, retomadas e novas tentativas, Ultracode e agentes em segundo plano, ciclo coordenador → trabalhador → revisão |
| `src/claude.rs` | Processo do Claude Code em `stream-json` (argumentos e ambiente do turno), leitura dos eventos, versão do Claude Code e conferência da sessão |
| `src/mcp.rs` | Servidor MCP local com as ferramentas do Vitral |
| `src/revisor.rs` | Revisor independente e o hook que roda os testes quando o trabalhador para |
| `src/processos.rs` | Escopos do systemd (ou grupos de processos) para parar cada processo por inteiro |
| `src/contas.rs` | Contas extras (`CLAUDE_CONFIG_DIR`), login e o uso e o plano que o Claude Code guarda em `.claude.json` |
| `src/codex.rs` | Uso do plano do Codex, lido das sessões em `~/.codex/sessions` |
| `src/git.rs` | Cópia isolada, diff, fotos/volta, junção (uma por vez por projeto), verificação, limpeza das cópias |
| `src/prompts.rs` | Instruções do coordenador e dos trabalhadores |
| `src/modelo.rs` | Tipos e gravação em `.vitral/` |
| `src/tema.rs` | Tokens da linguagem visual (cores do vidro e das partes pretas, medidas em rem com `r()` e `tam()`, tipografia, sombras e gradientes), tema do GPUI Kit, cores de sintaxe e ícones extras do Lucide |
| `src/ui/` | Trilho, lista, painéis (`conversa.rs` é a casca: aba, cabeçalho, menu do painel e popover de uso; `mensagens.rs` o que vai dentro; `paineis.rs` os painéis lado a lado e a soltura), caixa de mensagem, Equipe, revisão, gavetas e diálogos; `lateral.rs` (a lista), `composer.rs` (a caixa de mensagem) e `composer_seletores.rs` (os chips, menus e popovers dela), `menus.rs` (os menus do projeto, da cópia e da conversa, iguais em todo lugar), `equipe.rs` (a Equipe) e `painel_arquivo.rs` (o arquivo aberto à direita), `revisao.rs` (o Revisar: diff, comentários, pedir ajuste e aprovar), `caixa_entrada.rs` (o popover do sino e o Ctrl+G), `boas_vindas.rs`, `uso_plano.rs` (anéis e painel de uso, também no modo recolhido), `icone_projeto.rs` (ícone e cor dos projetos), `tirar_projeto.rs` (arquivar e mandar para a lixeira: a pergunta, os avisos e o que a janela esquece), `zoom.rs`, `movimento.rs` (a única porta de animação: durações, curvas, teto de 60 quadros por segundo, entradas e transições) |
| `src/ui/componentes/` | Os componentes da linguagem visual (chip, botões, segmentado, popover, menus, alertas e avisos, cartões de lista, opções, lista de escolha, selos, anéis, abas, diálogos, caixa de código), o estado único (sinal de cada chat e projeto, estado visual de cada tarefa) e a vitrine (`--vitrine`) |
| `assets/` | Logo completo e pequeno, PNGs de 48 a 256 px, logos do Claude e do Codex (e os discos coloridos em `marcas/`), o escudo cheio do modo Manual e os ícones de projeto (`icones/`, Lucide, ISC) |

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
(`campo_da_lista`). Os trabalhadores vão no nível 1 do `CartaoDeLista`: o `.sinal()` e, sem sinal,
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
conversa, Equipe, arquivo aberto, gavetas, popovers, menus, diálogos, avisos, a caixa de mensagem e
a tela cheia); `CARTAO` 14 no que vai dentro delas (cartões da Equipe e da conversa, balão,
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
clique). Na Equipe, o `Revelar` abre e fecha a altura em 180 ms pelo `movimento::valor`, medindo a
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

Projetos arquivados e lixeira: a marca é `ConfigProjeto.arquivado` (gravada só quando ligada).
`Motor::projetos()` já vem sem os arquivados (`Config::separar_arquivados`, com teste), e tudo o
que lista projetos passa por ele; `projetos_arquivados()` dá os outros. Uma conversa conta como
arquivada pela `Motor::arquivada` (`Config::conversa_arquivada`: ela mesma ou o projeto), usada no
sino, no Ctrl+G, nos sinais, na lista e no `arquivadas_que_casam`; o `c.arquivado` cru fica para
o menu da conversa e os trabalhadores. `ocupacao_do_projeto` diz o que roda (conversas: turno,
vaga, nova tentativa; tarefas: conferência, junção, preparação ou retomada na fila).
`mover_projeto_para_lixeira` confere de novo, valida a pasta (`pasta_para_lixeira`: subpasta direta
da pasta de trabalho, nunca ela nem a `.vitral`; com teste), roda o `gio trash` na vez do projeto
(`sistema::para_a_lixeira`, que confere que a pasta saiu) e, só depois, as cópias e o
`tirar_do_estado` (puro, com teste: conversas do projeto, trabalhadores das tarefas dele, tarefas,
filas, rotinas e configuração); o `esquecer` limpa os mapas do motor e nega pedidos de permissão. As
cópias vão para a Lixeira em vez do `git worktree remove`: com o repositório já na Lixeira o git
não as acha, e removê-las antes perderia o trabalho não juntado se a lixeira falhasse; o registro
delas fica no repositório da Lixeira (um `git worktree prune` limpa, se ele voltar). O gravador
apaga arquivos na vez deles (`Gravador::apagar`, que tira da fila uma gravação pendente do mesmo
arquivo): as mensagens das conversas e as notas saem depois do estado novo.

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

Ritmo da Equipe: o painel é uma pilha de `Bloco` (Caixa, Resumo, Secao, Linha), sem `gap`; o
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
coordenador o bloqueia em todo turno, junto com `Task`. O limite de agentes vai em todo turno. O
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
só então a tarefa vai para a verificação; e, no modo Manual (o Vitral pede permissão para o
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
