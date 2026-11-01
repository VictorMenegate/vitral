//! Rotinas: pedidos que o Vitral manda sozinho num horário, ao Mestre ou num chat novo do projeto
//! (com o Vitral aberto). Ex.: "todo dia às 8h, rode os testes e me diga o que quebrou".
//! O diálogo tem duas páginas: a lista e o formulário (nova ou editar).

use chrono::{NaiveTime, Timelike as _};
use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    input::{InputEvent, Textarea, TextareaState},
    switch::Switch,
    time_field::{TimeField, TimeFieldEvent, TimeFieldState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    AbaLateral, Filtro, Janela,
    componentes::{
        CabecalhoDialogo, Campo, CartaoDeLista, Chip, EstadoVazio, GRUPO_DO_CARTAO, GrupoDeOpcoes, RodapeDialogo,
        Simbolo, TamanhoBotao, TamanhoChip, Tom, aviso, aviso_com_acao, botao_fantasma, botao_icone, botao_perigo,
        botao_primario, icone,
    },
    configuracoes::{Opcao, menu_escolha},
    dialogos::cabecalho,
    icone_projeto,
};
use crate::{
    modelo::{Rotina, novo_id},
    motor::Motor,
    tema::{Cores, Lucide, r},
};

const DIAS: [&str; 7] = ["Dom", "Seg", "Ter", "Qua", "Qui", "Sex", "Sáb"];
const DIAS_POR_EXTENSO: [&str; 7] =
    ["Domingo", "Segunda-feira", "Terça-feira", "Quarta-feira", "Quinta-feira", "Sexta-feira", "Sábado"];
const DIAS_UTEIS: [u8; 5] = [1, 2, 3, 4, 5];
const EXEMPLO: &str = "rode os testes e me diga o que quebrou";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Pagina {
    Lista,
    Formulario,
}

pub struct Rotinas {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    pagina: Pagina,
    texto: Entity<TextareaState>,
    hora: Entity<TimeFieldState>,
    dias: Vec<u8>,
    projeto: Option<String>,
    /// O do filtro do trilho (ou o primeiro): é com ele que uma rotina nova começa.
    projeto_inicial: Option<String>,
    /// Rotina carregada no formulário para editar.
    editando: Option<String>,
    /// O pedido ficou vazio ao sair do campo: o erro aparece embaixo dele.
    pedido_tocado: bool,
    _observa: Subscription,
    _digitando: Vec<Subscription>,
}

fn hora_em(minutos: u32) -> NaiveTime {
    NaiveTime::from_hms_opt(minutos / 60 % 24, minutos % 60, 0).unwrap_or(NaiveTime::MIN)
}

