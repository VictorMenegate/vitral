//! As contas do Codex no motor (ver `crate::contas_codex`): qual conta cada chat usa, a troca
//! (que no Codex abre uma sessão nova, com a conversa até ali anexada à próxima mensagem), o
//! entrar, o tirar e a troca sozinha no limite do plano.

use super::*;
use crate::contas_codex::{self, ContaCodex};

impl Motor {
    /// A conta do Codex que este chat usa: a dele, senão a padrão das Configurações, senão a
    /// principal. Uma conta que sumiu do cadastro volta à principal.
    pub fn conta_codex_para(&self, chat: &Chat) -> ContaCodex {
        let escolha = chat.conta_codex.clone().unwrap_or_else(|| self.config.conta_codex.clone());
        self.contas_codex
            .iter()
            .find(|c| c.id == escolha)
            .or_else(|| self.contas_codex.first())
            .cloned()
            .unwrap_or_else(|| ContaCodex { id: contas_codex::PRINCIPAL.into(), nome: "Principal".into() })
    }

    /// Troca a conta do Codex do chat. Se o Codex já tinha uma conversa aberta nele, ela não
    /// continua na outra conta: a sessão recomeça, e a conversa até aqui vai num arquivo anexado,
    /// como nota, à próxima mensagem. `None` volta a seguir a padrão.
    pub fn definir_conta_codex(&mut self, chat_id: &str, conta: Option<String>, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_gravado(chat_id) else {
            return;
        };
        let antes = self.conta_codex_para(chat).id;
        let mut depois = chat.clone();
        depois.conta_codex = conta.clone();
        let nova = self.conta_codex_para(&depois);
        let com_sessao = chat.agente == Agente::Codex && chat.sessao_criada;
        if let Some(c) = self.chat_mut(chat_id) {
            c.conta_codex = conta;
        }
        if nova.id != antes && com_sessao && !self.processos.contains_key(chat_id) {
            if let Some(nota) = self.recomecar_codex_noutra_conta(chat_id, &nova) {
                self.filas.entry(chat_id.to_string()).or_default().push(nota);
            }
        } else {
            self.avisar(chat_id, &format!("O Codex deste chat passa a usar a conta {}.", nova.nome), false);
        }
        self.salvar();
        cx.notify();
    }

    /// A conversa do Codex recomeça na conta `nova`: sessão nova, e a nota com o arquivo da
    /// conversa até aqui, para ir junto com a próxima mensagem (o Zerosub faz o mesmo: o Codex não
    /// lê a conversa cifrada da outra conta).
    fn recomecar_codex_noutra_conta(&mut self, chat_id: &str, nova: &ContaCodex) -> Option<Entrada> {
        let arquivo = self.exportar_conversa(chat_id).ok();
        if let Some(c) = self.chat_mut(chat_id) {
            c.sessao = uuid::Uuid::new_v4().to_string();
            c.sessao_criada = false;
            c.contexto = None;
        }
        self.avisar(
            chat_id,
            &format!("O Codex deste chat passa à conta {}, numa sessão nova: a conversa até aqui vai junto com a próxima mensagem.", nova.nome),
            false,
        );
        let arquivo = arquivo?;
        let texto = format!(
            "[Vitral] Esta conversa continua numa sessão nova, na conta {} do Codex. O que aconteceu até aqui está em {}: \
             leia antes de responder e confira os arquivos e o git diff antes de continuar.",
            nova.nome,
            arquivo.display()
        );
        Some(Entrada { de: Some("Vitral".into()), texto, registrada: true, nota: true, ..Entrada::default() })
    }

    /// O `/conta` num chat do Codex: sem nome, as contas e a em uso; com nome (ou o começo dele) ou
    /// "padrão", troca a deste chat.
    pub(super) fn comando_conta_codex(&mut self, chat_id: &str, argumento: &str, cx: &mut Context<Self>) {
        let Some(chat) = self.chat_gravado(chat_id) else {
            return;
        };
        let alvo = crate::ui::acentos_normalizados(argumento);
        if alvo.is_empty() {
            let usada = self.conta_codex_para(chat).id;
            let linhas: Vec<String> = self
                .contas_codex
                .iter()
                .map(|c| format!("{}{}: {}", c.nome, if c.id == usada { " (em uso)" } else { "" }, if c.logada() { "com login" } else { "sem login" }))
                .collect();
            self.avisar(chat_id, &format!("Contas do Codex: {}. Troque com /conta <nome> ou /conta padrão.", linhas.join("; ")), false);
            cx.notify();
            return;
        }
        let nova = if ["padrao", "default"].contains(&alvo.as_str()) {
            None
        } else {
            let achada = self
                .contas_codex
                .iter()
                .find(|c| crate::ui::acentos_normalizados(&c.nome) == alvo)
                .or_else(|| self.contas_codex.iter().find(|c| crate::ui::acentos_normalizados(&c.nome).starts_with(&alvo)));
            match achada {
                Some(c) => Some(c.id.clone()),
                None => {
                    let nomes: Vec<&str> = self.contas_codex.iter().map(|c| c.nome.as_str()).collect();
                    self.avisar(chat_id, &format!("Não há conta do Codex \u{201c}{argumento}\u{201d}. As contas: {}.", nomes.join(", ")), true);
                    cx.notify();
                    return;
                }
            }
        };
        self.definir_conta_codex(chat_id, nova, cx);
    }

