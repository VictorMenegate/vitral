import { Logo, Metal } from 'vitral-ui';

const legenda = { fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--apagado)' } as const;
const item = { display: 'flex', flexDirection: 'column', alignItems: 'center', gap: 8 } as const;

export const Tamanhos = () => (
  <Metal tipo="base" style={{ display: 'flex', gap: 24, alignItems: 'flex-end', padding: 16, borderRadius: 12 }}>
    <div style={item}><Logo tamanho={128} /><span style={legenda}>128</span></div>
    <div style={item}><Logo tamanho={64} /><span style={legenda}>64</span></div>
    <div style={item}><Logo tamanho={44} /><span style={legenda}>44, O BOTÃO</span></div>
    <div style={item}><Logo tamanho={20} /><span style={legenda}>20, MENU</span></div>
  </Metal>
);

export const SobreOMetal = () => (
  <div style={{ display: 'flex', gap: 24, alignItems: 'center' }}>
    <Logo forma="metal" tamanho={64} />
    <Logo forma="metal" tamanho={44} />
  </div>
);
