//! Previsão de quando uma janela do plano chega a 100% no ritmo de agora.
//!
//! O motor guarda, por conta, as últimas leituras do uso (as da busca na Anthropic, as do
//! `.claude.json` e as que cada turno traz no `rate_limit_event`). A inclinação entre a leitura
//! mais velha do trecho recente e a última diz em quanto tempo a janela enche; se isso cai antes
//! da renovação, o popover do anel diz a hora e o anel do canto fica âmbar.
//!
//! As leituras ficam só na memória: depois de abrir o Vitral, a previsão aparece quando houver
//! leituras bastantes (`MINIMO_5H`).

use std::collections::VecDeque;

use crate::motor::Uso;

/// O trecho que conta como "o ritmo de agora" na janela de 5 h.
const RITMO_5H: i64 = 30 * 60;
/// Na semana o trecho é maior: meia hora de trabalho pesado esticada por dias diria sempre que a
/// semana estoura.
const RITMO_SEMANA: i64 = 6 * 3600;
/// Menos que isso entre a primeira e a última leitura do trecho não é ritmo, é ruído.
const MINIMO_5H: i64 = 5 * 60;
const MINIMO_SEMANA: i64 = 30 * 60;
/// A hora da renovação da mesma janela pode variar alguns segundos de uma fonte para a outra.
const MESMA_JANELA: i64 = 10 * 60;
/// Teto de leituras guardadas por janela (com o popover aberto chega uma por minuto).
const TETO: usize = 512;

/// O que a previsão de uma janela diz.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Previsao {
    /// Sem leituras bastantes, leitura velha, janela que acabou de renovar ou já em 100%.
    #[default]
    SemDado,
    /// No ritmo de agora (ou parado), a janela renova antes de encher.
    AteRenovar,
    /// Chega a 100% neste instante (segundos Unix), antes de renovar.
    Estoura(i64),
}

impl Previsao {
    /// A hora em que estoura, se for antes da renovação.
    pub fn estoura(self) -> Option<i64> {
        match self {
            Previsao::Estoura(quando) => Some(quando),
            _ => None,
        }
    }
}

/// As previsões das duas janelas de uma conta.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Previsoes {
    pub cinco_horas: Previsao,
    pub semana: Previsao,
}

/// Uma leitura de uma janela: quando foi medida, a fração usada e quando a janela renova.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Leitura {
    em: i64,
    fracao: f64,
    renova: Option<i64>,
}

/// As últimas leituras das duas janelas de uma conta.
#[derive(Clone, Debug, Default)]
pub struct Historico {
    cinco_horas: VecDeque<Leitura>,
    semana: VecDeque<Leitura>,
}

impl Historico {
    /// Guarda uma leitura CRUA (antes de juntar com o que já se sabia): a janela sem número nela
    /// fica de fora, para uma leitura que só trouxe a semana não virar um ponto parado nas 5 h.
    pub fn registrar(&mut self, lido: &Uso) {
        let Some(em) = lido.medido_em else {
            return;
        };
        guardar(&mut self.cinco_horas, em, lido.cinco_horas, lido.renova_5h, RITMO_5H);
        guardar(&mut self.semana, em, lido.semana, lido.renova_semana, RITMO_SEMANA);
    }

    pub fn prever(&self, agora: i64) -> Previsoes {
        Previsoes {
            cinco_horas: prever(&self.cinco_horas, RITMO_5H, MINIMO_5H, agora),
            semana: prever(&self.semana, RITMO_SEMANA, MINIMO_SEMANA, agora),
        }
    }
}

fn guardar(fila: &mut VecDeque<Leitura>, em: i64, fracao: Option<f64>, renova: Option<i64>, ritmo: i64) {
    let Some(fracao) = fracao else {
        return;
    };
    match fila.back_mut() {
        // A releitura do mesmo arquivo traz a mesma medição; uma mais velha não entra.
        Some(ultima) if ultima.em > em => return,
        Some(ultima) if ultima.em == em => *ultima = Leitura { em, fracao, renova },
        _ => fila.push_back(Leitura { em, fracao, renova }),
    }
    while fila.front().is_some_and(|l| l.em < em - ritmo) || fila.len() > TETO {
        fila.pop_front();
    }
}

