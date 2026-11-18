//! A memória de cada chat trabalhando: quanto o agente e tudo o que ele abriu (shells, testes,
//! servidores) ocupam agora, lido do cgroup do escopo do systemd (`Escopo::memoria`). Só no Linux
//! com systemd; no resto, `None`, e nada aparece.
//!
//! A leitura é de quem pergunta (a dica da linha de estado do turno), guardada por alguns
//! segundos: a linha redesenha a cada segundo, e dois arquivos do /proc e do /sys por desenho
//! seriam à toa. Chat parado não tem processo, e não é lido.

use std::{
    cell::RefCell,
    collections::HashMap,
    time::{Duration, Instant},
};

use super::*;

/// Por quanto tempo uma leitura vale.
const VALE_POR: Duration = Duration::from_secs(3);

/// As últimas leituras, por chat.
#[derive(Debug, Default)]
pub struct Leituras(RefCell<HashMap<String, (Instant, Option<u64>)>>);

impl Leituras {
    /// A leitura guardada do chat, se ainda vale; senão, lê com `ler` e guarda.
    fn do_chat(&self, chat: &str, agora: Instant, ler: impl FnOnce() -> Option<u64>) -> Option<u64> {
        let mut leituras = self.0.borrow_mut();
        if let Some((quando, memoria)) = leituras.get(chat)
            && agora.saturating_duration_since(*quando) < VALE_POR
        {
            return *memoria;
        }
        let memoria = ler();
        leituras.insert(chat.to_string(), (agora, memoria));
        memoria
    }

    fn esquecer(&self, chat: &str) {
        self.0.borrow_mut().remove(chat);
    }
}

impl Motor {
    /// A memória que o turno em andamento do chat ocupa, em bytes. `None` com o chat parado, no
    /// Windows e sem systemd.
    pub fn memoria_do_chat(&self, chat: &str) -> Option<u64> {
        let Some(processo) = self.processos.get(chat) else {
            self.memorias.esquecer(chat);
            return None;
        };
        self.memorias.do_chat(chat, Instant::now(), || processo.memoria())
    }
}

#[cfg(test)]
mod testes {
    use super::{Leituras, VALE_POR};
    use std::{
        cell::Cell,
        time::{Duration, Instant},
    };

    #[test]
    fn a_leitura_vale_por_alguns_segundos() {
        let leituras = Leituras::default();
        let vezes = Cell::new(0u64);
        let ler = || {
            vezes.set(vezes.get() + 1);
            Some(vezes.get() * 1_000)
        };
        let inicio = Instant::now();
        assert_eq!(leituras.do_chat("a", inicio, ler), Some(1_000));
        // Um segundo depois, a mesma leitura: nada é lido de novo.
        assert_eq!(leituras.do_chat("a", inicio + Duration::from_secs(1), ler), Some(1_000));
        assert_eq!(vezes.get(), 1);
        // Outro chat tem a leitura dele.
        assert_eq!(leituras.do_chat("b", inicio + Duration::from_secs(1), ler), Some(2_000));
        // Passado o prazo, lê de novo.
        assert_eq!(leituras.do_chat("a", inicio + VALE_POR, ler), Some(3_000));
        // O "sem leitura" também fica guardado (não insiste a cada desenho).
        leituras.esquecer("a");
        assert_eq!(leituras.do_chat("a", inicio, || None), None);
        assert_eq!(leituras.do_chat("a", inicio + Duration::from_secs(1), ler), None);
        assert_eq!(vezes.get(), 3);
    }
}
