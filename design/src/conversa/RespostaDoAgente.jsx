import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * A resposta do agente corre **solta na coluna, sem balão nem cartão**: texto em `corpo` (15/23) na
 * cor `texto`, parágrafos a 12, títulos de 18, 16 e 15, código em linha em `codigo` sobre `hover`,
 * caminhos como link em `destaque-texto`. Entre os parágrafos entram a TrilhaDeFerramentas, a
 * PilhaDeImagens, a CitacaoComCopiar e as PilulaDeArquivo, como irmãos. `acoes` mostra a barra do
 * pé (a hora e o Copiar), 8 abaixo do fim, à direita. Ritmo: 12 entre os blocos do turno, 32 antes do
 * próximo balão.
 * @param {RespostaDoAgenteProps} props
 */
export function RespostaDoAgente({ hora, acoes = false, children, className, style, ...resto }) {
  const texto = <div className={cx('vt-resposta', 'vt-tipo-corpo', !acoes && className)} style={acoes ? undefined : style} {...(acoes ? {} : resto)}>{children}</div>;
  if (!acoes) return texto;
  return (
    <div className={className} style={{ position: 'relative', paddingBottom: 36, ...style }} {...resto}>
      {texto}
      <div style={{ position: 'absolute', right: 0, bottom: 0 }}>
        <span className="vt-acoes-msg">
          {hora ? <span className="vt-hora">{hora}</span> : null}
          <button type="button" className="vt-botao vt-so-icone vt-linha" title="Copiar a resposta" aria-label="Copiar a resposta">
            <Icone nome="copy" tamanho={14} />
          </button>
        </span>
      </div>
    </div>
  );
}

/**
 * @typedef {object} RespostaDoAgenteProps
 * @property {import('react').ReactNode} children O texto: <p>, <h2>, <ul>, <a href> (caminhos), <code> (código em linha), <b>.
 * @property {string} [hora] A hora na barra de ações ("12:53").
 * @property {boolean} [acoes] Mostra a barra de ações no pé, à direita (no app, só no hover).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
