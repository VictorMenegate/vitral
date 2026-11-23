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
//! Corrigir uma mensagem usa o mesmo corte (`Motor::retirar_mensagem`): em vez de jogar a sessão
//! fora e recomeçar com um resumo, o próximo turno sai numa cópia da própria sessão do chat, até
//! o ponto antes da mensagem corrigida (`Bifurcacao::correcao`). O agente fica com o contexto
//! exato, e o começo da conversa é o mesmo que o Claude Code já tinha guardado.
//!
//! A flag `--resume-session-at` existe no Claude Code 2.1.325 fora do `--help` ("only messages up
//! to and including the chain entry with <message.id>", só com `--resume` no modo print). Medido
//! em 18/11/2026 numa sessão de verdade, abrindo o processo sem mandar mensagem (não gasta o
//! plano): ela aceita o uuid de qualquer entrada da cadeia; a sessão inteira tinha 40.388 tokens
//! de mensagens e, cortada na primeira mensagem, 39 (`get_context_usage`); um uuid que não está
//! na sessão sai com "No message found with message.uuid of". Nesse caso o turno é repetido numa
//! sessão nova com o bloco de passagem (`consertar_bifurcacao`).

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

/// O que a correção de uma mensagem faz com a sessão do chat (os itens já saíram da conversa).
/// Com `volta` (a sessão até o ponto antes da mensagem), o próximo turno sai numa cópia dela, sem
/// resumo; sem, a sessão é trocada por uma nova. Devolve `true` quando o próximo turno precisa do
/// bloco de passagem. O chat que nunca teve sessão (nem espera uma cópia) não muda.
pub(super) fn sessao_depois_da_correcao(chat: &mut Chat, volta: Option<Bifurcacao>) -> bool {
    if !chat.sessao_criada && chat.bifurcado_de.is_none() {
        return false;
    }
    // A sessão nova nasce no próximo turno: a de agora fica para trás (é dela que sai a cópia).
    if chat.sessao_criada {
        let visto = custo_visto(chat);
        chat.sessao = uuid::Uuid::new_v4().to_string();
        chat.sessao_criada = false;
        chat.custo_da_sessao = visto.map(|v| (chat.sessao.clone(), v));
    }
    chat.contexto = None;
    let passagem = volta.is_none();
    // A sessão vazia da passagem conta o custo do zero.
    if passagem {
        chat.custo_da_sessao = None;
    }
    chat.bifurcado_de = volta;
    passagem
}

/// O último total de custo que o Vitral viu na sessão do chat. A cópia de uma sessão começa desse
/// total, e não do zero: o Claude Code restaura nela o custo acumulado da sessão de origem (o
/// `cost-state` do arquivo dela), e o `total_cost_usd` dos resultados seguintes continua dele. Quem
/// abre uma cópia leva o visto junto (`Chat::somar_custo` soma só o que crescer), senão o custo da
/// sessão de origem entraria de novo.
fn custo_visto(chat: &Chat) -> Option<f64> {
    chat.custo_da_sessao.as_ref().filter(|(sessao, _)| *sessao == chat.sessao).map(|(_, visto)| *visto)
}

/// O ponto de corte lido do arquivo da sessão.
fn ponto_no_arquivo(arquivo: &Path, marca: &str) -> Option<String> {
    let leitor = std::io::BufReader::new(std::fs::File::open(arquivo).ok()?);
    ponto_de_corte(leitor.lines().map_while(Result::ok), marca)
}

impl Motor {
    /// De onde o agente continua com a conversa até o item `fim` (fora), quando dá para copiar a
    /// sessão: a sessão que guarda o que ele viu e o uuid do corte (`None`: ela inteira, o ponto
    /// é o fim da conversa). A sessão é a do chat ou, antes do primeiro turno de um chat que já
    /// espera uma cópia (bifurcado ou corrigido há pouco), a de origem dessa cópia.
    fn origem_da_bifurcacao(&self, chat: &Chat, fim: usize) -> Option<Bifurcacao> {
        if chat.agente != Agente::Claude {
            return None;
        }
        let (sessao, ate_de_agora) = if chat.sessao_criada {
            (chat.sessao.clone(), None)
        } else {
            let pendente = chat.bifurcado_de.as_ref()?;
            (pendente.sessao.clone(), pendente.ate.clone())
        };
        let pasta_conta = self.conta_para(chat).pasta();
        let arquivo = claude::arquivo_da_sessao(pasta_conta.as_deref(), &chat.cwd, &sessao)?;
        if fim >= chat.itens.len() {
            return Some(Bifurcacao { sessao, ate: ate_de_agora, correcao: false });
        }
        // Só a mensagem sua que abriu um turno tem marca (a dirigida ao turno e a entregue, não).
        let Item::Usuario { marca: Some(marca), .. } = &chat.itens[fim] else {
            return None;
        };
        let ate = ponto_no_arquivo(&arquivo, marca)?;
        Some(Bifurcacao { sessao, ate: Some(ate), correcao: false })
    }

    /// Corrigir a mensagem do item `ix` deixa o agente na mesma sessão, de volta ao ponto antes
    /// dela (`Some`), ou só dá para recomeçar numa sessão nova com o bloco de passagem (`None`:
    /// Codex, conversa de antes das marcas, a primeira mensagem da sessão, arquivo que sumiu).
    pub(super) fn sessao_antes_da_mensagem(&self, chat_id: &str, ix: usize) -> Option<Bifurcacao> {
        let chat = self.chat_gravado(chat_id)?;
        (ix < chat.itens.len()).then(|| self.origem_da_bifurcacao(chat, ix)).flatten().map(|b| Bifurcacao { correcao: true, ..b })
    }

