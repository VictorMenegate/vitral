import { Marca } from 'vitral-ui';

const fileira = { display: 'flex', gap: 16, alignItems: 'center' } as const;

export const Discos = () => (
  <div style={fileira}>
    <Marca marca="claude" tamanho={20} />
    <Marca marca="codex" tamanho={20} />
    <Marca marca="linear" tamanho={20} />
  </div>
);

export const Glifos = () => (
  <div style={fileira}>
    <Marca marca="claude" forma="glifo" tamanho={14} />
    <Marca marca="codex" forma="glifo" tamanho={14} />
    <Marca marca="claude" forma="glifo" tamanho={20} />
    <Marca marca="codex" forma="glifo" tamanho={20} />
  </div>
);
