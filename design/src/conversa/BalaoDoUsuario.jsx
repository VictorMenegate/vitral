import * as React from 'react';
import { cx } from '../util.js';
import { Botao } from '../base/Botao.jsx';
import { Icone } from '../base/Icone.jsx';

/**
 * O que você escreveu: um balão azul baixo, **na largura toda da coluna** (regra do dono), fundo
 * `balao` (o destaque a 16% misturado à base, opaco) com fio `balao-borda`, canto 16 e **a ponta
 * embaixo à direita** em 10. Texto em `fala` (14/22), um degrau abaixo da resposta. Os anexos
 * (PilulaDeArquivo com `noBalao`) vêm em cima do texto. Caminhos viram link em `destaque-texto`;
 * código em linha vai no poço (`<code>`). `comando` sozinho ("/compact") vira a pílula de 32 em mono.
 * `linhas` acima de 14 corta em 12 com o véu e o "Mostrar tudo (N linhas)". `acoes` mostra a barra do
 * hover (a hora, Editar, Copiar), 14 acima do balão e 16 da borda direita.
 * @param {BalaoDoUsuarioProps} props
 */
export function BalaoDoUsuario({ anexos, comando, hora, acoes = false, linhas, children, className, style, ...resto }) {
  if (comando) {
    return (
      <div className={className} style={{ display: 'flex', ...style }} {...resto}>
        <span className="vt-comando vt-tipo-mono">{comando}</span>
      </div>
    );
  }
  const longo = linhas != null && linhas > 14;
  const balao = (
    <div className={cx('vt-balao', !acoes && className)} style={acoes ? undefined : style} {...(acoes ? {} : resto)}>
      {anexos ? <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6 }}>{anexos}</div> : null}
      {children != null ? (
        longo ? (
          <div className="vt-tipo-fala" style={{ position: 'relative', maxHeight: 12 * 22, overflow: 'hidden' }}>
            {children}
            <div className="vt-veu-pe" />
          </div>
        ) : (
          <div className="vt-tipo-fala">{children}</div>
        )
      ) : null}
      {longo ? (
        <div style={{ display: 'flex', marginLeft: -8 }}>
          <Botao tamanho="linha" icone="chevron-down">{`Mostrar tudo (${linhas} linhas)`}</Botao>
        </div>
      ) : null}
    </div>
  );
  if (!acoes) return balao;
  return (
    <div className={className} style={{ position: 'relative', ...style }} {...resto}>
      {balao}
      <div style={{ position: 'absolute', top: -14, right: 16 }}>
        <span className="vt-acoes-msg">
          {hora ? <span className="vt-hora">{hora}</span> : null}
          <button type="button" className="vt-botao vt-so-icone vt-linha" title="Editar" aria-label="Editar">
            <Icone nome="pencil" tamanho={14} />
          </button>
          <button type="button" className="vt-botao vt-so-icone vt-linha" title="Copiar" aria-label="Copiar">
            <Icone nome="copy" tamanho={14} />
          </button>
        </span>
      </div>
    </div>
  );
}

/**
 * @typedef {object} BalaoDoUsuarioProps
 * @property {import('react').ReactNode} [children] O texto. Use <a href> para caminhos e <code className="vt-code-linha"> para código em linha.
 * @property {import('react').ReactNode} [anexos] Pílulas dos anexos (PilulaDeArquivo noBalao), em cima do texto.
 * @property {string} [comando] Um comando de barra sozinho ("/compact"): vira a pílula de 32 em mono, sem balão.
 * @property {string} [hora] A hora na barra de ações ("12:51").
 * @property {boolean} [acoes] Mostra a barra de ações do hover (hora, Editar, Copiar).
 * @property {number} [linhas] Quantas linhas o texto tem; acima de 14, corta em 12 com "Mostrar tudo (N linhas)".
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
