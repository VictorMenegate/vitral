//! Um chat, um processo: os processos que já entregaram o resultado do turno e estão saindo
//! (gravando a sessão, rodando os hooks de fim), guardados POR CHAT até saírem de fato.
//!
//! Antes (até a 0.9.5) eles iam para uma lista sem dono e o motor os esquecia: o turno seguinte
//! abria outro processo da mesma sessão sem conferir se o anterior tinha saído, e o Parar não
//! alcançava mais o antigo. Em 23/11/2026 um chat chegou a ter três processos do Claude Code na
//! mesma sessão, cada um achando que era o único (ver `claude::VezDaMensagem` para o que disparou).
//!
//! As regras, que o motor aplica com o que este módulo responde:
//! - com um processo do chat ainda vivo aqui, o turno novo não abre: espera ele sair (`vivo`);
//! - quem não sai no prazo é derrubado (`vencer`): o prazo normal cobre os 5 s que o Claude Code
//!   dá aos comandos de fundo antes de sair, e o de quem foi parado por você é curto;
//! - o Parar e o fechar do Vitral derrubam também os que estão aqui (`derrubar_do_chat`,
//!   `derrubar_todos`);
//! - o processo que volta a trabalhar sozinho (um aviso de tarefa de fundo abre outro turno nele)
//!   volta a ser o processo do chat, ou é derrubado se foi parado ou se há turno esperando por ele
//!   (`ao_acordar`).

use std::time::{Duration, Instant};

/// Quanto um processo que terminou o turno tem para sair depois de a entrada fechar. Medido no
/// Claude Code 2.1.328: de 0,5 a 1,6 s sem nada no fundo, e uns 7 s com um comando de fundo (ele
/// espera 5 s antes de parar o comando).
pub const PRAZO_DE_SAIDA: Duration = Duration::from_secs(12);

/// O prazo de quem você parou (ou cujo turno deu erro): o Claude Code interrompido sai em 1 s.
pub const PRAZO_DO_PARADO: Duration = Duration::from_secs(5);

/// Quanto o derrubado (SIGTERM, para ele limpar o que abriu) tem para sair antes do SIGKILL.
pub const GRACA_DO_DERRUBADO: Duration = Duration::from_secs(5);

/// De quanto em quanto tempo o turno que espera confere se o processo anterior saiu.
pub const PASSO_DA_ESPERA: Duration = Duration::from_millis(100);

/// O que o registro precisa de um processo (o `claude::Processo`; nos testes, um de mentira).
pub trait Sai {
    /// Ainda roda (não colhe nada além do próprio filho).
    fn vivo(&mut self) -> bool;
    /// SIGTERM nele e em tudo o que abriu.
    fn derrubar(&mut self);
    /// SIGKILL: para o que não saiu nem derrubado.
    fn matar_de_vez(&mut self);
}

impl Sai for crate::claude::Processo {
    fn vivo(&mut self) -> bool {
        self.esperar().is_none()
    }

    fn derrubar(&mut self) {
        self.matar();
    }

    fn matar_de_vez(&mut self) {
        crate::claude::Processo::matar_de_vez(self);
    }
}

/// Um processo que está saindo.
pub struct Saindo<P> {
    pub chat: String,
    /// A geração do turno dele: é por ela que os eventos atrasados o reconhecem.
    pub geracao: u64,
    pub processo: P,
    /// Foi parado por você (ou o turno deu erro): se voltar a trabalhar, é derrubado.
    pub parado: bool,
    limite: Instant,
    /// Quando foi derrubado (o SIGTERM), e se já levou o SIGKILL.
    derrubado: Option<Instant>,
    morto_de_vez: bool,
}

pub struct Saidas<P> {
    lista: Vec<Saindo<P>>,
}

impl<P> Default for Saidas<P> {
    fn default() -> Self {
        Self { lista: Vec::new() }
    }
}

impl<P: Sai> Saidas<P> {
    /// Guarda o processo que acabou de fechar a entrada, até ele sair.
    pub fn guardar(&mut self, chat: &str, geracao: u64, processo: P, parado: bool, agora: Instant) {
        let prazo = if parado { PRAZO_DO_PARADO } else { PRAZO_DE_SAIDA };
        self.lista.push(Saindo { chat: chat.to_string(), geracao, processo, parado, limite: agora + prazo, derrubado: None, morto_de_vez: false });
    }

