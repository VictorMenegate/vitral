import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from './Icone.jsx';

/**
 * A etiqueta de fio: um fato curto em mono 11 numa pílula de contorno (a pasta, o ramo, "revisar",
 * "saída", o relógio do turno). Não é botão nem selo de contagem: é o que um aparelho escreveria do
 * lado do mostrador. Fio `borda-forte`, texto `tenue`, ícone de 11 em `apagado` antes do texto.
 * @param {EtiquetaProps} props
 */
export function Etiqueta({ icone, tom, baixa = false, comFundo = false, children, className, style, ...resto }) {
  const cor = tom ? { borderColor: `var(--tinta-borda-${tom})`, color: `var(--${tom})` } : null;
  const altura = baixa ? { height: 16, borderRadius: 7 } : null;
  const fundo = comFundo ? { background: 'var(--recuo)' } : null;
  return (
    <span className={cx('vt-etiqueta', className)} style={{ ...cor, ...altura, ...fundo, ...style }} {...resto}>
      {icone ? (
        <span style={{ color: 'var(--apagado)', display: 'flex' }}>
          <Icone nome={icone} tamanho={11} />
        </span>
      ) : null}
      {children}
    </span>
  );
}

/**
 * @typedef {object} EtiquetaProps
 * @property {string} [icone] Ícone Lucide de 11 antes do texto (folder, clock, git-branch...).
 * @property {'ambar' | 'vermelho' | 'destaque'} [tom] Fio na tinta da cor e texto na cor ("direto" em âmbar).
 * @property {boolean} [baixa] 16 de altura, para caber numa linha de lista.
 * @property {boolean} [comFundo] Fundo `recuo`, para quem fica por cima de texto ou pontilhado.
 * @property {import('react').ReactNode} children O fato: "~/eita", "main", "00:04".
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
