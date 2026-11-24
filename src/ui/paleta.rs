//! A paleta de comandos (Ctrl+Shift+P): tudo o que o Vitral faz, numa lista só com busca, para
//! chegar a qualquer função pelo teclado sem lembrar em qual menu ela ficou. Duas fontes:
//!
//! - a tabela de `atalhos.rs`: cada linha com rótulo entra com as teclas dela, e o Enter dispara a
//!   mesma ação do atalho, a partir da caixa do painel ativo;
//! - os comandos que só existem em menu (`Direto`): os do menu do logo, do "…" da caixa e do menu
//!   da conversa, que chamam o mesmo método que o item do menu chama.
//!
//! O que não vale agora aparece apagado, com o motivo embaixo. A montagem da lista é pura
//! (`entradas`), sobre uma foto do momento (`Momento`), e os testes estão no fim.
//!
//! Com algo escrito, a lista acha também as conversas (pelo título) e os arquivos da pasta do chat
//! ativo (pelo nome), em duas seções depois dos comandos (`grupos_com_achados`). O Ctrl+P abre a
//! lista só dos arquivos (`abrir_arquivo_pelo_nome`), e o Enter abre o escolhido à direita.

use std::{cell::RefCell, path::Path, rc::Rc};

use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use super::{
    Janela, acentos,
    atalhos::{self, Grupo},
    componentes::{EventoEscolha, GrupoDeEscolha, ItemDeEscolha, ListaDeEscolha, Simbolo, Tom, aviso, desenho_teclas, fio_de_pontos, largura},
    dialogos::{cabecalho, com_til, moldura, nome_da_pasta, topo},
    seletor_arquivos,
};
use crate::{
    modelo::Agente,
    tema::{Cores, Lucide, Tipo as _, r},
};

// Ctrl+Shift+P: a paleta de comandos. Ctrl+P: a lista dos arquivos da pasta do chat ativo.
actions!(vitral, [AbrirPaleta, AbrirArquivoPeloNome]);

/// Os arquivos da pasta para uma lista de escolha: chegam do segundo plano quando ainda não
/// foram lidos, e a lista os consulta a cada letra.
type ArquivosDaLista = Rc<RefCell<Option<Rc<Vec<String>>>>>;

/// O rótulo da linha da própria paleta na tabela de atalhos (ela aparece apagada dentro dela).
pub(super) const ROTULO_DA_PALETA: &str = "Paleta de comandos";

/// Um comando que não tem atalho: mora num menu (o do logo, o "…" da caixa, o da conversa).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Direto {
    MandarDepois,
    PerguntarAoOutro,
    Compactar,
    Agentes,
    SessaoNova,
    Bifurcar,
    Renomear,
    Fixar,
    Arquivar,
    NovoChatEmOutraPasta,
    Importar,
    ModoFoco,
    PastaDeTrabalho,
    Contas,
    ServidoresMcp,
    MemoriaDaEquipe,
    ConsultarMemoria,
    Reiniciar,
    PastaDeDados,
}

impl Direto {
    const TODOS: [Direto; 19] = [
        Direto::MandarDepois,
        Direto::PerguntarAoOutro,
        Direto::Compactar,
        Direto::Agentes,
        Direto::SessaoNova,
        Direto::Bifurcar,
        Direto::Renomear,
        Direto::Fixar,
        Direto::Arquivar,
        Direto::NovoChatEmOutraPasta,
        Direto::Importar,
        Direto::ModoFoco,
        Direto::PastaDeTrabalho,
        Direto::Contas,
        Direto::ServidoresMcp,
        Direto::MemoriaDaEquipe,
        Direto::ConsultarMemoria,
        Direto::Reiniciar,
        Direto::PastaDeDados,
    ];

    /// A seção da paleta em que ele aparece (as mesmas do diálogo de atalhos).
    fn grupo(self) -> Grupo {
        use Direto::*;
        match self {
            MandarDepois | PerguntarAoOutro | Compactar | Agentes | SessaoNova | Bifurcar | Renomear | Fixar | Arquivar => {
                Grupo::Conversa
            }
            NovoChatEmOutraPasta | Importar => Grupo::Lista,
            ModoFoco => Grupo::Paineis,
            PastaDeTrabalho | Contas | ServidoresMcp | MemoriaDaEquipe | ConsultarMemoria | Reiniciar | PastaDeDados => Grupo::Janela,
        }
    }

