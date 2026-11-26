//! Comentários por linha no diff (dono, 24/11/2026; ideia do Paseo, `review/store.ts`): na edição
//! do agente, clicar na margem de uma linha abre um campo de comentário; os comentários se
//! acumulam, aparecem marcados nas linhas (clicar de novo edita ou apaga) e o botão "Enviar
//! comentários (N)" manda todos ao agente do chat numa mensagem só, com o arquivo e a linha de
//! cada um. A mensagem entra no turno ou espera na fila, como o Enter da caixa (`Config.enter_na_fila`).

use std::collections::HashMap;

use gpui_kit::component::{
    h_flex,
    input::{Escape, Input, InputEvent, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Flutuante as _, TamanhoBotao, botao_fantasma, botao_icone},
};
use crate::tema::{Cores, Lucide, Tipo as _, esp, raio};

/// Um comentário numa linha do diff. A linha é a do arquivo de agora (1 é a primeira); numa linha
/// que saiu (`removida`) não há número, e o `trecho` (o texto dela) diz qual é.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Comentario {
    pub caminho: String,
    pub linha: Option<usize>,
    pub removida: bool,
    pub trecho: String,
    pub texto: String,
}

impl Comentario {
    /// Onde o comentário está: o mesmo lugar é o mesmo comentário (o clique de novo o edita).
    fn no_mesmo_lugar(&self, caminho: &str, linha: Option<usize>, removida: bool, trecho: &str) -> bool {
        self.caminho == caminho && self.linha == linha && self.removida == removida && (!removida || self.trecho == trecho)
    }
}

/// Os comentários pendentes de um chat.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Lista(pub Vec<Comentario>);

impl Lista {
    pub fn do_lugar(&self, caminho: &str, linha: Option<usize>, removida: bool, trecho: &str) -> Option<&Comentario> {
        self.0.iter().find(|c| c.no_mesmo_lugar(caminho, linha, removida, trecho))
    }

    /// Guarda o comentário; no mesmo lugar troca o texto, e com o texto vazio apaga.
    pub fn guardar(&mut self, novo: Comentario) {
        let lugar = self.0.iter().position(|c| c.no_mesmo_lugar(&novo.caminho, novo.linha, novo.removida, &novo.trecho));
        match (lugar, novo.texto.trim().is_empty()) {
            (Some(i), true) => {
                self.0.remove(i);
            }
            (Some(i), false) => self.0[i].texto = novo.texto.trim().to_string(),
            (None, true) => {}
            (None, false) => self.0.push(Comentario { texto: novo.texto.trim().to_string(), ..novo }),
        }
    }

    pub fn apagar(&mut self, caminho: &str, linha: Option<usize>, removida: bool, trecho: &str) {
        self.0.retain(|c| !c.no_mesmo_lugar(caminho, linha, removida, trecho));
    }
}

/// A mensagem que vai ao agente: um bloco por comentário, com o arquivo, a linha e o trecho dela.
/// Os arquivos saem na ordem em que apareceram, e as linhas de cada um em ordem; as que saíram
/// ficam depois das que existem.
pub fn mensagem(comentarios: &[Comentario]) -> String {
    let mut arquivos: Vec<&str> = Vec::new();
    for c in comentarios {
        if !arquivos.contains(&c.caminho.as_str()) {
            arquivos.push(&c.caminho);
        }
    }
    let n = comentarios.len();
    let mut saida = format!(
        "Revisei o diff e deixei {n} {}:\n",
        if n == 1 { "comentário" } else { "comentários" }
    );
    for arquivo in arquivos {
        let mut do_arquivo: Vec<&Comentario> = comentarios.iter().filter(|c| c.caminho == arquivo).collect();
        do_arquivo.sort_by_key(|c| (c.removida, c.linha));
        for c in do_arquivo {
            let lugar = match (c.removida, c.linha) {
                (true, _) => format!("`{}`, linha que saiu", c.caminho),
                (false, Some(l)) => format!("`{}` linha {l}", c.caminho),
                (false, None) => format!("`{}`", c.caminho),
            };
            saida.push_str(&format!("\n{lugar}\n"));
            let trecho = c.trecho.trim();
            if !trecho.is_empty() {
                saida.push_str(&format!("> {trecho}\n"));
            }
            saida.push_str(&c.texto);
            saida.push('\n');
        }
    }
    saida.trim_end().to_string()
}

