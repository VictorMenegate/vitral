//! Diálogos: novo chat (escolher o projeto, ou Avulsos, e a pasta), mover para projeto, projetos
//! arquivados, localizar pasta, ativar git, renomear, contas, arquivos do projeto, conversa como
//! contexto e rotinas, todos na mesma moldura. Novo e Editar projeto ficam em `projeto.rs`.

use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc};

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    dialog::Dialog,
    input::{Input, InputState, Textarea, TextareaState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela, Vista,
    componentes::{
        CabecalhoDialogo, Confirmacao, EventoEscolha, GrupoDeEscolha, ItemDeEscolha, ListaDeEscolha, RodapeDialogo, Selo,
        Simbolo, Sinal, TamanhoBotao, Tom, TomConfirmacao, aviso, aviso_erro, botao_fantasma, botao_primario, confirmar, contagem, desenho_teclas, largura,
        sinal_do_chat,
    },
    configuracoes::Configuracoes,
    icone_projeto,
    painel_contas::PainelContas,
    seletor_arquivos,
    tempo_relativo,
};
use crate::{
    modelo::{Item, Papel},
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, r, raio, tam},
};

/// Moldura de todos os diálogos: opaca, raio de peça, margem 24, borda forte e sem o × do kit (o do
/// cabeçalho tem a dica). A largura é a da classe; o kit ainda corta pela janela.
pub(super) fn moldura(dialogo: Dialog, largura: f32, window: &Window) -> Dialog {
    dialogo
        .bg(Cores::dialogo())
        .w(tam(window, largura))
        .p(r(24.))
        .rounded(raio::PECA)
        .border_color(Cores::borda_forte())
        .close_button(false)
}

/// Versão estreita: o diálogo não coube na largura da classe (zoom alto ou janela pequena). Os
/// controles das linhas descem para baixo do texto.
pub(super) fn estreito(window: &Window, largura: f32) -> bool {
    tam(window, largura) > window.viewport_size().width - px(32.)
}

/// Cabeçalho com o subtítulo em peso normal (o título do diálogo põe tudo em semibold).
pub(super) fn cabecalho(titulo: impl Into<SharedString>, subtitulo: Option<String>) -> CabecalhoDialogo {
    let cabecalho = CabecalhoDialogo::new(titulo);
    match subtitulo {
        Some(texto) => cabecalho.subtitulo(div().font_weight(FontWeight::NORMAL).child(texto)),
        None => cabecalho,
    }
}

/// O cabeçalho no topo do diálogo, a 16 do corpo (8 aqui e os 8 do kit).
pub(super) fn topo(cabecalho: CabecalhoDialogo) -> impl IntoElement {
    div().pb(r(8.)).child(cabecalho)
}

/// Caminho com a pasta pessoal como "~".
pub(super) fn com_til(caminho: &Path) -> String {
    match dirs::home_dir().and_then(|casa| caminho.strip_prefix(&casa).ok().map(Path::to_path_buf)) {
        Some(resto) if resto.as_os_str().is_empty() => "~".into(),
        Some(resto) => format!("~/{}", resto.display()),
        None => caminho.display().to_string(),
    }
}

/// "↑ ↓ escolher": teclas fixas da lista e o que fazem, no rodapé.
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

fn contas_conectadas(n: usize) -> String {
    contagem(n, "conta conectada", "contas conectadas")
}

/// Chaves da paleta que não são projetos (nenhum id de projeto tem o "\0").
const NOVO_PROJETO: &str = "\0novo";
const CRIAR_PROJETO: &str = "\0criar:";
const AVULSOS: &str = "\0avulsos";

/// Para onde vai o Ctrl+N.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum AlvoCtrlN {
    /// Cria direto no projeto (`""`: avulso).
    Direto(String),
    /// A paleta, com este destino marcado (`Some("")`: Avulsos).
    Paleta(Option<String>),
}

/// O Ctrl+N cria direto na vista de um projeto e nos Avulsos. Na visão geral abre a paleta,
/// marcando Avulsos quando o chat aberto não tem projeto; senão, o último destino (ou o projeto do
/// chat aberto). `chat_ativo` é o projeto do chat aberto.
pub(super) fn alvo_do_ctrl_n(vista: &Vista, chat_ativo: Option<&str>, ultimo_destino: Option<&str>, projetos: &[String]) -> AlvoCtrlN {
    let existe = |p: &str| p.is_empty() || projetos.iter().any(|x| x == p);
    match vista {
        Vista::Projeto(id) if existe(id) => return AlvoCtrlN::Direto(id.clone()),
        Vista::Avulsos => return AlvoCtrlN::Direto(String::new()),
        _ => {}
    }
    let marcado = match chat_ativo {
        Some("") => Some(String::new()),
        _ => ultimo_destino
            .filter(|d| existe(d))
            .or(chat_ativo.filter(|p| existe(p)))
            .map(str::to_string),
    };
    AlvoCtrlN::Paleta(marcado)
}

