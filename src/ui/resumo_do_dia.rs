//! "Resumo de hoje" (menu do logo): o que os chats fizeram no dia, por pasta, montado só do que o
//! app já tem (os itens das conversas e o estado de cada chat), sem chamar modelo nenhum. Cada
//! chat que trabalhou no período entra com o título, a primeira frase da última resposta, os
//! arquivos que mudou, o tempo trabalhado e o que ficou pendente (parado, com erro, esperando
//! você). Um seletor troca para Ontem e 7 dias, e "Copiar como Markdown" leva o texto para a área
//! de transferência.
//!
//! O tempo trabalhado é a soma dos intervalos entre um item e o seguinte dentro de cada turno;
//! um intervalo de mais de 10 minutos sem nada acontecer conta como espera (um pedido de
//! permissão parado, o Vitral fechado) e entra só até esse teto.

use std::path::Path;

use gpui_kit::component::{WindowExt as _, h_flex, scroll::ScrollableElement as _, tooltip::{ManagedTooltipExt as _, Tooltip}, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{ControleSegmentado, EstadoVazio, Reflexo as _, TamanhoBotao, Tom, aviso, botao_fantasma, contagem, duracao_curta, largura},
    dialogos::{cabecalho, moldura, topo},
};
use crate::{
    modelo::Item,
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, dentro_da_borda, esp, r, raio, tam},
};

/// O maior intervalo entre dois itens seguidos que ainda conta inteiro como trabalho.
const TETO_DO_INTERVALO: u64 = 600;

/// Quantos arquivos de um chat a tela lista antes do "e mais N" (o Markdown leva todos).
const ARQUIVOS_NA_TELA: usize = 8;

/// O período do resumo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Periodo {
    Hoje,
    Ontem,
    SeteDias,
}

impl Periodo {
    const TODOS: [Periodo; 3] = [Periodo::Hoje, Periodo::Ontem, Periodo::SeteDias];

    fn rotulo(self) -> &'static str {
        match self {
            Periodo::Hoje => "Hoje",
            Periodo::Ontem => "Ontem",
            Periodo::SeteDias => "7 dias",
        }
    }

    fn titulo(self) -> &'static str {
        match self {
            Periodo::Hoje => "Resumo de hoje",
            Periodo::Ontem => "Resumo de ontem",
            Periodo::SeteDias => "Resumo dos últimos 7 dias",
        }
    }

    fn vazio(self) -> &'static str {
        match self {
            Periodo::Hoje => "Nenhum chat trabalhou hoje",
            Periodo::Ontem => "Nenhum chat trabalhou ontem",
            Periodo::SeteDias => "Nenhum chat trabalhou nos últimos 7 dias",
        }
    }

    /// O intervalo do período, em segundos (o fim não entra): hoje, do começo do dia até agora;
    /// ontem, o dia inteiro; 7 dias, do começo do dia de seis dias atrás até agora. `comeco_do_dia`
    /// devolve a meia-noite local do dia de um instante.
    fn intervalo(self, agora: u64, comeco_do_dia: impl Fn(u64) -> u64) -> (u64, u64) {
        let hoje = comeco_do_dia(agora);
        match self {
            Periodo::Hoje => (hoje, agora + 1),
            Periodo::Ontem => (comeco_do_dia(hoje.saturating_sub(1)), hoje),
            // Do meio do dia para trás: a hora de verão não joga a conta para o dia vizinho.
            Periodo::SeteDias => (comeco_do_dia((hoje + 43_200).saturating_sub(6 * 86_400)), agora + 1),
        }
    }
}

/// A meia-noite local do dia de um instante.
fn meia_noite_local(instante: u64) -> u64 {
    use chrono::{Local, TimeDelta};
    let dia = crate::modelo::no_fuso_local(instante as i64).date_naive().and_time(chrono::NaiveTime::MIN);
    // Onde a meia-noite não existe (a virada da hora de verão), a primeira hora que existe.
    let inicio = [dia, dia + TimeDelta::hours(1)].into_iter().find_map(|hora| hora.and_local_timezone(Local).earliest());
    match inicio.map(|i| i.timestamp()) {
        Some(inicio) if inicio >= 0 => inicio as u64,
        _ => instante - instante % 86_400,
    }
}

