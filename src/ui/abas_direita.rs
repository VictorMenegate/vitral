//! As abas do painel da direita (seção 4.1 da especificação): "Este chat" e o arquivo aberto.
//! Abaixo de 520 px, com mais de uma aba, elas viram só o símbolo, com o nome na dica.

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Aba, Simbolo, TomAba},
    painel_arquivo,
};
use crate::{
    modelo::Chat,
    motor::Motor,
    tema::{Cores, Lucide, altura, esp},
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

    /// A faixa de cima da direita (44, arrasta a janela): as abas e o canto. Com uma aba só, ela fica
    /// como antes (a única, sobre a linha); com mais, a de agora fica em destaque e um clique troca.
    pub(super) fn barra_de_abas(
        &self,
        motor: &Motor,
        visivel: AbaDireita,
        rolado: bool,
        id: &'static str,
        window: &Window,
        cx: &Context<Self>,
    ) -> Stateful<Div> {
        let chat = self.selecionado.as_ref().and_then(|id| motor.chat(id));
        let arquivo = self.arquivo_aberto.clone();
        let n = usize::from(chat.is_some()) + usize::from(arquivo.is_some());
        let compacta = n > 1 && self.direita_compacta;
        let tom = |ativa: bool| match () {
            _ if n <= 1 => TomAba::Unica,
            _ if ativa => TomAba::Selecionada,
            _ => TomAba::Normal,
        };
        let dica = |e: Stateful<Div>, d: Option<SharedString>| match d {
            Some(d) => e.tooltip(move |window, cx| Tooltip::new(d.clone()).build(window, cx)),
            None => e,
        };
        let mut abas: Vec<AnyElement> = Vec::new();
        if let Some(chat) = chat {
            let ativa = visivel == AbaDireita::Chat;
            let (aba, d) = self.aba_do_chat(motor, chat);
            let mut aba = aba.tom(tom(ativa)).so_simbolo(compacta).construir(window, cx);
            if n > 1 && !ativa {
                aba = aba.cursor_pointer().on_click(cx.listener(|this, _, _, cx| this.escolher_aba(AbaDireita::Chat, cx)));
            }
            abas.push(super::sem_arrastar().flex().min_w_0().child(dica(aba, d)).into_any_element());
        }
        if let Some((raiz, caminho)) = arquivo {
            let ativa = matches!(visivel, AbaDireita::Arquivo(_));
            let nome = caminho.rsplit_once('/').map_or(caminho.clone(), |(_, n)| n.to_string());
            let janela = cx.entity().downgrade();
            let mut aba = Aba::new("aba-arquivo", nome)
                .tom(tom(ativa))
                .so_simbolo(compacta)
                .simbolo(Simbolo::icone_cor(painel_arquivo::icone_do_arquivo(&caminho), Cores::tenue()))
                .fechar("Fechar o arquivo", !compacta, move |_, cx| {
                    let _ = janela.update(cx, |this, cx| this.fechar_arquivo(cx));
                })
                .construir(window, cx);
            if n > 1 && !ativa {
                aba = aba.cursor_pointer().on_click(cx.listener(|this, _, _, cx| this.escolher_aba(AbaDireita::Arquivo(0), cx)));
            }
            let caminho = SharedString::from(raiz.join(&caminho).display().to_string());
            abas.push(super::sem_arrastar().flex().min_w_0().child(dica(aba, Some(caminho))).into_any_element());
        }
        self.arrastavel(id)
            .flex_none()
            .h(altura::CABECALHO)
            .px(esp::E8)
            .flex()
            .items_center()
            .gap(esp::E4)
            .border_b_1()
            .border_color(if rolado { Cores::traco() } else { gpui_kit::transparent_black().into() })
            .children(abas)
            .child(div().flex_1().min_w(esp::E8))
            .child(self.botoes_da_direita(false, window, cx))
    }

    /// Fecha o arquivo da direita; a aba volta à do chat.
    pub(super) fn fechar_arquivo(&mut self, cx: &mut Context<Self>) {
        self.arquivo_aberto = None;
        if matches!(self.aba_direita, AbaDireita::Arquivo(_)) {
            self.aba_direita = AbaDireita::Chat;
        }
        cx.notify();
    }

    /// A aba "Este chat" do chat selecionado: o nome e o símbolo, o número do painel e a dica (a
    /// pasta).
    fn aba_do_chat(&self, motor: &Motor, chat: &Chat) -> (Aba, Option<SharedString>) {
        let numero = self.painel_da_conversa(&chat.id);
        let mut aba = Aba::new("aba-este-chat", "Este chat").simbolo(Simbolo::icone_cor(Lucide::Info, Cores::tenue()));
        if let Some((n, ativo)) = numero {
            aba = aba.numero(n, ativo);
        }
        let pasta = if motor.sem_pasta(chat) { "sem pasta".to_string() } else { super::dialogos::com_til(&chat.cwd) };
        let mut dica = format!("Onde “{}” trabalha · {pasta}", chat.titulo);
        if let Some((n, _)) = numero {
            dica.push_str(&format!(" · painel {n}"));
        }
        (aba, Some(dica.into()))
    }
}
