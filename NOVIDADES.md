# Novidades do Vitral

Cada versão tem a sua seção (`## 0.2.0`); o texto dela vira as notas do Release no GitHub e o que
o app mostra em **Novidades**. Os pré-lançamentos (`0.2.0-rc.1`) usam a seção da versão.

## 0.8.0

Um app mais enxuto, o Codex ao lado do Claude e uma conversa que mostra melhor o que o agente faz.

- **Abas consistentes.** Todos os painéis usam a mesma moldura nas abas; a selecionada muda de tom.
- **Fila junto da mensagem.** Abre acima da caixa, dentro do painel, com **Mandar agora** para
  enviar uma mensagem ao turno em andamento. O Codex também recebe novas instruções enquanto trabalha.
- **Trilha de atividade.** Resumo do trabalho, ramificações entre as chamadas e identificação dos
  arquivos. Os grupos abrem enquanto trabalham e recolhem ao terminar, respeitando sua escolha.
  Abrir uma edição mostra o diff; abrir um comando reúne o comando e sua saída.
- **Claude e Codex no mesmo chat.** Ligue "Claude e Codex juntos" no seletor de modelo. Cada mensagem
  vai aos dois, um de cada vez: um trabalha e o outro revisa o que ele fez (completa, corrige ou
  discorda), até um dizer "Nada a acrescentar" ou até 6 turnos. Antes da vez dele, cada um recebe o
  que o outro disse e os arquivos que mexeu. @claude ou @codex no começo da mensagem fala só com um.
  A conversa mostra quem está trabalhando.
- **Subagentes à vista de novo.** No Claude Code novo os subagentes trabalham em segundo plano, e o
  Vitral os dava como terminados logo ao começar. Agora eles ficam como trabalhando até o fim, com o
  que estão fazendo e quantas ferramentas usaram, e terminam com o resumo do que fizeram.
- **Corrigir qualquer mensagem sua.** O Editar aparece em todas as suas mensagens, não só na última
  (e não some mais quando o Claude pensa antes de responder). A conversa volta ao ponto antes dela.
- **Suas mensagens na largura toda** da coluna.
- **Imagens em pílulas, como na extensão do Claude Code.** As imagens que você manda (dentro do
  balão, em cima do texto), as que a resposta aponta e as que o agente leu ou gravou viram uma pílula
  com a miniatura, o nome e as medidas; o clique abre o visor.
- **Aviso de conversa grande e parada.** Depois de uma hora sem uso, uma conversa do Claude com
  mais de 100 mil tokens avisa, acima da caixa, quanto a próxima mensagem vai reler por inteiro, com
  o botão **Compactar**. O seletor de modelo avisa o mesmo custo antes de trocar o modelo.
- **Menos gasto escondido.** O Vitral desliga no Claude Code as chamadas que ele não mostra: a
  sugestão da próxima mensagem, o resumo de quando você volta e o título do terminal.
- **Conta que perdeu o login.** Quando o Claude Code avisa que o login de uma conta caiu, o chat
  continua sozinho em outra conta que esteja com login e diz qual; sem outra, o aviso mostra onde
  entrar de novo. Antes, o "Tentar de novo" repetia na mesma conta e falhava igual.
- **A conta mora no seletor de modelo.** Com mais de uma conta do Claude, a escolha fica na aba do
  Claude do seletor, e não mais solta na caixa de mensagem.
- **Histórico ao subir.** As mensagens anteriores aparecem automaticamente ao rolar para cima,
  mantendo o ponto de leitura. A barra fica visível quando a conversa tem conteúdo para rolar.

- **Mais simples.** Saíram os projetos, o Mestre, as rotinas, as issues e PRs e as cópias isoladas.
  Ficou o essencial: uma lista de chats, e cada chat trabalha direto na pasta dele, como o Claude
  Code no terminal ou no VS Code. Os dados antigos ficam guardados em `estado.versao-2.json`.
