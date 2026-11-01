//! Instalação gerenciada no Windows (`distribuicao.md`, seção 3.8): uma pasta por versão em
//! `%LOCALAPPDATA%\Programs\Vitral`, o `versoes.txt` (que o lançador lê) trocado de uma vez, e o
//! lançador na raiz, para onde apontam o atalho do Menu Iniciar e o pino da barra. O `.exe` em uso
//! não pode ser sobrescrito nem apagado: por isso cada versão tem a pasta dela, e a limpeza deixa
//! para a próxima vez a pasta que estiver em uso. Qualquer passo pode ser cortado por uma queda de
//! energia sem deixar o app quebrado: o `versoes.txt` sempre aponta para uma versão completa (e,
//! sem ele, o lançador abre a maior que estiver inteira) e o que sobrar pela metade sai na próxima
//! abertura.
//!
//! ```text
//! Vitral.exe   versoes.txt   app-0.2.0\vitral.exe   app-0.3.0\vitral.exe
//! estado.json  .baixando\    .novo-*\   .lixo-*\    .trava
//! ```
//!
//! É o layout que o instalador cria (`pacote/windows/vitral.iss`, que também apaga tudo isto ao
//! desinstalar). O módulo compila em todo sistema (o que é só do Windows, o `MoveFileExW` e o
//! espaço livre, fica atrás de `cfg`): os testes do layout rodam também no Linux.

#![cfg_attr(not(windows), allow(dead_code, unused_imports, reason = "fora do Windows, só os testes usam este layout"))]

use std::{
    fmt, fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use anyhow::{Result, anyhow};
use sha2::{Digest as _, Sha256};

pub use super::estado_app::{Abertura, Confirmar, EstadoApp, Volta};
use super::pacote::{self, LIMITE_EXTRAIDO, PRAZO_CONFERENCIA, Resposta, gravar_atomico, trocar_por_cima};

/// O que o lançador lê e escolhe, o mesmo arquivo da crate `lancador/`: o formato do `versoes.txt`
/// e a ordem das versões são um só para o instalador, o lançador e o atualizador.
#[path = "../../lancador/src/escolha.rs"]
#[allow(dead_code, reason = "o lançador usa a escolha inteira; daqui, a leitura e os testes")]
mod escolha;

use escolha::{BINARIO, VERSOES, Versoes};

/// O lançador dentro do pacote (e de cada `app-<versão>`) e na raiz da instalação.
const LANCADOR_NO_PACOTE: &str = "vitral-lancador.exe";
const LANCADOR: &str = "Vitral.exe";

/// Onde o Vitral gerenciado mora: `<dados>\Programs\Vitral`, com `dados` o `%LOCALAPPDATA%` (os
/// testes passam uma pasta temporária). A pasta é a do instalador e não muda nunca: o AppId do Inno,
/// o atalho e o desinstalador dependem dela.
#[derive(Debug, Clone)]
pub struct Raiz {
    pub dados: PathBuf,
    /// Sem uso no Windows (o Linux põe o link em `~/.local/bin`).
    #[allow(dead_code, reason = "a mesma Raiz do linux.rs")]
    pub bin: Option<PathBuf>,
}

impl Raiz {
    pub fn do_usuario() -> Option<Raiz> {
        Some(Raiz { dados: dirs::data_local_dir()?, bin: None })
    }

    pub fn app(&self) -> PathBuf {
        self.dados.join("Programs").join("Vitral")
    }

    /// O lançador da raiz: o que o atalho abre e para onde o Reiniciar volta (ele abre a versão do
    /// `versoes.txt`, a nova depois de uma instalação).
    pub fn binario_atual(&self) -> PathBuf {
        self.app().join(LANCADOR)
    }
}

/// Os passos da instalação, na ordem. Os testes cortam depois de cada um.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passo {
    Aberto,
    Conferido,
    NoLugar,
    Lancador,
    Versoes,
    Estado,
    Limpo,
}

/// De onde vem a versão nova. No Windows, só do pacote do release (o `--instalar` é do Linux; a
/// primeira instalação é a do instalador).
pub enum Origem<'a> {
    Pacote { arquivo: &'a Path, prefixo: &'a str },
    #[allow(dead_code, reason = "a mesma Origem do linux.rs, que instala o próprio binário")]
    Binario(&'a Path),
}

#[derive(Debug)]
pub enum Falha {
    /// O pacote não se abre ou tem o que não devia.
    Pacote(anyhow::Error),
    /// O `vitral.exe` novo não respondeu ao `--versao --json`.
    Binario(anyhow::Error),
    /// Disco: permissão, espaço, rename, pasta em uso.
    Disco(anyhow::Error),
    /// Queda simulada depois de um passo (só os testes pedem).
    Queda(Passo),
}

impl fmt::Display for Falha {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Falha::Pacote(erro) => write!(f, "o pacote não serve: {erro:#}"),
            Falha::Binario(erro) => write!(f, "a versão nova não respondeu ao teste: {erro:#}"),
            Falha::Disco(erro) => write!(f, "{erro:#}"),
            Falha::Queda(passo) => write!(f, "queda depois de {passo:?}"),
        }
    }
}

