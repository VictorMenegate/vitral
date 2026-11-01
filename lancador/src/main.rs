//! O lançador do Vitral no Windows: o `Vitral.exe` da raiz da instalação
//! (`%LOCALAPPDATA%\Programs\Vitral`), para onde apontam o atalho do Menu Iniciar e o pino da barra
//! de tarefas. O `vitral.exe` em uso não pode ser sobrescrito (só renomeado), então cada versão
//! fica na própria `app-<versão>\` e quem fica parado é este (especificação em `distribuicao.md`,
//! seção 3.8):
//!
//! 1. Escolhe o `vitral.exe` (`escolha.rs`: a atual do `versoes.txt`, a anterior, a maior pasta).
//! 2. Abre com os mesmos argumentos e `VITRAL_LANCADOR` = este executável (o Reiniciar do app volta
//!    por aqui) e sai sem esperar: assim o próprio lançador pode ser trocado numa versão futura.
//! 3. Se nada servir, uma caixa de mensagem explica.
//!
//! Fora do Windows, o `main` só avisa e sai com 2: o `cargo build` do workspace passa no Linux e os
//! testes da escolha rodam nos dois sistemas.

// Sem janela de console (o vitral.exe do release também não abre uma). Não nos testes: o harness
// escreve no console.
#![cfg_attr(not(test), windows_subsystem = "windows")]

#[cfg_attr(not(windows), allow(dead_code, reason = "fora do Windows, só os testes escolhem"))]
mod escolha;

use std::{
    ffi::OsString,
    path::Path,
    process::{Command, ExitCode},
};

/// A variável com o caminho do lançador, que o app usa para reiniciar pelo mesmo caminho do atalho.
const VARIAVEL: &str = "VITRAL_LANCADOR";

/// `exe` com os argumentos deste processo e o caminho do lançador no ambiente.
#[cfg_attr(not(any(windows, test)), allow(dead_code, reason = "só o lançador do Windows abre o app"))]
fn comando(exe: &Path, lancador: &Path, argumentos: impl IntoIterator<Item = OsString>) -> Command {
    let mut comando = Command::new(exe);
    comando.args(argumentos).env(VARIAVEL, lancador);
    comando
}

#[cfg(windows)]
fn main() -> ExitCode {
    let lancador = match std::env::current_exe() {
        Ok(lancador) => lancador,
        Err(erro) => return falhar(&format!("O Vitral não abriu: não sei onde ele está instalado ({erro}).")),
    };
    let raiz = lancador.parent().map(Path::to_path_buf).unwrap_or_default();
    let Some(exe) = escolha::escolher(&raiz) else {
        return falhar(&format!(
            "O Vitral não está instalado direito: não achei nenhuma versão em {}.\n\nReinstale pelo \
             instalador. Na versão portátil (o .zip), abra o vitral.exe.",
            raiz.display()
        ));
    };
    match comando(&exe, &lancador, std::env::args_os().skip(1)).spawn() {
        Ok(_) => ExitCode::SUCCESS,
        Err(erro) => falhar(&format!("O Vitral não abriu ({}): {erro}.\n\nReinstale pelo instalador.", exe.display())),
    }
}

#[cfg(not(windows))]
fn main() -> ExitCode {
    eprintln!("vitral-lancador: o lançador é do Windows; neste sistema, abra o vitral.");
    ExitCode::from(2)
}

/// A caixa de mensagem do Windows com o erro (o lançador não tem console para escrever).
#[cfg(windows)]
fn falhar(texto: &str) -> ExitCode {
    use std::ffi::c_void;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBoxW(janela: *mut c_void, texto: *const u16, titulo: *const u16, tipo: u32) -> i32;
    }
    const MB_OK_ICONERROR: u32 = 0x10;
    let largo = |texto: &str| texto.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (texto, titulo) = (largo(texto), largo("Vitral"));
    // SAFETY: os dois textos terminam em zero e vivem até a caixa fechar; sem janela dona.
    unsafe { MessageBoxW(std::ptr::null_mut(), texto.as_ptr(), titulo.as_ptr(), MB_OK_ICONERROR) };
    ExitCode::FAILURE
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn abre_com_os_mesmos_argumentos_e_o_caminho_do_lancador() {
        let exe = Path::new(r"C:\Users\ana\AppData\Local\Programs\Vitral\app-0.3.0\vitral.exe");
        let lancador = Path::new(r"C:\Users\ana\AppData\Local\Programs\Vitral\Vitral.exe");
        let argumentos = ["C:\\Projetos\\loja com espaço", "--nova", "loja", "o que \"fazer\""].map(OsString::from);
        let comando = comando(exe, lancador, argumentos.clone());
        assert_eq!(comando.get_program(), exe.as_os_str());
        assert_eq!(comando.get_args().collect::<Vec<_>>(), argumentos.iter().map(OsString::as_os_str).collect::<Vec<_>>());
        let ambiente: Vec<_> = comando.get_envs().collect();
        assert_eq!(ambiente, [(VARIAVEL.as_ref(), Some(lancador.as_os_str()))]);
        // Sem pasta própria: o app herda a do atalho (a pasta do usuário), nunca a app-<versão>.
        assert_eq!(comando.get_current_dir(), None);
    }
}