- **Claude ou Codex.** O seletor de modelo ganhou as abas **Claude** e **Codex**; a do Codex
  mostra os modelos que o Codex instalado oferece. Nas famílias do Claude, a versão de hoje
  aparece num selo ao lado (Fable 5.2, Opus 5.5). Escolher um deles passa o chat para o Codex; escolher um do Claude traz de
  volta. Na troca, a conversa segue numa sessão nova do outro, com as últimas mensagens. O Codex
  usa o login e o plano dele, e o modo de permissão vira o sandbox dele (Planejar só lê, Sem
  perguntar roda sem sandbox, os outros escrevem só na pasta). Nas Configurações dá para escolher
  quem responde nos chats novos.
- **A mesma pergunta para os dois.** No "…" da caixa, **Perguntar também ao Codex** (ou ao Claude)
  manda a mensagem para o chat e para um chat novo do outro agente, aberto ao lado, para comparar
  as respostas. O chat novo só lê, para os dois não mexerem nos mesmos arquivos ao mesmo tempo.
- **Raciocínio e subagentes na conversa.** Quando o modelo pensa, a trilha mostra "Pensou por 9,8 s",
  e abrir a linha mostra o raciocínio quando o Claude Code o entrega. O subagente virou um cartão
  com o que ele está fazendo agora, quantas ferramentas já usou e o relógio; aberto, lista os passos.
- **Mapa de agentes.** Com subagentes trabalhando, a linha da caixa mostra quantos são; o clique
  abre o mapa: a conversa de um lado e, ligados a ela, os agentes, cada um com o estado, o tempo e
  as ferramentas usadas. Clicar num agente mostra os passos e o resultado dele. O mapa também
  abre pelo "…" da caixa (**Agentes desta conversa**).
- **Mandar mensagem com o chat trabalhando.** A mensagem enviada durante um turno entra nele sem
  pará-lo, também nos turnos que começaram por uma mensagem assim (antes, essas iam para a fila).
  Na fila, **Mandar agora** põe uma mensagem que esperava dentro do turno que está rodando.
- **"Pensando…" na conversa.** Enquanto o modelo pensa, a linha viva aparece no fim da conversa,
  onde a resposta vai chegar. A linha embaixo da caixa ficou só com o relógio, as ações e a fila.
- **Suas mensagens.** O balão ficou azul e do tamanho do texto, e as imagens anexadas ficam fora
  dele, em cima.
- **Esforço automático.** No **Auto**, o seletor de esforço diz qual nível o modelo está usando.
- **Primeira abertura.** Uma tela de boas-vindas apresenta o app, pede a pasta de trabalho (como
  abrir uma pasta no VS Code) e pergunta o básico: permissões, modelo e se traz as conversas que já
  existem. O último passo permite conectar a memória da equipe. O "+" cria os chats nessa pasta;
  **Pasta de trabalho…**, no menu do logo, troca.
- **Memória da equipe.** Conecte o Vitral ao servidor ai-memory da empresa com seu endereço e sua
  chave. Claude e Codex consultam e registram decisões, procedimentos e aprendizados na mesma
  memória. **Consultar memória** busca e abre as páginas dentro do app, sem gastar o plano dos
  agentes. Não precisa instalar o ai-memory no computador, e o histórico dos chats não é enviado
  automaticamente. A conexão também pode ser configurada depois no menu do logo e nas Configurações.
- **Revisão da memória.** Depois de uma sessão que editou arquivos, o Vitral pede ao agente que
  avalie o que vale guardar. Pode ser desligada nas Configurações.
- **A janela volta como estava.** Ao abrir, o Vitral recoloca os painéis, as abas, a largura da
  lista e o tamanho da janela de quando foi fechado.
- **Imagens à vista.** As imagens que o agente lê ou gera aparecem na conversa, em grade quando são
  várias. O visor ganhou zoom pela roda do mouse, arrasto e os botões de aproximar e afastar.
- **Responder a um trecho.** Selecione um pedaço de uma resposta e clique em **Responder a este
  trecho**: ele vai citado junto com a sua mensagem.
- **Skills no "/".** A barra mostra os comandos e as skills da pasta já antes da primeira mensagem.
- **Modelos sempre em dia.** O seletor pergunta ao Claude Code instalado quais modelos existem, e a
  família (Fable, Opus, Sonnet, Haiku) usa sempre a versão mais nova que ele oferece.
