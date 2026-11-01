# Novidades do Vitral

Cada versão tem a sua seção (`## 0.2.0`); o texto dela vira as notas do Release no GitHub e o que
o app mostra em **Novidades**. Os pré-lançamentos (`0.2.0-rc.1`) usam a seção da versão.

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
