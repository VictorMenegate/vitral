//! O Mestre no motor: o turno dele (prompt, catálogo, referências `#`/`@`, fila com o usuário
//! primeiro, passagem de sessão), as ferramentas que só ele tem, os cartões (divisão, sugestão de
//! projeto, memória), a entrega dos chats de projeto, a junção sozinha das entregas verdes, o freio
//! de ciclo e o resumo ao voltar. A lógica fica em funções puras, testadas no fim.

use std::{cell::RefCell, time::SystemTime};

use super::*;
use crate::modelo::{
    CriterioEntregue, Divisao, Entrega, EstadoCartao, ID_MESTRE, Parte, ParteCriada, SEM_PROJETO, SugestaoProjeto,
};

/// No máximo tantas partes numa divisão.
pub const MAX_PARTES: usize = 8;
/// A memória do Mestre (o `mestre/memoria.md`) não passa disso.
pub const MAX_MEMORIA: usize = 6000;
/// O `pedido_original` de uma divisão guarda até isso das mensagens do usuário.
pub const MAX_PEDIDO_ORIGINAL: usize = 8000;
/// Uma divisão igual (mesmos destinos e títulos) proposta dentro desse tempo é a mesma.
const JANELA_DA_ASSINATURA: u64 = 2 * 3600;
/// Mensagens automáticas seguidas entre dois chats antes do freio.
pub const LIMITE_DO_CICLO: u32 = 6;
/// Fração da janela de contexto que, no fim de um turno do Mestre, faz a sessão seguinte ser nova.
const CONTEXTO_CHEIO: f64 = 0.70;
/// O começo do lembrete de entregar: um turno aberto por ele conta como aberto pelo dono.
pub const LEMBRETE_ENTREGA: &str = "Se já terminou o pedido";
/// As ferramentas de arquivo do Mestre (o resto do Claude Code não existe para ele).
pub const FERRAMENTAS_DO_MESTRE: &str = "Read,Grep,Glob";
/// A segunda camada: mesmo que o `--tools` deixe passar, nada que escreve ou roda comando.
const BLOQUEADAS_DO_MESTRE: [&str; 11] = [
    "Edit", "Write", "NotebookEdit", "Task", "Bash", "WebFetch", "WebSearch", "Monitor", "EnterWorktree",
    "ExitWorktree", "Workflow",
];

/// Por que o próximo turno começa numa sessão nova, com o bloco de passagem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotivoPassagem {
    /// O Claude Code apagou a sessão gravada (`cleanupPeriodDays`).
    Expirou,
    /// O contexto do Mestre passou de 70% no fim de um turno.
    ContextoCheio,
}

/// "Enquanto você esteve fora": só na memória, montado das ocorrências; nunca vai para o disco.
#[derive(Clone, Debug, PartialEq)]
pub struct ResumoVolta {
    pub desde: u64,
    pub ate: u64,
    pub linhas: Vec<LinhaResumo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinhaResumo {
    /// "terminou", "precisa" ou "falhou".
    pub grupo: &'static str,
    pub projeto: String,
    pub chat: String,
    pub texto: String,
}

/// Para onde uma referência leva: um projeto ("" é Avulsos) e, se for um chat, qual.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Destino {
    pub projeto: String,
    pub chat: Option<String>,
}

/// O bloco `[Vitral] Referências` de uma mensagem ao Mestre e o destino único, quando há um.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Referencias {
    pub texto: String,
    pub unica: Option<Destino>,
}

/// Como a divisão começa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Comeco {
    Cartao,
    /// A opção "começar sem perguntar" ou um turno de rotina.
    SemPerguntar,
    /// Uma parte só, para o destino que o usuário citou.
    PorReferencia,
}

/// O que fazer no fim do turno de um chat com um pedido aberto.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FimDoPedido {
    /// Vai para a conferência; `formal` é que houve `entregar_tarefa`.
    Entregar { formal: bool },
    /// Uma vez por rodada: "chame entregar_tarefa".
    Lembrar,
    /// Perguntou ou tem mensagem na fila: ainda não é a entrega.
    Esperar,
    Nada,
}

/// As mensagens automáticas entre pares de chats sem o usuário no meio (só na memória).
#[derive(Default)]
pub struct Ciclos {
    pares: HashMap<(String, String), u32>,
}

impl Ciclos {
    fn par(a: &str, b: &str) -> (String, String) {
        if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
    }

    /// Mais uma mensagem automática entre os dois: quantas já foram, com esta.
    pub fn contar(&mut self, a: &str, b: &str) -> u32 {
        let n = self.pares.entry(Self::par(a, b)).or_default();
        *n += 1;
        *n
    }

    /// O usuário escreveu para o chat: os pares dele recomeçam.
    pub fn zerar(&mut self, chat: &str) {
        self.pares.retain(|(a, b), _| a != chat && b != chat);
    }

    /// Os pares que passaram do limite e esperam o usuário.
    pub fn travados(&self) -> Vec<(String, String)> {
        let mut pares: Vec<(String, String)> =
            self.pares.iter().filter(|(_, n)| **n > LIMITE_DO_CICLO).map(|(p, _)| p.clone()).collect();
        pares.sort();
        pares
    }
}

// ---------- puras ----------

/// Caminho com a pasta pessoal como "~".
pub fn com_til(caminho: &Path) -> String {
    match dirs::home_dir().as_deref().and_then(|c| caminho.strip_prefix(c).ok()) {
        Some(resto) if resto.as_os_str().is_empty() => "~".into(),
        Some(resto) => format!("~/{}", resto.display()),
        None => caminho.display().to_string(),
    }
}

/// "~/x" com a pasta pessoal de volta.
fn sem_til(texto: &str) -> PathBuf {
    match (texto.strip_prefix("~/"), dirs::home_dir()) {
        (Some(resto), Some(casa)) => casa.join(resto),
        _ if texto == "~" => dirs::home_dir().unwrap_or_default(),
        _ => PathBuf::from(texto),
    }
}

/// "agora", "há 12 min", "há 3 h", "há 2 dias".
pub fn ha_quanto(segundos: u64) -> String {
    match segundos {
        0..60 => "agora".into(),
        60..3600 => format!("há {} min", segundos / 60),
        3600..86_400 => format!("há {} h", segundos / 3600),
        _ if segundos < 2 * 86_400 => "há 1 dia".into(),
        _ => format!("há {} dias", segundos / 86_400),
    }
}

/// O número com que o usuário e o Mestre chamam o chat: o da tarefa dele, senão o id.
pub fn numero_do_chat(c: &Chat) -> String {
    match (c.papel, c.tarefa) {
        (Papel::Mestre, _) => ID_MESTRE.into(),
        (_, Some(n)) => n.to_string(),
        _ => c.id.clone(),
    }
}

/// O chat que o texto cita: "mestre", "12", "chat 12", "#12", "@12" ou o id.
pub fn achar_chat<'a>(chats: &'a [Chat], texto: &str) -> Option<&'a Chat> {
    let limpo = texto.trim().trim_start_matches(['@', '#']);
    let limpo = limpo.strip_prefix("chat ").unwrap_or(limpo).trim();
    if limpo.eq_ignore_ascii_case(ID_MESTRE) {
        return chats.iter().find(|c| c.id == ID_MESTRE);
    }
    if let Some(c) = chats.iter().find(|c| c.id == limpo) {
        return Some(c);
    }
    let n: u64 = limpo.trim_start_matches('#').parse().ok()?;
    chats.iter().find(|c| c.tarefa == Some(n) && c.papel != Papel::Mestre)
}

/// Os tokens `#…` e `@…` de um texto, como aparecem (o sinal, o nome e se veio entre aspas). Só
/// no começo ou depois de espaço ou abre-parêntese; `#` seguido de algo que não é letra não conta
/// (`#12`, o título `# Plano`).
fn tokens(texto: &str) -> Vec<(char, String, bool)> {
    let chars: Vec<char> = texto.chars().collect();
    let mut achados = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let antes_ok = i == 0 || chars[i - 1].is_whitespace() || matches!(chars[i - 1], '(' | '[' | '{' | '"' | '\'');
        if !matches!(c, '#' | '@') || !antes_ok {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if c == '@' && chars.get(j) == Some(&'"') {
            let fim = chars[j + 1..].iter().position(|x| *x == '"').map(|p| j + 1 + p);
            if let Some(fim) = fim {
                let nome: String = chars[j + 1..fim].iter().collect();
                if !nome.trim().is_empty() {
                    achados.push((c, nome.trim().to_string(), true));
                }
                i = fim + 1;
                continue;
            }
        }
        if c == '#' && !chars.get(j).is_some_and(|x| x.is_alphabetic()) {
            i += 1;
            continue;
        }
        while j < chars.len() && (chars[j].is_alphanumeric() || matches!(chars[j], '-' | '_' | '.')) {
            j += 1;
        }
        let nome: String = chars[i + 1..j].iter().collect();
        let nome = nome.trim_end_matches(['.', '-', '_']).to_string();
        if !nome.is_empty() {
            achados.push((c, nome, false));
        }
        i = j.max(i + 1);
    }
    achados
}

enum Achado {
    Projeto(String),
    Chat(String),
    Mestre,
    Ambiguo(Vec<String>),
    Nenhum,
}

fn achar_projeto_por_chave(projetos: &[Projeto], nome: &str) -> Achado {
    let chave = dados::chave(nome);
    if chave == "avulsos" {
        return Achado::Projeto(SEM_PROJETO.into());
    }
    match projetos.iter().find(|p| p.chave() == chave).or_else(|| projetos.iter().find(|p| p.id == nome)) {
        Some(p) => Achado::Projeto(p.id.clone()),
        None => Achado::Nenhum,
    }
}

fn achar_chat_por_nome(chats: &[Chat], nome: &str) -> Achado {
    if nome.eq_ignore_ascii_case(ID_MESTRE) {
        return Achado::Mestre;
    }
    if nome.chars().all(|c| c.is_ascii_digit()) {
        return match achar_chat(chats, nome) {
            Some(c) => Achado::Chat(c.id.clone()),
            None => Achado::Nenhum,
        };
    }
    if let Some(c) = chats.iter().find(|c| c.id == nome) {
        return Achado::Chat(c.id.clone());
    }
    let alvo = slug(nome);
    let vivos = || chats.iter().filter(|c| !c.arquivado && c.papel != Papel::Mestre);
    let mut iguais: Vec<String> = vivos().filter(|c| slug(&c.titulo) == alvo).map(|c| c.id.clone()).collect();
    if iguais.is_empty() {
        iguais = vivos()
            .filter(|c| {
                let s = slug(&c.titulo);
                s.starts_with(&format!("{alvo}-")) || s == alvo
            })
            .map(|c| c.id.clone())
            .collect();
    }
    match iguais.len() {
        0 => Achado::Nenhum,
        1 => Achado::Chat(iguais.remove(0)),
        _ => Achado::Ambiguo(iguais),
    }
}

fn descrever_projeto(projetos: &[Projeto], id: &str) -> String {
    if id.is_empty() {
        return "Avulsos (chats sem projeto, sem pasta de código)".into();
    }
    let Some(p) = projetos.iter().find(|p| p.id == id) else {
        return format!("projeto {id}");
    };
    let pastas: Vec<String> = p
        .pastas
        .iter()
        .map(|l| if l.propria { "pasta própria".to_string() } else { com_til(&l.caminho) })
        .collect();
    let mut texto = format!("projeto {} (id {}; pastas {}", p.nome, p.id, if pastas.is_empty() { "nenhuma".into() } else { pastas.join(", ") });
    if p.arquivado {
        texto.push_str("; arquivado");
    }
    texto.push(')');
    texto
}

/// "chat 7 \"Docs\" (projeto Vitral, parado, em conversa)".
fn descrever_chat(c: &Chat, projetos: &[Projeto], tarefas: &[Tarefa], rodando: &dyn Fn(&str) -> bool) -> String {
    let projeto = if c.projeto.is_empty() {
        "Avulsos".to_string()
    } else {
        format!("projeto {}", projetos.iter().find(|p| p.id == c.projeto).map_or(c.projeto.as_str(), |p| p.nome.as_str()))
    };
    let execucao = if rodando(&c.id) { "rodando" } else { "parado" };
    let pedido = match c.tarefa.and_then(|n| tarefas.iter().find(|t| t.id == n)) {
        Some(t) if t.estado == EstadoTarefa::Conversa => "em conversa".to_string(),
        Some(t) if t.dono() == Some(ID_MESTRE) => format!("pedido do Mestre {}", t.estado.nome()),
        Some(t) => t.estado.nome().to_string(),
        None => c.papel.rotulo().to_string(),
    };
    format!("chat {} \"{}\" ({projeto}, {execucao}, {pedido})", numero_do_chat(c), c.titulo)
}

/// Resolve os `#projeto` e `@chat` de uma mensagem ao Mestre. O texto vai em `Entrada.extra` (só
/// o Claude vê); `unica` é o destino quando tudo o que foi citado leva a um lugar só.
pub fn resolver_referencias(
    texto: &str,
    projetos: &[Projeto],
    chats: &[Chat],
    tarefas: &[Tarefa],
    rodando: &dyn Fn(&str) -> bool,
) -> Referencias {
    let mut partes: Vec<String> = Vec::new();
    let mut vistos: Vec<String> = Vec::new();
    let mut projetos_citados: Vec<String> = Vec::new();
    let mut chats_citados: Vec<String> = Vec::new();
    let mut tudo_resolvido = true;
    for (sinal, nome, aspas) in tokens(texto) {
        let citado = if aspas { format!("{sinal}\"{nome}\"") } else { format!("{sinal}{nome}") };
        if vistos.iter().any(|v| v.eq_ignore_ascii_case(&citado)) {
            continue;
        }
        vistos.push(citado.clone());
        let achado = if sinal == '#' { achar_projeto_por_chave(projetos, &nome) } else { achar_chat_por_nome(chats, &nome) };
        match achado {
            Achado::Projeto(id) => {
                partes.push(format!("{citado} = {}", descrever_projeto(projetos, &id)));
                if !projetos_citados.contains(&id) {
                    projetos_citados.push(id);
                }
            }
            Achado::Chat(id) => {
                if let Some(c) = chats.iter().find(|c| c.id == id) {
                    partes.push(format!("{citado} = {}", descrever_chat(c, projetos, tarefas, rodando)));
                    if !chats_citados.contains(&id) {
                        chats_citados.push(id);
                    }
                }
            }
            Achado::Mestre => partes.push(format!("{citado} = o Mestre (você)")),
            Achado::Ambiguo(ids) => {
                tudo_resolvido = false;
                let nomes: Vec<String> = ids
                    .iter()
                    .filter_map(|id| chats.iter().find(|c| c.id == *id))
                    .map(|c| format!("chat {} \"{}\"", numero_do_chat(c), c.titulo))
                    .collect();
                partes.push(format!("{citado}: ambíguo ({})", nomes.join(", ")));
            }
            Achado::Nenhum => {
                tudo_resolvido = false;
                let o_que = if sinal == '#' { "nenhum projeto" } else { "nenhum chat" };
                partes.push(format!("{citado}: {o_que} com esse nome"));
            }
        }
    }
    if partes.is_empty() {
        return Referencias::default();
    }
    let unica = if !tudo_resolvido {
        None
    } else {
        match (projetos_citados.as_slice(), chats_citados.as_slice()) {
            ([p], []) => Some(Destino { projeto: p.clone(), chat: None }),
            (ps, [c]) => {
                let projeto = chats.iter().find(|x| x.id == *c).map(|x| x.projeto.clone()).unwrap_or_default();
                (ps.is_empty() || ps == [projeto.clone()]).then(|| Destino { projeto, chat: Some(c.clone()) })
            }
            _ => None,
        }
    };
    Referencias { texto: format!("[Vitral] Referências: {}.", partes.join("; ")), unica }
}

/// Uma parte de `propor_divisao` como veio, antes de validar (o projeto e o chat ainda como texto).
pub fn parte_do_json(v: &Value) -> Parte {
    let texto = |chave: &str| v[chave].as_str().map(str::trim).unwrap_or_default().to_string();
    let opcional = |chave: &str| Some(texto(chave)).filter(|t| !t.is_empty());
    let chat = match &v["chat"] {
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) if !s.trim().is_empty() => Some(s.trim().to_string()),
        _ => None,
    };
    Parte {
        projeto: texto("projeto"),
        pasta: opcional("pasta").map(|p| sem_til(&p)),
        chat,
        titulo: texto("titulo"),
        pedido: texto("pedido"),
        criterios: lista_de_textos(&v["criterios"]).into_iter().map(|c| c.trim().to_string()).filter(|c| !c.is_empty()).collect(),
        verificacao: opcional("verificacao"),
        modelo: opcional("modelo"),
        arquivos: lista_de_textos(&v["arquivos"]),
        ..Default::default()
    }
}

/// Confere as partes de uma divisão e devolve cada uma com o id do projeto e do chat, ou a
/// mensagem exata para o Mestre corrigir. `ocupado`: o chat tem processo, fila, espera ou cópia
/// sendo preparada.
pub fn validar_divisao(
    partes: &[Parte],
    projetos: &[Projeto],
    chats: &[Chat],
    tarefas: &[Tarefa],
    ocupado: &dyn Fn(&str) -> bool,
) -> Result<Vec<Parte>, String> {
    if partes.is_empty() {
        return Err("Mande pelo menos uma parte.".into());
    }
    if partes.len() > MAX_PARTES {
        return Err(format!("No máximo {MAX_PARTES} partes por divisão; junte as menores ou proponha o resto depois."));
    }
    let mut validas: Vec<Parte> = Vec::new();
    for (i, bruta) in partes.iter().enumerate() {
        let rotulo = if bruta.titulo.is_empty() { format!("Parte {}", i + 1) } else { format!("Parte {} (\"{}\")", i + 1, bruta.titulo) };
        let p = validar_parte(bruta, projetos, chats, tarefas, ocupado).map_err(|motivo| format!("{rotulo}: {motivo}"))?;
        if let Some(chat) = &p.chat
            && validas.iter().any(|v| v.chat.as_ref() == Some(chat))
        {
            let numero = chats.iter().find(|c| c.id == *chat).map_or_else(|| chat.clone(), numero_do_chat);
            return Err(format!("{rotulo}: duas partes para o chat {numero}; junte as duas numa só."));
        }
        validas.push(p);
    }
    Ok(validas)
}

/// Uma parte só: o projeto existe, à vista e com pasta; a pasta é dele e está no disco; o chat
/// citado é do projeto, está em conversa e livre. O erro é o motivo, sem o número da parte (no
/// Começar ele vai para a falha da parte).
pub fn validar_parte(
    bruta: &Parte,
    projetos: &[Projeto],
    chats: &[Chat],
    tarefas: &[Tarefa],
    ocupado: &dyn Fn(&str) -> bool,
) -> Result<Parte, String> {
    let mut p = bruta.clone();
    if p.titulo.is_empty() || p.pedido.is_empty() {
        return Err("falta o título ou o pedido.".into());
    }
    if p.criterios.is_empty() {
        return Err("dê pelo menos um critério de aceitação verificável.".into());
    }
    let nome = p.projeto.trim().trim_start_matches('#').trim().to_string();
    let id = if nome.is_empty() || dados::chave(&nome) == "avulsos" {
        String::new()
    } else {
        match resolver_projeto(projetos, &nome) {
            Resolucao::Um(id) => id,
            Resolucao::Nenhum => {
                let mut nomes: Vec<String> =
                    projetos.iter().filter(|p| !p.arquivado).map(|p| format!("{} (#{})", p.nome, p.chave())).collect();
                nomes.push("Avulsos (#avulsos)".into());
                return Err(format!("não há projeto \"{nome}\". Projetos: {}.", nomes.join(", ")));
            }
            Resolucao::Varios(ids) => return Err(format!("\"{nome}\" serve para mais de um projeto ({}); use o id.", ids.join(", "))),
        }
    };
    p.projeto = id.clone();
    match projetos.iter().find(|x| x.id == id) {
        Some(projeto) => {
            if projeto.arquivado {
                return Err(format!("o projeto {} está arquivado.", projeto.nome));
            }
            if projeto.pastas.is_empty() {
                return Err(format!(
                    "o projeto {} não tem pasta ligada; peça ao usuário para ligar uma (Editar projeto).",
                    projeto.nome
                ));
            }
            let lista = || projeto.pastas.iter().map(|l| com_til(&l.caminho)).collect::<Vec<_>>().join(", ");
            let pasta = match &p.pasta {
                Some(pedida) => {
                    let canonica = dunce::canonicalize(&pedida).unwrap_or_else(|_| pedida.clone());
                    let achada = projeto.pastas.iter().find(|l| {
                        l.caminho == *pedida
                            || l.caminho == canonica
                            || (pedida.is_relative() && l.caminho.file_name() == Some(pedida.as_os_str()))
                    });
                    match achada {
                        Some(l) => l.caminho.clone(),
                        None => {
                            return Err(format!("a pasta {} não é do projeto {}. Pastas: {}.", pedida.display(), projeto.nome, lista()));
                        }
                    }
                }
                None if projeto.pastas.len() > 1 => {
                    return Err(format!("o projeto {} tem mais de uma pasta; diga qual em `pasta`: {}.", projeto.nome, lista()));
                }
                None => projeto.pastas[0].caminho.clone(),
            };
            if !pasta.is_dir() {
                return Err(format!(
                    "a pasta {} não foi encontrada (disco desmontado ou pasta movida); avise o usuário.",
                    com_til(&pasta)
                ));
            }
            p.pasta = (projeto.pastas.len() > 1).then_some(pasta);
        }
        None if !id.is_empty() => return Err(format!("não há projeto \"{nome}\".")),
        None => p.pasta = None,
    }
    if let Some(citado) = p.chat.clone() {
        let Some(c) = achar_chat(chats, &citado) else {
            return Err(format!("não há chat \"{citado}\". Use listar_chats."));
        };
        let numero = numero_do_chat(c);
        if c.papel != Papel::Projeto || c.projeto != id {
            let onde = match projetos.iter().find(|x| x.id == id) {
                Some(x) => format!("do projeto {}", x.nome),
                None => "de Avulsos".into(),
            };
            return Err(format!("o chat {numero} não é um chat {onde}; deixe `chat` vazio para abrir um novo."));
        }
        if c.arquivado {
            return Err(format!("o chat {numero} está arquivado."));
        }
        match c.tarefa.and_then(|n| tarefas.iter().find(|t| t.id == n)) {
            Some(t) if t.estado == EstadoTarefa::Conversa => {
                if t.mudancas.as_ref().is_some_and(|m| m.arquivos > 0) {
                    p.aviso = Some("o chat tem mudanças ainda não juntadas".into());
                }
            }
            Some(t) => {
                return Err(format!(
                    "o chat {numero} já tem um pedido aberto ({}); espere ele terminar ou abra um chat novo.",
                    t.estado.nome()
                ));
            }
            None => return Err(format!("o chat {numero} não recebe pedidos.")),
        }
        if ocupado(&c.id) {
            return Err(format!("o chat {numero} está trabalhando ou tem mensagens esperando; espere ou abra um chat novo."));
        }
        p.chat = Some(c.id.clone());
    }
    Ok(p)
}

