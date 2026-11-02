//! GitHub pelo `gh` (2.97) como subprocesso: argumentos em vetor, `--json`, e o repositório sempre
//! explícito (`-R dono/nome`, ou o caminho `repos/O/R` no `gh api`). O login é todo do gh: o
//! Vitral só confere quem está conectado e com que escopos, e o git usa o helper que já existe.

use std::{path::Path, time::Duration};

use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

use crate::{
    git,
    issues::{self, ComentarioIssue, Confianca, EstadoIssue, Etiqueta, FonteIssue, IssueDetalhe, IssueResumo, Pessoa, TipoEstado},
    remoto::{Ambiente, ErroRemoto, Executor, SaidaBruta, classificar_gh},
};

const LEITURA: Duration = Duration::from_secs(30);
const ESCRITA: Duration = Duration::from_secs(60);

/// Variáveis que mudariam a conta, o repositório ou o que o gh escreve no stderr.
const REMOVER: [&str; 9] = [
    "GH_TOKEN",
    "GITHUB_TOKEN",
    "GH_ENTERPRISE_TOKEN",
    "GITHUB_ENTERPRISE_TOKEN",
    "GH_HOST",
    "GH_REPO",
    "GH_CONFIG_DIR",
    "GH_DEBUG",
    "DEBUG",
];

/// Sem perguntas, sem pager, sem cor e sem aviso de versão nova. As variáveis de proxy ficam.
fn ambiente() -> Ambiente {
    let definir = [
        ("GH_PROMPT_DISABLED", "1"),
        ("GH_PAGER", "cat"),
        ("GH_NO_UPDATE_NOTIFIER", "1"),
        ("GH_SPINNER_DISABLED", "1"),
        ("NO_COLOR", "1"),
        ("CLICOLOR", "0"),
        ("GIT_TERMINAL_PROMPT", "0"),
    ];
    Ambiente {
        limpar: false,
        manter: Vec::new(),
        definir: definir.iter().map(|(n, v)| (n.to_string(), v.to_string())).collect(),
        remover: REMOVER.to_vec(),
    }
}

/// O gh de uma sessão do Vitral: o executor e a pasta de dados, que é a pasta atual de todo gh
/// (assim ele nunca lê o `.git` de um projeto).
pub struct Gh<'a> {
    pub e: &'a dyn Executor,
    pub pasta: &'a Path,
}

pub struct Saida {
    pub stdout: String,
    /// Os cabeçalhos HTTP (o `gh api -i`); nenhuma chamada do v1 pede.
    #[allow(dead_code)]
    pub cabecalhos: Vec<(String, String)>,
}

fn conferir_repo(repo: &str) -> Result<(), ErroRemoto> {
    if issues::repositorio_valido(repo) {
        Ok(())
    } else {
        Err(ErroRemoto::Validacao(format!("repositório inválido: {repo}")))
    }
}

/// Um sha (curto ou inteiro): só hexadecimal.
fn conferir_sha(sha: &str) -> Result<(), ErroRemoto> {
    if (7..=40).contains(&sha.len()) && sha.chars().all(|c| c.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(ErroRemoto::Validacao(format!("sha inválido: {sha}")))
    }
}

fn ler_json<T: for<'de> Deserialize<'de>>(texto: &str) -> Result<T, ErroRemoto> {
    serde_json::from_str(texto).map_err(|erro| ErroRemoto::Outro(format!("resposta do gh fora do esperado: {erro}")))
}

/// `null` vira o valor padrão (o gh devolve `null` em vários campos de texto).
fn nulo<'de, D: Deserializer<'de>, T: Default + Deserialize<'de>>(d: D) -> Result<T, D::Error> {
    Ok(Option::<T>::deserialize(d)?.unwrap_or_default())
}

