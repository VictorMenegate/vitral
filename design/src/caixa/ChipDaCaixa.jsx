import * as React from 'react';
import { MARCAS } from '../imagens.js';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

const MODOS = {
  manual: { icone: 'escudo-cheio', rotulo: 'Manual' },
  aceitar: { icone: 'file-pen', rotulo: 'Aceitar edições' },
  planejar: { icone: 'map', rotulo: 'Planejar' },
  automatico: { icone: 'wand-sparkles', rotulo: 'Automático' },
  'sem-perguntar': { icone: 'shield-off', rotulo: 'Sem perguntar', cor: 'var(--ambar)' },
};

function Barrinhas({ nivel = 0, arco = false }) {
  return (
    <span className={cx('vt-barrinhas', arco && 'vt-arco')}>
      {[1, 2, 3, 4, 5].map((n) => (
        <i key={n} className={n <= nivel ? 'vt-on' : undefined} />
      ))}
    </span>
  );
}

/**
 * O controle da barra da caixa de mensagem: símbolo, rótulo e seta, **sem fundo em repouso**; é o
 * gatilho do menu do modo e dos popovers do modelo e do esforço. Os três chips da caixa: o **modo**
 * (Manual, Aceitar edições, Planejar, Automático em `tenue`; **Sem perguntar** em `ambar`, o único
 * com cor), o **modelo** (o glifo solto da marca, nunca o disco; na dupla, as duas marcas) e o
 * **esforço** (as barrinhas e o nível, "Auto · Extra"; Ultrathink em arco-íris, Ultracode em
 * `violeta`). Rótulo `tenue` no padrão e `texto` quando `mudado` nesta conversa. **Aberto**:
 * `pressionado` com fio `borda` e a seta para cima. Na faixa estreita, só o símbolo (`soIcone`).
 * Nos painéis de fundo, a 0,55 (`repouso`), menos o Sem perguntar.
 * @param {ChipDaCaixaProps} props
 */
export function ChipDaCaixa({
  modo,
  marca,
  esforco,
  icone,
  ultrathink = false,
  ultracode = false,
  seta = false,
  aberto = false,
  mudado = false,
  soIcone = false,
  repouso = false,
  desabilitado = false,
  tamanho = 32,
  dica,
  children,
  className,
  style,
  ...resto
}) {
  const m = modo ? MODOS[modo] : null;
  let simbolo = null;
  let cor;
  if (m) {
    simbolo = <Icone nome={m.icone} tamanho={14} cor={!soIcone ? m.cor : undefined} />;
    if (soIcone && m.cor) cor = m.cor;
  } else if (marca === 'dupla') {
    simbolo = (
      <span style={{ position: 'relative', width: 20, height: 14, flex: 'none' }}>
        <img src={MARCAS.codex.disco} alt="" style={{ position: 'absolute', right: 0, top: 0 }} />
        <img src={MARCAS.claude.disco} alt="" style={{ position: 'absolute', left: 0, top: 0 }} />
      </span>
    );
  } else if (marca) {
    simbolo = <img src={MARCAS[marca]?.glifo ?? MARCAS.claude.glifo} alt="" />;
  } else if (ultracode) {
    simbolo = <Icone nome="workflow" tamanho={14} />;
    cor = 'var(--violeta)';
  } else if (esforco != null || ultrathink) {
    simbolo = <Barrinhas nivel={ultrathink ? 5 : esforco} arco={ultrathink} />;
  } else if (icone) {
    simbolo = <Icone nome={icone} tamanho={14} />;
  }
  if (ultracode) cor = 'var(--violeta)';
  const rotulo = children ?? (m ? m.rotulo : null);
  const altura = tamanho === 32 ? null : { height: tamanho, ...(soIcone ? { width: tamanho } : null), borderRadius: tamanho === 28 ? 'var(--raio-de-28)' : 'var(--raio-de-24)', fontSize: tamanho === 24 ? 12 : undefined };
  return (
    <button
      type="button"
      title={dica ?? (soIcone && typeof rotulo === 'string' ? rotulo : undefined)}
      aria-label={soIcone && typeof rotulo === 'string' ? rotulo : undefined}
      aria-expanded={aberto || undefined}
      disabled={desabilitado || undefined}
      className={cx(
        'vt-chip',
        (mudado || m?.cor) && !soIcone && 'vt-mudado',
        soIcone && 'vt-so-icone',
        aberto && 'vt-aberto',
        repouso && 'vt-repouso',
        desabilitado && 'vt-desabilitado',
        className,
      )}
      style={{ ...(cor ? { color: cor } : null), ...altura, ...style }}
      {...resto}
    >
      {simbolo}
      {!soIcone && rotulo != null ? ultrathink ? <span className="vt-arco-iris">{rotulo}</span> : rotulo : null}
      {!soIcone && (seta || aberto) ? <Icone nome={aberto ? 'chevron-up' : 'chevron-down'} tamanho={12} className="vt-seta" /> : null}
    </button>
  );
}

/**
 * @typedef {object} ChipDaCaixaProps
 * @property {'manual' | 'aceitar' | 'planejar' | 'automatico' | 'sem-perguntar'} [modo] Chip do modo de permissão: põe o ícone e o rótulo do modo ("sem-perguntar" em âmbar).
 * @property {'claude' | 'codex' | 'dupla'} [marca] Chip do modelo: o glifo da marca (na "dupla", Claude e Codex sobrepostos).
 * @property {number} [esforco] Chip do esforço: quantas das 5 barrinhas acendem (0 a 5).
 * @property {string} [icone] Outro símbolo Lucide de 14, quando não é modo, marca nem esforço.
 * @property {boolean} [ultrathink] Ultrathink armado: barrinhas e rótulo em arco-íris.
 * @property {boolean} [ultracode] Ultracode: o ícone Workflow e o rótulo em violeta.
 * @property {boolean} [seta] Mostra a seta de 12 (o chip abre menu ou popover).
 * @property {boolean} [aberto] O menu está aberto: fundo pressionado, fio e seta para cima.
 * @property {boolean} [mudado] Mudado nesta conversa: rótulo em texto (em vez de tenue).
 * @property {boolean} [soIcone] Só o símbolo, quadrado da altura (faixa estreita); o rótulo vai para a dica.
 * @property {boolean} [repouso] A 0,55 (painel de fundo).
 * @property {boolean} [desabilitado] A 0,45, com o motivo na dica.
 * @property {32 | 28 | 24} [tamanho] Altura: 32 (padrão), 28 ou 24 (rótulo 12).
 * @property {string} [dica]
 * @property {import('react').ReactNode} [children] O rótulo ("Opus 5.5", "Auto · Extra"). No modo, o padrão é o nome do modo.
 * @property {(evento: import('react').MouseEvent<HTMLButtonElement>) => void} [onClick]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
