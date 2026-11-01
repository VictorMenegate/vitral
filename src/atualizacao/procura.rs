//! O ciclo da atualização (procurar, conferir a assinatura, ler o manifesto, baixar, instalar) e a
//! thread `vitral-atualizacao`, que o roda de tempos em tempos com prioridade baixa. O `rodar` não
//! sabe de relógio nem de thread: os testes servem um release falso com o tiny_http e passam a
//! chave de teste.

use std::{
    fs, io,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use semver::Version;

use super::{
    Comando, Erro, Estado, Noticia, Novidade, REPO, VERSAO, assinatura,
    config::{self, Config, DESISTE_DEPOIS, Modo},
    instalador::{self, Falha, Origem, Raiz},
    manifesto::{self, LIMITE_ASSINATURA, LIMITE_MANIFESTO, Lido, Publicada},
    plataforma,
    rede::{self, Baixar, Rede, Resposta},
};

/// Além de 3 vezes o pacote (o .tar.gz, a pasta aberta e a versão anterior).
const FOLGA_DE_ESPACO: u64 = 100 * 1024 * 1024;
/// O progresso do download vai à interface no máximo 2 vezes por segundo.
const PASSO_DO_PROGRESSO: Duration = Duration::from_millis(500);

/// De onde vêm o manifesto e os pacotes.
pub struct Fonte {
    manifesto: String,
    assinatura: String,
    pacotes: Pacotes,
}

enum Pacotes {
    /// `…/releases/download/v<versão>/<nome>`: montado pelo app, nunca lido do manifesto.
    PorTag(String),
    /// `<base>/<nome>`.
    NaBase(String),
}

impl Fonte {
    /// O canal estável: `releases/latest` ignora rascunhos e pré-lançamentos. Sem API, sem limite
    /// por hora e sem token.
    pub fn do_github() -> Fonte {
        let releases = format!("https://github.com/{REPO}/releases");
        Fonte {
            manifesto: format!("{releases}/latest/download/vitral-versao.json"),
            assinatura: format!("{releases}/latest/download/vitral-versao.json.minisig"),
            pacotes: Pacotes::PorTag(format!("{releases}/download")),
        }
    }

    /// Uma pasta de release só (o `VITRAL_ATUALIZACAO_BASE` do canal de teste, apontado para o tag
    /// de um pré-lançamento, e os testes).
    pub fn na_base(base: &str) -> Fonte {
        let base = base.trim_end_matches('/');
        Fonte {
            manifesto: format!("{base}/vitral-versao.json"),
            assinatura: format!("{base}/vitral-versao.json.minisig"),
            pacotes: Pacotes::NaBase(base.to_string()),
        }
    }

    fn pacote(&self, versao: &Version, nome: &str) -> String {
        match &self.pacotes {
            Pacotes::PorTag(base) => format!("{base}/v{versao}/{nome}"),
            Pacotes::NaBase(base) => format!("{base}/{nome}"),
        }
    }
}

/// Tudo o que o ciclo precisa saber de fora.
pub struct Ambiente {
    pub fonte: Fonte,
    pub rede: Rede,
    /// Confere a assinatura e devolve o comentário confiável (no app, as chaves embutidas).
    pub conferir: fn(&[u8], &str) -> anyhow::Result<String>,
    /// A versão que roda.
    pub versao: Version,
    pub plataforma: String,
    /// Onde instalar (None sem pasta de dados).
    pub raiz: Option<Raiz>,
    /// A versão gerenciada que roda (vira a anterior).
    pub rodando: Option<String>,
    /// Release do GitHub na pasta do app: instala sem perguntar no modo Automáticas.
    pub instala_sozinho: bool,
    /// Canal de teste: aceita pré-lançamento.
    pub aceita_pre: bool,
    /// Atualiza os caches do GNOME depois de mexer no menu.
    pub caches: bool,
}

impl Ambiente {
    /// O do app. Fora do canal estável (compilado no PC ou pré-lançamento), o
    /// `VITRAL_ATUALIZACAO_BASE` aponta para outra pasta de release e aceita pré-lançamento.
    pub fn do_app(raiz: Option<Raiz>, rodando: Option<String>, instala_sozinho: bool) -> Ambiente {
        let versao = Version::parse(VERSAO).unwrap_or(Version::new(0, 0, 0));
        let teste = super::canal() != "estavel" || !versao.pre.is_empty();
        let base = std::env::var("VITRAL_ATUALIZACAO_BASE").ok().filter(|b| teste && !b.is_empty());
        Ambiente {
            fonte: base.as_deref().map_or_else(Fonte::do_github, Fonte::na_base),
            rede: Rede::nova(&format!("Vitral/{VERSAO} ({})", plataforma())),
            conferir: conferir_do_app(base.is_some()),
            versao,
            plataforma: plataforma(),
            raiz,
            rodando,
            instala_sozinho,
            aceita_pre: base.is_some(),
            caches: true,
        }
    }
}

/// As chaves embutidas; no build de desenvolvimento apontado para outro release, também a do
/// `VITRAL_ATUALIZACAO_CHAVE` (o release falso da bancada).
fn conferir_do_app(outra_base: bool) -> fn(&[u8], &str) -> anyhow::Result<String> {
    #[cfg(debug_assertions)]
    if outra_base && std::env::var_os("VITRAL_ATUALIZACAO_CHAVE").is_some() {
        return assinatura::conferir_com_a_de_teste;
    }
    let _ = outra_base;
    assinatura::conferir
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pedido {
    /// A procura de tempos em tempos.
    Sozinho,
    /// "Procurar agora".
    Procurar,
    /// "Instalar" (Só avisar, build do PC): instala mesmo sem permissão para fazer sozinho.
    Instalar,
}

/// Uma volta inteira: procura e, se for o caso, baixa e instala. Grava em `config` a procura que
/// deu certo, o ETag e as falhas; quem grava o arquivo é quem chama. `avisar` recebe os estados do
/// meio (procurando, baixando, instalando); o fim é o que volta.
pub fn rodar(amb: &Ambiente, config: &mut Config, pedido: Pedido, agora: u64, avisar: &mut dyn FnMut(Estado)) -> Estado {
    let fim = uma_volta(amb, config, pedido, agora, avisar);
    // Outra abertura já instalou uma mais nova (e o release pode ter saído do ar): segue pronta.
    match (&fim, &amb.raiz) {
        (Estado::EmDia, Some(raiz)) if amb.rodando.is_some() => {
            super::trocada_por_fora(&raiz.app(), &amb.versao.to_string()).map_or(fim, Estado::Pronta)
        }
        _ => fim,
    }
}

fn uma_volta(amb: &Ambiente, config: &mut Config, pedido: Pedido, agora: u64, avisar: &mut dyn FnMut(Estado)) -> Estado {
    avisar(Estado::Procurando);
    let publicada = match procurar(amb, config, pedido, agora) {
        Ok(Ok(publicada)) => publicada,
        Ok(Err(fim)) => return fim,
        Err(erro) => return Estado::Erro(erro),
    };
    let versao = publicada.versao.to_string();
    let novidade = Novidade { versao: versao.clone(), notas: publicada.notas.clone(), pagina: publicada.pagina.clone(), pela_pagina: false };
    // Já instalada por esta ou outra abertura, esperando o reinício. Só na cópia instalada: no
    // build do PC, o Reiniciar abriria o app no lugar do build.
    if amb.rodando.is_some() && amb.raiz.as_ref().and_then(|r| instalador::versao_atual(&r.app())).as_deref() == Some(versao.as_str()) {
        return Estado::Pronta(novidade);
    }
    let instalar = pedido == Pedido::Instalar || (amb.instala_sozinho && config.modo == Modo::Automaticas);
    if !instalar {
        return Estado::Disponivel(novidade);
    }
    if pedido != Pedido::Instalar && config.falhas.get(&versao).is_some_and(|&n| n >= DESISTE_DEPOIS) {
        return Estado::Erro(Erro::Desistiu { versao });
    }
    match baixar_e_instalar(amb, &publicada, avisar) {
        Ok(()) => {
            config.falhas.remove(&versao);
            Estado::Pronta(novidade)
        }
        Err(erro) => {
            if matches!(erro, Erro::Arquivo { .. } | Erro::Binario { .. } | Erro::Instalacao { .. }) {
                *config.falhas.entry(versao).or_default() += 1;
            }
            Estado::Erro(erro)
        }
    }
}

/// Lê e confere o manifesto. `Ok(Err(estado))` quando a volta acaba aqui (em dia, só avisar).
fn procurar(amb: &Ambiente, config: &mut Config, pedido: Pedido, agora: u64) -> Result<Result<Publicada, Estado>, Erro> {
    let versao = amb.versao.to_string();
    let etag = config.etag.as_deref().filter(|_| config.etag_de.as_deref() == Some(versao.as_str()) && pedido != Pedido::Instalar);
    let (bytes, etag) = match amb.rede.texto(&amb.fonte.manifesto, LIMITE_MANIFESTO, etag).map_err(Erro::Rede)? {
        Resposta::NaoMudou | Resposta::NaoExiste => {
            config.ultima_procura = Some(agora);
            return Ok(Err(Estado::EmDia));
        }
        Resposta::Dados { bytes, etag } => (bytes, etag),
    };
    // A assinatura antes de ler o JSON.
    let assinatura = match amb.rede.texto(&amb.fonte.assinatura, LIMITE_ASSINATURA, None).map_err(Erro::Rede)? {
        Resposta::Dados { bytes, .. } => String::from_utf8(bytes).map_err(|_| Erro::Assinatura)?,
        Resposta::NaoMudou | Resposta::NaoExiste => return Err(Erro::Assinatura),
    };
    let comentario = (amb.conferir)(&bytes, &assinatura).map_err(|_| Erro::Assinatura)?;
    let lido = manifesto::ler(&bytes, &comentario, &amb.plataforma).map_err(|e| Erro::Manifesto(format!("{e:#}")))?;
    config.ultima_procura = Some(agora);
    let recusadas = amb.raiz.as_ref().map(|r| instalador::EstadoApp::ler(&r.app()).recusadas).unwrap_or_default();
    let nova = |publicada: &Version| manifesto::e_nova(&amb.versao, publicada, amb.aceita_pre, &recusadas, config.ignorada.as_deref());
    let publicada = match lido {
        Lido::FormatoNovo { versao: publicada, pagina } if nova(&publicada) => {
            let novidade = Novidade { versao: publicada.to_string(), notas: String::new(), pagina, pela_pagina: true };
            return Ok(Err(Estado::Disponivel(novidade)));
        }
        Lido::Versao(publicada) if nova(&publicada.versao) => publicada,
        Lido::FormatoNovo { .. } | Lido::Versao(_) => {
            // Nada a fazer com este manifesto: da próxima vez, um 304 basta.
            config.etag = etag;
            config.etag_de = Some(versao);
            return Ok(Err(Estado::EmDia));
        }
    };
    config.etag = None;
    config.etag_de = None;
    Ok(Ok(publicada))
}

fn baixar_e_instalar(amb: &Ambiente, publicada: &Publicada, avisar: &mut dyn FnMut(Estado)) -> Result<(), Erro> {
    let versao = publicada.versao.to_string();
    let instalacao = |detalhe: String| Erro::Instalacao { versao: versao.clone(), detalhe };
    let raiz = amb.raiz.as_ref().ok_or_else(|| instalacao("sem pasta de dados".into()))?;
    let app = raiz.app();
    let trava = instalador::travar(&app).map_err(|e| instalacao(e.to_string()))?.ok_or(Erro::Ocupado)?;
    let pacote = &publicada.pacote;
    let livre = instalador::espaco_livre(&app).map_err(|e| instalacao(e.to_string()))?;
    if !cabe(livre, pacote.tamanho) {
        return Err(Erro::Espaco { versao: versao.clone() });
    }
    let baixando = app.join(".baixando");
    fs::create_dir_all(&baixando).map_err(|e| instalacao(e.to_string()))?;
    let destino = baixando.join(&pacote.nome);
    // Um pacote inteiro de uma abertura que caiu antes de instalar serve, se confere.
    if !rede::confere(&destino, pacote.tamanho, &pacote.sha256) {
        if let Ok(entradas) = fs::read_dir(&baixando) {
            for entrada in entradas.flatten() {
                let _ = fs::remove_file(entrada.path());
            }
        }
        let parte = baixando.join(format!("{}.parte", pacote.nome));
        let total = pacote.tamanho;
        avisar(Estado::Baixando { versao: versao.clone(), feito: 0, total });
        let url = amb.fonte.pacote(&publicada.versao, &pacote.nome);
        let baixado = amb.rede.arquivo(&url, &parte, total, &pacote.sha256, &mut |feito| {
            avisar(Estado::Baixando { versao: versao.clone(), feito, total })
        });
        if let Err(erro) = baixado {
            let _ = fs::remove_file(&parte);
            return Err(match erro {
                Baixar::Rede(detalhe) => Erro::Rede(detalhe),
                Baixar::Diferente => Erro::Arquivo { versao: versao.clone() },
                Baixar::Disco(erro) => instalacao(erro.to_string()),
            });
        }
        fs::rename(&parte, &destino)
            .and_then(|()| fs::File::open(&baixando)?.sync_all())
            .map_err(|e| instalacao(e.to_string()))?;
    }
    avisar(Estado::Instalando { versao: versao.clone() });
    let prefixo = pacote.nome.trim_end_matches(".tar.gz");
    let instalada = instalador::instalar(&app, Origem::Pacote { arquivo: &destino, prefixo }, &versao, amb.rodando.as_deref(), &trava);
    let _ = fs::remove_file(&destino);
    match instalada {
        Ok(_) => {}
        Err(Falha::Binario(erro)) => return Err(Erro::Binario { versao: versao.clone(), detalhe: format!("{erro:#}") }),
        Err(falha) => return Err(instalacao(falha.to_string())),
    }
    // O .desktop aponta para o link: só muda na primeira vez (o build do PC que vira app).
    let _ = instalador::integrar(raiz, amb.caches);
    Ok(())
}

/// O pacote, a pasta aberta e a versão anterior, com folga.
fn cabe(livre: u64, tamanho: u64) -> bool {
    livre >= tamanho.saturating_mul(3).saturating_add(FOLGA_DE_ESPACO)
}

/// Abre a thread `vitral-atualizacao`. A primeira procura sai 60 s depois de abrir (nunca antes de
/// 1 h da última que deu certo); depois, a cada 6 h com folga, ou 1 h depois de um erro de rede.
/// Pronta, só procura de novo se pedirem.
pub fn abrir(
    amb: Ambiente,
    caminho: Option<PathBuf>,
    inicial: Estado,
    saida: smol::channel::Sender<Noticia>,
    acompanhar: Arc<AtomicBool>,
) -> io::Result<mpsc::Sender<Comando>> {
    let (comandos, recebe) = mpsc::channel();
    std::thread::Builder::new()
        .name("vitral-atualizacao".into())
        .spawn(move || laco(amb, caminho, inicial, recebe, saida, acompanhar))?;
    Ok(comandos)
}

fn laco(
    amb: Ambiente,
    caminho: Option<PathBuf>,
    mut estado: Estado,
    comandos: mpsc::Receiver<Comando>,
    saida: smol::channel::Sender<Noticia>,
    acompanhar: Arc<AtomicBool>,
) {
    baixar_prioridade();
    if let Some(raiz) = &amb.raiz {
        let app = raiz.app();
        if app.is_dir()
            && let Ok(Some(trava)) = instalador::travar(&app)
        {
            instalador::limpar_restos(&app, &trava);
        }
        // Toda abertura da versão atual confere o menu (os ícones podem ter mudado nela).
        if amb.rodando.is_some() && amb.rodando == instalador::versao_atual(&app) {
            let _ = instalador::integrar(raiz, amb.caches);
        }
    }
    let mut config = caminho.as_deref().map(Config::ler).unwrap_or_default();
    let agendar = |config: &Config, estado: &Estado, depois: Duration| {
        (config.modo != Modo::Desligadas && !matches!(estado, Estado::Pronta(_))).then(|| Instant::now() + depois)
    };
    let mut proxima = agendar(&config, &estado, config::primeira_espera(agora(), config.ultima_procura));
    loop {
        let recebido = match proxima {
            Some(quando) => comandos.recv_timeout(quando.saturating_duration_since(Instant::now())),
            None => comandos.recv().map_err(|_| mpsc::RecvTimeoutError::Disconnected),
        };
        let pedido = match recebido {
            Ok(Comando::Procurar) => Pedido::Procurar,
            Ok(Comando::Instalar) => Pedido::Instalar,
            Ok(Comando::Modo(modo)) => {
                config.modo = modo;
                gravar(&caminho, &config);
                if modo == Modo::Automaticas && matches!(estado, Estado::Disponivel(_)) {
                    Pedido::Procurar
                } else {
                    if proxima.is_none() || modo == Modo::Desligadas {
                        proxima = agendar(&config, &estado, config::primeira_espera(agora(), config.ultima_procura));
                    }
                    continue;
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Pedido::Sozinho,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let mut ultimo_progresso: Option<Instant> = None;
        let ultima_procura = config.ultima_procura;
        let antes = proxima.map(relogio);
        estado = rodar(&amb, &mut config, pedido, agora(), &mut |meio| {
            if let Estado::Baixando { feito, total, .. } = &meio
                && *feito != 0
                && *feito != *total
            {
                let cedo = ultimo_progresso.is_some_and(|antes| antes.elapsed() < PASSO_DO_PROGRESSO);
                if cedo || !acompanhar.load(Ordering::Relaxed) {
                    return;
                }
                ultimo_progresso = Some(Instant::now());
            }
            let _ = saida.send_blocking(Noticia { estado: meio, ultima_procura, proxima: antes });
        });
        gravar(&caminho, &config);
        let rede = matches!(estado, Estado::Erro(Erro::Rede(_) | Erro::Ocupado));
        proxima = agendar(&config, &estado, config::espera_depois(rede, folga()));
        let noticia = Noticia { estado: estado.clone(), ultima_procura: config.ultima_procura, proxima: proxima.map(relogio) };
        if saida.send_blocking(noticia).is_err() {
            return;
        }
    }
}

/// O instante da próxima procura em segundos desde 1970 (o que a interface mostra).
fn relogio(quando: Instant) -> u64 {
    agora() + quando.saturating_duration_since(Instant::now()).as_secs()
}

fn gravar(caminho: &Option<PathBuf>, config: &Config) {
    if let Some(caminho) = caminho
        && let Err(erro) = config.gravar(caminho)
    {
        eprintln!("vitral: não gravei {}: {erro}", caminho.display());
    }
}

/// `nice 10` só nesta thread (no Linux, o tid vale como processo para o `setpriority`); o
/// `--versao` do binário novo herda.
#[cfg(target_os = "linux")]
fn baixar_prioridade() {
    // SAFETY: chamadas sem ponteiros; o pior caso é não mudar a prioridade.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, libc::gettid() as libc::id_t, 10);
    }
}

/// Fora do Linux a procura ainda não roda (`iniciar`).
#[cfg(not(target_os = "linux"))]
fn baixar_prioridade() {}

fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

fn folga() -> Duration {
    Duration::from_secs((uuid::Uuid::new_v4().as_u128() % (config::FOLGA.as_secs() as u128 + 1)) as u64)
}

#[cfg(all(test, target_os = "linux"))]
mod testes {
    use super::*;
    use crate::atualizacao::linux::{self, testes::layout_com};
    use std::{
        collections::HashMap,
        path::Path,
        sync::Mutex,
    };

    const MANIFESTO: &[u8] = include_bytes!("teste/ponta/vitral-versao.json");
    const ASSINATURA: &[u8] = include_bytes!("teste/ponta/vitral-versao.json.minisig");
    const PACOTE: &[u8] = include_bytes!("teste/ponta/vitral-0.9.0-linux-x86_64.tar.gz");
    const NOME: &str = "vitral-0.9.0-linux-x86_64.tar.gz";
    const ETAG: &str = "\"e1\"";

    /// O release 0.9.0 (assinado com a chave de teste da ponta a ponta), servido em
    /// `http://127.0.0.1:<porta>/r/`. Responde 304 ao ETag e guarda os pedidos.
    struct ReleaseFalso {
        base: String,
        arquivos: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        pedidos: Arc<Mutex<Vec<String>>>,
        servidor: Arc<tiny_http::Server>,
    }

    impl ReleaseFalso {
        fn novo() -> ReleaseFalso {
            let servidor = Arc::new(tiny_http::Server::http("127.0.0.1:0").unwrap());
            let porta = servidor.server_addr().to_ip().unwrap().port();
            let arquivos = Arc::new(Mutex::new(HashMap::from([
                ("vitral-versao.json".to_string(), MANIFESTO.to_vec()),
                ("vitral-versao.json.minisig".to_string(), ASSINATURA.to_vec()),
                (NOME.to_string(), PACOTE.to_vec()),
            ])));
            let pedidos = Arc::new(Mutex::new(Vec::new()));
            let (s, a, p) = (servidor.clone(), arquivos.clone(), pedidos.clone());
            std::thread::spawn(move || {
                for pedido in s.incoming_requests() {
                    let nome = pedido.url().trim_start_matches("/r/").to_string();
                    p.lock().unwrap().push(nome.clone());
                    let etag = pedido.headers().iter().find(|h| h.field.equiv("If-None-Match")).map(|h| h.value.to_string());
                    let resposta = match a.lock().unwrap().get(&nome) {
                        _ if nome == "vitral-versao.json" && etag.as_deref() == Some(ETAG) => tiny_http::Response::from_data(Vec::new()).with_status_code(304),
                        Some(bytes) => tiny_http::Response::from_data(bytes.clone())
                            .with_header(tiny_http::Header::from_bytes("ETag", ETAG).unwrap()),
                        None => tiny_http::Response::from_data(Vec::new()).with_status_code(404),
                    };
                    let _ = pedido.respond(resposta);
                }
            });
            ReleaseFalso { base: format!("http://127.0.0.1:{porta}/r"), arquivos, pedidos, servidor }
        }

        fn trocar(&self, nome: &str, bytes: Option<Vec<u8>>) {
            let mut arquivos = self.arquivos.lock().unwrap();
            match bytes {
                Some(bytes) => arquivos.insert(nome.to_string(), bytes),
                None => arquivos.remove(nome),
            };
        }

        fn pedidos(&self) -> Vec<String> {
            std::mem::take(&mut *self.pedidos.lock().unwrap())
        }
    }

    impl Drop for ReleaseFalso {
        fn drop(&mut self) {
            self.servidor.unblock();
        }
    }

    fn chave_de_teste(dados: &[u8], minisig: &str) -> anyhow::Result<String> {
        let assinatura = minisign_verify::Signature::decode(minisig)?;
        let chave = minisign_verify::PublicKey::decode(include_str!("teste/ponta/teste.pub"))?;
        chave.verify(dados, &assinatura, false)?;
        Ok(assinatura.trusted_comment().to_string())
    }

    /// A 0.8.0 instalada e rodando da pasta do app, com o release 0.9.0 publicado.
    fn ambiente(release: &ReleaseFalso, instala_sozinho: bool) -> (PathBuf, Ambiente) {
        let (pasta, _app) = layout_com("0.8.0");
        let raiz = Raiz { dados: pasta.join("dados"), bin: None };
        let amb = Ambiente {
            fonte: Fonte::na_base(&release.base),
            rede: Rede::nova("Vitral/teste"),
            conferir: chave_de_teste,
            versao: Version::new(0, 8, 0),
            plataforma: "linux-x86_64".into(),
            raiz: Some(raiz),
            rodando: Some("0.8.0".into()),
            instala_sozinho,
            aceita_pre: false,
            caches: false,
        };
        (pasta, amb)
    }

    fn app(amb: &Ambiente) -> PathBuf {
        amb.raiz.as_ref().unwrap().app()
    }

    fn volta(amb: &Ambiente, config: &mut Config, pedido: Pedido) -> (Estado, Vec<Estado>) {
        let mut meio = Vec::new();
        let fim = rodar(amb, config, pedido, 1_800_000_000, &mut |e| meio.push(e));
        (fim, meio)
    }

    fn nomes(pasta: &Path) -> Vec<String> {
        let mut nomes: Vec<_> = fs::read_dir(pasta).map_or(Vec::new(), |d| d.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect());
        nomes.sort();
        nomes
    }

    #[test]
    fn de_ponta_a_ponta_procura_confere_baixa_instala_e_fica_pronta() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let mut config = Config::default();
        let (fim, meio) = volta(&amb, &mut config, Pedido::Sozinho);
        let Estado::Pronta(novidade) = &fim else { panic!("{fim:?}") };
        assert_eq!(novidade.versao, "0.9.0");
        assert_eq!(novidade.notas, "Versão de teste do atualizador.");
        assert_eq!(novidade.pagina, "https://github.com/VictorMenegate/vitral/releases/tag/v0.9.0");
        assert_eq!(meio.first(), Some(&Estado::Procurando));
        assert!(meio.contains(&Estado::Baixando { versao: "0.9.0".into(), feito: 0, total: PACOTE.len() as u64 }));
        assert!(meio.contains(&Estado::Baixando { versao: "0.9.0".into(), feito: PACOTE.len() as u64, total: PACOTE.len() as u64 }));
        assert_eq!(meio.last(), Some(&Estado::Instalando { versao: "0.9.0".into() }));
        assert_eq!(release.pedidos(), ["vitral-versao.json", "vitral-versao.json.minisig", NOME]);

        let app = app(&amb);
        assert_eq!(linux::versao_atual(&app).as_deref(), Some("0.9.0"));
        assert_eq!(linux::versao_do_link(&app, "anterior").as_deref(), Some("0.8.0"));
        crate::atualizacao::pacote::conferir_binario(&app.join("atual/vitral"), "0.9.0", Duration::from_secs(10)).unwrap();
        assert!(nomes(&app.join(".baixando")).is_empty(), "o pacote sai depois de instalar");
        assert!(pasta.join("dados/applications/vitral.desktop").is_file());
        assert_eq!(config.ultima_procura, Some(1_800_000_000));
        assert!(config.falhas.is_empty() && config.etag.is_none());

        // Outra volta não baixa de novo: já está pronta, esperando o reinício.
        let (fim, _) = volta(&amb, &mut config, Pedido::Sozinho);
        assert!(matches!(fim, Estado::Pronta(_)), "{fim:?}");
        assert_eq!(release.pedidos(), ["vitral-versao.json", "vitral-versao.json.minisig"]);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn so_avisar_e_build_do_pc_nao_baixam_ate_pedirem() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let mut config = Config { modo: Modo::SoAvisar, ..Config::default() };
        let (fim, _) = volta(&amb, &mut config, Pedido::Sozinho);
        assert!(matches!(&fim, Estado::Disponivel(n) if n.versao == "0.9.0" && !n.pela_pagina), "{fim:?}");
        assert!(!release.pedidos().contains(&NOME.to_string()));
        assert_eq!(linux::versao_atual(&app(&amb)).as_deref(), Some("0.8.0"));

        // O build do PC só avisa, mesmo no modo Automáticas; o "Instalar como app" instala.
        let amb_pc = Ambiente { instala_sozinho: false, rodando: None, ..amb };
        let mut config = Config::default();
        let (fim, _) = volta(&amb_pc, &mut config, Pedido::Procurar);
        assert!(matches!(&fim, Estado::Disponivel(_)), "{fim:?}");
        assert!(!release.pedidos().contains(&NOME.to_string()));
        let (fim, _) = volta(&amb_pc, &mut config, Pedido::Instalar);
        assert!(matches!(&fim, Estado::Pronta(n) if n.versao == "0.9.0"), "{fim:?}");
        assert!(release.pedidos().contains(&NOME.to_string()));
        assert_eq!(linux::versao_atual(&app(&amb_pc)).as_deref(), Some("0.9.0"));
        // Sem versão rodando da pasta, a anterior é a que era a atual.
        assert_eq!(linux::versao_do_link(&app(&amb_pc), "anterior").as_deref(), Some("0.8.0"));
        // A próxima procura do build do PC volta a só avisar (o app instalado é outra coisa).
        let (fim, _) = volta(&amb_pc, &mut config, Pedido::Procurar);
        assert!(matches!(&fim, Estado::Disponivel(_)), "{fim:?}");
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn em_dia_guarda_o_etag_e_o_304_nao_baixa_a_assinatura() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let amb = Ambiente { versao: Version::new(0, 9, 0), rodando: Some("0.9.0".into()), ..amb };
        let mut config = Config::default();
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::EmDia);
        assert_eq!(config.etag.as_deref(), Some(ETAG));
        assert_eq!(config.etag_de.as_deref(), Some("0.9.0"));
        release.pedidos();
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::EmDia);
        assert_eq!(release.pedidos(), ["vitral-versao.json"]);
        // Outra versão rodando não confia no ETag de antes.
        let amb = Ambiente { versao: Version::new(0, 8, 0), ..amb };
        let (fim, _) = volta(&amb, &mut config, Pedido::Sozinho);
        assert!(matches!(fim, Estado::Pronta(_)), "{fim:?}");
        assert_eq!(release.pedidos(), ["vitral-versao.json", "vitral-versao.json.minisig", NOME]);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn versao_igual_ou_menor_nao_instala() {
        let release = ReleaseFalso::novo();
        for rodando in [Version::new(0, 9, 0), Version::new(1, 0, 0)] {
            let (pasta, amb) = ambiente(&release, true);
            let amb = Ambiente { versao: rodando.clone(), ..amb };
            assert_eq!(volta(&amb, &mut Config::default(), Pedido::Sozinho).0, Estado::EmDia, "{rodando}");
            assert_eq!(volta(&amb, &mut Config::default(), Pedido::Instalar).0, Estado::EmDia, "{rodando}");
            assert_eq!(linux::versao_atual(&app(&amb)).as_deref(), Some("0.8.0"));
            fs::remove_dir_all(&pasta).unwrap();
        }
        assert!(!release.pedidos().contains(&NOME.to_string()));
    }

    #[test]
    fn pacote_com_soma_ou_tamanho_errado_nao_entra_e_desiste_na_terceira() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let mut trocado = PACOTE.to_vec();
        trocado[PACOTE.len() / 2] ^= 1;
        let mut config = Config::default();
        for (bytes, tentativa) in [(trocado, 1), (PACOTE[..PACOTE.len() - 10].to_vec(), 2), ([PACOTE, b"mais"].concat(), 3)] {
            release.trocar(NOME, Some(bytes));
            let (fim, _) = volta(&amb, &mut config, Pedido::Sozinho);
            assert_eq!(fim, Estado::Erro(Erro::Arquivo { versao: "0.9.0".into() }), "tentativa {tentativa}");
            assert_eq!(config.falhas.get("0.9.0"), Some(&tentativa));
            assert!(nomes(&app(&amb).join(".baixando")).is_empty(), "o .parte sai");
            assert_eq!(linux::versao_atual(&app(&amb)).as_deref(), Some("0.8.0"));
        }
        // Na quarta nem baixa; a pessoa ainda pode pedir.
        release.trocar(NOME, Some(PACOTE.to_vec()));
        release.pedidos();
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::Erro(Erro::Desistiu { versao: "0.9.0".into() }));
        assert!(!release.pedidos().contains(&NOME.to_string()));
        assert!(matches!(volta(&amb, &mut config, Pedido::Instalar).0, Estado::Pronta(_)));
        assert!(config.falhas.is_empty());
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn assinatura_errada_ou_ausente_e_recusada_sem_baixar() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let mut config = Config::default();
        let mut trocado = MANIFESTO.to_vec();
        let i = trocado.iter().position(|&b| b == b'9').unwrap();
        trocado[i] = b'8';
        release.trocar("vitral-versao.json", Some(trocado));
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::Erro(Erro::Assinatura));
        release.trocar("vitral-versao.json", Some(MANIFESTO.to_vec()));
        release.trocar("vitral-versao.json.minisig", None);
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::Erro(Erro::Assinatura));
        // A chave do app não reconhece a de teste.
        release.trocar("vitral-versao.json.minisig", Some(ASSINATURA.to_vec()));
        let amb = Ambiente { conferir: assinatura::conferir, ..amb };
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::Erro(Erro::Assinatura));
        assert!(!release.pedidos().contains(&NOME.to_string()));
        assert_eq!(config.ultima_procura, None, "procura recusada não conta");
        assert_eq!(linux::versao_atual(&app(&amb)).as_deref(), Some("0.8.0"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn sem_conexao_ou_sem_release_publicado() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let mut config = Config::default();
        release.trocar("vitral-versao.json", None);
        assert_eq!(volta(&amb, &mut config, Pedido::Sozinho).0, Estado::EmDia, "404: nada publicado");
        assert_eq!(config.ultima_procura, Some(1_800_000_000));
        // Porta fechada.
        let fechado = Ambiente { fonte: Fonte::na_base("http://127.0.0.1:9/r"), ..amb };
        let mut config = Config::default();
        assert!(matches!(volta(&fechado, &mut config, Pedido::Sozinho).0, Estado::Erro(Erro::Rede(_))));
        assert_eq!(config.ultima_procura, None);
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn instalada_por_outra_abertura_segue_pronta_sem_o_release() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        assert!(matches!(volta(&amb, &mut Config::default(), Pedido::Sozinho).0, Estado::Pronta(_)));
        release.trocar("vitral-versao.json", None);
        let (fim, _) = volta(&amb, &mut Config::default(), Pedido::Procurar);
        assert!(matches!(&fim, Estado::Pronta(n) if n.versao == "0.9.0"), "{fim:?}");
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn binario_que_nao_abre_no_teste_nao_entra() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        // Um pacote 0.9.0 cujo binário diz outra versão (manifesto e soma batem: foi publicado assim).
        let pacote_ruim = crate::atualizacao::pacote::testes::pacote_bom("vitral-0.9.0-linux-x86_64", "0.7.0");
        let gerado = gerar_manifesto(&pacote_ruim);
        release.trocar(NOME, Some(pacote_ruim));
        let amb = Ambiente { conferir: |_, _| Ok("vitral 0.9.0 vitral-versao.json".into()), ..amb };
        release.trocar("vitral-versao.json", Some(gerado));
        let mut config = Config::default();
        let (fim, _) = volta(&amb, &mut config, Pedido::Sozinho);
        assert!(matches!(&fim, Estado::Erro(Erro::Binario { versao, .. }) if versao == "0.9.0"), "{fim:?}");
        assert_eq!(config.falhas.get("0.9.0"), Some(&1));
        let app = app(&amb);
        assert_eq!(linux::versao_atual(&app).as_deref(), Some("0.8.0"));
        assert!(nomes(&app).iter().all(|n| !n.starts_with(".novo-")), "{:?}", nomes(&app));
        fs::remove_dir_all(&pasta).unwrap();
    }

    /// O manifesto da ponta a ponta com o tamanho e a soma de outro pacote (a assinatura é trocada
    /// por uma conferência que aceita tudo).
    fn gerar_manifesto(pacote: &[u8]) -> Vec<u8> {
        use sha2::{Digest as _, Sha256};
        let mut valor: serde_json::Value = serde_json::from_slice(MANIFESTO).unwrap();
        valor["arquivos"]["linux-x86_64"]["tamanho"] = (pacote.len() as u64).into();
        valor["arquivos"]["linux-x86_64"]["sha256"] = Sha256::digest(pacote).iter().map(|b| format!("{b:02x}")).collect::<String>().into();
        serde_json::to_vec(&valor).unwrap()
    }

    #[test]
    fn com_outro_instalando_nao_baixa() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let trava = linux::travar(&app(&amb)).unwrap().unwrap();
        assert_eq!(volta(&amb, &mut Config::default(), Pedido::Sozinho).0, Estado::Erro(Erro::Ocupado));
        assert!(!release.pedidos().contains(&NOME.to_string()));
        drop(trava);
        assert!(matches!(volta(&amb, &mut Config::default(), Pedido::Sozinho).0, Estado::Pronta(_)));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn so_baixa_com_tres_vezes_o_pacote_livre() {
        let mb = 1024 * 1024;
        assert!(cabe(160 * mb, 20 * mb));
        assert!(!cabe(159 * mb, 20 * mb));
        assert!(!cabe(u64::MAX - 1, u64::MAX / 2));
        assert!(!cabe(0, 1));
    }

    #[test]
    fn formato_novo_do_manifesto_so_avisa_pela_pagina() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let mut valor: serde_json::Value = serde_json::from_slice(MANIFESTO).unwrap();
        valor["formato"] = 2.into();
        release.trocar("vitral-versao.json", Some(serde_json::to_vec(&valor).unwrap()));
        let amb = Ambiente { conferir: |_, _| Ok("vitral 0.9.0 vitral-versao.json".into()), ..amb };
        let (fim, _) = volta(&amb, &mut Config::default(), Pedido::Instalar);
        assert!(matches!(&fim, Estado::Disponivel(n) if n.pela_pagina && n.versao == "0.9.0"), "{fim:?}");
        assert!(!release.pedidos().contains(&NOME.to_string()));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn a_fonte_do_github_monta_o_endereco_do_pacote() {
        let fonte = Fonte::do_github();
        assert_eq!(fonte.manifesto, "https://github.com/VictorMenegate/vitral/releases/latest/download/vitral-versao.json");
        assert_eq!(fonte.assinatura, "https://github.com/VictorMenegate/vitral/releases/latest/download/vitral-versao.json.minisig");
        assert_eq!(
            fonte.pacote(&Version::new(0, 3, 0), "vitral-0.3.0-linux-x86_64.tar.gz"),
            "https://github.com/VictorMenegate/vitral/releases/download/v0.3.0/vitral-0.3.0-linux-x86_64.tar.gz"
        );
        let base = Fonte::na_base("https://github.com/VictorMenegate/vitral/releases/download/v0.3.0-rc.2/");
        assert_eq!(base.manifesto, "https://github.com/VictorMenegate/vitral/releases/download/v0.3.0-rc.2/vitral-versao.json");
        assert_eq!(
            base.pacote(&Version::parse("0.3.0-rc.2").unwrap(), "x.tar.gz"),
            "https://github.com/VictorMenegate/vitral/releases/download/v0.3.0-rc.2/x.tar.gz"
        );
    }

    #[test]
    fn a_thread_procura_sozinha_e_conta_a_interface() {
        let release = ReleaseFalso::novo();
        let (pasta, amb) = ambiente(&release, true);
        let caminho = pasta.join("config/atualizacoes.json");
        // A última procura foi há mais de 1 h; a primeira espera é de 60 s, então pede na mão.
        let (envia, recebe) = smol::channel::unbounded();
        let comandos = abrir(amb, Some(caminho.clone()), Estado::Parado, envia, Arc::new(AtomicBool::new(true))).unwrap();
        comandos.send(Comando::Procurar).unwrap();
        let mut vistos = Vec::new();
        let fim = loop {
            let noticia = smol::block_on(async {
                smol::future::or(async { recebe.recv().await.ok() }, async {
                    smol::Timer::after(Duration::from_secs(30)).await;
                    None
                })
                .await
            })
            .expect("a thread não respondeu em 30 s");
            vistos.push(noticia.estado.clone());
            if !matches!(noticia.estado, Estado::Procurando | Estado::Baixando { .. } | Estado::Instalando { .. }) {
                break noticia;
            }
        };
        assert!(matches!(&fim.estado, Estado::Pronta(n) if n.versao == "0.9.0"), "{vistos:?}");
        assert!(fim.ultima_procura.is_some());
        let gravada = Config::ler(&caminho);
        assert_eq!(gravada.ultima_procura, fim.ultima_procura);
        // Mudar o modo grava, sem procurar.
        comandos.send(Comando::Modo(Modo::Desligadas)).unwrap();
        let limite = Instant::now() + Duration::from_secs(10);
        while Config::ler(&caminho).modo != Modo::Desligadas {
            assert!(Instant::now() < limite, "o modo não foi gravado");
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(comandos);
        fs::remove_dir_all(&pasta).unwrap();
    }
}