fn prever(fila: &VecDeque<Leitura>, ritmo: i64, minimo: i64, agora: i64) -> Previsao {
    let Some(ultima) = fila.back() else {
        return Previsao::SemDado;
    };
    let Some(renova) = ultima.renova else {
        return Previsao::SemDado;
    };
    // Janela que já renovou, já cheia (o alerta do limite fala por ela) ou leitura velha demais
    // para dizer o ritmo de agora.
    if renova <= agora || ultima.fracao >= 1.0 || agora - ultima.em > ritmo {
        return Previsao::SemDado;
    }
    // A leitura mais velha do trecho, da MESMA janela: as de antes da renovação não contam.
    let da_janela = |l: &&Leitura| l.renova.is_some_and(|r| (r - renova).abs() <= MESMA_JANELA);
    let Some(base) = fila.iter().filter(|l| l.em >= agora - ritmo).find(da_janela) else {
        return Previsao::SemDado;
    };
    let tempo = ultima.em - base.em;
    if tempo < minimo {
        return Previsao::SemDado;
    }
    let subiu = ultima.fracao - base.fracao;
    if subiu <= 0.0 {
        return Previsao::AteRenovar;
    }
    let falta = (1.0 - ultima.fracao) * tempo as f64 / subiu;
    // A leitura tem alguns minutos: a hora prevista nunca fica no passado.
    let quando = ultima.em.saturating_add(falta.round() as i64).max(agora);
    if quando < renova { Previsao::Estoura(quando) } else { Previsao::AteRenovar }
}

#[cfg(test)]
mod testes {
    use super::{Historico, Previsao};
    use crate::motor::Uso;

    const T0: i64 = 1_800_000_000;
    const MIN: i64 = 60;
    const HORA: i64 = 3600;

    fn leitura(em: i64, cinco: f64, renova: i64) -> Uso {
        Uso {
            cinco_horas: Some(cinco),
            renova_5h: Some(renova),
            medido_em: Some(em),
            ..Uso::default()
        }
    }

    #[test]
    fn leituras_subindo_dizem_a_hora_em_que_estoura() {
        let renova = T0 + 3 * HORA;
        let mut h = Historico::default();
        // 40% e, 20 minutos depois, 60%: 1% por minuto, faltam 40 minutos.
        h.registrar(&leitura(T0, 0.40, renova));
        h.registrar(&leitura(T0 + 10 * MIN, 0.50, renova));
        h.registrar(&leitura(T0 + 20 * MIN, 0.60, renova));
        assert_eq!(h.prever(T0 + 20 * MIN).cinco_horas, Previsao::Estoura(T0 + 60 * MIN));
        assert_eq!(h.prever(T0 + 20 * MIN).cinco_horas.estoura(), Some(T0 + 60 * MIN));
        // A semana não teve leitura nenhuma.
        assert_eq!(h.prever(T0 + 20 * MIN).semana, Previsao::SemDado);
        // Devagar (1% em 20 minutos, faltam 13 horas): a renovação chega antes.
        let mut devagar = Historico::default();
        devagar.registrar(&leitura(T0, 0.20, renova));
        devagar.registrar(&leitura(T0 + 20 * MIN, 0.21, renova));
        assert_eq!(devagar.prever(T0 + 20 * MIN).cinco_horas, Previsao::AteRenovar);
    }

    #[test]
    fn leituras_paradas_dao_ate_a_renovacao() {
        let renova = T0 + 2 * HORA;
        let mut h = Historico::default();
        h.registrar(&leitura(T0, 0.55, renova));
        h.registrar(&leitura(T0 + 15 * MIN, 0.55, renova));
        assert_eq!(h.prever(T0 + 16 * MIN).cinco_horas, Previsao::AteRenovar);
        // Subiu antes, mas parou há mais tempo que o trecho: só o trecho recente conta.
        let mut h = Historico::default();
        h.registrar(&leitura(T0, 0.10, renova));
        h.registrar(&leitura(T0 + 10 * MIN, 0.55, renova));
        h.registrar(&leitura(T0 + 45 * MIN, 0.55, renova));
        h.registrar(&leitura(T0 + 60 * MIN, 0.55, renova));
        assert_eq!(h.prever(T0 + 60 * MIN).cinco_horas, Previsao::AteRenovar);
    }

