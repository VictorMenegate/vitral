//! A linha de pílulas acima da caixa de mensagem de cada painel: o que acontece naquele chat sem
//! ocupar a conversa. Os comandos que o agente deixou no fundo, os subagentes por estado, os
//! passos do agente e o que o turno mudou. A linha só existe quando há pílula; ela flutua sobre o
//! fim da conversa e não empurra a caixa. Referência: as "tracks" do Paseo (23/11/2026).

use gpui_kit::component::{
    button::{Button, ButtonVariants as _},
    h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{AnelGirando, CorpoPopover, Flutuante as _, LarguraPopover, TamanhoBotao, botao_fantasma, contagem, popover, variante_fantasma},
    mapa_agentes::{Estado, agentes_do_chat, e_agente, meta_do_agente, ponto},
    mensagens::{coluna, icone_de, larga, linhas_da_edicao_gravada, nome_curto, texto_dos_passos},
    movimento::{self, De},
};
use crate::{
    modelo::{Chat, Item, agora},
    motor::{EstadoDoPasso, Motor, Passo},
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, r, raio},
};

/// Quanto do pé da conversa a linha ocupa, a 100%: a pílula e o respiro em cima e embaixo dela. A
/// lista de mensagens guarda esse espaço no fim enquanto a linha existe (`mensagens`), para a
/// última linha da conversa não ficar atrás das pílulas.
pub(super) const ALTURA_DA_LINHA: f32 = 32.;
const ALTURA_DA_PILULA: f32 = 24.;

/// O estado de um subagente na pílula.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NaPilula {
    Trabalhando,
    /// Pediu uma permissão e espera a sua resposta.
    EsperandoVoce,
    Falhou,
}

/// O que a linha de um chat mostra agora. Tudo vazio: a linha não existe.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Pilulas {
    /// Os comandos que o agente deixou rodando no fundo: o id da tarefa e a descrição.
    pub comandos: Vec<(String, String)>,
    /// Os subagentes do turno por estado, na ordem: trabalhando, esperando você, falhou. Só os
    /// estados que têm alguém.
    pub agentes: Vec<(NaPilula, usize)>,
    /// Os passos do agente: quantos feitos e quantos ao todo.
    pub passos: Option<(usize, usize)>,
    /// O que o turno mudou: quantos arquivos, as linhas que entraram e as que saíram.
    pub mudancas: Option<(usize, usize, usize)>,
}

impl Pilulas {
    /// Nada a mostrar: a linha não é desenhada.
    pub fn vazia(&self) -> bool {
        self.comandos.is_empty() && self.agentes.is_empty() && self.passos.is_none() && self.mudancas.is_none()
    }
}

/// As pílulas de um chat. `rodando`: há um turno em andamento; `comandos`: o que o motor diz que
/// está no fundo; `esperando`: as chamadas dos subagentes com pedido de permissão pendente;
/// `passos`: a lista do agente; `itens`: a conversa. Roda a cada desenho da casca de cada painel:
/// só conta, sem copiar nada da conversa.
pub(super) fn montar(rodando: bool, comandos: &[(String, String)], esperando: &[String], passos: &[Passo], itens: &[Item], _agora: u64) -> Pilulas {
    // O turno: o que veio depois da sua última mensagem (como no `mudancas_do_chat`).
    let turno = itens.iter().rposition(|i| matches!(i, Item::Usuario { .. })).unwrap_or(0);
    let mut agentes = Vec::new();
    if rodando {
        // Quem trabalha: a chamada sem resultado e sem aviso depois dela (o turno que para ou
        // falha deixa um aviso), como no `agentes_do_chat`. O que falhou só conta neste turno.
        let ultimo_aviso = itens.iter().rposition(Item::aviso_que_fecha);
        let marco = inicio_do_turno(itens);
        let (mut trabalham, mut esperam, mut falharam) = (0, 0, 0);
        for (posicao, item) in itens.iter().enumerate().filter(|(_, item)| e_agente(item)) {
            let Item::Ferramenta { id, resultado, erro, .. } = item else {
                continue;
            };
            match resultado {
                None if ultimo_aviso.is_some_and(|aviso| aviso > posicao) => {}
                None if esperando.contains(id) => esperam += 1,
                None => trabalham += 1,
                Some(_) if *erro && posicao >= marco => falharam += 1,
                Some(_) => {}
            }
        }
        let por_estado = [(NaPilula::Trabalhando, trabalham), (NaPilula::EsperandoVoce, esperam), (NaPilula::Falhou, falharam)];
        agentes.extend(por_estado.into_iter().filter(|(_, n)| *n > 0));
    }
    // O que o turno editou: cada arquivo uma vez, e as linhas de todas as edições dele.
    let mut arquivos: Vec<&str> = Vec::new();
    let (mut entram, mut saem) = (0, 0);
    for item in itens.iter().skip(turno + 1) {
        let Item::Ferramenta { nome, arquivo, resumo, erro: false, .. } = item else {
            continue;
        };
        if !matches!(nome_curto(nome), "Edit" | "MultiEdit" | "Write" | "NotebookEdit") {
            continue;
        }
        let caminho = arquivo.as_deref().unwrap_or(resumo);
        if !arquivos.contains(&caminho) {
            arquivos.push(caminho);
        }
        let (mais, menos) = linhas_da_edicao_gravada(item).unwrap_or((0, 0));
        (entram, saem) = (entram + mais, saem + menos);
    }
    let feitos = passos.iter().filter(|p| p.estado == EstadoDoPasso::Feito).count();
    Pilulas {
        comandos: comandos.to_vec(),
        agentes,
        passos: (rodando && !passos.is_empty()).then_some((feitos, passos.len())),
        mudancas: (!arquivos.is_empty()).then_some((arquivos.len(), entram, saem)),
    }
}

