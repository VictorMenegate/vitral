import * as React from 'react';
import { cx } from '../util.js';
import { Icone } from '../base/Icone.jsx';

/**
 * Um arquivo na conversa, como a extensão do Claude Code mostra (regra do dono): uma pílula de 28
 * com a miniatura de 20 de altura (ou o ícone), o nome em `texto` (até 220, com reticências) e a
 * medida em `tenue` ("601×263", "184 KB"). Fora do balão: `cartao-elevado` com fio `borda-forte`.
 * **Dentro do seu balão** (`noBalao`): o poço escuro (`recuo` com fio `borda`). O que saiu do disco
 * fica apagado (`sumiu`): ImageOff em `apagado`, nome em `tenue`, "sumiu". Duas imagens ou mais
 * viram a PilhaDeImagens.
 * @param {PilulaDeArquivoProps} props
 */
export function PilulaDeArquivo({ nome, medida, imagem, icone = 'file', sumiu = false, noBalao = false, className, style, ...resto }) {
  const comImagem = !!imagem && !sumiu;
  return (
    <span
      title={medida ? `${nome} · ${medida}` : nome}
      className={cx('vt-pilula-arq', comImagem && 'vt-com-imagem', noBalao && 'vt-no-balao', sumiu && 'vt-sumiu', className)}
      style={style}
      {...resto}
    >
      {comImagem ? (
        <span className="vt-mini" style={{ width: 36 }}>
          <img src={imagem} alt="" style={{ width: '100%', height: '100%', objectFit: 'cover', objectPosition: '30% 40%', display: 'block' }} />
        </span>
      ) : (
        <Icone nome={sumiu ? (imagem ? 'image-off' : 'file-x') : icone} tamanho={14} cor={sumiu ? 'var(--apagado)' : 'var(--tenue)'} />
      )}
      <span className="vt-nome">{nome}</span>
      {sumiu ? <span className="vt-medida">sumiu</span> : medida ? <span className="vt-medida">{medida}</span> : null}
    </span>
  );
}

/**
 * @typedef {object} PilulaDeArquivoProps
 * @property {string} nome O nome do arquivo: "colado-b51d81ef3e.png", "relatorio.pdf".
 * @property {string} [medida] As medidas da imagem ("601×263") ou o tamanho do arquivo ("184 KB").
 * @property {string} [imagem] URL da imagem: mostra a miniatura. Num arquivo que não é imagem, deixe vazio.
 * @property {string} [icone] Ícone Lucide do arquivo sem miniatura. Padrão "file".
 * @property {boolean} [sumiu] Saiu do disco: ícone apagado e "sumiu" no lugar da medida.
 * @property {boolean} [noBalao] Dentro do balão do usuário: o poço escuro.
 * @property {string} [className]
 * @property {import('react').CSSProperties} [style]
 */
