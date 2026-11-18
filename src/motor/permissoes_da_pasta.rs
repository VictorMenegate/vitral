//! "Sempre nesta pasta": as regras de permitir sempre que valem para todos os chats de uma pasta,
//! os abertos e os novos, e não só para o chat em que foram dadas (`Chat.permitidas`). São as
//! mesmas regras de `modelo::regra_de_permissao` ("edicoes", "bash:<comando>",
//! "ferramenta:<nome>"), guardadas num mapa de pasta para regras, no arquivo
//! `permissoes-da-pasta.json` da pasta de dados.
//!
//! O chat Sem pasta (a pasta dele é só dele) não tem regra de pasta, e o comando com trecho
//! arriscado continua sem regra nenhuma: quem não oferece a opção é o cartão do pedido.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::*;
use crate::modelo::regra_de_permissao;

/// As regras de cada pasta. O `BTreeMap` deixa o arquivo sempre na mesma ordem.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RegrasPorPasta(BTreeMap<PathBuf, Vec<String>>);

impl RegrasPorPasta {
    /// As regras dadas para a pasta, na ordem em que entraram.
    pub fn da_pasta(&self, pasta: &Path) -> &[String] {
        self.0.get(pasta).map_or(&[], Vec::as_slice)
    }

    /// O pedido de um chat dessa pasta cabe numa regra dela.
    pub fn permite(&self, ferramenta: &str, entrada: &Value, pasta: &Path) -> bool {
        regra_de_permissao(ferramenta, entrada, pasta).is_some_and(|r| self.da_pasta(pasta).contains(&r))
    }

    /// Guarda a regra na pasta. `false` quando ela já estava lá.
    fn guardar(&mut self, pasta: &Path, regra: String) -> bool {
        let regras = self.0.entry(pasta.to_path_buf()).or_default();
        if regras.contains(&regra) {
            return false;
        }
        regras.push(regra);
        true
    }

    /// Tira a regra da pasta (a pasta sem regra sai do mapa). `false` quando ela não estava lá.
    fn tirar(&mut self, pasta: &Path, regra: &str) -> bool {
        let Some(regras) = self.0.get_mut(pasta) else {
            return false;
        };
        let antes = regras.len();
        regras.retain(|r| r != regra);
        let mudou = regras.len() != antes;
        if regras.is_empty() {
            self.0.remove(pasta);
        }
        mudou
    }

    /// Tira todas as regras da pasta. `false` quando ela não tinha nenhuma.
    fn limpar(&mut self, pasta: &Path) -> bool {
        self.0.remove(pasta).is_some()
    }
}

fn arquivo(dados: &Dados) -> PathBuf {
    dados.pasta.join("permissoes-da-pasta.json")
}

/// As regras gravadas; sem arquivo (ou com ele ilegível), nenhuma: o pior que acontece é o Vitral
/// voltar a perguntar.
pub(super) fn ler(dados: &Dados) -> RegrasPorPasta {
    dados::ler_json_r(&arquivo(dados)).ok().flatten().unwrap_or_default()
}

impl Motor {
    /// As regras de "sempre nesta pasta" que valem para o chat; nenhuma no chat Sem pasta.
    pub fn regras_da_pasta(&self, chat: &Chat) -> &[String] {
        if self.sem_pasta(chat) { &[] } else { self.regras_da_pasta.da_pasta(&chat.cwd) }
    }

    /// O pedido passa sem perguntar: cabe numa regra do chat ou numa da pasta dele.
    pub fn permitido_sempre(&self, chat: &Chat, ferramenta: &str, entrada: &Value) -> bool {
        chat.permite(ferramenta, entrada) || (!self.sem_pasta(chat) && self.regras_da_pasta.permite(ferramenta, entrada, &chat.cwd))
    }

    /// Permite o pedido e guarda a regra na pasta do chat: os pedidos iguais passam sem perguntar
    /// em todos os chats dessa pasta, os que já estão esperando (em qualquer um deles) inclusive.
    /// Sem regra possível (o plano, a edição fora da pasta, o chat Sem pasta), só permite este.
    pub fn permitir_sempre_na_pasta(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(pedido) = self.permissoes.iter().find(|p| p.id == id) else {
            return;
        };
        let alvo = self.chat(&pedido.chat).filter(|c| !self.sem_pasta(c)).and_then(|c| {
            regra_de_permissao(&pedido.ferramenta, &pedido.entrada, &c.cwd).map(|regra| (c.cwd.clone(), regra))
        });
        let Some((pasta, regra)) = alvo else {
            self.responder_permissao(id, true, None, cx);
            return;
        };
        if self.regras_da_pasta.guardar(&pasta, regra) {
            self.gravar_regras_da_pasta();
        }
        let cobertos: Vec<String> = self
            .permissoes
            .iter()
            .filter(|p| {
                p.id == id
                    || self.chat(&p.chat).is_some_and(|c| c.cwd == pasta && self.regras_da_pasta.permite(&p.ferramenta, &p.entrada, &pasta))
            })
            .map(|p| p.id.clone())
            .collect();
        for pedido in cobertos {
            self.responder_permissao(&pedido, true, None, cx);
        }
    }

    /// Tira uma regra da pasta (o menu do modo): os chats dela voltam a perguntar.
    pub fn tirar_regra_da_pasta(&mut self, pasta: &Path, regra: &str, cx: &mut Context<Self>) {
        if self.regras_da_pasta.tirar(pasta, regra) {
            self.gravar_regras_da_pasta();
            cx.notify();
        }
    }

