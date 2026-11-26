//! Arranjos de painéis com nome, um por frente de trabalho ("Central", "Vitral"): o mesmo `Arranjo`
//! que a janela grava ao fechar (os chats de cada painel, as abas, a aba à vista, a divisão das
//! conversas e as larguras), guardado sob um nome na chave `arranjos` do interface.json.
//!
//! Trocar de arranjo guarda antes o que estava em uso, como a janela está agora, para a volta ser
//! de onde parou. Os chats que saem da tela continuam rodando: nada aqui mexe no motor. Um chat
//! arquivado ou apagado some do arranjo ao reabrir (`Arranjo::reabrivel`), sem erro.
//!
//! O que é lista e troca é puro (`Arranjos`), testado no fim; o resto é a janela aplicando.

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    input::{Input, InputState},
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::Arranjo;
use crate::tema::Lucide;
use crate::ui::{
    Janela,
    componentes::{RodapeDialogo, TamanhoBotao, Tom, aviso, botao_fantasma, botao_primario, largura},
    dialogos::{cabecalho, moldura, topo},
};

// Ctrl+Alt+PageDown e Ctrl+Alt+PageUp: o arranjo seguinte e o anterior, dando a volta.
actions!(vitral, [ArranjoSeguinte, ArranjoAnterior, SalvarArranjo]);

/// A chave do interface.json.
const CHAVE: &str = "arranjos";
/// Quantos arranjos dá para guardar (o menu do logo lista todos).
const MAX_ARRANJOS: usize = 12;
/// O tamanho máximo do nome.
const LETRAS_DO_NOME: usize = 40;

/// Um arranjo guardado com nome.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::ui) struct Salvo {
    pub nome: String,
    pub arranjo: Arranjo,
}

/// Os arranjos guardados, na ordem em que foram criados, e o que está em uso.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(in crate::ui) struct Arranjos {
    pub salvos: Vec<Salvo>,
    /// O nome do arranjo em uso; `None` quando a janela não segue nenhum.
    pub em_uso: Option<String>,
}

/// O nome como vai guardado: sem espaços nas pontas nem repetidos, até `LETRAS_DO_NOME` letras.
fn nome_limpo(nome: &str) -> String {
    nome.split_whitespace().collect::<Vec<_>>().join(" ").chars().take(LETRAS_DO_NOME).collect()
}

