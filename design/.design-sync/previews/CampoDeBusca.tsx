import { BotaoRedondo, CampoDeBusca, Metal } from 'vitral-ui';

const palco = { display: 'flex', flexDirection: 'column', gap: 12, padding: 12, borderRadius: 12 } as const;

export const VazioEComFoco = () => (
  <Metal tipo="base" style={palco}>
    <CampoDeBusca style={{ width: 260 }} />
    <CampoDeBusca focada valor="pesagem" style={{ width: 260 }} />
  </Metal>
);

export const NoPeDaLista = () => (
  <Metal tipo="base" style={{ ...palco, flexDirection: 'row', alignItems: 'center', gap: 10 }}>
    <CampoDeBusca alta teclas={null} style={{ width: 220 }} />
    <BotaoRedondo tipo="metal" tamanho={36} icone="plus" dica="Novo chat" style={{ width: 40, height: 40, borderRadius: 18 }} />
  </Metal>
);

export const Plana = () => (
  <div style={{ width: 360, background: 'var(--flutuante)', borderRadius: 20, padding: 6 }}>
    <CampoDeBusca plana placeholder="Buscar modelo" />
  </div>
);
