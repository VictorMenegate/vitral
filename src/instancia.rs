//! Um Vitral só por usuário: a trava (`File::try_lock` em `~/.local/share/vitral.trava`, fora da
//! pasta de dados) e o canal por onde uma segunda abertura entrega o pedido (`vitral <pasta>`,
//! `--nova`) ao Vitral que já está aberto e sai sem abrir janela. A trava nunca espera: com ela
//! ocupada, quem chegou depois só tenta entregar o pedido e sai.
//!
//! O canal é um socket Unix numa pasta só do dono. No Windows, uma porta TCP em 127.0.0.1 com uma
//! senha nova a cada abertura: as duas vão no arquivo do canal (na pasta local do usuário, que
//! outro usuário do PC não lê), e um pedido sem a senha é recusado.

#[cfg(windows)]
use std::net::{Ipv4Addr, TcpListener, TcpStream};
#[cfg(unix)]
use std::os::unix::{
    fs::{DirBuilderExt as _, PermissionsExt as _},
    net::{UnixListener, UnixStream},
};
use std::{
    ffi::OsStr,
    fs, io,
    io::{BufRead as _, BufReader, Write as _},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use smol::channel::{Receiver, Sender, unbounded};

/// O que pedir ao abrir: uma pasta (`vitral <pasta>`) e um chat novo (`--nova destino [mensagem]`).
/// É a mesma coisa que vai pelo canal, numa linha de JSON.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Abertura {
    pub pasta: Option<PathBuf>,
    pub nova: Option<(String, Option<String>)>,
}

impl Abertura {
    /// Dos argumentos (sem o binário): a primeira palavra que não é opção é a pasta (relativa a
    /// `atual`; inválida, só avisa e fica de fora) e `--nova` leva o destino e, se vier, a mensagem.
    pub fn de_args(args: &[String], atual: &Path) -> Abertura {
        let nova_em = args.iter().position(|a| a == "--nova");
        let antes = &args[..nova_em.unwrap_or(args.len())];
        let pasta = antes.iter().find(|a| !a.starts_with("--")).and_then(|a| {
            let caminho = atual.join(a);
            match dunce::canonicalize(&caminho) {
                Ok(c) if c.is_dir() => Some(c),
                _ => {
                    eprintln!("vitral: {a} não é uma pasta; segui sem ela");
                    None
                }
            }
        });
        let nova = nova_em.and_then(|i| {
            let destino = args.get(i + 1)?.trim().to_string();
            (!destino.is_empty()).then(|| (destino, args.get(i + 2).cloned().filter(|m| !m.trim().is_empty())))
        });
        Abertura { pasta, nova }
    }

    pub fn vazia(&self) -> bool {
        self.pasta.is_none() && self.nova.is_none()
    }
}

/// Enquanto existir, este é o Vitral aberto.
pub struct Trava {
    _arquivo: fs::File,
}

/// No Windows, o Vitral que reabre este (Tentar de novo) vem com isso: a trava ainda é do que
/// está saindo, então espera por ela em vez de entregar o pedido. `processos::novo_comando` tira
/// a variável de todo outro processo.
#[cfg(windows)]
pub const REABRINDO: &str = "VITRAL_REABRINDO";

/// `None`: outro Vitral já está com a trava. No Linux, o arquivo abre com `O_CLOEXEC` (o padrão
/// do Rust): num exec (Tentar de novo) a trava sai junto.
pub fn travar(caminho: &Path) -> io::Result<Option<Trava>> {
    if let Some(pasta) = caminho.parent() {
        fs::create_dir_all(pasta)?;
    }
    let arquivo = fs::OpenOptions::new().create(true).truncate(false).write(true).open(caminho)?;
    #[cfg(windows)]
    let limite = Instant::now() + if std::env::var_os(REABRINDO).is_some() { Duration::from_secs(10) } else { Duration::ZERO };
    loop {
        match arquivo.try_lock() {
            Ok(()) => return Ok(Some(Trava { _arquivo: arquivo })),
            #[cfg(windows)]
            Err(fs::TryLockError::WouldBlock) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(50)),
            Err(fs::TryLockError::WouldBlock) => return Ok(None),
            Err(fs::TryLockError::Error(erro)) => return Err(erro),
        }
    }
}

