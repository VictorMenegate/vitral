import * as React from 'react';
import { cx } from '../util.js';

/**
 * Ícone Lucide do app (os mesmos SVG que o Vitral embute), pintado na cor do texto em volta.
 * Tamanhos do app: 12 (chevrons, marcos), 14 (linhas, botões de linha, chips), 16 (botões de barra e
 * controle, menus), 20 (estado vazio). Cor `tenue` por padrão de quem o envolve; passe `cor` só
 * quando a cor é o sinal (o escudo do Sem perguntar em âmbar, a ferramenta que falhou em vermelho).
 * Nomes: os do Lucide em kebab-case (`search`, `message-circle`, `chevron-down`...) mais `escudo-cheio`.
 * @param {IconeProps} props
 */
export function Icone({ nome, tamanho = 16, cor, className, style, ...resto }) {
  return (
    <i
      aria-hidden="true"
      className={cx('vt-i', `vt-i${tamanho}`, `vt-i-${nome}`, className)}
      style={cor ? { color: cor, ...style } : style}
      {...resto}
    />
  );
}

/**
 * @typedef {object} IconeProps
 * @property {string} nome Nome Lucide em kebab-case ("search", "plus", "chevron-down") ou "escudo-cheio"; a lista está no Icone.prompt.md.
 * @property {10 | 11 | 12 | 13 | 14 | 16 | 18 | 20 | 24} [tamanho] Lado em px. Padrão 16.
 * @property {string} [cor] Cor CSS, de preferência um token: "var(--ambar)". Sem ela, herda a cor do texto.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
