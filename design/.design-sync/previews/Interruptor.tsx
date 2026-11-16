import { GrupoDeOpcoes, Interruptor } from 'vitral-ui';

export const Estados = () => (
  <div style={{ display: 'flex', gap: 16, alignItems: 'center' }}>
    <Interruptor rotulo="Desligado" />
    <Interruptor ligado rotulo="Ligado" />
    <Interruptor desabilitado rotulo="Desligado e desabilitado" />
    <Interruptor ligado desabilitado rotulo="Ligado e desabilitado" />
  </div>
);

export const NaLinhaDeOpcao = () => (
  <GrupoDeOpcoes
    style={{ maxWidth: 560 }}
    opcoes={[
      {
        titulo: 'Mostrar o uso do plano no canto da janela',
        descricao: 'Os anéis ficam no canto de cima, à direita. Desligado, o uso fica só em Contas do Claude.',
        controle: <Interruptor ligado />,
      },
    ]}
  />
);
