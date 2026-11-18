//! Desfazer só um bloco de mudança de um arquivo: do diff do arquivo contra o último commit
//! (`git diff HEAD`), monta o remendo (patch) de um bloco só e o aplica ao contrário, com `git
//! apply -R` e o remendo pela entrada padrão. O git aplica tudo ou nada: se o remendo não casar
//! com o arquivo de agora, nada muda.
//!
//! Um bloco é uma sequência de linhas seguidas que saem e entram dentro de um trecho ("@@") do
//! diff: a mesma conta que o visor faz para as setas de uma mudança à outra (`ui/edicao.rs`), e o
//! número do bloco é o da mudança lá.

use std::{io::Write as _, path::Path, process::Stdio};

use anyhow::{Context as _, Result, bail};

/// Um trecho ("@@ -a,b +c,d @@") do diff: onde começa de cada lado e as linhas dele como vieram
/// (o sinal na frente; a marca "\ No newline at end of file" fica junto da linha a que pertence).
struct Trecho<'a> {
    novo_inicio: usize,
    novo_total: usize,
    linhas: Vec<Linha<'a>>,
}

struct Linha<'a> {
    /// ' ', '-' ou '+'.
    sinal: u8,
    /// A linha inteira, com o sinal (a de contexto vazia, que algum programa deixou sem o espaço,
    /// também).
    texto: &'a str,
    /// A marca de "sem quebra de linha no fim" que vem depois dela.
    sem_quebra: Option<&'a str>,
}

/// "-3,4" ou "+3": o começo e o total (sem número, 1).
fn medida(campo: &str, sinal: char) -> Option<(usize, usize)> {
    let campo = campo.strip_prefix(sinal)?;
    match campo.split_once(',') {
        Some((a, b)) => Some((a.parse().ok()?, b.parse().ok()?)),
        None => Some((campo.parse().ok()?, 1)),
    }
}

/// O cabeçalho do diff (até o primeiro "@@") e os trechos. `None` no que não é a mudança de um
/// arquivo que existe dos dois lados (o arquivo novo e o apagado têm "/dev/null"; o binário não
/// tem trecho).
fn ler(diff: &str) -> Option<(&str, Vec<Trecho<'_>>)> {
    let primeiro = if diff.starts_with("@@") { 0 } else { diff.find("\n@@")? + 1 };
    let cabecalho = &diff[..primeiro];
    if cabecalho.lines().any(|l| l == "--- /dev/null" || l == "+++ /dev/null") {
        return None;
    }
    let mut trechos: Vec<Trecho> = Vec::new();
    // O que falta ler do trecho aberto, de cada lado.
    let (mut velho, mut novo) = (0usize, 0usize);
    // Só pelo '\n': o '\r' de um arquivo com fim de linha do Windows é parte da linha.
    for linha in diff[primeiro..].split('\n') {
        if linha.starts_with("@@") && velho == 0 && novo == 0 {
            let mut campos = linha.trim_start_matches('@').split_whitespace();
            let (_, velho_total) = medida(campos.next()?, '-')?;
            let (novo_inicio, novo_total) = medida(campos.next()?, '+')?;
            (velho, novo) = (velho_total, novo_total);
            trechos.push(Trecho { novo_inicio, novo_total, linhas: Vec::new() });
            continue;
        }
        let trecho = trechos.last_mut()?;
        let sinal = match linha.as_bytes().first() {
            Some(b'\\') => {
                trecho.linhas.last_mut()?.sem_quebra = Some(linha);
                continue;
            }
            // Fora de um trecho (depois do último), o que vier não é dele.
            _ if velho == 0 && novo == 0 => continue,
            Some(b'-') => b'-',
            Some(b'+') => b'+',
            Some(b' ') | None => b' ',
            Some(_) => return None,
        };
        match sinal {
            b'-' => velho = velho.checked_sub(1)?,
            b'+' => novo = novo.checked_sub(1)?,
            _ => (velho, novo) = (velho.checked_sub(1)?, novo.checked_sub(1)?),
        }
        trecho.linhas.push(Linha { sinal, texto: linha, sem_quebra: None });
    }
    // Um trecho que acabou antes da conta do cabeçalho: o diff veio cortado.
    (velho == 0 && novo == 0 && !trechos.is_empty()).then_some((cabecalho, trechos))
}

/// Os blocos de um trecho: as faixas (de, até) das linhas seguidas que saem ou entram.
fn blocos_do_trecho(trecho: &Trecho) -> Vec<(usize, usize)> {
    let mut blocos = Vec::new();
    let mut aberto: Option<usize> = None;
    for (i, linha) in trecho.linhas.iter().enumerate() {
        match (linha.sinal != b' ', aberto) {
            (true, None) => aberto = Some(i),
            (false, Some(de)) => {
                blocos.push((de, i));
                aberto = None;
            }
            _ => {}
        }
    }
    if let Some(de) = aberto {
        blocos.push((de, trecho.linhas.len()));
    }
    blocos
}

