//! A versão embutida no binário, os comandos que respondem sem abrir janela e a atualização
//! automática (especificação em `distribuicao.md`, seção 3): a procura numa thread própria
//! (`procura.rs`), o manifesto assinado (`manifesto.rs`, `assinatura.rs`), o download (`rede.rs`),
//! o pacote (`pacote.rs`), a instalação por versão no Linux (`linux.rs`), o
//! `atualizacoes.json` (`config.rs`) e a saúde da versão nova, com a volta para a anterior
//! (`saude.rs`).
//!
//! A interface (`ui/atualizacao.rs`) lê a `Global` [`Atualizacoes`] e redesenha com
//! `cx.observe_global::<Atualizacoes>(…)`, que só dispara quando o estado muda. As ações são
//! [`procurar_agora`], [`instalar_agora`], [`escolher_modo`], [`acompanhar_progresso`],
//! [`voltar_a_mao`] e [`reiniciar`] (depois do `pode_parar_tudo`). Nunca reinicia sozinho.
//!
//! De onde veio o binário vem da compilação: `VITRAL_CANAL` ("estavel" só no release do GitHub
//! Actions; sem ele, compilado no PC), `VITRAL_COMMIT` e, no `ferramentas/instalar-dev.sh`,
//! `VITRAL_VERSAO` (a versão de desenvolvimento no lugar da do Cargo.toml).

pub mod assinatura;
pub mod config;
pub mod linux;
pub mod manifesto;
pub mod pacote;
pub mod procura;
pub mod rede;
pub mod saude;

use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

use gpui_kit::{App, AppContext as _, BorrowAppContext as _, Global, Subscription, Task};
use serde_json::json;

use crate::{dados, modelo::FORMATO_DADOS};
use config::Modo;

pub const REPO: &str = "VictorMenegate/vitral";
/// As novidades de todas as versões; a desta vai no binário.
const NOVIDADES: &str = include_str!("../../NOVIDADES.md");
/// A versão nova fica confirmada 10 s depois do primeiro quadro (ou ao fechar normalmente).
const CONFIRMA_DEPOIS: Duration = Duration::from_secs(10);

/// A versão deste binário (a do Cargo.toml, ou a de desenvolvimento do `instalar-dev.sh`).
pub const VERSAO: &str = match option_env!("VITRAL_VERSAO") {
    Some(versao) if !versao.is_empty() => versao,
    _ => env!("CARGO_PKG_VERSION"),
};

/// "estavel" no que sai do release; "pc" no que foi compilado no PC.
pub fn canal() -> &'static str {
    match option_env!("VITRAL_CANAL") {
        Some(canal) if !canal.is_empty() => canal,
        _ => "pc",
    }
}

/// O commit de onde o binário saiu (vazio no compilado no PC).
pub fn commit() -> &'static str {
    option_env!("VITRAL_COMMIT").unwrap_or("")
}

/// "linux-x86_64", o mesmo sufixo dos pacotes do release.
pub fn plataforma() -> String {
    format!("{}-{}", std::env::consts::OS, std::env::consts::ARCH)
}

/// O que `--versao --json` responde. O atualizador roda o binário novo com isso antes de trocar de
/// versão: um binário zerado, sem biblioteca ou de outro pacote não responde igual.
pub fn json() -> String {
    json!({
        "versao": VERSAO,
        "formato_dados": FORMATO_DADOS,
        "canal": canal(),
        "commit": commit(),
        "plataforma": plataforma(),
    })
    .to_string()
}

/// Os comandos que respondem sem janela, antes do GPUI (servem sem tela: no runner do GitHub e no
/// atualizador). Devolve o código de saída quando o argumento é um deles.
pub fn sem_janela(argumentos: &[String]) -> Option<i32> {
    let tem = |nome: &str| argumentos.iter().any(|a| a == nome);
    if tem("--versao") || tem("--version") {
        if tem("--json") {
            println!("{}", json());
        } else {
            println!("vitral {VERSAO}");
        }
        return Some(0);
    }
    if tem("--instalar") {
        // Com os dados desligados, o Vitral só abre na bancada: o menu abriria um que sai na hora.
        if !crate::dados::DADOS_GLOBAIS {
            eprintln!("Não instalei o Vitral: este build tem os dados globais desligados (DADOS_GLOBAIS) e só abre na bancada.");
            return Some(1);
        }
        return Some(linux::instalar_por_comando());
    }
    None
}

