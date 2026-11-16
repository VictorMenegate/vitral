import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from './Icone.jsx';
import { Selo } from './Selo.jsx';

const TIPO = {
  metal: 'vt-metal-btn vt-tecla-metal',
  neutro: '',
  plano: 'vt-plano',
  vazio: 'vt-vazio',
  perigo: 'vt-perigo',
};

/**
 * O botão redondo do app (raio::de da altura, bem arredondado, nunca círculo). A **tecla de metal**
 * (a chapa azul com o destaque a 60%) é o + da lista, o enviar e o parar; o **vazio** é o enviar sem
 * texto; o **neutro** tem fundo `hover`; o **plano** é transparente (os botões do trilho, o − e o +
 * do contador); o **perigo** é vermelho. `parar` troca o ícone pelo quadrado de 10 com canto 2,6.
 * `selo` põe um selo de contagem recortado no canto (o sino com pedidos esperando).
 * @param {BotaoRedondoProps} props
 */
export function BotaoRedondo({
  tipo = 'neutro',
  tamanho = 32,
  icone,
  parar = false,
  selo,
  dica,
  desabilitado = false,
  className,
  style,
  ...resto
}) {
  const classeTamanho = tamanho === 32 ? '' : `vt-d${tamanho}`;
  return (
    <button
      type="button"
      title={dica}
      aria-label={dica}
      disabled={desabilitado || undefined}
      className={cx('vt-redondo', classeTamanho, TIPO[tipo], desabilitado && 'vt-desabilitado', className)}
      style={style}
      {...resto}
    >
      {parar ? <span className="vt-quadrado" /> : icone ? <Icone nome={icone} tamanho={tamanho <= 24 ? 14 : 16} /> : null}
      {selo != null ? (
        <span className="vt-selo-canto">
          <Selo tipo="contagem" tom="espera" solido>
            {selo}
          </Selo>
        </span>
      ) : null}
    </button>
  );
}

/**
 * @typedef {object} BotaoRedondoProps
 * @property {'metal' | 'neutro' | 'plano' | 'vazio' | 'perigo'} [tipo] "metal" (tecla de metal: +, enviar, parar), "neutro" (padrão), "plano", "vazio" (o enviar sem texto) ou "perigo".
 * @property {24 | 28 | 32 | 36} [tamanho] Diâmetro: 24, 28, 32 (padrão) ou 36 (as teclas de metal).
 * @property {string} [icone] Nome Lucide (plus, arrow-up, bell, archive, minus, x...).
 * @property {boolean} [parar] Desenha o quadrado do parar no lugar do ícone.
 * @property {import('react').ReactNode} [selo] Contagem no canto, num selo sólido `espera` recortado (o sino).
 * @property {string} [dica] A dica (title); sempre, porque não há rótulo.
 * @property {boolean} [desabilitado]
 * @property {(evento: import('react').MouseEvent<HTMLButtonElement>) => void} [onClick]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
