import * as React from 'react';
import { cx } from '../util.js';

const DEGRAU = {
  'cartao-opaco': 'var(--cartao-opaco)',
  'cartao-elevado': 'var(--cartao-elevado)',
  flutuante: 'var(--flutuante)',
  preto: 'var(--preto)',
};

/**
 * Os materiais do Vitral, feitos só de luz e sombra de 1 px por dentro. **vidro**: a peça da janela
 * (a conversa, os painéis da direita): canto 16, `sombra-flutuante`, fio claro só nos lados, sombra
 * na base e um brilho de 3% no topo, **sem a bordinha de cima** (regra do dono). **casca**: o
 * flutuante dos menus e popovers (`flutuante` opaco, canto 20, `sombra-popover`). **poco**: o que é
 * cavado (`recuo` com `aresta-de-poco`: busca, trilho, contador). **tecla**: o que está em relevo
 * (`hover` com `aresta-de-tecla`). **fio**: o fio de pontos (2 aceso, 2 apagado) que substitui toda
 * linha cheia. `degrau` troca o fundo do vidro pelos degraus do grafite. Opacos por padrão.
 * @param {MateriaisProps} props
 */
export function Materiais({ material = 'vidro', degrau, children, className, style, ...resto }) {
  if (material === 'fio') {
    return <span role="separator" className={cx('vt-fio-pontos', className)} style={{ display: 'block', flex: 'none', width: '100%', ...style }} {...resto} />;
  }
  const base = {
    vidro: { classe: 'vt-peca', estilo: null },
    casca: { classe: 'vt-pop', estilo: null },
    poco: { classe: '', estilo: { background: 'var(--recuo)', boxShadow: 'var(--aresta-de-poco)', borderRadius: 'var(--raio-controle)' } },
    tecla: { classe: '', estilo: { background: 'var(--hover)', boxShadow: 'var(--aresta-de-tecla)', borderRadius: 'var(--raio-controle)' } },
  }[material] ?? { classe: 'vt-peca', estilo: null };
  return (
    <div
      className={cx(base.classe, className)}
      style={{ position: 'relative', ...base.estilo, ...(degrau ? { background: DEGRAU[degrau] } : null), ...style }}
      {...resto}
    >
      {children}
    </div>
  );
}

/**
 * @typedef {object} MateriaisProps
 * @property {'vidro' | 'casca' | 'poco' | 'tecla' | 'fio'} [material] "vidro" (padrão, a peça da janela), "casca" (menus e popovers), "poco", "tecla" ou "fio" (o separador de pontos).
 * @property {'cartao-opaco' | 'cartao-elevado' | 'flutuante' | 'preto'} [degrau] Troca o fundo pelo degrau do grafite.
 * @property {import('react').ReactNode} [children]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style] Dê o tamanho e o padding por aqui.
 */