/// Um arquivo que o chat mudou no período.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ArquivoNoResumo {
    pub caminho: String,
    pub entram: usize,
    pub saem: usize,
    pub novo: bool,
}

/// Um chat que trabalhou no período.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChatNoResumo {
    pub id: String,
    pub titulo: String,
    /// A primeira frase da última resposta do período (vazia, se ele só usou ferramentas).
    pub fala: String,
    pub arquivos: Vec<ArquivoNoResumo>,
    /// Segundos trabalhados no período.
    pub trabalhado: u64,
    pub turnos: usize,
    /// "Parada · Vitral fechou", "Erro · …", "Permissão · …"; vazio quando nada espera.
    pub pendente: String,
    /// O último instante em que ele fez algo no período (ordena os chats e as pastas).
    pub ultimo: u64,
}

/// Os chats de uma pasta que trabalharam no período.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct PastaNoResumo {
    pub pasta: String,
    pub chats: Vec<ChatNoResumo>,
}

/// O caminho como o resumo o mostra: relativo à pasta do chat quando está nela.
fn relativo(caminho: &str, pasta: &Path) -> String {
    Path::new(caminho).strip_prefix(pasta).ok().map(crate::git::caminho_em_texto).filter(|c| !c.is_empty()).unwrap_or_else(|| caminho.to_string())
}

