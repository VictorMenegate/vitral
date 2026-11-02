//! O trabalho do PR fora da thread da interface: publicar (commit, fetch, simulação, push e o PR),
//! conferir os PRs de um repositório (o vigia), ler a falha do CI e os comentários, mesclar e o que
//! vem depois de mesclar. Tudo recebe o `Executor` (o falso nos testes) e bloqueia, menos as
//! esperas pelas travas, que são assíncronas.
//!
//! A ordem das travas é fixa: a vez da rede do repositório (fetch e push), a do projeto por dentro
//! dela só para o que é local (o commit, o merge) e, por último, o gh na fila do provedor. Nenhuma
//! chamada de rede roda dentro da vez do projeto.

use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

use super::regras::{self, EventoPr};
use crate::{
    git::{self, Achado, Empurrao, Juncao},
    github::{Check, ComentarioPr, Gh, Mescla, ResumoDoRepo},
    modelo::Pr,
    prompts::{self, CheckFalho},
    remoto::{ErroRemoto, Executor, sem_segredos},
};

/// O que publicar: tudo já resolvido no motor (o remote, a base, o destino, o ramo).
#[derive(Clone, Debug, Default)]
pub struct Publicacao {
    /// A pasta atual do gh (a pasta de dados do Vitral).
    pub pasta_dados: PathBuf,
    pub copia: PathBuf,
    pub origem: PathBuf,
    pub remoto: String,
    pub base: String,
    /// "dono/nome" do destino do PR.
    pub repositorio: String,
    pub ramo: String,
    /// O `--head` do gh: o ramo, ou "dono:ramo" a partir de um fork.
    pub cabeca: String,
    pub dono_do_head: String,
    pub mensagem: String,
    /// Os arquivos copiados da origem: nunca entram no commit.
    pub excluir: Vec<String>,
    /// O usuário marcou "Confirmo que não são segredos".
    pub segredos_confirmados: bool,
    /// Sem o usuário (o Ship, o Mestre): segredo ou `.github/` param tudo.
    pub automatico: bool,
    /// O que o Vitral enviou da última vez (o `.github/` conta desde ele).
    pub sha_publicado: Option<String>,
    /// O número do PR aberto da tarefa (sem ele, o PR é recuperado ou criado).
    pub pr_aberto: Option<u64>,
    pub url_aberto: String,
    pub titulo: String,
    /// `None`: não muda a descrição (o "Atualizar PR" sem "Atualizar a descrição").
    pub corpo: Option<String>,
    /// Mudar o título do PR aberto também.
    pub novo_titulo: bool,
    pub rascunho: bool,
}

/// O PR depois de publicar.
#[derive(Clone, Debug, PartialEq)]
pub struct Publicado {
    pub numero: u64,
    pub url: String,
    pub titulo: String,
    /// O sha enviado (o `sha_publicado`).
    pub sha: String,
    /// Foi criado agora (e não atualizado, nem recuperado pelo `pr_do_ramo`).
    pub criado: bool,
    pub rascunho: bool,
    /// O push foi, mas a descrição não mudou (o motivo).
    pub aviso: Option<String>,
}

/// Por que não publicou. O texto de cada uma já sai sem segredos.
#[derive(Clone, Debug, PartialEq)]
pub enum FalhaPublicacao {
    Segredos(Vec<Achado>),
    Workflows,
    /// Conflito com `<remoto>/<base>` (o alvo e os arquivos).
    Conflito { alvo: String, arquivos: Vec<String> },
    /// O ramo do PR mudou no GitHub (commits de fora).
    Rejeitado(String),
    /// A proteção de push do GitHub achou um segredo (os lugares).
    SegredoRecusado(String),
    /// O push foi, mas o create estourou o tempo: o próximo passo é conferir com `pr_do_ramo`.
    Incerto { sha: String },
    Remoto(ErroRemoto),
    Git(String),
}

impl FalhaPublicacao {
    /// O que o usuário (ou o chat) lê.
    pub fn texto(&self) -> String {
        match self {
            FalhaPublicacao::Segredos(achados) => {
                let lugares: Vec<String> = achados.iter().map(Achado::descricao).collect();
                format!("Parece haver segredo nas mudanças: {}. Revise pelo Revisar e abrir PR.", lugares.join("; "))
            }
            FalhaPublicacao::Workflows => {
                "As mudanças mexem em .github/ (os workflows rodam com os segredos do repositório): publique você, pelo Revisar e atualizar PR.".into()
            }
            FalhaPublicacao::Conflito { alvo, arquivos } => {
                format!("Deu conflito com o que entrou em {alvo} (arquivos {}); nada foi enviado.", arquivos.join(", "))
            }
            FalhaPublicacao::Rejeitado(_) => "O ramo do PR mudou no GitHub (commits de fora): traga as mudanças do GitHub antes de enviar.".into(),
            FalhaPublicacao::SegredoRecusado(lugar) => format!(
                "O GitHub recusou o push porque achou um segredo: {lugar}. Tire o segredo do histórico da cópia antes de publicar."
            ),
            FalhaPublicacao::Incerto { .. } => "Resultado incerto: o Vitral está conferindo no GitHub.".into(),
            FalhaPublicacao::Remoto(erro) => erro.para_usuario("GitHub"),
            FalhaPublicacao::Git(erro) => sem_segredos(erro),
        }
    }
}

/// Publica: commit (sem os copiados), fetch da base, simulação de conflito, os segredos, o push e
/// o PR (edita o aberto, recupera o de uma queda entre o push e o create, ou cria). Nunca repete o
/// create sem antes o `pr_do_ramo`. `fila`: a vez do gh no provedor (o usuário ou o fundo).
pub async fn publicar(e: &dyn Executor, p: &Publicacao, fila: Option<Arc<smol::lock::Semaphore>>) -> Result<Publicado, FalhaPublicacao> {
    let git_erro = |erro: anyhow::Error| FalhaPublicacao::Git(format!("{erro:#}"));
    // Fetch, conferências, commit e push, na vez da rede do repositório (a do projeto, só para o
    // commit, por dentro dela: a ordem é sempre rede e depois projeto).
    let sha = {
        let _rede = git::na_vez_da_rede(&p.origem).await;
        git::buscar(e, &p.copia, &p.remoto, &[&p.base]).map_err(FalhaPublicacao::Remoto)?;
        let alvo = format!("{}/{}", p.remoto, p.base);
        // As conferências olham a cópia como está, antes do commit: o segredo barrado não fica no
        // histórico (apagado depois, ele iria no push seguinte do mesmo jeito).
        let desde = git::base_da_tarefa(&p.origem, &p.copia, "HEAD", Some(&alvo));
        let achados = git::segredos_no_diff(&p.copia, &desde, &p.excluir);
        if !achados.is_empty() && (p.automatico || !p.segredos_confirmados) {
            return Err(FalhaPublicacao::Segredos(achados));
        }
        if p.automatico {
            let desde_o_enviado = p.sha_publicado.as_deref().filter(|s| git::tem_commit(&p.copia, s)).unwrap_or(&desde);
            if git::mexe_em_workflows(&git::arquivos_mudados(&p.copia, desde_o_enviado)) {
                return Err(FalhaPublicacao::Workflows);
            }
        }
        // O commit, só local, na vez do projeto.
        git::na_vez_do_projeto(&p.origem, || git::commitar_sem_ganchos(&p.copia, &p.mensagem, &p.excluir)).map_err(git_erro)?;
        let conflitos = git::simular_mescla(&p.copia, &alvo).map_err(git_erro)?;
        if !conflitos.is_empty() {
            return Err(FalhaPublicacao::Conflito { alvo, arquivos: conflitos });
        }
        match git::empurrar(e, &p.copia, &p.remoto, &p.ramo) {
            Empurrao::Enviado(sha) => sha,
            Empurrao::Rejeitado(texto) => return Err(FalhaPublicacao::Rejeitado(texto)),
            Empurrao::SegredoRecusado(lugar) => return Err(FalhaPublicacao::SegredoRecusado(lugar)),
            Empurrao::Erro(erro) => return Err(FalhaPublicacao::Remoto(erro)),
        }
    };
    // 3. O PR, na fila do gh.
    let _vez = match fila {
        Some(fila) => Some(fila.acquire_arc().await),
        None => None,
    };
    let gh = Gh { e, pasta: &p.pasta_dados };
    if let Some(numero) = p.pr_aberto {
        let titulo = p.novo_titulo.then_some(p.titulo.as_str());
        let aviso = gh.editar_pr(&p.repositorio, numero, titulo, p.corpo.as_deref()).err().map(|erro| {
            format!("As mudanças foram enviadas, mas a descrição do PR não mudou ({}).", erro.para_usuario("GitHub"))
        });
        return Ok(Publicado {
            numero,
            url: p.url_aberto.clone(),
            titulo: p.titulo.clone(),
            sha,
            criado: false,
            rascunho: p.rascunho,
            aviso,
        });
    }
    // Uma queda entre o push e o create deixou o PR aberto: ele é recuperado, nunca duplicado.
    if let Some(achado) = gh.pr_do_ramo(&p.repositorio, &p.ramo, &p.dono_do_head, &sha).map_err(FalhaPublicacao::Remoto)? {
        return Ok(Publicado {
            numero: achado.numero,
            url: achado.url,
            titulo: achado.titulo,
            sha,
            criado: false,
            rascunho: achado.rascunho,
            aviso: None,
        });
    }
    let corpo = p.corpo.clone().unwrap_or_default();
    match gh.criar_pr(&p.repositorio, &p.cabeca, &p.base, &p.titulo, &corpo, p.rascunho) {
        Ok((numero, url)) => Ok(Publicado { numero, url, titulo: p.titulo.clone(), sha, criado: true, rascunho: p.rascunho, aviso: None }),
        Err(ErroRemoto::TempoEsgotado) => Err(FalhaPublicacao::Incerto { sha }),
        // O PR existe, mas o GitHub ainda mostrava o head de antes do push: a leitura estrita de
        // novo, algumas vezes, antes de desistir (nunca outro create).
        Err(ErroRemoto::Validacao(motivo)) if motivo.contains("already exists") => {
            for _ in 0..3 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                if let Some(achado) = gh.pr_do_ramo(&p.repositorio, &p.ramo, &p.dono_do_head, &sha).map_err(FalhaPublicacao::Remoto)? {
                    return Ok(Publicado { numero: achado.numero, url: achado.url, titulo: achado.titulo, sha, criado: false, rascunho: achado.rascunho, aviso: None });
                }
            }
            Err(FalhaPublicacao::Remoto(ErroRemoto::Validacao(motivo)))
        }
        Err(erro) => Err(FalhaPublicacao::Remoto(erro)),
    }
}

