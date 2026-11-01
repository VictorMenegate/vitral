//! Dados do Vitral que vão para o disco: chats, tarefas e configuração.
//!
//! Tudo fica em `<pasta de trabalho>/.vitral/`:
//! - `estado.json`: chats (sem as mensagens) e tarefas;
//! - `chats/<id>.json`: mensagens de cada chat;
//! - `config.json`: preferências desta pasta;
//! - `notas/<projeto>.md`: decisões anotadas pelo coordenador;
//! - `worktrees/<projeto>/t<N>`: cópias isoladas (git worktree) das tarefas.

use std::{
    collections::{HashMap, VecDeque},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context as _, Result};
use serde::{Deserialize, Serialize};

/// O formato do que fica em `.vitral/`. Sobe quando uma migração muda o formato (guardando uma
/// cópia antes); o atualizador só volta para uma versão que leia o formato que está no disco. O
/// `pacote/manifesto.py` lê esta linha.
pub const FORMATO_DADOS: u32 = 1;

pub fn agora() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn novo_id() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..10].to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Papel {
    /// Conversa com o usuário, distribui tarefas e revisa.
    Coordenador,
    /// Executa uma tarefa do coordenador.
    Trabalhador,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Item {
    Usuario {
        texto: String,
        #[serde(default)]
        anexos: Vec<PathBuf>,
        /// Foto dos arquivos antes desta mensagem (para voltar o código a este ponto).
        #[serde(default)]
        ponto: Option<String>,
        /// Foi com Ultrathink: o histórico mostra o selo, não a palavra.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ultrathink: bool,
        /// Foi com Ultracode (fluxo de agentes): o histórico mostra o selo.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        ultracode: bool,
    },
    /// Mensagem que o Vitral (ou outro chat) entregou a este chat, no lugar do usuário.
    Entrega {
        de: String,
        texto: String,
        #[serde(default)]
        ponto: Option<String>,
    },
    Assistente {
        texto: String,
    },
    Ferramenta {
        id: String,
        nome: String,
        resumo: String,
        resultado: Option<String>,
        erro: bool,
    },
    Aviso {
        texto: String,
        erro: bool,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Chat {
    pub id: String,
    pub projeto: String,
    pub titulo: String,
    pub papel: Papel,
    /// Coordenador de quem este trabalhador recebe tarefas.
    pub pai: Option<String>,
    pub tarefa: Option<u64>,
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
    #[serde(skip)]
    pub itens: Vec<Item>,
}

impl Chat {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoTarefa {
    Rodando,
    EmRevisao,
    Ajustando,
    Aprovada,
    Falhou,
    Cancelada,
}

impl EstadoTarefa {
    pub fn nome(self) -> &'static str {
        match self {
            EstadoTarefa::Rodando => "rodando",
            EstadoTarefa::EmRevisao => "em revisão",
            EstadoTarefa::Ajustando => "ajustando",
            EstadoTarefa::Aprovada => "aprovada",
            EstadoTarefa::Falhou => "falhou",
            EstadoTarefa::Cancelada => "cancelada",
        }
    }

    pub fn ativa(self) -> bool {
        matches!(self, EstadoTarefa::Rodando | EstadoTarefa::Ajustando)
    }

    pub fn encerrada(self) -> bool {
        matches!(
            self,
            EstadoTarefa::Aprovada | EstadoTarefa::Falhou | EstadoTarefa::Cancelada
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Verificacao {
    pub comando: String,
    pub passou: bool,
    pub saida: String,
    /// Quanto levou; None nas gravadas antes de o Vitral medir.
    #[serde(default)]
    pub duracao_s: Option<u64>,
}

impl Verificacao {
    /// Quanto levou, no formato curto da interface ("12 s", "3 min 05 s").
    pub fn tempo(&self) -> Option<String> {
        self.duracao_s.map(crate::ui::componentes::duracao_curta)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Tarefa {
    pub id: u64,
    pub projeto: String,
    pub coordenador: String,
    pub trabalhador: String,
    pub titulo: String,
    pub objetivo: String,
    pub criterios: Vec<String>,
    pub verificacao: Option<String>,
    pub estado: EstadoTarefa,
    pub rodada: u32,
    pub worktree: Option<PathBuf>,
    pub branch: Option<String>,
    pub base: Option<String>,
    /// A cópia isolada (pasta, branch e pontos de volta) já foi apagada: o trabalhador não roda mais.
    #[serde(default)]
    pub copia_limpa: bool,
    pub progresso: Option<String>,
    pub relatorio: Option<String>,
    pub ultima_verificacao: Option<Verificacao>,
    pub criada_em: u64,
    /// Parecer do revisor independente sobre a última entrega.
    #[serde(default)]
    pub revisao: Option<Revisao>,
    /// Outras tarefas em andamento que mexem nos mesmos arquivos.
    #[serde(default)]
    pub sobreposicao: Vec<String>,
    /// Resultado do script de preparação da cópia (instalar dependências etc.).
    #[serde(default)]
    pub preparacao: Option<Verificacao>,
    /// Por que a tarefa parou no meio (Vitral fechado, Parar, verificação cortada); só anda de
    /// novo quando alguém manda retomar. Na verificação refeita sozinha ao abrir, fica marcada
    /// enquanto ela roda, para uma segunda queda não repetir sem ninguém pedir.
    #[serde(default)]
    pub parada: Option<String>,
    /// O relatório da última rodada ainda não chegou ao coordenador (a verificação ou o revisor
    /// ficaram no meio); guarda se o turno do trabalhador terminou com erro.
    #[serde(default)]
    pub entrega_pendente: Option<bool>,
    /// A cópia isolada está pronta e o trabalhador já pode rodar. Enquanto não está, as mensagens
    /// para ele esperam na fila (gravadas antes desta marca, as tarefas contam como prontas).
    #[serde(default = "sim")]
    pub preparada: bool,
    /// O coordenador aprovou com a verificação falhando ou problema grave do revisor: o que foi
    /// ignorado e por quê (vai também no commit).
    #[serde(default)]
    pub ressalva: Option<String>,
}

fn sim() -> bool {
    true
}

impl Tarefa {
    /// A cópia foi apagada: limpa, ou removida numa aprovação (que antes não marcava `copia_limpa`).
    pub fn copia_apagada(&self) -> bool {
        self.copia_limpa || (self.worktree.is_none() && self.branch.is_some())
    }

    /// A verificação e o revisor estão conferindo a entrega agora.
    pub fn conferindo(&self) -> bool {
        self.estado == EstadoTarefa::EmRevisao && self.progresso.is_some()
    }

    /// A conferência da entrega ficou no meio (Vitral fechado): refaz ao retomar.
    pub fn conferencia_cortada(&self) -> bool {
        self.estado == EstadoTarefa::EmRevisao && self.entrega_pendente.is_some() && !self.conferindo()
    }

    /// A verificação e o parecer guardados são desta rodada: nem o trabalhador está ajustando nem
    /// a conferência está rodando ou ficou no meio.
    pub fn resultados_valem(&self) -> bool {
        !self.estado.ativa() && !self.conferindo() && !self.conferencia_cortada()
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Problema {
    pub arquivo: String,
    pub linha: Option<u64>,
    /// "alta", "media" ou "baixa".
    pub gravidade: String,
    pub descricao: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Revisao {
    /// "aprovar" ou "ajustar".
    pub veredito: String,
    pub resumo: String,
    pub problemas: Vec<Problema>,
}

/// Configuração de um projeto da pasta de trabalho.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigProjeto {
    /// Comando rodado em cada cópia nova antes do trabalhador começar (ex.: npm install).
    pub preparacao: String,
    /// Comando que sobe o projeto para testar (botão "Rodar").
    pub rodar: String,
    /// Arquivos fora do git copiados para cada cópia (padrões; o padrão é ".env*").
    pub copiar: Vec<String>,
    /// Ícone do atalho no trilho (nome do Lucide); vazio é palpite pelo conteúdo da pasta.
    pub icone: String,
    /// Cor do atalho ("#rrggbb"); vazio é escolhida pelo nome.
    pub cor: String,
    /// Fora do trilho, dos filtros, do "Nova conversa" e da lista (as conversas dele vão para
    /// Arquivadas); não roda rotinas.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub arquivado: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// `bypassPermissions`, `auto`, `acceptEdits` ou `manual`.
    pub permissao: String,
    /// Modelo dos coordenadores; vazio usa o padrão do Claude Code.
    pub modelo_coordenador: String,
    pub modelo_trabalhador: String,
    pub max_trabalhadores: usize,
    pub max_rodadas: u32,
    /// Apaga sozinho, ao abrir, as cópias das tarefas encerradas paradas há mais dias que isso; 0 é nunca.
    pub limpar_copias_dias: u32,
    /// Conta padrão das conversas desta pasta: id, "auto" ou vazio (principal).
    pub conta: String,
    /// Um revisor novo, só de leitura, revisa cada entrega antes do coordenador.
    pub revisor: bool,
    /// Modelo do revisor; vazio usa o dos trabalhadores.
    pub modelo_revisor: String,
    /// Trabalhadores também carregam os servidores MCP do Claude Code do usuário.
    pub mcp_trabalhadores: bool,
    pub projetos: HashMap<String, ConfigProjeto>,
    pub rotinas: Vec<Rotina>,
    /// Arquiva sozinho as conversas paradas há tantos dias (0 = nunca).
    pub arquivar_apos_dias: u32,
}

/// Pedido que o Vitral manda sozinho para um coordenador novo, num horário (com o Vitral aberto).
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Rotina {
    pub id: String,
    pub projeto: String,
    pub texto: String,
    /// Minutos desde a meia-noite.
    pub hora: u32,
    /// Dias da semana (0 = domingo).
    pub dias: Vec<u8>,
    pub ativa: bool,
    /// Data (AAAA-MM-DD) da última vez que rodou.
    pub ultima: String,
}

/// Dia da semana (0 = domingo), minutos desde a meia-noite e a data AAAA-MM-DD, no fuso local.
pub fn agora_local() -> (u8, u32, String) {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let segundos = agora() as libc::time_t;
    unsafe {
        libc::localtime_r(&segundos, &mut tm);
    }
    (
        tm.tm_wday as u8,
        (tm.tm_hour * 60 + tm.tm_min) as u32,
        format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday),
    )
}

/// "às 15:42", ou "em 03/11 às 15:42" quando não é hoje, no fuso local.
pub fn quando_local(segundos: u64) -> String {
    let local = |s: u64| {
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        let s = s as libc::time_t;
        unsafe {
            libc::localtime_r(&s, &mut tm);
        }
        tm
    };
    let (tm, hoje) = (local(segundos), local(agora()));
    let hora = format!("às {:02}:{:02}", tm.tm_hour, tm.tm_min);
    if (tm.tm_year, tm.tm_yday) == (hoje.tm_year, hoje.tm_yday) {
        hora
    } else {
        format!("em {:02}/{:02} {hora}", tm.tm_mday, tm.tm_mon + 1)
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            permissao: "bypassPermissions".into(),
            modelo_coordenador: String::new(),
            modelo_trabalhador: String::new(),
            max_trabalhadores: 4,
            max_rodadas: 3,
            limpar_copias_dias: 0,
            conta: String::new(),
            revisor: true,
            modelo_revisor: String::new(),
            mcp_trabalhadores: false,
            projetos: HashMap::new(),
            rotinas: Vec::new(),
            arquivar_apos_dias: 0,
        }
    }
}

impl Config {
    pub fn projeto_arquivado(&self, projeto: &str) -> bool {
        self.projetos.get(projeto).is_some_and(|p| p.arquivado)
    }

    /// A conversa fica fora da lista principal: arquivada ela mesma ou com o projeto.
    pub fn conversa_arquivada(&self, c: &Chat) -> bool {
        c.arquivado || self.projeto_arquivado(&c.projeto)
    }

    /// Os projetos à vista e os arquivados, cada grupo na ordem de `nomes`.
    pub fn separar_arquivados(&self, nomes: Vec<String>) -> (Vec<String>, Vec<String>) {
        nomes.into_iter().partition(|p| !self.projeto_arquivado(p))
    }
}

/// Uma mensagem a caminho de um chat. `de` vazio é o usuário; anexos são arquivos dele.
#[derive(Clone, Debug, Serialize, Deserialize)]
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
}

/// O que a caixa ligou só para a próxima mensagem.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reforco {
    pub ultrathink: bool,
    pub ultracode: bool,
}

/// Mensagens esperando o chat ficar livre.
pub type Filas = HashMap<String, Vec<Entrada>>;

#[derive(Default, Serialize, Deserialize)]
pub struct Estado {
    pub chats: Vec<Chat>,
    pub tarefas: Vec<Tarefa>,
    pub proxima_tarefa: u64,
    #[serde(default)]
    pub filas: Filas,
}

/// O mesmo `Estado` emprestado, para gravar sem clonar os chats com todas as mensagens.
#[derive(Serialize)]
pub struct EstadoRef<'a> {
    pub chats: &'a [Chat],
    pub tarefas: &'a [Tarefa],
    pub proxima_tarefa: u64,
    pub filas: &'a Filas,
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

/// Leitura e escrita da pasta `.vitral`.
pub struct Disco {
    /// Projetos e notas lidos há pouco: a interface pergunta por eles a cada quadro.
    lembrado: std::cell::RefCell<Lembrado>,
    pub raiz: PathBuf,
    gravador: Gravador,
}

/// Escreve as mensagens dos chats, o estado e a configuração numa thread só, fora da janela: o
/// fsync de conversas de alguns MB a cada poucos segundos engasgava a interface. A ordem fica (um arquivo por vez) e um arquivo
/// que ainda espera a vez só troca de conteúdo.
struct Gravador {
    fila: Arc<(Mutex<FilaDeGravacao>, Condvar)>,
}

#[derive(Default)]
struct FilaDeGravacao {
    /// O conteúdo de cada arquivo, ou `None` para apagá-lo.
    pendentes: VecDeque<(PathBuf, Option<Vec<u8>>)>,
    gravando: bool,
    fechada: bool,
}

impl Gravador {
    fn new() -> Self {
        let fila: Arc<(Mutex<FilaDeGravacao>, Condvar)> = Default::default();
        let dela = fila.clone();
        std::thread::spawn(move || {
            let (trava, sinal) = &*dela;
            loop {
                let (caminho, bytes) = {
                    let mut f = trava.lock().unwrap_or_else(|e| e.into_inner());
                    loop {
                        if let Some(proxima) = f.pendentes.pop_front() {
                            f.gravando = true;
                            break proxima;
                        }
                        if f.fechada {
                            return;
                        }
                        f = sinal.wait(f).unwrap_or_else(|e| e.into_inner());
                    }
                };
                match bytes {
                    Some(bytes) => {
                        if let Err(erro) = gravar_bytes(&caminho, &bytes) {
                            eprintln!("vitral: não salvou {}: {erro:#}", caminho.display());
                        }
                    }
                    None => match fs::remove_file(&caminho) {
                        Err(erro) if erro.kind() != std::io::ErrorKind::NotFound => {
                            eprintln!("vitral: não apagou {}: {erro}", caminho.display())
                        }
                        _ => {}
                    },
                }
                trava.lock().unwrap_or_else(|e| e.into_inner()).gravando = false;
                sinal.notify_all();
            }
        });
        Self { fila }
    }

    fn gravar(&self, caminho: PathBuf, bytes: Vec<u8>) {
        let (trava, sinal) = &*self.fila;
        let mut f = trava.lock().unwrap_or_else(|e| e.into_inner());
        match f.pendentes.iter_mut().find(|(c, _)| *c == caminho) {
            Some(pendente) => pendente.1 = Some(bytes),
            None => f.pendentes.push_back((caminho, Some(bytes))),
        }
        sinal.notify_all();
    }

    /// Apaga o arquivo na vez dele, depois do que já está na fila: uma gravação dele que ainda
    /// esperava sai da fila, para não recriá-lo.
    fn apagar(&self, caminho: PathBuf) {
        let (trava, sinal) = &*self.fila;
        let mut f = trava.lock().unwrap_or_else(|e| e.into_inner());
        f.pendentes.retain(|(c, _)| *c != caminho);
        f.pendentes.push_back((caminho, None));
        sinal.notify_all();
    }

    /// Espera tudo o que está na fila chegar ao disco.
    fn esperar(&self) {
        let (trava, sinal) = &*self.fila;
        let mut f = trava.lock().unwrap_or_else(|e| e.into_inner());
        while f.gravando || !f.pendentes.is_empty() {
            f = sinal.wait(f).unwrap_or_else(|e| e.into_inner());
        }
    }
}

impl Drop for Gravador {
    /// A thread termina o que já está na fila e sai.
    fn drop(&mut self) {
        let (trava, sinal) = &*self.fila;
        trava.lock().unwrap_or_else(|e| e.into_inner()).fechada = true;
        sinal.notify_all();
    }
}

#[derive(Default)]
struct Lembrado {
    projetos: Option<(std::time::Instant, Vec<String>)>,
    notas: HashMap<String, (std::time::Instant, String)>,
}

/// Quanto tempo uma leitura de `Lembrado` vale antes de ir ao disco de novo.
const VALIDADE_LEMBRADO: std::time::Duration = std::time::Duration::from_secs(2);

impl Disco {
    pub fn new(raiz: &Path) -> Self {
        Self {
            lembrado: Default::default(),
            raiz: raiz.to_path_buf(),
            gravador: Gravador::new(),
        }
    }

    pub fn pasta(&self) -> PathBuf {
        self.raiz.join(".vitral")
    }

    pub fn carregar(&self) -> (Estado, Config) {
        let mut estado: Estado = ler_json(&self.pasta().join("estado.json")).unwrap_or_default();
        let config: Config = ler_json(&self.pasta().join("config.json")).unwrap_or_default();
        for chat in &mut estado.chats {
            chat.itens = ler_json(&self.arquivo_chat(&chat.id)).unwrap_or_default();
        }
        // Nunca repete o número de uma tarefa que já existe (a cópia e o branch levam o número).
        let maior = estado.tarefas.iter().map(|t| t.id).max().unwrap_or(0);
        estado.proxima_tarefa = estado.proxima_tarefa.max(maior + 1).max(1);
        (estado, config)
    }

    /// Como as conversas: serializa aqui e deixa a escrita (com fsync) para o `Gravador`, fora
    /// da thread da janela.
    pub fn salvar_estado(&self, estado: &EstadoRef) {
        match serde_json::to_vec_pretty(estado) {
            Ok(bytes) => self.gravador.gravar(self.pasta().join("estado.json"), bytes),
            Err(erro) => eprintln!("vitral: não salvou o estado: {erro:#}"),
        }
    }

    /// Serializa aqui e deixa a escrita (com fsync) para a thread do `Gravador`.
    pub fn salvar_chat(&self, chat: &Chat) {
        match serde_json::to_vec_pretty(&chat.itens) {
            Ok(bytes) => self.gravador.gravar(self.arquivo_chat(&chat.id), bytes),
            Err(erro) => eprintln!("vitral: não salvou o chat {}: {erro:#}", chat.id),
        }
    }

    /// Espera as conversas, o estado e a configuração chegarem ao disco (ao fechar o Vitral).
    pub fn esperar_gravacoes(&self) {
        self.gravador.esperar();
    }

    pub fn salvar_config(&self, config: &Config) {
        match serde_json::to_vec_pretty(config) {
            Ok(bytes) => self.gravador.gravar(self.pasta().join("config.json"), bytes),
            Err(erro) => eprintln!("vitral: não salvou a configuração: {erro:#}"),
        }
    }

    /// Apaga as mensagens de uma conversa, na fila, depois do estado que já não a tem.
    pub fn apagar_chat(&self, id: &str) {
        self.gravador.apagar(self.arquivo_chat(id));
    }

    pub fn apagar_notas(&self, projeto: &str) {
        self.lembrado.borrow_mut().notas.remove(projeto);
        self.gravador.apagar(self.arquivo_notas(projeto));
    }

    /// A pasta mudou por fora (um projeto foi para a lixeira): a próxima pergunta vai ao disco.
    pub fn esquecer_projetos(&self) {
        self.lembrado.borrow_mut().projetos = None;
    }

    fn arquivo_chat(&self, id: &str) -> PathBuf {
        self.pasta().join("chats").join(format!("{id}.json"))
    }

    pub fn arquivo_notas(&self, projeto: &str) -> PathBuf {
        self.pasta().join("notas").join(format!("{projeto}.md"))
    }

    pub fn notas(&self, projeto: &str) -> String {
        let mut lembrado = self.lembrado.borrow_mut();
        if let Some((quando, notas)) = lembrado.notas.get(projeto)
            && quando.elapsed() < VALIDADE_LEMBRADO
        {
            return notas.clone();
        }
        let notas = fs::read_to_string(self.arquivo_notas(projeto)).unwrap_or_default();
        lembrado
            .notas
            .insert(projeto.to_string(), (std::time::Instant::now(), notas.clone()));
        notas
    }

    pub fn anotar(&self, projeto: &str, texto: &str) -> Result<()> {
        self.lembrado.borrow_mut().notas.remove(projeto);
        let caminho = self.arquivo_notas(projeto);
        fs::create_dir_all(caminho.parent().unwrap())?;
        let mut atual = fs::read_to_string(&caminho).unwrap_or_default();
        if !atual.is_empty() && !atual.ends_with('\n') {
            atual.push('\n');
        }
        atual.push_str(&format!("- {}\n", texto.trim()));
        fs::write(&caminho, atual)?;
        Ok(())
    }

    pub fn pasta_worktrees(&self, projeto: &str) -> PathBuf {
        self.pasta().join("worktrees").join(projeto)
    }

    /// Subpastas da raiz que contam como projetos (ocultas ficam de fora).
    pub fn projetos(&self) -> Vec<String> {
        let mut lembrado = self.lembrado.borrow_mut();
        if let Some((quando, nomes)) = &lembrado.projetos
            && quando.elapsed() < VALIDADE_LEMBRADO
        {
            return nomes.clone();
        }
        let nomes = self.ler_projetos();
        lembrado.projetos = Some((std::time::Instant::now(), nomes.clone()));
        nomes
    }

    fn ler_projetos(&self) -> Vec<String> {
        let mut nomes: Vec<String> = fs::read_dir(&self.raiz)
            .map(|entradas| {
                entradas
                    .flatten()
                    .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|nome| !nome.starts_with('.'))
                    .collect()
            })
            .unwrap_or_default();
        nomes.sort_by_key(|n| n.to_lowercase());
        nomes
    }
}

fn ler_json<T: for<'de> Deserialize<'de>>(caminho: &Path) -> Option<T> {
    let texto = fs::read_to_string(caminho).ok()?;
    match serde_json::from_str(&texto) {
        Ok(valor) => Some(valor),
        Err(erro) => {
            // Guarda o arquivo antes que a próxima gravação o substitua por um vazio.
            let copia = caminho.with_extension(format!("ilegivel-{}.json", agora()));
            let _ = fs::copy(caminho, &copia);
            eprintln!(
                "vitral: {} ilegível ({erro}); cópia em {}",
                caminho.display(),
                copia.display()
            );
            None
        }
    }
}

pub(crate) fn gravar_json<T: Serialize>(caminho: &Path, valor: &T) -> Result<()> {
    gravar_bytes(caminho, &serde_json::to_vec_pretty(valor)?)
}

fn gravar_bytes(caminho: &Path, bytes: &[u8]) -> Result<()> {
    fs::create_dir_all(caminho.parent().unwrap())?;
    let temporario = caminho.with_extension("json.tmp");
    // Grava no disco de verdade antes de trocar: uma queda de energia logo depois não pode
    // deixar o arquivo zerado (aconteceu com o binário durante o desenvolvimento).
    {
        use std::io::Write as _;
        let mut arquivo =
            fs::File::create(&temporario).with_context(|| temporario.display().to_string())?;
        arquivo.write_all(bytes)?;
        arquivo.sync_all()?;
    }
    fs::rename(&temporario, caminho)?;
    if let Ok(pasta) = fs::File::open(caminho.parent().unwrap()) {
        let _ = pasta.sync_all();
    }
    Ok(())
}

/// Pastas de trabalho abertas recentemente (em `~/.config/vitral/recentes.json`).
pub fn recentes() -> Vec<PathBuf> {
    dirs::config_dir()
        .and_then(|d| ler_json(&d.join("vitral").join("recentes.json")))
        .unwrap_or_default()
}

pub fn lembrar_recente(raiz: &Path) {
    let Some(pasta) = dirs::config_dir().map(|d| d.join("vitral")) else {
        return;
    };
    let mut lista = recentes();
    lista.retain(|p| p != raiz);
    lista.insert(0, raiz.to_path_buf());
    lista.truncate(8);
    let _ = gravar_json(&pasta.join("recentes.json"), &lista);
}

#[cfg(test)]
mod testes {
    use super::*;

    fn chat_de_teste() -> Chat {
        let mut chat: Chat = serde_json::from_value(serde_json::json!({
            "id": "c1", "projeto": "loja", "titulo": "Conversa", "papel": "Trabalhador",
            "pai": "c0", "tarefa": 3, "sessao": "s", "sessao_criada": true, "cwd": "/tmp/loja",
            "modelo": "opus", "criado_em": 1, "atualizado_em": 2, "custo_usd": 0.5
        }))
        .unwrap();
        chat.itens.push(Item::Assistente { texto: "oi".into() });
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
        let tarefa: Tarefa = serde_json::from_value(serde_json::json!({
            "id": 3, "projeto": "loja", "coordenador": "c0", "trabalhador": "c1",
            "titulo": "t", "objetivo": "o", "criterios": ["a"], "estado": "Rodando",
            "rodada": 1, "criada_em": 5
        }))
        .unwrap();
        let mut filas = Filas::new();
        filas.insert(
            "c1".into(),
            vec![Entrada {
                de: None,
                texto: "depois".into(),
                anexos: Vec::new(),
                registrada: false,
                ultrathink: false,
                ultracode: false,
            }],
        );
        let referencia = serde_json::to_value(EstadoRef {
            chats: std::slice::from_ref(&chat),
            tarefas: std::slice::from_ref(&tarefa),
            proxima_tarefa: 4,
            filas: &filas,
        })
        .unwrap();
        let copia = serde_json::to_value(Estado {
            chats: vec![chat],
            tarefas: vec![tarefa],
            proxima_tarefa: 4,
            filas,
        })
        .unwrap();
        assert_eq!(referencia, copia);
        let lido: Estado = serde_json::from_value(referencia).unwrap();
        assert_eq!(lido.chats[0].id, "c1");
        assert_eq!(lido.proxima_tarefa, 4);
        assert_eq!(lido.filas["c1"][0].texto, "depois");
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
        let disco = Disco::new(&raiz);
        let mut chat = chat_de_teste();
        let filas = Filas::new();
        disco.salvar_estado(&EstadoRef {
            chats: std::slice::from_ref(&chat),
            tarefas: &[],
            proxima_tarefa: 1,
            filas: &filas,
        });
        disco.salvar_chat(&chat);
        chat.itens.push(Item::Assistente { texto: "tchau".into() });
        disco.salvar_chat(&chat);
        disco.esperar_gravacoes();
        let (estado, _) = disco.carregar();
        assert_eq!(estado.chats[0].itens.len(), 2);
        assert!(!disco.arquivo_chat("c1").with_extension("json.tmp").exists());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn notas_lembradas_mudam_logo_depois_de_anotar() {
        let raiz = std::env::temp_dir().join(format!("vitral-disco-{}", novo_id()));
        fs::create_dir_all(raiz.join("loja")).unwrap();
        fs::create_dir_all(raiz.join(".oculta")).unwrap();
        let disco = Disco::new(&raiz);
        assert_eq!(disco.projetos(), vec!["loja".to_string()]);
        assert_eq!(disco.notas("loja"), "");
        disco.anotar("loja", "usar Decimal para preços").unwrap();
        assert_eq!(disco.notas("loja"), "- usar Decimal para preços\n");
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn estado_antigo_conta_tarefas_como_prontas_e_nao_repete_numero() {
        let raiz = std::env::temp_dir().join(format!("vitral-estado-{}", novo_id()));
        let disco = Disco::new(&raiz);
        let tarefa = serde_json::json!({
            "id": 5, "projeto": "p", "coordenador": "c", "trabalhador": "w", "titulo": "t",
            "objetivo": "o", "criterios": ["c"], "estado": "Rodando", "rodada": 1, "criada_em": 0,
        });
        let estado = serde_json::json!({ "chats": [], "tarefas": [tarefa], "proxima_tarefa": 1 });
        gravar_json(&disco.pasta().join("estado.json"), &estado).unwrap();
        let (estado, _) = disco.carregar();
        assert!(estado.tarefas[0].preparada);
        assert_eq!(estado.proxima_tarefa, 6);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn projeto_arquivado_sai_da_vista_e_leva_as_conversas() {
        let mut config = Config::default();
        config.projetos.entry("site".into()).or_default().arquivado = true;
        config.projetos.entry("loja".into()).or_default().icone = "store".into();
        let nomes = ["extensao", "loja", "site", "rascunhos"].map(String::from).to_vec();
        let (vista, arquivados) = config.separar_arquivados(nomes);
        assert_eq!(vista, ["extensao", "loja", "rascunhos"]);
        assert_eq!(arquivados, ["site"]);
        assert!(config.projeto_arquivado("site") && !config.projeto_arquivado("loja"));
        assert!(!config.projeto_arquivado("sem-config"));

        let mut chat = chat_de_teste();
        assert!(!config.conversa_arquivada(&chat));
        chat.arquivado = true;
        assert!(config.conversa_arquivada(&chat));
        (chat.arquivado, chat.projeto) = (false, "site".into());
        assert!(config.conversa_arquivada(&chat));

        // Só o arquivado vai para o arquivo; a marca volta igual.
        let gravado = serde_json::to_value(&config).unwrap();
        assert_eq!(gravado["projetos"]["site"]["arquivado"], true);
        assert!(gravado["projetos"]["loja"].get("arquivado").is_none());
        let lido: Config = serde_json::from_value(gravado).unwrap();
        assert!(lido.projeto_arquivado("site") && !lido.projeto_arquivado("loja"));
    }

    #[test]
    fn apagar_na_fila_tira_a_gravacao_pendente_e_o_arquivo() {
        let raiz = std::env::temp_dir().join(format!("vitral-apagar-{}", novo_id()));
        let disco = Disco::new(&raiz);
        let chat = chat_de_teste();
        disco.salvar_chat(&chat);
        disco.esperar_gravacoes();
        assert!(disco.arquivo_chat("c1").exists());
        disco.salvar_chat(&chat);
        disco.apagar_chat("c1");
        disco.anotar("loja", "nota").unwrap();
        disco.apagar_notas("loja");
        disco.apagar_chat("nunca-existiu");
        disco.esperar_gravacoes();
        assert!(!disco.arquivo_chat("c1").exists());
        assert!(!disco.arquivo_notas("loja").exists());
        assert_eq!(disco.notas("loja"), "");
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn config_antiga_sem_icone_e_cor_continua_abrindo() {
        let raiz = std::env::temp_dir().join(format!("vitral-config-{}", novo_id()));
        let disco = Disco::new(&raiz);
        fs::create_dir_all(disco.pasta()).unwrap();
        fs::write(
            disco.pasta().join("config.json"),
            r#"{"permissao":"auto","projetos":{"loja":{"preparacao":"pip install","rodar":"","copiar":[".env*"]}}}"#,
        )
        .unwrap();
        let (_, config) = disco.carregar();
        assert_eq!(config.permissao, "auto");
        let loja = &config.projetos["loja"];
        assert_eq!(loja.preparacao, "pip install");
        assert!(loja.icone.is_empty() && loja.cor.is_empty());
        // Lida sem erro: nenhuma cópia "ilegível" ao lado.
        assert_eq!(fs::read_dir(disco.pasta()).unwrap().count(), 1);

        let mut config = config;
        let loja = config.projetos.get_mut("loja").unwrap();
        loja.icone = "shopping-bag".into();
        loja.cor = "#f49ac1".into();
        disco.salvar_config(&config);
        disco.esperar_gravacoes();
        let (_, relida) = disco.carregar();
        assert_eq!(relida.projetos["loja"].icone, "shopping-bag");
        assert_eq!(relida.projetos["loja"].cor, "#f49ac1");
        let _ = fs::remove_dir_all(&raiz);
    }
}
