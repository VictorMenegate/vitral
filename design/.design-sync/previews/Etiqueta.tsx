import { Etiqueta } from 'vitral-ui';

const fileira = { display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' } as const;

export const Fatos = () => (
  <div style={fileira}>
    <Etiqueta icone="folder">~/eita</Etiqueta>
    <Etiqueta icone="git-branch">main</Etiqueta>
    <Etiqueta icone="clock">00:04</Etiqueta>
    <Etiqueta>revisar</Etiqueta>
  </div>
);

export const TomEAltura = () => (
  <div style={fileira}>
    <Etiqueta tom="ambar">direto</Etiqueta>
    <Etiqueta baixa>baixa</Etiqueta>
    <Etiqueta comFundo>com fundo</Etiqueta>
  </div>
);
