//! A revisão do turno pelo outro agente (0.9.6): num chat do Claude, o Codex; num chat do Codex, o
//! Claude. Sempre só lendo (o Codex no sandbox `read-only`, o Claude em Planejar), e nada do que
//! ele diz vai ao agente do chat sozinho.
//!
//! - **Automática**: o turno que terminou bem e mudou `LINHAS_PARA_REVISAR` linhas ou mais (o
//!   +N −M das Mudanças do turno) é revisto em seguida (`Motor::pedir_revisao_do_par`, no fim do
//!   turno). Não roda com o outro agente fora (não instalado, ou no limite do plano), com o chat
//!   em Planejar, quando o turno já veio de uma revisão (nada de laço) nem com mensagem sua
//!   esperando; e a mensagem que você manda com a revisão rodando tem a vez: a revisão para.
//! - **Com um clique**: "Revisar com o Codex" na linha "Trabalhou…" do último turno pronto
//!   (`Motor::revisar_com_o_outro`), para qualquer turno.
//! - **O que aparece**: sem nada a dizer, uma linha, "Codex revisou: nada a acrescentar"; com
//!   achado, o `Item::Revisao`, um cartão com o texto e a ação "Mandar ao Claude"
//!   (`Motor::mandar_revisao`), que entrega a revisão ao agente do chat.
//!
//! É o mecanismo da dupla que já existia (`iniciar_dupla`, `Parceiro`, `Chat::trocar_vez`,
//! `o_que_o_outro_fez`, o "Nada a acrescentar"), sem as rodadas: o revisor entra como o parceiro
//! do chat, recebe o que o outro fez, roda um turno só (o processo do chat é um só, então a
//! revisão é o turno dele enquanto dura) e o chat volta a quem era. O que ele fez nesse turno (as
//! leituras, o texto) sai da conversa e dá lugar ao resultado; o parceiro criado só para a
//! revisão sai junto, e o chat continua de um agente só.

use super::*;

/// Quantas linhas o turno tem de mudar (as que entram mais as que saem) para a revisão automática,
/// de fábrica (`Config.revisao_minimo`).
pub const LINHAS_PARA_REVISAR: usize = 20;

/// O nome de quem entrega a revisão ao agente do chat ("Revisão do Codex"): é por ele que o turno
/// seguinte se reconhece como vindo de uma revisão.
const ENTREGA_DA_REVISAO: &str = "Revisão do ";

/// A revisão em curso num chat. Mora no modelo porque vai gravada no chat também
/// (`Chat.revisao_do_par`): o Vitral fechado no meio dela a desfaz na abertura seguinte.
pub use crate::modelo::RevisaoEmCurso as EmCurso;

/// O que a revisão cortada pelo Vitral fechado deixa na conversa (depois de "não terminou: ").
const CORTADA_AO_FECHAR: &str = "o Vitral foi fechado no meio dela";

/// Por que a revisão não roda.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemRevisao {
    /// A opção `Config.revisar_edicoes` está desligada.
    Desligada,
    /// O turno mudou menos de `LINHAS_PARA_REVISAR` linhas.
    PoucasLinhas,
    /// O outro agente não está instalado.
    SemOOutro,
    /// O outro agente bateu o limite do plano.
    OutroNoLimite,
    /// O chat está em Planejar.
    Planejando,
    /// O turno já veio de uma revisão: nada de laço.
    VeioDeRevisao,
}

/// O que o fim de um turno precisa dizer para a revisão automática se decidir.
#[derive(Clone, Copy, Debug)]
pub struct Turno {
    pub ligada: bool,
    pub linhas: usize,
    /// O mínimo de linhas (`Config.revisao_minimo`; zero com "todo turno").
    pub minimo: usize,
    pub outro_instalado: bool,
    pub outro_no_limite: bool,
    pub planejando: bool,
    pub veio_de_revisao: bool,
}

/// A revisão automática roda neste fim de turno? `None` é sim.
pub fn motivo_sem_revisao(turno: &Turno) -> Option<SemRevisao> {
    if !turno.ligada {
        Some(SemRevisao::Desligada)
    } else if turno.planejando {
        Some(SemRevisao::Planejando)
    } else if turno.veio_de_revisao {
        Some(SemRevisao::VeioDeRevisao)
    } else if turno.linhas < turno.minimo {
        Some(SemRevisao::PoucasLinhas)
    } else {
        motivo_sem_revisao_pedida(turno)
    }
}

/// A revisão pedida pelo clique: só precisa do outro agente.
pub fn motivo_sem_revisao_pedida(turno: &Turno) -> Option<SemRevisao> {
    if !turno.outro_instalado {
        Some(SemRevisao::SemOOutro)
    } else if turno.outro_no_limite {
        Some(SemRevisao::OutroNoLimite)
    } else {
        None
    }
}