/// Um projeto na paleta de Nova conversa.
struct ProjetoNaPaleta {
    id: String,
    nome: String,
    icone: &'static icone_projeto::Icone,
    cor: u32,
    descricao: String,
    selo: Option<&'static str>,
}

/// "3 conversas · última há 2 h" ou "Nenhuma conversa ainda".
fn descricao_do_projeto(conversas: usize, ultima: Option<u64>) -> String {
    match ultima.filter(|_| conversas > 0) {
        None => "Nenhuma conversa ainda".into(),
        Some(instante) => {
            let quando = tempo_relativo(instante);
            let quando = if quando == "agora" { quando } else { format!("há {quando}") };
            format!("{} · última {quando}", contagem(conversas, "conversa", "conversas"))
        }
    }
}

/// Os projetos que têm a busca no nome (ou na descrição), Avulsos e, no fim, "Novo projeto…"
/// (ou, sem nenhum, "Criar o projeto “x”"), que o teclado também alcança.
fn grupos_da_paleta(projetos: &[ProjetoNaPaleta], busca: &str) -> Vec<GrupoDeEscolha> {
    let busca = busca.trim();
    let termo = busca.to_lowercase();
    let avulsos = "avulsos chat sem projeto".contains(&termo).then(|| ItemDeEscolha {
        simbolo: Some(Simbolo::icone(Lucide::MessageCircle)),
        descricao: Some("Um chat sem projeto, numa pasta só dele".into()),
        ..ItemDeEscolha::new(AVULSOS, "Avulsos")
    });
    let itens: Vec<ItemDeEscolha> = projetos
        .iter()
        .filter(|p| p.nome.to_lowercase().contains(&termo) || p.descricao.to_lowercase().contains(&termo))
        .map(|p| ItemDeEscolha {
            simbolo: Some(Simbolo::Projeto(p.icone, p.cor)),
            descricao: Some(p.descricao.clone().into()),
            selos: p.selo.map(Selo::neutro).into_iter().collect(),
            ..ItemDeEscolha::new(p.id.clone(), p.nome.clone())
        })
        .collect();
    let novo = if itens.is_empty() && avulsos.is_none() && !busca.is_empty() {
        ItemDeEscolha::new(format!("{CRIAR_PROJETO}{busca}"), format!("Criar o projeto “{busca}”"))
    } else {
        ItemDeEscolha::new(NOVO_PROJETO, "Novo projeto…")
    };
    let novo = ItemDeEscolha { simbolo: Some(Simbolo::icone(Lucide::FolderPlus)), ..novo };
    let mut grupos = Vec::new();
    if !itens.is_empty() {
        grupos.push(GrupoDeEscolha { titulo: Some("Projetos".into()), itens });
    }
    grupos.push(GrupoDeEscolha { titulo: None, itens: avulsos.into_iter().chain([novo]).collect() });
    grupos
}