- **Outra conta sem código.** Ao entrar numa conta extra, basta autorizar no navegador: o Vitral
  percebe sozinho e a conta aparece.
- **Servidores MCP.** `/mcp` na caixa (ou **Servidores MCP**, no menu do logo) mostra os servidores
  do chat, se estão ligados e quantas ferramentas cada um dá, sem precisar do terminal.
- **Qualquer arquivo abre no Vitral.** O link de um arquivo de fora das pastas dos chats abre no
  painel da direita, como os outros.
- **Mais leve.** Cada painel desenha só as últimas 60 mensagens (**Mostrar anteriores** traz o
  resto), o histórico carrega de 60 em 60 e a rolagem ficou suave. Com um chat
  trabalhando, a janela redesenha no máximo 12 vezes por segundo (eram até 28), e a digitação e a
  rolagem respondem melhor.
- **Acabamento.** Cantos mais redondos, que acompanham o zoom; o uso do plano foi para o canto de
  cima, à direita; o ícone de cada chat mostra o estado dele (trabalhando, esperando você, com
  erro); arrastar a borda da lista até o fim a recolhe; e os fundos podem ficar um pouco
  transparentes (**Transparência dos fundos**, nas Configurações; o padrão é opaco).
- **Lista mais legível.** Com o chat aberto num painel, a linha mostra só o número do painel, e o
  título ganha o lugar da hora (ela vai para a dica do ícone, junto da pasta). A hora ficou mais
  clara, e o número do painel ativo deixou de parecer um aviso de não lidas. O número das
  Arquivadas foi para a dica.
- **Porcentagem do contexto sempre à vista**, também com três painéis lado a lado: o anel sozinho
  parecia algo carregando.
- **Histórico em cada painel.** O relógio no cabeçalho, como na extensão do Claude Code, abre a
  lista de todos os chats, do mais recente ao mais antigo, com busca: o escolhido abre neste
  painel, numa aba.
- **Os anéis de uso não cobrem mais o texto.** Com o painel da direita aberto, o conteúdo dele
  deixa o lugar dos anéis; sem ele, as mensagens, a faixa e a caixa do último painel terminam na
  mesma linha, ao lado dos anéis.
- **Menos texto cortado.** O título "Enquanto você esteve fora" quebra em duas linhas na lista
  estreita, em vez de virar "Enquanto voc…". A faixa de cima da caixa diz "Parada · Vitral
  fechou", como a lista, com o motivo inteiro na dica. O aviso de turno cortado pelo fechamento do
  Vitral virou um marco de uma linha, "O Vitral fechou no meio do turno", sem repetir a instrução
  a cada turno nem falar do Claude no chat do Codex.
- **Trilha de ferramentas mais regular.** O chevron fica sempre no fim da linha, o detalhe que é
  código vai num chip em mono e o que é texto (a tarefa de um subagente, uma busca na web) vai em
  letra normal. A chamada sozinha não tem mais o ramo pendurado nem o vão entre o verbo e o detalhe.
  No painel médio, o resumo do grupo sai curto ("13 comandos · 1 editado · 3 lidos"), sem cortar o
  último número.
- **Escolhas iguais em todo lugar.** Nas Configurações e no boas-vindas, a opção escolhida de um
  seletor (como "7 dias") usa a mesma ficha de metal do esforço e do alto da lista.
- **Rolagem sem corte seco.** Rolada para cima, a conversa esmaece no pé em vez de cortar a última
  linha no meio, e o pé da lista ganhou um fio que o separa da última conversa.
- **Fileira de cima alinhada.** O logo e as abas da lista ficam na mesma linha das abas dos
  painéis (antes, uns 5 px abaixo).
- **O × da janela separado do painel.** No cabeçalho da última conversa, um traço separa o × da
  janela das ações do painel, como já era no painel da direita.
- **Textos das Configurações acertados.** A dos anéis de uso diz que eles ficam no canto de cima, a
  dos servidores MCP acompanha o interruptor, e as linhas de modo, agente e modelo dizem o que cada
  chat troca.
- **Ultracode desligado legível.** O motivo de ele não valer no chat aparece claro (só a chave fica
  apagada) e diz "Só no Claude Code", certo também com Claude e Codex juntos.
