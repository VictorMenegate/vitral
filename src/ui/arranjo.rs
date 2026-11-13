//! Como a janela estava quando o Vitral fechou: as conversas lado a lado com as abas de cada uma,
//! qual estava ativa, a lista aberta ou recolhida, as larguras dadas no arrasto das divisórias e o
//! tamanho da janela. Vai para o interface.json pouco depois de cada mudança (e na hora, ao sair)
//! e volta na abertura seguinte. Antes voltava só o último chat, numa janela de 1440 por 920; o
//! dono abriu, fechou e viu que "ele não lembra como tava antes de fechar" (13/11/2026).

use std::time::Duration;

use gpui_kit::*;
use serde::{Deserialize, Serialize};

use super::{
    FAIXA_DIREITA, FAIXA_LISTA, Janela, Larguras,
    paineis::{MAX_ABAS, MAX_PAINEIS},
};

/// A chave do interface.json.
const CHAVE: &str = "arranjo";

/// Quanto esperar a última mudança antes de gravar: arrastar uma divisória ou a borda da janela
/// muda o arranjo a cada quadro, e isso não pode virar uma ida ao disco por quadro.
const ESPERA: Duration = Duration::from_millis(600);

/// O tamanho com que a janela abre sem nada guardado, e o menor que ela aceita.
pub const TAMANHO_INICIAL: (f32, f32) = (1440., 920.);
pub const TAMANHO_MINIMO: (f32, f32) = (960., 600.);

/// Um painel de conversa como ficou guardado.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct PainelGuardado {
    /// As abas, na ordem do cabeçalho.
    pub abas: Vec<String>,
    /// A aba à vista.
    pub chat: Option<String>,
}

/// O arranjo da janela. Tudo com valor padrão: um arquivo de outra versão, com chave a mais ou a
/// menos, ainda abre.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Arranjo {
    /// Os painéis, da esquerda para a direita.
    pub paineis: Vec<PainelGuardado>,
    /// O painel ativo (o da luz de baixo).
    pub ativo: usize,
    /// A lista da esquerda recolhida (Ctrl+B).
    pub lista_recolhida: bool,
    /// As larguras dadas no arrasto, a 100% de zoom.
    pub largura_lista: Option<f32>,
    pub largura_direita: Option<f32>,
    /// A fração de cada conversa no espaço das conversas; vazia, partes iguais.
    pub conversas: Vec<f32>,
    /// A largura e a altura da janela fora de maximizada.
    pub janela: Option<(f32, f32)>,
    pub maximizada: bool,
}

impl Arranjo {
    /// O arranjo gravado; `None` sem a chave ou com ela ilegível (aí a janela abre como antes).
    pub(super) fn ler(interface: Option<&serde_json::Value>) -> Option<Arranjo> {
        serde_json::from_value(interface?.get(CHAVE)?.clone()).ok()
    }