impl Janela {
    /// Ctrl+N: direto na vista de um projeto (com duas pastas, pergunta qual) e nos Avulsos; na
    /// visão geral, a paleta dos projetos, de Avulsos e do projeto novo.
    pub(super) fn dialogo_nova_conversa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let projetos: Vec<String> = motor.read(cx).projetos_visiveis().iter().map(|p| p.id.clone()).collect();
        let m = motor.read(cx);
        let ativo = self.selecionado.as_ref().and_then(|id| m.chat(id)).map(|c| c.projeto.clone());
        let marcado = match alvo_do_ctrl_n(&self.vista, ativo.as_deref(), self.ultimo_destino.as_deref(), &projetos) {
            AlvoCtrlN::Direto(projeto) => {
                self.novo_chat_em(&projeto, window, cx);
                return;
            }
            AlvoCtrlN::Paleta(marcado) => marcado,
        };
        let mut dados = Vec::new();
        for id in &projetos {
            let conversas: Vec<u64> = m
                .chats
                .iter()
                .filter(|c| c.projeto == *id && c.papel.na_lista() && !m.arquivada(c))
                .map(|c| c.atualizado_em)
                .collect();
            let (icone, cor) = icone_projeto::visual_do(m, id);
            let descricao = m
                .projeto(id)
                .map(|p| p.descricao.lines().next().unwrap_or_default().trim().to_string())
                .filter(|d| !d.is_empty())
                .unwrap_or_else(|| descricao_do_projeto(conversas.len(), conversas.iter().max().copied()));
            dados.push(ProjetoNaPaleta {
                id: id.clone(),
                nome: m.nome_do_projeto(id),
                icone,
                cor,
                descricao,
                selo: (ativo.as_deref() == Some(id.as_str())).then_some("atual"),
            });
        }
        let realce = marcado.map(|d| if d.is_empty() { SharedString::from(AVULSOS) } else { SharedString::from(d) });
        let dados = Rc::new(dados);
        let janela = cx.entity().downgrade();
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar projeto…", window, cx);
            lista.definir(grupos_da_paleta(&dados, ""), realce, cx);
            let filtro = dados.clone();
            lista.filtro_proprio(move |busca| grupos_da_paleta(&filtro, busca));
            lista.rodape(move |window, _| {
                let largo = tam(window, 420.) < window.viewport_size().width - px(32.);
                div()
                    .flex()
                    .items_center()
                    .gap(r(12.))
                    .w_full()
                    .child(div().flex_1())
                    .when(largo, |s| {
                        s.child(dica_teclas(&["↑", "↓"], "escolher")).child(dica_teclas(&["Enter"], "abrir"))
                    })
                    .into_any_element()
            });
            lista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(chave) = evento else {
                return;
            };
            window.close_dialog(cx);
            if chave.as_ref() == NOVO_PROJETO {
                this.dialogo_criar_projeto(window, cx);
            } else if let Some(nome) = chave.strip_prefix(CRIAR_PROJETO) {
                this.dialogo_projeto_novo(nome.to_string(), None, window, cx);
            } else {
                let projeto = if chave.as_ref() == AVULSOS { String::new() } else { chave.to_string() };
                this.lembrar_destino(&projeto, cx);
                this.novo_chat_em(&projeto, window, cx);
            }
        }));
        self.foco_antes_do_dialogo(window, cx);
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            let fechou = janela.clone();
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Novo chat", Some("Em qual projeto?".into()))))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
                // Fechado sem escolher (Esc, clique fora), "Nova conversa ao lado" não vale mais.
                .on_close(move |_, _, cx| {
                    let _ = fechou.update(cx, |this, _| this.nova_ao_lado = false);
                })
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Um chat novo no projeto (`""`: avulso). Com mais de uma pasta à mão, pergunta em qual.
    pub(super) fn novo_chat_em(&mut self, projeto: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let pastas: Vec<(PathBuf, bool)> =
            motor.read(cx).pastas_do(projeto).iter().map(|p| (p.caminho.clone(), p.propria)).filter(|(p, _)| p.is_dir()).collect();
        if pastas.len() < 2 {
            self.nova_conversa(projeto, window, cx);
            return;
        }
        let itens: Vec<ItemDeEscolha> = pastas
            .iter()
            .enumerate()
            .map(|(i, (caminho, propria))| {
                let nome = if *propria {
                    "Pasta própria".to_string()
                } else {
                    caminho.file_name().map_or_else(|| caminho.display().to_string(), |n| n.to_string_lossy().to_string())
                };
                let descricao = if i == 0 { format!("principal · {}", com_til(caminho)) } else { com_til(caminho) };
                ItemDeEscolha {
                    simbolo: Some(Simbolo::icone(Lucide::Folder)),
                    descricao: Some(descricao.into()),
                    ..ItemDeEscolha::new(caminho.display().to_string(), nome)
                }
            })
            .collect();
        let primeira = itens.first().map(|i| i.chave.clone());
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar pasta…", window, cx);
            lista.definir(vec![GrupoDeEscolha { titulo: None, itens }], primeira, cx);
            lista
        });
        let alvo = projeto.to_string();
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, move |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(pasta) = evento else {
                return;
            };
            window.close_dialog(cx);
            this.nova_conversa_na_pasta(&alvo, Some(PathBuf::from(pasta.as_ref())), window, cx);
        }));
        self.foco_antes_do_dialogo(window, cx);
        let vista = lista.clone();
        let nome = motor.read(cx).nome_do_projeto(projeto);
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Em qual pasta?", Some(format!("O chat novo de {nome} trabalha numa pasta só.")))))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Grava o destino do último chat criado pela paleta (a próxima já vem marcada nele).
    fn lembrar_destino(&mut self, projeto: &str, cx: &mut Context<Self>) {
        self.ultimo_destino = Some(projeto.to_string());
        let valor = if projeto.is_empty() { "avulsos".to_string() } else { projeto.to_string() };
        super::gravar_interface("ultimo_destino", serde_json::Value::String(valor), cx);
    }

    /// "Mover para projeto…" do menu do chat: os projetos e Avulsos; mover pergunta quando o motor
    /// avisa de algo, e a recusa vira aviso.
    pub(super) fn dialogo_mover_chat(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let m = motor.read(cx);
        let Some(atual) = m.chat(&chat).map(|c| c.projeto.clone()) else {
            return;
        };
        let mut itens: Vec<ItemDeEscolha> = m
            .projetos_visiveis()
            .into_iter()
            .filter(|p| p.id != atual)
            .map(|p| {
                let (icone, cor) = icone_projeto::visual(p);
                ItemDeEscolha { simbolo: Some(Simbolo::Projeto(icone, cor)), ..ItemDeEscolha::new(p.id.clone(), p.nome.clone()) }
            })
            .collect();
        if !atual.is_empty() {
            itens.push(ItemDeEscolha {
                simbolo: Some(Simbolo::icone(Lucide::MessageCircle)),
                descricao: Some("Sem projeto".into()),
                ..ItemDeEscolha::new(AVULSOS, "Avulsos")
            });
        }
        let titulo = m.chat(&chat).map(|c| c.titulo.clone()).unwrap_or_default();
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar projeto…", window, cx);
            lista.definir(vec![GrupoDeEscolha { titulo: None, itens }], None, cx);
            lista
        });
        let id = chat.clone();
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, move |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(destino) = evento else {
                return;
            };
            window.close_dialog(cx);
            let destino = if destino.as_ref() == AVULSOS { String::new() } else { destino.to_string() };
            this.soltar_chat(&id, &destino, window, cx);
        }));
        self.foco_antes_do_dialogo(window, cx);
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Mover para projeto", Some(titulo.clone()))))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// "Projetos arquivados…": cada um com o quadrado; escolher desarquiva e abre o projeto.
    pub(super) fn dialogo_projetos_arquivados(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let itens: Vec<ItemDeEscolha> = motor
            .read(cx)
            .projetos_arquivados()
            .into_iter()
            .map(|p| {
                let (icone, cor) = icone_projeto::visual(p);
                ItemDeEscolha {
                    simbolo: Some(Simbolo::Projeto(icone, cor)),
                    descricao: Some("Escolher desarquiva".into()),
                    ..ItemDeEscolha::new(p.id.clone(), p.nome.clone())
                }
            })
            .collect();
        if itens.is_empty() {
            window.push_notification(aviso(Tom::Info, "Nenhum projeto arquivado"), cx);
            return;
        }
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar projeto…", window, cx);
            lista.definir(vec![GrupoDeEscolha { titulo: None, itens }], None, cx);
            lista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(projeto) = evento else {
                return;
            };
            window.close_dialog(cx);
            this.desarquivar_projeto(projeto.to_string(), cx);
            this.abrir_vista(super::Vista::Projeto(projeto.to_string()), window, cx);
        }));
        self.foco_antes_do_dialogo(window, cx);
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Projetos arquivados", None)))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    pub(super) fn abrir_configuracoes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        // O caminho só na dica: no cabeçalho, ele não diz nada a quem usa.
        let dica: SharedString = format!("Gravadas em {}", com_til(&motor.read(cx).dados.arquivo_config())).into();
        let vista = cx.new(|cx| Configuracoes::new(motor, cx));
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, _| {
            let dica = dica.clone();
            let subtitulo = div()
                .id("configuracoes-pasta")
                .font_weight(FontWeight::NORMAL)
                .child("Valem para todos os projetos e chats")
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx));
            moldura(dialogo, largura::G, window)
                .title(topo(CabecalhoDialogo::new("Configurações").subtitulo(subtitulo)))
                .child(vista.clone())
        });
    }
}

