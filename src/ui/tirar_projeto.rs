//! Tirar um projeto da frente: arquivar (a marca na configuração da pasta, que se desfaz) ou
//! mandar a pasta dele para a Lixeira do GNOME, com o que o Vitral guarda dele.

use std::path::Path;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, Tom, TomConfirmacao, aviso, aviso_com_acao, confirmar, contagem},
    lateral::milhar,
    trilho::pasta_curta,
};
use crate::{
    git,
    modelo::Papel,
    motor::{self, Motor, NaLixeira, Ocupacao, ResumoDoProjeto},
    tema::Lucide,
};

/// Até quantas entradas a pasta é medida antes da pergunta (o `node_modules` não trava nada).
const LIMITE_DA_MEDIDA: usize = 100_000;

/// Por que o projeto ainda não pode sair: uma tarefa que o Vitral confere, junta ou prepara
/// sozinho (não dá para parar) e, na lixeira, também as conversas trabalhando. Arquivar com
/// conversa trabalhando pergunta antes de parar. Curto: o aviso tem duas linhas.
fn por_que_esperar(projeto: &str, o: &Ocupacao, lixeira: bool) -> Option<String> {
    if !o.tarefas.is_empty() {
        let quais = o.tarefas.iter().map(|t| format!("#{t}")).collect::<Vec<_>>().join(", ");
        let (a, dela) = if o.tarefas.len() == 1 { ("a tarefa", "dela") } else { ("as tarefas", "delas") };
        return Some(format!("Espere {a} {quais} de {projeto}: o Vitral ainda mexe na cópia {dela}."));
    }
    let n = o.conversas.len();
    (lixeira && n > 0).then(|| {
        let pare = if n == 1 { "pare-a" } else { "pare-as" };
        let quantas = contagem(n, "conversa", "conversas");
        format!("{projeto} tem {quantas} trabalhando; {pare} antes de mover para a lixeira.")
    })
}

/// A pergunta antes de parar o que trabalha e arquivar.
fn texto_de_parar_e_arquivar(projeto: &str, n: usize) -> String {
    let (quem, verbo, pronome) = if n == 1 {
        ("1 conversa".to_string(), "está", "ela para")
    } else {
        (format!("{n} conversas"), "estão", "elas param")
    };
    format!(
        "{quem} de {projeto} {verbo} trabalhando. Ao arquivar, {pronome} no meio, como no Parar; \
         depois de desarquivar, dá para retomar."
    )
}

/// O aviso depois de arquivar, com o número de conversas que foram junto para Arquivadas.
fn aviso_de_arquivado(projeto: &str, conversas: usize) -> String {
    match conversas {
        0 => format!("Projeto {projeto} arquivado"),
        n => format!("Projeto {projeto} arquivado · {} em Arquivadas", contagem(n, "conversa", "conversas")),
    }
}

/// O que a pergunta da lixeira diz: o título, o texto, os detalhes (ícone, texto, se vai em
/// vermelho: o que sai de vez) e o botão. `medida` é (arquivos, bytes, se contou tudo); `None`
/// quando a pasta já não existe.
struct PerguntaDaLixeira {
    titulo: String,
    texto: String,
    detalhes: Vec<(Lucide, String, bool)>,
    botao: &'static str,
}

