//! Diálogos: novo chat (em qual pasta), renomear, contas, configurações, mencionar arquivo e
//! conversa como contexto, todos na mesma moldura.

use std::{cell::RefCell, path::{Path, PathBuf}, rc::Rc};

use gpui_kit::component::{
    Disableable as _, WindowExt as _, v_flex,
    dialog::Dialog,
    input::{Input, InputState},
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        CabecalhoDialogo, EventoEscolha, GrupoDeEscolha, ItemDeEscolha, ListaDeEscolha, RodapeDialogo, Selo, Simbolo,
        Sinal, TamanhoBotao, Tom, aviso, aviso_erro, botao_fantasma, botao_primario, contagem, desenho_teclas,
        fio_de_pontos, largura, sinal_do_chat,
    },
    configuracoes::Configuracoes,
    painel_contas::PainelContas,
    seletor_arquivos,
    tempo_relativo,
};
use crate::{
    modelo::Item,
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
        Some(resto) => format!("~{}{}", std::path::MAIN_SEPARATOR, resto.display()),
        None => caminho.display().to_string(),
    }
}

/// O nome da pasta (a última parte do caminho), o que a lista e as dicas mostram de um chat.
pub(super) fn nome_da_pasta(pasta: &Path) -> String {
    pasta.file_name().map_or_else(|| pasta.display().to_string(), |n| n.to_string_lossy().to_string())
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

/// Chaves da paleta que não são pastas (nenhum caminho tem o "\0").
const SEM_PASTA: &str = "\0sem-pasta";
/// "Outra pasta…" na paleta do Novo chat: escolhe a pasta no disco e o chat nasce nela.
const OUTRA_PASTA: &str = "\0outra-pasta";

/// Uma pasta na paleta de Novo chat.
struct PastaNaPaleta {
    caminho: PathBuf,
    nome: String,
    descricao: String,
    atual: bool,
}

impl PastaNaPaleta {
    fn new(caminho: PathBuf, atual: bool) -> Self {
        Self { descricao: com_til(&caminho), nome: nome_da_pasta(&caminho), caminho, atual }
    }
}

/// As pastas que já têm chat (as mais recentes primeiro, com a busca no nome ou no caminho),
/// "Outra pasta…", que escolhe uma no disco, e "Sem pasta".
fn grupos_da_paleta(pastas: &[PastaNaPaleta], busca: &str) -> Vec<GrupoDeEscolha> {
    let termo = busca.trim().to_lowercase();
    let sem_pasta = "sem pasta avulso só conversar".contains(&termo).then(|| ItemDeEscolha {
        simbolo: Some(Simbolo::icone(Lucide::MessageCircle)),
        descricao: Some("Só conversar: o chat ganha uma pasta vazia só dele".into()),
        ..ItemDeEscolha::new(SEM_PASTA, "Sem pasta")
    });
    let outra = "outra pasta abrir escolher".contains(&termo).then(|| ItemDeEscolha {
        simbolo: Some(Simbolo::icone(Lucide::FolderOpen)),
        descricao: Some("Escolher uma pasta do computador".into()),
        ..ItemDeEscolha::new(OUTRA_PASTA, "Outra pasta…")
    });
    let itens: Vec<ItemDeEscolha> = pastas
        .iter()
        .filter(|p| p.nome.to_lowercase().contains(&termo) || p.descricao.to_lowercase().contains(&termo))
        .map(|p| ItemDeEscolha {
            simbolo: Some(Simbolo::icone(Lucide::Folder)),
            descricao: Some(p.descricao.clone().into()),
            selos: p.atual.then(|| Selo::neutro("atual")).into_iter().collect(),
            ..ItemDeEscolha::new(p.caminho.display().to_string(), p.nome.clone())
        })
        .collect();
    let mut grupos = Vec::new();
    if !itens.is_empty() {
        grupos.push(GrupoDeEscolha { titulo: Some("Pastas".into()), itens });
    }
    let resto: Vec<ItemDeEscolha> = outra.into_iter().chain(sem_pasta).collect();
    if !resto.is_empty() {
        grupos.push(GrupoDeEscolha { titulo: None, itens: resto });
    }
    grupos
}

/// Onde nasce o chat do "+" e do Ctrl+N.
#[derive(Clone, Debug, PartialEq)]
enum Destino {
    /// Numa pasta.
    Pasta(PathBuf),
    /// Numa pasta vazia só dele.
    SemPasta,
    /// Não há pasta à mão: a paleta pergunta.
    Perguntar,
}

/// Com pasta de trabalho, é nela, sempre (como a pasta aberta no VS Code). Sem ela: na pasta do
/// chat aberto (o chat Sem pasta abre outro Sem pasta: a pasta dele é só dele); sem chat aberto,
/// na última pasta usada; sem nenhuma, pergunta. Pasta que sumiu do disco não vale.
fn destino_do_novo_chat(
    de_trabalho: Option<&Path>,
    aberto: Option<(&Path, bool)>,
    ultima: Option<&Path>,
    existe: impl Fn(&Path) -> bool,
) -> Destino {
    if let Some(pasta) = de_trabalho.filter(|p| existe(p)) {
        return Destino::Pasta(pasta.to_path_buf());
    }
    match aberto {
        Some((_, true)) => Destino::SemPasta,
        Some((pasta, false)) if existe(pasta) => Destino::Pasta(pasta.to_path_buf()),
        _ => match ultima.filter(|p| existe(p)) {
            Some(pasta) => Destino::Pasta(pasta.to_path_buf()),
            None => Destino::Perguntar,
        },
    }
}

impl Janela {
    /// O "+" e o Ctrl+N: um chat novo na hora, sem perguntar nada, como o "novo chat" do VS Code
    /// (pedido do dono, 12/11/2026). Escolher outra pasta fica no menu do logo ("Novo chat em outra
    /// pasta…").
    pub(super) fn novo_chat_direto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let destino = {
            let m = motor.read(cx);
            let aberto = self.selecionado.as_ref().and_then(|id| m.chat(id)).map(|c| (c.cwd.as_path(), m.sem_pasta(c)));
            // A última pasta usada: a do último chat criado; sem ela, a do chat mexido por último.
            let ultima = self.ultimo_destino.clone().or_else(|| m.pastas_dos_chats().into_iter().next());
            destino_do_novo_chat(m.config.pasta_de_trabalho.as_deref(), aberto, ultima.as_deref(), Path::is_dir)
        };
        match destino {
            Destino::Pasta(pasta) => {
                self.lembrar_destino(&pasta, cx);
                self.nova_conversa_na_pasta(Some(pasta), window, cx);
            }
            Destino::SemPasta => self.nova_conversa_na_pasta(None, window, cx),
            Destino::Perguntar => self.dialogo_nova_conversa(window, cx),
        }
    }

    /// "Trocar a pasta de trabalho…": o seletor do sistema; os chats novos passam a nascer nela (os
    /// que já existem ficam onde estão).
    pub(super) fn trocar_pasta_de_trabalho(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
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
            let _ = this.update(cx, |this, cx| {
                let pasta = dunce::canonicalize(&pasta).unwrap_or(pasta);
                this.lembrar_destino(&pasta, cx);
                motor.update(cx, |m, cx| m.mudar_config(|c| c.pasta_de_trabalho = Some(pasta), cx));
                cx.notify();
            });
        })
        .detach();
    }

    /// "Novo chat em outra pasta…": as pastas que já têm chat, "Outra pasta…" e "Sem pasta". Nada
    /// de projeto por baixo: o chat nasce direto na pasta escolhida.
    pub(super) fn dialogo_nova_conversa(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let m = motor.read(cx);
        let aberta = self.selecionado.as_ref().and_then(|id| m.chat(id)).filter(|c| !m.sem_pasta(c)).map(|c| c.cwd.clone());
        let pastas: Vec<PastaNaPaleta> = m
            .pastas_dos_chats()
            .into_iter()
            .filter(|p| p.is_dir())
            .map(|p| {
                let atual = aberta.as_ref() == Some(&p);
                PastaNaPaleta::new(p, atual)
            })
            .collect();
        let realce = aberta.or_else(|| self.ultimo_destino.clone()).map(|p| SharedString::from(p.display().to_string()));
        let pastas = Rc::new(pastas);
        let janela = cx.entity().downgrade();
        let lista = cx.new(|cx| {
            let mut lista = ListaDeEscolha::new("Buscar pasta…", window, cx);
            lista.definir(grupos_da_paleta(&pastas, ""), realce, cx);
            let filtro = pastas.clone();
            lista.filtro_proprio(move |busca| grupos_da_paleta(&filtro, busca));
            lista.rodape(move |window, _| {
                let largo = tam(window, 420.) < window.viewport_size().width - px(32.);
                // O fio de pontos em cima das teclas, como no rodapé dos popovers da caixa.
                v_flex()
                    .w_full()
                    .gap(r(6.))
                    .when(largo, |s| s.child(fio_de_pontos(Cores::fio(), window)))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(r(12.))
                            .w_full()
                            .child(div().flex_1())
                            .when(largo, |s| {
                                s.child(dica_teclas(&["↑", "↓"], "escolher")).child(dica_teclas(&["Enter"], "abrir"))
                            }),
                    )
                    .into_any_element()
            });
            lista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&lista, window, |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(chave) = evento else {
                return;
            };
            window.close_dialog(cx);
            match chave.as_ref() {
                OUTRA_PASTA => this.novo_chat_noutra_pasta(window, cx),
                SEM_PASTA => this.nova_conversa_na_pasta(None, window, cx),
                pasta => {
                    let pasta = PathBuf::from(pasta);
                    this.lembrar_destino(&pasta, cx);
                    this.nova_conversa_na_pasta(Some(pasta), window, cx);
                }
            }
        }));
        self.foco_antes_do_dialogo(window, cx);
        let vista = lista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            let fechou = janela.clone();
            moldura(dialogo, largura::M, window)
                .title(topo(cabecalho("Novo chat", Some("Em qual pasta?".into()))))
                .child(vista.clone())
                .on_ok(|_, _, _| false)
                // Fechado sem escolher (Esc, clique fora), "Nova conversa ao lado" não vale mais.
                .on_close(move |_, _, cx| {
                    let _ = fechou.update(cx, |this, _| this.nova_ao_lado = false);
                })
        });
        lista.update(cx, |l, cx| l.abrir(window, cx));
    }

    /// "Outra pasta…": escolhe a pasta no disco e abre o chat nela.
    fn novo_chat_noutra_pasta(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Abrir o chat nesta pasta".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else {
                return;
            };
            let Some(pasta) = caminhos.into_iter().next() else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                this.lembrar_destino(&pasta, cx);
                this.nova_conversa_na_pasta(Some(pasta), window, cx);
            });
        })
        .detach();
    }

    /// Grava a pasta do último chat criado: o próximo, sem chat aberto, nasce nela.
    fn lembrar_destino(&mut self, pasta: &Path, cx: &mut Context<Self>) {
        self.ultimo_destino = Some(pasta.to_path_buf());
        super::gravar_interface("ultimo_destino", serde_json::Value::String(pasta.to_string_lossy().to_string()), cx);
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
                .child("Valem para todos os chats")
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx));
            moldura(dialogo, largura::G, window)
                .title(topo(CabecalhoDialogo::new("Configurações").subtitulo(subtitulo)))
                .child(vista.clone())
        });
    }
}

