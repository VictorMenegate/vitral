# Novidades do Vitral

Cada versão tem a sua seção (`## 0.2.0`); o texto dela vira as notas do Release no GitHub e o que
o app mostra em **Novidades**. Os pré-lançamentos (`0.2.0-rc.1`) usam a seção da versão.

## 0.11.1

Mais fluido, com os menus chapados e uma rodada de ajustes do jeito novo.

- **Mais fluido.** As animações acompanham a tela, a roda do mouse rola suave como no navegador,
  e com um chat trabalhando a janela não engasga mais: o fundo animado ficou bem mais leve. Os
  giros continuam andando quando você está em outra janela.
- **Menus chapados.** Sem degradê nem borda clara, com os grupos separados só pelo espaço.
- **Lista fixa.** Ela não recolhe nem fica mais estreita que a do desenho; quem a esconde é o
  modo foco. Só a conversa escolhida tem fundo.
- **Abas no painel estreito.** As outras abas continuam na faixa, encolhendo.
- **Nesta conversa.** Uma busca pequena em cima da lista, que rola sem levar o painel junto.
- **Detalhes.** O aro do foco por dentro da moldura da caixa, o anel do contexto igual ao da
  barra de baixo, barras de rolagem discretas e o uso do modo foco que arrasta sem abrir.

## 0.11.0

O Vitral ganhou cara nova, chapada e mais limpa, no jeito do Linear. As cores do tema agora são
suas, o .md abre como uma página que se edita no lugar, e a primeira abertura ficou num cartão só.

- **Visual novo.** A lista com a busca e o "+" no alto e as seções com título; a barra de baixo
  com o uso de cada agente, o contexto, a Revisão, as Arquivadas e os Avisos; abas, caixa de
  mensagem, menus e Configurações redesenhados, com textos mais curtos. As larguras padrão da
  lista e do painel da direita ficaram menores.
- **Modo foco (Ctrl+.).** Só as conversas na tela, com o uso flutuando num canto, arrastável. O
  clique duplo na aba maximiza o painel.
- **Cores do tema.** Em Configurações › Aparência › Cores: temas prontos (Grafite, Noite,
  Violeta, Floresta, Brasa e Oceano), destaque, fundo e contraste, cores próprias da lista,
  bordas finas e Importar/Copiar o tema. O metal e o logo seguem a cor do destaque.
- **Fundo animado.** Smoke, Mesh drift ou desligado, nas cores do tema ou nas da receita. Anda
  quando um chat está trabalhando.
- **Página do .md.** O .md abre como página de blocos: um clique edita o bloco, "/" escolhe o
  tipo, "# " e "- " viram título e lista, a alça põe um bloco embaixo e arrasta, e a seleção tem
  negrito, itálico, código, Mencionar e Copiar. Grava sozinho, e o que não foi mexido volta ao
  arquivo como estava.
- **Painel da direita.** Este chat em grupos, a aba Arquivos com a árvore da pasta e as marcas
  do git, Mudanças | Arquivo com as linhas iguais dobradas e vários arquivos abertos em abas.
- **Menção de um trecho.** Selecione um pedaço do código e mencione só ele na conversa.
- **Revisão com mais opções.** Quando revisar, o mínimo de linhas, pular em Planejar, o foco
  do revisor, as suas instruções e mandar o achado ao agente sozinho.
- **Primeira abertura nova**, num cartão com os passos no alto.
- **Saíram** os Perfis de agente e o "Enquanto você esteve fora". As Contas moram agora em
  Configurações › Claude Code.
- **Correções.** A revisão cortada ao fechar o Vitral não volta pela metade; o texto do editor
  de arquivo é gravado inteiro ao fechar; a barra de ações das mensagens deixou de ficar por
  baixo do texto de baixo.

## 0.10.3

O Codex e o Gemini ganharam nas Configurações o mesmo cuidado do Claude Code: dá para instalar,
atualizar, entrar e sair da conta e ver os servidores MCP de cada um. As pílulas do turno voltaram
para baixo do campo de mensagem.

- **Codex e Gemini nas Configurações.** Duas partes novas, depois do Claude Code, no mesmo molde:
  a versão instalada com **Atualizar** (ou **Instalar…** se o programa não está no computador), a
  conta com **Entrar…** (e **Sair** no Codex, com pergunta antes), a lista dos servidores MCP e o
  arquivo de configuração com **Abrir**. O Atualizar roda fora dos turnos, espera os chats do
  agente terminarem e mostra o que o comando respondeu. O Codex que ficava em Agentes mudou para a
  parte dele.
- **Os comandos no fundo, os subagentes, os passos e as mudanças voltaram para dentro da caixa.**
  As pílulas ficam de novo embaixo do campo de mensagem, na linha de estado, e não há mais linha
  solta em cima da caixa. Continuam com o clique que lista e para cada comando, a lista dos
  subagentes que abre a conversa de cada um e as mudanças que abrem "Este chat".

## 0.10.2

O Codex ficou quase tão completo quanto o Claude no Vitral: pede permissão no meio do turno pelo
mesmo cartão, tem o `/compact`, o `/review` e as skills no menu "/", mostra o contexto, os tokens
e os passos, e corrige uma mensagem ou continua daqui em outro chat sem resumo. O diff ganhou
comentário por linha, o menu "/" mostra tudo, e há atalhos novos para andar entre as mensagens,
maximizar o painel, mover abas e arquivar.

- **Atalhos para andar e organizar.** Alt+↑ e Alt+↓ pulam para a sua mensagem anterior e a seguinte;
  PgUp e PgDn rolam a conversa (na caixa com texto continuam sendo do texto). Ctrl+Shift+Enter
  maximiza o painel e volta (o Esc também volta), Alt+PgUp e Alt+PgDn levam a aba ao painel vizinho,
  Alt+Shift+PgUp e Alt+Shift+PgDn abrem um painel ao lado com ela, e Ctrl+Alt+A arquiva o chat do
  painel, com Ctrl+Z desfazendo enquanto o aviso estiver na tela. Todos estão em Atalhos do teclado
  e na paleta.
- **Marcar como não lido.** O menu do chat devolve o ponto azul, e ele fica até você abrir o chat de
  novo.
- **Busca na conversa sem ficar embaixo dos anéis.** Com o painel encostado na direita da janela, a
  barra do Ctrl+F deixa o lugar dos anéis de uso, seja com um, dois, três ou nenhum.
- **Comentário por linha no diff.** Na edição do agente, clique na margem de uma linha e escreva o
  comentário. Eles se acumulam, ficam marcados nas linhas, e o **Enviar comentários (N)** manda
  todos de uma vez ao agente, com arquivo e linha de cada um.
- **Colar HTML vira Markdown.** O que você copia de um navegador ou documento entra na caixa com
  links, listas, negrito e código; o Ctrl+Z volta ao texto puro. No Linux com Wayland.
- **O menu “/” mostra tudo.** Todos os comandos e skills numa lista que rola, cada um com a
  descrição, a dica de argumento, o selo skill e a origem; o ícone (ou o Ctrl+O) abre o arquivo da
  skill à direita, e os que mais combinam com o que você digitou vêm primeiro.
- **“@” e “/” onde está o cursor.** O menu abre para a palavra em que você está, e não só para a
  última, e completa ali. Caminho com espaço vai entre aspas.
- **Caixa estreita com o nome do modelo.** Com três painéis lado a lado, o chip mostra “Opus” ou
  “Fable”, e o modo e o esforço ficam só com o ícone.
- **Imagens coladas sem uso saem sozinhas.** Ao abrir, o Vitral apaga as imagens coladas que nenhuma
  conversa, fila ou rascunho usa e que estão paradas há mais de um dia.
- **O Codex pede permissão.** Nos modos Manual, Aceitar edições, Planejar e Automático, o Codex
  pergunta no meio do turno pelo mesmo cartão do Claude, com o comando inteiro ou o diff do arquivo,
  Negar e Permitir. Antes esses modos viravam só o sandbox dele. O "Permitir sempre" vale para o
  Codex também, e o Aceitar edições deixa passar as edições dentro da pasta.
- **Comandos de barra no Codex.** O `/compact` resume a conversa do Codex (também pelo Compactar), o
  `/review` pede a revisão das mudanças (ou do que vier escrito depois dele) e as skills do Codex na
  pasta aparecem no menu "/".
- **Contexto e tokens do Codex.** O medidor do contexto aparece também no chat do Codex, com a
  janela do modelo, e a linha "Trabalhou" diz quantos tokens o turno usou. O Codex não diz o custo
  em dólar, então o popover mostra só o contexto e os tokens.
- **Passos do Codex.** O plano que o Codex mantém aparece como os passos do Claude: a pílula "1/3"
  acima da caixa e as linhas na trilha.
- **Corrigir e continuar daqui no Codex, sem resumo.** Corrigir uma mensagem ou continuar a conversa
  em outro chat leva o Codex exatamente ao ponto escolhido, como já fazia com o Claude, em vez de
  começar uma conversa nova com um resumo das últimas mensagens.

## 0.10.1

O chat que outro chat chama aparece numa aba, e fechar a aba de um chat que trabalha o para. O
mapa dos agentes ficou parecido com um chat, o que aparece na tela entra com uma animação curta,
o outro agente revisa o turno grande e os menus de modelo, do logo e do "…" da caixa ficaram mais
curtos. As Configurações ganharam opções: a seta ↑ da caixa, o Enter com o turno rodando, os
números das abas, o que a aba Arquivos lembra, o título pelo Claude Code, os anéis de uso, a
consulta de versão nova e a letra da conversa na caixa. Essas começam no jeito de antes; só a aba
do chat chamado e o fechar que para já vêm ligados. E o Codex e o Gemini aparecem também no
Windows.

