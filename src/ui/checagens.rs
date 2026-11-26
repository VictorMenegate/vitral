//! As checagens que um turno rodou (teste, typecheck, lint e build) e como cada uma terminou,
//! para a linha "Trabalhou…" do turno pronto (rodada do Paseo, 23/11/2026, item 3): "tsc passou ·
//! vitest falhou 2×, passou · cargo test passou". Vale a última execução de cada uma, e as falhas
//! de antes viram "falhou N×".
//!
//! Tudo sai das chamadas de Bash do turno, na hora, sem gravar nada: o comando e se a chamada
//! falhou (`Item::Ferramenta.erro`, que o Claude Code marca quando o comando sai com código
//! diferente de zero). Só conta o que dá para saber:
//!
//! - o comando conta por ele mesmo quando o código de saída é o dele: nada depois dele além de
//!   outros comandos ligados por `&&`;
//! - com um `|` na frente (`cargo test 2>&1 | tail -40`), um `;` ou um `||` depois, o código de
//!   saída é de outro comando: aí vale o que a saída diz (a linha "test result:" do cargo, "error
//!   TS" do tsc, "Test Files" do vitest…), e sem sinal nenhum na saída a checagem fica de fora,
//!   em vez de aparecer como "passou" sem ter passado.

use crate::modelo::Item;

/// Uma checagem do turno: o nome ("cargo test", "tsc", "npm run build"), como a última execução
/// terminou e quantas vezes falhou no turno (a última também conta, se falhou).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Checagem {
    pub nome: String,
    pub passou: bool,
    pub falhas: usize,
}

impl Checagem {
    /// "tsc passou", "vitest falhou 2×, passou", "cargo test falhou", "cargo test falhou 3×".
    pub fn texto(&self) -> String {
        match (self.passou, self.falhas) {
            (true, 0) => crate::tr!("{nome} passou", nome = self.nome).to_string(),
            (true, n) => crate::tr!("{nome} falhou {n}×, passou", nome = self.nome, n = n).to_string(),
            (false, 0 | 1) => crate::tr!("{nome} falhou", nome = self.nome).to_string(),
            (false, n) => crate::tr!("{nome} falhou {n}×", nome = self.nome, n = n).to_string(),
        }
    }
}

/// O trecho de `texto` (o de uma checagem) que diz a falha, para a tela pôr em vermelho:
/// "falhou" ou "falhou 2×".
pub(super) fn trecho_da_falha(texto: &str) -> Option<std::ops::Range<usize>> {
    let inicio = texto.find(&format!(" {}", crate::tr!("falhou")))? + 1;
    let fim = texto[inicio..].find(',').map_or(texto.len(), |v| inicio + v);
    Some(inicio..fim)
}

/// As checagens das chamadas de Bash de `itens` (o trecho de um turno), na ordem em que cada uma
/// apareceu pela primeira vez.
pub(super) fn checagens_do_turno(itens: &[Item]) -> Vec<Checagem> {
    let mut saida: Vec<Checagem> = Vec::new();
    for item in itens {
        let Item::Ferramenta { nome, resumo, detalhe, resultado: Some(resultado), erro, .. } = item else { continue };
        if nome != "Bash" {
            continue;
        }
        // O comando que foi para o segundo plano ainda não terminou: o resultado é só o aviso.
        if resultado.starts_with("Command running in background") || resultado.contains("was moved to the background") {
            continue;
        }
        let comando = detalhe.as_deref().and_then(|d| d.strip_prefix("$ ")).or_else(|| resumo.strip_prefix("Bash · ")).unwrap_or(resumo);
        for (nome, passou) in resultados_do_comando(comando, resultado, *erro) {
            match saida.iter_mut().find(|c| c.nome == nome) {
                Some(c) => {
                    c.passou = passou;
                    c.falhas += usize::from(!passou);
                }
                None => saida.push(Checagem { nome, passou, falhas: usize::from(!passou) }),
            }
        }
    }
    saida
}

