import * as React from 'react';
import { MARCAS } from '../imagens.js';
import { cx } from '../util.js';

/**
 * A marca de um serviço (Claude, Codex, Linear) pelo arquivo do app, nunca recolorida. O **disco**
 * cheio vai nas listas, popovers, na dupla e no anel de uso; o **glifo** solto vai nos chips da caixa
 * e das Configurações.
 * @param {MarcaProps} props
 */
export function Marca({ marca, forma = 'disco', tamanho = 18, className, style, ...resto }) {
  const m = MARCAS[marca] ?? MARCAS.claude;
  return (
    <img
      src={forma === 'glifo' ? m.glifo : m.disco}
      alt={marca === 'codex' ? 'Codex' : marca === 'linear' ? 'Linear' : 'Claude'}
      width={tamanho}
      height={tamanho}
      className={cx('vt-marca', className)}
      style={{ width: tamanho, height: tamanho, flex: 'none', display: 'inline-block', ...style }}
      {...resto}
    />
  );
}

/**
 * @typedef {object} MarcaProps
 * @property {'claude' | 'codex' | 'linear'} marca O serviço.
 * @property {'disco' | 'glifo'} [forma] "disco" (padrão) ou "glifo" (o símbolo solto, nos chips).
 * @property {number} [tamanho] Lado em px. Padrão 18 (o disco no anel de uso); 14 nos chips; 20 nas listas.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