- **O Codex e o Gemini no Windows.** O Vitral só procurava o `codex.cmd` do npm, e o Codex do
  instalador oficial (`codex.exe`) nunca aparecia; o Gemini só aparecia com a pasta dele já no
  PATH do app aberto. Agora ele procura os dois nomes do Codex e as pastas dos instaladores
  oficiais, e o **Instalar o Codex…** e o **Instalar o Gemini…** também existem no Windows, rodando
  os instaladores oficiais em PowerShell e o login de cada um.
- **O chat chamado por outro aparece numa aba.** Quando um chat manda mensagem a outro, ou abre
  um, o destino entra como aba de fundo no painel ativo: o que você lê não troca, a janela não vem
  para a frente e o contorno da aba mostra que está rodando. Já à vista ou já aba, nada muda. Em
  Configurações, Chats, Abas.
- **Fechar a aba para o chat.** Fechar (× ou Ctrl+W) a aba de um chat que está trabalhando para o
  turno dele, como o Parar, a menos que ele continue à vista em outro painel. Com agentes em
  segundo plano, o Vitral pergunta antes.
- **Seta ↑ percorre as enviadas.** Com a opção, a seta na caixa vazia traz as mensagens que você
  já enviou neste chat, uma por vez; ↓ volta; editar devolve as setas ao cursor. O corrigir fica
  no lápis da mensagem e no Ctrl+↑.
- **Enter vai para a fila.** Com o turno rodando, o Enter espera a vez e o Ctrl+Enter entra no
  turno.
- **Números só segurando Ctrl.** Os números das abas e da lista aparecem depois de uns 150 ms
  com o Ctrl apertado e somem ao soltar.
- **A aba Arquivos lembra.** “Mostrar ignorados” e “Ignorar espaço em branco” voltam como
  ficaram, se a opção estiver ligada.
- **Título pelo Claude Code, anéis de uso, versão nova e letra na caixa.** Dá para desligar o
  título gerado, escolher quais anéis de uso aparecem (Claude, Codex, Gemini), deixar a consulta
  de versão nova dos agentes só no botão e pôr a letra da conversa também na caixa de mensagem.
- **O mapa dos agentes ficou mais parecido com um chat.** Quem está trabalhando fica em cima, depois
  quem espera você, quem falhou e os terminados. O lado direito mostra a conversa do agente como a de
  um chat: o pedido num balão, as respostas em Markdown, as chamadas agrupadas e recolhíveis, as
  imagens em miniatura e o relatório, com o botão **Abrir em aba**. E o agente que a conversa deu por
  interrompido, mas que seguiu trabalhando depois de um turno retomado, volta a aparecer como
  trabalhando (ou como terminou).
- **Menos coisa aparecendo a seco.** Trocar de aba faz a conversa entrar com um fade curto. Os
  pedidos de permissão, as perguntas e o plano, a fila, cada pílula nova, a faixa quando muda de
  estado, a chamada que chega numa trilha, o que você abre na conversa (com o chevron girando), a
  parte das Configurações, a troca Conversas/Arquivos, a direita do Ctrl+J, os avisos do canto e a
  mensagem presa entram com fade e um deslize de poucos pixels. Tudo em 160 ms, só quando
  acontece; sair e recolher continuam na hora, e com Animações desligadas tudo aparece pronto.
- **O outro agente revisa o turno.** Quando um turno muda 20 linhas ou mais, o Codex (num chat do
  Claude) ou o Claude (nos outros) confere o que mudou, só lendo. Sem nada a dizer, fica uma linha
  discreta; com um achado, um cartão com **Mandar ao Claude**. A linha “Trabalhou…” do último
  turno ganha **Revisar com o Codex** para pedir quando quiser. A revisão gasta o plano do outro
  agente: dá para desligar em Configurações, Agentes. A chave “Os dois em toda mensagem” saiu.
- **Seletor de modelo mais curto.** As versões exatas ficam atrás de **Outras versões** e a conta
  vira uma linha no pé, “Conta: tecnologia · semana 44%”, que abre a escolha no lugar. Escrevendo
  na busca, as versões e as contas que casam aparecem direto. O aviso de reler só aparece com a
  conversa acima de 100 mil tokens, numa linha curta.
- **Menu do logo mais curto.** Três grupos (os chats, as vistas, a conta e os ajustes), a versão
  no alto e, quando há versão nova pronta, a linha **Versão nova pronta · Reiniciar** logo abaixo.
  Servidores MCP, Memória da equipe, a pasta dos chats novos e a pasta de dados saíram do menu: estão
  nas Configurações e na paleta. O menu cabe sem rolar a 150%, e a versão pronta não fica mais para
  trás quando outra janela do Vitral instala uma mais nova.
- **O "…" da caixa em três grupos.** Anexar, mandar e a conversa, com duas divisórias no máximo.
  O que não serve agora sai do menu em vez de ficar cinza: os de mandar só aparecem com texto na
  caixa, o compactar só com conversa e a sessão nova só com o chat parado. “Arquivo ou imagem…”
  não sai mais cortado.
- **Ultracode em uma linha.** Embaixo do nome, só “Agentes em paralelo, só na próxima · gasta bem
  mais”; o resto está no (i).

## 0.10.0

O turno que está rodando ouve o que você muda nele, o pedido de permissão se responde escrevendo,
o de um hook seu aparece em todos os modos, a pergunta com opções tem o cartão dela, o plano
continua na conversa, a compactação deixa um marco e as imagens que as ferramentas devolvem
aparecem na conversa. O chat ganha um título de verdade, o aviso de fim sai legível e os arquivos
citados abrem em mais lugares. Achar e abrir arquivo ficou mais direto: Ctrl+P, Ctrl+F dentro do
arquivo, os ignorados na aba Arquivos, PDF e planilha abrindo fora e o VS Code na linha certa. E
o arquivo aberto à direita agora se edita ali mesmo.

A conversa mais leve (rolar, digitar e acompanhar a resposta chegando pesam menos), quanto tempo e
quanto cache cada turno gastou, o que cada agente em segundo plano está fazendo e a conversa do
subagente numa aba. Nas Configurações, a letra da conversa separada do zoom, as suas instruções
para todos os chats e os perfis de agente. E cada conversa abre um processo do Claude só.

- **O limite de um modelo só não para a conta.** Com a "Semana · Fable" em 100% e a semana geral
  em 66%, a recusa do Fable marcava a conta inteira como no limite até a semana renovar. Agora só
  o limite de 5 h e o semanal geral param a conta; o de um modelo fica no erro do turno dele.
- **O uso do Gemini no canto.** Com o Antigravity CLI entrando, um terceiro anel mostra a cota da
  semana dos modelos do Gemini (fora) e a do grupo Claude e GPT (dentro); o clique abre as duas,
  com quando renovam. A leitura é a do `agy -p /usage`, de 5 em 5 minutos, sem gastar cota.
- **A mensagem presa no alto com o fundo transparente.** A conversa não é mais desenhada embaixo
  da faixa da mensagem presa: ela não precisa de fundo próprio, fica como o resto do painel, e o
  texto que rola não aparece mais por baixo dela.
- **O esforço não some da caixa.** Quando não cabia inteiro ao lado do modelo, o esforço ia para
  uma segunda linha escondida e sumia; agora fica só com o ícone até ter lugar de novo.
- **Modo, modelo e esforço valem no turno que está rodando.** Trocar o modo (Shift+Tab), o modelo
  ou o esforço no meio de um turno só valia no turno seguinte. Agora a troca chega ao turno em
  andamento em menos de um segundo.
- **Responder a um pedido de permissão escrevendo.** Com um pedido aberto, o Enter na caixa nega
  o pedido e manda ao Claude o que você escreveu, em vez de deixar a mensagem na fila. O
  Ctrl+Enter continua mandando para a fila.
- **Parar com mensagem esperando não derruba mais o Claude Code.** Parar um turno logo depois de
  mandar uma mensagem para dentro dele encerrava o processo à força, antes de ele fechar o turno
  e gravar a conversa. Agora o turno fecha direito e a mensagem volta para a fila.
- **"Sem sinal" só quando o turno travou.** Um comando demorado (um build, uma suíte de testes)
  ganhava o aviso de sem sinal depois de 10 minutos. Agora o aviso só aparece se o Claude Code
  parar mesmo de responder.
- **O raciocínio aparece com o texto.** O "Pensou por…" da conversa vinha vazio; agora abre com o
  resumo do que o modelo pensou (Claude Code 2.1.323 ou mais novo).
- **O que um hook manda perguntar vira o cartão de permissão, até em Sem perguntar.** Quando um
  hook seu do Claude Code pedia confirmação antes de um comando (a trava de um deploy, por
  exemplo), o cartão não aparecia no modo Sem perguntar: o comando voltava recusado e o agente
  insistia. Agora o cartão aparece em qualquer modo, com o motivo que o hook deu, e esse pedido
  não tem "Não perguntar de novo".
- **Pergunta com opções.** Quando o Claude pergunta algo com alternativas, a pergunta aparece
  num cartão com as opções para clicar (uma ou várias), a descrição de cada uma, "Outra…" para
  escrever a sua e Enviar. Antes ela vinha como um pedido de permissão comum, e o Permitir
  devolvia a pergunta sem resposta.
