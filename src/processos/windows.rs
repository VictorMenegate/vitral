//! Os processos no Windows: cada um roda num Job Object próprio. Encerrar o job derruba a árvore
//! inteira, e ele se encerra sozinho quando o último handle fecha
//! (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`): o `Escopo` sair, ou o Vitral morrer, derruba o que o
//! processo abriu. É o que o escopo do systemd, o grupo de processos e o PDEATHSIG fazem no Linux.
//! Não existe o SIGTERM: parar e matar são a mesma coisa, na hora.
//!
//! O processo nasce suspenso (`CREATE_SUSPENDED`), entra no job e só então roda: nada do que ele
//! abrir escapa, porque os filhos entram no job do pai sozinhos.

use std::{
    ffi::OsStr,
    io,
    os::windows::{
        io::{AsHandle as _, AsRawHandle as _, FromRawHandle as _, OwnedHandle, RawHandle},
        process::CommandExt as _,
    },
    process::Child,
    sync::{Arc, Mutex, Weak},
    time::Duration,
};

use windows_sys::Win32::{
    Foundation::{HANDLE, INVALID_HANDLE_VALUE, WAIT_OBJECT_0},
    System::{
        Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, TH32CS_SNAPTHREAD, THREADENTRY32, Thread32First, Thread32Next},
        JobObjects::{
            AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation, SetInformationJobObject,
            TerminateJobObject,
        },
        Threading::{
            CREATE_NEW_PROCESS_GROUP, CREATE_NO_WINDOW, CREATE_SUSPENDED, GetExitCodeProcess, OpenProcess, OpenThread,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE, ResumeThread,
            THREAD_SUSPEND_RESUME, TerminateProcess, WaitForSingleObject,
        },
    },
};

use super::{Comando, novo_comando};

/// O código de saída de quem o Vitral derrubou (o mesmo de um console fechado com Ctrl+C):
/// `codigo_de_saida` devolve -1 para ele, como para quem morreu por sinal no Linux.
const DERRUBADO: u32 = 0xC000_013A;

/// Os jobs vivos, para o `encerrar_todos`. Quem segura cada um é o `Escopo` dele.
static JOBS: Mutex<Vec<Weak<Job>>> = Mutex::new(Vec::new());

/// Um Job Object com `KILL_ON_JOB_CLOSE`: fechar o handle (no `Drop`) derruba o que está nele.
struct Job(OwnedHandle);

