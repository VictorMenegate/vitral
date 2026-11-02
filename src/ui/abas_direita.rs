//! As abas do painel da direita (seção 4.1 da especificação): "Este chat" (o "Distribuído" no
//! Mestre, a equipe numa conversa antiga), "PR #12" só quando o chat tem PR, e o arquivo aberto.
//! A aba de cada chat fica lembrada no `interface.json` (`abas_por_chat`); abaixo de 520 px, com
//! mais de uma aba, elas viram só o símbolo, com o nome na dica.

use std::collections::HashMap;

use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::{
    Janela,
    componentes::{Aba, Simbolo, TomAba},
    gravar_interface, painel_arquivo, painel_pr,
};
use crate::{
    modelo::{Chat, Papel, Tarefa},
    motor::Motor,
    tema::{Cores, Lucide, altura, esp},
};

/// A aba da direita. `Arquivo(i)` é o i-ésimo arquivo aberto (por enquanto, um só).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AbaDireita {
    #[default]
    Chat,
    Pr,
    Arquivo(usize),
}

/// Abaixo desta largura (px a 100%), com mais de uma aba, elas ficam só com o símbolo.
pub const LIMITE_DAS_ABAS: f32 = 520.;

/// O chat tem a aba PR: um chat de projeto (não o Mestre) cuja tarefa tem PR ou PRs anteriores.
pub fn tem_aba_pr(chat: &Chat, tarefa: Option<&Tarefa>) -> bool {
    chat.papel != Papel::Mestre && tarefa.is_some_and(|t| t.pr.is_some() || !t.prs_anteriores.is_empty())
}

/// A aba com que o chat abre: a lembrada, se ainda existe; sem nada lembrado, a do PR quando ele
/// está aberto (a tarefa em `NoPr`), senão "Este chat".
pub fn aba_inicial(chat: &Chat, tarefa: Option<&Tarefa>, lembrada: Option<AbaDireita>) -> AbaDireita {
    let pr = tem_aba_pr(chat, tarefa);
    match lembrada {
        Some(AbaDireita::Pr) if pr => AbaDireita::Pr,
        Some(AbaDireita::Pr | AbaDireita::Chat) => AbaDireita::Chat,
        _ if pr && tarefa.is_some_and(|t| t.estado == crate::modelo::EstadoTarefa::NoPr) => AbaDireita::Pr,
        _ => AbaDireita::Chat,
    }
}

/// O que ficou lembrado no `interface.json`: a aba de cada chat (só "Este chat" e o PR; o
/// arquivo não sobrevive a reabrir).
pub fn ler_abas() -> HashMap<String, AbaDireita> {
    super::zoom::interface()
        .and_then(|i| i.get("abas_por_chat").cloned())
        .and_then(|v| serde_json::from_value::<HashMap<String, AbaDireita>>(v).ok())
        .map(|mapa| mapa.into_iter().filter(|(_, a)| !matches!(a, AbaDireita::Arquivo(_))).collect())
        .unwrap_or_default()
}

impl Janela {
    /// A aba que a direita mostra agora: o arquivo, se é ele que está em foco e continua aberto;
    /// senão a do chat selecionado.
    pub(super) fn aba_visivel(&self, motor: &Motor) -> AbaDireita {
        if let AbaDireita::Arquivo(i) = self.aba_direita
            && self.arquivo_aberto.is_some()
        {
            return AbaDireita::Arquivo(i);
        }
        match self.selecionado.as_ref().and_then(|id| motor.chat(id)) {
            Some(chat) => {
                let tarefa = chat.tarefa.and_then(|t| motor.tarefa(t));
                aba_inicial(chat, tarefa, self.abas_por_chat.get(&chat.id).copied())
            }
            None if self.arquivo_aberto.is_some() => AbaDireita::Arquivo(0),
            None => AbaDireita::Chat,
        }
    }