/// FNV-1a de 64 bits, em hexadecimal: estável entre versões (o `DefaultHasher` não promete isso).
fn hash_estavel(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(*b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// A chave do turno: as entradas que abriram o turno atual do Mestre. A repetição do mesmo turno
/// (Tentar de novo, a sessão consertada) marca as entradas como já na conversa: isso não conta.
pub fn chave_do_turno(entradas: &[Entrada]) -> String {
    let mesmas: Vec<Entrada> = entradas.iter().cloned().map(|e| Entrada { registrada: false, ..e }).collect();
    hash_estavel(serde_json::to_string(&mesmas).unwrap_or_default().as_bytes())
}

/// O destino e o título de cada parte, em ordem: duas divisões com a mesma assinatura são a mesma.
pub fn assinatura_da_divisao(partes: &[Parte]) -> String {
    let mut linhas: Vec<String> = partes
        .iter()
        .map(|p| format!("{}|{}|{}", p.projeto, p.chat.as_deref().unwrap_or(""), slug(&p.titulo)))
        .collect();
    linhas.sort();
    linhas.join("\n")
}

/// A divisão do Mestre, aberta ou já começada, do mesmo turno ou igual a uma das últimas 2 horas.
pub fn divisao_repetida<'a>(cartoes: &'a [Cartao], turno: &str, assinatura: &str, agora: u64) -> Option<&'a Cartao> {
    cartoes.iter().rev().find(|c| {
        c.chat == ID_MESTRE
            && matches!(c.estado, EstadoCartao::Aberto | EstadoCartao::Aceito)
            && c.divisao().is_some_and(|d| {
                d.turno == turno || (d.assinatura == assinatura && agora.saturating_sub(c.criado_em) < JANELA_DA_ASSINATURA)
            })
    })
}

/// O que fazer com uma `propor_divisao`: responder com a que já existe, ou criar a nova e substituir
/// os cartões de divisão ainda abertos.
pub enum Proposta<'a> {
    Repetida(&'a Cartao),
    Nova { substituir: Vec<u64> },
}

pub fn decidir_proposta<'a>(cartoes: &'a [Cartao], turno: &str, assinatura: &str, agora: u64) -> Proposta<'a> {
    if let Some(c) = divisao_repetida(cartoes, turno, assinatura, agora) {
        return Proposta::Repetida(c);
    }
    let substituir = cartoes.iter().filter(|c| c.aberto() && c.chat == ID_MESTRE && c.divisao().is_some()).map(|c| c.id).collect();
    Proposta::Nova { substituir }
}

/// O que o Começar faz com cada parte.
#[derive(Debug)]
pub enum PlanoDaParte {
    /// Removida pelo usuário, ou já começada antes.
    Pular,
    /// Já não vale (projeto arquivado, chat ocupado…): fica com o motivo e as outras seguem.
    Falha(String),
    /// O pedido vai ao chat que já existe (o id dele está na parte).
    Reabrir(Parte),
    Novo(Parte),
}

/// A revalidação do Começar, parte por parte.
pub fn planejar_comeco(
    partes: &[Parte],
    projetos: &[Projeto],
    chats: &[Chat],
    tarefas: &[Tarefa],
    ocupado: &dyn Fn(&str) -> bool,
) -> Vec<PlanoDaParte> {
    partes
        .iter()
        .map(|p| {
            if p.removida || p.criada.is_some() {
                return PlanoDaParte::Pular;
            }
            match validar_parte(p, projetos, chats, tarefas, ocupado) {
                Err(motivo) => PlanoDaParte::Falha(motivo),
                Ok(v) if v.chat.is_some() => PlanoDaParte::Reabrir(v),
                Ok(v) => PlanoDaParte::Novo(v),
            }
        })
        .collect()
}

/// A regra do dono: só quem fez o pedido aberto aprova, pede ajuste ou cancela.
pub fn pedido_de(t: &Tarefa, quem: &str) -> Result<(), String> {
    if t.coordenador == quem {
        return Ok(());
    }
    Err(match t.dono() {
        None => format!("o chat {} não tem pedido seu (é uma conversa do usuário)", t.id),
        Some(_) => format!("o chat {} não tem pedido seu (o pedido aberto é de outro chat)", t.id),
    })
}

/// Se a divisão começa direto (a opção, a rotina ou o destino citado) ou mostra o cartão.
pub fn como_comecar(config: &Config, entradas: &[Entrada], partes: &[Parte], perguntas: &[String], unicas: &[Destino]) -> Comeco {
    if config.comecar_sem_perguntar || entradas.iter().any(|e| e.rotina.is_some()) {
        return Comeco::SemPerguntar;
    }
    if let [parte] = partes
        && perguntas.is_empty()
        && unicas.contains(&Destino { projeto: parte.projeto.clone(), chat: parte.chat.clone() })
    {
        return Comeco::PorReferencia;
    }
    Comeco::Cartao
}

