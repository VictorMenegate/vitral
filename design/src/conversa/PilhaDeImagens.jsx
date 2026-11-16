import * as React from 'react';
import { cx } from '../util.js';

/**
 * As imagens que as chamadas de um grupo recolhido leram ou gravaram, numa linha só: até 4
 * miniaturas quadradas de 22 sobrepostas 6 (canto `raio-controle`, borda de 2 em `cartao-opaco`),
 * como avatares empilhados, e "9 imagens" em `metadado` `tenue`. Altura 28, hover `hover`. Uma imagem
 * só fica na PilulaDeArquivo; duas ou mais viram a pilha (regra do dono). Recuada 34 para a coluna do
 * texto da trilha.
 * @param {PilhaDeImagensProps} props
 */
export function PilhaDeImagens({ imagens, total, comPonteiro = false, className, style, ...resto }) {
  const n = total ?? imagens.length;
  return (
    <span
      role="button"
      tabIndex={0}
      className={cx('vt-pilha', className)}
      style={{ ...(comPonteiro ? { background: 'var(--hover)' } : null), ...style }}
      {...resto}
    >
      <span style={{ display: 'inline-flex' }}>
        {imagens.slice(0, 4).map((src, i) => (
          <span key={i} className="vt-mini" style={{ width: 22 }}>
            <img src={src} alt="" style={{ width: '100%', height: '100%', objectFit: 'cover', objectPosition: '30% 40%', display: 'block' }} />
          </span>
        ))}
      </span>
      <span>{`${n} imagens`}</span>
    </span>
  );
}

/**
 * @typedef {object} PilhaDeImagensProps
 * @property {string[]} imagens URLs das imagens; só as 4 primeiras aparecem.
 * @property {number} [total] Quantas são ao todo ("9 imagens"). Padrão, o tamanho da lista.
 * @property {boolean} [comPonteiro] Mostra o hover.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