- **O plano continua na conversa.** Depois de aprovado ou recusado, o plano fica no ponto em que
  foi proposto, num cartão recolhido ("Plano aprovado" ou "Plano recusado") que abre o texto
  dele. Antes ele sumia da conversa assim que você respondia.
- **Marco de compactação.** Enquanto o Claude Code resume a conversa, a linha do turno diz
  "Compactando…". Quando termina, a conversa ganha o marco "Contexto compactado", com o tamanho
  de antes e se foi automático ou pelo Compactar.
- **As imagens que as ferramentas devolvem aparecem na conversa.** O print do navegador (por
  MCP), a imagem gerada e a imagem que o agente foi olhar vinham só como o texto "(image/png)".
  Agora a chamada mostra a miniatura, que abre no visor junto com as outras imagens da conversa;
  no Codex, o view_image e os prints por MCP também. As imagens ficam guardadas no computador,
  até 100 MB por conversa (as mais antigas saem primeiro).
- **O chat ganha um título de verdade.** O título do chat era o começo da sua primeira mensagem
  ("olha esse erro aqui que deu no"). Depois do primeiro turno ele passa a ser um nome curto do
  assunto, gerado pelo Claude Code. O que você renomeou fica como está, e no Codex nada muda.
- **Aviso de fim legível.** A notificação de um chat que terminou mostrava a primeira linha crua
  da resposta ("Terminou: ## Resumo"). Agora mostra o começo do texto, sem as marcas de markdown,
  com até 220 letras. O "Enquanto você esteve fora" usa o mesmo texto.
- **Mais arquivos citados viram link.** `arquivo#L10-L20` abre com as linhas marcadas,
  `arquivo:12:5` e `file:///caminho` também abrem, e o nome citado sem a pasta (`mensagens.rs:120`)
  abre o arquivo da pasta do chat que termina assim, quando é um só.
- **Links na saída de comando.** `src/x.rs:120:5` no erro do cargo, `src/x.ts(12,5)` no do tsc e
  `"x.py", line 12` no do Python abrem o arquivo na linha. O ponteiro parado no link mostra o
  caminho inteiro.
- **Comando de barra citado vira link.** Quando o agente escreve `/compact` ou outro comando do
  chat, o clique põe o comando na caixa de mensagem. Quem envia é você.
- **Abrir arquivo pelo nome (Ctrl+P).** O Ctrl+P abre a lista dos arquivos da pasta do chat, com
  busca; o Enter abre o arquivo à direita. A busca não liga para acento ("relatorio" acha
  "relatório.md"), e isso vale também para o "@" da caixa.
- **A paleta acha conversas e arquivos.** Na paleta de comandos (Ctrl+Shift+P), o que você
  escreve acha também as conversas pelo título e os arquivos da pasta do chat, além dos comandos.
- **Buscar dentro do arquivo (Ctrl+F).** Com um clique no arquivo aberto à direita, o Ctrl+F
  busca dentro dele, com "3 de 12", as setas e o Enter para o próximo. Também está no "…" do
  arquivo. O arquivo em markdown aparece em linhas enquanto a busca está aberta.
- **Mostrar ignorados na aba Arquivos.** O olho ao lado do ↻ mostra também o que o git ignora
  (as pastas arquivo/, dados/ e tarefas/, com prints e exports), que antes nunca apareciam. As
  pastas ignoradas só são lidas quando você as abre. Começa desligado.
- **VS Code na linha certa.** O botão VS Code do arquivo aberto à direita abre o arquivo na linha
  que você está vendo: a que escolheu, a da mudança ou a que o link da conversa marcou.
- **PDF, planilha e outros abrem fora.** O arquivo que o Vitral não mostra (PDF, planilha, zip)
  ganhou o Abrir fora, que abre no aplicativo do sistema. Antes só havia Abrir no VS Code, que
  não serve para eles. O "…" do arquivo também tem o Abrir fora, para ver um .html no navegador.
- **SVG aparece como imagem.** O arquivo .svg abre desenhado à direita e no visor, e não mais
  como código.
- **Editar o arquivo no painel.** Um clique no texto do arquivo aberto à direita abre o editor
  ali, com a cor da sintaxe. O que você escreve se salva sozinho quando você para de digitar, e o
  Ctrl+S salva na hora. O Esc volta à leitura. O número da linha continua escolhendo linhas para
  mencionar.
- **Aviso quando o agente muda o arquivo que você está editando.** Se o arquivo muda no disco com
  texto seu ainda não salvo, uma faixa pergunta: Recarregar (fica o do disco) ou Manter o meu
  (o seu passa por cima ao salvar). Sem texto seu por salvar, o editor só passa a mostrar o novo.
- **Arquivo grande abre para ler.** Arquivos de 400 KB a 5 MB (um log, um export) abriam só no
  VS Code. Agora abrem aqui, só para ler, com a busca e a escolha de linhas.
- **Markdown: Prévia ou Fonte.** O .md aberto à direita tem um seletor Prévia | Fonte. Na prévia,
  o cabeçalho do arquivo (o nome e a descrição de uma skill, por exemplo) aparece numa tabelinha,
  e os links para outros arquivos abrem o arquivo no painel. Na Fonte, um clique edita.
- **Rever as mudanças do turno em sequência.** Na edição aberta à direita, a seta da última
  mudança passa ao próximo arquivo da lista de Mudanças do chat, e a faixa mostra "arquivo 2 de
  8". Antes as setas davam a volta dentro do mesmo arquivo.
- **Diff sem espaço em branco.** No diff de "Na pasta", o "…" do arquivo tem Ignorar espaço em
  branco: o que só mudou de recuo sai do diff. Começa desligado.
- **Rolar a conversa pesa menos.** O Vitral desenha só as mensagens que estão à vista (e um pouco
  em volta); as outras entram como espaço. Antes, cada passo da roda refazia as 60 últimas
  mensagens do painel, e mais ainda depois de subir bastante na conversa.
- **A resposta chegando não mexe nos outros painéis.** O texto que vai chegando redesenha só a
  conversa dele: a lista e as conversas ao lado ficam paradas. Antes, cada pedaço de texto refazia
  a janela inteira.
- **Quanto tempo o turno levou, e onde.** A dica da linha "Trabalhou…" diz também o modelo,
  quantos passos o turno teve, o tempo total, o tempo de modelo, o tempo até a primeira palavra,
  quanto levou para abrir o Claude Code e quanto da entrada veio do cache.
- **A chamada demorada diz quanto levou.** O comando que levou 10 s ou mais fica com a duração na
  linha dele, "Rodou cargo test · 2 min 14 s". O que foi para o segundo plano não ganha número: ele
  segue rodando.
- **O cache esfriando.** O popover do contexto ganhou a linha **Cache**, que diz por quanto tempo
  o cache do último turno ainda vale: "quente por mais 3 min" ou "frio". Com ele frio, a próxima
  mensagem relê a conversa inteira pelo preço cheio.
- **Digitar e passar o ponteiro não refazem as conversas.** Cada tecla na caixa de mensagem e
  cada passada do ponteiro por um botão refaziam as conversas de todos os painéis; com três
  conversas longas abertas, a digitação atrasava.
- **O que o chat está fazendo, numa linha acima da caixa.** Quando há o que mostrar, uma linha de
  pílulas aparece logo acima da caixa de mensagem: os comandos que o agente deixou rodando no
  fundo (o clique lista cada um, com Parar), os subagentes por estado ("2 trabalhando · 1
  esperando você"), os passos dele ("3/5") e o que o turno mudou ("+40 −12", que abre a lista à
  direita). Sem nada disso, a linha não existe, e ela não empurra a caixa. O "N arquivos mudados",
  que ficava numa faixa acima da caixa, virou a pílula das mudanças.
- **A conversa inteira do subagente, numa aba.** Clicar no cartão de um subagente (ou nele, na
  pílula dos subagentes) abre numa aba tudo o que ele leu, rodou e respondeu, com o desenho da
  conversa normal, só para ler. Ela acompanha o subagente enquanto ele trabalha e continua
  disponível depois de fechar e abrir o Vitral. O cartão fica na conversa, e a setinha dele
  mostra os passos e o relatório ali mesmo.
- **O que cada agente em segundo plano está fazendo.** Quando a resposta já veio e os agentes
  seguem trabalhando, a conversa mostra uma linha por agente, com a descrição, o que ele faz agora
  ("Rodando cargo test…", "Lendo views.py…") e há quanto tempo foi lançado; o clique abre a
  conversa dele numa aba. Antes havia só a frase "Agentes trabalhando em segundo plano…". A pílula
  "N trabalhando" passou a contar esses agentes (ela zerava assim que a resposta chegava), e o
  cartão deles não aparece mais como "interrompida".
- **"Terminou: …" quando um agente em segundo plano acaba.** No ponto da conversa em que ele
  termina entra uma linha, "Terminou: Mapear o carrinho · 25 min" (ou "Falhou: …"), e o clique
  nela abre o relatório dele. Com vários agentes, o turno agora termina uma vez só: antes o
  "Terminou" do chat podia sair uma vez por agente.
- **O diff da edição mostra só o que mudou.** Ao abrir uma edição na conversa, aparecem as linhas
  que mudaram, com 3 linhas em volta, e a palavra trocada vem marcada dentro da linha. Antes o
  trecho antigo vinha inteiro em vermelho e o novo inteiro em verde. O pedido de permissão de uma
  edição mostra o mesmo diff.