- **Corrigir uma mensagem não perde o que está na caixa.** Com algo escrito, o Vitral pergunta
  antes, e o Ctrl+Z traz o texto de volta. A mensagem corrigida fica guardada como rascunho na hora,
  mesmo se o Vitral cair logo depois, e a caixa mostra o fim dela, com o cursor. O aviso de depois
  cabe inteiro.
- **Menu "/" pelo teclado.** O Enter completa o comando marcado no menu (antes podia completar
  outro, até o /clear), as setas andam pelo menu, o Tab completa e o Esc só fecha o menu, sem
  apagar o que foi escrito.
- **Abas que cabem.** Sem lugar para o título, a aba de fundo vira um quadrado só com o ícone
  (antes sobravam duas reticências), e a que está à vista guarda sempre o começo do título: sem
  lugar nem assim, as de fundo saem da faixa (continuam no histórico do painel). Com muitas abas,
  a que está à vista não some mais por baixo do "⋯".
- **Trocar de chat pelo teclado.** Na busca (Ctrl+K), Enter abre o primeiro resultado e o Ctrl+K
  seguinte troca o termo em vez de somar. Depois de clicar num chat da lista, o que se digita vai
  para a caixa. O teclado não trava mais quando um painel aberto ao lado empurra a lista para fora.
- **Esc para o turno de qualquer lugar.** Depois de clicar na conversa (o foco sai da caixa), o Esc
  continua parando o turno do painel ativo, como a linha de estado mostra.
- **Suas quebras de linha ficam.** O balão da mensagem enviada mostra as linhas como você as
  escreveu (antes juntava tudo num parágrafo, e o "Mostrar tudo" aparecia sem nada escondido).
- **Fila mais fácil de mexer.** O × de tirar uma mensagem da fila funciona com o turno rodando e
  várias na fila (a barra de rolagem ficava por cima dele), os botões cabem no painel estreito, a
  lista mostra três mensagens inteiras e o "Entregar agora" não aparece duas vezes.
- **Buscar na conversa (Ctrl+F) mais confiável.** O achado dentro de uma trilha com raciocínio
  aparece (antes o contador andava e a tela não), não para mais na última linha debaixo da pílula
  de voltar ao fim, o campo não encolhe até uma letra no painel estreito e o × de fechar a busca
  não fica debaixo dos anéis de uso.
- **O lápis não foge do ponteiro.** A barra de ações da mensagem (hora, editar, copiar) continua
  à vista quando o ponteiro chega nela por cima.
- **O modo escolhido pelo teclado devolve o cursor à caixa**, como já faziam o mouse e os outros
  seletores.
- **Título do chat sem marcas de lista.** Um chat que começa com "- ponto 1" ganha o título "ponto 1".
- **Sinal da troca na caixa estreita.** Com os chips só em ícone, trocar o modo (Shift+Tab) ou o
  modelo mostra um aviso curto com o que ficou.
- **A dupla à vista na caixa.** Com Claude e Codex juntos, o chip do modelo mostra as duas marcas,
  também no painel estreito (antes parecia que a conversa tinha passado só para um deles).
- **Um menu aberto por vez.** Abrir o esforço pelo Ctrl+E com o sino aberto (ou qualquer par de
  menus e popovers) fecha o outro, em vez de empilhar os dois.

## 0.7.0

O metal líquido, abas de chat, o novo chat na hora e pedidos de permissão mais seguros.

- **Metal líquido.** O fundo do app virou um campo azul de metal líquido, com bolhas que derivam
  devagar e um grão fino; o papel de parede não aparece mais através da janela. A lista mora direto
  nesse fundo, e a conversa e os painéis da direita flutuam por cima, como cartões. Parado, o metal
  fica quieto: ele só anda enquanto um chat à vista está trabalhando.
- **Abas de chat.** Clicar num chat da lista abre uma aba no painel, e as anteriores ficam (até 8).
  O × ou o clique do meio fecha a aba; o Ctrl+W fecha a que está à vista e, na última, o painel.
  As abas se arrastam como no VS Code: soltas no meio de outro painel, viram aba dele; na borda de
  um painel, viram um painel novo daquele lado. Dois cliques na divisória entre duas conversas
  deixam todas com a mesma largura.
