//! Os tipos do que vai para o disco: projetos, chats, tarefas, cartões e configuração. Onde e
//! como cada um é gravado fica em `dados.rs`.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

/// O formato dos dados no disco. Sobe quando uma migração muda o formato; o atualizador só volta
/// para uma versão que leia o formato que está no disco (`dados::formato_em`). O 1 é o Vitral antigo
/// (o `.vitral/` em cada pasta de trabalho, que a migração não muda), o 2 os dados globais com
/// projetos e tarefas e o 3 só os chats, cada um na pasta dele. O `pacote/manifesto.py` lê esta
/// linha.
pub const FORMATO_DADOS: u32 = 3;

pub fn agora() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn novo_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_string()
}

/// O chat único do Mestre das versões com ele (só para a carga tirá-lo da lista).
pub const ID_MESTRE: &str = "mestre";

/// O papel de um chat nas versões com o Mestre, os coordenadores e os ajudantes: só lido, para a
/// carga tirar da lista quem não era conversa do usuário. Os chats de agora não têm papel.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
pub enum PapelAntigo {
    /// O chat de projeto ou avulso (e todo chat gravado sem papel).
    #[default]
    Projeto,
    /// A conversa do Vitral antigo, que distribuía tarefas.
    Coordenador,
    /// O ajudante de um chat de projeto, ou o trabalhador de uma tarefa do coordenador.
    Trabalhador,
    Mestre,
    /// Um papel que esta versão não conhece: conta como chat.
    #[serde(other)]
    Outro,
}

/// Estado de execução de um chat. Não vai para o disco: ao abrir, todos estão parados.
#[derive(Clone, Debug, PartialEq)]
pub enum Execucao {
    Parado,
    Rodando {
        desde: u64,
    },
    /// Pedido de permissão esperando o usuário.
    EsperandoVoce,
    Erro(String),
}

impl Execucao {
    pub fn rodando(&self) -> bool {
        matches!(self, Execucao::Rodando { .. } | Execucao::EsperandoVoce)
    }
}

/// O item não tem hora: o `em` de uma conversa de antes dela. Não vai para o disco (os arquivos
/// das conversas antigas continuam iguais).
fn sem_hora(em: &u64) -> bool {
    *em == 0
}

/// O que um subagente fez dentro da chamada dele: quantas ferramentas usou, o que faz agora (a
/// última ferramenta ou o que ele disse), os últimos passos e quando terminou (0: ainda não).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Subagente {
    pub ferramentas: u32,
    pub atividade: String,
    pub passos: Vec<String>,
    pub fim: u64,
}