- **Os passos dizem o que mudou.** Quando o agente mexe na lista de passos dele, a linha na
  conversa diz o quê: "Criou 5 passos", "Começou: rodar os testes", "Concluiu: rodar os testes".
  Antes toda chamada era "Atualizou os passos". O Vitral lê também as ferramentas de tarefa novas
  do Claude Code (TaskCreate, TaskUpdate e TaskList), e a pílula "3/5" conta por elas.
- **As checagens do turno na linha "Trabalhou…".** O turno pronto diz como terminaram os testes,
  o typecheck, o lint e o build que ele rodou: "tsc passou · vitest falhou 2×, passou · cargo test
  passou". Vale a última execução de cada um. Quando não dá para saber o resultado (o comando
  atrás de um `| tail`, sem nada na saída que diga), a checagem não aparece.
- **Copiar tabela.** Cada tabela de uma resposta ganhou um Copiar no canto de baixo: o clique
  copia a tabela em Markdown, e com Shift copia pronta para colar numa planilha.
- **O relatório do subagente aparece no cartão.** No modo automático, o cartão do subagente
  mostrava um aviso em inglês ("This agent's report was delivered…") no lugar do relatório.
- **Letra da conversa.** Em Configurações > Aparência, o tamanho das mensagens muda sem mexer no
  resto: de 80% a 150% da interface. O zoom (Ctrl+roda) continua valendo para tudo e agora também
  aparece ali.
- **Suas instruções para todos os chats.** Em Configurações > Agentes, um texto seu que vai junto
  do que o Vitral já diz ao Claude e ao Codex ("rode os testes antes de dizer que terminou"). Vale
  a partir da próxima mensagem.
- **Perfis de agente.** Um nome para quem responde, o modelo, o modo e o esforço, com uma nota de
  quando usar. Ficam na parte nova **Perfis** das Configurações, aparecem no alto do seletor de
  modelo, e escolher um ajusta tudo de uma vez.
- **Versão do Claude Code e do Codex.** Nas Configurações, a versão instalada de cada um (a do
  Codex, em Agentes); quando sai uma mais nova, a linha avisa.
- **Testar a notificação.** Em Configurações > Chats, um botão manda uma notificação do sistema na
  hora.
- **Diagnóstico para pedir ajuda.** Em Configurações > Atualizações, **Copiar** junta as versões, o
  sistema, as pastas dos dados e o fim do registro de quedas, sem chaves, tokens nem conversas.
- **Um chat nunca abre o Claude duas vezes.** Com agentes em segundo plano, parar e mandar outra
  mensagem podia deixar dois ou três processos do Claude na mesma conversa, cada um respondendo e
  soltando agentes por conta própria. Agora cada conversa tem um processo só, e o Parar derruba
  também o que ainda estava saindo.
- **Uso do plano só de quem está conectado.** O anel de uso do Claude e o do Codex, no canto da
  janela, aparecem só com a conta daquele serviço conectada. Sem conta, o anel some; entrar e
  instalar continuam no menu do logo.

## 0.9.6

O Gemini como terceiro agente, o modo foco como no VS Code e as configurações por partes.

- **O Gemini no Vitral.** Ao lado do Claude e do Codex, um chat pode responder pelo Gemini, do
  Google, pelo Antigravity CLI (o `agy`, o sucessor do Gemini CLI). Ele aparece na aba **Gemini**
  do seletor de modelo quando o `agy` está instalado, com os modelos da sua conta. Lê imagens,
  recebe mensagem no meio do turno, retoma a conversa, mostra as edições e pede permissão no mesmo
  cartão do Claude. Sem ele, o menu do logo tem **Instalar o Gemini…**, que instala e faz o login
  com a conta Google. A dupla aceita `@gemini`.
- **Sem o brilho no topo.** A linha de luz no alto das abas, da caixa de mensagem, dos cartões e
  dos menus saiu.
- **O título das abas no meio.** Sem o ícone, o título ficava mais perto da borda da esquerda.
- **Mensagem com agentes no fundo.** Com os agentes trabalhando em segundo plano, a mensagem que
  você manda entra na hora; antes ela esperava na fila até todos acabarem.
- **Modo foco como no VS Code.** Cada turno vira uma linha só com o que o agente fez, que abre no
  clique: rodando, “Buscando cupom · 2 chamadas”; pronto, “Trabalhou 16 s · 1 comando”, com a
  resposta embaixo. Antes as ferramentas sumiam de vez.
- **A tela dos agentes.** A lista de todos os agentes à esquerda e o escolhido à direita, com os
  passos numerados e o resultado. Antes só dois cabiam, e os outros ficavam escondidos.
- **O Permitir à vista.** No pedido de permissão, o botão Permitir saía quase apagado e só
  clareava com o ponteiro em cima; agora é claro desde o começo.
- **O resumo da volta começa pelo que precisa de você.** No “Enquanto você esteve fora”, os
  pedidos vêm primeiro e o que terminou vem por último, até 3 linhas por grupo, cada uma com o
  nome do chat.
- **A lista mais limpa.** A linha do chat aberto fica só com o fundo, sem a barrinha azul na ponta;
  a hora fica mais legível com a janela transparente; e o pé da lista esmaece, em vez de cortar a
  última linha a seco.
- **Configurações por partes.** Uma coluna à esquerda com Chats, Agentes, Memória, Aparência,
  Claude Code e Atualizações, e as opções de cada parte à direita. Os seletores (modo, agente,
  modelo) ficam na mesma caixa escura do contador, alinhados, e toda linha tem o ⓘ.
- **Acabamento.** A barra de ações do seu balão (hora, editar, copiar) não cobre mais a primeira
  linha do texto; as linhas do “Enquanto você esteve fora” ficam alinhadas, sem o balão de cada
  uma e com o que é pedido (“Bash: ls -la”); o “1 rodando” usa o mesmo aro da lista; os campos
  sem reticências (“Mensagem para este chat”); e as dicas de tecla no mesmo tempo (“↑ ↓ escolhe”,
  “Enter abre”).
- **Menus sem aperto.** Quando o menu não cabe na janela, ele rola; antes os itens se espremiam
  até as letras encostarem. Os interruptores seguem o zoom, como o resto.
- **Configurações mais arrumadas.** O Claude Code num cartão só; os botões Atualizar, Ligar e
  Procurar agora na mesma caixa escura dos seletores; e o subtítulo diz que tudo grava na hora.
- **Atalhos do teclado em duas colunas de verdade**, na largura das configurações, sem o botão
  Fechar (o × e o Esc fecham).
- **Tipografia.** O segmento escolhido (como “7 dias”) engrossa sem mudar de largura, então os
  vizinhos não andam; o nome de arquivo corta no meio e a extensão fica à vista (na lista de
  arquivos, nos anexos e nas edições do chat); as aspas são curvas; e o título de uma ideia no
  quadro vai até 2 linhas. O menu de anexar ficou mais largo, para os rótulos não cortarem.
- **Movimento.** A chave recusada (na Memória da equipe e no Linear) deixa o campo vermelho e o
  faz tremer; o endereço que não responde aponta o campo dele. Nas boas-vindas, o passo novo
  desliza do lado para onde você vai. Trocar a transparência ou o zoom não anima as cores do tema
  velho para o novo. E todas as dicas seguem a regra dos botões: só a primeira espera; com uma
  aberta, a próxima aparece na hora.
- **As cores de estado iguais em todo lugar.** O que terminou e você ainda não viu é verde na
  lista também (era azul só nela); o azul fica para o que roda.

## 0.9.5

O fundo das suas mensagens parado e as imagens delas com o canto redondo.

- **O fundo das suas mensagens parado.** Com um chat trabalhando, o fundo dos balões trocava de
  tom quando o ponteiro passava pela conversa.
- **As imagens das mensagens com o canto redondo.** A miniatura de uma imagem anexada saía de
  canto reto dentro da pílula redonda, no balão e na caixa de mensagem.

## 0.9.4

A mensagem presa no alto da conversa sem cobrir as abas.

- **A mensagem presa não sobe por cima das abas.** Empurrada pela mensagem seguinte, a faixa da
  mensagem presa no alto subia por cima do cabeçalho do painel e cobria as abas; agora ela sai
  cortada no alto da conversa.

## 0.9.3

O envio sem travar a janela, as abas, a seleção de texto por painel, o chat arquivado que volta a
andar e a imagem anexada por cima.

- **Mandar mensagem não trava mais a janela.** A mensagem ia para o Claude Code pela própria
  janela, e uma com print anexado (mais de 1 MB) esperava ele terminar de subir para ser lida: a
  janela ficava parada de 2 a 6 s, e às vezes o GNOME perguntava se era para forçar a saída.
  Agora ela segue por fora, e a conversa responde na hora.
- **A seleção fica no painel.** Marcar um trecho num painel pegava também a faixa da mesma altura
  nos outros painéis abertos.
- **As abas sem sobras.** Uma aba de título curto deixava um vão vazio antes da seguinte; segurar a
  aba punha o × ao lado do título, fazendo ela crescer; o arrasto mostrava o balão que as abas não
  têm mais; a aba de fundo com rascunho ficava com uma ou duas letras do título; e, no painel da
  direita estreito, clicar na aba do arquivo só com o ícone fechava o arquivo.
- **Mensagem num chat arquivado começa na hora.** Escrever num chat arquivado, ou apertar o
  **Entregar agora** da fila dele, o tira das Arquivadas e a mensagem entra; antes ela ficava
  esperando na fila, e o botão não fazia nada.
- **A imagem anexada abre por cima de tudo.** A imagem aberta em cima da caixa saía cortada na
  borda do painel e passava por baixo do painel vizinho; agora ela flutua por cima, como os menus,
  sem sair da janela.