/// Quantos blocos de mudança o diff tem (zero no que não dá para desfazer por bloco: arquivo
/// novo, apagado ou binário).
pub fn blocos(diff: &str) -> usize {
    ler(diff).map_or(0, |(_, trechos)| trechos.iter().map(|t| blocos_do_trecho(t).len()).sum())
}

/// O remendo que, aplicado ao contrário, desfaz só o bloco `bloco` (0 é o primeiro do arquivo): o
/// trecho dele, com as outras mudanças do mesmo trecho como estão no arquivo de agora (o que
/// entrou vira contexto; o que saiu não aparece). O lado novo do remendo é o arquivo de agora, e
/// o velho, o arquivo com só este bloco de volta: os dois começam na mesma linha.
pub fn remendo_do_bloco(diff: &str, bloco: usize) -> Option<String> {
    let (cabecalho, trechos) = ler(diff)?;
    let mut falta = bloco;
    let (trecho, (de, ate)) = trechos.iter().find_map(|t| {
        let blocos = blocos_do_trecho(t);
        match blocos.get(falta) {
            Some(b) => Some((t, *b)),
            None => {
                falta -= blocos.len();
                None
            }
        }
    })?;
    let mut corpo = String::new();
    let mut velho_total = 0;
    for (i, linha) in trecho.linhas.iter().enumerate() {
        let no_bloco = (de..ate).contains(&i);
        let sinal = match (linha.sinal, no_bloco) {
            (b'-', false) => continue,
            (b'+', false) => b' ',
            (sinal, _) => sinal,
        };
        if sinal != b'+' {
            velho_total += 1;
        }
        corpo.push(sinal as char);
        corpo.push_str(linha.texto.get(1..).unwrap_or_default());
        corpo.push('\n');
        if let Some(marca) = linha.sem_quebra {
            corpo.push_str(marca);
            corpo.push('\n');
        }
    }
    // A linha em que o trecho começa no arquivo de agora; com um lado vazio, o cabeçalho leva a
    // linha de antes (é como o diff unificado escreve "nenhuma linha").
    let comeco = if trecho.novo_total == 0 { trecho.novo_inicio + 1 } else { trecho.novo_inicio };
    let velho_inicio = if velho_total == 0 { comeco.saturating_sub(1) } else { comeco };
    Some(format!(
        "{cabecalho}@@ -{velho_inicio},{velho_total} +{},{} @@\n{corpo}",
        trecho.novo_inicio, trecho.novo_total
    ))
}

/// O diff do arquivo contra o último commit, com as opções do `diff_do_arquivo`, mas como o git
/// o escreve, sem cortar nada no fim: o remendo precisa das linhas inteiras.
fn diff_cru(raiz: &Path, caminho: &str) -> Result<String> {
    let saida = super::git_em(raiz)
        .args(["-c", "core.quotePath=false"])
        .args(super::DIFF_DO_ARQUIVO)
        .args(["--", caminho])
        .env("GIT_OPTIONAL_LOCKS", "0")
        .output()?;
    if !saida.status.success() {
        bail!("git diff: {}", String::from_utf8_lossy(&saida.stderr).trim());
    }
    String::from_utf8(saida.stdout).context("o arquivo não é texto em UTF-8")
}

