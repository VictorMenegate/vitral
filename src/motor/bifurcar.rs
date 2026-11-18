//! "Continuar daqui em outro chat": um chat novo na mesma pasta, com a conversa até um ponto, para
//! testar outro caminho sem mexer no original. Os itens até o ponto são copiados, e o agente do
//! chat novo continua com o que viu até ali:
//!
//! - no Claude, com sessão gravada: o primeiro turno sai numa cópia da sessão de origem
//!   (`--resume <origem> --fork-session --session-id <nova>`, o mesmo caminho das conversas
//!   trazidas), cortada no ponto com `--resume-session-at <uuid>`. O uuid é o da última entrada
//!   do agente (ou sua) antes da mensagem que abre o turno seguinte, achado no arquivo da sessão
//!   subindo a cadeia de `parentUuid` a partir da marca dessa mensagem (`ponto_de_corte`);
//! - no Codex, sem sessão, sem marca ou sem o arquivo: uma sessão nova com o bloco de passagem
//!   (as últimas mensagens da conversa copiada).
//!
//! O corte é sempre na borda de um turno: bifurcar numa resposta leva o turno dela inteiro, e
//! bifurcar numa mensagem sua leva tudo o que veio antes (o texto dela vai para a caixa do chat
//! novo, para mandar de novo ou mudar).
//!
//! A flag `--resume-session-at` existe no Claude Code 2.1.325 fora do `--help` (lida no binário:
//! "only messages up to and including the chain entry with <message.id>", só com `--resume` no
//! modo print). Se o Claude Code a recusar ("No message found with message.uuid of"), o turno é
//! repetido numa sessão nova com o bloco de passagem (`consertar_bifurcacao`).

use std::io::BufRead as _;

use serde::Deserialize;

use super::*;
use crate::modelo::Bifurcacao;

/// O que o Claude Code diz quando o uuid do corte não está na sessão retomada.
const SEM_O_PONTO: &str = "No message found with message.uuid of";
/// E quando a sessão de origem não existe (mais).
const SEM_A_SESSAO: &str = "No conversation found with session ID";

/// O chat que nasceu de uma bifurcação, para a janela abrir.
#[derive(Clone, Debug, PartialEq)]
pub struct Bifurcado {
    pub chat: String,
    /// A sua mensagem do ponto (o texto e os anexos), quando a bifurcação foi nela: vai para a
    /// caixa do chat novo.
    pub mensagem: Option<(String, Vec<PathBuf>)>,
    /// O agente continua com a sessão copiada (`false`: recebe as últimas mensagens como resumo).
    pub com_sessao: bool,
}

/// Onde a conversa copiada termina (o índice do primeiro item que fica de fora) para uma
/// bifurcação pedida no item `ix`: numa mensagem sua, antes dela; numa resposta, no fim do turno
/// dela (antes da próxima mensagem sua ou entregue). `None` quando o item não é um nem outro.
pub fn fim_da_copia(itens: &[Item], ix: usize) -> Option<usize> {
    match itens.get(ix)? {
        Item::Usuario { .. } => Some(ix),
        Item::Assistente { .. } => Some(
            itens[ix + 1..]
                .iter()
                .position(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }))
                .map_or(itens.len(), |k| ix + 1 + k),
        ),
        _ => None,
    }
}

/// Uma linha do arquivo da sessão do Claude Code: só o que a cadeia precisa.
#[derive(Deserialize)]
struct Entrada {
    #[serde(default)]
    uuid: Option<String>,
    #[serde(default, rename = "parentUuid")]
    pai: Option<String>,
    #[serde(default, rename = "type")]
    tipo: String,
}