- **Novo chat na hora.** O "+" e o Ctrl+N abrem um chat novo sem perguntar nada, na pasta do chat
  aberto (com o Mestre aberto, no último lugar usado). Para escolher a pasta, **Novo chat em outra
  pasta…**, no menu do logo.
- **A lista mais limpa.** No alto ficam só o logo e as abas Conversas e Arquivos; a busca (Ctrl+K)
  e o "+" desceram para o pé da lista. A lista é uma só, uma linha por chat, com o Mestre na
  primeira; os projetos ficam escondidos e com o ícone neutro (**Mostrar os projetos no alto da
  lista**, nas Configurações, traz de volta). Arrastando a borda, a lista encolhe até 200 px, e o
  recolher (Ctrl+B) mora no menu do logo.
- **Caixa de mensagem com linha de estado.** Embaixo do campo, com o chat parado, aparecem as
  teclas úteis; com ele trabalhando, o que está fazendo, o relógio, o número de ações e o Esc para
  parar (a faixa "Rodando" saiu de cima da caixa). Com o foco, a caixa ganha um aro de metal, e o
  enviar e o parar são uma tecla de metal. O anel do contexto mostra a porcentagem ao lado.
- **Pedidos de permissão mais seguros.** O cartão diz o que vai acontecer ("Rodar um comando",
  "Editar views.py") e mostra o comando numa caixa com "$". O "Não perguntar de novo" virou uma
  caixa de marcar ao lado do Permitir. O comando perigoso fica em vermelho e só passa com **Segure
  para permitir** (ou Enter duas vezes). Pelo sino não se aprova mais comando perigoso nem
  comprido: o botão leva à conversa. Comando com mais de 11 linhas avisa para rolar a caixa antes.
- **Uso do plano no canto.** Os anéis de uso saíram do cabeçalho e flutuam no canto de baixo, à
  direita, da janela: um anel por serviço, com a janela mais apertada (5 h ou semana). O popover
  abre para cima, com a porcentagem dentro de cada anel. Dá para esconder nas Configurações.
- **Modelo novo sem esperar o Vitral.** As versões exatas do seletor de modelo vêm do próprio
  Claude Code instalado, que o Vitral consulta ao abrir, sem gastar uso: um modelo novo aparece
  quando o Claude Code se atualiza. Cada família diz qual versão ela é hoje ("hoje, Fable 5.1").
- **As conversas do VS Code aparecem sozinhas.** A cada abertura e a cada 2 minutos, o Vitral olha
  se há conversa nova do Claude Code (do terminal ou da extensão do VS Code) e a traz para a lista,
  sem precisar importar à mão. Quem já usava o Vitral recebe as conversas daqui para a frente; o
  histórico inteiro continua vindo pelo **Importar do Claude Code…**, no menu do logo.
- **Arquivos citados viram links.** O caminho de um arquivo na resposta abre o arquivo no painel da
  direita, na linha citada. A linha de uma ferramenta, aberta, mostra o que ela pediu (o diff do
  Edit, o que o Write gravou, o comando do Bash) e tem o botão de abrir o arquivo. O painel da
  direita começa fechado e abre pelo "…" da conversa ou pelo Ctrl+J.
- **Um chat abre outro.** Quando você pede ("abre um chat para isso"), o chat de projeto abre um
  chat novo já com o pedido, e você continua por lá.
- **Acabamento.** O painel selecionado tem uma luz de metal no pé; os cantos seguem um padrão só;
  o escolhido nos menus vai numa ficha azul; os menus ganharam o cabeçalho de metal e as teclas
  desenhadas; os anéis têm uma fenda embaixo. O logo novo (o V sobre o metal) vale em todo lugar,
  inclusive no ícone do app. O cabeçalho da conversa ficou com menos botões.

## 0.6.0

Um visual mais quieto, a versão exata do modelo e o Claude Code atualizado pelo próprio Vitral.

- **Visual novo.** O fundo desce em degraus de tom, as peças perderam o contorno, os vãos ficaram
  justos (4 px, canto 14) e o topo das peças ganhou um degradê escuro no lugar do claro.
