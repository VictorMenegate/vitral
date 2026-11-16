import * as React from 'react';
import { cx } from '../util.js';
import { BotaoRedondo } from '../base/BotaoRedondo.jsx';
import { Icone } from '../base/Icone.jsx';
import { AnelDeContexto } from './AnelDeContexto.jsx';

/**
 * Onde se escreve para o agente, no pé do painel de conversa (a doca). Uma **moldura preta** de raio
 * 20 com **2 de folga** (regra do dono) e, dentro, o **miolo** opaco de raio 18 (`cartao-elevado`,
 * fio `borda`, linha de luz no topo) com o campo 15/22 e a barra: à esquerda os chips (ChipDaCaixa:
 * modo, modelo, esforço), à direita o anel do contexto **com a porcentagem**, o clipe, o "⋯" e o
 * enviar (tecla de metal de 36; vazio, só o fio). **Ativa** (o painel selecionado): a **luz azul
 * embaixo** (o metal com 20% de destaque sumindo para o escuro em cima e nos lados), nunca borda nem
 * cantoneira. **Foco**: o aro de metal de 2 px em volta. **Rodando**: a linha de estado em mono
 * dentro da moldura (o anel girando, o relógio, os agentes, Esc) e o enviar vira o parar. Nos painéis
 * de fundo (`repouso`), o clipe e o "⋯" ficam a 0,55 (os chips recebem `repouso` também, menos o Sem
 * perguntar). Sem a linha das teclas embaixo quando parada.
 * @param {CaixaDeMensagemProps} props
 */
export function CaixaDeMensagem({
  valor,
  placeholder = 'Mensagem para este chat…',
  chips,
  anexos,
  contexto = 0,
  ativa = false,
  foco = false,
  repouso = false,
  rodando = false,
  tempo = '00:00',
  agentes,
  estreita = false,
  comDoca = true,
  className,
  style,
  ...resto
}) {
  const temTexto = valor != null && valor !== '';
  const enviar = rodando ? (
    <>
      {temTexto ? <BotaoRedondo tipo="metal" tamanho={estreita ? 32 : 36} icone="arrow-up" dica="Enviar" /> : null}
      <BotaoRedondo tipo="metal" tamanho={estreita ? 32 : 36} parar dica="Parar (Esc)" />
    </>
  ) : (
    <BotaoRedondo tipo={temTexto ? 'metal' : 'vazio'} tamanho={estreita ? 32 : 36} icone="arrow-up" dica="Enviar" />
  );
  const caixa = (
    <div className={cx('vt-caixa', !comDoca && className)} style={comDoca ? undefined : style} {...(comDoca ? {} : resto)}>
      {foco ? <span className="vt-aro" /> : null}
      <div className="vt-moldura">
        <div className="vt-miolo" style={estreita ? { padding: '8px 6px 6px' } : undefined}>
          {anexos ? <div style={{ display: 'flex', flexWrap: 'wrap', gap: 6, padding: '0 3px 8px' }}>{anexos}</div> : null}
          <div className={cx('vt-campo', !temTexto && 'vt-vazio')}>{temTexto ? valor : placeholder}</div>
          <div className="vt-barra-caixa" style={estreita ? { height: 28 } : undefined}>
            <div className="vt-esquerda">{chips}</div>
            <div className="vt-direita">
              <AnelDeContexto fracao={contexto} />
              <button type="button" className={cx('vt-botao vt-so-icone', estreita && 'vt-barra', repouso && 'vt-repouso')} title="Anexar" aria-label="Anexar">
                <Icone nome="paperclip" tamanho={16} />
              </button>
              <button type="button" className={cx('vt-botao vt-so-icone', estreita && 'vt-barra', repouso && 'vt-repouso')} title="Mais" aria-label="Mais">
                <Icone nome="ellipsis" tamanho={16} />
              </button>
              {enviar}
            </div>
          </div>
        </div>
        {rodando ? (
          <div className="vt-estado-caixa">
            <span style={{ display: 'flex', gap: 12, alignItems: 'center' }}>
              <span className="vt-girando" style={{ width: 12, height: 12 }} />
              {tempo}
              {agentes ? (
                <span style={{ display: 'inline-flex', gap: 4, alignItems: 'center' }}>
                  <Icone nome="workflow" tamanho={11} />
                  {agentes}
                </span>
              ) : null}
            </span>
            <span className="vt-tecla">Esc</span>
          </div>
        ) : null}
      </div>
    </div>
  );
  if (!comDoca) return caixa;
  return (
    <div className={cx('vt-doca', className)} style={{ paddingTop: ativa ? undefined : 12, ...style }} {...resto}>
      {ativa ? <div className="vt-luz" /> : null}
      {caixa}
    </div>
  );
}

/**
 * @typedef {object} CaixaDeMensagemProps
 * @property {string} [valor] O texto digitado. Vazio mostra o placeholder e o enviar vazio.
 * @property {string} [placeholder] Padrão "Mensagem para este chat…".
 * @property {import('react').ReactNode} [chips] Os chips da esquerda, na ordem modo, modelo, esforço (ChipDaCaixa).
 * @property {import('react').ReactNode} [anexos] Pílulas de arquivo ou a citação, na moldura em cima do campo.
 * @property {number} [contexto] O uso do contexto, de 0 a 1 (o anel com a porcentagem).
 * @property {boolean} [ativa] Painel selecionado: a luz azul embaixo (a doca).
 * @property {boolean} [foco] O aro de metal de 2 px em volta ("é aqui").
 * @property {boolean} [repouso] Painel de fundo: clipe e "⋯" a 0,55.
 * @property {boolean} [rodando] Turno em andamento: linha de estado e o parar.
 * @property {string} [tempo] O relógio do turno, com rodando ("00:03").
 * @property {number} [agentes] Quantos subagentes rodam, com rodando.
 * @property {boolean} [estreita] Faixa estreita (abaixo de 35 rem): barra de 28, enviar de 32. Passe os chips com soIcone.
 * @property {boolean} [comDoca] Embrulha no pé do painel (padding 40/16/12, onde a luz aparece). Padrão true.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
