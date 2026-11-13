//! Migração do Vitral antigo, com um `.vitral/` em cada pasta de trabalho, para os dados globais.
//!
//! Roda uma vez só, no primeiro uso. Nada é escrito nas pastas antigas, que ficam como cópia de
//! segurança: lê tudo sem escrever, planeja numa função pura e grava direto na pasta de dados, com
//! o `estado.json` por último. Uma queda antes dele deixa a próxima abertura como primeiro uso,
//! que refaz tudo por cima.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context as _, Result, bail};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::{
    dados::{self, Dados},
    modelo::{Config, Forma},
};

/// Uma pasta de trabalho do Vitral antigo, lida sem escrever nada nela.
#[derive(Clone, Debug, Default)]
pub struct RaizAntiga {
    /// Canônica.
    pub raiz: PathBuf,
    /// `.vitral/estado.json`; `None` sem ele ou com ele ilegível (aí vêm só as pastas).
    pub estado: Option<Value>,
    pub config: Option<Value>,
    /// Bytes de `chats/<id>.json` dos chats do estado.
    pub chats: BTreeMap<String, Vec<u8>>,
    /// Texto de `notas/<projeto>.md`.
    pub notas: BTreeMap<String, String>,
    pub mtime_estado: u64,
    pub tamanho_estado: u64,
    /// As subpastas que contam como projeto, com o caminho canônico.
    pub subpastas: Vec<(String, PathBuf)>,
    /// A pasta de cada projeto citado: canônica, ou o caminho cru se ela não existe mais.
    pub caminhos: BTreeMap<String, PathBuf>,
    pub avisos: Vec<String>,
}

/// Tudo o que a migração grava, montado antes de gravar qualquer coisa.
#[derive(Clone, Debug)]
pub struct Plano {
    /// O `estado.json` novo (versão `VERSAO_ESTADO`).
    pub estado: Value,
    /// A configuração global; `None` deixa a padrão.
    pub config: Option<Value>,
    pub chats: Vec<(String, Vec<u8>)>,
    pub registro: Registro,
}

/// O `migracao.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Registro {
    pub concluida_em: u64,
    pub raizes: Vec<RaizMigrada>,
    pub relatorio: Relatorio,
}

/// O que veio de uma pasta antiga. O mtime e o tamanho do `estado.json` dela dizem, depois, se o
/// Vitral antigo mexeu nela.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RaizMigrada {
    pub raiz: PathBuf,
    pub mtime_estado: u64,
    pub tamanho_estado: u64,
    /// Só as renumeradas: número antigo → novo.
    pub tarefas: BTreeMap<String, String>,
    pub chats: usize,
    pub avisos: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Relatorio {
    pub raizes: Vec<PathBuf>,
    pub chats: usize,
    pub tarefas: usize,
    /// (antigo, novo).
    pub renumeradas: Vec<(u64, u64)>,
    pub avisos: Vec<String>,
}

#[derive(Debug)]
pub enum Resultado {
    /// Não é o primeiro uso: a migração não roda mais.
    NaoPrecisa,
    /// Primeiro uso sem Vitral antigo: o primeiro salvar cria o estado.
    SemRaizes,
    Migrou(Relatorio),
    /// O Vitral antigo está aberto: nada foi lido nem escrito.
    Adiada { raizes: Vec<PathBuf>, pids: Vec<u32> },
    /// Nada de `estado.json`: a próxima abertura tenta de novo.
    Falhou { raiz: PathBuf, erro: String },
}

/// A migração inteira, no primeiro uso. `proc` é o `/proc` (outro nos testes), `proprio`, o pid
/// deste processo, e `na_bancada`, se ele roda na bancada (que tem os dados dela).
pub fn migrar(dados: &Dados, raizes: &[PathBuf], proc: &Path, proprio: u32, na_bancada: bool, quando: u64) -> Resultado {
    if !dados.primeiro_uso() {
        return Resultado::NaoPrecisa;
    }
    if raizes.is_empty() {
        return Resultado::SemRaizes;
    }
    let pids = vitral_antigo_vivo(proc, proprio, na_bancada);
    if !pids.is_empty() {
        return Resultado::Adiada { raizes: raizes.to_vec(), pids };
    }
    let mut lidas = Vec::new();
    for raiz in raizes {
        match ler_raiz(raiz) {
            Ok(lida) => lidas.push(lida),
            Err(erro) => return Resultado::Falhou { raiz: raiz.clone(), erro: format!("{erro:#}") },
        }
    }
    let plano = planejar(&lidas, quando);
    match aplicar(dados, &plano) {
        Ok(()) => Resultado::Migrou(plano.registro.relatorio),
        Err(erro) => Resultado::Falhou { raiz: dados.pasta.clone(), erro: format!("{erro:#}") },
    }
}

/// As pastas de trabalho do Vitral antigo, da mais recente para a mais antiga: a do argumento
/// primeiro (o Vitral antigo a abriria, e ela viraria a mais recente), depois as de
/// `recentes.json`. Só as que existem e têm `.vitral/estado.json` ou `.vitral/config.json`.
pub fn raizes_antigas(dados: &Dados, argumento: Option<&Path>) -> Vec<PathBuf> {
    let recentes = dados::ler_valor_sem_escrever(&dados.arquivo_recentes()).ok().flatten();
    let recentes = recentes
        .as_ref()
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(PathBuf::from);
    let mut raizes: Vec<PathBuf> = Vec::new();
    for raiz in argumento.map(Path::to_path_buf).into_iter().chain(recentes) {
        let Ok(raiz) = dunce::canonicalize(raiz) else {
            continue;
        };
        if tem_vitral_antigo(&raiz) && !raizes.contains(&raiz) {
            raizes.push(raiz);
        }
    }
    raizes
}

/// As pastas antigas que a migração trouxe (do `migracao.json`).
pub fn raizes_migradas(dados: &Dados) -> Vec<PathBuf> {
    dados::ler_valor_sem_escrever(&dados.arquivo_migracao())
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_value::<Registro>(v).ok())
        .map(|r| r.raizes.into_iter().map(|r| r.raiz).collect())
        .unwrap_or_default()
}

/// A pasta tem o `.vitral/` do Vitral antigo.
pub fn tem_vitral_antigo(pasta: &Path) -> bool {
    let antiga = pasta.join(".vitral");
    antiga.join("estado.json").is_file() || antiga.join("config.json").is_file()
}

/// Os pids dos processos `vitral` vivos, fora este, os filhos dele (um fork ainda sem o exec tem o
/// mesmo nome) e os do outro lado da bancada: fora dela, os da bancada (`DENTRO_DA_BANCADA=1`) não
/// contam; dentro, só os dela contam (o Vitral do dono, aberto fora, usa outras pastas), e só os
/// desta mesma bancada (a de outro agente rodando ao mesmo tempo tem outro `XDG_DATA_HOME`). Este
/// processo segura a trava, então outro `vitral` só pode ser o binário antigo. Sem o `environ`
/// legível, o processo conta.
pub fn vitral_antigo_vivo(proc: &Path, proprio: u32, na_bancada: bool) -> Vec<u32> {
    let Ok(entradas) = fs::read_dir(proc) else {
        return Vec::new();
    };
    let meus_dados = std::env::var_os("XDG_DATA_HOME").unwrap_or_default();
    let mut pids: Vec<u32> = entradas
        .flatten()
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter(|&pid| pid != proprio)
        .filter(|pid| {
            let processo = proc.join(pid.to_string());
            let vitral = fs::read_to_string(processo.join("comm")).is_ok_and(|c| c.trim_end() == "vitral");
            let (da_bancada, outra_bancada) = match fs::read(processo.join("environ")) {
                Ok(e) => {
                    let bancada = e.split(|&b| b == 0).any(|v| v == b"DENTRO_DA_BANCADA=1");
                    let dados = e.split(|&b| b == 0).find_map(|v| v.strip_prefix(b"XDG_DATA_HOME=")).unwrap_or_default();
                    (bancada, na_bancada && bancada && dados != meus_dados.as_encoded_bytes())
                }
                Err(_) => (na_bancada, false),
            };
            vitral && da_bancada == na_bancada && !outra_bancada && pai(&processo) != Some(proprio)
        })
        .collect();
    pids.sort_unstable();
    pids
}

