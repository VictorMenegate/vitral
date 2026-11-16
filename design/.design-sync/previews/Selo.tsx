import { Selo } from 'vitral-ui';

const fileira = { display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap' } as const;

export const Contagem = () => (
  <div style={fileira}>
    <Selo tipo="contagem">3</Selo>
    <Selo tipo="contagem" tom="espera">2</Selo>
    <Selo tipo="contagem" tom="destaque">12</Selo>
    <Selo tipo="contagem" tom="espera" solido>2</Selo>
    <Selo tipo="contagem">99+</Selo>
  </div>
);

export const Rotulo = () => (
  <div style={fileira}>
    <Selo tom="vermelho">1 falhou</Selo>
    <Selo tom="destaque">novo</Selo>
    <Selo>1M de contexto</Selo>
    <Selo tom="ultrathink" />
    <Selo tom="ultracode" />
  </div>
);

export const NumeroDoPainel = () => (
  <div style={fileira}>
    <Selo tipo="painel" ativo>1</Selo>
    <Selo tipo="painel">2</Selo>
    <Selo tipo="painel">3</Selo>
  </div>
);
