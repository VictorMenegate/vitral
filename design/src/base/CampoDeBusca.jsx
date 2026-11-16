import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from './Icone.jsx';
import { Teclas } from './Teclas.jsx';

/**
 * A busca da lista e dos painéis: um poço de 32 sem fio (a `aresta-de-poco` desenha a borda), a lupa
 * de 14 em `fantasma`, o placeholder em `tenue` e, vazio, as teclas do atalho à direita. **Com foco**
 * ("é aqui"): o aro de metal de 2 px em volta de um miolo opaco `preto`, sem azul chapado nem halo.
 * **Alta** (40) é a do pé da lista, ao lado do + de metal. **Plana** é a de dentro de uma lista de
 * escolha: 44 de altura, sem fundo, lupa de 16 e um fio `borda` embaixo.
 * @param {CampoDeBuscaProps} props
 */
export function CampoDeBusca({
  valor,
  placeholder = 'Buscar',
  teclas = ['Ctrl', 'K'],
  focada = false,
  alta = false,
  plana = false,
  onChange,
  className,
  style,
  ...resto
}) {
  const vazio = valor == null || valor === '';
  const campo = onChange ? (
    <input
      value={valor ?? ''}
      placeholder={placeholder}
      onChange={(e) => onChange(e.target.value)}
      style={{ flex: 1, minWidth: 0, background: 'none', border: 0, outline: 'none', font: 'inherit', color: 'var(--texto)', padding: 0 }}
    />
  ) : (
    <span style={{ flex: 1, minWidth: 0, color: vazio ? undefined : 'var(--texto)', overflow: 'hidden', whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>
      {vazio ? placeholder : valor}
    </span>
  );
  if (plana) {
    return (
      <div className={cx('vt-busca-plana', className)} style={style} {...resto}>
        <Icone nome="search" tamanho={16} />
        {campo}
      </div>
    );
  }
  const alto = alta ? { height: 40, borderRadius: 18 } : null;
  return (
    <div className={cx('vt-busca', focada && 'vt-focada', className)} style={{ ...alto, ...style }} {...resto}>
      {focada ? <span className="vt-aro" /> : null}
      <span className="vt-lupa">
        <Icone nome="search" tamanho={14} />
      </span>
      {campo}
      {focada && !onChange ? <span className="vt-cursor" /> : null}
      {vazio && !focada && teclas && teclas.length ? <Teclas teclas={teclas} /> : null}
    </div>
  );
}

/**
 * @typedef {object} CampoDeBuscaProps
 * @property {string} [valor] O texto digitado. Vazio mostra o placeholder e as teclas.
 * @property {string} [placeholder] Padrão "Buscar".
 * @property {string[] | null} [teclas] O atalho à direita quando vazio. Padrão ["Ctrl", "K"]; null tira.
 * @property {boolean} [focada] Mostra o aro de metal e o cursor.
 * @property {boolean} [alta] 40 de altura (o pé da lista).
 * @property {boolean} [plana] A variante de dentro das listas de escolha (44, sem poço, fio embaixo).
 * @property {(valor: string) => void} [onChange] Com ele, vira um input de verdade.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