impl Rotinas {
    pub fn new(
        motor: Entity<Motor>,
        janela: WeakEntity<Janela>,
        projeto_filtro: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let _observa = cx.observe(&motor, |_, _, cx| cx.notify());
        let texto = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(2, 5)
                .submit_on_enter(true)
                .placeholder("O que pedir (ex.: rode os testes e me diga o que quebrou)")
        });
        let hora = cx.new(|cx| {
            let mut hora = TimeFieldState::new(window, cx);
            hora.set_time(hora_em(8 * 60), window, cx);
            hora
        });
        // O Enter (sem Shift) chega ao diálogo, que salva; aqui só o redesenho e a saída do campo.
        let _digitando = vec![
            cx.subscribe_in(&texto, window, |this, estado, evento, _, cx| match evento {
                InputEvent::Change => cx.notify(),
                InputEvent::Blur => {
                    this.pedido_tocado = estado.read(cx).value().trim().is_empty();
                    cx.notify();
                }
                _ => {}
            }),
            cx.subscribe(&hora, |_, _, _: &TimeFieldEvent, cx| cx.notify()),
        ];
        let projetos: Vec<String> = motor.read(cx).projetos_visiveis().iter().map(|p| p.id.clone()).collect();
        let projeto_inicial = projeto_filtro.filter(|p| projetos.contains(p)).or_else(|| projetos.first().cloned());
        Self {
            texto,
            hora,
            dias: DIAS_UTEIS.to_vec(),
            projeto: projeto_inicial.clone(),
            projeto_inicial,
            editando: None,
            pedido_tocado: false,
            pagina: Pagina::Lista,
            motor,
            janela,
            _observa,
            _digitando,
        }
    }

    fn minutos(&self, cx: &App) -> u32 {
        let hora = self.hora.read(cx).time();
        hora.hour() * 60 + hora.minute()
    }

    /// O que falta para salvar, numa frase só ("Escreva o pedido e escolha pelo menos um dia").
    fn motivo(&self, cx: &App) -> Option<String> {
        let projetos: Vec<String> = self.motor.read(cx).projetos_visiveis().iter().map(|p| p.id.clone()).collect();
        motivo_para_nao_salvar(
            !self.texto.read(cx).value().trim().is_empty(),
            !self.dias.is_empty(),
            self.projeto.as_ref().is_some_and(|p| p.is_empty() || projetos.contains(p)),
        )
    }

    /// Abre o formulário vazio (ou com o exemplo).
    fn nova(&mut self, texto: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.editando = None;
        self.projeto = self.projeto_inicial.clone();
        self.preencher(texto, 8 * 60, DIAS_UTEIS.to_vec(), window, cx);
        self.abrir_formulario(window, cx);
    }

    /// Carrega a rotina no formulário.
    fn editar(&mut self, rotina: &Rotina, window: &mut Window, cx: &mut Context<Self>) {
        self.editando = Some(rotina.id.clone());
        self.projeto = Some(rotina.projeto.clone());
        self.preencher(&rotina.texto, rotina.hora, rotina.dias.clone(), window, cx);
        self.abrir_formulario(window, cx);
    }

    fn abrir_formulario(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pagina = Pagina::Formulario;
        self.pedido_tocado = false;
        let foco = self.texto.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Volta à lista sem gravar.
    pub(super) fn voltar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.pagina = Pagina::Lista;
        self.editando = None;
        self.preencher("", 8 * 60, DIAS_UTEIS.to_vec(), window, cx);
        cx.notify();
    }

    fn preencher(&mut self, texto: &str, hora: u32, dias: Vec<u8>, window: &mut Window, cx: &mut Context<Self>) {
        let texto = texto.to_string();
        self.texto.update(cx, |t, cx| t.set_value(texto, window, cx));
        self.hora.update(cx, |h, cx| h.set_time(hora_em(hora), window, cx));
        self.dias = dias;
    }

    /// Enter no diálogo: no formulário, salva.
    pub(super) fn enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pagina == Pagina::Formulario {
            self.salvar(window, cx);
        }
    }

    /// Cria a rotina, ou grava a que está em edição, e volta à lista. Com algo faltando, mostra
    /// o erro no campo e fica.
    fn salvar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.motivo(cx).is_some() {
            self.pedido_tocado = self.texto.read(cx).value().trim().is_empty();
            cx.notify();
            return;
        }
        let (Some(projeto), texto) = (self.projeto.clone(), self.texto.read(cx).value().trim().to_string()) else {
            return;
        };
        let minutos = self.minutos(cx);
        let mut dias = self.dias.clone();
        dias.sort_unstable();
        dias.dedup();
        let (_, agora, hoje) = crate::modelo::agora_local();
        // Hora que já passou hoje: começa no próximo dia, senão rodaria em até 30 s.
        let ja_passou = agora >= minutos;
        match self.editando.take() {
            Some(id) => self.motor.update(cx, |m, cx| {
                m.mudar_rotinas(
                    |rotinas| {
                        if let Some(r) = rotinas.iter_mut().find(|r| r.id == id) {
                            r.projeto = projeto;
                            r.texto = texto;
                            r.hora = minutos;
                            r.dias = dias;
                            if ja_passou {
                                r.ultima = hoje;
                            }
                        }
                    },
                    cx,
                )
            }),
            None => {
                let rotina = Rotina {
                    id: novo_id(),
                    projeto,
                    texto,
                    hora: minutos,
                    dias,
                    ativa: true,
                    ultima: if ja_passou { hoje } else { String::new() },
                };
                self.motor.update(cx, |m, cx| m.mudar_rotinas(|rotinas| rotinas.push(rotina), cx));
            }
        }
        self.voltar(window, cx);
    }

    /// Tira a rotina da lista e oferece desfazer, na mesma posição.
    fn apagar(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let mut tirada = None;
        self.motor.update(cx, |m, cx| {
            m.mudar_rotinas(
                |rotinas| {
                    if let Some(ix) = rotinas.iter().position(|r| r.id == id) {
                        tirada = Some((ix, rotinas.remove(ix)));
                    }
                },
                cx,
            )
        });
        let Some((ix, rotina)) = tirada else {
            return;
        };
        if self.editando.as_deref() == Some(id) {
            self.voltar(window, cx);
        }
        let motor = self.motor.clone();
        let texto = format!("Rotina apagada: “{}”", resumo(&rotina.texto, 40));
        let nota = aviso_com_acao(Tom::Info, texto, "Desfazer", move |_, cx| {
            let rotina = rotina.clone();
            motor.update(cx, |m, cx| {
                m.mudar_rotinas(
                    |rotinas| {
                        if !rotinas.iter().any(|r| r.id == rotina.id) {
                            let ix = ix.min(rotinas.len());
                            rotinas.insert(ix, rotina);
                        }
                    },
                    cx,
                )
            });
        });
        window.push_notification(nota, cx);
    }

    /// O cabeçalho da página: a lista, ou o formulário com a seta de voltar.
    pub(super) fn cabecalho(vista: &Entity<Rotinas>, cx: &App) -> CabecalhoDialogo {
        let v = vista.read(cx);
        match v.pagina {
            Pagina::Lista => cabecalho(
                "Rotinas",
                Some("Rodam com o Vitral aberto; se ele estiver fechado, rodam ao abrir.".into()),
            ),
            Pagina::Formulario => {
                let voltar = vista.clone();
                CabecalhoDialogo::new(if v.editando.is_some() { "Editar rotina" } else { "Nova rotina" })
                    .voltar(move |window, cx| voltar.update(cx, |v, cx| v.voltar(window, cx)))
            }
        }
    }

    /// O rodapé da página: "Nova rotina" na lista; no formulário, apagar (na edição), cancelar e
    /// criar ou salvar, desabilitado com o que falta na dica.
    pub(super) fn rodape(vista: &Entity<Rotinas>, window: &Window, cx: &App) -> RodapeDialogo {
        let v = vista.read(cx);
        let alvo = vista.clone();
        if v.pagina == Pagina::Lista {
            return RodapeDialogo::new().principal(
                botao_primario("rotina-nova", Some(Lucide::Plus.into()), "Nova rotina", TamanhoBotao::Controle, window, cx)
                    .on_click(move |_, window, cx| alvo.update(cx, |v, cx| v.nova("", window, cx))),
            );
        }
        let motivo = v.motivo(cx);
        let editando = v.editando.clone();
        let (cancelar, salvar) = (vista.clone(), vista.clone());
        let mut rodape = RodapeDialogo::new();
        if let Some(id) = editando.clone() {
            rodape = rodape.esquerda(
                botao_perigo("rotina-apagar", Some(Lucide::Trash.into()), "Apagar rotina", TamanhoBotao::Controle, window, cx)
                    .on_click(move |_, window, cx| alvo.update(cx, |v, cx| v.apagar(&id, window, cx))),
            );
        }
        rodape
            .cancelar(
                botao_fantasma("rotina-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                    .on_click(move |_, window, cx| cancelar.update(cx, |v, cx| v.voltar(window, cx))),
            )
            .principal(
                botao_primario(
                    "rotina-salvar",
                    None,
                    if editando.is_some() { "Salvar" } else { "Criar rotina" },
                    TamanhoBotao::Controle,
                    window,
                    cx,
                )
                .when_some(motivo, |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
                .on_click(move |_, window, cx| salvar.update(cx, |v, cx| v.salvar(window, cx))),
            )
    }

    fn lista(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let motor = self.motor.read(cx);
        let rotinas = motor.rotinas.clone();
        if rotinas.is_empty() {
            return EstadoVazio::new(Lucide::Calendar, "Nenhuma rotina ainda")
                .texto(format!("Ex.: todo dia útil às 08:00, “{EXEMPLO}”."))
                .acao(
                    Chip::new("rotina-exemplo", "Usar exemplo")
                        .tamanho(TamanhoChip::Medio)
                        .on_click(cx.listener(|this, _, window, cx| this.nova(EXEMPLO, window, cx)))
                        .construir(window, cx),
                )
                .into_any_element();
        }
        let (dia, minutos, hoje) = crate::modelo::agora_local();
        let mut grupo = GrupoDeOpcoes::new(None);
        for rotina in rotinas {
            let quando = format!("{} às {}", descrever_dias(&rotina.dias), hora_legivel(rotina.hora));
            let arquivado = motor.projeto_arquivado(&rotina.projeto);
            let estado = if arquivado {
                "projeto arquivado, não roda".into()
            } else if rotina.ativa {
                format!("próxima: {}", proxima_execucao(&rotina, dia, minutos, &hoje))
            } else {
                "pausada".into()
            };
            let (simbolo, destino) = if rotina.projeto.is_empty() {
                (Simbolo::icone_cor(Lucide::Sparkles, Cores::destaque_texto()), "Mestre".to_string())
            } else {
                let (icone, cor) = icone_projeto::visual_do(motor, &rotina.projeto);
                (Simbolo::Projeto(icone, cor), motor.nome_do_projeto(&rotina.projeto))
            };
            let (ativar, rodar, apagar, editar) = (self.motor.clone(), rotina.clone(), rotina.id.clone(), rotina.clone());
            let (motor_rodar, janela) = (self.motor.clone(), self.janela.clone());
            let id = rotina.id.clone();
            let acoes = div()
                .w(r(84.))
                .flex()
                .flex_none()
                .justify_end()
                .gap(r(4.))
                .opacity(0.)
                .group_hover(GRUPO_DO_CARTAO, |s| s.opacity(1.))
                .child(
                    botao_icone(SharedString::from(format!("rotina-rodar-{id}")), Lucide::Play, "Rodar agora e abrir a conversa", TamanhoBotao::Linha, window, cx)
                        .disabled(arquivado)
                        .on_click(move |_, window, cx| {
                            let chat = motor_rodar.update(cx, |m, cx| m.rodar_rotina(&rodar, cx));
                            window.close_dialog(cx);
                            let projeto = rodar.projeto.clone();
                            match chat {
                                Ok(chat) => {
                                    let _ = janela.update(cx, |j, cx| j.mostrar_conversa_da_rotina(&projeto, chat, window, cx));
                                }
                                Err(erro) => window.push_notification(aviso(Tom::Erro, format!("A rotina não rodou: {erro}")), cx),
                            }
                        }),
                )
                .child(
                    botao_icone(SharedString::from(format!("rotina-editar-{id}")), Lucide::Pencil, "Editar", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener({
                            let editar = editar.clone();
                            move |this, _, window, cx| this.editar(&editar, window, cx)
                        })),
                )
                .child(
                    botao_icone(SharedString::from(format!("rotina-apagar-{id}")), Lucide::Trash, "Apagar", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(move |this, _, window, cx| this.apagar(&apagar, window, cx))),
                );
            let interruptor = Switch::new(SharedString::from(format!("rotina-ativa-{}", rotina.id)))
                .checked(rotina.ativa)
                .tooltip(if rotina.ativa { "Pausar a rotina" } else { "Ativar a rotina" })
                .on_click(move |ativa: &bool, _, cx| {
                    let (id, ativa) = (id.clone(), *ativa);
                    ativar.update(cx, |m, cx| {
                        m.mudar_rotinas(
                            |rotinas| {
                                if let Some(r) = rotinas.iter_mut().find(|r| r.id == id) {
                                    r.ativa = ativa;
                                }
                            },
                            cx,
                        )
                    });
                });
            grupo = grupo.linha(
                CartaoDeLista::new(SharedString::from(format!("rotina-{}", rotina.id)), rotina.texto.clone())
                    .simbolo(simbolo)
                    .subtitulo(format!("{destino} · {quando} · {estado}"))
                    .acao_fixa(
                        // O clique no interruptor e nas ações não abre a edição.
                        div()
                            .id(SharedString::from(format!("rotina-controles-{}", rotina.id)))
                            .flex()
                            .items_center()
                            .gap(r(8.))
                            .on_click(|_, _, cx| cx.stop_propagation())
                            .child(interruptor)
                            .child(acoes),
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.editar(&editar, window, cx))),
            );
        }
        grupo.into_any_element()
    }

    fn formulario(&self, window: &Window, cx: &Context<Self>) -> AnyElement {
        let motor = self.motor.read(cx);
        let projetos: Vec<String> = motor.projetos_visiveis().iter().map(|p| p.id.clone()).collect();
        let pedido_vazio = self.texto.read(cx).value().trim().is_empty();
        let dias = div().flex().flex_wrap().gap(r(4.)).children(DIAS.iter().enumerate().map(|(i, nome)| {
            let dia = i as u8;
            Chip::new(("rotina-dia", i), *nome)
                .tamanho(TamanhoChip::Medio)
                .mudado(false)
                .alternavel(self.dias.contains(&dia))
                .dica(DIAS_POR_EXTENSO[i])
                .on_click(cx.listener(move |this, _, _, cx| {
                    if this.dias.contains(&dia) {
                        this.dias.retain(|d| *d != dia);
                    } else {
                        this.dias.push(dia);
                    }
                    cx.notify();
                }))
                .construir(window, cx)
                .min_w(r(44.))
        }));
        let atalhos = div()
            .flex()
            .gap(r(4.))
            .child(
                botao_fantasma("rotina-dias-uteis", None, "Dias úteis", TamanhoBotao::Linha, window, cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.dias = DIAS_UTEIS.to_vec();
                        cx.notify();
                    },
                )),
            )
            .child(
                botao_fantasma("rotina-todo-dia", None, "Todo dia", TamanhoBotao::Linha, window, cx).on_click(cx.listener(
                    |this, _, _, cx| {
                        this.dias = (0..7).collect();
                        cx.notify();
                    },
                )),
            );
        let hora = div()
            .flex()
            .items_center()
            .gap(r(8.))
            .child(div().text_color(Cores::tenue()).child(icone(Lucide::Clock, 14., window)))
            .child(TimeField::new(&self.hora).w(r(88.)));
        let quando = div()
            .flex()
            .flex_col()
            .gap(r(8.))
            .child(div().flex().items_center().gap(r(12.)).child(hora).child(atalhos))
            .child(dias);
        let eu = cx.entity().downgrade();
        // O Mestre primeiro (guarda ""), depois os projetos com o quadrado de cada um.
        let mestre = Opcao::new("", "Mestre")
            .descricao("Ele decide para onde vai o pedido")
            .simbolo(Simbolo::icone_cor(Lucide::Sparkles, Cores::destaque_texto()));
        let opcoes = std::iter::once(mestre)
            .chain(projetos.iter().map(|p| {
                let (icone, cor) = icone_projeto::visual_do(motor, p);
                let descricao = motor
                    .projeto(p)
                    .map(|x| x.descricao.lines().next().unwrap_or_default().trim().to_string())
                    .filter(|d| !d.is_empty())
                    .unwrap_or_else(|| "Num chat novo do projeto".into());
                Opcao::new(p.clone(), motor.nome_do_projeto(p)).descricao(descricao).simbolo(Simbolo::Projeto(icone, cor))
            }))
            .collect();
        let atual = self.projeto.clone().unwrap_or_default();
        let projeto = menu_escolha(
            "rotina-projeto",
            Anchor::TopLeft,
            opcoes,
            &atual,
            "",
            move |projeto, _, cx| {
                let _ = eu.update(cx, |this, cx| {
                    this.projeto = Some(projeto);
                    cx.notify();
                });
            },
            window,
            cx,
        );
        div()
            .flex()
            .flex_col()
            .gap(r(16.))
            .child(
                Campo::new("Pedido", Textarea::new(&self.texto))
                    .erro((self.pedido_tocado && pedido_vazio).then(|| "Escreva o pedido".into())),
            )
            .child(Campo::new("Quando", quando).erro(self.dias.is_empty().then(|| "Escolha pelo menos um dia".into())))
            .child(Campo::new("Para quem", div().flex().child(projeto)))
            .into_any_element()
    }
}