impl std::error::Error for Falha {}

fn disco(erro: impl Into<anyhow::Error>) -> Falha {
    Falha::Disco(erro.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instalada {
    pub versao: String,
    pub formato_dados: u32,
}

/// A trava de `.trava`: dois Vitrals não instalam (nem limpam, nem contam aberturas) juntos.
pub struct Trava(#[allow(dead_code, reason = "segura a trava enquanto existe")] fs::File);

/// A trava, ou None se outro já a tem. Antes de dizer que está ocupada, espera um instante, como no
/// Linux: lá (onde estes testes também rodam), um processo que outra thread acabou de abrir herda a
/// trava até o exec.
pub fn travar(app: &Path) -> io::Result<Option<Trava>> {
    fs::create_dir_all(app)?;
    let arquivo = fs::OpenOptions::new().create(true).truncate(false).write(true).open(app.join(".trava"))?;
    let limite = Instant::now() + Duration::from_millis(500);
    loop {
        match arquivo.try_lock() {
            Ok(()) => return Ok(Some(Trava(arquivo))),
            Err(fs::TryLockError::WouldBlock) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(10)),
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(erro)) => return Err(erro),
        }
    }
}

/// Como o binário novo é conferido (o `--versao --json` de verdade; os testes leem a resposta).
type Conferir = fn(&Path, &str, Duration) -> Result<Resposta>;

/// Instala `versao` em `app-<versao>\` e aponta o `versoes.txt` para ela; a que estava rodando (ou
/// a que era a atual) vira a anterior. Ordem: abre numa `.novo-*`, testa o `vitral.exe`, põe no
/// lugar com um rename, troca o lançador da raiz se o do pacote é outro, grava o `versoes.txt` (de
/// uma vez), o `estado.json`, e tira as versões que não são a atual, a anterior nem a que roda.
pub fn instalar(app: &Path, origem: Origem, versao: &str, rodando: Option<&str>, trava: &Trava) -> Result<Instalada, Falha> {
    instalar_com(app, origem, versao, rodando, trava, None, pacote::conferir_binario)
}

fn instalar_com(
    app: &Path,
    origem: Origem,
    versao: &str,
    rodando: Option<&str>,
    _trava: &Trava,
    cair: Option<Passo>,
    conferir: Conferir,
) -> Result<Instalada, Falha> {
    let ponto = |passo: Passo| if cair == Some(passo) { Err(Falha::Queda(passo)) } else { Ok(()) };
    if !nome_de_versao(versao) {
        return Err(Falha::Pacote(anyhow!("versão inválida: {versao}")));
    }
    let Origem::Pacote { arquivo, prefixo } = origem else {
        return Err(Falha::Pacote(anyhow!("no Windows, a versão nova vem do pacote do release")));
    };
    fs::create_dir_all(app).map_err(disco)?;
    let novo = app.join(format!(".novo-{versao}-{}", aleatorio()));
    fs::create_dir(&novo).map_err(disco)?;
    let resposta = match preparar(&novo, arquivo, prefixo, versao, ponto, conferir) {
        Ok(resposta) => resposta,
        Err(Falha::Queda(passo)) => return Err(Falha::Queda(passo)),
        Err(falha) => {
            let _ = fs::remove_dir_all(&novo);
            return Err(falha);
        }
    };

    let antes = ler_versoes(app);
    por_no_lugar(app, &novo, versao, conferir).map_err(disco)?;
    ponto(Passo::NoLugar)?;
    // O lançador de antes abre a versão nova do mesmo jeito: se ele estiver aberto agora (e não
    // puder ser trocado), a próxima abertura troca.
    if let Err(erro) = trocar_lancador(app, versao) {
        eprintln!("vitral: o lançador fica para a próxima abertura: {erro}");
    }
    ponto(Passo::Lancador)?;

    let anterior = [rodando.map(str::to_string), antes.atual, antes.anterior]
        .into_iter()
        .flatten()
        .find(|v| v != versao && tem_binario(&binario_da_versao(app, v)));
    gravar_versoes(app, versao, anterior.as_deref()).map_err(disco)?;
    ponto(Passo::Versoes)?;

    let mut estado = EstadoApp::ler(app);
    estado.formatos.insert(versao.to_string(), resposta.formato_dados);
    estado.formatos.retain(|v, _| v == versao || Some(v) == anterior.as_ref());
    estado.confirmar = Some(Confirmar { versao: versao.to_string(), aberturas: Vec::new() });
    estado.recusadas.retain(|v| v != versao);
    estado.gravar(app).map_err(disco)?;
    ponto(Passo::Estado)?;

    tirar_versoes(app, &[Some(versao), anterior.as_deref(), rodando]);
    ponto(Passo::Limpo)?;
    Ok(Instalada { versao: versao.to_string(), formato_dados: resposta.formato_dados })
}

/// O pacote aberto em `novo` e o `vitral.exe` dele respondendo.
fn preparar(
    novo: &Path,
    arquivo: &Path,
    prefixo: &str,
    versao: &str,
    ponto: impl Fn(Passo) -> Result<(), Falha>,
    conferir: Conferir,
) -> Result<Resposta, Falha> {
    pacote::extrair(arquivo, novo, prefixo, BINARIO, LIMITE_EXTRAIDO).map_err(Falha::Pacote)?;
    ponto(Passo::Aberto)?;
    let resposta = conferir(&novo.join(BINARIO), versao, PRAZO_CONFERENCIA).map_err(Falha::Binario)?;
    ponto(Passo::Conferido)?;
    Ok(resposta)
}

/// `novo` vira `app-<versao>` de uma vez. Se ela já existe (a mesma versão de novo), fica a que já
/// está lá quando responde (pode ser a que está rodando, que não sai do lugar); senão ela sai antes.
fn por_no_lugar(app: &Path, novo: &Path, versao: &str, conferir: Conferir) -> io::Result<()> {
    let alvo = pasta_da_versao(app, versao);
    if fs::symlink_metadata(&alvo).is_ok() {
        if conferir(&alvo.join(BINARIO), versao, PRAZO_CONFERENCIA).is_ok() {
            jogar_fora(app, novo, versao);
            return Ok(());
        }
        if !jogar_fora(app, &alvo, versao) {
            return Err(io::Error::other(format!("a pasta {} está em uso", alvo.display())));
        }
    }
    renomear_pasta(novo, &alvo)
}

/// O rename de uma pasta recém-escrita. No Windows, o antivírus lê o `.exe` novo e segura a pasta
/// por um instante ("Acesso negado"): tenta por até 2 s.
fn renomear_pasta(de: &Path, para: &Path) -> io::Result<()> {
    let mut tentativas = if cfg!(windows) { 20 } else { 1 };
    loop {
        match fs::rename(de, para) {
            Err(erro)
                if tentativas > 1
                    && (erro.kind() == io::ErrorKind::PermissionDenied || matches!(erro.raw_os_error(), Some(32 | 33))) =>
            {
                tentativas -= 1;
                std::thread::sleep(Duration::from_millis(100));
            }
            resultado => return resultado,
        }
    }
}

/// Tira a pasta do lugar com um rename para `.lixo-*` e apaga. A pasta de um `.exe` aberto não se
/// renomeia (ou não se apaga inteira): fica para a próxima limpeza, sem nunca ficar uma
/// `app-<versão>` pela metade. Devolve se ela saiu do lugar.
fn jogar_fora(app: &Path, pasta: &Path, versao: &str) -> bool {
    if fs::symlink_metadata(pasta).is_err() {
        return true;
    }
    let lixo = app.join(format!(".lixo-{versao}-{}", aleatorio()));
    if fs::rename(pasta, &lixo).is_err() {
        return false;
    }
    let _ = fs::remove_dir_all(&lixo);
    true
}

/// O lançador da raiz passa a ser o de `versao`, se for outro (pela soma: com o `/Brepro` ele só
/// muda quando o código muda) ou se faltar. Copia para `Vitral.exe.novo` e troca de uma vez; com o
/// lançador aberto naquele instante, falha e a próxima abertura tenta de novo. Devolve se trocou.
fn trocar_lancador(app: &Path, versao: &str) -> io::Result<bool> {
    let novo = pasta_da_versao(app, versao).join(LANCADOR_NO_PACOTE);
    if !novo.is_file() {
        return Ok(false);
    }
    let raiz = app.join(LANCADOR);
    if soma(&raiz).ok() == Some(soma(&novo)?) {
        return Ok(false);
    }
    let temporario = app.join(format!("{LANCADOR}.novo"));
    fs::copy(&novo, &temporario)?;
    fs::OpenOptions::new().write(true).open(&temporario)?.sync_all()?;
    if let Err(erro) = trocar_por_cima(&temporario, &raiz) {
        let _ = fs::remove_file(&temporario);
        return Err(erro);
    }
    Ok(true)
}

fn soma(caminho: &Path) -> io::Result<[u8; 32]> {
    let mut arquivo = fs::File::open(caminho)?;
    let mut soma = Sha256::new();
    let mut bloco = vec![0u8; 64 * 1024];
    loop {
        match arquivo.read(&mut bloco)? {
            0 => return Ok(soma.finalize().into()),
            lidos => soma.update(&bloco[..lidos]),
        }
    }
}

/// O `versoes.txt` com a `atual` e a `anterior`, no mesmo texto que o instalador grava, escrito ao
/// lado e trocado de uma vez (`MoveFileExW` com `MOVEFILE_WRITE_THROUGH`): o lançador lê o velho ou
/// o novo, nunca a metade.
fn gravar_versoes(app: &Path, atual: &str, anterior: Option<&str>) -> io::Result<()> {
    let mut texto = format!("atual={atual}\r\n");
    if let Some(anterior) = anterior.filter(|a| *a != atual) {
        texto.push_str(&format!("anterior={anterior}\r\n"));
    }
    gravar_atomico(&app.join(VERSOES), texto.as_bytes(), 0o644)
}

/// O `versoes.txt` lido como o lançador lê, só com nomes que são versão.
fn ler_versoes(app: &Path) -> Versoes {
    let versoes = Versoes::ler(&fs::read_to_string(app.join(VERSOES)).unwrap_or_default());
    let valida = |v: Option<String>| v.filter(|v| nome_de_versao(v));
    Versoes { atual: valida(versoes.atual), anterior: valida(versoes.anterior) }
}

/// A pasta de `versao` (`app-<versão>`).
pub fn pasta_da_versao(app: &Path, versao: &str) -> PathBuf {
    app.join(format!("app-{versao}"))
}

/// O binário de `versao` (`app-<versão>\vitral.exe`).
pub fn binario_da_versao(app: &Path, versao: &str) -> PathBuf {
    escolha::binario(app, versao)
}

/// Um `vitral.exe` vazio é o que sobra de uma queda de energia no meio: não serve.
fn tem_binario(exe: &Path) -> bool {
    fs::metadata(exe).is_ok_and(|m| m.is_file() && m.len() > 0)
}

/// A versão que o `versoes.txt` diz para `nome` (`atual` ou `anterior`), como os links do Linux.
pub fn versao_do_link(app: &Path, nome: &str) -> Option<String> {
    let versoes = ler_versoes(app);
    match nome {
        "atual" => versoes.atual,
        "anterior" => versoes.anterior,
        _ => None,
    }
}

pub fn versao_atual(app: &Path) -> Option<String> {
    versao_do_link(app, "atual")
}

/// A volta (da saúde ou das Configurações): a `atual` passa para `para`, e a anterior, que era ela,
/// sai do `versoes.txt`.
pub fn voltar_para(app: &Path, para: &str) -> io::Result<()> {
    let anterior = versao_do_link(app, "anterior").filter(|a| a != para);
    gravar_versoes(app, para, anterior.as_deref())
}

/// A versão gerenciada que está rodando (None se este `.exe` não é de `app-<versão>\`).
pub fn versao_rodando(app: &Path) -> Option<String> {
    versao_do_executavel(app, &std::env::current_exe().ok()?)
}

fn versao_do_executavel(app: &Path, executavel: &Path) -> Option<String> {
    let executavel = dunce::canonicalize(executavel).ok()?;
    let pasta = executavel.parent()?;
    if !executavel.file_name()?.to_str()?.eq_ignore_ascii_case(BINARIO) || pasta.parent()? != dunce::canonicalize(app).ok()? {
        return None;
    }
    let versao = pasta.file_name()?.to_str()?.strip_prefix("app-")?;
    nome_de_versao(versao).then(|| versao.to_string())
}

/// Um nome de versão: semver, sem metadado de build (e que o lançador também lê).
fn nome_de_versao(texto: &str) -> bool {
    semver::Version::parse(texto).is_ok_and(|v| v.build.is_empty()) && escolha::Versao::ler(texto).is_some()
}

fn aleatorio() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// As `app-<versão>` que não estão em `ficar` vão embora; as que estão em uso ficam para a próxima.
fn tirar_versoes(app: &Path, ficar: &[Option<&str>]) {
    let Ok(entradas) = fs::read_dir(app) else { return };
    for entrada in entradas.flatten() {
        let nome = entrada.file_name();
        let Some(versao) = nome.to_str().and_then(|n| n.strip_prefix("app-")) else { continue };
        if nome_de_versao(versao) && !ficar.contains(&Some(versao)) && entrada.file_type().is_ok_and(|t| t.is_dir()) {
            jogar_fora(app, &entrada.path(), versao);
        }
    }
}

/// O que sobrou de uma instalação ou de um download cortados no meio (`.novo-*`, `.lixo-*`, os
/// temporários, o `.parte`) e as versões que uma limpeza anterior deixou por estarem em uso. Só com
/// a trava, e as versões só com um `versoes.txt` que aponta para uma versão inteira: sem ele, o
/// lançador escolhe entre as pastas, e nenhuma sai.
pub fn limpar_restos(app: &Path, _trava: &Trava) {
    if let Ok(entradas) = fs::read_dir(app) {
        for entrada in entradas.flatten() {
            let nome = entrada.file_name();
            let nome = nome.to_string_lossy();
            let caminho = entrada.path();
            if nome.starts_with(".novo-") || nome.starts_with(".lixo-") {
                let _ = fs::remove_dir_all(&caminho);
            } else if (nome.starts_with('.') && nome.ends_with(".tmp")) || nome == format!("{LANCADOR}.novo") {
                let _ = fs::remove_file(&caminho);
            }
        }
    }
    if let Ok(entradas) = fs::read_dir(app.join(".baixando")) {
        for entrada in entradas.flatten() {
            if entrada.file_name().to_string_lossy().ends_with(".parte") {
                let _ = fs::remove_file(entrada.path());
            }
        }
    }
    let versoes = ler_versoes(app);
    if versoes.atual.as_deref().is_some_and(|v| tem_binario(&binario_da_versao(app, v))) {
        let rodando = versao_rodando(app);
        tirar_versoes(app, &[versoes.atual.as_deref(), versoes.anterior.as_deref(), rodando.as_deref()]);
    }
}

/// Espaço livre no disco de `pasta`, em bytes.
#[cfg(windows)]
pub fn espaco_livre(pasta: &Path) -> io::Result<u64> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let caminho: Vec<u16> = pasta.as_os_str().encode_wide().chain([0]).collect();
    let mut livre = 0u64;
    // SAFETY: caminho terminado em zero; os dois totais que não interessam vão nulos.
    if unsafe { GetDiskFreeSpaceExW(caminho.as_ptr(), &mut livre, std::ptr::null_mut(), std::ptr::null_mut()) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(livre)
}

#[cfg(not(windows))]
pub fn espaco_livre(_pasta: &Path) -> io::Result<u64> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "o espaço livre deste layout é medido no Windows"))
}

