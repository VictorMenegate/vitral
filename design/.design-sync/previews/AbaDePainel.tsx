import { AbaDePainel } from 'vitral-ui';

const abas = { display: 'flex', alignItems: 'center', gap: 6 } as const;

export const PainelAtivo = () => (
  <div style={abas}>
    <AbaDePainel quadrada titulo="Alertas de cupons" />
    <AbaDePainel quadrada titulo="WMS" estado="espera" />
    <AbaDePainel ativa titulo="Nova conversa" painel={1} painelAtivo fechar />
  </div>
);

export const PainelDeFundo = () => (
  <div style={abas}>
    <AbaDePainel ativa titulo="WMS" estado="rodando" painel={2} />
    <AbaDePainel titulo="Projeto Vitral com integração GitHub" />
  </div>
);

export const SobOPonteiroEArrastando = () => (
  <div style={abas}>
    <AbaDePainel titulo="Pesagem de caixas" estado="nova" sobOPonteiro fechar larguraMaxima={220} />
    <AbaDePainel ativa titulo="Arrastando…" arrastando larguraMaxima={200} />
    <AbaDePainel quadrada titulo="Montagem da cozinha" estado="erro" />
  </div>
);