## 0.9.2

O modo foco para todos os chats e o fundo da conversa vazia sem emenda.

- **Modo foco para todos os chats.** Ligado no menu do painel (ou na paleta), ele esconde as
  ferramentas de todas as conversas de uma vez, e fica guardado. O botão marcado no alto do painel
  saiu.
- **O fundo da conversa vazia sem emenda.** Na conversa vazia, com a transparência ligada, o fundo
  passava do transparente para o escuro da caixa numa linha reta; agora ele fecha devagar antes.

## 0.9.1

Correções da 0.9.0 na conversa, nos menus e nas contas.

- **A mensagem presa não cobre a seguinte.** A sua mensagem seguinte, chegando no alto da
  conversa, empurra a faixa da mensagem presa para cima, como no VS Code; antes ela passava por
  baixo da faixa e ficava escondida.
- **Sem a sombra embaixo da mensagem presa.** A faixa termina seca, sem a sombra escura de antes.
- **Menus menores.** As linhas das listas de escolha (o modelo, o "/", a paleta, as mensagens)
  ficaram mais baixas, com o título em 13: cabe mais sem rolar.
- **A conta repetida sai.** Quando o login do Claude Code passa a ser o de uma conta extra, ela
  some da escolha (eram duas linhas da mesma conta, e a Automática revezava entre ela e ela mesma).
- **Realce arredondado nas linhas.** O realce do ponteiro nas linhas de lista (os chats, as contas)
  tem o canto das abas e dos botões, e não mais quase reto.
- **A trilha encosta na caixa.** Quando a conversa termina numa linha da trilha ("Pensou por 5 s"),
  o vão até a caixa é de 12, e não mais de 32; os 32 ficam só depois de uma resposta.
- **Todo anel da conversa gira.** A chamada rodando ("Rodando npm test"), o grupo com uma chamada
  rodando e a revisão da memória giram como o "Pensando…": todos numa camada só, por cima da
  conversa, que não se redesenha a cada volta. Parados, eles pareciam travados.
- **Copiar e hora só na resposta que fecha o turno, e por cima.** No texto do meio do turno, a barra
  ficava por cima da linha da trilha logo abaixo; e a linha de baixo (a revisão da memória) era
  pintada por cima dela, que parecia transparente. Agora ela é pintada por último, opaca.

## 0.9.0

O quadro de ideias, a paleta de comandos e o uso do plano à vista, num app mais leve e com a conversa redesenhada.

- **Quadro de ideias (Ctrl+I).** A lista do que você quer fazer no futuro, num kanban com
  Ideias, Próximas, Fazendo e Feitas. A ideia se anota no alto de Ideias, os cartões se arrastam
  entre as colunas e o clique abre o título, o pedido, o chat e a pasta. Uma ideia pode ser de um
  chat: **Ideias deste chat**, no menu da conversa, mostra só as dela, e **Mandar no chat** põe o
  pedido na fila dela, sem criar outro. Sem chat, **Abrir chat** cria um com o pedido.
- **As ideias ficam no Linear.** Com a chave pessoal do Linear, as ideias são as issues com a
  etiqueta Ideia do time (o EFI): as colunas seguem o estado, e o que você cria, edita, arrasta ou
  apaga aqui muda lá. As tarefas do time não entram. O filtro do cabeçalho mostra todas, só as do
  Linear ou só as do Vitral; com Vitral, a ideia nova fica só aqui.
- **Paleta de comandos (Ctrl+Shift+P).** Tudo o que o Vitral faz numa lista com busca: os atalhos
  (com as teclas ao lado) e o que só existia em menu, como Compactar, Servidores MCP e Contas. O
  Enter faz no painel ativo; o que não vale no momento aparece apagado, com o motivo.
- **Suas mensagens já enviadas (Ctrl+R).** Uma lista das mensagens que você mandou em todos os
  chats, com busca sem acento: as do chat aberto e as da mesma pasta vêm primeiro, sem repetir. O
  Enter põe a escolhida na caixa, sem enviar.
- **Arranjos de painéis com nome.** No menu do logo, **Arranjos** guarda os painéis, as abas e as
  larguras de agora com um nome ("Central", "Vitral") e troca entre eles; a volta é de onde você
  parou. Ctrl+Alt+PageDown e PageUp andam pelos arranjos, e os chats fora de vista continuam
  rodando.
- **Continuar daqui em outro chat.** Nas ações de uma mensagem sua e de uma resposta, o ícone do
  ramo abre ao lado um chat novo com a conversa até aquele ponto, e o agente dele lembra do que
  veio antes. O chat original fica como está: dá para testar outro caminho sem perder o primeiro.
- **Corrigir mensagem sem perder o contexto.** Corrigir uma mensagem não troca mais a conversa do
  agente por um resumo: ele volta ao ponto exato antes dela, com tudo o que veio antes. Quando
  isso não dá (Codex, conversa antiga), continua como era, com o resumo.
- **Previsão de quando o limite acaba.** No popover do uso do Claude, cada janela diz se o ritmo
  de agora chega a 100% antes de renovar ("No ritmo de agora, chega a 100% às 15:10") ou se "Dá
  até a renovação". Quando as 5 h vão acabar antes da hora, o anel do canto fica âmbar e a dica
  dele diz a hora, mesmo abaixo de 70%.
- **Mandar a mensagem numa hora marcada.** No **…** da caixa, **Mandar quando a janela de 5 h
  renovar** e **Mandar às…** deixam a mensagem na fila do chat com o selo da hora; ela sai sozinha
  na hora, com o Vitral aberto, e continua lá depois de reiniciar. O × e o lápis da fila valem
  para ela, e **Entregar agora** a manda antes.
- **Onde o uso foi.** Em Contas do Claude, uma seção nova mostra o gasto de hoje ou dos últimos
  7 dias por pasta, em barras, e dentro de cada pasta os chats que mais gastaram, com os tokens e
  o custo estimado; o clique num chat abre a conversa. Conta o que rodou a partir desta versão.
- **Tokens e custo de cada turno.** A dica da linha "Trabalhou 6 min · 13 comandos" do turno
  recolhido diz o que ele gastou, "182 mil tokens · US$ 0,84", e abre em entrada nova, cache lido
  e saída. O popover do contexto ganhou **Último turno**, com os mesmos três números. Os
  turnos de antes desta versão continuam como eram.
- **Memória de cada chat.** No Linux, a dica do relógio do turno diz quanta memória o chat ocupa
  agora, com tudo o que o agente abriu (comandos, testes, servidores): "Memória deste chat: 3,1 GB".
- **Ramo do git à vista.** A seção Na pasta do Este chat mostra no cabeçalho o ramo em que a pasta
  está (ou o commit, com a cabeça solta) e se ela é um worktree; a dica da linha do chat na lista e
  a da aba dizem o mesmo.
- **Novo chat direto num worktree.** A paleta "Novo chat · Em qual pasta?" ganhou o grupo
  Worktrees: as cópias dos repositórios das pastas que já têm chat, com o ramo embaixo do nome. A
  busca acha pela pasta e pelo ramo, e escolher abre o chat já na cópia.
- **Sumário da conversa no Este chat.** A seção Nesta conversa tem uma linha por mensagem sua,
  com a hora, o começo do texto e o que o turno fez ("3 editados · 12 comandos"). O clique rola a
  conversa até a mensagem (trazendo as anteriores, se for preciso) e a realça por um instante; a
  que você está lendo fica marcada enquanto a conversa rola.
- **Resumo do dia para copiar.** "Resumo de hoje", no menu do logo, mostra por pasta os chats
  que trabalharam: o título, a primeira frase da última resposta, os arquivos mudados, o tempo
  trabalhado e o que ficou pendente. Dá para trocar para Ontem e 7 dias e copiar tudo como
  Markdown. É montado do que o app já guarda, sem gastar token.
- **O estado do chat no contorno da aba.** As abas perderam o ícone. Com o chat trabalhando, o fio
  da aba fica azul e respira; quando ele termina e você ainda não leu, verde; esperando você, âmbar;
  erro, vermelho.
- **Mensagem presa sem o cinza.** A faixa da sua mensagem no alto da conversa tem as mesmas
  camadas do cartão, transparente como ele, com uma sombra curta embaixo.
- **Conversa com duas bordas só.** O texto da resposta, o ícone de cada linha do que o agente fez,
  o do erro e o de quem entregou começam todos na mesma borda; o texto dessas linhas e tudo o que
  elas abrem (a saída, o diff, o raciocínio, o relatório) começam numa segunda, 24 depois. Eram
  cinco começos diferentes. O grupo aberto virou uma linha do tempo: os ícones um embaixo do
  outro, ligados por um fio fino, sem o recuo e sem os ramos em curva.
- **Avisos num desenho só.** O dia, "Trazida do Claude Code", "Turno interrompido" e a troca de
  conta são a mesma nota: centrada, pequena e tênue, sem fio e sem ícone. Só o "Novo" continua com
  o desenho dele.
- **Copiar e hora sem cobrir a resposta.** A barra que aparece com o ponteiro na resposta fica
  embaixo do texto, no vão até a próxima mensagem; antes ela cobria o fim da última linha.
- **Espaços em três passos.** 32 entre turnos e no fim da conversa, 12 dentro do turno e nada
  entre as linhas da trilha. Saíram os passos de 16 e de 4.
- **Linha mais curta no painel largo.** A coluna de leitura passa a ter uns 80 caracteres por
  linha (eram perto de 100); a caixa de mensagem acompanha.
