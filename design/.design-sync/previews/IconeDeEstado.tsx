import { IconeDeEstado } from 'vitral-ui';

const fileira = { display: 'flex', gap: 20, alignItems: 'center', flexWrap: 'wrap' } as const;

export const OIconeDoChat = () => (
  <div style={fileira}>
    <IconeDeEstado estado="parado" />
    <IconeDeEstado estado="rodando" />
    <IconeDeEstado estado="espera" />
    <IconeDeEstado estado="nova" />
    <IconeDeEstado estado="erro" />
  </div>
);

export const NaAba16 = () => (
  <div style={fileira}>
    <IconeDeEstado estado="parado" tamanho={16} />
    <IconeDeEstado estado="rodando" tamanho={16} />
    <IconeDeEstado estado="espera" tamanho={16} />
    <IconeDeEstado estado="nova" tamanho={16} />
    <IconeDeEstado estado="erro" tamanho={16} />
  </div>
);

export const Pontos = () => (
  <div style={fileira}>
    <IconeDeEstado forma="ponto" estado="erro" />
    <IconeDeEstado forma="ponto" estado="espera" />
    <IconeDeEstado forma="ponto" estado="nova" />
    <IconeDeEstado forma="ponto" estado="rodando" />
  </div>
);

export const Pilulas = () => (
  <div style={{ ...fileira, gap: 8 }}>
    <IconeDeEstado forma="pilula" estado="rodando" detalhe="2 min 13 s" />
    <IconeDeEstado forma="pilula" estado="espera" />
    <IconeDeEstado forma="pilula" estado="pronto" />
    <IconeDeEstado forma="pilula" estado="erro" />
    <IconeDeEstado forma="pilula" estado="parado" />
  </div>
);