    /// Escolhe a aba; a do chat fica lembrada para ele.
    pub(super) fn escolher_aba(&mut self, aba: AbaDireita, cx: &mut Context<Self>) {
        self.aba_direita = aba;
        if !matches!(aba, AbaDireita::Arquivo(_))
            && let Some(chat) = self.selecionado.clone()
            && self.abas_por_chat.get(&chat) != Some(&aba)
        {
            self.abas_por_chat.insert(chat, aba);
            if let Ok(valor) = serde_json::to_value(&self.abas_por_chat) {
                gravar_interface("abas_por_chat", valor, cx);
            }
        }
        cx.notify();
    }

    /// O chip do PR (no cabeçalho da conversa, no Distribuído, na linha de acompanhamento): abre o
    /// chat da tarefa na aba PR, com a direita à vista.
    pub(super) fn abrir_aba_pr(&mut self, chat: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.selecionado.as_deref() != Some(chat) {
            self.selecionar(Some(chat.to_string()), window, cx);
        }
        self.escolher_aba(AbaDireita::Pr, cx);
        self.mostrar_direita(window, cx);
    }

    /// O conteúdo da direita pela aba: o arquivo, o PR ou "Este chat".
    pub(super) fn conteudo_da_direita(&mut self, motor: &Entity<Motor>, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        match self.aba_visivel(motor.read(cx)) {
            AbaDireita::Arquivo(_) => self.painel_arquivo(motor, window, cx),
            AbaDireita::Pr => self.painel_do_pr(motor, window, cx),
            AbaDireita::Chat => self.equipe(motor, window, cx).into_any_element(),
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
        let tarefa = chat.and_then(|c| c.tarefa).and_then(|t| motor.tarefa(t));
        let com_pr = chat.is_some_and(|c| tem_aba_pr(c, tarefa));
        let arquivo = self.arquivo_aberto.clone();
        let n = usize::from(chat.is_some()) + usize::from(com_pr) + usize::from(arquivo.is_some());
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
        if let Some(t) = tarefa.filter(|_| com_pr) {
            let ativa = visivel == AbaDireita::Pr;
            let (aba, d) = painel_pr::aba_do_pr(t, compacta);
            let mut aba = aba.tom(tom(ativa)).construir(window, cx);
            if !ativa {
                aba = aba.cursor_pointer().on_click(cx.listener(|this, _, _, cx| this.escolher_aba(AbaDireita::Pr, cx)));
            }
            abas.push(super::sem_arrastar().flex().min_w_0().child(dica(aba, Some(d))).into_any_element());
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
            .border_color(if rolado { Cores::borda() } else { gpui_kit::transparent_black().into() })
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

    /// A aba "Este chat" do chat selecionado: o nome e o símbolo pelo papel, o número do painel e
    /// a dica (o projeto e de quem é a equipe).
    fn aba_do_chat(&self, motor: &Motor, chat: &Chat) -> (Aba, Option<SharedString>) {
        let coordenador = super::equipe::coordenador_da_equipe(motor, chat);
        let titulo_coordenador = coordenador.as_deref().and_then(|c| motor.chat(c)).map(|c| c.titulo.clone());
        let numero = self.painel_da_conversa(&chat.id);
        let (titulo, icone) = match chat.papel {
            Papel::Mestre => ("Distribuído", Lucide::GitFork),
            Papel::Projeto => ("Este chat", Lucide::Info),
            Papel::Coordenador => ("Esta conversa", Lucide::Info),
            _ => ("Ajudantes", Lucide::UsersRound),
        };
        let mut aba = Aba::new("aba-equipe", titulo).simbolo(Simbolo::icone_cor(icone, Cores::tenue()));
        if let Some((n, ativo)) = numero {
            aba = aba.numero(n, ativo);
        }
        let nome = motor.nome_do_projeto(&chat.projeto);
        let mut dica = match (chat.papel, &titulo_coordenador) {
            (Papel::Mestre, _) => "O que o Mestre mandou aos chats, por projeto".to_string(),
            (Papel::Projeto, _) => format!("Onde “{}” trabalha · {nome}", chat.titulo),
            (Papel::Coordenador, _) => format!("Conversa do Vitral antigo · {nome}"),
            (_, Some(t)) => format!("Ajudantes de “{t}” · {nome}"),
            (_, None) => format!("Ajudantes · {nome}"),
        };
        if let Some((n, _)) = numero {
            dica.push_str(&format!(" · painel {n}"));
        }
        (aba, Some(dica.into()))
    }
}

#[cfg(test)]
mod testes {
    use super::{AbaDireita, aba_inicial, tem_aba_pr};
    use crate::modelo::{Chat, EstadoTarefa, Papel, Pr, PrAnterior, Tarefa};

    fn chat(papel: Papel) -> Chat {
        let mut c: Chat = serde_json::from_str(
            r#"{"id": "c1", "projeto": "loja", "titulo": "Cupons", "papel": "Projeto", "pai": null,
                "tarefa": 3, "sessao": "s", "sessao_criada": false, "cwd": "/tmp/loja", "modelo": null,
                "criado_em": 0, "atualizado_em": 0, "custo_usd": 0.0}"#,
        )
        .unwrap();
        c.papel = papel;
        c
    }

    #[test]
    fn aba_inicial_pela_tarefa_e_pelo_lembrado() {
        let c = chat(Papel::Projeto);
        let sem_pr = Tarefa { estado: EstadoTarefa::Conversa, ..Default::default() };
        let no_pr = Tarefa { estado: EstadoTarefa::NoPr, pr: Some(Pr { numero: 12, ..Default::default() }), ..Default::default() };
        let anterior = Tarefa {
            estado: EstadoTarefa::Conversa,
            prs_anteriores: vec![PrAnterior { numero: 11, ..Default::default() }],
            ..Default::default()
        };
        // Sem PR, só "Este chat", mesmo com o PR lembrado de antes.
        assert_eq!(aba_inicial(&c, Some(&sem_pr), None), AbaDireita::Chat);
        assert_eq!(aba_inicial(&c, Some(&sem_pr), Some(AbaDireita::Pr)), AbaDireita::Chat);
        assert_eq!(aba_inicial(&c, None, None), AbaDireita::Chat);
        // Com o PR aberto, ele abre na aba PR; o lembrado vale.
        assert_eq!(aba_inicial(&c, Some(&no_pr), None), AbaDireita::Pr);
        assert_eq!(aba_inicial(&c, Some(&no_pr), Some(AbaDireita::Chat)), AbaDireita::Chat);
        // Só com anteriores: a aba existe, mas não abre nela sozinha.
        assert!(tem_aba_pr(&c, Some(&anterior)));
        assert_eq!(aba_inicial(&c, Some(&anterior), None), AbaDireita::Chat);
        assert_eq!(aba_inicial(&c, Some(&anterior), Some(AbaDireita::Pr)), AbaDireita::Pr);
        // O Mestre não tem aba PR.
        let mestre = chat(Papel::Mestre);
        assert!(!tem_aba_pr(&mestre, Some(&no_pr)));
        assert_eq!(aba_inicial(&mestre, Some(&no_pr), Some(AbaDireita::Pr)), AbaDireita::Chat);
        // O arquivo não fica lembrado.
        assert_eq!(aba_inicial(&c, Some(&no_pr), Some(AbaDireita::Arquivo(0))), AbaDireita::Pr);
    }

    #[test]
    fn as_abas_lembradas_saem_e_voltam_do_json() {
        let mapa: std::collections::HashMap<String, AbaDireita> =
            serde_json::from_str(r#"{"c1": "Pr", "c2": "Chat", "c3": {"Arquivo": 0}}"#).unwrap();
        assert_eq!(mapa["c1"], AbaDireita::Pr);
        assert_eq!(mapa["c3"], AbaDireita::Arquivo(0));
        assert_eq!(serde_json::to_string(&AbaDireita::Pr).unwrap(), "\"Pr\"");
    }
}