- **O lado é uma peça só.** A coluna de ícones saiu: o logo, o Mestre e os projetos viraram a
  primeira fileira da lista, na horizontal, e rolam de lado quando não cabem (arrastar para
  reordenar continua valendo). O sino e o uso do Claude desceram para o rodapé da lista; com a
  lista recolhida, o uso do Claude é a primeira coisa da esquerda. As ações do cabeçalho da
  conversa ficam apagadas até o ponteiro chegar perto.
- **A versão exata do modelo.** O seletor de modelo ganhou, embaixo das famílias, as **versões
  exatas** (Fable 5.1, Opus 5.5, Opus 5, Sonnet 5.5, Haiku 5.5 e as anteriores), com a que está
  rodando marcada, inclusive quando a conversa segue a família: é como se vê qual Opus o "Opus"
  pega hoje. Num Claude Code anterior ao 2.1.280, que não aceita o 5.5, elas não aparecem e uma
  linha diz por quê.
- **Atualizar o Claude Code pelas Configurações.** O grupo **Claude Code** mostra a versão que os
  turnos abrem e tem o **Atualizar**, que roda `claude update` fora de qualquer turno e mostra o
  que ele respondeu (em âmbar, se falhar). Ele espera os chats que estão trabalhando ou com
  mensagem na fila; a versão nova vale no turno seguinte, sem reiniciar o Vitral.
- **Os agentes em segundo plano deixam de ser cortados no meio.** O Claude Code espera 10 minutos
  por eles e então os encerra, e o turno terminava com "Os agentes em segundo plano não terminaram"
  e "O último turno falhou" — um lote de uma hora morria sempre. O Vitral já pedia a espera sem
  teto nos turnos com Ultracode; agora pede em todos, porque um turno comum também deixa agente
  em segundo plano. Parar continua derrubando tudo quando você quiser.

## 0.5.0

As imagens no chat e as mensagens que você manda enquanto ele trabalha.

- **Mandar enquanto ele está trabalhando.** O Enviar com o turno rodando deixa de enfileirar: a
  mensagem entra na conversa e **no turno**, e o Claude muda de rumo sem perder o que já fez (a
  faixa do turno diz "dirigido"). Parar continua sendo o Esc. O que não dá para dirigir (Ultrathink,
  Ultracode, comandos de barra, texto de terceiros) continua pela fila, e a dica do botão diz por
  quê. **Mandar depois** (no "…" da caixa ou Ctrl+Enter) põe na fila de propósito.
- **A fila dá para mexer.** O popover mostra cada mensagem esperando, com o texto inteiro na dica,
  o lápis para editar, as setas para mudar a ordem e o × para tirar (o texto volta para a caixa).
  Ela sobrevive a fechar o Vitral e agora aparece também quando o chat está parado.
- **Imagens.** A miniatura entra na proporção certa (uma captura larga parava de virar um quadrado
  ilegível) e o clique abre um **visor**: a imagem inteira ou em tamanho real com rolagem, o nome,
  as medidas e o tamanho, **Abrir fora**, ←/→ pelas outras imagens da conversa e Esc para fechar.
  O que você anexa vira miniatura com o tamanho ainda na caixa; a imagem que o agente põe na
  resposta aparece; o anexo apagado do disco diz "sumiu"; e um `.png` do projeto é desenhado no
  painel da direita, no lugar de "arquivo binário".
- **README no GitHub**, com prints do app, como instalar no Linux e no Windows e como conferir a
  assinatura do que você baixou.

## 0.4.0

A conversa ficou mais fácil de ler, de achar e de corrigir.

- **Hora em cada mensagem** (no hover, ao lado do Copiar) e **separador de dia** entre os turnos:
  dá para saber quando cada coisa aconteceu numa conversa de dias. As conversas de antes desta
  versão não têm hora e não mostram nada.
- **Copiar em qualquer mensagem**, não só na última resposta de cada turno.
- **Buscar dentro da conversa** com Ctrl+F: contador "3 de 12", F3 e Shift+F3 para andar, realce em
  âmbar, sem ligar para acento nem maiúscula.