/// O uuid para o `--resume-session-at` que deixa a sessão como estava antes da mensagem de uuid
/// `marca`: subindo a cadeia a partir do pai dela, a primeira entrada do agente ou do usuário (as
/// de anexo e de sistema ficam no caminho, mas a retomada pode não tê-las entre as mensagens).
/// `None` quando a marca não está no arquivo ou nada veio antes dela.
pub fn ponto_de_corte(linhas: impl Iterator<Item = String>, marca: &str) -> Option<String> {
    let mut cadeia: HashMap<String, (Option<String>, bool)> = HashMap::new();
    for linha in linhas {
        // O arquivo tem linhas enormes (os pedidos à API): só as da cadeia interessam.
        if !linha.contains("\"uuid\"") {
            continue;
        }
        let Ok(entrada) = serde_json::from_str::<Entrada>(&linha) else {
            continue;
        };
        if let Some(uuid) = entrada.uuid {
            cadeia.insert(uuid, (entrada.pai, matches!(entrada.tipo.as_str(), "assistant" | "user")));
        }
    }
    let mut atual = cadeia.get(marca)?.0.clone();
    // O teto evita um laço num arquivo com a cadeia estragada.
    for _ in 0..cadeia.len() {
        let uuid = atual?;
        let (pai, mensagem) = cadeia.get(&uuid)?;
        if *mensagem {
            return Some(uuid);
        }
        atual = pai.clone();
    }
    None
}

/// O ponto de corte lido do arquivo da sessão.
fn ponto_no_arquivo(arquivo: &Path, marca: &str) -> Option<String> {
    let leitor = std::io::BufReader::new(std::fs::File::open(arquivo).ok()?);
    ponto_de_corte(leitor.lines().map_while(Result::ok), marca)
}

impl Motor {
    /// De onde o agente do chat novo continua, quando dá para copiar a sessão: a sessão de origem
    /// e o uuid do corte (`None`: a sessão inteira, a bifurcação foi no último turno).
    fn origem_da_bifurcacao(&self, chat: &Chat, fim: usize) -> Option<Bifurcacao> {
        if chat.agente != Agente::Claude || !chat.sessao_criada {
            return None;
        }
        let pasta_conta = self.conta_para(chat).pasta();
        let arquivo = claude::arquivo_da_sessao(pasta_conta.as_deref(), &chat.cwd, &chat.sessao)?;
        if fim >= chat.itens.len() {
            return Some(Bifurcacao { sessao: chat.sessao.clone(), ate: None });
        }
        // Só a mensagem sua que abriu um turno tem marca (a dirigida ao turno e a entregue, não).
        let Item::Usuario { marca: Some(marca), .. } = &chat.itens[fim] else {
            return None;
        };
        let ate = ponto_no_arquivo(&arquivo, marca)?;
        Some(Bifurcacao { sessao: chat.sessao.clone(), ate: Some(ate) })
    }