/// O texto das mensagens do usuário do turno (até 8000 caracteres) e os anexos delas.
pub fn pedido_do_turno(entradas: &[Entrada]) -> (String, Vec<PathBuf>) {
    let do_usuario: Vec<&Entrada> = entradas.iter().filter(|e| e.do_usuario()).collect();
    let texto = do_usuario.iter().map(|e| e.texto.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join("\n\n");
    let texto = if texto.chars().count() > MAX_PEDIDO_ORIGINAL {
        format!("{}…", texto.chars().take(MAX_PEDIDO_ORIGINAL).collect::<String>())
    } else {
        texto
    };
    (texto, do_usuario.iter().flat_map(|e| e.anexos.iter().cloned()).collect())
}

/// Só o usuário falou neste turno (nem o Vitral, nem outro chat, nem uma rotina): só assim
/// `lembrar` grava direto.
pub fn turno_so_do_usuario(entradas: &[Entrada]) -> bool {
    !entradas.is_empty() && entradas.iter().all(|e| e.de.is_none() && e.rotina.is_none())
}

/// A memória com mais uma linha, ou por que não coube.
pub fn lembrar_em(memoria: &str, texto: &str) -> Result<String, String> {
    let linha = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    let linha = linha.trim_start_matches("- ").trim().to_string();
    if linha.is_empty() {
        return Err("Diga o que lembrar.".into());
    }
    if memoria.lines().any(|l| l.trim().trim_start_matches("- ").trim().eq_ignore_ascii_case(&linha)) {
        return Ok(memoria.to_string());
    }
    let mut nova = memoria.trim_end().to_string();
    if !nova.is_empty() {
        nova.push('\n');
    }
    nova.push_str(&format!("- {linha}\n"));
    if nova.chars().count() > MAX_MEMORIA {
        return Err(format!("memória cheia ({MAX_MEMORIA} caracteres): junte ou esqueça itens"));
    }
    Ok(nova)
}

/// A memória sem a única linha que contém o texto; com nenhuma ou várias, o erro lista as candidatas.
pub fn esquecer_em(memoria: &str, texto: &str) -> Result<String, String> {
    let alvo = texto.trim().to_lowercase();
    if alvo.is_empty() {
        return Err("Diga o que esquecer.".into());
    }
    let linhas: Vec<&str> = memoria.lines().collect();
    let achadas: Vec<usize> = (0..linhas.len()).filter(|i| linhas[*i].to_lowercase().contains(&alvo)).collect();
    match achadas.as_slice() {
        [i] => {
            let ficam: Vec<&str> = linhas.iter().enumerate().filter(|(j, _)| j != i).map(|(_, l)| *l).collect();
            let mut nova = ficam.join("\n");
            if !nova.trim().is_empty() {
                nova.push('\n');
            } else {
                nova.clear();
            }
            Ok(nova)
        }
        [] => {
            let todas: Vec<&str> = linhas.iter().copied().filter(|l| !l.trim().is_empty()).collect();
            Err(if todas.is_empty() {
                "A memória está vazia.".to_string()
            } else {
                format!("Nenhuma linha da memória tem \"{}\". A memória:\n{}", texto.trim(), todas.join("\n"))
            })
        }
        varias => Err(format!(
            "Mais de uma linha tem \"{}\"; use um trecho que só uma tenha:\n{}",
            texto.trim(),
            varias.iter().map(|i| linhas[*i]).collect::<Vec<_>>().join("\n")
        )),
    }
}

/// Todos os critérios do pedido, pelo número, marcados "feito".
fn criterios_feitos(entrega: &Entrega, total: usize) -> bool {
    total > 0 && (1..=total).all(|n| entrega.criterios.iter().any(|c| c.indice == n && c.estado == "feito"))
}

/// A entrega pode ser juntada sem acordar o Mestre: tudo verde.
pub fn pode_juntar_sozinho(t: &Tarefa, config: &Config) -> bool {
    let verificacao = match (&t.verificacao, &t.ultima_verificacao) {
        (Some(_), Some(v)) => v.passou,
        (Some(_), None) => false,
        (None, v) => v.as_ref().is_none_or(|v| v.passou),
    };
    config.aprovar_sozinho
        && t.dono() == Some(ID_MESTRE)
        && verificacao
        && config.revisor
        && t.revisao.as_ref().is_some_and(|r| r.veredito == "aprovar" && r.problemas.iter().all(|p| p.gravidade == "baixa"))
        && t.entrega.as_ref().is_some_and(|e| e.rodada == t.rodada && criterios_feitos(e, t.criterios.len()))
        && t.sobreposicao.is_empty()
        && portao_aprovacao(t, false, "").is_ok()
}

/// O que fazer quando o turno de um chat com pedido aberto termina. `pendente`: perguntou ao dono ou
/// tem mensagem na fila; `do_dono`: o turno foi aberto pelo dono (ou pelo lembrete).
pub fn decidir_fim(t: &Tarefa, pendente: bool, do_dono: bool) -> FimDoPedido {
    if !t.estado.ativa() || t.dono().is_none() {
        return FimDoPedido::Nada;
    }
    if pendente {
        return FimDoPedido::Esperar;
    }
    if t.entrega.as_ref().is_some_and(|e| e.rodada == t.rodada) {
        return FimDoPedido::Entregar { formal: true };
    }
    if t.lembrete_rodada < t.rodada {
        return FimDoPedido::Lembrar;
    }
    if do_dono {
        return FimDoPedido::Entregar { formal: false };
    }
    FimDoPedido::Nada
}

/// O turno foi aberto pelo dono do pedido ou pelo lembrete de entregar.
pub fn aberto_pelo_dono(entradas: &[Entrada], dono: &str) -> bool {
    entradas.iter().any(|e| {
        e.remetente.as_deref() == Some(dono)
            || (e.de.as_deref() == Some("Vitral") && e.texto.starts_with(LEMBRETE_ENTREGA))
    })
}

/// A entrega formal como texto, para o relatório e para o revisor.
pub fn texto_da_entrega(e: &Entrega, criterios: &[String]) -> String {
    let mut texto = format!("Resumo: {}\nComo verificou: {}\n", e.resumo.trim(), e.como_verificou.trim());
    if !criterios.is_empty() {
        texto.push_str("Critérios:\n");
        for (i, criterio) in criterios.iter().enumerate() {
            let n = i + 1;
            match e.criterios.iter().find(|c| c.indice == n) {
                Some(c) if c.evidencia.trim().is_empty() => texto.push_str(&format!("{n}. {} ({criterio})\n", c.estado)),
                Some(c) => texto.push_str(&format!("{n}. {} ({criterio}): {}\n", c.estado, c.evidencia.trim())),
                None => texto.push_str(&format!("{n}. sem estado ({criterio})\n")),
            }
        }
    }
    if !e.pendencias.trim().is_empty() {
        texto.push_str(&format!("Pendências: {}\n", e.pendencias.trim()));
    }
    texto
}

/// Os critérios de `entregar_tarefa`, com o estado normalizado ("não" e "nao feito" viram "nao").
pub fn criterios_da_entrega(valor: &Value) -> Vec<CriterioEntregue> {
    lista_de(valor)
        .unwrap_or_default()
        .iter()
        .filter_map(|v| {
            let indice = v["indice"].as_u64().or_else(|| v["indice"].as_str()?.trim().parse().ok())? as usize;
            let estado = slug(v["estado"].as_str().unwrap_or_default());
            let estado = match estado.as_str() {
                "feito" | "ok" | "sim" | "cumprido" => "feito",
                "parcial" | "parcialmente" => "parcial",
                _ => "nao",
            };
            Some(CriterioEntregue {
                indice,
                estado: estado.into(),
                evidencia: v["evidencia"].as_str().unwrap_or_default().trim().to_string(),
            })
        })
        .collect()
}

/// O relatório que acorda o Mestre: curto, com o que ele precisa para decidir. A saída da
/// verificação só vai quando ela falhou.
pub fn relatorio_compacto(
    t: &Tarefa,
    projeto: &str,
    titulo: &str,
    relatorio: &str,
    estatistica: Option<&str>,
    erro: bool,
    falha_juncao: Option<&str>,
) -> String {
    let mut texto = format!("{projeto} · chat {} \"{titulo}\" · rodada {}", t.id, t.rodada);
    if erro {
        texto.push_str(" · terminou com ERRO");
    }
    texto.push('\n');
    if let Some(falha) = falha_juncao {
        texto.push_str(&format!("\nA junção sozinha não deu: {}\n", falha.trim()));
    }
    match t.entrega.as_ref().filter(|e| e.rodada == t.rodada) {
        Some(e) => texto.push_str(&format!("\nEntrega:\n{}", texto_da_entrega(e, &t.criterios))),
        None => texto.push_str(&format!(
            "\nSem entrega formal; a última resposta do chat:\n{}\n",
            git::inicio_do_texto(relatorio.trim(), 3000)
        )),
    }
    conferencia_no_relatorio(t, estatistica, &mut texto);
    let n = t.id;
    texto.push_str(&format!("\nDecida: aprovar_tarefa {n} ou pedir_ajuste {n}. Detalhes: ver_diff {n} (por arquivo) e ver_chat {n}."));
    texto
}

/// O relatório que chega ao chat que criou o ajudante, no fim de cada rodada dele: o que ele disse
/// (a última resposta da vez) e a conferência, curta como a do Mestre.
pub fn relatorio_ao_pai(t: &Tarefa, relatorio: &str, estatistica: Option<&str>, erro: bool, falha_revisor: Option<&str>) -> String {
    let n = t.id;
    let mut texto = format!(
        "Ajudante #{n} \"{}\" terminou a rodada {}{}.\n\nRelatório do ajudante:\n{}\n",
        t.titulo,
        t.rodada,
        if erro { " com ERRO" } else { "" },
        git::inicio_do_texto(relatorio.trim(), 3000)
    );
    conferencia_no_relatorio(t, estatistica, &mut texto);
    if let Some(falha) = falha_revisor {
        texto.push_str(&format!("\nO revisor independente não conseguiu revisar: {}\n", falha.trim()));
    }
    texto.push_str(&format!(
        "\nRevise (ver_diff {n}; num diff grande vem o resumo, e você pede os arquivos que importam) e decida: \
         aprovar_tarefa {n} (junta na sua pasta) ou pedir_ajuste {n}."
    ));
    texto
}

/// O que o relatório diz dos arquivos, da verificação, do revisor e dos outros chats nos mesmos
/// arquivos. A saída da verificação só vai quando ela falhou; do revisor, só os problemas altos e
/// médios.
fn conferencia_no_relatorio(t: &Tarefa, estatistica: Option<&str>, texto: &mut String) {
    match estatistica.map(str::trim) {
        Some("") => texto.push_str("\nNenhum arquivo mudou.\n"),
        Some(e) => texto.push_str(&format!("\nArquivos (git diff --stat):\n{e}\n")),
        None => {}
    }
    if let Some(v) = &t.ultima_verificacao {
        let tempo = v.tempo().map(|t| format!(" em {t}")).unwrap_or_default();
        if v.passou {
            texto.push_str(&format!("\nVerificação `{}`: PASSOU{tempo}\n", v.comando));
        } else {
            let linhas: Vec<&str> = v.saida.trim_end().lines().collect();
            let fim = linhas[linhas.len().saturating_sub(30)..].join("\n");
            texto.push_str(&format!("\nVerificação `{}`: FALHOU{tempo} (últimas linhas):\n```\n{fim}\n```\n", v.comando));
        }
    }
    if let Some(r) = &t.revisao {
        let veredito = if r.veredito == "aprovar" { "APROVARIA" } else { "PEDE AJUSTES" };
        texto.push_str(&format!("\nRevisor: {veredito} — {}\n", r.resumo.trim()));
        let mut baixas = 0;
        for (n, p) in r.problemas.iter().enumerate().map(|(i, p)| (i + 1, p)) {
            if p.gravidade == "baixa" {
                baixas += 1;
            } else {
                texto.push_str(&format!("{n}. [{}] {}: {}\n", p.gravidade, crate::revisor::onde(p), p.descricao.trim()));
            }
        }
        if baixas > 0 {
            texto.push_str(&format!("(e {} de gravidade baixa)\n", crate::ui::componentes::contagem(baixas, "problema", "problemas")));
        }
    }
    if !t.sobreposicao.is_empty() {
        texto.push_str(&format!(
            "\nAtenção: outros chats mexem nos mesmos arquivos ({}). Junte um de cada vez.\n",
            t.sobreposicao.join("; ")
        ));
    }
}

/// O aviso que acorda o Mestre quando todas as partes que começaram têm resultado.
pub fn conclusao_da_divisao(id: u64, d: &Divisao, nome_do_projeto: &dyn Fn(&str) -> String) -> Option<String> {
    let criadas: Vec<&Parte> = d.partes.iter().filter(|p| p.criada.is_some() && !p.removida).collect();
    if d.conclusao_avisada || criadas.is_empty() || criadas.iter().any(|p| p.resultado.is_none()) {
        return None;
    }
    let mut itens: Vec<String> = criadas
        .iter()
        .map(|p| {
            let n = p.criada.as_ref().map_or(0, |c| c.tarefa);
            let onde = format!("chat {n} \"{}\" ({})", p.titulo, nome_do_projeto(&p.projeto));
            let detalhe = p.detalhe.as_deref().unwrap_or("").trim();
            match p.resultado.as_deref() {
                Some("juntada") if detalhe.is_empty() => format!("{onde} juntado"),
                Some("juntada") => format!("{onde} juntado {detalhe}"),
                _ if detalhe.is_empty() => format!("{onde} cancelado"),
                _ => format!("{onde} cancelado: {detalhe}"),
            }
        })
        .collect();
    for p in d.partes.iter().filter(|p| p.criada.is_none() && !p.removida) {
        itens.push(format!("parte que não começou: \"{}\" ({})", p.titulo, p.falha.as_deref().unwrap_or("sem motivo")));
    }
    Some(format!(
        "A divisão {id} terminou: {}. Responda ao usuário com o resumo por projeto (regra 6).",
        itens.join("; ")
    ))
}

/// O primeiro parágrafo de texto de um README (sem títulos, selos, imagens nem HTML), até 200
/// caracteres.
pub fn primeiro_paragrafo(texto: &str) -> String {
    let mut linhas: Vec<&str> = Vec::new();
    let mut em_codigo = false;
    for linha in texto.lines() {
        let l = linha.trim();
        if l.starts_with("```") {
            em_codigo = !em_codigo;
            if !linhas.is_empty() {
                break;
            }
            continue;
        }
        let enfeite = l.starts_with('#')
            || l.starts_with('<')
            || l.starts_with("![")
            || l.starts_with("[![")
            || l.starts_with('|')
            || l.starts_with("---")
            || l.starts_with("===")
            || l.starts_with('>');
        if em_codigo || l.is_empty() || enfeite {
            if !linhas.is_empty() {
                break;
            }
            continue;
        }
        linhas.push(l);
    }
    let mut junto = linhas.join(" ").split_whitespace().collect::<Vec<_>>().join(" ");
    // A frase que só apresenta o que vem depois ("Para rodar:") fica de fora.
    if junto.ends_with(':')
        && let Some(fim) = junto.rfind(". ")
    {
        junto.truncate(fim);
    }
    let junto = junto.trim_end_matches('.').to_string();
    if junto.chars().count() > 200 {
        format!("{}…", junto.chars().take(199).collect::<String>().trim_end())
    } else {
        junto
    }
}

/// Uma linha do catálogo do prompt do Mestre. `readme`: o primeiro parágrafo do README da pasta
/// principal e de onde veio, para o projeto sem descrição; `git`: a pasta tem repositório.
pub fn linha_do_catalogo(p: &Projeto, readme: Option<(String, &str)>, git: &dyn Fn(&Path) -> bool) -> String {
    let descricao = p.descricao.split_whitespace().collect::<Vec<_>>().join(" ");
    let descricao = descricao.trim_end_matches('.').to_string();
    let descricao = if !descricao.is_empty() {
        descricao
    } else if let Some((texto, origem)) = readme.filter(|(t, _)| !t.is_empty()) {
        format!("{texto} (do {origem})")
    } else {
        "sem descrição".into()
    };
    let pastas: Vec<String> = p
        .pastas
        .iter()
        .map(|l| {
            if l.propria {
                return "pasta própria".to_string();
            }
            let mut texto = com_til(&l.caminho);
            if !l.caminho.is_dir() {
                texto.push_str(" (não encontrada)");
            } else if git(&l.caminho) {
                texto.push_str(" [git]");
            }
            texto
        })
        .collect();
    let pastas = if pastas.is_empty() { "nenhuma".to_string() } else { pastas.join(", ") };
    format!("- {} (#{}) — {descricao}. Pastas: {pastas}.", p.nome, p.chave())
}

pub const LINHA_DOS_AVULSOS: &str = "- Avulsos (#avulsos, projeto \"\") — conversas sem projeto e sem pasta de código.";

/// O que o usuário perdeu fora da janela, das ocorrências do período (sem o que ele mesmo fez) e
/// dos cartões que abriram nele.
pub fn resumo_desde(desde: u64, ate: u64, ocorrencias: &[Ocorrencia], cartoes: &[Cartao]) -> Option<ResumoVolta> {
    let mut linhas: Vec<LinhaResumo> = Vec::new();
    for o in ocorrencias.iter().filter(|o| o.quando > desde && o.quando <= ate && o.autor != "usuario") {
        let grupo = match o.tipo.as_str() {
            "entrega" | "juntou" => "terminou",
            "permissao" | "ciclo" | "pergunta" => "precisa",
            "falhou" | "parou" | "conflito" => "falhou",
            _ => continue,
        };
        // Uma linha por chat em cada grupo: a mais nova (a junção vem depois da entrega).
        if !o.chat.is_empty() {
            linhas.retain(|l| !(l.grupo == grupo && l.chat == o.chat));
        }
        linhas.push(LinhaResumo { grupo, projeto: o.projeto.clone(), chat: o.chat.clone(), texto: o.texto.clone() });
    }
    for c in cartoes.iter().filter(|c| c.aberto() && c.criado_em > desde && c.criado_em <= ate) {
        let texto = match &c.tipo {
            TipoCartao::Divisao(d) => {
                let n = d.partes.iter().filter(|p| !p.removida).count();
                format!("A divisão {} espera o Começar ({})", c.id, crate::ui::componentes::contagem(n, "parte", "partes"))
            }
            TipoCartao::Projeto(s) => format!("O Mestre sugere criar o projeto {}", s.nome),
            TipoCartao::Memoria { texto } => format!("O Mestre quer lembrar: {texto}"),
            TipoCartao::Desconhecido(_) => format!("Cartão {} aberto", c.id),
        };
        linhas.push(LinhaResumo { grupo: "precisa", projeto: String::new(), chat: c.chat.clone(), texto });
    }
    if linhas.is_empty() {
        return None;
    }
    let ordem = |g: &str| ["terminou", "precisa", "falhou"].iter().position(|x| *x == g).unwrap_or(3);
    linhas.sort_by_key(|l| ordem(l.grupo));
    Some(ResumoVolta { desde, ate, linhas })
}

/// O contexto do último turno passou de 70% da janela.
pub fn contexto_cheio(contexto: Option<u64>, janela: u64) -> bool {
    janela > 0 && contexto.is_some_and(|c| c as f64 / janela as f64 >= CONTEXTO_CHEIO)
}

/// As entradas do turno do Mestre com as do usuário primeiro, sem mudar a ordem dentro de cada grupo.
pub fn usuario_primeiro(entradas: &mut [Entrada]) {
    entradas.sort_by_key(|e| !e.do_usuario());
}

// ---------- o Mestre no motor ----------

/// O que o motor lembra do README de cada pasta, pelo mtime: o prompt só muda quando o arquivo muda.
pub type Readmes = RefCell<HashMap<PathBuf, (Option<SystemTime>, String)>>;

/// Uma divisão que começou: o que foi para cada chat, o que o usuário tirou e o que não começou.
#[derive(Default)]
pub struct Comecada {
    pub enviadas: Vec<String>,
    pub removidas: Vec<String>,
    pub falhas: Vec<String>,
}

impl Comecada {
    /// "chat novo 12 \"Checkout com Pix\" (Ninee); pedido enviado ao chat 7 \"Docs\" (Vitral)."
    pub fn texto(&self) -> String {
        let mut texto = if self.enviadas.is_empty() {
            "nenhuma parte começou.".to_string()
        } else {
            format!("{}.", self.enviadas.join("; "))
        };
        if !self.removidas.is_empty() {
            texto.push_str(&format!(" Parte removida: {}.", self.removidas.join(", ")));
        }
        if !self.falhas.is_empty() {
            texto.push_str(&format!(" Parte que não começou: {}.", self.falhas.join("; ")));
        }
        texto
    }
}

impl Motor {
    // ---------- leitura para a interface ----------

    pub fn cartao(&self, id: u64) -> Option<&Cartao> {
        self.cartoes.iter().find(|c| c.id == id)
    }

    pub fn cartoes(&self) -> &[Cartao] {
        &self.cartoes
    }

    /// Os pares de chats que trocaram mensagens demais e esperam o usuário.
    pub fn ciclos_travados(&self) -> Vec<(String, String)> {
        self.ciclos.travados()
    }

    pub fn memoria_do_mestre(&self) -> String {
        self.dados.memoria()
    }

    /// Grava a memória editada pelo usuário (o diálogo confere antes se o arquivo mudou no meio).
    pub fn gravar_memoria_do_mestre(&self, texto: &str) -> anyhow::Result<()> {
        self.dados.gravar_memoria(texto)
    }

    // ---------- o turno do Mestre ----------

    /// O primeiro parágrafo do README (ou do CLAUDE.md) da pasta, pelo mtime: só relê quando o
    /// arquivo muda.
    pub(super) fn readme_de(&self, pasta: &Path) -> Option<(String, &'static str)> {
        for (arquivo, origem) in [("README.md", "README"), ("readme.md", "README"), ("README", "README"), ("CLAUDE.md", "CLAUDE.md")] {
            let caminho = pasta.join(arquivo);
            let Ok(meta) = std::fs::metadata(&caminho) else {
                continue;
            };
            let mtime = meta.modified().ok();
            if let Some((visto, texto)) = self.readmes.borrow().get(&caminho)
                && *visto == mtime
            {
                return Some((texto.clone(), origem)).filter(|(t, _)| !t.is_empty());
            }
            let texto = std::fs::read_to_string(&caminho).map(|t| primeiro_paragrafo(&t)).unwrap_or_default();
            self.readmes.borrow_mut().insert(caminho, (mtime, texto.clone()));
            if !texto.is_empty() {
                return Some((texto, origem));
            }
        }
        None
    }

    /// Os projetos à vista, um por linha, e os Avulsos.
    pub(super) fn catalogo(&self) -> String {
        let mut linhas: Vec<String> = self
            .projetos_visiveis()
            .into_iter()
            .map(|p| {
                let readme = p.principal().filter(|_| p.descricao.trim().is_empty()).and_then(|l| self.readme_de(&l.caminho));
                linha_do_catalogo(p, readme, &|pasta| pasta.join(".git").exists())
            })
            .collect();
        linhas.push(LINHA_DOS_AVULSOS.into());
        linhas.join("\n")
    }

    /// O prompt, as ferramentas bloqueadas, as pastas que ele lê e o modelo padrão do Mestre. Ele lê
    /// todas as pastas dos projetos e as cópias, e não escreve em nenhuma.
    pub(super) fn opcoes_do_mestre(&self) -> (String, Vec<String>, Vec<PathBuf>, String) {
        let prompt = prompts::mestre(&self.catalogo(), &self.dados.memoria(), &self.config);
        let mut pastas = vec![self.dados.pasta.join("copias")];
        for p in self.projetos.iter().filter(|p| !p.arquivado) {
            for l in p.pastas.iter().filter(|l| l.caminho.is_dir()) {
                if !pastas.contains(&l.caminho) {
                    pastas.push(l.caminho.clone());
                }
            }
        }
        // As cópias antigas (`.vitral/worktrees`) das tarefas abertas.
        for t in self.tarefas.iter().filter(|t| !t.estado.encerrada()) {
            if let Some(area) = t.worktree.as_deref().and_then(Path::parent)
                && !area.starts_with(self.dados.pasta.join("copias"))
                && area.is_dir()
                && !pastas.iter().any(|p| p == area)
            {
                pastas.push(area.to_path_buf());
            }
        }
        let bloqueadas = BLOQUEADAS_DO_MESTRE.map(String::from).to_vec();
        (prompt, bloqueadas, pastas, self.config.modelo_mestre.clone())
    }

    /// O que vai junto com a mensagem do usuário ao Mestre (só o Claude vê): as referências e a
    /// divisão que ainda espera o Começar.
    pub(super) fn extra_para_o_mestre(&self, texto: &str) -> String {
        let rodando = |id: &str| self.execucao(id).rodando();
        let mut extra = resolver_referencias(texto, &self.projetos, &self.chats, &self.tarefas, &rodando).texto;
        for c in self.cartoes.iter().filter(|c| c.aberto() && c.chat == ID_MESTRE && c.divisao().is_some()) {
            if !extra.is_empty() {
                extra.push('\n');
            }
            extra.push_str(&format!("[Vitral] A divisão {} ainda espera o Começar.", c.id));
        }
        extra
    }

    /// O começo do primeiro turno de uma sessão nova do Mestre: os pedidos abertos dele, os cartões
    /// abertos e as últimas mensagens do usuário. A conversa inteira continua na tela.
    pub(super) fn bloco_de_passagem_do_mestre(&self, motivo: MotivoPassagem) -> String {
        let porque = match motivo {
            MotivoPassagem::Expirou => "a sessão anterior do Claude Code expirou",
            MotivoPassagem::ContextoCheio => "o contexto da sessão anterior encheu",
        };
        let mut bloco = format!(
            "[Vitral] Passagem de sessão: {porque}, e esta é uma nova. A conversa continua na tela do \
             usuário; use as ferramentas de estado (estado, listar_chats, ver_chat, historico) para o que \
             precisar saber."
        );
        let pedidos: Vec<String> = self
            .tarefas
            .iter()
            .filter(|t| t.dono() == Some(ID_MESTRE) && !t.estado.encerrada() && t.estado != EstadoTarefa::Conversa)
            .map(|t| format!("- chat {} \"{}\" ({}): {}", t.id, t.titulo, self.nome_do_projeto(&t.projeto), t.estado.nome()))
            .collect();
        if !pedidos.is_empty() {
            bloco.push_str(&format!("\n\nSeus pedidos abertos:\n{}", pedidos.join("\n")));
        }
        let cartoes: Vec<String> = self
            .cartoes
            .iter()
            .filter(|c| c.aberto() && c.chat == ID_MESTRE)
            .map(|c| format!("- {}", self.texto_do_cartao(c)))
            .collect();
        if !cartoes.is_empty() {
            bloco.push_str(&format!("\n\nCartões esperando o usuário:\n{}", cartoes.join("\n")));
        }
        let ultimas: Vec<String> = self
            .chat(ID_MESTRE)
            .map(|c| {
                let mut falas: Vec<String> = c
                    .itens
                    .iter()
                    .rev()
                    .filter_map(|i| match i {
                        Item::Usuario { texto, .. } => Some(format!("- {}", git::inicio_do_texto(texto.trim(), 1200))),
                        _ => None,
                    })
                    .take(5)
                    .collect();
                falas.reverse();
                falas
            })
            .unwrap_or_default();
        if !ultimas.is_empty() {
            bloco.push_str(&format!("\n\nAs últimas mensagens do usuário:\n{}", ultimas.join("\n")));
        }
        bloco
    }

    /// No fim de um turno do Mestre: com o contexto acima de 70%, o próximo turno começa numa sessão
    /// nova, com o bloco de passagem.
    pub(super) fn conferir_contexto_do_mestre(&mut self) {
        let Some(c) = self.chat(ID_MESTRE) else {
            return;
        };
        let janela = c.janela.unwrap_or_else(|| janela_padrao(c.modelo_atual.as_deref().or(c.modelo.as_deref())));
        if !c.sessao_criada || !contexto_cheio(c.contexto, janela) {
            return;
        }
        if let Some(c) = self.chat_mut(ID_MESTRE) {
            c.sessao = uuid::Uuid::new_v4().to_string();
            c.sessao_criada = false;
            c.contexto = None;
        }
        self.passagens.insert(ID_MESTRE.into(), MotivoPassagem::ContextoCheio);
        self.avisar(ID_MESTRE, "Sessão nova (contexto cheio).", false);
    }

    /// Um aviso na conversa do Mestre (a migração, o Vitral antigo, as boas-vindas).
    pub fn avisar_no_mestre(&mut self, texto: &str) {
        if self.chat(ID_MESTRE).is_some() {
            self.avisar(ID_MESTRE, texto, false);
        }
    }

    /// Uma nota ao Mestre que não o acorda: entra junto com a próxima mensagem que abre turno.
    pub(super) fn nota_ao_mestre(&mut self, texto: String, cx: &mut Context<Self>) {
        let nota = Entrada { de: Some("Vitral".into()), texto, nota: true, ..Default::default() };
        self.entregar_entrada(ID_MESTRE, nota, cx);
    }

    /// Uma mensagem do Vitral que acorda o Mestre.
    pub(super) fn acordar_mestre(&mut self, texto: String, cx: &mut Context<Self>) {
        self.entregar(ID_MESTRE, Some("Vitral".into()), texto, cx);
    }

    /// Ao abrir: o Vitral antigo mexeu numa pasta depois da migração. Avisa no Mestre uma vez por
    /// mudança (o aviso tem a data, e o mesmo texto não se repete).
    pub(super) fn conferir_vitral_antigo(&mut self) {
        let registro = dados::ler_valor_sem_escrever(&self.dados.arquivo_migracao())
            .ok()
            .flatten()
            .and_then(|v| serde_json::from_value::<crate::migracao::Registro>(v).ok());
        let Some(registro) = registro else {
            return;
        };
        for raiz in &registro.raizes {
            let Ok(meta) = std::fs::metadata(raiz.raiz.join(".vitral").join("estado.json")) else {
                continue;
            };
            let mtime = meta.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
            if (mtime, meta.len()) == (raiz.mtime_estado, raiz.tamanho_estado) {
                continue;
            }
            let texto = format!(
                "O Vitral antigo mexeu em {} depois da importação ({}). O que foi feito lá não aparece aqui.",
                com_til(&raiz.raiz),
                crate::modelo::quando_local(mtime).trim_start_matches("às ").trim_start_matches("em ")
            );
            let ja = self
                .chat(ID_MESTRE)
                .is_some_and(|c| c.itens.iter().any(|i| matches!(i, Item::Aviso { texto: t, .. } if *t == texto)));
            if !ja {
                self.avisar_no_mestre(&texto);
            }
        }
    }

    // ---------- cartões ----------

    fn texto_do_cartao(&self, c: &Cartao) -> String {
        match &c.tipo {
            TipoCartao::Divisao(d) => {
                let n = d.partes.iter().filter(|p| !p.removida).count();
                format!("divisão {} ({}): {}", c.id, crate::ui::componentes::contagem(n, "parte", "partes"), d.resumo)
            }
            TipoCartao::Projeto(s) => format!("sugestão de criar o projeto {}", s.nome),
            TipoCartao::Memoria { texto } => format!("pedido para lembrar: {texto}"),
            TipoCartao::Desconhecido(_) => format!("cartão {} de uma versão mais nova", c.id),
        }
    }

    fn novo_cartao(&mut self, tipo: TipoCartao) -> u64 {
        let id = self.proximo_cartao.max(self.cartoes.iter().map(|c| c.id + 1).max().unwrap_or(1)).max(1);
        self.proximo_cartao = id + 1;
        self.cartoes.push(Cartao {
            id,
            chat: ID_MESTRE.into(),
            criado_em: agora(),
            decidido_em: 0,
            estado: EstadoCartao::Aberto,
            tipo,
            resto: Default::default(),
        });
        if let Some(c) = self.chat_mut(ID_MESTRE) {
            c.itens.push(Item::Cartao { id });
            c.atualizado_em = agora();
        }
        id
    }

    /// Cartões gravam na hora: o estado e a conversa do Mestre.
    fn gravar_cartoes(&mut self) {
        self.salvar();
        if let Some(c) = self.chat(ID_MESTRE) {
            let _ = self.dados.salvar_chat(c);
        }
        self.chats_sujos.remove(ID_MESTRE);
    }

    fn decidir(&mut self, cartao: u64, estado: EstadoCartao) {
        if let Some(c) = self.cartoes.iter_mut().find(|c| c.id == cartao) {
            c.estado = estado;
            c.decidido_em = agora();
        }
    }

    /// O × de uma parte do cartão aberto (e o desfazer).
    pub fn tirar_parte(&mut self, cartao: u64, indice: usize, removida: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let c = self.cartoes.iter_mut().find(|c| c.id == cartao).ok_or("Cartão não encontrado.")?;
        if !c.aberto() {
            return Err("Esta divisão já foi decidida.".into());
        }
        let parte = c.divisao_mut().and_then(|d| d.partes.get_mut(indice)).ok_or("Parte não encontrada.")?;
        parte.removida = removida;
        self.gravar_cartoes();
        cx.notify();
        Ok(())
    }

    /// "Começar": cria os chats (ou reaproveita o indicado) e manda o pedido de cada parte. O Mestre
    /// fica sabendo por uma nota que não o acorda.
    pub fn comecar_divisao(&mut self, cartao: u64, cx: &mut Context<Self>) -> Result<String, String> {
        let c = self.cartao(cartao).ok_or("Cartão não encontrado.")?;
        if !c.aberto() || c.divisao().is_none() {
            return Err("Esta divisão já foi decidida.".into());
        }
        if c.divisao().is_some_and(|d| d.partes.iter().all(|p| p.removida)) {
            return Err("Todas as partes foram tiradas.".into());
        }
        let comecada = self.comecar_divisao_agora(cartao, "usuario", cx);
        self.nota_ao_mestre(format!("O usuário começou a divisão {cartao}: {}", comecada.texto()), cx);
        cx.notify();
        Ok(comecada.texto())
    }

    /// "Descartar": o cartão vira recusado e o Mestre recebe a nota sem acordar.
    pub fn descartar_cartao(&mut self, cartao: u64, cx: &mut Context<Self>) {
        let Some(c) = self.cartao(cartao).filter(|c| c.aberto()) else {
            return;
        };
        match &c.tipo {
            TipoCartao::Projeto(_) => {
                let _ = self.responder_sugestao(cartao, None, cx);
                return;
            }
            TipoCartao::Memoria { .. } => {
                let _ = self.responder_memoria(cartao, false, cx);
                return;
            }
            _ => {}
        }
        self.decidir(cartao, EstadoCartao::Recusado);
        self.registrar("divisao", "usuario", ID_MESTRE, "", None, &format!("Divisão {cartao} descartada"));
        self.gravar_cartoes();
        self.nota_ao_mestre(format!("O usuário descartou a divisão {cartao}."), cx);
        cx.notify();
    }

    /// "Lembrar" ou "Não" no cartão da memória.
    pub fn responder_memoria(&mut self, cartao: u64, aceitar: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let c = self.cartao(cartao).ok_or("Cartão não encontrado.")?;
        let TipoCartao::Memoria { texto } = &c.tipo else {
            return Err("Este cartão não é da memória.".into());
        };
        if !c.aberto() {
            return Err("Este cartão já foi decidido.".into());
        }
        if aceitar {
            let nova = lembrar_em(&self.dados.memoria(), texto)?;
            self.dados.gravar_memoria(&nova).map_err(|e| format!("Não gravou a memória: {e:#}"))?;
        }
        self.decidir(cartao, if aceitar { EstadoCartao::Aceito } else { EstadoCartao::Recusado });
        self.gravar_cartoes();
        cx.notify();
        Ok(())
    }

    /// "Criar projeto…" (com o formulário preenchido) ou "Agora não" (`None`) na sugestão do Mestre.
    /// Criar e aceitar o cartão vão numa gravação só; repetir não cria outro.
    pub fn responder_sugestao(&mut self, cartao: u64, novo: Option<NovoProjeto>, cx: &mut Context<Self>) -> Task<anyhow::Result<Option<String>>> {
        let Some(c) = self.cartao(cartao) else {
            return Task::ready(Err(anyhow::anyhow!("Cartão não encontrado.")));
        };
        let TipoCartao::Projeto(sugestao) = &c.tipo else {
            return Task::ready(Err(anyhow::anyhow!("Este cartão não é uma sugestão de projeto.")));
        };
        let (nome, criado_em) = (sugestao.nome.clone(), c.criado_em);
        match (c.estado, novo) {
            (EstadoCartao::Aceito, _) => Task::ready(Ok(sugestao.criado.clone())),
            (EstadoCartao::Aberto, None) => {
                self.decidir(cartao, EstadoCartao::Recusado);
                self.gravar_cartoes();
                self.acordar_mestre(format!("O usuário não quis criar o projeto {nome} agora. Siga sem ele ou pergunte."), cx);
                cx.notify();
                Task::ready(Ok(None))
            }
            (EstadoCartao::Aberto, Some(novo)) => {
                // Um projeto com o mesmo nome criado depois do cartão (o usuário criou à mão) é este.
                let chave = dados::chave(&novo.nome);
                if let Some(id) = self.projetos.iter().find(|p| p.chave() == chave && p.criado_em >= criado_em).map(|p| p.id.clone()) {
                    self.aceitar_sugestao(cartao, &id, cx);
                    self.gravar_cartoes();
                    return Task::ready(Ok(Some(id)));
                }
                if !self.criando_projeto_de.insert(cartao) {
                    return Task::ready(Err(anyhow::anyhow!("O projeto desta sugestão já está sendo criado.")));
                }
                let tarefa = self.criar_projeto_com(novo, Some(cartao), cx);
                cx.spawn(async move |motor, cx| {
                    let resultado = tarefa.await;
                    let _ = motor.update(cx, |m, _| m.criando_projeto_de.remove(&cartao));
                    resultado.map(Some)
                })
            }
            _ => Task::ready(Err(anyhow::anyhow!("Esta sugestão já foi decidida."))),
        }
    }

    /// O cartão da sugestão aceito com o projeto criado (chamado dentro da mesma gravação que cria
    /// o projeto) e o Mestre acordado para continuar o pedido.
    pub(super) fn aceitar_sugestao(&mut self, cartao: u64, projeto: &str, cx: &mut Context<Self>) {
        let (mut pendente, mut instrucoes) = (String::new(), String::new());
        if let Some(c) = self.cartoes.iter_mut().find(|c| c.id == cartao)
            && let TipoCartao::Projeto(s) = &mut c.tipo
        {
            s.criado = Some(projeto.to_string());
            pendente = s.pedido_pendente.trim().to_string();
            instrucoes = s.instrucoes.trim().to_string();
            c.estado = EstadoCartao::Aceito;
            c.decidido_em = agora();
        }
        // Sem instruções no formulário, valem as que o Mestre sugeriu.
        if let Some(p) = self.projetos.iter_mut().find(|p| p.id == projeto)
            && p.instrucoes.trim().is_empty()
        {
            p.instrucoes = instrucoes;
        }
        let Some(p) = self.projeto(projeto) else {
            return;
        };
        let pastas: Vec<String> = p
            .pastas
            .iter()
            .map(|l| if l.propria { format!("pasta própria {}", com_til(&l.caminho)) } else { com_til(&l.caminho) })
            .collect();
        let nome = p.nome.clone();
        self.registrar("projeto", "usuario", ID_MESTRE, projeto, None, &format!("Projeto {nome} criado pela sugestão do Mestre"));
        let continuar = if pendente.is_empty() { "Continue o pedido.".to_string() } else { format!("Continue o pedido: {pendente}") };
        self.acordar_mestre(format!("O usuário criou o projeto {nome} ({}). {continuar}", pastas.join(", ")), cx);
    }

    /// Cria os chats da divisão (ou reaproveita os indicados), põe o pedido de cada um na fila e grava
    /// tudo de uma vez; só então os turnos começam. A parte que já não vale ganha a falha e as outras
    /// seguem.
    pub(super) fn comecar_divisao_agora(&mut self, cartao: u64, autor: &str, cx: &mut Context<Self>) -> Comecada {
        let Some(divisao) = self.cartao(cartao).and_then(|c| c.divisao()).cloned() else {
            return Comecada::default();
        };
        let conta_do_mestre = self.chat(ID_MESTRE).and_then(|m| m.conta.clone());
        let mut partes = divisao.partes.clone();
        let mut comecada = Comecada::default();
        let mut enviadas: Vec<(usize, String, u64, bool)> = Vec::new();
        let plano = {
            let ocupado = |id: &str| self.ocupado_para_pedido(id);
            planejar_comeco(&partes, &self.projetos, &self.chats, &self.tarefas, &ocupado)
        };
        for (i, plano) in plano.into_iter().enumerate() {
            if partes[i].removida {
                comecada.removidas.push(format!("\"{}\"", partes[i].titulo));
            }
            let criado = match plano {
                PlanoDaParte::Pular => continue,
                PlanoDaParte::Falha(motivo) => {
                    partes[i].falha = Some(motivo);
                    continue;
                }
                PlanoDaParte::Reabrir(parte) => {
                    let chat = parte.chat.clone().unwrap_or_default();
                    self.reabrir_pedido(&chat, &parte, cartao).map(|t| (parte, chat, t, false))
                }
                PlanoDaParte::Novo(parte) => {
                    self.chat_para_parte(&parte, cartao, conta_do_mestre.clone(), cx).map(|(c, t)| (parte, c, t, true))
                }
            };
            match criado {
                Ok((parte, chat, tarefa, novo)) => {
                    partes[i].projeto = parte.projeto.clone();
                    partes[i].chat = parte.chat.clone();
                    partes[i].criada = Some(ParteCriada { chat: chat.clone(), tarefa });
                    enviadas.push((i, chat, tarefa, novo));
                }
                Err(motivo) => partes[i].falha = Some(motivo),
            }
        }
        for p in partes.iter().filter(|p| p.falha.is_some() && p.criada.is_none() && !p.removida) {
            comecada.falhas.push(format!("\"{}\" ({})", p.titulo, p.falha.as_deref().unwrap_or_default()));
        }
        // Os pedidos, já sabendo o que foi para cada chat ("Outras partes desta divisão").
        let mut chats = Vec::new();
        for (i, chat, tarefa, novo) in &enviadas {
            let p = &partes[*i];
            let projeto = self.nome_do_projeto(&p.projeto);
            let outras: Vec<String> = enviadas
                .iter()
                .filter(|o| o.2 != *tarefa)
                .map(|o| format!("chat {} no {} ({})", o.2, self.nome_do_projeto(&partes[o.0].projeto), partes[o.0].titulo))
                .collect();
            let Some(t) = self.tarefa(*tarefa) else {
                continue;
            };
            let info = prompts::PedidoInfo {
                projeto: &projeto,
                titulo: &p.titulo,
                arquivos: &p.arquivos,
                outras: &outras,
                pedido_original: &divisao.pedido_original,
            };
            let texto = prompts::pedido_do_mestre(t, &info);
            let titulo_do_chat = self.chat(chat).map_or_else(|| p.titulo.clone(), |c| c.titulo.clone());
            comecada.enviadas.push(if *novo {
                format!("chat novo {tarefa} \"{}\" ({projeto})", p.titulo)
            } else {
                format!("pedido enviado ao chat {tarefa} \"{titulo_do_chat}\" ({projeto})")
            });
            let entrada = Entrada {
                de: Some("Mestre".into()),
                remetente: Some(ID_MESTRE.into()),
                texto,
                anexos: divisao.anexos.clone(),
                ..Default::default()
            };
            self.filas.entry(chat.clone()).or_default().push(entrada);
            self.registrar("pedido", autor, chat, &p.projeto, Some(*tarefa), &format!("Pedido \"{}\"", p.titulo));
            chats.push(chat.clone());
        }
        if let Some(c) = self.cartoes.iter_mut().find(|c| c.id == cartao) {
            c.estado = EstadoCartao::Aceito;
            c.decidido_em = agora();
            if let Some(d) = c.divisao_mut() {
                d.partes = partes;
            }
        }
        self.registrar("divisao", autor, ID_MESTRE, "", None, &format!("Divisão {cartao} começada: {}", comecada.texto()));
        // O ponto atômico: os chats, as tarefas, as filas e o cartão aceito numa gravação só.
        self.gravar_cartoes();
        for chat in chats {
            self.entregar_fila(&chat, cx);
        }
        self.conferir_conclusao(cartao, cx);
        comecada
    }

    /// O chat tem processo, fila, espera ou cópia sendo preparada: não recebe pedido agora.
    pub(super) fn ocupado_para_pedido(&self, id: &str) -> bool {
        !self.chat_livre(id)
            || self.na_fila(id) > 0
            || self.esperando_pasta.iter().any(|c| c == id)
            || self.chat(id).and_then(|c| c.tarefa).is_some_and(|t| {
                self.preparando.contains(&t) || self.na_fila_pesada(t) || self.juntando.contains(&t) || self.sincronizando.contains(&t)
            })
    }

    /// Um chat novo para a parte, já com o pedido do Mestre aberto (rodada 1).
    fn chat_para_parte(&mut self, p: &Parte, cartao: u64, conta: Option<String>, cx: &mut Context<Self>) -> Result<(String, u64), String> {
        let id = self.criar_chat(&p.projeto, p.pasta.as_deref(), ID_MESTRE, cx)?;
        let n = self.chat(&id).and_then(|c| c.tarefa).ok_or("O chat novo ficou sem tarefa.")?;
        if let Some(c) = self.chat_mut(&id) {
            c.titulo = p.titulo.clone();
            c.conta = conta;
            if p.modelo.is_some() {
                c.modelo = p.modelo.clone();
            }
        }
        let origem = self.tarefa(n).map(|t| self.origem_da_tarefa(t)).unwrap_or_default();
        if let Some(t) = self.tarefa_mut(n) {
            t.titulo = p.titulo.clone();
            abrir_pedido(t, p, cartao);
            t.rodada = 1;
            if t.forma == Forma::DiretaGit {
                t.base = git::cabeca(&origem);
            }
        }
        Ok((id, n))
    }

    /// O pedido do Mestre num chat que já existe (em conversa): rodada nova, o mesmo título.
    fn reabrir_pedido(&mut self, chat: &str, p: &Parte, cartao: u64) -> Result<u64, String> {
        let n = self.chat(chat).and_then(|c| c.tarefa).ok_or("O chat não recebe pedidos.")?;
        let origem = self.tarefa(n).map(|t| self.origem_da_tarefa(t)).unwrap_or_default();
        let t = self.tarefa_mut(n).ok_or("O chat não recebe pedidos.")?;
        t.rodada += 1;
        abrir_pedido(t, p, cartao);
        if t.forma == Forma::DiretaGit {
            t.base = git::cabeca(&origem);
        }
        Ok(n)
    }

    /// A parte do cartão do chat terminou (juntada ou cancelada); a divisão inteira pode ter acabado.
    pub(super) fn parte_terminou(&mut self, tarefa: u64, resultado: &str, detalhe: Option<String>, cx: &mut Context<Self>) {
        let Some(cartao) = self.tarefa(tarefa).and_then(|t| t.divisao) else {
            return;
        };
        if let Some(d) = self.cartoes.iter_mut().find(|c| c.id == cartao).and_then(|c| c.divisao_mut())
            && let Some(p) = d.partes.iter_mut().find(|p| p.criada.as_ref().is_some_and(|c| c.tarefa == tarefa) && p.resultado.is_none())
        {
            p.resultado = Some(resultado.into());
            p.detalhe = detalhe;
        }
        self.conferir_conclusao(cartao, cx);
    }

    /// Toda parte que começou tem resultado: o Mestre é acordado uma vez para o resumo.
    fn conferir_conclusao(&mut self, cartao: u64, cx: &mut Context<Self>) {
        // A parte cujo chat saiu do Vitral (projeto apagado) conta como terminada.
        let tarefas: HashSet<u64> = self.tarefas.iter().map(|t| t.id).collect();
        if let Some(d) = self.cartoes.iter_mut().find(|c| c.id == cartao).and_then(|c| c.divisao_mut()) {
            for p in d.partes.iter_mut().filter(|p| p.resultado.is_none()) {
                if p.criada.as_ref().is_some_and(|c| !tarefas.contains(&c.tarefa)) {
                    p.resultado = Some("cancelada".into());
                    p.detalhe = Some("o chat saiu do Vitral".into());
                }
            }
        }
        let texto = {
            let Some(d) = self.cartao(cartao).and_then(|c| c.divisao()) else {
                return;
            };
            let nome = |p: &str| self.nome_do_projeto(p);
            conclusao_da_divisao(cartao, d, &nome)
        };
        let Some(texto) = texto else {
            return;
        };
        if let Some(d) = self.cartoes.iter_mut().find(|c| c.id == cartao).and_then(|c| c.divisao_mut()) {
            d.conclusao_avisada = true;
        }
        self.registrar("divisao", "vitral", ID_MESTRE, "", None, &format!("A divisão {cartao} terminou"));
        self.salvar();
        self.acordar_mestre(texto, cx);
    }

    // ---------- entrega e junção sozinha ----------

    /// A conferência da entrega de um pedido do Mestre terminou: verde, junta sozinho e o Mestre só
    /// recebe uma nota (que a junção manda); senão, o relatório compacto o acorda.
    pub(super) fn entrega_ao_mestre(&mut self, id: u64, relatorio: String, estatistica: Option<String>, erro: bool, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            return;
        };
        let titulo = self.chat(&t.trabalhador).map_or_else(|| t.titulo.clone(), |c| c.titulo.clone());
        let projeto = self.nome_do_projeto(&t.projeto);
        self.registrar("entrega", &t.trabalhador, &t.trabalhador, &t.projeto, Some(id), &format!("chat {id} \"{titulo}\" entregou"));
        if !erro && pode_juntar_sozinho(&t, &self.config) && self.recusa_aprovar(id).is_none() {
            let (envia, recebe) = smol::channel::bounded::<Resposta>(1);
            self.juntar_chat(id, "junção automática: verificação e revisor aprovaram".into(), None, false, "vitral", envia, cx);
            cx.spawn(async move |motor, cx| {
                let Ok(resposta) = recebe.recv().await else {
                    return;
                };
                if !resposta.erro {
                    return;
                }
                let _ = motor.update(cx, |m, cx| {
                    let t = m.tarefa(id).cloned().unwrap_or(t);
                    let texto = relatorio_compacto(&t, &projeto, &titulo, &relatorio, estatistica.as_deref(), erro, Some(&resposta.texto));
                    m.acordar_mestre(texto, cx);
                    m.salvar();
                });
            })
            .detach();
            return;
        }
        let texto = relatorio_compacto(&t, &projeto, &titulo, &relatorio, estatistica.as_deref(), erro, None);
        self.acordar_mestre(texto, cx);
    }

    /// O pedido do chat entrou na origem (ou, sem cópia, a rodada fechou): fica no registro, o Mestre
    /// recebe a nota da junção sozinha e a parte da divisão termina.
    pub(super) fn pedido_juntado(&mut self, t: &Tarefa, lugar: &str, autor: &str, cx: &mut Context<Self>) {
        let titulo = self.chat(&t.trabalhador).map_or_else(|| t.titulo.clone(), |c| c.titulo.clone());
        let lugar = com_til(Path::new(lugar));
        self.registrar("juntou", autor, &t.trabalhador, &t.projeto, Some(t.id), &format!("chat {} \"{titulo}\" entrou em {lugar}", t.id));
        if autor == "vitral" {
            let resumo = t
                .entrega
                .as_ref()
                .and_then(|e| e.resumo.lines().map(str::trim).find(|l| !l.is_empty()))
                .map_or_else(|| "sem resumo".to_string(), |l| git::inicio_do_texto(l, 200));
            let projeto = self.nome_do_projeto(&t.projeto);
            self.nota_ao_mestre(
                format!("Chat {} \"{titulo}\" ({projeto}) entregou e foi juntado sozinho em {lugar}: {resumo}", t.id),
                cx,
            );
        }
        if t.dono().is_some() {
            let detalhe = if t.tem_copia() { format!("em {lugar}") } else { format!("(direto em {lugar})") };
            self.parte_terminou(t.id, "juntada", Some(detalhe), cx);
        }
    }

    /// O fim da vez de um chat de projeto com pedido aberto: com a entrega desta rodada, vai para a
    /// conferência; sem ela, um lembrete por rodada e, depois dele, o turno aberto pelo dono (ou pelo
    /// lembrete) conta como entrega sem formulário.
    pub(super) fn fim_do_pedido(&mut self, chat: &Chat, t: &Tarefa, erro: bool, texto: &str, cx: &mut Context<Self>) {
        let Some(dono) = t.dono().filter(|_| t.estado.ativa()).map(str::to_string) else {
            return;
        };
        if erro {
            self.registrar("falhou", "vitral", &chat.id, &chat.projeto, Some(t.id), &format!("chat {} terminou com erro", t.id));
            self.tarefa_terminou(t.id, true, texto.to_string(), cx);
            return;
        }
        // Com ajudante aberto, o relatório dele é que acorda o chat de novo.
        let ajudantes = !self.ajudantes_abertos(&chat.id).is_empty();
        let pendente = self.aguardando_resposta.contains(&chat.id) || self.na_fila(&chat.id) > 0 || ajudantes;
        let do_dono = aberto_pelo_dono(self.ultima_entrada.get(&chat.id).map_or(&[][..], Vec::as_slice), &dono);
        match decidir_fim(t, pendente, do_dono) {
            FimDoPedido::Entregar { formal } => {
                let relatorio = match t.entrega.as_ref().filter(|_| formal) {
                    Some(e) => texto_da_entrega(e, &t.criterios),
                    None => texto.to_string(),
                };
                self.tarefa_terminou(t.id, false, relatorio, cx);
            }
            FimDoPedido::Lembrar => {
                if let Some(t) = self.tarefa_mut(t.id) {
                    t.lembrete_rodada = t.rodada;
                }
                let quem = if dono == ID_MESTRE {
                    "Mestre".to_string()
                } else {
                    self.chat(&dono).map_or_else(|| dono.clone(), |c| format!("chat \"{}\"", c.titulo))
                };
                let lembrete = format!(
                    "{LEMBRETE_ENTREGA} do {quem} (chat {}), verifique e chame entregar_tarefa; se não, continue.",
                    t.id
                );
                self.entregar(&chat.id, Some("Vitral".into()), lembrete, cx);
            }
            FimDoPedido::Esperar => {
                if let Some(t) = self.tarefa_mut(t.id) {
                    t.progresso = Some(if ajudantes { "esperando os ajudantes" } else { "esperando resposta do Mestre" }.into());
                    // Quando os ajudantes terminarem, o lembrete de entregar vale de novo.
                    if ajudantes {
                        t.lembrete_rodada = 0;
                    }
                }
            }
            FimDoPedido::Nada => {}
        }
    }

    /// O dono (o Mestre) cancela o pedido: o que ele mandou sai da fila, o chat para e volta a
    /// conversar com o usuário, sem dono. A conversa e a cópia continuam.
    pub(super) fn cancelar_pedido(&mut self, id: u64, motivo: String, cx: &mut Context<Self>) -> Resposta {
        self.cancelar_pedido_por(id, motivo, ID_MESTRE, cx)
    }

    /// `autor`: o Mestre (`cancelar_tarefa`) ou o usuário (arquivar o projeto).
    fn cancelar_pedido_por(&mut self, id: u64, motivo: String, autor: &str, cx: &mut Context<Self>) -> Resposta {
        if self.juntando.contains(&id) || self.tarefa(id).is_some_and(|t| t.juntando_desde > 0) {
            return Resposta::erro(format!("As mudanças do chat {id} estão sendo juntadas agora; espere o resultado."));
        }
        let Some(t) = self.tarefa(id).cloned() else {
            return Resposta::erro(format!("Não há chat {id}."));
        };
        if !(t.estado.ativa() || t.estado == EstadoTarefa::EmRevisao) {
            return Resposta::erro(format!("O chat {id} não tem pedido aberto ({}).", t.estado.nome()));
        }
        let (chat, dono) = (t.trabalhador.clone(), t.coordenador.clone());
        if let Some(fila) = self.filas.get_mut(&chat) {
            tirar_do_dono(fila, &dono);
            if fila.is_empty() {
                self.filas.remove(&chat);
            }
        }
        if self.na_fila(&chat) == 0 {
            self.espera.retain(|c| *c != chat);
        }
        if self.processos.contains_key(&chat) {
            self.parar(&chat, cx);
        }
        let motivo = if motivo.trim().is_empty() { "sem motivo informado".to_string() } else { motivo.trim().to_string() };
        self.parte_terminou(id, "cancelada", Some(motivo.clone()), cx);
        if let Some(t) = self.tarefa_mut(id) {
            t.estado = EstadoTarefa::Conversa;
            t.coordenador.clear();
            // A rodada nova descarta uma conferência que ainda volte.
            t.rodada += 1;
            (t.entrega, t.entrega_pendente, t.divisao) = (None, None, None);
            (t.progresso, t.parada) = (None, None);
            t.atualizada_em = agora();
        }
        let mut aviso = if autor == ID_MESTRE {
            format!("O Mestre cancelou o pedido: {motivo}.")
        } else if dono == ID_MESTRE {
            format!("O pedido do Mestre foi cancelado: {motivo}.")
        } else {
            let de = self.chat(&dono).map_or_else(|| dono.clone(), |c| c.titulo.clone());
            format!("O pedido de {de} foi cancelado: {motivo}.")
        };
        aviso.push_str(match t.forma {
            Forma::Copia => " A conversa e a cópia continuam.",
            Forma::DiretaGit => " A conversa continua; as mudanças já feitas ficam na pasta (Voltar no tempo desfaz).",
            Forma::Direta => " A conversa continua; as mudanças já feitas ficam na pasta.",
        });
        self.avisar(&chat, &aviso, false);
        self.registrar("cancelada", autor, &chat, &t.projeto, Some(id), &motivo);
        self.salvar();
        cx.notify();
        Resposta::ok(format!("Pedido ao chat {id} cancelado. A conversa e a cópia dele continuam."))
    }

    /// "Parar e arquivar": o projeto é arquivado, os pedidos de fora dele (do Mestre) aos chats dele
    /// são cancelados, com o aviso no chat e a nota ao dono, as partes dele saem das divisões que
    /// ainda esperam o Começar e o que trabalha para.
    pub fn parar_e_arquivar(&mut self, projeto: &str, parar: &[String], cx: &mut Context<Self>) {
        let nome = self.nome_do_projeto(projeto);
        let do_projeto: Vec<String> = self.chats.iter().filter(|c| c.projeto == projeto).map(|c| c.id.clone()).collect();
        let pedidos: Vec<(u64, String, String)> = self
            .tarefas
            .iter()
            .filter(|t| t.projeto == projeto && (t.estado.ativa() || t.estado == EstadoTarefa::EmRevisao))
            .filter(|t| !self.juntando.contains(&t.id) && t.juntando_desde == 0)
            .filter_map(|t| {
                let dono = t.dono().filter(|d| !do_projeto.iter().any(|c| c == d))?;
                Some((t.id, t.trabalhador.clone(), dono.to_string()))
            })
            .collect();
        let divisoes = tirar_partes_do_projeto(&mut self.cartoes, projeto);
        if !divisoes.is_empty() {
            self.gravar_cartoes();
        }
        // A nota vai antes: a conclusão de uma divisão que o cancelamento fechar já a leva.
        let do_mestre: Vec<String> = pedidos.iter().filter(|(_, _, d)| d == ID_MESTRE).map(|(t, _, _)| format!("chat {t}")).collect();
        let mut nota = Vec::new();
        if !do_mestre.is_empty() {
            nota.push(format!("os pedidos a {} foram cancelados", do_mestre.join(", ")));
        }
        if !divisoes.is_empty() {
            let quais = divisoes.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
            nota.push(format!("as partes dele saíram da divisão {quais}"));
        }
        if !nota.is_empty() {
            self.nota_ao_mestre(format!("O usuário arquivou o projeto {nome}: {}.", nota.join("; ")), cx);
        }
        // Arquivado antes de cancelar: a conclusão que o cancelamento acordar já não o vê no catálogo.
        self.arquivar_projeto(projeto, true, cx);
        let motivo = format!("o projeto {nome} foi arquivado");
        let mut parados = Vec::new();
        for (id, chat, dono) in pedidos {
            if self.cancelar_pedido_por(id, motivo.clone(), "usuario", cx).erro {
                continue;
            }
            parados.push(chat);
            if dono != ID_MESTRE && self.chat(&dono).is_some() {
                self.avisar(&dono, &format!("O pedido ao chat {id} foi cancelado: {motivo}."), false);
            }
        }
        for chat in parar.iter().filter(|c| !parados.contains(c)) {
            self.parar(chat, cx);
        }
    }

    // ---------- resumo ao voltar ----------

    /// A janela perdeu o foco (ou o app vai fechar): a hora fica gravada.
    pub fn usuario_saiu(&mut self) {
        self.visto_em = agora();
        self.salvar();
    }

    /// A janela voltou depois de `minutos_para_resumo` fora (ou abriu, com uma saída gravada): monta o
    /// resumo do que aconteceu, só na memória.
    pub fn usuario_voltou(&mut self, cx: &mut Context<Self>) {
        let desde = self.visto_em;
        let ate = agora();
        if desde == 0 || ate.saturating_sub(desde) < self.config.minutos_para_resumo * 60 {
            return;
        }
        if let Some(resumo) = resumo_desde(desde, ate, &self.ocorrencias, &self.cartoes) {
            self.resumo_volta = Some(resumo);
            cx.notify();
        }
    }

    pub fn fechar_resumo(&mut self, cx: &mut Context<Self>) {
        self.resumo_volta = None;
        cx.notify();
    }

    /// "Pedir ao Mestre um resumo": acorda o Mestre com a hora em que o usuário saiu.
    pub fn pedir_resumo_ao_mestre(&mut self, cx: &mut Context<Self>) {
        let desde = self.resumo_volta.as_ref().map_or(self.visto_em, |r| r.desde);
        let hora = crate::modelo::quando_local(desde);
        let hora = hora.trim_start_matches("às ");
        self.resumo_volta = None;
        self.acordar_mestre(
            format!("O usuário voltou e pediu um resumo do que aconteceu desde {hora}. Use historico e estado; responda curto, por projeto."),
            cx,
        );
        cx.notify();
    }
}

