import * as React from 'react';
import { cx } from '../util.js';
import { Botao } from '../base/Botao.jsx';
import { Icone } from '../base/Icone.jsx';

const TOM = { espera: 'var(--espera)', vermelho: 'var(--vermelho)', tenue: 'var(--tenue)' };

/**
 * A faixa de 28 logo acima da caixa de mensagem: diz o que parou e oferece o botão que resolve
 * (Retomar, Tentar de novo, Compactar). Só para o que pede um botão seu: o turno em andamento mora
 * na linha de estado dentro da caixa. Ícone de 14 na cor do sinal (`espera` para o que precisa de
 * você, `vermelho` para limite e falha, `tenue` para rede e conversa fria), texto 13/18 `tenue` numa
 * linha só com reticências no fim; a ação fica sempre inteira. O "Tentar de novo" mora só aqui.
 * @param {FaixaDoTurnoProps} props
 */
export function FaixaDoTurno({ icone, tom = 'tenue', acao, iconeAcao, onAcao, dica, children, className, style, ...resto }) {
  return (
    <div className={cx('vt-faixa', className)} style={style} {...resto}>
      <div className="vt-texto" title={dica}>
        {icone ? <Icone nome={icone} tamanho={14} cor={TOM[tom] ?? tom} /> : null}
        <span>{children}</span>
      </div>
      {acao ? (
        <Botao tamanho="barra" icone={iconeAcao} onClick={onAcao}>
          {acao}
        </Botao>
      ) : null}
    </div>
  );
}

/**
 * @typedef {object} FaixaDoTurnoProps
 * @property {string} [icone] Ícone Lucide de 14: pause, gauge, key-round, wifi-off, clock, minimize, folder-x.
 * @property {'espera' | 'vermelho' | 'tenue'} [tom] Cor do ícone. Padrão "tenue".
 * @property {import('react').ReactNode} [acao] Rótulo do botão: "Retomar", "Tentar de novo", "Compactar".
 * @property {string} [iconeAcao] Ícone do botão: play, rotate-cw, minimize.
 * @property {() => void} [onAcao]
 * @property {string} [dica] O motivo inteiro, na dica do texto.
 * @property {import('react').ReactNode} children O texto curto: "Parada · Vitral fechou".
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
