import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

// Peças internas das listas de escolha (não exportadas): a linha com a ficha de metal no escolhido,
// o rótulo de seção da lista e o rodapé de teclas.

export function LinhaDeEscolha({ titulo, descricao, simbolo, selos, escolhido = false, desabilitado = false }) {
  return (
    <div
      role="option"
      aria-selected={escolhido}
      aria-disabled={desabilitado || undefined}
      className={cx('vt-item', descricao && 'vt-dois', escolhido && 'vt-escolhido', desabilitado && 'vt-desabilitado')}
      style={{ margin: '0 8px', padding: 0 }}
    >
      {escolhido ? <span className="vt-ficha" style={{ left: -8, right: -8, borderRadius: 'var(--raio-controle)' }} /> : null}
      {simbolo ? <span style={{ display: 'flex', width: 20, justifyContent: 'center', position: 'relative', flex: 'none' }}>{simbolo}</span> : null}
      <span style={{ display: 'flex', flexDirection: 'column', minWidth: 0, flex: 1 }}>
        <span className="vt-t">{titulo}</span>
        {descricao ? <span className="vt-d">{descricao}</span> : null}
      </span>
      {selos}
      {escolhido ? (
        <span className="vt-check">
          <Icone nome="check" tamanho={16} />
        </span>
      ) : null}
    </div>
  );
}

export function SecaoDaLista({ children }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', height: 28, padding: '0 8px' }}>
      <span className="vt-secao">{children}</span>
    </div>
  );
}

export function RodapeDeTeclas({ teclas }) {
  return (
    <>
      <div style={{ display: 'flex', padding: '4px 8px 0' }}>
        <span className="vt-fio-pontos" />
      </div>
      <div className="vt-tipo-metadado" style={{ display: 'flex', gap: 12, padding: 8, color: 'var(--tenue)' }}>
        {teclas.map((t, i) => (
          <span key={i} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span className="vt-teclas">
              {t.teclas.map((k, j) => (
                <span key={j} className="vt-tecla">
                  {k}
                </span>
              ))}
            </span>
            <span>{t.rotulo}</span>
          </span>
        ))}
      </div>
    </>
  );
}
