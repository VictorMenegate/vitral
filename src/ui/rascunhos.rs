//! O texto que você escreveu e não enviou. Trocar de conversa ou fechar o painel já guardava o
//! rascunho na memória da janela; aqui ele também vai para o interface.json, pouco depois de
//! parar de digitar, para voltar quando o Vitral abrir de novo (inclusive depois de uma queda).

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
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

/// O rascunho inteiro de uma conversa (dono, 16/11/2026: rodada de uso, item 1.4): o texto, os
/// anexos e a citação. Antes só o texto ia para o disco, e os prints colados se perdiam ao reabrir
/// ou numa queda. O Ultrathink e o Ultracode continuam começando desligados.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub(super) struct Rascunho {
    pub texto: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub anexos: Vec<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub citacao: Option<String>,
}

impl Rascunho {
    fn vazio(&self) -> bool {
        self.texto.trim().is_empty() && self.anexos.is_empty() && self.citacao.is_none()
    }
}

/// O que vai para o disco: os rascunhos guardados mais o texto das caixas abertas (que ganha
/// deles, por ser o de agora), com os anexos e a citação de cada conversa, sem os vazios e no
/// máximo `MAX_CONVERSAS`, do mais recente para o mais antigo (as caixas abertas primeiro). Um
/// rascunho só com anexo também vai.
pub(super) fn a_gravar(
    guardados: &HashMap<String, String>,
    abertos: &[(String, String)],
    anexos: &HashMap<String, Vec<PathBuf>>,
    citacoes: &HashMap<String, String>,
) -> BTreeMap<String, Rascunho> {
    let mut ordem: Vec<(&str, &str)> = abertos.iter().map(|(c, t)| (c.as_str(), t.as_str())).collect();
    ordem.extend(guardados.iter().map(|(c, t)| (c.as_str(), t.as_str())));
    // As conversas só com anexo ou citação (sem texto em lugar nenhum) vêm depois.
    let mut so_anexos: Vec<&str> = anexos.keys().chain(citacoes.keys()).map(String::as_str).collect();
    so_anexos.sort_unstable();
    so_anexos.dedup();
    ordem.extend(so_anexos.into_iter().map(|c| (c, "")));
    let mut tudo = BTreeMap::new();
    let mut vistos = Vec::new();
    for (chat, texto) in ordem {
        if vistos.contains(&chat) {
            continue;
        }
        vistos.push(chat);
        let rascunho = Rascunho {
            // Uma caixa esvaziada apaga o texto guardado da mesma conversa.
            texto: if texto.trim().is_empty() { String::new() } else { cortado(texto) },
            anexos: anexos.get(chat).cloned().unwrap_or_default(),
            citacao: citacoes.get(chat).cloned(),
        };
        if rascunho.vazio() {
            continue;
        }
        tudo.insert(chat.to_string(), rascunho);
        if tudo.len() >= MAX_CONVERSAS {
            break;
        }
    }
    tudo
}

