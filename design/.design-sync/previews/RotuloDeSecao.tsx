import { Metal, RotuloDeSecao } from 'vitral-ui';

export const NaLista = () => (
  <Metal tipo="base" style={{ display: 'flex', flexDirection: 'column', gap: 16, padding: 12, width: 312, borderRadius: 12 }}>
    <RotuloDeSecao contagem={2} tomContagem="espera">Paradas</RotuloDeSecao>
    <RotuloDeSecao contagem={12} acao="plus" dicaAcao="Nova pasta">Pastas</RotuloDeSecao>
    <RotuloDeSecao recolhivel aberto={false}>Arquivados</RotuloDeSecao>
  </Metal>
);

export const NoPopoverDeEscolha = () => (
  <div style={{ width: 400, padding: 12, background: 'var(--flutuante)', borderRadius: 20 }}>
    <RotuloDeSecao fio teclas={['Shift', 'Tab']} rotuloTeclas="troca">Conta do Claude</RotuloDeSecao>
  </div>
);
