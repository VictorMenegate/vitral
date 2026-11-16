import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * A citação em bloco ("> ...") da resposta com o **Copiar** no canto, como o bloco de código: é onde
 * o agente escreve a mensagem pronta para mandar a outra pessoa. Fio de 2 px à esquerda em
 * `borda-forte`, recuo de 12 e 32 à direita; texto em `corpo` na cor `texto` (negrito em 700 e
 * `forte`). `tipo="codigo"` dá o bloco de código: fundo `preto`, canto `raio-cartao`, mono 13/20,
 * 40 à direita guardados para o botão. `copiado` troca o ícone pelo visto em `verde` (2 s no app).
 * @param {CitacaoComCopiarProps} props
 */
export function CitacaoComCopiar({ tipo = 'citacao', copiado = false, dica, children, className, style, ...resto }) {
  const copiar = (
    <span className="vt-copiar">
      <button
        type="button"
        className="vt-botao vt-so-icone vt-linha"
        title={dica ?? (tipo === 'codigo' ? 'Copiar o código' : 'Copiar a citação')}
        aria-label={dica ?? (tipo === 'codigo' ? 'Copiar o código' : 'Copiar a citação')}
        style={copiado ? { color: 'var(--verde)' } : undefined}
      >
        <Icone nome={copiado ? 'check' : 'copy'} tamanho={14} />
      </button>
    </span>
  );
  if (tipo === 'codigo') {
    return (
      <pre className={cx('vt-codigo-bloco', className)} style={style} {...resto}>
        {children}
        {copiar}
      </pre>
    );
  }
  return (
    <blockquote className={cx('vt-citacao', 'vt-tipo-corpo', className)} style={{ margin: 0, ...style }} {...resto}>
      {children}
      {copiar}
    </blockquote>
  );
}

/**
 * @typedef {object} CitacaoComCopiarProps
 * @property {'citacao' | 'codigo'} [tipo] "citacao" (padrão) ou "codigo" (o bloco de código no preto).
 * @property {boolean} [copiado] O visto verde no lugar do Copiar.
 * @property {string} [dica] A dica do Copiar.
 * @property {import('react').ReactNode} children O texto citado, ou o código (use spans com color: var(--sintaxe-*) para realce).
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
