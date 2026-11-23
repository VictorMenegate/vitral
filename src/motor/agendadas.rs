//! Mensagens com hora marcada: "Mandar quando a janela de 5 h renovar" e "Mandar às…".
//!
//! A mensagem fica na fila do chat (`Motor::filas`, gravada no estado: sobrevive ao reinício) com
//! a hora em `Entrada::agendada`. Enquanto a hora não chega, ela não entra nos turnos: quem tira
//! mensagens da fila para um turno passa por `tirar_prontas`, que deixa as futuras onde estão. A
//! rotina de 15 s do motor (`entregar_agendadas`) abre o turno do chat livre quando a hora de uma
//! chega; com o chat ocupado, ela entra no fim do turno, como qualquer outra da fila. A hora já
//! passada na abertura do Vitral sai na primeira volta da rotina.
//!
//! O ×, o lápis, a ordem e o "Mandar agora" da fila valem para ela como para as outras; o
//! "Entregar agora" do chat parado manda tudo, com hora ou sem (`entregar_fila_agora`).

use super::*;

/// Depois da hora em que a janela renova, para o plano já ter renovado de fato (a mesma folga do
/// "Continuar às 14:20").
const FOLGA_DA_RENOVACAO: u64 = 30;

/// A mensagem ainda espera a hora dela.
fn no_futuro(entrada: &Entrada, agora: u64) -> bool {
    entrada.agendada.is_some_and(|quando| quando > agora)
}

/// Tira da fila o que pode entrar num turno agora: as mensagens sem hora e as de hora já passada
/// (que saem sem a hora: daqui em diante são mensagens comuns). As de hora futura ficam. `None`,
/// com a fila intacta, quando nada do que está pronto abre um turno (só notas, ou só futuras).
pub fn separar_prontas(fila: &mut Vec<Entrada>, agora: u64) -> Option<Vec<Entrada>> {
    if !fila.iter().any(|e| !e.nota && !no_futuro(e, agora)) {
        return None;
    }
    let (futuras, mut prontas): (Vec<Entrada>, Vec<Entrada>) = std::mem::take(fila).into_iter().partition(|e| no_futuro(e, agora));
    *fila = futuras;
    for entrada in &mut prontas {
        entrada.agendada = None;
    }
    Some(prontas)
}

/// Os chats com mensagem cuja hora já chegou.
pub fn vencidas(filas: &Filas, agora: u64) -> Vec<String> {
    let mut chats: Vec<String> = filas
        .iter()
        .filter(|(_, fila)| fila.iter().any(|e| e.agendada.is_some_and(|quando| quando <= agora)))
        .map(|(chat, _)| chat.clone())
        .collect();
    chats.sort();
    chats
}

impl Motor {
    /// A mensagem do usuário com hora marcada: vai para a fila do chat, com a hora, sem dirigir o
    /// turno que roda nem abrir um. Texto vazio sem anexo não agenda nada.
    pub fn agendar_do_usuario(
        &mut self,
        chat: &str,
        texto: String,
        anexos: Vec<PathBuf>,
        reforco: Reforco,
        quando: u64,
        cx: &mut Context<Self>,
    ) {
        let texto = texto.trim().to_string();
        if (texto.is_empty() && anexos.is_empty()) || self.chat_gravado(chat).is_none() {
            return;
        }
        // O título sai da primeira mensagem, como no envio comum.
        if let Some(c) = self.chat_mut(chat)
            && c.itens.is_empty()
            && c.titulo == "Nova conversa"
            && !texto.starts_with('/')
        {
            c.titulo = titulo_de(&texto);
        }
        let entrada = Entrada { agendada: Some(quando), ..entrada_do_usuario(texto, anexos, reforco) };
        self.filas.entry(chat.to_string()).or_default().push(entrada);
        self.salvar();
        cx.notify();
    }

    /// O que a fila do chat tem para um turno agora (ver `separar_prontas`).
    pub(super) fn tirar_prontas(&mut self, chat: &str) -> Option<Vec<Entrada>> {
        let fila = self.filas.get_mut(chat)?;
        let prontas = separar_prontas(fila, agora());
        if fila.is_empty() {
            self.filas.remove(chat);
        }
        prontas
    }

    /// A rotina: os chats livres com mensagem na hora começam o turno dela. Ocupado, a mensagem
    /// entra quando o turno acabar.
    pub(super) fn entregar_agendadas(&mut self, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() || self.falha_gravacao.is_some() {
            return;
        }
        for chat in vencidas(&self.filas, agora()) {
            if self.chat_livre(&chat) {
                self.entregar_fila(&chat, cx);
            }
        }
    }

