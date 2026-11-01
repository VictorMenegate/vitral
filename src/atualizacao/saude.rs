//! Saúde da versão nova (`distribuicao.md`, seção 3.9). A versão que acabou de ser instalada só
//! fica confirmada depois de abrir direito: 10 s depois do primeiro quadro, ou fechando normalmente.
//! Até lá, cada abertura fica no `estado.json` com a sessão do PC (o boot id). Na terceira abertura
//! sem confirmar na mesma sessão, a versão vai para as recusadas e o Vitral volta para a anterior,
//! se ela abre e lê os dados do disco (`FORMATO_DADOS`). Uma queda de energia muda a sessão: as
//! aberturas de antes não contam contra a versão.
//!
//! A decisão (`decidir`, `confirmar`, `marcar_volta`) não toca no disco; `ao_abrir`, `confirmar_esta`
//! e `voltar` leem e gravam o `estado.json` com a trava das instalações (temporário + fsync +
//! rename: o velho ou o novo, nunca pela metade).

use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use super::{
    instalador::{self, Abertura, EstadoApp, Raiz, Volta},
    pacote,
};

/// Duas aberturas sem confirmar na mesma sessão: a terceira volta.
const TENTATIVAS: usize = 2;
/// As aberturas guardadas quando não dá para voltar (a lista não cresce sem fim).
const GUARDADAS: usize = 8;

/// O que o `main` faz antes do GPUI.
#[derive(Debug, PartialEq)]
pub enum AoAbrir {
    /// Abre esta versão; o aviso vai uma vez para a interface.
    Seguir(Option<Aviso>),
    /// Abre este binário (a anterior) no lugar, com os mesmos argumentos.
    Voltar(PathBuf),
}

/// O que a interface conta uma vez, ao abrir.
#[derive(Debug, Clone, PartialEq)]
pub enum Aviso {
    /// A primeira abertura da versão que entrou por cima de uma mais velha.
    Atualizada { versao: String },
    /// A versão nova foi recusada e esta é a anterior.
    Voltou { de: String, para: String, automatica: bool },
}

/// O que a decisão sabe desta abertura.
pub struct Contexto<'a> {
    pub minha: &'a str,
    pub sessao: &'a str,
    pub agora: u64,
    /// Para onde aponta o link `atual`.
    pub atual: Option<&'a str>,
    /// A anterior instalada (nunca esta), que abre.
    pub anterior: Option<&'a str>,
}

#[derive(Debug, PartialEq)]
pub enum Decisao {
    Seguir(Option<Aviso>),
    Voltar(String),
}

/// Lê e muda o `estado` para esta abertura. `formato_disco` só é lido quando há volta a decidir.
pub fn decidir(estado: &mut EstadoApp, c: &Contexto, formato_disco: &dyn Fn() -> u32) -> Decisao {
    let mut aviso = estado
        .voltou
        .take_if(|v| v.para == c.minha)
        .map(|v| Aviso::Voltou { de: v.de, para: v.para, automatica: v.automatica });
    if c.atual != Some(c.minha) {
        return Decisao::Seguir(aviso);
    }
    let destino = |estado: &EstadoApp| c.anterior.filter(|a| pode_voltar(estado, a, formato_disco())).map(str::to_string);
    // Uma volta que caiu entre gravar o estado e trocar o link termina agora.
    if estado.recusadas.iter().any(|v| v == c.minha) {
        return destino(estado).map_or(Decisao::Seguir(aviso), Decisao::Voltar);
    }
    let Some(confirmar) = estado.confirmar.as_mut().filter(|conf| conf.versao == c.minha) else {
        return Decisao::Seguir(aviso);
    };
    if confirmar.aberturas.is_empty() && c.anterior.is_some_and(|a| mais_nova(c.minha, a)) {
        aviso = aviso.or(Some(Aviso::Atualizada { versao: c.minha.to_string() }));
    }
    confirmar.aberturas.retain(|a| a.sessao == c.sessao);
    if confirmar.aberturas.len() >= TENTATIVAS
        && let Some(para) = destino(estado)
    {
        marcar_volta(estado, c.minha, &para, true);
        return Decisao::Voltar(para);
    }
    if let Some(confirmar) = estado.confirmar.as_mut() {
        confirmar.aberturas.push(Abertura { sessao: c.sessao.to_string(), quando: c.agora });
        let sobra = confirmar.aberturas.len().saturating_sub(GUARDADAS);
        confirmar.aberturas.drain(..sobra);
    }
    Decisao::Seguir(aviso)
}

