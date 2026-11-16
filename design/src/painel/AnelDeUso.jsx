import * as React from 'react';
import { MARCAS } from '../imagens.js';
import { corDaFaixa, cx } from '../util.js';

/**
 * O uso do plano de um serviço: o disco da marca com UM anel com fenda em volta, que mostra a janela
 * mais apertada (5 h ou semana). O arco vai na cor da marca até 69%, `ambar` de 70% a 89% e
 * `vermelho` a partir de 90%. Botão de 30 no disco opaco da peça, com fio e sombra curta, para
 * flutuar sobre a conversa. Com várias contas, a `inicial` da conta em uso num disquinho no canto.
 * **Moram no canto de cima à direita da janela, embaixo do ×, um em cima do outro** (14 da borda,
 * 60 do topo; regra do dono); a conversa passa por baixo. Serviço ausente: `apagado`, sem arco.
 * @param {AnelDeUsoProps} props
 */
export function AnelDeUso({ marca = 'claude', fracao = 0, inicial, apagado = false, dica, className, style, ...resto }) {
  const f = Math.max(0, Math.min(1, fracao));
  const arco = corDaFaixa(f, `var(--${marca})`);
  return (
    <button
      type="button"
      title={dica ?? `${marca === 'codex' ? 'Codex' : 'Claude'}: ${Math.round(f * 100)}% do plano`}
      aria-label={dica ?? `${marca === 'codex' ? 'Codex' : 'Claude'}: ${Math.round(f * 100)}% do plano`}
      className={cx('vt-uso', apagado && 'vt-apagado', className)}
      style={style}
      {...resto}
    >
      <span className="vt-anel" style={{ '--vt-f': apagado ? 0 : f, '--vt-arco': arco }} />
      <img src={MARCAS[marca]?.disco ?? MARCAS.claude.disco} alt="" />
      {inicial ? <span className="vt-inicial">{inicial}</span> : null}
    </button>
  );
}

/**
 * @typedef {object} AnelDeUsoProps
 * @property {'claude' | 'codex'} [marca] O serviço. Padrão "claude".
 * @property {number} [fracao] O uso da janela mais apertada, de 0 a 1.
 * @property {string} [inicial] Inicial da conta em uso ("P"), quando há várias contas.
 * @property {boolean} [apagado] Serviço ausente: disco cinza e esmaecido, sem arco.
 * @property {string} [dica] A dica; o padrão diz a porcentagem.
 * @property {(evento: import('react').MouseEvent<HTMLButtonElement>) => void} [onClick]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