/// As linhas que o último turno mudou: as que entraram mais as que saíram nas edições que deram
/// certo desde a sua última mensagem (a mesma conta das Mudanças do turno, na tela).
pub fn linhas_mudadas_no_turno(itens: &[Item]) -> usize {
    let turno = itens.iter().rposition(|i| matches!(i, Item::Usuario { .. })).unwrap_or(0);
    itens[turno..]
        .iter()
        .filter_map(|item| match item {
            Item::Ferramenta { nome, erro: false, mudanca: Some(m), .. }
                if matches!(nome.as_str(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit") =>
            {
                Some((m.entram + m.saem) as usize)
            }
            _ => None,
        })
        .sum()
}

/// A resposta é só o "Nada a acrescentar".
pub fn nada_a_dizer(texto: &str) -> bool {
    let texto = texto.trim();
    texto.chars().count() < 80 && texto.to_lowercase().starts_with(&NADA_A_ACRESCENTAR.to_lowercase())
}

/// O pedido que o revisor recebe.
pub fn pedido_de_revisao(principal: Agente, foco: &[String], instrucoes: &str) -> String {
    let mut pedido = format!(
        "[Vitral] Revisão do turno. O {nome} acabou de trabalhar neste chat (o que ele fez está acima, e o que mudou nos \
         arquivos já está na pasta). Você só lê: não mude arquivo nenhum nem rode comando que escreva. Confira o que ele \
         mudou (o diff da pasta, os arquivos que ele editou) e o que ele respondeu, e aponte o que está errado, o que \
         faltou e o risco que ele não viu, do mais grave para o menos, com o arquivo e a linha. Seja direto e curto: só \
         o que vale o usuário ler. Não repita o que ele fez nem elogie. Se estiver tudo certo e você não tiver nada a \
         acrescentar, responda só: {NADA_A_ACRESCENTAR}.",
        nome = principal.nome()
    );
    // O foco escolhido nas Configurações, em minúsculas e com o "e" antes do último.
    let foco: Vec<String> = foco.iter().map(|f| f.trim().to_lowercase()).filter(|f| !f.is_empty()).collect();
    if let Some((ultimo, antes)) = foco.split_last() {
        let lista = if antes.is_empty() { ultimo.clone() } else { format!("{} e {ultimo}", antes.join(", ")) };
        pedido.push_str(&format!(" Dê atenção especial a: {lista}."));
    }
    if !instrucoes.trim().is_empty() {
        pedido.push_str(&format!(" Do usuário: {}", instrucoes.trim()));
    }
    pedido
}

/// Põe o revisor na vez: o parceiro do chat (criado agora, se o chat era de um agente só), o
/// outro agente do chat. Só os testes: o motor abre com a escolha das Configurações.
#[cfg(test)]
pub fn abrir(chat: &mut Chat) -> EmCurso {
    let revisor = chat.agente.outro();
    abrir_com(chat, revisor, None)
}

/// Como `abrir`, com o revisor e o modelo escolhidos nas Configurações. Num chat que já é uma
/// dupla, quem revisa é o parceiro dele, com o modelo dele.
pub fn abrir_com(chat: &mut Chat, revisor: Agente, modelo: Option<String>) -> EmCurso {
    let criou_o_parceiro = chat.parceiro.is_none();
    let revisor = match &chat.parceiro {
        Some(p) => p.agente,
        None if revisor == chat.agente => chat.agente.outro(),
        None => revisor,
    };
    let em_curso = EmCurso {
        desde_item: chat.itens.len(),
        principal: chat.agente,
        criou_o_parceiro,
        atualizado_antes: chat.atualizado_em,
        revisor: Some(revisor),
    };
    if em_curso.criou_o_parceiro {
        iniciar_dupla(chat, revisor);
        // O parceiro nasceu agora e já está na vez: o modelo dele é o do revisor.
        if modelo.is_some() {
            chat.modelo = modelo;
        }
    } else {
        chat.trocar_vez();
    }
    chat.revisao_do_par = Some(em_curso.clone());
    em_curso
}

/// Fecha a revisão: o que o revisor fez sai da conversa, o chat volta ao agente dele e o
/// `resultado` (a linha, o cartão ou nada) entra no lugar.
pub fn fechar(chat: &mut Chat, em_curso: &EmCurso, resultado: Option<Item>) {
    chat.itens.truncate(em_curso.desde_item);
    if chat.agente != em_curso.principal {
        chat.trocar_vez();
    }
    if em_curso.criou_o_parceiro {
        chat.parceiro = None;
    }
    chat.revisao_do_par = None;
    chat.itens.extend(resultado);
}

/// Na abertura: a revisão que o Vitral fechado (ou uma queda) cortou no meio é desfeita como a
/// cancelada (o chat volta ao agente dele e o turno do revisor sai), com a linha de que ela não
/// terminou no lugar. O turno de trabalho já tinha terminado: não há o que retomar, e o
/// "Revisar com o Codex" continua à mão. Devolve os chats mexidos.
///
/// O estado (com a revisão em curso) e a conversa são arquivos gravados um depois do outro, e o
/// estado pode voltar do `.anterior`: a marca pode ser mais velha que a conversa. Por isso só se
/// corta o que vem depois de `desde_item` quando é mesmo o turno do revisor (marcas de vez,
/// ferramentas, falas). Com mensagem sua, entrega ou o resultado de uma revisão ali, a revisão já
/// tinha acabado: só o agente volta, e a conversa fica inteira.
pub fn desfazer_as_cortadas(chats: &mut [Chat]) -> Vec<String> {
    let mut mexidos = Vec::new();
    for chat in chats.iter_mut() {
        let Some(mut em_curso) = chat.revisao_do_par.clone() else {
            continue;
        };
        let depois = &chat.itens[em_curso.desde_item.min(chat.itens.len())..];
        let acabou = depois.iter().any(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }) || e_resultado_de_revisao(i));
        if acabou {
            em_curso.desde_item = chat.itens.len();
            fechar(chat, &em_curso, None);
        } else {
            let aviso = resultado(em_curso.revisor(), &[], CORTADA_AO_FECHAR, true);
            fechar(chat, &em_curso, Some(aviso));
            chat.turno_aberto = false;
            chat.atualizado_em = chat.atualizado_em.min(em_curso.atualizado_antes.max(chat.lido_em));
        }
        mexidos.push(chat.id.clone());
    }
    mexidos
}

