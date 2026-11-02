//! O laço do vigia, sem o gpui: um só alvo periódico por repositório (os PRs acompanhados dele numa
//! consulta), as consultas "na hora" (ao abrir, ao voltar o foco e 8 s depois de um envio) uma por
//! vez, e a sonda quando a rede cai. O motor chama `passo` a cada volta e espera o que ele diz com
//! `esperar`: o timer do próximo prazo ou o sinal `acordar`, que o fim de toda consulta manda. Sem
//! prazo nenhum, espera só o sinal: nunca acorda à toa (a fonte do PC não aguenta laço apertado).

use std::{
    collections::HashMap,
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use smol::channel::{Receiver, Sender};

use crate::{
    remoto::{EstadoRede, Recuo},
    vigia::{self, Contexto, EstadoAlvo},
};

/// Depois de um envio, criação ou mescla do Vitral, a consulta extra sai em tanto tempo.
pub const EXTRA_DEPOIS: Duration = Duration::from_secs(8);
/// O limite de uma consulta do gh (leitura) mais a folga: passou disso sem voltar, o laço segue.
pub const SEGURANCA: Duration = Duration::from_secs(30 + 10);

/// O que fazer nesta volta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Passo {
    /// Consultar os PRs deste repositório (com a guarda de `comecou`).
    Consultar(String),
    /// A rede está fora e é a hora da sonda.
    Sondar,
    /// Esperar até o prazo (ou o sinal); `None`: só o sinal.
    Esperar(Option<Instant>),
}

/// A consulta em voo: some em qualquer caminho (fim, erro, pânico, o motor solto) e acorda o laço.
pub struct EmVoo {
    marca: Arc<AtomicBool>,
    acordar: Sender<()>,
}

impl Drop for EmVoo {
    fn drop(&mut self) {
        self.marca.store(false, Ordering::SeqCst);
        let _ = self.acordar.try_send(());
    }
}

/// Um repositório que o vigia consulta de tempos em tempos, com o contexto da última volta.
#[derive(Clone, Debug)]
struct Alvo {
    estado: EstadoAlvo,
    contexto: Contexto,
}

/// O estado do laço.
pub struct Vigia {
    alvos: HashMap<String, Alvo>,
    /// As consultas na hora, em série (sem repetir o mesmo repositório).
    na_hora: Vec<String>,
    /// A consulta extra depois de um envio, por repositório.
    extra: HashMap<String, Instant>,
    /// A marca da consulta em voo (uma por vez) e desde quando.
    em_voo: Arc<AtomicBool>,
    em_voo_desde: Option<Instant>,
    acordar: Sender<()>,
    /// Passou disso em voo, o laço deixa a consulta para trás e segue.
    pub seguranca: Duration,
    /// Quantas voltas o laço deu (o teste confere que ele não gira).
    pub voltas: u64,
    /// As consultas que passaram do limite sem voltar.
    pub perdidas: u64,
}

impl Vigia {
    pub fn novo(acordar: Sender<()>) -> Self {
        Self {
            alvos: HashMap::new(),
            na_hora: Vec::new(),
            extra: HashMap::new(),
            em_voo: Arc::default(),
            em_voo_desde: None,
            acordar,
            seguranca: SEGURANCA,
            voltas: 0,
            perdidas: 0,
        }
    }

    /// Acorda o laço agora (uma ação mudou alguma coisa).
    pub fn acordar(&self) {
        let _ = self.acordar.try_send(());
    }

    /// Consultar este repositório na próxima volta (abrir, voltar o foco, a aba PR, ⟳).
    pub fn na_hora(&mut self, repo: &str) {
        if !self.na_hora.iter().any(|r| r == repo) {
            self.na_hora.push(repo.to_string());
        }
        self.acordar();
    }

    /// A consulta extra, 8 s depois de um envio, criação ou mescla do Vitral.
    pub fn depois_do_envio(&mut self, repo: &str, agora: Instant) {
        self.extra.insert(repo.to_string(), agora + EXTRA_DEPOIS);
        self.acordar();
    }

    pub fn em_voo(&self) -> bool {
        self.em_voo.load(Ordering::SeqCst)
    }