    /// Cria o chat novo com a conversa de `chat_id` até o ponto do item `ix` (ver `fim_da_copia`).
    /// O original não muda.
    pub fn bifurcar_chat(&mut self, chat_id: &str, ix: usize, cx: &mut Context<Self>) -> Result<Bifurcado, String> {
        let origem = self.chat(chat_id).ok_or("a conversa não existe mais")?;
        let fim = fim_da_copia(&origem.itens, ix).ok_or("só dá para continuar a partir de uma mensagem sua ou de uma resposta")?;
        // O começo de uma conversa trazida do Claude Code (e já continuada aqui) vem do arquivo dela.
        let trazida = origem.importada.as_ref().is_some_and(|i| i.continuada);
        if fim == 0 && !trazida {
            return Err("não há conversa antes desta mensagem: um chat novo já começa assim".into());
        }
        // O último turno ainda roda: a sessão gravada está no meio dele.
        if fim >= origem.itens.len() && self.processos.contains_key(chat_id) {
            return Err("espere o turno terminar para continuar a partir da última resposta".into());
        }
        let bifurcacao = self.origem_da_bifurcacao(origem, fim);
        let mensagem = match &origem.itens[ix] {
            Item::Usuario { texto, anexos, .. } => Some((texto.clone(), anexos.clone())),
            _ => None,
        };
        let itens: Vec<Item> = origem.itens[..fim].to_vec();
        let (pasta, titulo) = (origem.cwd.clone(), format!("{} (ramo)", origem.titulo));
        let (agente, modelo, esforco, permissao, conta, permitidas, importada) = (
            origem.agente,
            origem.modelo.clone(),
            origem.esforco.clone(),
            origem.permissao.clone(),
            origem.conta.clone(),
            origem.permitidas.clone(),
            // O começo de uma conversa trazida do Claude Code continua vindo do arquivo dela.
            origem.importada.clone().filter(|i| i.continuada),
        );
        // Na mesma pasta, sempre (a do chat Sem pasta inclusive): a sessão copiada é achada por ela.
        let id = self.novo_chat(Some(&pasta), "", cx)?;
        let com_sessao = bifurcacao.is_some();
        if let Some(c) = self.chat_mut(&id) {
            c.titulo = titulo;
            (c.agente, c.modelo, c.esforco, c.permissao, c.conta, c.permitidas) = (agente, modelo, esforco, permissao, conta, permitidas);
            c.importada = importada;
            c.itens = itens;
            c.bifurcado_de = bifurcacao;
        }
        if !com_sessao {
            self.passagens.insert(id.clone(), MotivoPassagem::Bifurcou);
        }
        if let Some(c) = self.chat(&id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.salvar();
        cx.notify();
        Ok(Bifurcado { chat: id, mensagem, com_sessao })
    }

    /// O primeiro turno de um chat bifurcado falhou porque o Claude Code não achou o ponto do
    /// corte (ou a sessão de origem): a cópia da sessão é abandonada e o mesmo turno roda de novo
    /// numa sessão nova, com o bloco de passagem. Uma vez só; devolve `true` quando repetiu.
    pub(super) fn consertar_bifurcacao(&mut self, chat_id: &str, erro: &str, cx: &mut Context<Self>) -> bool {
        let Some(chat) = self.chat(chat_id) else {
            return false;
        };
        if chat.bifurcado_de.is_none() || chat.sessao_criada || !(erro.contains(SEM_O_PONTO) || erro.contains(SEM_A_SESSAO)) {
            return false;
        }
        let Some((_, mensagens)) = self.ultima_entrada.get(chat_id).cloned() else {
            return false;
        };
        if let Some(c) = self.chat_mut(chat_id) {
            c.bifurcado_de = None;
            (c.sessao, c.contexto) = (uuid::Uuid::new_v4().to_string(), None);
        }
        self.passagens.insert(chat_id.to_string(), MotivoPassagem::Bifurcou);
        self.avisar(chat_id, "Não deu para copiar a sessão da conversa de origem; continuei numa nova com um resumo.", false);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        self.salvar();
        self.comecar_turno_com(chat_id, mensagens, false, cx);
        true
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn usuario(texto: &str, marca: Option<&str>) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: marca.map(str::to_string) }
    }

    fn assistente(texto: &str) -> Item {
        Item::Assistente { texto: texto.into(), em: 0 }
    }

    fn ferramenta() -> Item {
        Item::Ferramenta {
            id: "t".into(),
            nome: "Bash".into(),
            resumo: "ls".into(),
            resultado: Some("ok".into()),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
        }
    }

