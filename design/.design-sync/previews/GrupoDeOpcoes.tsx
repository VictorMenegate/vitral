import { ChipDaCaixa, ControleSegmentado, GrupoDeOpcoes, Interruptor } from 'vitral-ui';

export const Chats = () => (
  <GrupoDeOpcoes
    style={{ maxWidth: 640 }}
    titulo="Chats"
    opcoes={[
      { titulo: 'Resumo ao voltar depois de', descricao: 'Minutos com a janela fora de foco antes do “Enquanto você esteve fora”.', contador: 15 },
      {
        titulo: 'Mostrar o uso do plano no canto da janela',
        descricao: 'Os anéis ficam no canto de cima, à direita. Desligado, o uso fica só em Contas do Claude.',
        controle: <Interruptor ligado />,
      },
      {
        titulo: 'Modo de permissão padrão',
        info: 'Vale para chats novos',
        descricao: 'Cada chat pode trocar o modo na caixa de mensagem.',
        controle: <ChipDaCaixa modo="sem-perguntar" seta />,
      },
      {
        titulo: 'Arquivar chats parados depois de',
        info: true,
        descricao: 'Nada é apagado: eles vão para Arquivadas.',
        controle: <ControleSegmentado opcoes={['Nunca', '3 dias', '7 dias', '30 dias']} valor={2} />,
      },
      { titulo: 'Ultracode', descricao: 'Só no Claude Code', desabilitado: true, controle: <Interruptor /> },
    ]}
  />
);

export const ComAvisoDeCusto = () => (
  <GrupoDeOpcoes
    style={{ maxWidth: 640 }}
    titulo="Padrões do agente"
    opcoes={[
      { titulo: 'Esforço Máx por padrão', descricao: 'Vale para todo chat novo.', aviso: 'Gasta bem mais do plano', controle: <Interruptor /> },
      { titulo: 'Claude e Codex juntos', descricao: 'Um responde e o outro revisa.', controle: <Interruptor ligado /> },
    ]}
  />
);