/// O pid do pai, do `stat` (o nome entre parênteses pode ter espaços: o resto vem depois do último).
fn pai(processo: &Path) -> Option<u32> {
    let stat = fs::read_to_string(processo.join("stat")).ok()?;
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

enum Antigo {
    Ausente,
    Lido(Value),
    Ilegivel,
}

fn ler_antigo(caminho: &Path) -> Result<Antigo> {
    match dados::ler_valor_sem_escrever(caminho) {
        Ok(None) => Ok(Antigo::Ausente),
        Ok(Some(valor)) => Ok(Antigo::Lido(valor)),
        Err(erro) if dados::e_ilegivel(&erro) => Ok(Antigo::Ilegivel),
        Err(erro) => Err(erro),
    }
}

/// Lê uma pasta antiga sem escrever nada nela (nem a cópia de um arquivo ilegível). Conteúdo
/// estragado vira aviso; erro de leitura do disco faz a migração inteira falhar.
pub fn ler_raiz(raiz: &Path) -> Result<RaizAntiga> {
    let raiz = dunce::canonicalize(raiz).with_context(|| raiz.display().to_string())?;
    let pasta = raiz.join(".vitral");
    let mut lida = RaizAntiga { raiz: raiz.clone(), ..Default::default() };
    let arquivo = pasta.join("estado.json");
    lida.estado = match ler_antigo(&arquivo)? {
        Antigo::Ausente => None,
        Antigo::Lido(estado) if forma_de_estado(&estado) => Some(estado),
        _ => {
            lida.avisos.push(format!(
                "O estado antigo de {} está ilegível; as conversas dele não vieram.",
                raiz.display()
            ));
            None
        }
    };
    if let Ok(meta) = fs::metadata(&arquivo) {
        lida.tamanho_estado = meta.len();
        lida.mtime_estado = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
    }
    lida.config = match ler_antigo(&pasta.join("config.json"))? {
        Antigo::Ausente => None,
        Antigo::Lido(config) if config.is_object() => Some(config),
        _ => {
            lida.avisos.push(format!(
                "A configuração antiga de {} está ilegível; ficou a padrão.",
                raiz.display()
            ));
            None
        }
    };
    for id in lista(lida.estado.as_ref(), "chats").filter_map(|c| c.get("id")?.as_str()) {
        if !nome_de_arquivo(id) {
            continue;
        }
        let caminho = pasta.join("chats").join(format!("{id}.json"));
        match fs::read(&caminho) {
            Ok(bytes) => {
                lida.chats.insert(id.to_string(), bytes);
            }
            Err(erro) if erro.kind() == io::ErrorKind::NotFound => {}
            Err(erro) => return Err(anyhow::Error::new(erro).context(caminho.display().to_string())),
        }
    }
    lida.notas = ler_notas(&pasta.join("notas"))?;
    lida.subpastas = subpastas_antigas(&raiz).with_context(|| raiz.display().to_string())?;
    for nome in nomes_de_projeto(&lida) {
        let caminho = match lida.subpastas.iter().find(|(n, _)| *n == nome) {
            Some((_, caminho)) => caminho.clone(),
            None if nome_de_arquivo(&nome) => {
                let cru = raiz.join(&nome);
                dunce::canonicalize(&cru).unwrap_or(cru)
            }
            None => continue,
        };
        lida.caminhos.insert(nome, caminho);
    }
    Ok(lida)
}

/// Os projetos do Vitral antigo (as subpastas da raiz e os citados no estado), em ordem alfabética:
/// é deles que sai a pasta de cada chat.
fn nomes_de_projeto(raiz: &RaizAntiga) -> Vec<String> {
    let mut nomes: BTreeSet<String> = raiz.subpastas.iter().map(|(n, _)| n.clone()).collect();
    let citados = lista(raiz.estado.as_ref(), "chats")
        .chain(lista(raiz.estado.as_ref(), "tarefas"))
        .filter_map(|item| item.get("projeto")?.as_str())
        .filter(|p| !p.is_empty());
    nomes.extend(citados.map(str::to_string));
    let mut nomes: Vec<String> = nomes.into_iter().collect();
    nomes.sort_by(|a, b| ordem_alfabetica(a, b));
    nomes
}

/// Com a regra da lista de projetos do Vitral antigo: pastas de verdade (links simbólicos ficam
/// de fora), sem as ocultas, em ordem alfabética sem diferenciar maiúsculas.
pub fn subpastas_antigas(raiz: &Path) -> io::Result<Vec<(String, PathBuf)>> {
    let mut pastas = Vec::new();
    for entrada in fs::read_dir(raiz)?.flatten() {
        if !entrada.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(nome) = entrada.file_name().into_string() else {
            continue;
        };
        if nome.starts_with('.') {
            continue;
        }
        let caminho = dunce::canonicalize(entrada.path()).unwrap_or_else(|_| entrada.path());
        pastas.push((nome, caminho));
    }
    pastas.sort_by(|a, b| ordem_alfabetica(&a.0, &b.0));
    Ok(pastas)
}

fn ler_notas(pasta: &Path) -> Result<BTreeMap<String, String>> {
    let mut notas = BTreeMap::new();
    let entradas = match fs::read_dir(pasta) {
        Ok(entradas) => entradas,
        Err(erro) if erro.kind() == io::ErrorKind::NotFound => return Ok(notas),
        Err(erro) => return Err(anyhow::Error::new(erro).context(pasta.display().to_string())),
    };
    for entrada in entradas.flatten() {
        if !entrada.file_type().is_ok_and(|t| t.is_file()) {
            continue;
        }
        let nome = entrada.file_name();
        let Some(projeto) = nome.to_str().and_then(|n| n.strip_suffix(".md")) else {
            continue;
        };
        let bytes = fs::read(entrada.path()).with_context(|| entrada.path().display().to_string())?;
        notas.insert(projeto.to_string(), String::from_utf8_lossy(&bytes).into_owned());
    }
    Ok(notas)
}

fn forma_de_estado(estado: &Value) -> bool {
    estado.is_object()
        && ["chats", "tarefas"].iter().all(|l| estado.get(l).is_none_or(Value::is_array))
        && estado.get("filas").is_none_or(Value::is_object)
}

/// Serve de nome de arquivo e de pasta: vai em `notas/<id>.md`, `chats/<id>.json` e `copias/<id>`.
fn nome_de_arquivo(nome: &str) -> bool {
    !nome.is_empty() && !nome.starts_with('.') && !nome.contains(['/', '\0'])
}

fn ordem_alfabetica(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

fn lista<'a>(valor: Option<&'a Value>, campo: &str) -> impl Iterator<Item = &'a Value> {
    valor
        .and_then(|v| v.get(campo))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
}

/// `base` ou `base-2`, `base-3`…: o primeiro que `serve` e ainda não está em `usados` (e passa a
/// estar).
fn livre(base: &str, usados: &mut HashSet<String>, serve: impl Fn(&str) -> bool) -> String {
    let id = std::iter::once(base.to_string())
        .chain((2..).map(|n| format!("{base}-{n}")))
        .find(|c| serve(c) && !usados.contains(c))
        .expect("sempre há um sufixo livre");
    usados.insert(id.clone());
    id
}

/// Troca o texto do campo pelo do mapa, quando ele está lá.
fn trocar(objeto: &mut Map<String, Value>, campo: &str, mapa: &HashMap<String, String>) {
    if let Some(Value::String(texto)) = objeto.get_mut(campo)
        && let Some(novo) = mapa.get(texto.as_str())
    {
        *texto = novo.clone();
    }
}

/// A forma de uma tarefa antiga: direta quando ela roda na pasta do projeto (sem cópia, sem ter
/// apagado uma, já preparada); senão, cópia.
fn forma_antiga(tarefa: &Map<String, Value>) -> Forma {
    let tem = |campo: &str| tarefa.get(campo).is_some_and(|v| !v.is_null());
    let limpa = tarefa.get("copia_limpa").and_then(Value::as_bool).unwrap_or(false);
    let preparada = tarefa.get("preparada").and_then(Value::as_bool).unwrap_or(true);
    let apagada = limpa || (!tem("worktree") && tem("branch"));
    if !tem("worktree") && !apagada && preparada { Forma::Direta } else { Forma::Copia }
}

/// A conversa com avisos do Vitral no fim; `None` se ela não for uma lista legível.
fn com_avisos(bytes: &[u8], textos: &[String]) -> Option<Vec<u8>> {
    let mut itens: Vec<Value> = serde_json::from_slice(bytes).ok()?;
    itens.extend(textos.iter().map(|t| json!({"Aviso": {"texto": t, "erro": false}})));
    serde_json::to_vec_pretty(&itens).ok()
}