/// O começo do texto, cortado com "…" quando passa de `max` caracteres.
fn resumo(texto: &str, max: usize) -> String {
    let mut curto: String = texto.chars().take(max).collect();
    if texto.chars().count() > max {
        curto.push('…');
    }
    curto
}

/// Os motivos somados numa frase: "Escreva o pedido e escolha pelo menos um dia".
fn motivo_para_nao_salvar(texto: bool, dias: bool, projeto: bool) -> Option<String> {
    let faltam: Vec<&str> = [
        (!texto).then_some("escreva o pedido"),
        (!dias).then_some("escolha pelo menos um dia"),
        (!projeto).then_some("escolha para quem"),
    ]
    .into_iter()
    .flatten()
    .collect();
    let frase = match faltam.as_slice() {
        [] => return None,
        [um] => um.to_string(),
        [resto @ .., ultimo] => format!("{} e {ultimo}", resto.join(", ")),
    };
    let mut letras = frase.chars();
    letras.next().map(|primeira| primeira.to_uppercase().chain(letras).collect())
}

impl Janela {
    /// Abre a conversa que a rotina acabou de criar e garante que ela apareça na lista.
    fn mostrar_conversa_da_rotina(&mut self, projeto: &str, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        self.lista = true;
        if self.vista.filtro().is_some_and(|p| p != projeto) {
            self.mudar_vista(super::Vista::Geral, cx);
        }
        if self.filtro == Filtro::Arquivados {
            self.filtro = Filtro::Todos;
        }
        if self.aba != AbaLateral::Conversas {
            self.trocar_aba(AbaLateral::Conversas, window, cx);
        }
        self.selecionar(Some(chat), window, cx);
    }
}