/// Quantos passos de um subagente ficam guardados (os últimos).
pub const PASSOS_DO_SUBAGENTE: usize = 60;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Item {
    Usuario {
        texto: String,
        #[serde(default)]
        anexos: Vec<PathBuf>,
        /// Foi com Ultrathink: o histórico mostra o selo, não a palavra.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ultrathink: bool,
        /// Foi com Ultracode (fluxo de agentes): o histórico mostra o selo.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ultracode: bool,
        /// Quando entrou na conversa (Unix, segundos); 0 nas conversas de antes da hora nas
        /// mensagens, que não mostram nada.
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    /// Mensagem que o Vitral (ou outro chat) entregou a este chat, no lugar do usuário.
    Entrega {
        de: String,
        texto: String,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    Assistente {
        texto: String,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    Ferramenta {
        id: String,
        nome: String,
        resumo: String,
        resultado: Option<String>,
        erro: bool,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
        /// O que a chamada pediu, para ler ao abrir a linha: o diff de um Edit ("- " e "+ "), o
        /// começo do que um Write gravou, o comando inteiro de um Bash. As chamadas de antes
        /// disto não têm.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detalhe: Option<String>,
        /// O arquivo que a chamada leu ou mudou (o caminho como ela o deu), para abrir pelo clique.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        arquivo: Option<String>,
        /// Quando a chamada é um subagente (Task/Agent): o que ele fez lá dentro.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        sub: Option<Subagente>,
    },
    /// Um bloco de raciocínio do modelo: o texto (pode vir vazio, quando o Claude Code não o
    /// entrega) e quanto durou, em milissegundos (0: não medido).
    Raciocinio {
        texto: String,
        #[serde(default)]
        milis: u64,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    Aviso {
        texto: String,
        erro: bool,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    /// Cartão interativo; os dados ficam em `Estado.cartoes`.
    Cartao {
        id: u64,
        #[serde(default, skip_serializing_if = "sem_hora")]
        em: u64,
    },
    /// Item de uma versão mais nova, guardado e regravado como veio. Sempre a última variante.
    #[serde(untagged)]
    Desconhecido(serde_json::Value),
}

impl Item {
    /// O instante do item, ou 0 quando ele não tem hora (conversa de antes dela, ou item de uma
    /// versão mais nova).
    pub fn em(&self) -> u64 {
        match self {
            Item::Usuario { em, .. }
            | Item::Entrega { em, .. }
            | Item::Assistente { em, .. }
            | Item::Ferramenta { em, .. }
            | Item::Raciocinio { em, .. }
            | Item::Aviso { em, .. }
            | Item::Cartao { em, .. } => *em,
            Item::Desconhecido(_) => 0,
        }
    }
}

/// Quem responde num chat: o Claude Code ou o Codex (CLI da OpenAI).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Agente {
    #[default]
    Claude,
    Codex,
}

impl Agente {
    /// O nome de mostrar do programa que roda os turnos.
    pub fn programa(self) -> &'static str {
        match self {
            Agente::Claude => "Claude Code",
            Agente::Codex => "Codex",
        }
    }
}

fn eh_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub titulo: String,
    /// O projeto do chat nas versões com projetos: só lido, para a carga herdar o que era dele.
    #[serde(rename = "projeto", default, skip_serializing)]
    pub projeto_antigo: String,
    /// O papel nas versões com o Mestre e os ajudantes: só lido (ver `PapelAntigo`).
    #[serde(rename = "papel", default, skip_serializing)]
    pub papel_antigo: PapelAntigo,
    /// O chat que criou este ajudante (versões com ajudantes): só lido, para a carga tirá-lo da lista.
    #[serde(default, skip_serializing)]
    pub pai: Option<String>,
    /// A tarefa do chat nas versões com cópias: só lida, para a carga achar a pasta de origem.
    #[serde(default, skip_serializing)]
    pub tarefa: Option<u64>,
    /// Quem responde neste chat. Trocar começa uma sessão nova no outro, com as últimas mensagens.
    #[serde(default)]
    pub agente: Agente,
    /// A sessão do Claude Code, ou o `thread_id` do Codex depois do primeiro turno.
    pub sessao: String,
    /// A sessão do Claude Code já existe (próximas vezes usam `--resume`).
    pub sessao_criada: bool,
    pub cwd: PathBuf,
    /// Modelo escolhido para a conversa (apelido como "opus"); `None` segue a configuração.
    pub modelo: Option<String>,
    /// Modelo que o Claude Code disse estar usando no último turno.
    #[serde(default)]
    pub modelo_atual: Option<String>,
    /// Modo de permissão escolhido para a conversa; `None` segue a configuração.
    #[serde(default)]
    pub permissao: Option<String>,
    /// Regras de "permitir sempre" desta conversa (ver `regra_de_permissao`).
    #[serde(default)]
    pub permitidas: Vec<String>,
    #[serde(default)]
    pub esforco: Option<String>,
    /// Conta do Claude (id, ou "auto"); `None` segue a configuração.
    #[serde(default)]
    pub conta: Option<String>,
    /// Tokens no contexto e tamanho da janela, do último turno.
    #[serde(default)]
    pub contexto: Option<u64>,
    #[serde(default)]
    pub janela: Option<u64>,
    /// Comandos de barra que o Claude Code aceita nesta conversa.
    #[serde(default)]
    pub comandos: Vec<String>,
    pub criado_em: u64,
    pub atualizado_em: u64,
    pub custo_usd: f64,
    /// O último `total_cost_usd` que o Claude Code deu, com a sessão a que ele se refere.
    #[serde(default)]
    pub custo_da_sessao: Option<(String, f64)>,
    #[serde(default)]
    pub fixado: bool,
    #[serde(default)]
    pub arquivado: bool,
    /// Quando o usuário viu a conversa pela última vez (para marcar o que é novo).
    #[serde(default)]
    pub lido_em: u64,
    /// Há um turno aberto (o processo começou e ainda não terminou). Ao reabrir, marca o turno
    /// que o Vitral fechado (ou uma queda de energia) cortou no meio.
    #[serde(default)]
    pub turno_aberto: bool,
    /// Modo em que a conversa estava antes de entrar em Planejar: aprovar o plano volta a ele.
    #[serde(default)]
    pub modo_antes_do_plano: Option<String>,
    /// A sessão em que o Vitral já pediu a revisão da memória (uma vez por sessão) e quando:
    /// só as edições de arquivo depois dessa hora contam para o próximo pedido.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memoria_da_sessao: Option<String>,
    #[serde(default, skip_serializing_if = "eh_zero")]
    pub memoria_pedida_em: u64,
    /// Quem criou: "" o usuário, "migracao", "importacao", "chat:<id>" (um chat abriu este).
    #[serde(default)]
    pub criado_por: String,
    /// A conversa do Claude Code que este chat continua (o histórico vem do arquivo dela).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importada: Option<crate::importar::Importada>,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
    #[serde(skip)]
    pub itens: Vec<Item>,
}

impl Chat {
    /// O construtor único dos chats novos.
    pub fn novo(id: String, titulo: &str, cwd: PathBuf) -> Chat {
        let agora = agora();
        Chat {
            id,
            titulo: titulo.to_string(),
            projeto_antigo: String::new(),
            papel_antigo: PapelAntigo::default(),
            pai: None,
            tarefa: None,
            agente: Agente::default(),
            sessao: uuid::Uuid::new_v4().to_string(),
            sessao_criada: false,
            cwd,
            modelo: None,
            modelo_atual: None,
            permissao: None,
            permitidas: Vec::new(),
            esforco: None,
            conta: None,
            contexto: None,
            janela: None,
            comandos: Vec::new(),
            criado_em: agora,
            atualizado_em: agora,
            custo_usd: 0.0,
            custo_da_sessao: None,
            fixado: false,
            arquivado: false,
            lido_em: agora,
            turno_aberto: false,
            modo_antes_do_plano: None,
            memoria_da_sessao: None,
            memoria_pedida_em: 0,
            criado_por: String::new(),
            importada: None,
            resto: Default::default(),
            itens: Vec::new(),
        }
    }

    /// Soma o custo de um `result`. O `total_cost_usd` dele é o total da sessão, não o do turno:
    /// cresce a cada `result` do mesmo processo e, numa retomada (`--resume`), continua do total
    /// que a sessão gravou. Entra só o que cresceu desde o último visto. Um total menor (a sessão
    /// sem o total gravado, depois de uma queda) conta inteiro; o zerado de um processo que caiu ao
    /// abrir não muda nada.
    pub fn somar_custo(&mut self, total: f64) {
        if !total.is_finite() || total <= 0.0 {
            return;
        }
        let visto = match &self.custo_da_sessao {
            Some((sessao, visto)) if *sessao == self.sessao => *visto,
            _ => 0.0,
        };
        self.custo_usd += if total >= visto { total - visto } else { total };
        self.custo_da_sessao = Some((self.sessao.clone(), total));
    }

    /// O pedido de permissão cabe numa regra de "permitir sempre" desta conversa.
    pub fn permite(&self, ferramenta: &str, entrada: &serde_json::Value) -> bool {
        regra_de_permissao(ferramenta, entrada, &self.cwd).is_some_and(|r| self.permitidas.contains(&r))
    }
}

/// A regra de "permitir sempre" que cobre um pedido: "edicoes" para mudar arquivos dentro da
/// pasta do chat, "bash:<comando>" para um comando exato e "ferramenta:<nome>" para as outras. O
/// plano e as edições fora da pasta não têm regra: perguntam sempre.
pub fn regra_de_permissao(
    ferramenta: &str,
    entrada: &serde_json::Value,
    pasta: &Path,
) -> Option<String> {
    match ferramenta {
        "ExitPlanMode" => None,
        "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => {
            let caminho = entrada["file_path"].as_str().or(entrada["notebook_path"].as_str());
            dentro_da_pasta(Path::new(caminho?), pasta).then(|| "edicoes".to_string())
        }
        "Bash" => {
            let comando = entrada["command"].as_str()?.trim();
            (!comando.is_empty()).then(|| format!("bash:{comando}"))
        }
        _ => Some(format!("ferramenta:{ferramenta}")),
    }
}

/// O caminho fica na pasta do chat: relativo ou abaixo dela, e sem `..` (que pode sair dela).
pub fn dentro_da_pasta(caminho: &Path, pasta: &Path) -> bool {
    !caminho.components().any(|c| c == std::path::Component::ParentDir)
        && (caminho.is_relative() || caminho.starts_with(pasta))
}

/// A tarefa de um chat como as versões com cópias e ajudantes a gravavam: só o que a carga precisa
/// para pôr o chat direto na pasta de origem. Lida do `estado.json` antigo e nunca regravada.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct TarefaAntiga {
    pub id: u64,
    /// Quem pediu (o coordenador do Vitral antigo, o Mestre, o chat do ajudante).
    pub coordenador: String,
    pub trabalhador: String,
    /// "Rodando", "EmRevisao", "Aprovada"… (o nome da variante, como era gravado).
    pub estado: String,
    pub forma: Forma,
    /// A pasta de onde a cópia saiu; na forma direta, a pasta de trabalho.
    pub origem: PathBuf,
    /// A cópia isolada (git worktree) em que o chat trabalhava.
    pub worktree: Option<PathBuf>,
}