- **Menos tamanhos e menos peso.** O nome de quem entregou e a frase do erro usam o tamanho do
  balão; a chamada que falhou perde o fundo vermelho da linha inteira e fica com o ícone e a
  palavra "falhou" em vermelho, em qualquer largura; a prévia da entrega tem duas linhas também
  na coluna estreita.
- **Código que não embaralha na coluna estreita.** O bloco de código da resposta não quebra mais
  as linhas: quebrada, a linha voltava à margem e a indentação se perdia (com três conversas lado
  a lado, seis linhas de código viravam catorze). Agora ele rola de lado, como a saída de comando
  e o diff, e a roda em pé continua levando a conversa.
- **Tabela sem quebra à toa no zoom.** Com o zoom acima de 100%, a última palavra de cada célula
  caía para a linha de baixo mesmo com a tabela sobrando de largura. As colunas passam a ter a
  largura do texto em qualquer zoom.
- **Trilha mais igual e sem palavra cortada.** O "Pensou" fica tênue como os outros verbos, com a
  duração na coluna dos detalhes. No subagente, a tarefa aparece inteira, e quem cede lugar é a
  conta de tokens, ferramentas e tempo, uma parte por vez. O resumo do grupo e o do "Trabalhou"
  mostram só as partes que cabem inteiras ("1 comando · 1 lido"), em vez de terminar em
  reticências no meio de uma palavra.
- **Caixas da trilha com um desenho só.** O diff aberto perdeu o cabeçalho que repetia o nome do
  arquivo e a caixa cinza dentro da preta, e o Copiar dele aparece no hover, como nas outras. A
  saída de comando só diz "Falhou" ou "Em execução" (o "Concluído" de toda saída saiu), o
  resultado sem comando não tem mais a faixa vazia em cima, e o raciocínio e os passos do
  subagente ficaram com o mesmo canto do relatório. No erro com o título em duas linhas, o ícone
  fica na primeira.
- **Suas mensagens como um buraco no cartão.** O balão do que você escreve mostra o fundo da
  lista (o metal com o véu azul), com a sombra caindo da beirada de cima, sem contorno e sem ponta.
  A mensagem longa fica em duas linhas e abre com um clique (o chevron no canto diz que há mais).
- **A sua última mensagem presa no alto.** Assim que a sua mensagem sobe além do topo, ela gruda
  lá, em duas linhas, como no VS Code (também nas conversas trazidas do Claude Code); o clique
  leva até ela.
- **Uso e contexto redesenhados.** Os anéis de uso do Claude e do Codex moram numa cápsula só no
  canto, com um anel fino e inteiro que enche a partir do alto (a inicial da conta, menor,
  recortada nele). O contexto da conversa, na caixa, virou um medidor deitado com a porcentagem:
  o anel com um pedaço azul lia como o "carregando".
- **Mais leve com um chat trabalhando.** As mensagens de cada painel e a lista viraram partes que
  só se redesenham quando mudam: o anel girando, o metal e a rolagem de um painel não refazem o
  resto. Medido na bancada com um chat rodando: a thread da janela caiu de 48% para 34% de um
  núcleo, e montar a janela, de 3,3 ms para 0,8 ms por quadro. O anel do "Pensando…" gira por cima
  da conversa, numa parte só dele, e não a redesenha a cada volta; os anéis das linhas de
  ferramenta dentro dela ficam parados.
- **Títulos sem reticências.** As abas e os chats da lista cortam o título seco, sem os três
  pontinhos, e o número da aba fica colado nele. O "Este chat" perdeu o número do painel, e com o
  painel da direita estreito a aba à vista mantém o nome.
- **Sombra mais macia** entre a lista e as conversas, sumindo devagar em vez de acabar numa beirada.

## 0.8.0

Um app mais enxuto, o Codex ao lado do Claude e uma conversa que mostra melhor o que o agente faz.

- **Digitar em qualquer ponto escreve na caixa.** Depois de clicar na conversa (para abrir uma
  trilha ou selecionar um trecho) ou numa aba, a letra que você digita vai para a caixa do painel
  ativo, e o Ctrl+V cola nela: um print copiado vira pílula sem clicar antes.
- **A conversa abre onde a resposta começa.** Um chat que terminou abre no começo da resposta
  nova quando ela não cabe na tela, com um separador "Novo" antes do que você não viu. Trocar de
  aba no meio de uma leitura guarda o lugar, e voltar à aba volta a ele. Abrir não marca mais como
  lido: o chat só sai de "Terminou" quando você chega ao fim.
- **Turno pronto vira "Trabalhou 6 min".** Ao terminar, o que o agente fez antes da resposta final
  recolhe numa linha ("Trabalhou 6 min 12 s · 13 comandos · editou carrinho.py e motor.rs"), com a
  resposta logo embaixo. As perguntas do meio, os erros e as imagens continuam à vista, e o clique
  na linha mostra o resto.
- **Quem espera você aparece onde você está olhando.** O pedido de permissão, o plano e a falha de
  um chat fora de vista viram um aviso no canto de baixo, à direita, com Negar e Permitir (ou Ver
  plano, Tentar de novo) ali mesmo. Ele fica até ser respondido e nunca pega o foco da caixa. Com
  a janela atrás, o pedido de permissão fica na tela do GNOME até o clique. Desliga em
  Configurações, **Avisar dentro do app**.
- **Falha que diz a causa e já oferece a saída.** A faixa acima da caixa mostra a primeira linha do
  erro (o texto inteiro na dica). No limite de uma conta escolhida à mão, com outra conta livre,
  **Continuar na <outra>** troca a conta do chat e repete o turno num clique. No login perdido,
  **Entrar de novo** abre as Contas já refazendo o login daquela conta. O turno com erro passa a
  notificar com a janela atrás.
- **Entrar de novo na conta que perdeu o login.** Nas Contas do Claude, a conta sem login mostra
  "desconectada" e o botão **Entrar de novo**, que refaz o login nela mesma, sem criar uma conta
  duplicada. A marca continua depois de fechar o Vitral, a conta cujo token a Anthropic recusa na
  leitura do uso também aparece desconectada, e o "…" de cada conta tem **Entrar de novo** a
  qualquer hora. O **Entrar de novo** do erro abre já no login da conta que caiu; sem saber qual
  foi, mostra o botão em cada conta.
