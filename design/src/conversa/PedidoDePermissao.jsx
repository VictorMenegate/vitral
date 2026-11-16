import * as React from 'react';
import { cx } from '../util.js';
import { Botao } from '../base/Botao.jsx';
import { Icone } from '../base/Icone.jsx';

/**
 * O que espera a sua decisão antes de o agente agir: um miolo de raio 16 (`cartao-elevado`, opaco)
 * dentro de uma moldura de raio 20 com 4 de folga (a moldura dupla). **Esperando você**, a moldura é
 * o **metal vivo** (o aro de 4 px): nunca halo nem borda azul. No comando **perigoso**, a moldura é
 * `moldura-perigo` (vermelho a 35%, nunca metal) e o Permitir vira "Segure para permitir" (900 ms,
 * enchendo da esquerda para a direita). Cabeçalho: o azulejo de 28 com o ícone da ferramenta, o
 * título em `titulo-cartao` `forte` e a etiqueta mono da ferramenta. O pedido (comando, diff) no
 * preto. Botões: Negar (fantasma) e **Permitir** no botão claro.
 * @param {PedidoDePermissaoProps} props
 */
export function PedidoDePermissao({
  titulo = 'Rodar um comando',
  ferramenta = 'Bash',
  icone = 'terminal',
  posicao,
  perigoso = false,
  recolhido = false,
  children,
  onPermitir,
  onNegar,
  className,
  style,
  ...resto
}) {
  const moldura = recolhido ? { background: 'var(--preto)', boxShadow: 'inset 0 0 0 1px var(--borda)' } : null;
  return (
    <div role="group" aria-label={titulo} className={cx('vt-moldura-dupla', perigoso && 'vt-perigo', className)} style={{ ...moldura, ...style }} {...resto}>
      {!perigoso && !recolhido ? <span className="vt-aro" /> : null}
      <div className="vt-miolo-pedido">
        <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
          <span
            style={{
              width: 28,
              height: 28,
              borderRadius: 'var(--raio-controle)',
              background: 'var(--hover)',
              display: 'flex',
              alignItems: 'center',
              justifyContent: 'center',
              color: perigoso ? 'var(--vermelho)' : 'var(--tenue)',
              flex: 'none',
            }}
          >
            <Icone nome={icone} tamanho={14} />
          </span>
          <span className="vt-tipo-titulo-cartao" style={{ flex: 1, minWidth: 0, color: 'var(--forte)' }}>
            {titulo}
          </span>
          {posicao ? (
            <span className="vt-tipo-metadado" style={{ color: 'var(--tenue)' }}>
              {posicao}
            </span>
          ) : null}
          <span className="vt-tipo-etiqueta" style={{ color: 'var(--apagado)' }}>
            {ferramenta}
          </span>
        </div>
        {children != null ? (
          <pre className="vt-codigo-bloco" style={{ paddingRight: 11 }}>
            {children}
          </pre>
        ) : null}
        <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8 }}>
          <Botao onClick={onNegar}>Negar</Botao>
          {perigoso ? (
            <button type="button" className="vt-segurar" onClick={onPermitir}>
              <i className="vt-enche" />
              <span className="vt-rotulo">Segure para permitir</span>
            </button>
          ) : (
            <Botao variante="claro" onClick={onPermitir}>
              Permitir
            </Botao>
          )}
        </div>
      </div>
    </div>
  );
}

/**
 * @typedef {object} PedidoDePermissaoProps
 * @property {import('react').ReactNode} [titulo] "Rodar um comando", "Editar um arquivo". Padrão "Rodar um comando".
 * @property {string} [ferramenta] A ferramenta em mono à direita: "Bash", "Edit", "Write". Padrão "Bash".
 * @property {string} [icone] Ícone Lucide de 14 no azulejo: terminal (padrão), file-pen-line, file-plus, globe.
 * @property {string} [posicao] "1 de 3" quando há vários pedidos.
 * @property {boolean} [perigoso] Comando perigoso: moldura vermelha e "Segure para permitir".
 * @property {boolean} [recolhido] Há outro pedido aberto: moldura preta com fio, sem metal.
 * @property {import('react').ReactNode} [children] O pedido no preto ("$ cargo test tema"); o trecho perigoso em vermelho sobre --diff-sai.
 * @property {() => void} [onPermitir]
 * @property {() => void} [onNegar]
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
