//! Apagar as cópias isoladas das tarefas que já não vão ser juntadas: com node_modules, .venv e
//! target/, cada uma pode passar de 1 GB no SSD.

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, Tom, TomConfirmacao, aviso, confirmar, contagem},
};
use crate::{git, modelo::EstadoTarefa, motor::Motor, tema::Lucide};

/// Textos da pergunta antes de apagar, montados com o tamanho já medido. O que some (o tamanho,
/// os branches e os pontos de volta) vai nos detalhes.
pub struct Pergunta {
    pub titulo: String,
    pub texto: String,
    pub botao: String,
}

/// Mede o espaço das cópias das tarefas e pergunta antes de apagar (o tamanho é o que convence).
/// O Enter só fecha: apagar pede o clique.
pub fn confirmar_limpeza(
    motor: Entity<Motor>,
    ids: Vec<u64>,
    pergunta: impl Fn(&str) -> Pergunta + 'static,
    window: &mut Window,
    cx: &mut App,
) {
    let (pastas, branches): (Vec<std::path::PathBuf>, Vec<String>) = {
        let m = motor.read(cx);
        let tarefas: Vec<_> = ids.iter().filter_map(|id| m.tarefa(*id)).filter(|t| t.worktree.is_some()).collect();
        (
            tarefas.iter().filter_map(|t| t.worktree.clone()).collect(),
            tarefas.iter().filter_map(|t| t.branch.clone()).collect(),
        )
    };
    if pastas.is_empty() {
        return;
    }
    let medir = cx.background_spawn(async move { pastas.iter().map(|p| git::espaco_ocupado(p)).sum::<u64>() });
    window
        .spawn(cx, async move |cx| {
            let tamanho = git::tamanho(medir.await);
            let Pergunta { titulo, texto, botao } = pergunta(&tamanho);
            let branch = match branches.as_slice() {
                [um] => um.clone(),
                varios => contagem(varios.len(), "branch", "branches"),
            };
            let detalhes = vec![
                (Lucide::HardDrive.into(), tamanho.into(), false),
                (Lucide::GitBranch.into(), branch.into(), false),
                (Lucide::RotateCcwClock.into(), "Os pontos de volta".into(), false),
            ];
            let _ = cx.update(|window, cx| {
                confirmar(
                    Confirmacao {
                        tom: TomConfirmacao::Perigo,
                        icone: Lucide::Trash.into(),
                        titulo: titulo.into(),
                        texto: texto.into(),
                        detalhes,
                        ok: botao.into(),
                        cancelar: "Manter".into(),
                        ok_perigo: true,
                    },
                    move |window, cx| limpar(motor.clone(), ids.clone(), window, cx),
                    None,
                    window,
                    cx,
                );
            });
        })
        .detach();
}

/// Apaga e avisa quantas cópias saíram (ou o que falhou).
fn limpar(motor: Entity<Motor>, ids: Vec<u64>, window: &mut Window, cx: &mut App) {
    let tarefa = motor.update(cx, |m, cx| m.limpar_copias(&ids, cx));
    window
        .spawn(cx, async move |cx| {
            let (apagadas, erros) = tarefa.await;
            let (tom, texto) = aviso_da_limpeza(apagadas, &erros);
            let _ = cx.update(|window, cx| window.push_notification(aviso(tom, texto), cx));
        })
        .detach();
}

/// O aviso depois de apagar: quantas saíram e, se alguma falhou, de qual tarefa e por quê
/// (`erros` vem como "#3: motivo").
fn aviso_da_limpeza(apagadas: usize, erros: &[String]) -> (Tom, String) {
    if erros.is_empty() {
        return match apagadas {
            0 => (Tom::Info, "Nenhuma cópia apagada".into()),
            n => (Tom::Pronto, contagem(n, "cópia apagada", "cópias apagadas")),
        };
    }
    let quais = erros.join("; ");
    if apagadas == 0 {
        let o_que = if erros.len() == 1 { "a cópia da tarefa" } else { "as cópias das tarefas" };
        return (Tom::Erro, format!("Não deu para apagar {o_que} {quais}"));
    }
    (
        Tom::Atencao,
        format!(
            "{}. Não deu para apagar {}: {quais}",
            contagem(apagadas, "cópia apagada", "cópias apagadas"),
            erros.len()
        ),
    )
}