/// Abre o pedido do Mestre na tarefa: o que fazer, os critérios e a verificação da parte; o que era
/// da rodada anterior sai.
fn abrir_pedido(t: &mut Tarefa, p: &Parte, cartao: u64) {
    t.objetivo = p.pedido.clone();
    t.criterios = p.criterios.clone();
    t.verificacao = p.verificacao.clone();
    t.estado = EstadoTarefa::Rodando;
    t.coordenador = ID_MESTRE.into();
    t.divisao = Some(cartao);
    (t.entrega, t.entrega_pendente, t.relatorio) = (None, None, None);
    (t.revisao, t.ultima_verificacao, t.parada, t.ressalva) = (None, None, None, None);
    t.sobreposicao.clear();
    t.atualizada_em = agora();
}

/// As partes do projeto saem das divisões que ainda esperam o Começar (como o × de cada uma).
/// Devolve as divisões mexidas.
pub fn tirar_partes_do_projeto(cartoes: &mut [Cartao], projeto: &str) -> Vec<u64> {
    let mut mexidas = Vec::new();
    for c in cartoes.iter_mut().filter(|c| c.aberto()) {
        let id = c.id;
        let Some(d) = c.divisao_mut() else {
            continue;
        };
        let mut tirou = false;
        for p in d.partes.iter_mut().filter(|p| p.projeto == projeto && !p.removida) {
            p.removida = true;
            tirou = true;
        }
        if tirou {
            mexidas.push(id);
        }
    }
    mexidas
}