/// A configuração global: a da pasta mais recente, conferida pelo `Config`, sem projetos e
/// rotinas. Das outras só vêm as configurações por pasta; se elas escolhiam outra permissão,
/// outros modelos ou outra conta, o relatório avisa.
fn config_global(raizes: &[RaizAntiga], avisos: &mut Vec<String>) -> Option<Value> {
    let tipada = |raiz: &RaizAntiga| -> Option<Result<Config, ()>> {
        let valor = raiz.config.as_ref()?;
        Some(serde_json::from_value(dados::config_sem_legado(valor.clone())).map_err(|_| ()))
    };
    let escolhas = |c: &Config| {
        [&c.permissao, &c.modelo_chats, &c.conta]
            .map(String::clone)
    };
    let primeira = raizes.first()?;
    let referencia = match tipada(primeira) {
        Some(Ok(config)) => config,
        Some(Err(())) => {
            avisos.push(format!(
                "A configuração de {} não pôde ser lida; ficou a padrão.",
                primeira.raiz.display()
            ));
            Config::default()
        }
        None => Config::default(),
    };
    for outra in &raizes[1..] {
        if let Some(Ok(config)) = tipada(outra)
            && escolhas(&config) != escolhas(&referencia)
        {
            avisos.push(format!(
                "A permissão, os modelos ou a conta de {} eram outros; ficaram os de {}.",
                outra.raiz.display(),
                primeira.raiz.display()
            ));
        }
    }
    match tipada(primeira)? {
        Ok(_) => Some(dados::config_sem_legado(primeira.config.clone()?)),
        Err(()) => None,
    }
}

/// Monta tudo o que a migração grava, para todas as pastas antigas juntas (a mais recente
/// primeiro). Pura: a mesma entrada dá a mesma saída.
pub fn planejar(raizes: &[RaizAntiga], quando: u64) -> Plano {
    let mut avisos: Vec<Vec<String>> = raizes.iter().map(|r| r.avisos.clone()).collect();
    // As notas dos projetos não vêm (os chats não têm mais projeto): ficam no .vitral antigo.
    for (r, raiz) in raizes.iter().enumerate() {
        if !raiz.notas.is_empty() {
            avisos[r].push(format!(
                "As decisões anotadas nos projetos ficaram em {}.",
                raiz.raiz.join(".vitral").join("notas").display()
            ));
        }
    }

    // Chats: o id fica; um repetido entre pastas (uma pasta copiada com o .vitral junto) ganha
    // sufixo, na pasta mais antiga.
    let mut chats_usados = HashSet::new();
    let mut ids_chat: Vec<HashMap<String, String>> = vec![HashMap::new(); raizes.len()];
    for (r, raiz) in raizes.iter().enumerate() {
        for chat in lista(raiz.estado.as_ref(), "chats") {
            let Some(id) = chat.get("id").and_then(Value::as_str) else {
                continue;
            };
            if ids_chat[r].contains_key(id) {
                continue;
            }
            let base = if nome_de_arquivo(id) { id } else { "chat" };
            let novo = livre(base, &mut chats_usados, nome_de_arquivo);
            ids_chat[r].insert(id.to_string(), novo);
        }
    }

    // Tarefas, em duas passadas: cada uma fica com o número se ele ainda não apareceu (a pasta
    // mais recente primeiro); as repetidas vão para depois do maior número de todas as pastas.
    let numeros: Vec<Vec<u64>> = raizes
        .iter()
        .map(|r| lista(r.estado.as_ref(), "tarefas").filter_map(|t| t.get("id")?.as_u64()).collect())
        .collect();
    let mut tomados = HashSet::new();
    let mut ids_tarefa: Vec<HashMap<u64, u64>> = vec![HashMap::new(); raizes.len()];
    let mut repetidas = Vec::new();
    for (r, da_raiz) in numeros.iter().enumerate() {
        for &n in da_raiz {
            if tomados.insert(n) {
                ids_tarefa[r].insert(n, n);
            } else {
                repetidas.push((r, n));
            }
        }
    }
    // A próxima de cada pasta também conta: o branch `vitral/t<N>` pode existir sem a tarefa.
    let proximas = raizes.iter().map(|r| {
        let proxima = r.estado.as_ref().and_then(|e| e.get("proxima_tarefa")?.as_u64());
        proxima.unwrap_or(0).saturating_sub(1)
    });
    let mut proxima = numeros.iter().flatten().copied().chain(proximas).max().unwrap_or(0) + 1;
    let mut renumeradas: Vec<Vec<(u64, u64)>> = vec![Vec::new(); raizes.len()];
    for (r, n) in repetidas {
        // Repetida dentro da mesma pasta: já vinha assim e fica assim.
        if ids_tarefa[r].contains_key(&n) {
            continue;
        }
        ids_tarefa[r].insert(n, proxima);
        renumeradas[r].push((n, proxima));
        proxima += 1;
    }

    let mut chats = Vec::new();
    let mut tarefas = Vec::new();
    let mut avisos_do_chat: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (r, raiz) in raizes.iter().enumerate() {
        for chat in lista(raiz.estado.as_ref(), "chats") {
            let com_id = chat.as_object().filter(|c| c.get("id").is_some_and(Value::is_string));
            let Some(mut chat) = com_id.cloned() else {
                avisos[r].push(format!("Um chat sem id em {} ficou de fora.", raiz.raiz.display()));
                continue;
            };
            trocar(&mut chat, "id", &ids_chat[r]);
            trocar(&mut chat, "pai", &ids_chat[r]);
            // O projeto arquivado (a marca era da pasta) leva as conversas dele para as arquivadas.
            let projeto = chat.get("projeto").and_then(Value::as_str).unwrap_or_default();
            let arquivado = raiz.config.as_ref().and_then(|c| c.get("projetos")?.get(projeto)?.get("arquivado")?.as_bool());
            if arquivado == Some(true) {
                chat.insert("arquivado".into(), json!(true));
            }
            if let Some(n) = chat.get("tarefa").and_then(Value::as_u64)
                && let Some(novo) = ids_tarefa[r].get(&n)
            {
                chat.insert("tarefa".into(), json!(novo));
            }
            chat.insert("criado_por".into(), json!("migracao"));
            chats.push(Value::Object(chat));
        }
        for tarefa in lista(raiz.estado.as_ref(), "tarefas") {
            let Some((mut tarefa, n)) = tarefa
                .as_object()
                .and_then(|t| Some((t.clone(), t.get("id")?.as_u64()?)))
            else {
                avisos[r].push(format!("Um pedido sem número em {} ficou de fora.", raiz.raiz.display()));
                continue;
            };
            let novo = ids_tarefa[r][&n];
            tarefa.insert("id".into(), json!(novo));
            trocar(&mut tarefa, "coordenador", &ids_chat[r]);
            trocar(&mut tarefa, "trabalhador", &ids_chat[r]);
            // A pasta do projeto da tarefa: de onde a cópia saiu (a carga põe o chat nela).
            let origem = tarefa.get("projeto").and_then(Value::as_str).and_then(|p| raiz.caminhos.get(p));
            tarefa.insert("origem".into(), json!(origem.cloned().unwrap_or_default()));
            tarefa.insert("forma".into(), json!(forma_antiga(&tarefa)));
            if novo != n
                && let Some(trabalhador) = tarefa.get("trabalhador").and_then(Value::as_str)
            {
                avisos_do_chat
                    .entry(trabalhador.to_string())
                    .or_default()
                    .push(format!("Na importação, este chat passou de #{n} para chat {novo}."));
            }
            tarefas.push(Value::Object(tarefa));
        }
    }

    let mut arquivos_de_chat = Vec::new();
    for (r, raiz) in raizes.iter().enumerate() {
        for (antigo, bytes) in &raiz.chats {
            let Some(id) = ids_chat[r].get(antigo) else {
                continue;
            };
            let bytes = match avisos_do_chat.remove(id) {
                Some(textos) => com_avisos(bytes, &textos).unwrap_or_else(|| {
                    avisos[r].push(format!(
                        "A conversa {antigo} de {} não deu para ler; foi como estava, sem o aviso \
                         do número novo.",
                        raiz.raiz.display()
                    ));
                    bytes.clone()
                }),
                None => bytes.clone(),
            };
            arquivos_de_chat.push((id.clone(), bytes));
        }
    }
    // Renumerada sem conversa gravada: o arquivo nasce só com o aviso.
    let ids_novos: HashSet<&str> = ids_chat.iter().flat_map(|m| m.values().map(String::as_str)).collect();
    for (id, textos) in avisos_do_chat {
        if ids_novos.contains(id.as_str())
            && let Some(bytes) = com_avisos(b"[]", &textos)
        {
            arquivos_de_chat.push((id, bytes));
        }
    }

    let mut filas = Map::new();
    for (r, raiz) in raizes.iter().enumerate() {
        let Some(antigas) = raiz.estado.as_ref().and_then(|e| e.get("filas")?.as_object()) else {
            continue;
        };
        for (chat, entradas) in antigas {
            let chat = ids_chat[r].get(chat).unwrap_or(chat);
            filas.entry(chat.clone()).or_insert_with(|| entradas.clone());
        }
    }

    let mut gerais = Vec::new();
    let config = config_global(raizes, &mut gerais);

    // Os chats e as tarefas como vieram: a carga põe cada chat direto na pasta dele e larga as
    // tarefas, como faz com os dados das versões com tarefas.
    let mut estado = Map::new();
    estado.insert("versao".into(), json!(crate::modelo::VERSAO_ESTADO));
    estado.insert("chats".into(), Value::Array(chats));
    estado.insert("tarefas".into(), Value::Array(tarefas));
    estado.insert("proxima_tarefa".into(), json!(proxima));
    estado.insert("filas".into(), Value::Object(filas));
    // Campos que o antigo tinha e esta versão não conhece passam como vieram (o da pasta mais
    // recente vence).
    for antigo in raizes.iter().filter_map(|r| r.estado.as_ref()?.as_object()) {
        for (chave, valor) in antigo {
            if !estado.contains_key(chave) {
                estado.insert(chave.clone(), valor.clone());
            }
        }
    }

    let raizes_migradas: Vec<RaizMigrada> = raizes
        .iter()
        .enumerate()
        .map(|(r, raiz)| RaizMigrada {
            raiz: raiz.raiz.clone(),
            mtime_estado: raiz.mtime_estado,
            tamanho_estado: raiz.tamanho_estado,
            tarefas: renumeradas[r].iter().map(|(a, n)| (a.to_string(), n.to_string())).collect(),
            chats: lista(raiz.estado.as_ref(), "chats").count(),
            avisos: avisos[r].clone(),
        })
        .collect();
    let relatorio = Relatorio {
        raizes: raizes.iter().map(|r| r.raiz.clone()).collect(),
        chats: estado["chats"].as_array().map_or(0, Vec::len),
        tarefas: estado["tarefas"].as_array().map_or(0, Vec::len),
        renumeradas: renumeradas.into_iter().flatten().collect(),
        avisos: avisos.into_iter().flatten().chain(gerais).collect(),
    };
    Plano {
        estado: Value::Object(estado),
        config,
        chats: arquivos_de_chat,
        registro: Registro { concluida_em: quando, raizes: raizes_migradas, relatorio },
    }
}

