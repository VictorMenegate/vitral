//! Processos que o Vitral abre e precisa conseguir parar por inteiro (claude, verificação, revisor).
//!
//! Cada um roda num escopo próprio do systemd do usuário: parar o escopo derruba também o que ele
//! abriu, até os shells que o claude separa com setsid (servidores, testes, cargo). Sem systemd,
//! cai num grupo de processos próprio. Nos dois casos o processo morre se o Vitral morrer.

use std::{
    ffi::OsStr,
    os::unix::process::CommandExt,
    process::{Child, Command, ExitStatus, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

/// Argumentos do systemd-run que funcionaram aqui; None quando o systemd do usuário não abre
/// escopos (aí vale o grupo de processos).
static SYSTEMD: OnceLock<Option<Vec<String>>> = OnceLock::new();
static CONTADOR: AtomicU64 = AtomicU64::new(0);

fn systemd() -> Option<&'static Vec<String>> {
    SYSTEMD
        .get_or_init(|| {
            let base: Vec<String> = [
                "--user",
                "--scope",
                "--quiet",
                "--collect",
                // Sem isso o systemd-run troca `$VAR` no comando antes do bash ver.
                "--expand-environment=no",
                "--slice=vitral.slice",
                "-p",
                "TimeoutStopSec=5s",
            ]
            .into_iter()
            .map(String::from)
            .collect();
            // Preso à unidade do próprio Vitral: se ele cair, o systemd para os escopos também.
            let preso = unidade_propria().map(|u| {
                let mut args = base.clone();
                args.extend(["-p".to_string(), format!("BindsTo={u}")]);
                args
            });
            preso.into_iter().chain([base]).find(|args| abre_escopo(args))
        })
        .as_ref()
}

/// A unidade do systemd do usuário em que o Vitral roda (o escopo do app no GNOME, por exemplo).
fn unidade_propria() -> Option<String> {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup").ok()?;
    let caminho = cgroup.lines().find_map(|l| l.strip_prefix("0::"))?;
    let partes: Vec<&str> = caminho.split('/').collect();
    let gerente = partes
        .iter()
        .position(|p| p.starts_with("user@") && p.ends_with(".service"))?;
    let ultima = *partes.last()?;
    (partes.len() > gerente + 1 && (ultima.ends_with(".scope") || ultima.ends_with(".service")))
        .then(|| ultima.to_string())
}

/// Testa os argumentos com um `true`, uma vez só: se o systemd-run falhar depois, o claude nem roda.
fn abre_escopo(args: &[String]) -> bool {
    let Ok(mut filho) = Command::new("systemd-run")
        .args(args)
        .arg(format!("--unit={}", nome_unidade("teste")))
        .args(["--", "true"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let limite = Instant::now() + Duration::from_secs(3);
    loop {
        match filho.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < limite => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = filho.kill();
                let _ = filho.wait();
                return false;
            }
        }
    }
}

fn nome_unidade(tipo: &str) -> String {
    let numero = CONTADOR.fetch_add(1, Ordering::Relaxed);
    format!("vitral-{}-{tipo}-{numero}.scope", std::process::id())
}

#[cfg(test)]
pub fn usa_systemd() -> bool {
    systemd().is_some()
}

/// Comando que roda `programa` num escopo novo (ou num grupo próprio, sem systemd). Quem chama
/// acrescenta argumentos, ambiente e pastas normalmente e, depois do spawn, cria o Escopo.
pub fn comando(programa: impl AsRef<OsStr>, tipo: &str) -> (Command, Option<String>) {
    montar(programa, tipo, systemd())
}

fn montar(programa: impl AsRef<OsStr>, tipo: &str, systemd: Option<&Vec<String>>) -> (Command, Option<String>) {
    let unidade = systemd.map(|args| (args, nome_unidade(tipo)));
    let mut comando = match &unidade {
        Some((args, nome)) => {
            // No modo escopo, o systemd-run executa o programa no próprio processo: o pid, os
            // pipes, a pasta e o ambiente continuam os mesmos.
            let mut c = Command::new("systemd-run");
            c.args(args.iter()).arg(format!("--unit={nome}")).arg("--").arg(programa);
            c
        }
        None => Command::new(programa),
    };
    comando.process_group(0);
    unsafe {
        comando.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    (comando, unidade.map(|(_, nome)| nome))
}

/// Um processo aberto por `comando`, com o que ele abrir.
pub struct Escopo {
    unidade: Option<String>,
    pid: i32,
}

impl Escopo {
    pub fn novo(unidade: Option<String>, pid: u32) -> Self {
        Self {
            unidade,
            pid: pid as i32,
        }
    }

    /// SIGTERM em tudo, para cada um fazer a própria limpeza; o systemd mata de vez quem não sair
    /// em 5 s, mesmo que o Vitral já tenha fechado. Só antes de colher o processo (wait).
    pub fn parar(&self) {
        match &self.unidade {
            Some(unidade) => parar_unidades(&[unidade.as_str()]),
            None => unsafe {
                libc::killpg(self.pid, libc::SIGTERM);
            },
        }
    }

    /// SIGKILL no grupo e o escopo parado: derruba o que sobrou em segundo plano. Só antes de
    /// colher o processo; depois, o número pode já ser de outro.
    pub fn matar(&self) {
        unsafe {
            libc::killpg(self.pid, libc::SIGKILL);
        }
        if let Some(unidade) = &self.unidade {
            parar_unidades(&[unidade.as_str()]);
        }
    }

    /// O processo principal terminou? Não colhe (WNOWAIT), então o número do grupo continua dele.
    pub fn terminou(&self) -> bool {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let opcoes = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
        let resultado = unsafe { libc::waitid(libc::P_PID, self.pid as libc::id_t, &mut info, opcoes) };
        resultado != 0 || unsafe { info.si_pid() } != 0
    }
}

/// Código de saída de um filho que já terminou, sem colhê-lo (WNOWAIT): quem colhe continua sendo
/// o dono do `Child`. -1 se morreu por sinal; `None` enquanto roda ou se já foi colhido.
pub fn codigo_de_saida(pid: u32) -> Option<i32> {
    let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
    let opcoes = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
    let resultado = unsafe { libc::waitid(libc::P_PID, pid as libc::id_t, &mut info, opcoes) };
    if resultado != 0 || unsafe { info.si_pid() } == 0 {
        return None;
    }
    Some(if info.si_code == libc::CLD_EXITED { unsafe { info.si_status() } } else { -1 })
}

/// Espera o processo terminar até o prazo; estourado, para tudo e dá 2 s antes do SIGKILL. No fim
/// derruba o que ficou rodando em segundo plano e colhe. None quando estourou o prazo.
pub fn esperar(filho: &mut Child, escopo: &Escopo, prazo: Duration) -> Option<ExitStatus> {
    let limite = Instant::now() + prazo;
    let mut estourou = false;
    while !escopo.terminou() {
        if Instant::now() > limite {
            estourou = true;
            escopo.parar();
            let folga = Instant::now() + Duration::from_secs(2);
            while !escopo.terminou() && Instant::now() < folga {
                std::thread::sleep(Duration::from_millis(100));
            }
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    escopo.matar();
    let status = filho.wait().ok();
    if estourou { None } else { status }
}

/// Colhe o processo numa thread, sem travar quem chamou; se passar do prazo, SIGKILL no grupo.
pub fn colher(pid: u32, prazo: Duration) {
    std::thread::spawn(move || {
        let pid = pid as i32;
        let limite = Instant::now() + prazo;
        let mut status = 0;
        while unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } == 0 {
            if Instant::now() > limite {
                unsafe {
                    libc::killpg(pid, libc::SIGKILL);
                    libc::waitpid(pid, &mut status, 0);
                }
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    });
}

/// `systemctl stop` sem esperar o fim da parada (o systemd segue sozinho) e sem travar quem chamou.
fn parar_unidades(unidades: &[&str]) {
    let filho = Command::new("systemctl")
        .args(["--user", "stop", "--no-block", "--"])
        .args(unidades)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut filho) = filho {
        std::thread::spawn(move || filho.wait());
    }
}

/// Para tudo o que este Vitral abriu (ao fechar a janela ou trocar de pasta). Espera só o pedido
/// chegar ao systemd, que termina a parada sozinho mesmo com o Vitral já fechado.
pub fn encerrar_todos() {
    if SYSTEMD.get().is_none_or(Option::is_none) {
        return;
    }
    let filho = Command::new("systemctl")
        .args(["--user", "stop", "--no-block"])
        .arg(format!("vitral-{}-*.scope", std::process::id()))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Ok(mut filho) = filho {
        let limite = Instant::now() + Duration::from_secs(1);
        while matches!(filho.try_wait(), Ok(None)) && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(10));
        }
        std::thread::spawn(move || filho.wait());
    }
}

/// Para os escopos que um Vitral que caiu deixou rodando. Roda uma vez, ao abrir.
pub fn limpar_sobras() {
    if systemd().is_none() {
        return;
    }
    let Ok(saida) = Command::new("systemctl")
        .args(["--user", "list-units", "--plain", "--no-legend", "--type=scope", "vitral-*"])
        .stderr(Stdio::null())
        .output()
    else {
        return;
    };
    let texto = String::from_utf8_lossy(&saida.stdout);
    let sobras: Vec<&str> = texto
        .lines()
        .filter_map(|l| l.split_whitespace().next())
        .filter(|unidade| pid_da_unidade(unidade).is_some_and(|pid| !vitral_vivo(pid)))
        .collect();
    if !sobras.is_empty() {
        parar_unidades(&sobras);
    }
}

/// O pid do Vitral no nome `vitral-<pid>-<tipo>-<n>.scope`; None para qualquer outro nome.
fn pid_da_unidade(unidade: &str) -> Option<u32> {
    let resto = unidade.strip_prefix("vitral-")?.strip_suffix(".scope")?;
    let partes: Vec<&str> = resto.split('-').collect();
    let [pid, tipo, numero] = partes[..] else {
        return None;
    };
    numero.parse::<u64>().ok()?;
    (!tipo.is_empty() && tipo.chars().all(|c| c.is_ascii_lowercase())).then_some(())?;
    pid.parse().ok()
}

fn vitral_vivo(pid: u32) -> bool {
    pid == std::process::id()
        || std::fs::read_to_string(format!("/proc/{pid}/comm")).is_ok_and(|nome| nome.starts_with("vitral"))
}

/// Nos testes: o processo some (ou vira zumbi esperando quem o colha) em até 6 s.
#[cfg(test)]
pub fn morre_logo(pid: u32) -> bool {
    let limite = Instant::now() + Duration::from_secs(6);
    loop {
        let estado = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok();
        let vivo = estado.is_some_and(|s| s.rsplit_once(") ").is_some_and(|(_, r)| !r.starts_with('Z')));
        if !vivo {
            return true;
        }
        if Instant::now() > limite {
            return false;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn codigo_de_saida_sem_colher() {
        let mut filho = Command::new("sh").args(["-c", "exit 3"]).spawn().unwrap();
        let pid = filho.id();
        let limite = Instant::now() + Duration::from_secs(5);
        let mut codigo = None;
        while codigo.is_none() && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(10));
            codigo = codigo_de_saida(pid);
        }
        assert_eq!(codigo, Some(3));
        // Quem colhe continua sendo o dono do processo.
        assert_eq!(filho.wait().unwrap().code(), Some(3));
        assert_eq!(codigo_de_saida(pid), None);
    }

    #[test]
    fn so_reconhece_os_escopos_do_vitral() {
        assert_eq!(pid_da_unidade("vitral-1234-claude-7.scope"), Some(1234));
        assert_eq!(pid_da_unidade("vitral-99-verificacao-0.scope"), Some(99));
        // O Vitral aberto como serviço e outros nomes parecidos ficam de fora.
        assert_eq!(pid_da_unidade("vitral-1793463195.service"), None);
        assert_eq!(pid_da_unidade("vitral-1234.slice"), None);
        assert_eq!(pid_da_unidade("vitral-teste-claude-1.scope"), None);
        assert_eq!(pid_da_unidade("vitral-1-a-b-2.scope"), None);
    }

    #[test]
    fn parar_derruba_o_que_o_processo_abriu() {
        parar_derruba_o_que_o_processo_abriu_com(systemd());
    }

    #[test]
    fn sem_systemd_o_grupo_de_processos_faz_o_mesmo() {
        parar_derruba_o_que_o_processo_abriu_com(None);
    }

    fn parar_derruba_o_que_o_processo_abriu_com(systemd: Option<&Vec<String>>) {
        let pasta = std::env::temp_dir().join(format!("vitral-proc-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let (mut comando, unidade) = montar("bash", "teste", systemd);
        comando
            .args(["-c", "sleep 60 & echo $! > neto; wait"])
            .current_dir(&pasta)
            .stdout(Stdio::null());
        let mut filho = comando.spawn().unwrap();
        let escopo = Escopo::novo(unidade, filho.id());
        let arquivo = pasta.join("neto");
        let limite = Instant::now() + Duration::from_secs(5);
        let neto = loop {
            if let Some(pid) = std::fs::read_to_string(&arquivo).ok().and_then(|t| t.trim().parse::<u32>().ok()) {
                break pid;
            }
            assert!(Instant::now() < limite, "o neto não abriu");
            std::thread::sleep(Duration::from_millis(20));
        };
        let inicio = Instant::now();
        let status = esperar(&mut filho, &escopo, Duration::from_millis(300));
        assert!(status.is_none(), "devia estourar o prazo");
        assert!(inicio.elapsed() < Duration::from_secs(4));
        assert!(morre_logo(neto), "o neto continuou rodando");
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
