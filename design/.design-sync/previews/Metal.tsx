import { BotaoRedondo, Metal } from 'vitral-ui';

const legenda = { fontFamily: 'var(--font-mono)', fontSize: 11, lineHeight: '14px', color: 'var(--apagado)' } as const;
const item = { display: 'flex', flexDirection: 'column', gap: 6, width: 200 } as const;

export const ChapaEBase = () => (
  <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap' }}>
    <div style={item}>
      <Metal tipo="chapa" style={{ height: 110, borderRadius: 12 }} />
      <span style={legenda}>A CHAPA: CINCO BOLHAS, GRÃO E VINHETA</span>
    </div>
    <div style={item}>
      <Metal tipo="base" style={{ height: 110, borderRadius: 12 }} />
      <span style={legenda}>A BASE: O VÉU AZUL-ESCURO POR CIMA</span>
    </div>
    <div style={{ ...item, width: 260 }}>
      <Metal tipo="luz" style={{ height: 110, borderRadius: '0 0 16px 16px', background: 'var(--cartao-opaco)' }} />
      <span style={legenda}>A LUZ DE BAIXO DO PAINEL ATIVO</span>
    </div>
  </div>
);

export const FichaPlacaTeclaAro = () => (
  <div style={{ display: 'flex', gap: 16, flexWrap: 'wrap', alignItems: 'flex-start' }}>
    <div style={item}>
      <Metal tipo="ficha" style={{ height: 40, display: 'flex', alignItems: 'center', padding: '0 12px', fontWeight: 500 }}>
        Padrão
      </Metal>
      <span style={legenda}>A FICHA DO ESCOLHIDO</span>
    </div>
    <div style={item}>
      <Metal tipo="placa" style={{ height: 52, borderRadius: '19px 19px 0 0', display: 'flex', alignItems: 'center', padding: '0 16px', fontWeight: 600, color: 'var(--forte)' }}>
        Vitral
      </Metal>
      <span style={legenda}>A PLACA DO MENU</span>
    </div>
    <div style={{ ...item, width: 240 }}>
      <div style={{ display: 'flex', gap: 16, alignItems: 'center', height: 52 }}>
        <BotaoRedondo tipo="metal" tamanho={36} icone="plus" dica="Novo chat" />
        <Metal tipo="aro" style={{ width: 90, height: 36 }} />
      </div>
      <span style={legenda}>A TECLA DE METAL E O ARO DE 2 PX</span>
    </div>
  </div>
);
