//! Instalação gerenciada no Linux: uma pasta por versão em `~/.local/share/vitral/app/`, o link
//! `atual` (o `.desktop` abre por ele) e o `anterior`, cada um trocado com um rename só. Qualquer
//! passo pode ser cortado por uma queda de energia sem deixar o app quebrado: `atual` sempre aponta
//! para uma versão completa e o que sobrar pela metade é apagado na próxima abertura
//! (`distribuicao.md`, seções 3.7 e 4).
//!
//! ```text
//! app/0.2.0/vitral   app/0.3.0/vitral   app/atual -> 0.3.0   app/anterior -> 0.2.0
//! app/estado.json    app/.baixando/     app/.novo-*/  app/.lixo-*/  app/.trava
//! ```

use std::{
    ffi::CString,
    fmt, fs, io,
    os::unix::{ffi::OsStrExt as _, fs::PermissionsExt as _},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow};

pub use super::estado_app::{Abertura, Confirmar, EstadoApp, Volta};
use super::pacote::{self, LIMITE_EXTRAIDO, PRAZO_CONFERENCIA, gravar_atomico};
use crate::processos::novo_comando;

/// Onde o Vitral gerenciado mora (`<dados>/vitral/app`) e onde ele se mostra ao sistema
/// (`<dados>/applications`, `<dados>/icons` e o link em `<bin>`). Os testes passam uma pasta
/// temporária; o app usa a do usuário.
#[derive(Debug, Clone)]
pub struct Raiz {
    pub dados: PathBuf,
    pub bin: Option<PathBuf>,
}

impl Raiz {
    /// `~/.local/share` e `~/.local/bin` (ou os `XDG_*` equivalentes).
    pub fn do_usuario() -> Option<Raiz> {
        Some(Raiz { dados: dirs::data_dir()?, bin: dirs::executable_dir() })
    }

    pub fn app(&self) -> PathBuf {
        self.dados.join("vitral").join(crate::dados::PASTA_DO_APP)
    }

    /// `app/atual/vitral`: o que o menu abre e para onde o Reiniciar volta.
    pub fn binario_atual(&self) -> PathBuf {
        self.app().join("atual").join("vitral")
    }

    fn aplicativos(&self) -> PathBuf {
        self.dados.join("applications")
    }

    fn hicolor(&self) -> PathBuf {
        self.dados.join("icons").join("hicolor")
    }
}

/// Os passos da instalação, na ordem. Os testes cortam depois de cada um.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Passo {
    Aberto,
    Conferido,
    NoLugar,
    Anterior,
    Atual,
    Estado,
    Limpo,
}

/// De onde vem a versão nova.
pub enum Origem<'a> {
    /// O `.tar.gz` do release, com a pasta `<prefixo>/` dentro.
    Pacote { arquivo: &'a Path, prefixo: &'a str },
    /// Um binário pronto (o `vitral --instalar`), com os textos que estiverem ao lado dele.
    Binario(&'a Path),
}

