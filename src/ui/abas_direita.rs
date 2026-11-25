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
    Arquivo(usize),
}

/// Abaixo desta largura (px a 100%), com mais de uma aba, elas ficam só com o símbolo.
pub const LIMITE_DAS_ABAS: f32 = 520.;

impl Janela {
    /// A aba que a direita mostra agora: o arquivo, se é ele que está em foco e continua aberto (ou
    /// sem chat selecionado); senão a do chat selecionado.
    pub(super) fn aba_visivel(&self, motor: &Motor) -> AbaDireita {
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
            let aba = aba_da_direita("aba-este-chat", Lucide::MessageCircle, "Este chat".into(), ativa, window)
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .when(!ativa, |a| a.on_click(cx.listener(|this, _, _, cx| this.escolher_aba(AbaDireita::Chat, cx))));
            abas.push(super::sem_arrastar().flex().flex_none().child(aba).into_any_element());
        }
        if let Some((raiz, caminho)) = arquivo {
            let ativa = matches!(visivel, AbaDireita::Arquivo(_));
            let nome = caminho.rsplit_once('/').map_or(caminho.clone(), |(_, n)| n.to_string());
            let dica = SharedString::from(raiz.join(&caminho).display().to_string());
            let janela = cx.entity().downgrade();
            let fechar = botao_icone("fechar-arquivo", Lucide::X, "Fechar o arquivo", TamanhoBotao::Linha, window, cx)
                .size(r(18.))
                .on_click(move |_, window, cx| {
                    cx.stop_propagation();
                    let _ = janela.update(cx, |this, cx| this.fechar_arquivo(window, cx));
                });
            let aba = aba_da_direita("aba-arquivo", painel_arquivo::icone_do_arquivo(&caminho), nome.into(), ativa, window)
                .when(ativa, |a| a.pr(esp::E2).child(fechar))
                .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .when(!ativa, |a| a.on_click(cx.listener(|this, _, _, cx| this.escolher_aba(AbaDireita::Arquivo(0), cx))));
            abas.push(super::sem_arrastar().flex().min_w_0().child(aba).into_any_element());
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

    /// Fecha o arquivo da direita; a aba volta à do chat. O editor grava o que falta antes de sair.
    pub(super) fn fechar_arquivo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.largar_editor(window, cx);
        self.arquivo_fonte = false;
        self.arquivo_aberto = None;
        self.arquivo_edicao = None;
        self.arquivo_selecao = None;
        self.busca_arquivo.aberta = false;
        if matches!(self.aba_direita, AbaDireita::Arquivo(_)) {
            self.aba_direita = AbaDireita::Chat;
        }
        cx.notify();
    }

    /// A dica da aba "Este chat": a pasta e o painel do chat selecionado.
    fn dica_do_chat(&self, motor: &Motor, chat: &Chat) -> SharedString {
        let pasta = if motor.sem_pasta(chat) { "sem pasta".to_string() } else { super::dialogos::com_til(&chat.cwd) };
        let mut dica = format!("Este chat · {pasta}");
        if let Some((n, _)) = self.painel_da_conversa(&chat.id) {
            dica.push_str(&format!(" · painel {n}"));
        }
        dica.into()
    }
}

/// Uma aba da direita: 24 de altura; a escolhida com o nome em 12,5 no fundo elevado, as outras
/// só com o símbolo de 14 (protótipo Linear, 25/11/2026).
fn aba_da_direita(id: &'static str, simbolo: Lucide, nome: SharedString, ativa: bool, window: &Window) -> Stateful<Div> {
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
                d.flex_shrink(1.).px(esp::E8).gap(esp::E6).bg(Cores::cartao_elevado()).text_color(Cores::texto()).child(div().min_w_0().truncate().child(nome))
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