/// A conferência de um create que estourou o tempo: o PR do ramo, se ele foi criado.
pub fn conferir_criacao(e: &dyn Executor, pasta: &Path, repositorio: &str, ramo: &str, dono: &str, sha: &str) -> Result<Option<crate::github::PrAchado>, ErroRemoto> {
    Gh { e, pasta }.pr_do_ramo(repositorio, ramo, dono, sha)
}

/// Um PR do repositório para o vigia conferir: o que a tarefa tem e onde está a cópia (para saber
/// se um head é ancestral do que o Vitral enviou).
#[derive(Clone, Debug)]
pub struct Acompanhado {
    pub tarefa: u64,
    pub pr: Pr,
    pub copia: Option<PathBuf>,
}

/// O resultado da consulta de um PR: o PR novo e o que mudou, ou o erro só dele.
pub type Conferido = (u64, Result<(Pr, Vec<EventoPr>), ErroRemoto>);

/// Todos os PRs acompanhados de um repositório numa consulta (`resumo_prs`) e a transição de cada
/// um. Um PR que sumiu dá erro só nele.
pub fn conferir_prs(e: &dyn Executor, pasta: &Path, repo: &str, prs: &[Acompanhado], agora: u64) -> Result<(Vec<Conferido>, ResumoDoRepo), ErroRemoto> {
    let numeros: Vec<u64> = prs.iter().map(|a| a.pr.numero).collect();
    let mut resumo = Gh { e, pasta }.resumo_prs(repo, &numeros)?;
    let mut conferidos = Vec::new();
    for a in prs {
        let Some(i) = resumo.prs.iter().position(|(n, _)| *n == a.pr.numero) else {
            continue;
        };
        let (_, r) = resumo.prs.remove(i);
        let r = r.map(|r| {
            let publicado = a.pr.sha_publicado.clone();
            let conhecido = |sha: &str| sha == publicado || a.copia.as_deref().is_some_and(|c| git::e_ancestral(c, sha, &publicado));
            regras::transicao_pr_com(&a.pr, &r, agora, &conhecido)
        });
        conferidos.push((a.tarefa, r));
    }
    Ok((conferidos, ResumoDoRepo { prs: Vec::new(), ..resumo }))
}

/// A falha do CI lida no GitHub: a chave (sha + links), os checks que falharam e o texto do aviso
/// (`None` enquanto algum log ainda não saiu: a execução roda).
#[derive(Clone, Debug, PartialEq)]
pub struct FalhaLida {
    pub chave: String,
    pub falhos: Vec<Check>,
    pub texto: Option<String>,
}

/// Lê os checks e o fim do log de até dois que falharam (6000 caracteres no total). `so`: só
/// aquele check (o "Mandar ao chat" de uma linha). `sem_log`: segue sem o log (depois de 3
/// tentativas com a execução ainda rodando).
#[allow(clippy::too_many_arguments)]
pub fn ler_falha(
    e: &dyn Executor,
    pasta: &Path,
    pr: &Pr,
    so: Option<&str>,
    sem_log: bool,
    ship: bool,
) -> Result<FalhaLida, ErroRemoto> {
    let gh = Gh { e, pasta };
    let checks = gh.checks(&pr.repositorio, pr.numero)?;
    let chave = regras::chave_da_falha(&pr.sha, &checks);
    let falhos: Vec<Check> = regras::falhos(&checks).filter(|c| so.is_none_or(|l| c.link == l)).cloned().collect();
    let mut logs: Vec<Option<String>> = Vec::new();
    let mut rodando = false;
    for c in falhos.iter().take(2) {
        if sem_log {
            logs.push(None);
            continue;
        }
        match gh.log_da_falha(&pr.repositorio, &c.link) {
            Ok(Some(log)) => logs.push(Some(sem_segredos(&log))),
            Ok(None) => {
                rodando = true;
                logs.push(None);
            }
            // Sem o log (vencido, de um app sem saída): o aviso segue com os nomes.
            Err(erro) if !erro.transitorio() => logs.push(None),
            Err(erro) => return Err(erro),
        }
    }
    if rodando {
        return Ok(FalhaLida { chave, falhos, texto: None });
    }
    let itens: Vec<CheckFalho> = falhos
        .iter()
        .enumerate()
        .map(|(i, c)| CheckFalho { nome: &c.nome, workflow: &c.workflow, log: logs.get(i).and_then(|l| l.as_deref()) })
        .collect();
    let texto = sem_segredos(&prompts::ci_falhou(pr.numero, &pr.sha, &pr.checks, &itens, ship));
    Ok(FalhaLida { chave, falhos, texto: Some(texto) })
}

/// Os comentários do PR desde `desde` (ISO; o `atualizado` de antes menos 60 s), com a paginação.
pub fn ler_comentarios(e: &dyn Executor, pasta: &Path, pr: &Pr, desde: Option<&str>) -> Result<Vec<ComentarioPr>, ErroRemoto> {
    Gh { e, pasta }.comentarios_novos(&pr.repositorio, pr.numero, desde)
}

/// Mescla depois de conferir de novo: o resumo de agora (o `sha` e o `mergeStateStatus`) tem de
/// bater com o que se quer mesclar. Devolve o PR conferido e a mescla.
pub fn mesclar_conferindo(
    e: &dyn Executor,
    pasta: &Path,
    pr: &Pr,
    metodo: &str,
    sha: &str,
) -> Result<Mescla, ErroRemoto> {
    Gh { e, pasta }.mesclar(&pr.repositorio, pr.numero, metodo, sha)
}

/// O que fazer na cópia e na origem depois do PR mesclado.
#[derive(Clone, Debug)]
pub struct DepoisDaMescla {
    pub copia: Option<PathBuf>,
    pub origem: PathBuf,
    pub remoto: String,
    pub base: String,
    pub numero: u64,
    pub head_final: String,
    pub ramo: String,
    pub ramo_novo: String,
    pub excluir: Vec<String>,
    pub grupo: String,
    /// Apagar o ramo no GitHub (só quando foi o Vitral que mesclou, sem `deleteBranchOnMerge` e com o
    /// head no nosso repositório).
    pub apagar_ramo: bool,
    /// Mexer na cópia agora (o chat parado e a fila vazia); senão, o recomeço fica pendente.
    pub recomecar: bool,
}

