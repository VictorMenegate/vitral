import { BotaoRedondo, Metal } from 'vitral-ui';

const fileira = { display: 'flex', gap: 16, alignItems: 'center', flexWrap: 'wrap' } as const;

export const TeclasDeMetal = () => (
  <div style={fileira}>
    <BotaoRedondo tipo="metal" tamanho={36} icone="plus" dica="Novo chat" />
    <BotaoRedondo tipo="metal" tamanho={36} icone="arrow-up" dica="Enviar" />
    <BotaoRedondo tipo="metal" tamanho={36} parar dica="Parar (Esc)" />
    <BotaoRedondo tipo="vazio" tamanho={36} icone="arrow-up" dica="Enviar" />
  </div>
);

export const NoTrilho = () => (
  <Metal tipo="base" style={{ ...fileira, padding: 12, borderRadius: 12, width: 'max-content' }}>
    <BotaoRedondo tipo="neutro" parar dica="Parar" />
    <BotaoRedondo tipo="plano" tamanho={24} icone="minus" dica="Menos" />
    <BotaoRedondo tipo="perigo" icone="x" dica="Remover" />
    <BotaoRedondo tipo="plano" tamanho={36} icone="bell" selo={2} dica="2 pedidos esperando você" />
    <BotaoRedondo tipo="plano" tamanho={36} icone="archive" dica="Arquivadas" />
  </Metal>
);