/// Esta abriu direito: não há mais o que confirmar. Devolve se mudou.
pub fn confirmar(estado: &mut EstadoApp, minha: &str) -> bool {
    if estado.confirmar.as_ref().is_some_and(|c| c.versao == minha) {
        estado.confirmar = None;
        true
    } else {
        false
    }
}

/// A volta só vai para uma versão que leia o formato que está no disco. Sem o formato dela
/// guardado, conta o 1.
pub fn pode_voltar(estado: &EstadoApp, anterior: &str, formato_disco: u32) -> bool {
    estado.formatos.get(anterior).copied().unwrap_or(1) >= formato_disco
}

/// `de` vai para as recusadas (a atualização não a instala de novo) e `para` conta a volta ao abrir.
pub fn marcar_volta(estado: &mut EstadoApp, de: &str, para: &str, automatica: bool) {
    if !estado.recusadas.iter().any(|v| v == de) {
        estado.recusadas.push(de.to_string());
    }
    estado.confirmar = None;
    estado.voltou = Some(Volta { de: de.to_string(), para: para.to_string(), automatica });
}

fn mais_nova(a: &str, b: &str) -> bool {
    matches!((semver::Version::parse(a), semver::Version::parse(b)), (Ok(a), Ok(b)) if a > b)
}

/// A sessão do PC: muda a cada boot (uma queda de energia é outra sessão). Sem ela (fora do
/// Linux, por enquanto), todas as aberturas contam como uma sessão só.
pub fn sessao() -> String {
    if !cfg!(target_os = "linux") {
        return String::new();
    }
    fs::read_to_string("/proc/sys/kernel/random/boot_id").map(|s| s.trim().to_string()).unwrap_or_default()
}

fn agora() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// No começo do `main`, antes do GPUI: só a versão que roda da pasta do app tem o que conferir.
pub fn ao_abrir(formato_disco: &dyn Fn() -> u32) -> AoAbrir {
    let Some(raiz) = Raiz::do_usuario() else { return AoAbrir::Seguir(None) };
    let app = raiz.app();
    let Some(minha) = instalador::versao_rodando(&app) else { return AoAbrir::Seguir(None) };
    ao_abrir_em(&app, &minha, &sessao(), agora(), formato_disco)
}

fn ao_abrir_em(app: &Path, minha: &str, sessao: &str, agora: u64, formato_disco: &dyn Fn() -> u32) -> AoAbrir {
    // Outro Vitral instalando agora: esta abertura não conta.
    let Ok(Some(_trava)) = instalador::travar(app) else { return AoAbrir::Seguir(None) };
    let mut estado = EstadoApp::ler(app);
    let antes = estado.clone();
    let atual = instalador::versao_atual(app);
    let anterior = anterior_que_abre(app, minha);
    let contexto = Contexto { minha, sessao, agora, atual: atual.as_deref(), anterior: anterior.as_deref() };
    let decisao = decidir(&mut estado, &contexto, formato_disco);
    if estado != antes
        && let Err(erro) = estado.gravar(app)
    {
        eprintln!("vitral: não gravei {}: {erro}", app.join("estado.json").display());
        return AoAbrir::Seguir(None);
    }
    match decisao {
        Decisao::Seguir(aviso) => AoAbrir::Seguir(aviso),
        Decisao::Voltar(para) => match apontar_para(app, &para) {
            Ok(()) => {
                eprintln!("vitral: a {minha} não abriu duas vezes; voltando para a {para}");
                AoAbrir::Voltar(app.join(&para).join("vitral"))
            }
            Err(erro) => {
                eprintln!("vitral: não voltei para a {para}: {erro}");
                AoAbrir::Seguir(None)
            }
        },
    }
}

/// A anterior instalada (nunca esta), se o binário dela responde.
fn anterior_que_abre(app: &Path, minha: &str) -> Option<String> {
    let anterior = instalador::versao_do_link(app, "anterior").filter(|a| a != minha)?;
    let binario = app.join(&anterior).join("vitral");
    pacote::conferir_binario(&binario, &anterior, pacote::PRAZO_CONFERENCIA).ok()?;
    Some(anterior)
}

/// `atual` passa para `para`; o `anterior` apontava para ela e sobra.
fn apontar_para(app: &Path, para: &str) -> io::Result<()> {
    instalador::apontar(app, "atual", para)?;
    if instalador::versao_do_link(app, "anterior").as_deref() == Some(para) {
        fs::remove_file(app.join("anterior"))?;
        fs::File::open(app)?.sync_all()?;
    }
    Ok(())
}

