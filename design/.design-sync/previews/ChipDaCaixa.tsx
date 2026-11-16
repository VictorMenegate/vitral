import { ChipDaCaixa } from 'vitral-ui';

const fileira = { display: 'flex', gap: 4, alignItems: 'center', flexWrap: 'wrap' } as const;

export const FaixaLarga = () => (
  <div style={fileira}>
    <ChipDaCaixa modo="sem-perguntar" seta />
    <ChipDaCaixa marca="claude" seta>Opus 5.5</ChipDaCaixa>
    <ChipDaCaixa esforco={4} seta>Auto · Extra</ChipDaCaixa>
  </div>
);

export const FaixaEstreita = () => (
  <div style={fileira}>
    <ChipDaCaixa modo="sem-perguntar" soIcone />
    <ChipDaCaixa marca="claude" soIcone>Opus 5.5</ChipDaCaixa>
    <ChipDaCaixa esforco={4} soIcone>Auto · Extra</ChipDaCaixa>
  </div>
);

export const ModosEModelos = () => (
  <div style={fileira}>
    <ChipDaCaixa marca="claude" aberto>Opus 5.5</ChipDaCaixa>
    <ChipDaCaixa modo="manual" />
    <ChipDaCaixa modo="planejar" />
    <ChipDaCaixa marca="codex" mudado>GPT-6.1</ChipDaCaixa>
    <ChipDaCaixa marca="dupla" mudado>Claude + Codex</ChipDaCaixa>
  </div>
);

export const EsforcoEspecial = () => (
  <div style={fileira}>
    <ChipDaCaixa ultrathink>Ultrathink</ChipDaCaixa>
    <ChipDaCaixa ultracode>Ultracode</ChipDaCaixa>
    <ChipDaCaixa esforco={0} desabilitado dica="Este modelo não usa esforço">Auto</ChipDaCaixa>
    <ChipDaCaixa marca="claude" repouso>Opus 5.5</ChipDaCaixa>
  </div>
);
