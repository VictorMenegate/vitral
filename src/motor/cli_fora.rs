//! O Codex e o Gemini como programas instalados (e não como turnos): a versão, a conta, o
//! `update` e o `logout`. Mora no motor, e não no diálogo das Configurações, para o `update`
//! sobreviver a fechar a janela no meio e para reabrir não deixar começar outro (como o
//! `atualizar_claude`).

use std::time::Duration;

use gpui_kit::{AppContext as _, Context};

use super::{AlvoDaDupla, Motor, alvo_da_dupla};
use crate::{
    cli_agentes::{self, CliFora},
    modelo::Agente,
};

impl Motor {
    pub fn cli(&self, agente: Agente) -> &CliFora {
        match agente {
            Agente::Gemini => &self.cli_gemini,
            _ => &self.cli_codex,
        }
    }

    fn cli_mut(&mut self, agente: Agente) -> &mut CliFora {
        match agente {
            Agente::Gemini => &mut self.cli_gemini,
            _ => &mut self.cli_codex,
        }
    }

    /// Quantos chats um `update` do programa deste agente pegaria no meio: os que ele está
    /// trabalhando ou com mensagem na fila para ele. Num chat em que ele é o parceiro da dupla,
    /// só quando está revisando o turno do outro ou tem mensagem com o @ dele na fila: antes contava
    /// o chat em que o OUTRO trabalhava, e as Configurações do Codex diziam "1 chat está
    /// trabalhando" sem nenhum chat do Codex rodando (dono, 26/11/2026).
    pub fn turnos_a_caminho_de(&self, agente: Agente) -> usize {
        self.chats
            .iter()
            .filter(|c| {
                if c.agente == agente {
                    self.execucao_real(&c.id).rodando() || self.na_fila(&c.id) > 0
                } else if c.parceiro.as_ref().is_some_and(|p| p.agente == agente) {
                    self.revisando_o_par(&c.id).is_some()
                        || self.filas.get(&c.id).is_some_and(|fila| {
                            fila.iter().any(|e| matches!(alvo_da_dupla(&e.texto), AlvoDaDupla::So(chamado) if chamado == agente))
                        })
                } else {
                    false
                }
            })
            .count()
    }

    /// O agente já entrou na conta (o Codex pelo `login status`, o Gemini pela cota que o `agy`
    /// devolveu).
    fn logado_em(&self, agente: Agente) -> bool {
        match agente {
            Agente::Gemini => self.gemini.is_some(),
            _ => self.cli_codex.conta.as_ref().is_some_and(|c| c.entrou()),
        }
    }

    /// Relê em segundo plano a versão instalada (e, no Codex, a conta): ao abrir a parte do agente
    /// nas Configurações e depois de atualizar, entrar ou sair.
    pub fn reler_cli(&mut self, agente: Agente, cx: &mut Context<Self>) {
        cx.spawn(async move |motor, cx| {
            let (versao, conta) = cx
                .background_spawn(async move {
                    let conta = (agente == Agente::Codex).then(cli_agentes::ler_conta_do_codex).flatten();
                    (cli_agentes::versao_de(agente), conta)
                })
                .await;
            let _ = motor.update(cx, |m, cx| {
                let cli = m.cli_mut(agente);
                let antes = (cli.versao, cli.conta.clone());
                cli.versao = Some(versao);
                if agente == Agente::Codex {
                    cli.conta = conta;
                }
                if (cli.versao, cli.conta.clone()) != antes {
                    cx.notify();
                }
            });
        })
        .detach();
    }

    /// Roda o `codex update` ou o `agy update` em segundo plano e, no fim, guarda a resposta e
    /// relê a versão. Só com o programa instalado.
    pub fn atualizar_cli(&mut self, agente: Agente, cx: &mut Context<Self>) {
        if self.cli(agente).atualizando || !agente.instalado() {
            return;
        }
        let cli = self.cli_mut(agente);
        cli.atualizando = true;
        cli.resposta = None;
        cx.notify();
        cx.spawn(async move |motor, cx| {
            let resposta = cx.background_spawn(async move { cli_agentes::atualizar(agente) }).await;
            let _ = motor.update(cx, |m, cx| {
                let cli = m.cli_mut(agente);
                cli.atualizando = false;
                cli.resposta = Some(resposta);
                m.reler_cli(agente, cx);
                // Versão nova do Codex pode trazer modelo novo.
                if agente == Agente::Codex {
                    m.reler_modelos_do_codex(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// O `codex logout`, em segundo plano; no fim relê a conta e o uso (o anel some).
    pub fn sair_do_codex(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |motor, cx| {
            cx.background_spawn(async move { cli_agentes::sair_do_codex() }).await;
            let _ = motor.update(cx, |m, cx| {
                m.reler_cli(Agente::Codex, cx);
                m.reler_uso(cx);
            });
        })
        .detach();
    }

    /// Depois do "Entrar…" (o login acontece no terminal, fora do Vitral), olha de tempos em
    /// tempos se a conta já entrou, por uns minutos, para a linha da conta acertar sozinha.
    pub fn esperar_login(&mut self, agente: Agente, cx: &mut Context<Self>) {
        cx.spawn(async move |motor, cx| {
            let passo = Duration::from_secs(if agente == Agente::Gemini { 8 } else { 4 });
            for _ in 0..30 {
                cx.background_executor().timer(passo).await;
                let Ok(entrou) = motor.update(cx, |m, cx| {
                    if m.logado_em(agente) {
                        return true;
                    }
                    match agente {
                        Agente::Gemini => m.reler_uso_gemini(cx),
                        _ => m.reler_cli(agente, cx),
                    }
                    false
                }) else {
                    return;
                };
                if entrou {
                    let _ = motor.update(cx, |m, cx| m.reler_uso(cx));
                    return;
                }
            }
        })
        .detach();
    }
}