/// Tira da fila o que o dono mandou (o pedido e os ajustes dele); o resto (do usuário, as notas)
/// fica.
pub fn tirar_do_dono(fila: &mut Vec<Entrada>, dono: &str) {
    fila.retain(|e| e.remetente.as_deref() != Some(dono));
}

// ---------- as ferramentas ----------

impl Motor {
    /// As ferramentas do Mestre, as que mudaram para todos (`listar_chats`, `mensagem_para_chat`) e as
    /// do chat de projeto que falam com o dono. Devolve a chamada que não é daqui.
    pub(super) fn ferramenta_nova(&mut self, chat: &Chat, chamada: Chamada, cx: &mut Context<Self>) -> Result<(), Chamada> {
        let a = chamada.argumentos.clone();
        let texto = |chave: &str| a[chave].as_str().unwrap_or_default().trim().to_string();
        let numero = |chave: &str| a[chave].as_u64().or_else(|| a[chave].as_str()?.trim().trim_start_matches(['#', '@']).parse().ok());
        let mestre = chat.papel == Papel::Mestre;
        let resposta = match chamada.ferramenta.as_str() {
            "listar_chats" => self.texto_listar_chats(chat, &a).map_or_else(Resposta::erro, Resposta::ok),
            "mensagem_para_chat" => self.mensagem_para_chat(chat, &texto("chat"), texto("texto"), cx),
            "estado" => {
                self.estado_para_o_mestre(chamada.resposta, cx);
                return Ok(());
            }
            "ver_chat" => self.texto_ver_chat(&texto("chat"), a["ultimas"].as_u64().unwrap_or(10)).map_or_else(Resposta::erro, Resposta::ok),
            "historico" => self.texto_historico(a["horas"].as_u64().unwrap_or(24), &texto("projeto")).map_or_else(Resposta::erro, Resposta::ok),
            "propor_divisao" => self.propor_divisao(&a, cx),
            "sugerir_projeto" => self.sugerir_projeto(&a, cx),
            "lembrar" => self.lembrar(&texto("texto"), cx),
            "esquecer" => match esquecer_em(&self.dados.memoria(), &texto("texto")) {
                Ok(nova) => match self.dados.gravar_memoria(&nova) {
                    Ok(()) => Resposta::ok("Esquecido."),
                    Err(e) => Resposta::erro(format!("Não gravou a memória: {e:#}")),
                },
                Err(e) => Resposta::erro(e),
            },
            "anotar_decisao" if mestre => {
                let nome = texto("projeto");
                match resolver_projeto(&self.projetos, nome.trim_start_matches('#')) {
                    Resolucao::Um(id) => match self.dados.anotar(&id, &texto("texto")) {
                        Ok(()) => Resposta::ok(format!("Anotado em {}. Vale já no próximo turno dos chats dele.", self.nome_do_projeto(&id))),
                        Err(e) => Resposta::erro(format!("Não anotou: {e}")),
                    },
                    _ => Resposta::erro(format!("Não há projeto \"{nome}\".")),
                }
            }
            "ver_diff" if mestre => {
                let Some(id) = numero("tarefa") else {
                    let _ = chamada.resposta.try_send(Resposta::erro("Informe o número do chat."));
                    return Ok(());
                };
                self.ver_diff_do_mestre(id, texto("arquivo"), lista_de_textos(&a["arquivos"]), chamada.resposta, cx);
                return Ok(());
            }
            "pedir_ajuste" | "aprovar_tarefa" | "cancelar_tarefa" if mestre => {
                let Some(id) = numero("tarefa") else {
                    let _ = chamada.resposta.try_send(Resposta::erro("Informe o número do chat."));
                    return Ok(());
                };
                if let Err(recusa) = self.pedido_seu(chat, id) {
                    let _ = chamada.resposta.try_send(Resposta::erro(recusa));
                    return Ok(());
                }
                match chamada.ferramenta.as_str() {
                    "aprovar_tarefa" => {
                        let forcar = a["forcar"].as_bool().unwrap_or(false) || a["forcar"].as_str().is_some_and(|f| f.trim() == "true");
                        let portao = match (self.recusa_aprovar(id), self.tarefa(id)) {
                            (None, Some(t)) => portao_aprovacao(t, forcar, &texto("motivo")),
                            (recusa, _) => Err(recusa.unwrap_or_default()),
                        };
                        match portao {
                            Ok(ressalva) => self.aprovar_tarefa(id, texto("nota"), ressalva, chamada.resposta, ID_MESTRE, cx),
                            Err(recusa) => {
                                let _ = chamada.resposta.try_send(Resposta::erro(recusa));
                            }
                        }
                        return Ok(());
                    }
                    "pedir_ajuste" => {
                        let incluir = lista_de(&a["incluir"])
                            .map(|l| l.iter().filter_map(|v| v.as_str().map(|s| s.trim().to_lowercase())).collect::<Vec<_>>());
                        let mut ignorar: Vec<usize> = lista_de(&a["ignorar_problemas"])
                            .unwrap_or_default()
                            .iter()
                            .filter_map(|v| v.as_u64().or_else(|| v.as_str()?.trim().trim_start_matches('#').parse().ok()))
                            .map(|n| n as usize)
                            .collect();
                        // Ao Mestre só vão os altos e médios.
                        if let Some(r) = self.tarefa(id).and_then(|t| t.revisao.as_ref()) {
                            ignorar.extend(r.problemas.iter().enumerate().filter(|(_, p)| p.gravidade == "baixa").map(|(i, _)| i + 1));
                        }
                        self.pedir_ajuste_como(id, texto("feedback"), incluir, ignorar, "Mestre", cx)
                    }
                    _ => self.cancelar_pedido(id, texto("motivo"), cx),
                }
            }
            "entregar_tarefa" if chat.papel == Papel::Projeto => self.entregar_tarefa(chat, &a),
            "perguntar_coordenador" if chat.papel == Papel::Projeto => self.perguntar_ao_dono(chat, &texto("pergunta"), cx),
            _ => return Err(chamada),
        };
        let _ = chamada.resposta.try_send(resposta);
        Ok(())
    }

    /// Recusa quando o pedido aberto no chat não é de quem chama (a regra do dono).
    fn pedido_seu(&self, quem: &Chat, id: u64) -> Result<(), String> {
        pedido_de(self.tarefa(id).ok_or_else(|| format!("Não há chat {id}."))?, &quem.id)
    }

    /// "rodando", "esperando você", "na fila", "erro", "parada: motivo" ou "parado".
    fn estado_para_o_claude(&self, c: &Chat) -> String {
        match self.execucao(&c.id) {
            Execucao::EsperandoVoce => "esperando você".into(),
            Execucao::Rodando { .. } => "rodando".into(),
            Execucao::Erro(_) => "erro".into(),
            Execucao::Parado if self.esperando_vaga(&c.id) || self.na_fila(&c.id) > 0 => "na fila".into(),
            Execucao::Parado => match self.parada(&c.id) {
                Some(motivo) => format!("parada: {}", motivo_para_o_claude(&motivo)),
                None => "parado".into(),
            },
        }
    }

    /// "conversa", ou "pedido do Mestre: rodada 2, em revisão, verificação passou, revisor aprovaria".
    fn pedido_para_o_claude(&self, c: &Chat) -> String {
        let Some(t) = c.tarefa.and_then(|n| self.tarefa(n)) else {
            return if c.papel == Papel::Coordenador { "conversa antiga".into() } else { "-".into() };
        };
        if t.estado == EstadoTarefa::Conversa {
            return match t.mudancas.as_ref().filter(|m| m.arquivos > 0) {
                Some(m) => format!("conversa, {} para juntar", crate::ui::componentes::contagem(m.arquivos as usize, "arquivo", "arquivos")),
                None => "conversa".into(),
            };
        }
        let dono = match t.dono() {
            Some(ID_MESTRE) => "pedido do Mestre".to_string(),
            Some(outro) => format!("ajudante de {}", self.chat(outro).map_or(outro, |c| c.titulo.as_str())),
            None => "pedido".into(),
        };
        let mut partes = vec![format!("rodada {}", t.rodada), t.estado.nome().to_string()];
        if t.resultados_valem() {
            if let Some(v) = &t.ultima_verificacao {
                partes.push(if v.passou { "verificação passou".into() } else { "verificação FALHOU".into() });
            }
            if let Some(r) = &t.revisao {
                partes.push(if r.veredito == "aprovar" { "revisor aprovaria".into() } else { "revisor pede ajustes".into() });
            }
        }
        if let Some(progresso) = t.progresso.as_deref().filter(|p| !p.is_empty()) {
            partes.push(progresso.to_string());
        }
        format!("{dono}: {}", partes.join(", "))
    }

    fn linha_do_chat(&self, c: &Chat) -> String {
        let quando = ha_quanto(agora().saturating_sub(c.atualizado_em));
        if c.papel == Papel::Mestre {
            return format!("- mestre | — | Mestre | {} | {quando}", self.estado_para_o_claude(c));
        }
        format!(
            "- chat {} | {} | {} | {} | {} | {} | {quando}",
            numero_do_chat(c),
            self.nome_do_projeto(&c.projeto),
            c.papel.rotulo(),
            c.titulo,
            self.estado_para_o_claude(c),
            self.pedido_para_o_claude(c)
        )
    }

    fn texto_listar_chats(&self, quem: &Chat, a: &Value) -> Result<String, String> {
        let projeto = match a["projeto"].as_str().map(str::trim).filter(|p| !p.is_empty()) {
            None => None,
            Some(nome) if dados::chave(nome.trim_start_matches('#')) == "avulsos" => Some(String::new()),
            Some(nome) => match resolver_projeto(&self.projetos, nome.trim_start_matches('#')) {
                Resolucao::Um(id) => Some(id),
                _ => return Err(format!("Não há projeto \"{nome}\".")),
            },
        };
        let sim = |chave: &str| a[chave].as_bool().unwrap_or(false) || a[chave].as_str() == Some("true");
        let (ativos, com_pedido) = (sim("ativos"), sim("com_pedido"));
        let mut lista: Vec<&Chat> = self
            .chats
            .iter()
            .filter(|c| c.id != quem.id && !self.arquivada(c))
            .filter(|c| quem.papel != Papel::Projeto || c.papel == Papel::Mestre || c.projeto == quem.projeto || c.pai.as_deref() == Some(quem.id.as_str()))
            .filter(|c| projeto.as_ref().is_none_or(|p| c.projeto == *p && c.papel != Papel::Mestre))
            .filter(|c| !ativos || self.execucao(&c.id).rodando() || self.esperando_vaga(&c.id) || self.na_fila(&c.id) > 0)
            .filter(|c| !com_pedido || c.tarefa.and_then(|n| self.tarefa(n)).is_some_and(|t| t.dono().is_some() && t.estado != EstadoTarefa::Conversa))
            .collect();
        lista.sort_by_key(|c| (c.papel != Papel::Mestre, std::cmp::Reverse(c.atualizado_em)));
        let fora = lista.len().saturating_sub(40);
        let mut texto: String = lista.iter().take(40).map(|c| self.linha_do_chat(c) + "\n").collect();
        if fora > 0 {
            texto.push_str(&format!("({fora} ficaram de fora)\n"));
        }
        if texto.is_empty() {
            texto.push_str("Nenhum chat.");
        }
        Ok(texto)
    }

    /// `mensagem_para_chat`, com o freio de ciclo: depois de 6 mensagens seguidas entre os dois sem o
    /// usuário no meio, a próxima fica guardada como nota.
    fn mensagem_para_chat(&mut self, chat: &Chat, destino: &str, corpo: String, cx: &mut Context<Self>) -> Resposta {
        let Some(alvo) = achar_chat(&self.chats, destino).cloned() else {
            return Resposta::erro(format!("Não existe chat \"{destino}\". Use listar_chats."));
        };
        if alvo.id == chat.id {
            return Resposta::erro("Esse é você mesmo.");
        }
        if corpo.is_empty() {
            return Resposta::erro("Escreva o texto da mensagem.");
        }
        let numero = numero_do_chat(&alvo);
        if alvo.papel == Papel::Projeto && self.arquivada(&alvo) {
            return Resposta::erro(format!("O chat {numero} está arquivado; a mensagem não foi entregue."));
        }
        if alvo.papel == Papel::Coordenador {
            return Resposta::erro(format!("O chat {numero} é uma conversa do Vitral antigo, só para leitura; a mensagem não foi entregue."));
        }
        if !alvo.cwd.is_dir() && alvo.papel == Papel::Trabalhador {
            return Resposta::erro("A pasta desse ajudante não existe mais (a cópia dele já foi removida); a mensagem não foi entregue.");
        }
        if alvo.papel == Papel::Trabalhador
            && alvo.tarefa.and_then(|t| self.tarefa(t)).is_some_and(|t| !t.preparada && !t.estado.ativa())
        {
            return Resposta::erro(
                "A cópia desse ajudante não foi criada e ele não roda; a mensagem não foi entregue. Crie outro se ainda \
                 fizer sentido.",
            );
        }
        let de = match chat.papel {
            Papel::Mestre => "Mestre".to_string(),
            Papel::Projeto => format!("chat {} \"{}\"", numero_do_chat(chat), chat.titulo),
            _ => format!("{} ({})", chat.titulo, chat.id),
        };
        let nota = self.contar_ciclo(&chat.id, &alvo.id);
        self.entregar_entrada(&alvo.id, Entrada { de: Some(de), texto: corpo, remetente: Some(chat.id.clone()), nota, ..Default::default() }, cx);
        if nota {
            return Resposta::ok(
                "Muitas mensagens seguidas entre vocês dois: esta ficou guardada e entra quando o usuário falar com um dos \
                 dois. Encerre sua vez.",
            );
        }
        let sem_pedido = alvo
            .tarefa
            .and_then(|n| self.tarefa(n))
            .is_none_or(|t| t.dono() != Some(chat.id.as_str()) || t.estado == EstadoTarefa::Conversa);
        if chat.papel == Papel::Mestre && sem_pedido && alvo.papel != Papel::Mestre {
            return Resposta::ok(format!(
                "Mensagem entregue ao chat {numero} (ou na fila, se ele estiver ocupado). Isso não cria pedido nem passa pelo \
                 revisor; para pedir trabalho use propor_divisao."
            ));
        }
        Resposta::ok("Mensagem entregue (ou na fila, se o chat estiver ocupado).")
    }

    /// Conta mais uma mensagem automática entre os dois chats e diz se ela vai como nota (passou do
    /// limite). Na primeira que passa, os dois ganham o aviso e fica a ocorrência.
    pub(super) fn contar_ciclo(&mut self, de: &str, para: &str) -> bool {
        let n = self.ciclos.contar(de, para);
        if n == LIMITE_DO_CICLO + 1 {
            let nome = |m: &Motor, id: &str| match m.chat(id) {
                Some(c) if c.papel == Papel::Mestre => "o Mestre".to_string(),
                Some(c) => format!("chat {} \"{}\"", numero_do_chat(c), c.titulo),
                None => id.to_string(),
            };
            let aviso = format!("Muitas mensagens seguidas entre {} e {}; esperando você.", nome(self, de), nome(self, para));
            self.avisar(de, &aviso, true);
            self.avisar(para, &aviso, true);
            let projeto = self.chat(para).map(|c| c.projeto.clone()).unwrap_or_default();
            self.registrar("ciclo", "vitral", para, &projeto, None, &aviso);
        }
        n > LIMITE_DO_CICLO
    }

