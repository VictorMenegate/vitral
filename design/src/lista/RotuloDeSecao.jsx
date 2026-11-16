import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';
import { Selo } from '../base/Selo.jsx';
import { Teclas } from '../base/Teclas.jsx';

/**
 * A segunda voz do app em cima de um grupo: o tracinho de 8 x 1 em `fio` e o título em mono 11
 * `apagado`, em caixa alta só no desenho ("PASTAS"; o dado continua "Pastas"). Linha de 24, recuo de
 * 8, 16 de respiro em cima. Com `contagem`, um selo de 18; `recolhivel` põe o chevron de 12 à
 * esquerda; `acao` põe um botão de linha à direita; `fio` estica o fio de pontos até a borda (nos
 * popovers de escolha), com as `teclas` do atalho no fim.
 * @param {RotuloDeSecaoProps} props
 */
export function RotuloDeSecao({
  children,
  contagem,
  tomContagem = 'neutro',
  recolhivel = false,
  aberto = true,
  acao,
  dicaAcao,
  fio = false,
  teclas,
  rotuloTeclas,
  className,
  style,
  ...resto
}) {
  if (fio) {
    return (
      <div className={className} style={{ display: 'flex', alignItems: 'center', gap: 8, minHeight: 20, padding: '0 8px', ...style }} {...resto}>
        <span className="vt-secao">{children}</span>
        <span className="vt-fio-pontos" />
        {teclas ? <Teclas teclas={teclas} rotulo={rotuloTeclas} /> : null}
      </div>
    );
  }
  return (
    <div
      className={className}
      style={{
        position: 'relative',
        display: 'flex',
        alignItems: 'center',
        gap: 6,
        height: 24,
        paddingLeft: recolhivel ? 20 : 8,
        color: recolhivel ? 'var(--apagado)' : undefined,
        ...style,
      }}
      {...resto}
    >
      {recolhivel ? (
        <span style={{ position: 'absolute', left: 6, display: 'flex' }}>
          <Icone nome={aberto ? 'chevron-down' : 'chevron-right'} tamanho={12} />
        </span>
      ) : null}
      <span className="vt-secao">{children}</span>
      {contagem != null ? (
        <Selo tipo="contagem" tom={tomContagem}>
          {contagem}
        </Selo>
      ) : null}
      {acao ? (
        <>
          <span style={{ flex: 1 }} />
          <button type="button" className="vt-botao vt-so-icone vt-linha" title={dicaAcao} aria-label={dicaAcao}>
            <Icone nome={acao} tamanho={14} />
          </button>
        </>
      ) : null}
    </div>
  );
}

/**
 * @typedef {object} RotuloDeSecaoProps
 * @property {import('react').ReactNode} children O título, escrito normal ("Pastas"); a caixa alta é do desenho.
 * @property {number | string} [contagem] Selo de contagem ao lado.
 * @property {'neutro' | 'espera' | 'destaque' | 'vermelho'} [tomContagem] Cor do selo de contagem.
 * @property {boolean} [recolhivel] Chevron de 12 à esquerda.
 * @property {boolean} [aberto] Com recolhivel: aberto (chevron para baixo) ou fechado. Padrão aberto.
 * @property {string} [acao] Ícone Lucide de um botão de linha à direita (plus).
 * @property {string} [dicaAcao] A dica desse botão.
 * @property {boolean} [fio] Estica o fio de pontos até a borda (popovers de escolha).
 * @property {string[]} [teclas] Com fio: as teclas do atalho no fim.
 * @property {import('react').ReactNode} [rotuloTeclas] Com fio: o texto depois das teclas ("troca").
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