    /// "Entregar agora", na fila do chat parado: tudo entra já, as com hora marcada também.
    pub fn entregar_fila_agora(&mut self, chat: &str, cx: &mut Context<Self>) {
        if let Some(fila) = self.filas.get_mut(chat) {
            for entrada in fila {
                entrada.agendada = None;
            }
        }
        // O "Entregar agora" num chat arquivado: o arquivado segurava a fila, e o botão não fazia
        // nada (dono, 19/11/2026: "o entregar agora não vai"). Ele sai das Arquivadas, e o
        // desarquivar já entrega o que esperava.
        if self.chat_gravado(chat).is_some_and(|c| c.arquivado) {
            self.alternar_arquivado(chat, cx);
            return;
        }
        self.entregar_fila(chat, cx);
    }

    /// Quando mandar a mensagem "quando a janela de 5 h renovar": a renovação da conta em que o
    /// chat roda (ou a do Codex), com a folga. `None` sem a hora lida ou com ela já passada.
    pub fn renovacao_do_chat(&self, chat: &str) -> Option<u64> {
        let c = self.chat_gravado(chat)?;
        let renova = match c.agente {
            Agente::Codex => self.codex.as_ref()?.uso.renova_5h,
            Agente::Claude => self.uso_da_conta(&self.conta_para(c).id).renova_5h,
        }?;
        let renova = u64::try_from(renova).ok().filter(|r| *r > agora())?;
        Some(renova + FOLGA_DA_RENOVACAO)
    }
}

#[cfg(test)]
mod testes {
    use super::{separar_prontas, vencidas};
    use crate::modelo::{Entrada, Filas};

    fn entrada(texto: &str, agendada: Option<u64>) -> Entrada {
        Entrada { texto: texto.into(), agendada, ..Default::default() }
    }

    fn textos(fila: &[Entrada]) -> Vec<&str> {
        fila.iter().map(|e| e.texto.as_str()).collect()
    }

    #[test]
    fn a_mensagem_com_hora_futura_fica_e_as_outras_entram() {
        let agora = 1_000;
        let mut fila = vec![entrada("comum", None), entrada("às 15h", Some(agora + 600)), entrada("outra", None)];
        let prontas = separar_prontas(&mut fila, agora).unwrap();
        assert_eq!(textos(&prontas), ["comum", "outra"]);
        assert_eq!(textos(&fila), ["às 15h"]);
        assert_eq!(fila[0].agendada, Some(agora + 600));
        // Só a futura na fila: nada entra, e ela continua lá.
        assert_eq!(separar_prontas(&mut fila, agora).map(|p| p.len()), None);
        assert_eq!(textos(&fila), ["às 15h"]);
        // Chegou a hora: ela entra, e sai sem a hora (vira uma mensagem comum).
        let prontas = separar_prontas(&mut fila, agora + 600).unwrap();
        assert_eq!((textos(&prontas), prontas[0].agendada), (vec!["às 15h"], None));
        assert!(fila.is_empty());
    }

    #[test]
    fn a_hora_ja_passada_na_abertura_envia_logo() {
        // O Vitral ficou fechado além da hora marcada: ao abrir, a mensagem já está vencida.
        let abertura = 50_000;
        let mut filas = Filas::new();
        filas.insert("a".into(), vec![entrada("era para ontem", Some(abertura - 3_600))]);
        filas.insert("b".into(), vec![entrada("ainda não", Some(abertura + 120))]);
        filas.insert("c".into(), vec![entrada("sem hora", None)]);
        // Só o chat com a hora vencida começa sozinho; a fila comum continua esperando você.
        assert_eq!(vencidas(&filas, abertura), ["a"]);
        let prontas = separar_prontas(filas.get_mut("a").unwrap(), abertura).unwrap();
        assert_eq!(textos(&prontas), ["era para ontem"]);
        // Entregue, ela não vence de novo; dois minutos depois é a vez da outra.
        assert!(vencidas(&filas, abertura).is_empty());
        assert_eq!(vencidas(&filas, abertura + 120), ["b"]);
    }

    #[test]
    fn nota_sozinha_nao_abre_turno_nem_com_a_fila_agendada() {
        let agora = 1_000;
        let nota = Entrada { nota: true, ..entrada("nota", None) };
        let mut fila = vec![nota, entrada("depois", Some(agora + 60))];
        assert!(separar_prontas(&mut fila, agora).is_none());
        assert_eq!(fila.len(), 2);
        // Na hora, a nota vai junto com a mensagem.
        let prontas = separar_prontas(&mut fila, agora + 60).unwrap();
        assert_eq!(textos(&prontas), ["nota", "depois"]);
    }
}