fn pergunta_da_lixeira(
    projeto: &str,
    pasta: &Path,
    resumo: &ResumoDoProjeto,
    medida: Option<(usize, u64, bool)>,
) -> PerguntaDaLixeira {
    let existe = medida.is_some();
    let mut detalhes = vec![(Lucide::FolderOpen, pasta_curta(pasta), false)];
    if let Some((arquivos, bytes, inteira)) = medida {
        let palavra = if arquivos == 1 { "arquivo" } else { "arquivos" };
        detalhes.push((
            Lucide::HardDrive,
            if inteira {
                format!("{} {palavra} · {}", milhar(arquivos), git::tamanho(bytes))
            } else {
                format!("Mais de {} {palavra} · mais de {}", milhar(arquivos), git::tamanho(bytes))
            },
            false,
        ));
    }
    let (c, t) = (resumo.conversas, resumo.trabalhadores);
    if c + t > 0 {
        let quem = match (c, t) {
            (c, 0) => contagem(c, "conversa", "conversas"),
            (0, t) => contagem(t, "trabalhador", "trabalhadores"),
            (c, t) => format!("{} e {}", contagem(c, "conversa", "conversas"), contagem(t, "trabalhador", "trabalhadores")),
        };
        let verbo = if c + t == 1 { "sai" } else { "saem" };
        detalhes.push((Lucide::MessagesSquare, format!("{quem} {verbo} do Vitral, com as mensagens"), true));
    }
    let copias = resumo.copias.len();
    if copias > 0 {
        let quantas = if copias == 1 {
            "1 cópia de tarefa vai para a Lixeira junto".to_string()
        } else {
            format!("{copias} cópias de tarefas vão para a Lixeira junto")
        };
        let branches = if existe { "; os branches ficam no repositório" } else { "" };
        detalhes.push((Lucide::Wrench, format!("{quantas}{branches}"), false));
    }
    if resumo.rotinas > 0 {
        let rotinas = if resumo.rotinas == 1 {
            "1 rotina é apagada".to_string()
        } else {
            format!("{} rotinas são apagadas", resumo.rotinas)
        };
        detalhes.push((Lucide::CalendarClock, rotinas, true));
    }
    if existe {
        PerguntaDaLixeira {
            titulo: format!("Mover {projeto} para a lixeira?"),
            texto: "A pasta vai para a Lixeira, e dá para restaurá-la pelo Arquivos (Lixeira › Restaurar). \
                    O que o Vitral guarda do projeto sai de vez."
                .into(),
            detalhes,
            botao: "Mover para a lixeira",
        }
    } else {
        PerguntaDaLixeira {
            titulo: format!("Tirar {projeto} do Vitral?"),
            texto: format!("A pasta de {projeto} já não existe; o projeto sai só do Vitral, com o que ele guarda."),
            detalhes,
            botao: "Tirar do Vitral",
        }
    }
}

/// O aviso depois da lixeira: o que saiu ou, se a lixeira falhou, que nada foi apagado. O que
/// importa vem primeiro: o aviso corta na segunda linha.
fn aviso_da_lixeira(projeto: &str, resultado: &Result<NaLixeira, String>) -> (Tom, String) {
    let r = match resultado {
        Ok(r) => r,
        Err(erro) => return (Tom::Erro, format!("Nada foi apagado: {projeto} não foi para a lixeira ({erro})")),
    };
    let apagadas = contagem(r.conversas, "conversa apagada", "conversas apagadas");
    let feito = match (r.pasta, r.conversas) {
        (true, 0) => format!("{projeto} foi para a Lixeira"),
        (true, _) => format!("{projeto} na Lixeira · {apagadas}"),
        (false, 0) => format!("{projeto} saiu do Vitral"),
        (false, _) => format!("{projeto} saiu do Vitral · {apagadas}"),
    };
    match r.erros.as_slice() {
        [] => (Tom::Pronto, feito),
        [erro] => (Tom::Atencao, format!("{projeto} na Lixeira, mas 1 cópia ficou no disco: {erro}")),
        erros => (Tom::Atencao, format!("{projeto} na Lixeira, mas {} cópias ficaram no disco: {}", erros.len(), erros.join("; "))),
    }
}