/// Onde o turno de agora começa: depois da sua última mensagem ou da última entrega (de outro
/// chat, ou do Vitral pedindo a revisão da memória). Zero sem nenhuma.
fn inicio_do_turno(itens: &[Item]) -> usize {
    itens.iter().rposition(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. })).map_or(0, |i| i + 1)
}

/// As pílulas do chat agora, pelo que o motor sabe. A conversa de um subagente, que é só para
/// ler, não tem linha.
pub(super) fn pilulas_do_chat(motor: &Motor, chat: &Chat) -> Pilulas {
    if crate::motor::subconversa::partes_do_id(&chat.id).is_some() {
        return Pilulas::default();
    }
    let rodando = motor.execucao(&chat.id).rodando();
    let esperando = if rodando { motor.subagentes_esperando(&chat.id) } else { Vec::new() };
    montar(rodando, motor.comandos_no_fundo(&chat.id), &esperando, motor.passos(&chat.id), &chat.itens, agora())
}

/// A linha de baixo de um subagente na lista da pílula: o que ele faz agora, com as mesmas
/// palavras da linha dele na conversa; depois de terminar, quanto levou e quanto usou.
fn meta_na_lista(a: &super::mapa_agentes::Agente) -> String {
    match a.estado {
        Estado::Rodando => super::mensagens::no_fundo::acao_ao_vivo(&a.atividade),
        _ => meta_do_agente(a),
    }
}

/// "2 comandos no fundo"; no painel estreito, só o número (o ícone diz o resto).
pub(super) fn rotulo_dos_comandos(n: usize, curto: bool) -> String {
    if curto { n.to_string() } else { contagem(n, "comando no fundo", "comandos no fundo") }
}

/// O que um estado diz na pílula: "2 trabalhando"; no painel estreito, só o número.
fn rotulo_do_estado(estado: NaPilula, n: usize, curto: bool) -> String {
    if curto {
        return n.to_string();
    }
    match estado {
        NaPilula::Trabalhando => format!("{n} trabalhando"),
        NaPilula::EsperandoVoce => format!("{n} esperando você"),
        NaPilula::Falhou if n == 1 => "1 falhou".to_string(),
        NaPilula::Falhou => format!("{n} falharam"),
    }
}

/// "2 trabalhando · 1 esperando você"; no painel estreito, só os números (a marca de cada estado
/// diz o resto).
pub(super) fn rotulo_dos_agentes(agentes: &[(NaPilula, usize)], curto: bool) -> String {
    agentes.iter().map(|(estado, n)| rotulo_do_estado(*estado, *n, curto)).collect::<Vec<_>>().join(" · ")
}

/// "3/5".
pub(super) fn rotulo_dos_passos((feitos, total): (usize, usize)) -> String {
    format!("{feitos}/{total}")
}

/// "+40 −12" (o lado zerado não aparece; sem linha contada, "2 arquivos").
pub(super) fn rotulo_das_mudancas((arquivos, entram, saem): (usize, usize, usize)) -> String {
    let linhas: Vec<String> = [(entram, '+'), (saem, '−')].into_iter().filter(|(n, _)| *n > 0).map(|(n, sinal)| format!("{sinal}{n}")).collect();
    if linhas.is_empty() { contagem(arquivos, "arquivo", "arquivos") } else { linhas.join(" ") }
}