/// A versão abriu direito (10 s depois do primeiro quadro, ou ao fechar). Com outro Vitral
/// instalando, deixa para depois: a instalação já troca o que falta confirmar.
pub fn confirmar_esta(app: &Path, minha: &str) {
    let Ok(Some(_trava)) = instalador::travar(app) else { return };
    let mut estado = EstadoApp::ler(app);
    if confirmar(&mut estado, minha)
        && let Err(erro) = estado.gravar(app)
    {
        eprintln!("vitral: não gravei {}: {erro}", app.join("estado.json").display());
    }
}

/// Para onde a volta à mão iria, visto desta versão: a anterior e se ela lê o disco.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VoltaPossivel {
    pub para: String,
    pub le_o_disco: bool,
}

/// A volta das Configurações, se há anterior (só a versão atual volta: a de antes de uma
/// instalação já vai trocar no reinício).
pub fn volta_possivel(app: &Path, minha: &str, formato_disco: u32) -> Option<VoltaPossivel> {
    if instalador::versao_atual(app).as_deref() != Some(minha) {
        return None;
    }
    let para = instalador::versao_do_link(app, "anterior").filter(|a| a != minha && app.join(a).join("vitral").is_file())?;
    let le_o_disco = pode_voltar(&EstadoApp::ler(app), &para, formato_disco);
    Some(VoltaPossivel { para, le_o_disco })
}

/// Configurações › Voltar para a anterior: a de agora vai para as recusadas e `atual` aponta para
/// a anterior. Quem chama reinicia. Devolve a versão para onde voltou.
pub fn voltar(app: &Path, minha: &str, formato_disco: u32) -> Result<String, String> {
    let trava = instalador::travar(app).map_err(|e| e.to_string())?;
    let Some(_trava) = trava else { return Err("Outro Vitral está instalando agora; tente de novo daqui a pouco.".into()) };
    let Some(para) = instalador::versao_do_link(app, "anterior").filter(|a| a != minha) else {
        return Err("Não há versão anterior instalada.".into());
    };
    if pacote::conferir_binario(&app.join(&para).join("vitral"), &para, pacote::PRAZO_CONFERENCIA).is_err() {
        return Err(format!("A {para} não abriu no teste; fico na {minha}."));
    }
    let mut estado = EstadoApp::ler(app);
    if !pode_voltar(&estado, &para, formato_disco) {
        return Err(format!("A {para} não lê as conversas do formato novo."));
    }
    marcar_volta(&mut estado, minha, &para, false);
    // O estado antes do link: uma queda no meio termina a volta na próxima abertura.
    estado.gravar(app).map_err(|e| e.to_string())?;
    apontar_para(app, &para).map_err(|e| e.to_string())?;
    Ok(para)
}

#[cfg(all(test, target_os = "linux"))]
mod testes {
    use super::*;
    use crate::atualizacao::{
        linux::{self, Confirmar, Instalada, Origem, testes::layout_com, travar},
        pacote::{gravar_atomico, testes::binario_falso},
    };
    use std::collections::BTreeMap;

    /// A 0.9.0 instalada agora por cima da 0.8.0, as duas no formato 1.
    fn instalada() -> EstadoApp {
        EstadoApp {
            confirmar: Some(Confirmar { versao: "0.9.0".into(), aberturas: Vec::new() }),
            formatos: BTreeMap::from([("0.8.0".into(), 1), ("0.9.0".into(), 1)]),
            ..EstadoApp::default()
        }
    }

    /// O `estado.json` e o link `atual`, com a 0.8.0 de anterior.
    struct Pc {
        estado: EstadoApp,
        atual: String,
    }

    impl Pc {
        fn novo() -> Pc {
            Pc { estado: instalada(), atual: "0.9.0".into() }
        }

        fn abrir(&mut self, minha: &str, sessao: &str, disco: u32) -> Decisao {
            let contexto = Contexto {
                minha,
                sessao,
                agora: 1_800_000_000,
                atual: Some(&self.atual),
                anterior: (minha != "0.8.0").then_some("0.8.0"),
            };
            let decisao = decidir(&mut self.estado, &contexto, &|| disco);
            if let Decisao::Voltar(para) = &decisao {
                self.atual = para.clone();
            }
            decisao
        }
    }