    /// O que fazer agora. `repos`: cada repositório com PR acompanhado (ou à vista) e o contexto do PR
    /// mais quente dele (o `sem_mudanca` vem do alvo).
    pub fn passo(&mut self, rede: &EstadoRede, repos: &[(String, Contexto)], agora: Instant) -> Passo {
        self.voltas += 1;
        if self.em_voo() {
            let desde = *self.em_voo_desde.get_or_insert(agora);
            if agora.saturating_duration_since(desde) < self.seguranca {
                return Passo::Esperar(Some(desde + self.seguranca));
            }
            // Não voltou no limite: segue sem ela (a guarda dela, se um dia soltar, não mexe na nova).
            self.perdidas += 1;
            self.em_voo = Arc::default();
            self.em_voo_desde = None;
        }
        // Os alvos periódicos seguem os repositórios de agora; o que não tem intervalo sai.
        self.alvos.retain(|repo, _| repos.iter().any(|(r, _)| r == repo));
        for (repo, contexto) in repos {
            let sem_mudanca = self.alvos.get(repo).map_or(0, |a| a.estado.sem_mudanca);
            let contexto = Contexto { sem_mudanca, ..contexto.clone() };
            match vigia::intervalo(&contexto) {
                None => {
                    self.alvos.remove(repo);
                }
                Some(intervalo) => {
                    let alvo = self.alvos.entry(repo.clone()).or_insert_with(|| Alvo {
                        estado: EstadoAlvo { repo: repo.clone(), proxima: agora + intervalo, recuo: Recuo::default(), sem_mudanca: 0 },
                        contexto: contexto.clone(),
                    });
                    // Ficou mais quente (um envio, checks rodando): o prazo encurta, nunca alonga.
                    alvo.estado.proxima = alvo.estado.proxima.min(agora + intervalo);
                    alvo.contexto = contexto;
                }
            }
        }
        match rede {
            EstadoRede::Fora { proxima_sonda, .. } => {
                return if agora >= *proxima_sonda { Passo::Sondar } else { Passo::Esperar(Some(*proxima_sonda)) };
            }
            EstadoRede::Limite { ate } if agora < *ate => return Passo::Esperar(Some(*ate)),
            _ => {}
        }
        if !self.na_hora.is_empty() {
            let repo = self.na_hora.remove(0);
            self.extra.remove(&repo);
            return Passo::Consultar(repo);
        }
        if let Some(repo) = self.extra.iter().find(|(_, quando)| **quando <= agora).map(|(r, _)| r.clone()) {
            self.extra.remove(&repo);
            return Passo::Consultar(repo);
        }
        if let Some(repo) = self.alvos.values().filter(|a| a.estado.proxima <= agora).min_by_key(|a| a.estado.proxima).map(|a| a.estado.repo.clone()) {
            return Passo::Consultar(repo);
        }
        let mut prazos: Vec<EstadoAlvo> = self.alvos.values().map(|a| a.estado.clone()).collect();
        prazos.extend(self.extra.iter().map(|(repo, quando)| EstadoAlvo {
            repo: repo.clone(),
            proxima: *quando,
            recuo: Recuo::default(),
            sem_mudanca: 0,
        }));
        Passo::Esperar(vigia::proximo_despertar(&prazos, rede, false, agora))
    }

    /// A consulta (ou a sonda) começou: a guarda vai junto com ela e acorda o laço no fim.
    pub fn comecou(&mut self, agora: Instant) -> EmVoo {
        self.em_voo.store(true, Ordering::SeqCst);
        self.em_voo_desde = Some(agora);
        EmVoo { marca: self.em_voo.clone(), acordar: self.acordar.clone() }
    }

    /// A consulta do repositório voltou: o próximo prazo pelo contexto (com o recuo, depois de erro
    /// transitório). Qualquer mudança volta ao começo da tabela.
    pub fn terminou(&mut self, repo: &str, mudou: bool, erro_transitorio: bool, agora: Instant) {
        let Some(alvo) = self.alvos.get_mut(repo) else {
            return;
        };
        alvo.estado.recuo.falhas = if erro_transitorio { alvo.estado.recuo.falhas.saturating_add(1) } else { 0 };
        alvo.estado.sem_mudanca = if mudou || erro_transitorio { 0 } else { alvo.estado.sem_mudanca.saturating_add(1) };
        let contexto = Contexto { sem_mudanca: alvo.estado.sem_mudanca, ..alvo.contexto.clone() };
        if let Some(proxima) = vigia::proxima_consulta(&contexto, &alvo.estado.recuo, agora) {
            alvo.estado.proxima = proxima;
        }
    }
}