/// Chaves do `estado.json` de versões anteriores que esta não usa mais: saem do `resto` ao
/// carregar, para não serem regravadas para sempre.
pub const CHAVES_DO_ESTADO_QUE_SAIRAM: &[&str] = &["rotinas", "cartoes", "proximo_cartao", "proxima_tarefa"];

/// O mesmo para os campos de cada chat.
pub const CHAVES_DO_CHAT_QUE_SAIRAM: &[&str] = &["isolado", "acoes_externas"];

/// O mesmo para o `config.json` (que guarda as chaves que esta versão não conhece).
pub const CHAVES_DA_CONFIG_QUE_SAIRAM: &[&str] = &[
    "conexoes",
    "esconder_botao_linear",
    "modelo_mestre",
    "modelo_coordenador",
    "comecar_sem_perguntar",
    "aprovar_sozinho",
    "max_trabalhadores",
    "max_rodadas",
    "limpar_copias_dias",
    "revisor",
    "modelo_revisor",
    "max_ajudantes",
    "fileira_de_projetos",
];

/// Formato do `estado.json` dos dados globais. 0 (ou ausente) é o do `.vitral/` de cada pasta de
/// trabalho, 2 o dos projetos e das tarefas e 3 o de agora (só os chats); um maior que este abre o
/// app só para leitura.
pub const VERSAO_ESTADO: u32 = 3;

/// Como os chats de um projeto trabalhavam nas versões com cópias (só lida das tarefas antigas).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Forma {
    /// Numa cópia (git worktree) da origem, que é uma pasta ligada com git.
    #[default]
    Copia,
    /// Direto na origem, que tem git (a pasta própria): commit no fim de cada vez, sem cópia.
    DiretaGit,
    /// Direto na origem, sem git (pasta ligada sem git, pasta dentro de outro repositório, avulso).
    Direta,
}