/// O rodapé do menu do logo.
pub fn rotulo() -> String {
    format!("Vitral {VERSAO}")
}

/// A dica do rodapé: de onde veio este binário.
pub fn origem() -> String {
    origem_de(canal(), commit())
}

fn origem_de(canal: &str, commit: &str) -> String {
    let curto = commit.get(..7).unwrap_or(commit);
    match (canal == "pc", curto.is_empty()) {
        (true, true) => "Compilado no PC".to_string(),
        (true, false) => format!("Compilado no PC · commit {curto}"),
        (false, true) => format!("Canal {canal}"),
        (false, false) => format!("Commit {curto}"),
    }
}

/// Em que pé está a atualização.
#[derive(Debug, Clone, PartialEq)]
pub enum Estado {
    /// Ainda não procurou nesta abertura.
    Parado,
    Procurando,
    /// Procurou e esta é a mais nova.
    EmDia,
    /// Há versão nova, mas esta cópia só avisa: Só avisar, compilada no PC, fora da pasta do app,
    /// ou manifesto de formato novo (`pela_pagina`).
    Disponivel(Novidade),
    Baixando { versao: String, feito: u64, total: u64 },
    Instalando { versao: String },
    /// Instalada; vale quando reiniciar.
    Pronta(Novidade),
    Erro(Erro),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Novidade {
    pub versao: String,
    /// O `notas` do manifesto, em Markdown (vazio quando não se sabe).
    pub notas: String,
    pub pagina: String,
    /// Esta cópia não sabe instalar essa versão: só pela página.
    pub pela_pagina: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Erro {
    /// Sem conexão, prazo, servidor fora: tenta de novo em 1 h.
    Rede(String),
    /// A assinatura do manifesto não confere (ou não veio).
    Assinatura,
    /// O manifesto assinado não passa na validação.
    Manifesto(String),
    /// O pacote veio com outro tamanho ou outra soma.
    Arquivo { versao: String },
    Espaco { versao: String },
    /// O binário novo não respondeu ao `--versao --json`.
    Binario { versao: String, detalhe: String },
    Instalacao { versao: String, detalhe: String },
    /// Falhou três vezes seguidas; espera sair outra versão.
    Desistiu { versao: String },
    /// Outro Vitral está instalando agora.
    Ocupado,
}

impl Erro {
    /// Uma linha, no tom do LEIA-ME (a interface põe a hora da próxima tentativa nos de rede).
    pub fn texto(&self) -> String {
        match self {
            Erro::Rede(_) => "Sem conexão com o GitHub; tento de novo em 1 h".to_string(),
            Erro::Assinatura => "A atualização foi recusada: a assinatura não confere. Nada foi instalado.".to_string(),
            Erro::Manifesto(_) => "O aviso da versão nova veio com defeito; nada foi instalado.".to_string(),
            Erro::Arquivo { versao } => format!("O arquivo da {versao} veio diferente do publicado; nada foi instalado."),
            Erro::Espaco { versao } => format!("Sem espaço para instalar a {versao}."),
            Erro::Binario { versao, .. } => format!("A {versao} não abriu no teste; nada foi instalado."),
            Erro::Instalacao { versao, .. } => format!("Não consegui instalar a {versao}; tento de novo mais tarde."),
            Erro::Desistiu { versao } => format!("A {versao} falhou três vezes; espero a próxima versão."),
            Erro::Ocupado => "Outro Vitral está instalando agora.".to_string(),
        }
    }

    /// Vermelho (assinatura, soma, binário) ou âmbar (rede, espaço, disco).
    pub fn grave(&self) -> bool {
        matches!(self, Erro::Assinatura | Erro::Manifesto(_) | Erro::Arquivo { .. } | Erro::Binario { .. } | Erro::Desistiu { .. })
    }
}

/// Se esta cópia pode instalar sozinha.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permissao {
    /// Release do GitHub rodando da pasta do app: baixa e instala (no modo Automáticas).
    Sozinha,
    /// Compilado no PC (`cargo run`, `target/release`, `instalar-dev.sh`): só avisa, com
    /// "Instalar como app".
    CompiladoNoPc,
    /// Release aberto fora da pasta do app (o `.tar.gz` aberto à mão): só avisa.
    ForaDaPasta,
}

fn permissao(canal: &str, gerenciado: bool) -> Permissao {
    match (canal == "estavel", gerenciado) {
        (true, true) => Permissao::Sozinha,
        (true, false) => Permissao::ForaDaPasta,
        (false, _) => Permissao::CompiladoNoPc,
    }
}

/// O que a interface pede à thread.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comando {
    Procurar,
    Instalar,
    Modo(Modo),
}

