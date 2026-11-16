import * as React from 'react';
import { cx } from '../util.js';

/**
 * O liga e desliga (o `Switch` do kit): 36 x 20 com o pino de 16. Desligado, trilho branco a 12% e
 * pino em `texto`; ligado, trilho `destaque` chapado. Desabilitado a 0,45, com o motivo escrito
 * embaixo do título da linha. Mora na linha de opção (GrupoDeOpcoes), onde a linha inteira alterna.
 * @param {InterruptorProps} props
 */
export function Interruptor({ ligado = false, desabilitado = false, onChange, rotulo, className, style, ...resto }) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={ligado}
      aria-label={rotulo}
      disabled={desabilitado || undefined}
      onClick={onChange ? () => onChange(!ligado) : undefined}
      className={cx('vt-interruptor', ligado && 'vt-ligado', desabilitado && 'vt-desabilitado', className)}
      style={style}
      {...resto}
    />
  );
}

/**
 * @typedef {object} InterruptorProps
 * @property {boolean} [ligado] Ligado (trilho azul).
 * @property {boolean} [desabilitado] A 0,45.
 * @property {(ligado: boolean) => void} [onChange] Recebe o valor novo.
 * @property {string} [rotulo] Nome acessível, quando não há título ao lado.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