    /// Esquece os que já saíram.
    pub fn colher(&mut self) {
        self.lista.retain_mut(|s| s.processo.vivo());
    }

    /// O chat ainda tem processo vivo saindo: o turno novo dele espera.
    pub fn vivo(&mut self, chat: &str) -> bool {
        self.colher();
        self.lista.iter().any(|s| s.chat == chat)
    }

    /// Derruba os que passaram do prazo sem sair, e mata de vez o derrubado que não saiu na
    /// graça. Devolve quantos derrubou agora.
    pub fn vencer(&mut self, agora: Instant) -> usize {
        self.colher();
        let mut derrubados = 0;
        for saindo in &mut self.lista {
            match saindo.derrubado {
                None if agora >= saindo.limite => {
                    saindo.processo.derrubar();
                    saindo.derrubado = Some(agora);
                    derrubados += 1;
                }
                Some(quando) if !saindo.morto_de_vez && agora >= quando + GRACA_DO_DERRUBADO => {
                    saindo.processo.matar_de_vez();
                    saindo.morto_de_vez = true;
                }
                _ => {}
            }
        }
        derrubados
    }

    /// O Parar: derruba já os processos deste chat que ainda estão saindo.
    pub fn derrubar_do_chat(&mut self, chat: &str) -> usize {
        self.colher();
        let mut derrubados = 0;
        let agora = Instant::now();
        for saindo in self.lista.iter_mut().filter(|s| s.chat == chat && s.derrubado.is_none()) {
            saindo.processo.derrubar();
            saindo.derrubado = Some(agora);
            derrubados += 1;
        }
        derrubados
    }

    /// Fechar o Vitral: derruba todos.
    pub fn derrubar_todos(&mut self) {
        let agora = Instant::now();
        for saindo in self.lista.iter_mut().filter(|s| s.derrubado.is_none()) {
            saindo.processo.derrubar();
            saindo.derrubado = Some(agora);
        }
    }

    /// O processo do chat com essa geração ainda está aqui, e se foi parado.
    pub fn parado(&self, chat: &str, geracao: u64) -> Option<bool> {
        self.lista.iter().find(|s| s.chat == chat && s.geracao == geracao).map(|s| s.parado)
    }

    /// Tira o processo daqui (o motor vai adotá-lo de volta, ou derrubá-lo).
    pub fn tirar(&mut self, chat: &str, geracao: u64) -> Option<P> {
        let indice = self.lista.iter().position(|s| s.chat == chat && s.geracao == geracao)?;
        Some(self.lista.remove(indice).processo)
    }

    /// Quantos processos ainda estão saindo (para os testes e a conta de fechar).
    #[cfg(test)]
    pub fn quantos(&self) -> usize {
        self.lista.len()
    }
}

/// O que fazer com o processo que já tinha terminado o turno e voltou a trabalhar sozinho.
#[derive(Debug, PartialEq, Eq)]
pub enum Acordou {
    /// Volta a ser o processo do chat: o turno dele aparece, recebe mensagem e o Parar o alcança.
    Adotar,
    /// Você o parou, ou já há turno novo esperando ele sair: não pode continuar por fora.
    Derrubar,
}