/// Uma pílula: o botão baixo e redondo, com a dica. Serve de gatilho de popover.
fn pilula(id: impl Into<ElementId>, dica: impl Into<SharedString>, conteudo: impl IntoElement, cx: &App) -> Button {
    Button::new(id)
        .custom(variante_fantasma(cx))
        .h(r(ALTURA_DA_PILULA))
        .px(esp::E8)
        .rounded(raio::de(ALTURA_DA_PILULA))
        .tooltip(dica)
        .child(h_flex().gap(esp::E4).metadado().text_color(Cores::texto()).child(conteudo))
}

/// A superfície de uma pílula: opaca, com o anel e a sombra de quem flutua sobre o texto.
fn flutuando(pilula: impl IntoElement, cx: &App) -> Div {
    div().flex_none().flutuante(cx).rounded(raio::de(ALTURA_DA_PILULA)).child(pilula)
}

/// A marca de um estado na pílula dos subagentes: o anel girando de quem trabalha, o ponto âmbar
/// de quem espera você e o vermelho de quem falhou.
fn marca_do_estado(estado: NaPilula, painel: u64) -> AnyElement {
    let ponto = |cor: Hsla| div().flex_none().size(r(7.)).rounded_full().bg(cor).into_any_element();
    match estado {
        NaPilula::Trabalhando => AnelGirando::new(11.).cor(Cores::destaque_texto()).vivo(format!("pilula-agentes-{painel}")).into_any_element(),
        NaPilula::EsperandoVoce => ponto(Cores::espera()),
        NaPilula::Falhou => ponto(Cores::vermelho()),
    }
}

/// O título de um popover da linha.
fn titulo_do_popover(texto: impl Into<SharedString>, conta: Option<String>) -> Div {
    h_flex()
        .gap(esp::E6)
        .child(div().rotulo().text_color(Cores::forte()).child(texto.into()))
        .children(conta.map(|c| div().font_family(FONTE_MONO).metadado().text_color(Cores::apagado()).child(c)))
}

