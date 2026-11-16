import { PedidoDePermissao } from 'vitral-ui';

const largura = { maxWidth: 600 } as const;

export const EsperandoVoce = () => (
  <PedidoDePermissao style={largura} titulo="Rodar um comando" ferramenta="Bash">
    $ cargo test tema
  </PedidoDePermissao>
);

export const ComandoPerigoso = () => (
  <PedidoDePermissao style={largura} perigoso titulo="Rodar um comando" ferramenta="Bash">
    $ rm -rf <span style={{ color: 'var(--vermelho)', background: 'var(--diff-sai)' }}>~/eita/dados</span>
  </PedidoDePermissao>
);

export const EdicaoUmDeTres = () => (
  <PedidoDePermissao style={largura} titulo="Editar um arquivo" ferramenta="Edit" icone="file-pen-line" posicao="1 de 3">
    <span style={{ color: 'var(--apagado)' }}>src/ui/mensagens.rs</span>
    {'\n'}
    <span style={{ color: 'var(--vermelho)', background: 'var(--diff-sai)' }}>- .rounded(raio::PECA)</span>
    {'\n'}
    <span style={{ color: 'var(--verde)', background: 'var(--diff-entra)' }}>+ .rounded(raio::DA_PECA)</span>
  </PedidoDePermissao>
);
