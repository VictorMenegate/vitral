//! Arquivos do projeto para mencionar com "@": a lista (do git ou varrendo a pasta), o filtro
//! (o nome antes da pasta) e as linhas da lista de escolha.

use std::path::Path;

use super::componentes::{GrupoDeEscolha, ItemDeEscolha, Simbolo};
use crate::tema::Lucide;

/// Quantos arquivos a lista mostra de uma vez (ela é virtual; a busca acha o resto).
const LIMITE: usize = 200;

/// Os arquivos com todas as palavras da busca no caminho; o nome batendo vem antes da pasta
/// batendo, e o mais curto antes.
pub(super) fn filtrar_arquivos<'a>(arquivos: &'a [String], busca: &str, limite: usize) -> Vec<&'a String> {
    let busca = busca.to_lowercase();
    let termos: Vec<&str> = busca.split_whitespace().collect();
    let mut lista: Vec<&String> = arquivos
        .iter()
        .filter(|a| {
            let a = a.to_lowercase();
            termos.iter().all(|t| a.contains(t))
        })
        .collect();
    if let Some(ultimo) = termos.last() {
        lista.sort_by_key(|a| {
            let nome = a.rsplit('/').next().unwrap_or(a).to_lowercase();
            (!nome.contains(ultimo), a.len())
        });
    }
    lista.truncate(limite);
    lista
}

/// O ícone pelo tipo: código, texto, imagem ou o resto.
pub(super) fn icone_do_arquivo(nome: &str) -> Lucide {
    let extensao = nome.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
    match extensao.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" | "bmp" | "avif" => Lucide::FileImage,
        "md" | "markdown" | "txt" | "rst" => Lucide::FileText,
        "rs" | "py" | "js" | "mjs" | "ts" | "tsx" | "jsx" | "go" | "java" | "kt" | "c" | "h" | "cpp" | "hpp"
        | "cs" | "rb" | "php" | "swift" | "sh" | "html" | "css" | "scss" | "vue" | "svelte" | "json" | "toml"
        | "yaml" | "yml" | "sql" | "lua" | "dart" | "zig" | "ex" | "exs" => Lucide::FileCode,
        _ => Lucide::File,
    }
}

/// As linhas da lista: `None` enquanto lê; a pasta vazia e a leitura pela metade viram uma linha
/// desabilitada que explica (o vazio da busca é o da lista de escolha).
pub(super) fn grupos_de_arquivos(arquivos: Option<&[String]>, busca: &str) -> Vec<GrupoDeEscolha> {
    let aviso = |texto: &str| {
        let item = ItemDeEscolha { desabilitado: Some(texto.to_string().into()), ..ItemDeEscolha::new("", texto.to_string()) };
        vec![GrupoDeEscolha { titulo: None, itens: vec![item] }]
    };
    let Some(arquivos) = arquivos else {
        return aviso("Lendo os arquivos…");
    };
    if arquivos.is_empty() {
        return aviso("Nenhum arquivo nesta pasta");
    }
    let itens: Vec<ItemDeEscolha> = filtrar_arquivos(arquivos, busca, LIMITE)
        .into_iter()
        .map(|caminho| {
            let (pasta, nome) = caminho.rsplit_once('/').unwrap_or(("", caminho));
            ItemDeEscolha {
                simbolo: Some(Simbolo::icone(icone_do_arquivo(nome))),
                descricao: (!pasta.is_empty()).then(|| pasta.to_string().into()),
                ..ItemDeEscolha::new(caminho.clone(), nome.to_string())
            }
        })
        .collect();
    if itens.is_empty() {
        return Vec::new();
    }
    vec![GrupoDeEscolha { titulo: None, itens }]
}