impl Janela {
    /// A linha de pílulas do painel, ou `None` quando não há pílula (a linha só existe com
    /// alguma). Vai por cima do pé da conversa, fora do fluxo (`conversa`).
    pub(super) fn linha_de_pilulas(&mut self, motor: &Motor, chat: &Chat, painel_id: u64, cx: &Context<Self>) -> Option<AnyElement> {
        let pilulas = pilulas_do_chat(motor, chat);
        let painel = self.paineis.iter_mut().find(|p| p.id == painel_id)?;
        // Entra com fade e 8 px de baixo no quadro em que aparece, como a faixa do turno.
        let nova = !std::mem::replace(&mut painel.pilulas_vistas, !pilulas.vazia());
        if pilulas.vazia() {
            return None;
        }
        let nivel = painel.medida.nivel;
        let curto = !larga(nivel);
        // A margem é a da caixa (a 461ab7a juntou a regra das mensagens e da caixa em `coluna`).
        let caixa_estreita = painel.medida.faixa == super::componentes::FaixaCaixa::Estreita;
        // Com a linha já à vista, a pílula que aparece depois (o primeiro comando no fundo, as
        // mudanças no fim do turno) entra sozinha, com fade e 8 px de baixo; na linha que acabou de
        // aparecer, quem entra é a linha.
        let vistos = self.paineis.iter().find(|p| p.id == painel_id).map(|p| &p.ja_vistos);
        let mut filhos: Vec<AnyElement> = Vec::new();
        let mut por = |tipo: &'static str, pilula: AnyElement| {
            let aparece = vistos.is_some_and(|v| v.borrow_mut().novo(format!("pilula:{tipo}")));
            let id = SharedString::from(format!("entra-pilula-{tipo}-{painel_id}"));
            filhos.push(movimento::entrada(id, aparece && !nova, De::Baixo, div().flex_none().child(pilula)).into_any_element());
        };
        if !pilulas.comandos.is_empty() {
            por("comandos", self.pilula_dos_comandos(&pilulas.comandos, &chat.id, painel_id, curto, cx));
        }
        if !pilulas.agentes.is_empty() {
            por("agentes", self.pilula_dos_agentes(&pilulas.agentes, &chat.id, painel_id, curto, cx));
        }
        if let Some(passos) = pilulas.passos {
            por("passos", self.pilula_dos_passos(passos, motor.passos(&chat.id), &chat.id, painel_id, cx));
        }
        if let Some(mudancas) = pilulas.mudancas {
            por("mudancas", self.pilula_das_mudancas(mudancas, painel_id, curto, cx));
        }
        let linha = h_flex()
            .map(|s| coluna(s, caixa_estreita))
            .h(r(ALTURA_DA_PILULA))
            .gap(esp::E6)
            .children(filhos);
        Some(movimento::entrada(("entra-pilulas", painel_id as usize), nova, De::Baixo, div().w_full().child(linha)).into_any_element())
    }

    /// Os comandos que o agente deixou rodando no fundo: quantos; o clique lista e deixa parar
    /// cada um (o `stop_task` dele) ou todos.
    fn pilula_dos_comandos(&self, comandos: &[(String, String)], chat: &str, painel: u64, curto: bool, cx: &Context<Self>) -> AnyElement {
        let n = comandos.len();
        let dica = format!("{}\nClique para ver e parar", contagem(n, "comando rodando no fundo", "comandos rodando no fundo"));
        let gatilho = pilula(
            ("pilula-comandos", painel as usize),
            dica,
            h_flex().gap(esp::E4).child(icone_de(Lucide::Terminal, 12.).text_color(Cores::tenue())).child(rotulo_dos_comandos(n, curto)),
            cx,
        );
        let (motor, chat) = (self.motor.clone(), chat.to_string());
        let lista = popover(("pop-comandos", painel as usize)).anchor(Anchor::BottomLeft).offset(px(8.)).trigger(gatilho).content(move |_, window, cx| {
            let Some(motor) = motor.clone() else {
                return div().into_any_element();
            };
            let comandos = motor.read(cx).comandos_no_fundo(&chat).to_vec();
            let linhas: Vec<AnyElement> = comandos
                .iter()
                .enumerate()
                .map(|(i, (tarefa, descricao))| {
                    let (motor, chat, tarefa) = (motor.clone(), chat.clone(), tarefa.clone());
                    h_flex()
                        .gap(esp::E8)
                        .child(icone_de(Lucide::Terminal, 14.).text_color(Cores::tenue()))
                        .child(div().flex_1().min_w_0().truncate().font_family(FONTE_MONO).metadado().text_color(Cores::codigo()).child(descricao.clone()))
                        .child(
                            botao_fantasma(("parar-comando", i), Some(Lucide::Square.into()), "Parar", TamanhoBotao::Linha, window, cx)
                                .tooltip("Parar este comando")
                                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.parar_comando_no_fundo(&chat, &tarefa, cx))),
                        )
                        .into_any_element()
                })
                .collect();
            let corpo = CorpoPopover::new(LarguraPopover::P360)
                .child(titulo_do_popover("Rodando no fundo", Some(comandos.len().to_string())))
                .child(v_flex().gap(esp::E6).children(linhas));
            if comandos.len() < 2 {
                return corpo.into_any_element();
            }
            let (motor, chat) = (motor.clone(), chat.clone());
            corpo
                .rodape(
                    h_flex().justify_end().child(
                        botao_fantasma("parar-comandos", Some(Lucide::Square.into()), "Parar todos", TamanhoBotao::Barra, window, cx)
                            .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.parar_comandos_no_fundo(&chat, cx))),
                    ),
                )
                .into_any_element()
        });
        flutuando(lista, cx).into_any_element()
    }

    /// Os subagentes do turno por estado ("2 trabalhando · 1 esperando você"), cada estado com a
    /// marca dele; o clique lista os subagentes deste turno, e o clique num deles abre a conversa
    /// inteira dele numa aba.
    fn pilula_dos_agentes(&self, agentes: &[(NaPilula, usize)], chat: &str, painel: u64, curto: bool, cx: &Context<Self>) -> AnyElement {
        let dica = format!("Subagentes deste turno: {}\nClique para ver cada um e abrir a conversa dele", rotulo_dos_agentes(agentes, false));
        let segmentos: Vec<Div> = agentes
            .iter()
            .map(|(estado, n)| h_flex().gap(esp::E4).child(marca_do_estado(*estado, painel)).child(rotulo_do_estado(*estado, *n, curto)))
            .collect();
        let gatilho = pilula(("pilula-agentes", painel as usize), dica, h_flex().gap(esp::E8).children(segmentos), cx);
        let (motor, janela, chat) = (self.motor.clone(), cx.entity().downgrade(), chat.to_string());
        let lista = popover(("pop-agentes", painel as usize)).anchor(Anchor::BottomLeft).offset(px(8.)).trigger(gatilho).content(move |_, _, cx| {
            let pop = cx.entity().downgrade();
            let Some(m) = motor.as_ref().map(|m| m.read(cx)) else {
                return div().into_any_element();
            };
            let Some(c) = m.chat(&chat) else {
                return div().into_any_element();
            };
            let esperando = m.subagentes_esperando(&chat);
            // Só os deste turno (e quem ainda trabalha, se uma mensagem sua entrou no meio dele):
            // o mapa dos agentes, no menu da conversa, tem todos.
            let marco = inicio_do_turno(&c.itens);
            let de_antes = c.itens[..marco].iter().filter(|item| e_agente(item)).count();
            let do_turno: Vec<_> = agentes_do_chat(&c.itens, m.execucao(&chat).rodando(), agora())
                .into_iter()
                .enumerate()
                .filter(|(i, a)| *i >= de_antes || a.estado == Estado::Rodando)
                .map(|(_, a)| a)
                .collect();
            let linhas: Vec<AnyElement> = do_turno
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let espera = a.estado == Estado::Rodando && esperando.contains(&a.id);
                    let (meta, cor) = if espera { ("Esperando a sua resposta".to_string(), Cores::espera()) } else { (meta_na_lista(a), Cores::tenue()) };
                    let (janela, pop, chat, chamada) = (janela.clone(), pop.clone(), chat.clone(), a.id.clone());
                    h_flex()
                        .id(("agente-do-turno", i))
                        .gap(esp::E8)
                        .px(esp::E8)
                        .py(esp::E6)
                        .mx(r(-8.))
                        .rounded(raio::CONTROLE)
                        .cursor_pointer()
                        .hover(|s| s.bg(Cores::hover()))
                        .child(ponto(a.estado))
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .child(div().truncate().corpo_ui().text_color(Cores::forte()).child(a.titulo.clone()))
                                .child(div().truncate().metadado().text_color(cor).child(meta)),
                        )
                        .child(icone_de(Lucide::ChevronRight, 14.).text_color(Cores::apagado()))
                        .on_click(move |_, window, cx| {
                            if let Some(pop) = pop.upgrade() {
                                pop.update(cx, |p, cx| p.dismiss(window, cx));
                            }
                            let _ = janela.update(cx, |this, cx| this.abrir_subagente(painel, chat.clone(), chamada.clone(), window, cx));
                        })
                        .into_any_element()
                })
                .collect();
            CorpoPopover::new(LarguraPopover::P360)
                .child(titulo_do_popover("Subagentes deste turno", Some(do_turno.len().to_string())))
                .child(v_flex().id("agentes-do-turno").max_h(r(320.)).overflow_y_scroll().children(linhas))
                .into_any_element()
        });
        flutuando(lista, cx).into_any_element()
    }

    /// Os passos do agente ("3/5"): a lista inteira na dica, e no clique com o que está fazendo
    /// em destaque.
    fn pilula_dos_passos(&self, passos: (usize, usize), lista: &[Passo], chat: &str, painel: u64, cx: &Context<Self>) -> AnyElement {
        let gatilho = pilula(
            ("pilula-passos", painel as usize),
            texto_dos_passos(lista),
            h_flex().gap(esp::E4).child(icone_de(Lucide::ListChecks, 12.).text_color(Cores::tenue())).child(rotulo_dos_passos(passos)),
            cx,
        );
        let (motor, chat) = (self.motor.clone(), chat.to_string());
        let lista = popover(("pop-passos", painel as usize)).anchor(Anchor::BottomLeft).offset(px(8.)).trigger(gatilho).content(move |_, _, cx| {
            let Some(m) = motor.as_ref().map(|m| m.read(cx)) else {
                return div().into_any_element();
            };
            let lista = m.passos(&chat);
            let feitos = lista.iter().filter(|p| p.estado == EstadoDoPasso::Feito).count();
            let linhas: Vec<Div> = lista.iter().map(super::este_chat::linha_do_passo).collect();
            CorpoPopover::new(LarguraPopover::P360)
                .child(titulo_do_popover("Passos", Some(format!("{feitos} de {}", lista.len()))))
                .child(v_flex().id("passos-do-turno").max_h(r(320.)).overflow_y_scroll().gap(esp::E8).children(linhas))
                .into_any_element()
        });
        flutuando(lista, cx).into_any_element()
    }

    /// O que o turno mudou ("+40 −12"): o clique abre a lista, à direita, em "Este chat".
    fn pilula_das_mudancas(&self, mudancas: (usize, usize, usize), painel: u64, curto: bool, cx: &Context<Self>) -> AnyElement {
        let (arquivos, entram, saem) = mudancas;
        let dica = format!(
            "{} neste turno: {}\nClique para ver a lista, à direita. O que um comando mudou não aparece aqui.",
            contagem(arquivos, "arquivo mudado", "arquivos mudados"),
            rotulo_das_mudancas(mudancas)
        );
        let contadas = entram + saem > 0;
        let conteudo = h_flex()
            .gap(esp::E4)
            .child(icone_de(Lucide::FileDiff, 12.).text_color(Cores::tenue()))
            // Com lugar, quantos arquivos; as linhas vão sempre que foram contadas.
            .when(!curto || !contadas, |s| s.child(if curto { arquivos.to_string() } else { contagem(arquivos, "arquivo", "arquivos") }))
            .when(contadas, |s| {
                s.child(
                    h_flex()
                        .gap(esp::E4)
                        .font_family(FONTE_MONO)
                        .when(entram > 0, |s| s.child(div().text_color(Cores::verde()).child(format!("+{entram}"))))
                        .when(saem > 0, |s| s.child(div().text_color(Cores::vermelho()).child(format!("−{saem}")))),
                )
            });
        let gatilho = pilula(("pilula-mudancas", painel as usize), dica, conteudo, cx).on_click(cx.listener(|this, _, window, cx| {
            this.aba_direita = super::abas_direita::AbaDireita::Chat;
            this.mostrar_direita(window, cx);
        }));
        flutuando(gatilho, cx).into_any_element()
    }
}