/// O que a thread do canal responde a uma linha: "ok" (o pedido foi para a janela) ou "?" (não
/// entendeu, ou veio sem a senha). `None` quando ninguém mais lê os pedidos.
fn responder(linha: &str, senha: &str, envia: &Sender<Abertura>) -> Option<&'static str> {
    let Some(json) = linha.trim().strip_prefix(senha) else {
        return Some("?\n");
    };
    match serde_json::from_str::<Abertura>(json.trim_start()) {
        Ok(abertura) => envia.send_blocking(abertura).is_ok().then_some("ok\n"),
        Err(_) => Some("?\n"),
    }
}

/// Abre o socket (a pasta com 0700; um socket velho, de um Vitral que caiu, sai antes) e uma
/// thread que recebe os pedidos e os guarda até alguém ler: a janela ainda nem existe.
#[cfg(unix)]
pub fn ouvir(socket: &Path) -> io::Result<Receiver<Abertura>> {
    if let Some(pasta) = socket.parent() {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(pasta)?;
        // Já existindo, fica só para o dono também.
        fs::set_permissions(pasta, fs::Permissions::from_mode(0o700))?;
    }
    match fs::remove_file(socket) {
        Err(erro) if erro.kind() != io::ErrorKind::NotFound => return Err(erro),
        _ => {}
    }
    let ouvinte = UnixListener::bind(socket)?;
    let (envia, recebe) = unbounded();
    std::thread::spawn(move || {
        for conexao in ouvinte.incoming().flatten() {
            let _ = conexao.set_read_timeout(Some(Duration::from_secs(2)));
            let mut linha = String::new();
            if BufReader::new(&conexao).read_line(&mut linha).is_err() {
                continue;
            }
            let Some(resposta) = responder(&linha, "", &envia) else { return };
            let _ = (&conexao).write_all(resposta.as_bytes());
        }
    });
    Ok(recebe)
}

/// Abre a porta em 127.0.0.1 (o Firewall do Windows não pergunta nada por ela), grava a porta e a
/// senha no arquivo do canal e abre a thread que recebe os pedidos, como no Linux.
#[cfg(windows)]
pub fn ouvir(arquivo: &Path) -> io::Result<Receiver<Abertura>> {
    let ouvinte = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
    let senha = uuid::Uuid::new_v4().simple().to_string();
    let porta = ouvinte.local_addr()?.port();
    crate::dados::gravar_texto(arquivo, &format!("{porta} {senha}\n")).map_err(io::Error::other)?;
    let (envia, recebe) = unbounded();
    std::thread::spawn(move || {
        for conexao in ouvinte.incoming().flatten() {
            let _ = conexao.set_read_timeout(Some(Duration::from_secs(2)));
            let mut linha = String::new();
            if BufReader::new(&conexao).read_line(&mut linha).is_err() {
                continue;
            }
            let Some(resposta) = responder(&linha, &senha, &envia) else { return };
            let _ = (&conexao).write_all(resposta.as_bytes());
        }
    });
    Ok(recebe)
}

/// A segunda abertura: tenta entregar o pedido ao Vitral aberto a cada 200 ms, até o prazo (ele
/// pode estar no meio da migração, antes de abrir o canal).
pub fn entregar(canal: &Path, abertura: &Abertura, prazo: Duration) -> io::Result<()> {
    let json = serde_json::to_string(abertura)?;
    let fim = Instant::now() + prazo;
    loop {
        let tentativa = enviar(canal, &json).and_then(|resposta| {
            if resposta.trim() == "ok" {
                Ok(())
            } else {
                Err(io::Error::other("o Vitral aberto não entendeu o pedido"))
            }
        });
        match tentativa {
            Ok(()) => return Ok(()),
            Err(erro) if Instant::now() >= fim => return Err(erro),
            Err(_) => std::thread::sleep(Duration::from_millis(200)),
        }
    }
}