impl Janela {
    /// "Arquivar projeto": arquiva na hora e o aviso tem Desfazer, como as conversas. Com conversa
    /// trabalhando, pergunta antes de parar; com tarefa sendo conferida, juntada ou preparada, não
    /// arquiva e diz por quê.
    pub(super) fn arquivar_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let ocupacao = motor.read(cx).ocupacao_do_projeto(&projeto);
        if let Some(motivo) = por_que_esperar(&projeto, &ocupacao, false) {
            window.push_notification(aviso(Tom::Atencao, motivo), cx);
            return;
        }
        if ocupacao.conversas.is_empty() {
            self.arquivar_agora(&motor, projeto, Vec::new(), window, cx);
            return;
        }
        let janela = cx.entity().downgrade();
        // Parar pede o clique, como no fechar: o Enter fica com o "continuar".
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Atencao,
                icone: Lucide::Archive.into(),
                titulo: format!("Parar e arquivar {projeto}?").into(),
                texto: texto_de_parar_e_arquivar(&projeto, ocupacao.conversas.len()).into(),
                detalhes: Vec::new(),
                ok: "Parar e arquivar".into(),
                cancelar: "Continuar trabalhando".into(),
                ok_perigo: true,
            },
            move |window, cx| {
                let _ = janela.update(cx, |this, cx| {
                    let Some(motor) = this.motor.clone() else {
                        return;
                    };
                    // Com o diálogo aberto, uma tarefa pode ter ido para a conferência.
                    let ocupacao = motor.read(cx).ocupacao_do_projeto(&projeto);
                    if let Some(motivo) = por_que_esperar(&projeto, &ocupacao, false) {
                        window.push_notification(aviso(Tom::Atencao, motivo), cx);
                        return;
                    }
                    this.arquivar_agora(&motor, projeto.clone(), ocupacao.conversas, window, cx);
                });
            },
            None,
            window,
            cx,
        );
    }

    fn arquivar_agora(
        &mut self,
        motor: &Entity<Motor>,
        projeto: String,
        parar: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (conversas, chats) = motor.update(cx, |m, cx| {
            for chat in &parar {
                m.parar(chat, cx);
            }
            m.arquivar_projeto(&projeto, true, cx);
            let do_projeto: Vec<_> = m.chats.iter().filter(|c| c.projeto == projeto).collect();
            (
                do_projeto.iter().filter(|c| c.papel == Papel::Coordenador && !c.arquivado).count(),
                do_projeto.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            )
        });
        self.largar_projeto(&projeto, &chats, None, window, cx);
        let janela = cx.entity().downgrade();
        let nota = aviso_com_acao(Tom::Info, aviso_de_arquivado(&projeto, conversas), "Desfazer", move |_, cx| {
            let _ = janela.update(cx, |this, cx| this.desarquivar_projeto(projeto.clone(), cx));
        });
        window.push_notification(nota, cx);
    }

    /// Desarquivar não avisa: o projeto volta ao trilho na hora.
    pub(super) fn desarquivar_projeto(&mut self, projeto: String, cx: &mut Context<Self>) {
        if let Some(motor) = self.motor.clone() {
            motor.update(cx, |m, cx| m.arquivar_projeto(&projeto, false, cx));
        }
        cx.notify();
    }

    /// "Mover para a lixeira…": com algo rodando no projeto, explica e não faz; senão mede a pasta
    /// e pergunta. O Enter só fecha: mandar para a lixeira pede o clique no botão vermelho.
    pub(super) fn perguntar_lixeira(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let m = motor.read(cx);
        if let Some(motivo) = por_que_esperar(&projeto, &m.ocupacao_do_projeto(&projeto), true) {
            window.push_notification(aviso(Tom::Atencao, motivo), cx);
            return;
        }
        let pasta = match motor::pasta_para_lixeira(m.raiz(), &projeto) {
            Ok(pasta) => pasta,
            Err(erro) => {
                window.push_notification(aviso(Tom::Erro, format!("Não deu para mover para a lixeira: {erro}.")), cx);
                return;
            }
        };
        let resumo = m.resumo_do_projeto(&projeto);
        let medir = {
            let pasta = pasta.clone();
            cx.background_spawn(async move { pasta.is_dir().then(|| git::medir_pasta(&pasta, LIMITE_DA_MEDIDA)) })
        };
        let janela = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let medida = medir.await;
                let p = pergunta_da_lixeira(&projeto, &pasta, &resumo, medida);
                let _ = cx.update(|window, cx| {
                    confirmar(
                        Confirmacao {
                            tom: TomConfirmacao::Perigo,
                            icone: Lucide::Trash.into(),
                            titulo: p.titulo.into(),
                            texto: p.texto.into(),
                            detalhes: p.detalhes.into_iter().map(|(i, t, d)| (i.into(), t.into(), d)).collect(),
                            ok: p.botao.into(),
                            cancelar: "Manter".into(),
                            ok_perigo: true,
                        },
                        move |window, cx| {
                            let _ = janela.update(cx, |this, cx| this.mover_para_lixeira(projeto.clone(), window, cx));
                        },
                        None,
                        window,
                        cx,
                    );
                });
            })
            .detach();
    }

    fn mover_para_lixeira(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let (chats, tarefas) = {
            let m = motor.read(cx);
            motor::do_projeto(&m.chats, &m.tarefas, &projeto)
        };
        let tarefa = motor.update(cx, |m, cx| m.mover_projeto_para_lixeira(&projeto, cx));
        let janela = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let resultado = tarefa.await;
                let _ = cx.update(|window, cx| {
                    if resultado.is_ok() {
                        let _ = janela.update(cx, |this, cx| this.largar_projeto(&projeto, &chats, Some(&tarefas), window, cx));
                    }
                    let (tom, texto) = aviso_da_lixeira(&projeto, &resultado);
                    if let Ok(r) = &resultado {
                        for erro in &r.erros {
                            eprintln!("vitral: cópia de {projeto} não foi para a lixeira: {erro}");
                        }
                    }
                    let nota = if resultado.as_ref().is_ok_and(|r| r.pasta && r.erros.is_empty()) {
                        aviso_com_acao(tom, texto, "Abrir a Lixeira", |_, _| crate::sistema::abrir_lixeira())
                    } else {
                        aviso(tom, texto)
                    };
                    window.push_notification(nota, cx);
                });
            })
            .detach();
    }

    /// Esquece o que a janela guarda do projeto que saiu da vista: o filtro e o arquivo aberto, e
    /// fecha os painéis das conversas dele (o último fica vazio). Com `saiu` (as tarefas que
    /// saíram do estado com ele), também os rascunhos, as revisões, os arquivos e o git dele.
    fn largar_projeto(
        &mut self,
        projeto: &str,
        chats: &[String],
        saiu: Option<&[u64]>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.projeto_filtro.as_deref() == Some(projeto) {
            self.projeto_filtro = None;
        }
        if self.arquivo_aberto.as_ref().is_some_and(|(p, _)| p == projeto) {
            self.arquivo_aberto = None;
            *self.arquivo_lido.borrow_mut() = None;
        }
        let fechar: Vec<u64> = self
            .paineis
            .iter()
            .filter(|p| p.chat.as_ref().is_some_and(|c| chats.contains(c)))
            .map(|p| p.id)
            .collect();
        for id in fechar {
            if self.paineis.len() > 1 {
                self.fechar_painel(id, cx);
            } else if let Some(ix) = self.paineis.iter().position(|p| p.id == id) {
                self.ativo = ix;
                self.selecionar(None, window, cx);
            }
        }
        if self.selecionado.as_ref().is_some_and(|c| chats.contains(c)) {
            self.selecionado = None;
        }
        if self.realce_lista.as_ref().is_some_and(|c| chats.contains(c)) {
            self.realce_lista = None;
        }
        if let Some(tarefas) = saiu {
            for chat in chats {
                self.rascunhos.remove(chat);
                self.anexos.remove(chat);
                self.ultrathink.remove(chat);
                self.ultracode.remove(chat);
                self.familias_abertas.remove(chat);
            }
            self.revisoes.retain(|t, _| !tarefas.contains(t));
            self.cache_arquivos.borrow_mut().remove(projeto);
            self.arvores.remove(projeto);
            self.git_projetos.remove(projeto);
            self.git_consultas.remove(projeto);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use std::path::{Path, PathBuf};

    use super::{Tom, aviso_da_lixeira, aviso_de_arquivado, pergunta_da_lixeira, por_que_esperar, texto_de_parar_e_arquivar};
    use crate::motor::{NaLixeira, Ocupacao, ResumoDoProjeto};

    #[test]
    fn so_espera_o_que_nao_da_para_parar() {
        let livre = Ocupacao::default();
        assert_eq!(por_que_esperar("loja", &livre, false), None);
        assert_eq!(por_que_esperar("loja", &livre, true), None);
        // Conversa trabalhando: arquivar pergunta (sem motivo aqui); a lixeira não deixa.
        let conversando = Ocupacao { conversas: vec!["c1".into(), "w2".into()], tarefas: Vec::new() };
        assert_eq!(por_que_esperar("loja", &conversando, false), None);
        assert_eq!(
            por_que_esperar("loja", &conversando, true).unwrap(),
            "loja tem 2 conversas trabalhando; pare-as antes de mover para a lixeira."
        );
        let uma = Ocupacao { conversas: vec!["c1".into()], tarefas: Vec::new() };
        assert_eq!(
            por_que_esperar("loja", &uma, true).unwrap(),
            "loja tem 1 conversa trabalhando; pare-a antes de mover para a lixeira."
        );
        // A tarefa que o Vitral confere, junta ou prepara vence: essa não dá para parar.
        let juntando = Ocupacao { conversas: vec!["c1".into()], tarefas: vec![4] };
        assert_eq!(por_que_esperar("loja", &juntando, false).unwrap(), "Espere a tarefa #4 de loja: o Vitral ainda mexe na cópia dela.");
        let duas = Ocupacao { conversas: Vec::new(), tarefas: vec![4, 7] };
        assert_eq!(por_que_esperar("loja", &duas, true).unwrap(), "Espere as tarefas #4, #7 de loja: o Vitral ainda mexe na cópia delas.");
    }

    #[test]
    fn textos_de_arquivar() {
        assert_eq!(aviso_de_arquivado("site", 0), "Projeto site arquivado");
        assert_eq!(aviso_de_arquivado("site", 3), "Projeto site arquivado · 3 conversas em Arquivadas");
        assert!(texto_de_parar_e_arquivar("loja", 1).starts_with("1 conversa de loja está trabalhando. Ao arquivar, ela para"));
        assert!(texto_de_parar_e_arquivar("loja", 2).starts_with("2 conversas de loja estão trabalhando. Ao arquivar, elas param"));
    }

    fn resumo(conversas: usize, trabalhadores: usize, copias: usize, rotinas: usize) -> ResumoDoProjeto {
        ResumoDoProjeto {
            conversas,
            trabalhadores,
            copias: (0..copias).map(|i| PathBuf::from(format!("/p/.vitral/worktrees/loja/t{i}"))).collect(),
            rotinas,
        }
    }

    #[test]
    fn a_pergunta_diz_o_caminho_o_tamanho_e_o_que_vai_junto() {
        let pasta = Path::new("/p/loja");
        let p = pergunta_da_lixeira("loja", pasta, &resumo(5, 8, 3, 2), Some((1234, 5 * 1024 * 1024, true)));
        assert_eq!(p.titulo, "Mover loja para a lixeira?");
        assert!(p.texto.contains("Lixeira › Restaurar"));
        assert_eq!(p.botao, "Mover para a lixeira");
        let linhas: Vec<(&str, bool)> = p.detalhes.iter().map(|(_, t, d)| (t.as_str(), *d)).collect();
        assert_eq!(
            linhas,
            [
                ("/p/loja", false),
                ("1.234 arquivos · 5,0 MB", false),
                ("5 conversas e 8 trabalhadores saem do Vitral, com as mensagens", true),
                ("3 cópias de tarefas vão para a Lixeira junto; os branches ficam no repositório", false),
                ("2 rotinas são apagadas", true),
            ]
        );
        // Medida cortada, uma conversa só, nada mais.
        let p = pergunta_da_lixeira("loja", pasta, &resumo(1, 0, 0, 0), Some((100_000, 3 * 1024 * 1024 * 1024, false)));
        let linhas: Vec<&str> = p.detalhes.iter().map(|(_, t, _)| t.as_str()).collect();
        assert_eq!(linhas, ["/p/loja", "Mais de 100.000 arquivos · mais de 3,0 GB", "1 conversa sai do Vitral, com as mensagens"]);
        // A pasta já não existe: sai só do Vitral, e as cópias não falam de branches.
        let p = pergunta_da_lixeira("loja", pasta, &resumo(0, 0, 1, 1), None);
        assert_eq!((p.titulo.as_str(), p.botao), ("Tirar loja do Vitral?", "Tirar do Vitral"));
        let linhas: Vec<&str> = p.detalhes.iter().map(|(_, t, _)| t.as_str()).collect();
        assert_eq!(linhas, ["/p/loja", "1 cópia de tarefa vai para a Lixeira junto", "1 rotina é apagada"]);
    }

    #[test]
    fn o_aviso_diz_o_que_saiu_ou_que_nada_foi_apagado() {
        let feito = |pasta, conversas, erros: &[&str]| {
            Ok(NaLixeira { pasta, conversas, erros: erros.iter().map(|e| e.to_string()).collect() })
        };
        assert_eq!(aviso_da_lixeira("loja", &feito(true, 5, &[])), (Tom::Pronto, "loja na Lixeira · 5 conversas apagadas".into()));
        assert_eq!(aviso_da_lixeira("loja", &feito(true, 0, &[])), (Tom::Pronto, "loja foi para a Lixeira".into()));
        assert_eq!(aviso_da_lixeira("loja", &feito(false, 1, &[])), (Tom::Pronto, "loja saiu do Vitral · 1 conversa apagada".into()));
        assert_eq!(
            aviso_da_lixeira("loja", &feito(true, 1, &["/c/t4: sem permissão"])),
            (Tom::Atencao, "loja na Lixeira, mas 1 cópia ficou no disco: /c/t4: sem permissão".into())
        );
        assert_eq!(
            aviso_da_lixeira("loja", &feito(true, 1, &["/c/t4: a", "/c/t5: b"])).1,
            "loja na Lixeira, mas 2 cópias ficaram no disco: /c/t4: a; /c/t5: b"
        );
        assert_eq!(
            aviso_da_lixeira("loja", &Err("a lixeira não aceitou".into())),
            (Tom::Erro, "Nada foi apagado: loja não foi para a lixeira (a lixeira não aceitou)".into())
        );
    }
}
