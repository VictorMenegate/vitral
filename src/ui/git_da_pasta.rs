//! "Na pasta": o que o git vê mudado desde o último commit no repositório do chat, inclusive o que
//! um comando mudou (a lista de Mudanças só vê as ferramentas de edição). O clique abre o arquivo
//! com o diff por cima, no mesmo visor das edições, onde o número da linha escolhe um trecho para
//! mencionar; o Desfazer volta um arquivo ao último commit, depois de perguntar.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui_kit::component::{WindowExt as _, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, Reflexo as _, Selo, TamanhoBotao, Tom, TomConfirmacao, aviso, botao_icone, confirmar},
};
use crate::{
    git::{EstadoNoGit, MudancaNaPasta, RamoDaPasta, RetratoDaPasta},
    modelo::Chat,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, dentro_da_borda, esp, r, raio},
};

/// De quanto em quanto tempo a seção relê o git enquanto está à vista.
const RELER_A_CADA: Duration = Duration::from_secs(4);

/// O último retrato lido e de qual pasta.
#[derive(Default)]
pub(super) struct GitDaPasta {
    pasta: Option<PathBuf>,
    lido_em: Option<Instant>,
    retrato: Option<RetratoDaPasta>,
    lendo: bool,
}

impl Janela {
    /// Relê o git da pasta em segundo plano quando ela mudou ou o retrato envelheceu.
    fn reler_git_se_preciso(&mut self, pasta: &Path, cx: &Context<Self>) {
        let estado = &mut self.git_da_pasta;
        let outra = estado.pasta.as_deref() != Some(pasta);
        let velho = estado.lido_em.is_none_or(|em| em.elapsed() >= RELER_A_CADA);
        if estado.lendo || !(outra || velho) {
            return;
        }
        if outra {
            estado.retrato = None;
        }
        estado.pasta = Some(pasta.to_path_buf());
        estado.lendo = true;
        let pasta = pasta.to_path_buf();
        cx.spawn(async move |janela, cx| {
            let lida = pasta.clone();
            let retrato = cx.background_spawn(async move { crate::git::retrato_da_pasta(&lida) }).await;
            let _ = janela.update(cx, |this, cx| {
                let estado = &mut this.git_da_pasta;
                estado.lendo = false;
                if estado.pasta.as_deref() == Some(pasta.as_path()) {
                    let mudou = estado.retrato != retrato;
                    estado.retrato = retrato;
                    estado.lido_em = Some(Instant::now());
                    if mudou {
                        cx.notify();
                    }
                }
            });
        })
        .detach();
        // O próximo quadro depois do prazo relê de novo, mesmo sem nada mais pedir desenho.
        cx.spawn(async move |janela, cx| {
            cx.background_executor().timer(RELER_A_CADA).await;
            let _ = janela.update(cx, |_, cx| cx.notify());
        })
        .detach();
    }

    /// A seção "Na pasta" do Este chat: nada fora de um repositório git.
    pub(super) fn secao_git(&mut self, chat: &Chat, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        self.reler_git_se_preciso(&chat.cwd, cx);
        let retrato = self.git_da_pasta.retrato.as_ref()?;
        let raiz = retrato.raiz.clone();
        // O ramo vai no cabeçalho, à direita; sem lugar na linha, desce inteiro para a de baixo.
        let titulo = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap_x(esp::E6)
            .gap_y(esp::E4)
            .child(div().rotulo().text_color(Cores::forte()).child("Na pasta"))
            .child(div().font_family(FONTE_MONO).metadado().text_color(Cores::apagado()).child(retrato.mudancas.len().to_string()))
            .children(retrato.ramo.as_ref().map(|ramo| selo_do_ramo(ramo, window)));
        let corpo: Vec<AnyElement> = if retrato.mudancas.is_empty() {
            vec![div().corpo_ui().text_color(Cores::tenue()).child("Nada mudado desde o último commit.").into_any_element()]
        } else {
            retrato.mudancas.iter().enumerate().map(|(i, m)| linha_git(&raiz, m, i, window, cx)).collect()
        };
        Some(
            div()
                .flex()
                .flex_col()
                .gap(esp::E8)
                .p(dentro_da_borda(12.))
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::linha())
                .reflexo(raio::cartao())
                .child(titulo)
                .child(div().flex().flex_col().gap(esp::E2).children(corpo))
                .when(retrato.cortado, |s| s.child(div().metadado().text_color(Cores::tenue()).child("Mostrando os primeiros 200.")))
                .child(
                    div()
                        .metadado()
                        .text_color(Cores::tenue())
                        .child("O que o git vê desde o último commit, de qualquer chat ou de você. O clique mostra o diff."),
                )
                .into_any_element(),
        )
    }

    /// Abre o arquivo com o diff do git por cima (o novo, sem diff: só o arquivo).
    fn abrir_diff_do_git(&mut self, raiz: PathBuf, caminho: String, novo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if novo {
            self.abrir_arquivo(raiz, caminho, window, cx);
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let (r, c) = (raiz.clone(), caminho.clone());
            let diff = cx.background_spawn(async move { crate::git::diff_do_arquivo(&r, &c) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let trechos = super::edicao::trechos(&diff);
                let (entram, saem) = super::edicao::contar(&trechos);
                let sem_registro = trechos.is_empty();
                this.edicao_pedida = Some(std::rc::Rc::new(super::edicao::Edicao { trechos, entram, saem, sem_registro }));
                this.abrir_arquivo(raiz, caminho, window, cx);
            });
        })
        .detach();
    }

    /// Pergunta e volta um arquivo ao último commit.
    fn desfazer_no_git(&mut self, raiz: PathBuf, caminho: String, window: &mut Window, cx: &mut Context<Self>) {
        let janela = cx.entity().downgrade();
        let nome = Path::new(&caminho).file_name().map_or_else(|| caminho.clone(), |n| n.to_string_lossy().into_owned());
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Perigo,
                icone: Lucide::RotateCcw.into(),
                titulo: format!("Desfazer {nome}?").into(),
                texto: "O arquivo volta a como está no último commit.".into(),
                detalhes: vec![(
                    Lucide::TriangleAlert.into(),
                    "Tudo o que mudou nele desde então sai, de qualquer chat ou seu".into(),
                    true,
                )],
                ok: "Desfazer".into(),
                cancelar: "Deixar como está".into(),
                ok_perigo: true,
            },
            move |window, cx| {
                let (raiz, caminho, janela) = (raiz.clone(), caminho.clone(), janela.clone());
                window
                    .spawn(cx, async move |cx| {
                        let feito = cx.background_spawn(async move { crate::git::desfazer_arquivo(&raiz, &caminho) }).await;
                        let _ = cx.update(|window, cx| {
                            match feito {
                                Ok(()) => window.push_notification(aviso(Tom::Pronto, "Arquivo de volta ao último commit"), cx),
                                Err(erro) => window.push_notification(aviso(Tom::Atencao, format!("Não desfiz: {erro:#}")), cx),
                            }
                            let _ = janela.update(cx, |this, cx| {
                                this.git_da_pasta.lido_em = None;
                                cx.notify();
                            });
                        });
                    })
                    .detach();
            },
            None,
            window,
            cx,
        );
    }
}