    /// O que está vivo agora, para o Mestre responder sobre o estado. As pastas com mudanças sem
    /// commit são conferidas em segundo plano, e a resposta sai depois.
    fn estado_para_o_mestre(&mut self, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let mut secoes: Vec<(String, Vec<String>)> = Vec::new();
        let vivos: Vec<&Chat> = self.chats.iter().filter(|c| !self.arquivada(c) && c.papel != Papel::Mestre).collect();
        let rodando: Vec<String> = vivos.iter().filter(|c| self.execucao(&c.id).rodando()).map(|c| self.linha_do_chat(c)).collect();
        secoes.push(("Rodando agora".into(), rodando));
        let mut esperando: Vec<String> = Vec::new();
        for p in &self.permissoes {
            if let Some(c) = self.chat(&p.chat) {
                esperando.push(format!("- permissão: chat {} \"{}\" quer usar {}", numero_do_chat(c), c.titulo, claude::resumo_ferramenta(&p.ferramenta, &p.entrada)));
            }
        }
        for c in self.cartoes.iter().filter(|c| c.aberto()) {
            esperando.push(format!("- {}", self.texto_do_cartao(c)));
        }
        for (a, b) in self.ciclos.travados() {
            let nome = |id: &str| self.chat(id).map_or_else(|| id.to_string(), |c| if c.papel == Papel::Mestre { "o Mestre".into() } else { format!("chat {}", numero_do_chat(c)) });
            esperando.push(format!("- muitas mensagens seguidas entre {} e {}", nome(&a), nome(&b)));
        }
        for c in &vivos {
            match self.execucao(&c.id) {
                Execucao::Erro(erro) => esperando.push(format!("- erro no chat {} \"{}\": {}", numero_do_chat(c), c.titulo, git::inicio_do_texto(erro.trim(), 200))),
                _ if self.precisa_retomar(&c.id) => {
                    esperando.push(format!("- chat {} \"{}\" parou no meio ({}); o usuário retoma", numero_do_chat(c), c.titulo, self.parada(&c.id).unwrap_or_default()))
                }
                _ => {}
            }
        }
        secoes.push(("Esperando o usuário".into(), esperando));
        let fila: Vec<String> = vivos
            .iter()
            .filter(|c| !self.execucao(&c.id).rodando() && (self.esperando_vaga(&c.id) || self.na_fila(&c.id) > 0))
            .map(|c| {
                let motivo = if self.esperando_vaga(&c.id) { "esperando vaga" } else { "esperando a vez" };
                format!("- chat {} \"{}\": {} ({motivo})", numero_do_chat(c), c.titulo, crate::ui::componentes::contagem(self.na_fila(&c.id), "mensagem", "mensagens"))
            })
            .collect();
        secoes.push(("Na fila".into(), fila));
        let mut por_projeto: Vec<(String, Vec<String>)> = Vec::new();
        for t in self.tarefas.iter().filter(|t| t.dono().is_some() && !t.estado.encerrada() && t.estado != EstadoTarefa::Conversa) {
            let Some(c) = self.chat(&t.trabalhador) else {
                continue;
            };
            let nome = self.nome_do_projeto(&t.projeto);
            let linha = format!("chat {} \"{}\" ({})", t.id, c.titulo, self.pedido_para_o_claude(c));
            match por_projeto.iter_mut().find(|(n, _)| *n == nome) {
                Some((_, lista)) => lista.push(linha),
                None => por_projeto.push((nome, vec![linha])),
            }
        }
        secoes.push(("Pedidos abertos por projeto".into(), por_projeto.into_iter().map(|(n, l)| format!("- {n}: {}", l.join("; "))).collect()));
        let sumidas: Vec<String> = self
            .projetos_visiveis()
            .iter()
            .flat_map(|p| p.pastas.iter().filter(|l| !l.caminho.is_dir()).map(move |l| format!("- {}: {} não foi encontrada", p.nome, com_til(&l.caminho))))
            .collect();
        secoes.push(("Pastas não encontradas".into(), sumidas));
        let usos: Vec<String> = self
            .contas
            .iter()
            .map(|c| {
                let uso = self.uso_da_conta(&c.id);
                let texto = crate::ui::componentes::resumo_uso(uso.cinco_horas.map(|f| f as f32), uso.semana.map(|f| f as f32));
                format!("- {}: {texto}{}", c.id, if uso.bloqueado { " (no limite)" } else { "" })
            })
            .collect();
        let origens: Vec<(String, PathBuf)> = self
            .projetos_visiveis()
            .iter()
            .flat_map(|p| p.pastas.iter().filter(|l| l.caminho.join(".git").exists()).map(move |l| (p.nome.clone(), l.caminho.clone())))
            .collect();
        cx.background_spawn(async move {
            let sujas: Vec<String> = origens
                .iter()
                .filter_map(|(nome, pasta)| {
                    let n = git::arquivos_sem_commit(pasta).len();
                    (n > 0).then(|| format!("- {} ({nome}): {}", com_til(pasta), crate::ui::componentes::contagem(n, "arquivo", "arquivos")))
                })
                .collect();
            secoes.push(("Pastas com mudanças sem commit (juntar ali recusa até o usuário resolver)".into(), sujas));
            secoes.push(("Uso do plano".into(), usos));
            let texto: Vec<String> = secoes
                .into_iter()
                .filter(|(_, linhas)| !linhas.is_empty())
                .map(|(titulo, linhas)| format!("{titulo}:\n{}", linhas.join("\n")))
                .collect();
            let texto = if texto.is_empty() { "Nada rodando e nada esperando o usuário.".to_string() } else { texto.join("\n\n") };
            let _ = resposta.try_send(Resposta::ok(texto));
        })
        .detach();
    }

    fn texto_ver_chat(&self, citado: &str, ultimas: u64) -> Result<String, String> {
        let c = achar_chat(&self.chats, citado).ok_or_else(|| format!("Não existe chat \"{citado}\". Use listar_chats."))?;
        let mut texto = self.linha_do_chat(c).trim_start_matches("- ").to_string();
        texto.push('\n');
        if let Some(t) = c.tarefa.and_then(|n| self.tarefa(n)) {
            if t.estado != EstadoTarefa::Conversa && !t.objetivo.trim().is_empty() {
                texto.push_str(&format!("\nPedido:\n{}\n", git::inicio_do_texto(t.objetivo.trim(), 1500)));
                for (i, criterio) in t.criterios.iter().enumerate() {
                    texto.push_str(&format!("{}. {criterio}\n", i + 1));
                }
            }
            if let Some(e) = t.entrega.as_ref().filter(|e| e.rodada == t.rodada) {
                texto.push_str(&format!("\nEntrega desta rodada:\n{}", texto_da_entrega(e, &t.criterios)));
            }
            if let Some(origem) = Some(self.origem_da_tarefa(t)).filter(|o| !o.as_os_str().is_empty()) {
                texto.push_str(&format!("\nPasta: {}{}\n", com_til(&origem), if t.tem_copia() { " (numa cópia)" } else { " (direto)" }));
            }
        }
        let quem = if c.papel == Papel::Mestre { "Mestre" } else { "Chat" };
        let mut linhas: Vec<String> = Vec::new();
        let (mut contadas, mut tamanho) = (0, 0);
        for item in c.itens.iter().rev() {
            let linha = match item {
                Item::Usuario { texto, .. } => format!("Usuário: {}", git::inicio_do_texto(texto.trim(), 1500)),
                Item::Entrega { de, texto, .. } => format!("[de: {de}] {}", git::inicio_do_texto(texto.trim(), 1500)),
                Item::Assistente { texto } => format!("{quem}: {}", git::inicio_do_texto(texto.trim(), 1500)),
                Item::Ferramenta { resumo, erro, .. } => format!("  · {resumo}{}", if *erro { " (erro)" } else { "" }),
                Item::Aviso { texto, erro: true } => format!("  [aviso] {}", git::inicio_do_texto(texto.trim(), 300)),
                _ => continue,
            };
            let conta = matches!(item, Item::Usuario { .. } | Item::Entrega { .. } | Item::Assistente { .. });
            if (conta && contadas >= ultimas.clamp(1, 50)) || tamanho + linha.len() > 8000 {
                break;
            }
            contadas += usize::from(conta) as u64;
            tamanho += linha.len();
            linhas.push(linha);
        }
        linhas.reverse();
        if linhas.is_empty() {
            texto.push_str("\nSem mensagens.");
        } else {
            texto.push_str(&format!("\nÚltimas mensagens:\n{}", linhas.join("\n")));
        }
        Ok(texto)
    }

    fn texto_historico(&self, horas: u64, projeto: &str) -> Result<String, String> {
        let horas = horas.clamp(1, 24 * 30);
        let filtro = match projeto.trim().trim_start_matches('#') {
            "" => None,
            nome if dados::chave(nome) == "avulsos" => Some(String::new()),
            nome => match resolver_projeto(&self.projetos, nome) {
                Resolucao::Um(id) => Some(id),
                _ => return Err(format!("Não há projeto \"{nome}\".")),
            },
        };
        let desde = agora().saturating_sub(horas * 3600);
        let autor = |a: &str| match a {
            "usuario" => "usuário".to_string(),
            "mestre" => "Mestre".into(),
            "vitral" => "Vitral".into(),
            id => self.chat(id).map_or_else(|| id.to_string(), |c| format!("chat {}", numero_do_chat(c))),
        };
        let linhas: Vec<String> = self
            .ocorrencias
            .iter()
            .filter(|o| o.quando >= desde && filtro.as_ref().is_none_or(|p| o.projeto == *p))
            .map(|o| {
                let projeto = if o.projeto.is_empty() { String::new() } else { format!(" ({})", self.nome_do_projeto(&o.projeto)) };
                format!("{} · {} · {}{projeto}: {}", crate::modelo::quando_local(o.quando).trim_start_matches("às "), o.tipo, autor(&o.autor), o.texto)
            })
            .collect();
        if linhas.is_empty() {
            return Ok(format!("Nada nas últimas {horas} h."));
        }
        Ok(linhas.join("\n"))
    }

    /// `propor_divisao`: valida, confere se já não foi proposta (mesmo turno ou mesma assinatura em 2
    /// h), grava o cartão e, nos três casos da regra, começa direto.
    fn propor_divisao(&mut self, a: &Value, cx: &mut Context<Self>) -> Resposta {
        let entradas = self.ultima_entrada.get(ID_MESTRE).cloned().unwrap_or_default();
        let brutas: Vec<Parte> = lista_de(&a["partes"]).unwrap_or_default().iter().map(parte_do_json).collect();
        let perguntas: Vec<String> = lista_de_textos(&a["perguntas"]).into_iter().filter(|p| !p.trim().is_empty()).collect();
        let partes = {
            let ocupado = |id: &str| self.ocupado_para_pedido(id);
            match validar_divisao(&brutas, &self.projetos, &self.chats, &self.tarefas, &ocupado) {
                Ok(p) => p,
                Err(e) => return Resposta::erro(e),
            }
        };
        let turno = chave_do_turno(&entradas);
        let assinatura = assinatura_da_divisao(&partes);
        let substituir = match decidir_proposta(&self.cartoes, &turno, &assinatura, agora()) {
            Proposta::Nova { substituir } => substituir,
            Proposta::Repetida(c) => return Resposta::ok(match c.estado {
                EstadoCartao::Aberto => format!("Divisão {} já está com o usuário.", c.id),
                _ => {
                    let criadas: Vec<String> = c
                        .divisao()
                        .map(|d| {
                            d.partes
                                .iter()
                                .filter_map(|p| p.criada.as_ref().map(|cr| format!("chat {} \"{}\" ({})", cr.tarefa, p.titulo, self.nome_do_projeto(&p.projeto))))
                                .collect()
                        })
                        .unwrap_or_default();
                    format!("Já começada: {}.", criadas.join("; "))
                }
            }),
        };
        for id in substituir {
            self.decidir(id, EstadoCartao::Substituido);
        }
        let unicas: Vec<Destino> = {
            let rodando = |id: &str| self.execucao(id).rodando();
            entradas
                .iter()
                .filter(|e| e.do_usuario())
                .filter_map(|e| resolver_referencias(&e.texto, &self.projetos, &self.chats, &self.tarefas, &rodando).unica)
                .collect()
        };
        let comeco = como_comecar(&self.config, &entradas, &partes, &perguntas, &unicas);
        let (pedido_original, anexos) = pedido_do_turno(&entradas);
        let anexos = anexos.into_iter().map(|a| self.anexo_guardado(a)).collect();
        let n = partes.len();
        let divisao = Divisao {
            resumo: a["resumo"].as_str().unwrap_or_default().trim().to_string(),
            partes,
            perguntas,
            pedido_original,
            anexos,
            turno,
            assinatura,
            sem_perguntar: comeco == Comeco::SemPerguntar,
            por_referencia: comeco == Comeco::PorReferencia,
            conclusao_avisada: false,
        };
        let resumo = divisao.resumo.clone();
        let id = self.novo_cartao(TipoCartao::Divisao(divisao));
        self.registrar("divisao", ID_MESTRE, ID_MESTRE, "", None, &format!("Divisão {id} proposta: {resumo}"));
        if comeco != Comeco::Cartao {
            let comecada = self.comecar_divisao_agora(id, ID_MESTRE, cx);
            cx.notify();
            return Resposta::ok(format!("Começada: {}", comecada.texto()));
        }
        self.gravar_cartoes();
        if !self.janela_ativa
            && let Some(m) = self.chat(ID_MESTRE).cloned()
        {
            self.notificar(&m, &format!("Vou dividir assim: {}", if resumo.is_empty() { "a divisão espera você".to_string() } else { resumo }));
        }
        cx.notify();
        Resposta::ok(format!(
            "Divisão {id} mostrada ao usuário ({}). Encerre sua vez com uma frase curta; o Vitral avisa quando ele decidir.",
            crate::ui::componentes::contagem(n, "parte", "partes")
        ))
    }

    /// O anexo do cache (de uma versão antiga) vai para `dados/anexos`, para a divisão não depender
    /// do cache; os outros ficam onde estão.
    fn anexo_guardado(&self, anexo: PathBuf) -> PathBuf {
        if !anexo.starts_with(&self.dados.cache) || !anexo.is_file() {
            return anexo;
        }
        let pasta = self.dados.pasta_anexos();
        let destino = pasta.join(anexo.file_name().unwrap_or_default());
        let copiou = std::fs::create_dir_all(&pasta).is_ok() && dados::copiar_com_fsync(&anexo, &destino).is_ok();
        if copiou { destino } else { anexo }
    }

    /// `sugerir_projeto`: confere o nome, as pastas e o ícone e mostra o cartão.
    fn sugerir_projeto(&mut self, a: &Value, cx: &mut Context<Self>) -> Resposta {
        let texto = |chave: &str| a[chave].as_str().unwrap_or_default().trim().to_string();
        let nome = texto("nome");
        if let Err(e) = dados::chave_valida(&nome, &self.projetos, None) {
            return Resposta::erro(format!("Nome \"{nome}\": {e}"));
        }
        let chave = dados::chave(&nome);
        if let Some(c) = self.cartoes.iter().find(|c| c.aberto() && matches!(&c.tipo, TipoCartao::Projeto(s) if dados::chave(&s.nome) == chave)) {
            return Resposta::ok(format!("A sugestão do projeto {nome} já está com o usuário (cartão {}).", c.id));
        }
        let mut pastas = Vec::new();
        let mut avisos = Vec::new();
        for pasta in lista_de_textos(&a["pastas"]).iter().map(|p| sem_til(p.trim())).filter(|p| !p.as_os_str().is_empty()) {
            if !pasta.is_dir() {
                return Resposta::erro(format!("A pasta {} não existe (ou não é uma pasta).", pasta.display()));
            }
            let pasta = dunce::canonicalize(&pasta).unwrap_or(pasta);
            for p in &self.projetos {
                for l in &p.pastas {
                    if l.caminho == pasta {
                        avisos.push(format!("{} já é do projeto {}", com_til(&pasta), p.nome));
                    } else if pasta.starts_with(&l.caminho) || l.caminho.starts_with(&pasta) {
                        avisos.push(format!("{} fica junto de {} (projeto {})", com_til(&pasta), com_til(&l.caminho), p.nome));
                    }
                }
            }
            pastas.push(pasta);
        }
        let icone = texto("icone");
        let icone = if crate::ui::icone_projeto::ICONES.iter().any(|i| i.id == icone) { icone } else { String::new() };
        let cor = Some(texto("cor")).filter(|c| c.len() == 7 && c.starts_with('#') && c[1..].chars().all(|x| x.is_ascii_hexdigit())).unwrap_or_default();
        let sugestao = SugestaoProjeto {
            nome: nome.clone(),
            descricao: texto("descricao"),
            instrucoes: texto("instrucoes"),
            icone,
            cor,
            pastas,
            motivo: texto("motivo"),
            pedido_pendente: texto("pedido_pendente"),
            criado: None,
        };
        let id = self.novo_cartao(TipoCartao::Projeto(sugestao));
        self.registrar("projeto", ID_MESTRE, ID_MESTRE, "", None, &format!("Sugestão do projeto {nome}"));
        self.gravar_cartoes();
        cx.notify();
        let mut resposta = format!("Sugestão mostrada ao usuário (cartão {id}); encerre sua vez.");
        if !avisos.is_empty() {
            resposta.push_str(&format!(" Atenção: {}.", avisos.join("; ")));
        }
        Resposta::ok(resposta)
    }

    /// `lembrar`: num turno só do usuário, grava; nos outros, o usuário confirma pelo cartão.
    fn lembrar(&mut self, texto: &str, cx: &mut Context<Self>) -> Resposta {
        let entradas = self.ultima_entrada.get(ID_MESTRE).cloned().unwrap_or_default();
        if turno_so_do_usuario(&entradas) {
            return match lembrar_em(&self.dados.memoria(), texto) {
                Ok(nova) => match self.dados.gravar_memoria(&nova) {
                    Ok(()) => Resposta::ok("Lembrado."),
                    Err(e) => Resposta::erro(format!("Não gravou a memória: {e:#}")),
                },
                Err(e) => Resposta::erro(e),
            };
        }
        let linha = texto.split_whitespace().collect::<Vec<_>>().join(" ");
        if linha.is_empty() {
            return Resposta::erro("Diga o que lembrar.");
        }
        let ja = self.cartoes.iter().any(|c| c.aberto() && matches!(&c.tipo, TipoCartao::Memoria { texto } if *texto == linha));
        if !ja {
            self.novo_cartao(TipoCartao::Memoria { texto: linha });
            self.gravar_cartoes();
            cx.notify();
        }
        Resposta::ok("O usuário vai confirmar.")
    }

    /// `ver_diff` do Mestre: sem arquivo, o --stat e a lista; com ele, o diff completo dele. Na pasta
    /// própria, da base do pedido até o HEAD.
    fn ver_diff_do_mestre(&mut self, id: u64, arquivo: String, mut arquivos: Vec<String>, resposta: Sender<Resposta>, cx: &mut Context<Self>) {
        let Some(t) = self.tarefa(id).cloned() else {
            let _ = resposta.try_send(Resposta::erro(format!("Não há chat {id}.")));
            return;
        };
        if !self.de_projeto(&t.trabalhador) {
            self.ver_diff(id, arquivos, resposta, cx);
            return;
        }
        let origem = self.origem_da_tarefa(&t);
        let pasta = match t.forma {
            Forma::Copia => t.worktree.clone(),
            Forma::DiretaGit => Some(origem.clone()),
            Forma::Direta => {
                let _ = resposta.try_send(Resposta::ok(format!(
                    "O chat {id} trabalha numa pasta sem git ({}): não há diff. Leia os arquivos (Read) ou use ver_chat {id}.",
                    com_til(&origem)
                )));
                return;
            }
        };
        let (Some(pasta), Some(base)) = (pasta.filter(|p| p.is_dir()), t.base.clone()) else {
            let _ = resposta.try_send(Resposta::ok(format!("O chat {id} ainda não tem cópia (ou ponto de partida): não há o que mostrar.")));
            return;
        };
        if !arquivo.is_empty() {
            arquivos.push(arquivo);
        }
        cx.background_spawn(async move {
            let base = if t.tem_copia() && origem.is_dir() { git::base_efetiva(&origem, &pasta, &base) } else { base };
            let texto = if arquivos.is_empty() {
                let estatistica = git::diff_stat(&pasta, &base).unwrap_or_default();
                let lista = git::arquivos_mudados(&pasta, &base);
                if lista.is_empty() {
                    "Nada mudou desde o começo do pedido.".to_string()
                } else {
                    format!(
                        "{}\nArquivos ({}):\n{}\nPeça um por vez com `arquivo`.",
                        estatistica.trim_end(),
                        lista.len(),
                        lista.iter().map(|a| format!("- {a}")).collect::<Vec<_>>().join("\n")
                    )
                }
            } else {
                git::diff_para_coordenador(&pasta, &origem, &base, &arquivos, &[], mcp::MAX_RESPOSTA - 2000)
                    .unwrap_or_else(|erro| format!("Não deu para ler o diff: {erro}"))
            };
            let _ = resposta.try_send(Resposta::ok(texto));
        })
        .detach();
    }

    /// `entregar_tarefa`: a entrega desta rodada fica gravada; no fim da vez ela vai para a conferência.
    fn entregar_tarefa(&mut self, chat: &Chat, a: &Value) -> Resposta {
        let abertos = self.ajudantes_abertos(&chat.id);
        if !abertos.is_empty() {
            let lista: Vec<String> = abertos.iter().map(|n| format!("#{n}")).collect();
            return Resposta::erro(format!(
                "Você ainda tem ajudantes abertos ({}): aprove (aprovar_tarefa) ou cancele (cancelar_tarefa) cada um \
                 antes de entregar. Se estiver esperando o relatório deles, encerre sua vez: ele chega sozinho.",
                lista.join(", ")
            ));
        }
        let Some(t) = chat.tarefa.and_then(|n| self.tarefa_mut(n)) else {
            return Resposta::erro("Este chat não recebe pedidos.");
        };
        if !t.estado.ativa() {
            return Resposta::erro("Não há pedido aberto neste chat: converse com o usuário na resposta, sem entregar_tarefa.");
        }
        let texto = |chave: &str| a[chave].as_str().unwrap_or_default().trim().to_string();
        let resumo = texto("resumo");
        if resumo.is_empty() {
            return Resposta::erro("Falta o resumo da entrega.");
        }
        let criterios = criterios_da_entrega(&a["criterios"]);
        let fora: Vec<usize> = criterios.iter().map(|c| c.indice).filter(|i| *i == 0 || *i > t.criterios.len()).collect();
        if !fora.is_empty() {
            return Resposta::erro(format!(
                "O pedido tem {} critérios (de 1 a {}); não existe o {}.",
                t.criterios.len(),
                t.criterios.len(),
                fora.iter().map(usize::to_string).collect::<Vec<_>>().join(", ")
            ));
        }
        t.entrega = Some(Entrega { resumo, como_verificou: texto("como_verificou"), criterios, pendencias: texto("pendencias"), rodada: t.rodada, em: agora() });
        Resposta::ok(format!(
            "Entrega da rodada {} registrada. Encerre sua vez: o Vitral confere (verificação e revisor) e manda ao Mestre.",
            t.rodada
        ))
    }

