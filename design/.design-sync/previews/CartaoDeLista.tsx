import { CartaoDeLista, Metal } from 'vitral-ui';

const lista = { display: 'flex', flexDirection: 'column', gap: 2, padding: 8, width: 312, borderRadius: 12 } as const;

export const EstadosDaLista = () => (
  <Metal tipo="base" style={lista}>
    <CartaoDeLista titulo="Projeto Vitral com integração GitHub" estado="espera" painel={3} />
    <CartaoDeLista titulo="WMS" estado="rodando" selecionado painel={2} painelAtivo />
    <CartaoDeLista titulo="Nova conversa" abertoEmOutro painel={1} />
    <CartaoDeLista titulo="Alertas de cupons" estado="nova" naoLido hora="2 h" />
    <CartaoDeLista titulo="Pesagem de caixas" estado="erro" hora="3 d" />
    <CartaoDeLista titulo="Eita Gestão" fixado hora="4 d" />
  </Metal>
);

export const PonteiroERealce = () => (
  <Metal tipo="base" style={lista}>
    <CartaoDeLista titulo="Tagueamento de vendas" hora="4 d" comPonteiro />
    <CartaoDeLista titulo="Montagem da cozinha" estado="rodando" rodandoHa="02:13" realce />
    <CartaoDeLista titulo="Relatório semanal do site" hora="5 d" />
  </Metal>
);
