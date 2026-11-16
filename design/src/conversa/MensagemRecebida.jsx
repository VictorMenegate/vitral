import * as React from 'react';
import { LOGO } from '../imagens.js';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * Uma mensagem que outro chat (ou o próprio Vitral) entregou nesta conversa: quem mandou numa linha
 * de 28 (o logo do Vitral de 20, ou o círculo do chat) com o nome 13/18 em 600 e a seta à direita;
 * fechada, a prévia em `tenue` numa linha com reticências, recuada 32; aberta, o texto inteiro em
 * `corpo` na cor `texto`.
 * @param {MensagemRecebidaProps} props
 */
export function MensagemRecebida({ de = 'Vitral', doVitral, aberta = false, children, className, style, ...resto }) {
  const vitral = doVitral ?? de === 'Vitral';
  return (
    <div className={cx('vt-recebida', className)} style={style} {...resto}>
      <div className="vt-cab">
        {vitral ? (
          <span style={{ width: 24, display: 'flex', justifyContent: 'center' }}>
            <img src={LOGO} alt="" style={{ width: 20, height: 20 }} />
          </span>
        ) : (
          <span style={{ width: 24, height: 24, borderRadius: 11, background: 'var(--hover)', display: 'flex', alignItems: 'center', justifyContent: 'center', color: 'var(--tenue)' }}>
            <Icone nome="message-circle" tamanho={12} />
          </span>
        )}
        <span className="vt-nome">{de}</span>
        <span style={{ flex: 1 }} />
        <span style={{ width: 24, display: 'flex', justifyContent: 'center', color: 'var(--tenue)' }}>
          <Icone nome={aberta ? 'chevron-down' : 'chevron-right'} tamanho={12} />
        </span>
      </div>
      {aberta ? (
        <div className="vt-tipo-corpo" style={{ paddingLeft: 32, color: 'var(--texto)' }}>
          {children}
        </div>
      ) : (
        <div className="vt-previa">{children}</div>
      )}
    </div>
  );
}

/**
 * @typedef {object} MensagemRecebidaProps
 * @property {string} [de] Quem mandou: "Vitral" (padrão) ou o nome do chat de origem.
 * @property {boolean} [doVitral] Força o logo do Vitral no avatar (padrão: quando de é "Vitral").
 * @property {boolean} [aberta] Texto inteiro (aberta) ou só a prévia numa linha (fechada, padrão).
 * @property {import('react').ReactNode} children O texto da mensagem.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
