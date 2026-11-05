# Novidades do Vitral

Cada versão tem a sua seção (`## 0.2.0`); o texto dela vira as notas do Release no GitHub e o que
o app mostra em **Novidades**. Os pré-lançamentos (`0.2.0-rc.1`) usam a seção da versão.

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
