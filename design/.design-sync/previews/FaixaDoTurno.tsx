import { FaixaDoTurno } from 'vitral-ui';

const coluna = { display: 'flex', flexDirection: 'column', gap: 6, maxWidth: 600 } as const;

export const ParadaQuePedeRetomar = () => (
  <div style={coluna}>
    <FaixaDoTurno icone="pause" tom="espera" acao="Retomar" iconeAcao="play" dica="Parada: o Vitral fechou no meio do turno">
      Parada · Vitral fechou
    </FaixaDoTurno>
  </div>
);

export const Falhou = () => (
  <div style={coluna}>
    <FaixaDoTurno icone="gauge" tom="vermelho" acao="Tentar de novo" iconeAcao="rotate-cw">Limite de uso do Codex · volta 12:30</FaixaDoTurno>
    <FaixaDoTurno icone="key-round" tom="espera" acao="Tentar de novo" iconeAcao="rotate-cw">Sem login no Claude Code</FaixaDoTurno>
    <FaixaDoTurno icone="wifi-off" acao="Tentar de novo" iconeAcao="rotate-cw">Sem conexão</FaixaDoTurno>
  </div>
);

export const ConversaFria = () => (
  <div style={coluna}>
    <FaixaDoTurno icone="clock" acao="Compactar" iconeAcao="minimize">Relê 182 mil tokens · parada há 3 h</FaixaDoTurno>
    <FaixaDoTurno icone="folder-x" tom="vermelho">A pasta ~/eita/antiga não foi encontrada</FaixaDoTurno>
  </div>
);