impl Janela {
    /// "Localizar pasta…": uma pasta do projeto não está mais lá (renomeada, disco em outro lugar);
    /// a escolhida passa a ser ela, e as tarefas que trabalhavam nela vão junto.
    pub(super) fn localizar_pasta(&mut self, projeto: String, indice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Usar esta pasta".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else {
                return;
            };
            let Some(pasta) = caminhos.into_iter().next() else {
                return;
            };
            let Ok(Some(tarefa)) = this.update(cx, |this, cx| {
                this.motor.clone().map(|motor| motor.update(cx, |m, cx| m.localizar_pasta(&projeto, indice, &pasta, cx)))
            }) else {
                return;
            };
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                let nota = match resultado {
                    Ok(None) => aviso(Tom::Pronto, format!("Pasta localizada: {}", com_til(&pasta))),
                    Ok(Some(atencao)) => aviso(Tom::Atencao, format!("Pasta localizada. {atencao}")),
                    Err(erro) => aviso(Tom::Erro, format!("A pasta não mudou: {erro}")),
                };
                window.push_notification(nota, cx);
                this.git_pastas.clear();
                this.readmes.borrow_mut().clear();
                cx.notify();
            });
        })
        .detach();
    }

    /// "Ativar git em “pasta”?": o repositório nasce com um commit dos arquivos atuais.
    pub(super) fn confirmar_git_na_pasta(&mut self, pasta: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let nome = pasta.file_name().map_or_else(|| pasta.display().to_string(), |n| n.to_string_lossy().to_string());
        let sem_gitignore = !pasta.join(".gitignore").exists();
        let detalhes = if sem_gitignore {
            vec![(Lucide::FileText.into(), "Cria um .gitignore sem .env, ambientes e builds".into(), false)]
        } else {
            Vec::new()
        };
        let janela = cx.entity().downgrade();
        let alvo = pasta.clone();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Neutro,
                icone: Lucide::GitBranch.into(),
                titulo: format!("Ativar git em “{nome}”?").into(),
                texto: "Cria um repositório com um commit dos arquivos atuais. Nada é apagado nem enviado.".into(),
                detalhes,
                ok: "Ativar git".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: false,
            },
            move |window, cx| {
                let pasta = alvo.clone();
                let _ = janela.update(cx, |this, cx| this.ativar_git(pasta, window, cx));
            },
            None,
            window,
            cx,
        );
    }

    fn ativar_git(&mut self, pasta: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let nome = pasta.file_name().map_or_else(|| pasta.display().to_string(), |n| n.to_string_lossy().to_string());
        let tarefa = motor.update(cx, |m, cx| m.ativar_git(pasta.clone(), cx));
        cx.spawn_in(window, async move |this, cx| {
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match resultado {
                    Ok(novo) => {
                        // O repositório nasce com o commit: o aviso de sem git sai já.
                        this.git_pastas.insert(pasta.clone(), true);
                        let resumo = novo.resumo(&nome);
                        let resumo = resumo.strip_suffix('.').unwrap_or(&resumo).to_string();
                        window.push_notification(aviso(Tom::Pronto, resumo), cx);
                    }
                    Err(erro) => {
                        this.git_pastas.remove(&pasta);
                        window.push_notification(aviso_erro("Não deu para ativar o git", &erro), cx);
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn dialogo_renomear(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let atual = motor.read(cx).chat(&chat).map(|c| c.titulo.clone()).unwrap_or_default();
        let campo = cx.new(|cx| InputState::new(window, cx).default_value(atual.clone()));
        campo.update(cx, |c, cx| c.select_all(window, cx));
        // Título igual ao de antes só fecha; vazio não salva.
        let salvar = {
            let campo = campo.clone();
            Rc::new(move |cx: &mut App| -> bool {
                let titulo = campo.read(cx).value().trim().to_string();
                if titulo.is_empty() {
                    return false;
                }
                if titulo != atual {
                    motor.update(cx, |m, cx| m.renomear(&chat, titulo, cx));
                }
                true
            })
        };
        let foco = campo.read(cx).focus_handle(cx);
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let vazio = campo.read(cx).value().trim().is_empty();
            let (no_botao, no_enter) = (salvar.clone(), salvar.clone());
            moldura(dialogo, largura::P, window)
                .title(topo(cabecalho("Renomear conversa", None)))
                .child(Input::new(&campo))
                .footer(
                    RodapeDialogo::new()
                        .cancelar(
                            botao_fantasma("renomear-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(
                            botao_primario("renomear-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx)
                                .when(vazio, |b| b.disabled(true).opacity(0.45).tooltip("Escreva um título"))
                                .on_click(move |_, window, cx| {
                                    if no_botao(cx) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
                .on_ok(move |_, _, cx| no_enter(cx))
        });
        window.focus(&foco, cx);
    }
}

/// O que a memória ganhou no disco desde que o diálogo abriu: as linhas que não estavam lá nem
/// estão no texto editado (o Mestre lembrou algo com o diálogo aberto).
pub(super) fn linhas_novas_da_memoria(inicial: &str, no_disco: &str, editado: &str) -> Vec<String> {
    let antes: std::collections::HashSet<&str> = inicial.lines().map(str::trim).collect();
    let agora: std::collections::HashSet<&str> = editado.lines().map(str::trim).collect();
    no_disco
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !antes.contains(l) && !agora.contains(l))
        .map(str::to_string)
        .collect()
}

impl Janela {
    /// "Memória do Mestre": o texto que entra no prompt dele a cada turno, editável (até 6000
    /// caracteres), com Esquecer tudo, Cancelar e Salvar. Se o Mestre lembrou algo com o diálogo
    /// aberto, o Salvar traz as linhas novas para o fim e pede para conferir.
    pub(super) fn dialogo_memoria(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        const LIMITE: usize = 6000;
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let inicial = motor.read(cx).memoria_do_mestre();
        let campo = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(8, 18)
                .placeholder("Nada lembrado ainda. Ex.: pagamentos vão para a Ninee; os textos da loja em português.")
                .default_value(inicial.clone())
        });
        // O que estava no disco da última vez que o diálogo olhou.
        let visto = Rc::new(RefCell::new(inicial));
        let salvar = {
            let (campo, motor, visto) = (campo.clone(), motor.clone(), visto.clone());
            Rc::new(move |window: &mut Window, cx: &mut App| -> bool {
                let editado = campo.read(cx).value().to_string();
                let no_disco = motor.read(cx).memoria_do_mestre();
                if no_disco != *visto.borrow() {
                    let novas = linhas_novas_da_memoria(&visto.borrow(), &no_disco, &editado);
                    *visto.borrow_mut() = no_disco;
                    if !novas.is_empty() {
                        let junto = format!("{}\n{}", editado.trim_end(), novas.join("\n"));
                        campo.update(cx, |c, cx| c.set_value(junto, window, cx));
                        let texto = format!(
                            "O Mestre lembrou {} enquanto você editava; {} no fim. Confira e salve de novo.",
                            contagem(novas.len(), "linha nova", "linhas novas"),
                            if novas.len() == 1 { "ela entrou" } else { "elas entraram" }
                        );
                        window.push_notification(aviso(Tom::Atencao, texto), cx);
                        return false;
                    }
                }
                if editado.chars().count() > LIMITE {
                    window.push_notification(aviso(Tom::Atencao, format!("A memória passa de {LIMITE} caracteres: junte ou tire itens.")), cx);
                    return false;
                }
                match motor.read(cx).gravar_memoria_do_mestre(&editado) {
                    Ok(()) => {
                        window.push_notification(aviso(Tom::Pronto, "Memória do Mestre salva"), cx);
                        true
                    }
                    Err(erro) => {
                        window.push_notification(aviso_erro("Não deu para salvar a memória", &erro), cx);
                        false
                    }
                }
            })
        };
        let foco = campo.read(cx).focus_handle(cx);
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let texto = campo.read(cx).value().to_string();
            let n = texto.chars().count();
            let no_botao = salvar.clone();
            let (m, c) = (motor.clone(), campo.clone());
            let esquecer = botao_fantasma("memoria-esquecer", Some(Lucide::Trash.into()), "Esquecer tudo", TamanhoBotao::Controle, window, cx)
                .when(texto.trim().is_empty(), |b| b.disabled(true).opacity(0.45))
                .on_click(move |_, window, cx| {
                    let (m, c) = (m.clone(), c.clone());
                    confirmar(
                        Confirmacao {
                            tom: TomConfirmacao::Perigo,
                            icone: Lucide::Brain.into(),
                            titulo: "Esquecer tudo o que o Mestre lembra?".into(),
                            texto: "A memória fica vazia: o Mestre deixa de saber as preferências que você disse.".into(),
                            detalhes: Vec::new(),
                            ok: "Esquecer tudo".into(),
                            cancelar: "Cancelar".into(),
                            ok_perigo: true,
                        },
                        move |window, cx| match m.read(cx).gravar_memoria_do_mestre("") {
                            Ok(()) => {
                                c.update(cx, |c, cx| c.set_value("", window, cx));
                                window.push_notification(aviso(Tom::Pronto, "Memória do Mestre esvaziada"), cx);
                            }
                            Err(erro) => window.push_notification(aviso_erro("Não deu para esvaziar a memória", &erro), cx),
                        },
                        None,
                        window,
                        cx,
                    );
                });
            let principal = botao_primario("memoria-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx)
                .when(n > LIMITE, |b| b.disabled(true).opacity(0.45).tooltip(format!("Passa de {LIMITE} caracteres")))
                .on_click(move |_, window, cx| {
                    if no_botao(window, cx) {
                        window.close_dialog(cx);
                    }
                });
            let cor = if n > LIMITE { Cores::vermelho() } else { Cores::tenue() };
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Memória do Mestre", Some("Entra no começo de cada conversa dele; você pode editar".into()))))
                .child(Textarea::new(&campo))
                .child(
                    div()
                        .pt(r(8.))
                        .flex()
                        .gap(r(8.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .metadado()
                                .text_color(Cores::tenue())
                                .child("Uma preferência por linha. O Mestre só grava sozinho o que você mesmo disse; o resto pede confirmação."),
                        )
                        .child(div().flex_none().metadado().text_color(cor).child(format!("{} de {LIMITE}", super::lateral::milhar(n)))),
                )
                .footer(
                    RodapeDialogo::new()
                        .esquerda(esquecer)
                        .cancelar(
                            botao_fantasma("memoria-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(principal),
                )
                // O Enter quebra a linha no campo; quem salva é o botão.
                .on_ok(|_, _, _| false)
        });
        window.focus(&foco, cx);
    }

    pub(super) fn dialogo_contas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_contas(false, window, cx);
    }

    /// O diálogo das contas já começando o login de uma conta nova (o código da página de login
    /// é colado nele), com a lista recolhida numa linha.
    pub(super) fn dialogo_entrar_conta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_contas(true, window, cx);
    }

    fn abrir_contas(&mut self, entrar: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let painel = cx.new(|cx| PainelContas::new(motor.clone(), window, cx));
        if entrar {
            painel.update(cx, |p, cx| p.entrar(window, cx));
        }
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let conectadas = contas_conectadas(motor.read(cx).contas.len());
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Contas do Claude", Some(conectadas))))
                .child(painel.clone())
                // O Enter do código entra pelo próprio campo; o diálogo não fecha com ele.
                .on_ok(|_, _, _| false)
        });
    }

    /// Arquivos da pasta da conversa, lidos em segundo plano; o escolhido entra na mensagem como
    /// "@caminho".
    pub(super) fn dialogo_arquivos_projeto(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(pasta) = self.motor.as_ref().and_then(|m| m.read(cx).chat(&chat).map(|c| c.cwd.clone())) else {
            return;
        };
        let arquivos: Rc<RefCell<Option<Rc<Vec<String>>>>> = Rc::default();
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar arquivo…", window, cx);
            lista.definir(seletor_arquivos::grupos_de_arquivos(None, ""), None, cx);
            let lidos = arquivos.clone();
            lista.filtro_proprio(move |busca| {
                let lidos = lidos.borrow().clone();
                seletor_arquivos::grupos_de_arquivos(lidos.as_deref().map(Vec::as_slice), busca)
            });
            lista
        });
        let leitura = cx.background_spawn(async move { seletor_arquivos::listar(&pasta) });
        let (alvo, guardados) = (lista.downgrade(), arquivos.clone());
        cx.spawn_in(window, async move |_, cx| {
            let lidos = Rc::new(leitura.await);
            *guardados.borrow_mut() = Some(lidos.clone());
            let _ = alvo.update(cx, |l, cx| l.definir(seletor_arquivos::grupos_de_arquivos(Some(lidos.as_slice()), ""), None, cx));
        })
        .detach();
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, |this, _, evento: &EventoEscolha, window, cx| {
            if let EventoEscolha::Escolheu(caminho) = evento {
                window.close_dialog(cx);
                this.mencionar(caminho, window, cx);
            }
        }));
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Mencionar arquivo", None)))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Põe "@caminho " na caixa de mensagem, trocando o "@" solto que abriu a lista.
    pub(super) fn mencionar(&mut self, caminho: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.composer_ativo() else {
            return;
        };
        let mut texto = composer.read(cx).value().to_string();
        if texto.ends_with('@') {
            texto.pop();
        }
        if !texto.is_empty() && !texto.ends_with(char::is_whitespace) {
            texto.push(' ');
        }
        texto.push_str(&format!("@{caminho} "));
        composer.update(cx, |c, cx| super::composer::por_texto(c, texto, window, cx));
        // Vindo do arquivo na gaveta, a caixa está embaixo dela: aberta, o Esc pararia o turno.
        if self.gaveta.take().is_some() {
            cx.notify();
        }
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
    }
}

