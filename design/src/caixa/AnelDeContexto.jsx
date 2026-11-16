import * as React from 'react';
import { corDaFaixa, cx } from '../util.js';

/**
 * O anel com fenda que mostra quanto da janela de contexto a conversa já usa, **com a porcentagem ao
 * lado, em mono** (regra do dono: sem ela o anel parecia o "carregando"), também na caixa estreita.
 * Todo anel do app tem a fenda de 60 graus embaixo (das 7 às 5 horas), trilho `borda-forte` e o arco
 * por cima: `destaque` até 69%, `ambar` de 70% a 89%, `vermelho` a partir de 90%. A porcentagem fica
 * em `tenue` até 69% e na cor da faixa depois. Mora na barra da caixa, num gatilho fantasma de 32.
 * `carregando` dá o mesmo anel aberto e girando, sem trilho (o "carregando" do app: 14 nas linhas, 12
 * na linha de estado da caixa). `porcentagem={false}` dá só o anel (28 e 36 no popover de uso).
 * @param {AnelDeContextoProps} props
 */
export function AnelDeContexto({ fracao = 0, tamanho, porcentagem = true, carregando = false, className, style, ...resto }) {
  if (carregando) {
    const lado = tamanho ?? 14;
    return <span role="img" aria-label="carregando" className={cx('vt-girando', className)} style={{ width: lado, height: lado, ...style }} {...resto} />;
  }
  const f = Math.max(0, Math.min(1, fracao));
  const cor = corDaFaixa(f);
  const lado = tamanho ?? 20;
  const espessura = `${Math.max(lado * 0.13, 3).toFixed(1)}px`;
  const anel = <span className="vt-anel" style={{ '--vt-f': f, '--vt-arco': cor, '--vt-esp': espessura, width: lado, height: lado }} />;
  if (!porcentagem) {
    return (
      <span role="img" aria-label={`${Math.round(f * 100)}% do contexto`} className={className} style={{ display: 'inline-flex', ...style }} {...resto}>
        {anel}
      </span>
    );
  }
  return (
    <span className={cx('vt-contexto', className)} style={style} title="Uso do contexto desta conversa" {...resto}>
      {anel}
      <span style={{ color: f >= 0.7 ? cor : 'var(--tenue)' }}>{Math.round(f * 100)}%</span>
    </span>
  );
}

/**
 * @typedef {object} AnelDeContextoProps
 * @property {number} [fracao] O uso do contexto, de 0 a 1.
 * @property {number} [tamanho] Diâmetro: 20 (padrão) na caixa; 28 e 36 nos popovers. No carregando, 14 (padrão) ou 12.
 * @property {boolean} [porcentagem] Mostra a porcentagem ao lado (padrão). Na caixa, sempre.
 * @property {boolean} [carregando] O anel aberto girando (o "carregando" do app).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
