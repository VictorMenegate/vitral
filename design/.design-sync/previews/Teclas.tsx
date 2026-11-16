import { Teclas } from 'vitral-ui';

export const Atalhos = () => (
  <div style={{ display: 'flex', gap: 16, alignItems: 'center', flexWrap: 'wrap' }}>
    <Teclas teclas={['Ctrl', 'K']} />
    <Teclas teclas={['Shift', 'Tab']} />
    <Teclas teclas={['Esc']} />
    <Teclas teclas={['↑', '↓']} />
    <Teclas teclas={['Ctrl', 'Enter']} />
    <Teclas teclas={['?']} />
  </div>
);

export const ComRotulo = () => (
  <div style={{ display: 'flex', gap: 12, alignItems: 'center', flexWrap: 'wrap' }}>
    <Teclas teclas={['Shift', 'Tab']} rotulo="troca" />
    <Teclas teclas={['Espaço']} rotulo="Ultrathink" />
    <Teclas teclas={['Enter']} rotulo="fecha" />
  </div>
);