/// As outras conversas com mensagens, por projeto (as mais recentes primeiro): coordenador ou
/// trabalhador, quando falou e quantas mensagens.
fn grupos_de_conversas(motor: &Motor, chat: &str) -> Vec<GrupoDeEscolha> {
    let mut conversas: Vec<_> = motor.chats.iter().filter(|c| c.id != chat && !c.itens.is_empty()).collect();
    conversas.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    let mut grupos: Vec<GrupoDeEscolha> = Vec::new();
    let mut projetos: Vec<String> = motor.projetos_visiveis().iter().map(|p| p.id.clone()).collect();
    projetos.push(String::new());
    for projeto in projetos {
        let nome = motor.nome_do_projeto(&projeto);
        let itens: Vec<ItemDeEscolha> = conversas
            .iter()
            .filter(|c| c.projeto == projeto && c.papel != Papel::Mestre)
            .map(|c| {
                let mensagens = c
                    .itens
                    .iter()
                    .filter(|i| matches!(i, Item::Usuario { .. } | Item::Assistente { .. } | Item::Entrega { .. }))
                    .count();
                let quando = tempo_relativo(c.atualizado_em);
                let quando = if quando == "agora" { quando } else { format!("há {quando}") };
                let icone = if c.papel == Papel::Trabalhador { Lucide::Wrench } else { Lucide::MessagesSquare };
                let rodando = sinal_do_chat(motor, c).filter(|s| *s == Sinal::Rodando);
                ItemDeEscolha {
                    simbolo: Some(Simbolo::icone(icone)),
                    descricao: Some(format!("{quando} · {}", contagem(mensagens, "mensagem", "mensagens")).into()),
                    selos: rodando.map(|s| Selo::ponto(s.cor()).vazado()).into_iter().collect(),
                    termos: Some(nome.clone().into()),
                    ..ItemDeEscolha::new(c.id.clone(), c.titulo.clone())
                }
            })
            .collect();
        if !itens.is_empty() {
            grupos.push(GrupoDeEscolha { titulo: Some(nome.into()), itens });
        }
    }
    if grupos.is_empty() {
        let vazio = "Nenhuma outra conversa com mensagens";
        grupos.push(GrupoDeEscolha {
            titulo: None,
            itens: vec![ItemDeEscolha { desabilitado: Some(vazio.into()), ..ItemDeEscolha::new("", vazio) }],
        });
    }
    grupos
}

