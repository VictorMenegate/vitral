import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';
import { Selo } from '../base/Selo.jsx';

function Chamada({ c, ramo, continua }) {
  const icone = c.rodando ? (
    <span className="vt-girando" />
  ) : (
    <Icone
      nome={c.icone ?? 'terminal'}
      tamanho={14}
      cor={c.falhou ? 'var(--vermelho)' : c.interrompida ? 'var(--fantasma)' : undefined}
    />
  );
  return (
    <div className={cx('vt-log', ramo && 'vt-ramo', ramo && continua && 'vt-continua', c.falhou && 'vt-falhou')}>
      {icone}
      <span className={cx('vt-verbo', ramo && c.alvo != null && 'vt-col')}>{c.verbo}</span>
      {c.alvo != null ? <span className="vt-chip-mono">{c.alvo}</span> : null}
      {c.prosa != null ? <span className="vt-prosa">{c.prosa}</span> : null}
      {c.mais != null || c.menos != null ? (
        <span style={{ display: 'flex', gap: 6 }}>
          {c.mais != null ? <span className="vt-mais">+{c.mais}</span> : null}
          {c.menos != null ? <span className="vt-menos">{`−${c.menos}`}</span> : null}
        </span>
      ) : null}
      {c.falhou ? <span style={{ color: 'var(--texto)' }}>falhou</span> : null}
      {c.meta != null ? <span className="vt-meta">{c.meta}</span> : null}
      <Icone nome="chevron-right" tamanho={12} className="vt-chev" />
    </div>
  );
}

/**
 * As chamadas que o agente fez num turno: uma linha de 32 por chamada, com o ícone de 14 do tipo
 * (book-open ler, file-pen-line editar, file-plus gravar, terminal comando, text-search buscar, globe
 * web, bot subagente, workflow fluxo, list-checks passos, brain pensou), o **verbo** em 500 no passado
 * ou ao vivo ("Leu", "Rodando"), o **alvo** em mono num chip de 22 no `recuo` (comando, caminho) ou a
 * **prosa** em Inter (a tarefa do subagente) e a **meta** em mono `apagado`; o chevron sempre no
 * fim. Com `resumo`, vira o **grupo**: a linha do resumo com list-tree, o selo "N falhou" e o chevron,
 * e as chamadas penduradas em ramos de 1 px em `traco` (verbo numa coluna de 56). Recolhido
 * (`aberta={false}`), só o resumo, e as `imagens` lidas viram a PilhaDeImagens embaixo. Falha: fundo
 * `diff-sai` e a palavra "falhou".
 * @param {TrilhaDeFerramentasProps} props
 */
export function TrilhaDeFerramentas({ resumo, falhas, aberta = true, chamadas = [], abaixoDoResumo, className, style, ...resto }) {
  if (resumo == null) {
    return (
      <div className={className} style={style} {...resto}>
        {chamadas.map((c, i) => (
          <Chamada key={i} c={c} />
        ))}
      </div>
    );
  }
  return (
    <div className={className} style={style} {...resto}>
      <div className="vt-log" role="button" aria-expanded={aberta}>
        <Icone nome="list-tree" tamanho={14} />
        <span style={{ minWidth: 0, overflow: 'hidden', whiteSpace: 'nowrap', textOverflow: 'ellipsis' }}>{resumo}</span>
        {falhas ? (
          <Selo tom="vermelho">{`${falhas} falhou`}</Selo>
        ) : null}
        <Icone nome={aberta ? 'chevron-down' : 'chevron-right'} tamanho={12} className="vt-chev" />
      </div>
      {aberta ? chamadas.map((c, i) => <Chamada key={i} c={c} ramo continua={i < chamadas.length - 1} />) : null}
      {!aberta && abaixoDoResumo ? <div style={{ padding: '4px 0 4px 34px' }}>{abaixoDoResumo}</div> : null}
    </div>
  );
}

/**
 * @typedef {object} ChamadaDeFerramenta
 * @property {string} verbo "Leu", "Editou", "Rodou", "Rodando", "Pensou", "Subagente".
 * @property {string} [icone] Ícone Lucide de 14 do tipo: book-open, file-pen-line, terminal, text-search, globe, bot, brain.
 * @property {string} [alvo] O que é código, em mono no chip: "tema.rs", "npm run build".
 * @property {string} [prosa] O que é texto, em Inter: a tarefa do subagente.
 * @property {string} [meta] Em mono apagado: "por 9,8 s", "00:12", "14 ferramentas · 01:20".
 * @property {number} [mais] Linhas que entraram (verde).
 * @property {number} [menos] Linhas que saíram (vermelho).
 * @property {boolean} [falhou] Fundo diff-sai, ícone vermelho e "falhou".
 * @property {boolean} [rodando] O anel girando no lugar do ícone.
 * @property {boolean} [interrompida] Ícone em fantasma.
 */

/**
 * @typedef {object} TrilhaDeFerramentasProps
 * @property {ChamadaDeFerramenta[]} [chamadas] As chamadas, na ordem.
 * @property {import('react').ReactNode} [resumo] Com ele, vira o grupo: "Rodou 2 comandos · editou 1 arquivo · leu 1 arquivo".
 * @property {number} [falhas] Selo "N falhou" no resumo.
 * @property {boolean} [aberta] Grupo aberto (padrão) ou recolhido.
 * @property {import('react').ReactNode} [abaixoDoResumo] Recolhido: o que fica embaixo do resumo (a PilhaDeImagens das imagens lidas).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