/// As checagens de um comando e se cada uma passou, com a saída e o `erro` da chamada.
fn resultados_do_comando(comando: &str, saida: &str, erro: bool) -> Vec<(String, bool)> {
    let mut checagens: Vec<(String, bool)> = Vec::new();
    for (nome, confiavel) in checagens_do_comando(comando) {
        if !checagens.iter().any(|(n, _)| *n == nome) {
            checagens.push((nome, confiavel));
        }
    }
    let sinais: Vec<Option<bool>> = checagens.iter().map(|(nome, _)| sinal_na_saida(nome, saida)).collect();
    // Numa cadeia de `&&` que falhou, a saída diz até onde ela foi: a última checagem que deixou
    // sinal rodou, então as de antes dela passaram; e se ela falhou, as de depois nem rodaram.
    let ultima_que_rodou = sinais.iter().rposition(Option::is_some);
    let confiaveis = checagens.iter().filter(|(_, c)| *c).count();
    let mut resultados = Vec::new();
    for (i, ((nome, confiavel), sinal)) in checagens.into_iter().zip(&sinais).enumerate() {
        let passou = match (confiavel, erro) {
            (true, false) => Some(true),
            (true, true) if confiaveis == 1 => Some(false),
            (true, true) => match ultima_que_rodou {
                Some(ultima) if i < ultima => Some(true),
                Some(ultima) if i == ultima => *sinal,
                Some(ultima) if sinais[ultima] == Some(false) => None,
                // Sem sinal nenhum (ou depois de uma que passou): não dá para saber qual foi.
                _ => Some(false),
            },
            (false, _) => *sinal,
        };
        if let Some(passou) = passou {
            resultados.push((nome, passou));
        }
    }
    resultados
}

/// Os pedaços de um comando de shell e o operador que vem depois de cada um ("&&", "||", "|",
/// ";" ou "" no fim), sem cortar dentro de aspas.
fn pedacos(comando: &str) -> Vec<(String, &'static str)> {
    let mut saida = Vec::new();
    let mut atual = String::new();
    let mut aspas: Option<char> = None;
    let mut letras = comando.chars().peekable();
    while let Some(c) = letras.next() {
        if let Some(a) = aspas {
            if c == a {
                aspas = None;
            }
            atual.push(c);
            continue;
        }
        let operador = match c {
            '\'' | '"' => {
                aspas = Some(c);
                None
            }
            '\\' => {
                // A barra no fim da linha continua o comando; antes de outra letra, vale a letra.
                if let Some(seguinte) = letras.next()
                    && seguinte != '\n'
                {
                    atual.push(seguinte);
                }
                continue;
            }
            '&' if letras.peek() == Some(&'&') => {
                letras.next();
                Some("&&")
            }
            // "2>&1" e "&>arquivo" são redirecionamentos; o "&" sozinho põe no fundo.
            '&' if atual.ends_with('>') || letras.peek() == Some(&'>') => None,
            '&' => Some(";"),
            '|' if letras.peek() == Some(&'|') => {
                letras.next();
                Some("||")
            }
            '|' => {
                if letras.peek() == Some(&'&') {
                    letras.next();
                }
                Some("|")
            }
            ';' | '\n' => Some(";"),
            _ => None,
        };
        match operador {
            Some(operador) => saida.push((std::mem::take(&mut atual), operador)),
            None => atual.push(c),
        }
    }
    saida.push((atual, ""));
    saida.retain(|(pedaco, _)| !pedaco.trim().is_empty());
    saida
}

/// As checagens de um comando: o nome de cada uma e se o código de saída da chamada é o dela
/// (nada depois dela além de `&&`).
fn checagens_do_comando(comando: &str) -> Vec<(String, bool)> {
    let pedacos = pedacos(comando);
    let mut saida = Vec::new();
    for (i, (pedaco, _)) in pedacos.iter().enumerate() {
        if let Some(nome) = checagem_do_pedaco(pedaco) {
            let confiavel = pedacos[i..].iter().all(|(_, operador)| matches!(*operador, "&&" | ""));
            saida.push((nome, confiavel));
        }
    }
    saida
}

