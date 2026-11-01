//! Tirar um projeto da frente: arquivar (a marca no projeto, que se desfaz) ou apagar o projeto
//! do Vitral, com as conversas dele. Apagar nunca mexe nas pastas ligadas: só a pasta própria vai
//! para a Lixeira do GNOME, e só quando o projeto não tem nenhuma pasta ligada.

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
    modelo::{Cartao, Chat, EstadoTarefa, Tarefa},
    motor::{self, Apagado, Motor, Ocupacao, ResumoDoProjeto},
    tema::Lucide,
};

/// Até quantas entradas a pasta é medida antes da pergunta (o `node_modules` não trava nada).
const LIMITE_DA_MEDIDA: usize = 100_000;

/// Por que o projeto ainda não pode sair: um chat que o Vitral confere, junta ou prepara
/// sozinho (não dá para parar) e, ao apagar, também as conversas trabalhando. Arquivar com
/// conversa trabalhando pergunta antes de parar. Curto: o aviso tem duas linhas.
fn por_que_esperar(projeto: &str, o: &Ocupacao, apagar: bool) -> Option<String> {
    if !o.tarefas.is_empty() {
        let quais = o.tarefas.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(", ");
        let (o_chat, dele) = if o.tarefas.len() == 1 { ("o chat", "dele") } else { ("os chats", "deles") };
        return Some(format!("Espere {o_chat} {quais} de {projeto}: o Vitral ainda mexe na cópia {dele}."));
    }
    let n = o.conversas.len();
    (apagar && n > 0).then(|| {
        let pare = if n == 1 { "pare-a" } else { "pare-as" };
        let quantas = contagem(n, "conversa", "conversas");
        format!("{projeto} tem {quantas} trabalhando; {pare} antes de apagar o projeto.")
    })
}

/// O que o projeto tem em andamento na hora de arquivar: os chats trabalhando (o "Parar e
/// arquivar" para), as tarefas que o Vitral mexe sozinho (não dá para parar: espera), os pedidos
/// abertos de quem é de fora do projeto (o Mestre), que são cancelados, e as divisões do Mestre que
/// esperam o Começar com parte nele, de onde a parte sai.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Ocupado {
    pub parar: Vec<String>,
    pub esperar: Vec<u64>,
    pub pedidos: Vec<u64>,
    pub divisoes: Vec<u64>,
}

impl Ocupado {
    /// Nada para parar, nenhum pedido de fora nem divisão: arquiva na hora (com Desfazer).
    fn livre(&self) -> bool {
        self.parar.is_empty() && self.esperar.is_empty() && self.pedidos.is_empty() && self.divisoes.is_empty()
    }
}

pub(super) fn projeto_ocupado(projeto: &str, ocupacao: &Ocupacao, chats: &[Chat], tarefas: &[Tarefa], cartoes: &[Cartao]) -> Ocupado {
    let do_projeto: Vec<&str> = chats.iter().filter(|c| c.projeto == projeto).map(|c| c.id.as_str()).collect();
    let pedidos = tarefas
        .iter()
        .filter(|t| t.projeto == projeto && (t.estado.ativa() || t.estado == EstadoTarefa::EmRevisao))
        .filter(|t| t.dono().is_some_and(|d| !do_projeto.contains(&d)))
        .map(|t| t.id)
        .collect();
    let divisoes = cartoes.iter().filter(|c| c.aberto() && motor::cartao_com_parte_em(c, projeto)).map(|c| c.id).collect();
    Ocupado { parar: ocupacao.conversas.clone(), esperar: ocupacao.tarefas.clone(), pedidos, divisoes }
}