    /// O nome na lista, o mesmo do menu de onde ele veio.
    fn rotulo(self, m: &Momento) -> String {
        use Direto::*;
        let chat = m.chat.as_ref();
        match self {
            MandarDepois => "Mandar depois, na fila".into(),
            PerguntarAoOutro => format!("Perguntar também ao {}", chat.map_or(Agente::Codex, |c| c.agente.outro()).nome()),
            Compactar => "Compactar a conversa".into(),
            Agentes => "Agentes desta conversa".into(),
            SessaoNova => "Sessão nova".into(),
            Bifurcar => "Continuar em outro chat a partir da última resposta".into(),
            Renomear => "Renomear a conversa…".into(),
            Fixar if chat.is_some_and(|c| c.fixado) => "Desafixar a conversa".into(),
            Fixar => "Fixar a conversa".into(),
            Arquivar if chat.is_some_and(|c| c.arquivado) => "Desarquivar a conversa".into(),
            Arquivar => "Arquivar a conversa".into(),
            NovoChatEmOutraPasta => "Novo chat em outra pasta…".into(),
            Importar => "Importar do Claude Code…".into(),
            ModoFoco if chat.is_some_and(|c| c.modo_foco) => "Sair do modo foco (todos os chats)".into(),
            ModoFoco => "Modo foco em todos os chats".into(),
            PastaDeTrabalho => "Escolher a pasta de trabalho…".into(),
            Contas => "Contas do Claude".into(),
            ServidoresMcp => "Servidores MCP".into(),
            MemoriaDaEquipe => "Memória da equipe…".into(),
            ConsultarMemoria => "Consultar memória".into(),
            Reiniciar => match &m.versao_pronta {
                Some(versao) => format!("Reiniciar para a {versao}"),
                None => "Reiniciar para a versão nova".into(),
            },
            PastaDeDados => "Abrir a pasta de dados".into(),
        }
    }

    /// Outras palavras que a busca acha (o que a pessoa digitaria procurando por ele).
    fn termos(self) -> &'static str {
        use Direto::*;
        match self {
            MandarDepois => "fila esperar",
            PerguntarAoOutro => "segunda resposta raia comparar",
            Compactar => "compact resumir contexto",
            Agentes => "mapa subagentes",
            SessaoNova => "clear limpar recomeçar",
            Bifurcar => "bifurcar ramo fork copiar conversa",
            Renomear => "título nome",
            Fixar => "pin",
            Arquivar => "guardar",
            NovoChatEmOutraPasta => "nova conversa abrir pasta",
            Importar => "trazer conversas",
            ModoFoco => "esconder ferramentas",
            PastaDeTrabalho => "trocar pasta",
            Contas => "login plano uso",
            ServidoresMcp => "mcp ferramentas",
            MemoriaDaEquipe => "ai-memory chave",
            ConsultarMemoria => "buscar memória equipe",
            Reiniciar => "atualizar versão",
            PastaDeDados => "arquivos do vitral",
        }
    }

    /// Por que ele não vale agora; `None` quando vale.
    fn motivo(self, m: &Momento) -> Option<String> {
        use Direto::*;
        let sem_conversa = || "Abra uma conversa no painel ativo".to_string();
        match self {
            NovoChatEmOutraPasta | Importar | PastaDeTrabalho | Contas | ServidoresMcp | MemoriaDaEquipe | PastaDeDados => None,
            ConsultarMemoria => (!m.memoria_ligada).then(|| "Ligue antes a memória da equipe".into()),
            Reiniciar => m.versao_pronta.is_none().then(|| "Não há versão nova esperando".into()),
            _ => {
                let Some(c) = m.chat.as_ref() else {
                    return Some(sem_conversa());
                };
                match self {
                    MandarDepois if !c.dirige => Some("Só com o turno rodando (parado, o Enter já envia)".into()),
                    MandarDepois if c.caixa_vazia => Some("Escreva a mensagem na caixa antes".into()),
                    PerguntarAoOutro if !c.outro_instalado => Some(format!("O {} não está instalado", c.agente.outro().nome())),
                    PerguntarAoOutro if c.caixa_vazia || c.caixa_com_barra => Some("Escreva a pergunta na caixa antes".into()),
                    Compactar if c.agente == Agente::Gemini => Some("O Gemini não tem o /compact".into()),
                    Compactar if !c.com_sessao => Some("A conversa ainda não tem sessão".into()),
                    Agentes if !c.tem_agentes => Some("Esta conversa não abriu agentes".into()),
                    SessaoNova if c.rodando => Some("Espere o turno terminar".into()),
                    Bifurcar if !c.tem_resposta => Some("A conversa ainda não tem resposta".into()),
                    Bifurcar if c.rodando => Some("Espere o turno terminar".into()),
                    _ => None,
                }
            }
        }
    }
}

/// O chat do painel ativo, como a paleta o vê ao abrir.
#[derive(Clone, Debug, Default, PartialEq)]
struct ChatAgora {
    agente: Agente,
    /// O parceiro padrão deste agente (`Agente::outro`) está instalado.
    outro_instalado: bool,
    /// A sessão do agente já existe (há o que compactar).
    com_sessao: bool,
    rodando: bool,
    /// O Enviar entraria no turno que está rodando (aí o "Mandar depois" faz diferença).
    dirige: bool,
    fixado: bool,
    arquivado: bool,
    tem_agentes: bool,
    caixa_vazia: bool,
    caixa_com_barra: bool,
    modo_foco: bool,
    /// A conversa tem pelo menos uma resposta (de onde bifurcar).
    tem_resposta: bool,
}

/// A foto do app no instante em que a paleta abre: decide os rótulos e o que fica apagado.
#[derive(Clone, Debug, Default, PartialEq)]
struct Momento {
    chat: Option<ChatAgora>,
    memoria_ligada: bool,
    versao_pronta: Option<String>,
    /// Os arranjos de painéis salvos (`arranjo/salvos.rs`) e o que está em uso.
    arranjos: Vec<String>,
    arranjo_em_uso: Option<String>,
}

