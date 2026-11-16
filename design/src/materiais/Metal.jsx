import * as React from 'react';
import { cx } from '../util.js';

/**
 * O metal líquido azul, a assinatura do Vitral, e os recortes dele que dizem "é aqui", "escolhido" e
 * "ação". **base**: a janela inteira (a chapa debaixo do `vidro-leitura`); é onde a lista mora e
 * onde as peças flutuam: use como fundo da tela. **chapa**: o metal cru com grão (nunca texto em
 * cima). **ficha**: o escolhido (o miolo sob o véu marinho; em cima, só `forte` e `texto`).
 * **placa**: o cabeçalho de 52 dos menus. **tecla**: o metal com o destaque a 60% (o +, o enviar).
 * **aro**: o "é aqui", 2 px de metal em volta de um miolo `preto` (4 px no pedido que espera você).
 * **luz**: a doca, a luz azul de baixo do painel ativo, sumindo para o escuro em cima e nos lados.
 * Sem halo, sem degradê azul, sem borda azul. Parado em repouso: só anda com um chat trabalhando.
 * @param {MetalProps} props
 */
export function Metal({ tipo = 'base', espessura = 2, children, className, style, ...resto }) {
  if (tipo === 'ficha') {
    return (
      <div className={className} style={{ position: 'relative', isolation: 'isolate', borderRadius: 'var(--raio-controle)', color: 'var(--forte)', ...style }} {...resto}>
        <span className="vt-ficha" style={{ zIndex: -1, borderRadius: 'inherit' }} />
        {children}
      </div>
    );
  }
  if (tipo === 'aro') {
    return (
      <div className={className} style={{ position: 'relative', isolation: 'isolate', borderRadius: 14, ...style }} {...resto}>
        <span className="vt-aro" style={{ zIndex: -1, inset: -espessura, borderRadius: `calc(14px + ${espessura}px)` }} />
        <div style={{ position: 'relative', borderRadius: 'inherit', background: 'var(--preto)', width: '100%', height: '100%' }}>{children}</div>
      </div>
    );
  }
  if (tipo === 'luz') {
    return (
      <div className={className} style={{ position: 'relative', overflow: 'hidden', ...style }} {...resto}>
        <div className="vt-luz" />
        <div style={{ position: 'relative' }}>{children}</div>
      </div>
    );
  }
  const classe = {
    base: 'vt-base',
    chapa: 'vt-metal vt-grao',
    placa: 'vt-placa',
    tecla: 'vt-tecla-metal',
  }[tipo];
  return (
    <div className={cx(classe, className)} style={{ position: 'relative', ...(tipo === 'tecla' ? { color: 'var(--sobre-destaque)', boxShadow: 'var(--aresta-de-tecla), var(--sombra-elevado)' } : null), ...style }} {...resto}>
      {tipo === 'placa' ? <div style={{ position: 'relative' }}>{children}</div> : children}
    </div>
  );
}

/**
 * @typedef {object} MetalProps
 * @property {'base' | 'chapa' | 'ficha' | 'placa' | 'tecla' | 'aro' | 'luz'} [tipo] O recorte. Padrão "base" (o fundo da janela).
 * @property {number} [espessura] No "aro": 2 (padrão, foco e hover) ou 4 (o pedido que espera você).
 * @property {import('react').ReactNode} [children] O conteúdo por cima (na ficha e na placa, só texto forte ou texto).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style] Dê o tamanho, o canto e o padding por aqui.
 */