/// O ramo no cabeçalho de "Na pasta": o ícone (o do commit, com a cabeça solta), o nome em mono e
/// "worktree" quando a pasta é uma cópia; a frase inteira na dica.
fn selo_do_ramo(ramo: &RamoDaPasta, window: &Window) -> AnyElement {
    let icone = if ramo.solta { Lucide::GitCommitHorizontal } else { Lucide::GitBranch };
    let dica = SharedString::from(super::ramo::onde_esta(ramo));
    div()
        .id("ramo-da-pasta")
        .ml_auto()
        .min_w_0()
        .flex()
        .items_center()
        .gap(esp::E4)
        .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(icone, 12., window)))
        .child(div().min_w_0().truncate().font_family(FONTE_MONO).metadado().text_color(Cores::texto()).child(ramo.nome.clone()))
        .when(ramo.solta, |s| s.child(div().flex_none().metadado().text_color(Cores::tenue()).child("cabeça solta")))
        .when(ramo.worktree, |s| s.child(Selo::cor("worktree", Cores::tenue())))
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .into_any_element()
}

/// Uma linha de "Na pasta": ícone, nome, pasta tênue, +N −M, o selo do estado e, no hover, o
/// Desfazer (no que o git já conhece).
fn linha_git(raiz: &Path, m: &MudancaNaPasta, i: usize, window: &Window, cx: &Context<Janela>) -> AnyElement {
    let (pasta, nome) = m.caminho.rsplit_once('/').unwrap_or(("", m.caminho.as_str()));
    let (pasta, nome) = (pasta.to_string(), nome.to_string());
    let icone = super::seletor_arquivos::icone_do_arquivo(&nome);
    let grupo = SharedString::from(format!("git-{i}"));
    let apagado = m.estado == EstadoNoGit::Apagado;
    let novo = m.estado == EstadoNoGit::Novo;
    let selo = match m.estado {
        EstadoNoGit::Novo => Some(("novo", Cores::verde())),
        EstadoNoGit::Apagado => Some(("apagado", Cores::vermelho())),
        EstadoNoGit::Renomeado => Some(("renomeado", Cores::tenue())),
        EstadoNoGit::Mudado => None,
    };
    let dica = SharedString::from(m.caminho.clone());
    let (raiz_a, caminho_a) = (raiz.to_path_buf(), m.caminho.clone());
    let (raiz_d, caminho_d) = (raiz.to_path_buf(), m.caminho.clone());
    let desfazer = (!novo && m.estado != EstadoNoGit::Renomeado).then(|| {
        div().flex_none().invisible().group_hover(grupo.clone(), |s| s.visible()).child(
            botao_icone(format!("git-desfazer-{i}"), Lucide::RotateCcw, "Desfazer: volta ao último commit", TamanhoBotao::Linha, window, cx)
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.desfazer_no_git(raiz_d.clone(), caminho_d.clone(), window, cx);
                })),
        )
    });
    div()
        .id(SharedString::from(format!("git-linha-{i}")))
        .group(grupo)
        .flex()
        .items_center()
        .gap(esp::E8)
        .h(r(28.))
        .px(esp::E6)
        .rounded(raio::CONTROLE)
        .when(!apagado, |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
        .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(icone, 14., window)))
        .child(
            div()
                .flex_shrink(1.)
                .min_w_0()
                .truncate()
                .corpo_ui()
                .text_color(if apagado { Cores::tenue() } else { Cores::texto() })
                .child(nome),
        )
        .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(pasta))
        .child(
            div()
                .flex()
                .flex_none()
                .gap(esp::E6)
                .font_family(FONTE_MONO)
                .metadado()
                .when(m.entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{}", m.entram))))
                .when(m.saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{}", m.saem)))),
        )
        .children(selo.map(|(t, cor)| Selo::cor(t, cor)))
        .children(desfazer)
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .when(!apagado, |s| {
            s.on_click(cx.listener(move |this, _, window, cx| {
                this.abrir_diff_do_git(raiz_a.clone(), caminho_a.clone(), novo, window, cx)
            }))
        })
        .into_any_element()
}
