import { PopoverDeEsforco } from 'vitral-ui';

export const NoAuto = () => <PopoverDeEsforco nivel={0} />;

export const AltoComUltrathink = () => <PopoverDeEsforco nivel={3} frase="Pensa mais antes de responder." ultrathink />;

export const UltracodeIndisponivel = () => (
  <PopoverDeEsforco nivel={4} frase="O máximo que este modelo aceita." ultracodeIndisponivel="Só no Claude Code" largura={320} />
);
