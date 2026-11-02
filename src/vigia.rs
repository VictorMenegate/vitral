//! A conta do vigia dos PRs: de quanto em quanto tempo consultar cada repositório e quando o laço
//! acorda. Só lógica pura; o laço (um só, `select` entre o timer e o canal `acordar`) fica no
//! motor. Há um único tipo de alvo periódico: os PRs acompanhados de um repositório, numa
//! consulta; o PR mais "quente" decide o intervalo.

use std::time::{Duration, Instant};

use crate::remoto::{EstadoRede, Recuo};

/// Um repositório que o vigia consulta.
#[derive(Clone, Debug)]
pub struct EstadoAlvo {
    pub repo: String,
    pub proxima: Instant,
    pub recuo: Recuo,
    pub sem_mudanca: u32,
}

/// O que decide o intervalo de um repositório (do PR mais quente dele).
#[derive(Clone, Debug, Default)]
pub struct Contexto {
    pub janela_ativa: bool,
    /// Desde o último push, create ou merge do Vitral nesse repositório.
    pub desde_push: Option<Duration>,
    /// As contagens dos checks mudaram nos últimos 30 min, `UNKNOWN` há menos de 10 min ou fila de
    /// merge.
    pub progresso: bool,
    /// Revisão, BLOCKED, BEHIND, REVIEW_REQUIRED ou checks parados. O intervalo é o mesmo de "sem
    /// progresso" (a tabela junta os dois): fica para a dica da interface.
    #[allow(dead_code)]
    pub esperando_pessoas: bool,
    pub sem_mudanca: u32,
    pub algum_acompanhado: bool,
    pub aba_visivel: bool,
}

const LOGO_DEPOIS: Duration = Duration::from_secs(2 * 60);
const ESPERANDO: Duration = Duration::from_secs(300);
const TETO_ESPERANDO: Duration = Duration::from_secs(30 * 60);
/// O laço nunca acorda antes disso, nem com um alvo atrasado: não gira em laço apertado mesmo
/// que alguém esqueça de reagendar.
const PISO: Duration = Duration::from_secs(1);

/// De quanto em quanto tempo consultar; `None` = não consulta periodicamente (só nos momentos
/// "na hora": abrir, voltar o foco e 8 s depois de push, create ou merge).
pub fn intervalo(c: &Contexto) -> Option<Duration> {
    if !c.algum_acompanhado {
        // Só com a aba PR à vista, para perceber o merge ou o fechamento feito por outra pessoa.
        return (c.janela_ativa && c.aba_visivel).then_some(ESPERANDO);
    }
    if c.desde_push.is_some_and(|d| d <= LOGO_DEPOIS) {
        return Some(Duration::from_secs(20));
    }
    if c.progresso {
        return Some(Duration::from_secs(if c.janela_ativa { 40 } else { 60 }));
    }
    // Esperando pessoas, ou sem progresso: dobra a cada 3 consultas sem mudança.
    let dobras = (c.sem_mudanca / 3).min(3);
    Some((ESPERANDO * (1 << dobras)).min(TETO_ESPERANDO))
}

/// Quando consultar de novo esse alvo: o intervalo normal ou, depois de erro transitório, o
/// recuo sobre ele (×2, teto de 15 min).
pub fn proxima_consulta(c: &Contexto, recuo: &Recuo, agora: Instant) -> Option<Instant> {
    let base = intervalo(c)?;
    let espera = if recuo.falhas > 0 { recuo.proxima(base) } else { base };
    Some(agora + espera)
}

/// Quando o laço acorda pelo timer; `None` = espera só o canal. Com uma consulta em voo, o fim
/// dela é que acorda. Com a rede `Fora` ou no `Limite`, os alvos ficam parados e vale a sonda ou
/// o fim do limite.
pub fn proximo_despertar(alvos: &[EstadoAlvo], rede: &EstadoRede, em_voo: bool, agora: Instant) -> Option<Instant> {
    if em_voo || alvos.is_empty() {
        return None;
    }
    let quando = match rede {
        EstadoRede::Fora { proxima_sonda, .. } => *proxima_sonda,
        EstadoRede::Limite { ate } => *ate,
        EstadoRede::Ok => alvos.iter().map(|a| a.proxima).min()?,
    };
    Some(quando.max(agora + PISO))
}

#[cfg(test)]
mod testes {
    use super::*;

    fn s(segundos: u64) -> Option<Duration> {
        Some(Duration::from_secs(segundos))
    }

