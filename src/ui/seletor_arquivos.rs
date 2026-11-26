//! Arquivos da pasta do chat para mencionar com "@": a lista (do git ou varrendo a pasta), o filtro
//! (o nome antes da pasta) e as linhas da lista de escolha. E o que o git ignora, para a aba
//! Arquivos com o "Mostrar ignorados" (`listar_com_ignorados`): as pastas só com o nome, lidas por
//! dentro quando são abertas.

use std::path::Path;

use gpui_kit::SharedString;

use super::componentes::{GrupoDeEscolha, ItemDeEscolha, Simbolo};
use crate::tema::Lucide;

/// Quantos arquivos a lista mostra de uma vez (ela é virtual; a busca acha o resto).
const LIMITE: usize = 200;

/// Os arquivos com todas as palavras da busca no caminho; o nome batendo vem antes da pasta
/// batendo, e o mais curto antes. Sem acento e sem caixa dos dois lados, como as outras buscas
/// (`acentos.rs`): "relatorio" acha "relatório.md".
pub(super) fn filtrar_arquivos<'a>(arquivos: &'a [String], busca: &str, limite: usize) -> Vec<&'a String> {
    filtrar_mencoes(arquivos, &[], busca, limite)
}

/// O mesmo filtro sobre os arquivos e as pastas ("central/src/") do "@" da caixa (item 1.2). A
/// chave de cada um é calculada uma vez (são milhares com os repositórios de dentro).
pub(super) fn filtrar_mencoes<'a>(arquivos: &'a [String], pastas: &'a [String], busca: &str, limite: usize) -> Vec<&'a String> {
    let termos = super::acentos::palavras(busca);
    let todos = arquivos.iter().chain(pastas.iter());
    let Some(ultimo) = termos.last() else {
        return todos.take(limite).collect();
    };
    let mut lista: Vec<(bool, usize, &String)> = todos
        .filter_map(|a| {
            let baixo = super::acentos::normalizar(a);
            if !super::acentos::tem_todas(&baixo, &termos) {
                return None;
            }
            let nome = baixo.trim_end_matches('/').rsplit('/').next().unwrap_or(&baixo);
            Some((!nome.contains(ultimo.as_str()), a.len(), a))
        })
        .collect();
    if lista.len() > limite {
        lista.select_nth_unstable_by_key(limite, |(n, t, _)| (*n, *t));
        lista.truncate(limite);
    }
    lista.sort_by_key(|(n, t, _)| (*n, *t));
    lista.into_iter().map(|(_, _, a)| a).collect()
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
    let aviso = |texto: SharedString| {
        let item = ItemDeEscolha { desabilitado: Some(texto.clone()), ..ItemDeEscolha::new("", texto) };
        vec![GrupoDeEscolha { titulo: None, itens: vec![item] }]
    };
    let Some(arquivos) = arquivos else {
        return aviso(tr!("Lendo os arquivos…"));
    };
    if arquivos.is_empty() {
        return aviso(tr!("Nenhum arquivo nesta pasta"));
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

/// Até quantos arquivos a lista guarda (a varredura e os repositórios de dentro param aqui).
pub(super) const TETO: usize = 20_000;

/// Até quantos níveis de repositório dentro de repositório a lista desce.
const NIVEIS_DE_REPOSITORIO: usize = 3;

/// Arquivos da pasta: o que o git conhece (respeita o .gitignore); sem git, uma varredura que pula
/// pastas ocultas e de dependências. Lista vazia do git (repositório novo, ou pasta que o
/// repositório de cima ignora) também cai na varredura. Os repositórios de dentro (uma subpasta
/// com git próprio que o de cima ignora ou não conhece) entram com o prefixo dela: em ~/eita, a raiz
/// é lista-branca, e central/, layout/ e vitral/ ficavam de fora do "@", da árvore e do Mencionar
/// (rodada de uso, item 7.1). Só conta o `.git` que é pasta: o worktree tem `.git` em arquivo e
/// duplicaria tudo. Tudo dentro do mesmo teto.
pub(super) fn listar(pasta: &Path) -> Vec<String> {
    let mut lista = Vec::new();
    listar_em(pasta, "", 0, &mut lista, None);
    lista
}

/// O que o git ignora na pasta, para a aba Arquivos com o "Mostrar ignorados" ligado.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Ignorados {
    /// Os ignorados que aparecem, em ordem: arquivos, e pastas com a barra no fim ("arquivo/"). De
    /// uma pasta ignorada só entra o que está dentro depois de ela ser aberta na árvore.
    pub entradas: Vec<String>,
    /// As pastas ignoradas já lidas (uma que não tem nada dentro aparece vazia, e não "lendo").
    pub carregadas: std::collections::BTreeSet<String>,
}

/// Os arquivos da pasta (como `listar`) e os ignorados pelo git. Uma pasta ignorada entra só com
/// o nome: `node_modules` e `target` têm dezenas de milhares de arquivos. O que há dentro é lido
/// um nível por vez, só das pastas de `abertas` (as que a pessoa abriu na árvore), e tudo para em
/// `teto` entradas, somando as duas listas.
pub(super) fn listar_com_ignorados(pasta: &Path, abertas: &std::collections::BTreeSet<String>, teto: usize) -> (Vec<String>, Ignorados) {
    let mut lista = Vec::new();
    let mut entradas = Vec::new();
    listar_em(pasta, "", 0, &mut lista, Some(&mut entradas));
    lista.truncate(teto);
    entradas.sort();
    entradas.dedup();
    entradas.truncate(teto - lista.len());
    // As pastas que podem ser abertas: as ignoradas e as que aparecem dentro de uma delas.
    let mut conhecidas: std::collections::HashSet<String> = entradas.iter().filter(|e| e.ends_with('/')).cloned().collect();
    let mut carregadas = std::collections::BTreeSet::new();
    // De fora para dentro: na ordem do conjunto, a pasta vem antes das de dentro dela.
    for aberta in abertas {
        if !conhecidas.contains(aberta) {
            continue;
        }
        let Ok(dentro) = std::fs::read_dir(pasta.join(aberta.trim_end_matches('/'))) else {
            continue;
        };
        let mut filhos: Vec<String> = dentro
            .flatten()
            .filter_map(|entrada| {
                let nome = entrada.file_name().to_string_lossy().to_string();
                // O `.git` de um worktree ou de um repositório de dentro não é arquivo de trabalho.
                (nome != ".git").then(|| if entrada.path().is_dir() { format!("{aberta}{nome}/") } else { format!("{aberta}{nome}") })
            })
            .collect();
        filhos.sort();
        carregadas.insert(aberta.clone());
        for filho in filhos {
            if lista.len() + entradas.len() >= teto {
                break;
            }
            if filho.ends_with('/') {
                conhecidas.insert(filho.clone());
            }
            entradas.push(filho);
        }
    }
    entradas.sort();
    (lista, Ignorados { entradas, carregadas })
}

/// `ignorados`, quando vem, junta o que a lista deixa de fora: o que o git ignora (a pasta inteira
/// como "pasta/", sem descer nela) e, na varredura, as pastas e os arquivos pulados.
fn listar_em(pasta: &Path, prefixo: &str, nivel: usize, lista: &mut Vec<String>, mut ignorados: Option<&mut Vec<String>>) {
    // -z: sem ele, "relatório.md" sai como "relat\303\263rio.md", entre aspas.
    let ls_files = |extras: &[&str]| -> Option<Vec<String>> {
        let saida = crate::git::git_em(pasta).args(["ls-files", "-z", "--others", "--exclude-standard"]).args(extras).output().ok()?;
        saida.status.success().then(|| String::from_utf8_lossy(&saida.stdout).split('\0').filter(|a| !a.is_empty()).map(str::to_string).collect())
    };
    if let Some(do_git) = ls_files(&["--cached"]).filter(|l| !l.is_empty()) {
        // Um repositório de dentro que o de cima não conhece aparece como pasta ("proj/").
        let mut de_dentro: Vec<String> = Vec::new();
        for entrada in do_git {
            if entrada.ends_with('/') {
                de_dentro.push(entrada);
            } else if lista.len() < TETO {
                lista.push(format!("{prefixo}{entrada}"));
            }
        }
        // E os que ele ignora: as pastas ignoradas que têm `.git` (pasta) dentro.
        let desce = nivel < NIVEIS_DE_REPOSITORIO;
        if desce || ignorados.is_some() {
            for entrada in ls_files(&["--ignored", "--directory"]).unwrap_or_default() {
                if entrada.ends_with('/') {
                    de_dentro.push(entrada);
                } else if let Some(ignorados) = ignorados.as_deref_mut() {
                    ignorados.push(format!("{prefixo}{entrada}"));
                }
            }
            de_dentro.sort();
            de_dentro.dedup();
            for sub in de_dentro {
                let caminho = pasta.join(sub.trim_end_matches('/'));
                if desce && caminho.join(".git").is_dir() {
                    if lista.len() < TETO {
                        listar_em(&caminho, &format!("{prefixo}{sub}"), nivel + 1, lista, ignorados.as_deref_mut());
                    }
                } else if let Some(ignorados) = ignorados.as_deref_mut() {
                    // A pasta ignorada (ou o worktree, que a lista pula) entra só com o nome.
                    ignorados.push(format!("{prefixo}{sub}"));
                }
            }
        }
        return;
    }
    let mut achados = Vec::new();
    let mut pendentes = vec![pasta.to_path_buf()];
    while let Some(atual) = pendentes.pop() {
        let Ok(entradas) = std::fs::read_dir(&atual) else {
            continue;
        };
        for entrada in entradas.flatten() {
            let nome = entrada.file_name().to_string_lossy().to_string();
            let caminho = entrada.path();
            // O que a varredura pula conta como ignorado: a pasta só com o nome, o arquivo oculto inteiro.
            let mut pulado = |e_pasta: bool| {
                if let (Some(ignorados), Ok(relativo)) = (ignorados.as_deref_mut(), caminho.strip_prefix(pasta))
                    && nome != ".git"
                {
                    let barra = if e_pasta { "/" } else { "" };
                    ignorados.push(format!("{prefixo}{}{barra}", crate::git::caminho_em_texto(relativo)));
                }
            };
            if nome.starts_with('.')
                || matches!(nome.as_str(), "node_modules" | "target" | "dist" | "build" | "__pycache__")
            {
                pulado(caminho.is_dir());
                continue;
            }
            if caminho.is_dir() {
                // Ambiente virtual do Python (venv, env...), seja qual for o nome.
                if !caminho.join("pyvenv.cfg").exists() {
                    pendentes.push(caminho);
                } else {
                    pulado(true);
                }
            } else if let Ok(relativo) = caminho.strip_prefix(pasta) {
                // Com `/`, como a lista do git (no Windows o caminho viria com `\`).
                achados.push(format!("{prefixo}{}", crate::git::caminho_em_texto(relativo)));
            }
            if lista.len() + achados.len() >= TETO {
                lista.append(&mut achados);
                return;
            }
        }
    }
    achados.sort();
    lista.append(&mut achados);
}

/// As pastas que os arquivos da lista mostram ("central/src/"), sem repetir, para o "@" mencionar
/// uma pasta inteira (rodada de uso, item 1.2).
pub(super) fn pastas_dos_arquivos(arquivos: &[String]) -> Vec<String> {
    let mut pastas: std::collections::BTreeSet<String> = Default::default();
    for arquivo in arquivos {
        let mut fim = arquivo.len();
        while let Some(barra) = arquivo[..fim].rfind('/') {
            if !pastas.insert(arquivo[..=barra].to_string()) {
                break;
            }
            fim = barra;
        }
    }
    pastas.into_iter().collect()
}

#[cfg(test)]
mod testes {
    use std::path::Path;

    use super::{filtrar_arquivos, grupos_de_arquivos, icone_do_arquivo, listar, pastas_dos_arquivos};
    use crate::{git::novo_git, tema::Lucide};

    fn git(pasta: &Path, args: &[&str]) {
        let saida = novo_git().arg("-C").arg(pasta).args(args).output().unwrap();
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
    fn desce_nos_repositorios_de_dentro_e_pula_o_worktree() {
        let raiz = std::env::temp_dir().join(format!("vitral-aninhados-{}", crate::modelo::novo_id()));
        let projeto = raiz.join("central");
        std::fs::create_dir_all(projeto.join("src")).unwrap();
        std::fs::create_dir_all(raiz.join("wt/outra")).unwrap();
        std::fs::create_dir_all(raiz.join("solto/src")).unwrap();
        // A raiz é lista-branca, como ~/eita: ignora tudo e libera um nome só.
        git(&raiz, &["init", "-q"]);
        std::fs::write(raiz.join(".gitignore"), "*\n!.gitignore\n!AGENTS.md\n").unwrap();
        std::fs::write(raiz.join("AGENTS.md"), "x").unwrap();
        git(&projeto, &["init", "-q"]);
        std::fs::write(projeto.join("src/main.rs"), "x").unwrap();
        // O worktree tem `.git` em arquivo: não entra (duplicaria tudo).
        std::fs::write(raiz.join("wt/outra/.git"), "gitdir: /x").unwrap();
        std::fs::write(raiz.join("wt/outra/a.rs"), "x").unwrap();
        // Pasta ignorada sem git próprio também fica de fora.
        std::fs::write(raiz.join("solto/src/b.rs"), "x").unwrap();
        let mut lista = listar(&raiz);
        lista.sort();
        assert_eq!(lista, [".gitignore", "AGENTS.md", "central/src/main.rs"]);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn mencoes_com_arquivos_e_pastas() {
        use super::filtrar_mencoes;
        let arquivos: Vec<String> = ["central/src/motor.rs", "central/src/ui/motor_ui.rs", "layout/motor.twig"].map(String::from).to_vec();
        let pastas = pastas_dos_arquivos(&arquivos);
        // A pasta casa pelo nome dela, sem a barra do fim.
        assert_eq!(filtrar_mencoes(&arquivos, &pastas, "ui", 8)[0], "central/src/ui/");
        // O nome do arquivo antes da pasta, e o mais curto antes; o limite corta.
        assert_eq!(filtrar_mencoes(&arquivos, &pastas, "motor", 2), ["layout/motor.twig", "central/src/motor.rs"]);
        // Sem nada digitado, os primeiros da lista.
        assert_eq!(filtrar_mencoes(&arquivos, &pastas, "", 2).len(), 2);
    }

    #[test]
    fn pastas_dos_arquivos_sem_repetir() {
        let arquivos: Vec<String> = ["central/src/a.rs", "central/src/b.rs", "central/x.md", "raiz.md"].map(String::from).to_vec();
        assert_eq!(pastas_dos_arquivos(&arquivos), ["central/", "central/src/"]);
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

    #[test]
    fn a_busca_de_arquivo_nao_liga_para_acento() {
        let arquivos: Vec<String> = ["docs/relatório.md", "src/configuração/mod.rs", "src/main.rs"].map(String::from).to_vec();
        assert_eq!(filtrar_arquivos(&arquivos, "relatorio", 10), ["docs/relatório.md"]);
        assert_eq!(filtrar_arquivos(&arquivos, "CONFIGURACAO mod", 10), ["src/configuração/mod.rs"]);
        // E quem escreve com acento acha o nome sem ele.
        assert_eq!(filtrar_arquivos(&["docs/relatorio.md".to_string()], "relatório", 10), ["docs/relatorio.md"]);
    }

    #[test]
    fn os_ignorados_entram_pela_pasta_e_so_abrem_quando_pedidos() {
        use super::listar_com_ignorados;
        use std::collections::BTreeSet;
        let raiz = std::env::temp_dir().join(format!("vitral-ignorados-{}", crate::modelo::novo_id()));
        for pasta in ["arquivo/prints", "node_modules/pacote", "src"] {
            std::fs::create_dir_all(raiz.join(pasta)).unwrap();
        }
        git(&raiz, &["init", "-q"]);
        std::fs::write(raiz.join(".gitignore"), "arquivo/\nnode_modules/\n*.log\n").unwrap();
        for arquivo in ["src/a.rs", "arquivo/print.png", "arquivo/prints/tela.png", "node_modules/pacote/index.js", "saida.log"] {
            std::fs::write(raiz.join(arquivo), "x").unwrap();
        }
        let abertas = |pastas: &[&str]| pastas.iter().map(|p| p.to_string()).collect::<BTreeSet<String>>();
        // Fechadas, as pastas ignoradas entram só com o nome: nada de dentro do node_modules.
        let (lista, ignorados) = listar_com_ignorados(&raiz, &abertas(&[]), 100);
        let mut lista = lista;
        lista.sort();
        assert_eq!(lista, [".gitignore", "src/a.rs"]);
        assert_eq!(ignorados.entradas, ["arquivo/", "node_modules/", "saida.log"]);
        assert!(ignorados.carregadas.is_empty());
        // Aberta, a pasta mostra um nível: os arquivos e as pastas dela, ainda fechadas.
        let (_, ignorados) = listar_com_ignorados(&raiz, &abertas(&["arquivo/"]), 100);
        assert_eq!(ignorados.entradas, ["arquivo/", "arquivo/print.png", "arquivo/prints/", "node_modules/", "saida.log"]);
        assert_eq!(ignorados.carregadas, abertas(&["arquivo/"]));
        // A de dentro abre do mesmo jeito; a que não é ignorada (ou não existe) não é lida por aqui.
        let (_, ignorados) = listar_com_ignorados(&raiz, &abertas(&["arquivo/", "arquivo/prints/", "src/", "nada/"]), 100);
        assert!(ignorados.entradas.contains(&"arquivo/prints/tela.png".to_string()));
        assert!(!ignorados.entradas.iter().any(|e| e.starts_with("src/") || e.starts_with("nada/")));
        assert_eq!(ignorados.carregadas, abertas(&["arquivo/", "arquivo/prints/"]));
        // O teto vale para a soma: com 2 da lista e teto de 4, entram só 2 ignorados.
        let (lista, ignorados) = listar_com_ignorados(&raiz, &abertas(&["arquivo/", "node_modules/"]), 4);
        assert_eq!(lista.len() + ignorados.entradas.len(), 4);
        let _ = std::fs::remove_dir_all(&raiz);
    }

    #[test]
    fn sem_git_as_pastas_puladas_sao_as_ignoradas() {
        use super::listar_com_ignorados;
        let raiz = std::env::temp_dir().join(format!("vitral-ignorados-sem-git-{}", crate::modelo::novo_id()));
        let projeto = raiz.join("projeto");
        std::fs::create_dir_all(projeto.join("node_modules/pacote")).unwrap();
        std::fs::create_dir_all(projeto.join("src")).unwrap();
        std::fs::write(projeto.join("node_modules/pacote/index.js"), "x").unwrap();
        std::fs::write(projeto.join("src/a.rs"), "x").unwrap();
        std::fs::write(projeto.join(".env"), "x").unwrap();
        // O repositório de cima ignora o projeto: vale a varredura, que pula o node_modules e o oculto.
        git(&raiz, &["init", "-q"]);
        std::fs::write(raiz.join(".gitignore"), "projeto/\n").unwrap();
        let (lista, ignorados) = listar_com_ignorados(&projeto, &Default::default(), 100);
        assert_eq!(lista, ["src/a.rs"]);
        assert_eq!(ignorados.entradas, [".env", "node_modules/"]);
        let _ = std::fs::remove_dir_all(&raiz);
    }
}