    /// Os painéis que dá para reabrir e qual fica ativo. Só entram os chats que `existe` aceita
    /// (os apagados e os arquivados nesse meio tempo saem). Um chat aparece à vista num painel só:
    /// repetido, vale o primeiro. O painel cuja aba à vista sumiu mostra a última das que
    /// sobraram; sem nenhuma, ele não volta. O ativo acompanha o painel dele (ou o anterior, se
    /// ele não voltou).
    pub(super) fn reabrivel(&self, existe: impl Fn(&str) -> bool) -> (Vec<PainelGuardado>, usize) {
        // O que cada painel guardado mostrava, já sem os repetidos.
        let mut a_vista: Vec<Option<&str>> = Vec::new();
        for guardado in &self.paineis {
            let chat = guardado.chat.as_deref().filter(|c| existe(c) && !a_vista.contains(&Some(*c)));
            a_vista.push(chat);
        }
        let mut paineis: Vec<PainelGuardado> = Vec::new();
        let mut ativo = 0;
        for (ix, guardado) in self.paineis.iter().enumerate() {
            if paineis.len() >= MAX_PAINEIS {
                break;
            }
            // À vista em outro painel (dos guardados ou dos que já voltaram): aqui não entra.
            let de_outro = |id: &str| {
                a_vista.iter().enumerate().any(|(j, v)| j != ix && *v == Some(id))
                    || paineis.iter().any(|p| p.chat.as_deref() == Some(id))
            };
            let mut abas: Vec<String> = Vec::new();
            // A aba à vista entra nas abas mesmo que a lista guardada não a traga.
            for id in guardado.abas.iter().map(String::as_str).chain(a_vista[ix]) {
                if existe(id) && !de_outro(id) && !abas.iter().any(|a| a == id) {
                    abas.push(id.to_string());
                }
            }
            let Some(chat) = a_vista[ix].map(str::to_string).or_else(|| abas.last().cloned()) else {
                continue;
            };
            // Como no `acertar_abas`: passou do limite, sai a mais antiga que não está à vista.
            while abas.len() > MAX_ABAS {
                let Some(velha) = abas.iter().position(|a| *a != chat) else {
                    break;
                };
                abas.remove(velha);
            }
            if ix <= self.ativo {
                ativo = paineis.len();
            }
            paineis.push(PainelGuardado { abas, chat: Some(chat) });
        }
        (paineis, ativo)
    }

    /// As larguras guardadas, dentro das faixas de cada parte. A divisão das conversas só vale
    /// para o mesmo número de painéis e com frações que fecham a conta; senão, partes iguais.
    pub(super) fn larguras(&self, paineis: usize) -> Larguras {
        let padrao = Larguras::default();
        let na_faixa = |guardada: Option<f32>, faixa: (f32, f32), padrao: f32| {
            guardada.filter(|v| v.is_finite()).map_or(padrao, |v| v.clamp(faixa.0, faixa.1))
        };
        let soma: f32 = self.conversas.iter().sum();
        let fecha = paineis > 1
            && self.conversas.len() == paineis
            && self.conversas.iter().all(|f| f.is_finite() && *f > 0.02)
            && (soma - 1.).abs() < 0.02;
        Larguras {
            lista: na_faixa(self.largura_lista, FAIXA_LISTA, padrao.lista),
            direita: na_faixa(self.largura_direita, FAIXA_DIREITA, padrao.direita),
            conversas: if fecha { self.conversas.clone() } else { Vec::new() },
        }
    }
}

/// O tamanho com que a janela abre: o guardado, nunca menor que o mínimo nem maior que a tela
/// (o monitor de antes pode não estar mais ligado); sem nada guardado, o inicial.
fn tamanho_da_janela(guardado: Option<(f32, f32)>, tela: Option<(f32, f32)>) -> (f32, f32) {
    let (largura, altura) = guardado.filter(|(l, a)| l.is_finite() && a.is_finite()).unwrap_or(TAMANHO_INICIAL);
    let (max_l, max_a) = tela.filter(|(l, a)| l.is_finite() && a.is_finite()).unwrap_or((f32::MAX, f32::MAX));
    (
        largura.min(max_l).max(TAMANHO_MINIMO.0).round(),
        altura.min(max_a).max(TAMANHO_MINIMO.1).round(),
    )
}

/// Os limites com que a janela principal abre: o tamanho de quando fechou, no meio da tela (a
/// posição não se guarda: no Wayland quem põe a janela é o sistema), e maximizada se estava.
pub fn limites_da_janela(cx: &App) -> WindowBounds {
    let arranjo = Arranjo::ler(super::zoom::interface().as_ref());
    let tela = cx.primary_display().map(|d| d.bounds().size).map(|t| (t.width.as_f32(), t.height.as_f32()));
    let (largura, altura) = tamanho_da_janela(arranjo.as_ref().and_then(|a| a.janela), tela);
    let limites = Bounds::centered(None, size(px(largura), px(altura)), cx);
    if arranjo.is_some_and(|a| a.maximizada) { WindowBounds::Maximized(limites) } else { WindowBounds::Windowed(limites) }
}

