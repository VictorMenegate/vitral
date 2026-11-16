import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * Um fato do turno numa linha só, no meio de dois traços de 1 px em `linha`: o ícone de 12 e o texto
 * `metadado` `tenue` ("Turno interrompido", "O Vitral fechou no meio do turno", "Sessão nova, com um
 * resumo da anterior"). **dia** é o separador de dia ("Hoje"), com o nome em 600. **info** é a linha
 * de informação (troca de conta ou de agente, nova tentativa): linha de log de 32 com ícone de 14 e
 * texto 13/18, tudo `tenue`. Sempre impessoal e sem instrução.
 * @param {MarcoDaConversaProps} props
 */
export function MarcoDaConversa({ tipo = 'marco', icone, children, className, style, ...resto }) {
  if (tipo === 'info') {
    return (
      <div
        className={className}
        style={{ display: 'flex', alignItems: 'flex-start', gap: 8, minHeight: 32, padding: '4px 12px', color: 'var(--tenue)', fontSize: 13, lineHeight: '18px', ...style }}
        {...resto}
      >
        <Icone nome={icone ?? 'info'} tamanho={14} style={{ marginTop: 2 }} />
        <span>{children}</span>
      </div>
    );
  }
  return (
    <div role="separator" className={cx('vt-marco', tipo === 'dia' && 'vt-dia', className)} style={style} {...resto}>
      <span>
        {icone && tipo !== 'dia' ? <Icone nome={icone} tamanho={12} /> : null}
        {children}
      </span>
    </div>
  );
}

/**
 * @typedef {object} MarcoDaConversaProps
 * @property {'marco' | 'dia' | 'info'} [tipo] "marco" (padrão), "dia" (separador de dia) ou "info" (linha de informação).
 * @property {string} [icone] Ícone Lucide: circle-pause, refresh-cw no marco; shuffle, gauge, rotate-cw, info na linha de informação.
 * @property {import('react').ReactNode} children O fato, impessoal.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
