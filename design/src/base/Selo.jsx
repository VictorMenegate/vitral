import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from './Icone.jsx';

const COR = {
  espera: 'var(--espera)',
  destaque: 'var(--destaque-texto)',
  vermelho: 'var(--vermelho)',
  verde: 'var(--verde)',
  ambar: 'var(--ambar)',
  violeta: 'var(--violeta)',
};
const BASE = { espera: 'espera', destaque: 'destaque', vermelho: 'vermelho', verde: 'verde', ambar: 'ambar', violeta: 'violeta' };

function fundoTingido(tom, tipo) {
  if (tom === 'vermelho') return 'var(--tinta-vermelho)';
  if (tom === 'ambar') return 'var(--tinta-ambar)';
  if (tom === 'destaque') return tipo === 'rotulo' ? 'color-mix(in srgb, var(--destaque) 18%, transparent)' : 'var(--tinta-destaque)';
  return `color-mix(in srgb, var(--${BASE[tom]}) 12%, transparent)`;
}

/**
 * O selo: a **contagem** de 18 (11 em 600, tabular; neutra no `traco`, tingida a 12% na cor do
 * estado, ou sólida e recortada em 16 quando fica sobre um botão), o **rótulo** de 20 ("1 falhou",
 * "novo", "1M de contexto", Ultrathink com o arco-íris, Ultracode em violeta) e o **número do
 * painel** de 18 (azul no painel ativo, neutro nos outros). Cor de estado sempre com a palavra.
 * @param {SeloProps} props
 */
export function Selo({ tipo = 'rotulo', tom = 'neutro', solido = false, ativo = false, icone, children, className, style, ...resto }) {
  if (tipo === 'painel') {
    return (
      <span className={cx('vt-numero-painel', ativo && 'vt-ativo', className)} style={style} {...resto}>
        {children}
      </span>
    );
  }
  if (tipo === 'contagem') {
    let cor;
    if (tom !== 'neutro') {
      cor = solido
        ? { background: `var(--${BASE[tom] ?? 'espera'})`, color: 'var(--sobre-cor)' }
        : { background: fundoTingido(tom, tipo), color: COR[tom] };
    }
    return (
      <span className={cx('vt-selo', 'vt-contagem', solido && 'vt-recortado', className)} style={{ ...cor, ...style }} {...resto}>
        {children}
      </span>
    );
  }
  // rótulo
  if (tom === 'ultrathink') {
    return (
      <span className={cx('vt-selo', className)} style={{ background: 'var(--grupo)', ...style }} {...resto}>
        <span className="vt-arco-iris">{children ?? 'Ultrathink'}</span>
      </span>
    );
  }
  if (tom === 'ultracode') {
    return (
      <span className={cx('vt-selo', className)} style={{ background: 'var(--grupo)', color: 'var(--violeta)', ...style }} {...resto}>
        <Icone nome={icone ?? 'workflow'} tamanho={12} />
        {children ?? 'Ultracode'}
      </span>
    );
  }
  const neutro = tom === 'neutro';
  return (
    <span
      className={cx('vt-selo', neutro && 'vt-neutro', className)}
      style={neutro ? style : { background: fundoTingido(tom, tipo), color: COR[tom], ...style }}
      {...resto}
    >
      {icone ? <Icone nome={icone} tamanho={12} /> : null}
      {children}
    </span>
  );
}

/**
 * @typedef {object} SeloProps
 * @property {'rotulo' | 'contagem' | 'painel'} [tipo] "rotulo" (padrão, 20 de altura), "contagem" (18, número) ou "painel" (o número do painel, 18).
 * @property {'neutro' | 'espera' | 'destaque' | 'vermelho' | 'verde' | 'ambar' | 'violeta' | 'ultrathink' | 'ultracode'} [tom] A cor; padrão "neutro". "espera" é o que precisa de você; "ultrathink" e "ultracode" só no rótulo.
 * @property {boolean} [solido] Contagem sólida na cor, recortada (16) para ficar sobre um botão.
 * @property {boolean} [ativo] Número do painel ativo (azul).
 * @property {string} [icone] Ícone de 12 antes do texto (só no rótulo).
 * @property {import('react').ReactNode} [children] O número ou a palavra.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