/// O resultado do que vem depois de mesclar.
#[derive(Clone, Debug, Default)]
pub struct Recomecado {
    pub ramo_apagado: Option<Result<(), String>>,
    pub recomeco: Option<Result<git::Recomeco, String>>,
    pub origem_andou: bool,
}

/// Apaga o ramo remoto (com o lease no head final), recomeça a cópia a partir da base nova e, com a
/// origem limpa no ramo da base, anda com ela por fast-forward. A rede fica fora da vez do projeto.
pub async fn depois_da_mescla(e: &dyn Executor, d: &DepoisDaMescla) -> Recomecado {
    let mut feito = Recomecado::default();
    {
        let _rede = git::na_vez_da_rede(&d.origem).await;
        if d.apagar_ramo {
            let onde = d.copia.as_deref().filter(|c| c.is_dir()).unwrap_or(&d.origem);
            feito.ramo_apagado =
                Some(git::apagar_ramo_remoto(e, onde, &d.remoto, &d.ramo, &d.head_final).map_err(|erro| erro.para_usuario("GitHub")));
        }
        if let Some(copia) = d.copia.as_deref().filter(|c| d.recomecar && c.is_dir()) {
            feito.recomeco = Some(
                git::recomecar_depois_do_pr(e, copia, &d.remoto, &d.base, d.numero, &d.head_final, &d.ramo_novo, &d.excluir, &d.grupo)
                    .map_err(|erro| sem_segredos(&format!("{erro:#}"))),
            );
        } else if d.origem.is_dir() {
            // Sem cópia para recomeçar, a base vem para a origem andar.
            let _ = git::buscar(e, &d.origem, &d.remoto, &[&d.base]);
        }
    }
    if d.origem.is_dir() {
        feito.origem_andou = git::na_vez_do_projeto(&d.origem, || git::avancar_origem(&d.origem, &d.remoto, &d.base)).unwrap_or(false);
    }
    feito
}

#[allow(dead_code)] // Provisório: a aba PR (G5).
/// "Atualizar com main": traz a base para a cópia e envia (o `sha_publicado` vira o HEAD novo). No
/// conflito, nada é enviado e os arquivos voltam.
pub async fn atualizar_com_base(e: &dyn Executor, copia: &Path, origem: &Path, remoto: &str, base: &str, ramo: &str) -> Result<Option<String>, FalhaPublicacao> {
    let _rede = git::na_vez_da_rede(origem).await;
    git::buscar(e, copia, remoto, &[base]).map_err(FalhaPublicacao::Remoto)?;
    let alvo = format!("{remoto}/{base}");
    match git::na_vez_do_projeto(origem, || git::trazer(copia, &alvo)).map_err(|e| FalhaPublicacao::Git(format!("{e:#}")))? {
        Juncao::Conflito { arquivos, .. } => Err(FalhaPublicacao::Conflito { alvo, arquivos }),
        Juncao::SemMudancas => Ok(None),
        _ => match git::empurrar(e, copia, remoto, ramo) {
            Empurrao::Enviado(sha) => Ok(Some(sha)),
            Empurrao::Rejeitado(texto) => Err(FalhaPublicacao::Rejeitado(texto)),
            Empurrao::SegredoRecusado(lugar) => Err(FalhaPublicacao::SegredoRecusado(lugar)),
            Empurrao::Erro(erro) => Err(FalhaPublicacao::Remoto(erro)),
        },
    }
}

#[allow(dead_code)] // Provisório: a aba PR (G5).
/// "Trazer as mudanças do GitHub": o ramo do PR para a cópia (sem enviar nada). Devolve os commits
/// que vieram (os de fora).
pub async fn trazer_do_github(e: &dyn Executor, copia: &Path, origem: &Path, remoto: &str, ramo: &str) -> Result<Vec<String>, FalhaPublicacao> {
    let _rede = git::na_vez_da_rede(origem).await;
    git::buscar(e, copia, remoto, &[ramo]).map_err(FalhaPublicacao::Remoto)?;
    let alvo = format!("{remoto}/{ramo}");
    let antes = git::cabeca(copia).unwrap_or_default();
    match git::na_vez_do_projeto(origem, || git::trazer(copia, &alvo)).map_err(|e| FalhaPublicacao::Git(format!("{e:#}")))? {
        Juncao::Conflito { arquivos, .. } => Err(FalhaPublicacao::Conflito { alvo, arquivos }),
        _ => Ok(git::commits_entre(copia, &antes, &alvo)),
    }
}

#[cfg(test)]
pub mod testes {
    use super::*;
    use crate::{
        github::EstadoPr,
        remoto::{Ambiente, Real, SaidaBruta, falso},
    };

    /// O git de verdade (contra a origem bare local) e o gh falso, com as respostas na ordem.
    pub struct Misto {
        pub gh: falso::Falso,
    }

    impl Executor for Misto {
        fn rodar(
            &self,
            programa: &'static str,
            args: &[String],
            ambiente: &Ambiente,
            pasta: &Path,
            entrada: Option<&[u8]>,
            limite: std::time::Duration,
        ) -> Result<SaidaBruta, ErroRemoto> {
            if programa == "git" { Real.rodar(programa, args, ambiente, pasta, entrada, limite) } else { self.gh.rodar(programa, args, ambiente, pasta, entrada, limite) }
        }
    }