impl Janela {
    /// O arranjo de agora, como vai para o disco.
    fn arranjo_de_agora(&self, window: &Window) -> Arranjo {
        let anterior = self.arranjo.as_ref();
        // Os limites de fora (com a margem da sombra), que são os que a abertura recebe de
        // volta: com os de dentro a janela encolhia 40 px a cada abertura.
        let (janela, maximizada) = match window.window_bounds() {
            WindowBounds::Windowed(limites) => {
                (Some((limites.size.width.as_f32().round(), limites.size.height.as_f32().round())), false)
            }
            // Maximizada ou em tela cheia, o tamanho guardado continua o de antes: é para ele que
            // a janela volta ao sair desse estado.
            WindowBounds::Maximized(_) => (anterior.and_then(|a| a.janela), true),
            WindowBounds::Fullscreen(_) => (anterior.and_then(|a| a.janela), anterior.is_some_and(|a| a.maximizada)),
        };
        Arranjo {
            paineis: self
                .paineis
                .iter()
                .map(|p| PainelGuardado { abas: p.abas.clone(), chat: p.chat.clone() })
                .collect(),
            ativo: self.ativo,
            lista_recolhida: !self.lista,
            largura_lista: Some(self.larguras.lista),
            largura_direita: Some(self.larguras.direita),
            conversas: self.larguras.conversas.clone(),
            janela,
            maximizada,
        }
    }

    /// Na abertura: põe a janela como estava. Devolve `false` se não havia o que reabrir (aí a
    /// abertura segue como antes, no último chat). Chamado com o motor já ligado.
    pub(super) fn reabrir_arranjo(
        &mut self,
        guardado: Option<Arranjo>,
        existe: impl Fn(&str) -> bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(guardado) = guardado else {
            return false;
        };
        let (paineis, ativo) = guardado.reabrivel(existe);
        self.lista = !guardado.lista_recolhida;
        self.larguras = guardado.larguras(paineis.len());
        // O tamanho guardado continua valendo se a janela abrir maximizada.
        self.arranjo = Some(guardado);
        if paineis.is_empty() {
            return false;
        }
        for guardado in paineis {
            let mut painel = self.criar_painel(guardado.chat, window, cx);
            painel.abas = guardado.abas;
            self.paineis.push(painel);
        }
        self.ativo = ativo.min(self.paineis.len() - 1);
        self.selecionado = self.paineis[self.ativo].chat.clone();
        true
    }

    /// Chamado a cada desenho, depois de as abas estarem acertadas: se o arranjo mudou, marca a
    /// gravação para daqui a pouco (cada mudança nova recomeça a espera). Só vale depois da
    /// abertura: antes dela a janela ainda está vazia, e gravar isso apagaria o que ficou guardado.
    pub(super) fn lembrar_arranjo(&mut self, window: &Window, cx: &mut Context<Self>) {
        if !self.arranjo_ligado {
            return;
        }
        let agora = self.arranjo_de_agora(window);
        if self.arranjo.as_ref() == Some(&agora) {
            return;
        }
        self.arranjo = Some(agora);
        self._gravar_arranjo = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ESPERA).await;
            let _ = this.update(cx, |this, cx| {
                if let Some(arranjo) = &this.arranjo {
                    super::gravar_interface(CHAVE, serde_json::to_value(arranjo).unwrap_or_default(), cx);
                }
            });
        }));
    }

    /// Ao fechar o Vitral: grava na hora, na própria linha (a gravação marcada não teria tempo de
    /// acontecer).
    pub(super) fn gravar_arranjo_ao_sair(&self) {
        if let Some(arranjo) = self.arranjo.as_ref().filter(|_| self.arranjo_ligado) {
            super::zoom::gravar_na_interface(CHAVE, serde_json::to_value(arranjo).unwrap_or_default());
        }
    }
}