#[cfg(test)]
mod testes {
    // Sem o `use super::*`: ele traria o `test` do GPUI (do `gpui_kit::*` lá de cima), e o
    // `#[test]` daqui deixaria de ser o do Rust.
    use super::{NaPilula, Pilulas, montar, rotulo_das_mudancas, rotulo_dos_agentes, rotulo_dos_comandos, rotulo_dos_passos};
    use crate::{
        modelo::{Item, Mudanca},
        motor::{EstadoDoPasso, Passo},
    };

    fn chamada(id: &str, nome: &str, resumo: &str, resultado: Option<&str>, erro: bool, mudanca: Option<Mudanca>) -> Item {
        Item::Ferramenta {
            id: id.into(),
            nome: nome.into(),
            resumo: resumo.into(),
            resultado: resultado.map(str::to_string),
            erro,
            em: 100,
            detalhe: None,
            arquivo: mudanca.map(|_| resumo.to_string()),
            sub: None,
            mudanca,
            durou: 0,
            imagens: Vec::new(),
        }
    }

    fn agente(id: &str, resultado: Option<&str>, erro: bool) -> Item {
        chamada(id, "Task", "Task · Explorar o carrinho", resultado, erro, None)
    }

    fn edicao(arquivo: &str, entram: u32, saem: u32) -> Item {
        chamada("e", "Edit", arquivo, Some("ok"), false, Some(Mudanca { entram, saem, novo: false }))
    }