    #[test]
    fn janela_renovada_nao_herda_o_ritmo_da_anterior() {
        let renova = T0 + 40 * MIN;
        let mut h = Historico::default();
        h.registrar(&leitura(T0, 0.70, renova));
        h.registrar(&leitura(T0 + 20 * MIN, 0.90, renova));
        assert_eq!(h.prever(T0 + 20 * MIN).cinco_horas, Previsao::Estoura(T0 + 30 * MIN));
        // Passou a hora de renovar e nada novo chegou: não há o que prever.
        assert_eq!(h.prever(T0 + 41 * MIN).cinco_horas, Previsao::SemDado);
        // A primeira leitura da janela nova (2%) não se compara com os 90% da anterior.
        let nova = renova + 5 * HORA;
        h.registrar(&leitura(T0 + 45 * MIN, 0.02, nova));
        assert_eq!(h.prever(T0 + 45 * MIN).cinco_horas, Previsao::SemDado);
        // Com duas leituras da janela nova, o ritmo é só o dela.
        h.registrar(&leitura(T0 + 55 * MIN, 0.03, nova));
        assert_eq!(h.prever(T0 + 55 * MIN).cinco_horas, Previsao::AteRenovar);
    }

    #[test]
    fn sem_dado_com_pouca_leitura_ou_leitura_velha() {
        let renova = T0 + 4 * HORA;
        let mut h = Historico::default();
        assert_eq!(h.prever(T0).cinco_horas, Previsao::SemDado);
        h.registrar(&leitura(T0, 0.40, renova));
        assert_eq!(h.prever(T0).cinco_horas, Previsao::SemDado);
        // Dois minutos entre as leituras não dizem ritmo.
        h.registrar(&leitura(T0 + 2 * MIN, 0.45, renova));
        assert_eq!(h.prever(T0 + 2 * MIN).cinco_horas, Previsao::SemDado);
        h.registrar(&leitura(T0 + 10 * MIN, 0.60, renova));
        assert!(h.prever(T0 + 10 * MIN).cinco_horas.estoura().is_some());
        // Uma hora sem leitura nova: o ritmo de agora não se sabe.
        assert_eq!(h.prever(T0 + 70 * MIN).cinco_horas, Previsao::SemDado);
        // Já em 100%: quem fala é o alerta do limite.
        h.registrar(&leitura(T0 + 12 * MIN, 1.0, renova));
        assert_eq!(h.prever(T0 + 12 * MIN).cinco_horas, Previsao::SemDado);
    }

    #[test]
    fn leitura_sem_o_numero_da_janela_e_releitura_nao_viram_ponto() {
        let renova = T0 + 3 * HORA;
        let mut h = Historico::default();
        h.registrar(&leitura(T0, 0.40, renova));
        h.registrar(&leitura(T0 + 10 * MIN, 0.50, renova));
        // O evento de um turno que só trouxe a semana não achata as 5 h.
        h.registrar(&Uso { semana: Some(0.3), renova_semana: Some(T0 + 99 * HORA), medido_em: Some(T0 + 14 * MIN), ..Uso::default() });
        // A releitura do arquivo (mais velha) e a repetição da última não mudam nada.
        h.registrar(&leitura(T0 + 5 * MIN, 0.99, renova));
        h.registrar(&leitura(T0 + 10 * MIN, 0.50, renova));
        assert_eq!(h.cinco_horas.len(), 2);
        // 1% por minuto, faltam 50 minutos depois da leitura das 0:10.
        assert_eq!(h.prever(T0 + 14 * MIN).cinco_horas, Previsao::Estoura(T0 + 60 * MIN));
        // A previsão nunca fica no passado, mesmo com a leitura um pouco velha.
        let mut rapido = Historico::default();
        rapido.registrar(&leitura(T0, 0.50, renova));
        rapido.registrar(&leitura(T0 + 5 * MIN, 0.95, renova));
        assert_eq!(rapido.prever(T0 + 20 * MIN).cinco_horas, Previsao::Estoura(T0 + 20 * MIN));
    }

    #[test]
    fn a_semana_olha_um_trecho_maior() {
        let renova = T0 + 48 * HORA;
        let semana = |em: i64, f: f64| Uso { semana: Some(f), renova_semana: Some(renova), medido_em: Some(em), ..Uso::default() };
        let mut h = Historico::default();
        h.registrar(&semana(T0, 0.80));
        // 20 minutos ainda é pouco para a semana.
        h.registrar(&semana(T0 + 20 * MIN, 0.81));
        assert_eq!(h.prever(T0 + 20 * MIN).semana, Previsao::SemDado);
        // 2% em 2 horas: os 18% que faltam levam 18 horas, antes das 46 que faltam para renovar.
        h.registrar(&semana(T0 + 2 * HORA, 0.82));
        assert_eq!(h.prever(T0 + 2 * HORA).semana, Previsao::Estoura(T0 + 20 * HORA));
    }
}