/// O que a thread conta à interface.
#[derive(Debug, Clone, PartialEq)]
pub struct Noticia {
    pub estado: Estado,
    pub ultima_procura: Option<u64>,
    /// Quando a thread procura sozinha de novo (segundos desde 1970), se vai procurar.
    pub proxima: Option<u64>,
}

/// O estado da atualização para a interface (`cx.global::<Atualizacoes>()`).
pub struct Atualizacoes {
    pub estado: Estado,
    pub modo: Modo,
    pub permissao: Permissao,
    /// Segundos desde 1970 da última procura que deu certo ("procurada hoje às 14:02").
    pub ultima_procura: Option<u64>,
    /// A próxima procura sozinha ("tento de novo às 15:10").
    pub proxima: Option<u64>,
    /// O que a saúde contou ao abrir (atualizou, voltou), para a interface mostrar uma vez.
    pub aviso_ao_abrir: Option<saude::Aviso>,
    /// A volta das Configurações (Voltar para a anterior), na versão atual da pasta do app.
    pub volta: Option<saude::VoltaPossivel>,
    comandos: Option<mpsc::Sender<Comando>>,
    progresso: Arc<AtomicBool>,
    /// `app/atual/vitral`, para onde o Reiniciar vai (o `/proc/self/exe` é a versão velha).
    atual: Option<PathBuf>,
    /// A pasta do app e a versão gerenciada que roda dela.
    app: Option<PathBuf>,
    rodando: Option<String>,
    /// A pasta de dados, de onde sai o formato do disco que a volta confere.
    dados: PathBuf,
    _ponte: Task<()>,
    _ao_sair: Subscription,
}

impl Global for Atualizacoes {}

impl Atualizacoes {
    /// A versão pronta esperando o reinício (o ponto no logo e o item do menu).
    pub fn pronta(&self) -> Option<&Novidade> {
        match &self.estado {
            Estado::Pronta(novidade) => Some(novidade),
            _ => None,
        }
    }

    /// As novidades de `novidade` em Markdown: as do manifesto, as do NOVIDADES.md que veio com
    /// ela na pasta do app ou, para esta versão, as que vêm no binário.
    pub fn notas(&self, novidade: &Novidade) -> String {
        if !novidade.notas.trim().is_empty() {
            return novidade.notas.clone();
        }
        let texto = if novidade.versao == VERSAO {
            Some(NOVIDADES.to_string())
        } else {
            self.app.as_ref().and_then(|app| std::fs::read_to_string(app.join(&novidade.versao).join("NOVIDADES.md")).ok())
        };
        texto.and_then(|t| secao_de(&t, &novidade.versao)).unwrap_or_default()
    }
}

/// A página do release de `versao`.
pub fn pagina(versao: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{versao}")
}

/// A seção `## <versão>` do NOVIDADES.md (num pré-lançamento, a da versão sem o sufixo), como o
/// `release.yml` tira as notas.
pub fn secao_de(novidades: &str, versao: &str) -> Option<String> {
    let base = versao.split('-').next().unwrap_or(versao);
    let titulo = |v: &str| format!("## {v}");
    let linhas: Vec<&str> = novidades.lines().collect();
    let inicio = [versao, base].into_iter().find_map(|v| linhas.iter().position(|l| l.trim_end() == titulo(v)))?;
    let resto = &linhas[inicio + 1..];
    let fim = resto.iter().position(|l| l.starts_with("## ")).unwrap_or(resto.len());
    let texto = resto[..fim].join("\n").trim().to_string();
    (!texto.is_empty()).then_some(texto)
}

