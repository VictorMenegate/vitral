import * as React from 'react';
import { cx } from '../util.js';

/**
 * A raiz de toda tela feita com o vitral-ui: liga o tema do app (só escuro), a fonte Inter 14/20, o
 * texto em `texto`, o antialiasing e o fundo. Sem ela, o texto solto cai na fonte e na cor do
 * navegador. `fundo="cartao"` (padrão) é o `cartao-opaco` das peças; `fundo="base"` é a base da
 * janela (o metal parado sob o véu azul-escuro), para montar a janela inteira; `fundo="nenhum"` só
 * liga a tipografia, para quem já está sobre uma peça.
 * @param {RaizVitralProps} props
 */
export function RaizVitral({ fundo = 'cartao', children, className, style, ...resto }) {
  return (
    <div className={cx('vt-raiz', fundo === 'base' && 'vt-base', fundo === 'nenhum' && 'vt-sem-fundo', className)} style={style} {...resto}>
      {children}
    </div>
  );
}

/**
 * @typedef {object} RaizVitralProps
 * @property {'cartao' | 'base' | 'nenhum'} [fundo] "cartao" (padrão, cartao-opaco), "base" (a base da janela, com o metal) ou "nenhum".
 * @property {import('react').ReactNode} [children] A tela.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