/// A checagem que um comando simples é, pelo programa e pelo subcomando: `cargo test`, `check`,
/// `clippy`, `build` e `nextest`; `tsc`, `vitest`, `jest`, `pytest`; `go test`, `build` e `vet`;
/// e os scripts `test`, `build`, `typecheck` e `lint` do npm, pnpm, yarn e bun. Na frente passam
/// as variáveis (`CI=1`), o `timeout`, o `npx` e os parentes deles.
fn checagem_do_pedaco(pedaco: &str) -> Option<String> {
    let mut palavras = pedaco.split_whitespace().map(|p| p.trim_start_matches(['(', '{'])).filter(|p| !p.is_empty()).peekable();
    // As variáveis e os comandos que só embrulham outro.
    let programa = loop {
        let palavra = palavras.next()?;
        let nome = palavra.rsplit('/').next().unwrap_or(palavra);
        let e_variavel = palavra.split_once('=').is_some_and(|(chave, _)| !chave.is_empty() && chave.chars().all(|c| c.is_alphanumeric() || c == '_'));
        if e_variavel {
            continue;
        }
        match nome {
            "time" | "env" | "sudo" | "exec" | "command" | "npx" | "bunx" | "pnpx" | "uvx" | "nice" | "timeout" | "ionice" => {
                // As opções deles e o número que as acompanha ("-n 19", "600", "10m").
                while palavras.peek().is_some_and(|p| p.starts_with('-') || p.starts_with(|c: char| c.is_ascii_digit())) {
                    palavras.next();
                }
            }
            "pnpm" | "yarn" | "npm" | "bun" if matches!(palavras.peek(), Some(&("exec" | "dlx" | "x"))) => {
                palavras.next();
                while palavras.peek().is_some_and(|p| p.starts_with('-')) {
                    palavras.next();
                }
            }
            "uv" | "poetry" | "pipenv" | "pdm" if palavras.peek() == Some(&"run") => {
                palavras.next();
            }
            "python" | "python3" if palavras.peek() == Some(&"-m") => {
                palavras.next();
            }
            _ => break nome.trim_end_matches(')'),
        }
    };
    // O subcomando: a primeira palavra que não é opção (nem o `+nightly` do cargo).
    let mut subcomando = || {
        while let Some(p) = palavras.next() {
            // As opções que levam um valor.
            if matches!(p, "-C" | "--prefix" | "--filter" | "-F" | "--dir" | "--cwd" | "--manifest-path") {
                palavras.next();
            } else if !p.starts_with('-') && !p.starts_with('+') {
                return Some(p.trim_end_matches(')'));
            }
        }
        None
    };
    match programa {
        "cargo" => match subcomando()? {
            "test" | "t" => Some("cargo test".into()),
            "check" | "c" => Some("cargo check".into()),
            "build" | "b" => Some("cargo build".into()),
            "clippy" => Some("cargo clippy".into()),
            "nextest" => Some("cargo nextest".into()),
            _ => None,
        },
        "tsc" | "vitest" | "jest" | "pytest" => Some(programa.to_string()),
        "py.test" => Some("pytest".into()),
        "go" => match subcomando()? {
            sub @ ("test" | "build" | "vet") => Some(format!("go {sub}")),
            _ => None,
        },
        "npm" | "pnpm" | "yarn" | "bun" => {
            let mut script = subcomando()?;
            if matches!(script, "run" | "run-script") {
                script = subcomando()?;
            }
            let base = script.split(':').next().unwrap_or(script);
            match base {
                "test" if script == "test" => Some(format!("{programa} test")),
                "test" | "build" | "typecheck" | "lint" => Some(format!("{programa} run {script}")),
                _ => None,
            }
        }
        _ => None,
    }
}

