import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * A fileira de 44 no topo de cada painel de conversa, com pouca coisa (regra do dono): as abas
 * (AbaDePainel, separadas por 6), o modo foco (só quando ligado) e o "+", que abre um chat novo neste
 * painel ou um do histórico. **Sem "⋯"**: o menu do painel mora no botão direito. As ações ficam a
 * 0,55 até o ponteiro chegar. No último painel, `fecharJanela` põe um traço de 1 x 16 e o × da
 * janela ("Fechar o Vitral"). Sem linha embaixo. Vai no topo de uma peça (Materiais "vidro").
 * @param {CabecalhoDoPainelProps} props
 */
export function CabecalhoDoPainel({ children, foco = false, fecharJanela = false, onMais, className, style, ...resto }) {
  return (
    <div role="tablist" className={cx('vt-cabecalho', className)} style={style} {...resto}>
      <div className="vt-abas" style={{ flex: 1 }}>
        {children}
      </div>
      <div className="vt-acoes">
        {foco ? (
          <button type="button" className="vt-botao vt-so-icone vt-barra vt-pressionado" title="Sair do modo foco" aria-label="Sair do modo foco">
            <Icone nome="focus" tamanho={16} />
          </button>
        ) : null}
        <button type="button" className="vt-botao vt-so-icone vt-barra" title="Novo chat ou histórico" aria-label="Novo chat ou histórico" onClick={onMais}>
          <Icone nome="plus" tamanho={16} />
        </button>
      </div>
      {fecharJanela ? (
        <>
          <span style={{ width: 8 }} />
          <span className="vt-traco-v" />
          <button type="button" className="vt-botao vt-so-icone vt-barra" title="Fechar o Vitral" aria-label="Fechar o Vitral">
            <Icone nome="x" tamanho={16} />
          </button>
        </>
      ) : null}
    </div>
  );
}

/**
 * @typedef {object} CabecalhoDoPainelProps
 * @property {import('react').ReactNode} children As abas do painel (AbaDePainel).
 * @property {boolean} [foco] O modo foco ligado: mostra o botão para sair dele.
 * @property {boolean} [fecharJanela] Último painel: o traço e o × da janela à direita.
 * @property {() => void} [onMais] O "+" (novo chat neste painel ou histórico; veja HistoricoDoMais).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