/// O que fica na conversa: a linha "Codex revisou: nada a acrescentar", o cartão com o achado, ou
/// a linha de que a revisão não terminou. `da_revisao` é o que o revisor fez no turno dele e
/// `fim` o texto do `result`.
pub fn resultado(revisor: Agente, da_revisao: &[Item], fim: &str, erro: bool) -> Item {
    let nome = revisor.nome();
    if erro {
        let linha = fim.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("sem resposta");
        return Item::Aviso { texto: format!("A revisão do {nome} não terminou: {linha}"), erro: false, em: agora() };
    }
    // A última fala dele é a revisão; o que disse no caminho ("vou ler o diff") não entra.
    let texto = da_revisao
        .iter()
        .rev()
        .find_map(|i| match i {
            Item::Assistente { texto, .. } if !texto.trim().is_empty() => Some(texto.trim()),
            _ => None,
        })
        .unwrap_or(fim.trim());
    if texto.is_empty() || nada_a_dizer(texto) {
        return Item::Aviso { texto: format!("{nome} revisou: nada a acrescentar"), erro: false, em: agora() };
    }
    Item::Revisao { de: revisor, texto: texto.to_string(), em: agora(), mandada: false }
}

/// O item é o que uma revisão deixou: o cartão ou a linha de quem não achou nada.
fn e_resultado_de_revisao(item: &Item) -> bool {
    match item {
        Item::Revisao { .. } => true,
        Item::Aviso { texto, erro: false, .. } => texto.ends_with(" revisou: nada a acrescentar"),
        _ => false,
    }
}

/// A conversa tem um turno pronto do agente do chat que ainda não foi revisto.
pub fn revisavel(itens: &[Item]) -> bool {
    let turno = itens.iter().rposition(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. })).map_or(0, |i| i + 1);
    let do_turno = &itens[turno.min(itens.len())..];
    do_turno.iter().any(|i| matches!(i, Item::Assistente { .. })) && !do_turno.iter().any(e_resultado_de_revisao)
}

/// O turno veio de uma revisão mandada ao agente do chat.
pub fn veio_de_revisao(mensagens: &[Entrada]) -> bool {
    mensagens.iter().any(|e| e.de.as_deref().is_some_and(|de| de.starts_with(ENTREGA_DA_REVISAO)))
}

/// De quem e o quê o agente do chat recebe quando você manda a revisão a ele.
pub fn entrega_ao_principal(revisor: Agente, texto: &str) -> (String, String) {
    let de = format!("{ENTREGA_DA_REVISAO}{}", revisor.nome());
    let texto = format!(
        "O {} revisou o que você fez no último turno e apontou o que segue. Confira cada ponto no código antes de \
         aceitar: corrija o que proceder e diga o que não procede e por quê.\n\n{}",
        revisor.nome(),
        texto.trim()
    );
    (de, texto)
}

impl Motor {
    /// Quem revisa este chat: o parceiro, num chat que já é uma dupla; senão o escolhido nas
    /// Configurações (`Config.revisao_agente`) ou, sem escolha ou num chat do próprio escolhido, o
    /// outro agente.
    pub fn revisor_para(&self, chat: &Chat) -> Agente {
        if let Some(p) = &chat.parceiro {
            return p.agente;
        }
        Agente::TODOS
            .into_iter()
            .find(|a| a.nome().eq_ignore_ascii_case(self.config.revisao_agente.trim()))
            .filter(|a| *a != chat.agente)
            .unwrap_or(chat.agente.outro())
    }

    /// O modelo escolhido para o revisor `agente` (vazio: o padrão dele).
    fn modelo_do_revisor(&self, agente: Agente) -> Option<String> {
        self.config.revisao_modelos.get(&agente.nome().to_lowercase()).map(|m| m.trim().to_string()).filter(|m| !m.is_empty())
    }

    /// O revisor deste chat pode revisar agora: se está instalado e se bateu o limite do plano.
    fn revisor_do_chat(&self, chat: &Chat) -> (Agente, bool, bool) {
        let limite = |uso: Uso| uso.bloqueado || uso.cinco_horas.is_some_and(|f| f >= 1.0) || uso.semana.is_some_and(|f| f >= 1.0);
        match self.revisor_para(chat) {
            Agente::Codex => {
                let no_limite = self.codex.as_ref().is_some_and(|c| limite(c.uso));
                (Agente::Codex, crate::agente_codex::instalado(), no_limite)
            }
            // O Claude revisa na conta que este chat usaria; no limite quando ela está.
            Agente::Claude => {
                let conta = self.conta_padrao();
                (Agente::Claude, self.versao_claude.is_some() || claude::binario_claude().is_file(), limite(self.uso_da_conta(&conta.id)))
            }
            // O Gemini e os ACP novos revisam quando são os escolhidos; o uso do plano deles não diz
            // o limite.
            outro => (outro, outro.instalado(), false),
        }
    }

    /// Quem está revisando o último turno deste chat agora (a linha de estado diz, discreta).
    pub fn revisando_o_par(&self, chat: &str) -> Option<Agente> {
        let em_curso = self.revisoes_do_par.get(chat)?;
        self.com_turno(chat).then(|| em_curso.revisor())
    }

    /// Desde que item a conversa esconde o que o revisor está fazendo (ele vira o cartão no fim).
    pub fn revisao_em_curso_desde(&self, chat: &str) -> Option<usize> {
        self.revisoes_do_par.get(chat).map(|r| r.desde_item)
    }

