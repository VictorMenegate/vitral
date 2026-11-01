//! Um Vitral só por usuário: a trava (`flock` em `~/.local/share/vitral.trava`, fora da pasta de
//! dados) e o socket por onde uma segunda abertura entrega o pedido (`vitral <pasta>`, `--nova`) ao
//! Vitral que já está aberto e sai sem abrir janela. A trava nunca espera: com ela ocupada, quem
//! chegou depois só tenta entregar o pedido e sai.

use std::{
    fs, io,
    io::{BufRead as _, BufReader, Write as _},
    os::{
        fd::AsRawFd as _,
        unix::{
            fs::{DirBuilderExt as _, PermissionsExt as _},
            net::{UnixListener, UnixStream},
        },
    },
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use smol::channel::{Receiver, unbounded};

/// O que pedir ao abrir: uma pasta (`vitral <pasta>`) e um chat novo (`--nova destino [mensagem]`).
/// É a mesma coisa que vai pelo socket, numa linha de JSON.
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
            match caminho.canonicalize() {
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

/// `None`: outro Vitral já está com a trava. O arquivo abre com `O_CLOEXEC` (o padrão do Rust):
/// num exec (Tentar de novo) a trava sai junto.
pub fn travar(caminho: &Path) -> io::Result<Option<Trava>> {
    if let Some(pasta) = caminho.parent() {
        fs::create_dir_all(pasta)?;
    }
    let arquivo = fs::OpenOptions::new().create(true).truncate(false).write(true).open(caminho)?;
    if unsafe { libc::flock(arquivo.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
        return Ok(Some(Trava { _arquivo: arquivo }));
    }
    let erro = io::Error::last_os_error();
    if erro.raw_os_error() == Some(libc::EWOULDBLOCK) {
        Ok(None)
    } else {
        Err(erro)
    }
}

/// Abre o socket (a pasta com 0700; um socket velho, de um Vitral que caiu, sai antes) e uma
/// thread que recebe os pedidos e os guarda até alguém ler: a janela ainda nem existe.
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
            let resposta = match serde_json::from_str::<Abertura>(linha.trim()) {
                Ok(abertura) => {
                    if envia.send_blocking(abertura).is_err() {
                        return;
                    }
                    "ok\n"
                }
                Err(_) => "?\n",
            };
            let _ = (&conexao).write_all(resposta.as_bytes());
        }
    });
    Ok(recebe)
}

/// A segunda abertura: tenta entregar o pedido ao Vitral aberto a cada 200 ms, até o prazo (ele
/// pode estar no meio da migração, antes de abrir o socket).
pub fn entregar(socket: &Path, abertura: &Abertura, prazo: Duration) -> io::Result<()> {
    let linha = format!("{}\n", serde_json::to_string(abertura)?);
    let fim = Instant::now() + prazo;
    loop {
        let tentativa = UnixStream::connect(socket).and_then(|mut conexao| {
            conexao.set_read_timeout(Some(Duration::from_secs(2)))?;
            conexao.write_all(linha.as_bytes())?;
            let mut resposta = String::new();
            BufReader::new(&conexao).read_line(&mut resposta)?;
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

/// Abre o binário de novo no lugar deste processo, com os mesmos argumentos. A trava e o socket
/// fecham no exec (os arquivos abrem com `O_CLOEXEC`); só volta com o erro.
pub fn reexecutar() -> io::Error {
    use std::os::unix::process::CommandExt as _;
    std::process::Command::new("/proc/self/exe").args(std::env::args_os().skip(1)).exec()
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
        // Um socket velho, de um Vitral que caiu, não atrapalha.
        fs::create_dir_all(socket.parent().unwrap()).unwrap();
        fs::write(&socket, "").unwrap();
        let pedidos = ouvir(&socket).unwrap();
        let abertura = Abertura { pasta: Some(pasta.clone()), nova: Some(("loja".into(), Some("oi".into()))) };
        entregar(&socket, &abertura, Duration::from_secs(5)).unwrap();
        assert_eq!(pedidos.recv_blocking().unwrap(), abertura);
        let modo = fs::metadata(socket.parent().unwrap()).unwrap().permissions().mode();
        assert_eq!(modo & 0o777, 0o700);
        // Ninguém ouvindo: desiste no prazo.
        drop(pedidos);
        let sem = pasta.join("ninguem.sock");
        assert!(entregar(&sem, &Abertura::default(), Duration::from_millis(300)).is_err());
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn abertura_pelos_argumentos() {
        let pasta = pasta_de_teste("args");
        fs::create_dir_all(pasta.join("loja")).unwrap();
        let canonica = pasta.join("loja").canonicalize().unwrap();
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
        // A linha que vai pelo socket.
        let linha = serde_json::to_value(&os_dois).unwrap();
        assert_eq!(linha["nova"], serde_json::json!(["loja", "oi"]));
        let so_destino = serde_json::to_value(Abertura { pasta: None, nova: Some(("x".into(), None)) }).unwrap();
        assert_eq!(so_destino, serde_json::json!({"pasta": null, "nova": ["x", null]}));
        let _ = fs::remove_dir_all(&pasta);
    }
}
