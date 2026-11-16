import * as React from 'react';
import { LOGO, LOGO_V } from '../imagens.js';
import { cx } from '../util.js';

/**
 * O logo do Vitral: o V do dono (uma gota à esquerda ligada por um pescoço fino à perna grossa da
 * direita), em branco, sobre um quadrado de metal líquido de cantos 45%. É o mesmo em todo lugar e
 * **nunca se redesenha**: usa os arquivos do app. `forma="imagem"` (padrão) é o PNG pronto (o botão
 * do menu de 44, o menu de 20, as mensagens do Vitral de 20); `forma="metal"` recria o botão com o
 * metal parado em CSS e o V de verdade por cima (no app o metal é vivo).
 * @param {LogoProps} props
 */
export function Logo({ tamanho = 64, forma = 'imagem', className, style, ...resto }) {
  if (forma === 'metal') {
    return (
      <span
        role="img"
        aria-label="Vitral"
        className={cx('vt-metal', 'vt-grao', className)}
        style={{ display: 'inline-block', position: 'relative', width: tamanho, height: tamanho, borderRadius: Math.round(tamanho * 0.45), flex: 'none', ...style }}
        {...resto}
      >
        <img src={LOGO_V} alt="" style={{ position: 'absolute', inset: 0, width: '100%', height: '100%' }} />
      </span>
    );
  }
  return <img src={LOGO} alt="Vitral" width={tamanho} height={tamanho} className={className} style={{ width: tamanho, height: tamanho, flex: 'none', ...style }} {...resto} />;
}

/**
 * @typedef {object} LogoProps
 * @property {number} [tamanho] Lado em px: 128, 64 (padrão), 44 (o botão do menu), 20 (menu e mensagens).
 * @property {'imagem' | 'metal'} [forma] "imagem" (padrão, o PNG) ou "metal" (o V sobre o metal em CSS).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
