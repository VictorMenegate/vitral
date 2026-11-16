import { ControleSegmentado, Metal } from 'vitral-ui';

export const Neutro = () => <ControleSegmentado opcoes={['Nunca', '3 dias', '7 dias', '30 dias']} valor={2} />;

export const SoltoNoAltoDaLista = () => (
  <Metal tipo="base" style={{ padding: 8, borderRadius: 12, width: 260 }}>
    <ControleSegmentado
      solto
      largo
      tamanho={40}
      opcoes={[
        { icone: 'messages-square', dica: 'Chats' },
        { icone: 'folder', dica: 'Pastas' },
      ]}
      valor={0}
    />
  </Metal>
);

export const NoFlutuanteCompacto = () => (
  <div style={{ background: 'var(--flutuante)', padding: 16, borderRadius: 20, width: 400, boxShadow: 'var(--sombra-popover)' }}>
    <ControleSegmentado noFlutuante compacto largo opcoes={['Auto', 'Baixo', 'Médio', 'Alto', 'Extra', 'Máx']} valor={0} />
  </div>
);

export const PequenoEDesabilitado = () => (
  <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
    <ControleSegmentado tamanho={28} opcoes={['Claude', 'Codex']} valor={1} />
    <ControleSegmentado desabilitado opcoes={['Opaco', 'Leve', 'Média', 'Forte']} valor={0} />
  </div>
);