    /// Quem revisaria o último turno deste chat pelo clique, quando isso faz sentido agora: chat
    /// parado sem erro, turno pronto e ainda não revisto, o outro agente à mão e sem revisão
    /// rodando. É o que decide se a ação "Revisar com o Codex" aparece.
    pub fn pode_revisar(&self, chat_id: &str) -> Option<Agente> {
        let chat = self.chat(chat_id)?;
        if self.somente_leitura.is_some()
            || chat.arquivado
            || self.com_turno(chat_id)
            || self.revisoes_do_par.contains_key(chat_id)
            || self.revisando.contains_key(chat_id)
            || !matches!(self.execucao_real(chat_id), Execucao::Parado)
            || self.na_fila(chat_id) > 0
            || !revisavel(&chat.itens)
        {
            return None;
        }
        let (revisor, instalado, no_limite) = self.revisor_do_chat(chat);
        let turno = Turno { ligada: true, linhas: 0, minimo: 0, outro_instalado: instalado, outro_no_limite: no_limite, planejando: false, veio_de_revisao: false };
        motivo_sem_revisao_pedida(&turno).is_none().then_some(revisor)
    }

    /// "Revisar com o Codex" (ou "com o Claude"), da linha "Trabalhou…": a mesma revisão da
    /// automática, para qualquer turno. Devolve se ela começou.
    pub fn revisar_com_o_outro(&mut self, chat_id: &str, cx: &mut Context<Self>) -> bool {
        if self.pode_revisar(chat_id).is_none() {
            return false;
        }
        self.comecar_revisao_do_par(chat_id, cx)
    }

    /// No fim de um turno que terminou bem: a revisão automática, quando é o caso
    /// (`motivo_sem_revisao`). Devolve se ela começou.
    pub(super) fn pedir_revisao_do_par(&mut self, chat_id: &str, cx: &mut Context<Self>) -> bool {
        if self.somente_leitura.is_some() || self.revisoes_do_par.contains_key(chat_id) {
            return false;
        }
        let Some(chat) = self.chat(chat_id) else {
            return false;
        };
        let (_, instalado, no_limite) = self.revisor_do_chat(chat);
        // "Só no botão" desliga a automática; "Todo turno" é o mínimo zero.
        let quando = self.config.revisao_quando.as_str();
        let turno = Turno {
            ligada: self.config.revisar_edicoes && quando != "botao",
            linhas: linhas_mudadas_no_turno(&chat.itens),
            minimo: if quando == "todo" { 0 } else { self.config.revisao_minimo as usize },
            outro_instalado: instalado,
            outro_no_limite: no_limite,
            planejando: self.config.revisao_pula_planejar && self.permissao_de(chat) == "plan",
            veio_de_revisao: self.ultima_entrada.get(chat_id).is_some_and(|(_, mensagens)| veio_de_revisao(mensagens)),
        };
        if motivo_sem_revisao(&turno).is_some() || !revisavel(&chat.itens) {
            return false;
        }
        self.comecar_revisao_do_par(chat_id, cx)
    }

    /// Abre o turno do revisor: ele entra na vez, recebe o que o outro fez (`o_que_o_outro_fez`,
    /// no começo do turno) e o pedido de revisão, que não aparece na conversa. O turno roda em
    /// Planejar (`comecar_turno_com` vê a revisão em curso), que no Codex é o sandbox `read-only`.
    fn comecar_revisao_do_par(&mut self, chat_id: &str, cx: &mut Context<Self>) -> bool {
        let Some(revisor) = self.chat(chat_id).map(|c| self.revisor_para(c)) else {
            return false;
        };
        let modelo = self.modelo_do_revisor(revisor);
        let Some(c) = self.chat_mut(chat_id) else {
            return false;
        };
        let principal = c.agente;
        let em_curso = abrir_com(c, revisor, modelo);
        self.revisoes_do_par.insert(chat_id.to_string(), em_curso);
        let entrada = Entrada {
            de: Some("Vitral".into()),
            texto: pedido_de_revisao(principal, &self.config.revisao_foco, &self.config.revisao_instrucoes),
            // Fica fora da conversa: o que aparece é a linha de estado e, no fim, o resultado.
            registrada: true,
            ..Entrada::default()
        };
        self.comecar_turno_com(chat_id, vec![entrada], true, cx);
        // Não abriu (a pasta sumiu, o Codex saiu do PATH): o chat volta a quem era, sem nada.
        if !self.com_turno(chat_id) {
            self.desfazer_revisao_do_par(chat_id);
            return false;
        }
        cx.notify();
        true
    }

    /// A revisão sai sem deixar nada (a sua mensagem chegou, ou ela nem abriu): o que o revisor
    /// tinha feito some e o chat volta ao agente dele.
    pub(super) fn desfazer_revisao_do_par(&mut self, chat_id: &str) {
        let Some(em_curso) = self.revisoes_do_par.remove(chat_id) else {
            return;
        };
        if let Some(c) = self.chat_mut(chat_id) {
            fechar(c, &em_curso, None);
            c.turno_aberto = false;
            c.atualizado_em = c.atualizado_em.min(em_curso.atualizado_antes.max(c.lido_em));
        }
        self.parcial.remove(chat_id);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        self.chats_sujos.insert(chat_id.to_string());
    }