impl Render for Rotinas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.pagina {
            Pagina::Lista => self.lista(window, cx),
            Pagina::Formulario => self.formulario(window, cx),
        }
    }
}

fn hora_legivel(minutos: u32) -> String {
    format!("{:02}:{:02}", minutos / 60, minutos % 60)
}

/// Os dias da semana de uma rotina por extenso: "Todo dia", "Seg a Sex", "Fins de semana",
/// "Seg a Qua" ou "Seg, Qua, Sex".
fn descrever_dias(dias: &[u8]) -> String {
    let mut d: Vec<usize> = dias.iter().map(|d| *d as usize).filter(|d| *d < 7).collect();
    d.sort_unstable();
    d.dedup();
    let seguidos = d.windows(2).all(|par| par[1] == par[0] + 1);
    match d.as_slice() {
        [] => "Nenhum dia".into(),
        [0, 1, 2, 3, 4, 5, 6] => "Todo dia".into(),
        [1, 2, 3, 4, 5] => "Seg a Sex".into(),
        [0, 6] => "Fins de semana".into(),
        [primeiro, .., ultimo] if seguidos && d.len() >= 3 => format!("{} a {}", DIAS[*primeiro], DIAS[*ultimo]),
        _ => d.iter().map(|i| DIAS[*i]).collect::<Vec<_>>().join(", "),
    }
}