/// Uma tentativa de entrega: manda a linha e devolve a resposta.
#[cfg(unix)]
fn enviar(socket: &Path, json: &str) -> io::Result<String> {
    let mut conexao = UnixStream::connect(socket)?;
    conexao.set_read_timeout(Some(Duration::from_secs(2)))?;
    conexao.write_all(format!("{json}\n").as_bytes())?;
    let mut resposta = String::new();
    BufReader::new(&conexao).read_line(&mut resposta)?;
    Ok(resposta)
}

/// Uma tentativa de entrega: lê de novo a porta e a senha (o Vitral aberto pode ter acabado de
/// gravá-las), manda a linha com a senha na frente e devolve a resposta.
#[cfg(windows)]
fn enviar(arquivo: &Path, json: &str) -> io::Result<String> {
    let texto = fs::read_to_string(arquivo)?;
    let (porta, senha) = texto.trim().split_once(' ').ok_or_else(|| io::Error::other("arquivo do canal ilegível"))?;
    let porta: u16 = porta.parse().map_err(io::Error::other)?;
    let endereco = (Ipv4Addr::LOCALHOST, porta).into();
    let mut conexao = TcpStream::connect_timeout(&endereco, Duration::from_secs(2))?;
    conexao.set_read_timeout(Some(Duration::from_secs(2)))?;
    conexao.write_all(format!("{senha} {json}\n").as_bytes())?;
    let mut resposta = String::new();
    BufReader::new(&conexao).read_line(&mut resposta)?;
    Ok(resposta)
}

/// Abre `binario` com `argumentos` no lugar deste processo. A trava e o socket fecham no exec (os
/// arquivos abrem com `O_CLOEXEC`); só volta com o erro.
#[cfg(unix)]
pub fn trocar_por<A: AsRef<OsStr>>(binario: &Path, argumentos: impl IntoIterator<Item = A>) -> io::Error {
    use std::os::unix::process::CommandExt as _;
    crate::processos::novo_comando(binario).args(argumentos).exec()
}

/// No Windows não há exec: abre o novo, que espera a trava deste (`REABRINDO`), e sai. Só volta
/// com o erro.
#[cfg(windows)]
pub fn trocar_por<A: AsRef<OsStr>>(binario: &Path, argumentos: impl IntoIterator<Item = A>) -> io::Error {
    match crate::processos::novo_comando(binario).args(argumentos).env(REABRINDO, "1").spawn() {
        Ok(_) => std::process::exit(0),
        Err(erro) => erro,
    }
}

