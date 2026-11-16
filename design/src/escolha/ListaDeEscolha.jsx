import * as React from 'react';
import { cx } from '../util.js';
import { CampoDeBusca } from '../base/CampoDeBusca.jsx';
import { Icone } from '../base/Icone.jsx';
import { ControleSegmentado } from './ControleSegmentado.jsx';
import { LinhaDeEscolha, RodapeDeTeclas, SecaoDaLista } from './linha.jsx';

/**
 * A lista com busca dos popovers (o seletor de modelo, o "/", o "@", o histórico do +): num popover
 * `flutuante` de 400 com padding 6, canto `raio-peca` e `sombra-popover`. Em cima, opcionalmente,
 * `abas` num ControleSegmentado de largura toda (Claude | Codex); depois a busca plana com a lupa na
 * coluna dos ícones e um fio cheio embaixo; os itens de 48 com descrição (32 sem), título 14/20 em
 * 500 e descrição `metadado` `tenue`. **O escolhido vai na ficha de metal com o visto**, sangrando 8
 * para fora da linha. Grupos com rótulo de seção mono. Rodapé: `children` (a linha "Claude e Codex
 * juntos" com o interruptor) e as teclas ("↑ ↓ escolher", "Enter usa").
 * @param {ListaDeEscolhaProps} props
 */
export function ListaDeEscolha({
  abas,
  abaEscolhida = 0,
  busca = 'Buscar',
  valorDaBusca,
  itens,
  escolhido,
  teclas = [
    { teclas: ['↑', '↓'], rotulo: 'escolher' },
    { teclas: ['Enter'], rotulo: 'usa' },
  ],
  largura = 400,
  children,
  className,
  style,
  ...resto
}) {
  return (
    <div role="listbox" className={cx('vt-pop', className)} style={{ width: largura, padding: 6, ...style }} {...resto}>
      {abas ? (
        <div style={{ padding: '0 2px 6px' }}>
          <ControleSegmentado opcoes={abas} valor={abaEscolhida} largo />
        </div>
      ) : null}
      <CampoDeBusca plana valor={valorDaBusca} placeholder={busca} style={{ height: 40, margin: '0 2px' }} />
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, padding: '6px 0' }}>
        {itens.map((it, i) =>
          it.secao ? (
            <SecaoDaLista key={i}>{it.secao}</SecaoDaLista>
          ) : (
            <LinhaDeEscolha
              key={i}
              titulo={it.titulo}
              descricao={it.descricao}
              simbolo={it.simbolo ?? (it.icone ? <Icone nome={it.icone} tamanho={16} /> : null)}
              selos={it.selos}
              escolhido={i === escolhido}
              desabilitado={it.desabilitado}
            />
          ),
        )}
      </div>
      {children ? (
        <>
          <div style={{ display: 'flex', padding: '0 8px' }}>
            <span className="vt-fio-pontos" />
          </div>
          {children}
        </>
      ) : null}
      {teclas && teclas.length ? <RodapeDeTeclas teclas={teclas} /> : null}
    </div>
  );
}

/**
 * @typedef {object} ItemDeEscolha
 * @property {import('react').ReactNode} [titulo] O nome: "Opus", "Padrão".
 * @property {import('react').ReactNode} [descricao] A linha de baixo: "Para trabalho complexo e o dia a dia".
 * @property {string} [icone] Símbolo Lucide de 16 na coluna dos ícones.
 * @property {import('react').ReactNode} [simbolo] Outro símbolo (Marca, IconeDeEstado) no lugar do ícone.
 * @property {import('react').ReactNode} [selos] Selos à direita do texto.
 * @property {boolean} [desabilitado]
 * @property {string} [secao] Em vez de item, um rótulo de seção ("Conta do Claude").
 */

/**
 * @typedef {object} ListaDeEscolhaProps
 * @property {ItemDeEscolha[]} itens Os itens e os rótulos de seção, na ordem.
 * @property {number} [escolhido] Índice (em itens) do escolhido: a ficha de metal com o visto.
 * @property {string[]} [abas] Abas no segmentado do topo: ["Claude", "Codex"].
 * @property {number} [abaEscolhida] Índice da aba. Padrão 0.
 * @property {string} [busca] Placeholder da busca. Padrão "Buscar".
 * @property {string} [valorDaBusca] O que já foi digitado.
 * @property {Array<{teclas: string[], rotulo: string}> | null} [teclas] O rodapé de teclas. Padrão "↑ ↓ escolher", "Enter usa"; null tira.
 * @property {number} [largura] Padrão 400 (320 na caixa estreita).
 * @property {import('react').ReactNode} [children] O rodapé, depois do fio de pontos (uma linha de opção com interruptor).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
