import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from './Icone.jsx';

const ALTURA = { linha: 'vt-linha', barra: 'vt-barra', controle: '' };
const VARIANTE = { fantasma: '', primario: 'vt-primario', perigo: 'vt-perigo', claro: 'vt-claro', aviso: 'vt-aviso' };

/**
 * O botão com texto ou só com ícone (o `Button` do kit montado por controles.rs). **Fantasma é o
 * padrão** e serve a quase tudo (Retomar, Tentar de novo, Detalhes, Compactar); o primário azul é
 * raro (um destaque só por tela, nunca num cartão de erro); o claro é o "Permitir" do pedido de
 * permissão; o perigo pinta rótulo e ícone de vermelho. Sem `children` vira o botão só de ícone,
 * quadrado da altura, e aí a `dica` é obrigatória. O ícone vem antes do rótulo.
 * @param {BotaoProps} props
 */
export function Botao({
  variante = 'fantasma',
  tamanho = 'controle',
  icone,
  pressionado = false,
  desabilitado = false,
  dica,
  children,
  className,
  style,
  ...resto
}) {
  const soIcone = children == null || children === false;
  const ladoIcone = tamanho === 'linha' ? 14 : 16;
  return (
    <button
      type="button"
      title={dica}
      aria-label={soIcone ? dica : undefined}
      aria-pressed={pressionado || undefined}
      disabled={desabilitado || undefined}
      className={cx(
        'vt-botao',
        VARIANTE[variante],
        ALTURA[tamanho],
        soIcone && 'vt-so-icone',
        pressionado && 'vt-pressionado',
        desabilitado && 'vt-desabilitado',
        className,
      )}
      style={style}
      {...resto}
    >
      {icone ? <Icone nome={icone} tamanho={ladoIcone} /> : null}
      {soIcone ? null : <span className="vt-rotulo">{children}</span>}
    </button>
  );
}

/**
 * @typedef {object} BotaoProps
 * @property {'fantasma' | 'primario' | 'perigo' | 'claro' | 'aviso'} [variante] Padrão "fantasma". "primario" é raro (um por tela); "claro" é o Permitir; "aviso" é o Desfazer de um aviso.
 * @property {'linha' | 'barra' | 'controle'} [tamanho] "linha" 24 (ícone 14), "barra" 28 (ícone 16) ou "controle" 32 (padrão, ícone 16).
 * @property {string} [icone] Nome Lucide do ícone, antes do rótulo (veja Icone).
 * @property {boolean} [pressionado] Apertado, ou com o menu aberto.
 * @property {boolean} [desabilitado] A 0,45; diga o motivo na `dica`.
 * @property {string} [dica] A dica (title). Obrigatória no botão só de ícone.
 * @property {import('react').ReactNode} [children] O rótulo (13/18 em 500). Sem ele, o botão é só ícone.
 * @property {(evento: import('react').MouseEvent<HTMLButtonElement>) => void} [onClick]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
