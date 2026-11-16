import * as React from 'react';
import { cx } from '../util.js';
import { Botao } from '../base/Botao.jsx';
import { Icone } from '../base/Icone.jsx';

const TOM = { vermelho: 'vt-vermelho', espera: 'vt-espera', ambar: 'vt-ambar', neutro: '' };

/**
 * O erro de um turno na conversa: o ícone de 16 na cor do assunto, o título em português (14/20 em
 * 600), a frase do que fazer (13/18 `tenue`) e as ações (botões fantasma de 24); o texto cru do
 * agente fica atrás de "Detalhes" (sempre a última ação). A cor fica no ícone e num véu de 6%, nunca
 * num cartão vermelho cheio. **vermelho**: o turno não pôde seguir; **espera** (azul-claro): precisa
 * de você; **ambar**: seguiu com ressalva; **neutro**: ninguém errou, é só esperar. O Copiar fica
 * sempre no canto. **"Tentar de novo" nunca entra aqui** (mora na FaixaDoTurno) e nunca botão
 * primário azul.
 * @param {CartaoDeErroProps} props
 */
export function CartaoDeErro({ tom = 'vermelho', icone = 'circle-x', titulo, frase, acoes, detalhes, detalhesAbertos = false, className, style, ...resto }) {
  const lista = [...(acoes ?? [])];
  if (detalhes) lista.push({ rotulo: 'Detalhes', icone: detalhesAbertos ? 'chevron-down' : 'chevron-right' });
  return (
    <div role="alert" className={cx('vt-erro', TOM[tom], className)} style={style} {...resto}>
      <div className="vt-cab">
        <span className="vt-icone">
          <Icone nome={icone} tamanho={16} />
        </span>
        <div className="vt-titulo">{titulo}</div>
        <button type="button" className="vt-botao vt-so-icone vt-linha" title="Copiar o erro" aria-label="Copiar o erro">
          <Icone nome="copy" tamanho={14} />
        </button>
      </div>
      {frase || lista.length || (detalhes && detalhesAbertos) ? (
        <div className="vt-corpo">
          {frase ? <div className="vt-frase">{frase}</div> : null}
          {lista.length ? (
            <div className="vt-acoes">
              {lista.map((a, i) => (
                <Botao key={i} tamanho="linha" icone={a.icone} onClick={a.onClick}>
                  {a.rotulo}
                </Botao>
              ))}
            </div>
          ) : null}
          {detalhes && detalhesAbertos ? <div className="vt-cru">{detalhes}</div> : null}
        </div>
      ) : null}
    </div>
  );
}

/**
 * @typedef {object} AcaoDoErro
 * @property {string} rotulo "Ver o uso", "Continuar no Claude", "Entrar de novo".
 * @property {string} [icone] Ícone Lucide de 14.
 * @property {() => void} [onClick]
 */

/**
 * @typedef {object} CartaoDeErroProps
 * @property {'vermelho' | 'espera' | 'ambar' | 'neutro'} [tom] O assunto. Padrão "vermelho".
 * @property {string} [icone] Ícone Lucide de 16: gauge (limite), key-round (login), zap, wifi-off, folder-x, circle-x.
 * @property {import('react').ReactNode} titulo O que aconteceu, em português: "O Codex chegou ao limite de uso".
 * @property {import('react').ReactNode} [frase] O que fazer: "Volta às 12:30."
 * @property {AcaoDoErro[]} [acoes] No máximo duas; o "Detalhes" entra sozinho por último quando há `detalhes`.
 * @property {import('react').ReactNode} [detalhes] O texto cru do agente (mono 12 no recuo).
 * @property {boolean} [detalhesAbertos] Mostra o texto cru.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
