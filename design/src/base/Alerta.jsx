import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from './Icone.jsx';

const TONS = {
  info: { icone: 'info', cor: null, fundo: null, fio: null },
  atencao: { icone: 'triangle-alert', cor: 'var(--ambar)', fundo: 'var(--tinta-leve-ambar)', fio: 'var(--tinta-borda-leve-ambar)' },
  erro: { icone: 'circle-x', cor: 'var(--vermelho)', fundo: 'var(--tinta-leve-vermelho)', fio: 'var(--tinta-borda-leve-vermelho)' },
  pronto: {
    icone: 'circle-check',
    cor: 'var(--verde)',
    fundo: 'color-mix(in srgb, var(--verde) 6%, transparent)',
    fio: 'color-mix(in srgb, var(--verde) 18%, transparent)',
  },
};

/**
 * O alerta em linha, para o que precisa ficar na tela (aviso de painel, validação de formulário):
 * o ícone na cor do tom e o texto, no material leve do cartão de erro (a cor num véu de 6% e no fio
 * a 18%). Título opcional 14/20 em 600; texto 13/18 `tenue`; ação embaixo; × de 20 à direita.
 * **Compacto**: numa linha, sem caixa, ícone de 14 na cor e texto `metadado` `tenue`.
 * @param {AlertaProps} props
 */
export function Alerta({ tom = 'info', titulo, icone, acao, onFechar, compacto = false, children, className, style, ...resto }) {
  const t = TONS[tom] ?? TONS.info;
  if (compacto) {
    return (
      <div
        className={cx('vt-tipo-metadado', className)}
        style={{ display: 'flex', gap: 6, color: 'var(--tenue)', ...style }}
        {...resto}
      >
        <span style={{ color: t.cor ?? 'var(--tenue)', display: 'flex', height: 16, alignItems: 'center' }}>
          <Icone nome={icone ?? t.icone} tamanho={14} />
        </span>
        <span>{children}</span>
      </div>
    );
  }
  const vars = {};
  if (t.cor) vars['--vt-cor'] = t.cor;
  if (t.fundo) vars['--vt-fundo'] = t.fundo;
  if (t.fio) vars['--vt-fio'] = t.fio;
  return (
    <div role={tom === 'erro' ? 'alert' : 'status'} className={cx('vt-alerta', className)} style={{ ...vars, ...style }} {...resto}>
      <span className="vt-icone">
        <Icone nome={icone ?? t.icone} tamanho={16} />
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        {titulo ? <div className="vt-t">{titulo}</div> : null}
        {children ? <div className="vt-d">{children}</div> : null}
        {acao ? <div style={{ marginTop: 6, marginLeft: -8, display: 'flex', gap: 4 }}>{acao}</div> : null}
      </div>
      {onFechar ? (
        <button type="button" className="vt-botao vt-so-icone" title="Fechar" aria-label="Fechar" onClick={onFechar} style={{ width: 20, height: 20, borderRadius: 9 }}>
          <Icone nome="x" tamanho={14} />
        </button>
      ) : null}
    </div>
  );
}

/**
 * @typedef {object} AlertaProps
 * @property {'info' | 'atencao' | 'erro' | 'pronto'} [tom] "info" (padrão, neutro), "atencao" (âmbar), "erro" (vermelho) ou "pronto" (verde).
 * @property {import('react').ReactNode} [titulo] Título opcional em 600.
 * @property {string} [icone] Troca o ícone do tom (Lucide).
 * @property {import('react').ReactNode} [acao] Botões embaixo do texto (Botao tamanho "linha").
 * @property {() => void} [onFechar] Mostra o × de 20 à direita.
 * @property {boolean} [compacto] Numa linha, sem caixa.
 * @property {import('react').ReactNode} [children] O texto: o que aconteceu e o que fazer.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