/// Grava o plano direto na pasta de dados, com o `estado.json` por último: ele é o ponto em que a
/// migração conta como feita. Recusa se já houver estado.
pub fn aplicar(dados: &Dados, plano: &Plano) -> Result<()> {
    if !dados.primeiro_uso() {
        bail!("{} já tem um estado; a migração não grava por cima", dados.pasta.display());
    }
    gravar_antes_do_estado(dados, plano)?;
    dados::gravar_json_seguro(&dados.arquivo_estado(), &plano.estado, false)?;
    // Depois do estado: até aqui o disco é só do Vitral antigo, e a volta para ele ainda vale.
    dados.marcar_formato();
    if let Some(mae) = dados.pasta.parent() {
        dados::sincronizar_pasta(mae)?;
    }
    Ok(())
}

/// Tudo menos o `estado.json`. Uma queda aqui no meio deixa restos que a próxima vez grava por cima.
fn gravar_antes_do_estado(dados: &Dados, plano: &Plano) -> Result<()> {
    let chats = dados.pasta.join("chats");
    fs::create_dir_all(&chats).with_context(|| chats.display().to_string())?;
    for (id, bytes) in &plano.chats {
        dados::gravar_seguro(&dados.arquivo_chat(id), bytes, false)?;
    }
    dados::gravar_json_seguro(&dados.arquivo_migracao(), &plano.registro, false)?;
    if let Some(config) = &plano.config
        && dados::nao_existe(&dados.arquivo_config())
    {
        dados::gravar_json_seguro(&dados.arquivo_config(), config, false)?;
        if let Some(mae) = dados.config.parent() {
            dados::sincronizar_pasta(mae)?;
        }
    }
    // As pastas novas no disco antes do estado.json, que aponta para o que está nelas.
    for pasta in [&chats, &dados.pasta] {
        dados::sincronizar_pasta(pasta)?;
    }
    if let Some(mae) = dados.pasta.parent() {
        dados::sincronizar_pasta(mae)?;
    }
    Ok(())
}

impl Relatorio {
    /// O aviso da migração: "Trouxe 2 conversas do Vitral antigo de ~/Projetos."
    pub fn texto(&self, casa: Option<&Path>) -> String {
        let curto = |p: &Path| match casa.and_then(|c| p.strip_prefix(c).ok()) {
            Some(resto) if resto.as_os_str().is_empty() => "~".to_string(),
            Some(resto) => format!("~/{}", resto.display()),
            None => p.display().to_string(),
        };
        let raizes = juntar(&self.raizes.iter().map(|r| curto(r)).collect::<Vec<_>>());
        let mut partes = vec![match self.chats {
            0 => format!("Não havia conversas do Vitral antigo em {raizes}."),
            1 => format!("Trouxe 1 conversa do Vitral antigo de {raizes}."),
            n => format!("Trouxe {n} conversas do Vitral antigo de {raizes}."),
        }];
        match self.renumeradas.len() {
            0 => {}
            1 => partes.push("Um chat mudou de número (o aviso está nele).".into()),
            n => partes.push(format!("{n} chats mudaram de número (o aviso está em cada um).")),
        }
        // Os avisos levam o caminho inteiro (no migracao.json também); no texto, com "~".
        let casa_em_texto = casa.map(|c| format!("{}/", c.display())).filter(|c| c.len() > 1);
        partes.extend(self.avisos.iter().map(|a| match &casa_em_texto {
            Some(c) => a.replace(c.as_str(), "~/"),
            None => a.clone(),
        }));
        partes.join(" ")
    }
}

fn juntar(itens: &[String]) -> String {
    match itens {
        [] => String::new(),
        [um] => um.clone(),
        [inicio @ .., ultimo] => format!("{} e {ultimo}", inicio.join(", ")),
    }
}

#[cfg(test)]
mod testes {
    use std::time::SystemTime;

