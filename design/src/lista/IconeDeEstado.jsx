import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

const DISCO = {
  espera: { cor: 'var(--espera)', fundo: 'var(--estado-espera)' },
  nova: { cor: 'var(--destaque)', fundo: 'var(--estado-nova)' },
  erro: { cor: 'var(--vermelho)', fundo: 'var(--estado-erro)' },
};
const PILULA = {
  rodando: { rotulo: 'rodando', fundo: 'var(--tinta-destaque)', cor: 'var(--destaque-texto)' },
  espera: { rotulo: 'esperando você', fundo: 'color-mix(in srgb, var(--espera) 12%, transparent)', cor: 'var(--espera)' },
  nova: { rotulo: 'resposta nova', fundo: 'var(--tinta-destaque)', cor: 'var(--destaque-texto)' },
  pronto: { rotulo: 'pronto', fundo: 'color-mix(in srgb, var(--verde) 12%, transparent)', cor: 'var(--verde)' },
  erro: { rotulo: 'falhou', fundo: 'var(--tinta-vermelho)', cor: 'var(--vermelho)' },
  parado: { rotulo: 'parada', fundo: 'var(--hover)', cor: 'var(--tenue)' },
};
const PONTO = { erro: 'var(--vermelho)', espera: 'var(--espera)', nova: 'var(--destaque)', pronto: 'var(--verde)' };

/**
 * O ícone de um chat é o estado dele, na aba, na linha da lista, no sino e no histórico. Na ordem de
 * prioridade: **erro** (disco vermelho com ponto), **espera** (pedido de permissão ou parada que pede
 * Retomar: disco azul-claro `espera`, nunca âmbar), **nova** (resposta não lida: disco `destaque`),
 * **rodando** (o anel girando em `destaque`) e **parado** (o balão MessageCircle em `apagado`). Caixa
 * de 20 (16 na aba). `forma="ponto"` dá o ponto de sinal de 8; `forma="pilula"` dá a pílula de estado
 * de 22 com o rótulo minúsculo ("rodando", "esperando você") e o `detalhe` depois de " · ".
 * @param {IconeDeEstadoProps} props
 */
export function IconeDeEstado({ estado = 'parado', forma = 'icone', tamanho = 20, detalhe, children, className, style, ...resto }) {
  if (forma === 'pilula') {
    const p = PILULA[estado] ?? PILULA.parado;
    return (
      <span className={cx('vt-pilula-estado', className)} style={{ background: p.fundo, color: p.cor, ...style }} {...resto}>
        {children ?? p.rotulo}
        {detalhe ? <span style={{ color: 'var(--tenue)' }}>&nbsp;· {detalhe}</span> : null}
      </span>
    );
  }
  if (forma === 'ponto') {
    const rodando = estado === 'rodando';
    return (
      <span
        className={cx('vt-ponto', className)}
        style={rodando ? { border: '2px solid var(--destaque)', ...style } : { background: PONTO[estado] ?? 'var(--apagado)', ...style }}
        {...resto}
      />
    );
  }
  const pequeno = tamanho === 16;
  const caixa = pequeno ? { width: 16, height: 16 } : null;
  if (estado === 'rodando') {
    const lado = pequeno ? 13 : 16;
    return (
      <span className={cx('vt-estado', className)} style={{ ...caixa, ...style }} role="img" aria-label="rodando" {...resto}>
        <span className="vt-girando" style={{ width: lado, height: lado }} />
      </span>
    );
  }
  const d = DISCO[estado];
  if (d) {
    return (
      <span
        className={cx('vt-estado', pequeno && 'vt-16', className)}
        style={{ '--vt-cor': d.cor, ...style }}
        role="img"
        aria-label={estado === 'espera' ? 'esperando você' : estado === 'nova' ? 'resposta nova' : 'erro'}
        {...resto}
      >
        <span className="vt-disco" style={{ background: d.fundo }} />
      </span>
    );
  }
  return (
    <span className={cx('vt-estado', className)} style={{ ...caixa, color: 'var(--apagado)', ...style }} {...resto}>
      <Icone nome="message-circle" tamanho={pequeno ? 14 : 18} />
    </span>
  );
}

/**
 * @typedef {object} IconeDeEstadoProps
 * @property {'parado' | 'rodando' | 'espera' | 'nova' | 'erro' | 'pronto'} [estado] O estado do chat; padrão "parado". "espera" precisa de você; "nova" não lida; "pronto" só no ponto e na pílula.
 * @property {'icone' | 'ponto' | 'pilula'} [forma] "icone" (padrão, o ícone do chat), "ponto" (o ponto de sinal de 8) ou "pilula" (a pílula de estado de 22).
 * @property {20 | 16} [tamanho] Caixa do ícone: 20 (padrão, lista e histórico) ou 16 (aba).
 * @property {import('react').ReactNode} [detalhe] Na pílula, o que vem depois de " · " em `tenue` ("2 min 13 s").
 * @property {import('react').ReactNode} [children] Na pílula, troca o rótulo padrão do estado.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
