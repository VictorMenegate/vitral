import { Interruptor, ListaDeEscolha } from 'vitral-ui';

export const SeletorDeModelo = () => (
  <ListaDeEscolha
    largura={388}
    abas={['Claude', 'Codex']}
    busca="Buscar modelo"
    escolhido={0}
    itens={[
      { titulo: 'Padrão', descricao: 'O que o Claude Code escolher' },
      { titulo: 'Fable', descricao: 'Para os desafios mais difíceis' },
      { titulo: 'Opus', descricao: 'Para trabalho complexo e o dia a dia' },
      { titulo: 'Sonnet', descricao: 'O mais eficiente para tarefas simples' },
      { secao: 'Conta do Claude' },
      { titulo: 'Principal', descricao: 'semana 16%' },
      { titulo: 'Automática', descricao: 'A conta com mais folga no plano' },
    ]}
  >
    <div className="vt-opcao">
      <div className="vt-txt">
        <div className="vt-t">Claude e Codex juntos</div>
        <div className="vt-d">Um responde e o outro revisa, um de cada vez. @claude ou @codex no começo fala só com um.</div>
      </div>
      <Interruptor />
    </div>
  </ListaDeEscolha>
);

export const ComandosDeBarra = () => (
  <ListaDeEscolha
    largura={360}
    busca="/"
    valorDaBusca="/co"
    escolhido={0}
    itens={[
      { icone: 'minimize', titulo: '/compact', descricao: 'Resume a conversa para liberar contexto' },
      { icone: 'settings', titulo: '/config', descricao: 'Abre as configurações do Claude Code' },
      { icone: 'circle-user', titulo: '/cost', descricao: 'Mostra o custo desta sessão', desabilitado: true },
    ]}
  />
);
