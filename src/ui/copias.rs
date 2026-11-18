//! Os worktrees na paleta "Novo chat · Em qual pasta?": o grupo **Worktrees**, com as cópias dos
//! repositórios das pastas que já têm chat (o que o `git worktree list` de cada uma devolve), o
//! nome da pasta e o ramo embaixo. O Vitral só lista e abre o chat na cópia: não cria nem apaga
//! worktree. A leitura (`git::copias_das_pastas`) roda fora da thread da interface, ao abrir a
//! paleta (`dialogos.rs`).

use std::path::{Path, PathBuf};

use super::componentes::{GrupoDeEscolha, ItemDeEscolha, Simbolo};
use crate::{git::Copia, tema::Lucide};

/// O que a linha de uma cópia diz embaixo do nome: o ramo (ou o commit, com a cabeça solta) e o
/// caminho ("feat/pesagem · ~/eita/wt/pesagem").
fn descricao(copia: &Copia) -> String {
    let onde = super::dialogos::com_til(&copia.caminho);
    match &copia.ramo {
        Some(ramo) => format!("{ramo} · {onde}"),
        None => format!("cabeça solta em {} · {onde}", copia.commit),
    }
}

/// A pasta como a paleta a compara: canônica quando existe (o git e o Vitral podem escrever o
/// mesmo lugar de dois jeitos).
fn canonica(pasta: &Path) -> PathBuf {
    dunce::canonicalize(pasta).unwrap_or_else(|_| pasta.to_path_buf())
}

/// O grupo "Worktrees" da paleta: as cópias que não estão entre as pastas já listadas, com a busca
/// no nome da pasta, no ramo e no caminho. `None` sem cópia para mostrar.
pub(super) fn grupo_das_copias(copias: &[Copia], ja_listadas: &[PathBuf], busca: &str) -> Option<GrupoDeEscolha> {
    let termo = busca.trim().to_lowercase();
    let listadas: Vec<PathBuf> = ja_listadas.iter().map(|p| canonica(p)).collect();
    let itens: Vec<ItemDeEscolha> = copias
        .iter()
        .filter(|c| !listadas.contains(&canonica(&c.caminho)))
        .filter_map(|c| {
            let nome = super::dialogos::nome_da_pasta(&c.caminho);
            let descricao = descricao(c);
            let casa = nome.to_lowercase().contains(&termo) || descricao.to_lowercase().contains(&termo);
            casa.then(|| ItemDeEscolha {
                simbolo: Some(Simbolo::icone(Lucide::GitBranch)),
                descricao: Some(descricao.into()),
                ..ItemDeEscolha::new(c.caminho.display().to_string(), nome)
            })
        })
        .collect();
    (!itens.is_empty()).then(|| GrupoDeEscolha { titulo: Some("Worktrees".into()), itens })
}

#[cfg(test)]
mod testes {
    use std::path::PathBuf;

    use super::grupo_das_copias;
    use crate::git::Copia;

    fn copia(caminho: &str, ramo: Option<&str>) -> Copia {
        Copia { caminho: PathBuf::from(caminho), ramo: ramo.map(str::to_string), commit: "1a2b3c4".into(), apagada: false, principal: false }
    }

    #[test]
    fn o_grupo_mostra_o_ramo_e_acha_pela_pasta_e_pelo_ramo() {
        let copias = [copia("/p/central", Some("main")), copia("/p/wt/pesagem", Some("feat/balanca")), copia("/p/wt/solta", None)];
        let listadas = [PathBuf::from("/p/central")];
        let linhas = |busca: &str| -> Vec<(String, String)> {
            grupo_das_copias(&copias, &listadas, busca)
                .map(|g| g.itens.iter().map(|i| (i.titulo.to_string(), i.descricao.clone().unwrap_or_default().to_string())).collect())
                .unwrap_or_default()
        };
        // A pasta que já está em "Pastas" não repete aqui.
        assert_eq!(
            linhas(""),
            [
                ("pesagem".to_string(), "feat/balanca · /p/wt/pesagem".to_string()),
                ("solta".to_string(), "cabeça solta em 1a2b3c4 · /p/wt/solta".to_string()),
            ]
        );
        assert_eq!(linhas("PESAG").len(), 1);
        assert_eq!(linhas("balanca")[0].0, "pesagem");
        assert_eq!(linhas("solta")[0].0, "solta");
        assert!(linhas("nada disso").is_empty());
        let grupo = grupo_das_copias(&copias, &listadas, "").unwrap();
        assert_eq!(grupo.titulo.as_deref(), Some("Worktrees"));
        assert_eq!(grupo.itens[0].chave.as_ref(), "/p/wt/pesagem");
        assert!(grupo_das_copias(&copias[..1], &listadas, "").is_none());
    }
}