    /// O turno do revisor terminou: o resultado entra na conversa no lugar do que ele fez, e o
    /// chat volta ao agente dele. Só o achado deixa o chat como não lido. Em seguida, o que
    /// esperava: a fila, ou a revisão da memória.
    pub(super) fn fechar_revisao_do_par(&mut self, chat_id: &str, erro: bool, fim: &str, cx: &mut Context<Self>) {
        let Some(em_curso) = self.revisoes_do_par.remove(chat_id) else {
            return;
        };
        let mostrar_nada = self.config.revisao_mostra_nada;
        if let Some(c) = self.chat_mut(chat_id) {
            let desde = em_curso.desde_item.min(c.itens.len());
            let item = resultado(em_curso.revisor(), &c.itens[desde..], fim, erro);
            let achou = matches!(item, Item::Revisao { .. });
            // "Mostrar 'nada a acrescentar'" desligado: a linha de quem não achou nada não entra.
            let item = Some(item).filter(|i| mostrar_nada || !e_resultado_de_revisao(i) || matches!(i, Item::Revisao { .. }));
            fechar(c, &em_curso, item);
            c.turno_aberto = false;
            if achou {
                c.atualizado_em = agora();
            } else {
                c.atualizado_em = c.atualizado_em.min(em_curso.atualizado_antes.max(c.lido_em));
            }
        }
        self.parcial.remove(chat_id);
        self.execucao.insert(chat_id.to_string(), Execucao::Parado);
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        // "Mandar ao agente sozinho": o achado vai ao agente do chat na hora (o cartão fica
        // marcado como mandado, como no clique).
        let achado = self
            .chat(chat_id)
            .and_then(|c| c.itens.iter().rposition(|i| matches!(i, Item::Revisao { mandada: false, .. })))
            .filter(|i| self.config.revisao_manda_sozinha && self.chat(chat_id).is_some_and(|c| *i + 1 == c.itens.len()));
        if let Some(indice) = achado {
            self.mandar_revisao(chat_id, indice, cx);
        } else if self.na_fila(chat_id) > 0 {
            self.entregar_fila(chat_id, cx);
        } else {
            self.pedir_revisao_da_memoria(chat_id, cx);
        }
        self.salvar();
        cx.notify();
    }

    /// "Mandar ao Claude" (ou "ao Codex"), do cartão da revisão: ela vai ao agente do chat como
    /// uma mensagem recebida, e o cartão fica marcado como mandado. Com o chat ocupado, espera na
    /// fila como qualquer entrega.
    pub fn mandar_revisao(&mut self, chat_id: &str, indice: usize, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        let Some(c) = self.chat_mut(chat_id) else {
            return;
        };
        let Some(Item::Revisao { de, texto, mandada, .. }) = c.itens.get_mut(indice) else {
            return;
        };
        if *mandada {
            return;
        }
        *mandada = true;
        let (de, texto) = entrega_ao_principal(*de, texto);
        if let Some(c) = self.chat(chat_id) {
            let _ = self.dados.salvar_chat(c);
        }
        self.entregar(chat_id, Some(de), texto, cx);
        self.salvar();
        cx.notify();
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn edicao(nome: &str, entram: u32, saem: u32, erro: bool) -> Item {
        Item::Ferramenta {
            id: "t".into(),
            nome: nome.into(),
            resumo: "a.rs".into(),
            resultado: Some("ok".into()),
            erro,
            em: 0,
            detalhe: None,
            arquivo: Some("a.rs".into()),
            sub: None,
            mudanca: Some(crate::modelo::Mudanca { entram, saem, novo: false }),
            imagens: Vec::new(),
            durou: 0,
        }
    }

    fn usuario(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0, marca: None }
    }

    fn fala(texto: &str) -> Item {
        Item::Assistente { texto: texto.into(), em: 0 }
    }

    fn turno_bom() -> Turno {
        Turno {
            ligada: true,
            linhas: LINHAS_PARA_REVISAR,
            minimo: LINHAS_PARA_REVISAR,
            outro_instalado: true,
            outro_no_limite: false,
            planejando: false,
            veio_de_revisao: false,
        }
    }

    /// As opções da revisão (Configurações › Revisão, protótipo Linear de 25/11/2026): o mínimo de
    /// linhas é o seu; "todo turno" é o mínimo zero; e o foco e as suas instruções vão no pedido.
    #[test]
    fn as_opcoes_da_revisao_mudam_o_minimo_e_o_pedido() {
        assert_eq!(motivo_sem_revisao(&Turno { minimo: 5, linhas: 6, ..turno_bom() }), None);
        assert_eq!(motivo_sem_revisao(&Turno { minimo: 50, linhas: 49, ..turno_bom() }), Some(SemRevisao::PoucasLinhas));
        assert_eq!(motivo_sem_revisao(&Turno { minimo: 0, linhas: 0, ..turno_bom() }), None, "todo turno");
        let pedido = pedido_de_revisao(Agente::Claude, &["Bugs".into(), "Segurança".into()], "Olhe as migrações.");
        assert!(pedido.contains("Dê atenção especial a: bugs e segurança."), "{pedido}");
        assert!(pedido.contains("Do usuário: Olhe as migrações."), "{pedido}");
        let simples = pedido_de_revisao(Agente::Claude, &[], "  ");
        assert!(!simples.contains("atenção especial") && !simples.contains("Do usuário"), "{simples}");
    }