    use super::*;
    use crate::{
        dados::{ErroCarga, NoDisco},
        modelo::{Estado, VERSAO_ESTADO, novo_id},
    };

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-migracao-{nome}-{}", novo_id()));
        fs::create_dir_all(&pasta).unwrap();
        dunce::canonicalize(pasta).unwrap()
    }

    /// Um `/proc` sem nenhum processo.
    fn proc_vazio(base: &Path) -> PathBuf {
        let proc = base.join("proc");
        fs::create_dir_all(&proc).unwrap();
        proc
    }

    /// Um chat como o Vitral antigo grava em `.vitral/estado.json`.
    fn chat_antigo(
        id: &str,
        projeto: &str,
        papel: &str,
        cwd: &Path,
        tarefa: Option<u64>,
        pai: Option<&str>,
    ) -> Value {
        json!({
            "id": id, "projeto": projeto, "titulo": "Nova conversa", "papel": papel, "pai": pai,
            "tarefa": tarefa, "sessao": format!("sessao-{id}"), "sessao_criada": false,
            "cwd": cwd, "modelo": null, "modelo_atual": null, "permissao": null, "permitidas": [],
            "esforco": null, "conta": null, "contexto": null, "janela": null, "comandos": [],
            "criado_em": 1793000000, "atualizado_em": 1793000000, "custo_usd": 0.0,
            "custo_da_sessao": null, "fixado": false, "arquivado": false, "lido_em": 0,
            "turno_aberto": false, "modo_antes_do_plano": null
        })
    }

    fn tarefa_antiga(
        id: u64,
        projeto: &str,
        coordenador: &str,
        trabalhador: &str,
        worktree: Option<PathBuf>,
    ) -> Value {
        json!({
            "id": id, "projeto": projeto, "coordenador": coordenador, "trabalhador": trabalhador,
            "titulo": format!("Tarefa {id}"), "objetivo": "o", "criterios": ["c"],
            "verificacao": null, "estado": "EmRevisao", "rodada": 1, "worktree": worktree,
            "branch": worktree.as_ref().map(|_| format!("vitral/t{id}-tarefa")), "base": null,
            "copia_limpa": false, "progresso": null, "relatorio": null,
            "ultima_verificacao": null, "criada_em": 1793000000, "revisao": null,
            "sobreposicao": [], "preparacao": null, "parada": null, "entrega_pendente": null,
            "preparada": true, "ressalva": null
        })
    }

    /// Uma pasta de trabalho antiga com as subpastas e o `.vitral/` dado.
    fn raiz_antiga(raiz: &Path, subpastas: &[&str], arquivos: &[(&str, String)]) {
        for subpasta in subpastas {
            fs::create_dir_all(raiz.join(subpasta)).unwrap();
        }
        for (nome, texto) in arquivos {
            let caminho = raiz.join(".vitral").join(nome);
            fs::create_dir_all(caminho.parent().unwrap()).unwrap();
            fs::write(caminho, texto).unwrap();
        }
    }

    /// Conteúdo e mtime de tudo abaixo de `pasta`, pastas inclusive.
    fn foto(pasta: &Path) -> BTreeMap<PathBuf, (Vec<u8>, Option<SystemTime>)> {
        let mut foto = BTreeMap::new();
        let mut pendentes = vec![pasta.to_path_buf()];
        while let Some(atual) = pendentes.pop() {
            for entrada in fs::read_dir(&atual).into_iter().flatten().flatten() {
                let caminho = entrada.path();
                let meta = fs::symlink_metadata(&caminho).unwrap();
                let relativo = caminho.strip_prefix(pasta).unwrap().to_path_buf();
                let bytes = if meta.is_dir() {
                    pendentes.push(caminho.clone());
                    Vec::new()
                } else {
                    fs::read(&caminho).unwrap()
                };
                foto.insert(relativo, (bytes, meta.modified().ok()));
            }
        }
        foto
    }

    fn conteudo(pasta: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        foto(pasta).into_iter().map(|(c, (bytes, _))| (c, bytes)).collect()
    }

    fn estado_gravado(dados: &Dados) -> Value {
        dados::ler_valor_sem_escrever(&dados.arquivo_estado()).unwrap().unwrap()
    }

    fn chat<'a>(estado: &'a Value, id: &str) -> &'a Value {
        estado["chats"].as_array().unwrap().iter().find(|c| c["id"] == id).unwrap()
    }

    fn tarefa(estado: &Value, id: u64) -> &Value {
        estado["tarefas"].as_array().unwrap().iter().find(|t| t["id"] == id).unwrap()
    }

    /// O estado como o Motor o abre: carregado e normalizado (sem gravar).
    fn aberto(dados: &Dados) -> Estado {
        let mut carregado = dados.carregar().unwrap();
        dados::normalizar(&mut carregado.estado, dados, &NoDisco, 1_793_100_000);
        carregado.estado
    }

    fn migrou(resultado: Resultado) -> Relatorio {
        match resultado {
            Resultado::Migrou(relatorio) => relatorio,
            outro => panic!("não migrou: {outro:?}"),
        }
    }

    #[test]
    fn formato_desta_maquina() {
        let base = pasta_de_teste("maquina");
        let raiz = base.join("Projetos");
        let estado = json!({
            "chats": [
                chat_antigo("a1b2c3d4e5", "vitral", "Coordenador", &raiz.join("vitral"), None, None),
                chat_antigo("f6a7b8c9d0", "ninee-loja", "Coordenador", &raiz.join("ninee-loja"), None, None),
            ],
            "tarefas": [], "proxima_tarefa": 1, "filas": {}
        });
        let subpastas =
            ["vitral", "ninee-loja", "repos-baixados", "verificacao-leads-apoio", "Agenda", "site"];
        raiz_antiga(&raiz, &subpastas, &[("estado.json", estado.to_string())]);
        fs::write(raiz.join("anotacoes.txt"), "solto").unwrap();
        fs::create_dir_all(raiz.join(".oculta")).unwrap();
        let dados = Dados::em(&base.join("novo"));

        let relatorio = migrou(migrar(&dados, &[raiz.clone()], &proc_vazio(&base), 1, false, 1000));
        let gravado = estado_gravado(&dados);
        assert_eq!(gravado["versao"], VERSAO_ESTADO);
        assert!(gravado.get("projetos").is_none(), "as pastas não viram projeto");
        // O formato novo fica gravado: o atualizador não volta para uma versão que só lê o antigo.
        assert_eq!(dados.formato_no_disco(), crate::modelo::FORMATO_DADOS);
        assert!(dados.arquivo_formato().is_file());
        assert_eq!(chat(&gravado, "a1b2c3d4e5")["criado_por"], "migracao");
        // Sem config.json antigo, a global fica a padrão (nada gravado).
        assert!(dados::nao_existe(&dados.arquivo_config()));
        let registro: Registro = dados::ler_json_r(&dados.arquivo_migracao()).unwrap().unwrap();
        assert_eq!(registro.relatorio, relatorio);
        assert_eq!((relatorio.chats, relatorio.tarefas), (2, 0));
        assert_eq!(registro.raizes[0].chats, 2);
        assert!(registro.raizes[0].tamanho_estado > 0 && registro.raizes[0].mtime_estado > 0);
        // Aberto como o Motor abre: cada conversa é um chat direto na pasta dela, com a sessão de lá.
        let lido = aberto(&dados);
        let pastas: Vec<(&str, PathBuf)> = lido.chats.iter().map(|c| (c.id.as_str(), c.cwd.clone())).collect();
        assert_eq!(pastas, [("a1b2c3d4e5", raiz.join("vitral")), ("f6a7b8c9d0", raiz.join("ninee-loja"))]);
        let vitral = &lido.chats[0];
        assert!(!vitral.sessao_criada);
        assert_eq!(vitral.sessao, "sessao-a1b2c3d4e5");
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn configuracao_global_vem_e_a_pasta_arquivada_arquiva_as_conversas() {
        let base = pasta_de_teste("config");
        let raiz = base.join("Projetos");
        let config = json!({
            "permissao": "auto",
            "projetos": {"loja": {"preparacao": "pip install -r requirements.txt", "rodar": "make run",
                "copiar": [".env*"], "icone": "shopping-bag", "cor": "#f49ac1"},
                "blog": {"arquivado": true}},
            "rotinas": [{"id": "r1", "projeto": "loja", "texto": "Rode os testes", "hora": 540,
                "dias": [1, 3], "ativa": true, "ultima": "2026-10-30"}]
        });
        let estado = json!({
            "chats": [
                chat_antigo("cl", "loja", "Coordenador", &raiz.join("loja"), None, None),
                chat_antigo("cb", "blog", "Coordenador", &raiz.join("blog"), None, None),
            ],
            "tarefas": [], "proxima_tarefa": 1, "rotinas": [{"id": "r2"}]
        });
        let arquivos = [("estado.json", estado.to_string()), ("config.json", config.to_string())];
        raiz_antiga(&raiz, &["loja", "blog"], &arquivos);
        let dados = Dados::em(&base.join("novo"));
        migrou(migrar(&dados, &[raiz.clone()], &proc_vazio(&base), 1, false, 7));

        // A marca de arquivado era da pasta: as conversas dela vão para as arquivadas.
        let gravado = estado_gravado(&dados);
        assert_eq!(chat(&gravado, "cb")["arquivado"], true);
        assert_eq!(chat(&gravado, "cl")["arquivado"], false);
        // Só a configuração global: as das pastas e as rotinas não vêm.
        let global = dados::ler_valor_sem_escrever(&dados.arquivo_config()).unwrap().unwrap();
        assert_eq!(global, json!({"permissao": "auto"}));
        let lido = aberto(&dados);
        assert!(!lido.resto.contains_key("rotinas"), "as rotinas não vêm mais");
        assert_eq!(lido.chats.iter().map(|c| (c.id.as_str(), c.arquivado)).collect::<Vec<_>>(), [("cl", false), ("cb", true)]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn chat_da_pasta_que_sumiu_fica_com_ela() {
        let base = pasta_de_teste("orfao");
        let raiz = base.join("Projetos");
        let estado = json!({
            "chats": [chat_antigo("c1", "sumiu", "Coordenador", &raiz.join("sumiu"), None, None)],
            "tarefas": [], "proxima_tarefa": 1
        });
        raiz_antiga(&raiz, &["loja"], &[("estado.json", estado.to_string())]);
        let dados = Dados::em(&base.join("novo"));
        migrou(migrar(&dados, &[raiz.clone()], &proc_vazio(&base), 1, false, 1));
        // "Não encontrada" na conversa: a pasta não existe, e o chat continua apontando para ela.
        let lido = aberto(&dados);
        assert_eq!(lido.chats[0].cwd, raiz.join("sumiu"));
        assert!(!lido.chats[0].cwd.exists());
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn duas_raizes_renumeram_tarefas_e_separam_projetos() {
        let base = pasta_de_teste("duas");
        let (a, b) = (base.join("A"), base.join("B"));
        let estado_a = json!({
            "chats": [
                chat_antigo("ca", "site", "Coordenador", &a.join("site"), None, None),
                chat_antigo("wa1", "site", "Trabalhador", &a.join("wt1"), Some(1), Some("ca")),
                chat_antigo("wa2", "site", "Trabalhador", &a.join("wt2"), Some(2), Some("ca")),
                chat_antigo("wa3", "site", "Trabalhador", &a.join("site"), Some(3), Some("ca")),
            ],
            "tarefas": [
                tarefa_antiga(1, "site", "ca", "wa1", Some(a.join("wt1"))),
                tarefa_antiga(2, "site", "ca", "wa2", Some(a.join("wt2"))),
                tarefa_antiga(3, "site", "ca", "wa3", None),
            ],
            "proxima_tarefa": 4
        });
        let estado_b = json!({
            "chats": [
                chat_antigo("cb", "site", "Coordenador", &b.join("site"), None, None),
                chat_antigo("wb1", "site", "Trabalhador", &b.join("wt1"), Some(1), Some("cb")),
                chat_antigo("wb4", "blog", "Trabalhador", &b.join("wt4"), Some(4), Some("cb")),
            ],
            "tarefas": [
                tarefa_antiga(1, "site", "cb", "wb1", Some(b.join("wt1"))),
                tarefa_antiga(4, "blog", "cb", "wb4", Some(b.join("wt4"))),
            ],
            "proxima_tarefa": 5,
            "filas": {"wb1": [{"de": null, "texto": "depois", "anexos": []}]}
        });
        let conversa = json!([{"Usuario": {"texto": "oi", "anexos": [], "ponto": null}}]).to_string();
        raiz_antiga(&a, &["site"], &[("estado.json", estado_a.to_string())]);
        let arquivos_b = [("estado.json", estado_b.to_string()), ("chats/wb1.json", conversa)];
        raiz_antiga(&b, &["site", "blog"], &arquivos_b);
        let dados = Dados::em(&base.join("novo"));
        // A mais recente primeiro; uma pasta sem .vitral e uma que não existe ficam de fora.
        fs::create_dir_all(&dados.config).unwrap();
        let recentes = json!([a, base.join("sem-vitral"), b, base.join("nao-existe"), a]);
        fs::create_dir_all(base.join("sem-vitral")).unwrap();
        fs::write(dados.arquivo_recentes(), recentes.to_string()).unwrap();
        let raizes = raizes_antigas(&dados, None);
        assert_eq!(raizes, [a.clone(), b.clone()]);
        let lidas: Vec<RaizAntiga> = raizes.iter().map(|r| ler_raiz(r).unwrap()).collect();
        assert_eq!(planejar(&lidas, 9).estado, planejar(&lidas, 9).estado);

        let relatorio = migrou(migrar(&dados, &raizes, &proc_vazio(&base), 1, false, 9));
        let gravado = estado_gravado(&dados);
        for (id, trabalhador) in [(1, "wa1"), (2, "wa2"), (3, "wa3"), (4, "wb4"), (5, "wb1")] {
            assert_eq!(tarefa(&gravado, id)["trabalhador"], trabalhador, "tarefa {id}");
        }
        assert_eq!(chat(&gravado, "wb1")["tarefa"], 5);
        assert_eq!(chat(&gravado, "wb4")["tarefa"], 4);
        assert_eq!(chat(&gravado, "wa1")["tarefa"], 1);
        assert_eq!(gravado["proxima_tarefa"], 6);
        assert_eq!(relatorio.renumeradas, [(1, 5)]);
        // A origem de cada tarefa é a pasta do projeto dela, na raiz de onde veio.
        assert_eq!(tarefa(&gravado, 5)["origem"], json!(b.join("site")));
        assert_eq!(tarefa(&gravado, 1)["origem"], json!(a.join("site")));
        assert_eq!(tarefa(&gravado, 4)["origem"], json!(b.join("blog")));
        // A tarefa 3 rodava direto na pasta (sem cópia); as outras, numa cópia.
        assert_eq!(tarefa(&gravado, 3)["forma"], "Direta");
        assert_eq!(tarefa(&gravado, 1)["forma"], "Copia");
        assert_eq!(gravado["filas"]["wb1"][0]["texto"], "depois");
        // O chat renumerado ganha o aviso no fim, depois do que já tinha.
        let itens: Vec<Value> = dados::ler_json_r(&dados.arquivo_chat("wb1")).unwrap().unwrap();
        assert_eq!(itens.len(), 2);
        assert_eq!(itens[0]["Usuario"]["texto"], "oi");
        assert_eq!(itens[1]["Aviso"]["texto"], "Na importação, este chat passou de #1 para chat 5.");
        let registro: Registro = dados::ler_json_r(&dados.arquivo_migracao()).unwrap().unwrap();
        assert_eq!(registro.raizes[1].tarefas, BTreeMap::from([("1".into(), "5".into())]));
        let (lido, _) = dados.ler_estado().unwrap().unwrap();
        assert_eq!(lido.chats.len(), 7);
        assert_eq!(lido.tarefas.len(), 5);
        // Aberto: os trabalhadores das cópias (que não existem mais) mexem direto na origem, e todo
        // chat tem uma pasta de verdade.
        let lido = aberto(&dados);
        assert_eq!(lido.chats.len(), 7);
        for c in &lido.chats {
            assert!(c.cwd.is_dir(), "{} em {}", c.id, c.cwd.display());
        }
        let wb1 = lido.chats.iter().find(|c| c.id == "wb1").unwrap();
        assert_eq!(wb1.cwd, b.join("site"));
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn chat_repetido_entre_raizes_ganha_sufixo_e_leva_as_referencias() {
        let base = pasta_de_teste("chat-repetido");
        let (a, b) = (base.join("A"), base.join("B"));
        let estado = |raiz: &Path| {
            json!({
                "chats": [
                    chat_antigo("c1", "site", "Coordenador", &raiz.join("site"), None, None),
                    chat_antigo("w1", "site", "Trabalhador", &raiz.join("site"), Some(1), Some("c1")),
                ],
                "tarefas": [tarefa_antiga(1, "site", "c1", "w1", None)],
                "proxima_tarefa": 2
            })
            .to_string()
        };
        // A pasta B é uma cópia da A, com o .vitral junto.
        raiz_antiga(&a, &["site"], &[("estado.json", estado(&a)), ("chats/c1.json", "[]".into())]);
        raiz_antiga(&b, &["site"], &[("estado.json", estado(&b)), ("chats/c1.json", "[]".into())]);
        let lidas = [ler_raiz(&a).unwrap(), ler_raiz(&b).unwrap()];
        let plano = planejar(&lidas, 1);
        let chats = plano.estado["chats"].as_array().unwrap();
        let ids: Vec<&str> = chats.iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["c1", "w1", "c1-2", "w1-2"]);
        assert_eq!(chat(&plano.estado, "w1-2")["pai"], "c1-2");
        assert_eq!(chat(&plano.estado, "w1-2")["tarefa"], 2);
        assert_eq!(tarefa(&plano.estado, 2)["coordenador"], "c1-2");
        assert_eq!(tarefa(&plano.estado, 2)["trabalhador"], "w1-2");
        let arquivos: Vec<&str> = plano.chats.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(arquivos, ["c1", "c1-2", "w1-2"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn config_com_chave_antiga_e_nova_juntas() {
        let base = pasta_de_teste("legada");
        let (a, b) = (base.join("A"), base.join("B"));
        let config = json!({
            "modelo_coordenador": "opus", "modelo_mestre": "sonnet", "modelo_trabalhador": "haiku",
            "permissao": "auto", "max_rodadas": 5, "de_uma_faixa": 7, "projetos": {}, "rotinas": []
        });
        let estado = json!({"chats": [], "tarefas": [], "proxima_tarefa": 1}).to_string();
        raiz_antiga(&a, &["loja"], &[("estado.json", estado.clone()), ("config.json", config.to_string())]);
        let manual = json!({"permissao": "manual"}).to_string();
        raiz_antiga(&b, &["blog"], &[("estado.json", estado), ("config.json", manual)]);
        let dados = Dados::em(&base.join("novo"));
        let relatorio = migrou(migrar(&dados, &[a.clone(), b.clone()], &proc_vazio(&base), 1, false, 1));
        let global = dados::ler_valor_sem_escrever(&dados.arquivo_config()).unwrap().unwrap();
        assert_eq!(
            global,
            json!({ "modelo_chats": "haiku", "permissao": "auto", "de_uma_faixa": 7 })
        );
        // A outra pasta escolhia outra permissão: fica a da mais recente, com aviso.
        let de_b = b.display().to_string();
        assert!(relatorio.avisos.iter().any(|a| a.contains(&de_b)), "{:?}", relatorio.avisos);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn queda_antes_do_estado_refaz_igual() {
        let base = pasta_de_teste("queda");
        let raiz = base.join("Projetos");
        let estado = json!({
            "chats": [
                chat_antigo("c1", "loja", "Coordenador", &raiz.join("loja"), None, None),
                chat_antigo("w1", "loja", "Trabalhador", &raiz.join("wt"), Some(1), Some("c1")),
            ],
            "tarefas": [tarefa_antiga(1, "loja", "c1", "w1", Some(raiz.join("wt")))],
            "proxima_tarefa": 2
        });
        let arquivos = [
            ("estado.json", estado.to_string()),
            ("config.json", json!({"permissao": "auto"}).to_string()),
            ("chats/c1.json", json!([{"Assistente": {"texto": "pronto"}}]).to_string()),
            ("chats/w1.json", json!([{"Assistente": {"texto": "feito"}}]).to_string()),
            ("notas/loja.md", "- usar Decimal para preços\n".into()),
        ];
        raiz_antiga(&raiz, &["loja"], &arquivos);
        let proc = proc_vazio(&base);
        let dados = Dados::em(&base.join("caiu"));
        let plano = planejar(&[ler_raiz(&raiz).unwrap()], 50);
        // A fonte desliga depois dos chats, das notas e da configuração, e no meio do estado.
        gravar_antes_do_estado(&dados, &plano).unwrap();
        fs::write(dados::com_sufixo(&dados.arquivo_estado(), ".tmp"), "{\"versao\": 2, \"cha").unwrap();
        assert!(dados.primeiro_uso());
        assert!(matches!(dados.ler_estado(), Ok(None)));

        migrou(migrar(&dados, &[raiz.clone()], &proc, 1, false, 50));
        let limpa = Dados::em(&base.join("limpa"));
        migrou(migrar(&limpa, &[raiz.clone()], &proc, 1, false, 50));
        assert_eq!(conteudo(&dados.pasta), conteudo(&limpa.pasta));
        assert_eq!(conteudo(&dados.config), conteudo(&limpa.config));
        // As notas ficam no .vitral antigo, e o relatório diz onde.
        assert!(dados::nao_existe(&dados.pasta.join("notas")));
        let registro: Registro = dados::ler_json_r(&dados.arquivo_migracao()).unwrap().unwrap();
        assert!(registro.relatorio.avisos.iter().any(|a| a.contains(".vitral/notas")), "{:?}", registro.relatorio.avisos);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn segunda_abertura_nao_migra() {
        let base = pasta_de_teste("segunda");
        let raiz = base.join("Projetos");
        let estado = json!({
            "chats": [chat_antigo("c1", "loja", "Coordenador", &raiz.join("loja"), None, None)],
            "tarefas": [], "proxima_tarefa": 1
        });
        raiz_antiga(&raiz, &["loja"], &[("estado.json", estado.to_string())]);
        let proc = proc_vazio(&base);
        let dados = Dados::em(&base.join("novo"));
        migrou(migrar(&dados, &[raiz.clone()], &proc, 1, false, 1));
        let antes = foto(&dados.pasta);
        assert!(matches!(migrar(&dados, &[raiz.clone()], &proc, 1, false, 2), Resultado::NaoPrecisa));
        assert!(matches!(aplicar(&dados, &planejar(&[ler_raiz(&raiz).unwrap()], 3)), Err(_)));
        assert_eq!(foto(&dados.pasta), antes);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn pasta_antiga_fica_intacta() {
        let base = pasta_de_teste("intacta");
        let raiz = base.join("Projetos");
        let estado = json!({
            "chats": [chat_antigo("c1", "loja", "Coordenador", &raiz.join("loja"), None, None)],
            "tarefas": [tarefa_antiga(1, "loja", "c1", "c1", None)], "proxima_tarefa": 2
        });
        let arquivos = [
            ("estado.json", estado.to_string()),
            ("config.json", "{\"projetos\": {\"loja\": {\"preparacao\": 3}}}".into()),
            ("chats/c1.json", "[{\"Assistente\": {\"texto\": \"oi\"}}]".into()),
            ("notas/loja.md", "- nota\n".into()),
        ];
        raiz_antiga(&raiz, &["loja"], &arquivos);
        let proc = proc_vazio(&base);
        let antes = foto(&raiz);
        let relatorio = migrou(migrar(&Dados::em(&base.join("novo")), &[raiz.clone()], &proc, 1, false, 1));
        assert_eq!(foto(&raiz), antes);
        // A configuração da pasta (estragada) não vem mais: nada a avisar dela. As notas ficam lá.
        assert_eq!(relatorio.avisos.len(), 1, "{:?}", relatorio.avisos);
        assert!(relatorio.avisos[0].starts_with("As decisões anotadas nos projetos ficaram em"), "{:?}", relatorio.avisos);

        // Com o estado antigo ilegível: nada vem, e nada é escrito nela, nem a cópia `.ilegivel`.
        let estragada = base.join("Estragada");
        raiz_antiga(&estragada, &["blog", "loja"], &[("estado.json", "{\"chats\": [".into())]);
        let antes = foto(&estragada);
        let dados = Dados::em(&base.join("outro"));
        let relatorio = migrou(migrar(&dados, &[estragada.clone()], &proc, 1, false, 1));
        assert_eq!(foto(&estragada), antes);
        assert!(!antes.keys().any(|c| c.to_string_lossy().contains("ilegivel")));
        assert_eq!(relatorio.chats, 0);
        assert!(relatorio.avisos[0].contains("ilegível; as conversas dele não vieram"), "{:?}", relatorio.avisos);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn estado_global_estragado_nao_migra_nem_muda_nada() {
        let base = pasta_de_teste("global-estragado");
        let raiz = base.join("Projetos");
        let estado = json!({"chats": [], "tarefas": [], "proxima_tarefa": 1});
        raiz_antiga(&raiz, &["loja"], &[("estado.json", estado.to_string())]);
        let dados = Dados::em(&base.join("novo"));
        fs::create_dir_all(&dados.pasta).unwrap();
        fs::write(dados.arquivo_estado(), "{\"chats\": [").unwrap();
        fs::write(dados::com_sufixo(&dados.arquivo_estado(), ".anterior"), "[1,").unwrap();
        fs::create_dir_all(&dados.config).unwrap();
        fs::write(dados.arquivo_recentes(), json!([raiz]).to_string()).unwrap();
        let (antes, antes_config) = (foto(&dados.pasta), foto(&dados.config));

        let raizes = raizes_antigas(&dados, None);
        assert_eq!(raizes, [raiz.clone()]);
        assert!(matches!(migrar(&dados, &raizes, &proc_vazio(&base), 1, false, 1), Resultado::NaoPrecisa));
        assert!(matches!(
            dados.ler_estado(),
            Err(ErroCarga::Ilegivel { arquivo, .. }) if arquivo == dados.arquivo_estado()
        ));
        assert_eq!(foto(&dados.pasta), antes);
        assert_eq!(foto(&dados.config), antes_config);
        let _ = fs::remove_dir_all(&base);
    }

    fn processo_falso(proc: &Path, pid: u32, comm: &str, environ: Option<&[u8]>) {
        let pasta = proc.join(pid.to_string());
        fs::create_dir_all(&pasta).unwrap();
        fs::write(pasta.join("comm"), format!("{comm}\n")).unwrap();
        if let Some(environ) = environ {
            fs::write(pasta.join("environ"), environ).unwrap();
        }
    }

    #[test]
    fn vitral_antigo_aberto_adia_sem_gravar() {
        let base = pasta_de_teste("vivo");
        let raiz = base.join("Projetos");
        let estado = json!({"chats": [], "tarefas": [], "proxima_tarefa": 1});
        raiz_antiga(&raiz, &["loja"], &[("estado.json", estado.to_string())]);
        let proc = proc_vazio(&base);
        processo_falso(&proc, 4242, "vitral", Some(b"HOME=/home/x\0PATH=/usr/bin\0"));
        processo_falso(&proc, 77, "vitral", Some(b"HOME=/home/x\0"));
        processo_falso(&proc, 99, "bash", Some(b""));
        fs::create_dir_all(proc.join("self")).unwrap();
        let dados = Dados::em(&base.join("novo"));
        match migrar(&dados, &[raiz.clone()], &proc, 77, false, 1) {
            Resultado::Adiada { raizes, pids } => {
                assert_eq!(pids, [4242]);
                assert_eq!(raizes, [raiz.clone()]);
            }
            outro => panic!("não adiou: {outro:?}"),
        }
        assert!(dados::nao_existe(&dados.pasta) && dados::nao_existe(&dados.config));
        // Sem o environ legível, o processo conta; um filho deste (fork antes do exec), não.
        processo_falso(&proc, 5000, "vitral", None);
        processo_falso(&proc, 6000, "vitral", Some(b"HOME=/x\0"));
        fs::write(proc.join("6000").join("stat"), "6000 (vitral) R 77 6000 6000 0 -1").unwrap();
        assert_eq!(vitral_antigo_vivo(&proc, 77, false), [4242, 5000]);
        // Dentro da bancada, o Vitral aberto fora dela não conta (o sem environ legível, sim).
        assert_eq!(vitral_antigo_vivo(&proc, 77, true), [5000]);
        fs::remove_dir_all(proc.join("5000")).unwrap();
        // O da bancada não é o antigo.
        let meus_dados = std::env::var("XDG_DATA_HOME").unwrap_or_default();
        let environ = format!("HOME=/x\0DENTRO_DA_BANCADA=1\0XDG_DATA_HOME={meus_dados}\0");
        fs::write(proc.join("4242").join("environ"), environ).unwrap();
        assert_eq!(vitral_antigo_vivo(&proc, 77, true), [4242]);
        // O de outra bancada aberta ao mesmo tempo (outro agente, outra pasta de dados) também não.
        processo_falso(&proc, 7000, "vitral", Some(b"DENTRO_DA_BANCADA=1\0XDG_DATA_HOME=/outra/bancada/data\0"));
        assert_eq!(vitral_antigo_vivo(&proc, 77, true), [4242]);
        migrou(migrar(&dados, &[raiz], &proc, 77, false, 1));
        let _ = fs::remove_dir_all(&base);
    }

    // No Windows o link simbólico pede o Modo de Desenvolvedor.
    #[cfg(unix)]
    #[test]
    fn link_simbolico_e_pasta_oculta_ficam_de_fora() {
        let base = pasta_de_teste("links");
        let raiz = base.join("Projetos");
        fs::create_dir_all(base.join("fora")).unwrap();
        let estado = json!({"chats": [], "tarefas": [], "proxima_tarefa": 1}).to_string();
        raiz_antiga(&raiz, &["b", "A", ".oculta"], &[("estado.json", estado)]);
        std::os::unix::fs::symlink(base.join("fora"), raiz.join("atalho")).unwrap();
        fs::write(raiz.join("solto.txt"), "x").unwrap();
        let nomes: Vec<String> = subpastas_antigas(&raiz).unwrap().into_iter().map(|(n, _)| n).collect();
        assert_eq!(nomes, ["A", "b"]);
        // As mesmas subpastas dão a pasta de origem das tarefas.
        let caminhos: Vec<String> = ler_raiz(&raiz).unwrap().caminhos.into_keys().collect();
        assert_eq!(caminhos, ["A", "b"]);
        let _ = fs::remove_dir_all(&base);
    }

    #[test]
    fn argumento_vem_antes_dos_recentes_e_sem_raizes_nao_grava() {
        let base = pasta_de_teste("argumento");
        let (a, b) = (base.join("A"), base.join("B"));
        let estado = json!({"chats": [], "tarefas": [], "proxima_tarefa": 1}).to_string();
        raiz_antiga(&a, &[], &[("estado.json", estado)]);
        raiz_antiga(&b, &[], &[("config.json", "{}".into())]);
        let dados = Dados::em(&base.join("novo"));
        fs::create_dir_all(&dados.config).unwrap();
        fs::write(dados.arquivo_recentes(), json!([a, b]).to_string()).unwrap();
        assert_eq!(raizes_antigas(&dados, Some(b.join("..").join("B").as_path())), [b.clone(), a.clone()]);
        assert_eq!(raizes_antigas(&dados, Some(base.join("nao-existe").as_path())), [a.clone(), b.clone()]);
        assert!(matches!(migrar(&dados, &[], &proc_vazio(&base), 1, false, 1), Resultado::SemRaizes));
        assert!(dados::nao_existe(&dados.pasta));
        let _ = fs::remove_dir_all(&base);
    }

    /// Conferência à mão numa cópia dos dados de um usuário, nunca nos de verdade:
    /// `VITRAL_COPIA_DA_MIGRACAO=<casa> compilar.sh test migrar_uma_copia -- --ignored --nocapture`,
    /// com `<casa>/.config/vitral/recentes.json` apontando para as cópias das pastas antigas (e HOME e
    /// os XDG em `<casa>`). Migra para `<casa>/.local/share/vitral`, abre como o Motor abriria (carregar
    /// e normalizar, sem gravar a normalização) e escreve o que virou o quê no stdout.
    #[test]
    #[ignore]
    fn migrar_uma_copia() {
        let Some(casa) = std::env::var_os("VITRAL_COPIA_DA_MIGRACAO").map(PathBuf::from) else {
            return;
        };
        let mut dados = Dados::em(&casa);
        dados.pasta = casa.join(".local/share/vitral");
        dados.config = casa.join(".config/vitral");
        dados.cache = casa.join(".cache/vitral");
        // Sem processos: o Vitral antigo aberto de verdade adiaria a migração (com
        // `VITRAL_COPIA_PROC_REAL=1`, é o que se confere, sem nada gravado).
        let real = std::env::var_os("VITRAL_COPIA_PROC_REAL").is_some();
        let proc = if real { PathBuf::from("/proc") } else { casa.join("proc-vazio") };
        fs::create_dir_all(&proc).unwrap();
        let raizes = raizes_antigas(&dados, None);
        println!("raízes: {raizes:?}");
        let quando = crate::modelo::agora();
        match migrar(&dados, &raizes, &proc, std::process::id(), false, quando) {
            Resultado::Migrou(r) => println!("relatório: {}\n{r:?}", r.texto(Some(&casa))),
            Resultado::Adiada { raizes, pids } if real => {
                println!("adiada: o Vitral antigo está aberto (pids {pids:?}); raízes {raizes:?}");
                assert!(dados.primeiro_uso() && !dados.pasta.exists(), "a migração adiada gravou algo");
                return;
            }
            outro => panic!("não migrou: {outro:?}"),
        }
        let segunda = migrar(&dados, &raizes, &proc, std::process::id(), false, quando);
        assert!(matches!(segunda, Resultado::NaoPrecisa), "a segunda abertura migrou: {segunda:?}");
        let mut carregado = dados.carregar().expect("os dados migrados abrem");
        let n = crate::dados::normalizar(&mut carregado.estado, &dados, &crate::dados::NoDisco, quando);
        println!("avisos ao carregar: {:?}\nnormalização: {n:?}", carregado.avisos);
        let e = &carregado.estado;
        println!("versão {} · formato {}", e.versao, crate::dados::formato_em(&dados.pasta));
        for c in &e.chats {
            println!(
                "chat {} \"{}\" criado_por {:?} itens {} sessão_criada {} arquivado {} cwd {} (existe: {})",
                c.id, c.titulo, c.criado_por, c.itens.len(), c.sessao_criada, c.arquivado, c.cwd.display(), c.cwd.is_dir()
            );
        }
    }

    #[test]
    fn texto_do_relatorio() {
        let mut relatorio = Relatorio { raizes: vec!["/home/x/Projetos".into()], chats: 2, ..Default::default() };
        let casa = Some(Path::new("/home/x"));
        assert_eq!(relatorio.texto(casa), "Trouxe 2 conversas do Vitral antigo de ~/Projetos.");
        relatorio.raizes.push("/srv/outra".into());
        relatorio.chats = 1;
        relatorio.renumeradas = vec![(1, 5)];
        assert_eq!(
            relatorio.texto(casa),
            "Trouxe 1 conversa do Vitral antigo de ~/Projetos e /srv/outra. Um chat mudou de número (o aviso está nele)."
        );
        relatorio.renumeradas.clear();
        relatorio.avisos = vec!["O estado antigo de /home/x/Velho está ilegível; as conversas dele não vieram.".into()];
        assert!(relatorio.texto(casa).ends_with("/srv/outra. O estado antigo de ~/Velho está ilegível; as conversas dele não vieram."));
        relatorio.chats = 0;
        assert!(relatorio.texto(casa).starts_with("Não havia conversas do Vitral antigo em ~/Projetos e /srv/outra."));
    }
}
