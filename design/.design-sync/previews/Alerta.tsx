import { Alerta, Botao } from 'vitral-ui';

const coluna = { display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 560 } as const;

export const Tons = () => (
  <div style={coluna}>
    <Alerta tom="erro" titulo="Pasta não encontrada" onFechar={() => {}}>
      Não estou conseguindo gravar. Nada novo começa até voltar a gravar.
    </Alerta>
    <Alerta tom="atencao">O uso do plano passou de 70% na janela de 5 h.</Alerta>
    <Alerta tom="info">A conta Pessoal está em uso neste chat.</Alerta>
  </div>
);

export const ComAcao = () => (
  <div style={coluna}>
    <Alerta tom="pronto" titulo="Atualização baixada" acao={<Botao tamanho="linha" icone="refresh-cw">Reiniciar agora</Botao>}>
      O Vitral 0.8.0 entra na próxima vez que abrir.
    </Alerta>
  </div>
);

export const Compacto = () => (
  <div style={coluna}>
    <Alerta tom="pronto" compacto>Compacto: o ícone na cor e o texto 12 tênue, sem caixa.</Alerta>
    <Alerta tom="atencao" compacto>A chave expira em 3 dias.</Alerta>
  </div>
);