    /// "Adicionar conta" do Codex: cria a pasta e abre o login dela no terminal.
    pub fn adicionar_conta_codex(&mut self, cx: &mut Context<Self>) -> Result<ContaCodex, String> {
        let conta = contas_codex::nova(&self.contas_codex).map_err(|e| format!("{e:#}"))?;
        self.contas_codex.push(conta.clone());
        contas_codex::salvar(&self.contas_codex).map_err(|e| format!("{e:#}"))?;
        self.entrar_conta_codex(&conta.id);
        cx.notify();
        Ok(conta)
    }

    /// Abre o terminal com o `codex login` da conta (a pasta é refeita antes, se faltar algum link).
    pub fn entrar_conta_codex(&self, id: &str) {
        let Some(conta) = self.contas_codex.iter().find(|c| c.id == id) else {
            return;
        };
        if conta.id != contas_codex::PRINCIPAL && contas_codex::preparar_pasta(&conta.id).is_err() {
            return;
        }
        if let Some(casa) = dirs::home_dir() {
            let comando = contas_codex::comando_de_login(conta);
            if cfg!(windows) {
                crate::sistema::abrir_powershell(&casa, &comando);
            } else {
                crate::sistema::abrir_terminal(&casa, Some(&comando));
            }
        }
    }

    /// Tira uma conta extra do Codex: o cadastro e a pasta (o login dela some junto). Os chats
    /// que usavam a conta voltam à padrão.
    pub fn remover_conta_codex(&mut self, id: &str, cx: &mut Context<Self>) {
        if id == contas_codex::PRINCIPAL {
            return;
        }
        self.contas_codex.retain(|c| c.id != id);
        let _ = contas_codex::salvar(&self.contas_codex);
        contas_codex::apagar_pasta(id);
        for c in self.chats.iter_mut().filter(|c| c.conta_codex.as_deref() == Some(id)) {
            c.conta_codex = None;
        }
        if self.config.conta_codex == id {
            self.mudar_config(|c| c.conta_codex = String::new(), cx);
        }
        self.salvar();
        cx.notify();
    }

    /// Depois de um erro de limite num turno do Codex: passa o chat para outra conta do Codex com
    /// login e repete o turno nela, numa sessão nova (com a conversa junto). Só com a opção
    /// `trocar_conta_no_limite` e mais de uma conta logada. Diz se trocou.
    pub(super) fn trocar_conta_codex_no_limite(&mut self, chat_id: &str, erro: &str, cx: &mut Context<Self>) -> bool {
        let Some(chat) = self.chat_mut(chat_id).map(Chat::copia_sem_itens) else {
            return false;
        };
        let texto = erro.to_lowercase();
        let limite = texto.contains("usage limit") || texto.contains("usage_limit") || texto.contains("limite de uso") || texto.contains("out of credits");
        if chat.agente != Agente::Codex || !limite || !self.config.trocar_conta_no_limite {
            return false;
        }
        let atual = self.conta_codex_para(&chat);
        let Some(outra) = self.contas_codex.iter().find(|c| c.id != atual.id && c.logada()).cloned() else {
            return false;
        };
        if let Some(c) = self.chat_mut(chat_id) {
            c.conta_codex = Some(outra.id.clone());
        }
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        self.avisar(chat_id, &format!("A conta {} do Codex chegou ao limite do plano.", atual.nome), false);
        let nota = self.recomecar_codex_noutra_conta(chat_id, &outra);
        // O turno roda de novo na outra conta, com a conversa até aqui na frente.
        if let Some(mut mensagens) = self.entrada_para_repetir(chat_id) {
            if let Some(nota) = nota {
                mensagens.insert(0, nota);
            }
            self.comecar_turno_com(chat_id, mensagens, false, cx);
        }
        self.salvar();
        true
    }
}
