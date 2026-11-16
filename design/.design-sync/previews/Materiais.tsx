import { Materiais, Metal } from 'vitral-ui';

const legenda = { fontFamily: 'var(--font-mono)', fontSize: 11, lineHeight: '14px', color: 'var(--apagado)' } as const;
const item = { display: 'flex', flexDirection: 'column', gap: 6 } as const;

export const VidroCascaPoco = () => (
  <Metal tipo="base" style={{ display: 'flex', gap: 16, padding: 16, borderRadius: 12, flexWrap: 'wrap', alignItems: 'flex-start' }}>
    <div style={{ ...item, width: 180 }}>
      <Materiais material="vidro" style={{ height: 90 }} />
      <span style={legenda}>VIDRO: A PEÇA</span>
    </div>
    <div style={{ ...item, width: 180 }}>
      <Materiais material="casca" style={{ height: 90 }} />
      <span style={legenda}>CASCA: MENUS E POPOVERS</span>
    </div>
    <div style={{ ...item, width: 120 }}>
      <Materiais material="poco" style={{ height: 32 }} />
      <span style={legenda}>POÇO: O CAVADO</span>
    </div>
    <div style={{ ...item, width: 120 }}>
      <Materiais material="tecla" style={{ height: 32 }} />
      <span style={legenda}>TECLA: O RELEVO</span>
    </div>
  </Metal>
);

export const DegrausEFio = () => (
  <Metal tipo="base" style={{ display: 'flex', flexDirection: 'column', gap: 12, padding: 16, borderRadius: 12, maxWidth: 460 }}>
    <div style={{ display: 'flex', gap: 6, height: 60 }}>
      <Materiais material="poco" degrau="cartao-opaco" style={{ flex: 1, boxShadow: 'none' }} />
      <Materiais material="poco" degrau="cartao-elevado" style={{ flex: 1, boxShadow: 'none' }} />
      <Materiais material="poco" degrau="flutuante" style={{ flex: 1, boxShadow: 'none' }} />
      <Materiais material="poco" degrau="preto" style={{ flex: 1, boxShadow: 'none' }} />
    </div>
    <span style={legenda}>DEGRAUS: CARTÃO, ELEVADO, FLUTUANTE E O PRETO DE DENTRO</span>
    <Materiais material="fio" />
    <span style={legenda}>O FIO DE PONTOS NO LUGAR DE TODA LINHA CHEIA</span>
  </Metal>
);