impl Gh<'_> {
    /// Roda o gh e devolve a saída como veio, com qualquer código. Com `repo`, o `-R repo` entra
    /// logo depois do comando (`gh pr list -R o/r …`).
    fn bruto(&self, repo: Option<&str>, args: &[&str], entrada: Option<&str>, limite: Duration) -> Result<SaidaBruta, ErroRemoto> {
        let mut todos: Vec<String> = args.iter().map(|a| a.to_string()).collect();
        if let Some(repo) = repo {
            conferir_repo(repo)?;
            let posicao = todos.len().min(2);
            todos.splice(posicao..posicao, ["-R".to_string(), repo.to_string()]);
        }
        self.e.rodar("gh", &todos, &ambiente(), self.pasta, entrada.map(str::as_bytes), limite)
    }

    /// O gh que tem de sair com 0; senão, o erro classificado.
    fn gh(&self, repo: Option<&str>, args: &[&str], entrada: Option<&str>, limite: Duration) -> Result<Saida, ErroRemoto> {
        let saida = self.bruto(repo, args, entrada, limite)?;
        if saida.codigo != Some(0) {
            return Err(classificar_gh(saida.codigo, &saida.texto(), &saida.erros()));
        }
        Ok(Saida { stdout: saida.texto(), cabecalhos: Vec::new() })
    }

    // ---------- conta e repositório ----------

    /// Quem está conectado no github.com (`None` sem conta ativa). O `auth status --json` sai
    /// sempre com 0: quem decide é o `state`.
    pub fn login(&self) -> Result<Option<LoginGh>, ErroRemoto> {
        const JQ: &str = r#"(.hosts["github.com"] // [])[] | select(.active) | {login,state,error,scopes}"#;
        let saida = self.gh(None, &["auth", "status", "--json", "hosts", "--jq", JQ], None, LEITURA)?;
        let Some(linha) = saida.stdout.lines().find(|l| !l.trim().is_empty()) else {
            return Ok(None);
        };
        #[derive(Deserialize, Default)]
        #[serde(default)]
        struct Conta {
            #[serde(deserialize_with = "nulo")]
            login: String,
            #[serde(deserialize_with = "nulo")]
            state: String,
            #[serde(deserialize_with = "nulo")]
            error: String,
            #[serde(deserialize_with = "nulo")]
            scopes: String,
        }
        let conta: Conta = ler_json(linha)?;
        let estado = match conta.state.as_str() {
            "success" => EstadoLogin::Ok,
            "timeout" => EstadoLogin::TempoEsgotado,
            _ => EstadoLogin::Erro(crate::remoto::sem_segredos(&conta.error)),
        };
        let escopos = conta.scopes.split(',').map(|e| e.trim().to_string()).filter(|e| !e.is_empty()).collect();
        Ok(Some(LoginGh { login: conta.login, estado, escopos }))
    }

    pub fn info_repo(&self, repo: &str) -> Result<InfoRepo, ErroRemoto> {
        conferir_repo(repo)?;
        let campos = "nameWithOwner,owner,name,url,defaultBranchRef,isFork,parent,isPrivate,viewerPermission,\
                      viewerDefaultMergeMethod,squashMergeAllowed,mergeCommitAllowed,rebaseMergeAllowed,\
                      deleteBranchOnMerge,hasIssuesEnabled";
        let saida = self.gh(None, &["repo", "view", repo, "--json", campos], None, LEITURA)?;
        Ok(ler_json::<RepoGh>(&saida.stdout)?.into())
    }

    /// Quantos workflows do Actions o repositório tem (0 = "sem CI").
    pub fn workflows(&self, repo: &str) -> Result<u32, ErroRemoto> {
        conferir_repo(repo)?;
        let caminho = format!("repos/{repo}/actions/workflows");
        let saida = self.gh(None, &["api", "--cache", "1h", &caminho, "--jq", ".total_count"], None, LEITURA)?;
        saida.stdout.trim().parse().map_err(|_| ErroRemoto::Outro(format!("total de workflows ilegível: {}", saida.stdout.trim())))
    }

    pub fn ramos(&self, repo: &str) -> Result<Vec<String>, ErroRemoto> {
        conferir_repo(repo)?;
        let caminho = format!("repos/{repo}/branches?per_page=100");
        let saida = self.gh(None, &["api", "--cache", "1h", &caminho, "--jq", ".[].name"], None, LEITURA)?;
        Ok(saida.stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect())
    }

    /// Os ramos dos últimos PRs da conta no repositório (para sugerir o prefixo).
    pub fn meus_ramos_de_pr(&self, repo: &str) -> Result<Vec<String>, ErroRemoto> {
        let args = ["pr", "list", "--author", "@me", "--state", "all", "--limit", "20", "--json", "headRefName"];
        let saida = self.gh(Some(repo), &args, None, LEITURA)?;
        #[derive(Deserialize)]
        struct Ramo {
            #[serde(rename = "headRefName", default)]
            ramo: String,
        }
        Ok(ler_json::<Vec<Ramo>>(&saida.stdout)?.into_iter().map(|r| r.ramo).collect())
    }

    // ---------- PR: criar e mudar ----------

    /// O PR aberto deste ramo, para recuperar um create que caiu no meio. Estrito: só OPEN, com o
    /// head no repositório de `dono_do_head` e no sha que o Vitral acabou de enviar.
    pub fn pr_do_ramo(&self, repo: &str, ramo: &str, dono_do_head: &str, sha: &str) -> Result<Option<PrAchado>, ErroRemoto> {
        let head = format!("--head={ramo}");
        let campos = "number,state,url,title,headRefOid,isDraft,headRepositoryOwner,isCrossRepository";
        let args = ["pr", "list", &head, "--state", "open", "--limit", "5", "--json", campos];
        let saida = self.gh(Some(repo), &args, None, LEITURA)?;
        #[derive(Deserialize, Default)]
        #[serde(rename_all = "camelCase", default)]
        struct Linha {
            number: u64,
            #[serde(deserialize_with = "nulo")]
            state: String,
            #[serde(deserialize_with = "nulo")]
            url: String,
            #[serde(deserialize_with = "nulo")]
            title: String,
            #[serde(deserialize_with = "nulo")]
            head_ref_oid: String,
            is_draft: bool,
            head_repository_owner: Option<Login>,
        }
        let linhas: Vec<Linha> = ler_json(&saida.stdout)?;
        Ok(linhas
            .into_iter()
            .find(|l| {
                l.state == "OPEN"
                    && l.head_repository_owner.as_ref().is_some_and(|d| d.login.eq_ignore_ascii_case(dono_do_head))
                    && !sha.is_empty()
                    && l.head_ref_oid == sha
            })
            .map(|l| PrAchado { numero: l.number, url: l.url, titulo: l.title, rascunho: l.is_draft, sha: l.head_ref_oid }))
    }

    /// Abre o PR (o push já foi feito). `cabeca` é o ramo, ou "dono:ramo" a partir de um fork.
    /// Devolve o número (da URL que o gh imprime) e a URL.
    pub fn criar_pr(
        &self,
        repo: &str,
        cabeca: &str,
        base: &str,
        titulo: &str,
        corpo: &str,
        rascunho: bool,
    ) -> Result<(u64, String), ErroRemoto> {
        let (head, base, titulo) = (format!("--head={cabeca}"), format!("--base={base}"), format!("--title={titulo}"));
        let mut args = vec!["pr", "create", &head, &base, &titulo, "--body-file", "-"];
        if rascunho {
            args.push("--draft");
        }
        let saida = self.gh(Some(repo), &args, Some(corpo), ESCRITA)?;
        let url = saida.stdout.lines().map(str::trim).rfind(|l| !l.is_empty()).unwrap_or_default().to_string();
        let numero = numero_da_url(&url).ok_or_else(|| ErroRemoto::Outro(format!("o gh não devolveu a URL do PR: {url}")))?;
        Ok((numero, url))
    }

    pub fn editar_pr(&self, repo: &str, numero: u64, titulo: Option<&str>, corpo: Option<&str>) -> Result<(), ErroRemoto> {
        if titulo.is_none() && corpo.is_none() {
            return Ok(());
        }
        let (n, titulo) = (numero.to_string(), titulo.map(|t| format!("--title={t}")));
        let mut args = vec!["pr", "edit", n.as_str()];
        args.extend(titulo.as_deref());
        if corpo.is_some() {
            args.extend(["--body-file", "-"]);
        }
        self.gh(Some(repo), &args, corpo, ESCRITA).map(|_| ())
    }

    /// Tira do rascunho (ou volta para ele, com `desfazer`).
    pub fn pronto_para_revisao(&self, repo: &str, numero: u64, desfazer: bool) -> Result<(), ErroRemoto> {
        let n = numero.to_string();
        let mut args = vec!["pr", "ready", n.as_str()];
        if desfazer {
            args.push("--undo");
        }
        self.gh(Some(repo), &args, None, ESCRITA).map(|_| ())
    }

    pub fn reabrir_pr(&self, repo: &str, numero: u64) -> Result<(), ErroRemoto> {
        self.gh(Some(repo), &["pr", "reopen", &numero.to_string()], None, ESCRITA).map(|_| ())
    }

    pub fn fechar_pr(&self, repo: &str, numero: u64, comentario: Option<&str>) -> Result<(), ErroRemoto> {
        let (n, comentario) = (numero.to_string(), comentario.map(|c| format!("--comment={c}")));
        let mut args = vec!["pr", "close", n.as_str()];
        args.extend(comentario.as_deref());
        self.gh(Some(repo), &args, None, ESCRITA).map(|_| ())
    }

    // ---------- PR: acompanhar ----------

    /// Todos os PRs acompanhados de um repositório numa consulta só (o vigia). Um PR que não existe
    /// mais vira `NaoEncontrado` só nele; os outros seguem.
    pub fn resumo_prs(&self, repo: &str, numeros: &[u64]) -> Result<ResumoDoRepo, ErroRemoto> {
        conferir_repo(repo)?;
        let mut numeros = numeros.to_vec();
        numeros.sort_unstable();
        numeros.dedup();
        if numeros.is_empty() {
            return Ok(ResumoDoRepo::default());
        }
        let (dono, nome) = repo.split_once('/').unwrap_or_default();
        let consulta = consulta_resumo(&numeros);
        let (dono, nome) = (format!("owner={dono}"), format!("name={nome}"));
        // O owner e o name vão com -f: o -F faria de um repositório "2048" um número.
        let args = ["api", "graphql", "-H", PREVIA_MESCLA, "-f", &dono, "-f", &nome, "-F", "query=@-"];
        let saida = self.bruto(None, &args, Some(&consulta), LEITURA)?;
        interpretar_resumo(&numeros, saida.codigo, &saida.texto(), &saida.erros())
    }

    pub fn detalhe_pr(&self, repo: &str, numero: u64) -> Result<DetalhePr, ErroRemoto> {
        let campos = "number,title,body,state,isDraft,author,createdAt,updatedAt,baseRefName,headRefName,headRefOid,\
                      additions,deletions,changedFiles,files,mergeable,mergeStateStatus,reviewDecision,\
                      closingIssuesReferences,url,commits,latestReviews,autoMergeRequest,isCrossRepository,\
                      maintainerCanModify";
        let saida = self.gh(Some(repo), &["pr", "view", &numero.to_string(), "--json", campos], None, LEITURA)?;
        Ok(ler_json::<PrGh>(&saida.stdout)?.into())
    }

    /// Os checks do PR. Com `--json` o gh 2.97 sai com 0 mesmo com falhas ou pendentes (no modo
    /// texto seriam 1 e 8); aceitamos os três com JSON válido no stdout, para não depender disso.
    /// "no checks reported" (código 1) é lista vazia.
    pub fn checks(&self, repo: &str, numero: u64) -> Result<Vec<Check>, ErroRemoto> {
        let campos = "bucket,completedAt,description,event,link,name,startedAt,state,workflow";
        let saida = self.bruto(Some(repo), &["pr", "checks", &numero.to_string(), "--json", campos], None, LEITURA)?;
        let (stdout, stderr) = (saida.texto(), saida.erros());
        if matches!(saida.codigo, Some(0 | 1 | 8))
            && let Ok(checks) = serde_json::from_str::<Vec<Check>>(&stdout)
        {
            return Ok(checks);
        }
        if stderr.contains("no checks reported") {
            return Ok(Vec::new());
        }
        Err(classificar_gh(saida.codigo, &stdout, &stderr))
    }

    /// O trecho do log que explica a falha do check (até 6000 caracteres). `None` enquanto a
    /// execução ainda roda (o log só sai no fim).
    pub fn log_da_falha(&self, repo: &str, link: &str) -> Result<Option<String>, ErroRemoto> {
        conferir_repo(repo)?;
        match origem_do_link(link) {
            Some(OrigemDoLog::Job { job, .. }) => {
                let job = job.to_string();
                let saida = self.bruto(Some(repo), &["run", "view", "--job", &job, "--log-failed"], None, LEITURA)?;
                let (stdout, stderr) = (saida.texto(), saida.erros());
                if saida.codigo == Some(0) {
                    return Ok(Some(recortar_log(&stdout, 6000)));
                }
                if stderr.contains("still in progress") {
                    return Ok(None);
                }
                Err(classificar_gh(saida.codigo, &stdout, &stderr))
            }
            Some(OrigemDoLog::CheckRun(id)) => {
                // App de fora do Actions (CodeQL, por exemplo): sem log, só o que ele publicou.
                let caminho = format!("repos/{repo}/check-runs/{id}");
                let jq = "{t:.output.title,s:.output.summary,x:.output.text,st:.status}";
                let saida = self.gh(None, &["api", &caminho, "--jq", jq], None, LEITURA)?;
                let valor: Value = ler_json(&saida.stdout)?;
                if valor["st"] != "completed" {
                    return Ok(None);
                }
                let partes: Vec<&str> = ["t", "s", "x"]
                    .iter()
                    .filter_map(|c| valor[*c].as_str())
                    .map(str::trim)
                    .filter(|t| !t.is_empty())
                    .collect();
                Ok(Some(git::final_do_texto(&partes.join("\n\n"), 6000)))
            }
            None => Err(ErroRemoto::Validacao(format!("link de check que o Vitral não conhece: {link}"))),
        }
    }

    /// Roda de novo os jobs que falharam (ou um job só).
    pub fn rodar_de_novo(&self, repo: &str, execucao: u64, job: Option<u64>) -> Result<(), ErroRemoto> {
        let (execucao, job) = (execucao.to_string(), job.map(|j| j.to_string()));
        let args = match &job {
            Some(job) => vec!["run", "rerun", "--job", job.as_str()],
            None => vec!["run", "rerun", execucao.as_str(), "--failed"],
        };
        self.gh(Some(repo), &args, None, ESCRITA).map(|_| ())
    }

    /// Comentários em linha, comentários gerais e revisões do PR, com a paginação toda. `desde`
    /// (ISO 8601) vale para os comentários; as revisões vêm sempre todas (a API não filtra).
    pub fn comentarios_novos(&self, repo: &str, numero: u64, desde: Option<&str>) -> Result<Vec<ComentarioPr>, ErroRemoto> {
        conferir_repo(repo)?;
        let desde = desde.filter(|d| !d.is_empty() && d.chars().all(|c| c.is_ascii_digit() || "TZ:-.+".contains(c)));
        let filtro = desde.map(|d| format!("&since={d}")).unwrap_or_default();
        let fontes = [
            (TipoComentario::EmLinha, format!("repos/{repo}/pulls/{numero}/comments?per_page=100{filtro}")),
            (TipoComentario::Geral, format!("repos/{repo}/issues/{numero}/comments?per_page=100{filtro}")),
            (TipoComentario::Revisao, format!("repos/{repo}/pulls/{numero}/reviews?per_page=100")),
        ];
        let mut todos = Vec::new();
        for (tipo, caminho) in fontes {
            let saida = self.gh(None, &["api", "--paginate", "--slurp", &caminho], None, LEITURA)?;
            let paginas: Vec<Vec<ComentarioGh>> = ler_json(&saida.stdout)?;
            todos.extend(paginas.into_iter().flatten().map(|c| c.para(tipo)));
        }
        Ok(todos)
    }

    /// O diff do PR pelo GitHub (quando a cópia já foi limpa).
    pub fn diff_pr(&self, repo: &str, numero: u64) -> Result<String, ErroRemoto> {
        self.gh(Some(repo), &["pr", "diff", &numero.to_string(), "--color", "never"], None, LEITURA).map(|s| s.stdout)
    }

    /// Mescla o PR, só se o head ainda for `sha`. Nunca com `--auto`, `--delete-branch` nem
    /// `--admin`. Num ramo com fila de merge, o gh põe na fila ou liga a mesclagem automática.
    pub fn mesclar(&self, repo: &str, numero: u64, metodo: &str, sha: &str) -> Result<Mescla, ErroRemoto> {
        conferir_sha(sha)?;
        let opcao = match metodo.to_ascii_lowercase().as_str() {
            "squash" => "--squash",
            "merge" => "--merge",
            "rebase" => "--rebase",
            _ => return Err(ErroRemoto::Validacao(format!("método de mesclar desconhecido: {metodo}"))),
        };
        let (n, cabeca) = (numero.to_string(), format!("--match-head-commit={sha}"));
        let saida = self.bruto(Some(repo), &["pr", "merge", &n, opcao, &cabeca], None, ESCRITA)?;
        let (stdout, stderr) = (saida.texto(), saida.erros());
        if saida.codigo != Some(0) {
            return Err(classificar_gh(saida.codigo, &stdout, &stderr));
        }
        let texto = format!("{stdout}\n{stderr}").to_lowercase();
        Ok(if texto.contains("merge queue") {
            Mescla::NaFila
        } else if texto.contains("auto-merge") || texto.contains("automatically merged") {
            Mescla::AutoLigada
        } else {
            Mescla::Mesclado
        })
    }

    /// Desliga a mesclagem automática do GitHub (em todo desarme, se ela estiver ligada).
    pub fn desligar_auto(&self, repo: &str, numero: u64) -> Result<(), ErroRemoto> {
        self.gh(Some(repo), &["pr", "merge", &numero.to_string(), "--disable-auto"], None, ESCRITA).map(|_| ())
    }

    // ---------- issues ----------

    pub fn listar_issues(&self, repo: &str, filtro: &FiltroGh) -> Result<Vec<IssueResumo>, ErroRemoto> {
        let campos = "number,title,state,stateReason,author,assignees,labels,updatedAt,createdAt,url";
        let estado = if filtro.fechadas { "closed" } else { "open" };
        let termo = filtro.termo.as_deref().map(str::trim).filter(|t| !t.is_empty()).map(|t| format!("--search={t}"));
        let mut args = vec!["issue", "list", "--state", estado, "--limit", "50", "--json", campos];
        if filtro.minhas {
            args.extend(["--assignee", "@me"]);
        }
        args.extend(termo.as_deref());
        let saida = self.gh(Some(repo), &args, None, LEITURA)?;
        let lista: Vec<IssueGh> = ler_json(&saida.stdout)?;
        Ok(lista.into_iter().map(|i| i.resumo()).collect())
    }

    pub fn ver_issue(&self, repo: &str, numero: u64) -> Result<IssueDetalhe, ErroRemoto> {
        let campos = "number,title,body,state,stateReason,author,assignees,labels,milestone,comments,createdAt,\
                      updatedAt,url,closedByPullRequestsReferences";
        let saida = self.gh(Some(repo), &["issue", "view", &numero.to_string(), "--json", campos], None, LEITURA)?;
        let issue: IssueGh = ler_json(&saida.stdout)?;
        Ok(issue.detalhe(repo))
    }

    pub fn atribuir_a_mim(&self, repo: &str, numero: u64) -> Result<(), ErroRemoto> {
        self.gh(Some(repo), &["issue", "edit", &numero.to_string(), "--add-assignee", "@me"], None, ESCRITA).map(|_| ())
    }

    /// Fecha como concluída, ou como "não planejada".
    pub fn fechar_issue(&self, repo: &str, numero: u64, nao_planejada: bool) -> Result<(), ErroRemoto> {
        let motivo = if nao_planejada { "not planned" } else { "completed" };
        self.gh(Some(repo), &["issue", "close", &numero.to_string(), "--reason", motivo], None, ESCRITA).map(|_| ())
    }

    pub fn reabrir_issue(&self, repo: &str, numero: u64) -> Result<(), ErroRemoto> {
        self.gh(Some(repo), &["issue", "reopen", &numero.to_string()], None, ESCRITA).map(|_| ())
    }

    /// A sonda da rede (essa chamada não gasta cota).
    pub fn limite(&self) -> Result<Limites, ErroRemoto> {
        let saida = self.gh(None, &["api", "rate_limit"], None, Duration::from_secs(10))?;
        let valor: Value = ler_json(&saida.stdout)?;
        let restantes = |recurso: &str| valor["resources"][recurso]["remaining"].as_u64().map(|n| n as u32);
        Ok(Limites { rest: restantes("core"), graphql: restantes("graphql") })
    }
}