#[cfg(test)]
mod testes {
    use super::{Arranjo, PainelGuardado, TAMANHO_INICIAL, TAMANHO_MINIMO, tamanho_da_janela};
    use crate::ui::{
        FAIXA_LISTA, Larguras,
        paineis::{MAX_ABAS, MAX_PAINEIS},
    };

    fn painel(abas: &[&str], chat: Option<&str>) -> PainelGuardado {
        PainelGuardado { abas: abas.iter().map(|a| a.to_string()).collect(), chat: chat.map(str::to_string) }
    }

    fn arranjo(paineis: Vec<PainelGuardado>, ativo: usize) -> Arranjo {
        Arranjo { paineis, ativo, ..Default::default() }
    }

    #[test]
    fn volta_como_estava_quando_tudo_ainda_existe() {
        let a = arranjo(vec![painel(&["a", "b"], Some("b")), painel(&["c"], Some("c"))], 1);
        let (paineis, ativo) = a.reabrivel(|_| true);
        assert_eq!(paineis, a.paineis);
        assert_eq!(ativo, 1);
    }

    #[test]
    fn chat_que_sumiu_sai_das_abas_e_o_painel_mostra_outra() {
        let a = arranjo(vec![painel(&["a", "b", "sumiu"], Some("sumiu")), painel(&["c"], Some("c"))], 0);
        let (paineis, ativo) = a.reabrivel(|id| id != "sumiu");
        assert_eq!(paineis, vec![painel(&["a", "b"], Some("b")), painel(&["c"], Some("c"))]);
        assert_eq!(ativo, 0);
    }

    #[test]
    fn painel_sem_nada_para_mostrar_nao_volta_e_o_ativo_acompanha() {
        let a = arranjo(
            vec![painel(&["a"], Some("a")), painel(&["sumiu"], Some("sumiu")), painel(&["c"], Some("c"))],
            2,
        );
        let (paineis, ativo) = a.reabrivel(|id| id != "sumiu");
        assert_eq!(paineis, vec![painel(&["a"], Some("a")), painel(&["c"], Some("c"))]);
        assert_eq!(ativo, 1, "o ativo era o terceiro, que virou o segundo");
        // O ativo era o que sumiu: fica o anterior.
        let (_, ativo) = arranjo(a.paineis.clone(), 1).reabrivel(|id| id != "sumiu");
        assert_eq!(ativo, 0);
        // Nada sobrou: nada a reabrir.
        assert_eq!(a.reabrivel(|_| false), (Vec::new(), 0));
    }

    #[test]
    fn um_chat_fica_a_vista_num_painel_so() {
        // O mesmo chat à vista em dois: vale o primeiro, e o segundo mostra outra aba dele.
        let a = arranjo(vec![painel(&["a"], Some("a")), painel(&["b", "a"], Some("a"))], 0);
        let (paineis, _) = a.reabrivel(|_| true);
        assert_eq!(paineis, vec![painel(&["a"], Some("a")), painel(&["b"], Some("b"))]);
        // A aba de fundo que está à vista no painel ao lado não serve de reserva.
        let a = arranjo(vec![painel(&["b", "sumiu"], Some("sumiu")), painel(&["b"], Some("b"))], 0);
        let (paineis, ativo) = a.reabrivel(|id| id != "sumiu");
        assert_eq!(paineis, vec![painel(&["b"], Some("b"))]);
        assert_eq!(ativo, 0);
    }