    /// O critério do item 2: a revisão automática roda no turno que mudou 20 linhas ou mais, e a
    /// conta é a das Mudanças do turno (o que entrou mais o que saiu, desde a sua última mensagem,
    /// só nas edições que deram certo).
    #[test]
    fn a_revisao_automatica_roda_no_turno_que_mudou_vinte_linhas() {
        assert_eq!(LINHAS_PARA_REVISAR, 20);
        assert_eq!(motivo_sem_revisao(&turno_bom()), None);
        assert_eq!(motivo_sem_revisao(&Turno { linhas: 19, ..turno_bom() }), Some(SemRevisao::PoucasLinhas));
        assert_eq!(motivo_sem_revisao(&Turno { linhas: 0, ..turno_bom() }), Some(SemRevisao::PoucasLinhas));
        let itens = vec![
            usuario("antes"),
            edicao("Edit", 40, 40, false),
            fala("feito"),
            usuario("agora"),
            edicao("Edit", 9, 3, false),
            edicao("Write", 6, 0, false),
            edicao("Bash", 50, 50, false),
            edicao("Edit", 30, 30, true),
            edicao("MultiEdit", 1, 1, false),
            fala("pronto"),
        ];
        assert_eq!(linhas_mudadas_no_turno(&itens), 9 + 3 + 6 + 1 + 1, "só as edições deste turno que deram certo");
        assert_eq!(linhas_mudadas_no_turno(&itens[..3]), 80);
        assert_eq!(linhas_mudadas_no_turno(&[usuario("oi"), fala("olá")]), 0);
    }

    /// Quando não roda: o outro fora (não instalado, ou no limite do plano), o chat em Planejar,
    /// o turno que já veio de uma revisão (nada de laço) e a opção desligada. Pelo clique, só o
    /// outro agente fora impede.
    #[test]
    fn quando_a_revisao_nao_roda() {
        assert_eq!(motivo_sem_revisao(&Turno { outro_instalado: false, ..turno_bom() }), Some(SemRevisao::SemOOutro));
        assert_eq!(motivo_sem_revisao(&Turno { outro_no_limite: true, ..turno_bom() }), Some(SemRevisao::OutroNoLimite));
        assert_eq!(motivo_sem_revisao(&Turno { planejando: true, ..turno_bom() }), Some(SemRevisao::Planejando));
        assert_eq!(motivo_sem_revisao(&Turno { veio_de_revisao: true, ..turno_bom() }), Some(SemRevisao::VeioDeRevisao));
        assert_eq!(motivo_sem_revisao(&Turno { ligada: false, ..turno_bom() }), Some(SemRevisao::Desligada));
        let qualquer = Turno { ligada: false, linhas: 0, planejando: true, veio_de_revisao: true, ..turno_bom() };
        assert_eq!(motivo_sem_revisao_pedida(&qualquer), None, "o clique revisa qualquer turno");
        assert_eq!(motivo_sem_revisao_pedida(&Turno { outro_instalado: false, ..qualquer }), Some(SemRevisao::SemOOutro));
        assert_eq!(motivo_sem_revisao_pedida(&Turno { outro_no_limite: true, ..qualquer }), Some(SemRevisao::OutroNoLimite));
        // O turno que a revisão mandada abriu é reconhecido pelo remetente da entrega.
        let (de, texto) = entrega_ao_principal(Agente::Codex, "Faltou o teste do desconto.");
        assert_eq!(de, "Revisão do Codex");
        assert!(texto.contains("Faltou o teste do desconto."), "{texto}");
        let da_revisao = Entrada { de: Some(de), texto, ..Entrada::default() };
        assert!(veio_de_revisao(&[da_revisao]));
        assert!(!veio_de_revisao(&[Entrada { texto: "arruma".into(), ..Entrada::default() }]));
        assert!(!veio_de_revisao(&[Entrada { de: Some("Vitral".into()), ..Entrada::default() }]));
    }

    /// O que aparece: sem nada a dizer, uma linha discreta; com achado, o cartão com o texto, sem
    /// nada mandado; e a revisão que falhou vira uma linha, não um erro do chat.
    #[test]
    fn o_que_a_revisao_deixa_na_conversa() {
        let leu = edicao("Read", 0, 0, false);
        let nada = resultado(Agente::Codex, &[leu.clone(), fala("Nada a acrescentar.")], "Nada a acrescentar.", false);
        assert!(matches!(&nada, Item::Aviso { texto, erro: false, .. } if texto == "Codex revisou: nada a acrescentar"), "{nada:?}");
        let do_claude = resultado(Agente::Claude, &[fala("nada a acrescentar")], "", false);
        assert!(matches!(&do_claude, Item::Aviso { texto, .. } if texto == "Claude revisou: nada a acrescentar"), "{do_claude:?}");
        // A última fala do revisor é a revisão (o que ele disse no caminho não entra).
        let achado = resultado(Agente::Codex, &[fala("Vou ler o diff."), leu, fala("Faltou o teste do desconto.")], "x", false);
        assert!(
            matches!(&achado, Item::Revisao { de: Agente::Codex, texto, mandada: false, .. } if texto == "Faltou o teste do desconto."),
            "{achado:?}"
        );
        // Sem fala gravada, vale o texto do fim do turno.
        let pelo_fim = resultado(Agente::Codex, &[], "O desconto ficou negativo.", false);
        assert!(matches!(&pelo_fim, Item::Revisao { texto, .. } if texto == "O desconto ficou negativo."), "{pelo_fim:?}");
        let falhou = resultado(Agente::Codex, &[], "limite do plano\nvolta às 14h", true);
        assert!(matches!(&falhou, Item::Aviso { texto, erro: false, .. } if texto == "A revisão do Codex não terminou: limite do plano"), "{falhou:?}");
        assert!(nada_a_dizer("Nada a acrescentar.") && nada_a_dizer("  nada a acrescentar  ") && !nada_a_dizer("Nada a acrescentar, mas o teste do desconto falta e isso pode quebrar a conta do carrinho inteiro no fim."));
        // O pedido diz ao revisor que ele só lê e como responder quando não tem nada.
        let pedido = pedido_de_revisao(Agente::Claude, &[], "");
        assert!(pedido.contains("Claude") && pedido.contains("responda só: Nada a acrescentar"), "{pedido}");
        assert!(pedido.contains("não mude"), "{pedido}");
    }

