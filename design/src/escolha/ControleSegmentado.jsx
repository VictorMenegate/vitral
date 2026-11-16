import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

const RAIO = { 28: 13, 32: 14, 36: 16, 40: 18 };

/**
 * Escolha entre poucas opções numa pílula: o trilho é um **poço** (`recuo` com `aresta-de-poco` e
 * fio `borda`) e a opção escolhida é a **ficha de metal** azul (o miolo do metal sob o véu marinho),
 * o "escolhido" do app inteiro (regra do dono). Segmentos em 13/500 `tenue`; o escolhido em 600 e
 * `forte`. `solto` tira o fio (o alto da lista, 40); `noFlutuante` troca o poço pelo
 * `recuo-no-flutuante` dentro de popovers; `compacto` aperta o padding para 4; `largo` estica na
 * largura toda. Alturas 28, 32 (padrão), 36 ou 40; o canto do trilho é 45% da altura e o da ficha, 2
 * a menos. Só o segmentado de outra cor (`tom`) pinta o escolhido numa pílula tingida.
 * @param {ControleSegmentadoProps} props
 */
export function ControleSegmentado({
  opcoes,
  valor = 0,
  tamanho = 32,
  solto = false,
  largo = false,
  noFlutuante = false,
  compacto = false,
  desabilitado = false,
  tom,
  onChange,
  className,
  style,
  ...resto
}) {
  const raio = RAIO[tamanho] ?? Math.round(tamanho * 0.45);
  const raioSeg = raio - 2;
  const trilho = tamanho === 32 || tamanho === 28 ? null : { height: tamanho, borderRadius: raio };
  const lista = opcoes.map((o) => (typeof o === 'string' ? { rotulo: o } : o));
  const escolhido = typeof valor === 'number' ? valor : lista.findIndex((o) => o.rotulo === valor);
  const seg = (
    <span
      role="radiogroup"
      className={cx(
        'vt-seg',
        tamanho === 28 && 'vt-p',
        solto && 'vt-solto',
        largo && 'vt-largo',
        noFlutuante && 'vt-no-flutuante',
        compacto && 'vt-compacto',
        !desabilitado && className,
      )}
      style={{ ...trilho, ...(desabilitado ? null : style) }}
      {...(desabilitado ? {} : resto)}
    >
      {lista.map((o, i) => {
        const e = i === escolhido;
        const soIcone = o.icone && !o.rotulo;
        const tingido = e && tom
          ? {
              background: `color-mix(in srgb, var(--${tom}) 20%, transparent)`,
              boxShadow: `inset 0 0 0 1px var(--tinta-borda-${tom}, color-mix(in srgb, var(--${tom}) 30%, transparent)), var(--aresta-de-tecla)`,
              color: `var(--${tom})`,
            }
          : null;
        return (
          <span
            key={i}
            role="radio"
            aria-checked={e}
            title={o.dica ?? (soIcone ? o.rotulo : undefined)}
            className={cx('vt-segm', e && 'vt-escolhido')}
            style={{ borderRadius: raioSeg, ...(soIcone ? { padding: '0 8px' } : null), ...tingido }}
            onClick={onChange ? () => onChange(i) : undefined}
          >
            {e && !tom ? <span className="vt-ficha" style={{ borderRadius: 'inherit' }} /> : null}
            {o.icone ? <Icone nome={o.icone} tamanho={14} /> : null}
            {o.rotulo}
          </span>
        );
      })}
    </span>
  );
  if (!desabilitado) return seg;
  return (
    <span className={cx('vt-desabilitado', className)} style={{ display: largo ? 'flex' : 'inline-flex', ...style }} {...resto}>
      {seg}
    </span>
  );
}

/**
 * @typedef {object} Segmento
 * @property {string} [rotulo] O texto (13/500).
 * @property {string} [icone] Ícone Lucide de 14; sem rótulo, o segmento é só ícone (padding 8).
 * @property {string} [dica]
 */

/**
 * @typedef {object} ControleSegmentadoProps
 * @property {Array<string | Segmento>} opcoes As opções: ["Nunca", "3 dias", "7 dias"] ou objetos com ícone.
 * @property {number | string} [valor] O escolhido: índice (padrão 0) ou o rótulo.
 * @property {28 | 32 | 36 | 40} [tamanho] Altura: 28 (pequeno), 32 (padrão), 36 (grande), 40 (o alto da lista).
 * @property {boolean} [solto] Sem o fio do trilho (o alto da lista).
 * @property {boolean} [largo] Largura toda, segmentos iguais.
 * @property {boolean} [noFlutuante] Dentro de popover: o poço vira recuo-no-flutuante.
 * @property {boolean} [compacto] Padding 4 nos segmentos (os seis níveis do esforço).
 * @property {boolean} [desabilitado] A 0,45.
 * @property {'espera' | 'ambar' | 'vermelho' | 'verde' | 'destaque'} [tom] Segmentado de estado: o escolhido numa pílula tingida em vez da ficha de metal.
 * @property {(indice: number) => void} [onChange]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
