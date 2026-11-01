//! Revisor independente: uma sessão nova do Claude Code, só de leitura, que confere a entrega de
//! uma tarefa contra o objetivo e os critérios antes de o coordenador decidir. Como não viu o
//! trabalho sendo feito nem o plano do coordenador, tende a achar o que os dois deixaram passar.

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context as _, Result, bail};
use serde_json::{Value, json};

use crate::{
    claude::binario_claude,
    modelo::{Revisao, Tarefa},
    processos::{self, Escopo},
};

pub fn revisar(
    pasta: &Path,
    base: &str,
    tarefa: &Tarefa,
    modelo: Option<&str>,
    pasta_conta: Option<PathBuf>,
) -> Result<Revisao> {
    let esquema = json!({
        "type": "object",
        "properties": {
            "veredito": { "type": "string", "enum": ["aprovar", "ajustar"] },
            "resumo": { "type": "string" },
            "problemas": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "arquivo": { "type": "string" },
                        "linha": { "type": "integer" },
                        "gravidade": { "type": "string", "enum": ["alta", "media", "baixa"] },
                        "descricao": { "type": "string" }
                    },
                    "required": ["arquivo", "gravidade", "descricao"]
                }
            }
        },
        "required": ["veredito", "resumo", "problemas"]
    });
    let criterios: String = tarefa.criterios.iter().map(|c| format!("- {c}\n")).collect();
    let pedido = format!(
        "Você é um revisor de código independente. Revise as mudanças desta pasta em relação ao \
commit {base} (veja com `git diff {base}` e `git status`; arquivos novos aparecem como não \
rastreados). Não altere nada.\n\nTarefa #{id}: {titulo}\nObjetivo:\n{objetivo}\n\nCritérios de \
aceitação:\n{criterios}\nConfira cada critério, procure bugs, casos esquecidos, código que não \
compila, testes fracos e mudanças fora do escopo. Seja concreto: arquivo, linha e o que está \
errado. Aponte só problemas reais; estilo e preferência pessoal não contam. Veredito \"aprovar\" \
se não houver problema de gravidade alta ou média. Escreva em português.",
        id = tarefa.id,
        titulo = tarefa.titulo,
        objetivo = tarefa.objetivo,
    );
    // Num escopo próprio: no limite de tempo ou com o Vitral fechando, os comandos que ele roda
    // (cargo test, servidores) param junto.
    let (mut comando, unidade) = processos::comando(binario_claude(), "revisor");
    comando
        .current_dir(pasta)
        .args(["-p", "--output-format", "json", "--strict-mcp-config"])
        .args(["--tools", "Read,Grep,Glob,Bash"])
        .args(["--permission-mode", "bypassPermissions"])
        .args(["--disallowedTools", "Edit", "Write", "NotebookEdit"])
        .args(["--json-schema", &esquema.to_string()])
        .arg("--no-session-persistence")
        .arg(&pedido)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Some(modelo) = modelo.filter(|m| !m.is_empty()) {
        comando.args(["--model", modelo]);
    }
    if let Some(pasta) = pasta_conta {
        comando.env("CLAUDE_CONFIG_DIR", pasta);
    }
    for variavel in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"] {
        comando.env_remove(variavel);
    }
    let mut filho = comando.spawn().context("não abriu o claude")?;
    let escopo = Escopo::novo(unidade, filho.id());
    let mut saida = filho.stdout.take().unwrap();
    let leitura = std::thread::spawn(move || {
        let mut texto = String::new();
        let _ = saida.read_to_string(&mut texto);
        texto
    });
    if processos::esperar(&mut filho, &escopo, Duration::from_secs(10 * 60)).is_none() {
        bail!("o revisor passou de 10 minutos");
    }
    let texto = leitura.join().unwrap_or_default();
    let valor: Value = serde_json::from_str(&texto).context("resposta do revisor ilegível")?;
    if valor["is_error"] == true {
        bail!(
            "{}",
            valor["result"].as_str().unwrap_or("o revisor falhou")
        );
    }
    let estruturado = valor
        .get("structured_output")
        .cloned()
        .or_else(|| serde_json::from_str(valor["result"].as_str()?).ok())
        .context("o revisor não devolveu o parecer")?;
    Ok(serde_json::from_value(estruturado)?)
}

