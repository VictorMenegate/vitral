import * as React from 'react';
import { LOGO } from '../imagens.js';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * Os menus com cabeçalho de objeto (o do logo, o "⋯" de um chat): começam numa **placa de metal** de
 * 52 (o miolo do metal sob o véu marinho, com os cantos de cima do menu) com o símbolo de 20, o nome
 * 14/20 em 600 `forte` e o detalhe em mono `texto`. Itens de 32, margem 8, 2 entre eles, ícone de 16
 * em `tenue` e o rótulo 14/20; o item sob o ponteiro ganha `hover` com o relevo de tecla; as teclas
 * do atalho à direita; o que abre fora do app leva o external-link; o perigoso é `vermelho`.
 * Separador: o fio de pontos, nunca linha cheia. Pé: faixa de 32 no preto a 45% com a linha técnica
 * em mono `apagado`. Casca `flutuante` com `sombra-popover`, largura de 220 a 360.
 * @param {MenuComPlacaProps} props
 */
export function MenuComPlaca({ titulo, detalhe, simbolo, itens = [], realcado = -1, rodape, largura = 340, className, style, ...resto }) {
  return (
    <div role="menu" className={cx('vt-pop', className)} style={{ width: largura, overflow: 'hidden', ...style }} {...resto}>
      <div className="vt-placa" style={{ display: 'flex', alignItems: 'center', gap: 12, height: 52, padding: '0 16px', borderRadius: '19px 19px 0 0' }}>
        <span style={{ position: 'relative', display: 'flex', flex: 'none' }}>{simbolo ?? <img src={LOGO} alt="" style={{ width: 20, height: 20 }} />}</span>
        <div style={{ position: 'relative', display: 'flex', flexDirection: 'column', minWidth: 0 }}>
          <span style={{ fontSize: 14, lineHeight: '20px', fontWeight: 600, color: 'var(--forte)' }}>{titulo}</span>
          {detalhe ? (
            <span className="vt-tipo-etiqueta" style={{ color: 'var(--texto)' }}>
              {detalhe}
            </span>
          ) : null}
        </div>
      </div>
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, padding: '8px 0' }}>
        {itens.map((it, i) =>
          it === 'separador' || it.separador ? (
            <div key={i} style={{ display: 'flex', margin: '6px 18px' }}>
              <span className="vt-fio-pontos" style={{ background: 'repeating-linear-gradient(90deg,#ffffff24 0 2px,transparent 2px 4px)' }} />
            </div>
          ) : (
            <div
              key={i}
              role="menuitem"
              className="vt-item"
              style={{
                margin: '0 8px',
                padding: '0 10px',
                gap: 10,
                ...(i === realcado ? { background: it.perigoso ? 'var(--perigo-hover)' : 'var(--hover)', boxShadow: 'var(--aresta-de-tecla)' } : null),
              }}
            >
              {it.icone ? <Icone nome={it.icone} tamanho={16} cor={it.perigoso ? 'var(--vermelho)' : 'var(--tenue)'} /> : null}
              <span className="vt-t" style={{ fontWeight: 400, flex: 1, ...(it.perigoso ? { color: 'var(--vermelho)' } : null) }}>
                {it.rotulo}
              </span>
              {it.teclas ? (
                <span className="vt-teclas">
                  {it.teclas.map((k, j) => (
                    <span key={j} className="vt-tecla">
                      {k}
                    </span>
                  ))}
                </span>
              ) : null}
              {it.externo ? <Icone nome="external-link" tamanho={14} cor="var(--tenue)" /> : null}
            </div>
          ),
        )}
      </div>
      {rodape ? <div className="vt-rodape-menu">{rodape}</div> : null}
    </div>
  );
}

/**
 * @typedef {object} ItemDeMenu
 * @property {import('react').ReactNode} [rotulo] "Novo chat", "Configurações", "Novo chat em outra pasta…".
 * @property {string} [icone] Ícone Lucide de 16.
 * @property {string[]} [teclas] O atalho: ["Ctrl", "N"].
 * @property {boolean} [externo] Abre fora do app: o external-link à direita.
 * @property {boolean} [perigoso] Rótulo e ícone em vermelho.
 * @property {boolean} [separador] Em vez de item, o fio de pontos (ou passe a string "separador").
 */

/**
 * @typedef {object} MenuComPlacaProps
 * @property {import('react').ReactNode} titulo O nome do objeto na placa: "Vitral", o título do chat.
 * @property {import('react').ReactNode} [detalhe] O detalhe em mono: "25 chats", "~/eita · main".
 * @property {import('react').ReactNode} [simbolo] O símbolo de 20 na placa. Padrão, o logo do Vitral.
 * @property {Array<ItemDeMenu | 'separador'>} [itens] Os itens e separadores, na ordem.
 * @property {number} [realcado] Índice do item sob o ponteiro ou o teclado.
 * @property {import('react').ReactNode} [rodape] A linha técnica do pé: "Vitral 0.7.0 · pc".
 * @property {number} [largura] Padrão 340 (de 220 a 360).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