    #[test]
    fn duas_aberturas_sem_confirmar_e_a_terceira_volta() {
        let mut pc = Pc::novo();
        let atualizada = Decisao::Seguir(Some(Aviso::Atualizada { versao: "0.9.0".into() }));
        assert_eq!(pc.abrir("0.9.0", "boot-a", 1), atualizada, "a primeira avisa que atualizou");
        assert_eq!(pc.abrir("0.9.0", "boot-a", 1), Decisao::Seguir(None));
        assert_eq!(pc.estado.confirmar.as_ref().unwrap().aberturas.len(), 2);
        assert_eq!(pc.abrir("0.9.0", "boot-a", 1), Decisao::Voltar("0.8.0".into()));
        assert_eq!(pc.estado.recusadas, ["0.9.0"]);
        assert_eq!(pc.estado.confirmar, None);
        assert_eq!(pc.estado.voltou, Some(Volta { de: "0.9.0".into(), para: "0.8.0".into(), automatica: true }));
        // A anterior conta uma vez.
        let voltou = Aviso::Voltou { de: "0.9.0".into(), para: "0.8.0".into(), automatica: true };
        assert_eq!(pc.abrir("0.8.0", "boot-a", 1), Decisao::Seguir(Some(voltou)));
        assert_eq!(pc.estado.voltou, None);
        assert_eq!(pc.abrir("0.8.0", "boot-a", 1), Decisao::Seguir(None));
    }

    #[test]
    fn uma_queda_de_energia_entre_as_tentativas_nao_conta() {
        let mut pc = Pc::novo();
        pc.abrir("0.9.0", "boot-a", 1);
        pc.abrir("0.9.0", "boot-a", 1);
        // A fonte desligou o PC: outro boot, as duas de antes saem.
        assert_eq!(pc.abrir("0.9.0", "boot-b", 1), Decisao::Seguir(None));
        let aberturas = &pc.estado.confirmar.as_ref().unwrap().aberturas;
        assert_eq!(aberturas.iter().map(|a| a.sessao.as_str()).collect::<Vec<_>>(), ["boot-b"]);
        assert_eq!(pc.abrir("0.9.0", "boot-b", 1), Decisao::Seguir(None));
        assert_eq!(pc.abrir("0.9.0", "boot-c", 1), Decisao::Seguir(None));
        assert!(pc.estado.recusadas.is_empty());
    }

    #[test]
    fn confirmar_zera_e_depois_nada_conta() {
        let mut pc = Pc::novo();
        pc.abrir("0.9.0", "boot-a", 1);
        pc.abrir("0.9.0", "boot-a", 1);
        assert!(!confirmar(&mut pc.estado, "0.8.0"), "outra versão não confirma a nova");
        assert!(confirmar(&mut pc.estado, "0.9.0"));
        assert!(!confirmar(&mut pc.estado, "0.9.0"));
        for _ in 0..5 {
            assert_eq!(pc.abrir("0.9.0", "boot-a", 1), Decisao::Seguir(None));
        }
        assert_eq!(pc.estado, EstadoApp { confirmar: None, ..instalada() });
    }

    #[test]
    fn nao_volta_para_quem_nao_le_o_formato_do_disco() {
        let mut pc = Pc::novo();
        pc.estado.formatos.insert("0.9.0".into(), 2);
        for _ in 0..10 {
            assert!(matches!(pc.abrir("0.9.0", "boot-a", 2), Decisao::Seguir(_)));
        }
        assert!(pc.estado.recusadas.is_empty());
        assert_eq!(pc.estado.confirmar.as_ref().unwrap().aberturas.len(), GUARDADAS);
        // Com o disco ainda no 1, a mesma anterior serve.
        assert_eq!(pc.abrir("0.9.0", "boot-a", 1), Decisao::Voltar("0.8.0".into()));
        assert!(pode_voltar(&instalada(), "0.8.0", 1));
        assert!(!pode_voltar(&instalada(), "0.8.0", 2));
        assert!(pode_voltar(&EstadoApp::default(), "0.7.0", 1), "sem o formato guardado, conta o 1");
    }

    #[test]
    fn sem_anterior_segue_tentando() {
        let mut estado = instalada();
        let contexto = Contexto { minha: "0.9.0", sessao: "a", agora: 1, atual: Some("0.9.0"), anterior: None };
        for _ in 0..4 {
            assert_eq!(decidir(&mut estado, &contexto, &|| 1), Decisao::Seguir(None), "sem anterior não diz que atualizou");
        }
    }