/// As linhas da pergunta sobre o que é do Mestre: os pedidos cancelados e as partes que saem.
fn detalhes_do_mestre(projeto: &str, o: &Ocupado) -> Vec<String> {
    let mut linhas = Vec::new();
    if !o.pedidos.is_empty() {
        let quais = o.pedidos.iter().map(|t| format!("chat {t}")).collect::<Vec<_>>().join(", ");
        let (n, verbo) = (o.pedidos.len(), if o.pedidos.len() == 1 { "é cancelado" } else { "são cancelados" });
        linhas.push(format!("{} do Mestre ({quais}) {verbo}; a conversa e a cópia ficam", contagem(n, "pedido aberto", "pedidos abertos")));
    }
    if !o.divisoes.is_empty() {
        let quais = o.divisoes.iter().map(u64::to_string).collect::<Vec<_>>().join(", ");
        let divisao = if o.divisoes.len() == 1 { "divisão" } else { "divisões" };
        linhas.push(format!("A parte de {projeto} sai da {divisao} {quais} do Mestre"));
    }
    linhas
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

/// O que a pergunta de apagar diz: o título, o texto, os detalhes (ícone, texto, se vai em
/// vermelho: o que sai de vez) e o botão. `medida` é a da pasta própria que vai para a Lixeira
/// (arquivos, bytes, se contou tudo).
struct PerguntaDeApagar {
    titulo: String,
    texto: String,
    detalhes: Vec<(Lucide, String, bool)>,
}

fn pergunta_de_apagar(
    projeto: &str,
    resumo: &ResumoDoProjeto,
    vai_para_a_lixeira: Option<&Path>,
    medida: Option<(usize, u64, bool)>,
) -> PerguntaDeApagar {
    let mut detalhes: Vec<(Lucide, String, bool)> = resumo
        .ligadas
        .iter()
        .map(|p| (Lucide::FolderOpen, format!("{} fica como está", pasta_curta(p)), false))
        .collect();
    let (c, t) = (resumo.conversas, resumo.trabalhadores);
    if c + t > 0 {
        let quem = match (c, t) {
            (c, 0) => contagem(c, "conversa", "conversas"),
            (0, t) => contagem(t, "ajudante", "ajudantes"),
            (c, t) => format!("{} e {}", contagem(c, "conversa", "conversas"), contagem(t, "ajudante", "ajudantes")),
        };
        let verbo = if c + t == 1 { "sai" } else { "saem" };
        detalhes.push((Lucide::MessagesSquare, format!("{quem} {verbo} do Vitral, com as mensagens"), true));
    }
    let copias = resumo.copias.len();
    if copias > 0 {
        let quantas = if copias == 1 {
            "1 cópia de chat vai para a Lixeira".to_string()
        } else {
            format!("{copias} cópias de chats vão para a Lixeira")
        };
        detalhes.push((Lucide::Wrench, quantas, false));
    }
    if resumo.branches > 0
        && let Some(onde) = resumo.ligadas.first()
    {
        let quantos = if resumo.branches == 1 {
            "1 branch dos chats fica".to_string()
        } else {
            format!("{} branches dos chats ficam", resumo.branches)
        };
        detalhes.push((Lucide::GitBranch, format!("{quantos} em {}", pasta_curta(onde)), false));
    }
    if resumo.rotinas > 0 {
        let rotinas = if resumo.rotinas == 1 {
            "1 rotina é apagada".to_string()
        } else {
            format!("{} rotinas são apagadas", resumo.rotinas)
        };
        detalhes.push((Lucide::CalendarClock, rotinas, true));
    }
    if resumo.cartoes > 0 {
        let cartoes = contagem(resumo.cartoes, "cartão do Mestre perde", "cartões do Mestre perdem");
        detalhes.push((Lucide::Sparkles, format!("{cartoes} a parte deste projeto"), false));
    }
    match (vai_para_a_lixeira, &resumo.propria) {
        (Some(pasta), _) => {
            detalhes.push((Lucide::FolderOpen, format!("{} vai para a Lixeira", pasta_curta(pasta)), true));
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
        }
        (None, Some(propria)) => {
            detalhes.push((Lucide::FolderOpen, format!("A pasta própria {} fica no disco", pasta_curta(propria)), false));
        }
        (None, None) => {}
    }
    PerguntaDeApagar {
        titulo: format!("Apagar o projeto {projeto}?"),
        texto: "O projeto e as conversas dele saem do Vitral de vez. As pastas ligadas ficam como estão.".into(),
        detalhes,
    }
}

/// O aviso depois de apagar: o que saiu ou, se a pasta própria não foi para a Lixeira, que nada
/// foi apagado. O que importa vem primeiro: o aviso corta na segunda linha.
fn aviso_de_apagado(projeto: &str, resultado: &Result<Apagado, String>) -> (Tom, String) {
    let r = match resultado {
        Ok(r) => r,
        Err(erro) => return (Tom::Erro, format!("Nada foi apagado: {erro}")),
    };
    let feito = match r.conversas {
        0 => format!("{projeto} apagado"),
        n => format!("{projeto} apagado · {}", contagem(n, "conversa apagada", "conversas apagadas")),
    };
    match r.erros.as_slice() {
        [] => (Tom::Pronto, feito),
        [erro] => (Tom::Atencao, format!("{projeto} apagado, mas 1 cópia ficou no disco: {erro}")),
        erros => (Tom::Atencao, format!("{projeto} apagado, mas {} cópias ficaram no disco: {}", erros.len(), erros.join("; "))),
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
        let ocupado = {
            let m = motor.read(cx);
            projeto_ocupado(&projeto, &m.ocupacao_do_projeto(&projeto), &m.chats, &m.tarefas, m.cartoes())
        };
        let ocupacao = Ocupacao { conversas: ocupado.parar.clone(), tarefas: ocupado.esperar.clone() };
        let nome = motor.read(cx).nome_do_projeto(&projeto);
        if let Some(motivo) = por_que_esperar(&nome, &ocupacao, false) {
            window.push_notification(aviso(Tom::Atencao, motivo), cx);
            return;
        }
        if ocupado.livre() {
            self.arquivar_agora(&motor, projeto, Vec::new(), window, cx);
            return;
        }
        let janela = cx.entity().downgrade();
        let detalhes = detalhes_do_mestre(&nome, &ocupado)
            .into_iter()
            .map(|texto| (Lucide::Sparkles.into(), texto.into(), true))
            .collect();
        let (titulo, texto, ok) = if ocupado.parar.is_empty() {
            (
                format!("Arquivar {nome}?"),
                "Os chats dele vão para Arquivadas, sem perder nada.".to_string(),
                "Arquivar",
            )
        } else {
            (format!("Parar e arquivar {nome}?"), texto_de_parar_e_arquivar(&nome, ocupado.parar.len()), "Parar e arquivar")
        };
        // Parar pede o clique, como no fechar: o Enter fica com o "continuar".
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Atencao,
                icone: Lucide::Archive.into(),
                titulo: titulo.into(),
                texto: texto.into(),
                detalhes,
                ok: ok.into(),
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
                    if let Some(motivo) = por_que_esperar(&nome, &ocupacao, false) {
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
        let nome = motor.read(cx).nome_do_projeto(&projeto);
        let (conversas, chats) = motor.update(cx, |m, cx| {
            m.parar_e_arquivar(&projeto, &parar, cx);
            let do_projeto: Vec<_> = m.chats.iter().filter(|c| c.projeto == projeto).collect();
            (
                do_projeto.iter().filter(|c| c.papel.na_lista() && !c.arquivado).count(),
                do_projeto.iter().map(|c| c.id.clone()).collect::<Vec<_>>(),
            )
        });
        self.largar_projeto(&projeto, &chats, None, window, cx);
        let janela = cx.entity().downgrade();
        let nota = aviso_com_acao(Tom::Info, aviso_de_arquivado(&nome, conversas), "Desfazer", move |_, cx| {
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

    /// "Apagar projeto…": com algo rodando no projeto, explica e não faz; senão (medindo a pasta
    /// própria, quando ela vai para a Lixeira) pergunta. O Enter só fecha: apagar pede o clique no
    /// botão vermelho.
    pub(super) fn perguntar_apagar(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let m = motor.read(cx);
        let Some(p) = m.projeto(&projeto) else {
            return;
        };
        let nome = p.nome.clone();
        if let Some(motivo) = por_que_esperar(&nome, &m.ocupacao_do_projeto(&projeto), true) {
            window.push_notification(aviso(Tom::Atencao, motivo), cx);
            return;
        }
        let propria = match motor::pasta_propria_para_lixeira(p, &m.dados.pasta_projetos()) {
            Ok(propria) => propria,
            Err(erro) => {
                window.push_notification(aviso(Tom::Erro, format!("Não deu para apagar: {erro}.")), cx);
                return;
            }
        };
        let resumo = m.resumo_do_projeto(&projeto);
        let medir = {
            let propria = propria.clone();
            cx.background_spawn(async move {
                propria.filter(|p| p.is_dir()).map(|p| git::medir_pasta(&p, LIMITE_DA_MEDIDA))
            })
        };
        let janela = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let medida = medir.await;
                let p = pergunta_de_apagar(&nome, &resumo, propria.as_deref(), medida);
                let _ = cx.update(|window, cx| {
                    confirmar(
                        Confirmacao {
                            tom: TomConfirmacao::Perigo,
                            icone: Lucide::Trash.into(),
                            titulo: p.titulo.into(),
                            texto: p.texto.into(),
                            detalhes: p.detalhes.into_iter().map(|(i, t, d)| (i.into(), t.into(), d)).collect(),
                            ok: "Apagar projeto".into(),
                            cancelar: "Manter".into(),
                            ok_perigo: true,
                        },
                        move |window, cx| {
                            let _ = janela.update(cx, |this, cx| this.apagar_projeto(projeto.clone(), window, cx));
                        },
                        None,
                        window,
                        cx,
                    );
                });
            })
            .detach();
    }

    fn apagar_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let (chats, tarefas, nome) = {
            let m = motor.read(cx);
            let (chats, tarefas) = motor::do_projeto(&m.chats, &m.tarefas, &projeto);
            (chats, tarefas, m.nome_do_projeto(&projeto))
        };
        let tarefa = motor.update(cx, |m, cx| m.apagar_projeto(&projeto, cx));
        let janela = cx.entity().downgrade();
        window
            .spawn(cx, async move |cx| {
                let resultado = tarefa.await;
                let _ = cx.update(|window, cx| {
                    if resultado.is_ok() {
                        let _ = janela.update(cx, |this, cx| this.largar_projeto(&projeto, &chats, Some(&tarefas), window, cx));
                    }
                    let (tom, texto) = aviso_de_apagado(&nome, &resultado);
                    if let Ok(r) = &resultado {
                        for erro in &r.erros {
                            eprintln!("vitral: cópia de {nome} não foi para a lixeira: {erro}");
                        }
                    }
                    let nota = if resultado.as_ref().is_ok_and(|r| r.na_lixeira > 0) {
                        aviso_com_acao(tom, texto, "Abrir a Lixeira", |_, _| crate::sistema::abrir_lixeira())
                    } else {
                        aviso(tom, texto)
                    };
                    window.push_notification(nota, cx);
                });
            })
            .detach();
    }

    /// Fecha os painéis das conversas (o último fica vazio) e tira a seleção delas; com `sairam`
    /// (saíram do Vitral), esquece também os rascunhos e os anexos.
    pub(super) fn largar_chats(&mut self, chats: &[String], sairam: bool, window: &mut Window, cx: &mut Context<Self>) {
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
        if sairam {
            for chat in chats {
                self.rascunhos.remove(chat);
                self.anexos.remove(chat);
                self.ultrathink.remove(chat);
                self.ultracode.remove(chat);
                self.familias_abertas.remove(chat);
            }
        }
        cx.notify();
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
        if self.vista.projeto() == Some(projeto) {
            self.mudar_vista(super::Vista::Geral, cx);
        }
        let pastas: Vec<std::path::PathBuf> = self
            .motor
            .as_ref()
            .map(|m| m.read(cx).pastas_do(projeto).iter().map(|p| p.caminho.clone()).collect())
            .unwrap_or_default();
        if self.arquivo_aberto.as_ref().is_some_and(|(p, _)| pastas.iter().any(|l| p.starts_with(l))) {
            self.arquivo_aberto = None;
            *self.arquivo_lido.borrow_mut() = None;
        }
        self.largar_chats(chats, saiu.is_some(), window, cx);
        if let Some(tarefas) = saiu {
            self.revisoes.retain(|t, _| !tarefas.contains(t));
            self.cache_arquivos.borrow_mut().retain(|p, _| !pastas.contains(p));
            self.arvores.retain(|p, _| !pastas.iter().any(|l| l.display().to_string() == *p));
            self.git_pastas.retain(|p, _| !pastas.contains(p));
            self.pasta_arquivos.remove(projeto);
        }
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use std::path::{Path, PathBuf};

    use super::{
        Ocupado, Tom, aviso_de_apagado, aviso_de_arquivado, detalhes_do_mestre, pergunta_de_apagar, por_que_esperar,
        projeto_ocupado, texto_de_parar_e_arquivar,
    };
    use crate::{
        modelo::{Cartao, Chat, Divisao, EstadoCartao, EstadoTarefa, Papel, Parte, Tarefa, TipoCartao},
        motor::{Apagado, Ocupacao, ResumoDoProjeto},
    };

    #[test]
    fn projeto_ocupado_separa_o_que_para_o_que_espera_e_os_pedidos_de_fora() {
        let chats = vec![
            Chat::novo("c1".into(), Papel::Projeto, "loja", "Checkout", "/tmp".into()),
            Chat::novo("c2".into(), Papel::Coordenador, "loja", "Antigo", "/tmp".into()),
            Chat::novo("s1".into(), Papel::Projeto, "site", "Outro", "/tmp".into()),
        ];
        let tarefa = |id: u64, projeto: &str, dono: &str, estado: EstadoTarefa| Tarefa {
            id,
            projeto: projeto.into(),
            coordenador: dono.into(),
            estado,
            ..Default::default()
        };
        let tarefas = vec![
            // Pedido do Mestre rodando e em revisão: contam.
            tarefa(1, "loja", "mestre", EstadoTarefa::Rodando),
            tarefa(2, "loja", "mestre", EstadoTarefa::EmRevisao),
            // Do coordenador do próprio projeto (vai junto ao arquivar), sem dono e encerrada: não.
            tarefa(3, "loja", "c2", EstadoTarefa::Rodando),
            tarefa(4, "loja", "", EstadoTarefa::Conversa),
            tarefa(5, "loja", "mestre", EstadoTarefa::Aprovada),
            // De outro projeto: não.
            tarefa(6, "site", "mestre", EstadoTarefa::Rodando),
        ];
        let parte = |projeto: &str, removida: bool| Parte { projeto: projeto.into(), removida, ..Default::default() };
        let cartao = |id: u64, estado: EstadoCartao, partes: Vec<Parte>| Cartao {
            id,
            chat: "mestre".into(),
            criado_em: 0,
            decidido_em: 0,
            estado,
            tipo: TipoCartao::Divisao(Divisao { partes, ..Default::default() }),
            resto: Default::default(),
        };
        let cartoes = vec![
            // Aberta com parte na loja: conta; já decidida, ou com a parte da loja tirada: não.
            cartao(7, EstadoCartao::Aberto, vec![parte("site", false), parte("loja", false)]),
            cartao(8, EstadoCartao::Aceito, vec![parte("loja", false)]),
            cartao(9, EstadoCartao::Aberto, vec![parte("loja", true), parte("site", false)]),
        ];
        let ocupacao = Ocupacao { conversas: vec!["c1".into()], tarefas: vec![4] };
        let o = projeto_ocupado("loja", &ocupacao, &chats, &tarefas, &cartoes);
        assert_eq!(o, Ocupado { parar: vec!["c1".into()], esperar: vec![4], pedidos: vec![1, 2], divisoes: vec![7] });
        assert!(projeto_ocupado("loja", &Ocupacao::default(), &chats, &[], &cartoes[1..]).livre());
        assert_eq!(
            detalhes_do_mestre("loja", &o),
            [
                "2 pedidos abertos do Mestre (chat 1, chat 2) são cancelados; a conversa e a cópia ficam",
                "A parte de loja sai da divisão 7 do Mestre",
            ]
        );
        let so_divisao = Ocupado { divisoes: vec![7], ..Default::default() };
        assert!(!so_divisao.livre(), "a divisão com parte no projeto pergunta antes");
    }

    #[test]
    fn so_espera_o_que_nao_da_para_parar() {
        let livre = Ocupacao::default();
        assert_eq!(por_que_esperar("loja", &livre, false), None);
        assert_eq!(por_que_esperar("loja", &livre, true), None);
        // Conversa trabalhando: arquivar pergunta (sem motivo aqui); apagar não deixa.
        let conversando = Ocupacao { conversas: vec!["c1".into(), "w2".into()], tarefas: Vec::new() };
        assert_eq!(por_que_esperar("loja", &conversando, false), None);
        assert_eq!(
            por_que_esperar("loja", &conversando, true).unwrap(),
            "loja tem 2 conversas trabalhando; pare-as antes de apagar o projeto."
        );
        let uma = Ocupacao { conversas: vec!["c1".into()], tarefas: Vec::new() };
        assert_eq!(
            por_que_esperar("loja", &uma, true).unwrap(),
            "loja tem 1 conversa trabalhando; pare-a antes de apagar o projeto."
        );
        // A tarefa que o Vitral confere, junta ou prepara vence: essa não dá para parar.
        let juntando = Ocupacao { conversas: vec!["c1".into()], tarefas: vec![4] };
        assert_eq!(por_que_esperar("loja", &juntando, false).unwrap(), "Espere o chat 4 de loja: o Vitral ainda mexe na cópia dele.");
        let duas = Ocupacao { conversas: Vec::new(), tarefas: vec![4, 7] };
        assert_eq!(por_que_esperar("loja", &duas, true).unwrap(), "Espere os chats 4, 7 de loja: o Vitral ainda mexe na cópia deles.");
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
            copias: (0..copias).map(|i| PathBuf::from(format!("/d/copias/loja/t{i}"))).collect(),
            rotinas,
            ligadas: vec![PathBuf::from("/p/loja")],
            branches: copias,
            ..Default::default()
        }
    }

    #[test]
    fn a_pergunta_diz_o_que_sai_e_que_as_pastas_ligadas_ficam() {
        let p = pergunta_de_apagar("loja", &resumo(5, 8, 3, 2), None, None);
        assert_eq!(p.titulo, "Apagar o projeto loja?");
        assert!(p.texto.contains("As pastas ligadas ficam como estão"));
        let linhas: Vec<(&str, bool)> = p.detalhes.iter().map(|(_, t, d)| (t.as_str(), *d)).collect();
        assert_eq!(
            linhas,
            [
                ("/p/loja fica como está", false),
                ("5 conversas e 8 ajudantes saem do Vitral, com as mensagens", true),
                ("3 cópias de chats vão para a Lixeira", false),
                ("3 branches dos chats ficam em /p/loja", false),
                ("2 rotinas são apagadas", true),
            ]
        );
        // Só a pasta própria: ela vai para a Lixeira, com o tamanho.
        let so_propria = ResumoDoProjeto {
            conversas: 1,
            propria: Some("/d/projetos/loja".into()),
            ..Default::default()
        };
        let p = pergunta_de_apagar("loja", &so_propria, Some(Path::new("/d/projetos/loja")), Some((1234, 5 * 1024 * 1024, true)));
        let linhas: Vec<(&str, bool)> = p.detalhes.iter().map(|(_, t, d)| (t.as_str(), *d)).collect();
        assert_eq!(
            linhas,
            [
                ("1 conversa sai do Vitral, com as mensagens", true),
                ("/d/projetos/loja vai para a Lixeira", true),
                ("1.234 arquivos · 5,0 MB", false),
            ]
        );
        // A própria com uma ligada: a própria fica.
        let as_duas = ResumoDoProjeto { propria: Some("/d/projetos/loja".into()), ..resumo(0, 0, 0, 0) };
        let p = pergunta_de_apagar("loja", &as_duas, None, None);
        let linhas: Vec<&str> = p.detalhes.iter().map(|(_, t, _)| t.as_str()).collect();
        assert_eq!(linhas, ["/p/loja fica como está", "A pasta própria /d/projetos/loja fica no disco"]);
    }

    #[test]
    fn o_aviso_diz_o_que_saiu_ou_que_nada_foi_apagado() {
        let feito = |conversas, erros: &[&str]| {
            Ok(Apagado { na_lixeira: 1, conversas, erros: erros.iter().map(|e| e.to_string()).collect() })
        };
        assert_eq!(aviso_de_apagado("loja", &feito(5, &[])), (Tom::Pronto, "loja apagado · 5 conversas apagadas".into()));
        assert_eq!(aviso_de_apagado("loja", &feito(0, &[])), (Tom::Pronto, "loja apagado".into()));
        assert_eq!(
            aviso_de_apagado("loja", &feito(1, &["/c/t4: sem permissão"])),
            (Tom::Atencao, "loja apagado, mas 1 cópia ficou no disco: /c/t4: sem permissão".into())
        );
        assert_eq!(
            aviso_de_apagado("loja", &feito(1, &["/c/t4: a", "/c/t5: b"])).1,
            "loja apagado, mas 2 cópias ficaram no disco: /c/t4: a; /c/t5: b"
        );
        assert_eq!(
            aviso_de_apagado("loja", &Err("a lixeira não aceitou".into())),
            (Tom::Erro, "Nada foi apagado: a lixeira não aceitou".into())
        );
    }
}