/// O que um chat fez no período `[inicio, fim)`, pelos itens dele. `None` quando o agente não fez
/// nada nele (nenhuma resposta nem chamada com hora dentro do período).
pub(super) fn do_chat(id: &str, titulo: &str, itens: &[Item], pasta: &Path, pendente: &str, inicio: u64, fim: u64) -> Option<ChatNoResumo> {
    let dentro = |em: u64| em >= inicio && em < fim;
    let mut fala = String::new();
    let mut arquivos: Vec<ArquivoNoResumo> = Vec::new();
    let (mut trabalhado, mut turnos, mut ultimo) = (0u64, 0usize, 0u64);
    // O turno aberto: o instante do último item dele e se já contou como turno do período.
    let mut anterior: Option<u64> = None;
    let mut contado = false;
    for item in itens {
        let em = item.em();
        if matches!(item, Item::Usuario { .. } | Item::Entrega { .. }) {
            anterior = (em > 0).then_some(em);
            contado = false;
            continue;
        }
        if em == 0 {
            continue;
        }
        // O pedaço do intervalo desde o item anterior que cai no período, até o teto.
        if let Some(antes) = anterior {
            let (de, ate) = (antes.max(inicio), em.min(fim));
            trabalhado += ate.saturating_sub(de).min(TETO_DO_INTERVALO);
        }
        anterior = Some(em);
        if !dentro(em) {
            continue;
        }
        match item {
            Item::Assistente { texto, .. } => {
                let frase = super::falas::primeira_frase(texto);
                if !frase.is_empty() {
                    fala = frase;
                }
            }
            Item::Ferramenta { nome, arquivo, resumo, erro: false, mudanca, .. }
                if matches!(super::mensagens::nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") =>
            {
                let caminho = relativo(arquivo.as_deref().unwrap_or(resumo), pasta);
                let (entram, saem) = super::mensagens::linhas_da_edicao_gravada(item).unwrap_or((0, 0));
                let novo = mudanca.is_some_and(|m| m.novo);
                match arquivos.iter_mut().find(|a| a.caminho == caminho) {
                    Some(a) => {
                        a.entram += entram;
                        a.saem += saem;
                        a.novo |= novo;
                    }
                    None => arquivos.push(ArquivoNoResumo { caminho, entram, saem, novo }),
                }
            }
            Item::Ferramenta { .. } | Item::Raciocinio { .. } => {}
            // Aviso, cartão e troca de vez não são trabalho do agente.
            _ => continue,
        }
        ultimo = ultimo.max(em);
        if !std::mem::replace(&mut contado, true) {
            turnos += 1;
        }
    }
    (ultimo > 0).then(|| ChatNoResumo { id: id.to_string(), titulo: titulo.to_string(), fala, arquivos, trabalhado, turnos, pendente: pendente.to_string(), ultimo })
}

/// Junta os chats por pasta: a pasta mexida por último primeiro e, em cada uma, o chat mexido por
/// último primeiro.
pub(super) fn por_pasta(chats: Vec<(String, ChatNoResumo)>) -> Vec<PastaNoResumo> {
    let mut pastas: Vec<PastaNoResumo> = Vec::new();
    for (pasta, chat) in chats {
        match pastas.iter_mut().find(|p| p.pasta == pasta) {
            Some(p) => p.chats.push(chat),
            None => pastas.push(PastaNoResumo { pasta, chats: vec![chat] }),
        }
    }
    for pasta in &mut pastas {
        pasta.chats.sort_by_key(|c| std::cmp::Reverse(c.ultimo));
    }
    pastas.sort_by_key(|p| std::cmp::Reverse(p.chats.first().map_or(0, |c| c.ultimo)));
    pastas
}

/// "42 min 10 s em 3 turnos".
fn tempo_e_turnos(chat: &ChatNoResumo) -> String {
    format!("{} em {}", duracao_curta(chat.trabalhado), contagem(chat.turnos, "turno", "turnos"))
}

/// "+10 −2", "novo, +40", ou nada quando a chamada não guardou a conta.
fn linhas_do_arquivo(a: &ArquivoNoResumo) -> String {
    let mut partes = Vec::new();
    if a.novo {
        partes.push("novo".to_string());
    }
    let mut conta = Vec::new();
    if a.entram > 0 {
        conta.push(format!("+{}", a.entram));
    }
    if a.saem > 0 {
        conta.push(format!("−{}", a.saem));
    }
    if !conta.is_empty() {
        partes.push(conta.join(" "));
    }
    partes.join(", ")
}

/// O resumo em Markdown: um título, uma seção por pasta e, em cada chat, a frase, o tempo, o que
/// ficou pendente e os arquivos.
pub(super) fn markdown(titulo: &str, pastas: &[PastaNoResumo]) -> String {
    let mut saida = format!("# {titulo}\n");
    if pastas.is_empty() {
        saida.push_str("\nNenhum chat trabalhou no período.\n");
    }
    for pasta in pastas {
        saida.push_str(&format!("\n## {}\n", pasta.pasta));
        for chat in &pasta.chats {
            saida.push_str(&format!("\n### {}\n\n", chat.titulo));
            if !chat.fala.is_empty() {
                saida.push_str(&format!("{}\n\n", chat.fala));
            }
            saida.push_str(&format!("- Trabalhou {}\n", tempo_e_turnos(chat)));
            if !chat.pendente.is_empty() {
                saida.push_str(&format!("- Pendente: {}\n", chat.pendente));
            }
            if !chat.arquivos.is_empty() {
                saida.push_str(&format!("- {}:\n", contagem(chat.arquivos.len(), "arquivo mudado", "arquivos mudados")));
                for a in &chat.arquivos {
                    let linhas = linhas_do_arquivo(a);
                    if linhas.is_empty() {
                        saida.push_str(&format!("  - `{}`\n", a.caminho));
                    } else {
                        saida.push_str(&format!("  - `{}` ({linhas})\n", a.caminho));
                    }
                }
            }
        }
    }
    saida
}

/// O que ficou pendente num chat, pelo estado dele agora: o que roda, parou, falhou ou espera
/// você. A resposta que você ainda não leu ("Terminou") não é pendência.
fn pendencia(estado: String) -> String {
    if estado == "Terminou" { String::new() } else { estado }
}

/// O resumo do período, lido do motor: todos os chats (as arquivadas também), por pasta.
fn montar(motor: &Motor, periodo: Periodo) -> Vec<PastaNoResumo> {
    let (inicio, fim) = periodo.intervalo(crate::modelo::agora(), meia_noite_local);
    let chats = motor
        .chats
        .iter()
        .filter_map(|c| {
            let pendente = pendencia(super::falas::estado_do_chat(motor, c));
            let chat = do_chat(&c.id, &c.titulo, &c.itens, &c.cwd, &pendente, inicio, fim)?;
            let pasta = if motor.sem_pasta(c) { "Sem pasta".to_string() } else { super::dialogos::com_til(&c.cwd) };
            Some((pasta, chat))
        })
        .collect();
    por_pasta(chats)
}

/// O diálogo do resumo: o período escolhido e o que foi montado para ele (refeito só ao trocar de
/// período, não a cada quadro).
pub(super) struct ResumoDoDia {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    periodo: Periodo,
    pastas: Vec<PastaNoResumo>,
    rolagem: ScrollHandle,
}

impl ResumoDoDia {
    fn new(motor: Entity<Motor>, janela: WeakEntity<Janela>, cx: &mut Context<Self>) -> Self {
        let pastas = montar(motor.read(cx), Periodo::Hoje);
        ResumoDoDia { motor, janela, periodo: Periodo::Hoje, pastas, rolagem: ScrollHandle::new() }
    }

    fn trocar(&mut self, periodo: Periodo, cx: &mut Context<Self>) {
        self.periodo = periodo;
        self.pastas = montar(self.motor.read(cx), periodo);
        self.rolagem.set_offset(point(px(0.), px(0.)));
        cx.notify();
    }

    /// Um chat do resumo: o título (o clique abre o chat), a frase, o tempo, a pendência e os
    /// arquivos.
    fn chat(&self, chave: usize, chat: &ChatNoResumo) -> AnyElement {
        let (janela, id) = (self.janela.clone(), chat.id.clone());
        let arquivos = chat.arquivos.iter().take(ARQUIVOS_NA_TELA).map(|a| {
            let (pasta, nome) = a.caminho.rsplit_once('/').unwrap_or(("", a.caminho.as_str()));
            h_flex()
                .gap(esp::E8)
                .min_h(r(22.))
                .child(div().flex_shrink(1.).min_w_0().truncate().corpo_ui().text_color(Cores::texto()).child(nome.to_string()))
                .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).child(pasta.to_string()))
                .child(
                    h_flex()
                        .flex_none()
                        .gap(esp::E6)
                        .font_family(FONTE_MONO)
                        .metadado()
                        .when(a.novo, |s| s.child(div().text_color(Cores::verde()).child("novo")))
                        .when(a.entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{}", a.entram))))
                        .when(a.saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{}", a.saem)))),
                )
        });
        let a_mais = chat.arquivos.len().saturating_sub(ARQUIVOS_NA_TELA);
        v_flex()
            .gap(esp::E4)
            .p(dentro_da_borda(12.))
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::linha())
            .reflexo(raio::cartao())
            .child(
                div()
                    .id(("resumo-do-dia-chat", chave))
                    .rotulo()
                    .text_color(Cores::forte())
                    .cursor_pointer()
                    .hover(|s| s.text_color(Cores::destaque_texto()))
                    .child(chat.titulo.clone())
                    .dica_no_ponteiro(|window, cx| Tooltip::new("Abrir este chat").build(window, cx))
                    .on_click(move |_, window, cx| {
                        window.close_dialog(cx);
                        let _ = janela.update(cx, |this, cx| this.selecionar(Some(id.clone()), window, cx));
                    }),
            )
            .when(!chat.fala.is_empty(), |s| s.child(div().corpo_ui().text_color(Cores::texto()).child(chat.fala.clone())))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_x(esp::E8)
                    .metadado()
                    .text_color(Cores::tenue())
                    .child(format!("Trabalhou {}", tempo_e_turnos(chat)))
                    .when(!chat.pendente.is_empty(), |s| s.child(div().text_color(Cores::espera()).child(chat.pendente.clone()))),
            )
            .when(!chat.arquivos.is_empty(), |s| s.child(v_flex().pt(esp::E4).children(arquivos)))
            .when(a_mais > 0, |s| s.child(div().metadado().text_color(Cores::tenue()).child(format!("e mais {}", contagem(a_mais, "arquivo", "arquivos")))))
            .into_any_element()
    }
}

