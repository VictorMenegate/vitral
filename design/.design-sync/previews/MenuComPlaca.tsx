import { MenuComPlaca } from 'vitral-ui';

export const MenuDoLogo = () => (
  <MenuComPlaca
    titulo="Vitral"
    detalhe="25 chats"
    realcado={1}
    rodape="Vitral 0.7.0 · pc"
    itens={[
      { icone: 'panel-left-close', rotulo: 'Recolher a lista', teclas: ['Ctrl', 'B'] },
      { icone: 'message-square-plus', rotulo: 'Novo chat', teclas: ['Ctrl', 'N'] },
      { icone: 'folder-open', rotulo: 'Novo chat em outra pasta…' },
      { icone: 'download', rotulo: 'Importar do Claude Code…' },
      'separador',
      { icone: 'circle-user', rotulo: 'Contas do Claude' },
      { icone: 'container', rotulo: 'Servidores MCP' },
      { icone: 'settings', rotulo: 'Configurações', teclas: ['Ctrl', ','] },
      { icone: 'keyboard', rotulo: 'Atalhos do teclado', teclas: ['?'] },
      'separador',
      { icone: 'folder-open', rotulo: 'Abrir a pasta de dados', externo: true },
    ]}
  />
);

export const MenuDoChat = () => (
  <MenuComPlaca
    largura={300}
    titulo="Pesagem de caixas"
    detalhe="~/eita · main"
    simbolo={<span className="vt-i vt-i20 vt-i-message-circle" style={{ color: 'var(--forte)' }} />}
    itens={[
      { icone: 'columns-2', rotulo: 'Abrir ao lado' },
      { icone: 'pin', rotulo: 'Fixar chat' },
      { icone: 'pencil', rotulo: 'Renomear', teclas: ['F2'] },
      'separador',
      { icone: 'archive', rotulo: 'Arquivar' },
      { icone: 'trash', rotulo: 'Apagar', perigoso: true },
    ]}
    realcado={5}
  />
);
