import { Icone } from 'vitral-ui';

const fileira = { display: 'flex', gap: 14, alignItems: 'center', flexWrap: 'wrap', color: 'var(--tenue)' } as const;

export const Ferramentas = () => (
  <div style={fileira}>
    <Icone nome="book-open" tamanho={16} />
    <Icone nome="file-pen-line" tamanho={16} />
    <Icone nome="file-plus" tamanho={16} />
    <Icone nome="terminal" tamanho={16} />
    <Icone nome="text-search" tamanho={16} />
    <Icone nome="folder-search" tamanho={16} />
    <Icone nome="globe" tamanho={16} />
    <Icone nome="bot" tamanho={16} />
    <Icone nome="workflow" tamanho={16} />
    <Icone nome="list-checks" tamanho={16} />
    <Icone nome="brain" tamanho={16} />
  </div>
);

export const Modos = () => (
  <div style={fileira}>
    <Icone nome="escudo-cheio" tamanho={16} />
    <Icone nome="file-pen" tamanho={16} />
    <Icone nome="map" tamanho={16} />
    <Icone nome="wand-sparkles" tamanho={16} />
    <Icone nome="shield-off" tamanho={16} cor="var(--ambar)" />
  </div>
);

export const Tamanhos = () => (
  <div style={{ ...fileira, alignItems: 'flex-end' }}>
    <Icone nome="chevron-right" tamanho={12} />
    <Icone nome="copy" tamanho={14} />
    <Icone nome="plus" tamanho={16} />
    <Icone nome="message-circle" tamanho={18} />
    <Icone nome="search" tamanho={20} />
  </div>
);

export const CorDeSinal = () => (
  <div style={fileira}>
    <Icone nome="terminal" tamanho={16} cor="var(--vermelho)" />
    <Icone nome="key-round" tamanho={16} cor="var(--espera)" />
    <Icone nome="circle-check" tamanho={16} cor="var(--verde)" />
    <Icone nome="workflow" tamanho={16} cor="var(--violeta)" />
    <Icone nome="triangle-alert" tamanho={16} cor="var(--ambar)" />
  </div>
);