    #[test]
    fn intervalo_segue_a_tabela() {
        let acompanhado = Contexto { algum_acompanhado: true, ..Default::default() };
        for janela_ativa in [true, false] {
            let c = Contexto { janela_ativa, desde_push: s(30), progresso: true, ..acompanhado.clone() };
            assert_eq!(intervalo(&c), s(20));
            let depois = Contexto { desde_push: s(121), ..c.clone() };
            assert_eq!(intervalo(&depois), s(if janela_ativa { 40 } else { 60 }));
            let parado = Contexto { janela_ativa, esperando_pessoas: true, ..acompanhado.clone() };
            let esperas: Vec<_> = [0, 2, 3, 5, 6, 9, 30, u32::MAX]
                .iter()
                .map(|&sem_mudanca| intervalo(&Contexto { sem_mudanca, ..parado.clone() }).unwrap().as_secs())
                .collect();
            assert_eq!(esperas, [300, 300, 600, 600, 1200, 1800, 1800, 1800]);
        }
        // Nenhum PR acompanhado: só com a aba PR à vista e a janela ativa.
        let solto = Contexto { aba_visivel: true, janela_ativa: true, desde_push: s(5), ..Default::default() };
        assert_eq!(intervalo(&solto), s(300));
        assert_eq!(intervalo(&Contexto { aba_visivel: false, ..solto.clone() }), None);
        assert_eq!(intervalo(&Contexto { janela_ativa: false, ..solto }), None);
    }

    #[test]
    fn o_recuo_vale_sobre_o_intervalo() {
        let agora = Instant::now();
        let c = Contexto { algum_acompanhado: true, progresso: true, janela_ativa: true, ..Default::default() };
        assert_eq!(proxima_consulta(&c, &Recuo::default(), agora), Some(agora + Duration::from_secs(40)));
        let com_falhas = proxima_consulta(&c, &Recuo { falhas: 2 }, agora).unwrap() - agora;
        assert!(com_falhas >= Duration::from_secs(144) && com_falhas <= Duration::from_secs(176), "{com_falhas:?}");
        let muitas = proxima_consulta(&c, &Recuo { falhas: 40 }, agora).unwrap() - agora;
        assert!(muitas <= Duration::from_secs(15 * 60));
        assert_eq!(proxima_consulta(&Contexto::default(), &Recuo::default(), agora), None);
    }

    #[test]
    fn proximo_despertar_nunca_acorda_a_toa() {
        let agora = Instant::now();
        let alvo = |repo: &str, daqui: i64| EstadoAlvo {
            repo: repo.into(),
            proxima: if daqui >= 0 {
                agora + Duration::from_secs(daqui as u64)
            } else {
                agora.checked_sub(Duration::from_secs(daqui.unsigned_abs())).unwrap_or(agora)
            },
            recuo: Recuo::default(),
            sem_mudanca: 0,
        };
        let ok = EstadoRede::Ok;
        // Sem alvo nenhum, espera só o canal.
        assert_eq!(proximo_despertar(&[], &ok, false, agora), None);
        let alvos = [alvo("o/a", 300), alvo("o/b", 40)];
        assert_eq!(proximo_despertar(&alvos, &ok, false, agora), Some(agora + Duration::from_secs(40)));
        // Atrasado acorda logo, mas nunca antes do piso.
        let atrasado = [alvo("o/a", -50)];
        assert_eq!(proximo_despertar(&atrasado, &ok, false, agora), Some(agora + PISO));
        // Em voo: quem acorda é o fim da consulta, nunca o timer.
        assert_eq!(proximo_despertar(&atrasado, &ok, true, agora), None);
        // Fora: vale a sonda, não o alvo atrasado.
        let fora = EstadoRede::Fora { desde: agora, proxima_sonda: agora + Duration::from_secs(60), tentativas: 1 };
        assert_eq!(proximo_despertar(&atrasado, &fora, false, agora), Some(agora + Duration::from_secs(60)));
        let limite = EstadoRede::Limite { ate: agora + Duration::from_secs(900) };
        assert_eq!(proximo_despertar(&atrasado, &limite, false, agora), Some(agora + Duration::from_secs(900)));
        let sonda_vencida = EstadoRede::Fora { desde: agora, proxima_sonda: agora, tentativas: 0 };
        assert_eq!(proximo_despertar(&atrasado, &sonda_vencida, false, agora), Some(agora + PISO));
    }
}
