import { MensagemRecebida } from 'vitral-ui';

const coluna = { display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 620 } as const;

export const DoVitralFechada = () => (
  <div style={coluna}>
    <MensagemRecebida de="Vitral">Antes de encerrar: reveja o que decidiu nesta conversa e anote na memória da equipe.</MensagemRecebida>
  </div>
);

export const DeOutroChatAberta = () => (
  <div style={coluna}>
    <MensagemRecebida de="Pesagem de caixas na expedição" aberta>
      A balança da bancada 2 voltou a responder. Pode seguir com o lote 418.
    </MensagemRecebida>
  </div>
);
