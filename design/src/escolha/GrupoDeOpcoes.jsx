import * as React from 'react';
import { cx } from '../util.js';
import { BotaoRedondo } from '../base/BotaoRedondo.jsx';
import { Icone } from '../base/Icone.jsx';

function Contador({ valor }) {
  return (
    <span className="vt-contador">
      <BotaoRedondo tipo="plano" tamanho={24} icone="minus" dica="Menos" />
      <b>{valor}</b>
      <BotaoRedondo tipo="plano" tamanho={24} icone="plus" dica="Mais" />
    </span>
  );
}

/**
 * As preferências no estilo do libadwaita (as Configurações): um rótulo de seção e as linhas numa
 * caixa `grupo` com fio `borda` e canto `raio-cartao`, separadas por um **fio de pontos** recuado 12
 * (nunca linha cheia). Cada linha: padding 12, título 14/20 em 500 (com o (i) da dica longa),
 * descrição `metadado` `tenue`, aviso de custo em `ambar`, e o controle à direita a 16: o
 * Interruptor, o contador (− número +, num poço de 28), um ChipDaCaixa de menu ou um
 * ControleSegmentado. **Desabilitada**: só o título e o controle a 0,45; o motivo em `apagado`.
 * @param {GrupoDeOpcoesProps} props
 */
export function GrupoDeOpcoes({ titulo, opcoes = [], className, style, ...resto }) {
  return (
    <div className={className} style={{ display: 'flex', flexDirection: 'column', gap: 6, ...style }} {...resto}>
      {titulo ? (
        <div style={{ padding: '0 4px' }}>
          <span className="vt-secao">{titulo}</span>
        </div>
      ) : null}
      <div className="vt-grupo">
        {opcoes.map((o, i) => (
          <React.Fragment key={i}>
            {i > 0 ? (
              <div className="vt-sep">
                <span className="vt-fio-pontos" />
              </div>
            ) : null}
            <div className="vt-opcao">
              <div className="vt-txt">
                <div className={cx('vt-t', o.desabilitado && 'vt-desabilitado')}>
                  {o.titulo}
                  {o.info ? (
                    <span style={{ color: 'var(--tenue)', display: 'flex' }} title={typeof o.info === 'string' ? o.info : undefined}>
                      <Icone nome="info" tamanho={14} />
                    </span>
                  ) : null}
                </div>
                {o.descricao ? (
                  <div className="vt-d" style={o.desabilitado ? { color: 'var(--apagado)' } : undefined}>
                    {o.descricao}
                  </div>
                ) : null}
                {o.aviso ? <div className="vt-aviso">{o.aviso}</div> : null}
              </div>
              {o.contador != null ? (
                <Contador valor={o.contador} />
              ) : o.controle ? (
                o.desabilitado ? (
                  <span className="vt-desabilitado" style={{ display: 'flex' }}>
                    {o.controle}
                  </span>
                ) : (
                  o.controle
                )
              ) : null}
            </div>
          </React.Fragment>
        ))}
      </div>
    </div>
  );
}

/**
 * @typedef {object} LinhaDeOpcao
 * @property {import('react').ReactNode} titulo "Mostrar o uso do plano no canto da janela".
 * @property {import('react').ReactNode} [descricao] Uma frase em metadado tenue; na desabilitada, o motivo.
 * @property {boolean | string} [info] Mostra o (i) depois do título (a string vira a dica).
 * @property {import('react').ReactNode} [aviso] Aviso de custo em âmbar.
 * @property {import('react').ReactNode} [controle] O controle à direita: Interruptor, ChipDaCaixa, ControleSegmentado.
 * @property {number | string} [contador] Em vez de controle, o contador − N + num poço.
 * @property {boolean} [desabilitado] Título e controle a 0,45; a descrição em apagado.
 */

/**
 * @typedef {object} GrupoDeOpcoesProps
 * @property {string} [titulo] O rótulo de seção em cima ("Chats").
 * @property {LinhaDeOpcao[]} opcoes As linhas, na ordem.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