/// Os rascunhos gravados, só os das conversas que ainda existem (um chat apagado leva o dele). Lê
/// os dois formatos: o de antes (só o texto) e o de agora (o rascunho inteiro).
pub(super) fn ler(interface: Option<&serde_json::Value>, existe: impl Fn(&str) -> bool) -> HashMap<String, Rascunho> {
    interface
        .and_then(|i| i.get(CHAVE)?.as_object())
        .map(|mapa| {
            mapa.iter()
                .filter_map(|(chat, valor)| {
                    let rascunho = match valor {
                        serde_json::Value::String(texto) => Rascunho { texto: texto.clone(), ..Default::default() },
                        outro => serde_json::from_value::<Rascunho>(outro.clone()).ok()?,
                    };
                    (!rascunho.vazio() && existe(chat)).then(|| (chat.clone(), rascunho))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// O que a lista e a aba mostram de um rascunho: "Rascunho: começo do texto", ou "Rascunho: 2
/// anexos" sem texto.
pub(super) fn rotulo_do_rascunho(texto: Option<&str>, anexos: usize, citacao: bool) -> Option<String> {
    let linha = texto.and_then(|t| t.lines().map(str::trim).find(|l| !l.is_empty()));
    match (linha, anexos, citacao) {
        (Some(linha), _, _) => {
            let curta: String = linha.chars().take(80).collect();
            Some(format!("Rascunho: {curta}"))
        }
        (None, n, _) if n > 0 => Some(format!("Rascunho: {}", super::componentes::contagem(n, "anexo", "anexos"))),
        (None, _, true) => Some("Rascunho: uma citação".to_string()),
        _ => None,
    }
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
        let tudo = self.rascunhos_a_gravar(cx);
        self._gravar_rascunhos = None;
        super::gravar_interface(CHAVE, serde_json::to_value(tudo).unwrap_or_default(), cx);
    }

    /// Ao fechar o Vitral: grava na hora, na própria linha (a gravação em segundo plano não teria
    /// tempo de acontecer), para o rascunho estar lá quando ele abrir de novo.
    pub(super) fn gravar_ao_sair(&self, cx: &App) {
        let tudo = self.rascunhos_a_gravar(cx);
        super::zoom::gravar_na_interface(CHAVE, serde_json::to_value(tudo).unwrap_or_default());
    }

    fn rascunhos_a_gravar(&self, cx: &App) -> BTreeMap<String, Rascunho> {
        let abertos: Vec<(String, String)> = self
            .paineis
            .iter()
            .filter_map(|p| Some((p.chat.clone()?, p.composer.read(cx).value().to_string())))
            .collect();
        a_gravar(&self.rascunhos, &abertos, &self.anexos, &self.citacoes)
    }

    /// Os rascunhos lidos ao abrir: o texto volta para a caixa (pelo `selecionar`), e os anexos e a
    /// citação para o lugar deles.
    pub(super) fn restaurar_rascunhos(&mut self, lidos: HashMap<String, Rascunho>) {
        for (chat, r) in lidos {
            if !r.texto.trim().is_empty() {
                self.rascunhos.insert(chat.clone(), r.texto);
            }
            if !r.anexos.is_empty() {
                self.anexos.entry(chat.clone()).or_insert(r.anexos);
            }
            if let Some(citacao) = r.citacao {
                self.citacoes.entry(chat).or_insert(citacao);
            }
        }
    }

    /// O rascunho de uma conversa que não está à vista em nenhum painel, para a marca da lista e
    /// das abas (o painel em que ela está aberta não precisa: a caixa está ali).
    pub(super) fn rascunho_fora_de_vista(&self, chat: &str) -> Option<String> {
        if self.paineis.iter().any(|p| p.chat.as_deref() == Some(chat)) {
            return None;
        }
        rotulo_do_rascunho(
            self.rascunhos.get(chat).map(String::as_str),
            self.anexos.get(chat).map_or(0, Vec::len),
            self.citacoes.contains_key(chat),
        )
    }
}

#[cfg(test)]
mod testes {
    use super::{MAX_LETRAS, Rascunho, a_gravar, ler, rotulo_do_rascunho};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn gravar(g: &HashMap<String, String>, a: &[(String, String)]) -> std::collections::BTreeMap<String, Rascunho> {
        a_gravar(g, a, &HashMap::new(), &HashMap::new())
    }

    fn guardados(pares: &[(&str, &str)]) -> HashMap<String, String> {
        pares.iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
    }

    fn abertos(pares: &[(&str, &str)]) -> Vec<(String, String)> {
        pares.iter().map(|(c, t)| (c.to_string(), t.to_string())).collect()
    }

    #[test]
    fn a_caixa_aberta_ganha_do_guardado_e_o_vazio_apaga() {
        let g = guardados(&[("a", "velho"), ("b", "só este"), ("c", "   ")]);
        let tudo = gravar(&g, &abertos(&[("a", "novo"), ("d", "")]));
        assert_eq!(tudo.get("a").map(|r| r.texto.as_str()), Some("novo"));
        assert_eq!(tudo.get("b").map(|r| r.texto.as_str()), Some("só este"));
        // Nem o rascunho só com espaços, nem a caixa aberta vazia, vão para o disco.
        assert_eq!(tudo.get("c"), None);
        assert_eq!(tudo.get("d"), None);
        assert_eq!(tudo.len(), 2);
    }

    #[test]
    fn a_caixa_esvaziada_tira_o_rascunho_que_estava_gravado() {
        let g = guardados(&[("a", "o que sobrou de antes")]);
        let tudo = gravar(&g, &abertos(&[("a", "")]));
        assert!(tudo.is_empty(), "{tudo:?}");
    }

    #[test]
    fn o_rascunho_enorme_entra_cortado_e_so_60_conversas_vao() {
        let enorme = "á".repeat(MAX_LETRAS + 500);
        let tudo = gravar(&HashMap::new(), &abertos(&[("a", &enorme)]));
        assert_eq!(tudo["a"].texto.chars().count(), MAX_LETRAS);
        let muitos: Vec<(String, String)> = (0..100).map(|n| (format!("c{n}"), format!("texto {n}"))).collect();
        assert_eq!(gravar(&HashMap::new(), &muitos).len(), 60);
    }

    #[test]
    fn so_volta_o_rascunho_de_conversa_que_ainda_existe() {
        let json = serde_json::json!({"rascunhos": {"a": "oi", "b": " ", "sumiu": "x"}});
        let lidos = ler(Some(&json), |id| id != "sumiu");
        assert_eq!(lidos.len(), 1);
        assert_eq!(lidos.get("a").map(|r| r.texto.as_str()), Some("oi"));
        assert!(ler(None, |_| true).is_empty());
        assert!(ler(Some(&serde_json::json!({})), |_| true).is_empty());
    }

    #[test]
    fn o_rascunho_leva_os_anexos_e_a_citacao_e_le_o_formato_antigo() {
        let anexos: HashMap<String, Vec<PathBuf>> = [("a".to_string(), vec![PathBuf::from("/x/print.png")])].into();
        let citacoes: HashMap<String, String> = [("b".to_string(), "o trecho".to_string())].into();
        // Só com o print (a caixa vazia) também vai.
        let tudo = a_gravar(&HashMap::new(), &abertos(&[("a", "")]), &anexos, &citacoes);
        assert_eq!(tudo["a"], Rascunho { texto: String::new(), anexos: vec![PathBuf::from("/x/print.png")], citacao: None });
        assert_eq!(tudo["b"].citacao.as_deref(), Some("o trecho"));
        // Volta igual, e o formato antigo (só o texto) continua abrindo.
        let json = serde_json::json!({"rascunhos": serde_json::to_value(&tudo).unwrap()});
        let lidos = ler(Some(&json), |_| true);
        assert_eq!(lidos["a"].anexos, [PathBuf::from("/x/print.png")]);
        let antigo = serde_json::json!({"rascunhos": {"c": "texto de antes"}});
        assert_eq!(ler(Some(&antigo), |_| true)["c"].texto, "texto de antes");
    }

    #[test]
    fn o_rotulo_do_rascunho_na_lista() {
        assert_eq!(rotulo_do_rascunho(Some("\n  faz o deploy\nresto"), 1, false).as_deref(), Some("Rascunho: faz o deploy"));
        assert_eq!(rotulo_do_rascunho(None, 2, false).as_deref(), Some("Rascunho: 2 anexos"));
        assert_eq!(rotulo_do_rascunho(Some("   "), 0, true).as_deref(), Some("Rascunho: uma citação"));
        assert_eq!(rotulo_do_rascunho(None, 0, false), None);
    }
}
