//! Os processos no Linux: cada um roda num escopo próprio do systemd do usuário. Parar o escopo
//! derruba também o que ele abriu, até os shells que o claude separa com setsid (servidores,
//! testes, cargo). Sem systemd, cai num grupo de processos próprio. Nos dois casos o processo morre
//! se o Vitral morrer.

use std::{
    ffi::OsStr,
    io,
    os::unix::process::CommandExt,
    process::{Child, Stdio},
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use super::{Comando, novo_comando};

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
    let Ok(mut filho) = novo_comando("systemd-run")
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

/// Comando que roda `programa` num escopo novo (ou num grupo próprio, sem systemd). Quem chama
/// acrescenta argumentos, ambiente e pastas normalmente e abre com `abrir`.
pub fn comando(programa: impl AsRef<OsStr>, tipo: &str) -> Comando {
    montar(programa, tipo, systemd())
}

/// Como `comando`, mas sem o escopo do systemd: só o grupo próprio. Para os programas curtos de
/// rede (gh, curl, git), em que um systemd-run a cada chamada pesaria no relógio.
pub fn comando_leve(programa: impl AsRef<OsStr>, tipo: &str) -> Comando {
    montar(programa, tipo, None)
}

fn montar(programa: impl AsRef<OsStr>, tipo: &str, systemd: Option<&Vec<String>>) -> Comando {
    let unidade = systemd.map(|args| (args, nome_unidade(tipo)));
    let mut comando = match &unidade {
        Some((args, nome)) => {
            // No modo escopo, o systemd-run executa o programa no próprio processo: o pid, os
            // pipes, a pasta e o ambiente continuam os mesmos.
            let mut c = novo_comando("systemd-run");
            c.args(args.iter()).arg(format!("--unit={nome}")).arg("--").arg(programa);
            c
        }
        None => novo_comando(programa),
    };
    comando.process_group(0);
    unsafe {
        comando.pre_exec(|| {
            libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM);
            Ok(())
        });
    }
    Comando { comando, unidade: unidade.map(|(_, nome)| nome) }
}

/// Abre o processo de `comando` e o escopo dele. O `Command` sai daqui junto: as pontas de pipe que
/// ele guardava (a saída da verificação) fecham.
pub fn abrir(comando: Comando) -> io::Result<(Child, Escopo)> {
    let Comando { mut comando, unidade } = comando;
    let filho = comando.spawn()?;
    let escopo = Escopo::novo(unidade, filho.id());
    Ok((filho, escopo))
}

/// Um processo aberto por `comando`, com o que ele abrir.
pub struct Escopo {
    unidade: Option<String>,
    pid: i32,
}