    #[test]
    fn so_a_versao_do_menu_conta_aberturas() {
        let mut estado = instalada();
        // A 0.9.0 aberta pelo caminho, com o menu já em outra.
        let contexto = Contexto { minha: "0.9.0", sessao: "a", agora: 1, atual: Some("1.0.0"), anterior: Some("0.8.0") };
        for _ in 0..3 {
            assert_eq!(decidir(&mut estado, &contexto, &|| 1), Decisao::Seguir(None));
        }
        assert_eq!(estado, instalada());
    }

    #[test]
    fn volta_que_caiu_no_meio_termina_na_abertura_seguinte() {
        // A volta à mão gravou o estado e a fonte desligou antes de trocar o link.
        let mut estado = instalada();
        marcar_volta(&mut estado, "0.9.0", "0.8.0", false);
        let contexto = Contexto { minha: "0.9.0", sessao: "a", agora: 1, atual: Some("0.9.0"), anterior: Some("0.8.0") };
        assert_eq!(decidir(&mut estado, &contexto, &|| 1), Decisao::Voltar("0.8.0".into()));
        // Aberta pelo caminho, com o menu já na 0.8.0, a recusada abre.
        let contexto = Contexto { atual: Some("0.8.0"), ..contexto };
        assert_eq!(decidir(&mut estado, &contexto, &|| 1), Decisao::Seguir(None));
    }

    /// O layout de teste com a 0.8.0 e a 0.9.0 instalada por cima (binários de mentira que
    /// respondem ao `--versao --json`).
    fn layout() -> (PathBuf, PathBuf) {
        let (pasta, app) = layout_com("0.8.0");
        let fonte = pasta.join("fonte/0.9.0/vitral");
        gravar_atomico(&fonte, &binario_falso("0.9.0"), 0o755).unwrap();
        let trava = travar(&app).unwrap().unwrap();
        let instalada = linux::instalar(&app, Origem::Binario(&fonte), "0.9.0", Some("0.8.0"), &trava).unwrap();
        assert_eq!(instalada, Instalada { versao: "0.9.0".into(), formato_dados: 1 });
        (pasta, app)
    }