    /// `perguntar_coordenador` de um chat de projeto: vai para o dono do pedido (o Mestre).
    fn perguntar_ao_dono(&mut self, chat: &Chat, pergunta: &str, cx: &mut Context<Self>) -> Resposta {
        let Some(t) = chat.tarefa.and_then(|n| self.tarefa(n)).cloned() else {
            return Resposta::erro("Este chat não recebe pedidos.");
        };
        let Some(dono) = t.dono().filter(|_| t.estado != EstadoTarefa::Conversa).map(str::to_string) else {
            return Resposta::erro("Quem te pediu foi o usuário: faça a pergunta na sua resposta e encerre a vez.");
        };
        if pergunta.is_empty() {
            return Resposta::erro("Escreva a pergunta.");
        }
        let texto = format!(
            "Pergunta do chat {} \"{}\" ({}): {pergunta}\n\nResponda com mensagem_para_chat para o chat {}.",
            t.id,
            chat.titulo,
            self.nome_do_projeto(&chat.projeto),
            t.id
        );
        let nota = self.contar_ciclo(&chat.id, &dono);
        if !self.aguardando_resposta.contains(&chat.id) {
            self.aguardando_resposta.push(chat.id.clone());
        }
        self.registrar("pergunta", &chat.id, &chat.id, &chat.projeto, Some(t.id), &git::inicio_do_texto(pergunta, 200));
        let entrada = Entrada { de: Some(format!("chat {}", t.id)), texto, remetente: Some(chat.id.clone()), nota, ..Default::default() };
        self.entregar_entrada(&dono, entrada, cx);
        if nota {
            return Resposta::ok("Muitas mensagens seguidas: a pergunta ficou guardada até o usuário falar. Encerre sua vez.");
        }
        Resposta::ok("Pergunta enviada. A resposta chega como nova mensagem; se puder continuar sem ela, continue, senão encerre sua vez.")
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::{Mudancas, Ocorrencia, PastaLigada, Problema, Revisao, Verificacao};

    fn pasta(nome: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("vitral-mestre-{}-{nome}", novo_id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn projeto(id: &str, nome: &str, pastas: &[&Path]) -> Projeto {
        Projeto {
            id: id.into(),
            nome: nome.into(),
            pastas: pastas.iter().map(|p| PastaLigada { caminho: p.to_path_buf(), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    fn chat_de(id: &str, projeto: &str, titulo: &str, tarefa: u64) -> Chat {
        let mut c = Chat::novo(id.into(), Papel::Projeto, projeto, titulo, PathBuf::from("/tmp"));
        c.tarefa = Some(tarefa);
        c
    }

    fn tarefa_de(id: u64, chat: &str, estado: EstadoTarefa) -> Tarefa {
        Tarefa { id, trabalhador: chat.into(), estado, ..Default::default() }
    }

    fn parte(projeto: &str, titulo: &str) -> Parte {
        Parte {
            projeto: projeto.into(),
            titulo: titulo.into(),
            pedido: format!("faça {titulo}"),
            criterios: vec!["funciona".into()],
            ..Default::default()
        }
    }

    fn livre(_: &str) -> bool {
        false
    }

    struct Cena {
        projetos: Vec<Projeto>,
        chats: Vec<Chat>,
        tarefas: Vec<Tarefa>,
    }

    /// Ninee (uma pasta), Vitral (duas), Velho (arquivado) e Rascunho (provisório, sem pasta); os
    /// chats 7 "Docs" (Vitral, em conversa), 12 "Checkout com Pix" e 3 "Checkout antigo" (Ninee).
    fn cena() -> Cena {
        let (loja, vitral, docs) = (pasta("loja"), pasta("vitral"), pasta("docs"));
        let mut velho = projeto("velho", "Velho", &[loja.as_path()]);
        velho.arquivado = true;
        let mut rascunho = projeto("rascunho", "Rascunho", &[]);
        rascunho.provisorio = true;
        let projetos = vec![
            projeto("ninee-loja", "Ninee", &[loja.as_path()]),
            projeto("vitral", "Vitral", &[vitral.as_path(), docs.as_path()]),
            velho,
            rascunho,
        ];
        let mut mestre = Chat::novo(ID_MESTRE.into(), Papel::Mestre, "", "Mestre", PathBuf::from("/tmp"));
        mestre.tarefa = None;
        let chats = vec![
            mestre,
            chat_de("c7", "vitral", "Docs", 7),
            chat_de("c12", "ninee-loja", "Checkout com Pix", 12),
            chat_de("c3", "ninee-loja", "Checkout antigo", 3),
        ];
        let tarefas = vec![
            tarefa_de(7, "c7", EstadoTarefa::Conversa),
            tarefa_de(12, "c12", EstadoTarefa::Conversa),
            Tarefa { coordenador: ID_MESTRE.into(), ..tarefa_de(3, "c3", EstadoTarefa::Rodando) },
        ];
        Cena { projetos, chats, tarefas }
    }

    fn refs(c: &Cena, texto: &str) -> Referencias {
        resolver_referencias(texto, &c.projetos, &c.chats, &c.tarefas, &|id| id == "c12")
    }

    #[test]
    fn referencias_de_projeto_e_de_chat() {
        let c = cena();
        let r = refs(&c, "#ninee arrume o checkout");
        assert!(r.texto.starts_with("[Vitral] Referências: #ninee = projeto Ninee (id ninee-loja; pastas "), "{}", r.texto);
        assert_eq!(r.unica, Some(Destino { projeto: "ninee-loja".into(), chat: None }));
        // `#12` e o título de markdown não são referências.
        assert_eq!(refs(&c, "veja o #12 de novo"), Referencias::default());
        assert_eq!(refs(&c, "# Plano\n\nnada aqui"), Referencias::default());
        assert_eq!(refs(&c, "C# e email a@b.com"), Referencias::default());
        let r = refs(&c, "@12 pode continuar?");
        assert!(r.texto.contains("@12 = chat 12 \"Checkout com Pix\" (projeto Ninee, rodando, em conversa)"), "{}", r.texto);
        assert_eq!(r.unica, Some(Destino { projeto: "ninee-loja".into(), chat: Some("c12".into()) }));
        let r = refs(&c, "fale com @\"Docs\".");
        assert!(r.texto.contains("@\"Docs\" = chat 7 \"Docs\" (projeto Vitral, parado, em conversa)"), "{}", r.texto);
        let r = refs(&c, "o @checkout quebrou");
        assert!(r.texto.contains("@checkout: ambíguo (chat 12 \"Checkout com Pix\", chat 3 \"Checkout antigo\")"), "{}", r.texto);
        assert_eq!(r.unica, None);
        let r = refs(&c, "manda para #lojaa.");
        assert!(r.texto.ends_with("#lojaa: nenhum projeto com esse nome."), "{}", r.texto);
        assert_eq!(r.unica, None);
        assert!(refs(&c, "#avulsos uma pesquisa").texto.contains("#avulsos = Avulsos"));
        assert!(refs(&c, "@mestre oi").texto.contains("@mestre = o Mestre"));
        // Projeto e chat que concordam dão um destino só; dois projetos, nenhum.
        assert_eq!(refs(&c, "#ninee @12").unica, Some(Destino { projeto: "ninee-loja".into(), chat: Some("c12".into()) }));
        assert_eq!(refs(&c, "#ninee e #vitral").unica, None);
        assert_eq!(refs(&c, "#vitral @12").unica, None);
        assert_eq!(refs(&c, "#ninee #ninee").texto.matches("#ninee =").count(), 1);
    }

    #[test]
    fn divisao_valida_e_os_erros_para_o_mestre() {
        let c = cena();
        let validar = |partes: &[Parte]| validar_divisao(partes, &c.projetos, &c.chats, &c.tarefas, &livre);
        let ok = validar(&[parte("#ninee", "Pix")]).unwrap();
        assert_eq!((ok[0].projeto.as_str(), ok[0].pasta.as_ref()), ("ninee-loja", None));
        assert!(validar(&[parte("nao-existe", "x")]).unwrap_err().contains("não há projeto \"nao-existe\". Projetos: Ninee (#ninee)"));
        assert!(validar(&[parte("Velho", "x")]).unwrap_err().contains("está arquivado"));
        assert!(validar(&[parte("rascunho", "x")]).unwrap_err().contains("não tem pasta ligada"));
        let erro = validar(&[parte("vitral", "x")]).unwrap_err();
        assert!(erro.starts_with("Parte 1 (\"x\"): o projeto Vitral tem mais de uma pasta; diga qual em `pasta`:"), "{erro}");
        let docs = c.projetos[1].pastas[1].caminho.clone();
        let com_pasta = validar(&[Parte { pasta: Some(docs.clone()), ..parte("vitral", "x") }]).unwrap();
        assert_eq!(com_pasta[0].pasta.as_ref(), Some(&docs));
        let pelo_nome = Parte { pasta: docs.file_name().map(PathBuf::from), ..parte("vitral", "x") };
        assert!(validar(&[pelo_nome]).is_ok());
        assert!(validar(&[Parte { criterios: vec![], ..parte("ninee", "x") }]).unwrap_err().contains("pelo menos um critério"));
        assert!(validar(&[Parte { pedido: String::new(), ..parte("ninee", "x") }]).unwrap_err().contains("falta o título ou o pedido"));
        let muitas: Vec<Parte> = (0..9).map(|i| parte("ninee", &format!("p{i}"))).collect();
        assert!(validar(&muitas).unwrap_err().contains("No máximo 8 partes"));
        assert_eq!(validar(&[parte("avulsos", "pesquisa")]).unwrap()[0].projeto, "");
        // O chat citado: do mesmo projeto, em conversa e livre.
        let no_docs = Parte { chat: Some("7".into()), pasta: Some(c.projetos[1].pastas[0].caminho.clone()), ..parte("vitral", "doc") };
        assert_eq!(validar(&[no_docs.clone()]).unwrap()[0].chat.as_deref(), Some("c7"));
        assert!(validar(&[Parte { chat: Some("7".into()), ..parte("ninee", "x") }]).unwrap_err().contains("não é um chat do projeto Ninee"));
        assert!(validar(&[Parte { chat: Some("3".into()), ..parte("ninee", "x") }]).unwrap_err().contains("já tem um pedido aberto"));
        let ocupado = validar_divisao(&[no_docs.clone()], &c.projetos, &c.chats, &c.tarefas, &|id| id == "c7");
        assert!(ocupado.unwrap_err().contains("está trabalhando ou tem mensagens esperando"));
        assert!(validar(&[no_docs.clone(), Parte { titulo: "outra".into(), ..no_docs.clone() }]).unwrap_err().contains("duas partes para o chat 7"));
        let mut tarefas = c.tarefas.clone();
        tarefas[0].mudancas = Some(Mudancas { arquivos: 2, ..Default::default() });
        let com_mudancas = validar_divisao(&[no_docs], &c.projetos, &c.chats, &tarefas, &livre).unwrap();
        assert_eq!(com_mudancas[0].aviso.as_deref(), Some("o chat tem mudanças ainda não juntadas"));
        let sumida = projeto("sumida", "Sumida", &[Path::new("/nao/existe/vitral")]);
        let erro = validar_divisao(&[parte("sumida", "x")], &[sumida], &c.chats, &c.tarefas, &livre).unwrap_err();
        assert!(erro.contains("não foi encontrada"), "{erro}");
    }

    fn cartao_de(id: u64, estado: EstadoCartao, criado_em: u64, d: Divisao) -> Cartao {
        Cartao { id, chat: ID_MESTRE.into(), criado_em, decidido_em: 0, estado, tipo: TipoCartao::Divisao(d), resto: Default::default() }
    }

    #[test]
    fn a_mesma_divisao_nao_cria_nada_de_novo() {
        let entradas = vec![Entrada { texto: "#ninee faça o Pix".into(), ..Default::default() }];
        let turno = chave_do_turno(&entradas);
        // "Tentar de novo" marca as entradas como já na conversa: o turno é o mesmo.
        let repetidas: Vec<Entrada> = entradas.iter().cloned().map(|e| Entrada { registrada: true, ..e }).collect();
        assert_eq!(chave_do_turno(&repetidas), turno);
        assert_ne!(chave_do_turno(&[Entrada { texto: "outro".into(), ..Default::default() }]), turno);
        let partes = vec![parte("ninee-loja", "Checkout com Pix"), parte("vitral", "Docs")];
        let assinatura = assinatura_da_divisao(&partes);
        let invertidas = vec![partes[1].clone(), partes[0].clone()];
        assert_eq!(assinatura_da_divisao(&invertidas), assinatura, "a ordem não importa");
        let agora = 100_000;
        let aberta = cartao_de(3, EstadoCartao::Aberto, agora - 10, Divisao { turno: turno.clone(), assinatura: assinatura.clone(), ..Default::default() });
        // Mesmo turno: a que já existe.
        let cartoes = vec![aberta.clone()];
        assert!(matches!(decidir_proposta(&cartoes, &turno, "outra", agora), Proposta::Repetida(c) if c.id == 3));
        // Começada sem perguntar e proposta de novo no mesmo turno: nada novo.
        let comecada = cartao_de(3, EstadoCartao::Aceito, agora - 10, Divisao { turno: turno.clone(), ..Default::default() });
        assert!(matches!(decidir_proposta(&[comecada], &turno, "x", agora), Proposta::Repetida(c) if c.estado == EstadoCartao::Aceito));
        // Mesma assinatura dentro de 2 h, em outro turno: a mesma; depois de 2 h, nova.
        assert!(matches!(decidir_proposta(&cartoes, "outro", &assinatura, agora), Proposta::Repetida(_)));
        let velha = vec![cartao_de(3, EstadoCartao::Aceito, agora - 3 * 3600, Divisao { assinatura: assinatura.clone(), ..Default::default() })];
        assert!(matches!(decidir_proposta(&velha, "outro", &assinatura, agora), Proposta::Nova { ref substituir } if substituir.is_empty()));
        // Conteúdo diferente com a anterior aberta: nova, e a aberta é substituída.
        assert!(matches!(decidir_proposta(&cartoes, "outro", "diferente", agora), Proposta::Nova { substituir } if substituir == vec![3]));
        // Recusada não conta.
        let recusada = vec![cartao_de(3, EstadoCartao::Recusado, agora, Divisao { turno: turno.clone(), ..Default::default() })];
        assert!(matches!(decidir_proposta(&recusada, &turno, &assinatura, agora), Proposta::Nova { .. }));
    }

    #[test]
    fn comecar_revalida_cada_parte() {
        let c = cena();
        let mut tarefas = c.tarefas.clone();
        // Depois de proposta, o chat 7 ganhou um pedido.
        tarefas[0].estado = EstadoTarefa::Rodando;
        let docs = c.projetos[1].pastas[0].caminho.clone();
        let partes = vec![
            parte("ninee-loja", "Pix"),
            Parte { removida: true, ..parte("ninee-loja", "Tirada") },
            Parte { chat: Some("c7".into()), pasta: Some(docs.clone()), ..parte("vitral", "Doc") },
            Parte { criada: Some(ParteCriada { chat: "c12".into(), tarefa: 12 }), ..parte("ninee-loja", "Já") },
            Parte { chat: Some("c12".into()), ..parte("ninee-loja", "No 12") },
        ];
        let plano = planejar_comeco(&partes, &c.projetos, &c.chats, &tarefas, &livre);
        assert!(matches!(&plano[0], PlanoDaParte::Novo(p) if p.projeto == "ninee-loja"));
        assert!(matches!(plano[1], PlanoDaParte::Pular));
        assert!(matches!(&plano[2], PlanoDaParte::Falha(m) if m.contains("já tem um pedido aberto")));
        assert!(matches!(plano[3], PlanoDaParte::Pular));
        assert!(matches!(&plano[4], PlanoDaParte::Reabrir(p) if p.chat.as_deref() == Some("c12")));
        let comecada = Comecada {
            enviadas: vec!["chat novo 13 \"Pix\" (Ninee)".into()],
            removidas: vec!["\"Tirada\"".into()],
            falhas: vec!["\"Doc\" (o chat 7 já tem um pedido aberto)".into()],
        };
        assert_eq!(
            comecada.texto(),
            "chat novo 13 \"Pix\" (Ninee). Parte removida: \"Tirada\". Parte que não começou: \"Doc\" (o chat 7 já tem um pedido aberto)."
        );
    }

    #[test]
    fn comeca_direto_pela_opcao_pela_rotina_ou_pela_referencia_unica() {
        let c = cena();
        let config = Config::default();
        let usuario = vec![Entrada { texto: "#ninee faça".into(), ..Default::default() }];
        let unicas: Vec<Destino> = usuario.iter().filter_map(|e| refs(&c, &e.texto).unica).collect();
        let uma = vec![Parte { projeto: "ninee-loja".into(), ..parte("ninee-loja", "x") }];
        assert_eq!(como_comecar(&config, &usuario, &uma, &[], &unicas), Comeco::PorReferencia);
        assert_eq!(como_comecar(&config, &usuario, &uma, &["qual cor?".into()], &unicas), Comeco::Cartao);
        let duas = vec![uma[0].clone(), parte("vitral", "y")];
        assert_eq!(como_comecar(&config, &usuario, &duas, &[], &unicas), Comeco::Cartao);
        // Duas referências: nenhum destino único.
        let duas_refs: Vec<Destino> = refs(&c, "#ninee e #vitral").unica.into_iter().collect();
        assert_eq!(como_comecar(&config, &usuario, &uma, &[], &duas_refs), Comeco::Cartao);
        let outro = vec![parte("vitral", "x")];
        assert_eq!(como_comecar(&config, &usuario, &outro, &[], &unicas), Comeco::Cartao);
        let sem_perguntar = Config { comecar_sem_perguntar: true, ..Config::default() };
        assert_eq!(como_comecar(&sem_perguntar, &usuario, &duas, &[], &[]), Comeco::SemPerguntar);
        let rotina = vec![Entrada { de: Some("Rotina".into()), rotina: Some("r1".into()), texto: "bom dia".into(), ..Default::default() }];
        assert_eq!(como_comecar(&config, &rotina, &duas, &[], &[]), Comeco::SemPerguntar);
    }

    #[test]
    fn memoria_lembrar_e_esquecer() {
        let usuario = vec![Entrada { texto: "lembre disso".into(), ..Default::default() }];
        assert!(turno_so_do_usuario(&usuario));
        let mut misto = usuario.clone();
        misto.push(Entrada { de: Some("Vitral".into()), texto: "chat 12 juntado".into(), nota: true, ..Default::default() });
        assert!(!turno_so_do_usuario(&misto), "uma nota do Vitral no turno já faz o cartão");
        assert!(!turno_so_do_usuario(&[Entrada { de: Some("Rotina".into()), rotina: Some("r".into()), ..Default::default() }]));
        assert!(!turno_so_do_usuario(&[]));
        let m = lembrar_em("", "Pagamentos  sempre\nna Ninee").unwrap();
        assert_eq!(m, "- Pagamentos sempre na Ninee\n");
        let m = lembrar_em(&m, "Testes com pytest").unwrap();
        assert_eq!(m, "- Pagamentos sempre na Ninee\n- Testes com pytest\n");
        assert_eq!(lembrar_em(&m, "testes com pytest").unwrap(), m, "a mesma linha não repete");
        let cheia = "- x\n".repeat(MAX_MEMORIA / 4);
        assert_eq!(lembrar_em(&cheia, "mais uma").unwrap_err(), "memória cheia (6000 caracteres): junte ou esqueça itens");
        assert_eq!(esquecer_em(&m, "PYTEST").unwrap(), "- Pagamentos sempre na Ninee\n");
        assert_eq!(esquecer_em("- só esta\n", "esta").unwrap(), "");
        let varias = esquecer_em("- Ninee loja\n- Ninee site\n", "ninee").unwrap_err();
        assert!(varias.contains("Mais de uma linha") && varias.contains("- Ninee loja\n- Ninee site"), "{varias}");
        let nenhuma = esquecer_em(&m, "rust").unwrap_err();
        assert!(nenhuma.contains("Nenhuma linha da memória tem \"rust\"") && nenhuma.contains("- Testes com pytest"));
    }

    #[test]
    fn cancelar_tira_da_fila_o_que_o_dono_mandou() {
        let mut fila = vec![
            Entrada { de: Some("Mestre".into()), remetente: Some(ID_MESTRE.into()), texto: "pedido".into(), ..Default::default() },
            Entrada { de: Some("Vitral".into()), texto: "a cópia ficou pronta".into(), nota: true, ..Default::default() },
            Entrada { de: Some("Mestre".into()), remetente: Some(ID_MESTRE.into()), texto: "ajuste".into(), ..Default::default() },
        ];
        tirar_do_dono(&mut fila, ID_MESTRE);
        assert_eq!(fila.len(), 1);
        // Só sobrou nota: nada abre turno.
        assert!(fila.iter().all(|e| e.nota));
        let mut com_usuario = vec![Entrada { texto: "e aí?".into(), ..Default::default() }];
        tirar_do_dono(&mut com_usuario, ID_MESTRE);
        assert_eq!(com_usuario.len(), 1, "o que o usuário escreveu fica");
    }

    #[test]
    fn arquivar_tira_as_partes_do_projeto_so_das_divisoes_abertas() {
        let divisao = |partes: Vec<Parte>| Divisao { partes, ..Default::default() };
        let mut cartoes = vec![
            cartao_de(1, EstadoCartao::Aberto, 0, divisao(vec![parte("loja", "a"), parte("site", "b"), parte("loja", "c")])),
            cartao_de(2, EstadoCartao::Aceito, 0, divisao(vec![parte("loja", "d")])),
            cartao_de(3, EstadoCartao::Aberto, 0, divisao(vec![parte("site", "e")])),
        ];
        assert_eq!(tirar_partes_do_projeto(&mut cartoes, "loja"), [1]);
        let removidas = |c: &Cartao| c.divisao().unwrap().partes.iter().map(|p| p.removida).collect::<Vec<_>>();
        assert_eq!(removidas(&cartoes[0]), [true, false, true]);
        assert_eq!(removidas(&cartoes[1]), [false], "a divisão já começada fica como está");
        assert!(tirar_partes_do_projeto(&mut cartoes, "loja").is_empty(), "de novo não mexe");
    }

    #[test]
    fn fim_do_pedido_entrega_lembra_uma_vez_ou_espera() {
        let pedido = Tarefa { coordenador: ID_MESTRE.into(), rodada: 1, criterios: vec!["a".into()], ..tarefa_de(12, "c12", EstadoTarefa::Rodando) };
        assert_eq!(decidir_fim(&pedido, false, true), FimDoPedido::Lembrar);
        assert_eq!(decidir_fim(&pedido, true, true), FimDoPedido::Esperar);
        let lembrado = Tarefa { lembrete_rodada: 1, ..pedido.clone() };
        assert_eq!(decidir_fim(&lembrado, false, true), FimDoPedido::Entregar { formal: false });
        assert_eq!(decidir_fim(&lembrado, false, false), FimDoPedido::Nada, "o turno do usuário depois do lembrete não entrega");
        let entregue = Tarefa { entrega: Some(Entrega { rodada: 1, ..Default::default() }), ..pedido.clone() };
        assert_eq!(decidir_fim(&entregue, false, false), FimDoPedido::Entregar { formal: true });
        let velha = Tarefa { entrega: Some(Entrega { rodada: 1, ..Default::default() }), rodada: 2, lembrete_rodada: 1, ..pedido.clone() };
        // A rodada reaberta pelo usuário (mensagem a um pedido em revisão): o turno dele termina sem
        // entrega e ganha o lembrete da rodada nova.
        assert_eq!(decidir_fim(&Tarefa { estado: EstadoTarefa::Ajustando, ..velha }, false, false), FimDoPedido::Lembrar);
        assert_eq!(decidir_fim(&tarefa_de(7, "c7", EstadoTarefa::Conversa), false, true), FimDoPedido::Nada);
        assert_eq!(decidir_fim(&tarefa_de(7, "c7", EstadoTarefa::Rodando), false, true), FimDoPedido::Nada, "sem dono");
        let do_dono = vec![Entrada { remetente: Some(ID_MESTRE.into()), ..Default::default() }];
        assert!(aberto_pelo_dono(&do_dono, ID_MESTRE));
        let lembrete = vec![Entrada { de: Some("Vitral".into()), texto: format!("{LEMBRETE_ENTREGA} do Mestre (chat 12)"), ..Default::default() }];
        assert!(aberto_pelo_dono(&lembrete, ID_MESTRE));
        assert!(!aberto_pelo_dono(&[Entrada { texto: "oi".into(), ..Default::default() }], ID_MESTRE));
    }

    fn verde() -> Tarefa {
        Tarefa {
            coordenador: ID_MESTRE.into(),
            criterios: vec!["a".into(), "b".into()],
            verificacao: Some("cargo test".into()),
            ultima_verificacao: Some(Verificacao { comando: "cargo test".into(), passou: true, saida: "ok".into(), duracao_s: Some(3) }),
            revisao: Some(Revisao {
                veredito: "aprovar".into(),
                resumo: "bom".into(),
                problemas: vec![Problema { gravidade: "baixa".into(), arquivo: "a.rs".into(), ..Default::default() }],
            }),
            entrega: Some(Entrega {
                criterios: [1, 2].map(|indice| CriterioEntregue { indice, estado: "feito".into(), evidencia: String::new() }).to_vec(),
                rodada: 1,
                ..Default::default()
            }),
            ..tarefa_de(12, "c12", EstadoTarefa::EmRevisao)
        }
    }

    #[test]
    fn junta_sozinho_so_com_tudo_verde() {
        let config = Config::default();
        assert!(pode_juntar_sozinho(&verde(), &config));
        assert!(!pode_juntar_sozinho(&verde(), &Config { aprovar_sozinho: false, ..Config::default() }));
        assert!(!pode_juntar_sozinho(&verde(), &Config { revisor: false, ..Config::default() }));
        assert!(!pode_juntar_sozinho(&Tarefa { coordenador: String::new(), ..verde() }, &config), "sem dono");
        let mut falhou = verde();
        falhou.ultima_verificacao.as_mut().unwrap().passou = false;
        assert!(!pode_juntar_sozinho(&falhou, &config));
        assert!(!pode_juntar_sozinho(&Tarefa { ultima_verificacao: None, ..verde() }, &config), "a verificação não rodou");
        assert!(pode_juntar_sozinho(&Tarefa { verificacao: None, ultima_verificacao: None, ..verde() }, &config), "sem verificação");
        let mut media = verde();
        media.revisao.as_mut().unwrap().problemas[0].gravidade = "media".into();
        assert!(!pode_juntar_sozinho(&media, &config));
        let mut ajustar = verde();
        ajustar.revisao.as_mut().unwrap().veredito = "ajustar".into();
        assert!(!pode_juntar_sozinho(&ajustar, &config));
        assert!(!pode_juntar_sozinho(&Tarefa { revisao: None, ..verde() }, &config));
        let mut parcial = verde();
        parcial.entrega.as_mut().unwrap().criterios[1].estado = "parcial".into();
        assert!(!pode_juntar_sozinho(&parcial, &config));
        let mut faltou = verde();
        faltou.entrega.as_mut().unwrap().criterios.pop();
        assert!(!pode_juntar_sozinho(&faltou, &config), "todo critério precisa do estado");
        assert!(!pode_juntar_sozinho(&Tarefa { entrega: None, ..verde() }, &config), "sem entrega formal");
        assert!(!pode_juntar_sozinho(&Tarefa { rodada: 2, ..verde() }, &config), "a entrega é de outra rodada");
        assert!(!pode_juntar_sozinho(&Tarefa { sobreposicao: vec!["#13: a.rs".into()], ..verde() }, &config));
        assert!(!pode_juntar_sozinho(&Tarefa { entrega_pendente: Some(false), ..verde() }, &config), "o portão");
        assert!(!pode_juntar_sozinho(&Tarefa { estado: EstadoTarefa::Rodando, ..verde() }, &config));
    }

    #[test]
    fn relatorio_compacto_so_mostra_a_saida_que_falhou() {
        let t = verde();
        let texto = relatorio_compacto(&t, "Ninee", "Checkout com Pix", "fiz", Some(" a.rs | 2 +-"), false, None);
        assert!(texto.starts_with("Ninee · chat 12 \"Checkout com Pix\" · rodada 1\n"), "{texto}");
        assert!(texto.contains("Verificação `cargo test`: PASSOU em 3 s\n"));
        assert!(!texto.contains("ok\n```"), "a saída de uma verificação que passou não vai");
        assert!(texto.contains("Arquivos (git diff --stat):\na.rs | 2 +-"));
        assert!(texto.contains("1. feito (a)") && texto.contains("2. feito (b)"));
        assert!(texto.contains("Revisor: APROVARIA — bom\n(e 1 problema de gravidade baixa)"));
        assert!(texto.ends_with("Decida: aprovar_tarefa 12 ou pedir_ajuste 12. Detalhes: ver_diff 12 (por arquivo) e ver_chat 12."));
        let mut falhou = verde();
        let saida: String = (1..=50).map(|i| format!("linha {i}\n")).collect();
        falhou.ultima_verificacao = Some(Verificacao { comando: "pytest".into(), passou: false, saida, duracao_s: None });
        falhou.entrega = None;
        falhou.revisao.as_mut().unwrap().problemas = vec![Problema { gravidade: "alta".into(), arquivo: "b.py".into(), linha: Some(3), descricao: "quebra".into() }];
        let texto = relatorio_compacto(&falhou, "Ninee", "Pix", "o último texto", None, true, Some("conflito em b.py"));
        assert!(texto.contains("rodada 1 · terminou com ERRO"));
        assert!(texto.contains("A junção sozinha não deu: conflito em b.py"));
        assert!(texto.contains("Sem entrega formal; a última resposta do chat:\no último texto"));
        assert!(texto.contains("Verificação `pytest`: FALHOU (últimas linhas):\n```\nlinha 21\n") && texto.contains("linha 50\n```"));
        assert!(!texto.contains("linha 20\n"));
        assert!(texto.contains("1. [alta] b.py:3: quebra"));
    }

    #[test]
    fn relatorio_ao_pai_com_o_numero_do_ajudante_e_a_decisao() {
        let mut t = verde();
        t.id = 13;
        t.titulo = "Login".into();
        t.sobreposicao = vec!["#14: a.py".into()];
        let texto = relatorio_ao_pai(&t, "mudei a.py", Some(" a.py | 2 +\n"), false, Some("sem conta"));
        let primeira = texto.lines().next().unwrap();
        assert_eq!(primeira, "Ajudante #13 \"Login\" terminou a rodada 1.", "a interface acha o ajudante pelo #13");
        assert!(texto.contains("Relatório do ajudante:\nmudei a.py\n"));
        assert!(texto.contains("Arquivos (git diff --stat):\na.py | 2 +"));
        assert!(texto.contains("Verificação `cargo test`: PASSOU") && !texto.contains("```"));
        assert!(texto.contains("Revisor: APROVARIA"));
        assert!(texto.contains("outros chats mexem nos mesmos arquivos (#14: a.py)"));
        assert!(texto.contains("O revisor independente não conseguiu revisar: sem conta"));
        assert!(texto.ends_with("aprovar_tarefa 13 (junta na sua pasta) ou pedir_ajuste 13."));
        assert!(!texto.contains("ver_chat"), "o pai não tem ver_chat");
        assert!(relatorio_ao_pai(&t, "", None, true, None).starts_with("Ajudante #13 \"Login\" terminou a rodada 1 com ERRO."));
    }

    #[test]
    fn conclusao_da_divisao_acorda_uma_vez() {
        let nome = |p: &str| if p == "ninee-loja" { "Ninee".to_string() } else { "Vitral".to_string() };
        let mut d = Divisao {
            partes: vec![
                Parte { criada: Some(ParteCriada { chat: "c12".into(), tarefa: 12 }), ..parte("ninee-loja", "Checkout com Pix") },
                Parte { criada: Some(ParteCriada { chat: "c7".into(), tarefa: 7 }), ..parte("vitral", "Docs") },
                Parte { falha: Some("projeto arquivado".into()), ..parte("vitral", "Site") },
                Parte { removida: true, ..parte("vitral", "Tirada") },
            ],
            ..Default::default()
        };
        assert_eq!(conclusao_da_divisao(3, &d, &nome), None);
        d.partes[0].resultado = Some("juntada".into());
        d.partes[0].detalhe = Some("em ~/Projetos/ninee-loja".into());
        assert_eq!(conclusao_da_divisao(3, &d, &nome), None, "falta o chat 7");
        d.partes[1].resultado = Some("cancelada".into());
        d.partes[1].detalhe = Some("ficou para depois".into());
        let texto = conclusao_da_divisao(3, &d, &nome).unwrap();
        assert_eq!(
            texto,
            "A divisão 3 terminou: chat 12 \"Checkout com Pix\" (Ninee) juntado em ~/Projetos/ninee-loja; chat 7 \"Docs\" (Vitral) \
             cancelado: ficou para depois; parte que não começou: \"Site\" (projeto arquivado). Responda ao usuário com o resumo \
             por projeto (regra 6)."
        );
        d.conclusao_avisada = true;
        assert_eq!(conclusao_da_divisao(3, &d, &nome), None);
        assert_eq!(conclusao_da_divisao(4, &Divisao { partes: vec![parte("x", "y")], ..Default::default() }, &nome), None, "nada começou");
    }

    fn ocorrencia(quando: u64, tipo: &str, autor: &str, chat: &str) -> Ocorrencia {
        Ocorrencia { quando, tipo: tipo.into(), autor: autor.into(), chat: chat.into(), texto: format!("{tipo} {chat}"), ..Default::default() }
    }

    #[test]
    fn resumo_ao_voltar_por_grupo_sem_o_que_o_usuario_fez() {
        let ocorrencias = vec![
            ocorrencia(5, "juntou", "vitral", "c1"),
            ocorrencia(11, "entrega", "c12", "c12"),
            ocorrencia(12, "juntou", "vitral", "c12"),
            ocorrencia(13, "juntou", "usuario", "c3"),
            ocorrencia(14, "permissao", "c7", "c7"),
            ocorrencia(15, "conflito", "vitral", "c9"),
            ocorrencia(16, "pedido", "mestre", "c9"),
            ocorrencia(30, "falhou", "vitral", "c2"),
        ];
        let aberto = cartao_de(4, EstadoCartao::Aberto, 17, Divisao { partes: vec![parte("a", "b")], ..Default::default() });
        let antigo = cartao_de(2, EstadoCartao::Aberto, 1, Divisao::default());
        let r = resumo_desde(10, 20, &ocorrencias, &[aberto, antigo]).unwrap();
        let linhas: Vec<(&str, &str)> = r.linhas.iter().map(|l| (l.grupo, l.texto.as_str())).collect();
        assert_eq!(
            linhas,
            [
                ("terminou", "juntou c12"),
                ("precisa", "permissao c7"),
                ("precisa", "A divisão 4 espera o Começar (1 parte)"),
                ("falhou", "conflito c9"),
            ]
        );
        assert_eq!((r.desde, r.ate), (10, 20));
        assert_eq!(resumo_desde(20, 25, &ocorrencias, &[]), None);
        assert_eq!(resumo_desde(12, 13, &ocorrencias, &[]), None, "só o que o usuário fez");
    }

    #[test]
    fn usuario_primeiro_e_o_freio_de_ciclo() {
        let mut entradas = vec![
            Entrada { de: Some("Vitral".into()), texto: "relatório".into(), ..Default::default() },
            Entrada { texto: "primeira".into(), ..Default::default() },
            Entrada { de: Some("chat 12".into()), remetente: Some("c12".into()), texto: "pergunta".into(), ..Default::default() },
            Entrada { texto: "segunda".into(), ..Default::default() },
        ];
        usuario_primeiro(&mut entradas);
        let textos: Vec<&str> = entradas.iter().map(|e| e.texto.as_str()).collect();
        assert_eq!(textos, ["primeira", "segunda", "relatório", "pergunta"]);
        let mut ciclos = Ciclos::default();
        for n in 1..=LIMITE_DO_CICLO {
            assert_eq!(ciclos.contar("c12", ID_MESTRE), n);
        }
        assert!(ciclos.travados().is_empty());
        assert_eq!(ciclos.contar(ID_MESTRE, "c12"), LIMITE_DO_CICLO + 1, "o par não tem sentido");
        assert_eq!(ciclos.travados(), [("c12".to_string(), ID_MESTRE.to_string())]);
        assert_eq!(ciclos.contar("c7", "c12"), 1, "outro par conta à parte");
        ciclos.zerar("c12");
        assert!(ciclos.travados().is_empty());
        assert_eq!(ciclos.contar(ID_MESTRE, "c12"), 1);
    }

    #[test]
    fn regra_do_dono() {
        let do_mestre = Tarefa { coordenador: ID_MESTRE.into(), ..tarefa_de(12, "c12", EstadoTarefa::EmRevisao) };
        assert_eq!(pedido_de(&do_mestre, ID_MESTRE), Ok(()));
        assert_eq!(pedido_de(&tarefa_de(12, "c12", EstadoTarefa::Conversa), ID_MESTRE).unwrap_err(), "o chat 12 não tem pedido seu (é uma conversa do usuário)");
        let do_pai = Tarefa { coordenador: "c3".into(), ..tarefa_de(13, "c13", EstadoTarefa::Rodando) };
        assert!(pedido_de(&do_pai, ID_MESTRE).unwrap_err().contains("é de outro chat"));
    }

    #[test]
    fn catalogo_com_descricao_readme_e_pastas() {
        let loja = pasta("loja");
        let mut p = projeto("ninee-loja", "Ninee", &[loja.as_path(), Path::new("/nao/existe/x")]);
        p.descricao = "Loja online da Ninee (Django).\n".into();
        let git = |pasta: &Path| pasta == loja;
        let linha = linha_do_catalogo(&p, None, &git);
        assert_eq!(linha, format!("- Ninee (#ninee) — Loja online da Ninee (Django). Pastas: {} [git], /nao/existe/x (não encontrada).", loja.display()));
        p.descricao.clear();
        let readme = linha_do_catalogo(&p, Some(("ADE em Rust com GPUI".into(), "README")), &git);
        assert!(readme.contains("— ADE em Rust com GPUI (do README). Pastas:"), "{readme}");
        assert!(linha_do_catalogo(&p, None, &git).contains("— sem descrição."));
        let propria = Projeto { pastas: vec![PastaLigada { caminho: loja.clone(), propria: true, ..Default::default() }], ..projeto("receitas", "Receitas", &[]) };
        assert_eq!(linha_do_catalogo(&propria, None, &git), "- Receitas (#receitas) — sem descrição. Pastas: pasta própria.");
        let texto = "# Vitral\n\n[![ci](x)](y)\n<p>logo</p>\n\nAmbiente para trabalhar com vários\nchats do Claude Code.\n\nOutro parágrafo.";
        assert_eq!(primeiro_paragrafo(texto), "Ambiente para trabalhar com vários chats do Claude Code");
        assert_eq!(primeiro_paragrafo(&"palavra ".repeat(60)).chars().count(), 200);
        assert_eq!(primeiro_paragrafo("# Só título\n"), "");
        let com_codigo = "Site estático (HTML e CSS puros). Para ver localmente:\n```\npython3 -m http.server\n```";
        assert_eq!(primeiro_paragrafo(com_codigo), "Site estático (HTML e CSS puros)");
    }

    #[test]
    fn entrega_partes_e_chats_pelo_texto() {
        let criterios = criterios_da_entrega(&serde_json::json!([
            {"indice": 1, "estado": "feito", "evidencia": "test_pix passa"},
            {"indice": "2", "estado": "Não"},
            {"indice": 3, "estado": "parcial"},
            {"estado": "feito"}
        ]));
        let estados: Vec<(usize, &str)> = criterios.iter().map(|c| (c.indice, c.estado.as_str())).collect();
        assert_eq!(estados, [(1, "feito"), (2, "nao"), (3, "parcial")]);
        let texto = criterios_da_entrega(&serde_json::json!("[{\"indice\": 1, \"estado\": \"feito\"}]"));
        assert_eq!(texto.len(), 1, "o array que veio como texto");
        let e = Entrega { resumo: "Pix".into(), como_verificou: "pytest".into(), criterios, pendencias: "o QR".into(), ..Default::default() };
        let t = texto_da_entrega(&e, &["mostra".into(), "paga".into()]);
        assert_eq!(t, "Resumo: Pix\nComo verificou: pytest\nCritérios:\n1. feito (mostra): test_pix passa\n2. nao (paga)\nPendências: o QR\n");
        let p = parte_do_json(&serde_json::json!({
            "projeto": "#ninee", "chat": 12, "titulo": " Pix ", "pedido": "faça", "criterios": "[\"a\", \"b\"]",
            "verificacao": "", "arquivos": ["loja/pix.py"]
        }));
        assert_eq!((p.projeto.as_str(), p.chat.as_deref(), p.titulo.as_str()), ("#ninee", Some("12"), "Pix"));
        assert_eq!((p.criterios.len(), p.verificacao, p.arquivos.len()), (2, None, 1));
        let c = cena();
        assert_eq!(achar_chat(&c.chats, "chat 12").map(|c| c.id.as_str()), Some("c12"));
        assert_eq!(achar_chat(&c.chats, "#7").map(|c| c.id.as_str()), Some("c7"));
        assert_eq!(achar_chat(&c.chats, "Mestre").map(|c| c.id.as_str()), Some(ID_MESTRE));
        assert_eq!(achar_chat(&c.chats, "c3").map(|c| c.id.as_str()), Some("c3"));
        assert!(achar_chat(&c.chats, "99").is_none());
        assert!(contexto_cheio(Some(140_000), 200_000) && !contexto_cheio(Some(139_000), 200_000));
        assert!(!contexto_cheio(None, 200_000) && !contexto_cheio(Some(10), 0));
        assert_eq!((ha_quanto(30), ha_quanto(720), ha_quanto(7200), ha_quanto(90_000), ha_quanto(200_000)), ("agora".into(), "há 12 min".into(), "há 2 h".into(), "há 1 dia".into(), "há 2 dias".into()));
    }
}