/// A cada abertura da versão atual (e depois de cada instalação): o lançador da raiz é o da versão
/// atual (o que não pôde ser trocado na instalação, por estar aberto, troca agora). O atalho, o
/// ícone e o desinstalador são do instalador. Devolve se trocou.
pub fn integrar(raiz: &Raiz, _caches: bool) -> io::Result<bool> {
    let app = raiz.app();
    match versao_atual(&app) {
        Some(versao) => trocar_lancador(&app, &versao),
        None => Ok(false),
    }
}

/// `vitral --instalar` é do Linux: no Windows, o instalador do release.
pub fn instalar_por_comando() -> i32 {
    eprintln!(
        "Não instalei o Vitral: no Windows, a instalação é pelo vitral-<versão>-windows-x86_64-instalador.exe do release; \
         depois disso ele se atualiza sozinho."
    );
    1
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::atualizacao::pacote::testes::{Entrada, pasta_de_teste, tar_gz};
    use anyhow::{Context as _, ensure};
    use std::collections::BTreeMap;

    const LANCADOR_A: &[u8] = b"MZ lancador A";
    const LANCADOR_B: &[u8] = b"MZ lancador B";

    /// O `vitral.exe` de mentira guarda o que o de verdade responde ao `--versao --json`, e o
    /// conferir dos testes lê em vez de rodar: um `.exe` de mentira não roda no Windows. O
    /// `--versao --json` de verdade roda nos testes do `pacote.rs` (Linux) e no runner do Windows.
    fn exe_falso(versao: &str) -> Vec<u8> {
        format!(r#"{{"versao":"{versao}","formato_dados":1,"canal":"estavel","commit":"","plataforma":"windows-x86_64"}}"#).into_bytes()
    }

    fn conferir_falso(binario: &Path, versao: &str, _prazo: Duration) -> Result<Resposta> {
        let bytes = fs::read(binario).with_context(|| format!("não abriu {}", binario.display()))?;
        let resposta: Resposta = serde_json::from_slice(&bytes).context("respondeu outra coisa")?;
        ensure!(resposta.versao == versao, "diz ser a {} em vez da {versao}", resposta.versao);
        Ok(resposta)
    }

    fn prefixo(versao: &str) -> String {
        format!("vitral-{versao}-windows-x86_64")
    }

    /// O `.tar.gz` do release do Windows: o `vitral.exe`, o lançador e os textos.
    fn pacote_em(pasta: &Path, versao: &str, lancador: &[u8]) -> PathBuf {
        let prefixo = prefixo(versao);
        let (exe, lanc, novidades) = (format!("{prefixo}/vitral.exe"), format!("{prefixo}/vitral-lancador.exe"), format!("{prefixo}/NOVIDADES.md"));
        let bytes = tar_gz(&[
            Entrada::Arquivo(&exe, &exe_falso(versao)),
            Entrada::Arquivo(&lanc, lancador),
            Entrada::Arquivo(&novidades, b"## novidades\n"),
        ]);
        let caminho = pasta.join(format!("{prefixo}.tar.gz"));
        fs::write(&caminho, bytes).unwrap();
        caminho
    }

    fn instalar_pacote(app: &Path, pacote: &Path, versao: &str, rodando: Option<&str>, cair: Option<Passo>) -> Result<Instalada, Falha> {
        let trava = travar(app).unwrap().unwrap();
        let prefixo = prefixo(versao);
        instalar_com(app, Origem::Pacote { arquivo: pacote, prefixo: &prefixo }, versao, rodando, &trava, cair, conferir_falso)
    }

    /// Um layout com `versao` já instalada e o lançador A na raiz.
    fn layout_com(versao: &str) -> (PathBuf, PathBuf) {
        let pasta = pasta_de_teste("windows");
        let app = Raiz { dados: pasta.join("Local"), bin: None }.app();
        let pacote = pacote_em(&pasta, versao, LANCADOR_A);
        instalar_pacote(&app, &pacote, versao, None, None).unwrap();
        (pasta, app)
    }

    fn nomes(pasta: &Path) -> Vec<String> {
        let mut nomes: Vec<_> = fs::read_dir(pasta).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        nomes.sort();
        nomes
    }

    /// A versão que o lançador abre agora, inteira e respondendo como ela mesma.
    fn abre(app: &Path) -> String {
        let exe = escolha::escolher(app).unwrap_or_else(|| panic!("o lançador não acha nada em {:?}", nomes(app)));
        let versao = exe.parent().unwrap().file_name().unwrap().to_str().unwrap().strip_prefix("app-").unwrap().to_string();
        conferir_falso(&exe, &versao, PRAZO_CONFERENCIA).unwrap_or_else(|e| panic!("a {versao} está quebrada: {e:#}"));
        assert!(pasta_da_versao(app, &versao).join("NOVIDADES.md").is_file(), "a {versao} veio pela metade");
        versao
    }

    fn versoes(app: &Path) -> String {
        fs::read_to_string(app.join(VERSOES)).unwrap()
    }

    #[test]
    fn instala_aponta_e_guarda_a_anterior() {
        let (pasta, app) = layout_com("0.8.0");
        assert_eq!(app, pasta.join("Local").join("Programs").join("Vitral"));
        assert_eq!(versoes(&app), "atual=0.8.0\r\n", "o mesmo texto do instalador");
        assert_eq!(fs::read(app.join("Vitral.exe")).unwrap(), LANCADOR_A, "o lançador do pacote na raiz");
        assert_eq!(abre(&app), "0.8.0");

        let instalada = instalar_pacote(&app, &pacote_em(&pasta, "0.9.0", LANCADOR_A), "0.9.0", Some("0.8.0"), None).unwrap();
        assert_eq!(instalada, Instalada { versao: "0.9.0".into(), formato_dados: 1 });
        assert_eq!(versoes(&app), "atual=0.9.0\r\nanterior=0.8.0\r\n");
        assert_eq!(abre(&app), "0.9.0");
        assert_eq!(versao_atual(&app).as_deref(), Some("0.9.0"));
        assert_eq!(versao_do_link(&app, "anterior").as_deref(), Some("0.8.0"));
        let estado = EstadoApp::ler(&app);
        assert_eq!(estado.confirmar.as_ref().map(|c| c.versao.as_str()), Some("0.9.0"));
        assert_eq!(estado.formatos, BTreeMap::from([("0.8.0".into(), 1), ("0.9.0".into(), 1)]));

        // Na terceira, a mais velha sai; a que rodava fica como anterior.
        instalar_pacote(&app, &pacote_em(&pasta, "1.0.0", LANCADOR_A), "1.0.0", Some("0.9.0"), None).unwrap();
        assert_eq!(versoes(&app), "atual=1.0.0\r\nanterior=0.9.0\r\n");
        assert_eq!(nomes(&app), [".trava", "Vitral.exe", "app-0.9.0", "app-1.0.0", "estado.json", "versoes.txt"]);
        assert_eq!(nomes(&pasta_da_versao(&app, "1.0.0")), ["NOVIDADES.md", "vitral-lancador.exe", "vitral.exe"]);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn o_lancador_so_e_trocado_quando_muda_ou_falta() {
        let (pasta, app) = layout_com("0.8.0");
        let raiz = Raiz { dados: pasta.join("Local"), bin: None };
        assert!(!integrar(&raiz, false).unwrap(), "o mesmo lançador não é copiado de novo");
        instalar_pacote(&app, &pacote_em(&pasta, "0.9.0", LANCADOR_B), "0.9.0", Some("0.8.0"), None).unwrap();
        assert_eq!(fs::read(app.join("Vitral.exe")).unwrap(), LANCADOR_B);
        assert!(!app.join("Vitral.exe.novo").exists());
        // Sem o lançador na raiz (apagado à mão), a abertura da versão atual o devolve.
        fs::remove_file(app.join("Vitral.exe")).unwrap();
        assert!(integrar(&raiz, false).unwrap());
        assert_eq!(fs::read(app.join("Vitral.exe")).unwrap(), LANCADOR_B);
        assert_eq!(raiz.binario_atual(), app.join("Vitral.exe"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn uma_queda_em_qualquer_passo_deixa_o_lancador_abrindo_uma_versao_inteira() {
        use Passo::*;
        for passo in [Aberto, Conferido, NoLugar, Lancador, Versoes, Estado, Limpo] {
            let (pasta, app) = layout_com("0.8.0");
            let pacote = pacote_em(&pasta, "0.9.0", LANCADOR_B);
            let caiu = instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), Some(passo));
            assert!(matches!(caiu, Err(Falha::Queda(p)) if p == passo), "{passo:?}");
            let esperado = if matches!(passo, Aberto | Conferido | NoLugar | Lancador) { "0.8.0" } else { "0.9.0" };
            assert_eq!(abre(&app), esperado, "{passo:?}");
            assert_eq!(versao_atual(&app).as_deref(), Some(esperado), "{passo:?}");

            // A abertura seguinte limpa o resto e a instalação de novo vai até o fim.
            limpar_restos(&app, &travar(&app).unwrap().unwrap());
            assert!(nomes(&app).iter().all(|n| !n.starts_with(".novo-") && !n.starts_with(".lixo-") && !n.ends_with(".tmp") && !n.ends_with(".novo")), "{passo:?}: {:?}", nomes(&app));
            assert_eq!(abre(&app), esperado, "{passo:?}");
            instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), None).unwrap();
            assert_eq!(abre(&app), "0.9.0", "{passo:?}");
            assert_eq!(versoes(&app), "atual=0.9.0\r\nanterior=0.8.0\r\n", "{passo:?}");
            assert_eq!(fs::read(app.join("Vitral.exe")).unwrap(), LANCADOR_B, "{passo:?}");
            assert_eq!(nomes(&app), [".trava", "Vitral.exe", "app-0.8.0", "app-0.9.0", "estado.json", "versoes.txt"], "{passo:?}");
            fs::remove_dir_all(&pasta).unwrap();
        }
    }

    #[test]
    fn sem_o_versoes_txt_o_lancador_abre_a_maior_inteira_e_nada_e_apagado() {
        let (pasta, app) = layout_com("0.8.0");
        instalar_pacote(&app, &pacote_em(&pasta, "0.9.0", LANCADOR_A), "0.9.0", Some("0.8.0"), None).unwrap();
        fs::remove_file(app.join(VERSOES)).unwrap();
        // Uma maior pela metade (o vitral.exe zerado) não conta.
        fs::create_dir_all(pasta_da_versao(&app, "1.0.0")).unwrap();
        fs::write(binario_da_versao(&app, "1.0.0"), b"").unwrap();
        assert_eq!(abre(&app), "0.9.0");
        limpar_restos(&app, &travar(&app).unwrap().unwrap());
        assert!(nomes(&app).contains(&"app-0.8.0".to_string()), "sem o versoes.txt, nenhuma versão sai");
        assert_eq!(versao_atual(&app), None);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn a_abertura_apaga_o_que_ficou_pela_metade() {
        let (pasta, app) = layout_com("0.8.0");
        fs::create_dir_all(app.join(".novo-0.9.0-abcd1234/sub")).unwrap();
        fs::create_dir_all(app.join(".lixo-0.7.0-abcd1234")).unwrap();
        fs::write(app.join(".versoes.txt.123.tmp"), "atual=").unwrap();
        fs::write(app.join(".estado.json.123.tmp"), "{").unwrap();
        fs::write(app.join("Vitral.exe.novo"), "MZ").unwrap();
        fs::create_dir_all(app.join(".baixando")).unwrap();
        fs::write(app.join(".baixando/vitral-0.9.0-windows-x86_64.tar.gz.parte"), "meio").unwrap();
        // A que uma limpeza anterior deixou por estar em uso.
        fs::create_dir_all(pasta_da_versao(&app, "0.7.0")).unwrap();
        fs::write(binario_da_versao(&app, "0.7.0"), exe_falso("0.7.0")).unwrap();
        limpar_restos(&app, &travar(&app).unwrap().unwrap());
        assert_eq!(nomes(&app), [".baixando", ".trava", "Vitral.exe", "app-0.8.0", "estado.json", "versoes.txt"]);
        assert!(nomes(&app.join(".baixando")).is_empty());
        assert_eq!(abre(&app), "0.8.0");
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn versao_que_nao_responde_ou_pacote_ruim_nao_entram() {
        let (pasta, app) = layout_com("0.8.0");
        let antes = nomes(&app);
        let ruim = pasta.join("ruim.tar.gz");
        // O vitral.exe diz ser outra versão.
        let prefixo = prefixo("0.9.0");
        let exe = format!("{prefixo}/vitral.exe");
        fs::write(&ruim, tar_gz(&[Entrada::Arquivo(&exe, &exe_falso("0.7.0"))])).unwrap();
        let falha = instalar_pacote(&app, &ruim, "0.9.0", Some("0.8.0"), None).unwrap_err();
        assert!(matches!(falha, Falha::Binario(_)), "{falha}");
        // O pacote do Linux (sem o vitral.exe).
        let linux = format!("{prefixo}/vitral");
        fs::write(&ruim, tar_gz(&[Entrada::Arquivo(&linux, &exe_falso("0.9.0"))])).unwrap();
        let falha = instalar_pacote(&app, &ruim, "0.9.0", Some("0.8.0"), None).unwrap_err();
        assert!(matches!(&falha, Falha::Pacote(e) if e.to_string().contains("vitral.exe")), "{falha}");
        fs::write(&ruim, b"lixo").unwrap();
        assert!(matches!(instalar_pacote(&app, &ruim, "0.9.0", Some("0.8.0"), None), Err(Falha::Pacote(_))));
        assert!(matches!(instalar_pacote(&app, &ruim, "..\\fora", Some("0.8.0"), None), Err(Falha::Pacote(_))));
        assert_eq!(nomes(&app), antes);
        assert_eq!(abre(&app), "0.8.0");
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn reinstalar_a_mesma_versao_nao_mexe_na_que_esta_la() {
        let (pasta, app) = layout_com("0.8.0");
        let pacote = pacote_em(&pasta, "0.9.0", LANCADOR_A);
        instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), None).unwrap();
        let antes = nomes(&app);
        fs::write(pasta_da_versao(&app, "0.9.0").join("marca"), "a que estava").unwrap();
        // Rodando a 0.9.0: a pasta dela fica (o .exe aberto não sairia do lugar no Windows).
        instalar_pacote(&app, &pacote, "0.9.0", Some("0.9.0"), None).unwrap();
        assert_eq!(nomes(&app), antes);
        assert!(pasta_da_versao(&app, "0.9.0").join("marca").is_file());
        assert_eq!(versoes(&app), "atual=0.9.0\r\nanterior=0.8.0\r\n");
        // Quebrada (o vitral.exe zerado), ela é trocada pela do pacote.
        fs::write(binario_da_versao(&app, "0.9.0"), b"").unwrap();
        instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), None).unwrap();
        assert!(!pasta_da_versao(&app, "0.9.0").join("marca").exists());
        assert_eq!(abre(&app), "0.9.0");
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn a_volta_aponta_o_versoes_txt_para_a_anterior() {
        let (pasta, app) = layout_com("0.8.0");
        instalar_pacote(&app, &pacote_em(&pasta, "0.9.0", LANCADOR_A), "0.9.0", Some("0.8.0"), None).unwrap();
        voltar_para(&app, "0.8.0").unwrap();
        assert_eq!(versoes(&app), "atual=0.8.0\r\n");
        assert_eq!(abre(&app), "0.8.0");
        assert_eq!(versao_do_link(&app, "anterior"), None);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn sabe_qual_versao_gerenciada_esta_rodando() {
        let (pasta, app) = layout_com("0.8.0");
        assert_eq!(versao_do_executavel(&app, &binario_da_versao(&app, "0.8.0")).as_deref(), Some("0.8.0"));
        assert_eq!(versao_do_executavel(&app, &app.join("Vitral.exe")), None, "o lançador não é uma versão");
        fs::write(pasta_da_versao(&app, "0.8.0").join("outro.exe"), "MZ").unwrap();
        assert_eq!(versao_do_executavel(&app, &pasta_da_versao(&app, "0.8.0").join("outro.exe")), None);
        let zip = pasta.join("portatil");
        fs::create_dir_all(&zip).unwrap();
        fs::write(zip.join("vitral.exe"), "MZ").unwrap();
        assert_eq!(versao_do_executavel(&app, &zip.join("vitral.exe")), None, "o zip portátil não é gerenciado");
        fs::remove_dir_all(&pasta).unwrap();
    }

    /// Cada nome que este módulo pode deixar na pasta sai com o desinstalador.
    #[test]
    fn o_desinstalador_apaga_tudo_o_que_o_atualizador_cria() {
        let iss = include_str!("../../pacote/windows/vitral.iss");
        for nome in ["app-*", ".novo-*", ".lixo-*", ".baixando", "versoes.txt", "Vitral.exe.novo", "estado.json", ".*.tmp", ".trava"] {
            let linha = format!("Name: \"{{app}}\\{nome}\"");
            assert!(iss.lines().any(|l| l.starts_with("Type: ") && l.contains(&linha)), "falta {nome} no [UninstallDelete]");
        }
        assert!(iss.contains("DefaultDirName={localappdata}\\Programs\\Vitral"), "a mesma pasta do Raiz::app");
        assert!(iss.contains("'atual={#Versao}' + #13#10") && iss.contains("'anterior=' + Anterior + #13#10"), "o mesmo versoes.txt");
    }

    /// Com um arquivo da versão velha aberto (o `.exe` dela rodando), a limpeza não quebra nada: a
    /// pasta fica inteira (ou sai inteira para o `.lixo-*`) e sai na limpeza seguinte.
    #[cfg(windows)]
    #[test]
    fn a_versao_em_uso_fica_para_a_proxima_limpeza() {
        use std::os::windows::fs::OpenOptionsExt as _;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
        let (pasta, app) = layout_com("0.8.0");
        instalar_pacote(&app, &pacote_em(&pasta, "0.9.0", LANCADOR_A), "0.9.0", Some("0.8.0"), None).unwrap();
        let aberto = fs::OpenOptions::new().read(true).share_mode(FILE_SHARE_READ).open(binario_da_versao(&app, "0.8.0")).unwrap();
        instalar_pacote(&app, &pacote_em(&pasta, "1.0.0", LANCADOR_A), "1.0.0", Some("0.9.0"), None).unwrap();
        assert_eq!(abre(&app), "1.0.0");
        let velha = pasta_da_versao(&app, "0.8.0");
        if velha.exists() {
            assert!(binario_da_versao(&app, "0.8.0").is_file() && velha.join("NOVIDADES.md").is_file(), "a 0.8.0 ficou pela metade");
        }
        drop(aberto);
        limpar_restos(&app, &travar(&app).unwrap().unwrap());
        assert_eq!(nomes(&app), [".trava", "Vitral.exe", "app-0.9.0", "app-1.0.0", "estado.json", "versoes.txt"]);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn mede_o_espaco_livre() {
        assert!(espaco_livre(&std::env::temp_dir()).unwrap() > 0);
        assert!(espaco_livre(Path::new("Z:\\nao\\existe")).is_err());
    }
}