    #[test]
    fn no_disco_a_terceira_abertura_volta_e_a_anterior_conta() {
        let (pasta, app) = layout();
        let um = || 1;
        let atualizada = AoAbrir::Seguir(Some(Aviso::Atualizada { versao: "0.9.0".into() }));
        assert_eq!(ao_abrir_em(&app, "0.9.0", "boot-a", 10, &um), atualizada);
        assert_eq!(ao_abrir_em(&app, "0.9.0", "boot-a", 20, &um), AoAbrir::Seguir(None));
        assert_eq!(ao_abrir_em(&app, "0.9.0", "boot-a", 30, &um), AoAbrir::Voltar(app.join("0.8.0/vitral")));
        assert_eq!(linux::versao_atual(&app).as_deref(), Some("0.8.0"));
        assert_eq!(linux::versao_do_link(&app, "anterior"), None, "o anterior apontava para a que ficou");
        assert_eq!(EstadoApp::ler(&app).recusadas, ["0.9.0"]);
        let voltou = Aviso::Voltou { de: "0.9.0".into(), para: "0.8.0".into(), automatica: true };
        assert_eq!(ao_abrir_em(&app, "0.8.0", "boot-a", 40, &um), AoAbrir::Seguir(Some(voltou)));
        assert_eq!(ao_abrir_em(&app, "0.8.0", "boot-a", 50, &um), AoAbrir::Seguir(None));
        // A 0.9.0 não é oferecida de novo.
        assert!(!crate::atualizacao::manifesto::e_nova(
            &semver::Version::new(0, 8, 0),
            &semver::Version::new(0, 9, 0),
            false,
            &EstadoApp::ler(&app).recusadas,
            None
        ));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn no_disco_confirmada_ou_com_queda_entre_tentativas_fica() {
        let (pasta, app) = layout();
        let um = || 1;
        ao_abrir_em(&app, "0.9.0", "boot-a", 10, &um);
        ao_abrir_em(&app, "0.9.0", "boot-a", 20, &um);
        assert_eq!(ao_abrir_em(&app, "0.9.0", "boot-b", 30, &um), AoAbrir::Seguir(None));
        confirmar_esta(&app, "0.9.0");
        assert_eq!(EstadoApp::ler(&app).confirmar, None);
        for quando in 40..44 {
            assert_eq!(ao_abrir_em(&app, "0.9.0", "boot-b", quando, &um), AoAbrir::Seguir(None));
        }
        assert_eq!(linux::versao_atual(&app).as_deref(), Some("0.9.0"));
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn no_disco_anterior_que_nao_abre_ou_formato_novo_nao_volta() {
        let (pasta, app) = layout();
        // A anterior com o binário zerado (a queda de energia que já aconteceu).
        fs::write(app.join("0.8.0/vitral"), b"").unwrap();
        for quando in 0..4 {
            assert!(matches!(ao_abrir_em(&app, "0.9.0", "boot-a", quando, &|| 1), AoAbrir::Seguir(None)));
        }
        assert_eq!(voltar(&app, "0.9.0", 1), Err("A 0.8.0 não abriu no teste; fico na 0.9.0.".into()));
        gravar_atomico(&app.join("0.8.0/vitral"), &binario_falso("0.8.0"), 0o755).unwrap();
        assert_eq!(voltar(&app, "0.9.0", 2), Err("A 0.8.0 não lê as conversas do formato novo.".into()));
        assert_eq!(volta_possivel(&app, "0.9.0", 2), Some(VoltaPossivel { para: "0.8.0".into(), le_o_disco: false }));
        assert_eq!(linux::versao_atual(&app).as_deref(), Some("0.9.0"));
        assert!(EstadoApp::ler(&app).recusadas.is_empty());
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn volta_a_mao_recusa_a_de_agora_e_aponta_para_a_anterior() {
        let (pasta, app) = layout();
        assert_eq!(volta_possivel(&app, "0.9.0", 1), Some(VoltaPossivel { para: "0.8.0".into(), le_o_disco: true }));
        assert_eq!(volta_possivel(&app, "0.8.0", 1), None, "a anterior não volta para lugar nenhum");
        let trava = travar(&app).unwrap().unwrap();
        assert!(voltar(&app, "0.9.0", 1).unwrap_err().starts_with("Outro Vitral está instalando"));
        drop(trava);
        assert_eq!(voltar(&app, "0.9.0", 1), Ok("0.8.0".into()));
        assert_eq!(linux::versao_atual(&app).as_deref(), Some("0.8.0"));
        let estado = EstadoApp::ler(&app);
        assert_eq!(estado.recusadas, ["0.9.0"]);
        assert_eq!(estado.voltou, Some(Volta { de: "0.9.0".into(), para: "0.8.0".into(), automatica: false }));
        assert_eq!(volta_possivel(&app, "0.8.0", 1), None);
        // Reinstalar a recusada à mão tira a recusa.
        let fonte = pasta.join("fonte/0.9.0/vitral");
        let trava = travar(&app).unwrap().unwrap();
        linux::instalar(&app, Origem::Binario(&fonte), "0.9.0", Some("0.8.0"), &trava).unwrap();
        assert!(EstadoApp::ler(&app).recusadas.is_empty());
        fs::remove_dir_all(&pasta).unwrap();
    }

    #[test]
    fn o_estado_e_gravado_de_uma_vez() {
        let (pasta, app) = layout();
        let antes = fs::read(app.join("estado.json")).unwrap();
        // Uma gravação cortada pela fonte deixa só o temporário, que ninguém lê.
        fs::write(app.join(".estado.json.999.tmp"), b"{\"confirmar\":").unwrap();
        assert_eq!(fs::read(app.join("estado.json")).unwrap(), antes);
        assert_eq!(EstadoApp::ler(&app).confirmar.map(|c| c.versao).as_deref(), Some("0.9.0"));
        // Lixo no lugar do arquivo vale como nada a confirmar (e a versão abre).
        fs::write(app.join("estado.json"), b"{ quebrado").unwrap();
        assert_eq!(EstadoApp::ler(&app), EstadoApp::default());
        assert_eq!(ao_abrir_em(&app, "0.9.0", "boot-a", 1, &|| 1), AoAbrir::Seguir(None));
        let mut gravado = instalada();
        gravado.voltou = Some(Volta { de: "0.9.0".into(), para: "0.8.0".into(), automatica: true });
        gravado.gravar(&app).unwrap();
        assert_eq!(EstadoApp::ler(&app), gravado);
        let valor: serde_json::Value = serde_json::from_slice(&fs::read(app.join("estado.json")).unwrap()).unwrap();
        assert_eq!(valor["voltou"]["para"], "0.8.0");
        assert_eq!(valor["confirmar"]["versao"], "0.9.0");
        fs::remove_dir_all(&pasta).unwrap();
    }
}