    /// A pergunta da correção quer saber se o agente continua na mesma sessão (sem resumo).
    pub fn correcao_mantem_a_sessao(&self, chat_id: &str, ix: usize) -> bool {
        self.sessao_antes_da_mensagem(chat_id, ix).is_some()
    }

    /// Cria o chat novo com a conversa de `chat_id` até o ponto do item `ix` (ver `fim_da_copia`).
    /// O original não muda.
    pub fn bifurcar_chat(&mut self, chat_id: &str, ix: usize, cx: &mut Context<Self>) -> Result<Bifurcado, String> {
        let origem = self.chat_gravado(chat_id).ok_or("a conversa não existe mais")?;
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
        let visto = custo_visto(origem);
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
            // O chat novo não paga de novo o que a sessão de origem já custou (`custo_visto`).
            if com_sessao {
                c.custo_da_sessao = visto.map(|v| (c.sessao.clone(), v));
            }
            c.bifurcado_de = bifurcacao;
        }
        if !com_sessao {
            self.passagens.insert(id.clone(), MotivoPassagem::Bifurcou);
        }
        if let Some(c) = self.chat_gravado(&id) {
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
        let Some(chat) = self.chat_gravado(chat_id) else {
            return false;
        };
        if chat.bifurcado_de.is_none() || chat.sessao_criada || !(erro.contains(SEM_O_PONTO) || erro.contains(SEM_A_SESSAO)) {
            return false;
        }
        let Some((_, mensagens)) = self.ultima_entrada.get(chat_id).cloned() else {
            return false;
        };
        let correcao = chat.bifurcado_de.as_ref().is_some_and(|b| b.correcao);
        if let Some(c) = self.chat_mut(chat_id) {
            c.bifurcado_de = None;
            (c.sessao, c.contexto) = (uuid::Uuid::new_v4().to_string(), None);
        }
        // A correção cai no caminho de antes dela: sessão nova, com o que ficou na tela.
        let (motivo, aviso) = if correcao {
            (MotivoPassagem::Reenvio, "Não deu para voltar a sessão ao ponto antes da mensagem corrigida; continuei numa nova com um resumo.")
        } else {
            (MotivoPassagem::Bifurcou, "Não deu para copiar a sessão da conversa de origem; continuei numa nova com um resumo.")
        };
        self.passagens.insert(chat_id.to_string(), motivo);
        self.avisar(chat_id, aviso, false);
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
            durou: 0,
        }
    }

    #[test]
    fn a_correcao_volta_a_sessao_ao_ponto_ou_recomeca_com_a_passagem() {
        let com_sessao = || {
            let mut chat = Chat::novo("c".into(), "t", PathBuf::from("/p/loja"));
            (chat.sessao, chat.sessao_criada, chat.contexto) = ("s1".into(), true, Some(50_000));
            chat
        };
        // Com o ponto achado na sessão: o próximo turno sai numa cópia dela até ali, sem passagem.
        let mut chat = com_sessao();
        (chat.custo_usd, chat.custo_da_sessao) = (0.5, Some(("s1".into(), 0.5)));
        let volta = Bifurcacao { sessao: "s1".into(), ate: Some("r2".into()), correcao: true };
        assert!(!sessao_depois_da_correcao(&mut chat, Some(volta.clone())));
        assert_eq!(chat.bifurcado_de, Some(volta));
        assert!(!chat.sessao_criada && chat.sessao != "s1", "a sessão nova nasce no próximo turno");
        assert_eq!(chat.contexto, None);
        // A cópia traz o custo acumulado da sessão de origem: só o que crescer entra na conta.
        chat.somar_custo(0.6);
        assert!((chat.custo_usd - 0.6).abs() < 1e-9, "{}", chat.custo_usd);
        // Corrigir de novo, antes de mandar: a cópia continua saindo da sessão de origem, mais cedo.
        let mais_cedo = Bifurcacao { sessao: "s1".into(), ate: Some("r1".into()), correcao: true };
        let sessao_nova = chat.sessao.clone();
        assert!(!sessao_depois_da_correcao(&mut chat, Some(mais_cedo.clone())));
        assert_eq!((chat.bifurcado_de.as_ref(), chat.sessao.as_str()), (Some(&mais_cedo), sessao_nova.as_str()));
        // E se desta vez o ponto não foi achado, a cópia é abandonada e vai a passagem.
        assert!(sessao_depois_da_correcao(&mut chat, None));
        assert_eq!(chat.bifurcado_de, None);
        // Sem o ponto (Codex, conversa antiga, primeira mensagem): o caminho de antes, sessão nova
        // com o bloco de passagem, que conta o custo do zero.
        let mut chat = com_sessao();
        (chat.custo_usd, chat.custo_da_sessao) = (0.5, Some(("s1".into(), 0.5)));
        assert!(sessao_depois_da_correcao(&mut chat, None));
        assert!(!chat.sessao_criada && chat.sessao != "s1" && chat.bifurcado_de.is_none());
        chat.somar_custo(0.7);
        assert!((chat.custo_usd - 1.2).abs() < 1e-9, "{}", chat.custo_usd);
        // O chat que ainda não tem sessão não tem o que trocar nem o que resumir.
        let mut novo = Chat::novo("n".into(), "t", PathBuf::from("/p/loja"));
        let sessao = novo.sessao.clone();
        assert!(!sessao_depois_da_correcao(&mut novo, None));
        assert_eq!((novo.sessao.as_str(), novo.sessao_criada), (sessao.as_str(), false));
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