/// Desfaz só o bloco `bloco` do arquivo: ele volta a como está no último commit, e o resto do
/// arquivo fica. `visto` é o diff que a tela mostrava quando o bloco foi escolhido (o de
/// `diff_do_arquivo`): se o arquivo mudou desde então, o número do bloco pode ser de outra
/// mudança, e nada é feito. Também não mexe em nada se o remendo não aplicar.
pub fn desfazer_bloco(raiz: &Path, caminho: &str, visto: &str, bloco: usize) -> Result<()> {
    let agora = diff_cru(raiz, caminho)?;
    if agora.trim_end() != visto.trim_end() {
        bail!("o arquivo mudou desde que o diff foi aberto");
    }
    let remendo = remendo_do_bloco(&agora, bloco).context("este trecho não dá para desfazer sozinho")?;
    let mut filho = super::git_em(raiz)
        .args(["apply", "-R", "--whitespace=nowarn", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;
    // A entrada fecha ao sair do bloco: o git só começa a aplicar com o remendo inteiro lido.
    {
        let mut entrada = filho.stdin.take().context("sem a entrada do git")?;
        entrada.write_all(remendo.as_bytes())?;
    }
    let saida = filho.wait_with_output()?;
    if !saida.status.success() {
        bail!("o git não aplicou o remendo: {}", String::from_utf8_lossy(&saida.stderr).trim());
    }
    Ok(())
}

#[cfg(test)]
mod testes {
    use super::*;

    /// Um arquivo de 12 linhas mudado no começo, no meio e no fim: três trechos, um bloco em cada.
    const TRES_TRECHOS: &str = "diff --git a/src/a.txt b/src/a.txt\nindex 1111111..2222222 100644\n--- a/src/a.txt\n+++ b/src/a.txt\n\
@@ -1,4 +1,4 @@\n-um\n+UM\n dois\n tres\n quatro\n\
@@ -14,7 +14,8 @@\n treze\n catorze\n quinze\n-dezesseis\n+DEZESSEIS\n+e meio\n dezessete\n dezoito\n dezenove\n\
@@ -28,3 +29,2 @@\n vinte e oito\n vinte e nove\n-trinta\n";

    const CABECALHO: &str = "diff --git a/src/a.txt b/src/a.txt\nindex 1111111..2222222 100644\n--- a/src/a.txt\n+++ b/src/a.txt\n";

    #[test]
    fn remendo_do_trecho_do_comeco_do_meio_e_do_fim() {
        assert_eq!(blocos(TRES_TRECHOS), 3);
        assert_eq!(remendo_do_bloco(TRES_TRECHOS, 0).unwrap(), format!("{CABECALHO}@@ -1,4 +1,4 @@\n-um\n+UM\n dois\n tres\n quatro\n"));
        assert_eq!(
            remendo_do_bloco(TRES_TRECHOS, 1).unwrap(),
            format!("{CABECALHO}@@ -14,7 +14,8 @@\n treze\n catorze\n quinze\n-dezesseis\n+DEZESSEIS\n+e meio\n dezessete\n dezoito\n dezenove\n")
        );
        // O do fim: o lado velho começa na mesma linha do novo (o arquivo de agora, com só este
        // bloco de volta), e não na 28 que o diff inteiro dizia.
        assert_eq!(remendo_do_bloco(TRES_TRECHOS, 2).unwrap(), format!("{CABECALHO}@@ -29,3 +29,2 @@\n vinte e oito\n vinte e nove\n-trinta\n"));
        assert_eq!(remendo_do_bloco(TRES_TRECHOS, 3), None);
    }

    #[test]
    fn dois_blocos_no_mesmo_trecho_saem_separados() {
        // Duas mudanças a duas linhas uma da outra: o git as junta num trecho só.
        let diff = "--- a/x\n+++ b/x\n@@ -1,7 +1,7 @@\n a\n-b\n+B\n c\n d\n-e\n+E\n+E2\n-f\n g\n";
        assert_eq!(blocos(diff), 2);
        // Desfazer o primeiro: o segundo fica como está no arquivo (o que entrou vira contexto, o
        // que saiu não aparece).
        assert_eq!(remendo_do_bloco(diff, 0).unwrap(), "--- a/x\n+++ b/x\n@@ -1,7 +1,7 @@\n a\n-b\n+B\n c\n d\n E\n E2\n g\n");
        assert_eq!(remendo_do_bloco(diff, 1).unwrap(), "--- a/x\n+++ b/x\n@@ -1,7 +1,7 @@\n a\n B\n c\n d\n-e\n+E\n+E2\n-f\n g\n");
    }

    #[test]
    fn lado_vazio_fim_sem_quebra_e_o_que_nao_da() {
        // O arquivo que ficou vazio e o que era vazio: o lado sem linha leva a linha de antes.
        assert_eq!(remendo_do_bloco("--- a/x\n+++ b/x\n@@ -1,2 +0,0 @@\n-a\n-b\n", 0).unwrap(), "--- a/x\n+++ b/x\n@@ -1,2 +0,0 @@\n-a\n-b\n");
        assert_eq!(remendo_do_bloco("--- a/x\n+++ b/x\n@@ -0,0 +1,2 @@\n+a\n+b\n", 0).unwrap(), "--- a/x\n+++ b/x\n@@ -0,0 +1,2 @@\n+a\n+b\n");
        // A marca de "sem quebra de linha no fim" acompanha a linha dela; a do bloco que fica
        // como está vira marca do contexto.
        let diff = "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n-a\n+A\n b\n-c\n\\ No newline at end of file\n+C\n\\ No newline at end of file\n";
        assert_eq!(remendo_do_bloco(diff, 0).unwrap(), "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n-a\n+A\n b\n C\n\\ No newline at end of file\n");
        assert_eq!(
            remendo_do_bloco(diff, 1).unwrap(),
            "--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n A\n b\n-c\n\\ No newline at end of file\n+C\n\\ No newline at end of file\n"
        );
        // O fim de linha do Windows fica na linha.
        assert_eq!(remendo_do_bloco("--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\r\n+b\r\n", 0).unwrap(), "--- a/x\n+++ b/x\n@@ -1,1 +1,1 @@\n-a\r\n+b\r\n");
        // Arquivo novo, apagado, binário e diff cortado: nada para desfazer por bloco.
        assert_eq!(blocos("--- /dev/null\n+++ b/x\n@@ -0,0 +1 @@\n+a\n"), 0);
        assert_eq!(blocos("--- a/x\n+++ /dev/null\n@@ -1 +0,0 @@\n-a\n"), 0);
        assert_eq!(blocos("diff --git a/x.png b/x.png\nBinary files a/x.png and b/x.png differ\n"), 0);
        assert_eq!(blocos("--- a/x\n+++ b/x\n@@ -1,3 +1,3 @@\n a\n-b\n"), 0);
        assert_eq!(blocos(""), 0);
    }

    fn git(pasta: &Path, args: &[&str]) {
        let saida = crate::git::git_em(pasta).args(["-c", "user.name=Vitral", "-c", "user.email=vitral@localhost"]).args(args).output().unwrap();
        assert!(saida.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&saida.stderr));
    }

    /// Num repositório de verdade: três trechos mudados, o do meio desfeito, os outros dois ficam
    /// e a contagem de linhas do retrato acompanha.
    #[test]
    fn desfazer_o_trecho_do_meio_deixa_os_outros_dois() {
        let pasta = std::env::temp_dir().join(format!("vitral-git-remendo-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(pasta.join("src")).unwrap();
        git(&pasta, &["init", "-q"]);
        git(&pasta, &["config", "core.autocrlf", "false"]);
        let linhas = |troca: &dyn Fn(usize) -> Option<String>| -> String {
            (1..=30).map(|n| troca(n).unwrap_or_else(|| format!("linha {n}\n"))).collect()
        };
        let arquivo = pasta.join("src").join("ação.txt");
        std::fs::write(&arquivo, linhas(&|_| None)).unwrap();
        git(&pasta, &["add", "."]);
        git(&pasta, &["commit", "-q", "-m", "primeiro"]);
        // O começo trocado, o meio com uma linha a mais e duas mudanças vizinhas, o fim sem a última.
        let mudado = |com_o_meio: bool| {
            linhas(&|n| match n {
                1 => Some("LINHA 1\n".into()),
                15 if com_o_meio => Some("LINHA 15\ne meia\n".into()),
                18 if com_o_meio => Some(String::new()),
                30 => Some(String::new()),
                _ => None,
            })
        };
        std::fs::write(&arquivo, mudado(true)).unwrap();
        let caminho = "src/ação.txt";
        let visto = crate::git::diff_do_arquivo(&pasta, caminho);
        // O 15 e o 18 estão perto: o git os põe no mesmo trecho, e são dois blocos.
        assert_eq!(blocos(&visto), 4);
        let antes = crate::git::retrato_da_pasta(&pasta).unwrap().mudancas[0].clone();
        assert_eq!((antes.entram, antes.saem), (3, 4));

        // Com o diff de outra hora, nada muda.
        assert!(desfazer_bloco(&pasta, caminho, "outro diff", 1).is_err());
        assert_eq!(std::fs::read_to_string(&arquivo).unwrap(), mudado(true));

        // Os dois blocos do meio, um de cada vez (o diff é relido entre um e outro).
        desfazer_bloco(&pasta, caminho, &visto, 1).unwrap();
        let visto = crate::git::diff_do_arquivo(&pasta, caminho);
        assert_eq!(blocos(&visto), 3);
        desfazer_bloco(&pasta, caminho, &visto, 1).unwrap();
        assert_eq!(std::fs::read_to_string(&arquivo).unwrap(), mudado(false));
        let depois = crate::git::retrato_da_pasta(&pasta).unwrap().mudancas[0].clone();
        assert_eq!((depois.entram, depois.saem), (1, 2));

        // O do fim e o do começo: o arquivo volta ao último commit.
        let visto = crate::git::diff_do_arquivo(&pasta, caminho);
        desfazer_bloco(&pasta, caminho, &visto, 1).unwrap();
        let visto = crate::git::diff_do_arquivo(&pasta, caminho);
        desfazer_bloco(&pasta, caminho, &visto, 0).unwrap();
        assert_eq!(std::fs::read_to_string(&arquivo).unwrap(), linhas(&|_| None));
        assert!(crate::git::retrato_da_pasta(&pasta).unwrap().mudancas.is_empty());
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