impl Render for ResumoDoDia {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let vista = cx.entity().downgrade();
        let atual = Periodo::TODOS.iter().position(|p| *p == self.periodo).unwrap_or(0);
        let seletor = ControleSegmentado::new("periodo-do-resumo", Periodo::TODOS.map(Periodo::rotulo), atual, move |i, _, cx| {
            let _ = vista.update(cx, |v, cx| v.trocar(Periodo::TODOS[i.min(2)], cx));
        });
        let texto = markdown(self.periodo.titulo(), &self.pastas);
        let copiar = botao_fantasma("copiar-resumo-do-dia", Some(Lucide::Copy.into()), "Copiar como Markdown", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(texto.clone()));
                window.push_notification(aviso(Tom::Pronto, "Resumo copiado em Markdown."), cx);
            });
        // A altura acompanha a janela, e o corpo rola.
        let altura = (window.viewport_size().height - tam(window, 280.)).max(tam(window, 200.)).min(tam(window, 620.));
        let mut chave = 0;
        let mut corpo = v_flex().gap(esp::E8);
        for pasta in &self.pastas {
            corpo = corpo.child(
                h_flex()
                    .pt(esp::E8)
                    .gap(esp::E6)
                    .child(div().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::Folder, 14., window)))
                    .child(div().rotulo().text_color(Cores::tenue()).child(pasta.pasta.clone()))
                    .child(div().font_family(FONTE_MONO).metadado().text_color(Cores::apagado()).child(pasta.chats.len().to_string())),
            );
            for chat in &pasta.chats {
                corpo = corpo.child(self.chat(chave, chat));
                chave += 1;
            }
        }
        let corpo = if self.pastas.is_empty() {
            div()
                .h(altura)
                .flex()
                .items_center()
                .justify_center()
                .child(EstadoVazio::new(Lucide::CalendarClock, self.periodo.vazio()).texto("Só entra o chat em que o agente respondeu ou usou uma ferramenta no período."))
                .into_any_element()
        } else {
            div()
                .id("corpo-do-resumo-do-dia")
                .max_h(altura)
                .overflow_y_scroll()
                .track_scroll(&self.rolagem)
                .child(corpo)
                .vertical_scrollbar(&self.rolagem)
                .into_any_element()
        };
        v_flex()
            .w_full()
            .gap(esp::E12)
            .child(h_flex().gap(esp::E8).child(seletor).child(div().flex_1()).child(copiar))
            .child(corpo)
    }
}