/// Quando a rotina roda de novo, a partir de agora (`dia` da semana, `minutos` do dia e a data
/// `hoje`), seguindo a mesma regra do motor: no máximo uma vez por dia, nos dias marcados.
fn proxima_execucao(rotina: &Rotina, dia: u8, minutos: u32, hoje: &str) -> String {
    if rotina.dias.contains(&dia) && rotina.ultima != hoje {
        return if minutos < rotina.hora { "hoje".into() } else { "agora".into() };
    }
    match (1..=7u8).find(|k| rotina.dias.contains(&((dia + k) % 7))) {
        Some(1) => "amanhã".into(),
        Some(7) => format!("{} que vem", DIAS[dia as usize].to_lowercase()),
        Some(k) => DIAS[((dia + k) % 7) as usize].to_lowercase(),
        None => "nunca".into(),
    }
}

#[cfg(test)]
mod testes {
    use super::{descrever_dias, hora_em, motivo_para_nao_salvar, proxima_execucao, resumo};
    use crate::modelo::Rotina;
    use chrono::Timelike as _;

    #[test]
    fn dias_por_extenso() {
        assert_eq!(descrever_dias(&[0, 1, 2, 3, 4, 5, 6]), "Todo dia");
        assert_eq!(descrever_dias(&[5, 1, 2, 3, 4]), "Seg a Sex");
        assert_eq!(descrever_dias(&[6, 0]), "Fins de semana");
        assert_eq!(descrever_dias(&[1, 2, 3]), "Seg a Qua");
        assert_eq!(descrever_dias(&[1, 3, 5]), "Seg, Qua, Sex");
        assert_eq!(descrever_dias(&[2, 2]), "Ter");
        assert_eq!(descrever_dias(&[1, 2]), "Seg, Ter");
        assert_eq!(descrever_dias(&[]), "Nenhum dia");
    }

