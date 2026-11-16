import { CaixaDeMensagem, ChipDaCaixa, Materiais } from 'vitral-ui';

const peca = { width: 640 } as const;

export const PainelAtivoComFoco = () => (
  <Materiais material="vidro" style={peca}>
    <CaixaDeMensagem
      ativa
      foco
      valor="ola painel dois"
      contexto={0.01}
      chips={
        <>
          <ChipDaCaixa modo="sem-perguntar" seta />
          <ChipDaCaixa marca="claude" seta>Opus 5.5</ChipDaCaixa>
          <ChipDaCaixa esforco={4} seta>Auto · Extra</ChipDaCaixa>
        </>
      }
    />
  </Materiais>
);

export const PainelDeFundoVazio = () => (
  <Materiais material="vidro" style={peca}>
    <CaixaDeMensagem
      repouso
      contexto={0.52}
      chips={
        <>
          <ChipDaCaixa modo="sem-perguntar" soIcone />
          <ChipDaCaixa marca="claude" soIcone repouso>Opus 5.5</ChipDaCaixa>
          <ChipDaCaixa esforco={4} soIcone repouso>Auto · Extra</ChipDaCaixa>
        </>
      }
    />
  </Materiais>
);

export const Rodando = () => (
  <Materiais material="vidro" style={peca}>
    <CaixaDeMensagem
      rodando
      tempo="00:03"
      agentes={1}
      contexto={0.01}
      chips={
        <>
          <ChipDaCaixa modo="sem-perguntar" soIcone />
          <ChipDaCaixa marca="claude" soIcone>Opus 5.5</ChipDaCaixa>
          <ChipDaCaixa esforco={4} soIcone>Auto · Extra</ChipDaCaixa>
        </>
      }
    />
  </Materiais>
);
