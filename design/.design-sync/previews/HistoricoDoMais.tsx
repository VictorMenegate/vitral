import { HistoricoDoMais } from 'vitral-ui';

export const Aberto = () => (
  <HistoricoDoMais
    largura={388}
    chats={[
      { titulo: 'Projeto Vitral com integração GitHub', descricao: 'no painel 3 · há 35 min · eita', estado: 'espera' },
      { titulo: 'Nova conversa', descricao: 'à vista neste painel · há 1 h · eita' },
      { titulo: 'WMS', descricao: 'há 1 h · eita', estado: 'rodando' },
      { titulo: 'Alertas de cupons para grupos VIP do WhatsApp', descricao: 'há 3 h · eita', estado: 'nova' },
      { titulo: 'Pesagem de caixas na expedição', descricao: 'há 3 d · eita' },
    ]}
  />
);

export const BuscandoComEscolhido = () => (
  <HistoricoDoMais
    largura={388}
    valorDaBusca="pesagem"
    escolhido={0}
    chats={[
      { titulo: 'Pesagem de caixas na expedição', descricao: 'há 3 d · eita' },
      { titulo: 'Pesagem da fábrica', descricao: 'há 6 d · eita', estado: 'erro' },
    ]}
  />
);