    pub fn git(pasta: &Path, args: &[&str]) -> String {
        let saida = git::novo_git().arg("-C").arg(pasta).args(["-c", "user.name=T", "-c", "user.email=t@t"]).args(args).output().unwrap();
        assert!(saida.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&saida.stderr));
        String::from_utf8_lossy(&saida.stdout).trim().to_string()
    }

    /// Uma origem bare fazendo de GitHub, a pasta do projeto (clone dela) e uma cópia do modo PR.
    pub struct Bancada {
        pub raiz: PathBuf,
        pub bare: PathBuf,
        pub projeto: PathBuf,
        pub copia: PathBuf,
    }

    impl Bancada {
        pub fn nova(ramo: &str) -> Self {
            let raiz = std::env::temp_dir().join(format!("vitral-g4-{}", crate::modelo::novo_id()));
            let (bare, projeto, copia) = (raiz.join("github.git"), raiz.join("projeto"), raiz.join("copia"));
            std::fs::create_dir_all(&projeto).unwrap();
            git(&projeto, &["init", "-q", "-b", "main"]);
            git(&projeto, &["config", "core.autocrlf", "false"]);
            std::fs::write(projeto.join("a.txt"), "um\n").unwrap();
            git(&projeto, &["add", "-A"]);
            git(&projeto, &["commit", "-q", "-m", "inicio"]);
            let texto = |p: &Path| p.to_string_lossy().to_string();
            git(&raiz, &["clone", "-q", "--bare", &texto(&projeto), &texto(&bare)]);
            git(&projeto, &["remote", "add", "origin", &texto(&bare)]);
            git(&projeto, &["fetch", "-q", "origin"]);
            let c = git::criar_worktree_de(&projeto, &copia, ramo, Some("origin/main")).unwrap();
            assert_eq!(c.caminho, copia);
            Self { raiz, bare, projeto, copia }
        }

        pub fn publicacao(&self, ramo: &str) -> Publicacao {
            Publicacao {
                pasta_dados: self.raiz.clone(),
                copia: self.copia.clone(),
                origem: self.projeto.clone(),
                remoto: "origin".into(),
                base: "main".into(),
                repositorio: "eita/app".into(),
                ramo: ramo.into(),
                cabeca: ramo.into(),
                dono_do_head: "eita".into(),
                mensagem: "Corrige o login".into(),
                titulo: "Corrige o login".into(),
                corpo: Some("## O que muda\nO login.\n".into()),
                ..Default::default()
            }
        }
    }

    impl Drop for Bancada {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.raiz);
        }
    }

    fn gh_args(e: &Misto) -> Vec<Vec<String>> {
        e.gh.chamadas().into_iter().map(|c| c.args).collect()
    }

    #[test]
    fn publicar_no_bare_local_com_o_gh_falso() {
        let ramo = "vitral/t13-login";
        let b = Bancada::nova(ramo);
        std::fs::write(b.copia.join("login.rs"), "fn login() {}\n").unwrap();
        // O .env copiado da origem nunca entra no commit.
        std::fs::write(b.copia.join(".env"), concat!("TOKEN=", "ghp_", "0123456789abcdefABCDEF0123456789abcd\n")).unwrap();
        let e = Misto {
            gh: falso::Falso::com(vec![
                falso::saida(0, "[]", ""),
                falso::saida(0, "https://github.com/eita/app/pull/12\n", ""),
            ]),
        };
        let p = Publicacao { excluir: vec![".env".into()], ..b.publicacao(ramo) };
        let feito = smol::block_on(publicar(&e, &p, None)).unwrap();
        assert_eq!((feito.numero, feito.criado), (12, true));
        assert_eq!(feito.sha, git(&b.bare, &["rev-parse", &format!("refs/heads/{ramo}")]));
        let no_github = git(&b.bare, &["ls-tree", "-r", "--name-only", &format!("refs/heads/{ramo}")]);
        assert!(no_github.contains("login.rs") && !no_github.contains(".env"), "{no_github}");
        let args = gh_args(&e);
        assert_eq!(args[0][..2], ["pr", "list"]);
        assert!(args[0].contains(&format!("--head={ramo}")) && args[0].contains(&"-R".to_string()));
        assert_eq!(args[1][..2], ["pr", "create"]);
        assert!(args[1].contains(&"--body-file".to_string()) && !args[1].contains(&"--draft".to_string()));
        assert_eq!(e.gh.chamadas()[1].entrada.as_deref(), Some(p.corpo.clone().unwrap().as_bytes()));
        // O upstream não foi gravado no .git/config compartilhado.
        let config = std::fs::read_to_string(b.projeto.join(".git/config")).unwrap();
        assert!(!config.contains(ramo), "{config}");

        // De novo, com o PR aberto: só edita, nunca cria outro.
        std::fs::write(b.copia.join("login.rs"), "fn login() { ok() }\n").unwrap();
        let e2 = Misto { gh: falso::Falso::com(vec![falso::saida(0, "", "")]) };
        let p2 = Publicacao { pr_aberto: Some(12), corpo: None, sha_publicado: Some(feito.sha.clone()), ..p };
        let outra = smol::block_on(publicar(&e2, &p2, None)).unwrap();
        assert_eq!((outra.numero, outra.criado), (12, false));
        assert_ne!(outra.sha, feito.sha);
        assert!(gh_args(&e2).is_empty(), "sem descrição nova o PR nem é editado: {:?}", gh_args(&e2));
    }

    #[test]
    fn create_que_estoura_o_tempo_e_recuperado_pelo_pr_do_ramo() {
        let ramo = "vitral/t14-x";
        let b = Bancada::nova(ramo);
        std::fs::write(b.copia.join("x.rs"), "x\n").unwrap();
        let e = Misto { gh: falso::Falso::com(vec![falso::saida(0, "[]", ""), Err(ErroRemoto::TempoEsgotado)]) };
        let p = b.publicacao(ramo);
        let Err(FalhaPublicacao::Incerto { sha }) = smol::block_on(publicar(&e, &p, None)) else {
            panic!("o create estourou o tempo: resultado incerto");
        };
        // A conferência seguinte acha o PR (estrito: OPEN, do dono e no sha enviado).
        let lista = format!(
            r#"[{{"number":7,"state":"OPEN","url":"https://github.com/eita/app/pull/7","title":"x","headRefOid":"{sha}","isDraft":false,"headRepositoryOwner":{{"login":"eita"}},"isCrossRepository":false}}]"#
        );
        let gh = falso::Falso::com(vec![falso::saida(0, &lista, "")]);
        let achado = conferir_criacao(&gh, &b.raiz, "eita/app", ramo, "eita", &sha).unwrap().unwrap();
        assert_eq!(achado.numero, 7);
        // Publicar de novo (o usuário apertou de novo, ou o Vitral reabriu): o pr_do_ramo acha o PR e
        // não há outro create.
        let e2 = Misto { gh: falso::Falso::com(vec![falso::saida(0, &lista, "")]) };
        let de_novo = smol::block_on(publicar(&e2, &p, None)).unwrap();
        assert_eq!((de_novo.numero, de_novo.criado), (7, false));
        assert!(gh_args(&e2).iter().all(|a| a[1] != "create"));
    }

    #[test]
    fn publicacao_barra_segredo_workflows_e_conflito() {
        let ramo = "vitral/t15-x";
        let b = Bancada::nova(ramo);
        std::fs::write(b.copia.join(".env.local"), "SENHA=1\n").unwrap();
        let p = b.publicacao(ramo);
        let e = Misto { gh: falso::Falso::default() };
        assert!(matches!(smol::block_on(publicar(&e, &p, None)), Err(FalhaPublicacao::Segredos(_))));
        assert!(git(&b.bare, &["for-each-ref", "refs/heads/vitral"]).is_empty(), "nada foi enviado");
        // Sozinho, nem com a confirmação.
        let sozinho = Publicacao { automatico: true, segredos_confirmados: true, ..p.clone() };
        assert!(matches!(smol::block_on(publicar(&e, &sozinho, None)), Err(FalhaPublicacao::Segredos(_))));
        // Barrado antes do commit: o .env não ficou no histórico da cópia.
        assert_eq!(git(&b.copia, &["rev-parse", "HEAD"]), git(&b.bare, &["rev-parse", "main"]));
        // .github/ sozinho, não.
        std::fs::remove_file(b.copia.join(".env.local")).unwrap();
        std::fs::create_dir_all(b.copia.join(".github/workflows")).unwrap();
        std::fs::write(b.copia.join(".github/workflows/ci.yml"), "on: push\n").unwrap();
        let workflows = Publicacao { automatico: true, ..p.clone() };
        assert_eq!(smol::block_on(publicar(&e, &workflows, None)), Err(FalhaPublicacao::Workflows));
        // Conflito com o que entrou na base: nada é enviado.
        let site = b.raiz.join("site");
        git(&b.raiz, &["clone", "-q", &b.bare.to_string_lossy(), &site.to_string_lossy()]);
        std::fs::write(site.join("a.txt"), "do site\n").unwrap();
        git(&site, &["commit", "-q", "-am", "site"]);
        git(&site, &["push", "-q", "origin", "main"]);
        std::fs::write(b.copia.join("a.txt"), "da cópia\n").unwrap();
        let Err(FalhaPublicacao::Conflito { alvo, arquivos }) = smol::block_on(publicar(&e, &p, None)) else {
            panic!("conflito");
        };
        assert_eq!((alvo.as_str(), arquivos), ("origin/main", vec!["a.txt".to_string()]));
        assert!(git(&b.bare, &["for-each-ref", "refs/heads/vitral"]).is_empty());
        assert!(e.gh.chamadas().is_empty(), "o gh nem foi chamado");
    }

    #[test]
    fn o_push_do_vitral_nao_roda_os_ganchos_da_copia() {
        let ramo = "vitral/t16-x";
        let b = Bancada::nova(ramo);
        // Um agente isolado escreve um pre-push no .git compartilhado.
        let marca = b.raiz.join("rodou");
        let gancho = b.projeto.join(".git/hooks/pre-push");
        std::fs::write(&gancho, format!("#!/bin/sh\ntouch '{}'\n", marca.display())).unwrap();
        std::fs::write(b.projeto.join(".git/hooks/pre-commit"), format!("#!/bin/sh\ntouch '{}'\n", marca.display())).unwrap();
        #[cfg(unix)]
        for nome in ["pre-push", "pre-commit"] {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(b.projeto.join(".git/hooks").join(nome), std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        std::fs::write(b.copia.join("x.rs"), "x\n").unwrap();
        let e = Misto { gh: falso::Falso::com(vec![falso::saida(0, "[]", ""), falso::saida(0, "https://github.com/eita/app/pull/3\n", "")]) };
        smol::block_on(publicar(&e, &b.publicacao(ramo), None)).unwrap();
        assert!(!marca.exists(), "o gancho rodou fora do isolamento");
    }

    /// O "Squash and merge" do GitHub no bare: um commit só no main com o conteúdo do PR, e o ref do PR.
    fn mesclar_no_github(b: &Bancada, numero: u64, ramo: &str) -> String {
        let site = b.raiz.join("site");
        if !site.exists() {
            git(&b.raiz, &["clone", "-q", "-c", "core.autocrlf=false", &b.bare.to_string_lossy(), &site.to_string_lossy()]);
        }
        git(&site, &["fetch", "-q", "origin"]);
        let head = git(&site, &["rev-parse", &format!("origin/{ramo}")]);
        git(&b.bare, &["update-ref", &format!("refs/pull/{numero}/head"), &head]);
        git(&site, &["checkout", "-q", "-B", "main", "origin/main"]);
        git(&site, &["merge", "-q", "--squash", &format!("origin/{ramo}")]);
        git(&site, &["commit", "-q", "-m", &format!("PR (#{numero})")]);
        git(&site, &["push", "-q", "origin", "main"]);
        head
    }

    fn publicar_um(b: &Bancada, ramo: &str, arquivo: &str, numero: u64) -> Publicado {
        std::fs::write(b.copia.join(arquivo), "novo\n").unwrap();
        let url = format!("https://github.com/eita/app/pull/{numero}\n");
        let e = Misto { gh: falso::Falso::com(vec![falso::saida(0, "[]", ""), falso::saida(0, &url, "")]) };
        smol::block_on(publicar(&e, &Publicacao { ramo: ramo.into(), cabeca: ramo.into(), ..b.publicacao(ramo) }, None)).unwrap()
    }

    #[test]
    fn depois_da_mescla_apaga_o_ramo_recomeca_a_copia_e_anda_com_a_origem() {
        let ramo = "vitral/t20-login";
        let b = Bancada::nova(ramo);
        let feito = publicar_um(&b, ramo, "login.rs", 5);
        let head = mesclar_no_github(&b, 5, ramo);
        assert_eq!(head, feito.sha);
        let d = DepoisDaMescla {
            copia: Some(b.copia.clone()),
            origem: b.projeto.clone(),
            remoto: "origin".into(),
            base: "main".into(),
            numero: 5,
            head_final: head.clone(),
            ramo: ramo.into(),
            ramo_novo: format!("{ramo}-r2"),
            excluir: Vec::new(),
            grupo: "t20".into(),
            apagar_ramo: true,
            recomecar: true,
        };
        let r = smol::block_on(depois_da_mescla(&Real, &d));
        assert_eq!(r.ramo_apagado, Some(Ok(())));
        assert!(git(&b.bare, &["for-each-ref", &format!("refs/heads/{ramo}")]).is_empty(), "o ramo saiu do GitHub");
        let Some(Ok(git::Recomeco::Feito { ramo: novo, .. })) = r.recomeco else { panic!("{:?}", r.recomeco) };
        assert_eq!(novo, format!("{ramo}-r2"));
        assert_eq!(git(&b.copia, &["rev-parse", "HEAD"]), git(&b.bare, &["rev-parse", "main"]));
        assert!(r.origem_andou, "a origem limpa no main anda por fast-forward");
        assert_eq!(git(&b.projeto, &["rev-parse", "HEAD"]), git(&b.bare, &["rev-parse", "main"]));
        // O PR seguinte não leva os commits do primeiro.
        let segundo = publicar_um(&b, &novo, "outro.rs", 6);
        let fora = git(&b.bare, &["log", "--format=%s", &format!("main..{}", segundo.sha)]);
        assert_eq!(fora.lines().count(), 1, "só o commit novo: {fora}");
        // De novo, nada muda (idempotente): o ramo já foi, e o recomeço só roda com o ramo novo livre.
        let r2 = smol::block_on(depois_da_mescla(&Real, &DepoisDaMescla { recomecar: false, ..d.clone() }));
        assert_eq!(r2.ramo_apagado, Some(Ok(())));
        assert!(r2.recomeco.is_none());
    }

    #[test]
    fn com_o_chat_rodando_a_copia_fica_e_o_resto_segue() {
        let ramo = "vitral/t21-x";
        let b = Bancada::nova(ramo);
        let feito = publicar_um(&b, ramo, "x.rs", 7);
        let head = mesclar_no_github(&b, 7, ramo);
        // O chat continua trabalhando: uma mudança sem commit na cópia.
        std::fs::write(b.copia.join("x.rs"), "mexendo\n").unwrap();
        let d = DepoisDaMescla {
            copia: Some(b.copia.clone()),
            origem: b.projeto.clone(),
            remoto: "origin".into(),
            base: "main".into(),
            numero: 7,
            head_final: head,
            ramo: ramo.into(),
            ramo_novo: format!("{ramo}-r2"),
            excluir: Vec::new(),
            grupo: "t21".into(),
            // Mesclado por outra pessoa: o ramo fica no GitHub.
            apagar_ramo: false,
            recomecar: false,
        };
        let r = smol::block_on(depois_da_mescla(&Real, &d));
        assert!(r.ramo_apagado.is_none() && r.recomeco.is_none());
        assert_eq!(git(&b.bare, &["rev-parse", &format!("refs/heads/{ramo}")]), feito.sha, "não apaga o ramo de quem mesclou");
        assert_eq!(git::ramo_atual(&b.copia).as_deref(), Some(ramo), "a cópia espera o fim do turno");
        assert_eq!(std::fs::read_to_string(b.copia.join("x.rs")).unwrap(), "mexendo\n");
        // No fim do turno, o recomeço pendente leva a mudança para o ramo novo.
        let r = smol::block_on(depois_da_mescla(&Real, &DepoisDaMescla { recomecar: true, ..d }));
        let Some(Ok(git::Recomeco::Feito { voltaram_mudancas, .. })) = r.recomeco else { panic!("{:?}", r.recomeco) };
        assert!(voltaram_mudancas);
        assert_eq!(std::fs::read_to_string(b.copia.join("x.rs")).unwrap(), "mexendo\n");
        assert!(git(&b.copia, &["stash", "list"]).is_empty(), "sem stash");
    }

    #[test]
    fn base_da_tarefa_depois_de_trazer_a_base_nao_mostra_o_trabalho_dos_outros() {
        let ramo = "vitral/t22-x";
        let b = Bancada::nova(ramo);
        std::fs::write(b.copia.join("meu.rs"), "meu\n").unwrap();
        git::commitar(&b.copia, "meu", &[]).unwrap();
        // Outra pessoa mesclou no main do GitHub; "Atualizar com main" traz para a cópia.
        let site = b.raiz.join("site");
        git(&b.raiz, &["clone", "-q", &b.bare.to_string_lossy(), &site.to_string_lossy()]);
        std::fs::write(site.join("deles.rs"), "deles\n").unwrap();
        git(&site, &["add", "-A"]);
        git(&site, &["commit", "-q", "-m", "deles"]);
        git(&site, &["push", "-q", "origin", "main"]);
        let e = Misto { gh: falso::Falso::default() };
        let enviado = smol::block_on(atualizar_com_base(&e, &b.copia, &b.projeto, "origin", "main", ramo)).unwrap();
        assert!(enviado.is_some(), "a base nova foi para o PR");
        let base_gravada = git(&b.projeto, &["rev-parse", "origin/main~1"]);
        let base = git::base_da_tarefa(&b.projeto, &b.copia, &base_gravada, Some("origin/main"));
        let mudados = git::arquivos_mudados(&b.copia, &base);
        assert_eq!(mudados, vec!["meu.rs".to_string()], "o diff é só o da tarefa");
        // Com a base de antes (sem o modo PR), o trabalho deles apareceria.
        assert!(git::arquivos_mudados(&b.copia, &base_gravada).contains(&"deles.rs".to_string()));
    }

    // ---------- aceite de escrita (manual) ----------

    /// O aceite de escrita do G4 (passos 1 a 8 da especificação), com o git e o gh de verdade no
    /// repositório de teste privado `VictorMenegate/vitral-teste` (nunca em outro). Escreve lá: abre,
    /// comenta, mescla e fecha PRs, cria uma issue e um commit pela API. O clone fica em
    /// `$VITRAL_TESTE` (padrão ~/.cache/vitral-trabalho/vitral-teste) e o registro em
    /// ~/.cache/vitral-trabalho/g4-notas/aceite-escrita.log.
    /// `cargo test -- --ignored integracao_aceite_de_escrita --nocapture` (uns 5 minutos: o Ship
    /// espera os 90 s dos bots).
    #[test]
    #[ignore]
    fn integracao_aceite_de_escrita_no_vitral_teste() {
        use crate::{github::{Gh, Mescla}, issues, modelo::agora};
        const REPO: &str = "VictorMenegate/vitral-teste";
        let casa = dirs::home_dir().unwrap();
        let clone = std::env::var_os("VITRAL_TESTE").map(PathBuf::from).unwrap_or_else(|| casa.join(".cache/vitral-trabalho/vitral-teste"));
        let notas = casa.join(".cache/vitral-trabalho/g4-notas");
        let pasta = notas.join("pasta-do-gh");
        std::fs::create_dir_all(&pasta).unwrap();
        let log = std::sync::Mutex::new(std::fs::OpenOptions::new().create(true).append(true).open(notas.join("aceite-escrita.log")).unwrap());
        let anotar = |texto: &str| {
            use std::io::Write as _;
            let linha = format!("{} {texto}\n", agora());
            print!("{linha}");
            let _ = log.lock().unwrap().write_all(linha.as_bytes());
        };
        let e = Real;
        let gh = Gh { e: &e, pasta: &pasta };
        let cru = |args: &[&str], entrada: Option<&str>| -> String {
            let args: Vec<String> = args.iter().map(|a| a.to_string()).collect();
            let ambiente = Ambiente { definir: vec![("GH_PROMPT_DISABLED".into(), "1".into()), ("NO_COLOR".into(), "1".into())], ..Default::default() };
            let s = e.rodar("gh", &args, &ambiente, &pasta, entrada.map(str::as_bytes), std::time::Duration::from_secs(60)).unwrap();
            assert_eq!(s.codigo, Some(0), "gh {args:?}: {}", s.erros());
            s.texto().trim().to_string()
        };
        anotar(&format!("--- aceite de escrita do G4 em {REPO}"));
        let info = gh.info_repo(REPO).unwrap();
        assert!(info.pode_escrever() && !info.apaga_ramo_ao_mesclar);
        assert_eq!(gh.workflows(REPO).unwrap(), 0, "sem CI");
        git(&clone, &["fetch", "-q", "origin"]);
        git(&clone, &["checkout", "-q", "main"]);
        git(&clone, &["merge", "-q", "--ff-only", "origin/main"]);

        // Uma issue para o Closes.
        let carimbo = agora();
        let url_issue = cru(&["issue", "create", "-R", REPO, "--title", &format!("Aceite G4 {carimbo}: o PR fecha esta issue"), "--body", "Criada pelo teste de aceite do G4."], None);
        let n_issue: u64 = url_issue.rsplit('/').next().unwrap().parse().unwrap();
        anotar(&format!("issue #{n_issue} criada ({url_issue})"));
        let ligada = issues::IssueLigada {
            fonte: issues::FonteIssue::Github,
            id: n_issue.to_string(),
            chave: format!("#{n_issue}"),
            titulo: "o PR fecha esta issue".into(),
            repositorio: Some(REPO.into()),
            ..Default::default()
        };

        // 1. Revisar e abrir PR: título, descrição e Closes.
        let ramo = format!("vitral/t9{}-aceite", carimbo % 1000);
        let copia = notas.join(format!("copia-{carimbo}"));
        git::criar_worktree_de(&clone, &copia, &ramo, Some("origin/main")).unwrap();
        std::fs::write(copia.join("aceite-1.txt"), format!("primeiro PR {carimbo}\n")).unwrap();
        // O .env copiado da origem nunca entra no commit.
        std::fs::write(copia.join(".env"), "TESTE=1\n").unwrap();
        let titulo = issues::titulo_do_pr("Aceite do G4", Some(&ligada));
        let corpo = issues::corpo_do_pr(&issues::DadosDoCorpo { resumo: "O primeiro PR do aceite.", como_verificou: "o teste confere", verificacao: None, issue: Some(&ligada) });
        let plano = |ramo: &str, titulo: &str, corpo: Option<String>, aberto: Option<u64>| Publicacao {
            pasta_dados: pasta.clone(),
            copia: copia.clone(),
            origem: clone.clone(),
            remoto: "origin".into(),
            base: "main".into(),
            repositorio: REPO.into(),
            ramo: ramo.into(),
            cabeca: ramo.into(),
            dono_do_head: "VictorMenegate".into(),
            mensagem: titulo.into(),
            excluir: vec![".env".into()],
            titulo: titulo.into(),
            corpo,
            pr_aberto: aberto,
            ..Default::default()
        };
        let p1 = smol::block_on(publicar(&e, &plano(&ramo, &titulo, Some(corpo), None), None)).unwrap();
        assert!(p1.criado);
        anotar(&format!("1. PR #{} aberto ({}), título {titulo:?}", p1.numero, p1.url));
        let detalhe = gh.detalhe_pr(REPO, p1.numero).unwrap();
        assert!(detalhe.corpo.contains(&format!("Closes #{n_issue}")) && detalhe.titulo == titulo);
        assert!(!detalhe.arquivos.iter().any(|a| a.caminho == ".env"), "o .env não foi");
        let novo_pr = |p: &Publicado, ramo: &str| Pr {
            repositorio: REPO.into(),
            numero: p.numero,
            url: p.url.clone(),
            titulo: p.titulo.clone(),
            base: "main".into(),
            ramo: ramo.into(),
            cabeca: ramo.into(),
            dono_do_head: "VictorMenegate".into(),
            sha: p.sha.clone(),
            sha_publicado: p.sha.clone(),
            acompanhando: true,
            empurrado_em: agora(),
            ..Default::default()
        };
        let conferir = |pr: &Pr| -> (Pr, Vec<EventoPr>) {
            let a = [Acompanhado { tarefa: 1, pr: pr.clone(), copia: Some(copia.clone()) }];
            let (mut lista, _) = conferir_prs(&e, &pasta, REPO, &a, agora()).unwrap();
            lista.pop().unwrap().1.unwrap()
        };
        let (mut pr1, eventos) = conferir(&novo_pr(&p1, &ramo));
        anotar(&format!("   vigia: estado {:?}, mergeStateStatus {}, eventos {eventos:?}", pr1.estado, pr1.mesclavel));
        assert_eq!(pr1.estado, crate::github::EstadoPr::Aberto);

        // 2. Comentário do dono pelo site chega como revisão; o do Vitral não.
        cru(&["pr", "comment", &p1.numero.to_string(), "-R", REPO, "--body", "Comentário do dono: troque o texto do arquivo."], None);
        cru(&["pr", "comment", &p1.numero.to_string(), "-R", REPO, "--body", "Comentário do Vitral (marcado como nosso)."], None);
        let todos = ler_comentarios(&e, &pasta, &pr1, None).unwrap();
        let nosso = todos.iter().find(|c| c.corpo.contains("do Vitral")).unwrap().id;
        pr1.comentarios_nossos.push(nosso);
        std::thread::sleep(std::time::Duration::from_secs(3));
        let antes = pr1.clone();
        let (depois, eventos) = conferir(&pr1);
        pr1 = depois;
        assert!(eventos.contains(&EventoPr::Novidade), "{eventos:?}");
        let desde = (!antes.atualizado.is_empty()).then(|| regras::iso_menos(&antes.atualizado, 60)).flatten();
        let s = regras::separar_comentarios(&pr1, ler_comentarios(&e, &pasta, &pr1, desde.as_deref()).unwrap());
        assert_eq!(s.confiaveis.len(), 1, "{s:?}");
        assert!(s.confiaveis[0].corpo.contains("do dono") && s.confiaveis[0].associacao == "OWNER");
        assert!(s.confiaveis.iter().chain(&s.terceiros).all(|c| c.id != nosso));
        regras::marcar_entregues(&mut pr1, &s.vistos);
        let aviso = prompts::revisao_do_pr(pr1.numero, &s.confiaveis, &s.mudancas_pedidas_por);
        anotar(&format!("2. revisão no PR #{}: {} comentário do dono (OWNER), o do Vitral ficou de fora; aviso: {:?}", pr1.numero, s.confiaveis.len(), aviso.lines().next()));

        // 3. Mesclar (squash, pelo Vitral): o ramo remoto sai, a cópia recomeça de origin/main e a
        // origem limpa anda.
        let condicoes = regras::Condicoes { sem_ci: true, ..Default::default() };
        let espera = regras::ESPERA_DOS_BOTS.saturating_sub(agora().saturating_sub(pr1.empurrado_em));
        std::thread::sleep(std::time::Duration::from_secs(espera + 2));
        let (conferido, _) = conferir(&pr1);
        pr1 = conferido;
        regras::pronto_para_mesclar(&pr1, agora(), &condicoes).unwrap();
        assert_eq!(mesclar_conferindo(&e, &pasta, &pr1, "squash", &pr1.sha_publicado).unwrap(), Mescla::Mesclado);
        pr1.metodo_mesclado = Some("squash".into());
        let (mesclado, eventos) = conferir(&pr1);
        assert!(matches!(eventos.as_slice(), [EventoPr::Mesclado { .. }]), "{eventos:?}");
        let mut t = crate::modelo::Tarefa { id: 9, rodada: 1, pr: Some(mesclado.clone()), estado: crate::modelo::EstadoTarefa::NoPr, ..Default::default() };
        let saiu = regras::registrar_mescla(&mut t, agora()).unwrap();
        assert!(super::super::apagar_ramo_depois(t.prs_anteriores.last().unwrap(), &saiu, Some(&info)));
        let ramo2 = regras::ramo_depois_do_pr(&ramo, &ramo, t.rodada, &|_| false);
        let d = DepoisDaMescla {
            copia: Some(copia.clone()),
            origem: clone.clone(),
            remoto: "origin".into(),
            base: "main".into(),
            numero: saiu.numero,
            head_final: saiu.sha.clone(),
            ramo: ramo.clone(),
            ramo_novo: ramo2.clone(),
            excluir: vec![".env".into()],
            grupo: "t9".into(),
            apagar_ramo: true,
            recomecar: true,
        };
        let r = smol::block_on(depois_da_mescla(&e, &d));
        assert_eq!(r.ramo_apagado, Some(Ok(())));
        assert!(matches!(r.recomeco, Some(Ok(git::Recomeco::Feito { .. }))), "{:?}", r.recomeco);
        assert!(r.origem_andou);
        assert!(cru(&["api", &format!("repos/{REPO}/branches?per_page=100"), "--jq", ".[].name"], None).lines().all(|l| l != ramo), "o ramo saiu do GitHub");
        assert_eq!(git(&copia, &["rev-parse", "HEAD"]), git(&clone, &["rev-parse", "origin/main"]));
        assert_eq!(git(&clone, &["rev-parse", "HEAD"]), git(&clone, &["rev-parse", "origin/main"]));
        let mut issue = String::new();
        for _ in 0..10 {
            issue = cru(&["issue", "view", &n_issue.to_string(), "-R", REPO, "--json", "state", "--jq", ".state"], None);
            if issue == "CLOSED" {
                break;
            }
            std::thread::sleep(std::time::Duration::from_secs(2));
        }
        anotar(&format!("3. PR #{} mesclado (squash) pelo Vitral; ramo {ramo} apagado; cópia em {ramo2} a partir de origin/main; origem andou; issue #{n_issue} {issue}", saiu.numero));
        assert_eq!(issue, "CLOSED");

        // 4. O PR seguinte não leva os commits do primeiro.
        std::fs::write(copia.join("aceite-2.txt"), format!("segundo PR {carimbo}\n")).unwrap();
        let p2 = smol::block_on(publicar(&e, &plano(&ramo2, "Aceite do G4: segundo PR", Some("## O que muda\nO segundo.\n".into()), None), None)).unwrap();
        let commits = gh.detalhe_pr(REPO, p2.numero).unwrap().commits;
        assert_eq!(commits.len(), 1, "{commits:?}");
        anotar(&format!("4. PR #{} aberto com {} commit (só o novo)", p2.numero, commits.len()));

        // 5. Com o Ship num PR sem CI, ele mescla sozinho depois de 90 s.
        let mut pr2 = Pr { ao_ficar_pronto: Some("squash".into()), ..novo_pr(&p2, &ramo2) };
        let sozinho = regras::Condicoes { automatico: true, sem_ci: true, ..Default::default() };
        let comeco = agora();
        loop {
            let (conferido, _) = conferir(&pr2);
            pr2 = conferido;
            if regras::pronto_para_mesclar(&pr2, agora(), &sozinho).is_ok() {
                break;
            }
            assert!(agora() - comeco < 240, "não ficou pronto: {:?}", regras::pronto_para_mesclar(&pr2, agora(), &sozinho));
            std::thread::sleep(std::time::Duration::from_secs(10));
        }
        let esperou = agora() - pr2.empurrado_em;
        assert!(esperou >= regras::ESPERA_DOS_BOTS);
        assert_eq!(mesclar_conferindo(&e, &pasta, &pr2, "squash", &pr2.sha_publicado).unwrap(), Mescla::Mesclado);
        pr2.metodo_mesclado = Some("squash".into());
        let (mesclado2, _) = conferir(&pr2);
        assert_eq!(mesclado2.estado, crate::github::EstadoPr::Mesclado);
        let ramo3 = regras::ramo_depois_do_pr(&ramo, &ramo2, 3, &|r| r == ramo2 || r == ramo);
        let r = smol::block_on(depois_da_mescla(&e, &DepoisDaMescla { numero: p2.numero, head_final: mesclado2.sha.clone(), ramo: ramo2.clone(), ramo_novo: ramo3.clone(), ..d.clone() }));
        assert!(matches!(r.recomeco, Some(Ok(git::Recomeco::Feito { .. }))));
        anotar(&format!("5. Ship: PR #{} mesclado sozinho {esperou} s depois do envio (sem CI); cópia em {ramo3}", p2.numero));

        // 6. Um .env de teste criado na cópia barra a publicação.
        std::fs::write(copia.join(".env.teste"), "SENHA=teste\n").unwrap();
        let barrado = smol::block_on(publicar(&e, &plano(&ramo3, "Aceite do G4: com segredo", Some("x".into()), None), None));
        assert!(matches!(barrado, Err(FalhaPublicacao::Segredos(_))), "{barrado:?}");
        assert!(cru(&["api", &format!("repos/{REPO}/branches?per_page=100"), "--jq", ".[].name"], None).lines().all(|l| l != ramo3), "nada foi enviado");
        assert_eq!(git(&copia, &["rev-parse", "HEAD"]), git(&clone, &["rev-parse", "origin/main"]), "nem o commit ficou");
        std::fs::remove_file(copia.join(".env.teste")).unwrap();
        anotar(&format!("6. .env.teste na cópia barrou a publicação ({})", barrado.unwrap_err().texto()));

        // 8. Fechar o Vitral no meio e reabrir: o PR é recuperado pelo pr_do_ramo, sem duplicar.
        std::fs::write(copia.join("aceite-3.txt"), format!("terceiro PR {carimbo}\n")).unwrap();
        let p3 = smol::block_on(publicar(&e, &plano(&ramo3, "Aceite do G4: terceiro PR", Some("## O que muda\nO terceiro.\n".into()), None), None)).unwrap();
        std::fs::write(copia.join("aceite-3.txt"), format!("terceiro PR, mais {carimbo}\n")).unwrap();
        // O estado gravado se perdeu (o Vitral caiu entre o push e o create): publicar de novo acha o PR.
        let de_novo = smol::block_on(publicar(&e, &plano(&ramo3, "Aceite do G4: terceiro PR", Some("x".into()), None), None)).unwrap();
        assert_eq!((de_novo.numero, de_novo.criado), (p3.numero, false));
        let do_ramo = cru(&["pr", "list", "-R", REPO, "--head", &ramo3, "--state", "all", "--json", "number", "--jq", "length"], None);
        assert_eq!(do_ramo, "1");
        anotar(&format!("8. reaberto: o PR #{} foi recuperado pelo pr_do_ramo (um PR só no ramo {ramo3})", p3.numero));

        // 7. Um commit pelo site no ramo do PR desarma o Ship e vira "commit de fora".
        let mut pr3 = Pr { ao_ficar_pronto: Some("squash".into()), ..novo_pr(&de_novo, &ramo3) };
        let (conferido, _) = conferir(&pr3);
        pr3 = conferido;
        let conteudo = "ZG8gc2l0ZQo="; // "do site\n"
        cru(&["api", "-X", "PUT", &format!("repos/{REPO}/contents/site-{carimbo}.txt"), "-f", "message=commit pelo site", "-f", &format!("content={conteudo}"), "-f", &format!("branch={ramo3}")], None);
        std::thread::sleep(std::time::Duration::from_secs(3));
        let (conferido, eventos) = conferir(&pr3);
        pr3 = conferido;
        assert!(eventos.iter().any(|e| matches!(e, EventoPr::CommitDeFora(_))), "{eventos:?}");
        assert!(pr3.tem_commit_de_fora());
        assert!(regras::pronto_para_mesclar(&pr3, agora() + 1000, &sozinho).is_err(), "o Vitral não mescla sozinho");
        anotar(&format!("7. commit pelo site no PR #{}: commit de fora {:?}; o Ship desarma e o Vitral não mescla sozinho", pr3.numero, pr3.commits_de_fora));

        // E o PR fica fechado (6.9), com o comentário do arquivamento.
        gh.fechar_pr(REPO, pr3.numero, Some("Fechado pelo Vitral: aceite do G4.")).unwrap();
        let (_, eventos) = conferir(&pr3);
        assert_eq!(eventos, vec![EventoPr::Fechado]);
        anotar(&format!("   PR #{} fechado sem mesclar (o vigia viu: {eventos:?})", pr3.numero));
        let _ = git::na_vez_do_projeto(&clone, || git::remover_worktree(&clone, &copia, &ramo3));
        anotar("--- fim do aceite: tudo passou");
    }

    #[test]
    fn conferir_prs_com_o_executor_falso() {
        let amostra = include_str!("../../../tests/amostras/github/resumo_prs.json");
        let gh = falso::Falso::com(vec![falso::saida(1, amostra, "")]);
        let pr = |n: u64, sha: &str| Pr { repositorio: "cli/cli".into(), numero: n, sha: sha.into(), sha_publicado: sha.into(), acompanhando: true, ..Default::default() };
        let prs = [
            Acompanhado { tarefa: 1, pr: pr(14845, "04c4ebf52e7033a83eb38e9741c8ace05bf8617b"), copia: None },
            Acompanhado { tarefa: 2, pr: pr(14783, "0000000000000000000000000000000000000000"), copia: None },
            Acompanhado { tarefa: 3, pr: pr(999999, ""), copia: None },
        ];
        let (conferidos, resumo) = conferir_prs(&gh, Path::new("/"), "cli/cli", &prs, 5000).unwrap();
        assert_eq!(resumo.restantes, Some(4990));
        assert_eq!(conferidos.len(), 3);
        let (t, Ok((novo, eventos))) = &conferidos[0] else { panic!() };
        assert_eq!(*t, 1);
        assert_eq!((novo.estado, novo.mesclavel.as_str(), novo.checks.total), (EstadoPr::Aberto, "BLOCKED", 11));
        assert!(eventos.contains(&EventoPr::ChecksPassaram), "{eventos:?}");
        assert!(!eventos.iter().any(|e| matches!(e, EventoPr::CommitDeFora(_))));
        // O segundo está noutro head: commit de fora.
        let (_, Ok((_, eventos))) = &conferidos[1] else { panic!() };
        assert!(eventos.contains(&EventoPr::CommitDeFora("654d851d8145b2912fe474ceb0a47681854b37e2".into())), "{eventos:?}");
        // O que não existe dá erro só nele.
        assert!(matches!(conferidos[2], (3, Err(ErroRemoto::NaoEncontrado(_)))));
        let chamada = &gh.chamadas()[0];
        assert_eq!(chamada.args[..2], ["api", "graphql"]);
        assert!(String::from_utf8_lossy(chamada.entrada.as_deref().unwrap()).contains("p14783: pullRequest(number:14783)"));
    }

    /// O resumo real (gravado em 2026-11-01, só leitura) de dois PRs do jcm-climatizacao que o colega
    /// mesclou, com os nomes trocados: o dono, o repositório, os ramos e os shas (o repositório é
    /// privado e este é público). A forma, os nulos e o `UNKNOWN` são os de verdade.
    pub const MESCLADO_POR_TERCEIRO: &str = include_str!("../../../tests/amostras/github/resumo_mesclado_por_terceiro.json");

    #[test]
    fn mesclado_por_terceiro_da_amostra_real_vai_para_conversa_sem_apagar_o_ramo() {
        let gh = falso::Falso::com(vec![falso::saida(0, MESCLADO_POR_TERCEIRO, "")]);
        let aberto = Pr {
            repositorio: "eita/app".into(),
            numero: 180,
            ramo: "feature/vitor-e2e".into(),
            sha: "a29dd105f70541333267608011df4e7c82369846".into(),
            sha_publicado: "a29dd105f70541333267608011df4e7c82369846".into(),
            acompanhando: true,
            ..Default::default()
        };
        let prs = [Acompanhado { tarefa: 13, pr: aberto.clone(), copia: None }];
        let (conferidos, _) = conferir_prs(&gh, Path::new("/"), "eita/app", &prs, 9000).unwrap();
        let (_, Ok((novo, eventos))) = &conferidos[0] else { panic!("{conferidos:?}") };
        assert_eq!(eventos, &vec![EventoPr::Mesclado { por: Some("colega-do-time".into()), commit: Some("db08edc2d39b5746981e997fb947d1ce9d84de01".into()) }]);
        assert_eq!(novo.metodo_mesclado, None, "não foi o Vitral");
        let mut t = crate::modelo::Tarefa {
            id: 13,
            estado: crate::modelo::EstadoTarefa::NoPr,
            rodada: 2,
            pr: Some(novo.clone()),
            ..Default::default()
        };
        regras::registrar_mescla(&mut t, 9000).unwrap();
        assert_eq!(t.estado, crate::modelo::EstadoTarefa::Conversa);
        assert_eq!(t.prs_anteriores[0].mesclado_por.as_deref(), Some("colega-do-time"));
        assert!(t.prs_anteriores[0].metodo.is_none());
        // Quem decide apagar o ramo é `apagar_ramo_depois` (motor): só quando o Vitral mesclou.
        assert!(!super::super::apagar_ramo_depois(&t.prs_anteriores[0], &aberto, None));
    }

    /// Os checks reais do #180 do jcm (gravados em 2026-11-01, só leitura): 3 jobs do Actions em
    /// FAILURE e o Workers Builds (app de fora) passando; os nomes e os links trocados.
    pub const CHECKS_DO_180: &str = include_str!("../../../tests/amostras/github/checks_ci_falhou.json");

    #[test]
    fn falha_do_ci_da_amostra_com_o_log_de_dois_checks() {
        let log = "2026-10-25T13:00:00.0000000Z ##[group]Run npm test\n2026-10-25T13:00:01.0000000Z FAIL src/a.test.ts\n2026-10-25T13:00:02.0000000Z ##[error]Process completed with exit code 1.\n";
        let gh = falso::Falso::com(vec![
            falso::saida(0, CHECKS_DO_180, ""),
            falso::saida(0, log, ""),
            falso::saida(1, "", "run 40197232117 is still in progress; logs will be available when it is complete"),
        ]);
        let pr = Pr {
            repositorio: "eita/app".into(),
            numero: 180,
            sha: "a29dd105f70541333267608011df4e7c82369846".into(),
            checks: crate::github::ResumoChecks { estado: crate::github::EstadoChecks::Falhou, total: 4, terminados: 4, falhos: 3, pendentes: 0 },
            ..Default::default()
        };
        let lida = ler_falha(&gh, Path::new("/"), &pr, None, false, false).unwrap();
        assert_eq!(lida.falhos.len(), 3);
        assert!(lida.texto.is_none(), "um log ainda não saiu: tenta na próxima consulta");
        assert!(lida.chave.starts_with("a29dd105f70541333267608011df4e7c82369846 https://github.com/eita/app/actions/runs/"));
        // Depois de 3 tentativas, segue sem o log.
        let gh = falso::Falso::com(vec![falso::saida(0, CHECKS_DO_180, "")]);
        let sem_log = ler_falha(&gh, Path::new("/"), &pr, None, true, true).unwrap();
        let texto = sem_log.texto.unwrap();
        assert!(texto.starts_with("O CI do PR #180 falhou no commit a29dd10 (3 de 4 checks)."), "{texto}");
        assert!(texto.contains("\"lint-and-test\" (CI)") && !texto.contains("Workers Builds"));
        assert!(texto.ends_with("mescla quando ficar pronto."));
        assert_eq!(gh.chamadas().len(), 1, "sem log, nenhum gh run view");
        // A chave é a mesma: o aviso não se repete.
        assert_eq!(sem_log.chave, lida.chave);
    }
}
