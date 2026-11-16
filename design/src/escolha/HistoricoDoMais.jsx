import * as React from 'react';
import { cx } from '../util.js';
import { CampoDeBusca } from '../base/CampoDeBusca.jsx';
import { Icone } from '../base/Icone.jsx';
import { IconeDeEstado } from '../lista/IconeDeEstado.jsx';
import { LinhaDeEscolha } from './linha.jsx';

/**
 * O que o "+" do cabeçalho do painel abre: a primeira linha é **"Novo chat neste painel"**, já na
 * ficha de metal com o visto (o Enter cria), e embaixo o histórico com busca. Cada chat tem o ícone
 * de estado de 20, o título 14/500 e a linha de baixo em `metadado`: onde está ("no painel 3", "à
 * vista neste painel"), há quanto tempo e a pasta. É a ListaDeEscolha num popover de 400, padding 6,
 * ancorado no canto de cima à direita do "+". O escolhido aparece neste painel, como uma aba.
 * @param {HistoricoDoMaisProps} props
 */
export function HistoricoDoMais({ chats = [], escolhido = -1, busca = 'Buscar chat…', valorDaBusca, largura = 400, className, style, ...resto }) {
  return (
    <div role="listbox" className={cx('vt-pop', className)} style={{ width: largura, padding: 6, ...style }} {...resto}>
      <CampoDeBusca plana valor={valorDaBusca} placeholder={busca} style={{ height: 40, margin: '0 2px' }} />
      <div style={{ display: 'flex', flexDirection: 'column', gap: 2, padding: '6px 0' }}>
        <LinhaDeEscolha titulo="Novo chat neste painel" simbolo={<Icone nome="plus" tamanho={16} />} escolhido={escolhido === -1} />
        {chats.map((c, i) => (
          <LinhaDeEscolha
            key={i}
            titulo={c.titulo}
            descricao={c.descricao}
            simbolo={<IconeDeEstado estado={c.estado ?? 'parado'} />}
            escolhido={escolhido === i}
          />
        ))}
      </div>
    </div>
  );
}

/**
 * @typedef {object} ChatDoHistorico
 * @property {import('react').ReactNode} titulo O título do chat.
 * @property {import('react').ReactNode} [descricao] "no painel 3 · há 35 min · eita".
 * @property {'parado' | 'rodando' | 'espera' | 'nova' | 'erro'} [estado] O ícone de estado.
 */

/**
 * @typedef {object} HistoricoDoMaisProps
 * @property {ChatDoHistorico[]} [chats] O histórico, do mais recente para o mais antigo.
 * @property {number} [escolhido] Índice do chat escolhido; -1 (padrão) é o "Novo chat neste painel".
 * @property {string} [busca] Placeholder. Padrão "Buscar chat…".
 * @property {string} [valorDaBusca]
 * @property {number} [largura] Padrão 400.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