/// Um projeto das versões com projetos: só lido, para a carga levar aos chats dele o que era do
/// projeto (arquivado, o modelo) e achar a pasta de origem de quem trabalhava numa cópia.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct ProjetoAntigo {
    pub id: String,
    /// A primeira é a principal.
    pub pastas: Vec<PastaAntiga>,
    pub arquivado: bool,
    pub modelo: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(default)]
pub struct PastaAntiga {
    pub caminho: PathBuf,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub versao: u32,
    /// `bypassPermissions`, `auto`, `acceptEdits` ou `manual`.
    pub permissao: String,
    /// Modelo dos chats; vazio usa o padrão do Claude Code.
    #[serde(alias = "modelo_trabalhador")]
    pub modelo_chats: String,
    /// Conta padrão das conversas: id, "auto" ou vazio (principal).
    pub conta: String,
    /// Os chats também carregam os servidores MCP do Claude Code do usuário (o nome vem de quando
    /// isso valia só para os trabalhadores).
    pub mcp_trabalhadores: bool,
    /// Arquiva sozinho as conversas paradas há tantos dias (0 = nunca).
    pub arquivar_apos_dias: u32,
    /// Fora da janela por mais que isso, a volta mostra o resumo.
    pub minutos_para_resumo: u64,
    /// Os anéis de uso do plano flutuando no canto de baixo, à direita, da janela. Desligado,
    /// somem da tela (o uso continua em Contas do Claude).
    pub uso_no_canto: bool,
    /// Quanto os fundos deixam ver o que está atrás da janela: 0 opaco, 1 leve, 2 média, 3 forte
    /// (`tema::NIVEIS_DE_TRANSPARENCIA`).
    pub transparencia: u8,
    /// A pasta de trabalho, escolhida na primeira abertura (como o "Abrir pasta" do VS Code): é
    /// nela que os chats novos nascem.
    pub pasta_de_trabalho: Option<std::path::PathBuf>,
    /// Trazer para a lista as conversas do Claude Code que já existem na pasta de trabalho.
    pub importar_da_pasta: bool,
    /// Quem responde nos chats novos.
    pub agente_padrao: Agente,
    /// Ao fim de um turno que mexeu em arquivo, uma vez por sessão, o Vitral pede ao agente para
    /// rever o que vale guardar na memória e nas instruções do projeto.
    pub memoria_ao_terminar: bool,
    /// O endereço do servidor ai-memory da equipe (`memoria_equipe`); vazio é sem memória da
    /// equipe. A chave da pessoa fica fora daqui, num arquivo só dela.
    pub memoria_da_equipe: String,
}

/// Um instante Unix no fuso local (o do sistema, como o `localtime_r`); fora do alcance do chrono,
/// o próprio 1970.
pub fn no_fuso_local(segundos: i64) -> chrono::DateTime<chrono::Local> {
    chrono::DateTime::from_timestamp(segundos, 0).unwrap_or_default().with_timezone(&chrono::Local)
}

/// "12/10", no fuso local.
pub fn data_curta(segundos: u64) -> String {
    use chrono::Datelike as _;
    let local = no_fuso_local(segundos as i64);
    format!("{:02}/{:02}", local.day(), local.month())
}

/// "14:32", no fuso local: a hora de um item da conversa.
pub fn hora_local(segundos: u64) -> String {
    use chrono::Timelike as _;
    let local = no_fuso_local(segundos as i64);
    format!("{:02}:{:02}", local.hour(), local.minute())
}

/// O dia de um instante, no fuso local (ano e dia do ano): dois itens com o mesmo par ficam no
/// mesmo grupo da conversa.
pub fn dia_local(segundos: u64) -> (i32, u32) {
    use chrono::Datelike as _;
    let local = no_fuso_local(segundos as i64);
    (local.year(), local.ordinal())
}

const MESES: [&str; 12] = [
    "janeiro",
    "fevereiro",
    "março",
    "abril",
    "maio",
    "junho",
    "julho",
    "agosto",
    "setembro",
    "outubro",
    "novembro",
    "dezembro",
];

/// O nome do dia no separador de data da conversa: "Hoje", "Ontem", "12 de março" no ano de
/// `agora` e "12 de março de 2025" nos outros.
pub fn nome_do_dia(segundos: u64, agora: u64) -> String {
    use chrono::Datelike as _;
    let (dia, hoje) = (no_fuso_local(segundos as i64), no_fuso_local(agora as i64));
    let ontem = hoje.date_naive().pred_opt().unwrap_or(hoje.date_naive());
    if dia.date_naive() == hoje.date_naive() {
        "Hoje".to_string()
    } else if dia.date_naive() == ontem {
        "Ontem".to_string()
    } else {
        let mes = MESES[(dia.month() as usize - 1).min(11)];
        let texto = format!("{} de {mes}", dia.day());
        if dia.year() == hoje.year() { texto } else { format!("{texto} de {}", dia.year()) }
    }
}

