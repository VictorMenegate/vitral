//! "Na pasta": o que o git vê mudado desde o último commit no repositório do chat, inclusive o que
//! um comando mudou (a lista de Mudanças só vê as ferramentas de edição). O clique abre o arquivo
//! com o diff por cima, no mesmo visor das edições, onde o número da linha escolhe um trecho para
//! mencionar; o Desfazer volta um arquivo ao último commit, depois de perguntar.

use std::{
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui_kit::component::{WindowExt as _, tooltip::{ManagedTooltipExt as _, Tooltip}};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, TamanhoBotao, Tom, TomConfirmacao, aviso, botao_icone, confirmar},
    este_chat::{grupo_da_direita, linha_da_direita, mais_e_menos, mono_da_linha, simbolo_da_linha},
};
use crate::{
    git::{EstadoNoGit, MudancaNaPasta, RamoDaPasta, RetratoDaPasta},
    modelo::Chat,
    tema::{Cores, FONTE_MONO, Lucide, esp, r},
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

    /// O ramo da pasta do chat, para o grupo Pasta do Este chat (relê o git quando precisa).
    pub(super) fn ramo_da_pasta(&mut self, pasta: &Path, cx: &Context<Self>) -> Option<RamoDaPasta> {
        self.reler_git_se_preciso(pasta, cx);
        self.git_da_pasta.retrato.as_ref()?.ramo.clone()
    }

    /// O grupo "Na pasta" do Este chat (o que o git vê desde o último commit, de qualquer chat ou
    /// seu; o clique mostra o diff): nada fora de um repositório git.
    pub(super) fn secao_git(&mut self, chat: &Chat, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        self.reler_git_se_preciso(&chat.cwd, cx);
        let retrato = self.git_da_pasta.retrato.as_ref()?;
        let raiz = retrato.raiz.clone();
        let mut linhas: Vec<AnyElement> = if retrato.mudancas.is_empty() {
            vec![
                linha_da_direita("git-nada")
                    .text_color(Cores::tenue())
                    .child("Nada mudado desde o último commit")
                    .into_any_element(),
            ]
        } else {
            retrato.mudancas.iter().enumerate().map(|(i, m)| linha_git(&raiz, m, i, window, cx)).collect()
        };
        if retrato.cortado {
            linhas.push(linha_da_direita("git-cortado").text_color(Cores::tenue()).child("Mostrando os primeiros 200").into_any_element());
        }
        Some(grupo_da_direita("Na pasta", None, None, linhas))
    }

    /// Abre o arquivo com o diff do git por cima (o novo, sem diff: só o arquivo). Com o "Ignorar
    /// espaço em branco" ligado (`diff_sem_espaco`, no "…" do arquivo), o diff sai com o `-w`.
    pub(super) fn abrir_diff_do_git(&mut self, raiz: PathBuf, caminho: String, novo: bool, window: &mut Window, cx: &mut Context<Self>) {
        if novo {
            self.abrir_arquivo(raiz, caminho, window, cx);
            return;
        }
        let sem_espaco = self.diff_sem_espaco;
        cx.spawn_in(window, async move |this, cx| {
            let (r, c) = (raiz.clone(), caminho.clone());
            let diff = cx.background_spawn(async move { crate::git::diff_do_arquivo(&r, &c, sem_espaco) }).await;
            let _ = this.update_in(cx, |this, window, cx| {
                let trechos = super::edicao::trechos(&diff);
                let (entram, saem) = super::edicao::contar(&trechos);
                let sem_registro = trechos.is_empty();
                let origem = super::edicao::Origem::Git { sem_espaco };
                this.edicao_pedida = Some(std::rc::Rc::new(super::edicao::Edicao { trechos, entram, saem, sem_registro, origem }));
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

/// Uma linha de "Na pasta": ícone, nome em mono, +N −M, a letra do estado (M, A, D, R) e, no
/// hover, o Desfazer (no que o git já conhece). A pasta vai na dica.
fn linha_git(raiz: &Path, m: &MudancaNaPasta, i: usize, window: &Window, cx: &Context<Janela>) -> AnyElement {
    // Uma pasta nova inteira vem com a barra no fim ("capturas/").
    let e_pasta = m.caminho.ends_with('/');
    let sem_barra = m.caminho.trim_end_matches('/');
    let nome = sem_barra.rsplit_once('/').map_or(sem_barra, |(_, n)| n).to_string();
    let icone = if e_pasta { Lucide::Folder } else { super::seletor_arquivos::icone_do_arquivo(&nome) };
    let grupo = SharedString::from(format!("git-{i}"));
    let apagado = m.estado == EstadoNoGit::Apagado;
    let novo = m.estado == EstadoNoGit::Novo;
    let (letra, cor) = match m.estado {
        EstadoNoGit::Novo => ("A", Cores::verde()),
        EstadoNoGit::Apagado => ("D", Cores::vermelho()),
        EstadoNoGit::Renomeado => ("R", Cores::tenue()),
        EstadoNoGit::Mudado => ("M", Cores::ambar()),
    };
    let dica = SharedString::from(m.caminho.clone());
    let (raiz_a, caminho_a) = (raiz.to_path_buf(), m.caminho.clone());
    let (raiz_d, caminho_d) = (raiz.to_path_buf(), m.caminho.clone());
    let desfazer = (!novo && m.estado != EstadoNoGit::Renomeado).then(|| {
        div().flex_none().invisible().group_hover(grupo.clone(), |s| s.visible()).child(
            botao_icone(format!("git-desfazer-{i}"), Lucide::RotateCcw, "Desfazer: volta ao último commit", TamanhoBotao::Linha, window, cx)
                .size(r(20.))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.desfazer_no_git(raiz_d.clone(), caminho_d.clone(), window, cx);
                })),
        )
    });
    linha_da_direita(SharedString::from(format!("git-linha-{i}")))
        .group(grupo)
        .gap(esp::E6)
        .when(!apagado, |s| s.cursor_pointer().hover(|s| s.bg(Cores::hover())))
        .when(apagado, |s| s.text_color(Cores::tenue()))
        .child(simbolo_da_linha(super::componentes::icone(icone, 15., window)).text_color(Cores::tenue()))
        .child(mono_da_linha(nome).flex_shrink(1.))
        .child(mais_e_menos(m.entram as usize, m.saem as usize))
        .child(div().flex_none().w(r(10.)).font_family(FONTE_MONO).text_size(r(10.5)).font_weight(FontWeight::SEMIBOLD).text_color(cor).child(letra))
        .children(desfazer)
        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        .when(!apagado, |s| {
            s.on_click(cx.listener(move |this, _, window, cx| {
                this.abrir_diff_do_git(raiz_a.clone(), caminho_a.clone(), novo, window, cx)
            }))
        })
        .into_any_element()
}