impl Janela {
    /// Escolher outra conversa para ir anexada (em markdown) à próxima mensagem.
    pub(super) fn dialogo_conversa_contexto(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let grupos = grupos_de_conversas(motor.read(cx), &chat);
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar conversa…", window, cx);
            lista.definir(grupos, None, cx);
            lista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, move |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(id) = evento else {
                return;
            };
            window.close_dialog(cx);
            match motor.read(cx).exportar_conversa(id) {
                Ok(arquivo) => {
                    this.anexos.entry(chat.clone()).or_default().push(arquivo);
                    cx.notify();
                }
                Err(erro) => window.push_notification(aviso_erro("Não deu para anexar a conversa", &erro), cx),
            }
        }));
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Conversa como contexto", Some("A escolhida vai anexada à próxima mensagem.".into()))))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }
}

impl Janela {
    /// Lista e formulário no mesmo diálogo: o cabeçalho e o rodapé seguem a página.
    pub(super) fn dialogo_rotinas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let (janela, projeto) = (cx.entity().downgrade(), self.vista.projeto().map(str::to_string));
        let vista = cx.new(|cx| super::rotinas::Rotinas::new(motor, janela, projeto, window, cx));
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let enter = vista.clone();
            moldura(dialogo, largura::G, window)
                .title(topo(super::rotinas::Rotinas::cabecalho(&vista, cx)))
                .child(vista.clone())
                .footer(super::rotinas::Rotinas::rodape(&vista, window, cx))
                // Enter no pedido salva (Shift+Enter quebra a linha); o diálogo continua aberto.
                .on_ok(move |_, window, cx| {
                    enter.update(cx, |v, cx| v.enter(window, cx));
                    false
                })
        });
    }
}