/// "às 15:42", ou "em 03/11 às 15:42" quando não é hoje, no fuso local.
pub fn quando_local(segundos: u64) -> String {
    use chrono::{Datelike as _, Timelike as _};
    let (local, hoje) = (no_fuso_local(segundos as i64), no_fuso_local(agora() as i64));
    let hora = format!("às {:02}:{:02}", local.hour(), local.minute());
    if (local.year(), local.ordinal()) == (hoje.year(), hoje.ordinal()) {
        hora
    } else {
        format!("em {:02}/{:02} {hora}", local.day(), local.month())
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            versao: 0,
            permissao: "bypassPermissions".into(),
            modelo_chats: String::new(),
            conta: String::new(),
            mcp_trabalhadores: false,
            arquivar_apos_dias: 0,
            minutos_para_resumo: 15,
            uso_no_canto: true,
            transparencia: crate::tema::TRANSPARENCIA_PADRAO,
            pasta_de_trabalho: None,
            importar_da_pasta: true,
            agente_padrao: Agente::default(),
            memoria_ao_terminar: true,
            memoria_da_equipe: String::new(),
        }
    }
}

/// Uma mensagem a caminho de um chat. `de` vazio é o usuário; anexos são arquivos dele.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Entrada {
    pub de: Option<String>,
    pub texto: String,
    #[serde(default)]
    pub anexos: Vec<PathBuf>,
    /// Já está na conversa (nova tentativa que esperou vaga): não entra de novo como item.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub registrada: bool,
    /// Pensar mais neste turno: a palavra vai no fim do texto e o esforço sobe (ver
    /// `motor::esforco_do_ultrathink`). Fica na fila e sobrevive a um reinício.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ultrathink: bool,
    /// Turno com Ultracode (`--effort ultracode`): o Claude orquestra vários agentes por um
    /// fluxo. Só o usuário liga, só para esta mensagem.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub ultracode: bool,
    /// Chat que mandou; `None` é o usuário ou o Vitral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remetente: Option<String>,
    /// Não abre turno sozinha: entra junto com a próxima entrada que abre.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub nota: bool,
    /// Vai só ao Claude, sem aparecer na conversa (o que as referências da mensagem resolveram).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub extra: String,
}

impl Entrada {
    /// Escrita pelo usuário (nem o Vitral, nem outro chat).
    pub fn do_usuario(&self) -> bool {
        self.de.is_none() && self.remetente.is_none()
    }
}

/// O que a caixa ligou só para a próxima mensagem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reforco {
    pub ultrathink: bool,
    pub ultracode: bool,
    /// "Mandar depois": com o turno rodando, a mensagem espera na fila em vez de dirigir o turno.
    pub depois: bool,
}

/// Mensagens esperando o chat ficar livre.
pub type Filas = HashMap<String, Vec<Entrada>>;

#[derive(Default, Serialize, Deserialize)]
pub struct Estado {
    /// 0 no `.vitral/` de uma pasta de trabalho; `VERSAO_ESTADO` nos dados globais.
    #[serde(default)]
    pub versao: u32,
    /// Os projetos das versões com projetos: só lidos (ver `ProjetoAntigo`).
    #[serde(default, skip_serializing)]
    pub projetos: Vec<ProjetoAntigo>,
    pub chats: Vec<Chat>,
    /// As tarefas das versões com cópias: só lidas, para a carga pôr cada chat na pasta de origem.
    #[serde(default, skip_serializing)]
    pub tarefas: Vec<TarefaAntiga>,
    #[serde(default)]
    pub filas: Filas,
    #[serde(default)]
    pub ocorrencias: Vec<Ocorrencia>,
    /// Quando o usuário saiu da janela.
    #[serde(default)]
    pub visto_em: u64,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, serde_json::Value>,
}

/// O mesmo `Estado` emprestado, para gravar sem clonar os chats com todas as mensagens.
#[derive(Serialize)]
pub struct EstadoRef<'a> {
    pub versao: u32,
    pub chats: &'a [Chat],
    pub filas: &'a Filas,
    pub ocorrencias: &'a [Ocorrencia],
    pub visto_em: u64,
    #[serde(flatten)]
    pub resto: &'a serde_json::Map<String, serde_json::Value>,
}

impl Estado {
    #[cfg(test)]
    pub fn por_referencia(&self) -> EstadoRef<'_> {
        EstadoRef {
            versao: self.versao,
            chats: &self.chats,
            filas: &self.filas,
            ocorrencias: &self.ocorrencias,
            visto_em: self.visto_em,
            resto: &self.resto,
        }
    }
}

/// Até quantas ocorrências o estado guarda.
pub const MAX_OCORRENCIAS: usize = 500;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Ocorrencia {
    pub quando: u64,
    /// "terminou", "falhou", "permissao", "ciclo", "chat", "importacao" ou "migracao" (texto: uma
    /// versão nova não quebra). As versões antigas gravaram outros ("entrega", "juntou"…).
    pub tipo: String,
    /// "usuario", "vitral" ou o id de um chat.
    pub autor: String,
    pub chat: String,
    pub texto: String,
}

impl Chat {
    /// Cópia dos campos do chat com `itens` vazio, sem passar pelo histórico (que pode ter MBs).
    pub fn copia_sem_itens(&mut self) -> Chat {
        let itens = std::mem::take(&mut self.itens);
        let copia = self.clone();
        self.itens = itens;
        copia
    }
}