/// Espera o prazo (`None`: só o sinal) ou o sinal. Os sinais que chegaram juntos contam como um. Com
/// o canal fechado, só o prazo vale (nunca volta na hora: o laço não gira).
pub async fn esperar<T: Future<Output = ()>>(quando: Option<Instant>, acordado: &Receiver<()>, dormir: impl FnOnce(Duration) -> T) {
    let sinal = async {
        if acordado.recv().await.is_err() {
            std::future::pending::<()>().await;
        }
        while acordado.try_recv().is_ok() {}
    };
    match quando {
        None => sinal.await,
        Some(quando) => smol::future::or(sinal, dormir(quando.saturating_duration_since(Instant::now()))).await,
    }
}

#[cfg(test)]
mod testes {
    use std::sync::{Condvar, Mutex, atomic::AtomicUsize};

    use super::*;
    use crate::remoto::{Ambiente, ErroRemoto, Executor, SaidaBruta};

    /// Um executor que trava até o teste soltar (o gh que nunca volta, nem no limite).
    #[derive(Default)]
    struct Trava {
        solta: Mutex<bool>,
        sinal: Condvar,
        chamadas: AtomicUsize,
    }

    impl Executor for Trava {
        fn rodar(&self, _: &'static str, _: &[String], _: &Ambiente, _: &std::path::Path, _: Option<&[u8]>, _: Duration) -> Result<SaidaBruta, ErroRemoto> {
            self.chamadas.fetch_add(1, Ordering::SeqCst);
            let mut solta = self.solta.lock().unwrap();
            while !*solta {
                solta = self.sinal.wait(solta).unwrap();
            }
            Err(ErroRemoto::TempoEsgotado)
        }
    }

    fn acompanhado() -> Contexto {
        Contexto { algum_acompanhado: true, janela_ativa: true, esperando_pessoas: true, ..Default::default() }
    }

    /// Roda o laço de verdade (o `passo` e o `esperar`) por `quanto`, com as consultas no executor.
    fn rodar_por(vigia: &mut Vigia, acordado: &Receiver<()>, e: &Arc<Trava>, repos: &[(String, Contexto)], quanto: Duration) {
        let fim = Instant::now() + quanto;
        smol::block_on(async {
            while Instant::now() < fim {
                let agora = Instant::now();
                let quando = match vigia.passo(&EstadoRede::Ok, repos, agora) {
                    Passo::Consultar(repo) => {
                        let guarda = vigia.comecou(agora);
                        let e = e.clone();
                        std::thread::spawn(move || {
                            let _guarda = guarda;
                            let _ = e.rodar("gh", &[repo], &Ambiente::default(), std::path::Path::new("/"), None, Duration::from_secs(30));
                        });
                        continue;
                    }
                    Passo::Sondar => continue,
                    Passo::Esperar(q) => q.map(|q| q.min(fim)).or(Some(fim)),
                };
                esperar(quando, acordado, |d| async move {
                    smol::Timer::after(d).await;
                })
                .await;
            }
        });
    }

    #[test]
    fn o_laco_com_um_executor_que_trava_nao_gira() {
        let (acordar, acordado) = smol::channel::unbounded();
        let mut vigia = Vigia::novo(acordar);
        vigia.na_hora("eita/app");
        let e = Arc::new(Trava::default());
        let repos = [("eita/app".to_string(), acompanhado())];
        rodar_por(&mut vigia, &acordado, &e, &repos, Duration::from_millis(800));
        // Uma consulta só (ela trava); o laço acordou pela consulta e pelo fim do teste, e só.
        assert_eq!(e.chamadas.load(Ordering::SeqCst), 1);
        assert!(vigia.voltas <= 3, "o laço girou: {} voltas", vigia.voltas);
        assert!(vigia.em_voo());
        // Com o limite de segurança curto, a consulta que não volta é deixada para trás no limite, e o
        // laço volta a esperar o prazo: nada de laço apertado.
        let (acordar, acordado) = smol::channel::unbounded();
        let mut vigia = Vigia::novo(acordar);
        vigia.seguranca = Duration::from_millis(200);
        vigia.na_hora("eita/app");
        let e2 = Arc::new(Trava::default());
        rodar_por(&mut vigia, &acordado, &e2, &repos, Duration::from_millis(1000));
        // A perdida não é refeita na hora: a próxima é a do prazo do repositório (300 s).
        assert_eq!(e2.chamadas.load(Ordering::SeqCst), 1);
        assert_eq!(vigia.perdidas, 1);
        assert!(!vigia.em_voo());
        assert!(vigia.voltas <= 4, "{} voltas", vigia.voltas);
        for t in [&e, &e2] {
            *t.solta.lock().unwrap() = true;
            t.sinal.notify_all();
        }
    }