/// A linha do diff que o clique na margem escolheu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Alvo {
    pub caminho: String,
    pub linha: Option<usize>,
    pub removida: bool,
    pub trecho: String,
}

/// O estado dos comentários na janela: os pendentes de cada chat, o campo e a linha em que ele está.
pub(super) struct Estado {
    pub por_chat: HashMap<String, Lista>,
    pub aberto: Option<(String, Alvo)>,
    pub campo: Entity<InputState>,
}

impl Estado {
    pub(super) fn novo(campo: Entity<InputState>) -> Self {
        Self { por_chat: HashMap::new(), aberto: None, campo }
    }

    pub(super) fn do_chat(&self, chat: &str) -> Vec<Comentario> {
        self.por_chat.get(chat).map(|l| l.0.clone()).unwrap_or_default()
    }
}

/// O campo do comentário: o Enter guarda (Shift+Enter quebra a linha), o Esc fecha.
pub(super) fn campo_novo(window: &mut Window, cx: &mut Context<Janela>) -> (Entity<InputState>, Subscription) {
    let campo = cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Comentário sobre esta linha")));
    let assinatura = cx.subscribe_in(&campo, window, |this, _, evento, window, cx| {
        if let InputEvent::PressEnter { shift: false, .. } = evento {
            this.guardar_comentario(window, cx);
        }
    });
    (campo, assinatura)
}

impl Janela {
    /// O chat dono do diff aberto: o da edição, ou o selecionado.
    pub(super) fn chat_dos_comentarios(&self) -> Option<String> {
        match self.arquivo_edicao.as_ref().map(|e| &e.origem) {
            Some(super::edicao::Origem::Chat { chat, .. }) => Some(chat.clone()),
            _ => self.selecionado.clone(),
        }
    }

    /// O clique na margem de uma linha: abre o campo (com o texto de antes, se a linha já tem
    /// comentário).
    pub(super) fn abrir_comentario(&mut self, alvo: Alvo, window: &mut Window, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_dos_comentarios() else { return };
        let antes = self
            .comentarios_diff
            .por_chat
            .get(&chat)
            .and_then(|l| l.do_lugar(&alvo.caminho, alvo.linha, alvo.removida, &alvo.trecho))
            .map(|c| c.texto.clone())
            .unwrap_or_default();
        let campo = self.comentarios_diff.campo.clone();
        campo.update(cx, |c, cx| {
            c.set_value(antes, window, cx);
            let fim = c.value().len();
            c.set_selected_range(fim..fim, cx);
        });
        self.comentarios_diff.aberto = Some((chat, alvo));
        let foco = campo.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    pub(super) fn fechar_comentario(&mut self, cx: &mut Context<Self>) {
        if self.comentarios_diff.aberto.take().is_some() {
            cx.notify();
        }
    }

    /// O Enter no campo: guarda o comentário (vazio apaga o que já havia) e fecha.
    pub(super) fn guardar_comentario(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((chat, alvo)) = self.comentarios_diff.aberto.take() else { return };
        let texto = self.comentarios_diff.campo.read(cx).value().to_string();
        self.comentarios_diff.por_chat.entry(chat).or_default().guardar(Comentario {
            caminho: alvo.caminho,
            linha: alvo.linha,
            removida: alvo.removida,
            trecho: alvo.trecho,
            texto,
        });
        self.comentarios_diff.campo.update(cx, |c, cx| c.set_value("", window, cx));
        cx.notify();
    }

    fn apagar_comentario(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some((chat, alvo)) = self.comentarios_diff.aberto.take() else { return };
        if let Some(lista) = self.comentarios_diff.por_chat.get_mut(&chat) {
            lista.apagar(&alvo.caminho, alvo.linha, alvo.removida, &alvo.trecho);
        }
        self.comentarios_diff.campo.update(cx, |c, cx| c.set_value("", window, cx));
        cx.notify();
    }

    /// "Enviar comentários (N)": todos numa mensagem só ao agente do chat. Com o turno rodando, ela
    /// entra nele ou espera na fila, como o Enter da caixa (`enter_manda_depois`).
    fn enviar_comentarios(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(chat), Some(motor)) = (self.chat_dos_comentarios(), self.motor.clone()) else { return };
        let Some(lista) = self.comentarios_diff.por_chat.get(&chat).filter(|l| !l.0.is_empty()) else { return };
        let texto = mensagem(&lista.0);
        let depois = self.paineis.get(self.ativo).is_some_and(|p| self.enter_manda_depois(p.id, false, cx));
        let reforco = crate::modelo::Reforco { depois, ..Default::default() };
        motor.update(cx, |m, cx| m.enviar_do_usuario(&chat, texto, Vec::new(), reforco, cx));
        self.comentarios_diff.por_chat.remove(&chat);
        self.comentarios_diff.aberto = None;
        self.comentarios_diff.campo.update(cx, |c, cx| c.set_value("", window, cx));
        cx.notify();
    }