/// Uma linha da paleta.
#[derive(Clone, Debug, PartialEq)]
struct Entrada {
    /// "atalho:<posição na tabela>", "direto:<posição em `Direto::TODOS`>" ou "arranjo:<nome>".
    chave: String,
    titulo: String,
    grupo: Grupo,
    teclas: Option<Vec<SharedString>>,
    /// Por que não vale agora.
    motivo: Option<String>,
    /// O que a busca olha, sem acento: o título, as outras palavras e a seção.
    busca: String,
}

/// O atalho só vale num lugar que a paleta não alcança (ela devolve o foco à caixa do painel
/// ativo): o motivo que vai embaixo da linha apagada.
fn motivo_do_contexto(contexto: Option<&str>) -> Option<&'static str> {
    match contexto? {
        c if c.starts_with("Menu") => Some("Só com o menu “/” da caixa à vista"),
        "Esforco" => Some("Só dentro do seletor de esforço"),
        "Visor" => Some("Só com o visor de imagem aberto"),
        "Gaveta" => Some("Só com o foco numa gaveta aberta"),
        "BuscaConversa" => Some("Só com a busca da conversa aberta"),
        _ => None,
    }
}

/// Todas as linhas da paleta: as da tabela de atalhos com rótulo, depois os comandos de menu. Em
/// cada seção, o que vale agora vem antes do que está apagado.
fn entradas(m: &Momento) -> Vec<Entrada> {
    let busca_de = |titulo: &str, termos: &str, grupo: Grupo| acentos::normalizar(&format!("{titulo} {termos} {}", grupo.titulo()));
    let mut todas: Vec<Entrada> = atalhos::linhas()
        .into_iter()
        .enumerate()
        .filter(|(_, l)| !l.rotulo.is_empty())
        .map(|(i, l)| Entrada {
            chave: format!("atalho:{i}"),
            titulo: l.rotulo.to_string(),
            grupo: l.grupo,
            teclas: Some(l.partes()),
            motivo: if l.rotulo == ROTULO_DA_PALETA {
                Some("É esta lista".into())
            } else {
                motivo_do_contexto(l.contexto).map(str::to_string)
            },
            busca: busca_de(l.rotulo, "", l.grupo),
        })
        .collect();
    todas.extend(Direto::TODOS.iter().enumerate().map(|(i, d)| {
        let titulo = d.rotulo(m);
        Entrada {
            chave: format!("direto:{i}"),
            busca: busca_de(&titulo, d.termos(), d.grupo()),
            titulo,
            grupo: d.grupo(),
            teclas: None,
            motivo: d.motivo(m),
        }
    }));
    // Um "ir para" por arranjo salvo: trocar de frente de trabalho sem passar pelo menu do logo.
    todas.extend(m.arranjos.iter().map(|nome| {
        let titulo = format!("Arranjo: {nome}");
        Entrada {
            chave: format!("arranjo:{nome}"),
            busca: busca_de(&titulo, "painéis trocar frente de trabalho", Grupo::Paineis),
            titulo,
            grupo: Grupo::Paineis,
            teclas: None,
            motivo: (m.arranjo_em_uso.as_ref() == Some(nome)).then(|| "É o arranjo em uso".to_string()),
        }
    }));
    // Estável: dentro de cada metade, a ordem da tabela.
    todas.sort_by_key(|e| e.motivo.is_some());
    todas
}

/// As seções da paleta para uma busca (sem acento e por palavras, em qualquer ordem).
fn grupos(todas: &[Entrada], busca: &str) -> Vec<GrupoDeEscolha> {
    let palavras = acentos::palavras(busca);
    Grupo::TODOS
        .iter()
        .filter_map(|grupo| {
            let itens: Vec<ItemDeEscolha> = todas
                .iter()
                .filter(|e| e.grupo == *grupo && acentos::tem_todas(&e.busca, &palavras))
                .map(|e| ItemDeEscolha {
                    descricao: e.motivo.clone().map(SharedString::from),
                    desabilitado: e.motivo.clone().map(SharedString::from),
                    teclas: e.teclas.clone(),
                    ..ItemDeEscolha::new(e.chave.clone(), e.titulo.clone())
                })
                .collect();
            (!itens.is_empty()).then(|| GrupoDeEscolha { titulo: Some(grupo.titulo().into()), itens })
        })
        .collect()
}

/// Os títulos das duas seções que só aparecem com algo escrito. A dos comandos da lista já se
/// chama "Conversas" (a tabela de atalhos), então a das conversas achadas diz o que o Enter faz.
const GRUPO_CONVERSAS: &str = "Ir para a conversa";
const GRUPO_ARQUIVOS: &str = "Abrir arquivo";

/// Quantas conversas e quantos arquivos a paleta mostra de cada vez (o resto, refinando a busca).
const ACHADOS_POR_GRUPO: usize = 6;

/// Uma conversa que a paleta acha pelo título.
#[derive(Clone, Debug, PartialEq)]
struct ConversaNaPaleta {
    id: String,
    titulo: String,
    /// O que vai embaixo do título: a pasta (e "arquivada").
    onde: String,
}

