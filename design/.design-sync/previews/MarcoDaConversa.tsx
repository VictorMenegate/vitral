import { MarcoDaConversa } from 'vitral-ui';

const coluna = { display: 'flex', flexDirection: 'column', gap: 16, maxWidth: 620 } as const;

export const Marcos = () => (
  <div style={coluna}>
    <MarcoDaConversa icone="circle-pause">O Vitral fechou no meio do turno</MarcoDaConversa>
    <MarcoDaConversa icone="circle-pause">Turno interrompido</MarcoDaConversa>
    <MarcoDaConversa icone="refresh-cw">Sessão nova, com um resumo da anterior</MarcoDaConversa>
  </div>
);

export const DiaEInformacao = () => (
  <div style={coluna}>
    <MarcoDaConversa tipo="dia">Hoje</MarcoDaConversa>
    <MarcoDaConversa tipo="info" icone="shuffle">Daqui em diante quem responde é o Codex.</MarcoDaConversa>
    <MarcoDaConversa tipo="info" icone="rotate-cw">Nova tentativa com a conta Principal.</MarcoDaConversa>
  </div>
);