/// Liga a atualização: põe a `Global` e abre a thread (nada se ela estiver desligada por
/// `VITRAL_SEM_ATUALIZACAO=1`, como na bancada). Na versão gerenciada, já aponta o reinício do GPUI
/// para o link `atual` e confirma a saúde ao fechar. `aviso` é o que o `saude::ao_abrir` contou e
/// `dados` é a pasta de dados (`Dados::pasta`).
pub fn iniciar(aviso: Option<saude::Aviso>, dados: PathBuf, cx: &mut App) {
    let raiz = linux::Raiz::do_usuario();
    let app = raiz.as_ref().map(linux::Raiz::app);
    let rodando = app.as_deref().and_then(linux::versao_rodando);
    let atual = raiz.as_ref().map(linux::Raiz::binario_atual);
    if rodando.is_some()
        && let Some(caminho) = &atual
    {
        cx.set_restart_path(caminho.clone());
    }
    let caminho_config = config::caminho();
    let config = caminho_config.as_deref().map(config::Config::ler).unwrap_or_default();
    let estado = raiz
        .as_ref()
        .zip(rodando.as_deref())
        .and_then(|(raiz, rodando)| trocada_por_fora(&raiz.app(), rodando))
        .map_or(Estado::Parado, Estado::Pronta);
    let permissao = permissao(canal(), rodando.is_some());
    let progresso = Arc::new(AtomicBool::new(false));
    let (envia, recebe) = smol::channel::unbounded::<Noticia>();
    let comandos = if std::env::var_os("VITRAL_SEM_ATUALIZACAO").is_some_and(|v| v == "1") {
        None
    } else {
        let ambiente = procura::Ambiente::do_app(raiz, rodando.clone(), permissao == Permissao::Sozinha);
        procura::abrir(ambiente, caminho_config, estado.clone(), envia, progresso.clone())
            .inspect_err(|erro| eprintln!("vitral: a atualização não abriu: {erro}"))
            .ok()
    };
    let volta = app
        .as_deref()
        .zip(rodando.as_deref())
        .and_then(|(app, rodando)| saude::volta_possivel(app, rodando, dados::formato_em(&dados)));
    let (app_saida, rodando_saida) = (app.clone(), rodando.clone());
    let _ao_sair = cx.on_app_quit(move |_| {
        if let Some((app, rodando)) = app_saida.as_deref().zip(rodando_saida.as_deref()) {
            saude::confirmar_esta(app, rodando);
        }
        async {}
    });
    cx.set_global(Atualizacoes {
        estado,
        modo: config.modo,
        permissao,
        ultima_procura: config.ultima_procura,
        proxima: None,
        aviso_ao_abrir: aviso,
        volta,
        comandos,
        progresso,
        atual,
        app,
        rodando,
        dados,
        _ponte: Task::ready(()),
        _ao_sair,
    });
    let ponte = cx.spawn(async move |cx| {
        while let Ok(noticia) = recebe.recv().await {
            cx.update(|cx| receber(noticia, cx));
        }
    });
    cx.global_mut::<Atualizacoes>()._ponte = ponte;
}

/// A janela desenhou o primeiro quadro: 10 s depois, a versão nova está confirmada.
pub fn primeiro_quadro(cx: &mut App) {
    let Some((app, rodando)) = cx.try_global::<Atualizacoes>().and_then(|a| a.app.clone().zip(a.rodando.clone())) else {
        return;
    };
    cx.background_spawn(async move {
        smol::Timer::after(CONFIRMA_DEPOIS).await;
        saude::confirmar_esta(&app, &rodando);
    })
    .detach();
}

/// Outra abertura (ou outro Vitral) já instalou uma versão mais nova que esta: está pronta.
fn trocada_por_fora(app: &Path, rodando: &str) -> Option<Novidade> {
    let atual = linux::versao_atual(app)?;
    let maior = semver::Version::parse(&atual).ok()? > semver::Version::parse(rodando).ok()?;
    maior.then(|| Novidade { versao: atual, notas: String::new(), pagina: String::new(), pela_pagina: false })
}

