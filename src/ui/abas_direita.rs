//! As abas do painel da direita: "Este chat" e o arquivo aberto, no jeito do Linear (25/11/2026):
//! a escolhida com o nome, as outras só com o símbolo e o nome na dica.

use gpui_kit::component::tooltip::{ManagedTooltipExt as _, Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{TamanhoBotao, botao_icone},
    painel_arquivo,
};
use crate::{
    modelo::Chat,
    motor::Motor,
    tema::{Cores, Lucide, esp, r, raio},
};

/// A aba da direita. `Arquivo(i)` é o i-ésimo arquivo aberto (por enquanto, um só).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AbaDireita {
    #[default]
    Chat,
    /// A árvore da pasta de trabalho, com a busca (o item Arquivos da lista abre aqui).
    Arquivos,
    Arquivo(usize),
}

/// Um arquivo aberto numa aba da direita (protótipo Linear, 25/11/2026: vários abertos). O de
/// agora é o `arquivo_aberto` da janela, com o estado nela; os outros guardam o deles aqui.
pub(super) struct AbaDeArquivo {
    pub raiz: std::path::PathBuf,
    pub caminho: String,
    guardado: Option<EstadoDoArquivo>,
}

/// O que volta quando a aba volta: a edição (posta ou guardada pelo "Arquivo"), a prévia ou a
/// fonte, as dobras abertas e a mudança à vista.
struct EstadoDoArquivo {
    edicao: Option<std::rc::Rc<super::edicao::Edicao>>,
    edicao_fora: Option<std::rc::Rc<super::edicao::Edicao>>,
    fonte: bool,
    dobras: std::collections::HashSet<usize>,
    edicao_atual: usize,
}

/// Abaixo desta largura (px a 100%), com mais de uma aba, elas ficam só com o símbolo.
pub const LIMITE_DAS_ABAS: f32 = 520.;

impl Janela {
    /// A aba que a direita mostra agora: o arquivo, se é ele que está em foco e continua aberto (ou
    /// sem chat selecionado); senão a do chat selecionado.
    pub(super) fn aba_visivel(&self, motor: &Motor) -> AbaDireita {
        if self.aba_direita == AbaDireita::Arquivos {
            return AbaDireita::Arquivos;
        }
        if let AbaDireita::Arquivo(i) = self.aba_direita
            && self.arquivo_aberto.is_some()
        {
            return AbaDireita::Arquivo(i);
        }
        match self.selecionado.as_ref().and_then(|id| motor.chat(id)) {
            Some(_) => AbaDireita::Chat,
            None if self.arquivo_aberto.is_some() => AbaDireita::Arquivo(0),
            None => AbaDireita::Chat,
        }
    }

    pub(super) fn escolher_aba(&mut self, aba: AbaDireita, cx: &mut Context<Self>) {
        self.aba_direita = aba;
        cx.notify();
    }

    /// O conteúdo da direita pela aba: o arquivo ou "Este chat".
    pub(super) fn conteudo_da_direita(&mut self, motor: &Entity<Motor>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.aba_visivel(motor.read(cx)) {
            AbaDireita::Arquivo(_) => self.painel_arquivo(motor, window, cx),
            AbaDireita::Arquivos => self.painel_de_arquivos(motor, window, cx),
            AbaDireita::Chat => self.painel_do_chat(motor, window, cx).into_any_element(),
        }
    }

