# Novidades do Vitral

Cada versão tem a sua seção (`## 0.2.0`); o texto dela vira as notas do Release no GitHub e o que
o app mostra em **Novidades**. Os pré-lançamentos (`0.2.0-rc.1`) usam a seção da versão.

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
