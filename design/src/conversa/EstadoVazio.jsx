import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * O que aparece onde ainda não há nada. **Amplo** (o painel sem conversa): só o título em
 * `titulo-tela` (18/24, 600, `forte`) e o texto `tenue` até 420, sem disco em volta de ícone.
 * **Compacto** (listas, painéis, popovers): o ícone de 20 solto em `apagado`, o título 14/20 em 600
 * e o texto 13/18 até 300. Centrado, vão 8, 24 em cima e embaixo; ações opcionais centradas.
 * @param {EstadoVazioProps} props
 */
export function EstadoVazio({ titulo, descricao, icone, compacto = false, acoes, className, style, ...resto }) {
  return (
    <div className={cx('vt-vazio-amplo', compacto && 'vt-compacto', className)} style={style} {...resto}>
      {compacto && icone ? (
        <span style={{ color: 'var(--apagado)', display: 'flex' }}>
          <Icone nome={icone} tamanho={20} />
        </span>
      ) : null}
      <div className="vt-t">{titulo}</div>
      {descricao ? <div className="vt-d">{descricao}</div> : null}
      {acoes ? <div style={{ display: 'flex', gap: 8, justifyContent: 'center', marginTop: 4 }}>{acoes}</div> : null}
    </div>
  );
}

/**
 * @typedef {object} EstadoVazioProps
 * @property {import('react').ReactNode} titulo "Converse com este chat", "Nada encontrado".
 * @property {import('react').ReactNode} [descricao] O texto embaixo.
 * @property {string} [icone] Ícone Lucide de 20, só no compacto.
 * @property {boolean} [compacto] A variante pequena (listas, painéis, popovers).
 * @property {import('react').ReactNode} [acoes] Botões centrados embaixo.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