/// Texto do parecer para o coordenador.
pub fn texto(revisao: &Revisao) -> String {
    let mut texto = format!(
        "Revisor independente: {} — {}\n",
        if revisao.veredito == "aprovar" {
            "APROVARIA"
        } else {
            "PEDE AJUSTES"
        },
        revisao.resumo.trim()
    );
    texto.push_str(&linhas_problemas(revisao, &[]));
    texto
}

/// Os problemas numerados (o coordenador cita o número para descartar um falso positivo), menos
/// os de `ignorar`.
pub fn linhas_problemas(revisao: &Revisao, ignorar: &[usize]) -> String {
    let mut texto = String::new();
    for (n, p) in revisao.problemas.iter().enumerate().map(|(i, p)| (i + 1, p)) {
        if !ignorar.contains(&n) {
            texto.push_str(&format!("{n}. [{}] {}: {}\n", p.gravidade, onde(p), p.descricao));
        }
    }
    texto
}

/// "arquivo:linha" do problema, ou só o arquivo.
pub fn onde(p: &crate::modelo::Problema) -> String {
    match p.linha {
        Some(linha) => format!("{}:{linha}", p.arquivo),
        None => p.arquivo.clone(),
    }
}

/// Hook `Stop` do Claude Code para os trabalhadores: roda a verificação da tarefa quando ele
/// para; se falhar, o Claude Code devolve a saída e o trabalhador continua (uma vez só, graças ao
/// `stop_hook_active`). Devolve o JSON para `--settings`.
pub fn configuracao_hook_verificacao() -> Option<Value> {
    let pasta = dirs::cache_dir()?.join("vitral").join("hooks");
    std::fs::create_dir_all(&pasta).ok()?;
    let script = pasta.join("verificar-ao-parar.sh");
    let conteudo = r#"#!/bin/bash
# Hook Stop do Vitral: roda a verificação da tarefa; se falhar, o trabalhador continua.
entrada=$(cat)
case "$entrada" in *'"stop_hook_active":true'*|*'"stop_hook_active": true'*) exit 0 ;; esac
[ -z "$VITRAL_VERIFICACAO" ] && exit 0
saida=$(bash -lc "$VITRAL_VERIFICACAO" 2>&1) && exit 0
{
  echo "A verificação da tarefa falhou (\`$VITRAL_VERIFICACAO\`). Corrija antes de terminar:"
  echo "$saida" | tail -40
} >&2
exit 2
"#;
    if std::fs::read_to_string(&script).ok().as_deref() != Some(conteudo) {
        std::fs::write(&script, conteudo).ok()?;
    }
    Some(
        json!({
            "hooks": { "Stop": [ { "hooks": [ {
                "type": "command",
                "command": format!("bash '{}'", script.display()),
                "timeout": 900
            } ] } ] }
        }),
    )
}

#[cfg(test)]
mod testes {
    use std::{io::Write, process::Command};

    #[test]
    fn hook_de_verificacao_faz_o_trabalhador_continuar_so_uma_vez() {
        let valor = super::configuracao_hook_verificacao().unwrap();
        let comando = valor["hooks"]["Stop"][0]["hooks"][0]["command"].as_str().unwrap().to_string();
        let rodar = |verificacao: &str, entrada: &str| {
            let mut filho = Command::new("bash")
                .args(["-c", &comando])
                .env("VITRAL_VERIFICACAO", verificacao)
                .stdin(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped())
                .spawn()
                .unwrap();
            filho.stdin.take().unwrap().write_all(entrada.as_bytes()).unwrap();
            let saida = filho.wait_with_output().unwrap();
            (saida.status.code(), String::from_utf8_lossy(&saida.stderr).to_string())
        };
        let (codigo, erro) = rodar("echo falhou; exit 1", r#"{"stop_hook_active":false}"#);
        assert_eq!(codigo, Some(2));
        assert!(erro.contains("falhou"));
        assert_eq!(rodar("exit 1", r#"{"stop_hook_active": true}"#).0, Some(0));
        assert_eq!(rodar("true", r#"{"stop_hook_active":false}"#).0, Some(0));
    }
}