    #[test]
    fn a_copia_termina_na_borda_de_um_turno() {
        let itens = vec![
            usuario("um", Some("m1")),
            assistente("vou olhar"),
            ferramenta(),
            assistente("resposta um"),
            usuario("dois", Some("m2")),
            assistente("resposta dois"),
            Item::Entrega { de: "Vitral".into(), texto: "revise a memória".into(), em: 0 },
            assistente("revista"),
        ];
        // Na sua mensagem: tudo o que veio antes dela.
        assert_eq!(fim_da_copia(&itens, 0), Some(0));
        assert_eq!(fim_da_copia(&itens, 4), Some(4));
        // Numa resposta: o turno dela inteiro, mesmo no texto do meio.
        assert_eq!(fim_da_copia(&itens, 1), Some(4));
        assert_eq!(fim_da_copia(&itens, 3), Some(4));
        // A segunda resposta leva as duas primeiras trocas; a entrega abre outro turno.
        assert_eq!(fim_da_copia(&itens, 5), Some(6));
        // A última resposta: a conversa inteira.
        assert_eq!(fim_da_copia(&itens, 7), Some(itens.len()));
        // Ferramenta, entrega e índice que não existe não são ponto de bifurcar.
        assert_eq!(fim_da_copia(&itens, 2), None);
        assert_eq!(fim_da_copia(&itens, 6), None);
        assert_eq!(fim_da_copia(&itens, 99), None);
    }

    /// Uma sessão de dois turnos como o Claude Code 2.1.325 grava: as entradas de anexo e de
    /// sistema entram na cadeia, e as linhas sem uuid (fila, título, pedidos à API) ficam de fora.
    fn sessao() -> Vec<String> {
        [
            r#"{"type":"queue-operation","operation":"enqueue"}"#,
            r#"{"type":"attachment","uuid":"a0","parentUuid":null,"attachment":{"type":"hook_success"}}"#,
            r#"{"type":"user","uuid":"m1","parentUuid":"a0","message":{"role":"user","content":"um"}}"#,
            r#"{"type":"assistant","uuid":"r1a","parentUuid":"m1","message":{"content":[{"type":"tool_use"}]}}"#,
            r#"{"type":"user","uuid":"t1","parentUuid":"r1a","message":{"content":[{"type":"tool_result"}]}}"#,
            r#"{"type":"assistant","uuid":"r1b","parentUuid":"t1","message":{"content":[{"type":"text"}]}}"#,
            r#"{"type":"attachment","uuid":"a1","parentUuid":"r1b","attachment":{"type":"hook_success"}}"#,
            r#"{"type":"system","subtype":"stop_hook_summary","uuid":"s1","parentUuid":"a1"}"#,
            r#"{"type":"last-prompt","leafUuid":"s1"}"#,
            r#"linha estragada"#,
            r#"{"type":"attachment","uuid":"a2","parentUuid":"s1","attachment":{"type":"hook_success"}}"#,
            r#"{"type":"user","uuid":"m2","parentUuid":"a2","message":{"role":"user","content":"dois"}}"#,
            r#"{"type":"assistant","uuid":"r2","parentUuid":"m2","message":{"content":[{"type":"text"}]}}"#,
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn o_corte_e_a_ultima_entrada_do_turno_anterior_a_marca() {
        // Antes da segunda mensagem: a resposta final do primeiro turno (os anexos e o resumo dos
        // hooks que vieram depois dela não são mensagem).
        assert_eq!(ponto_de_corte(sessao().into_iter(), "m2").as_deref(), Some("r1b"));
        // Antes da primeira não há nada.
        assert_eq!(ponto_de_corte(sessao().into_iter(), "m1"), None);
        // A marca que não está no arquivo (outra sessão, sessão compactada).
        assert_eq!(ponto_de_corte(sessao().into_iter(), "m9"), None);
        // No meio do turno, o pai já serve: a entrada do resultado da ferramenta.
        assert_eq!(ponto_de_corte(sessao().into_iter(), "r1b").as_deref(), Some("t1"));
        // Cadeia em laço não trava.
        let laco = [
            r#"{"type":"attachment","uuid":"x","parentUuid":"y"}"#,
            r#"{"type":"attachment","uuid":"y","parentUuid":"x"}"#,
            r#"{"type":"user","uuid":"m","parentUuid":"x"}"#,
        ]
        .map(String::from);
        assert_eq!(ponto_de_corte(laco.into_iter(), "m"), None);
    }
}
