//! O registro das quedas: um pânico em qualquer thread vai para `queda.log`, na pasta dos dados,
//! com a versão, a thread, a mensagem e o rastro. Uma thread de fundo que cai não derruba o app,
//! mas sumia calada; a da janela derruba, e os chats que rodavam eram cortados sem pista. Na
//! abertura seguinte, o Vitral avisa que houve erro e diz onde está o registro.

use std::{
    io::Write,
    path::{Path, PathBuf},
    sync::OnceLock,
};

static ARQUIVO: OnceLock<PathBuf> = OnceLock::new();

/// Acima disto o registro troca de arquivo (o anterior vira `queda.log.1`).
const TAMANHO_MAXIMO: u64 = 1024 * 1024;

/// Liga o registro, mantendo o gancho que já existia (o que escreve no terminal).
pub fn instalar(pasta: &Path) {
    if ARQUIVO.set(pasta.join("queda.log")).is_err() {
        return;
    }
    let anterior = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        registrar(info);
        anterior(info);
    }));
}

fn registrar(info: &std::panic::PanicHookInfo) {
    let Some(arquivo) = ARQUIVO.get() else { return };
    let thread = std::thread::current();
    let mensagem = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| (*s).to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(sem mensagem)".into());
    let texto = texto_da_queda(
        &chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        thread.name().unwrap_or("sem nome"),
        &mensagem,
        &info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default(),
        &std::backtrace::Backtrace::force_capture().to_string(),
    );
    if std::fs::metadata(arquivo).is_ok_and(|m| m.len() > TAMANHO_MAXIMO) {
        let _ = std::fs::rename(arquivo, arquivo.with_extension("log.1"));
    }
    if let Ok(mut saida) = std::fs::OpenOptions::new().create(true).append(true).open(arquivo) {
        let _ = saida.write_all(texto.as_bytes());
    }
    let _ = std::fs::write(marca(arquivo), "");
}

fn texto_da_queda(hora: &str, thread: &str, mensagem: &str, onde: &str, rastro: &str) -> String {
    format!("== {hora} · versão {} · thread {thread}\n{mensagem}\nem {onde}\n{rastro}\n\n", crate::atualizacao::VERSAO)
}

/// A marca de que houve queda desde a última abertura (o registro em si fica).
fn marca(arquivo: &Path) -> PathBuf {
    arquivo.with_extension("nova")
}

/// Na abertura: houve erro interno desde a anterior? Devolve o aviso e apaga a marca.
pub fn aviso_da_anterior(pasta: &Path) -> Option<String> {
    let arquivo = pasta.join("queda.log");
    std::fs::remove_file(marca(&arquivo)).ok()?;
    Some(format!(
        "O Vitral teve um erro interno desde a última abertura; um chat que rodava pode ter sido cortado. O registro ficou em {}.",
        arquivo.display()
    ))
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn o_aviso_sai_uma_vez_so_com_a_marca() {
        let pasta = std::env::temp_dir().join(format!("vitral-queda-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        assert_eq!(aviso_da_anterior(&pasta), None);
        std::fs::write(marca(&pasta.join("queda.log")), "").unwrap();
        let aviso = aviso_da_anterior(&pasta).unwrap();
        assert!(aviso.contains("queda.log"), "{aviso}");
        assert_eq!(aviso_da_anterior(&pasta), None);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_texto_traz_versao_thread_e_lugar() {
        let texto = texto_da_queda("2026-11-17 12:00:00", "main", "deu ruim", "src/motor.rs:10", "rastro");
        assert!(texto.contains("thread main") && texto.contains("deu ruim") && texto.contains("src/motor.rs:10"));
    }
}