#[cfg(test)]
mod testes {
    // Só o que é preciso: o `use gpui_kit::*` lá de cima traz um `test` que troca o da std.
    use super::{
        AVULSOS, AlvoCtrlN, CRIAR_PROJETO, NOVO_PROJETO, ProjetoNaPaleta, alvo_do_ctrl_n, com_til, contas_conectadas,
        descricao_do_projeto, grupos_da_paleta,
    };
    use crate::ui::Vista;
    use crate::{modelo::agora, ui::icone_projeto::ICONES};

    #[test]
    fn contagem_de_contas() {
        assert_eq!(contas_conectadas(1), "1 conta conectada");
        assert_eq!(contas_conectadas(3), "3 contas conectadas");
    }

    #[test]
    fn caminho_com_a_pasta_pessoal_em_til() {
        let casa = dirs::home_dir().unwrap();
        assert_eq!(com_til(&casa.join("Projetos").join("loja")), "~/Projetos/loja");
        assert_eq!(com_til(&casa), "~");
        assert_eq!(com_til(std::path::Path::new("/etc/vitral")), "/etc/vitral");
    }

    fn projeto(nome: &str) -> ProjetoNaPaleta {
        ProjetoNaPaleta {
            id: nome.into(),
            nome: nome.into(),
            icone: &ICONES[0],
            cor: 0x8fb6ff,
            descricao: descricao_do_projeto(0, None),
            selo: None,
        }
    }