    fn sua(texto: &str) -> Item {
        Item::Usuario { texto: texto.into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 90, marca: None }
    }

    fn passo(estado: EstadoDoPasso) -> Passo {
        Passo { id: String::new(), texto: "Corrigir o desconto".into(), fazendo: "Corrigindo o desconto".into(), estado }
    }

    /// "A linha só existe quando há pílula."
    #[test]
    fn sem_nada_a_mostrar_a_linha_nao_existe() {
        let conversa = [sua("oi"), Item::Assistente { texto: "olá".into(), em: 95 }];
        assert!(montar(false, &[], &[], &[], &conversa, 200).vazia());
        // Rodando, mas sem comando no fundo, subagente, passos nem edição: também não.
        assert!(montar(true, &[], &[], &[], &conversa, 200).vazia());
        assert!(montar(true, &[], &[], &[], &[], 200).vazia());
        assert_eq!(montar(false, &[], &[], &[], &conversa, 200), Pilulas::default());
    }

    /// "Subagentes por estado (2 trabalhando · 1 esperando você)."
    #[test]
    fn subagentes_por_estado() {
        let conversa = [
            // De um turno anterior: o que falhou lá não volta a aparecer.
            sua("antes"),
            agente("velho", Some("deu ruim"), true),
            sua("agora"),
            agente("a", None, false),
            agente("b", None, false),
            agente("c", None, false),
            agente("d", Some("pronto"), false),
        ];
        let p = montar(true, &[], &["c".to_string()], &[], &conversa, 200);
        assert_eq!(p.agentes, [(NaPilula::Trabalhando, 2), (NaPilula::EsperandoVoce, 1)]);
        assert_eq!(rotulo_dos_agentes(&p.agentes, false), "2 trabalhando · 1 esperando você");
        assert_eq!(rotulo_dos_agentes(&p.agentes, true), "2 · 1");
        // O que falhou neste turno fica à vista enquanto o turno roda.
        let com_falha = [sua("agora"), agente("a", None, false), agente("b", Some("deu ruim"), true)];
        let p = montar(true, &[], &[], &[], &com_falha, 200);
        assert_eq!(p.agentes, [(NaPilula::Trabalhando, 1), (NaPilula::Falhou, 1)]);
        assert_eq!(rotulo_dos_agentes(&p.agentes, false), "1 trabalhando · 1 falhou");
        // Todos terminaram bem: nada a dizer. E com o turno parado ninguém trabalha.
        assert!(montar(true, &[], &[], &[], &[sua("agora"), agente("d", Some("pronto"), false)], 200).agentes.is_empty());
        assert!(montar(false, &[], &[], &[], &conversa, 200).agentes.is_empty());
    }