    /// O revisor entra como o parceiro do chat e, no fim, o turno dele some da conversa, o
    /// resultado entra no lugar e o chat volta a quem era: de um agente só, com a sessão dele.
    /// O revisor escolhido nas Configurações (26/11/2026): o Gemini num chat do Claude, com o
    /// modelo dele; o escolhido que é o próprio agente do chat cede a vez ao outro.
    #[test]
    fn o_revisor_escolhido_entra_com_o_modelo_dele() {
        let mut chat = Chat::novo("c".into(), "t", std::env::temp_dir());
        chat.modelo = Some("opus".into());
        let em_curso = abrir_com(&mut chat, Agente::Gemini, Some("gemini-3.8-flash".into()));
        assert_eq!(em_curso.revisor(), Agente::Gemini);
        assert_eq!((chat.agente, chat.modelo.as_deref()), (Agente::Gemini, Some("gemini-3.8-flash")));
        fechar(&mut chat, &em_curso, None);
        assert_eq!((chat.agente, chat.modelo.as_deref()), (Agente::Claude, Some("opus")), "o chat volta com o modelo dele");
        assert!(chat.parceiro.is_none());
        let mut chat = Chat::novo("d".into(), "t", std::env::temp_dir());
        let em_curso = abrir_com(&mut chat, Agente::Claude, None);
        assert_eq!(em_curso.revisor(), Agente::Codex, "num chat do Claude, o Claude escolhido cede ao Codex");
        // Uma revisão gravada antes do campo: o revisor é o outro agente.
        let antiga = EmCurso { revisor: None, ..em_curso };
        assert_eq!(antiga.revisor(), Agente::Codex);
    }

    #[test]
    fn a_revisao_entra_e_sai_sem_mexer_no_agente_do_chat() {
        let mut chat = Chat::novo("c".into(), "t", std::env::temp_dir());
        chat.itens = vec![usuario("arruma"), edicao("Edit", 20, 5, false), fala("pronto")];
        chat.sessao = "sessao-do-claude".into();
        chat.sessao_criada = true;
        let em_curso = abrir(&mut chat);
        assert_eq!(
            em_curso,
            EmCurso { desde_item: 3, principal: Agente::Claude, criou_o_parceiro: true, atualizado_antes: chat.atualizado_em, revisor: Some(Agente::Codex) }
        );
        assert_eq!(chat.agente, Agente::Codex, "o revisor está na vez");
        assert_ne!(chat.sessao, "sessao-do-claude", "na sessão dele");
        assert_eq!(chat.parceiro.as_ref().map(|p| (p.agente, p.sessao.as_str())), Some((Agente::Claude, "sessao-do-claude")));
        // O turno do revisor: a marca dele, uma leitura e o texto.
        chat.itens.extend([Item::Vez { agente: Agente::Codex, em: 0 }, edicao("Read", 0, 0, false), fala("Faltou o teste.")]);
        let item = resultado(chat.agente, &chat.itens[em_curso.desde_item..], "", false);
        fechar(&mut chat, &em_curso, Some(item));
        assert_eq!(chat.itens.len(), 4, "o turno do revisor saiu e o cartão entrou: {:?}", chat.itens);
        assert!(matches!(chat.itens.last(), Some(Item::Revisao { de: Agente::Codex, .. })));
        assert_eq!((chat.agente, chat.sessao.as_str(), chat.sessao_criada), (Agente::Claude, "sessao-do-claude", true));
        assert!(chat.parceiro.is_none(), "o chat continua de um agente só");
        assert!(chat.revisao_do_par.is_none(), "a revisão fechada sai do chat gravado");
        // Num chat que já era dos dois, o parceiro fica e a vez volta a quem estava.
        let mut dupla = Chat::novo("d".into(), "t", std::env::temp_dir());
        dupla.agente = Agente::Codex;
        dupla.parceiro = Some(Parceiro::novo(Agente::Claude));
        let sessao_do_claude = dupla.parceiro.as_ref().unwrap().sessao.clone();
        dupla.itens = vec![usuario("arruma"), fala("pronto")];
        let em_curso = abrir(&mut dupla);
        assert_eq!((em_curso.principal, em_curso.criou_o_parceiro), (Agente::Codex, false));
        assert_eq!((dupla.agente, dupla.sessao.as_str()), (Agente::Claude, sessao_do_claude.as_str()));
        dupla.itens.push(fala("Nada a acrescentar."));
        // Cancelada (a sua mensagem chegou): sai sem deixar nada.
        fechar(&mut dupla, &em_curso, None);
        assert_eq!(dupla.itens.len(), 2);
        assert_eq!(dupla.agente, Agente::Codex);
        assert_eq!(dupla.parceiro.as_ref().map(|p| p.agente), Some(Agente::Claude));
    }

