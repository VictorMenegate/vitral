import * as React from 'react';
import { cx } from '../util.js';

/**
 * O desenho único de tecla de atalho do app (dicas, menus, rodapés de popover, a busca): cada tecla
 * separada, 18 de altura, sem borda, fundo claro a 8% em relevo (`aresta-de-tecla`) e a letra em mono
 * 10 `tenue`. Com `rotulo`, o texto curto vem depois em `metadado` `tenue` ("Shift Tab troca").
 * @param {TeclasProps} props
 */
export function Teclas({ teclas, rotulo, className, style, ...resto }) {
  const grupo = (
    <span className={cx('vt-teclas', !rotulo && className)} style={rotulo ? undefined : style} {...(rotulo ? {} : resto)}>
      {teclas.map((t, i) => (
        <span key={i} className="vt-tecla">
          {t}
        </span>
      ))}
    </span>
  );
  if (!rotulo) return grupo;
  return (
    <span
      className={cx('vt-tipo-metadado', className)}
      style={{ display: 'inline-flex', alignItems: 'center', gap: 6, color: 'var(--tenue)', ...style }}
      {...resto}
    >
      {grupo}
      <span>{rotulo}</span>
    </span>
  );
}

/**
 * @typedef {object} TeclasProps
 * @property {string[]} teclas As teclas, cada uma separada: ["Ctrl", "K"], ["Shift", "Tab"], ["Esc"].
 * @property {import('react').ReactNode} [rotulo] O que o atalho faz, depois das teclas ("troca", "usa", "fecha").
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