    #[test]
    fn respeita_o_maximo_de_paineis_e_de_abas() {
        let muitos: Vec<PainelGuardado> =
            (0..MAX_PAINEIS + 2).map(|n| painel(&[&format!("c{n}")], Some(&format!("c{n}")))).collect();
        let (paineis, ativo) = arranjo(muitos, MAX_PAINEIS + 1).reabrivel(|_| true);
        assert_eq!(paineis.len(), MAX_PAINEIS);
        assert_eq!(ativo, MAX_PAINEIS - 1);
        let nomes: Vec<String> = (0..MAX_ABAS + 3).map(|n| format!("a{n}")).collect();
        let abas: Vec<&str> = nomes.iter().map(String::as_str).collect();
        let (paineis, _) = arranjo(vec![painel(&abas, Some("a0"))], 0).reabrivel(|_| true);
        assert_eq!(paineis[0].abas.len(), MAX_ABAS);
        assert!(paineis[0].abas.contains(&"a0".to_string()), "a aba à vista nunca sai");
    }

    #[test]
    fn a_aba_a_vista_entra_nas_abas_mesmo_sem_estar_na_lista() {
        let (paineis, _) = arranjo(vec![painel(&[], Some("a"))], 0).reabrivel(|_| true);
        assert_eq!(paineis, vec![painel(&["a"], Some("a"))]);
    }

    #[test]
    fn larguras_ficam_na_faixa_e_a_divisao_so_vale_se_fecha() {
        let a = Arranjo {
            largura_lista: Some(50.),
            largura_direita: Some(f32::NAN),
            conversas: vec![0.3, 0.7],
            ..Default::default()
        };
        let l = a.larguras(2);
        assert_eq!(l.lista, FAIXA_LISTA.0);
        assert_eq!(l.direita, Larguras::default().direita);
        assert_eq!(l.conversas, vec![0.3, 0.7]);
        // Outro número de painéis, ou frações que não somam 1: partes iguais.
        assert!(a.larguras(3).conversas.is_empty());
        assert!(a.larguras(1).conversas.is_empty());
        let torto = Arranjo { conversas: vec![0.9, 0.9], ..Default::default() };
        assert!(torto.larguras(2).conversas.is_empty());
        // Sem nada guardado, as larguras de sempre.
        assert_eq!(Arranjo::default().larguras(1), Larguras::default());
    }

    #[test]
    fn tamanho_da_janela_tem_minimo_e_cabe_na_tela() {
        assert_eq!(tamanho_da_janela(None, None), TAMANHO_INICIAL);
        assert_eq!(tamanho_da_janela(Some((1800., 1000.)), None), (1800., 1000.));
        assert_eq!(tamanho_da_janela(Some((300., 200.)), None), TAMANHO_MINIMO);
        // O monitor grande de antes não está mais ligado.
        assert_eq!(tamanho_da_janela(Some((3000., 1600.)), Some((1920., 1080.))), (1920., 1080.));
        // Uma tela menor que o mínimo não derruba o mínimo.
        assert_eq!(tamanho_da_janela(Some((1400., 900.)), Some((800., 500.))), TAMANHO_MINIMO);
        assert_eq!(tamanho_da_janela(Some((f32::NAN, 900.)), None), TAMANHO_INICIAL);
    }

    #[test]
    fn le_do_interface_e_aceita_arquivo_de_outra_versao() {
        let json = serde_json::json!({ "arranjo": {
            "paineis": [{ "abas": ["a", "b"], "chat": "b" }],
            "ativo": 0,
            "janela": [1600.0, 900.0],
            "maximizada": true,
            "chave_que_nao_existe_mais": 1,
        } });
        let a = Arranjo::ler(Some(&json)).unwrap();
        assert_eq!(a.paineis, vec![painel(&["a", "b"], Some("b"))]);
        assert_eq!(a.janela, Some((1600., 900.)));
        assert!(a.maximizada && !a.lista_recolhida);
        // O que foi gravado volta igual.
        let de_volta = serde_json::json!({ "arranjo": serde_json::to_value(&a).unwrap() });
        assert_eq!(Arranjo::ler(Some(&de_volta)), Some(a));
        assert_eq!(Arranjo::ler(None), None);
        assert_eq!(Arranjo::ler(Some(&serde_json::json!({}))), None);
        assert_eq!(Arranjo::ler(Some(&serde_json::json!({ "arranjo": "torto" }))), None);
    }
}