impl Job {
    fn novo() -> io::Result<Job> {
        // SAFETY: sem atributos nem nome; devolve um handle novo, ou nulo no erro.
        let bruto = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if bruto.is_null() {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: o handle acabou de ser criado e só está aqui.
        let job = Job(unsafe { OwnedHandle::from_raw_handle(bruto as RawHandle) });
        let mut limites = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limites.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        // SAFETY: a struct da classe pedida, com o tamanho dela, viva durante a chamada.
        let ok = unsafe {
            SetInformationJobObject(
                job.handle(),
                JobObjectExtendedLimitInformation,
                (&raw const limites).cast(),
                size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(job)
    }

    fn handle(&self) -> HANDLE {
        self.0.as_raw_handle() as HANDLE
    }

    fn encerrar(&self) {
        // SAFETY: o handle do job vive enquanto `self` existe.
        unsafe {
            TerminateJobObject(self.handle(), DERRUBADO);
        }
    }
}

/// Comando que roda `programa` num job novo. Quem chama acrescenta argumentos, ambiente e pastas
/// normalmente e abre com `abrir`. O `tipo` só dá nome ao escopo no Linux.
pub fn comando(programa: impl AsRef<OsStr>, _tipo: &str) -> Comando {
    Comando { comando: novo_comando(programa) }
}

/// No Windows não há systemd: o leve é o mesmo `comando`, com o job.
pub fn comando_leve(programa: impl AsRef<OsStr>, tipo: &str) -> Comando {
    comando(programa, tipo)
}

/// Abre o processo de `comando` suspenso, põe no job e só então solta. Se o Windows não aceitar o
/// job (um job dentro de outro que não deixa), o processo roda sem ele e parar derruba só ele. O
/// `Command` sai daqui junto: as pontas de pipe que ele guardava (a saída da verificação) fecham.
pub fn abrir(comando: Comando) -> io::Result<(Child, Escopo)> {
    let Comando { mut comando } = comando;
    comando.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    let job = Job::novo();
    let mut filho = comando.spawn()?;
    let preparado = (|| {
        let processo = filho.as_handle().try_clone_to_owned()?;
        let no_job = job.and_then(|job| {
            // SAFETY: os dois handles estão vivos (o do processo é do `Child`).
            let entrou = unsafe { AssignProcessToJobObject(job.handle(), filho.as_raw_handle() as HANDLE) } != 0;
            if entrou { Ok(job) } else { Err(io::Error::last_os_error()) }
        });
        let job = match no_job {
            Ok(job) => Some(Arc::new(job)),
            Err(erro) => {
                eprintln!("vitral: o processo roda fora de um job ({erro}); parar não derruba o que ele abrir");
                None
            }
        };
        retomar(filho.id())?;
        Ok::<_, io::Error>(Escopo { job, processo })
    })();
    match preparado {
        Ok(escopo) => {
            if let Some(job) = &escopo.job {
                let mut jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
                jobs.retain(|j| j.strong_count() > 0);
                jobs.push(Arc::downgrade(job));
            }
            Ok((filho, escopo))
        }
        Err(erro) => {
            // Suspenso, ele nunca sairia sozinho.
            let _ = filho.kill();
            let _ = filho.wait();
            Err(erro)
        }
    }
}

/// Solta as threads do processo que nasceu suspenso (é uma só, a principal).
fn retomar(pid: u32) -> io::Result<()> {
    // SAFETY: sem ponteiros; devolve um handle novo, ou INVALID_HANDLE_VALUE no erro.
    let bruto = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if bruto == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: o handle acabou de ser criado e só está aqui.
    let foto = unsafe { OwnedHandle::from_raw_handle(bruto as RawHandle) };
    let foto = foto.as_raw_handle() as HANDLE;
    let mut thread = THREADENTRY32 { dwSize: size_of::<THREADENTRY32>() as u32, ..Default::default() };
    let mut soltas = 0;
    // SAFETY: a foto vive até o fim da função; `thread` tem o dwSize certo.
    let mut tem = unsafe { Thread32First(foto, &mut thread) } != 0;
    while tem {
        if thread.th32OwnerProcessID == pid {
            // SAFETY: devolve um handle novo, ou nulo no erro.
            let bruto = unsafe { OpenThread(THREAD_SUSPEND_RESUME, 0, thread.th32ThreadID) };
            if !bruto.is_null() {
                // SAFETY: o handle acabou de ser aberto e só está aqui.
                let aberta = unsafe { OwnedHandle::from_raw_handle(bruto as RawHandle) };
                // SAFETY: handle vivo com o direito de retomar.
                if unsafe { ResumeThread(aberta.as_raw_handle() as HANDLE) } != u32::MAX {
                    soltas += 1;
                }
            }
        }
        // SAFETY: como no Thread32First.
        tem = unsafe { Thread32Next(foto, &mut thread) } != 0;
    }
    if soltas == 0 { Err(io::Error::other("não achei a thread do processo para soltar")) } else { Ok(()) }
}

/// Um processo aberto por `comando`, com o que ele abrir.
pub struct Escopo {
    /// None quando o Windows não aceitou o processo no job.
    job: Option<Arc<Job>>,
    /// Um handle só do escopo para o processo principal (o do `Child` sai com ele).
    processo: OwnedHandle,
}

impl Escopo {
    fn handle(&self) -> HANDLE {
        self.processo.as_raw_handle() as HANDLE
    }

    /// Derruba a árvore inteira na hora: no Windows não há como pedir para sair.
    pub fn parar(&self) {
        match &self.job {
            Some(job) => job.encerrar(),
            // SAFETY: o handle do processo vive enquanto o escopo existe.
            None => unsafe {
                TerminateProcess(self.handle(), DERRUBADO);
            },
        }
    }

    /// O mesmo que `parar`: derruba também o que sobrou em segundo plano depois que o principal saiu.
    pub fn matar(&self) {
        self.parar();
    }

    /// O processo principal terminou?
    pub fn terminou(&self) -> bool {
        // SAFETY: o handle do processo vive enquanto o escopo existe.
        unsafe { WaitForSingleObject(self.handle(), 0) == WAIT_OBJECT_0 }
    }
}

/// Um handle novo para o processo `pid`, com os direitos pedidos.
fn abrir_processo(pid: u32, direitos: u32) -> Option<OwnedHandle> {
    // SAFETY: devolve um handle novo, ou nulo no erro.
    let bruto = unsafe { OpenProcess(direitos, 0, pid) };
    // SAFETY: o handle acabou de ser aberto e só está aqui.
    (!bruto.is_null()).then(|| unsafe { OwnedHandle::from_raw_handle(bruto as RawHandle) })
}

/// Código de saída de um filho que já terminou. -1 se o Vitral o derrubou; `None` enquanto roda ou
/// se ninguém mais segura o processo (o `Child` já saiu). Enquanto o `Child` existe, o número não
/// passa para outro processo.
pub fn codigo_de_saida(pid: u32) -> Option<i32> {
    let processo = abrir_processo(pid, PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE)?;
    let handle = processo.as_raw_handle() as HANDLE;
    // SAFETY: handle vivo com os direitos de esperar e de ler o código.
    if unsafe { WaitForSingleObject(handle, 0) } != WAIT_OBJECT_0 {
        return None;
    }
    let mut codigo = 0u32;
    // SAFETY: como acima; `codigo` vive durante a chamada.
    if unsafe { GetExitCodeProcess(handle, &mut codigo) } == 0 {
        return None;
    }
    Some(if codigo == DERRUBADO { -1 } else { codigo as i32 })
}

/// Não há zumbi para colher no Windows: só espera numa thread, sem travar quem chamou, e derruba o
/// processo se ele passar do prazo. Quem chama ainda tem o `Child`, então o número é dele.
pub fn colher(pid: u32, prazo: Duration) {
    let Some(processo) = abrir_processo(pid, PROCESS_SYNCHRONIZE | PROCESS_TERMINATE) else {
        return;
    };
    std::thread::spawn(move || {
        let handle = processo.as_raw_handle() as HANDLE;
        let milissegundos = prazo.as_millis().min(u128::from(u32::MAX - 1)) as u32;
        // SAFETY: handle vivo até o fim da thread, com os direitos de esperar e de terminar.
        unsafe {
            if WaitForSingleObject(handle, milissegundos) != WAIT_OBJECT_0 {
                TerminateProcess(handle, DERRUBADO);
            }
        }
    });
}

/// Derruba tudo o que este Vitral abriu (ao fechar a janela ou trocar de pasta).
pub fn encerrar_todos() {
    let jobs = JOBS.lock().unwrap_or_else(|e| e.into_inner());
    for job in jobs.iter().filter_map(Weak::upgrade) {
        job.encerrar();
    }
}

/// Nada a fazer: o job de um Vitral que caiu se encerrou junto com ele.
pub fn limpar_sobras() {}

/// Nos testes: o processo some em até 6 s.
#[cfg(test)]
pub fn morre_logo(pid: u32) -> bool {
    let Some(processo) = abrir_processo(pid, PROCESS_SYNCHRONIZE) else {
        return true;
    };
    // SAFETY: handle vivo com o direito de esperar.
    unsafe { WaitForSingleObject(processo.as_raw_handle() as HANDLE, 6000) == WAIT_OBJECT_0 }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::processos::esperar;
    use std::{
        process::Stdio,
        time::{Duration, Instant},
    };
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };

    /// Os processos `nome` (o .exe, sem diferença de maiúsculas) que têm `pai` como pai.
    fn filhos(pai: u32, nome: &str) -> Vec<u32> {
        // SAFETY: como no `retomar`.
        let bruto = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
        assert!(bruto != INVALID_HANDLE_VALUE, "{}", io::Error::last_os_error());
        let foto = unsafe { OwnedHandle::from_raw_handle(bruto as RawHandle) };
        let foto = foto.as_raw_handle() as HANDLE;
        let mut processo = PROCESSENTRY32W { dwSize: size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut achados = Vec::new();
        let mut tem = unsafe { Process32FirstW(foto, &mut processo) } != 0;
        while tem {
            let fim = processo.szExeFile.iter().position(|&c| c == 0).unwrap_or(processo.szExeFile.len());
            let exe = String::from_utf16_lossy(&processo.szExeFile[..fim]);
            if processo.th32ParentProcessID == pai && exe.eq_ignore_ascii_case(nome) {
                achados.push(processo.th32ProcessID);
            }
            tem = unsafe { Process32NextW(foto, &mut processo) } != 0;
        }
        achados
    }

    /// Espera aparecer um `nome` filho de `pai` (até 10 s).
    fn esperar_filho(pai: u32, nome: &str) -> u32 {
        let limite = Instant::now() + Duration::from_secs(10);
        loop {
            if let Some(&pid) = filhos(pai, nome).first() {
                return pid;
            }
            assert!(Instant::now() < limite, "o {nome} filho de {pai} não abriu");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// cmd (filho) → cmd (neto) → ping (bisneto), que fica 60 s rodando.
    fn arvore() -> (Child, Escopo, u32, u32) {
        let mut comando = comando("cmd", "teste");
        comando.arg("/c").raw_arg("cmd /c ping -n 60 127.0.0.1").stdin(Stdio::null()).stdout(Stdio::null());
        let (filho, escopo) = abrir(comando).unwrap();
        let neto = esperar_filho(filho.id(), "cmd.exe");
        let bisneto = esperar_filho(neto, "ping.exe");
        (filho, escopo, neto, bisneto)
    }

    #[test]
    fn parar_derruba_neto_e_bisneto() {
        let (mut filho, escopo, neto, bisneto) = arvore();
        let inicio = Instant::now();
        let status = esperar(&mut filho, &escopo, Duration::from_millis(300));
        assert!(status.is_none(), "devia estourar o prazo");
        assert!(inicio.elapsed() < Duration::from_secs(4));
        assert!(morre_logo(neto), "o neto continuou rodando");
        assert!(morre_logo(bisneto), "o bisneto continuou rodando");
    }

    #[test]
    fn fechar_o_handle_do_job_derruba_a_arvore() {
        let (mut filho, escopo, neto, bisneto) = arvore();
        // O que o Vitral morrer faz: o último handle do job fecha.
        drop(escopo);
        assert!(morre_logo(filho.id()), "o filho continuou rodando");
        assert!(morre_logo(neto), "o neto continuou rodando");
        assert!(morre_logo(bisneto), "o bisneto continuou rodando");
        let _ = filho.wait();
    }

    #[test]
    fn o_que_fica_em_segundo_plano_morre_quando_o_principal_sai() {
        let mut comando = comando("cmd", "teste");
        comando.arg("/c").raw_arg("start /b ping -n 60 127.0.0.1 & exit 0").stdin(Stdio::null()).stdout(Stdio::null());
        let (mut filho, escopo) = abrir(comando).unwrap();
        let solto = esperar_filho(filho.id(), "ping.exe");
        let status = esperar(&mut filho, &escopo, Duration::from_secs(20));
        assert!(status.is_some_and(|s| s.success()), "{status:?}");
        assert!(morre_logo(solto), "o ping em segundo plano continuou rodando");
    }

    #[test]
    fn codigo_de_saida_sem_colher() {
        let mut filho = novo_comando("cmd").args(["/c", "exit 3"]).spawn().unwrap();
        let pid = filho.id();
        let limite = Instant::now() + Duration::from_secs(5);
        let mut codigo = None;
        while codigo.is_none() && Instant::now() < limite {
            std::thread::sleep(Duration::from_millis(10));
            codigo = codigo_de_saida(pid);
        }
        assert_eq!(codigo, Some(3));
        assert_eq!(filho.wait().unwrap().code(), Some(3));
    }

    #[test]
    fn quem_o_vitral_derruba_sai_com_menos_um() {
        let mut comando = comando("ping", "teste");
        comando.args(["-n", "60", "127.0.0.1"]).stdout(Stdio::null());
        let (mut filho, escopo) = abrir(comando).unwrap();
        assert_eq!(codigo_de_saida(filho.id()), None, "ainda roda");
        escopo.parar();
        assert!(morre_logo(filho.id()));
        assert_eq!(codigo_de_saida(filho.id()), Some(-1));
        let _ = filho.wait();
    }
}