impl Janela {
    /// "Apagar a cópia…" no menu da cópia de uma tarefa parada.
    pub(super) fn perguntar_limpeza(&mut self, tarefa: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let em_revisao = motor
            .read(cx)
            .tarefa(tarefa)
            .is_some_and(|t| t.estado == EstadoTarefa::EmRevisao);
        let pergunta = move |_: &str| Pergunta {
            titulo: format!("Apagar a cópia da tarefa #{tarefa}?"),
            texto: format!(
                "Somem a pasta, o branch e os pontos de volta.{} A conversa do trabalhador fica \
                 para leitura, mas não roda mais.",
                if em_revisao { " A tarefa está em revisão e vira cancelada." } else { "" }
            ),
            botao: "Apagar a cópia".into(),
        };
        confirmar_limpeza(motor, vec![tarefa], pergunta, window, cx);
    }

    /// Ao arquivar um coordenador sem tarefas rodando, oferece apagar as cópias que sobraram. É
    /// uma pergunta à parte: arquivar se desfaz, e uma tarefa em revisão ainda pode ser aprovada.
    pub(super) fn oferecer_limpeza_ao_arquivar(&mut self, coordenador: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let m = motor.read(cx);
        if !m.chat(coordenador).is_some_and(|c| c.arquivado) || m.tarefas_do(coordenador).any(|t| t.estado.ativa()) {
            return;
        }
        let paradas: Vec<(u64, bool)> = m
            .copias_paradas(true)
            .filter(|t| t.coordenador == coordenador)
            .map(|t| (t.id, t.estado == EstadoTarefa::EmRevisao))
            .collect();
        let n = paradas.len();
        let em_revisao = paradas.iter().any(|(_, r)| *r);
        let pergunta = move |_: &str| Pergunta {
            titulo: if n == 1 {
                "Apagar também a cópia da tarefa?".into()
            } else {
                format!("Apagar também as {n} cópias das tarefas?")
            },
            texto: format!(
                "A conversa foi arquivada, mas as tarefas dela ainda guardam {} do projeto.{}",
                if n == 1 { "uma cópia".to_string() } else { format!("{n} cópias") },
                if em_revisao { " Tarefas em revisão serão canceladas." } else { "" }
            ),
            botao: if n == 1 { "Apagar a cópia".into() } else { "Apagar as cópias".into() },
        };
        let ids = paradas.into_iter().map(|(id, _)| id).collect();
        confirmar_limpeza(motor, ids, pergunta, window, cx);
    }
}

#[cfg(test)]
mod testes {
    use super::{Tom, aviso_da_limpeza};

    #[test]
    fn aviso_da_limpeza_diz_quantas_e_o_que_falhou() {
        assert_eq!(aviso_da_limpeza(1, &[]), (Tom::Pronto, "1 cópia apagada".to_string()));
        assert_eq!(aviso_da_limpeza(3, &[]), (Tom::Pronto, "3 cópias apagadas".to_string()));
        let um = vec!["#3: pasta em uso".to_string()];
        assert_eq!(
            aviso_da_limpeza(0, &um),
            (Tom::Erro, "Não deu para apagar a cópia da tarefa #3: pasta em uso".to_string())
        );
        assert_eq!(
            aviso_da_limpeza(2, &um),
            (Tom::Atencao, "2 cópias apagadas. Não deu para apagar 1: #3: pasta em uso".to_string())
        );
        let dois = vec!["#3: pasta em uso".to_string(), "#4: sem permissão".to_string()];
        assert_eq!(
            aviso_da_limpeza(1, &dois),
            (Tom::Atencao, "1 cópia apagada. Não deu para apagar 2: #3: pasta em uso; #4: sem permissão".to_string())
        );
    }
}
