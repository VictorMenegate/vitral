// vitral-ui: os componentes do Vitral (Rust + GPUI) recriados em React para o Claude Design.
// Cada um monta as classes vt-* de dist/vitral-ui.css, que só usam os tokens de tokens/tokens.css.

// Base
export { Icone } from './base/Icone.jsx';
export { Botao } from './base/Botao.jsx';
export { BotaoRedondo } from './base/BotaoRedondo.jsx';
export { Selo } from './base/Selo.jsx';
export { Etiqueta } from './base/Etiqueta.jsx';
export { Teclas } from './base/Teclas.jsx';
export { Interruptor } from './base/Interruptor.jsx';
export { Alerta } from './base/Alerta.jsx';
export { CampoDeBusca } from './base/CampoDeBusca.jsx';

// Painel
export { AbaDePainel } from './painel/AbaDePainel.jsx';
export { CabecalhoDoPainel } from './painel/CabecalhoDoPainel.jsx';
export { AnelDeUso } from './painel/AnelDeUso.jsx';

// Lista
export { CartaoDeLista } from './lista/CartaoDeLista.jsx';
export { IconeDeEstado } from './lista/IconeDeEstado.jsx';
export { RotuloDeSecao } from './lista/RotuloDeSecao.jsx';

// Caixa de mensagem
export { CaixaDeMensagem } from './caixa/CaixaDeMensagem.jsx';
export { AnelDeContexto } from './caixa/AnelDeContexto.jsx';
export { ChipDaCaixa } from './caixa/ChipDaCaixa.jsx';
export { FaixaDoTurno } from './caixa/FaixaDoTurno.jsx';

// Conversa
export { BalaoDoUsuario } from './conversa/BalaoDoUsuario.jsx';
export { RespostaDoAgente } from './conversa/RespostaDoAgente.jsx';
export { TrilhaDeFerramentas } from './conversa/TrilhaDeFerramentas.jsx';
export { CartaoDeErro } from './conversa/CartaoDeErro.jsx';
export { CitacaoComCopiar } from './conversa/CitacaoComCopiar.jsx';
export { EstadoVazio } from './conversa/EstadoVazio.jsx';
export { MarcoDaConversa } from './conversa/MarcoDaConversa.jsx';
export { MensagemRecebida } from './conversa/MensagemRecebida.jsx';
export { PedidoDePermissao } from './conversa/PedidoDePermissao.jsx';
export { PilhaDeImagens } from './conversa/PilhaDeImagens.jsx';
export { PilulaDeArquivo } from './conversa/PilulaDeArquivo.jsx';

// Escolha
export { ControleSegmentado } from './escolha/ControleSegmentado.jsx';
export { GrupoDeOpcoes } from './escolha/GrupoDeOpcoes.jsx';
export { HistoricoDoMais } from './escolha/HistoricoDoMais.jsx';
export { ListaDeEscolha } from './escolha/ListaDeEscolha.jsx';
export { MenuComPlaca } from './escolha/MenuComPlaca.jsx';
export { PopoverDeEsforco } from './escolha/PopoverDeEsforco.jsx';

// Marca
export { Logo } from './marca/Logo.jsx';
export { Marca } from './marca/Marca.jsx';

// Materiais
export { Metal } from './materiais/Metal.jsx';
export { Materiais } from './materiais/Materiais.jsx';
export { RaizVitral } from './materiais/RaizVitral.jsx';