impl Escopo {
    fn novo(unidade: Option<String>, pid: u32) -> Self {
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

    /// A memória que o processo e tudo o que ele abriu ocupam agora, em bytes: o `memory.current`
    /// do cgroup do escopo. `None` sem systemd (o grupo de processos não tem conta própria), sem o
    /// controlador de memória no cgroup do usuário ou com o processo fora do escopo.
    pub fn memoria(&self) -> Option<u64> {
        let unidade = self.unidade.as_deref()?;
        let do_processo = std::fs::read_to_string(format!("/proc/{}/cgroup", self.pid)).ok()?;
        memoria_do_cgroup(std::path::Path::new("/sys/fs/cgroup"), &do_processo, unidade)
    }

    /// O processo principal terminou? Não colhe (WNOWAIT), então o número do grupo continua dele.
    #[cfg(test)]
    pub fn terminou(&self) -> bool {
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let opcoes = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
        let resultado = unsafe { libc::waitid(libc::P_PID, self.pid as libc::id_t, &mut info, opcoes) };
        resultado != 0 || unsafe { info.si_pid() } != 0
    }
}

/// Lê o `memory.current` do cgroup (v2) em que o processo está, pelo texto do `/proc/<pid>/cgroup`
/// dele ("0::/user.slice/…/vitral-12-claude-3.scope"). Só vale se esse cgroup é o da `unidade`: o
/// processo que ainda não entrou no escopo (ou que roda sem ele) está no cgroup do Vitral, e o
/// número seria o do app inteiro.
fn memoria_do_cgroup(raiz: &std::path::Path, do_processo: &str, unidade: &str) -> Option<u64> {
    let caminho = do_processo.lines().find_map(|l| l.strip_prefix("0::"))?.trim().trim_end_matches('/');
    if caminho.rsplit('/').next() != Some(unidade) {
        return None;
    }
    let arquivo = raiz.join(caminho.trim_start_matches('/')).join("memory.current");
    std::fs::read_to_string(arquivo).ok()?.trim().parse().ok()
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
    let filho = novo_comando("systemctl")
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
    let filho = novo_comando("systemctl")
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
    let Ok(saida) = novo_comando("systemctl")
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
    use crate::processos::esperar;

    #[test]
    fn codigo_de_saida_sem_colher() {
        let mut filho = novo_comando("sh").args(["-c", "exit 3"]).spawn().unwrap();
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

    #[test]
    fn le_a_memoria_do_cgroup_do_escopo() {
        let raiz = std::env::temp_dir().join(format!("vitral-cgroup-{}", crate::modelo::novo_id()));
        let unidade = "vitral-12-claude-3.scope";
        let caminho = format!("user.slice/user-1000.slice/user@1000.service/vitral.slice/{unidade}");
        std::fs::create_dir_all(raiz.join(&caminho)).unwrap();
        std::fs::write(raiz.join(&caminho).join("memory.current"), "3328599654\n").unwrap();
        let do_processo = format!("0::/{caminho}\n");
        assert_eq!(memoria_do_cgroup(&raiz, &do_processo, unidade), Some(3_328_599_654));
        // O processo no cgroup de outra unidade (a do próprio Vitral): o número não é o do chat.
        let do_vitral = "0::/user.slice/user-1000.slice/user@1000.service/app.slice/app-vitral.scope\n";
        assert_eq!(memoria_do_cgroup(&raiz, do_vitral, unidade), None);
        // cgroup v1 (sem a linha "0::"), arquivo ausente (sem o controlador de memória) e lixo.
        assert_eq!(memoria_do_cgroup(&raiz, "5:memory:/user.slice\n", unidade), None);
        std::fs::remove_file(raiz.join(&caminho).join("memory.current")).unwrap();
        assert_eq!(memoria_do_cgroup(&raiz, &do_processo, unidade), None);
        std::fs::write(raiz.join(&caminho).join("memory.current"), "max\n").unwrap();
        assert_eq!(memoria_do_cgroup(&raiz, &do_processo, unidade), None);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    /// De ponta a ponta, onde há systemd com o controlador de memória: o escopo de verdade diz
    /// quanto ocupa. Sem um dos dois, o teste não tem o que conferir.
    #[test]
    fn o_escopo_de_verdade_diz_a_memoria() {
        let Some(args) = systemd() else {
            return;
        };
        let mut comando = montar("sleep", "teste", Some(args));
        comando.arg("30").stdout(Stdio::null());
        let (mut filho, escopo) = abrir(comando).unwrap();
        // O systemd-run leva um instante para pôr o processo no escopo.
        let limite = Instant::now() + Duration::from_secs(3);
        let mut memoria = None;
        while memoria.is_none() && Instant::now() < limite {
            memoria = escopo.memoria();
            std::thread::sleep(Duration::from_millis(50));
        }
        escopo.matar();
        let _ = filho.wait();
        if let Some(bytes) = memoria {
            assert!(bytes > 0 && bytes < 4 * 1024 * 1024 * 1024, "{bytes}");
        }
    }

    fn parar_derruba_o_que_o_processo_abriu_com(systemd: Option<&Vec<String>>) {
        let pasta = std::env::temp_dir().join(format!("vitral-proc-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let mut comando = montar("bash", "teste", systemd);
        comando
            .args(["-c", "sleep 60 & echo $! > neto; wait"])
            .current_dir(&pasta)
            .stdout(Stdio::null());
        let (mut filho, escopo) = abrir(comando).unwrap();
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