    /// A faixa de cima da direita, no jeito do Linear (25/11/2026), arrastando a janela: as abas
    /// de 24 (a escolhida com o nome no fundo elevado, as outras só com o símbolo e o nome na dica)
    /// e, na ponta, o recolher. O × da janela fica sempre na última conversa.
    pub(super) fn barra_de_abas(
        &self,
        motor: &Motor,
        visivel: AbaDireita,
        rolado: bool,
        id: &'static str,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let _ = rolado;
        let chat = self.selecionado.as_ref().and_then(|id| motor.chat(id));
        let arquivo = self.arquivo_aberto.clone();
        let mut abas: Vec<AnyElement> = Vec::new();
        if let Some(chat) = chat {
            let ativa = visivel == AbaDireita::Chat;
            let dica = self.dica_do_chat(motor, chat);
            let aba = aba_da_direita("aba-este-chat", Lucide::MessageCircle, tr!("Este chat"), ativa, window)
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .when(!ativa, |a| a.on_click(cx.listener(|this, _, _, cx| this.escolher_aba(AbaDireita::Chat, cx))));
            abas.push(super::sem_arrastar().flex().flex_none().child(aba).into_any_element());
        }
        {
            let ativa = visivel == AbaDireita::Arquivos;
            let aba = aba_da_direita("aba-arquivos", Lucide::Folder, tr!("Arquivos"), ativa, window)
                .dica_no_ponteiro(|window, cx| Tooltip::new(tr!("Arquivos da pasta de trabalho")).build(window, cx))
                .when(!ativa, |a| a.on_click(cx.listener(|this, _, window, cx| this.abrir_aba_arquivos(window, cx))));
            abas.push(super::sem_arrastar().flex().flex_none().child(aba).into_any_element());
        }
        for (i, aba) in self.abas_de_arquivo.iter().enumerate() {
            let atual = arquivo.as_ref().is_some_and(|(r, c)| *r == aba.raiz && *c == aba.caminho);
            let ativa = atual && matches!(visivel, AbaDireita::Arquivo(_));
            let nome = aba.caminho.rsplit_once('/').map_or(aba.caminho.clone(), |(_, n)| n.to_string());
            let dica = SharedString::from(aba.raiz.join(&aba.caminho).display().to_string());
            let fechar = botao_icone(("fechar-arquivo", i), Lucide::X, tr!("Fechar o arquivo"), TamanhoBotao::Linha, window, cx)
                .size(r(16.))
                .rounded(raio::de(16.))
                .on_click(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    this.fechar_aba_de_arquivo(i, window, cx);
                }));
            let aba = aba_da_direita_com(("aba-arquivo", i), painel_arquivo::icone_do_arquivo(&aba.caminho), nome.into(), ativa, true, window)
                .when(ativa, |a| a.pr(esp::E6).child(fechar))
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .when(!ativa, |a| a.on_click(cx.listener(move |this, _, window, cx| this.trocar_para_aba_de_arquivo(i, window, cx))))
                // O clique do meio fecha, como nas abas das conversas.
                .on_mouse_up(MouseButton::Middle, cx.listener(move |this, _, window, cx| this.fechar_aba_de_arquivo(i, window, cx)));
            abas.push(super::sem_arrastar().flex().when(ativa, |d| d.min_w_0()).when(!ativa, |d| d.flex_none()).child(aba).into_any_element());
        }
        // 8 em cima e 6 dos lados, a fileira de 26 e 2 até o conteúdo (protótipo).
        self.arrastavel(id)
            .flex_none()
            .flex()
            .items_center()
            .h(r(36.))
            .pt(esp::E8)
            .pb(esp::E2)
            .px(esp::E6)
            .gap(esp::E4)
            .child(div().flex().flex_1().min_w_0().items_center().gap(esp::E2).children(abas))
            .child(self.botoes_da_direita(false, window, cx))
    }

    /// Fecha o arquivo de agora (a aba dele); a direita vai à aba vizinha, ou à do chat.
    pub(super) fn fechar_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let atual = self.indice_do_arquivo_atual();
        match atual {
            Some(i) => self.fechar_aba_de_arquivo(i, window, cx),
            None => self.limpar_arquivo(window, cx),
        }
    }

    /// O arquivo de agora entre as abas.
    fn indice_do_arquivo_atual(&self) -> Option<usize> {
        let (raiz, caminho) = self.arquivo_aberto.as_ref()?;
        self.abas_de_arquivo.iter().position(|a| a.raiz == *raiz && a.caminho == *caminho)
    }

    /// Sem arquivo à vista: o editor grava o que falta e a direita volta à aba do chat.
    fn limpar_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.largar_editor(window, cx);
        self.arquivo_fonte = false;
        self.arquivo_aberto = None;
        self.arquivo_edicao = None;
        self.edicao_fora = None;
        self.arquivo_selecao = None;
        self.busca_arquivo.aberta = false;
        if matches!(self.aba_direita, AbaDireita::Arquivo(_)) {
            self.aba_direita = AbaDireita::Chat;
        }
        cx.notify();
    }

    /// Fecha a aba `i`. A de agora passa para a vizinha (a da direita, ou a da esquerda).
    pub(super) fn fechar_aba_de_arquivo(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        if i >= self.abas_de_arquivo.len() {
            return;
        }
        let era_a_atual = self.indice_do_arquivo_atual() == Some(i);
        self.abas_de_arquivo.remove(i);
        if !era_a_atual {
            cx.notify();
            return;
        }
        if self.abas_de_arquivo.is_empty() {
            self.limpar_arquivo(window, cx);
            return;
        }
        // O estado do arquivo que fechou não vai para a aba nenhuma.
        self.arquivo_aberto = None;
        self.trocar_para_aba_de_arquivo(i.min(self.abas_de_arquivo.len() - 1), window, cx);
    }

    /// Guarda o estado do arquivo de agora na aba dele, antes de outro arquivo tomar o lugar.
    pub(super) fn guardar_aba_de_arquivo(&mut self) {
        let Some(i) = self.indice_do_arquivo_atual() else {
            return;
        };
        self.abas_de_arquivo[i].guardado = Some(EstadoDoArquivo {
            edicao: self.arquivo_edicao.clone(),
            edicao_fora: self.edicao_fora.clone(),
            fonte: self.arquivo_fonte,
            dobras: self.dobras_abertas.borrow().clone(),
            edicao_atual: self.edicao_atual,
        });
    }

    /// Põe o arquivo na lista das abas (depois do de agora), se ainda não está.
    pub(super) fn aba_para_o_arquivo(&mut self, raiz: &std::path::Path, caminho: &str) {
        if self.abas_de_arquivo.iter().any(|a| a.raiz == raiz && a.caminho == caminho) {
            return;
        }
        let depois = self.indice_do_arquivo_atual().map_or(self.abas_de_arquivo.len(), |i| i + 1);
        self.abas_de_arquivo.insert(depois, AbaDeArquivo { raiz: raiz.to_path_buf(), caminho: caminho.to_string(), guardado: None });
    }

    /// Vai à aba de arquivo `i`, com o estado que ela guardou.
    pub(super) fn trocar_para_aba_de_arquivo(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(aba) = self.abas_de_arquivo.get(i) else {
            return;
        };
        let (raiz, caminho) = (aba.raiz.clone(), aba.caminho.clone());
        if self.arquivo_aberto.as_ref().is_some_and(|(r, c)| *r == raiz && *c == caminho) {
            self.escolher_aba(AbaDireita::Arquivo(0), cx);
            return;
        }
        self.edicao_pedida = None;
        self.abrir_arquivo(raiz, caminho, window, cx);
        if let Some(estado) = self.abas_de_arquivo.get_mut(i).and_then(|a| a.guardado.take()) {
            self.arquivo_edicao = estado.edicao;
            self.edicao_fora = estado.edicao_fora;
            self.arquivo_fonte = estado.fonte;
            *self.dobras_abertas.borrow_mut() = estado.dobras;
            self.edicao_atual = estado.edicao_atual;
            self.edicao_rolar.set(self.arquivo_edicao.is_some());
            self.esquecer_vista();
        }
        cx.notify();
    }

    /// A dica da aba "Este chat": a pasta e o painel do chat selecionado.
    fn dica_do_chat(&self, motor: &Motor, chat: &Chat) -> SharedString {
        let pasta = if motor.sem_pasta(chat) { tr!("sem pasta").to_string() } else { super::dialogos::com_til(&chat.cwd) };
        let mut dica = tr!("Este chat · {pasta}", pasta = pasta).to_string();
        if let Some((n, _)) = self.painel_da_conversa(&chat.id) {
            dica.push_str(&tr!(" · painel {n}", n = n));
        }
        dica.into()
    }
}