impl Janela {
    /// "Resumo de hoje", no menu do logo.
    pub(super) fn abrir_resumo_do_dia(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else { return };
        let janela = cx.entity().downgrade();
        let vista = cx.new(|cx| ResumoDoDia::new(motor, janela, cx));
        self.foco_antes_do_dialogo(window, cx);
        window.open_dialog(cx, move |dialogo, window, cx| {
            let v = vista.read(cx);
            let chats: usize = v.pastas.iter().map(|p| p.chats.len()).sum();
            let subtitulo = match chats {
                0 => "O que os chats fizeram, por pasta".to_string(),
                n => format!("{} em {}", contagem(n, "chat", "chats"), contagem(v.pastas.len(), "pasta", "pastas")),
            };
            moldura(dialogo, largura::G, window)
                .title(topo(cabecalho(v.periodo.titulo(), Some(subtitulo))))
                .on_ok(|_, _, _| false)
                .child(vista.clone())
        });
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use std::path::Path;

    use super::{ArquivoNoResumo, ChatNoResumo, Periodo, do_chat, markdown, pendencia, por_pasta};
    use crate::modelo::{Item, Mudanca};

    fn usuario(texto: &str, em: u64) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em, marca: None }
    }

    fn resposta(texto: &str, em: u64) -> Item {
        Item::Assistente { texto: texto.into(), em }
    }

    fn chamada(nome: &str, arquivo: Option<&str>, mudanca: Option<(u32, u32, bool)>, em: u64) -> Item {
        Item::Ferramenta {
            id: "f".into(),
            nome: nome.into(),
            resumo: String::new(),
            resultado: Some("ok".into()),
            erro: false,
            em,
            detalhe: None,
            arquivo: arquivo.map(str::to_string),
            sub: None,
            mudanca: mudanca.map(|(entram, saem, novo)| Mudanca { entram, saem, novo }),
        }
    }

    /// O dia de teste vai de 1000 a 2000.
    const DIA: (u64, u64) = (1000, 2000);

    #[test]
    fn o_chat_no_periodo_pelos_itens_da_conversa() {
        let itens = vec![
            // Ontem: fora do período.
            usuario("arruma o frete", 500),
            chamada("Edit", Some("/p/loja/src/velho.rs"), Some((9, 9, false)), 510),
            resposta("Feito ontem.", 520),
            // Hoje: dois turnos.
            usuario("agora o cep", 1100),
            chamada("Edit", Some("/p/loja/src/cep.rs"), Some((3, 1, false)), 1110),
            chamada("Bash", None, None, 1120),
            chamada("Write", Some("/p/loja/docs/cep.md"), Some((40, 0, true)), 1130),
            chamada("Edit", Some("/p/loja/src/cep.rs"), Some((2, 2, false)), 1140),
            resposta("**Pronto**, o CEP valida. Faltam os testes.", 1150),
            usuario("roda os testes", 1200),
            // Uma espera longa (um pedido de permissão parado) conta só até o teto.
            chamada("Bash", None, None, 1210),
            resposta("Passaram os 12.", 1900),
        ];
        let chat = do_chat("c1", "CEP", &itens, Path::new("/p/loja"), "", DIA.0, DIA.1).unwrap();
        assert_eq!(chat.fala, "Passaram os 12.");
        assert_eq!(
            chat.arquivos,
            [
                ArquivoNoResumo { caminho: "src/cep.rs".into(), entram: 5, saem: 3, novo: false },
                ArquivoNoResumo { caminho: "docs/cep.md".into(), entram: 40, saem: 0, novo: true },
            ]
        );
        assert_eq!(chat.turnos, 2);
        // 50 s do primeiro turno; no segundo, 10 s e o intervalo de 690 s cortado em 600.
        assert_eq!(chat.trabalhado, 50 + 10 + 600);
        assert_eq!(chat.ultimo, 1900);
        // Ontem, só o primeiro turno; num dia sem nada, o chat não entra.
        let ontem = do_chat("c1", "CEP", &itens, Path::new("/p/loja"), "", 0, 1000).unwrap();
        assert_eq!((ontem.fala.as_str(), ontem.turnos, ontem.trabalhado, ontem.arquivos.len()), ("Feito ontem.", 1, 20, 1));
        assert_eq!(do_chat("c1", "CEP", &itens, Path::new("/p/loja"), "", 3000, 4000), None);
        // Só a sua mensagem, sem o agente ter feito nada: não trabalhou.
        assert_eq!(do_chat("c2", "Vazio", &[usuario("oi", 1500)], Path::new("/p"), "", DIA.0, DIA.1), None);
        // Um turno que atravessa a meia-noite conta em cada dia só o pedaço dele.
        let virada = vec![usuario("vira", 990), chamada("Bash", None, None, 995), resposta("ok", 1010)];
        assert_eq!(do_chat("c3", "Virada", &virada, Path::new("/p"), "", DIA.0, DIA.1).unwrap().trabalhado, 10);
        assert_eq!(do_chat("c3", "Virada", &virada, Path::new("/p"), "", 0, 1000).unwrap().trabalhado, 10);
    }

    fn chat(id: &str, titulo: &str, ultimo: u64) -> ChatNoResumo {
        ChatNoResumo { id: id.into(), titulo: titulo.into(), fala: String::new(), arquivos: Vec::new(), trabalhado: 65, turnos: 1, pendente: String::new(), ultimo }
    }

    #[test]
    fn agrupa_por_pasta_e_o_markdown_fica_legivel() {
        let mut frete = chat("a", "Frete por CEP", 1500);
        frete.fala = "Pronto, o frete sai pelo CEP.".into();
        frete.pendente = "Parada · Vitral fechou".into();
        frete.arquivos = vec![
            ArquivoNoResumo { caminho: "src/frete.rs".into(), entram: 10, saem: 2, novo: false },
            ArquivoNoResumo { caminho: "docs/frete.md".into(), entram: 40, saem: 0, novo: true },
            ArquivoNoResumo { caminho: "antigo.txt".into(), entram: 0, saem: 0, novo: false },
        ];
        let pastas = por_pasta(vec![
            ("~/loja".into(), chat("b", "Banner", 1200)),
            ("~/central".into(), chat("c", "Pesagem", 1900)),
            ("~/loja".into(), frete),
        ]);
        // A pasta mexida por último primeiro; em cada uma, o chat mexido por último primeiro.
        let ordem: Vec<(&str, Vec<&str>)> = pastas.iter().map(|p| (p.pasta.as_str(), p.chats.iter().map(|c| c.titulo.as_str()).collect())).collect();
        assert_eq!(ordem, [("~/central", vec!["Pesagem"]), ("~/loja", vec!["Frete por CEP", "Banner"])]);
        assert_eq!(
            markdown("Resumo de hoje", &pastas),
            "# Resumo de hoje\n\
             \n## ~/central\n\
             \n### Pesagem\n\n\
             - Trabalhou 1 min 05 s em 1 turno\n\
             \n## ~/loja\n\
             \n### Frete por CEP\n\n\
             Pronto, o frete sai pelo CEP.\n\n\
             - Trabalhou 1 min 05 s em 1 turno\n\
             - Pendente: Parada · Vitral fechou\n\
             - 3 arquivos mudados:\n\
             \x20 - `src/frete.rs` (+10 −2)\n\
             \x20 - `docs/frete.md` (novo, +40)\n\
             \x20 - `antigo.txt`\n\
             \n### Banner\n\n\
             - Trabalhou 1 min 05 s em 1 turno\n"
        );
        assert_eq!(markdown("Resumo de ontem", &[]), "# Resumo de ontem\n\nNenhum chat trabalhou no período.\n");
    }

    #[test]
    fn os_periodos_pelo_comeco_do_dia() {
        // Dias de 1000 segundos, para a conta ficar à vista.
        let comeco = |t: u64| t - t % 1000;
        assert_eq!(Periodo::Hoje.intervalo(7_450, comeco), (7_000, 7_451));
        assert_eq!(Periodo::Ontem.intervalo(7_450, comeco), (6_000, 7_000));
        // Com o dia de verdade: seis dias antes do começo de hoje.
        let dia = |t: u64| t - t % 86_400;
        let agora = 20 * 86_400 + 3_600;
        assert_eq!(Periodo::SeteDias.intervalo(agora, dia), (14 * 86_400, agora + 1));
        assert_eq!(Periodo::Hoje.intervalo(agora, dia), (20 * 86_400, agora + 1));
        assert_eq!(Periodo::Ontem.intervalo(agora, dia), (19 * 86_400, 20 * 86_400));
        // A resposta que você não leu não é pendência; o resto, sim.
        assert_eq!(pendencia("Terminou".into()), "");
        assert_eq!(pendencia("Parada · Vitral fechou".into()), "Parada · Vitral fechou");
    }
}