- **Revisão da memória sem prender o chat.** O "Terminou" sai assim que o turno de trabalho acaba;
  a revisão da memória roda por baixo e vira uma linha na conversa ("Memória revista · nada a
  guardar"). Uma mensagem sua no meio dela corta a revisão (se ela ainda não escreveu nada) e
  começa o seu turno.
- **Menu "@" na própria caixa.** O "@" abre um menu como o do "/", sem tirar o cursor da caixa:
  **@claude** e **@codex** na dupla (agora funcionam pelo teclado) e os arquivos e as pastas do
  chat, com Tab ou Enter para completar. A lista de arquivos passou a descer nos repositórios de
  dentro da pasta: em ~/eita, o "@", a aba Arquivos e o Mencionar acham o código de central/,
  layout/ e dos outros projetos.
- **Menções em pílula.** O arquivo mencionado vira uma pílula com o ícone, o nome e as linhas
  ("fila.rs:120-134"), dentro da caixa enquanto você escreve e no balão da mensagem enviada. O
  clique abre o arquivo ao lado, nas linhas. Para o agente, a menção continua o caminho inteiro.
- **Ver a edição no próprio arquivo.** Cada arquivo editado vira uma linha com o nome e o +N −M,
  também embaixo do "Trabalhou…" recolhido. O clique abre o arquivo ao lado já na edição: o que
  entrou em verde, o que saiu em vermelho entre as linhas, rolado até a primeira mudança, com
  setas para passar de uma à outra. No Codex, uma linha por arquivo, cada uma com a sua edição.
- **Mencionar um trecho.** No arquivo aberto, **Mencionar** leva o arquivo, as linhas que você
  escolheu (clique no número; Shift+clique estende) ou, na edição, a mudança à vista.
- **Cabeçalho do arquivo que cabe.** O nome, as linhas e a linguagem nunca passam por baixo dos
  botões nem dos anéis de uso: sem lugar, os botões ficam só com o ícone e o resto sai inteiro.
- **O que mudou fica à vista.** A trilha diz o nome dos arquivos editados ("editou carrinho.py e
  motor.rs") quando são até dois. Quando o turno muda arquivos, a faixa colada na caixa diz "3
  arquivos mudados · +40 −12", e **Ver mudanças** abre "Este chat" com a lista: neste turno e
  antes, com o +N −M de cada um e o selo "novo" no que foi criado. O clique abre o arquivo à direita.
- **Toda lista diz o que o chat respondeu.** O sino mostra a primeira frase da resposta nova e o que
  o turno fez ("3 editados · 12 comandos"); o histórico do "+" diz o estado e o começo da última
  fala; a dica da aba de fundo também; e o "Enquanto você esteve fora" deixa o título curto, com a
  frase ao lado.
- **Ctrl+K como seletor rápido.** Com a lista recolhida, o Ctrl+K a abre por cima dos painéis e
  ela recolhe sozinha ao abrir o chat. Com texto na busca, o primeiro resultado já vem marcado, e as
  setas movem a marca sem tirar o cursor do campo. No histórico do "+", o que espera você vem no alto.
- **Fila que não perde nada.** O × da fila devolve a mensagem inteira à caixa, com os anexos e o
  Ultrathink ou Ultracode, e com algo escrito ela se junta ao fim (Ctrl+Z desfaz). Com o turno
  rodando e a caixa vazia, a seta para cima traz de volta a última da fila. O lápis edita na própria
  linha, sem diálogo (Ctrl+Enter salva, Esc desiste).
- **Busca de chat sem acento e por palavras.** Na lista e no histórico do "+", "integracao" acha
  "integração" e "vip cupons" acha "Cupons para grupos VIP"; o que casa no título vem primeiro.
- **"+" do painel mais claro.** **Novo chat neste painel** fica no alto, com o + azul e o atalho
  (Ctrl+N), separado dos chats em "Recentes"; o Enter cria, e o visto marca o chat à vista.
- **Rascunho inteiro e à vista.** O rascunho guardado leva os prints colados e a citação, e volta
  inteiro quando o Vitral abre de novo. Na lista e nas abas, o chat com mensagem começada ganha um
  lápis e "Rascunho: começo do texto".
- **Ctrl+G percorre todas as pendências.** Ele vai à seguinte à do chat aberto e dá a volta
  (Ctrl+Shift+G volta), põe o foco no Retomar ou no Tentar de novo quando é o caso, e diz "2 de 3 ·
  Parada · WMS". No sino, as Paradas ganham **Retomar tudo**.
- **Esc que não custa caro.** Depois de um Esc, a faixa oferece **Continuar**, e o Enter com a caixa
  vazia continua o turno. Com agentes em segundo plano ou Ultracode rodando, o primeiro Esc só arma
  ("Esc de novo para parar os 3 agentes"), para um Esc distraído não jogar fora o trabalho deles.
- **Compactar num clique.** O "Compactar" do anel do contexto e o da conversa grande parada compactam
  na hora, sem apagar o que estava escrito na caixa; com o turno rodando, entram na fila.
- **Abas consistentes.** Todos os painéis usam a mesma moldura nas abas; a selecionada muda de tom.
- **Fila junto da mensagem.** Abre acima da caixa, dentro do painel, com **Mandar agora** para
  enviar uma mensagem ao turno em andamento. O Codex também recebe novas instruções enquanto trabalha.
- **Trilha de atividade.** Resumo do trabalho, ramificações entre as chamadas e identificação dos
  arquivos. Os grupos abrem enquanto trabalham e recolhem ao terminar, respeitando sua escolha.
  Abrir uma edição mostra o diff; abrir um comando reúne o comando e sua saída.
- **Claude e Codex no mesmo chat, sem ligar nada.** Num chat que já conversou, trocar de agente no
  seletor de modelo ou começar a mensagem com @claude ou @codex chama o outro: ele entra com a
  conversa até ali, e o de antes continua no chat com a sessão dele (o @ o chama de volta). Cada um,
  antes da vez dele, recebe o que o outro disse e os arquivos que mexeu, e a conversa mostra quem
  está trabalhando. Para os dois em toda mensagem (um trabalha e o outro revisa, até um dizer "Nada
  a acrescentar" ou até 6 turnos), ligue **Os dois em toda mensagem** no seletor de modelo.
- **Subagentes à vista de novo.** No Claude Code novo os subagentes trabalham em segundo plano, e o
  Vitral os dava como terminados logo ao começar. Agora eles ficam como trabalhando até o fim, com o
  que estão fazendo, e cada um mostra desde o começo os tokens, as ferramentas e o tempo, como na
  extensão. O relatório de cada um sai em texto, sem a moldura que o Claude Code põe em volta.
- **Sem o Ultrathink.** No Claude Code de agora a palavra não muda nada, e a chave só subia o esforço
  da mensagem para Extra, o mesmo que escolher o Extra no seletor de esforço.
- **Corrigir qualquer mensagem sua.** O Editar aparece em todas as suas mensagens, não só na última
  (e não some mais quando o Claude pensa antes de responder). A conversa volta ao ponto antes dela.
- **Suas mensagens na largura toda** da coluna, num balão azul mais baixo que a resposta: os
  caminhos de arquivo viram link, o código vai num poço escuro e um "/compact" sozinho vira uma
  pílula.
- **Erros em português.** O erro do turno vira um cartão com o que aconteceu, o que fazer e quando
  o limite volta, o Copiar no canto e o texto do agente em **Detalhes**, com os links curtos. No
  último erro, as ações que resolvem: ver o uso, continuar no outro agente, entrar de novo,
  instalar o Codex ou compactar. A faixa acima da caixa diz o que falhou.
- **Imagens em pílulas, como na extensão do Claude Code.** As imagens que você manda (dentro do
  balão, em cima do texto), as que a resposta aponta e as que o agente leu ou gravou viram uma pílula
  com a miniatura, o nome e as medidas; o clique abre o visor.
- **Aviso de conversa grande e parada.** Depois de uma hora sem uso, uma conversa do Claude com
  mais de 100 mil tokens avisa, acima da caixa, quanto a próxima mensagem vai reler por inteiro, com
  o botão **Compactar**. O seletor de modelo avisa o mesmo custo antes de trocar o modelo.
- **Menos gasto escondido.** O Vitral desliga no Claude Code as chamadas que ele não mostra: o
  resumo de quando você volta e o título do terminal.
- **Sugestão da próxima mensagem.** Depois de cada resposta do Claude, a mensagem que você
  provavelmente escreveria ("roda os testes", "commita") aparece apagada na caixa vazia, e o Tab a
  põe na caixa para mandar ou mudar. Quando o próximo passo não é óbvio, nada aparece. Desliga em
  Configurações, **Sugerir a próxima mensagem**.
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
  lista, a divisão entre as conversas e o tamanho da janela de quando foi fechado. Cada quantidade
  de conversas lado a lado lembra a sua divisão (fechar uma e abrir de novo volta como era), e a
  lista recolhida no arrasto volta, no Ctrl+B, na largura de antes.
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
- **Conversa mais enxuta.** As linhas do que o agente fez ficaram mais baixas (28), com o verbo em
  tom apagado e o detalhe em mono solto, sem o chip escuro em cada linha; o dia ("Hoje") perdeu os
  fios dos lados; a caixa de código perdeu o fio do cabeçalho e o Copiar só aparece com o ponteiro
  nela; a linha do turno não mostra mais "ação 5"; o enviar tem a altura dos chips ao lado; e as
  abas cortam o título sem as reticências, com o número mais perto.
- **Popovers dentro da janela.** O de modelo e o do uso do plano não passam mais da borda com o
  zoom alto: a lista encolhe e rola por dentro.
- **Na pasta: o que o git vê.** O **Este chat** ganha a seção Na pasta, com tudo o que mudou desde
  o último commit, inclusive o que um comando mudou (a lista de Mudanças só via as edições). O
  clique abre o arquivo com o diff por cima, onde o número da linha escolhe um trecho para
  mencionar na caixa; o **Desfazer** de cada linha volta o arquivo ao último commit, depois de
  perguntar.
- **Voltar os arquivos a antes de uma mensagem.** No balão de uma mensagem sua, o botão de voltar
  (o relógio girando para trás) mostra quais arquivos o Claude mudou desde ela e, no ok, volta
  todos a como estavam. A conversa fica como está, com um aviso do que voltou. Vale para as
  mensagens mandadas a partir desta versão.
- **Continuar quando o limite voltar.** Num chat parado no limite do plano, o botão **Às 14:20**
  agenda: na hora em que o limite volta, o turno é repetido sozinho. A faixa diz "Continua sozinho
  às 14:20" e o **Cancelar** desfaz; mandar outra mensagem também.
- **Os passos do agente.** Quando o Claude anota a lista do que vai fazer, a linha do turno mostra
  "2/5" (a lista inteira na dica), e o **Este chat** ganha a seção Passos, com o que ele está
  fazendo agora em destaque.
- **Comandos no fundo à vista.** Um servidor ou um build que o agente deixou rodando em segundo
  plano aparece na linha do turno ("1 comando no fundo", com o comando na dica); o clique para.
- **Dois chats no mesmo arquivo.** Quando um chat edita um arquivo que outro chat editou nas
  últimas 12 horas, a conversa avisa ("O chat «Fila» também editou fila.rs…"), uma vez por par.
- **Turno calado à vista.** Depois de 10 minutos sem nada do agente (nem texto, nem ferramenta), a
  linha de estado diz "sem sinal há 12 min". Pode ser um comando longo; se travou, Esc para parar.
- **Atalhos novos.** Ctrl+Tab e Ctrl+Shift+Tab andam pelas abas do painel ativo, Ctrl+Shift+T
  reabre a última aba fechada e Ctrl+Shift+C copia a última resposta do chat à vista.
- **Os atalhos não morrem mais.** Quando o botão com o foco some da tela (o Retomar e o Tentar de
  novo saem quando o turno começa), o foco volta para a caixa; antes, as teclas da janela paravam
  até um clique.
- **Registro de queda.** Um erro interno fica gravado em `queda.log`, na pasta dos dados, e na
  abertura seguinte o Vitral avisa que aconteceu.
- **O notebook não dorme no meio de um turno.** Enquanto um chat trabalha, o Vitral segura a
  suspensão automática do sistema (a tela ainda apaga e bloqueia). Ela volta assim que nenhum chat
  está andando; o chat parado num pedido de permissão não a segura.
- **Acabamento.** Cantos mais redondos, que acompanham o zoom; o uso do plano foi para o canto de
  cima, à direita; o ícone de cada chat mostra o estado dele (trabalhando, esperando você, com
  erro); arrastar a borda da lista até o fim a recolhe; e os fundos podem ficar um pouco
  transparentes (**Transparência dos fundos**, nas Configurações; o padrão é opaco), com um grão
  bem fino.
- **Copiar discreto.** O botão de copiar dos blocos de código, das citações e das mensagens tem o
  ícone pequeno e apagado dos outros botões de linha, e vira um visto verde por 2 s.
- **Selo do sino menor e recortado.** O número do sino tem o tamanho de um contador e um aro
  escuro que o separa do botão.
- **Nomes inteiros na aba Arquivos.** Os botões de cada arquivo só ocupam lugar com o ponteiro em
  cima: fora disso, o nome vai até o fim da linha.
- **Ícones de estado do mesmo tamanho.** Na lista e nas abas, o disco de "esperando" e o anel de
  "rodando" têm a medida do balão do chat parado, numa coluna só.
- **Lista mais leve.** Os títulos dos chats lidos ficaram em peso normal: a conversa fica em
  primeiro plano, e o chat não lido salta à vista em negrito.
- **Lista mais legível.** Com o chat aberto num painel, a linha mostra só o número do painel, e o
  título ganha o lugar da hora (ela vai para a dica do ícone, junto da pasta). A hora ficou mais
  clara, e o número do painel ativo deixou de parecer um aviso de não lidas. O número das
  Arquivadas foi para a dica.
- **Porcentagem do contexto sempre à vista**, também com três painéis lado a lado: o anel sozinho
  parecia algo carregando.
- **"+" em cada painel: chat novo ou histórico.** O "+" no cabeçalho abre uma lista com "Novo chat
  neste painel" no topo, já marcado (Enter cria), e todos os chats, do mais recente ao mais antigo,
  com busca: o escolhido abre neste painel, numa aba.
- **Sem o "⋯" no cabeçalho.** O menu do painel (modo foco, mostrar o painel da direita, nova
  conversa ao lado, renomear, fixar, arquivar) abre com o botão direito no cabeçalho ou numa aba
  (na aba, vale para aquele chat), e com Shift+F10 ou a tecla Menu.
- **Os anéis de uso num disco, sem folga na conversa.** Os anéis do Claude e do Codex continuam
  embaixo do × da janela, agora cada um num disco opaco com sombra curta, como um botão flutuante;
  a conversa, a caixa e as faixas do último painel vão até a borda, sem a folga que guardavam para
  eles.
- **Menos texto cortado.** O título "Enquanto você esteve fora" quebra em duas linhas na lista
  estreita, em vez de virar "Enquanto voc…". A faixa de cima da caixa diz "Parada · Vitral
  fechou", como a lista, com o motivo inteiro na dica. O aviso de turno cortado pelo fechamento do
  Vitral virou um marco de uma linha, "O Vitral fechou no meio do turno", sem repetir a instrução
  a cada turno nem falar do Claude no chat do Codex; no fim de um painel parado, em que a faixa
  acima da caixa já diz isso com o Retomar, ele não se repete.
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
- **Textos das Configurações acertados.** A dos anéis de uso diz onde eles ficam, a
  dos servidores MCP acompanha o interruptor, e as linhas de modo, agente e modelo dizem o que cada
  chat troca.
- **Ultracode desligado legível.** O motivo de ele não valer no chat aparece claro (só a chave fica
  apagada) e diz "Só no Claude Code", certo também com Claude e Codex juntos.
- **Corrigir uma mensagem não perde o que está na caixa.** Com algo escrito, o Vitral pergunta
  antes, e o Ctrl+Z traz o texto de volta. A mensagem corrigida fica guardada como rascunho na hora,
  mesmo se o Vitral cair logo depois, e a caixa mostra o fim dela, com o cursor. O aviso de depois
  cabe inteiro. E uma queda logo depois de enviar não traz mais a mensagem enviada de volta para a
  caixa. Colar um texto longo deixa à vista o fim do que foi colado, com o cursor.
- **Menu "/" pelo teclado.** O Enter completa o comando marcado no menu (antes podia completar
  outro, até o /clear), as setas andam pelo menu, o Tab completa e o Esc só fecha o menu, sem
  apagar o que foi escrito.
- **Fundo mais fosco.** Com a transparência ligada, o véu leitoso por cima do fundo ficou mais forte
  (mais ainda nos níveis mais transparentes): o que está atrás da janela aparece esmaecido, como
  vidro fosco, e não nítido como janela aberta.
- **Fosco de verdade.** Nos níveis transparentes, o que está atrás da janela aparece desfocado pelo
  próprio sistema: no Windows (o acrílico), no macOS, no KDE e no GNOME 51 em diante. No GNOME 50
  ou anterior a janela fica só transparente, com o véu fosco por cima. No Opaco o Vitral não pede
  desfoque nenhum, e trocar o nível nas Configurações vale na hora.
- **Editar na fila salva com Ctrl+Enter.** O lápis de uma mensagem da fila abre com o cursor no fim,
  e o Ctrl+Enter salva (antes quebrava a linha).
- **Aba sem o espaço do ×.** Nenhuma aba guarda mais lugar para o ×, nem a selecionada: ele surge
  por cima do fim da aba quando o ponteiro passa (ou com o foco do teclado), e a aba não muda de
  tamanho. A aba à vista também não abre mais a dica por cima da conversa.
- **O metal do escolhido e do aro de foco continua o de antes.** A rodada de aparência tinha
  trocado o metal vivo da ficha do escolhido e do aro de foco por um recorte mais liso; o dono
  preferiu o metal de antes ("por que tirou os shaders?"), e ele voltou.
- **Marca do modelo mais discreta.** No chip do modelo (e nos menus das Configurações) a marca do
  Claude ou do Codex aparece como glifo solto, sem o disco colorido cheio, e as barrinhas do
  esforço ficaram do tamanho de um ícone.
- **Painéis de fundo mais calmos.** Na caixa dos painéis que não estão ativos, os chips e botões
  recuam até o ponteiro chegar ou o painel ser ativado; o painel ativo fica com tudo aceso.
- **Hora e copiar no pé da resposta.** A barra que aparece no hover de uma resposta fica embaixo
  dela, e não mais por cima da primeira linha.
- **Turnos mais separados.** Mais espaço entre um turno e outro e menos entre a sua mensagem e a
  resposta: cada pergunta fica presa à resposta dela.
- **Menus por cima, não afundados.** Menus, popovers e o sino ficam um degrau mais claros que a
  caixa e as abas de onde saem, com uma sombra mais funda.
- **Aro de foco contínuo.** O aro de metal em volta da caixa com foco (e da busca, da aba sob o
  ponteiro e do pedido que espera você) é igual nos quatro lados.
- **Bordas mais finas.** As abas têm um contorno só, o fio preto de 1 px, e a moldura preta da
  caixa de mensagem caiu de 4 para 2 px. As conversas encostam no topo, na direita e embaixo da
  janela, e entre uma e outra (e o painel da direita) o vão virou uma divisória de 1 px: a
  fileira parece uma peça só, com os cantos da esquerda redondos. A lista continua separada dela
  por um vão de 8 px; com a lista recolhida, a fileira encosta também na esquerda.
- **Abas que cabem.** Sem lugar para o título, a aba de fundo vira um quadrado só com o ícone
  (antes sobravam duas reticências), e a que está à vista guarda sempre o começo do título: sem
  lugar nem assim, as de fundo saem da faixa, e um "+N" no fim dela abre a lista das que saíram. Com muitas abas,
  a que está à vista não some mais por baixo do "⋯".
- **Trocar de chat pelo teclado.** Na busca (Ctrl+K), Enter abre o primeiro resultado e o Ctrl+K
  seguinte troca o termo em vez de somar. Depois de clicar num chat da lista, o que se digita vai
  para a caixa. O teclado não trava mais quando um painel aberto ao lado empurra a lista para fora.
- **Esc para o turno de qualquer lugar.** Depois de clicar na conversa (o foco sai da caixa), o Esc
  continua parando o turno do painel ativo, como a linha de estado mostra.
- **Imagens do grupo recolhido numa linha só.** As imagens que o agente leu num grupo de
  ferramentas recolhido viram uma pilha de miniaturas e "9 imagens"; o clique abre o visor na
  primeira. Antes eram pílulas de larguras diferentes, quebrando em várias linhas.
- **O erro do turno aparece uma vez só.** Quando o Claude Code manda o erro também como resposta, a
  conversa mostra só o cartão do erro, e não o mesmo texto duas vezes.
- **Copiar a citação.** Quando a resposta traz uma citação (como a mensagem pronta para mandar a
  alguém), ela tem o próprio botão de copiar, que leva o texto limpo, sem os ">" nem o negrito em
  asteriscos, pronto para colar no WhatsApp. E o texto da citação ficou na cor do resto da resposta,
  em vez do cinza mais apagado.
- **Suas quebras de linha ficam.** O balão da mensagem enviada mostra as linhas como você as
  escreveu (antes juntava tudo num parágrafo, e o "Mostrar tudo" aparecia sem nada escondido).
- **Fila mais fácil de mexer.** O × de tirar uma mensagem da fila funciona com o turno rodando e
  várias na fila (a barra de rolagem ficava por cima dele), os botões cabem no painel estreito, a
  lista mostra três mensagens inteiras e o "Entregar agora" não aparece duas vezes.
- **Buscar na conversa (Ctrl+F) mais confiável.** O achado dentro de uma trilha com raciocínio
  aparece, também o que está na saída de um comando (antes o contador andava e a tela não), não
  para mais na última linha debaixo da pílula
  de voltar ao fim e o campo não encolhe até uma letra no painel estreito.
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