    #[test]
    fn hora_do_campo_em_minutos() {
        let hora = hora_em(8 * 60 + 30);
        assert_eq!((hora.hour(), hora.minute()), (8, 30));
        assert_eq!(hora_em(23 * 60 + 59).hour(), 23);
    }

    #[test]
    fn motivos_somados_numa_frase() {
        assert_eq!(motivo_para_nao_salvar(true, true, true), None);
        assert_eq!(motivo_para_nao_salvar(false, true, true).as_deref(), Some("Escreva o pedido"));
        assert_eq!(
            motivo_para_nao_salvar(false, false, true).as_deref(),
            Some("Escreva o pedido e escolha pelo menos um dia")
        );
        assert_eq!(
            motivo_para_nao_salvar(false, false, false).as_deref(),
            Some("Escreva o pedido, escolha pelo menos um dia e escolha para quem")
        );
    }

    #[test]
    fn nome_da_rotina_apagada_cortado() {
        assert_eq!(resumo("curto", 40), "curto");
        assert_eq!(resumo(&"á".repeat(45), 40), format!("{}…", "á".repeat(40)));
    }

    #[test]
    fn proxima_segue_a_regra_do_motor() {
        let rotina = |dias: &[u8], ultima: &str| Rotina {
            hora: 8 * 60,
            dias: dias.to_vec(),
            ativa: true,
            ultima: ultima.into(),
            ..Default::default()
        };
        let uteis = [1, 2, 3, 4, 5];
        // Segunda às 07:00: ainda roda hoje.
        assert_eq!(proxima_execucao(&rotina(&uteis, ""), 1, 7 * 60, "2026-11-02"), "hoje");
        // Segunda às 09:00 sem ter rodado (o Vitral estava fechado): roda já.
        assert_eq!(proxima_execucao(&rotina(&uteis, ""), 1, 9 * 60, "2026-11-02"), "agora");
        // Já rodou hoje: amanhã.
        assert_eq!(proxima_execucao(&rotina(&uteis, "2026-11-02"), 1, 9 * 60, "2026-11-02"), "amanhã");
        // Sexta depois de rodar: pula o fim de semana.
        assert_eq!(proxima_execucao(&rotina(&uteis, "2026-11-06"), 5, 9 * 60, "2026-11-06"), "seg");
        // Sábado: a próxima é segunda.
        assert_eq!(proxima_execucao(&rotina(&uteis, ""), 6, 9 * 60, "2026-11-07"), "seg");
        // Só às quartas, já rodou nesta quarta.
        assert_eq!(proxima_execucao(&rotina(&[3], "2026-11-04"), 3, 9 * 60, "2026-11-04"), "qua que vem");
        assert_eq!(proxima_execucao(&rotina(&[], ""), 3, 9 * 60, "2026-11-04"), "nunca");
    }
}