/// Só mexe na `Global` (e com isso nos observadores) quando algo mudou.
fn receber(noticia: Noticia, cx: &mut App) {
    let agora = cx.global::<Atualizacoes>();
    if agora.estado == noticia.estado && agora.ultima_procura == noticia.ultima_procura && agora.proxima == noticia.proxima {
        return;
    }
    cx.update_global::<Atualizacoes, _>(|a, cx| {
        if matches!(noticia.estado, Estado::Pronta(_)) {
            if let Some(caminho) = &a.atual {
                // Instalada agora (também pelo build do PC, no "Instalar como app"): o reinício vai para ela.
                cx.set_restart_path(caminho.clone());
            }
            // A anterior agora é esta: voltar já não é daqui.
            a.volta = None;
        }
        a.estado = noticia.estado;
        a.ultima_procura = noticia.ultima_procura;
        a.proxima = noticia.proxima;
    });
}

fn mandar(comando: Comando, cx: &App) {
    if let Some(comandos) = cx.try_global::<Atualizacoes>().and_then(|a| a.comandos.as_ref()) {
        let _ = comandos.send(comando);
    }
}

/// "Procurar agora" (vale também com as atualizações desligadas).
pub fn procurar_agora(cx: &mut App) {
    mandar(Comando::Procurar, cx);
}

/// "Instalar" quando a versão só foi avisada (Só avisar, build do PC: "Instalar como app").
pub fn instalar_agora(cx: &mut App) {
    mandar(Comando::Instalar, cx);
}

pub fn escolher_modo(modo: Modo, cx: &mut App) {
    if cx.has_global::<Atualizacoes>() {
        cx.update_global::<Atualizacoes, _>(|a, _| a.modo = modo);
        mandar(Comando::Modo(modo), cx);
    }
}

/// Com as Configurações abertas, o download conta o progresso (no máximo 2 vezes por segundo);
/// fechadas, só o começo e o fim. O progresso para quando o que voltou daqui sai (as Configurações
/// guardam enquanto estão abertas).
pub fn acompanhar_progresso(cx: &App) -> Option<Acompanhando> {
    let progresso = cx.try_global::<Atualizacoes>()?.progresso.clone();
    progresso.store(true, Ordering::Relaxed);
    Some(Acompanhando(progresso))
}

pub struct Acompanhando(Arc<AtomicBool>);