#[cfg(test)]
mod testes {
    use std::fs;

    use super::*;

    /// 12/03/2026 ao meio-dia UTC: ao meio-dia a data local é a mesma em qualquer fuso de verdade,
    /// e o teste não depende do fuso de quem o roda.
    const MEIO_DIA: u64 = 1_773_316_800;
    const DIA: u64 = 86_400;

    #[test]
    fn nome_do_dia_de_cada_separador() {
        assert_eq!(nome_do_dia(MEIO_DIA, MEIO_DIA), "Hoje");
        assert_eq!(nome_do_dia(MEIO_DIA - DIA, MEIO_DIA), "Ontem");
        assert_eq!(nome_do_dia(MEIO_DIA - 2 * DIA, MEIO_DIA), "10 de março");
        // Outro ano leva o ano no rótulo (o 12/03/2025 cai 365 dias antes).
        assert_eq!(nome_do_dia(MEIO_DIA - 365 * DIA, MEIO_DIA), "12 de março de 2025");
        // Depois de hoje (o relógio do sistema voltou) não é "Ontem" nem "Hoje".
        assert_eq!(nome_do_dia(MEIO_DIA + DIA, MEIO_DIA), "13 de março");
    }

    #[test]
    fn hora_local_no_formato_do_relogio() {
        let minuto = MEIO_DIA - MEIO_DIA % 60;
        let hora = hora_local(minuto);
        assert_eq!(hora.len(), 5, "{hora}");
        assert_eq!(hora.as_bytes()[2], b':', "{hora}");
        assert!(hora.bytes().enumerate().all(|(i, c)| i == 2 || c.is_ascii_digit()), "{hora}");
        // Os segundos não aparecem; o minuto seguinte muda o texto.
        assert_eq!(hora_local(minuto + 59), hora);
        assert_ne!(hora_local(minuto + 60), hora);
    }

    #[test]
    fn dia_local_agrupa_o_mesmo_dia() {
        assert_eq!(dia_local(MEIO_DIA), dia_local(MEIO_DIA + 3600));
        assert_ne!(dia_local(MEIO_DIA), dia_local(MEIO_DIA + DIA));
        // Fora do alcance do chrono, a virada de 1970 (nunca um pânico).
        assert!(dia_local(u64::MAX).0 <= 1970, "{:?}", dia_local(u64::MAX));
    }

    #[test]
    fn item_sem_hora_nao_vai_para_o_disco() {
        let sem = Item::Assistente { texto: "oi".into(), em: 0 };
        assert_eq!(serde_json::to_value(&sem).unwrap(), serde_json::json!({ "Assistente": { "texto": "oi" } }));
        let com = Item::Assistente { texto: "oi".into(), em: MEIO_DIA };
        assert_eq!(
            serde_json::to_value(&com).unwrap(),
            serde_json::json!({ "Assistente": { "texto": "oi", "em": MEIO_DIA } })
        );
        // O item de uma conversa de antes da hora abre sem ela, e não como desconhecido.
        let lido: Item = serde_json::from_value(serde_json::json!({ "Assistente": { "texto": "oi" } })).unwrap();
        assert!(matches!(&lido, Item::Assistente { texto, em: 0 } if texto == "oi"));
        assert_eq!(lido.em(), 0);
        assert_eq!(Item::Desconhecido(serde_json::json!({ "Coisa": {} })).em(), 0);
    }