/// O que a saída diz de uma checagem, quando o código de saída da chamada não é o dela: `true`
/// passou, `false` falhou, `None` sem sinal (aí ela fica de fora).
fn sinal_na_saida(nome: &str, saida: &str) -> Option<bool> {
    let tem = |trecho: &str| saida.contains(trecho);
    let linha_com = |a: &str, b: &str| saida.lines().any(|l| l.contains(a) && l.contains(b));
    let comeca_com = |trecho: &str| saida.lines().any(|l| l.trim_start().starts_with(trecho));
    let de = |falhou: bool, passou: bool| if falhou { Some(false) } else { passou.then_some(true) };
    match nome {
        "cargo test" | "cargo nextest" => de(
            tem("test result: FAILED") || tem("error: could not compile") || tem("error[E") || tem("error: test failed") || linha_com("tests run:", " failed"),
            tem("test result: ok") || linha_com("tests run:", " passed"),
        ),
        "cargo check" | "cargo build" | "cargo clippy" => de(tem("error: could not compile") || tem("error[E") || comeca_com("error:"), comeca_com("Finished ")),
        // O tsc que passa não escreve nada.
        "tsc" => de(tem("error TS"), saida.trim().is_empty()),
        "vitest" => de(linha_com("Test Files", "failed") || linha_com("Tests", " failed"), linha_com("Test Files", "passed")),
        "jest" => de(linha_com("Tests:", "failed") || linha_com("Test Suites:", "failed"), linha_com("Tests:", "passed")),
        "pytest" => de(linha_com(" failed", " in ") || linha_com(" error", " in "), linha_com(" passed", " in ")),
        "go test" | "go build" | "go vet" => de(comeca_com("FAIL") || comeca_com("--- FAIL"), comeca_com("ok ")),
        _ => None,
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn bash(comando: &str, saida: Option<&str>, erro: bool) -> Item {
        Item::Ferramenta {
            id: "b".into(),
            nome: "Bash".into(),
            resumo: format!("Bash · {comando}"),
            resultado: saida.map(str::to_string),
            erro,
            em: 0,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            durou: 0,
            imagens: Vec::new(),
        }
    }

    fn linha(itens: &[Item]) -> String {
        checagens_do_turno(itens).iter().map(Checagem::texto).collect::<Vec<_>>().join(" · ")
    }

    /// O exemplo do pedido: "tsc passou · vitest falhou 2×, passou · cargo test passou".
    #[test]
    fn a_ultima_execucao_vale_e_as_falhas_de_antes_viram_falhou_n_vezes() {
        let turno = [
            bash("npx tsc --noEmit", Some(""), false),
            bash("npx vitest run", Some("Exit code 1\n Test Files  1 failed | 3 passed"), true),
            bash("ls src", Some("a.ts"), false),
            bash("npx vitest run", Some("Exit code 1\n Test Files  1 failed | 3 passed"), true),
            bash("npx vitest run", Some(" Test Files  4 passed"), false),
            bash("cargo test", Some("test result: ok. 12 passed"), false),
        ];
        assert_eq!(linha(&turno), "tsc passou · vitest falhou 2×, passou · cargo test passou");
        // A última falhou: uma vez, várias, e depois de ter passado.
        assert_eq!(linha(&[bash("cargo test", Some("Exit code 101"), true)]), "cargo test falhou");
        let tres = [bash("cargo test", Some("x"), true), bash("cargo test", Some("x"), true), bash("cargo test", Some("x"), true)];
        assert_eq!(linha(&tres), "cargo test falhou 3×");
        assert_eq!(linha(&[bash("cargo test", Some("ok"), false), bash("cargo test", Some("x"), true)]), "cargo test falhou");
        // Um turno sem checagem não diz nada, e a chamada interrompida (sem resultado) não conta.
        assert_eq!(linha(&[bash("ls", Some("a"), false), bash("git status", Some(""), false)]), "");
        assert_eq!(linha(&[bash("cargo test", None, false)]), "");
        // O que foi para o segundo plano ainda não terminou.
        assert_eq!(linha(&[bash("cargo build", Some("Command running in background with ID: b1."), false)]), "");
        // Só as chamadas de Bash: um Read de um arquivo chamado "cargo test" não é checagem.
        let mut leitura = bash("cargo test", Some("x"), false);
        if let Item::Ferramenta { nome, .. } = &mut leitura {
            *nome = "Read".into();
        }
        assert_eq!(linha(&[leitura]), "");
    }

    #[test]
    fn reconhece_a_lista_de_comandos_de_teste_typecheck_lint_e_build() {
        let nome = |comando: &str| checagens_do_comando(comando).into_iter().map(|(n, _)| n).collect::<Vec<_>>().join(", ");
        for (comando, esperado) in [
            ("cargo test", "cargo test"),
            ("cargo test --all -- --nocapture", "cargo test"),
            ("cargo +nightly check", "cargo check"),
            ("cargo clippy -- -D warnings", "cargo clippy"),
            ("cargo build --release", "cargo build"),
            ("cd central && npx tsc --noEmit", "tsc"),
            ("./node_modules/.bin/tsc -p .", "tsc"),
            ("npx vitest run src/modules --maxWorkers=2", "vitest"),
            ("pnpm exec vitest run", "vitest"),
            ("jest --ci", "jest"),
            ("npm test", "npm test"),
            ("npm run test", "npm test"),
            ("pnpm run typecheck", "pnpm run typecheck"),
            ("pnpm lint", "pnpm run lint"),
            ("npm run build", "npm run build"),
            ("yarn build", "yarn run build"),
            ("npm --prefix web run test:unit", "npm run test:unit"),
            ("pytest -q", "pytest"),
            ("python3 -m pytest tests/", "pytest"),
            ("uv run pytest", "pytest"),
            ("go test ./...", "go test"),
            ("CI=1 NODE_ENV=test npm run build", "npm run build"),
            ("timeout 600 cargo test", "cargo test"),
            ("(cd api && cargo check)", "cargo check"),
            ("npm run typecheck && npm test", "npm run typecheck, npm test"),
        ] {
            assert_eq!(nome(comando), esperado, "{comando}");
        }
        // O que não é checagem, e a menção dentro de aspas.
        for comando in ["ls -la", "cargo run", "cargo fmt", "npm run dev", "npm install", "go run .", "git commit -m \"cargo test && tsc\"", "echo 'npm test; tsc'", "grep -rn vitest src"] {
            assert_eq!(nome(comando), "", "{comando}");
        }
    }

    #[test]
    fn com_pipe_vale_o_que_a_saida_diz_e_sem_sinal_fica_de_fora() {
        // O código de saída é o do `tail`: a chamada "deu certo" com o teste falhando.
        let falhou = bash("cargo test 2>&1 | tail -5", Some("test result: FAILED. 10 passed; 2 failed"), false);
        assert_eq!(linha(&[falhou]), "cargo test falhou");
        let passou = bash("cargo test 2>&1 | tail -5", Some("test result: ok. 12 passed; 0 failed"), false);
        assert_eq!(linha(&[passou]), "cargo test passou");
        // Sem sinal na saída não dá para saber: melhor não dizer do que dizer "passou".
        assert_eq!(linha(&[bash("cargo test 2>&1 | grep -c ok", Some("12"), false)]), "");
        assert_eq!(linha(&[bash("npm run build | tail -3", Some("done"), false)]), "");
        // O `;` e o `||` depois também tiram o código de saída da checagem.
        assert_eq!(linha(&[bash("npx tsc --noEmit; echo fim", Some("src/a.ts(1,1): error TS2304: x\nfim"), false)]), "tsc falhou");
        assert_eq!(linha(&[bash("pytest -q || true", Some("3 passed in 0.12s"), false)]), "pytest passou");
        assert_eq!(linha(&[bash("go test ./... | tee log", Some("ok  \tpkg/a\t0.1s\nFAIL\tpkg/b\t0.2s"), false)]), "go test falhou");
        // O redirecionamento não é pipe: o código de saída continua sendo o da checagem.
        assert_eq!(linha(&[bash("cargo test > saida.txt 2>&1", Some(""), true)]), "cargo test falhou");
        assert_eq!(linha(&[bash("cd x && cargo build 2>&1", Some("Finished"), false)]), "cargo build passou");
    }

    #[test]
    fn na_cadeia_que_falhou_a_saida_diz_ate_onde_ela_foi() {
        // As duas passaram.
        assert_eq!(linha(&[bash("npx tsc --noEmit && npx vitest run", Some(" Test Files  4 passed"), false)]), "tsc passou · vitest passou");
        // O vitest deixou sinal: ele rodou, então o tsc passou.
        let segunda = bash("npx tsc --noEmit && npx vitest run", Some("Exit code 1\n Test Files  1 failed | 3 passed"), true);
        assert_eq!(linha(&[segunda]), "tsc passou · vitest falhou");
        // O tsc falhou: o vitest nem rodou, e não aparece.
        let primeira = bash("npx tsc --noEmit && npx vitest run", Some("Exit code 2\nsrc/a.ts(1,1): error TS2304: x"), true);
        assert_eq!(linha(&[primeira]), "tsc falhou");
        // Sem sinal de nenhuma: não dá para saber qual, as duas ficam como falha.
        assert_eq!(linha(&[bash("npm run lint && npm run build", Some("Exit code 1"), true)]), "npm run lint falhou · npm run build falhou");
    }

    #[test]
    fn o_trecho_da_falha_para_a_tela() {
        let trecho = |texto: &str| trecho_da_falha(texto).map(|t| texto[t].to_string());
        assert_eq!(trecho("vitest falhou 2×, passou").as_deref(), Some("falhou 2×"));
        assert_eq!(trecho("cargo test falhou").as_deref(), Some("falhou"));
        assert_eq!(trecho("tsc passou"), None);
    }
}
