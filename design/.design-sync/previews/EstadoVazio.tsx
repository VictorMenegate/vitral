import { Botao, EstadoVazio } from 'vitral-ui';

export const Amplo = () => <EstadoVazio titulo="Converse com este chat" descricao="Este chat mexe direto em ~/eita." />;

export const Compacto = () => (
  <EstadoVazio compacto icone="search" titulo="Nada encontrado" descricao="Nada encontrado para “cozinha retrô”" />
);

export const ComAcao = () => (
  <EstadoVazio
    compacto
    icone="archive"
    titulo="Nenhum chat arquivado"
    descricao="Chats parados há 7 dias vêm para cá."
    acoes={<Botao tamanho="barra" icone="settings">Configurações</Botao>}
  />
);