    fn chat_de_teste() -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": "c1", "projeto": "loja", "titulo": "Conversa", "papel": "Trabalhador",
            "pai": "c0", "tarefa": 3, "sessao": "s", "sessao_criada": true, "cwd": "/tmp/loja",
            "modelo": "opus", "criado_em": 1, "atualizado_em": 2, "custo_usd": 0.5
        }))
        .unwrap();
        chat.itens.push(Item::Assistente { texto: "oi".into(), em: 0 });
        chat
    }

    #[test]
    fn custo_soma_so_o_que_a_sessao_cresceu() {
        let mut chat = chat_de_teste();
        let perto = |a: f64, b: f64| (a - b).abs() < 1e-9;
        chat.somar_custo(0.25);
        assert!(perto(chat.custo_usd, 0.75), "{}", chat.custo_usd);
        // A retomada continua do total gravado: 0,25 dos turnos anteriores e 0,15 deste.
        chat.somar_custo(0.4);
        assert!(perto(chat.custo_usd, 0.9), "{}", chat.custo_usd);
        // O `result` zerado de um processo que caiu ao abrir.
        chat.somar_custo(0.0);
        assert!(perto(chat.custo_usd, 0.9), "{}", chat.custo_usd);
        // Total menor: a sessão recomeçou do zero (o total não foi gravado na queda).
        chat.somar_custo(0.1);
        assert!(perto(chat.custo_usd, 1.0), "{}", chat.custo_usd);
        // Sessão nova conta do zero, mesmo com um total maior que o visto na anterior.
        chat.sessao = "s2".into();
        chat.somar_custo(0.3);
        assert!(perto(chat.custo_usd, 1.3), "{}", chat.custo_usd);
        let gravado = serde_json::to_value(&chat).unwrap();
        assert_eq!(gravado["custo_da_sessao"], serde_json::json!(["s2", 0.3]));
    }

    #[test]
    fn estado_por_referencia_grava_o_mesmo_json() {
        let chat = chat_de_teste();
        let mut filas = Filas::new();
        filas.insert("c1".into(), vec![Entrada { texto: "depois".into(), ..Default::default() }]);
        let mut resto = serde_json::Map::new();
        resto.insert("de_uma_versao_nova".into(), serde_json::json!({"x": 1}));
        let estado = Estado {
            versao: VERSAO_ESTADO,
            projetos: vec![ProjetoAntigo { id: "loja".into(), ..Default::default() }],
            chats: vec![chat],
            filas,
            ocorrencias: vec![Ocorrencia { quando: 3, tipo: "pedido".into(), ..Default::default() }],
            visto_em: 9,
            resto,
            ..Default::default()
        };
        let referencia = serde_json::to_value(estado.por_referencia()).unwrap();
        assert_eq!(referencia, serde_json::to_value(&estado).unwrap());
        // Os projetos das versões com projetos só são lidos: não voltam ao disco.
        assert!(referencia.get("projetos").is_none());
        let lido: Estado = serde_json::from_value(referencia.clone()).unwrap();
        assert_eq!(lido.chats[0].id, "c1");
        assert_eq!(lido.filas["c1"][0].texto, "depois");
        assert_eq!(lido.resto["de_uma_versao_nova"], serde_json::json!({"x": 1}));
        assert_eq!(serde_json::to_value(lido.por_referencia()).unwrap(), referencia);
    }

    #[test]
    fn desconhecido_e_resto_voltam_iguais() {
        let item = serde_json::json!({"Divisao": {"partes": [1, 2]}});
        let itens: Vec<Item> = serde_json::from_value(serde_json::json!([
            {"Assistente": {"texto": "oi"}}, item.clone(), {"Cartao": {"id": 4}}
        ]))
        .unwrap();
        assert!(matches!(&itens[1], Item::Desconhecido(v) if *v == item));
        assert!(matches!(itens[2], Item::Cartao { id: 4, .. }));
        assert_eq!(serde_json::to_value(&itens).unwrap()[1], item);

        let mut chat = serde_json::to_value(chat_de_teste()).unwrap();
        chat["campo_novo"] = serde_json::json!([1, 2]);
        let lido: Chat = serde_json::from_value(chat.clone()).unwrap();
        assert_eq!(lido.resto["campo_novo"], serde_json::json!([1, 2]));
        assert_eq!(serde_json::to_value(&lido).unwrap(), chat);

        // A tarefa das versões com cópias: só o que a migração usa, e o resto não atrapalha.
        let tarefa = serde_json::json!({
            "id": 3, "projeto": "loja", "coordenador": "", "trabalhador": "c1", "titulo": "t",
            "objetivo": "o", "criterios": [], "estado": "Conversa", "rodada": 1, "criada_em": 5,
            "forma": "DiretaGit", "origem": "/p/loja", "vindo_de_longe": true
        });
        let lida: TarefaAntiga = serde_json::from_value(tarefa).unwrap();
        assert_eq!((lida.id, lida.forma), (3, Forma::DiretaGit));
        assert_eq!((lida.trabalhador.as_str(), lida.origem.as_path()), ("c1", Path::new("/p/loja")));
    }

    #[test]
    fn entrada_antiga_le_com_os_padroes_e_grava_igual() {
        let antiga = serde_json::json!({"de": null, "texto": "oi", "anexos": []});
        let lida: Entrada = serde_json::from_value(antiga.clone()).unwrap();
        assert!(lida.remetente.is_none() && !lida.nota && lida.extra.is_empty());
        assert_eq!(serde_json::to_value(&lida).unwrap(), antiga);
    }

    #[test]
    fn projeto_e_papel_antigos_so_sao_lidos() {
        let mut antigo = serde_json::to_value(chat_de_teste()).unwrap();
        antigo["projeto"] = "loja".into();
        antigo["pai"] = "c0".into();
        antigo["tarefa"] = 7.into();
        for (papel, lido) in [
            ("Projeto", PapelAntigo::Projeto),
            ("Coordenador", PapelAntigo::Coordenador),
            ("Trabalhador", PapelAntigo::Trabalhador),
            ("Mestre", PapelAntigo::Mestre),
            ("DeUmaVersaoNova", PapelAntigo::Outro),
        ] {
            antigo["papel"] = papel.into();
            let chat: Chat = serde_json::from_value(antigo.clone()).unwrap();
            assert_eq!(chat.papel_antigo, lido, "{papel}");
            assert_eq!((chat.projeto_antigo.as_str(), chat.pai.as_deref(), chat.tarefa), ("loja", Some("c0"), Some(7)));
            let gravado = serde_json::to_value(&chat).unwrap();
            for chave in ["projeto", "papel", "pai", "tarefa"] {
                assert!(gravado.get(chave).is_none(), "{chave}");
            }
            assert!(chat.resto.is_empty(), "{:?}", chat.resto);
        }
    }

    #[test]
    fn copia_sem_itens_deixa_o_historico_no_original() {
        let mut chat = chat_de_teste();
        let copia = chat.copia_sem_itens();
        assert!(copia.itens.is_empty());
        assert_eq!(chat.itens.len(), 1);
        assert_eq!(
            serde_json::to_value(&copia).unwrap(),
            serde_json::to_value(&chat).unwrap()
        );
    }

    #[test]
    fn chat_gravado_em_segundo_plano_volta_igual() {
        let raiz = std::env::temp_dir().join(format!("vitral-gravador-{}", novo_id()));
        let dados = crate::dados::Dados::em(&raiz);
        let mut chat = chat_de_teste();
        let estado = Estado { chats: vec![chat.clone()], ..Default::default() };
        dados.salvar_estado(&estado.por_referencia()).unwrap();
        dados.salvar_chat(&chat).unwrap();
        chat.itens.push(Item::Assistente { texto: "tchau".into(), em: 0 });
        dados.salvar_chat(&chat).unwrap();
        dados.esperar_gravacoes();
        let carregado = dados.carregar().unwrap();
        assert_eq!(carregado.estado.chats[0].itens.len(), 2);
        assert!(!crate::dados::com_sufixo(&dados.arquivo_chat("c1"), ".tmp").exists());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn tarefa_antiga_sem_forma_era_copia() {
        let tarefa: TarefaAntiga = serde_json::from_value(serde_json::json!({
            "id": 5, "projeto": "p", "coordenador": "c", "trabalhador": "w", "titulo": "t",
            "objetivo": "o", "criterios": ["c"], "estado": "Rodando", "rodada": 1, "criada_em": 0,
        }))
        .unwrap();
        assert_eq!((tarefa.forma, tarefa.worktree), (Forma::Copia, None));
    }

    #[test]
    fn apagar_na_fila_tira_a_gravacao_pendente_e_o_arquivo() {
        let raiz = std::env::temp_dir().join(format!("vitral-apagar-{}", novo_id()));
        let dados = crate::dados::Dados::em(&raiz);
        let chat = chat_de_teste();
        dados.salvar_chat(&chat).unwrap();
        dados.esperar_gravacoes();
        assert!(dados.arquivo_chat("c1").exists());
        dados.salvar_chat(&chat).unwrap();
        dados.apagar_chat("c1");
        dados.apagar_chat("nunca-existiu");
        dados.esperar_gravacoes();
        assert!(!dados.arquivo_chat("c1").exists());
        assert_eq!(dados.falha_de_gravacao(), None);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn dados_das_versoes_com_o_github_continuam_abrindo() {
        // A tarefa com PR aberto abre como as outras (o estado e o PR não interessam mais).
        let estado: Estado = serde_json::from_value(serde_json::json!({
            "chats": [], "proxima_tarefa": 6, "cartoes": [{"id": 1}], "proximo_cartao": 2,
            "tarefas": [{
                "id": 5, "projeto": "a1", "coordenador": "", "trabalhador": "c", "titulo": "t", "objetivo": "o",
                "criterios": [], "estado": "NoPr", "rodada": 2, "criada_em": 0, "pr": {"numero": 12}
            }]
        }))
        .unwrap();
        assert_eq!(estado.tarefas[0].id, 5);
        // Os cartões (do PR, do Mestre) e o contador das tarefas saíram do estado: as chaves caem no
        // `resto` e saem ao carregar.
        assert!(estado.resto.contains_key("cartoes") && estado.resto.contains_key("proxima_tarefa"));
        for chave in ["cartoes", "proximo_cartao", "proxima_tarefa"] {
            assert!(CHAVES_DO_ESTADO_QUE_SAIRAM.contains(&chave), "{chave}");
        }
        // As tarefas não voltam ao disco.
        assert!(serde_json::to_value(&estado).unwrap().get("tarefas").is_none());
        // A bolha com as issues anexadas abre sem elas.
        let item: Item = serde_json::from_value(serde_json::json!({"Usuario": {"texto": "oi", "referencias": ["ENG-1"]}})).unwrap();
        assert!(matches!(&item, Item::Usuario { texto, .. } if texto == "oi"));
    }

    #[test]
    fn projeto_antigo_abre_com_o_que_a_carga_usa() {
        let estado: Estado = serde_json::from_value(serde_json::json!({
            "versao": 2, "chats": [],
            "projetos": [{
                "id": "loja", "nome": "Loja", "ordem": 3, "icone": "store", "cor": "#fff", "descricao": "d",
                "instrucoes": "i", "arquivado": true, "modelo": "sonnet", "provisorio": false,
                "pastas": [{"caminho": "/p/loja", "propria": false, "preparacao": "npm i", "copiar": [".env"]}]
            }]
        }))
        .unwrap();
        let p = &estado.projetos[0];
        assert_eq!((p.id.as_str(), p.arquivado, p.modelo.as_deref()), ("loja", true, Some("sonnet")));
        assert_eq!(p.pastas[0].caminho, Path::new("/p/loja"));
        assert!(estado.resto.is_empty(), "{:?}", estado.resto);
        assert!(serde_json::to_value(&estado).unwrap().get("projetos").is_none());
        // A configuração com a fileira dos projetos abre sem ela.
        assert!(CHAVES_DA_CONFIG_QUE_SAIRAM.contains(&"fileira_de_projetos"));
    }
}