impl Janela {
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
    pub(super) fn dialogo_mencionar_arquivo(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
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

/// As outras conversas com mensagens (as mais recentes primeiro): quando falou, quantas mensagens e
/// a pasta, que também vale na busca.
fn grupos_de_conversas(motor: &Motor, chat: &str) -> Vec<GrupoDeEscolha> {
    let mut conversas: Vec<_> = motor.chats.iter().filter(|c| c.id != chat && !c.itens.is_empty()).collect();
    conversas.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    let itens: Vec<ItemDeEscolha> = conversas
        .iter()
        .map(|c| {
            let mensagens = c
                .itens
                .iter()
                .filter(|i| matches!(i, Item::Usuario { .. } | Item::Assistente { .. } | Item::Entrega { .. }))
                .count();
            let quando = tempo_relativo(c.atualizado_em);
            let quando = if quando == "agora" { quando } else { format!("há {quando}") };
            let pasta = if motor.sem_pasta(c) { "sem pasta".to_string() } else { nome_da_pasta(&c.cwd) };
            let rodando = sinal_do_chat(motor, c).filter(|s| *s == Sinal::Rodando);
            ItemDeEscolha {
                simbolo: Some(Simbolo::icone(Lucide::MessagesSquare)),
                descricao: Some(format!("{quando} · {} · {pasta}", contagem(mensagens, "mensagem", "mensagens")).into()),
                selos: rodando.map(|s| Selo::ponto(s.cor()).vazado()).into_iter().collect(),
                termos: Some(com_til(&c.cwd).into()),
                ..ItemDeEscolha::new(c.id.clone(), c.titulo.clone())
            }
        })
        .collect();
    if itens.is_empty() {
        let vazio = "Nenhuma outra conversa com mensagens";
        return vec![GrupoDeEscolha {
            titulo: None,
            itens: vec![ItemDeEscolha { desabilitado: Some(vazio.into()), ..ItemDeEscolha::new("", vazio) }],
        }];
    }
    vec![GrupoDeEscolha { titulo: None, itens }]
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

#[cfg(test)]
mod testes {
    // Só o que é preciso: o `use gpui_kit::*` lá de cima traz um `test` que troca o da std.
    use std::path::{Path, PathBuf};

    use super::{Destino, OUTRA_PASTA, PastaNaPaleta, SEM_PASTA, com_til, contas_conectadas, destino_do_novo_chat, grupos_da_paleta};

    #[test]
    fn contagem_de_contas() {
        assert_eq!(contas_conectadas(1), "1 conta conectada");
        assert_eq!(contas_conectadas(3), "3 contas conectadas");
    }

    #[test]
    fn caminho_com_a_pasta_pessoal_em_til() {
        let casa = dirs::home_dir().unwrap();
        let barra = std::path::MAIN_SEPARATOR;
        assert_eq!(com_til(&casa.join("Projetos").join("loja")), format!("~{barra}Projetos{barra}loja"));
        assert_eq!(com_til(&casa), "~");
        assert_eq!(com_til(std::path::Path::new("/etc/vitral")), "/etc/vitral");
    }

    #[test]
    fn paleta_mostra_as_pastas_outra_e_sem_pasta() {
        let pastas = [PastaNaPaleta::new(PathBuf::from("/p/loja-api"), true), PastaNaPaleta::new(PathBuf::from("/p/site-ninee"), false)];
        let chaves = |busca: &str| -> Vec<String> {
            grupos_da_paleta(&pastas, busca).iter().flat_map(|g| g.itens.iter().map(|i| i.chave.to_string())).collect()
        };
        assert_eq!(chaves(""), ["/p/loja-api", "/p/site-ninee", OUTRA_PASTA, SEM_PASTA]);
        assert_eq!(chaves("NINEE"), ["/p/site-ninee"]);
        assert_eq!(chaves("/p/"), ["/p/loja-api", "/p/site-ninee"]);
        assert_eq!(chaves("sem pasta"), [SEM_PASTA]);
        assert_eq!(chaves("outra"), [OUTRA_PASTA]);
        // Nada de criar projeto: sem pasta que bata, a lista fica vazia.
        assert!(chaves(" ops ").is_empty());
        let grupos = grupos_da_paleta(&pastas, "");
        assert_eq!(grupos[0].titulo.as_deref(), Some("Pastas"));
        assert_eq!(grupos[0].itens[0].titulo.as_ref(), "loja-api");
        assert_eq!(grupos[0].itens[0].selos.len(), 1, "a pasta do chat aberto vem marcada");
        assert!(grupos[0].itens[1].selos.is_empty());
    }

    #[test]
    fn novo_chat_na_pasta_do_aberto_senao_na_ultima_senao_pergunta() {
        let existe = |p: &Path| p != Path::new("/sumiu");
        let (loja, site) = (Path::new("/p/loja"), Path::new("/p/site"));
        assert_eq!(destino_do_novo_chat(None, Some((loja, false)), Some(site), existe), Destino::Pasta(loja.into()));
        // O chat Sem pasta abre outro Sem pasta, não a pasta só dele.
        assert_eq!(destino_do_novo_chat(None, Some((Path::new("/d/avulsos/a"), true)), Some(site), existe), Destino::SemPasta);
        assert_eq!(destino_do_novo_chat(None, None, Some(site), existe), Destino::Pasta(site.into()));
        // A pasta do chat aberto sumiu do disco: vale a última usada.
        assert_eq!(destino_do_novo_chat(None, Some((Path::new("/sumiu"), false)), Some(site), existe), Destino::Pasta(site.into()));
        assert_eq!(destino_do_novo_chat(None, None, Some(Path::new("/sumiu")), existe), Destino::Perguntar);
        assert_eq!(destino_do_novo_chat(None, None, None, existe), Destino::Perguntar);
        // A pasta de trabalho vence o chat aberto e a última usada; sumida do disco, não vale.
        assert_eq!(destino_do_novo_chat(Some(site), Some((loja, false)), None, existe), Destino::Pasta(site.into()));
        assert_eq!(destino_do_novo_chat(Some(Path::new("/sumiu")), Some((loja, false)), None, existe), Destino::Pasta(loja.into()));
    }
}

impl Janela {
    /// Fecha os painéis das conversas (o último fica vazio) e tira a seleção delas; com `sairam`
    /// (saíram do Vitral), esquece também os rascunhos e os anexos.
    pub(super) fn largar_chats(&mut self, chats: &[String], sairam: bool, window: &mut Window, cx: &mut Context<Self>) {
        let fechar: Vec<u64> = self
            .paineis
            .iter()
            .filter(|p| p.chat.as_ref().is_some_and(|c| chats.contains(c)))
            .map(|p| p.id)
            .collect();
        // As abas de fundo dessas conversas saem primeiro, em todos os painéis.
        for painel in &mut self.paineis {
            let a_vista = painel.chat.clone();
            painel.abas.retain(|a| Some(a) == a_vista.as_ref() || !chats.contains(a));
        }
        for id in fechar {
            let (com_outras_abas, a_vista) = self
                .paineis
                .iter()
                .find(|p| p.id == id)
                .map_or((false, None), |p| (p.abas.len() > 1, p.chat.clone()));
            if let (true, Some(chat)) = (com_outras_abas, a_vista) {
                // O painel fica com as outras abas: só a da conversa que saiu fecha.
                self.fechar_aba(id, &chat, window, cx);
            } else if self.paineis.len() > 1 {
                self.fechar_painel(id, cx);
            } else if let Some(ix) = self.paineis.iter().position(|p| p.id == id) {
                self.ativo = ix;
                self.selecionar(None, window, cx);
            }
        }
        if self.selecionado.as_ref().is_some_and(|c| chats.contains(c)) {
            self.selecionado = None;
        }
        if self.realce_lista.as_ref().is_some_and(|c| chats.contains(c)) {
            self.realce_lista = None;
        }
        if sairam {
            for chat in chats {
                self.rascunhos.remove(chat);
                self.anexos.remove(chat);
                self.ultrathink.remove(chat);
                self.ultracode.remove(chat);
            }
            // A conversa saiu do Vitral: o rascunho dela também sai do disco.
            self.agendar_gravacao(cx);
        }
        cx.notify();
    }

    /// Arquivar pelo menu do chat: o chat vazio sai em vez de arquivar; arquivar se desfaz no aviso.
    pub(super) fn arquivar_do_menu(&mut self, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        if motor.update(cx, |m, cx| m.arquivar_chat(&chat, cx)) {
            self.largar_chats(std::slice::from_ref(&chat), true, window, cx);
            window.push_notification(aviso(Tom::Info, "Chat vazio apagado"), cx);
            return;
        }
        super::lateral::avisar_arquivadas(motor, vec![chat], "Conversa arquivada".into(), window, cx);
    }
}
