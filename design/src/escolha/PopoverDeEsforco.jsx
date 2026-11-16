import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';
import { Interruptor } from '../base/Interruptor.jsx';
import { ControleSegmentado } from './ControleSegmentado.jsx';

const NIVEIS = ['Auto', 'Baixo', 'Médio', 'Alto', 'Extra', 'Máx'];

/**
 * O popover do chip do esforço (400, padding 16, vão 12, `flutuante` com `sombra-popover`): o
 * cabeçalho com as barrinhas do nível, o rótulo "ESFORÇO" e o fio de pontos até a borda; o
 * segmentado compacto dos seis níveis (Auto, Baixo, Médio, Alto, Extra, Máx) com o escolhido na
 * ficha de metal; a frase do nível em `metadado` `tenue`; e as duas chaves da próxima mensagem:
 * **Ultrathink** (título no arco-íris) e **Ultracode** (Workflow e título em `violeta`, aviso de
 * custo em `ambar`), cada uma com o Interruptor; no pé, as teclas (Espaço, U, Enter).
 * @param {PopoverDeEsforcoProps} props
 */
export function PopoverDeEsforco({
  nivel = 0,
  frase = 'Usa o padrão do modelo, que agora é o Extra.',
  ultrathink = false,
  ultracode = false,
  ultracodeIndisponivel,
  largura = 400,
  className,
  style,
  ...resto
}) {
  const barras = nivel === 0 ? 4 : Math.min(5, nivel);
  return (
    <div role="dialog" aria-label="Esforço" className={cx('vt-pop', className)} style={{ width: largura, padding: 16, display: 'flex', flexDirection: 'column', gap: 12, ...style }} {...resto}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, minHeight: 20 }}>
        <span className="vt-barrinhas" style={{ opacity: 0.45 }}>
          {[1, 2, 3, 4, 5].map((n) => (
            <i key={n} className={n <= barras ? 'vt-on' : undefined} />
          ))}
        </span>
        <span className="vt-secao">Esforço</span>
        <span className="vt-fio-pontos" />
      </div>
      <ControleSegmentado opcoes={NIVEIS} valor={nivel} noFlutuante compacto largo />
      <div className="vt-tipo-metadado" style={{ color: 'var(--tenue)' }}>
        {frase}
      </div>
      <span className="vt-fio-pontos" style={{ flex: 'none', width: '100%' }} />
      <div className="vt-opcao" style={{ padding: 0 }}>
        <div className="vt-txt">
          <div className="vt-t">
            <span className="vt-arco-iris">Ultrathink</span>
          </div>
          <div className="vt-d">Só na próxima mensagem</div>
        </div>
        <Interruptor ligado={ultrathink} rotulo="Ultrathink" />
      </div>
      <div className="vt-opcao" style={{ padding: 0 }}>
        <div className="vt-txt">
          <div className={cx('vt-t', ultracodeIndisponivel && 'vt-desabilitado')} style={{ color: 'var(--violeta)' }}>
            <Icone nome="workflow" tamanho={14} />
            Ultracode
          </div>
          {ultracodeIndisponivel ? (
            <div className="vt-d" style={{ color: 'var(--apagado)' }}>
              {ultracodeIndisponivel}
            </div>
          ) : (
            <>
              <div className="vt-d">Vários agentes em paralelo, só na próxima mensagem</div>
              <div className="vt-aviso">Gasta bem mais do plano e roda no esforço Extra</div>
            </>
          )}
        </div>
        <Interruptor ligado={ultracode} desabilitado={!!ultracodeIndisponivel} rotulo="Ultracode" />
      </div>
      <div style={{ display: 'flex' }}>
        <span className="vt-fio-pontos" />
      </div>
      <div className="vt-tipo-metadado" style={{ display: 'flex', gap: 12, color: 'var(--tenue)' }}>
        {[
          [['Espaço'], 'Ultrathink'],
          [['U'], 'Ultracode'],
          [['Enter'], 'fecha'],
        ].map(([t, r], i) => (
          <span key={i} style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <span className="vt-teclas">
              {t.map((k) => (
                <span key={k} className="vt-tecla">
                  {k}
                </span>
              ))}
            </span>
            <span>{r}</span>
          </span>
        ))}
      </div>
    </div>
  );
}

/**
 * @typedef {object} PopoverDeEsforcoProps
 * @property {0 | 1 | 2 | 3 | 4 | 5} [nivel] O nível escolhido: 0 Auto (padrão), 1 Baixo, 2 Médio, 3 Alto, 4 Extra, 5 Máx.
 * @property {import('react').ReactNode} [frase] A frase do nível ("Usa o padrão do modelo, que agora é o Extra.").
 * @property {boolean} [ultrathink] Ultrathink ligado para a próxima mensagem.
 * @property {boolean} [ultracode] Ultracode ligado para a próxima mensagem.
 * @property {string} [ultracodeIndisponivel] O motivo, quando o Ultracode não está disponível ("Só no Claude Code"): título e interruptor a 0,45.
 * @property {number} [largura] Padrão 400 (320 na caixa estreita).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