// ---------- tipos ----------

#[derive(Clone, Debug, PartialEq)]
pub struct LoginGh {
    pub login: String,
    pub estado: EstadoLogin,
    pub escopos: Vec<String>,
}

impl LoginGh {
    pub fn tem_escopo(&self, escopo: &str) -> bool {
        self.escopos.iter().any(|e| e == escopo)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum EstadoLogin {
    Ok,
    /// Sem conexão com o GitHub agora.
    TempoEsgotado,
    /// O token não vale mais (o texto já sem segredos).
    Erro(String),
}

/// O que o Vitral usa do repositório de destino (cache de 1 h no motor).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InfoRepo {
    pub repositorio: String,
    pub dono: String,
    pub nome: String,
    pub url: String,
    pub ramo_padrao: String,
    pub fork: bool,
    /// "dono/nome" do original, num fork.
    pub original: Option<String>,
    pub privado: bool,
    /// ADMIN, MAINTAIN, WRITE, TRIAGE ou READ.
    pub permissao: String,
    /// "squash", "merge" ou "rebase".
    pub metodo_padrao: String,
    pub squash: bool,
    pub merge: bool,
    pub rebase: bool,
    pub apaga_ramo_ao_mesclar: bool,
    pub tem_issues: bool,
}

impl InfoRepo {
    pub fn pode_escrever(&self) -> bool {
        matches!(self.permissao.as_str(), "ADMIN" | "MAINTAIN" | "WRITE")
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Login {
    #[serde(deserialize_with = "nulo")]
    login: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Nome {
    #[serde(deserialize_with = "nulo")]
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RepoGh {
    #[serde(deserialize_with = "nulo")]
    name_with_owner: String,
    owner: Option<Login>,
    #[serde(deserialize_with = "nulo")]
    name: String,
    #[serde(deserialize_with = "nulo")]
    url: String,
    default_branch_ref: Option<Nome>,
    is_fork: bool,
    parent: Option<PaiGh>,
    is_private: bool,
    #[serde(deserialize_with = "nulo")]
    viewer_permission: String,
    #[serde(deserialize_with = "nulo")]
    viewer_default_merge_method: String,
    squash_merge_allowed: bool,
    merge_commit_allowed: bool,
    rebase_merge_allowed: bool,
    delete_branch_on_merge: bool,
    has_issues_enabled: bool,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PaiGh {
    #[serde(deserialize_with = "nulo")]
    name: String,
    owner: Option<Login>,
}

impl From<RepoGh> for InfoRepo {
    fn from(r: RepoGh) -> Self {
        let original = r.parent.and_then(|p| Some(format!("{}/{}", p.owner?.login, p.name)));
        Self {
            repositorio: r.name_with_owner,
            dono: r.owner.map(|o| o.login).unwrap_or_default(),
            nome: r.name,
            url: r.url,
            ramo_padrao: r.default_branch_ref.map(|b| b.name).unwrap_or_default(),
            fork: r.is_fork,
            original,
            privado: r.is_private,
            permissao: r.viewer_permission,
            metodo_padrao: r.viewer_default_merge_method.to_ascii_lowercase(),
            squash: r.squash_merge_allowed,
            merge: r.merge_commit_allowed,
            rebase: r.rebase_merge_allowed,
            apaga_ramo_ao_mesclar: r.delete_branch_on_merge,
            tem_issues: r.has_issues_enabled,
        }
    }
}

/// O PR que `pr_do_ramo` reconheceu como o do Vitral.
#[derive(Clone, Debug, PartialEq)]
pub struct PrAchado {
    pub numero: u64,
    pub url: String,
    pub titulo: String,
    pub rascunho: bool,
    pub sha: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mescla {
    Mesclado,
    /// O ramo usa fila de merge: o PR entrou nela.
    NaFila,
    /// Faltava algum check obrigatório: o gh ligou a mesclagem automática do GitHub.
    AutoLigada,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Limites {
    pub rest: Option<u32>,
    pub graphql: Option<u32>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoPr {
    #[default]
    Aberto,
    Rascunho,
    Mesclado,
    Fechado,
}

impl EstadoPr {
    pub fn do_gh(estado: &str, rascunho: bool) -> Self {
        match estado {
            "MERGED" => Self::Mesclado,
            "CLOSED" => Self::Fechado,
            _ if rascunho => Self::Rascunho,
            _ => Self::Aberto,
        }
    }

    pub fn nome(self) -> &'static str {
        match self {
            Self::Aberto => "aberto",
            Self::Rascunho => "rascunho",
            Self::Mesclado => "mesclado",
            Self::Fechado => "fechado",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum EstadoChecks {
    #[default]
    Nenhum,
    Pendente,
    Passou,
    Falhou,
}

/// As contagens dos checks do head do PR. `Falhou` só sem nenhum pendente; com falhas e
/// pendentes, é `Pendente` com `falhos > 0`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResumoChecks {
    pub estado: EstadoChecks,
    pub total: u32,
    /// Os que já terminaram, com os falhos.
    pub terminados: u32,
    pub falhos: u32,
    pub pendentes: u32,
}

const PENDENTES: [&str; 6] = ["QUEUED", "IN_PROGRESS", "PENDING", "WAITING", "REQUESTED", "EXPECTED"];
const FALHOS: [&str; 6] = ["FAILURE", "ERROR", "TIMED_OUT", "CANCELLED", "ACTION_REQUIRED", "STARTUP_FAILURE"];

impl ResumoChecks {
    /// A partir de pares (estado, quantidade), como o `checkRunCountsByState` e o
    /// `statusContextCountsByState` do GraphQL.
    pub fn das_contagens<'a>(contagens: impl IntoIterator<Item = (&'a str, u32)>) -> Self {
        let (mut total, mut pendentes, mut falhos) = (0u32, 0u32, 0u32);
        for (estado, quantos) in contagens {
            total = total.saturating_add(quantos);
            if PENDENTES.contains(&estado) {
                pendentes = pendentes.saturating_add(quantos);
            } else if FALHOS.contains(&estado) {
                falhos = falhos.saturating_add(quantos);
            }
        }
        let estado = match () {
            _ if total == 0 => EstadoChecks::Nenhum,
            _ if pendentes > 0 => EstadoChecks::Pendente,
            _ if falhos > 0 => EstadoChecks::Falhou,
            _ => EstadoChecks::Passou,
        };
        Self { estado, total, terminados: total - pendentes, falhos, pendentes }
    }

    /// A partir da lista do `gh pr checks` (pelo `bucket`).
    #[allow(dead_code)] // Provisório: o anel da sub-aba Checks (G5).
    pub fn da_lista(checks: &[Check]) -> Self {
        Self::das_contagens(checks.iter().map(|c| match c.balde.as_str() {
            "pending" => ("PENDING", 1),
            "fail" => ("FAILURE", 1),
            "cancel" => ("CANCELLED", 1),
            _ => ("SUCCESS", 1),
        }))
    }
}

/// O estado de um PR que o vigia acompanha, de uma consulta.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResumoPr {
    pub numero: u64,
    pub estado: EstadoPr,
    pub url: String,
    pub ramo: String,
    pub base: String,
    /// O `headRefOid`.
    pub sha: String,
    pub atualizado: String,
    pub dono_do_head: String,
    pub mesclado_por: Option<String>,
    pub commit_mesclado: Option<String>,
    /// MERGEABLE, CONFLICTING ou UNKNOWN.
    pub mesclavel_git: String,
    /// O `mergeStateStatus`: CLEAN, BLOCKED, BEHIND, DIRTY, UNSTABLE, HAS_HOOKS ou UNKNOWN.
    pub mesclavel: String,
    /// O `reviewDecision` ("" sem revisão exigida).
    pub decisao: String,
    /// O método da mesclagem automática do GitHub, se ligada.
    pub auto_github: Option<String>,
    /// O estado na fila de merge, se estiver nela.
    pub fila_de_merge: Option<String>,
    pub checks: ResumoChecks,
    pub conversas: u32,
    pub conversas_abertas: u32,
    pub comentarios: u32,
    pub ultimo_comentario: u64,
    pub revisoes: u32,
    pub ultima_revisao: u64,
    pub estado_ultima_revisao: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ResumoDoRepo {
    pub prs: Vec<(u64, Result<ResumoPr, ErroRemoto>)>,
    pub custo: Option<u32>,
    pub restantes: Option<u32>,
    pub reinicia: Option<String>,
}

/// O cabeçalho que libera o `mergeStateStatus` no GraphQL.
const PREVIA_MESCLA: &str = "Accept: application/vnd.github.merge-info-preview+json";

const FRAGMENTO_RESUMO: &str = "fragment R on PullRequest {
  number state isDraft url headRefName baseRefName headRefOid updatedAt
  headRepositoryOwner{ login } mergedBy{ login } mergeCommit{ oid }
  mergeable mergeStateStatus reviewDecision
  autoMergeRequest{ mergeMethod } mergeQueueEntry{ state }
  commits(last:1){ nodes{ commit{ oid statusCheckRollup{ state
    contexts(first:1){ totalCount checkRunCount checkRunCountsByState{state count}
                       statusContextCount statusContextCountsByState{state count} } } } } }
  reviewThreads(first:100){ totalCount nodes{ isResolved } }
  comments(last:1){ totalCount nodes{ databaseId } }
  reviews(last:1){ totalCount nodes{ databaseId state } }
}
";

/// A consulta do vigia: um apelido `p<N>` por PR.
pub fn consulta_resumo(numeros: &[u64]) -> String {
    let mut consulta = String::from("query($owner:String!,$name:String!){\n  repository(owner:$owner,name:$name){\n");
    for n in numeros {
        consulta.push_str(&format!("    p{n}: pullRequest(number:{n}){{ ...R }}\n"));
    }
    consulta.push_str("  }\n  rateLimit{ cost remaining resetAt }\n}\n");
    consulta.push_str(FRAGMENTO_RESUMO);
    consulta
}

/// Lê a resposta da consulta do vigia. O stdout vale mesmo com código 1: um apelido nulo com o
/// erro em `["repository","pN"]` marca só aquele PR. Sem `data`, vale o erro do gh.
pub fn interpretar_resumo(numeros: &[u64], codigo: Option<i32>, stdout: &str, stderr: &str) -> Result<ResumoDoRepo, ErroRemoto> {
    let valor: Value = serde_json::from_str(stdout).unwrap_or(Value::Null);
    let dados = &valor["data"];
    if !dados.is_object() {
        return Err(match codigo {
            Some(0) => ErroRemoto::Outro("resposta do GitHub sem data".into()),
            _ => classificar_gh(codigo, stdout, stderr),
        });
    }
    let erros = valor["errors"].as_array().cloned().unwrap_or_default();
    let erro_em = |caminho: &[&str]| {
        erros.iter().find(|e| e["path"].as_array().is_some_and(|p| p.iter().map(|x| x.as_str()).eq(caminho.iter().map(|c| Some(*c)))))
    };
    let repositorio = &dados["repository"];
    let prs = numeros
        .iter()
        .map(|&n| {
            let apelido = format!("p{n}");
            let no = &repositorio[&apelido];
            let resultado = if no.is_object() {
                serde_json::from_value::<NoPr>(no.clone())
                    .map(ResumoPr::from)
                    .map_err(|e| ErroRemoto::Outro(format!("resposta do GitHub fora do esperado: {e}")))
            } else {
                let erro = erro_em(&["repository", &apelido]).or_else(|| erro_em(&["repository"]));
                let mensagem = erro.and_then(|e| e["message"].as_str()).unwrap_or("não encontrado").to_string();
                match erro.and_then(|e| e["type"].as_str()) {
                    Some("NOT_FOUND") | None => Err(ErroRemoto::NaoEncontrado(mensagem)),
                    Some(_) => Err(ErroRemoto::Outro(mensagem)),
                }
            };
            (n, resultado)
        })
        .collect();
    let limite = &dados["rateLimit"];
    Ok(ResumoDoRepo {
        prs,
        custo: limite["cost"].as_u64().map(|n| n as u32),
        restantes: limite["remaining"].as_u64().map(|n| n as u32),
        reinicia: limite["resetAt"].as_str().map(String::from),
    })
}

#[derive(Deserialize, Default)]
#[serde(default, bound(deserialize = "T: Deserialize<'de> + Default"))]
struct Nos<T> {
    #[serde(deserialize_with = "nulo")]
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default, bound(deserialize = "T: Deserialize<'de> + Default"))]
struct Contados<T> {
    total_count: u32,
    #[serde(deserialize_with = "nulo")]
    nodes: Vec<Option<T>>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct NoPr {
    number: u64,
    #[serde(deserialize_with = "nulo")]
    state: String,
    is_draft: bool,
    #[serde(deserialize_with = "nulo")]
    url: String,
    #[serde(deserialize_with = "nulo")]
    head_ref_name: String,
    #[serde(deserialize_with = "nulo")]
    base_ref_name: String,
    #[serde(deserialize_with = "nulo")]
    head_ref_oid: String,
    #[serde(deserialize_with = "nulo")]
    updated_at: String,
    head_repository_owner: Option<Login>,
    merged_by: Option<Login>,
    merge_commit: Option<Oid>,
    #[serde(deserialize_with = "nulo")]
    mergeable: String,
    #[serde(deserialize_with = "nulo")]
    merge_state_status: String,
    #[serde(deserialize_with = "nulo")]
    review_decision: String,
    auto_merge_request: Option<PedidoAuto>,
    merge_queue_entry: Option<EntradaNaFila>,
    #[serde(deserialize_with = "nulo")]
    commits: Nos<NoCommit>,
    #[serde(deserialize_with = "nulo")]
    review_threads: Contados<Conversa>,
    #[serde(deserialize_with = "nulo")]
    comments: Contados<IdDoBanco>,
    #[serde(deserialize_with = "nulo")]
    reviews: Contados<IdDoBanco>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Oid {
    #[serde(deserialize_with = "nulo")]
    oid: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PedidoAuto {
    #[serde(deserialize_with = "nulo")]
    merge_method: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct EntradaNaFila {
    #[serde(deserialize_with = "nulo")]
    state: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct NoCommit {
    commit: Option<CommitDoRollup>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CommitDoRollup {
    status_check_rollup: Option<Rollup>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Rollup {
    contexts: Option<Contextos>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Contextos {
    #[serde(deserialize_with = "nulo")]
    check_run_counts_by_state: Vec<Contagem>,
    #[serde(deserialize_with = "nulo")]
    status_context_counts_by_state: Vec<Contagem>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Contagem {
    #[serde(deserialize_with = "nulo")]
    state: String,
    count: u32,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Conversa {
    is_resolved: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct IdDoBanco {
    database_id: Option<u64>,
    #[serde(deserialize_with = "nulo")]
    state: String,
}

impl From<NoPr> for ResumoPr {
    fn from(no: NoPr) -> Self {
        let rollup = no.commits.nodes.into_iter().flatten().next().and_then(|c| c.commit?.status_check_rollup);
        let checks = match rollup.and_then(|r| r.contexts) {
            Some(c) => ResumoChecks::das_contagens(
                c.check_run_counts_by_state
                    .iter()
                    .chain(&c.status_context_counts_by_state)
                    .map(|x| (x.state.as_str(), x.count)),
            ),
            None => ResumoChecks::default(),
        };
        let conversas_abertas = no.review_threads.nodes.iter().flatten().filter(|c| !c.is_resolved).count() as u32;
        let ultima = |c: &Contados<IdDoBanco>| c.nodes.iter().flatten().last().map(|n| (n.database_id.unwrap_or(0), n.state.clone()));
        let (ultimo_comentario, _) = ultima(&no.comments).unwrap_or_default();
        let (ultima_revisao, estado_ultima_revisao) = ultima(&no.reviews).unwrap_or_default();
        Self {
            numero: no.number,
            estado: EstadoPr::do_gh(&no.state, no.is_draft),
            url: no.url,
            ramo: no.head_ref_name,
            base: no.base_ref_name,
            sha: no.head_ref_oid,
            atualizado: no.updated_at,
            dono_do_head: no.head_repository_owner.map(|d| d.login).unwrap_or_default(),
            mesclado_por: no.merged_by.map(|d| d.login),
            commit_mesclado: no.merge_commit.map(|c| c.oid),
            mesclavel_git: no.mergeable,
            mesclavel: no.merge_state_status,
            decisao: no.review_decision,
            auto_github: no.auto_merge_request.map(|a| a.merge_method),
            fila_de_merge: no.merge_queue_entry.map(|f| f.state),
            checks,
            conversas: no.review_threads.total_count,
            conversas_abertas,
            comentarios: no.comments.total_count,
            ultimo_comentario,
            revisoes: no.reviews.total_count,
            ultima_revisao,
            estado_ultima_revisao,
        }
    }
}

/// Um check do `gh pr checks --json`.
#[derive(Clone, Debug, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct Check {
    #[serde(rename = "name", deserialize_with = "nulo")]
    pub nome: String,
    /// SUCCESS, FAILURE, SKIPPED, PENDING, IN_PROGRESS…
    #[serde(rename = "state", deserialize_with = "nulo")]
    pub estado: String,
    /// pass, fail, pending, skipping ou cancel.
    #[serde(rename = "bucket", deserialize_with = "nulo")]
    pub balde: String,
    #[serde(deserialize_with = "nulo")]
    pub link: String,
    #[serde(deserialize_with = "nulo")]
    pub workflow: String,
    #[serde(rename = "description", deserialize_with = "nulo")]
    pub descricao: String,
    #[serde(rename = "event", deserialize_with = "nulo")]
    pub evento: String,
    #[serde(rename = "startedAt", deserialize_with = "nulo")]
    pub inicio: String,
    #[serde(rename = "completedAt", deserialize_with = "nulo")]
    pub fim: String,
}

/// De onde vem o log de um check, pelo link dele.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrigemDoLog {
    /// `/actions/runs/<run>/job/<job>`: um job do Actions (o id do job é o do check-run).
    Job { execucao: u64, job: u64 },
    /// `/runs/<id>`: o check-run de um app de fora do Actions.
    CheckRun(u64),
}

fn numero_depois(texto: &str, marca: &str) -> Option<(u64, usize)> {
    let inicio = texto.find(marca)? + marca.len();
    let digitos: String = texto[inicio..].chars().take_while(char::is_ascii_digit).collect();
    Some((digitos.parse().ok()?, inicio + digitos.len()))
}

pub fn origem_do_link(link: &str) -> Option<OrigemDoLog> {
    let caminho = link.split(['?', '#']).next()?;
    if let Some((execucao, depois)) = numero_depois(caminho, "/actions/runs/") {
        let (job, _) = numero_depois(&caminho[depois..], "/job/")?;
        return Some(OrigemDoLog::Job { execucao, job });
    }
    let (id, depois) = numero_depois(caminho, "/runs/")?;
    caminho[depois..].trim_end_matches('/').is_empty().then_some(OrigemDoLog::CheckRun(id))
}

/// O número do PR na URL que o `gh pr create` imprime (`…/pull/12`).
pub fn numero_da_url(url: &str) -> Option<u64> {
    let (_, numero) = url.trim().trim_end_matches('/').rsplit_once("/pull/")?;
    (!numero.is_empty() && numero.chars().all(|c| c.is_ascii_digit())).then(|| numero.parse().ok())?
}

/// Tira as cores (`ESC[…m` e a forma que o gh imprime, `^[[…m`).
fn sem_escapes(linha: &str) -> String {
    let mut limpo = String::with_capacity(linha.len());
    let mut resto = linha;
    loop {
        let achou = [resto.find("\u{1b}["), resto.find("^[[")].into_iter().flatten().min();
        let Some(inicio) = achou else {
            limpo.push_str(resto);
            return limpo;
        };
        limpo.push_str(&resto[..inicio]);
        let depois = &resto[inicio..];
        let pulo = if depois.starts_with('\u{1b}') { 2 } else { 3 };
        let corpo = &depois[pulo..];
        let fim = corpo.find(|c: char| !(c.is_ascii_digit() || c == ';'));
        match fim {
            Some(fim) if corpo[fim..].starts_with(|c: char| c.is_ascii_alphabetic()) => resto = &corpo[fim + 1..],
            _ => {
                limpo.push_str(&depois[..pulo]);
                resto = corpo;
            }
        }
    }
}

/// Tira o carimbo de hora do começo (`2026-10-30T22:30:30.9305946Z `).
fn sem_carimbo(linha: &str) -> &str {
    let b = linha.as_bytes();
    let digitos = |r: std::ops::Range<usize>| r.into_iter().all(|i| b.get(i).is_some_and(u8::is_ascii_digit));
    let forma = b.len() >= 20
        && digitos(0..4)
        && b[4] == b'-'
        && digitos(5..7)
        && b[7] == b'-'
        && digitos(8..10)
        && b[10] == b'T'
        && digitos(11..13)
        && b[13] == b':'
        && digitos(14..16)
        && b[16] == b':'
        && digitos(17..19);
    if !forma {
        return linha;
    }
    let resto = &linha[19..];
    let resto = resto.strip_prefix('.').map_or(resto, |r| r.trim_start_matches(|c: char| c.is_ascii_digit()));
    match resto.strip_prefix('Z') {
        Some(resto) => resto.strip_prefix(' ').unwrap_or(resto),
        None => linha,
    }
}

/// O pedaço do log que interessa, até `limite`: sem BOM, cores, as colunas `job\tpasso\t` e o
/// carimbo de hora; do último `##[error]` volta até o `##[group]Run …` anterior (ou 200 linhas).
pub fn recortar_log(texto: &str, limite: usize) -> String {
    let linhas: Vec<String> = texto
        .lines()
        .map(|linha| {
            let linha = sem_escapes(&linha.replace('\u{feff}', ""));
            let resto = match linha.splitn(3, '\t').collect::<Vec<_>>()[..] {
                [_, _, resto] => resto.to_string(),
                _ => linha.clone(),
            };
            sem_carimbo(&resto).to_string()
        })
        .collect();
    let inicio = match linhas.iter().rposition(|l| l.contains("##[error]")) {
        Some(erro) => {
            let piso = erro.saturating_sub(200);
            linhas[piso..erro].iter().rposition(|l| l.contains("##[group]Run ")).map_or(piso, |p| piso + p)
        }
        None => 0,
    };
    git::final_do_texto(linhas[inicio..].join("\n").trim(), limite)
}

/// Um arquivo do PR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArquivoPr {
    pub caminho: String,
    pub adicoes: u64,
    pub remocoes: u64,
    /// ADDED, MODIFIED, DELETED, RENAMED…
    pub tipo: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CommitPr {
    pub sha: String,
    pub titulo: String,
    pub autores: Vec<String>,
    pub data: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct RevisaoPr {
    pub autor: String,
    pub associacao: String,
    pub estado: String,
    pub corpo: String,
    pub enviada: String,
}

/// O PR inteiro, para a aba PR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DetalhePr {
    pub numero: u64,
    pub titulo: String,
    pub corpo: String,
    pub estado: EstadoPr,
    pub autor: String,
    pub criado: String,
    pub atualizado: String,
    pub base: String,
    pub ramo: String,
    pub sha: String,
    pub adicoes: u64,
    pub remocoes: u64,
    pub arquivos_mudados: u64,
    pub arquivos: Vec<ArquivoPr>,
    pub mesclavel_git: String,
    pub mesclavel: String,
    pub decisao: String,
    /// Os números das issues que o PR fecha ao ser mesclado.
    pub issues_fechadas: Vec<u64>,
    pub url: String,
    pub commits: Vec<CommitPr>,
    pub revisoes: Vec<RevisaoPr>,
    pub auto_github: Option<String>,
    pub de_outro_repositorio: bool,
    pub mantenedor_pode_mudar: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct PrGh {
    number: u64,
    #[serde(deserialize_with = "nulo")]
    title: String,
    #[serde(deserialize_with = "nulo")]
    body: String,
    #[serde(deserialize_with = "nulo")]
    state: String,
    is_draft: bool,
    author: Option<Login>,
    #[serde(deserialize_with = "nulo")]
    created_at: String,
    #[serde(deserialize_with = "nulo")]
    updated_at: String,
    #[serde(deserialize_with = "nulo")]
    base_ref_name: String,
    #[serde(deserialize_with = "nulo")]
    head_ref_name: String,
    #[serde(deserialize_with = "nulo")]
    head_ref_oid: String,
    additions: u64,
    deletions: u64,
    changed_files: u64,
    #[serde(deserialize_with = "nulo")]
    files: Vec<ArquivoGh>,
    #[serde(deserialize_with = "nulo")]
    mergeable: String,
    #[serde(deserialize_with = "nulo")]
    merge_state_status: String,
    #[serde(deserialize_with = "nulo")]
    review_decision: String,
    #[serde(deserialize_with = "nulo")]
    closing_issues_references: Vec<Numero>,
    #[serde(deserialize_with = "nulo")]
    url: String,
    #[serde(deserialize_with = "nulo")]
    commits: Vec<CommitGh>,
    #[serde(deserialize_with = "nulo")]
    latest_reviews: Vec<RevisaoGh>,
    auto_merge_request: Option<PedidoAuto>,
    is_cross_repository: bool,
    maintainer_can_modify: bool,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ArquivoGh {
    #[serde(deserialize_with = "nulo")]
    path: String,
    additions: u64,
    deletions: u64,
    #[serde(deserialize_with = "nulo")]
    change_type: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Numero {
    number: u64,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct CommitGh {
    #[serde(deserialize_with = "nulo")]
    oid: String,
    #[serde(deserialize_with = "nulo")]
    message_headline: String,
    #[serde(deserialize_with = "nulo")]
    authors: Vec<Login>,
    #[serde(deserialize_with = "nulo")]
    committed_date: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct RevisaoGh {
    author: Option<Login>,
    #[serde(deserialize_with = "nulo")]
    author_association: String,
    #[serde(deserialize_with = "nulo")]
    state: String,
    #[serde(deserialize_with = "nulo")]
    body: String,
    #[serde(deserialize_with = "nulo")]
    submitted_at: String,
}

impl From<PrGh> for DetalhePr {
    fn from(p: PrGh) -> Self {
        Self {
            numero: p.number,
            titulo: p.title,
            corpo: p.body,
            estado: EstadoPr::do_gh(&p.state, p.is_draft),
            autor: p.author.map(|a| a.login).unwrap_or_default(),
            criado: p.created_at,
            atualizado: p.updated_at,
            base: p.base_ref_name,
            ramo: p.head_ref_name,
            sha: p.head_ref_oid,
            adicoes: p.additions,
            remocoes: p.deletions,
            arquivos_mudados: p.changed_files,
            arquivos: p
                .files
                .into_iter()
                .map(|a| ArquivoPr { caminho: a.path, adicoes: a.additions, remocoes: a.deletions, tipo: a.change_type })
                .collect(),
            mesclavel_git: p.mergeable,
            mesclavel: p.merge_state_status,
            decisao: p.review_decision,
            issues_fechadas: p.closing_issues_references.into_iter().map(|i| i.number).collect(),
            url: p.url,
            commits: p
                .commits
                .into_iter()
                .map(|c| CommitPr {
                    sha: c.oid,
                    titulo: c.message_headline,
                    autores: c.authors.into_iter().map(|a| a.login).filter(|l| !l.is_empty()).collect(),
                    data: c.committed_date,
                })
                .collect(),
            revisoes: p
                .latest_reviews
                .into_iter()
                .map(|r| RevisaoPr {
                    autor: r.author.map(|a| a.login).unwrap_or_default(),
                    associacao: r.author_association,
                    estado: r.state,
                    corpo: r.body,
                    enviada: r.submitted_at,
                })
                .collect(),
            auto_github: p.auto_merge_request.map(|a| a.merge_method),
            de_outro_repositorio: p.is_cross_repository,
            mantenedor_pode_mudar: p.maintainer_can_modify,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TipoComentario {
    #[default]
    Geral,
    EmLinha,
    Revisao,
}

/// Um comentário (geral ou em linha) ou uma revisão do PR.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ComentarioPr {
    pub id: u64,
    pub tipo: TipoComentario,
    pub autor: String,
    /// "User" ou "Bot".
    pub tipo_autor: String,
    /// O `author_association`: OWNER, MEMBER, COLLABORATOR, CONTRIBUTOR, NONE…
    pub associacao: String,
    pub corpo: String,
    pub caminho: Option<String>,
    /// A linha no arquivo (a original, num comentário desatualizado).
    pub linha: Option<u64>,
    pub trecho: Option<String>,
    pub criado: String,
    pub url: String,
    /// Numa revisão: APPROVED, CHANGES_REQUESTED, COMMENTED…
    pub estado_revisao: Option<String>,
    pub revisao: Option<u64>,
    pub resposta_a: Option<u64>,
}

impl ComentarioPr {
    pub fn confianca(&self) -> Confianca {
        issues::confianca(&self.associacao, &self.autor, &self.tipo_autor, self.caminho.is_some())
    }
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct UsuarioGh {
    #[serde(deserialize_with = "nulo")]
    login: String,
    #[serde(rename = "type", deserialize_with = "nulo")]
    tipo: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ComentarioGh {
    id: u64,
    user: Option<UsuarioGh>,
    #[serde(deserialize_with = "nulo")]
    author_association: String,
    #[serde(deserialize_with = "nulo")]
    body: String,
    path: Option<String>,
    line: Option<u64>,
    original_line: Option<u64>,
    diff_hunk: Option<String>,
    #[serde(deserialize_with = "nulo")]
    created_at: String,
    #[serde(deserialize_with = "nulo")]
    submitted_at: String,
    #[serde(deserialize_with = "nulo")]
    html_url: String,
    state: Option<String>,
    pull_request_review_id: Option<u64>,
    in_reply_to_id: Option<u64>,
}

impl ComentarioGh {
    fn para(self, tipo: TipoComentario) -> ComentarioPr {
        let usuario = self.user.unwrap_or_default();
        ComentarioPr {
            id: self.id,
            tipo,
            autor: usuario.login,
            tipo_autor: usuario.tipo,
            associacao: self.author_association,
            corpo: self.body,
            caminho: self.path,
            linha: self.line.or(self.original_line),
            trecho: self.diff_hunk,
            criado: if self.created_at.is_empty() { self.submitted_at } else { self.created_at },
            url: self.html_url,
            estado_revisao: self.state,
            revisao: self.pull_request_review_id,
            resposta_a: self.in_reply_to_id,
        }
    }
}

/// O filtro das issues do GitHub no popover.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FiltroGh {
    pub fechadas: bool,
    pub minhas: bool,
    pub termo: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct PessoaGh {
    #[serde(deserialize_with = "nulo")]
    login: String,
    #[serde(deserialize_with = "nulo")]
    name: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct EtiquetaGh {
    #[serde(deserialize_with = "nulo")]
    name: String,
    color: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct ComentarioIssueGh {
    author: Option<Login>,
    #[serde(deserialize_with = "nulo")]
    body: String,
    #[serde(deserialize_with = "nulo")]
    created_at: String,
    #[serde(deserialize_with = "nulo")]
    url: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct MarcoGh {
    #[serde(deserialize_with = "nulo")]
    title: String,
}

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct IssueGh {
    number: u64,
    #[serde(deserialize_with = "nulo")]
    title: String,
    #[serde(deserialize_with = "nulo")]
    body: String,
    #[serde(deserialize_with = "nulo")]
    state: String,
    #[serde(deserialize_with = "nulo")]
    state_reason: String,
    author: Option<Login>,
    #[serde(deserialize_with = "nulo")]
    assignees: Vec<PessoaGh>,
    #[serde(deserialize_with = "nulo")]
    labels: Vec<EtiquetaGh>,
    milestone: Option<MarcoGh>,
    #[serde(deserialize_with = "nulo")]
    comments: Vec<ComentarioIssueGh>,
    #[serde(deserialize_with = "nulo")]
    created_at: String,
    #[serde(deserialize_with = "nulo")]
    updated_at: String,
    #[serde(deserialize_with = "nulo")]
    url: String,
}

/// O estado de uma issue do GitHub no vocabulário comum.
pub fn estado_github(estado: &str, motivo: &str) -> EstadoIssue {
    let (tipo, nome) = match (estado, motivo) {
        ("OPEN", _) => (TipoEstado::Aberta, "Aberta"),
        (_, "COMPLETED") => (TipoEstado::Concluida, "Concluída"),
        (_, "NOT_PLANNED") => (TipoEstado::Cancelada, "Não planejada"),
        (_, "DUPLICATE") => (TipoEstado::Duplicada, "Duplicada"),
        _ => (TipoEstado::Fechada, "Fechada"),
    };
    EstadoIssue { id: estado.to_string(), nome: nome.to_string(), tipo, cor: None }
}

impl IssueGh {
    fn resumo(&self) -> IssueResumo {
        IssueResumo {
            fonte: FonteIssue::Github,
            id: self.number.to_string(),
            chave: format!("#{}", self.number),
            titulo: self.title.clone(),
            estado: estado_github(&self.state, &self.state_reason),
            responsavel: self.assignees.first().map(|p| Pessoa {
                id: p.login.clone(),
                nome: if p.name.is_empty() { p.login.clone() } else { p.name.clone() },
            }),
            etiquetas: self.labels.iter().map(|e| Etiqueta { nome: e.name.clone(), cor: e.color.clone() }).collect(),
            prioridade: None,
            atualizada: self.updated_at.clone(),
            criada: self.created_at.clone(),
            url: self.url.clone(),
            ramo_sugerido: None,
            autor: self.author.as_ref().map(|a| a.login.clone()),
        }
    }

    fn detalhe(self, repo: &str) -> IssueDetalhe {
        let resumo = self.resumo();
        IssueDetalhe {
            resumo,
            descricao: self.body,
            comentarios: self
                .comments
                .into_iter()
                .map(|c| ComentarioIssue {
                    autor: c.author.map(|a| a.login).unwrap_or_default(),
                    corpo: c.body,
                    criado: c.created_at,
                    url: c.url,
                })
                .collect(),
            ciclo: self.milestone.map(|m| m.title).filter(|t| !t.is_empty()),
            projeto: None,
            time: None,
            repositorio: Some(repo.to_string()),
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::remoto::falso::{Falso, saida};

    const RESUMO: &str = include_str!("../tests/amostras/github/resumo_prs.json");
    const CHECKS: &str = include_str!("../tests/amostras/github/pr_checks.json");
    const PR_VIEW: &str = include_str!("../tests/amostras/github/pr_view.json");
    const EM_LINHA: &str = include_str!("../tests/amostras/github/pr_comentarios_em_linha.json");
    const GERAIS: &str = include_str!("../tests/amostras/github/pr_comentarios_gerais.json");
    const REVISOES: &str = include_str!("../tests/amostras/github/pr_revisoes.json");
    const ISSUES: &str = include_str!("../tests/amostras/github/issue_list.json");
    const ISSUE: &str = include_str!("../tests/amostras/github/issue_view.json");

    fn gh<'a>(e: &'a Falso, pasta: &'a Path) -> Gh<'a> {
        Gh { e, pasta }
    }

    fn args(e: &Falso, i: usize) -> Vec<String> {
        e.chamadas()[i].args.clone()
    }

    #[test]
    fn github_rs_nunca_toca_no_token() {
        let fonte = include_str!("github.rs");
        let codigo = &fonte[..fonte.find("#[cfg(test)]\nmod testes").unwrap()];
        // A lista do que sai do ambiente é a única que pode citar essas variáveis.
        let inicio = codigo.find("const REMOVER").unwrap();
        let fim = inicio + codigo[inicio..].find("];").unwrap();
        let codigo = format!("{}{}", &codigo[..inicio], &codigo[fim..]);
        let proibidos = [
            concat!("auth", " token"),
            concat!("\"tok", "en\""),
            concat!("--show", "-token"),
            concat!("--with", "-token"),
            concat!("\"-", "t\""),
            concat!("GH_", "DEBUG"),
            concat!("GH_", "TOKEN"),
            concat!("hosts", ".yml"),
        ];
        for proibido in proibidos {
            assert!(!codigo.contains(proibido), "github.rs cita {proibido}");
        }
    }

    #[test]
    fn todo_gh_de_repositorio_leva_r_e_roda_na_pasta_de_dados() {
        let e = Falso::default();
        let pasta = Path::new("/dados/vitral");
        let g = gh(&e, pasta);
        let r = "o/r";
        let _ = g.info_repo(r);
        let _ = g.workflows(r);
        let _ = g.ramos(r);
        let _ = g.meus_ramos_de_pr(r);
        let _ = g.pr_do_ramo(r, "vitral/t1-x", "o", "abc1234");
        let _ = g.criar_pr(r, "vitral/t1-x", "main", "t", "c", true);
        let _ = g.editar_pr(r, 1, Some("t"), Some("c"));
        let _ = g.pronto_para_revisao(r, 1, true);
        let _ = g.reabrir_pr(r, 1);
        let _ = g.fechar_pr(r, 1, Some("tchau"));
        let _ = g.resumo_prs(r, &[1, 2]);
        let _ = g.detalhe_pr(r, 1);
        let _ = g.checks(r, 1);
        let _ = g.log_da_falha(r, "https://github.com/o/r/actions/runs/5/job/6");
        let _ = g.log_da_falha(r, "https://github.com/o/r/runs/7");
        let _ = g.rodar_de_novo(r, 5, None);
        let _ = g.rodar_de_novo(r, 5, Some(6));
        let _ = g.comentarios_novos(r, 1, Some("2026-10-27T17:00:00Z"));
        let _ = g.diff_pr(r, 1);
        let _ = g.mesclar(r, 1, "squash", "abc1234");
        let _ = g.desligar_auto(r, 1);
        let _ = g.listar_issues(r, &FiltroGh::default());
        let _ = g.ver_issue(r, 1);
        let _ = g.atribuir_a_mim(r, 1);
        let _ = g.fechar_issue(r, 1, true);
        let _ = g.reabrir_issue(r, 1);
        let chamadas = e.chamadas();
        assert!(chamadas.len() >= 26);
        for chamada in &chamadas {
            assert_eq!(chamada.programa, "gh");
            assert_eq!(chamada.pasta, pasta);
            for variavel in REMOVER {
                assert!(chamada.ambiente.remover.contains(&variavel));
            }
            assert!(chamada.ambiente.definir.contains(&("GH_PROMPT_DISABLED".into(), "1".into())));
            let a = &chamada.args;
            let texto = a.join(" ");
            assert!(!texto.contains("{owner}") && !texto.contains("{repo}"), "{texto}");
            match a[0].as_str() {
                "pr" | "issue" | "run" => assert_eq!(&a[2..4], ["-R", r], "{texto}"),
                "repo" => assert_eq!(a[2], r),
                "api" => {
                    let caminho = a.iter().skip(1).find(|x| x.starts_with("repos/") || *x == "graphql").unwrap();
                    assert!(caminho == "graphql" || caminho.starts_with("repos/o/r/"), "{texto}");
                }
                outro => panic!("gh {outro}"),
            }
        }
        // Repositório estranho nem chega ao gh.
        let antes = e.chamadas().len();
        assert!(matches!(g.detalhe_pr("o/r --help", 1), Err(ErroRemoto::Validacao(_))));
        assert!(matches!(g.workflows("../x"), Err(ErroRemoto::Validacao(_))));
        assert_eq!(e.chamadas().len(), antes);
    }

    #[test]
    fn login_sem_conta_com_timeout_e_conectado() {
        let pasta = Path::new("/");
        let e = Falso::com(vec![
            saida(0, "", ""),
            saida(0, r#"{"error":"","login":"VictorMenegate","scopes":"gist, read:org, repo, workflow","state":"timeout"}"#, ""),
            saida(0, r#"{"error":null,"login":"VictorMenegate","scopes":"gist, read:org, repo, workflow","state":"success"}"#, ""),
            saida(0, r#"{"error":"token inválido","login":"x","scopes":"","state":"error"}"#, ""),
        ]);
        let g = gh(&e, pasta);
        assert_eq!(g.login(), Ok(None));
        assert_eq!(g.login().unwrap().unwrap().estado, EstadoLogin::TempoEsgotado);
        let conectado = g.login().unwrap().unwrap();
        assert_eq!(conectado.estado, EstadoLogin::Ok);
        assert_eq!(conectado.login, "VictorMenegate");
        assert!(conectado.tem_escopo("repo") && !conectado.tem_escopo("project"));
        assert_eq!(g.login().unwrap().unwrap().estado, EstadoLogin::Erro("token inválido".into()));
        assert_eq!(
            args(&e, 0),
            ["auth", "status", "--json", "hosts", "--jq", r#"(.hosts["github.com"] // [])[] | select(.active) | {login,state,error,scopes}"#]
        );
        let sem_gh = Falso::com(vec![Err(ErroRemoto::SemPrograma("gh"))]);
        assert_eq!(gh(&sem_gh, pasta).login(), Err(ErroRemoto::SemPrograma("gh")));
    }

    #[test]
    fn consulta_do_vigia_tem_apelidos_e_os_argumentos_certos() {
        let esperada = "query($owner:String!,$name:String!){
  repository(owner:$owner,name:$name){
    p12: pullRequest(number:12){ ...R }
    p15: pullRequest(number:15){ ...R }
  }
  rateLimit{ cost remaining resetAt }
}
fragment R on PullRequest {
  number state isDraft url headRefName baseRefName headRefOid updatedAt
  headRepositoryOwner{ login } mergedBy{ login } mergeCommit{ oid }
  mergeable mergeStateStatus reviewDecision
  autoMergeRequest{ mergeMethod } mergeQueueEntry{ state }
  commits(last:1){ nodes{ commit{ oid statusCheckRollup{ state
    contexts(first:1){ totalCount checkRunCount checkRunCountsByState{state count}
                       statusContextCount statusContextCountsByState{state count} } } } } }
  reviewThreads(first:100){ totalCount nodes{ isResolved } }
  comments(last:1){ totalCount nodes{ databaseId } }
  reviews(last:1){ totalCount nodes{ databaseId state } }
}
";
        assert_eq!(consulta_resumo(&[12, 15]), esperada);
        let e = Falso::com(vec![saida(0, r#"{"data":{"repository":{},"rateLimit":null}}"#, "")]);
        let _ = gh(&e, Path::new("/")).resumo_prs("dono/2048", &[15, 12, 15]);
        let chamada = &e.chamadas()[0];
        assert_eq!(
            chamada.args,
            [
                "api",
                "graphql",
                "-H",
                "Accept: application/vnd.github.merge-info-preview+json",
                "-f",
                "owner=dono",
                "-f",
                "name=2048",
                "-F",
                "query=@-"
            ]
        );
        assert_eq!(chamada.entrada.as_deref(), Some(esperada.as_bytes()));
        assert_eq!(chamada.limite, LEITURA);
    }

    #[test]
    fn resumo_com_erro_parcial_afeta_so_o_pr_que_nao_existe() {
        let e = Falso::com(vec![saida(1, RESUMO, "gh: Could not resolve to a PullRequest with the number of 999999.\n")]);
        let resumo = gh(&e, Path::new("/")).resumo_prs("cli/cli", &[14845, 14783, 999999]).unwrap();
        assert_eq!(resumo.custo, Some(1));
        assert_eq!(resumo.restantes, Some(4990));
        let pr = |n: u64| resumo.prs.iter().find(|(x, _)| *x == n).unwrap().1.clone();
        assert!(matches!(pr(999999), Err(ErroRemoto::NaoEncontrado(m)) if m.contains("999999")));
        let a = pr(14845).unwrap();
        assert_eq!(a.estado, EstadoPr::Aberto);
        assert_eq!(a.sha, "04c4ebf52e7033a83eb38e9741c8ace05bf8617b");
        assert_eq!(a.ramo, "babakks/skip-refresh-ro-lock");
        assert_eq!(a.base, "trunk");
        assert_eq!(a.dono_do_head, "cli");
        assert_eq!(a.mesclavel, "BLOCKED");
        assert_eq!(a.decisao, "REVIEW_REQUIRED");
        assert_eq!(a.checks, ResumoChecks { estado: EstadoChecks::Passou, total: 11, terminados: 11, falhos: 0, pendentes: 0 });
        assert_eq!((a.conversas, a.conversas_abertas), (1, 0));
        assert_eq!((a.revisoes, a.ultima_revisao, a.estado_ultima_revisao.as_str()), (2, 5671811270, "COMMENTED"));
        assert_eq!(a.auto_github, None);
        let b = pr(14783).unwrap();
        assert_eq!(b.estado, EstadoPr::Rascunho);
        assert_eq!(b.checks.total, 21);
        assert_eq!(b.checks.estado, EstadoChecks::Passou);
        assert_eq!((b.conversas, b.conversas_abertas), (3, 3));
        assert_eq!((b.comentarios, b.ultimo_comentario), (1, 6444184320));
        // Sem data: vale o erro do gh, para o repositório todo.
        let sem_dados = Falso::com(vec![saida(1, "", "HTTP 401: Bad credentials (https://api.github.com/graphql)")]);
        assert_eq!(gh(&sem_dados, Path::new("/")).resumo_prs("cli/cli", &[1]), Err(ErroRemoto::SemLogin));
        // Repositório que sumiu: todos os PRs dele ficam como não encontrados.
        let sumiu = r#"{"data":{"repository":null,"rateLimit":{"cost":1,"remaining":4000,"resetAt":"x"}},"errors":[{"type":"NOT_FOUND","path":["repository"],"message":"Could not resolve to a Repository with the name 'o/r'."}]}"#;
        let e = Falso::com(vec![saida(1, sumiu, "")]);
        let resumo = gh(&e, Path::new("/")).resumo_prs("o/r", &[1, 2]).unwrap();
        assert!(resumo.prs.iter().all(|(_, r)| matches!(r, Err(ErroRemoto::NaoEncontrado(m)) if m.contains("Repository"))));
        assert!(gh(&Falso::default(), Path::new("/")).resumo_prs("o/r", &[]).unwrap().prs.is_empty());
    }

    #[test]
    fn resumo_dos_checks_pelas_contagens() {
        let r = ResumoChecks::das_contagens([("SUCCESS", 15), ("IN_PROGRESS", 24), ("FAILURE", 2), ("SKIPPED", 0)]);
        assert_eq!(r, ResumoChecks { estado: EstadoChecks::Pendente, total: 41, terminados: 17, falhos: 2, pendentes: 24 });
        let falhou = ResumoChecks::das_contagens([("SUCCESS", 38), ("TIMED_OUT", 1), ("STARTUP_FAILURE", 1), ("ACTION_REQUIRED", 1)]);
        assert_eq!((falhou.estado, falhou.falhos, falhou.terminados), (EstadoChecks::Falhou, 3, 41));
        let passou = ResumoChecks::das_contagens([("SUCCESS", 3), ("NEUTRAL", 1), ("SKIPPED", 2), ("STALE", 1), ("COMPLETED", 1)]);
        assert_eq!((passou.estado, passou.total), (EstadoChecks::Passou, 8));
        for pendente in PENDENTES {
            assert_eq!(ResumoChecks::das_contagens([(pendente, 1)]).estado, EstadoChecks::Pendente, "{pendente}");
        }
        for falho in FALHOS {
            assert_eq!(ResumoChecks::das_contagens([(falho, 1)]).estado, EstadoChecks::Falhou, "{falho}");
        }
        assert_eq!(ResumoChecks::das_contagens([]).estado, EstadoChecks::Nenhum);
        // Sem rollup (statusCheckRollup nulo) é Nenhum.
        let sem = r#"{"data":{"repository":{"p1":{"number":1,"state":"OPEN","commits":{"nodes":[{"commit":{"oid":"a","statusCheckRollup":null}}]}}}}}"#;
        let resumo = interpretar_resumo(&[1], Some(0), sem, "").unwrap();
        assert_eq!(resumo.prs[0].1.as_ref().unwrap().checks.estado, EstadoChecks::Nenhum);
    }

    #[test]
    fn checks_aceita_os_codigos_do_gh() {
        let falha = r#"[{"bucket":"fail","link":"https://github.com/o/r/actions/runs/41/job/124","name":"testes","state":"FAILURE"}]"#;
        let e = Falso::com(vec![
            saida(0, CHECKS, ""),
            saida(8, r#"[{"bucket":"pending","name":"build","state":"IN_PROGRESS"}]"#, ""),
            saida(1, falha, ""),
            saida(1, "", "no checks reported on the 'vitral/t1-x' branch"),
            saida(1, "", "HTTP 404: Not Found"),
        ]);
        let g = gh(&e, Path::new("/"));
        let todos = g.checks("cli/cli", 14845).unwrap();
        assert_eq!(todos.len(), 3);
        assert_eq!(todos[0].nome, "CodeQL");
        assert_eq!(origem_do_link(&todos[0].link), Some(OrigemDoLog::CheckRun(122364269635)));
        assert_eq!(origem_do_link(&todos[1].link), Some(OrigemDoLog::Job { execucao: 40553652299, job: 122363778241 }));
        assert_eq!(ResumoChecks::da_lista(&todos).estado, EstadoChecks::Passou);
        assert_eq!(ResumoChecks::da_lista(&g.checks("o/r", 1).unwrap()).estado, EstadoChecks::Pendente);
        let falhos = g.checks("o/r", 1).unwrap();
        assert_eq!((falhos[0].balde.as_str(), ResumoChecks::da_lista(&falhos).estado), ("fail", EstadoChecks::Falhou));
        assert_eq!(g.checks("o/r", 1), Ok(Vec::new()));
        assert!(matches!(g.checks("o/r", 1), Err(ErroRemoto::NaoEncontrado(_))));
        assert_eq!(args(&e, 0)[..5], ["pr", "checks", "-R", "cli/cli", "14845"]);
    }

    #[test]
    fn links_de_check_e_url_do_pr() {
        assert_eq!(
            origem_do_link("https://github.com/zed-industries/zed/actions/runs/41138429082/job/124346991273"),
            Some(OrigemDoLog::Job { execucao: 41138429082, job: 124346991273 })
        );
        assert_eq!(
            origem_do_link("https://github.com/o/r/actions/runs/5/job/6?pr=12#step:4:1"),
            Some(OrigemDoLog::Job { execucao: 5, job: 6 })
        );
        assert_eq!(origem_do_link("https://github.com/cli/cli/runs/122364269635"), Some(OrigemDoLog::CheckRun(122364269635)));
        assert_eq!(origem_do_link("https://github.com/o/r/actions/runs/5"), None);
        assert_eq!(origem_do_link("https://vercel.com/o/r/deploy"), None);
        assert_eq!(numero_da_url("https://github.com/o/r/pull/12"), Some(12));
        assert_eq!(numero_da_url("https://github.com/o/r/pull/12/\n"), Some(12));
        assert_eq!(numero_da_url("https://github.com/o/r/pull/12x"), None);
        assert_eq!(numero_da_url("https://github.com/o/r/issues/12"), None);
    }

    #[test]
    fn recortar_log_fica_com_o_passo_que_falhou() {
        let mut log = String::from("\u{feff}run_tests\tUNKNOWN STEP\t2026-10-30T22:30:30.9305946Z Current runner version: '2.338.0'\n");
        for i in 0..300 {
            log.push_str(&format!("run_tests\tUNKNOWN STEP\t2026-10-30T22:31:00.0000000Z linha {i}\n"));
        }
        log.push_str("run_tests\tUNKNOWN STEP\t2026-10-30T22:40:00.1Z ##[group]Run cargo test\n");
        log.push_str("run_tests\tUNKNOWN STEP\t2026-10-30T22:41:59.4400260Z ^[[31;1m        FAIL^[[0m [  13.109s] agent::teste\n");
        log.push_str("run_tests\tUNKNOWN STEP\t2026-10-30T22:41:59.4587967Z \u{1b}[31;1merror\u{1b}[0m: test run failed\n");
        log.push_str("run_tests\tUNKNOWN STEP\t2026-10-30T22:41:59.5330464Z ##[error]Process completed with exit code 1.\n");
        let recorte = recortar_log(&log, 6000);
        assert_eq!(
            recorte,
            "##[group]Run cargo test\n        FAIL [  13.109s] agent::teste\nerror: test run failed\n##[error]Process completed with exit code 1."
        );
        // Sem o group antes, volta no máximo 200 linhas; e o limite corta pelo começo.
        let sem_grupo = log.replace("##[group]Run cargo test", "outra coisa");
        let recorte = recortar_log(&sem_grupo, 100_000);
        assert_eq!(recorte.lines().count(), 201);
        assert!(recortar_log(&sem_grupo, 50).chars().count() <= 51);
        // Sem ##[error], fica o fim do log limpo; texto sem colunas passa como está.
        assert_eq!(recortar_log("a\nb ^[[1mnegrito^[[0m\n", 6000), "a\nb negrito");
        assert_eq!(recortar_log("custa ^[ e \u{1b}[ sozinho", 6000), "custa ^[ e \u{1b}[ sozinho");
    }

    #[test]
    fn pr_do_ramo_e_estrito() {
        let sha = "abc1234def";
        let lista = format!(
            r#"[{{"number":1,"state":"OPEN","url":"u1","title":"de outro","headRefOid":"{sha}","isDraft":false,"headRepositoryOwner":{{"login":"intruso"}},"isCrossRepository":true}},
                {{"number":2,"state":"CLOSED","url":"u2","title":"fechado","headRefOid":"{sha}","isDraft":false,"headRepositoryOwner":{{"login":"VictorMenegate"}},"isCrossRepository":false}},
                {{"number":3,"state":"OPEN","url":"u3","title":"outro sha","headRefOid":"fff0000","isDraft":false,"headRepositoryOwner":{{"login":"VictorMenegate"}},"isCrossRepository":false}}]"#
        );
        let certo = r#",{"number":4,"state":"OPEN","url":"https://github.com/o/r/pull/4","title":"o nosso","headRefOid":"abc1234def","isDraft":true,"headRepositoryOwner":{"login":"victormenegate"},"isCrossRepository":false}]"#;
        let com_o_certo = format!("{}{certo}", lista.trim_end().trim_end_matches(']'));
        let e = Falso::com(vec![saida(0, &lista, ""), saida(0, &com_o_certo, "")]);
        let g = gh(&e, Path::new("/"));
        assert_eq!(g.pr_do_ramo("o/r", "vitral/t1-x", "VictorMenegate", sha), Ok(None));
        let achado = g.pr_do_ramo("o/r", "vitral/t1-x", "VictorMenegate", sha).unwrap().unwrap();
        assert_eq!((achado.numero, achado.rascunho), (4, true));
        assert_eq!(args(&e, 0)[..5], ["pr", "list", "-R", "o/r", "--head=vitral/t1-x"]);
    }

    #[test]
    fn criar_pr_tira_o_numero_da_url_e_o_corpo_vai_pelo_stdin() {
        let e = Falso::com(vec![
            saida(0, "\nhttps://github.com/o/r/pull/12\n", "Creating pull request for vitral/t1-x into main in o/r\n"),
            saida(1, "", "a pull request for branch \"vitral/t1-x\" into branch \"main\" already exists"),
        ]);
        let g = gh(&e, Path::new("/"));
        let criado = g.criar_pr("o/r", "vitral/t1-x", "main", "-Título com hífen", "## O que muda\nx", true).unwrap();
        assert_eq!(criado, (12, "https://github.com/o/r/pull/12".to_string()));
        let chamada = &e.chamadas()[0];
        assert_eq!(
            chamada.args,
            ["pr", "create", "-R", "o/r", "--head=vitral/t1-x", "--base=main", "--title=-Título com hífen", "--body-file", "-", "--draft"]
        );
        assert_eq!(chamada.entrada.as_deref(), Some("## O que muda\nx".as_bytes()));
        assert_eq!(chamada.limite, ESCRITA);
        assert!(matches!(g.criar_pr("o/r", "vitral/t1-x", "main", "t", "c", false), Err(ErroRemoto::Validacao(_))));
        assert!(!args(&e, 1).contains(&"--draft".to_string()));
    }

    #[test]
    fn mesclar_sempre_com_o_head_conferido() {
        let e = Falso::com(vec![
            saida(0, "", ""),
            saida(0, "", "✓ Pull request o/r#3 will be added to the merge queue for main when ready\n"),
            saida(0, "", "✓ Pull request o/r#3 will be automatically merged via squash when all requirements are met\n"),
        ]);
        let g = gh(&e, Path::new("/"));
        assert_eq!(g.mesclar("o/r", 3, "SQUASH", "abc1234"), Ok(Mescla::Mesclado));
        assert_eq!(g.mesclar("o/r", 3, "merge", "abc1234"), Ok(Mescla::NaFila));
        assert_eq!(g.mesclar("o/r", 3, "rebase", "abc1234"), Ok(Mescla::AutoLigada));
        assert_eq!(args(&e, 0), ["pr", "merge", "-R", "o/r", "3", "--squash", "--match-head-commit=abc1234"]);
        assert!(matches!(g.mesclar("o/r", 3, "admin", "abc1234"), Err(ErroRemoto::Validacao(_))));
        assert!(matches!(g.mesclar("o/r", 3, "squash", "--admin"), Err(ErroRemoto::Validacao(_))));
        assert_eq!(e.chamadas().len(), 3);
        for chamada in e.chamadas() {
            for proibido in ["--auto", "--admin", "--delete-branch"] {
                assert!(!chamada.args.contains(&proibido.to_string()));
            }
        }
    }

    #[test]
    fn detalhe_comentarios_e_issues_das_amostras() {
        let e = Falso::com(vec![
            saida(0, PR_VIEW, ""),
            saida(0, EM_LINHA, ""),
            saida(0, GERAIS, ""),
            saida(0, REVISOES, ""),
            saida(0, ISSUES, ""),
            saida(0, ISSUE, ""),
        ]);
        let g = gh(&e, Path::new("/"));
        let detalhe = g.detalhe_pr("cli/cli", 14845).unwrap();
        assert_eq!(detalhe.numero, 14845);
        assert_eq!(detalhe.estado, EstadoPr::Aberto);
        assert_eq!(detalhe.autor, "babakks");
        assert_eq!(detalhe.issues_fechadas, [14844]);
        assert_eq!(detalhe.arquivos[0].caminho, "internal/config/config.go");
        assert_eq!(detalhe.commits[0].autores, ["babakks", "Copilot"]);
        assert_eq!(detalhe.revisoes[0].autor, "copilot-pull-request-reviewer");
        assert_eq!(detalhe.auto_github, None);

        let comentarios = g.comentarios_novos("cli/cli", 14845, Some("2026-10-27T17:00:00Z")).unwrap();
        let chamadas = e.chamadas();
        assert_eq!(chamadas[1].args, ["api", "--paginate", "--slurp", "repos/cli/cli/pulls/14845/comments?per_page=100&since=2026-10-27T17:00:00Z"]);
        assert_eq!(chamadas[2].args[3], "repos/cli/cli/issues/14845/comments?per_page=100&since=2026-10-27T17:00:00Z");
        assert_eq!(chamadas[3].args[3], "repos/cli/cli/pulls/14845/reviews?per_page=100");
        let em_linha: Vec<_> = comentarios.iter().filter(|c| c.tipo == TipoComentario::EmLinha).collect();
        assert_eq!(em_linha.len(), 2);
        assert_eq!(em_linha[0].caminho.as_deref(), Some("internal/config/refreshable.go"));
        assert_eq!(em_linha[0].linha, Some(451));
        assert_eq!(em_linha[0].confianca(), Confianca::Confiavel);
        assert!(em_linha[0].trecho.as_deref().unwrap().starts_with("@@ -417,6"));
        let geral = comentarios.iter().find(|c| c.tipo == TipoComentario::Geral).unwrap();
        assert_eq!((geral.autor.as_str(), geral.confianca()), ("xiemocun", Confianca::Terceiro));
        let revisao = comentarios.iter().find(|c| c.tipo == TipoComentario::Revisao).unwrap();
        assert_eq!(revisao.estado_revisao.as_deref(), Some("COMMENTED"));
        assert_eq!(revisao.criado, "2026-10-27T17:40:39Z");

        let issues = g.listar_issues("cli/cli", &FiltroGh { fechadas: false, minhas: true, termo: Some("lock".into()) }).unwrap();
        assert_eq!(
            args(&e, 4)[4..],
            ["--state", "open", "--limit", "50", "--json", "number,title,state,stateReason,author,assignees,labels,updatedAt,createdAt,url", "--assignee", "@me", "--search=lock"]
        );
        assert_eq!(issues.len(), 2);
        assert_eq!((issues[0].chave.as_str(), issues[0].estado.tipo), ("#14895", TipoEstado::Aberta));
        assert_eq!(issues[0].etiquetas[0].nome, "needs-triage");
        assert_eq!(issues[0].autor.as_deref(), Some("Izzur"));
        let issue = g.ver_issue("cli/cli", 14895).unwrap();
        assert_eq!(issue.resumo.fonte, FonteIssue::Github);
        assert_eq!(issue.comentarios[0].autor, "cli-triage");
        assert!(issue.descricao.starts_with("### Describe the bug"));
        assert_eq!(issue.repositorio.as_deref(), Some("cli/cli"));
        assert_eq!(estado_github("CLOSED", "NOT_PLANNED").tipo, TipoEstado::Cancelada);
        assert_eq!(estado_github("CLOSED", "COMPLETED").tipo, TipoEstado::Concluida);
    }

    #[test]
    fn info_do_repositorio_da_amostra() {
        let amostra = r#"{"defaultBranchRef":{"name":"main"},"deleteBranchOnMerge":false,"hasIssuesEnabled":true,"isFork":false,"isPrivate":false,"mergeCommitAllowed":true,"name":"top-bar-app-color","nameWithOwner":"VictorMenegate/top-bar-app-color","owner":{"id":"U_kgDOCMEUhA","login":"VictorMenegate"},"parent":null,"rebaseMergeAllowed":true,"squashMergeAllowed":true,"url":"https://github.com/VictorMenegate/top-bar-app-color","viewerDefaultMergeMethod":"MERGE","viewerPermission":"ADMIN"}"#;
        let fork = r#"{"nameWithOwner":"mrecachinas/cli","isFork":true,"parent":{"name":"cli","owner":{"login":"cli"}},"defaultBranchRef":{"name":"trunk"},"viewerPermission":null}"#;
        let e = Falso::com(vec![saida(0, amostra, ""), saida(0, fork, "")]);
        let g = gh(&e, Path::new("/"));
        let info = g.info_repo("VictorMenegate/top-bar-app-color").unwrap();
        assert_eq!(info.ramo_padrao, "main");
        assert_eq!(info.metodo_padrao, "merge");
        assert!(info.pode_escrever() && info.squash && info.tem_issues && !info.fork);
        assert_eq!(info.original, None);
        let fork = g.info_repo("mrecachinas/cli").unwrap();
        assert_eq!(fork.original.as_deref(), Some("cli/cli"));
        assert!(!fork.pode_escrever());
        assert_eq!(args(&e, 0)[..3], ["repo", "view", "VictorMenegate/top-bar-app-color"]);
    }

    // ---------- manuais, só leitura, contra o GitHub de verdade ----------
    // cargo test -- --ignored integracao_  (o gh logado; nada é escrito em repositório nenhum)

    const JCM: &str = "clodoaldomaldonado1972-boop/jcm-climatizacao";

    fn pasta_neutra() -> std::path::PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-gh-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    /// Os PRs abertos de um repositório, pelo mesmo gh (para não fixar números que fecham).
    fn abertos(g: &Gh, repo: &str, quantos: usize) -> Vec<u64> {
        let saida = g.gh(Some(repo), &["pr", "list", "--state", "open", "--limit", "30", "--json", "number"], None, LEITURA).unwrap();
        let lista: Vec<Value> = serde_json::from_str(&saida.stdout).unwrap();
        lista.iter().filter_map(|p| p["number"].as_u64()).take(quantos).collect()
    }

    #[test]
    #[ignore]
    fn integracao_info_repo() {
        let pasta = pasta_neutra();
        let g = Gh { e: &crate::remoto::Real, pasta: &pasta };
        let cli = g.info_repo("cli/cli").unwrap();
        eprintln!("cli/cli: {cli:?}");
        assert_eq!((cli.ramo_padrao.as_str(), cli.repositorio.as_str()), ("trunk", "cli/cli"));
        assert!(!cli.fork && cli.tem_issues && !cli.metodo_padrao.is_empty());
        let jcm = g.info_repo(JCM).unwrap();
        eprintln!("jcm: permissão {} · método {} · privado {}", jcm.permissao, jcm.metodo_padrao, jcm.privado);
        assert!(jcm.pode_escrever());
        let login = g.login().unwrap().unwrap();
        eprintln!("login: {} {:?} {:?}", login.login, login.estado, login.escopos);
        assert_eq!(login.estado, EstadoLogin::Ok);
        let limites = g.limite().unwrap();
        assert!(limites.rest.is_some() && limites.graphql.is_some());
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    #[ignore]
    fn integracao_resumo_prs_com_erro_parcial() {
        let pasta = pasta_neutra();
        let g = Gh { e: &crate::remoto::Real, pasta: &pasta };
        // Dois PRs mesclados do jcm e um que não existe.
        let resumo = g.resumo_prs(JCM, &[180, 189, 999_999]).unwrap();
        eprintln!("custo {:?} · restantes {:?}", resumo.custo, resumo.restantes);
        for (n, r) in &resumo.prs {
            eprintln!("#{n}: {r:?}");
        }
        let pr = |n: u64| resumo.prs.iter().find(|(x, _)| *x == n).unwrap().1.clone();
        assert!(matches!(pr(999_999), Err(ErroRemoto::NaoEncontrado(_))));
        let mesclado = pr(180).unwrap();
        assert_eq!(mesclado.estado, EstadoPr::Mesclado);
        assert!(mesclado.mesclado_por.is_some() && mesclado.commit_mesclado.is_some());
        assert_eq!(mesclado.checks.falhos, 3, "o #180 foi mesclado com 3 checks em FAILURE");
        assert_eq!(pr(189).unwrap().estado, EstadoPr::Mesclado);
        // PRs abertos do cli/cli: contagens do checkRunCountsByState e mergeStateStatus com a prévia.
        let numeros = abertos(&g, "cli/cli", 2);
        let resumo = g.resumo_prs("cli/cli", &numeros).unwrap();
        for (n, r) in &resumo.prs {
            let r = r.as_ref().unwrap();
            eprintln!("cli/cli#{n}: {:?} · {} · {} · checks {:?}", r.estado, r.mesclavel, r.decisao, r.checks);
            assert!(["CLEAN", "BLOCKED", "BEHIND", "DIRTY", "UNSTABLE", "HAS_HOOKS", "UNKNOWN"].contains(&r.mesclavel.as_str()));
        }
        assert!(resumo.prs.iter().any(|(_, r)| r.as_ref().unwrap().checks.total > 0));
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    #[ignore]
    fn integracao_checks_e_log_da_falha_no_180() {
        let pasta = pasta_neutra();
        let g = Gh { e: &crate::remoto::Real, pasta: &pasta };
        // O código de saída do `gh pr checks --json` com falhas.
        let campos = "bucket,completedAt,description,event,link,name,startedAt,state,workflow";
        let bruto = g.bruto(Some(JCM), &["pr", "checks", "180", "--json", campos], None, LEITURA).unwrap();
        eprintln!("gh pr checks --json com falhas: código {:?}", bruto.codigo);
        let checks = g.checks(JCM, 180).unwrap();
        let resumo = ResumoChecks::da_lista(&checks);
        eprintln!("#180: {resumo:?}");
        assert_eq!((resumo.falhos, resumo.estado), (3, EstadoChecks::Falhou));
        let falho = checks.iter().find(|c| c.balde == "fail" && matches!(origem_do_link(&c.link), Some(OrigemDoLog::Job { .. }))).unwrap();
        // Os logs das execuções do jcm já não estão no GitHub (a API dá 404): o erro diz isso.
        match g.log_da_falha(JCM, &falho.link) {
            Ok(Some(log)) => eprintln!("log de {} no #180: {} caracteres", falho.nome, log.len()),
            Ok(None) => panic!("a execução do #180 já terminou"),
            Err(erro) => {
                eprintln!("log de {} no #180: {:?} → {}", falho.nome, erro, erro.para_usuario("GitHub"));
                assert!(matches!(&erro, ErroRemoto::NaoEncontrado(m) if m == crate::remoto::LOG_SUMIU || m == crate::remoto::LOG_VENCIDO));
            }
        }
        // Um job que falhou há pouco num repositório público, para o recorte de verdade.
        let saida = g
            .gh(Some("cli/cli"), &["run", "list", "--status", "failure", "--limit", "10", "--json", "databaseId"], None, LEITURA)
            .unwrap();
        let execucoes: Vec<Value> = serde_json::from_str(&saida.stdout).unwrap();
        let link = execucoes.iter().filter_map(|r| r["databaseId"].as_u64()).find_map(|id| {
            let jq = r#"[.jobs[] | select(.conclusion=="failure") | .url] | .[0] // empty"#;
            let saida = g.gh(Some("cli/cli"), &["run", "view", &id.to_string(), "--json", "jobs", "--jq", jq], None, LEITURA).ok()?;
            Some(saida.stdout.trim().to_string()).filter(|l| !l.is_empty())
        });
        let link = link.expect("nenhum job com falha recente no cli/cli");
        let log = g.log_da_falha("cli/cli", &link).unwrap().expect("a execução terminou");
        eprintln!("log de {link} ({} caracteres), fim:\n{}", log.len(), git::final_do_texto(&log, 600));
        assert!(!log.is_empty() && log.len() <= 6000 + 4);
        assert!(!log.contains('\u{feff}') && !log.contains("^[[") && !log.contains("\tUNKNOWN STEP\t"));
        // O check do Cloudflare (app de fora, link sem /runs/) não tem log no GitHub.
        let fora: Vec<&Check> = checks.iter().filter(|c| origem_do_link(&c.link).is_none()).collect();
        eprintln!("checks sem log no GitHub: {:?}", fora.iter().map(|c| &c.nome).collect::<Vec<_>>());
        // Com pendentes: procura um PR aberto com checks rodando agora (o vscode quase sempre tem).
        let pendente = ["cli/cli", "microsoft/vscode"].iter().find_map(|repo| {
            abertos(&g, repo, 10).into_iter().find_map(|n| {
                let n = n.to_string();
                let bruto = g.bruto(Some(repo), &["pr", "checks", &n, "--json", campos], None, LEITURA).ok()?;
                let checks: Vec<Check> = serde_json::from_slice(&bruto.stdout).ok()?;
                checks.iter().any(|c| c.balde == "pending").then(|| (repo.to_string(), n, bruto.codigo))
            })
        });
        match pendente {
            Some((repo, n, codigo)) => {
                eprintln!("gh pr checks --json com pendentes ({repo}#{n}): código {codigo:?}");
                assert!(matches!(codigo, Some(0 | 8)));
            }
            None => eprintln!("nenhum PR aberto com checks pendentes agora"),
        }
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    #[ignore]
    fn integracao_paginate_slurp() {
        let pasta = pasta_neutra();
        let g = Gh { e: &crate::remoto::Real, pasta: &pasta };
        let comentarios = g.comentarios_novos("cli/cli", 14845, None).unwrap();
        let conta = |t: TipoComentario| comentarios.iter().filter(|c| c.tipo == t).count();
        eprintln!(
            "cli/cli#14845: {} em linha, {} gerais, {} revisões",
            conta(TipoComentario::EmLinha),
            conta(TipoComentario::Geral),
            conta(TipoComentario::Revisao)
        );
        assert!(conta(TipoComentario::EmLinha) >= 2 && conta(TipoComentario::Revisao) >= 1);
        for c in &comentarios {
            eprintln!("  {:?} {} {} {:?}", c.tipo, c.autor, c.associacao, c.confianca());
        }
        // O since filtra os comentários (o filtro é do GitHub).
        let depois = g.comentarios_novos("cli/cli", 14845, Some("2030-01-01T00:00:00Z")).unwrap();
        assert_eq!(depois.iter().filter(|c| c.tipo != TipoComentario::Revisao).count(), 0);
        let detalhe = g.detalhe_pr("cli/cli", 14845).unwrap();
        eprintln!("detalhe: {} arquivos, {} commits, fecha {:?}", detalhe.arquivos.len(), detalhe.commits.len(), detalhe.issues_fechadas);
        let issues = g.listar_issues("cli/cli", &FiltroGh::default()).unwrap();
        assert!(!issues.is_empty());
        let issue = g.ver_issue("cli/cli", issues[0].id.parse().unwrap()).unwrap();
        eprintln!("issue {}: {} comentários", issue.resumo.chave, issue.comentarios.len());
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