    /// Item 2: a pílula "N trabalhando" conta os agentes que continuam em segundo plano depois da
    /// resposta do turno (o `result`), quando a conversa já tem a resposta e o aviso de que eles
    /// seguem.
    #[test]
    fn os_agentes_que_seguem_depois_da_resposta_contam_como_trabalhando() {
        let resposta = |texto: &str| Item::Assistente { texto: texto.into(), em: 110 };
        let segue = Item::Aviso { texto: crate::modelo::AGENTES_SEGUEM.into(), erro: false, em: 111 };
        let conversa = [sua("lance os dois"), agente("a", None, false), agente("b", None, false), resposta("Lancei os dois."), segue.clone()];
        let p = montar(true, &[], &[], &[], &conversa, 200);
        assert_eq!(p.agentes, [(NaPilula::Trabalhando, 2)]);
        assert_eq!(rotulo_dos_agentes(&p.agentes, false), "2 trabalhando");
        // Um terminou (a linha do fim dele entrou na conversa, e o Claude comentou): o outro segue.
        let um_terminou = [
            sua("lance os dois"),
            agente("a", Some("Dois lugares mexem no cupom."), false),
            agente("b", None, false),
            resposta("Lancei os dois."),
            segue.clone(),
            Item::FimDoAgente { chamada: "a".into(), descricao: "Explorar o carrinho".into(), erro: false, durou: 60, em: 170 },
            resposta("O primeiro terminou."),
        ];
        assert_eq!(montar(true, &[], &[], &[], &um_terminou, 200).agentes, [(NaPilula::Trabalhando, 1)]);
        // O turno que parou de verdade deixa outro aviso, e aí ninguém mais trabalha.
        let parou = [sua("lance os dois"), agente("a", None, false), segue, Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 180 }];
        assert!(montar(true, &[], &[], &[], &parou, 200).agentes.is_empty());
    }

    /// Item 2: "a lista dela deve mostrar a mesma última ação" que a linha da conversa.
    #[test]
    fn a_lista_da_pilula_diz_a_mesma_ultima_acao_da_linha() {
        use crate::ui::{
            mapa_agentes::{Agente, Estado},
            mensagens::no_fundo::acao_ao_vivo,
        };
        let rodando = Agente {
            id: "a".into(),
            titulo: "Mapear o carrinho".into(),
            estado: Estado::Rodando,
            ferramentas: Some(3),
            tokens: 0,
            segundos: Some(90),
            atividade: "Bash · cargo test".into(),
            passos: Vec::new(),
            resultado: None,
            em: 0,
            esperando: false,
        };
        assert_eq!(super::meta_na_lista(&rodando), "Rodando cargo test…");
        assert_eq!(super::meta_na_lista(&rodando), acao_ao_vivo(&rodando.atividade));
        assert_eq!(super::meta_na_lista(&Agente { atividade: String::new(), ..rodando.clone() }), "Começando…");
        // Quem terminou diz quanto levou, como antes.
        assert_eq!(super::meta_na_lista(&Agente { estado: Estado::Terminou, ..rodando }), "1 min 30 s · 3 ferramentas");
    }

    /// "Os passos/tarefas do agente (3/5)."
    #[test]
    fn passos_do_agente_enquanto_o_turno_roda() {
        use EstadoDoPasso::*;
        let passos = [passo(Feito), passo(Feito), passo(Feito), passo(Fazendo), passo(Falta)];
        let p = montar(true, &[], &[], &passos, &[sua("oi")], 200);
        assert_eq!(p.passos, Some((3, 5)));
        assert_eq!(rotulo_dos_passos((3, 5)), "3/5");
        assert!(!p.vazia());
        // Com o turno parado a lista continua em "Este chat", à direita, e não na linha.
        assert_eq!(montar(false, &[], &[], &passos, &[sua("oi")], 200).passos, None);
        // Com as ferramentas de tarefa (TaskCreate e TaskUpdate, um passo por chamada) a conta é
        // a mesma: a lista que elas montam é a que a pílula lê.
        let mut lista = crate::motor::passos::Passos::default();
        for (chamada, assunto) in [("a", "ler"), ("b", "corrigir"), ("c", "testar")] {
            lista.chamada(chamada, "TaskCreate", &serde_json::json!({ "subject": assunto }));
        }
        for (n, chamada) in ["a", "b", "c"].iter().enumerate() {
            lista.resultado(chamada, &format!("Task #{} created successfully: x", n + 1), false);
        }
        assert_eq!(montar(true, &[], &[], lista.lista(), &[sua("oi")], 200).passos, Some((0, 3)));
        lista.chamada("d", "TaskUpdate", &serde_json::json!({ "taskId": "1", "status": "completed" }));
        lista.resultado("d", "Updated task #1 status", false);
        lista.chamada("e", "TaskUpdate", &serde_json::json!({ "taskId": "2", "status": "in_progress" }));
        lista.resultado("e", "Updated task #2 status", false);
        assert_eq!(montar(true, &[], &[], lista.lista(), &[sua("oi")], 200).passos, Some((1, 3)));
    }

    /// "As mudanças do turno (+N −M)."
    #[test]
    fn mudancas_do_turno() {
        let conversa = [
            sua("antes"),
            edicao("velho.rs", 100, 50),
            sua("agora"),
            edicao("a.rs", 5, 2),
            edicao("b.rs", 3, 0),
            edicao("a.rs", 1, 1),
        ];
        // Só o que este turno mudou, cada arquivo uma vez; vale com o turno rodando e depois dele.
        assert_eq!(montar(true, &[], &[], &[], &conversa, 200).mudancas, Some((2, 9, 3)));
        assert_eq!(montar(false, &[], &[], &[], &conversa, 200).mudancas, Some((2, 9, 3)));
        // É a mesma conta da seção Mudanças de "Este chat" (a lista que o clique abre).
        let neste_turno = crate::ui::mensagens::mudancas_do_chat(&conversa).0;
        assert_eq!(Some(crate::ui::mensagens::total_das_mudancas(&neste_turno)), montar(true, &[], &[], &[], &conversa, 200).mudancas);
        assert_eq!(rotulo_das_mudancas((2, 9, 3)), "+9 −3");
        assert_eq!(rotulo_das_mudancas((1, 4, 0)), "+4");
        assert_eq!(rotulo_das_mudancas((1, 0, 7)), "−7");
        // A edição de antes de as linhas serem contadas: diz os arquivos.
        assert_eq!(rotulo_das_mudancas((2, 0, 0)), "2 arquivos");
        assert_eq!(rotulo_das_mudancas((1, 0, 0)), "1 arquivo");
        // O turno que não editou nada não tem a pílula.
        assert_eq!(montar(true, &[], &[], &[], &[sua("antes"), edicao("velho.rs", 1, 1), sua("agora")], 200).mudancas, None);
    }

    /// "Comandos em segundo plano (quantos)": também depois do turno, enquanto eles rodam.
    #[test]
    fn comandos_no_fundo() {
        let comandos = [("b1".to_string(), "npm run dev".to_string()), ("b2".to_string(), "cargo watch".to_string())];
        for rodando in [true, false] {
            let p = montar(rodando, &comandos, &[], &[], &[sua("oi")], 200);
            assert_eq!(p.comandos, comandos);
            assert!(!p.vazia());
        }
        assert_eq!(rotulo_dos_comandos(2, false), "2 comandos no fundo");
        assert_eq!(rotulo_dos_comandos(1, false), "1 comando no fundo");
        assert_eq!(rotulo_dos_comandos(2, true), "2");
    }
}