#[derive(Debug)]
pub enum Falha {
    /// O pacote não se abre ou tem o que não devia.
    Pacote(anyhow::Error),
    /// O binário novo não respondeu ao `--versao --json`.
    Binario(anyhow::Error),
    /// Disco: permissão, espaço, rename.
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

/// A trava de `app/.trava`: dois Vitrals (ou o app e um `--instalar`) não instalam juntos.
pub struct Trava(#[expect(dead_code, reason = "segura a trava enquanto existe")] fs::File);

/// A trava, ou None se outro já a tem. Um processo que outra thread acabou de abrir herda a trava
/// até o exec (o motor abre processos o tempo todo): antes de dizer que está ocupada, espera um
/// instante.
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

fn travar_esperando(app: &Path) -> io::Result<Trava> {
    if let Some(trava) = travar(app)? {
        return Ok(trava);
    }
    eprintln!("Esperando outra instalação do Vitral terminar…");
    let arquivo = fs::OpenOptions::new().create(true).truncate(false).write(true).open(app.join(".trava"))?;
    arquivo.lock()?;
    Ok(Trava(arquivo))
}

/// Instala `versao` em `app/<versao>/` e aponta `atual` para ela; a que estava rodando (ou a que
/// era a atual) vira a `anterior`. Ordem: abre numa `.novo-*`, testa o binário, põe no lugar com um
/// rename, aponta `anterior` e depois `atual`, grava o `estado.json` e tira as versões que não são
/// nenhuma das duas.
pub fn instalar(app: &Path, origem: Origem, versao: &str, rodando: Option<&str>, trava: &Trava) -> Result<Instalada, Falha> {
    instalar_com(app, origem, versao, rodando, trava, None)
}

fn instalar_com(
    app: &Path,
    origem: Origem,
    versao: &str,
    rodando: Option<&str>,
    _trava: &Trava,
    cair: Option<Passo>,
) -> Result<Instalada, Falha> {
    let ponto = |passo: Passo| if cair == Some(passo) { Err(Falha::Queda(passo)) } else { Ok(()) };
    if !nome_de_versao(versao) {
        return Err(Falha::Pacote(anyhow!("versão inválida: {versao}")));
    }
    fs::create_dir_all(app).map_err(disco)?;
    let novo = app.join(format!(".novo-{versao}-{}", aleatorio()));
    fs::create_dir(&novo).map_err(disco)?;
    let preparado = preparar(&novo, origem, versao, ponto);
    let resposta = match preparado {
        Ok(resposta) => resposta,
        Err(Falha::Queda(passo)) => return Err(Falha::Queda(passo)),
        Err(falha) => {
            let _ = fs::remove_dir_all(&novo);
            return Err(falha);
        }
    };

    let antes_atual = versao_do_link(app, "atual");
    let antes_anterior = versao_do_link(app, "anterior");
    por_no_lugar(app, &novo, versao).map_err(disco)?;
    ponto(Passo::NoLugar)?;

    let anterior = [rodando.map(str::to_string), antes_atual, antes_anterior]
        .into_iter()
        .flatten()
        .find(|v| v != versao && app.join(v).join("vitral").is_file());
    if let Some(anterior) = &anterior {
        apontar(app, "anterior", anterior).map_err(disco)?;
    }
    ponto(Passo::Anterior)?;
    apontar(app, "atual", versao).map_err(disco)?;
    ponto(Passo::Atual)?;

    let mut estado = EstadoApp::ler(app);
    estado.formatos.insert(versao.to_string(), resposta.formato_dados);
    estado.formatos.retain(|v, _| v == versao || Some(v) == anterior.as_ref());
    estado.confirmar = Some(Confirmar { versao: versao.to_string(), aberturas: Vec::new() });
    estado.recusadas.retain(|v| v != versao);
    estado.gravar(app).map_err(disco)?;
    ponto(Passo::Estado)?;

    tirar_versoes(app, &[versao, anterior.as_deref().unwrap_or(versao)]);
    ponto(Passo::Limpo)?;
    Ok(Instalada { versao: versao.to_string(), formato_dados: resposta.formato_dados })
}

/// Passos 1 e 2: a versão nova aberta em `novo` e o binário dela respondendo.
fn preparar(
    novo: &Path,
    origem: Origem,
    versao: &str,
    ponto: impl Fn(Passo) -> Result<(), Falha>,
) -> Result<pacote::Resposta, Falha> {
    match origem {
        Origem::Pacote { arquivo, prefixo } => {
            pacote::extrair(arquivo, novo, prefixo, "vitral", LIMITE_EXTRAIDO).map_err(Falha::Pacote)?
        }
        Origem::Binario(binario) => copiar_binario(binario, novo).map_err(disco)?,
    }
    ponto(Passo::Aberto)?;
    let resposta = pacote::conferir_binario(&novo.join("vitral"), versao, PRAZO_CONFERENCIA).map_err(Falha::Binario)?;
    ponto(Passo::Conferido)?;
    Ok(resposta)
}

fn copiar_binario(binario: &Path, novo: &Path) -> Result<()> {
    let copiar = |de: &Path, nome: &str, modo: u32| -> Result<()> {
        let para = novo.join(nome);
        fs::copy(de, &para).with_context(|| format!("não copiou {}", de.display()))?;
        let arquivo = fs::File::open(&para)?;
        arquivo.set_permissions(fs::Permissions::from_mode(modo))?;
        arquivo.sync_all()?;
        Ok(())
    };
    copiar(binario, "vitral", 0o755)?;
    if let Some(pasta) = binario.parent() {
        for nome in ["LEIA-ME.md", "NOVIDADES.md", "LICENSE", "OFL-fontes.txt"] {
            let texto = pasta.join(nome);
            if texto.is_file() {
                copiar(&texto, nome, 0o644)?;
            }
        }
    }
    fs::File::open(novo)?.sync_all()?;
    Ok(())
}

/// Passo 3: `novo` vira `app/<versao>` de uma vez. Se ela já existe (reinstalar a mesma versão), as
/// duas trocam de lugar num rename só (`RENAME_EXCHANGE`) e a velha sai: `atual` nunca fica
/// apontando para uma pasta que não existe.
fn por_no_lugar(app: &Path, novo: &Path, versao: &str) -> io::Result<()> {
    let alvo = app.join(versao);
    if fs::symlink_metadata(&alvo).is_err() {
        fs::rename(novo, &alvo)?;
    } else {
        match trocar(novo, &alvo) {
            Ok(()) => {}
            // Sistema de arquivos sem a troca: fica a que já está lá, se responde.
            Err(erro) if erro.raw_os_error() == Some(libc::EINVAL) => {
                if pacote::conferir_binario(&alvo.join("vitral"), versao, PRAZO_CONFERENCIA).is_err() {
                    jogar_fora(app, &alvo, versao);
                    fs::rename(novo, &alvo)?;
                }
            }
            Err(erro) => return Err(erro),
        }
        jogar_fora(app, novo, versao);
    }
    fs::File::open(app)?.sync_all()
}

fn trocar(a: &Path, b: &Path) -> io::Result<()> {
    let a = CString::new(a.as_os_str().as_bytes())?;
    let b = CString::new(b.as_os_str().as_bytes())?;
    // SAFETY: dois caminhos terminados em zero, válidos durante a chamada.
    let r = unsafe { libc::renameat2(libc::AT_FDCWD, a.as_ptr(), libc::AT_FDCWD, b.as_ptr(), libc::RENAME_EXCHANGE) };
    if r == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
}

/// Renomeia para `.lixo-*` (de uma vez) e apaga. Se cair no meio, a abertura seguinte termina.
fn jogar_fora(app: &Path, pasta: &Path, versao: &str) {
    if !pasta.exists() {
        return;
    }
    let lixo = app.join(format!(".lixo-{versao}-{}", aleatorio()));
    if fs::rename(pasta, &lixo).is_ok() {
        let _ = fs::remove_dir_all(&lixo);
    }
}

/// Passo 4: o link `app/<nome>` passa a apontar para `versao` com um rename só (aponta para a
/// velha ou para a nova, nunca para nada).
pub(super) fn apontar(app: &Path, nome: &str, versao: &str) -> io::Result<()> {
    let temporario = app.join(format!(".{nome}.novo"));
    let _ = fs::remove_file(&temporario);
    std::os::unix::fs::symlink(versao, &temporario)?;
    fs::rename(&temporario, app.join(nome))?;
    fs::File::open(app)?.sync_all()
}

/// A pasta de `versao` (`app/<versão>`).
pub fn pasta_da_versao(app: &Path, versao: &str) -> PathBuf {
    app.join(versao)
}

/// O binário de `versao` (`app/<versão>/vitral`).
pub fn binario_da_versao(app: &Path, versao: &str) -> PathBuf {
    app.join(versao).join("vitral")
}

/// A volta (da saúde ou das Configurações): `atual` passa para `para`; o `anterior` apontava para
/// ela e sobra.
pub fn voltar_para(app: &Path, para: &str) -> io::Result<()> {
    apontar(app, "atual", para)?;
    if versao_do_link(app, "anterior").as_deref() == Some(para) {
        fs::remove_file(app.join("anterior"))?;
        fs::File::open(app)?.sync_all()?;
    }
    Ok(())
}

/// A versão para onde o link `app/<nome>` aponta (só um nome de versão, nunca um caminho).
pub fn versao_do_link(app: &Path, nome: &str) -> Option<String> {
    let alvo = fs::read_link(app.join(nome)).ok()?;
    let versao = alvo.to_str()?;
    nome_de_versao(versao).then(|| versao.to_string())
}

pub fn versao_atual(app: &Path) -> Option<String> {
    versao_do_link(app, "atual")
}

/// A versão gerenciada que está rodando (None se este binário não é de `app/<versão>/`).
pub fn versao_rodando(app: &Path) -> Option<String> {
    versao_do_executavel(app, &std::env::current_exe().ok()?)
}

fn versao_do_executavel(app: &Path, executavel: &Path) -> Option<String> {
    let executavel = executavel.canonicalize().ok()?;
    let pasta = executavel.parent()?;
    if executavel.file_name()? != "vitral" || pasta.parent()? != app.canonicalize().ok()? {
        return None;
    }
    let versao = pasta.file_name()?.to_str()?;
    nome_de_versao(versao).then(|| versao.to_string())
}

/// Um nome de pasta que é uma versão: semver, sem metadado de build.
fn nome_de_versao(texto: &str) -> bool {
    semver::Version::parse(texto).is_ok_and(|v| v.build.is_empty())
}

fn aleatorio() -> String {
    uuid::Uuid::new_v4().simple().to_string()[..8].to_string()
}

/// Passo 6: as versões que não são a atual nem a anterior vão para `.lixo-*` e são apagadas. No
/// Linux, apagar a pasta de um binário que está rodando não afeta o processo.
fn tirar_versoes(app: &Path, ficar: &[&str]) {
    let Ok(entradas) = fs::read_dir(app) else { return };
    for entrada in entradas.flatten() {
        let nome = entrada.file_name();
        let Some(nome) = nome.to_str() else { continue };
        if nome_de_versao(nome) && !ficar.contains(&nome) && entrada.file_type().is_ok_and(|t| t.is_dir()) {
            jogar_fora(app, &entrada.path(), nome);
        }
    }
}

/// O que sobrou de uma instalação ou de um download cortados no meio (`.novo-*`, `.lixo-*`, link
/// temporário, `.parte`). Só com a trava: outro Vitral pode estar instalando agora.
pub fn limpar_restos(app: &Path, _trava: &Trava) {
    if let Ok(entradas) = fs::read_dir(app) {
        for entrada in entradas.flatten() {
            let nome = entrada.file_name();
            let nome = nome.to_string_lossy();
            let caminho = entrada.path();
            if nome.starts_with(".novo-") || nome.starts_with(".lixo-") {
                let _ = fs::remove_dir_all(&caminho);
            } else if nome.starts_with('.') && (nome.ends_with(".novo") || nome.ends_with(".tmp")) {
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
}


/// Espaço livre no disco de `pasta`, em bytes.
pub fn espaco_livre(pasta: &Path) -> io::Result<u64> {
    let caminho = CString::new(pasta.as_os_str().as_bytes())?;
    let mut info: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: caminho terminado em zero e uma struct do tamanho certo para o kernel preencher.
    if unsafe { libc::statvfs(caminho.as_ptr(), &mut info) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(info.f_bavail.saturating_mul(info.f_frsize))
}

const DESKTOP: &str = include_str!("../../assets/vitral.desktop");

/// Os ícones vão dentro do binário: o pacote é só o `vitral`. O logo pequeno, feito para tamanhos
/// pequenos, fica com 16, 24 e 32 (notificações, alt+tab, barra).
const ICONES: [(&str, &[u8]); 8] = [
    ("scalable/apps/vitral.svg", include_bytes!("../../assets/vitral.svg")),
    ("48x48/apps/vitral.png", include_bytes!("../../assets/vitral-48.png")),
    ("64x64/apps/vitral.png", include_bytes!("../../assets/vitral-64.png")),
    ("128x128/apps/vitral.png", include_bytes!("../../assets/vitral-128.png")),
    ("256x256/apps/vitral.png", include_bytes!("../../assets/vitral-256.png")),
    ("16x16/apps/vitral.svg", include_bytes!("../../assets/vitral-pequeno.svg")),
    ("24x24/apps/vitral.svg", include_bytes!("../../assets/vitral-pequeno.svg")),
    ("32x32/apps/vitral.svg", include_bytes!("../../assets/vitral-pequeno.svg")),
];

/// Põe o Vitral no menu: o `vitral.desktop` (o mesmo nome de antes, então o favorito do dash
/// continua) com `Exec` no link `atual`, os ícones e, se `~/.local/bin` existe, o link `vitral`
/// ali. Só grava o que mudou (temporário + rename) e então, com `caches`, atualiza os caches do
/// GNOME. Roda no `--instalar`, depois de cada instalação e em toda abertura da versão atual.
/// Devolve se algo mudou.
pub fn integrar(raiz: &Raiz, caches: bool) -> io::Result<bool> {
    let desktop = DESKTOP.replace("@BINARIO@", &argumento_do_exec(&raiz.binario_atual()));
    let mudou_desktop = gravar_se_mudou(&raiz.aplicativos().join("vitral.desktop"), desktop.as_bytes())?;
    let mut mudou_icone = false;
    for (caminho, bytes) in ICONES {
        mudou_icone |= gravar_se_mudou(&raiz.hicolor().join(caminho), bytes)?;
    }
    if let Some(bin) = raiz.bin.as_deref().filter(|b| b.is_dir()) {
        ligar_no_bin(bin, &raiz.binario_atual())?;
    }
    if caches && mudou_desktop {
        melhor_esforco(novo_comando("update-desktop-database").arg(raiz.aplicativos()));
    }
    if caches && mudou_icone {
        melhor_esforco(novo_comando("gtk-update-icon-cache").arg("-q").arg("-t").arg(raiz.hicolor()));
    }
    Ok(mudou_desktop || mudou_icone)
}

fn gravar_se_mudou(caminho: &Path, bytes: &[u8]) -> io::Result<bool> {
    if fs::read(caminho).is_ok_and(|atual| atual == bytes) {
        return Ok(false);
    }
    gravar_atomico(caminho, bytes, 0o644)?;
    Ok(true)
}

/// `~/.local/bin/vitral` -> `app/atual/vitral`. Um arquivo comum com esse nome não é nosso: fica.
fn ligar_no_bin(bin: &Path, alvo: &Path) -> io::Result<()> {
    let link = bin.join("vitral");
    match fs::symlink_metadata(&link) {
        Ok(m) if !m.file_type().is_symlink() => return Ok(()),
        Ok(_) if fs::read_link(&link).is_ok_and(|a| a == alvo) => return Ok(()),
        _ => {}
    }
    let temporario = bin.join(".vitral.novo");
    let _ = fs::remove_file(&temporario);
    std::os::unix::fs::symlink(alvo, &temporario)?;
    fs::rename(&temporario, &link)
}

fn melhor_esforco(comando: &mut Command) {
    let _ = comando.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
}

/// O caminho como argumento do `Exec` (especificação Desktop Entry): entre aspas se tem espaço ou
/// caractere reservado, com `%` dobrado e as barras escapadas duas vezes (aspas e depois string).
fn argumento_do_exec(caminho: &Path) -> String {
    let texto = caminho.to_string_lossy().replace('%', "%%");
    if !texto.chars().any(|c| " \t\n\"'\\><~|&;$*?#()`".contains(c)) {
        return texto;
    }
    let mut saida = String::from("\"");
    for c in texto.chars() {
        match c {
            '"' | '`' | '$' => {
                saida.push_str("\\\\");
                saida.push(c);
            }
            '\\' => saida.push_str("\\\\\\\\"),
            _ => saida.push(c),
        }
    }
    saida.push('"');
    saida
}

/// `vitral --instalar`: copia este binário para o layout (passos 1 a 5), põe no menu e diz o que
/// fez. Usado pelo `instalar.sh` do release e pelo `ferramentas/instalar-dev.sh`.
pub fn instalar_por_comando() -> i32 {
    let feito = Raiz::do_usuario()
        .context("não achei a pasta de dados do usuário (HOME)")
        .and_then(|raiz| instalar_executavel(&raiz, &std::env::current_exe()?, super::VERSAO, true));
    match feito {
        Ok(texto) => {
            println!("{texto}");
            0
        }
        Err(erro) => {
            eprintln!("Não instalei o Vitral: {erro:#}");
            1
        }
    }
}

fn instalar_executavel(raiz: &Raiz, executavel: &Path, versao: &str, caches: bool) -> Result<String> {
    let app = raiz.app();
    if versao_do_executavel(&app, executavel).as_deref() == Some(versao) {
        integrar(raiz, caches).context("não pus no menu")?;
        return Ok(format!("O Vitral {versao} já está instalado; o atalho do menu foi conferido."));
    }
    let trava = travar_esperando(&app).with_context(|| format!("não abri {}", app.display()))?;
    limpar_restos(&app, &trava);
    instalar(&app, Origem::Binario(&executavel.canonicalize()?), versao, None, &trava)?;
    integrar(raiz, caches).context("instalei, mas não pus no menu")?;
    Ok(format!("Vitral {versao} instalado. Abra pelo menu de aplicativos."))
}

#[cfg(test)]
pub mod testes {
    use super::*;
    use crate::atualizacao::pacote::testes::{binario_falso, pacote_bom, pasta_de_teste};
    use std::collections::BTreeMap;

    /// Um layout com `versao` já instalada (binário de mentira) e `atual` nela.
    pub fn layout_com(versao: &str) -> (PathBuf, PathBuf) {
        let pasta = pasta_de_teste("layout");
        let app = pasta.join("dados/vitral/app");
        let fonte = pasta.join("fonte").join(versao);
        gravar_atomico(&fonte.join("vitral"), &binario_falso(versao), 0o755).unwrap();
        let trava = travar(&app).unwrap().unwrap();
        instalar(&app, Origem::Binario(&fonte.join("vitral")), versao, None, &trava).unwrap();
        (pasta, app)
    }

    fn pacote_em(pasta: &Path, versao: &str) -> PathBuf {
        let caminho = pasta.join(format!("vitral-{versao}-linux-x86_64.tar.gz"));
        fs::write(&caminho, pacote_bom(&format!("vitral-{versao}-linux-x86_64"), versao)).unwrap();
        caminho
    }

    fn instalar_pacote(app: &Path, pacote: &Path, versao: &str, rodando: Option<&str>, cair: Option<Passo>) -> Result<Instalada, Falha> {
        let trava = travar(app).unwrap().unwrap();
        let prefixo = format!("vitral-{versao}-linux-x86_64");
        instalar_com(app, Origem::Pacote { arquivo: pacote, prefixo: &prefixo }, versao, rodando, &trava, cair)
    }

    fn nomes(pasta: &Path) -> Vec<String> {
        let mut nomes: Vec<_> = fs::read_dir(pasta).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        nomes.sort();
        nomes
    }

    /// O link aponta para uma versão completa, que responde como ela mesma.
    fn completa(app: &Path, nome: &str) -> Option<String> {
        let versao = versao_do_link(app, nome)?;
        pacote::conferir_binario(&app.join(nome).join("vitral"), &versao, PRAZO_CONFERENCIA)
            .unwrap_or_else(|e| panic!("{nome} -> {versao} quebrada: {e:#}"));
        Some(versao)
    }

    #[test]
    fn instala_aponta_e_guarda_a_anterior() {
        let (pasta, app) = layout_com("0.8.0");
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.8.0"));
        assert_eq!(versao_do_link(&app, "anterior"), None);

        let instalada = instalar_pacote(&app, &pacote_em(&pasta, "0.9.0"), "0.9.0", Some("0.8.0"), None).unwrap();
        assert_eq!(instalada, Instalada { versao: "0.9.0".into(), formato_dados: 1 });
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.9.0"));
        assert_eq!(completa(&app, "anterior").as_deref(), Some("0.8.0"));
        assert!(app.join("0.9.0/NOVIDADES.md").is_file());
        let estado = EstadoApp::ler(&app);
        assert_eq!(estado.confirmar.as_ref().map(|c| c.versao.as_str()), Some("0.9.0"));
        assert_eq!(estado.formatos, BTreeMap::from([("0.8.0".into(), 1), ("0.9.0".into(), 1)]));

        // Na terceira, a mais velha sai; a que rodava fica como anterior.
        instalar_pacote(&app, &pacote_em(&pasta, "1.0.0"), "1.0.0", Some("0.9.0"), None).unwrap();
        assert_eq!(completa(&app, "atual").as_deref(), Some("1.0.0"));
        assert_eq!(completa(&app, "anterior").as_deref(), Some("0.9.0"));
        assert_eq!(nomes(&app), [".trava", "0.9.0", "1.0.0", "anterior", "atual", "estado.json"]);
        assert_eq!(EstadoApp::ler(&app).formatos.keys().collect::<Vec<_>>(), ["0.9.0", "1.0.0"]);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn reinstalar_a_mesma_versao_troca_sem_duplicar() {
        let (pasta, app) = layout_com("0.8.0");
        let pacote = pacote_em(&pasta, "0.9.0");
        instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), None).unwrap();
        let antes = nomes(&app);
        fs::write(app.join("0.9.0/sobra-da-velha"), "x").unwrap();
        instalar_pacote(&app, &pacote, "0.9.0", Some("0.9.0"), None).unwrap();
        assert_eq!(nomes(&app), antes);
        assert!(!app.join("0.9.0/sobra-da-velha").exists(), "a pasta velha foi trocada pela nova");
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.9.0"));
        assert_eq!(completa(&app, "anterior").as_deref(), Some("0.8.0"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn uma_queda_em_qualquer_passo_deixa_o_app_abrindo() {
        for passo in [Passo::Aberto, Passo::Conferido, Passo::NoLugar, Passo::Anterior, Passo::Atual, Passo::Estado, Passo::Limpo] {
            let (pasta, app) = layout_com("0.8.0");
            let pacote = pacote_em(&pasta, "0.9.0");
            let caiu = instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), Some(passo));
            assert!(matches!(caiu, Err(Falha::Queda(p)) if p == passo), "{passo:?}");
            let atual = completa(&app, "atual").unwrap_or_else(|| panic!("{passo:?}: atual sumiu"));
            let esperado = if matches!(passo, Passo::Aberto | Passo::Conferido | Passo::NoLugar | Passo::Anterior) { "0.8.0" } else { "0.9.0" };
            assert_eq!(atual, esperado, "{passo:?}");
            let _ = completa(&app, "anterior");

            // A abertura seguinte limpa o resto e a instalação de novo vai até o fim.
            limpar_restos(&app, &travar(&app).unwrap().unwrap());
            assert!(nomes(&app).iter().all(|n| !n.starts_with(".novo-") && !n.starts_with(".lixo-") && !n.ends_with(".novo")), "{passo:?}: {:?}", nomes(&app));
            instalar_pacote(&app, &pacote, "0.9.0", Some("0.8.0"), None).unwrap();
            assert_eq!(completa(&app, "atual").as_deref(), Some("0.9.0"), "{passo:?}");
            assert_eq!(completa(&app, "anterior").as_deref(), Some("0.8.0"), "{passo:?}");
            assert_eq!(nomes(&app), [".trava", "0.8.0", "0.9.0", "anterior", "atual", "estado.json"], "{passo:?}");
            fs::remove_dir_all(&pasta).unwrap();
        }
    }

    #[test]
    fn a_abertura_apaga_o_que_ficou_pela_metade() {
        let (pasta, app) = layout_com("0.8.0");
        fs::create_dir_all(app.join(".novo-0.9.0-abcd1234/sub")).unwrap();
        fs::create_dir_all(app.join(".lixo-0.7.0-abcd1234")).unwrap();
        std::os::unix::fs::symlink("0.9.0", app.join(".atual.novo")).unwrap();
        fs::write(app.join(".estado.json.123.tmp"), "{").unwrap();
        fs::create_dir_all(app.join(".baixando")).unwrap();
        fs::write(app.join(".baixando/vitral-0.9.0-linux-x86_64.tar.gz.parte"), "meio").unwrap();
        limpar_restos(&app, &travar(&app).unwrap().unwrap());
        assert_eq!(nomes(&app), [".baixando", ".trava", "0.8.0", "atual", "estado.json"]);
        assert!(nomes(&app.join(".baixando")).is_empty());
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.8.0"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn versao_que_nao_responde_ou_pacote_ruim_nao_entram() {
        let (pasta, app) = layout_com("0.8.0");
        let ruim = pasta.join("ruim.tar.gz");
        fs::write(&ruim, pacote_bom("vitral-0.9.0-linux-x86_64", "0.7.0")).unwrap();
        let falha = instalar_pacote(&app, &ruim, "0.9.0", Some("0.8.0"), None).unwrap_err();
        assert!(matches!(falha, Falha::Binario(_)), "{falha}");
        fs::write(&ruim, b"lixo").unwrap();
        let falha = instalar_pacote(&app, &ruim, "0.9.0", Some("0.8.0"), None).unwrap_err();
        assert!(matches!(falha, Falha::Pacote(_)), "{falha}");
        let falha = instalar_pacote(&app, &ruim, "../fora", Some("0.8.0"), None).unwrap_err();
        assert!(matches!(falha, Falha::Pacote(_)), "{falha}");
        assert_eq!(nomes(&app), [".trava", "0.8.0", "atual", "estado.json"]);
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.8.0"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn so_um_instala_por_vez() {
        let pasta = pasta_de_teste("trava");
        let primeira = travar(&pasta).unwrap();
        assert!(primeira.is_some());
        assert!(travar(&pasta).unwrap().is_none());
        drop(primeira);
        assert!(travar(&pasta).unwrap().is_some());
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn sabe_qual_versao_gerenciada_esta_rodando() {
        let (pasta, app) = layout_com("0.8.0");
        assert_eq!(versao_do_executavel(&app, &app.join("0.8.0/vitral")).as_deref(), Some("0.8.0"));
        assert_eq!(versao_do_executavel(&app, &app.join("atual/vitral")).as_deref(), Some("0.8.0"));
        assert_eq!(versao_do_executavel(&app, &pasta.join("fonte/0.8.0/vitral")), None);
        assert_eq!(versao_do_executavel(&app, &app.join("0.8.0/outro")), None);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn integrar_poe_no_menu_pelo_link_e_so_grava_o_que_mudou() {
        let pasta = pasta_de_teste("integrar");
        let raiz = Raiz { dados: pasta.join("dados"), bin: Some(pasta.join("bin")) };
        fs::create_dir_all(pasta.join("bin")).unwrap();
        assert!(integrar(&raiz, false).unwrap());
        let desktop = fs::read_to_string(pasta.join("dados/applications/vitral.desktop")).unwrap();
        let exec = format!("Exec={}/dados/vitral/app/atual/vitral %f", pasta.display());
        assert!(desktop.lines().any(|l| l == exec), "{desktop}");
        assert!(desktop.contains("Icon=vitral") && desktop.contains("StartupWMClass=vitral"));
        for (caminho, bytes) in ICONES {
            assert_eq!(fs::read(pasta.join("dados/icons/hicolor").join(caminho)).unwrap(), bytes);
        }
        assert_eq!(fs::read_link(pasta.join("bin/vitral")).unwrap(), raiz.binario_atual());
        let antes = nomes(&pasta.join("dados/applications"));
        assert!(!integrar(&raiz, false).unwrap(), "a segunda vez não muda nada");
        assert_eq!(nomes(&pasta.join("dados/applications")), antes);

        // Um vitral de outra origem em ~/.local/bin não é trocado.
        fs::remove_file(pasta.join("bin/vitral")).unwrap();
        fs::write(pasta.join("bin/vitral"), "meu").unwrap();
        integrar(&raiz, false).unwrap();
        assert_eq!(fs::read_to_string(pasta.join("bin/vitral")).unwrap(), "meu");
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn exec_com_espaco_vai_entre_aspas() {
        assert_eq!(argumento_do_exec(Path::new("/home/v/.local/share/vitral/app/atual/vitral")), "/home/v/.local/share/vitral/app/atual/vitral");
        assert_eq!(argumento_do_exec(Path::new("/home/meu nome/vitral")), "\"/home/meu nome/vitral\"");
        assert_eq!(argumento_do_exec(Path::new("/h/100%/a b")), "\"/h/100%%/a b\"");
        assert_eq!(argumento_do_exec(Path::new("/h/$x y")), "\"/h/\\\\$x y\"");
    }

    #[test]
    fn instalar_o_proprio_binario_copia_os_textos_e_nao_duplica() {
        let pasta = pasta_de_teste("instalar");
        let raiz = Raiz { dados: pasta.join("dados"), bin: None };
        let origem = pasta.join("vitral-0.9.0-linux-x86_64");
        gravar_atomico(&origem.join("vitral"), &binario_falso("0.9.0"), 0o755).unwrap();
        fs::write(origem.join("NOVIDADES.md"), "## 0.9.0").unwrap();
        let texto = instalar_executavel(&raiz, &origem.join("vitral"), "0.9.0", false).unwrap();
        assert_eq!(texto, "Vitral 0.9.0 instalado. Abra pelo menu de aplicativos.");
        let app = raiz.app();
        assert_eq!(nomes(&app.join("0.9.0")), ["NOVIDADES.md", "vitral"]);
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.9.0"));
        let antes = nomes(&app);
        instalar_executavel(&raiz, &origem.join("vitral"), "0.9.0", false).unwrap();
        assert_eq!(nomes(&app), antes);
        // Rodando de dentro do layout, só confere o menu.
        let texto = instalar_executavel(&raiz, &app.join("atual/vitral"), "0.9.0", false).unwrap();
        assert!(texto.contains("já está instalado"), "{texto}");
        // A versão do binário tem de ser a que ele diz.
        assert!(instalar_executavel(&raiz, &origem.join("vitral"), "1.0.0", false).is_err());
        assert_eq!(completa(&app, "atual").as_deref(), Some("0.9.0"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn o_instalar_sh_do_release_instala_o_release_de_onde_saiu() {
        let instalar = include_str!("../../pacote/linux/instalar.sh");
        assert_eq!(instalar.lines().filter(|l| *l == "TAG_DO_RELEASE=\"\"").count(), 1);
        assert!(instalar.contains("\"./$pasta/vitral\" --instalar"));
        let release = include_str!("../../.github/workflows/release.yml");
        assert!(release.contains(r#"sed 's|^TAG_DO_RELEASE=""$|TAG_DO_RELEASE="'"$TAG"'"|' pacote/linux/instalar.sh > dist/instalar.sh"#));
        let dev = include_str!("../../ferramentas/instalar-dev.sh");
        assert!(dev.contains("export VITRAL_VERSAO=") && dev.contains("\"$binario\" --instalar"));
    }

    #[test]
    fn mede_o_espaco_livre() {
        assert!(espaco_livre(&std::env::temp_dir()).unwrap() > 0);
        assert!(espaco_livre(Path::new("/nao/existe")).is_err());
    }
}