impl Drop for Acompanhando {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// Configurações › Voltar para a anterior: a de agora vai para as recusadas e o link `atual`
/// aponta para a anterior; depois disso, [`reiniciar`]. Devolve a versão para onde voltou, ou o
/// porquê de não voltar.
pub fn voltar_a_mao(cx: &mut App) -> Result<String, String> {
    let (app, rodando, pasta) = cx
        .try_global::<Atualizacoes>()
        .and_then(|a| Some((a.app.clone()?, a.rodando.clone()?, a.dados.clone())))
        .ok_or_else(|| "Só a versão instalada volta.".to_string())?;
    let para = saude::voltar(&app, &rodando, dados::formato_em(&pasta))?;
    cx.update_global::<Atualizacoes, _>(|a, _| a.volta = None);
    Ok(para)
}

/// Reinicia na versão atual (a nova, se já está pronta). Chamar depois do `pode_parar_tudo`. Sem a
/// atualização ligada (a vitrine), não faz nada.
pub fn reiniciar(cx: &mut App) {
    if !cx.has_global::<Atualizacoes>() {
        return;
    }
    let caminho = cx.try_global::<Atualizacoes>().and_then(|a| {
        let instalada = matches!(a.estado, Estado::Pronta(_));
        a.atual.clone().filter(|c| (a.rodando.is_some() || instalada) && c.exists())
    });
    if let Some(caminho) = caminho {
        cx.set_restart_path(caminho);
    }
    cx.restart();
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn json_da_versao_tem_o_que_o_atualizador_confere() {
        let valor: serde_json::Value = serde_json::from_str(&json()).unwrap();
        assert_eq!(valor["versao"], VERSAO);
        assert_eq!(valor["formato_dados"], FORMATO_DADOS);
        assert_eq!(valor["canal"], canal());
        assert_eq!(valor["commit"], commit());
        assert_eq!(valor["plataforma"], plataforma());
        assert!(!canal().is_empty());
        assert!(semver::Version::parse(VERSAO).is_ok());
    }

    #[test]
    fn so_os_comandos_sem_janela_sao_atendidos_antes_do_gpui() {
        let args = |lista: &[&str]| lista.iter().map(|a| a.to_string()).collect::<Vec<_>>();
        assert_eq!(sem_janela(&args(&[])), None);
        assert_eq!(sem_janela(&args(&["/home/victor/Projetos"])), None);
        assert_eq!(sem_janela(&args(&["--vitrine"])), None);
        assert_eq!(sem_janela(&args(&["--versao"])), Some(0));
    }

    #[test]
    fn a_dica_da_versao_diz_de_onde_veio() {
        assert_eq!(origem_de("pc", ""), "Compilado no PC");
        assert_eq!(origem_de("pc", "5109ec3aa0"), "Compilado no PC · commit 5109ec3");
        assert_eq!(origem_de("estavel", "5109ec3aa0b1"), "Commit 5109ec3");
        assert_eq!(origem_de("estavel", ""), "Canal estavel");
        assert_eq!(rotulo(), format!("Vitral {VERSAO}"));
    }

    #[test]
    fn so_o_release_na_pasta_do_app_instala_sozinho() {
        assert_eq!(permissao("estavel", true), Permissao::Sozinha);
        assert_eq!(permissao("estavel", false), Permissao::ForaDaPasta);
        assert_eq!(permissao("pc", true), Permissao::CompiladoNoPc);
        assert_eq!(permissao("pc", false), Permissao::CompiladoNoPc);
    }

    #[test]
    fn todo_erro_tem_uma_linha_e_a_cor_certa() {
        let versao = || "0.3.0".to_string();
        let erros = [
            Erro::Rede("x".into()),
            Erro::Assinatura,
            Erro::Manifesto("x".into()),
            Erro::Arquivo { versao: versao() },
            Erro::Espaco { versao: versao() },
            Erro::Binario { versao: versao(), detalhe: "x".into() },
            Erro::Instalacao { versao: versao(), detalhe: "x".into() },
            Erro::Desistiu { versao: versao() },
            Erro::Ocupado,
        ];
        for erro in &erros {
            let texto = erro.texto();
            assert!(!texto.is_empty() && !texto.contains('\n'), "{erro:?}");
        }
        assert!(Erro::Assinatura.grave() && Erro::Arquivo { versao: versao() }.grave());
        assert!(!Erro::Rede("x".into()).grave() && !Erro::Espaco { versao: versao() }.grave());
    }

    #[test]
    fn as_notas_saem_da_secao_da_versao() {
        let texto = "# Novidades\n\nIntro.\n\n## 0.3.0\n\n- **Uma.**\n- Outra.\n\n## 0.2.0\n\nA primeira.\n";
        assert_eq!(secao_de(texto, "0.3.0").as_deref(), Some("- **Uma.**\n- Outra."));
        assert_eq!(secao_de(texto, "0.2.0").as_deref(), Some("A primeira."));
        assert_eq!(secao_de(texto, "0.3.0-rc.2").as_deref(), Some("- **Uma.**\n- Outra."), "o pré-lançamento usa a da versão");
        assert_eq!(secao_de(texto, "0.4.0"), None);
        assert_eq!(secao_de("## 0.5.0\n\n## 0.4.0\nx", "0.5.0"), None, "seção vazia");
        assert!(secao_de(NOVIDADES, "0.2.0").is_some_and(|t| t.contains("atualiza")), "a 0.2.0 conta da atualização automática");
        assert!(secao_de(NOVIDADES, VERSAO).is_some(), "a versão que roda tem a seção dela");
        assert_eq!(pagina("0.3.0"), "https://github.com/VictorMenegate/vitral/releases/tag/v0.3.0");
    }

    #[test]
    fn versao_instalada_por_fora_fica_pronta() {
        let (pasta, app) = linux::testes::layout_com("0.9.0");
        assert_eq!(trocada_por_fora(&app, "0.8.0").map(|n| n.versao).as_deref(), Some("0.9.0"));
        assert_eq!(trocada_por_fora(&app, "0.9.0"), None);
        assert_eq!(trocada_por_fora(&app, "1.0.0"), None);
        std::fs::remove_dir_all(&pasta).unwrap();
    }
}