- **Corrigir a última mensagem**: a seta para cima na caixa vazia traz de volta o que você mandou,
  para ajustar e mandar de novo (a conversa volta a antes dela).
- **Lista de atalhos** com `?`, quando o foco não está num campo de texto. Ela é montada da mesma
  tabela que registra as teclas, então nunca desencontra do que o teclado faz.
- **Rascunho por chat**: o que você escreveu e não mandou fica guardado ao trocar de conversa.
- **Fila de mensagens** com o que está esperando e "Entregar agora"; tirar da fila devolve o texto
  à caixa.
- Na bancada, um `claude` de mentira que finge usar ferramentas, para ver a linha do turno
  ("Lendo motor.rs…", "Rodando: cargo test…") sem gastar a API.

## 0.3.0

O Vitral passa a ter um Mestre e projetos que vivem no app, no lugar da pasta de trabalho.

- **Mestre.** Uma conversa só, para todos os projetos (o círculo no alto do trilho, ou Ctrl+M). Ele
  entende o pedido, mostra como vai dividir ("Vou dividir assim", com **Começar**) e manda cada
  parte para o projeto certo. Quando os chats terminam, junta sozinho o que passou na verificação e
  no revisor e conta o resumo. Ele não edita nada. `#projeto` e `@chat` dizem para onde vai.
- **Projetos criados no app.** Nome, ícone, cor, descrição e uma ou mais pastas ligadas (ou uma
  pasta própria, com git, que o Vitral cria). O trilho rola e se reordena arrastando; a lista tem a
  **Visão geral**, cada projeto e os **Avulsos**, para os chats sem projeto.
- **Chats de projeto.** Cada chat conversa com você e trabalha numa cópia só dele da pasta (ou
  direto, numa pasta sem git). Acima da caixa, o que mudou, com **Revisar** e **Juntar**; a cópia
  continua depois de juntar.
- **Ajudantes.** Um chat divide um pedido grande em até 3 ajudantes, cada um numa cópia da pasta
  dele, e junta o trabalho deles antes de entregar.
- **Conversas do Claude Code.** As da extensão do VS Code e do terminal viram chats, no projeto da
  pasta delas ou em Avulsos, com o histórico. A primeira mensagem continua numa cópia da conversa: a
  original fica como está. **Importar do Claude Code…**, no menu do logo, traz as novas.
- **Uso do plano ao vivo.** Os anéis de 5 h e da semana se atualizam sozinhos (a cada minuto com o
  popover aberto, a cada 5 min fechado), buscando direto na Anthropic com o login que o Claude Code
  já guarda; o Vitral nunca renova esse login.
- **Modo foco no ⋯** de cada painel; o botão dele só aparece com ele ligado.
- **Windows (prévia).** Instalador por usuário (sem administrador), zip portátil e o mesmo visual,
  com os botões e o acrílico do Windows, notificações, terminal, Lixeira e Git Bash. As versões
  novas chegam sozinhas, como no Linux (quem instalou a 0.3.0-rc.6 instala esta uma vez pelo
  instalador). Ainda sem contas extras.
