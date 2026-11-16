import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';
import { Selo } from '../base/Selo.jsx';
import { IconeDeEstado } from './IconeDeEstado.jsx';

/**
 * A linha de um chat na lista: o ícone de estado (20), o título 15/20 em 400 e a meta à direita (o
 * número do painel em que está aberto, ou há quanto tempo foi atualizado); com o ponteiro em cima, as
 * ações (abrir ao lado, o "⋯") tomam o lugar da meta. Altura 36, canto `raio-controle`, 2 entre os
 * cartões. **Selecionado**: fundo `selecao` e a barra de 3 em `destaque` à esquerda (a 40% se está
 * aberto em outro painel). **Não lido**: título em 600 e `forte`. **Realce** do teclado: fio
 * `destaque-borda`. Rodando, a meta mostra há quanto tempo em `destaque-texto`.
 * @param {CartaoDeListaProps} props
 */
export function CartaoDeLista({
  titulo,
  estado = 'parado',
  selecionado = false,
  abertoEmOutro = false,
  naoLido = false,
  painel,
  painelAtivo = false,
  hora,
  rodandoHa,
  fixado = false,
  comPonteiro = false,
  realce = false,
  onClick,
  className,
  style,
  ...resto
}) {
  const meta = [];
  if (fixado) {
    meta.push(
      <span key="pin" style={{ color: 'var(--apagado)', display: 'flex' }}>
        <Icone nome="pin" tamanho={11} />
      </span>,
    );
  }
  if (painel != null) {
    meta.push(
      <Selo key="painel" tipo="painel" ativo={painelAtivo}>
        {painel}
      </Selo>,
    );
  } else if (rodandoHa) {
    meta.push(
      <span key="ha" style={{ color: 'var(--destaque-texto)' }}>
        {rodandoHa}
      </span>,
    );
  } else if (hora) {
    meta.push(
      <span key="hora" style={selecionado ? { color: 'var(--texto)' } : undefined}>
        {hora}
      </span>,
    );
  }
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={onClick}
      className={cx(
        'vt-cartao-lista',
        selecionado && 'vt-selecionado',
        naoLido && 'vt-nao-lido',
        comPonteiro && 'vt-com-ponteiro',
        realce && 'vt-realce',
        className,
      )}
      style={style}
      {...resto}
    >
      {selecionado || abertoEmOutro ? <span className={cx('vt-barra-sel', !selecionado && 'vt-outro')} /> : null}
      <IconeDeEstado estado={estado} />
      <span className="vt-t">{titulo}</span>
      <span className="vt-meta">{meta}</span>
      <span className="vt-acoes-hover">
        <button type="button" className="vt-botao vt-so-icone vt-linha" title="Abrir ao lado" aria-label="Abrir ao lado">
          <Icone nome="columns-2" tamanho={14} />
        </button>
        <button type="button" className="vt-botao vt-so-icone vt-linha" title="Mais ações" aria-label="Mais ações">
          <Icone nome="ellipsis" tamanho={14} />
        </button>
      </span>
    </div>
  );
}

/**
 * @typedef {object} CartaoDeListaProps
 * @property {import('react').ReactNode} titulo O título do chat (truncado com reticências).
 * @property {'parado' | 'rodando' | 'espera' | 'nova' | 'erro'} [estado] O estado, que vira o ícone (veja IconeDeEstado).
 * @property {boolean} [selecionado] O chat à vista no painel ativo.
 * @property {boolean} [abertoEmOutro] Aberto em outro painel: a barra azul a 40%, sem o fundo.
 * @property {boolean} [naoLido] Resposta nova: título em 600 e forte.
 * @property {number} [painel] Número do painel em que está aberto; some a hora.
 * @property {boolean} [painelAtivo] O número azul (o painel ativo).
 * @property {string} [hora] Há quanto tempo foi atualizado: "3 h", "4 d".
 * @property {string} [rodandoHa] Rodando há quanto tempo ("02:13"), em destaque-texto.
 * @property {boolean} [fixado] O alfinete na meta.
 * @property {boolean} [comPonteiro] Mostra o hover: as ações no lugar da meta.
 * @property {boolean} [realce] Realce do teclado (fio destaque-borda).
 * @property {(evento: import('react').MouseEvent<HTMLDivElement>) => void} [onClick]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