/// Abre o binário de novo no lugar deste processo, com os mesmos argumentos; só volta com o erro.
/// No Linux, pelo `/proc/self/exe`: é este binário mesmo que o arquivo tenha sido trocado.
pub fn reexecutar() -> io::Error {
    let binario = if cfg!(target_os = "linux") { Ok(PathBuf::from("/proc/self/exe")) } else { std::env::current_exe() };
    match binario {
        Ok(binario) => trocar_por(&binario, std::env::args_os().skip(1)),
        Err(erro) => erro,
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::novo_id;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-inst-{nome}-{}", novo_id()));
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    #[test]
    fn travar_duas_vezes_a_segunda_falha_e_depois_de_soltar_funciona() {
        let pasta = pasta_de_teste("trava");
        let caminho = pasta.join("vitral.trava");
        let primeira = travar(&caminho).unwrap();
        assert!(primeira.is_some());
        assert!(travar(&caminho).unwrap().is_none());
        drop(primeira);
        assert!(travar(&caminho).unwrap().is_some());
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn a_segunda_abertura_entrega_o_pedido() {
        let pasta = pasta_de_teste("socket");
        let socket = pasta.join("run").join("vitral.sock");
        // Um canal velho, de um Vitral que caiu, não atrapalha.
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        fs::write(&socket, "").unwrap();
        let pedidos = ouvir(&socket).unwrap();
        let abertura = Abertura { pasta: Some(pasta.clone()), nova: Some(("loja".into(), Some("oi".into()))) };
        entregar(&socket, &abertura, Duration::from_secs(5)).unwrap();
        assert_eq!(pedidos.recv_blocking().unwrap(), abertura);
        #[cfg(unix)]
        {
            let modo = fs::metadata(socket.parent().unwrap()).unwrap().permissions().mode();
            assert_eq!(modo & 0o777, 0o700);
        }
        // Ninguém ouvindo: desiste no prazo.
        drop(pedidos);
        let sem = pasta.join("ninguem.sock");
        assert!(entregar(&sem, &Abertura::default(), Duration::from_millis(300)).is_err());
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn sem_a_senha_o_pedido_nao_entra() {
        let (envia, recebe) = unbounded();
        let json = serde_json::to_string(&Abertura { pasta: None, nova: Some(("loja".into(), None)) }).unwrap();
        assert_eq!(responder(&format!("abc {json}\n"), "abc", &envia), Some("ok\n"));
        assert_eq!(recebe.try_recv().unwrap().nova, Some(("loja".into(), None)));
        assert_eq!(responder(&format!("xyz {json}\n"), "abc", &envia), Some("?\n"));
        assert_eq!(responder(&format!("{json}\n"), "abc", &envia), Some("?\n"));
        assert_eq!(responder("abc não é json\n", "abc", &envia), Some("?\n"));
        assert!(recebe.try_recv().is_err());
        // Sem senha (o socket do Linux), a linha é só o JSON.
        assert_eq!(responder(&format!("{json}\n"), "", &envia), Some("ok\n"));
        drop(recebe);
        assert_eq!(responder(&format!("{json}\n"), "", &envia), None, "ninguém mais lê");
    }

    #[test]
    fn abertura_pelos_argumentos() {
        let pasta = pasta_de_teste("args");
        fs::create_dir_all(pasta.join("loja")).unwrap();
        let canonica = dunce::canonicalize(pasta.join("loja")).unwrap();
        let args = |a: &[&str]| a.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(Abertura::de_args(&args(&[]), &pasta).vazia());
        assert_eq!(Abertura::de_args(&args(&["loja"]), &pasta).pasta, Some(canonica.clone()));
        let caminho = canonica.display().to_string();
        assert_eq!(Abertura::de_args(&args(&[&caminho]), Path::new("/")).pasta, Some(canonica.clone()));
        assert_eq!(Abertura::de_args(&args(&["nao-existe"]), &pasta), Abertura::default());
        assert_eq!(
            Abertura::de_args(&args(&["--nova", "site", "faça o rodapé"]), &pasta),
            Abertura { pasta: None, nova: Some(("site".into(), Some("faça o rodapé".into()))) }
        );
        assert_eq!(Abertura::de_args(&args(&["--nova", "mestre"]), &pasta).nova, Some(("mestre".into(), None)));
        assert_eq!(Abertura::de_args(&args(&["--nova"]), &pasta).nova, None);
        let os_dois = Abertura::de_args(&args(&["loja", "--nova", "loja", "oi"]), &pasta);
        assert_eq!(os_dois, Abertura { pasta: Some(canonica), nova: Some(("loja".into(), Some("oi".into()))) });
        // A linha que vai pelo canal.
        let linha = serde_json::to_value(&os_dois).unwrap();
        assert_eq!(linha["nova"], serde_json::json!(["loja", "oi"]));
        let so_destino = serde_json::to_value(Abertura { pasta: None, nova: Some(("x".into(), None)) }).unwrap();
        assert_eq!(so_destino, serde_json::json!({"pasta": null, "nova": ["x", null]}));
        let _ = fs::remove_dir_all(&pasta);
    }
}