    #[test]
    fn paleta_filtra_pelo_nome_e_oferece_criar() {
        let projetos = [projeto("loja-api"), projeto("site-ninee")];
        let chaves = |busca: &str| -> Vec<String> {
            grupos_da_paleta(&projetos, busca)
                .iter()
                .flat_map(|g| g.itens.iter().map(|i| i.chave.to_string()))
                .collect()
        };
        assert_eq!(chaves(""), ["loja-api", "site-ninee", AVULSOS, NOVO_PROJETO]);
        assert_eq!(chaves("NINEE"), ["site-ninee", NOVO_PROJETO]);
        assert_eq!(chaves("avul"), [AVULSOS, NOVO_PROJETO]);
        assert_eq!(chaves(" ops "), [format!("{CRIAR_PROJETO}ops")]);
        let titulo = &grupos_da_paleta(&projetos, "ops")[0].itens[0].titulo;
        assert_eq!(titulo.as_ref(), "Criar o projeto “ops”");
    }

    #[test]
    fn ctrl_n_direto_no_projeto_e_nos_avulsos_e_marcado_na_visao_geral() {
        let projetos = vec!["loja".to_string(), "site".to_string()];
        let projeto = Vista::Projeto("loja".into());
        assert_eq!(alvo_do_ctrl_n(&projeto, Some("site"), None, &projetos), AlvoCtrlN::Direto("loja".into()));
        assert_eq!(alvo_do_ctrl_n(&Vista::Avulsos, Some("loja"), None, &projetos), AlvoCtrlN::Direto(String::new()));
        // Um projeto que sumiu da vista cai na paleta.
        let sumiu = Vista::Projeto("velho".into());
        assert!(matches!(alvo_do_ctrl_n(&sumiu, None, None, &projetos), AlvoCtrlN::Paleta(_)));
        // Na visão geral: o chat aberto sem projeto (o Mestre, um avulso) marca Avulsos.
        assert_eq!(alvo_do_ctrl_n(&Vista::Geral, Some(""), Some("site"), &projetos), AlvoCtrlN::Paleta(Some(String::new())));
        // Senão, o último destino; sem ele (ou se sumiu), o projeto do chat aberto.
        assert_eq!(alvo_do_ctrl_n(&Vista::Geral, Some("loja"), Some("site"), &projetos), AlvoCtrlN::Paleta(Some("site".into())));
        assert_eq!(alvo_do_ctrl_n(&Vista::Geral, Some("loja"), Some("velho"), &projetos), AlvoCtrlN::Paleta(Some("loja".into())));
        assert_eq!(alvo_do_ctrl_n(&Vista::Geral, None, Some(""), &projetos), AlvoCtrlN::Paleta(Some(String::new())));
        assert_eq!(alvo_do_ctrl_n(&Vista::Geral, None, None, &projetos), AlvoCtrlN::Paleta(None));
    }

    #[test]
    fn descricao_do_projeto_na_paleta() {
        assert_eq!(descricao_do_projeto(0, None), "Nenhuma conversa ainda");
        assert_eq!(descricao_do_projeto(1, Some(agora())), "1 conversa · última agora");
        assert_eq!(descricao_do_projeto(3, Some(agora() - 7200)), "3 conversas · última há 2 h");
    }
}