/// As seções da paleta com os achados: os comandos, como em `grupos`, e, com algo escrito, as
/// conversas pelo título e os arquivos da pasta do chat ativo pelo nome (os dois sem acento). Sem
/// busca, só os comandos: nada entra sozinho na lista.
fn grupos_com_achados(todas: &[Entrada], conversas: &[ConversaNaPaleta], arquivos: &[String], busca: &str) -> Vec<GrupoDeEscolha> {
    let mut saida = grupos(todas, busca);
    let palavras = acentos::palavras(busca);
    if palavras.is_empty() {
        return saida;
    }
    let achadas: Vec<ItemDeEscolha> = conversas
        .iter()
        .filter(|c| acentos::tem_todas(&acentos::normalizar(&c.titulo), &palavras))
        .take(ACHADOS_POR_GRUPO)
        .map(|c| ItemDeEscolha {
            simbolo: Some(Simbolo::icone(Lucide::MessageSquare)),
            descricao: (!c.onde.is_empty()).then(|| c.onde.clone().into()),
            ..ItemDeEscolha::new(format!("conversa:{}", c.id), c.titulo.clone())
        })
        .collect();
    if !achadas.is_empty() {
        saida.push(GrupoDeEscolha { titulo: Some(GRUPO_CONVERSAS.into()), itens: achadas });
    }
    // As linhas são as do "@" e do Ctrl+P (o nome em cima, a pasta embaixo), com a chave da paleta.
    let achados: Vec<ItemDeEscolha> = if arquivos.is_empty() {
        Vec::new()
    } else {
        seletor_arquivos::grupos_de_arquivos(Some(arquivos), busca)
            .into_iter()
            .flat_map(|g| g.itens)
            .take(ACHADOS_POR_GRUPO)
            .map(|item| ItemDeEscolha { chave: format!("arquivo:{}", item.chave).into(), ..item })
            .collect()
    };
    if !achados.is_empty() {
        saida.push(GrupoDeEscolha { titulo: Some(GRUPO_ARQUIVOS.into()), itens: achados });
    }
    saida
}

/// O rodapé das listas: o fio e as teclas com o que o Enter faz ("faz", "abre").
fn rodape_das_teclas(enter: &'static str) -> impl Fn(&mut Window, &mut App) -> AnyElement + 'static {
    move |window, _| {
        v_flex()
            .w_full()
            .gap(r(6.))
            .child(fio_de_pontos(Cores::fio(), window))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(r(12.))
                    .w_full()
                    .child(div().flex_1())
                    .child(dica_teclas(&["↑", "↓"], "escolhe"))
                    .child(dica_teclas(&["Enter"], enter)),
            )
            .into_any_element()
    }
}