pub fn ao_acordar(parado: bool, turno_esperando: bool) -> Acordou {
    if parado || turno_esperando { Acordou::Derrubar } else { Acordou::Adotar }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::{cell::Cell, rc::Rc};

    /// Um processo de mentira: vive até o teste mandar sair, e anota se foi derrubado.
    #[derive(Clone, Default)]
    struct Falso {
        saiu: Rc<Cell<bool>>,
        derrubado: Rc<Cell<bool>>,
        /// Não sai com o SIGTERM: só morto de vez.
        teimoso: bool,
        morto_de_vez: Rc<Cell<bool>>,
    }

    impl Sai for Falso {
        fn vivo(&mut self) -> bool {
            !self.saiu.get()
        }

        fn derrubar(&mut self) {
            self.derrubado.set(true);
            if !self.teimoso {
                self.saiu.set(true);
            }
        }

        fn matar_de_vez(&mut self) {
            self.morto_de_vez.set(true);
            self.saiu.set(true);
        }
    }

    /// O critério do pedido: nunca dois processos do mesmo chat. Enquanto o anterior não sai, o
    /// chat conta como tendo processo (o turno novo espera); os outros chats não esperam por ele.
    #[test]
    fn com_o_processo_anterior_vivo_o_turno_novo_espera() {
        let agora = Instant::now();
        let mut saidas = Saidas::default();
        let antigo = Falso::default();
        saidas.guardar("a", 1, antigo.clone(), false, agora);
        assert!(saidas.vivo("a"), "o processo do chat a ainda não saiu");
        assert!(!saidas.vivo("b"), "outro chat não espera pelo processo do a");
        antigo.saiu.set(true);
        assert!(!saidas.vivo("a"), "saiu: o turno novo pode abrir");
        assert_eq!(saidas.quantos(), 0);
    }

    /// Quem não sai no prazo é derrubado, e só então o turno novo abre: o processo que ficou
    /// preso (ou trabalhando por fora) não segura a conversa para sempre.
    #[test]
    fn quem_nao_sai_no_prazo_e_derrubado() {
        let agora = Instant::now();
        let mut saidas = Saidas::default();
        let (normal, parado) = (Falso::default(), Falso::default());
        saidas.guardar("a", 1, normal.clone(), false, agora);
        saidas.guardar("b", 2, parado.clone(), true, agora);
        assert_eq!(saidas.vencer(agora + Duration::from_secs(1)), 0, "ainda no prazo");
        assert_eq!(saidas.vencer(agora + PRAZO_DO_PARADO), 1, "o parado tem o prazo curto");
        assert!(parado.derrubado.get() && !normal.derrubado.get());
        assert_eq!(saidas.vencer(agora + PRAZO_DE_SAIDA), 1);
        assert!(normal.derrubado.get());
        assert!(!saidas.vivo("a") && !saidas.vivo("b"));
        // Quem saiu sozinho antes do prazo não é derrubado.
        let bom = Falso::default();
        saidas.guardar("c", 3, bom.clone(), false, agora);
        bom.saiu.set(true);
        assert_eq!(saidas.vencer(agora + PRAZO_DE_SAIDA * 2), 0);
        assert!(!bom.derrubado.get());
        // O que não sai nem derrubado leva o SIGKILL depois da graça, e o chat fica livre.
        let teimoso = Falso { teimoso: true, ..Falso::default() };
        saidas.guardar("d", 4, teimoso.clone(), true, agora);
        assert_eq!(saidas.vencer(agora + PRAZO_DO_PARADO), 1);
        assert!(saidas.vivo("d") && !teimoso.morto_de_vez.get(), "ainda na graça");
        saidas.vencer(agora + PRAZO_DO_PARADO + GRACA_DO_DERRUBADO);
        assert!(teimoso.morto_de_vez.get() && !saidas.vivo("d"));
    }

    /// O Parar alcança o processo antigo do chat, e fechar o Vitral derruba todos.
    #[test]
    fn o_parar_e_o_fechar_derrubam_quem_esta_saindo() {
        let agora = Instant::now();
        let mut saidas = Saidas::default();
        let (do_a, do_b, outro_do_b) = (Falso::default(), Falso::default(), Falso::default());
        saidas.guardar("a", 1, do_a.clone(), false, agora);
        saidas.guardar("b", 2, do_b.clone(), false, agora);
        assert_eq!(saidas.derrubar_do_chat("a"), 1);
        assert!(do_a.derrubado.get() && !do_b.derrubado.get(), "só o do chat parado");
        assert_eq!(saidas.derrubar_do_chat("a"), 0, "já foi");
        saidas.guardar("b", 3, outro_do_b.clone(), true, agora);
        saidas.derrubar_todos();
        assert!(do_b.derrubado.get() && outro_do_b.derrubado.get());
    }

    /// O processo que acordou depois do fim: volta a ser o do chat, menos quando foi parado ou
    /// quando já há turno novo esperando ele sair (aí seriam dois trabalhando no mesmo chat).
    #[test]
    fn quem_acorda_depois_do_fim_e_adotado_ou_derrubado() {
        assert_eq!(ao_acordar(false, false), Acordou::Adotar);
        assert_eq!(ao_acordar(true, false), Acordou::Derrubar);
        assert_eq!(ao_acordar(false, true), Acordou::Derrubar);
        let agora = Instant::now();
        let mut saidas = Saidas::default();
        saidas.guardar("a", 7, Falso::default(), true, agora);
        assert_eq!(saidas.parado("a", 7), Some(true));
        assert_eq!(saidas.parado("a", 8), None, "outra geração não é ele");
        assert!(saidas.tirar("a", 7).is_some());
        assert!(!saidas.vivo("a"), "adotado de volta, não está mais saindo");
    }
}
