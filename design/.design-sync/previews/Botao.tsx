import { Botao } from 'vitral-ui';

const fileira = { display: 'flex', gap: 8, alignItems: 'center', flexWrap: 'wrap' } as const;

export const Variantes = () => (
  <div style={fileira}>
    <Botao variante="primario" icone="arrow-right">Continuar</Botao>
    <Botao icone="play">Retomar</Botao>
    <Botao icone="rotate-cw">Tentar de novo</Botao>
    <Botao variante="perigo">Apagar a cópia</Botao>
    <Botao variante="claro">Permitir</Botao>
  </div>
);

export const Tamanhos = () => (
  <div style={fileira}>
    <Botao tamanho="linha" icone="chevron-down">Detalhes</Botao>
    <Botao tamanho="barra" icone="minimize">Compactar</Botao>
    <Botao icone="folder-open">Abrir pasta</Botao>
  </div>
);

export const SoIcone = () => (
  <div style={fileira}>
    <Botao tamanho="linha" icone="copy" dica="Copiar" />
    <Botao tamanho="barra" icone="plus" dica="Novo chat" />
    <Botao icone="paperclip" dica="Anexar" />
    <Botao tamanho="barra" icone="check" dica="Aplicado" pressionado />
  </div>
);

export const DesabilitadoEAviso = () => (
  <div style={fileira}>
    <Botao icone="download" desabilitado dica="Sem internet para baixar">Instalar</Botao>
    <Botao variante="aviso" tamanho="barra">Desfazer</Botao>
  </div>
);
