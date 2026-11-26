//! O ramo do git à vista em cada chat: o texto dele para as dicas (a linha do chat na lista e a
//! aba). O cabeçalho da seção Na pasta mostra o ramo que o retrato do git traz (`git_da_pasta.rs`).
//! O ramo vem dos arquivos do git (`git::ramo_no_disco`), sem abrir processo. Não há aviso quando a
//! pasta troca de ramo (dono, 18/11/2026: nada de avisos novos).

use std::path::Path;

use crate::git::RamoDaPasta;

/// Onde a pasta está, numa frase curta: "No ramo main", "No ramo feat/x, num worktree", "Com a
/// cabeça solta em 1a2b3c4".
pub(super) fn onde_esta(ramo: &RamoDaPasta) -> String {
    let base = if ramo.solta { tr!("Com a cabeça solta em {nome}", nome = ramo.nome).to_string() } else { tr!("No ramo {nome}", nome = ramo.nome).to_string() };
    if ramo.worktree { tr!("{base}, num worktree", base = base).to_string() } else { base }
}

/// O texto de uma dica com o ramo da pasta numa linha a mais (nada, fora de um repositório). Lê o
/// disco: é para chamar na hora em que a dica aparece, não a cada quadro.
pub(super) fn com_ramo(texto: &str, pasta: &Path) -> String {
    match crate::git::ramo_no_disco(pasta) {
        Some(ramo) => format!("{texto}\n{}", onde_esta(&ramo)),
        None => texto.to_string(),
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use super::onde_esta;
    use crate::git::RamoDaPasta;

    fn ramo(nome: &str, solta: bool, worktree: bool) -> RamoDaPasta {
        RamoDaPasta { nome: nome.into(), solta, worktree }
    }

    #[test]
    fn o_ramo_numa_frase() {
        assert_eq!(onde_esta(&ramo("main", false, false)), "No ramo main");
        assert_eq!(onde_esta(&ramo("feat/x", false, true)), "No ramo feat/x, num worktree");
        assert_eq!(onde_esta(&ramo("1a2b3c4", true, false)), "Com a cabeça solta em 1a2b3c4");
    }
}
