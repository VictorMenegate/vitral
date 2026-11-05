//! O texto que você escreveu e não enviou. Trocar de conversa ou fechar o painel já guardava o
//! rascunho na memória da janela; aqui ele também vai para o interface.json, pouco depois de
//! parar de digitar, para voltar quando o Vitral abrir de novo (inclusive depois de uma queda).

use std::collections::{BTreeMap, HashMap};
use std::time::Duration;

use gpui_kit::*;

use super::Janela;

/// A chave do interface.json.
const CHAVE: &str = "rascunhos";

/// Quanto o Vitral espera a digitação parar antes de gravar (gravar a cada tecla seria uma ida ao
/// disco por letra).
const ESPERA: Duration = Duration::from_millis(1_200);

/// Quantas conversas ficam com rascunho gravado e o tamanho máximo de cada um: o interface.json
/// não é lugar para um texto sem fim.
const MAX_CONVERSAS: usize = 60;
const MAX_LETRAS: usize = 20_000;

/// Corta o rascunho no limite, sem partir uma letra no meio.
fn cortado(texto: &str) -> String {
    texto.chars().take(MAX_LETRAS).collect()
}

/// O que vai para o disco: os rascunhos guardados mais o texto das caixas abertas (que ganha
/// deles, por ser o de agora), sem os vazios e no máximo `MAX_CONVERSAS`, do mais recente para o
/// mais antigo (as caixas abertas primeiro).
pub(super) fn a_gravar(guardados: &HashMap<String, String>, abertos: &[(String, String)]) -> BTreeMap<String, String> {
    let mut ordem: Vec<(&str, &str)> = abertos.iter().map(|(c, t)| (c.as_str(), t.as_str())).collect();
    ordem.extend(guardados.iter().map(|(c, t)| (c.as_str(), t.as_str())));
    let mut tudo = BTreeMap::new();
    let mut vistos = Vec::new();
    for (chat, texto) in ordem {
        if vistos.contains(&chat) {
            continue;
        }
        vistos.push(chat);
        // Uma caixa esvaziada apaga o rascunho guardado da mesma conversa.
        if texto.trim().is_empty() {
            continue;
        }
        tudo.insert(chat.to_string(), cortado(texto));
        if tudo.len() >= MAX_CONVERSAS {
            break;
        }
    }
    tudo
}

/// Os rascunhos gravados, só os das conversas que ainda existem (um chat apagado leva o dele).
pub(super) fn ler(interface: Option<&serde_json::Value>, existe: impl Fn(&str) -> bool) -> HashMap<String, String> {
    interface
        .and_then(|i| i.get(CHAVE)?.as_object())
        .map(|mapa| {
            mapa.iter()
                .filter_map(|(chat, valor)| {
                    let texto = valor.as_str()?;
                    (!texto.trim().is_empty() && existe(chat)).then(|| (chat.clone(), texto.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}

impl Janela {
    /// Guarda na memória o rascunho de uma conversa que sai da caixa (ou o tira, se não sobrou
    /// nada escrito) e marca o disco para ser atualizado.
    pub(super) fn guardar_rascunho(&mut self, chat: String, texto: String, cx: &mut Context<Self>) {
        if texto.trim().is_empty() {
            self.rascunhos.remove(&chat);
        } else {
            self.rascunhos.insert(chat, texto);
        }
        self.agendar_gravacao(cx);
    }

    /// Grava os rascunhos pouco depois da última mudança. Cada mudança nova reinicia a espera, e
    /// o `Task` guardado na janela morre com ela (um Vitral fechando não deixa gravação pendente:
    /// o fechar passa por aqui antes, e a espera é curta).
    pub(super) fn agendar_gravacao(&mut self, cx: &mut Context<Self>) {
        self._gravar_rascunhos = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(ESPERA).await;
            let _ = this.update(cx, |this, cx| this.gravar_agora(cx));
        }));
    }

    /// Junta o que está guardado com o que está nas caixas abertas e grava a chave do
    /// interface.json (em segundo plano, como as outras).
    pub(super) fn gravar_agora(&mut self, cx: &mut Context<Self>) {
        let abertos: Vec<(String, String)> = self
            .paineis
            .iter()
            .filter_map(|p| Some((p.chat.clone()?, p.composer.read(cx).value().to_string())))
            .collect();
        let tudo = a_gravar(&self.rascunhos, &abertos);
        self._gravar_rascunhos = None;
        super::gravar_interface(CHAVE, serde_json::to_value(tudo).unwrap_or_default(), cx);
    }

    /// Ao fechar o Vitral: grava na hora, na própria linha (a gravação em segundo plano não teria
    /// tempo de acontecer), para o rascunho estar lá quando ele abrir de novo.
    pub(super) fn gravar_ao_sair(&self, cx: &App) {
        let abertos: Vec<(String, String)> = self
            .paineis
            .iter()
            .filter_map(|p| Some((p.chat.clone()?, p.composer.read(cx).value().to_string())))
            .collect();
        let tudo = a_gravar(&self.rascunhos, &abertos);
        super::zoom::gravar_na_interface(CHAVE, serde_json::to_value(tudo).unwrap_or_default());
    }
}

#[cfg(test)]
mod testes {
    use super::{MAX_LETRAS, a_gravar, ler};
    use std::collections::HashMap;

    fn guardados(pares: &[(&str, &str)]) -> HashMap<String, String> {
        pares.iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
    }

    fn abertos(pares: &[(&str, &str)]) -> Vec<(String, String)> {
        pares.iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
    }

    #[test]
    fn a_caixa_aberta_ganha_do_guardado_e_o_vazio_apaga() {
        let g = guardados(&[("a", "velho"), ("b", "só este"), ("c", "   ")]);
        let tudo = a_gravar(&g, &abertos(&[("a", "novo"), ("d", "")]));
        assert_eq!(tudo.get("a").map(String::as_str), Some("novo"));
        assert_eq!(tudo.get("b").map(String::as_str), Some("só este"));
        // Nem o rascunho só com espaços, nem a caixa aberta vazia, vão para o disco.
        assert_eq!(tudo.get("c"), None);
        assert_eq!(tudo.get("d"), None);
        assert_eq!(tudo.len(), 2);
    }

    #[test]
    fn a_caixa_esvaziada_tira_o_rascunho_que_estava_gravado() {
        let g = guardados(&[("a", "o que sobrou de antes")]);
        let tudo = a_gravar(&g, &abertos(&[("a", "")]));
        assert!(tudo.is_empty(), "{tudo:?}");
    }

    #[test]
    fn o_rascunho_enorme_entra_cortado_e_so_60_conversas_vao() {
        let enorme = "á".repeat(MAX_LETRAS + 500);
        let tudo = a_gravar(&HashMap::new(), &abertos(&[("a", &enorme)]));
        assert_eq!(tudo["a"].chars().count(), MAX_LETRAS);
        let muitos: Vec<(String, String)> = (0..100).map(|n| (format!("c{n}"), format!("texto {n}"))).collect();
        assert_eq!(a_gravar(&HashMap::new(), &muitos).len(), 60);
    }

    #[test]
    fn so_volta_o_rascunho_de_conversa_que_ainda_existe() {
        let json = serde_json::json!({"rascunhos": {"a": "oi", "b": " ", "sumiu": "x"}});
        let lidos = ler(Some(&json), |id| id != "sumiu");
        assert_eq!(lidos.len(), 1);
        assert_eq!(lidos.get("a").map(String::as_str), Some("oi"));
        assert!(ler(None, |_| true).is_empty());
        assert!(ler(Some(&serde_json::json!({})), |_| true).is_empty());
    }
}