/// Arquivos do projeto: o que o git conhece (respeita o .gitignore); sem git, uma varredura que
/// pula pastas ocultas e de dependências. Lista vazia do git (repositório novo, ou projeto que o
/// repositório de cima ignora) também cai na varredura.
pub(super) fn listar(pasta: &Path) -> Vec<String> {
    // -z: sem ele, "relatório.md" sai como "relat\303\263rio.md", entre aspas.
    let saida = crate::processos::novo_comando("git")
        .arg("-C")
        .arg(pasta)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard"])
        .output();
    if let Ok(saida) = saida
        && saida.status.success()
    {
        let lista: Vec<String> = String::from_utf8_lossy(&saida.stdout)
            .split('\0')
            .filter(|a| !a.is_empty())
            .map(str::to_string)
            .collect();
        if !lista.is_empty() {
            return lista;
        }
    }
    let mut lista = Vec::new();
    let mut pendentes = vec![pasta.to_path_buf()];
    while let Some(atual) = pendentes.pop() {
        let Ok(entradas) = std::fs::read_dir(&atual) else {
            continue;
        };
        for entrada in entradas.flatten() {
            let nome = entrada.file_name().to_string_lossy().to_string();
            if nome.starts_with('.')
                || matches!(nome.as_str(), "node_modules" | "target" | "dist" | "build" | "__pycache__")
            {
                continue;
            }
            let caminho = entrada.path();
            if caminho.is_dir() {
                // Ambiente virtual do Python (venv, env...), seja qual for o nome.
                if !caminho.join("pyvenv.cfg").exists() {
                    pendentes.push(caminho);
                }
            } else if let Ok(relativo) = caminho.strip_prefix(pasta) {
                lista.push(relativo.to_string_lossy().to_string());
            }
            if lista.len() > 20_000 {
                return lista;
            }
        }
    }
    lista.sort();
    lista
}

#[cfg(test)]
mod testes {
    use std::path::Path;

    use super::{filtrar_arquivos, grupos_de_arquivos, icone_do_arquivo, listar};
    use crate::{processos::novo_comando, tema::Lucide};

    fn git(pasta: &Path, args: &[&str]) {
        let saida = novo_comando("git").arg("-C").arg(pasta).args(args).output().unwrap();
        assert!(saida.status.success(), "{}", String::from_utf8_lossy(&saida.stderr));
    }

    #[test]
    fn nomes_com_acento_e_projeto_ignorado_pelo_repositorio_de_cima() {
        let raiz = std::env::temp_dir().join(format!("vitral-listar-{}", crate::modelo::novo_id()));
        let projeto = raiz.join("projeto");
        std::fs::create_dir_all(projeto.join("venv")).unwrap();
        std::fs::write(projeto.join("relatório.md"), "x").unwrap();
        std::fs::write(projeto.join("venv/pyvenv.cfg"), "x").unwrap();
        // O repositório de cima ignora o projeto: o git devolve lista vazia e vale a varredura.
        git(&raiz, &["init", "-q"]);
        std::fs::write(raiz.join(".gitignore"), "projeto/\n").unwrap();
        assert_eq!(listar(&projeto), ["relatório.md"]);
        // Com repositório próprio, a lista vem do git, com o nome legível.
        git(&projeto, &["init", "-q"]);
        std::fs::write(projeto.join(".gitignore"), "venv/\n").unwrap();
        let mut lista = listar(&projeto);
        lista.sort();
        assert_eq!(lista, [".gitignore", "relatório.md"]);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn nome_antes_da_pasta_e_ate_o_limite() {
        let arquivos: Vec<String> =
            ["cupom/LEIA-ME.md", "src/checkout/cupom.rs", "assets/logo.png"].map(String::from).to_vec();
        assert_eq!(filtrar_arquivos(&arquivos, "cupom", 10), ["src/checkout/cupom.rs", "cupom/LEIA-ME.md"]);
        assert_eq!(filtrar_arquivos(&arquivos, "CHECKOUT cupom", 10), ["src/checkout/cupom.rs"]);
        assert_eq!(filtrar_arquivos(&arquivos, "", 2).len(), 2);
        assert!(filtrar_arquivos(&arquivos, "nada", 10).is_empty());
    }

    #[test]
    fn linhas_com_o_icone_do_tipo_e_os_vazios() {
        assert_eq!(icone_do_arquivo("cupom.rs"), Lucide::FileCode);
        assert_eq!(icone_do_arquivo("LEIA-ME.MD"), Lucide::FileText);
        assert_eq!(icone_do_arquivo("logo.png"), Lucide::FileImage);
        assert_eq!(icone_do_arquivo("Makefile"), Lucide::File);
        let lendo = grupos_de_arquivos(None, "");
        assert!(lendo[0].itens[0].desabilitado.is_some());
        let vazio = grupos_de_arquivos(Some(&[]), "");
        assert_eq!(vazio[0].itens[0].titulo.as_ref(), "Nenhum arquivo nesta pasta");
        let arquivos = ["src/checkout/cupom.rs".to_string()];
        let grupos = grupos_de_arquivos(Some(&arquivos), "cupom");
        let item = &grupos[0].itens[0];
        assert_eq!((item.chave.as_ref(), item.titulo.as_ref()), ("src/checkout/cupom.rs", "cupom.rs"));
        assert_eq!(item.descricao.as_ref().map(|d| d.to_string()), Some("src/checkout".to_string()));
        assert!(grupos_de_arquivos(Some(&arquivos), "nada").is_empty());
    }
}