/// "↑ ↓ escolher": as teclas da lista e o que fazem, no rodapé.
fn dica_teclas(teclas: &[&'static str], texto: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(r(6.))
        .metadado()
        .text_color(Cores::tenue())
        .child(desenho_teclas(teclas.iter().map(|t| SharedString::from(*t)).collect()))
        .child(texto)
}

impl Janela {
    /// A foto do momento para a paleta: o chat do painel ativo e o que depende de fora dele.
    fn momento_da_paleta(&self, cx: &App) -> Momento {
        let Some(motor) = self.motor.as_ref() else {
            return Momento::default();
        };
        let m = motor.read(cx);
        let arranjos = self.arranjos_salvos();
        let painel = self.paineis.get(self.ativo);
        let chat = painel.and_then(|p| m.chat(p.chat.as_deref()?).map(|c| (p, c))).map(|(p, c)| {
            let caixa = p.composer.read(cx).value();
            ChatAgora {
                agente: c.agente,
                outro_instalado: c.agente.outro().instalado(),
                com_sessao: c.sessao_criada,
                rodando: m.execucao(&c.id).rodando(),
                // Também com um pedido de permissão aberto: aí o Enter responde a ele, e a fila
                // é a outra escolha.
                dirige: m.turno_recebe_mensagem(&c.id) || m.enviar_responde_pedido(&c.id),
                fixado: c.fixado,
                arquivado: c.arquivado,
                tem_agentes: super::mapa_agentes::tem_agentes(c),
                caixa_vazia: caixa.trim().is_empty(),
                caixa_com_barra: caixa.trim_start().starts_with('/'),
                modo_foco: m.config.modo_foco,
                tem_resposta: super::bifurcar::ultima_resposta(&c.itens).is_some(),
            }
        });
        Momento {
            chat,
            memoria_ligada: !m.config.memoria_da_equipe.is_empty() && crate::memoria_equipe::ler_chave(&m.dados.config).is_some(),
            versao_pronta: self.atualizacao_pronta.clone(),
            arranjos: arranjos.salvos.into_iter().map(|s| s.nome).collect(),
            arranjo_em_uso: arranjos.em_uso,
        }
    }

    /// A pasta do chat do painel ativo, quando ela existe no disco: é dela a lista do Ctrl+P e a
    /// seção de arquivos da paleta.
    fn pasta_do_chat_ativo(&self, cx: &App) -> Option<std::path::PathBuf> {
        let chat = self.paineis.get(self.ativo)?.chat.as_deref()?;
        self.motor.as_ref()?.read(cx).chat(chat).map(|c| c.cwd.clone()).filter(|p| p.is_dir())
    }

    /// As conversas que a paleta acha pelo título: todas menos a do painel ativo (você já está
    /// nela), da mexida por último à mais antiga, com a pasta embaixo.
    fn conversas_da_paleta(&self, cx: &App) -> Vec<ConversaNaPaleta> {
        let Some(motor) = self.motor.as_ref() else {
            return Vec::new();
        };
        let m = motor.read(cx);
        let ativa = self.paineis.get(self.ativo).and_then(|p| p.chat.as_deref());
        let mut chats: Vec<&crate::modelo::Chat> = m.chats.iter().filter(|c| Some(c.id.as_str()) != ativa).collect();
        chats.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        chats
            .into_iter()
            .map(|c| {
                let pasta = if m.sem_pasta(c) { "Sem pasta".to_string() } else { nome_da_pasta(&c.cwd) };
                ConversaNaPaleta {
                    id: c.id.clone(),
                    titulo: c.titulo.clone(),
                    onde: if c.arquivado { format!("{pasta} · arquivada") } else { pasta },
                }
            })
            .collect()
    }

    /// Os arquivos da pasta para uma lista de escolha. Os que a aba Arquivos e o "@" já leram
    /// valem na hora (e a lista é relida em segundo plano para a próxima vez); sem eles, são
    /// lidos agora, e `ao_chegar` avisa quem está mostrando.
    fn arquivos_para_escolher(
        &self,
        pasta: &Path,
        ao_chegar: impl FnOnce(&Rc<Vec<String>>, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ArquivosDaLista {
        let lidos = self.arquivos_e_pastas_lidos(pasta).map(|(lista, _)| lista).filter(|lista| !lista.is_empty());
        let arquivos: ArquivosDaLista = Rc::new(RefCell::new(lidos.clone()));
        if lidos.is_some() {
            self.ler_arquivos(pasta, cx);
        } else {
            let pasta = pasta.to_path_buf();
            let leitura = cx.background_spawn(async move { seletor_arquivos::listar(&pasta) });
            let guardados = arquivos.clone();
            cx.spawn_in(window, async move |_, cx| {
                let lista = Rc::new(leitura.await);
                *guardados.borrow_mut() = Some(lista.clone());
                let _ = cx.update(|_, cx| ao_chegar(&lista, cx));
            })
            .detach();
        }
        arquivos
    }

    /// Ctrl+P: a lista dos arquivos da pasta do chat ativo, com busca (o nome antes da pasta, sem
    /// acento). O Enter abre o arquivo no painel da direita.
    pub(super) fn abrir_arquivo_pelo_nome(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pasta) = self.pasta_do_chat_ativo(cx) else {
            window.push_notification(aviso(Tom::Info, "Abra uma conversa para escolher um arquivo da pasta dela."), cx);
            return;
        };
        let vista = cx.new(|cx| {
            let mut vista = ListaDeEscolha::new("Nome do arquivo…", window, cx);
            vista.em_dialogo();
            vista.rodape(rodape_das_teclas("abre"));
            vista
        });
        let alvo = vista.downgrade();
        let arquivos = self.arquivos_para_escolher(
            &pasta,
            move |lista, cx| {
                // A leitura chegou com a lista aberta: ela passa a mostrar os arquivos, com o que já foi escrito.
                let _ = alvo.update(cx, |l, cx| {
                    let busca = l.busca(cx);
                    l.definir(seletor_arquivos::grupos_de_arquivos(Some(lista.as_slice()), &busca), None, cx);
                });
            },
            window,
            cx,
        );
        vista.update(cx, |vista, cx| {
            vista.definir(seletor_arquivos::grupos_de_arquivos(arquivos.borrow().as_deref().map(Vec::as_slice), ""), None, cx);
            let lidos = arquivos.clone();
            vista.filtro_proprio(move |busca| {
                let lidos = lidos.borrow().clone();
                seletor_arquivos::grupos_de_arquivos(lidos.as_deref().map(Vec::as_slice), busca)
            });
        });
        let raiz = pasta.clone();
        self._assinatura_dialogo = Some(cx.subscribe_in(&vista, window, move |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(caminho) = evento else {
                return;
            };
            window.close_dialog(cx);
            this.abrir_arquivo(raiz.clone(), caminho.to_string(), window, cx);
        }));
        self.foco_antes_do_dialogo(window, cx);
        let no_dialogo = vista.clone();
        let onde = com_til(&pasta);
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::G, window)
                .title(topo(cabecalho("Abrir arquivo", Some(onde.clone()))))
                .child(no_dialogo.clone())
                .on_ok(|_, _, _| false)
        });
        vista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Ctrl+Shift+P: abre a paleta. O Enter executa no painel ativo; o Esc devolve o foco à caixa.
    pub(super) fn abrir_paleta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let todas = Rc::new(entradas(&self.momento_da_paleta(cx)));
        // O que a busca acha além dos comandos: as conversas e os arquivos da pasta do chat ativo.
        let conversas = Rc::new(self.conversas_da_paleta(cx));
        let arquivos: ArquivosDaLista = match self.pasta_do_chat_ativo(cx) {
            Some(pasta) => self.arquivos_para_escolher(&pasta, |_, _| {}, window, cx),
            None => Rc::default(),
        };
        let vista = cx.new(|cx| {
            let mut vista = ListaDeEscolha::new("Comando, conversa ou arquivo…", window, cx);
            vista.em_dialogo();
            vista.definir(grupos(&todas, ""), None, cx);
            let filtro = todas.clone();
            vista.filtro_proprio(move |busca| {
                let lidos = arquivos.borrow().clone();
                grupos_com_achados(&filtro, &conversas, lidos.as_deref().map_or(&[], Vec::as_slice), busca)
            });
            vista.rodape(rodape_das_teclas("faz"));
            vista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&vista, window, |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(chave) = evento else {
                return;
            };
            window.close_dialog(cx);
            this.executar_da_paleta(chave, window, cx);
        }));
        // O foco passa antes pela caixa do painel ativo: é para ela que ele volta quando a paleta
        // fecha, e é dela que a ação escolhida parte.
        self.foco_antes_do_dialogo(window, cx);
        let no_dialogo = vista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::G, window)
                .title(topo(cabecalho("Paleta de comandos", Some("Tudo o que o Vitral faz, com o atalho de cada coisa".into()))))
                .child(no_dialogo.clone())
                .on_ok(|_, _, _| false)
        });
        vista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Faz o que a linha escolhida diz. A linha da tabela dispara a ação do atalho a partir da
    /// caixa do painel ativo (o mesmo caminho da tecla); o comando de menu chama o método do item.
    fn executar_da_paleta(&mut self, chave: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.foco_antes_do_dialogo(window, cx);
        if let Some(i) = chave.strip_prefix("atalho:").and_then(|n| n.parse::<usize>().ok()) {
            if let Some(acao) = atalhos::acoes().into_iter().nth(i) {
                window.dispatch_action(acao, cx);
            }
            return;
        }
        if let Some(nome) = chave.strip_prefix("arranjo:") {
            self.trocar_de_arranjo(nome, window, cx);
            return;
        }
        // Os achados: a conversa abre no painel ativo, com o foco na caixa; o arquivo, à direita.
        if let Some(chat) = chave.strip_prefix("conversa:") {
            self.selecionar(Some(chat.to_string()), window, cx);
            self.focar_composer(window, cx);
            return;
        }
        if let Some(caminho) = chave.strip_prefix("arquivo:") {
            if let Some(pasta) = self.pasta_do_chat_ativo(cx) {
                self.abrir_arquivo(pasta, caminho.to_string(), window, cx);
            }
            return;
        }
        let Some(direto) = chave.strip_prefix("direto:").and_then(|n| n.parse::<usize>().ok()).and_then(|i| Direto::TODOS.get(i)) else {
            return;
        };
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let painel = self.paineis.get(self.ativo).map(|p| p.id);
        let chat = self.paineis.get(self.ativo).and_then(|p| p.chat.clone());
        match direto {
            Direto::NovoChatEmOutraPasta => self.dialogo_nova_conversa(window, cx),
            Direto::Importar => motor.update(cx, |m, cx| m.importar_do_claude_code(false, cx)),
            Direto::PastaDeTrabalho => self.trocar_pasta_de_trabalho(window, cx),
            Direto::Contas => self.dialogo_contas(window, cx),
            Direto::ServidoresMcp => self.abrir_mcp(window, cx),
            Direto::MemoriaDaEquipe => super::memoria_equipe::abrir(motor, window, cx),
            Direto::ConsultarMemoria => super::memoria_equipe::abrir_busca(motor, window, cx),
            Direto::Reiniciar => self.reiniciar(window, cx),
            Direto::PastaDeDados => {
                let pasta = motor.read(cx).dados.pasta.clone();
                crate::sistema::abrir_pasta(&pasta, cx);
            }
            Direto::ModoFoco => self.alternar_modo_foco(cx),
            _ => {
                let (Some(painel), Some(chat)) = (painel, chat) else {
                    return;
                };
                match direto {
                    Direto::MandarDepois => self.enviar_depois(painel, window, cx),
                    Direto::PerguntarAoOutro => self.enviar_aos_dois(painel, window, cx),
                    Direto::Compactar => self.compactar_conversa(painel, cx),
                    Direto::Agentes => self.abrir_mapa_de_agentes(chat, window, cx),
                    Direto::SessaoNova => motor.update(cx, |m, cx| m.nova_sessao(&chat, cx)),
                    Direto::Bifurcar => {
                        let ultima = motor.read(cx).chat(&chat).and_then(|c| super::bifurcar::ultima_resposta(&c.itens));
                        if let Some(ix) = ultima {
                            self.continuar_em_outro_chat(painel, ix, window, cx);
                        }
                    }
                    Direto::Renomear => self.dialogo_renomear(chat, window, cx),
                    Direto::Fixar => motor.update(cx, |m, cx| m.alternar_fixado(&chat, cx)),
                    Direto::Arquivar => {
                        if motor.read(cx).chat(&chat).is_some_and(|c| c.arquivado) {
                            motor.update(cx, |m, cx| m.alternar_arquivado(&chat, cx));
                        } else {
                            self.arquivar_do_menu(chat, window, cx);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

#[cfg(test)]
mod testes {
    use super::{
        ACHADOS_POR_GRUPO, ChatAgora, ConversaNaPaleta, Direto, Entrada, GRUPO_ARQUIVOS, GRUPO_CONVERSAS, Momento, ROTULO_DA_PALETA,
        entradas, grupos, grupos_com_achados,
    };
    use crate::ui::atalhos::{self, Grupo};

    fn com_chat(chat: ChatAgora) -> Momento {
        Momento { chat: Some(chat), ..Default::default() }
    }

    fn acha<'a>(todas: &'a [Entrada], titulo: &str) -> &'a Entrada {
        todas.iter().find(|e| e.titulo == titulo).unwrap_or_else(|| panic!("sem a linha {titulo}"))
    }

    #[test]
    fn toda_linha_com_rotulo_da_tabela_de_atalhos_esta_na_paleta() {
        let todas = entradas(&Momento::default());
        let linhas = atalhos::linhas();
        let acoes = atalhos::acoes();
        assert_eq!(linhas.len(), acoes.len(), "uma ação por linha da tabela");
        for (i, linha) in linhas.iter().enumerate().filter(|(_, l)| !l.rotulo.is_empty()) {
            let entrada = todas.iter().find(|e| e.chave == format!("atalho:{i}")).unwrap_or_else(|| panic!("fora da paleta: {}", linha.rotulo));
            assert_eq!(entrada.titulo, linha.rotulo);
            assert_eq!(entrada.grupo, linha.grupo);
            assert_eq!(entrada.teclas, Some(linha.partes()), "{}", linha.rotulo);
        }
        // Os que entraram por último também: o quadro de ideias e a busca das enviadas.
        assert!(acha(&todas, "Quadro de ideias").motivo.is_none());
        assert!(todas.iter().any(|e| e.titulo.starts_with("Buscar nas suas mensagens")));
        // Cada chave uma vez só, e toda linha numa seção que a paleta desenha.
        let mut chaves: Vec<&str> = todas.iter().map(|e| e.chave.as_str()).collect();
        chaves.sort_unstable();
        chaves.dedup();
        assert_eq!(chaves.len(), todas.len());
        let desenhadas: usize = grupos(&todas, "").iter().map(|g| g.itens.len()).sum();
        assert_eq!(desenhadas, todas.len());
    }

    #[test]
    fn os_comandos_de_menu_entram_com_o_nome_do_menu() {
        let todas = entradas(&com_chat(ChatAgora { com_sessao: true, ..Default::default() }));
        for titulo in [
            "Compactar a conversa",
            "Agentes desta conversa",
            "Perguntar também ao Codex",
            "Servidores MCP",
            "Contas do Claude",
            "Consultar memória",
            "Novo chat em outra pasta…",
        ] {
            acha(&todas, titulo);
        }
        assert_eq!(Direto::TODOS.len(), todas.iter().filter(|e| e.chave.starts_with("direto:")).count());
        // No chat do Codex, a segunda resposta é a do Claude; fixado, o item desafixa.
        let todas = entradas(&com_chat(ChatAgora { agente: crate::modelo::Agente::Codex, fixado: true, ..Default::default() }));
        acha(&todas, "Perguntar também ao Claude");
        acha(&todas, "Desafixar a conversa");
    }

    /// O Codex também compacta (`thread/compact/start`); só o Gemini não tem como.
    #[test]
    fn o_codex_compacta_e_o_gemini_nao() {
        use crate::modelo::Agente;
        let codex = entradas(&com_chat(ChatAgora { agente: Agente::Codex, com_sessao: true, ..Default::default() }));
        assert_eq!(acha(&codex, "Compactar a conversa").motivo, None);
        let gemini = entradas(&com_chat(ChatAgora { agente: Agente::Gemini, com_sessao: true, ..Default::default() }));
        assert!(acha(&gemini, "Compactar a conversa").motivo.is_some());
    }

    #[test]
    fn o_que_nao_vale_agora_fica_apagado_com_o_motivo() {
        // Sem sessão não há o que compactar; com ela, vale.
        let sem = entradas(&com_chat(ChatAgora::default()));
        assert_eq!(acha(&sem, "Compactar a conversa").motivo.as_deref(), Some("A conversa ainda não tem sessão"));
        let com = entradas(&com_chat(ChatAgora { com_sessao: true, ..Default::default() }));
        assert_eq!(acha(&com, "Compactar a conversa").motivo, None);
        // Sem conversa no painel ativo, os comandos da conversa pedem uma.
        let vazio = entradas(&Momento::default());
        assert_eq!(acha(&vazio, "Sessão nova").motivo.as_deref(), Some("Abra uma conversa no painel ativo"));
        assert_eq!(acha(&vazio, "Contas do Claude").motivo, None);
        // A caixa vazia não tem pergunta para o outro agente.
        let caixa_vazia = entradas(&com_chat(ChatAgora { caixa_vazia: true, outro_instalado: true, ..Default::default() }));
        assert_eq!(acha(&caixa_vazia, "Perguntar também ao Codex").motivo.as_deref(), Some("Escreva a pergunta na caixa antes"));
        // O atalho de um lugar que a paleta não alcança, e a própria paleta.
        assert!(acha(&vazio, "Andar pelo menu “/”").motivo.is_some());
        assert_eq!(acha(&vazio, ROTULO_DA_PALETA).motivo.as_deref(), Some("É esta lista"));
        // Em cada seção, o apagado vem depois do que vale.
        for grupo in Grupo::TODOS {
            let da_secao: Vec<bool> = vazio.iter().filter(|e| e.grupo == grupo).map(|e| e.motivo.is_some()).collect();
            assert!(da_secao.is_sorted(), "{grupo:?}: {da_secao:?}");
        }
    }

    #[test]
    fn cada_arranjo_salvo_vira_uma_linha_e_o_em_uso_fica_apagado() {
        let m = Momento { arranjos: vec!["Central".into(), "Vitral".into()], arranjo_em_uso: Some("Vitral".into()), ..Default::default() };
        let todas = entradas(&m);
        assert_eq!((acha(&todas, "Arranjo: Central").chave.as_str(), &acha(&todas, "Arranjo: Central").motivo), ("arranjo:Central", &None));
        assert_eq!(acha(&todas, "Arranjo: Vitral").motivo.as_deref(), Some("É o arranjo em uso"));
        // Salvar e andar pelos arranjos vêm da tabela de atalhos.
        acha(&todas, "Salvar o arranjo de painéis atual…");
        acha(&todas, "Arranjo de painéis seguinte (dá a volta)");
    }

    #[test]
    fn a_busca_acha_sem_acento_pelo_comeco_da_palavra_e_pelas_outras_palavras() {
        let todas = entradas(&com_chat(ChatAgora { com_sessao: true, ..Default::default() }));
        let titulos = |busca: &str| -> Vec<String> {
            grupos(&todas, busca).into_iter().flat_map(|g| g.itens).map(|i| i.titulo.to_string()).collect()
        };
        assert_eq!(titulos("compac"), ["Compactar a conversa"]);
        assert!(titulos("configuracoes").contains(&"Abrir as Configurações".to_string()));
        // "mapa" não está no nome, mas é como a pessoa procura os agentes.
        assert_eq!(titulos("mapa agentes"), ["Agentes desta conversa"]);
        assert!(titulos("nada que exista aqui").is_empty());
    }

    #[test]
    fn com_algo_escrito_a_paleta_acha_conversas_e_arquivos_depois_dos_comandos() {
        let todas = entradas(&Momento::default());
        let conversa = |id: &str, titulo: &str| ConversaNaPaleta { id: id.into(), titulo: titulo.into(), onde: "loja".into() };
        let conversas = [conversa("c1", "Cupons dos grupos VIP"), conversa("c2", "Integração do frete")];
        let arquivos: Vec<String> = ["src/checkout/cupom.rs", "docs/integração.md", "src/main.rs"].map(String::from).to_vec();
        let do_grupo = |grupos: &[crate::ui::componentes::GrupoDeEscolha], titulo: &str| -> Option<Vec<(String, String)>> {
            grupos
                .iter()
                .find(|g| g.titulo.as_ref().is_some_and(|t| t.as_ref() == titulo))
                .map(|g| g.itens.iter().map(|i| (i.chave.to_string(), i.titulo.to_string())).collect())
        };
        // Sem nada escrito, só os comandos, como antes.
        let vazia = grupos_com_achados(&todas, &conversas, &arquivos, "");
        assert_eq!(vazia.len(), grupos(&todas, "").len());
        assert_eq!((do_grupo(&vazia, GRUPO_CONVERSAS), do_grupo(&vazia, GRUPO_ARQUIVOS)), (None, None));
        // "integracao", sem acento, acha a conversa pelo título e o arquivo pelo nome.
        let achados = grupos_com_achados(&todas, &conversas, &arquivos, "integracao");
        assert_eq!(do_grupo(&achados, GRUPO_CONVERSAS), Some(vec![("conversa:c2".to_string(), "Integração do frete".to_string())]));
        assert_eq!(do_grupo(&achados, GRUPO_ARQUIVOS), Some(vec![("arquivo:docs/integração.md".to_string(), "integração.md".to_string())]));
        // Os comandos continuam e vêm antes dos achados.
        let achados = grupos_com_achados(&todas, &conversas, &arquivos, "conversa");
        let comandos = grupos(&todas, "conversa").len();
        assert!(comandos > 0 && achados.len() >= comandos);
        let titulos: Vec<String> = achados.iter().map(|g| g.titulo.clone().unwrap_or_default().to_string()).collect();
        assert!(!titulos[..comandos].iter().any(|t| t == GRUPO_CONVERSAS || t == GRUPO_ARQUIVOS), "{titulos:?}");
        // De cada tipo, só os primeiros: o resto aparece refinando a busca.
        let muitos: Vec<String> = (0..40).map(|i| format!("src/modulo{i}/cupom.rs")).collect();
        let achados = grupos_com_achados(&todas, &conversas, &muitos, "cupom");
        assert_eq!(do_grupo(&achados, GRUPO_ARQUIVOS).map(|g| g.len()), Some(ACHADOS_POR_GRUPO));
        assert_eq!(do_grupo(&achados, GRUPO_CONVERSAS), None);
    }
}