    /// A barra de baixo do painel da edição: o campo da linha escolhida e, com comentários
    /// pendentes, o botão que os envia. Sem nenhum dos dois, nada aparece.
    pub(super) fn barra_de_comentarios(&self, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let chat = self.chat_dos_comentarios()?;
        let pendentes = self.comentarios_diff.por_chat.get(&chat).map_or(0, |l| l.0.len());
        let aberto = self.comentarios_diff.aberto.as_ref().filter(|(c, _)| *c == chat).map(|(_, a)| a.clone());
        if pendentes == 0 && aberto.is_none() {
            return None;
        }
        let editando = aberto.as_ref().is_some_and(|a| {
            self.comentarios_diff.por_chat.get(&chat).is_some_and(|l| l.do_lugar(&a.caminho, a.linha, a.removida, &a.trecho).is_some())
        });
        let campo_aberto = aberto.map(|alvo| {
            let onde = match (alvo.removida, alvo.linha) {
                (true, _) => tr!("{caminho}, linha que saiu", caminho = alvo.caminho).to_string(),
                (false, Some(l)) => format!("{}:{l}", alvo.caminho),
                (false, None) => alvo.caminho.clone(),
            };
            v_flex()
                .gap(esp::E4)
                .on_action(cx.listener(|this, _: &Escape, _, cx| this.fechar_comentario(cx)))
                .child(div().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(onde))
                .child(
                    h_flex()
                        .gap(esp::E4)
                        .items_end()
                        .child(div().flex_1().min_w_0().child(Input::new(&self.comentarios_diff.campo)))
                        .child(
                            botao_fantasma("comentario-guardar", None, tr!("Guardar"), TamanhoBotao::Barra, window, cx)
                                .on_click(cx.listener(|this, _, window, cx| this.guardar_comentario(window, cx))),
                        )
                        .when(editando, |s| {
                            s.child(
                                botao_icone("comentario-apagar", Lucide::Trash, tr!("Apagar este comentário"), TamanhoBotao::Barra, window, cx)
                                    .on_click(cx.listener(|this, _, window, cx| this.apagar_comentario(window, cx))),
                            )
                        })
                        .child(
                            botao_icone("comentario-fechar", Lucide::X, tr!("Fechar (Esc)"), TamanhoBotao::Barra, window, cx)
                                .on_click(cx.listener(|this, _, _, cx| this.fechar_comentario(cx))),
                        ),
                )
        });
        let enviar = (pendentes > 0).then(|| {
            h_flex().justify_end().child(
                botao_fantasma(
                    "comentarios-enviar",
                    Some(Lucide::Send.into()),
                    tr!("Enviar comentários ({pendentes})", pendentes = pendentes),
                    TamanhoBotao::Barra,
                    window,
                    cx,
                )
                .on_click(cx.listener(|this, _, window, cx| this.enviar_comentarios(window, cx))),
            )
        });
        Some(
            v_flex()
                .flex_none()
                .gap(esp::E4)
                .p(esp::E8)
                .rounded(raio::PECA)
                .flutuante(cx)
                .children(campo_aberto)
                .children(enviar)
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod testes {
    use super::{Comentario, Lista, mensagem};

    fn c(caminho: &str, linha: Option<usize>, removida: bool, trecho: &str, texto: &str) -> Comentario {
        Comentario { caminho: caminho.into(), linha, removida, trecho: trecho.into(), texto: texto.into() }
    }

    #[test]
    fn os_comentarios_se_acumulam_e_o_mesmo_lugar_troca_o_texto() {
        let mut lista = Lista::default();
        lista.guardar(c("a.rs", Some(3), false, "let x = 1;", "por que 1?"));
        lista.guardar(c("a.rs", Some(9), false, "fim", "tirar"));
        assert_eq!(lista.0.len(), 2);
        // De novo na linha 3: troca o texto, sem duplicar.
        lista.guardar(c("a.rs", Some(3), false, "let x = 1;", "  use const  "));
        assert_eq!(lista.0.len(), 2);
        assert_eq!(lista.do_lugar("a.rs", Some(3), false, "").map(|c| c.texto.as_str()), Some("use const"));
        // Vazio apaga; apagar tira só o do lugar.
        lista.guardar(c("a.rs", Some(9), false, "fim", "   "));
        assert_eq!(lista.0.len(), 1);
        lista.apagar("a.rs", Some(3), false, "");
        assert!(lista.0.is_empty());
    }

    /// Uma linha que saiu se acha pelo texto dela, e não se confunde com a que existe.
    #[test]
    fn a_linha_que_saiu_e_achada_pelo_trecho() {
        let mut lista = Lista::default();
        lista.guardar(c("a.rs", None, true, "velha()", "voltou?"));
        lista.guardar(c("a.rs", None, true, "outra()", "e esta?"));
        lista.guardar(c("a.rs", Some(2), false, "velha()", "esta existe"));
        assert_eq!(lista.0.len(), 3);
        assert_eq!(lista.do_lugar("a.rs", None, true, "outra()").map(|c| c.texto.as_str()), Some("e esta?"));
    }

    #[test]
    fn a_mensagem_diz_arquivo_linha_e_trecho_de_cada_um() {
        let todos = [
            c("src/b.rs", Some(40), false, "fn b() {}", "renomear"),
            c("src/a.rs", Some(30), false, "let z = 3;", "e se for zero?"),
            c("src/a.rs", Some(12), false, "let a = 1;", "por que não const?"),
            c("src/a.rs", None, true, "old();", "isso voltou?"),
        ];
        assert_eq!(
            mensagem(&todos),
            "Revisei o diff e deixei 4 comentários:\n\n\
             `src/b.rs` linha 40\n> fn b() {}\nrenomear\n\n\
             `src/a.rs` linha 12\n> let a = 1;\npor que não const?\n\n\
             `src/a.rs` linha 30\n> let z = 3;\ne se for zero?\n\n\
             `src/a.rs`, linha que saiu\n> old();\nisso voltou?"
        );
        assert_eq!(
            mensagem(&[c("x.rs", Some(1), false, "", "ok")]),
            "Revisei o diff e deixei 1 comentário:\n\n`x.rs` linha 1\nok"
        );
    }
}