/// Uma aba da direita: 24 de altura; a escolhida com o nome em 12,5 no fundo elevado, as outras
/// só com o símbolo de 14 (protótipo Linear, 25/11/2026).
fn aba_da_direita(id: &'static str, simbolo: Lucide, nome: SharedString, ativa: bool, window: &Window) -> Stateful<Div> {
    aba_da_direita_com(id, simbolo, nome, ativa, false, window)
}

/// A aba da direita; a de arquivo leva o símbolo também quando escolhida (e o nome em 12).
fn aba_da_direita_com(
    id: impl Into<ElementId>,
    simbolo: Lucide,
    nome: SharedString,
    ativa: bool,
    simbolo_na_ativa: bool,
    window: &Window,
) -> Stateful<Div> {
    div()
        .id(id)
        .flex()
        .flex_none()
        .items_center()
        .min_w_0()
        .h(r(24.))
        .rounded(raio::de(24.))
        .text_size(r(12.5))
        .font_weight(FontWeight::SEMIBOLD)
        .map(|d| {
            if ativa {
                d.flex_shrink(1.)
                    .px(esp::E8)
                    .gap(esp::E6)
                    .bg(Cores::cartao_elevado())
                    .text_color(Cores::texto())
                    .when(simbolo_na_ativa, |d| d.child(div().flex_none().child(super::componentes::icone(simbolo, 14., window))))
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .when(simbolo_na_ativa, |d| d.text_size(r(12.)).font_weight(FontWeight::MEDIUM))
                            .child(nome),
                    )
            } else {
                d.w(r(24.))
                    .justify_center()
                    .text_color(Cores::tenue())
                    .cursor_pointer()
                    .hover(|s| s.bg(Cores::hover()).text_color(Cores::texto()))
                    .child(super::componentes::icone(simbolo, 14., window))
            }
        })
}