    /// Tira todas as regras da pasta.
    pub fn limpar_regras_da_pasta(&mut self, pasta: &Path, cx: &mut Context<Self>) {
        if self.regras_da_pasta.limpar(pasta) {
            self.gravar_regras_da_pasta();
            cx.notify();
        }
    }

    fn gravar_regras_da_pasta(&self) {
        if self.somente_leitura.is_some() {
            return;
        }
        if let Err(erro) = dados::gravar_json_seguro(&arquivo(&self.dados), &self.regras_da_pasta, false) {
            eprintln!("vitral: não gravou as permissões das pastas: {erro:#}");
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn chat_em(id: &str, pasta: &str) -> Chat {
        Chat::novo(id.into(), "t", PathBuf::from(pasta))
    }

    /// O que o motor pergunta antes de mostrar o pedido, sem o motor: a regra do chat ou a da pasta.
    fn passa(regras: &RegrasPorPasta, chat: &Chat, ferramenta: &str, entrada: &Value) -> bool {
        chat.permite(ferramenta, entrada) || regras.permite(ferramenta, entrada, &chat.cwd)
    }

    #[test]
    fn a_regra_da_pasta_vale_para_o_chat_novo_da_mesma_pasta_e_so_para_ela() {
        let mut regras = RegrasPorPasta::default();
        let npm = json!({"command": "npm test"});
        let antigo = chat_em("a", "/p/loja");
        // A regra sai do pedido do chat antigo, pela mesma `regra_de_permissao` do chat.
        let regra = regra_de_permissao("Bash", &npm, &antigo.cwd).unwrap();
        assert_eq!(regra, "bash:npm test");
        assert!(regras.guardar(&antigo.cwd, regra.clone()));
        assert!(!regras.guardar(&antigo.cwd, regra), "a mesma regra não entra duas vezes");
        // Um chat novo na mesma pasta roda sem perguntar, sem regra nenhuma dele.
        let novo = chat_em("b", "/p/loja");
        assert!(novo.permitidas.is_empty());
        assert!(passa(&regras, &novo, "Bash", &npm));
        // Só o comando exato, e só nessa pasta.
        assert!(!passa(&regras, &novo, "Bash", &json!({"command": "npm test && rm -rf /"})));
        assert!(!passa(&regras, &chat_em("c", "/p/outra"), "Bash", &npm));
        assert!(!passa(&regras, &chat_em("d", "/p/loja/sub"), "Bash", &npm));
        // Tirar a regra volta a perguntar.
        assert!(regras.tirar(Path::new("/p/loja"), "bash:npm test"));
        assert!(!passa(&regras, &novo, "Bash", &npm));
        assert_eq!(regras, RegrasPorPasta::default(), "a pasta sem regra sai do mapa");
        assert!(!regras.tirar(Path::new("/p/loja"), "bash:npm test"));
    }

    #[test]
    fn edicoes_e_ferramentas_da_pasta_seguem_a_regra_de_permissao() {
        let mut regras = RegrasPorPasta::default();
        let pasta = Path::new("/p/loja");
        regras.guardar(pasta, "edicoes".into());
        regras.guardar(pasta, "ferramenta:WebFetch".into());
        let chat = chat_em("a", "/p/loja");
        if cfg!(unix) {
            assert!(passa(&regras, &chat, "Edit", &json!({"file_path": "/p/loja/src/a.rs"})));
            // Fora da pasta não há regra: pergunta sempre, como no chat.
            assert!(!passa(&regras, &chat, "Write", &json!({"file_path": "/p/outra/a.rs"})));
            assert!(!passa(&regras, &chat, "Edit", &json!({"file_path": "/p/loja/../x"})));
        }
        assert!(passa(&regras, &chat, "WebFetch", &json!({"url": "x"})));
        assert!(!passa(&regras, &chat, "WebSearch", &json!({"query": "x"})));
        // O plano nunca tem regra.
        assert!(!passa(&regras, &chat, "ExitPlanMode", &json!({})));
        assert_eq!(regras.da_pasta(pasta), ["edicoes", "ferramenta:WebFetch"]);
        assert!(regras.limpar(pasta));
        assert!(regras.da_pasta(pasta).is_empty());
        assert!(!regras.limpar(pasta));
    }

    #[test]
    fn o_arquivo_e_um_mapa_de_pasta_para_regras_e_volta_igual() {
        let mut regras = RegrasPorPasta::default();
        regras.guardar(Path::new("/p/loja"), "bash:npm test".into());
        regras.guardar(Path::new("/p/central"), "edicoes".into());
        let gravado = serde_json::to_value(&regras).unwrap();
        assert_eq!(gravado, json!({ "/p/central": ["edicoes"], "/p/loja": ["bash:npm test"] }));
        assert_eq!(serde_json::from_value::<RegrasPorPasta>(gravado).unwrap(), regras);
        // Pela pasta de dados, com o arquivo que ainda não existe e depois de gravado.
        let raiz = std::env::temp_dir().join(format!("vitral-permissoes-{}", novo_id()));
        let dados = Dados::em(&raiz);
        std::fs::create_dir_all(&dados.pasta).unwrap();
        assert_eq!(ler(&dados), RegrasPorPasta::default());
        dados::gravar_json_seguro(&arquivo(&dados), &regras, false).unwrap();
        assert_eq!(ler(&dados), regras);
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