    /// O Vitral fechado no meio da revisão: o chat foi gravado com o revisor na vez e o turno dele
    /// pela metade. A revisão em curso vai gravada junto, e a abertura seguinte a desfaz: o chat
    /// volta ao agente dele, com a sessão dele, o turno do revisor sai e fica a linha de que a
    /// revisão não terminou (o turno continua revisável pelo clique).
    #[test]
    fn a_revisao_cortada_pelo_vitral_fechado_se_desfaz_ao_abrir() {
        let mut chat = Chat::novo("c".into(), "t", std::env::temp_dir());
        chat.itens = vec![usuario("arruma"), edicao("Edit", 20, 5, false), fala("pronto")];
        chat.sessao = "sessao-do-claude".into();
        chat.sessao_criada = true;
        let em_curso = abrir(&mut chat);
        assert_eq!(chat.revisao_do_par.as_ref(), Some(&em_curso), "a revisão em curso vai no chat");
        chat.turno_aberto = true;
        chat.itens.extend([Item::Vez { agente: Agente::Codex, em: 0 }, edicao("Read", 0, 0, false)]);
        // O que o disco guarda e a abertura lê (as conversas vão à parte, como no `salvar_chat`).
        let itens = chat.itens.clone();
        let mut lido: Chat = serde_json::from_value(serde_json::to_value(&chat).unwrap()).unwrap();
        lido.itens = itens;
        assert_eq!(lido.revisao_do_par.as_ref(), Some(&em_curso));
        assert_eq!(lido.agente, Agente::Codex, "gravado com o revisor na vez");
        let mut chats = vec![lido, Chat::novo("outro".into(), "t", std::env::temp_dir())];
        assert_eq!(desfazer_as_cortadas(&mut chats), ["c"]);
        let c = &chats[0];
        assert_eq!((c.agente, c.sessao.as_str(), c.sessao_criada), (Agente::Claude, "sessao-do-claude", true));
        assert!(c.parceiro.is_none() && c.revisao_do_par.is_none() && !c.turno_aberto);
        assert_eq!(c.itens.len(), 4, "{:?}", c.itens);
        assert!(
            matches!(c.itens.last(), Some(Item::Aviso { texto, erro: false, .. }) if texto == "A revisão do Codex não terminou: o Vitral foi fechado no meio dela"),
            "{:?}",
            c.itens.last()
        );
        assert!(revisavel(&c.itens), "o clique ainda revisa o turno");
        // Sem revisão em curso nada muda, e a segunda abertura não repete.
        assert!(desfazer_as_cortadas(&mut chats).is_empty());
        assert_eq!(chats[0].itens.len(), 4);
        // O chat sem revisão não grava o campo.
        assert!(serde_json::to_value(&chats[1]).unwrap().get("revisao_do_par").is_none());

        // A marca mais velha que a conversa (o estado gravado antes, ou de volta do `.anterior`):
        // a revisão já tinha acabado e a conversa seguiu. Só o agente volta; nada sai.
        let mut seguiu = Chat::novo("s".into(), "t", std::env::temp_dir());
        seguiu.itens = vec![usuario("arruma"), fala("pronto")];
        let em_curso = abrir(&mut seguiu);
        // A conversa como a revisão fechada a gravou: o turno do revisor já saiu e o cartão entrou.
        seguiu.itens = vec![
            usuario("arruma"),
            fala("pronto"),
            Item::Revisao { de: Agente::Codex, texto: "Faltou o teste.".into(), em: 0, mandada: false },
            usuario("e agora?"),
            fala("feito"),
        ];
        let mut chats = vec![seguiu];
        assert_eq!(desfazer_as_cortadas(&mut chats), ["s"]);
        let s = &chats[0];
        assert_eq!(s.itens.len(), 5, "a conversa fica inteira: {:?}", s.itens);
        assert_eq!(s.agente, em_curso.principal);
        assert!(s.parceiro.is_none() && s.revisao_do_par.is_none());
    }

    /// A ação "Revisar com o Codex" só faz sentido com um turno pronto do agente do chat que
    /// ainda não foi revisto.
    #[test]
    fn so_o_turno_pronto_e_ainda_nao_revisto_pode_ser_revisto() {
        assert!(!revisavel(&[]));
        assert!(!revisavel(&[usuario("oi")]), "sem resposta ainda");
        assert!(revisavel(&[usuario("oi"), fala("olá")]));
        assert!(revisavel(&[usuario("oi"), edicao("Edit", 1, 1, false), fala("olá"), Item::Aviso { texto: "Sessão nova".into(), erro: false, em: 0 }]));
        let revista = Item::Revisao { de: Agente::Codex, texto: "x".into(), em: 0, mandada: false };
        assert!(!revisavel(&[usuario("oi"), fala("olá"), revista.clone()]), "já tem a revisão deste turno");
        let sem_achado = resultado(Agente::Codex, &[], "Nada a acrescentar", false);
        assert!(!revisavel(&[usuario("oi"), fala("olá"), sem_achado]));
        assert!(revisavel(&[usuario("oi"), fala("olá"), revista, usuario("e isso?"), fala("feito")]), "o turno seguinte ainda não");
    }

    /// O item novo vai e volta do disco, e a chave antiga "Os dois em toda mensagem" continua
    /// abrindo (item 4): é lida e não é mais gravada.
    #[test]
    fn a_revisao_e_a_chave_antiga_no_disco() {
        let item = Item::Revisao { de: Agente::Codex, texto: "Faltou o teste.".into(), em: 7, mandada: true };
        let gravado = serde_json::to_value(&item).unwrap();
        assert_eq!(gravado, serde_json::json!({ "Revisao": { "de": "codex", "texto": "Faltou o teste.", "em": 7, "mandada": true } }));
        assert!(matches!(serde_json::from_value::<Item>(gravado).unwrap(), Item::Revisao { mandada: true, em: 7, .. }));
        let mut antigo = serde_json::to_value(Chat::novo("c".into(), "t", std::env::temp_dir())).unwrap();
        antigo["revisao_em_dupla"] = serde_json::json!(true);
        let chat: Chat = serde_json::from_value(antigo).expect("o chat com a chave ligada abre");
        assert!(serde_json::to_value(&chat).unwrap().get("revisao_em_dupla").is_none(), "a chave não é mais gravada");
        assert!(Config::default().revisar_edicoes, "a revisão automática vem ligada");
    }
}