- **Issues e PRs ao lado do chat.** Configurações › Conexões mostra o login do GitHub (pelo `gh`) e
  conecta o Linear (a chave fica no chaveiro do sistema, nunca em arquivo).
  - **Issues.** O botão do Linear e o do GitHub no cabeçalho do chat abrem a lista (Minhas, do time,
    do ciclo) com o detalhe. **Iniciar agente** cria o chat da issue, com o ramo dela, e no Linear
    a move para Em andamento com um comentário; **Só a cópia** prepara sem mexer na issue; até 3 de
    uma vez. Uma issue também se anexa a uma conversa, e o Mestre consulta e divide o trabalho por
    issue.
  - **Entregar por PR.** Por pasta, "Ao aprovar: Juntar aqui | Abrir PR". No modo PR, **Revisar e
    abrir PR** mostra o diff com o formulário do PR (título, descrição com o `Closes` da issue,
    rascunho), e o Vitral faz o commit, confere segredos e `.github/`, envia o ramo e abre ou
    atualiza o PR. O chat também abre quando você pede na conversa ("abre o PR e mescla quando
    passar").
  - **Watch e Ship.** O PR aparece numa linha de acompanhamento no chat e na aba **PR** à direita
    (visão geral, arquivos, checks com o log, Mesclar). O CI que falhou e a revisão chegam em
    Esperando você, com **Mandar ao chat**; com **Mesclar quando ficar pronto**, vão sozinhos ao
    chat, a correção atualiza o PR e o Vitral mescla quando os checks passam. Depois de mesclar, a
    cópia recomeça da base, a pasta original anda e a issue vai para Concluída.
  - **Confirmar.** Mesclar agora, mexer em `.github/` e o que não veio de uma mensagem sua viram um
    cartão para você decidir; "Tentar de novo" não repete no GitHub o que o turno já fez.
  - **Chats isolados.** Os chats que leem issues, comentários e logs de CI, e os de uma pasta que
    entrega por PR, rodam no `bwrap`: sem o chaveiro, o ssh, o login do `gh` nem as credenciais do
    git, e com o `.git` do repositório só leitura; quem fala com o GitHub é o Vitral.
- **Parar** pede ao Claude para interromper (ele fecha o turno direito) e só derruba o processo se
  ele não sair em 5 s. As fontes (Inter e JetBrains Mono) vêm dentro do Vitral.
- **Um Vitral só.** Abrir de novo (pelo menu, `vitral <pasta>` ou `--nova`) entrega o pedido ao que
  já está aberto.
- **Na primeira abertura**, o Vitral traz sozinho o que havia nas pastas do Vitral antigo: cada
  subpasta vira um projeto, e as conversas vão junto (as que já tinham conversado ficam só para
  leitura, com **Continuar num chat novo**). Nada nas pastas antigas é mudado nem apagado; os dados
  novos ficam em `~/.local/share/vitral`. Depois disso, a volta para a 0.2.0 fica desligada: ela não
  lê as conversas do formato novo.

## 0.2.0

A primeira versão publicada, para Linux.

- **Conversas que coordenam.** Cada conversa planeja, divide o pedido em tarefas e abre um
  trabalhador para cada uma, numa cópia isolada do projeto, com portas próprias. O projeto só
  muda quando a tarefa é aprovada.
- **Verificação em três camadas.** Os testes rodam quando o trabalhador para, o Vitral confere de
  novo e um revisor independente aponta os problemas. Aprovar faz o commit e junta; no **Revisar**
  você vê o diff, comenta e pede ajuste.
- **Nada se perde numa queda.** Se o Vitral fecha ou o PC desliga, a tarefa fica parada, com
  **Retomar**, e as conversas voltam como estavam.
- **Tudo à vista.** A lista separa **Esperando você** e **Rodando agora**, o sino junta o que
  precisa de você (Ctrl+G vai à próxima pendência), a **Equipe** mostra as tarefas da conversa e a
  notificação do GNOME abre a conversa certa.
- **Opções da conversa** como na extensão do Claude Code: modo de permissão, modelo, esforço,
  Ultrathink e Ultracode, anexos e o uso do contexto.
- **Contas e planos.** Várias contas do Claude e os anéis de uso das 5 h e da semana, também do
  Codex.
- **Projetos.** Criar, arquivar (com Desfazer) e mandar para a Lixeira do GNOME; ícone e cor de
  cada um; **Rotinas** que mandam um pedido num horário.
- **Visual de vidro** azul-marinho, zoom com Ctrl+= e Ctrl+-, e animações que seguem as do GNOME
  (ou desligadas nas Configurações).
- **Se atualiza sozinho.** Instalado pelo `instalar.sh` do release, o Vitral procura versão nova no
  GitHub, confere a assinatura, baixa e instala em segundo plano, e avisa "Versão 0.3.0 pronta ·
  Reiniciar". Nunca reinicia sozinho; com conversa trabalhando, pergunta antes. Em
  **Configurações › Atualizações** dá para escolher Automáticas, Só avisar ou Desligadas, procurar
  na hora e ver as novidades.
- **Volta sozinho se a versão nova não abrir.** Se ela cair duas vezes ao abrir, a terceira
  tentativa abre a anterior, com o aviso. Uma queda de energia no meio não conta contra a versão.
  Dá para voltar à mão nas Configurações.
