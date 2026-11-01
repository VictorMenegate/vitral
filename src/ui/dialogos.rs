//! Diálogos: nova conversa (escolher o projeto), projeto novo, renomear, configurações da pasta,
//! contas, arquivos do projeto, conversa como contexto e rotinas, todos na mesma moldura.

use std::{cell::{Cell, RefCell}, path::Path, rc::Rc};

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    dialog::Dialog,
    input::{Input, InputState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CabecalhoDialogo, Campo, Confirmacao, EventoEscolha, GrupoDeEscolha, GrupoDeOpcoes, ItemDeEscolha,
        LinhaDeOpcao, ListaDeEscolha, QuadradoDeProjeto, RodapeDialogo, Selo, Simbolo, Sinal, TamanhoBotao,
        Tom, TomConfirmacao, aviso, aviso_erro, botao_fantasma, botao_primario, confirmar, contagem,
        desenho_teclas, icone, largura, sinal_do_chat,
    },
    config_projeto::ConfigProjetoView,
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

/// Subtítulo com um caminho: cortado no meio, inteiro na dica.
pub(super) fn subtitulo_caminho(id: &'static str, texto: String) -> impl IntoElement {
    let dica: SharedString = texto.clone().into();
    div()
        .id(id)
        .font_weight(FontWeight::NORMAL)
        .overflow_hidden()
        .whitespace_nowrap()
        .text_ellipsis_middle()
        .child(texto)
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
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

/// Chaves da paleta que não são projetos (nenhum nome de pasta tem o "\0").
const NOVO_PROJETO: &str = "\0novo";
const CRIAR_PROJETO: &str = "\0criar:";

/// Um projeto na paleta de Nova conversa.
struct ProjetoNaPaleta {
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

/// Os projetos que têm a busca no nome e, no fim, "Novo projeto…" (ou, sem nenhum, "Criar o
/// projeto “x”"), que o teclado também alcança.
fn grupos_da_paleta(projetos: &[ProjetoNaPaleta], busca: &str) -> Vec<GrupoDeEscolha> {
    let busca = busca.trim();
    let termo = busca.to_lowercase();
    let itens: Vec<ItemDeEscolha> = projetos
        .iter()
        .filter(|p| p.nome.to_lowercase().contains(&termo))
        .map(|p| ItemDeEscolha {
            simbolo: Some(Simbolo::Projeto(p.icone, p.cor)),
            descricao: Some(p.descricao.clone().into()),
            selos: p.selo.map(Selo::neutro).into_iter().collect(),
            ..ItemDeEscolha::new(p.nome.clone(), p.nome.clone())
        })
        .collect();
    let novo = if itens.is_empty() && !busca.is_empty() {
        ItemDeEscolha::new(format!("{CRIAR_PROJETO}{busca}"), format!("Criar o projeto “{busca}”"))
    } else {
        ItemDeEscolha::new(NOVO_PROJETO, "Novo projeto…")
    };
    let novo = ItemDeEscolha { simbolo: Some(Simbolo::icone(Lucide::FolderPlus)), ..novo };
    let mut grupos = Vec::new();
    if !itens.is_empty() {
        grupos.push(GrupoDeEscolha { titulo: Some("Projetos".into()), itens });
    }
    grupos.push(GrupoDeEscolha { titulo: None, itens: vec![novo] });
    grupos
}

/// O nome que vai virar a pasta, ou por que não serve (`None`: o campo está vazio).
fn validar_nome(texto: &str, existe: impl Fn(&str) -> bool) -> Result<String, Option<String>> {
    if texto.trim().is_empty() {
        return Err(None);
    }
    let nome = Motor::nome_de_projeto(texto)
        .map_err(|_| Some("Use um nome sem “/” e que não comece com ponto.".to_string()))?;
    if existe(&nome) {
        return Err(Some("Já existe uma pasta com esse nome.".into()));
    }
    Ok(nome)
}

impl Janela {
    /// Ctrl+N: sem projetos, o diálogo de projeto novo; com um só ou com um projeto no filtro,
    /// cria direto nele; nos outros casos, a paleta dos projetos.
    pub(super) fn dialogo_nova_conversa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let projetos = motor.read(cx).projetos();
        if projetos.is_empty() {
            self.dialogo_criar_projeto(window, cx);
            return;
        }
        if let Some(projeto) = self.projeto_filtro.clone().filter(|p| projetos.contains(p)) {
            self.nova_conversa(&projeto, window, cx);
            return;
        }
        if projetos.len() == 1 {
            self.nova_conversa(&projetos[0], window, cx);
            return;
        }
        let m = motor.read(cx);
        // O projeto da conversa aberta, senão o da conversa mais recente.
        let atual = self.selecionado.as_ref().and_then(|id| m.chat(id)).map(|c| c.projeto.clone());
        let realce = atual.clone().or_else(|| {
            m.chats.iter().filter(|c| !m.arquivada(c)).max_by_key(|c| c.atualizado_em).map(|c| c.projeto.clone())
        });
        let mut dados = Vec::new();
        for nome in &projetos {
            let conversas: Vec<u64> = m
                .chats
                .iter()
                .filter(|c| c.projeto == *nome && c.papel == Papel::Coordenador && !m.arquivada(c))
                .map(|c| c.atualizado_em)
                .collect();
            let (icone, cor) = icone_projeto::visual(m, nome);
            // Só o que já se sabe: perguntar ao git por projeto travaria o Ctrl+N.
            let sem_git = self.git_projetos.get(nome) == Some(&false);
            dados.push(ProjetoNaPaleta {
                nome: nome.clone(),
                icone,
                cor,
                descricao: descricao_do_projeto(conversas.len(), conversas.iter().max().copied()),
                selo: if atual.as_deref() == Some(nome.as_str()) {
                    Some("atual")
                } else {
                    sem_git.then_some("sem git")
                },
            });
        }
        let dados = Rc::new(dados);
        let janela = cx.entity().downgrade();
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar projeto…", window, cx);
            lista.definir(grupos_da_paleta(&dados, ""), realce.map(SharedString::from), cx);
            let filtro = dados.clone();
            lista.filtro_proprio(move |busca| grupos_da_paleta(&filtro, busca));
            let criar = janela.clone();
            lista.rodape(move |window, cx| {
                let criar = criar.clone();
                let largo = tam(window, 420.) < window.viewport_size().width - px(32.);
                div()
                    .flex()
                    .items_center()
                    .gap(r(12.))
                    .w_full()
                    .child(
                        botao_fantasma("paleta-novo-projeto", Some(Lucide::FolderPlus.into()), "Novo projeto…", TamanhoBotao::Barra, window, cx)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = criar.update(cx, |this, cx| this.dialogo_criar_projeto(window, cx));
                            }),
                    )
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
                this.dialogo_projeto_novo(nome.to_string(), window, cx);
            } else {
                this.nova_conversa(chave, window, cx);
            }
        }));
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            let fechou = janela.clone();
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Nova conversa", Some("Em qual projeto? O coordenador dele recebe a mensagem.".into()))))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
                // Fechado sem escolher (Esc, clique fora), "Nova conversa ao lado" não vale mais.
                .on_close(move |_, _, cx| {
                    let _ = fechou.update(cx, |this, _| this.nova_ao_lado = false);
                })
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// Projeto novo: uma subpasta da pasta de trabalho, com git se marcado, e já uma conversa nela.
    pub(super) fn dialogo_criar_projeto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.dialogo_projeto_novo(String::new(), window, cx);
    }

    /// O mesmo, com o nome já escrito ("Criar o projeto “ops”" da paleta).
    fn dialogo_projeto_novo(&mut self, nome: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let raiz = motor.read(cx).raiz().to_path_buf();
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder("loja-api").default_value(nome));
        let com_git = Rc::new(Cell::new(true));
        let janela = cx.entity().downgrade();
        let foco = campo.read(cx).focus_handle(cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let texto = campo.read(cx).value().to_string();
            let validacao = validar_nome(&texto, |n| motor.read(cx).caminho_projeto(n).exists());
            let git = com_git.get();
            let previa = if texto.trim().is_empty() {
                div()
                    .flex_none()
                    .size(r(56.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(raio::do_icone(56.))
                    .bg(Cores::recuo())
                    .text_color(Cores::tenue())
                    .child(icone(Lucide::FolderPlus, 24., window))
                    .into_any_element()
            } else {
                QuadradoDeProjeto::new(&icone_projeto::ICONES[0], icone_projeto::cor_automatica(texto.trim()), 56.)
                    .into_any_element()
            };
            let (ajuda, erro, motivo) = match &validacao {
                Ok(nome) => (format!("Vira a pasta {}.", com_til(&raiz.join(nome))), None, None),
                Err(None) => ("A pasta fica dentro da pasta de trabalho.".into(), None, Some("Escreva o nome do projeto")),
                Err(Some(erro)) => (String::new(), Some(SharedString::from(erro.clone())), Some("O nome não serve")),
            };
            let entrada = Input::new(&campo).when(erro.is_some(), |i| i.border_color(Cores::vermelho()));
            let alternar = com_git.clone();
            let linha_git = LinhaDeOpcao::new("projeto-novo-git", "Usar git")
                .descricao(if git {
                    "Cada tarefa trabalha numa cópia isolada."
                } else {
                    "As tarefas vão trabalhar direto na pasta."
                })
                .interruptor(git, move |ligado, window, _| {
                    alternar.set(ligado);
                    window.refresh();
                });
            let criar = {
                let (janela, campo, com_git) = (janela.clone(), campo.clone(), com_git.clone());
                move |window: &mut Window, cx: &mut App| -> bool {
                    let texto = campo.read(cx).value().to_string();
                    let Some(motor) = janela.upgrade().and_then(|j| j.read(cx).motor.clone()) else {
                        return false;
                    };
                    let Ok(nome) = validar_nome(&texto, |n| motor.read(cx).caminho_projeto(n).exists()) else {
                        return false;
                    };
                    let git = com_git.get();
                    let _ = janela.update(cx, |this, cx| this.criar_projeto(nome, git, window, cx));
                    true
                }
            };
            let criar = Rc::new(criar);
            let no_botao = criar.clone();
            let principal = botao_primario("projeto-novo-criar", None, "Criar e abrir conversa", TamanhoBotao::Controle, window, cx)
                .when_some(motivo, |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
                .on_click(move |_, window, cx| {
                    if no_botao(window, cx) {
                        window.close_dialog(cx);
                    }
                });
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Novo projeto", Some(format!("Uma pasta nova dentro de {}.", com_til(&raiz))))))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(r(16.))
                        .child(
                            div()
                                .flex()
                                .items_start()
                                .gap(r(16.))
                                .child(previa)
                                .child(div().flex_1().min_w_0().child(Campo::new("Nome", entrada).ajuda(ajuda).erro(erro))),
                        )
                        .child(GrupoDeOpcoes::new(None).linha(linha_git)),
                )
                .footer(
                    RodapeDialogo::new()
                        .cancelar(
                            botao_fantasma("projeto-novo-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(principal),
                )
                .on_ok(move |_, window, cx| criar(window, cx))
        });
        window.focus(&foco, cx);
    }

    /// Cria a pasta (e o git) em segundo plano e abre uma conversa nela.
    fn criar_projeto(&mut self, nome: String, com_git: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let tarefa = motor.update(cx, |m, cx| m.criar_projeto(&nome, com_git, cx));
        cx.spawn_in(window, async move |this, cx| {
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.git_projetos.remove(&nome);
                match resultado {
                    Ok(()) => window.push_notification(aviso(Tom::Pronto, format!("Projeto {nome} criado")), cx),
                    Err(erro) => window.push_notification(aviso_erro("Não deu para criar o projeto", &erro), cx),
                }
                // Se só o git falhou, a pasta já está lá e a conversa pode começar.
                let existe = this.motor.as_ref().is_some_and(|m| m.read(cx).caminho_projeto(&nome).is_dir());
                if existe {
                    this.nova_conversa(&nome, window, cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn abrir_configuracoes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let pasta = com_til(motor.read(cx).raiz());
        let vista = cx.new(|cx| Configuracoes::new(motor, cx));
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::G, window)
                .title(topo(
                    CabecalhoDialogo::new("Configurações")
                        .subtitulo(subtitulo_caminho("configuracoes-pasta", format!("Valem para a pasta {pasta}"))),
                ))
                .child(vista.clone())
        });
    }
}

impl Janela {
    pub(super) fn confirmar_git(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.as_ref() else {
            return;
        };
        let sem_gitignore = !motor.read(cx).caminho_projeto(&projeto).join(".gitignore").exists();
        let detalhes = if sem_gitignore {
            vec![(Lucide::FileText.into(), "Cria um .gitignore sem .env, ambientes e builds".into(), false)]
        } else {
            Vec::new()
        };
        let janela = cx.entity().downgrade();
        let alvo = projeto.clone();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Neutro,
                icone: Lucide::GitBranch.into(),
                titulo: format!("Ativar git em “{projeto}”?").into(),
                texto: "Cria um repositório com um commit dos arquivos atuais. Nada é apagado nem enviado.".into(),
                detalhes,
                ok: "Ativar git".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: false,
            },
            move |window, cx| {
                let projeto = alvo.clone();
                let _ = janela.update(cx, |this, cx| this.ativar_git(projeto, window, cx));
            },
            None,
            window,
            cx,
        );
    }

    fn ativar_git(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let tarefa = motor.update(cx, |m, cx| m.ativar_git(&projeto, cx));
        cx.spawn_in(window, async move |this, cx| {
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match resultado {
                    Ok(novo) => {
                        // O repositório nasce com o commit: Configurar projeto tira o aviso já.
                        this.git_projetos.insert(projeto.clone(), true);
                        let resumo = novo.resumo(&projeto);
                        let resumo = resumo.strip_suffix('.').unwrap_or(&resumo).to_string();
                        window.push_notification(aviso(Tom::Pronto, resumo), cx);
                    }
                    Err(erro) => {
                        this.git_projetos.remove(&projeto);
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

impl Janela {
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
        composer.update(cx, |c, cx| c.set_value(texto, window, cx));
        // Vindo do arquivo na gaveta, a caixa está embaixo dela: aberta, o Esc pararia o turno.
        if self.gaveta.take().is_some() {
            cx.notify();
        }
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
    }
}

impl Janela {
    /// Preparação, comando de rodar, arquivos que vão junto e ícone e cor; grava no Salvar.
    pub(super) fn dialogo_config_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let usa_git = self.usa_git(motor.read(cx), &projeto);
        let janela = cx.entity().downgrade();
        let vista = cx.new(|cx| ConfigProjetoView::new(motor, projeto, janela, usa_git, window, cx));
        let foco = vista.read(cx).foco(cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let (cabecalho_projeto, mudou) = {
                let v = vista.read(cx);
                (v.cabecalho(estreito(window, largura::G), cx), v.mudou(cx))
            };
            let (no_botao, no_enter) = (vista.clone(), vista.clone());
            moldura(dialogo, largura::G, window)
                .title(topo(cabecalho_projeto))
                .child(vista.clone())
                .footer(
                    RodapeDialogo::new()
                        .cancelar(
                            botao_fantasma("projeto-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(
                            botao_primario("projeto-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx)
                                .when(!mudou, |b| b.disabled(true).opacity(0.45).tooltip("Nada mudou"))
                                .on_click(move |_, window, cx| {
                                    no_botao.update(cx, |v, cx| v.salvar(window, cx));
                                    window.close_dialog(cx);
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| {
                    no_enter.update(cx, |v, cx| {
                        if v.mudou(cx) {
                            v.salvar(window, cx);
                        }
                    });
                    true
                })
        });
        window.focus(&foco, cx);
    }
}

/// As outras conversas com mensagens, por projeto (as mais recentes primeiro): coordenador ou
/// trabalhador, quando falou e quantas mensagens.
fn grupos_de_conversas(motor: &Motor, chat: &str) -> Vec<GrupoDeEscolha> {
    let mut conversas: Vec<_> = motor.chats.iter().filter(|c| c.id != chat && !c.itens.is_empty()).collect();
    conversas.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    let mut grupos: Vec<GrupoDeEscolha> = Vec::new();
    for projeto in motor.projetos() {
        let itens: Vec<ItemDeEscolha> = conversas
            .iter()
            .filter(|c| c.projeto == projeto)
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
                    termos: Some(projeto.clone().into()),
                    ..ItemDeEscolha::new(c.id.clone(), c.titulo.clone())
                }
            })
            .collect();
        if !itens.is_empty() {
            grupos.push(GrupoDeEscolha { titulo: Some(projeto.into()), itens });
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
        let (janela, projeto) = (cx.entity().downgrade(), self.projeto_filtro.clone());
        let vista = cx.new(|cx| super::rotinas::Rotinas::new(motor, janela, projeto, window, cx));
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
        CRIAR_PROJETO, NOVO_PROJETO, ProjetoNaPaleta, com_til, contas_conectadas, descricao_do_projeto,
        grupos_da_paleta, validar_nome,
    };
    use crate::{modelo::agora, motor::Motor, ui::icone_projeto::ICONES};

    #[test]
    fn nome_de_projeto_novo() {
        assert_eq!(Motor::nome_de_projeto("  loja-api ").unwrap(), "loja-api");
        assert_eq!(Motor::nome_de_projeto("Meu site").unwrap(), "Meu site");
        for ruim in ["", "   ", "a/b", "/etc", "..", ".oculto", "../fora"] {
            assert!(Motor::nome_de_projeto(ruim).is_err(), "{ruim:?} passou");
        }
    }

    #[test]
    fn nome_validado_ao_digitar() {
        assert_eq!(validar_nome("  ", |_| false), Err(None));
        assert_eq!(validar_nome(" loja ", |_| false), Ok("loja".to_string()));
        assert_eq!(validar_nome("loja", |n| n == "loja"), Err(Some("Já existe uma pasta com esse nome.".into())));
        assert!(matches!(validar_nome("a/b", |_| false), Err(Some(_))));
        assert!(matches!(validar_nome(".oculto", |_| false), Err(Some(_))));
    }

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
        assert_eq!(chaves(""), ["loja-api", "site-ninee", NOVO_PROJETO]);
        assert_eq!(chaves("NINEE"), ["site-ninee", NOVO_PROJETO]);
        assert_eq!(chaves(" ops "), [format!("{CRIAR_PROJETO}ops")]);
        let titulo = &grupos_da_paleta(&projetos, "ops")[0].itens[0].titulo;
        assert_eq!(titulo.as_ref(), "Criar o projeto “ops”");
    }

    #[test]
    fn descricao_do_projeto_na_paleta() {
        assert_eq!(descricao_do_projeto(0, None), "Nenhuma conversa ainda");
        assert_eq!(descricao_do_projeto(1, Some(agora())), "1 conversa · última agora");
        assert_eq!(descricao_do_projeto(3, Some(agora() - 7200)), "3 conversas · última há 2 h");
    }
}