/// Dois nomes são o mesmo arranjo: sem ligar para maiúsculas.
fn mesmo_nome(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

impl Arranjos {
    /// Os arranjos gravados; sem a chave (ou com ela ilegível), nenhum. O "em uso" que aponta
    /// para um arranjo que não existe mais não vale.
    pub(in crate::ui) fn ler(interface: Option<&serde_json::Value>) -> Arranjos {
        let mut lidos: Arranjos =
            interface.and_then(|i| i.get(CHAVE)).and_then(|v| serde_json::from_value(v.clone()).ok()).unwrap_or_default();
        lidos.salvos.retain(|s| !s.nome.trim().is_empty());
        if lidos.em_uso.as_ref().is_some_and(|nome| lidos.posicao(nome).is_none()) {
            lidos.em_uso = None;
        }
        lidos
    }

    fn posicao(&self, nome: &str) -> Option<usize> {
        self.salvos.iter().position(|s| mesmo_nome(&s.nome, nome))
    }

    /// Guarda `arranjo` com esse nome (no lugar do que já tinha o nome, ou no fim da lista) e
    /// passa a usá-lo. Devolve o nome como ficou; `Err` com o motivo quando não dá.
    pub(in crate::ui) fn guardar(&mut self, nome: &str, arranjo: Arranjo) -> Result<String, &'static str> {
        let nome = nome_limpo(nome);
        if nome.is_empty() {
            return Err("Dê um nome ao arranjo");
        }
        if arranjo.paineis.iter().all(|p| p.chat.is_none()) {
            return Err("Abra pelo menos um chat antes de salvar o arranjo");
        }
        match self.posicao(&nome) {
            Some(i) => self.salvos[i] = Salvo { nome: nome.clone(), arranjo },
            None if self.salvos.len() >= MAX_ARRANJOS => return Err("Já há arranjos demais: apague um antes"),
            None => self.salvos.push(Salvo { nome: nome.clone(), arranjo }),
        }
        self.em_uso = Some(nome.clone());
        Ok(nome)
    }

    /// Tira o arranjo da lista; se era o em uso, a janela fica sem seguir nenhum.
    // O submenu Arranjos saiu do menu do Vitral no Linear (25/11/2026, a pedido do dono); o salvar
    // e o andar entre eles continuam nos atalhos. Fica para quando voltar a ter onde apagar.
    #[allow(dead_code)]
    pub(in crate::ui) fn apagar(&mut self, nome: &str) {
        self.salvos.retain(|s| !mesmo_nome(&s.nome, nome));
        if self.em_uso.as_deref().is_some_and(|em_uso| mesmo_nome(em_uso, nome)) {
            self.em_uso = None;
        }
    }

    /// O arranjo a `passo` do em uso (1: o seguinte; -1: o anterior), dando a volta. Sem nenhum
    /// em uso, o primeiro (ou o último, para trás). `None` quando não há para onde ir.
    pub(in crate::ui) fn vizinho(&self, passo: i32) -> Option<&str> {
        let n = self.salvos.len() as i32;
        let atual = self.em_uso.as_deref().and_then(|nome| self.posicao(nome));
        let alvo = match atual {
            Some(_) if n < 2 => return None,
            Some(i) => (i as i32 + passo).rem_euclid(n),
            None if n == 0 => return None,
            None if passo < 0 => n - 1,
            None => 0,
        };
        Some(self.salvos[alvo as usize].nome.as_str())
    }

    /// Troca para o arranjo `para`: o em uso guarda antes `agora` (como a janela está), para a
    /// volta ser de onde parou, e `para` passa a ser o em uso. Devolve o arranjo a aplicar.
    pub(in crate::ui) fn trocar(&mut self, para: &str, agora: Arranjo) -> Option<Arranjo> {
        let alvo = self.posicao(para)?;
        if let Some(i) = self.em_uso.as_deref().and_then(|nome| self.posicao(nome)).filter(|i| *i != alvo) {
            self.salvos[i].arranjo = agora;
        }
        self.em_uso = Some(self.salvos[alvo].nome.clone());
        Some(self.salvos[alvo].arranjo.clone())
    }

    /// Grava a lista no interface.json, na hora (são ações raras, e a troca seguinte já a relê).
    fn gravar(&self) {
        crate::ui::zoom::gravar_na_interface(CHAVE, serde_json::to_value(self).unwrap_or_default());
    }
}

/// O submenu "Arranjos" do menu do logo: os guardados (o em uso com o visto), "Salvar o arranjo
/// atual…" e, embaixo, um "Apagar" para cada um.
#[allow(dead_code)]
pub(in crate::ui) fn no_menu(menu: PopupMenu, janela: &WeakEntity<Janela>, window: &mut Window, cx: &mut Context<PopupMenu>) -> PopupMenu {
    let janela = janela.clone();
    menu.submenu_with_icon(Some(Lucide::LayoutGrid.into()), tr!("Arranjos"), window, cx, move |mut sub, _, _| {
        // Lido na hora em que o submenu é montado: é sempre o que está no disco.
        let arranjos = Arranjos::ler(crate::ui::zoom::interface().as_ref());
        for salvo in &arranjos.salvos {
            let (janela, nome) = (janela.clone(), salvo.nome.clone());
            let em_uso = arranjos.em_uso.as_deref().is_some_and(|n| mesmo_nome(n, &salvo.nome));
            sub = sub.item(PopupMenuItem::new(salvo.nome.clone()).checked(em_uso).on_click(move |_, window, cx| {
                let _ = janela.update(cx, |this, cx| this.trocar_de_arranjo(&nome, window, cx));
            }));
        }
        if !arranjos.salvos.is_empty() {
            sub = sub.separator();
        }
        let salvar = janela.clone();
        sub = sub.item(
            PopupMenuItem::new(tr!("Salvar o arranjo atual…"))
                .icon(Lucide::Plus)
                .action(Box::new(SalvarArranjo))
                .on_click(move |_, window, cx| {
                    let _ = salvar.update(cx, |this, cx| this.dialogo_salvar_arranjo(window, cx));
                }),
        );
        if !arranjos.salvos.is_empty() {
            sub = sub.separator();
        }
        for salvo in &arranjos.salvos {
            let (janela, nome) = (janela.clone(), salvo.nome.clone());
            sub = sub.item(PopupMenuItem::new(tr!("Apagar “{nome}”", nome = salvo.nome)).icon(Lucide::Trash).on_click(move |_, window, cx| {
                let _ = janela.update(cx, |this, cx| this.apagar_arranjo(&nome, window, cx));
            }));
        }
        sub
    })
}

