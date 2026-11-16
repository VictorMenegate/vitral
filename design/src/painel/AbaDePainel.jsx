import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';
import { Selo } from '../base/Selo.jsx';
import { IconeDeEstado } from '../lista/IconeDeEstado.jsx';

/**
 * A aba do chat no cabeçalho de um painel: 32 de altura, canto `raio-de-32`, **um fio preto de 1 px**
 * (`preto`) e um miolo opaco, cada aba com a moldura própria, numa linha só (regra do dono). A **à
 * vista** tem o material do campo da caixa (`cartao-elevado` com a linha de luz no topo, texto
 * `forte`); as **de fundo** são lisas (`aba-inativa`, texto `tenue`). O ícone É o estado do chat
 * (IconeDeEstado de 16). O **número do painel** fica à direita, antes do ×. **Quadrada**: só o ícone
 * num quadrado de 32, o título vai para a dica. **Sob o ponteiro**: o aro de metal de 2 px acende em
 * volta. **Arrastando**: a 0,45 e tracejada.
 * @param {AbaDePainelProps} props
 */
export function AbaDePainel({
  titulo,
  estado = 'parado',
  ativa = false,
  quadrada = false,
  painel,
  painelAtivo = false,
  fechar = false,
  sobOPonteiro = false,
  arrastando = false,
  larguraMaxima,
  onClick,
  onFechar,
  className,
  style,
  ...resto
}) {
  const max = larguraMaxima ?? (quadrada ? undefined : ativa ? 280 : 180);
  const tracejada = arrastando ? { borderStyle: 'dashed', borderColor: 'var(--borda-forte)' } : null;
  return (
    <div
      role="tab"
      aria-selected={ativa}
      title={quadrada ? String(titulo ?? '') : undefined}
      onClick={onClick}
      className={cx(
        'vt-aba',
        ativa && 'vt-ativa',
        quadrada && 'vt-quadrada',
        sobOPonteiro && 'vt-sob-o-ponteiro',
        arrastando && 'vt-desabilitado',
        className,
      )}
      style={{ maxWidth: max, ...tracejada, ...style }}
      {...resto}
    >
      <span className="vt-aro" />
      <IconeDeEstado estado={estado} tamanho={16} />
      {quadrada ? null : <span className="vt-titulo">{titulo}</span>}
      {!quadrada && painel != null ? (
        <Selo tipo="painel" ativo={painelAtivo}>
          {painel}
        </Selo>
      ) : null}
      {!quadrada && fechar ? (
        <span
          className="vt-fechar"
          role="button"
          aria-label="Fechar a aba"
          title="Fechar a aba"
          onClick={
            onFechar
              ? (e) => {
                  e.stopPropagation();
                  onFechar();
                }
              : undefined
          }
        >
          <Icone nome="x" tamanho={14} />
        </span>
      ) : null}
    </div>
  );
}

/**
 * @typedef {object} AbaDePainelProps
 * @property {import('react').ReactNode} titulo O título do chat (truncado com reticências; na quadrada vai para a dica).
 * @property {'parado' | 'rodando' | 'espera' | 'nova' | 'erro'} [estado] O estado do chat, que é o ícone.
 * @property {boolean} [ativa] A aba à vista no painel (miolo elevado, texto forte). Padrão de fundo.
 * @property {boolean} [quadrada] Só o ícone, num quadrado de 32 (aba de fundo sem lugar para o título).
 * @property {number} [painel] O número do painel (Alt+1…4), à direita, antes do ×.
 * @property {boolean} [painelAtivo] O número azul: este é o painel ativo.
 * @property {boolean} [fechar] Mostra o × (sempre só na aba do painel ativo com vários painéis; nas outras, no hover).
 * @property {boolean} [sobOPonteiro] Mostra o aro de metal de 2 px (o hover).
 * @property {boolean} [arrastando] A aba de origem de um arrasto: 0,45 e tracejada.
 * @property {number} [larguraMaxima] Teto da largura. Padrão 280 na à vista e 180 na de fundo.
 * @property {(evento: import('react').MouseEvent<HTMLDivElement>) => void} [onClick]
 * @property {() => void} [onFechar]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
