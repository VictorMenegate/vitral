import { AnelDeContexto } from 'vitral-ui';

const fileira = { display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' } as const;

export const Faixas = () => (
  <div style={fileira}>
    <AnelDeContexto fracao={0.01} />
    <AnelDeContexto fracao={0.52} />
    <AnelDeContexto fracao={0.74} />
    <AnelDeContexto fracao={0.93} />
  </div>
);

export const Carregando = () => (
  <div style={{ ...fileira, gap: 16 }}>
    <AnelDeContexto carregando />
    <AnelDeContexto carregando tamanho={12} />
    <AnelDeContexto carregando tamanho={16} />
  </div>
);

export const SoOAnel = () => (
  <div style={{ ...fileira, gap: 16 }}>
    <AnelDeContexto fracao={0.38} tamanho={28} porcentagem={false} />
    <AnelDeContexto fracao={0.81} tamanho={36} porcentagem={false} />
  </div>
);