impl Janela {
    /// O arranjo em uso e os guardados, lidos do interface.json (a paleta de comandos pergunta).
    pub(in crate::ui) fn arranjos_salvos(&self) -> Arranjos {
        Arranjos::ler(crate::ui::zoom::interface().as_ref())
    }

    /// "Salvar o arranjo atual…": pede o nome (já vem o do arranjo em uso, para salvar por cima) e
    /// guarda a janela como está.
    pub(in crate::ui) fn dialogo_salvar_arranjo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let em_uso = self.arranjos_salvos().em_uso.unwrap_or_default();
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder(tr!("Central, Vitral, Loja…")).default_value(em_uso));
        campo.update(cx, |c, cx| c.select_all(window, cx));
        let janela = cx.entity().downgrade();
        let salvar = {
            let campo = campo.clone();
            Rc::new(move |window: &mut Window, cx: &mut App| -> bool {
                let nome = campo.read(cx).value().to_string();
                janela.update(cx, |this, cx| this.salvar_arranjo(&nome, window, cx)).unwrap_or(false)
            })
        };
        let foco = campo.read(cx).focus_handle(cx);
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let vazio = campo.read(cx).value().trim().is_empty();
            let (no_botao, no_enter) = (salvar.clone(), salvar.clone());
            moldura(dialogo, largura::P, window)
                .title(topo(cabecalho(
                    tr!("Salvar o arranjo"),
                    Some(tr!("Guarda os chats de cada painel, as abas e as larguras com um nome. O mesmo nome salva por cima.").to_string()),
                )))
                .child(Input::new(&campo))
                .footer(
                    RodapeDialogo::new()
                        .cancelar(
                            botao_fantasma("arranjo-cancelar", None, tr!("Cancelar"), TamanhoBotao::Controle, window, cx)
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .principal(
                            botao_primario("arranjo-salvar", None, tr!("Salvar"), TamanhoBotao::Controle, window, cx)
                                .when(vazio, |b| b.disabled(true).opacity(0.45).tooltip(tr!("Dê um nome ao arranjo")))
                                .on_click(move |_, window, cx| {
                                    if no_botao(window, cx) {
                                        window.close_dialog(cx);
                                    }
                                }),
                        ),
                )
                .on_ok(move |_, window, cx| no_enter(window, cx))
        });
        window.focus(&foco, cx);
    }

    /// Guarda a janela como está com esse nome. Devolve `false` (com o aviso) quando não deu.
    fn salvar_arranjo(&mut self, nome: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let mut arranjos = self.arranjos_salvos();
        match arranjos.guardar(nome, self.arranjo_de_agora(window)) {
            Ok(nome) => {
                arranjos.gravar();
                window.push_notification(aviso(Tom::Pronto, tr!("Arranjo “{nome}” salvo", nome = nome)), cx);
                true
            }
            Err(motivo) => {
                window.push_notification(aviso(Tom::Atencao, crate::i18n::traduzir(motivo).into_owned()), cx);
                false
            }
        }
    }

    #[allow(dead_code)]
    fn apagar_arranjo(&mut self, nome: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut arranjos = self.arranjos_salvos();
        arranjos.apagar(nome);
        arranjos.gravar();
        window.push_notification(aviso(Tom::Info, tr!("Arranjo “{nome}” apagado (os chats continuam na lista)", nome = nome)), cx);
    }

    /// Ctrl+Alt+PageDown e PageUp: o arranjo vizinho do em uso.
    pub(in crate::ui) fn andar_nos_arranjos(&mut self, passo: i32, window: &mut Window, cx: &mut Context<Self>) {
        let arranjos = self.arranjos_salvos();
        match arranjos.vizinho(passo).map(str::to_string) {
            Some(nome) => self.trocar_de_arranjo(&nome, window, cx),
            None if arranjos.salvos.is_empty() => {
                window.push_notification(aviso(Tom::Info, tr!("Nenhum arranjo salvo ainda: “Salvar o arranjo atual…”, no menu do logo")), cx);
            }
            None => window.push_notification(aviso(Tom::Info, tr!("Só há este arranjo salvo")), cx),
        }
    }

    /// Troca a janela para o arranjo `nome`: guarda antes o que estava em uso e recoloca os
    /// painéis, as abas e as larguras do escolhido. Escolher o que já está em uso volta a janela a
    /// como ele foi guardado.
    pub(in crate::ui) fn trocar_de_arranjo(&mut self, nome: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let agora = self.arranjo_de_agora(window);
        let mut arranjos = self.arranjos_salvos();
        let Some(mut alvo) = arranjos.trocar(nome, agora.clone()) else {
            return;
        };
        let abriveis: std::collections::HashSet<String> =
            motor.read(cx).chats.iter().filter(|c| !c.arquivado).map(|c| c.id.clone()).collect();
        if alvo.reabrivel(|id| abriveis.contains(id)).0.is_empty() {
            window.push_notification(
                aviso(Tom::Atencao, tr!("Os chats do arranjo “{nome}” não existem mais (arquivados ou apagados)", nome = nome)),
                cx,
            );
            return;
        }
        arranjos.gravar();
        // O tamanho da janela não é do arranjo: fica o de agora.
        (alvo.janela, alvo.maximizada) = (agora.janela, agora.maximizada);
        // Os painéis de agora saem guardando o rascunho de cada caixa; as abas que eles tinham
        // não vão para o Ctrl+Shift+T (ninguém as fechou).
        let fechadas = self.abas_fechadas.clone();
        while let Some(id) = self.paineis.first().map(|p| p.id) {
            self.fechar_painel(id, cx);
        }
        self.abas_fechadas = fechadas;
        self.reabrir_arranjo(Some(alvo), |id| abriveis.contains(id), window, cx);
        // As divisórias recomeçam das larguras do arranjo.
        self.chave_partes = None;
        self.gravar_agora(cx);
        if let Some(composer) = self.composer_ativo() {
            let foco = composer.read(cx).focus_handle(cx);
            window.focus(&foco, cx);
        }
        window.push_notification(aviso(Tom::Info, tr!("Arranjo “{nome}”", nome = nome)), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use super::{Arranjos, LETRAS_DO_NOME, MAX_ARRANJOS, nome_limpo};
    use crate::ui::arranjo::{Arranjo, PainelGuardado};

    fn painel(abas: &[&str], chat: &str) -> PainelGuardado {
        PainelGuardado { abas: abas.iter().map(|a| a.to_string()).collect(), chat: Some(chat.to_string()) }
    }

    fn arranjo(paineis: Vec<PainelGuardado>) -> Arranjo {
        Arranjo { paineis, divisoes: vec![vec![0.6, 0.4]], largura_lista: Some(300.), ..Default::default() }
    }

    #[test]
    fn o_arranjo_gravado_volta_igual() {
        let mut arranjos = Arranjos::default();
        let central = arranjo(vec![painel(&["a", "b"], "b"), painel(&["c"], "c")]);
        let vitral = Arranjo { ativo: 1, lista_recolhida: true, ..arranjo(vec![painel(&["v"], "v"), painel(&["w", "x"], "w")]) };
        assert_eq!(arranjos.guardar("  Central ", central.clone()).as_deref(), Ok("Central"));
        assert_eq!(arranjos.guardar("Vitral", vitral.clone()).as_deref(), Ok("Vitral"));
        // Ida e volta pelo interface.json, com as outras chaves dele em volta.
        let interface = serde_json::json!({ "zoom": 1.0, "arranjos": serde_json::to_value(&arranjos).unwrap() });
        let lidos = Arranjos::ler(Some(&interface));
        assert_eq!(lidos, arranjos);
        assert_eq!(lidos.em_uso.as_deref(), Some("Vitral"));
        assert_eq!(lidos.salvos[0].arranjo, central);
        assert_eq!(lidos.salvos[1].arranjo, vitral);
        // Os painéis, as abas, a aba à vista e a divisão de cada um voltam para reabrir.
        let (paineis, ativo) = lidos.salvos[1].arranjo.reabrivel(|_| true);
        assert_eq!((paineis, ativo), (vitral.paineis.clone(), 1));
        assert_eq!(lidos.salvos[0].arranjo.larguras().divisao(2), Some(&[0.6, 0.4][..]));
        // Sem a chave, ou com ela torta, nenhum arranjo (e nada quebra).
        assert_eq!(Arranjos::ler(None), Arranjos::default());
        assert_eq!(Arranjos::ler(Some(&serde_json::json!({ "arranjos": "torto" }))), Arranjos::default());
        // O "em uso" de um arranjo que não existe mais não vale.
        let orfao = serde_json::json!({ "arranjos": { "salvos": [], "em_uso": "Sumiu" } });
        assert_eq!(Arranjos::ler(Some(&orfao)).em_uso, None);
    }

    #[test]
    fn trocar_guarda_antes_o_arranjo_em_uso() {
        let mut arranjos = Arranjos::default();
        let central = arranjo(vec![painel(&["a"], "a")]);
        let vitral = arranjo(vec![painel(&["v"], "v")]);
        arranjos.guardar("Central", central.clone()).unwrap();
        arranjos.guardar("Vitral", vitral.clone()).unwrap();
        // Em uso: o Vitral. A janela mudou desde que ele foi salvo (abriu outra aba).
        let vitral_de_agora = arranjo(vec![painel(&["v", "novo"], "novo")]);
        assert_eq!(arranjos.trocar("central", vitral_de_agora.clone()), Some(central.clone()));
        assert_eq!(arranjos.em_uso.as_deref(), Some("Central"));
        assert_eq!(arranjos.salvos[1].arranjo, vitral_de_agora, "o Vitral ficou como estava ao sair");
        // A volta é de onde parou.
        let central_de_agora = arranjo(vec![painel(&["a", "b"], "b")]);
        assert_eq!(arranjos.trocar("Vitral", central_de_agora.clone()), Some(vitral_de_agora));
        assert_eq!(arranjos.salvos[0].arranjo, central_de_agora);
        // Escolher o que já está em uso volta a janela a como ele foi guardado, sem regravar.
        let baguncado = arranjo(vec![painel(&["z"], "z")]);
        let guardado = arranjos.salvos[1].arranjo.clone();
        assert_eq!(arranjos.trocar("Vitral", baguncado), Some(guardado.clone()));
        assert_eq!(arranjos.salvos[1].arranjo, guardado);
        // Arranjo que não existe: nada muda.
        assert_eq!(arranjos.trocar("Nenhum", central), None);
        assert_eq!(arranjos.em_uso.as_deref(), Some("Vitral"));
    }

    #[test]
    fn o_atalho_anda_pelos_arranjos_dando_a_volta() {
        let mut arranjos = Arranjos::default();
        assert_eq!(arranjos.vizinho(1), None);
        for nome in ["A", "B", "C"] {
            arranjos.guardar(nome, arranjo(vec![painel(&["x"], "x")])).unwrap();
        }
        assert_eq!(arranjos.em_uso.as_deref(), Some("C"));
        assert_eq!(arranjos.vizinho(1), Some("A"));
        assert_eq!(arranjos.vizinho(-1), Some("B"));
        // Sem nenhum em uso, o primeiro para a frente e o último para trás.
        arranjos.em_uso = None;
        assert_eq!(arranjos.vizinho(1), Some("A"));
        assert_eq!(arranjos.vizinho(-1), Some("C"));
        // Com um só, e ele em uso, não há para onde ir.
        let mut um = Arranjos::default();
        um.guardar("Só", arranjo(vec![painel(&["x"], "x")])).unwrap();
        assert_eq!(um.vizinho(1), None);
    }

    #[test]
    fn guardar_por_cima_apagar_e_os_limites() {
        let mut arranjos = Arranjos::default();
        let um = arranjo(vec![painel(&["a"], "a")]);
        let outro = arranjo(vec![painel(&["b"], "b")]);
        arranjos.guardar("Central", um).unwrap();
        // O mesmo nome (sem ligar para maiúsculas) salva por cima, no mesmo lugar.
        arranjos.guardar("central", outro.clone()).unwrap();
        assert_eq!(arranjos.salvos.len(), 1);
        assert_eq!((arranjos.salvos[0].nome.as_str(), &arranjos.salvos[0].arranjo), ("central", &outro));
        // Sem nome e sem chat não salva.
        assert!(arranjos.guardar("   ", outro.clone()).is_err());
        assert!(arranjos.guardar("Vazio", Arranjo::default()).is_err());
        assert_eq!(nome_limpo(&format!("  dois   espaços {}", "x".repeat(80))).chars().count(), LETRAS_DO_NOME);
        // Apagar o em uso deixa a janela sem seguir nenhum.
        arranjos.apagar("CENTRAL");
        assert_eq!(arranjos, Arranjos::default());
        // O teto de arranjos.
        for n in 0..MAX_ARRANJOS {
            arranjos.guardar(&format!("a{n}"), outro.clone()).unwrap();
        }
        assert!(arranjos.guardar("mais um", outro.clone()).is_err());
        assert!(arranjos.guardar("a0", outro).is_ok(), "salvar por cima continua valendo");
    }
}