    #[test]
    fn sem_alvo_espera_so_o_sinal() {
        let (acordar, acordado) = smol::channel::unbounded();
        let mut vigia = Vigia::novo(acordar);
        let agora = Instant::now();
        // Sem repositório nenhum, nem com a janela ativa.
        assert_eq!(vigia.passo(&EstadoRede::Ok, &[], agora), Passo::Esperar(None));
        // Um PR que ninguém acompanha e sem a aba à vista também não tem prazo.
        let solto = [("o/r".to_string(), Contexto { janela_ativa: true, ..Default::default() })];
        assert_eq!(vigia.passo(&EstadoRede::Ok, &solto, agora), Passo::Esperar(None));
        // A espera sem prazo só volta com o sinal.
        let comeco = Instant::now();
        let voltou = smol::block_on(smol::future::or(
            async {
                esperar(None, &acordado, |_| async {}).await;
                true
            },
            async {
                smol::Timer::after(Duration::from_millis(300)).await;
                false
            },
        ));
        assert!(!voltou && comeco.elapsed() >= Duration::from_millis(300));
        vigia.acordar();
        smol::block_on(esperar(None, &acordado, |_| async {}));
        // Com o canal fechado e sem prazo, nunca volta na hora.
        drop(vigia);
        let fechado = smol::block_on(smol::future::or(
            async {
                esperar(None, &acordado, |_| async {}).await;
                true
            },
            async {
                smol::Timer::after(Duration::from_millis(100)).await;
                false
            },
        ));
        assert!(!fechado);
    }

    #[test]
    fn na_hora_em_serie_extra_depois_do_envio_e_rede_fora() {
        let (acordar, _acordado) = smol::channel::unbounded();
        let mut vigia = Vigia::novo(acordar);
        let agora = Instant::now();
        let repos = [("o/a".to_string(), acompanhado()), ("o/b".to_string(), acompanhado())];
        vigia.na_hora("o/a");
        vigia.na_hora("o/b");
        vigia.na_hora("o/a");
        assert_eq!(vigia.passo(&EstadoRede::Ok, &repos, agora), Passo::Consultar("o/a".into()));
        let guarda = vigia.comecou(agora);
        // Em voo: nada mais começa, e o prazo é o da segurança.
        assert_eq!(vigia.passo(&EstadoRede::Ok, &repos, agora), Passo::Esperar(Some(agora + SEGURANCA)));
        drop(guarda);
        vigia.terminou("o/a", true, false, agora);
        assert_eq!(vigia.passo(&EstadoRede::Ok, &repos, agora), Passo::Consultar("o/b".into()));
        vigia.terminou("o/b", false, false, agora);
        // Os dois esperando pessoas: 300 s.
        assert_eq!(vigia.passo(&EstadoRede::Ok, &repos, agora), Passo::Esperar(Some(agora + Duration::from_secs(300))));
        // Um envio: a consulta extra em 8 s.
        vigia.depois_do_envio("o/b", agora);
        assert_eq!(vigia.passo(&EstadoRede::Ok, &repos, agora), Passo::Esperar(Some(agora + EXTRA_DEPOIS)));
        assert_eq!(vigia.passo(&EstadoRede::Ok, &repos, agora + EXTRA_DEPOIS), Passo::Consultar("o/b".into()));
        // A rede fora: só a sonda, no prazo dela, mesmo com consultas vencidas.
        let fora = EstadoRede::Fora { desde: agora, proxima_sonda: agora + Duration::from_secs(30), tentativas: 0 };
        vigia.na_hora("o/a");
        assert_eq!(vigia.passo(&fora, &repos, agora), Passo::Esperar(Some(agora + Duration::from_secs(30))));
        assert_eq!(vigia.passo(&fora, &repos, agora + Duration::from_secs(30)), Passo::Sondar);
        // O erro transitório espaça (recuo), e a mudança volta ao começo.
        vigia.terminou("o/a", false, true, agora);
        vigia.terminou("o/a", false, true, agora);
        let ok = EstadoRede::Ok;
        let _ = vigia.passo(&ok, &repos, agora);
        let Passo::Esperar(Some(_)) = vigia.passo(&ok, &repos, agora) else { panic!() };
    }
}
