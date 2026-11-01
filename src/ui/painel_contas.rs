//! Diálogo das contas do Claude: a conta padrão da pasta (como no popover de uso do trilho: a
//! Automática e uma linha por conta, com o uso e o check na escolhida) e o login de contas novas
//! (o `claude auth login` mostra um endereço; o código da página volta para cá).

use std::time::Duration;

use gpui_kit::component::{
    Disableable as _, Icon, Sizable as _, StyledExt as _, WindowExt as _,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
    spinner::Spinner,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    componentes::{
        Alerta, AnelDeUso, CartaoDeLista, ChaveAviso as _, Confirmacao, Estado, GRUPO_DO_CARTAO, GrupoDeOpcoes, Marca,
        PilulaDeEstado, Simbolo, TamanhoBotao, Tom, TomConfirmacao, aviso, botao_fantasma, botao_icone, botao_primario,
        confirmar, dica_uso, icone, largura, resumo_uso,
    },
    dialogos::estreito,
};
use crate::{
    contas::{self, Conta, Login},
    modelo::{novo_id, quando_local},
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, Tipo as _, esp, r},
};

enum EstadoLogin {
    Parado,
    Abrindo,
    EsperandoCodigo { endereco: String, login: Login },
    Concluindo,
}

pub struct PainelContas {
    motor: Entity<Motor>,
    estado: EstadoLogin,
    codigo: Entity<InputState>,
    erro: Option<String>,
    /// Aberto pelo "Entrar com outra conta…": a lista fica numa linha até o "Ver".
    lista_recolhida: bool,
    /// "Copiar endereço" acabou de copiar: o botão diz "Copiado" por um instante.
    copiado: bool,
    _copiado: Task<()>,
    _assinaturas: Vec<Subscription>,
}

impl PainelContas {
    pub fn new(motor: Entity<Motor>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let codigo = cx.new(|cx| InputState::new(window, cx).placeholder("Código da página de login"));
        let assinaturas = vec![
            cx.observe(&motor, |_, _, cx| cx.notify()),
            cx.subscribe_in(&codigo, window, |this, _, evento, window, cx| match evento {
                InputEvent::PressEnter { .. } => this.concluir(window, cx),
                InputEvent::Change => cx.notify(),
                _ => {}
            }),
        ];
        Self {
            motor,
            estado: EstadoLogin::Parado,
            codigo,
            erro: None,
            lista_recolhida: false,
            copiado: false,
            _copiado: Task::ready(()),
            _assinaturas: assinaturas,
        }
    }

    /// Vindo do "Entrar com outra conta…": o login já começa, com a lista recolhida.
    pub(super) fn entrar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.lista_recolhida = true;
        self.adicionar(window, cx);
    }

    /// Começa o login de uma conta nova.
    pub(super) fn adicionar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.estado = EstadoLogin::Abrindo;
        self.erro = None;
        cx.notify();
        let id = novo_id();
        let tarefa = cx.background_spawn(async move { Login::iniciar(&id) });
        cx.spawn_in(window, async move |this, cx| {
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                match resultado {
                    Ok((login, endereco)) => {
                        this.estado = EstadoLogin::EsperandoCodigo { endereco, login };
                        let foco = this.codigo.read(cx).focus_handle(cx);
                        window.focus(&foco, cx);
                    }
                    Err(erro) => {
                        this.estado = EstadoLogin::Parado;
                        this.erro = Some(format!("{erro:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn concluir(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let codigo = self.codigo.read(cx).value().trim().to_string();
        if codigo.is_empty() {
            return;
        }
        let EstadoLogin::EsperandoCodigo { login, .. } = std::mem::replace(&mut self.estado, EstadoLogin::Concluindo)
        else {
            return;
        };
        cx.notify();
        let id = login.id.clone();
        let quantas = self.motor.read(cx).contas.len();
        let tarefa = cx.background_spawn(async move { login.concluir(&codigo) });
        cx.spawn_in(window, async move |this, cx| {
            let resultado = tarefa.await;
            let _ = this.update_in(cx, |this, window, cx| {
                this.estado = EstadoLogin::Parado;
                this.codigo.update(cx, |c, cx| c.set_value("", window, cx));
                match resultado {
                    Ok((email, plano)) => {
                        let nome = email
                            .split('@')
                            .next()
                            .filter(|n| !n.is_empty())
                            .map(str::to_string)
                            .unwrap_or_else(|| format!("Conta {}", quantas + 1));
                        let conta = Conta { id, nome, email: email.clone(), plano };
                        this.motor.update(cx, |m, cx| m.adicionar_conta(conta, cx));
                        this.lista_recolhida = false;
                        window.push_notification(aviso(Tom::Pronto, format!("Conta {email} conectada")), cx);
                    }
                    Err(erro) => {
                        contas::apagar_pasta(&id);
                        this.erro = Some(format!("{erro:#}"));
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn cancelar(&mut self, cx: &mut Context<Self>) {
        if let EstadoLogin::EsperandoCodigo { login, .. } = std::mem::replace(&mut self.estado, EstadoLogin::Parado) {
            let id = login.id.clone();
            drop(login);
            contas::apagar_pasta(&id);
        }
        cx.notify();
    }

    fn copiar_endereco(&mut self, endereco: String, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(endereco));
        self.copiado = true;
        cx.notify();
        self._copiado = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_millis(1500)).await;
            let _ = this.update(cx, |this, cx| {
                this.copiado = false;
                cx.notify();
            });
        });
    }

    /// Uma conta: o anel de uso (5 h por fora, a semana por dentro, como no trilho), o login e o
    /// plano, o uso (ou quando volta do limite), o "…" e o check na padrão. O clique a torna a
    /// padrão da pasta.
    fn linha_conta(&self, ix: usize, conta: &Conta, escolhida: bool, window: &Window, cx: &App) -> CartaoDeLista {
        let m = self.motor.read(cx);
        let uso = m.uso_da_conta(&conta.id);
        let principal = conta.id == contas::PRINCIPAL;
        let plano = m
            .plano_da_conta(&conta.id)
            .and_then(|p| p.nome.clone())
            .or_else(|| (!conta.plano.is_empty()).then(|| conta.plano.clone()));
        let login = if principal || conta.email.is_empty() { "login do Claude Code".to_string() } else { conta.email.clone() };
        let subtitulo = match plano {
            Some(p) => format!("{login} · plano {p}"),
            None => login,
        };
        let (cinco_h, semana) = (uso.cinco_horas.map(|f| f as f32), uso.semana.map(|f| f as f32));
        let anel = AnelDeUso::new(("conta-anel", ix), Marca::Claude, cinco_h)
            .semana(semana)
            .diametro(36.)
            .bloqueado(uso.bloqueado);
        let meta = if uso.bloqueado {
            let mut pilula = PilulaDeEstado::new(Estado::Erro).rotulo("no limite");
            if let Some(fim) = m.limite_vence(&conta.id) {
                pilula = pilula.detalhe(format!("volta {}", quando_local(fim)));
            }
            pilula.into_any_element()
        } else {
            let renova = |t: Option<i64>| t.map(|t| format!("renova {}", quando_local(t.max(0) as u64)));
            let (renova_5h, renova_semana) = (renova(uso.renova_5h), renova(uso.renova_semana));
            let dica: SharedString =
                dica_uso(Marca::Claude.nome(), cinco_h, renova_5h.as_deref(), semana, renova_semana.as_deref()).into();
            div()
                .id(("conta-uso", ix))
                .child(resumo_uso(cinco_h, semana))
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .into_any_element()
        };
        let menu = self.menu_conta(ix, conta, escolhida, window, cx);
        let escolher = (self.motor.clone(), conta.id.clone());
        CartaoDeLista::new(("conta", ix), conta.nome.clone())
            .simbolo(Simbolo::Elemento(anel.into_any_element()))
            .subtitulo(subtitulo)
            .meta(meta)
            .acao_fixa(acoes_da_linha(Some(menu), escolhida))
            .on_click(move |_, _, cx| escolher_conta(&escolher.0, &escolher.1, cx))
    }

    /// O "…" de uma conta: tornar padrão, copiar o e-mail e, nas extras, desconectar.
    fn menu_conta(&self, ix: usize, conta: &Conta, escolhida: bool, window: &Window, cx: &App) -> AnyElement {
        let (motor, id, email) = (self.motor.clone(), conta.id.clone(), conta.email.clone());
        let extra = conta.id != contas::PRINCIPAL;
        botao_icone(("conta-menu", ix), Lucide::Ellipsis, "Opções da conta", TamanhoBotao::Linha, window, cx)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, _, _| {
                let (padrao, sair) = ((motor.clone(), id.clone()), (motor.clone(), id.clone()));
                let copiar = email.clone();
                menu.item(
                    PopupMenuItem::new("Usar como padrão nesta pasta")
                        .checked(escolhida)
                        .on_click(move |_, _, cx| escolher_conta(&padrao.0, &padrao.1, cx)),
                )
                .when(!email.is_empty(), |menu| {
                    menu.item(PopupMenuItem::new("Copiar o e-mail").icon(Lucide::Copy).on_click(move |_, window, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(copiar.clone()));
                        window.push_notification(aviso(Tom::Pronto, "E-mail copiado").chave("copiar"), cx);
                    }))
                })
                .when(extra, |menu| {
                    menu.separator().item(
                        PopupMenuItem::new("Desconectar…")
                            .icon(Lucide::Unplug)
                            .danger(true)
                            .on_click(move |_, window, cx| confirmar_desconectar(sair.0.clone(), sair.1.clone(), window, cx)),
                    )
                })
            })
            .into_any_element()
    }

    /// "Entrar com outra conta": os dois passos (o navegador e o código) num grupo.
    fn passos_do_login(&self, estreito: bool, window: &Window, cx: &Context<Self>) -> AnyElement {
        let (primeiro, segundo) = match &self.estado {
            EstadoLogin::Abrindo => (EstadoPasso::Rodando, EstadoPasso::Esperando),
            EstadoLogin::EsperandoCodigo { .. } => (EstadoPasso::Feito, EstadoPasso::Ativo),
            _ => (EstadoPasso::Feito, EstadoPasso::Rodando),
        };
        let botoes_do_navegador = match &self.estado {
            EstadoLogin::EsperandoCodigo { endereco, .. } => {
                let (abrir, copiar) = (endereco.clone(), endereco.clone());
                let (icone_copiar, rotulo_copiar) =
                    if self.copiado { (Lucide::Check, "Copiado") } else { (Lucide::Copy, "Copiar endereço") };
                let abrir_botao = if estreito {
                    botao_icone("login-abrir", Lucide::ExternalLink, "Abrir de novo", TamanhoBotao::Linha, window, cx)
                } else {
                    botao_fantasma("login-abrir", Some(Lucide::ExternalLink.into()), "Abrir de novo", TamanhoBotao::Linha, window, cx)
                };
                let copiar_botao = if estreito {
                    botao_icone("login-copiar", icone_copiar, rotulo_copiar, TamanhoBotao::Linha, window, cx)
                } else {
                    botao_fantasma("login-copiar", Some(icone_copiar.into()), rotulo_copiar, TamanhoBotao::Linha, window, cx)
                };
                Some(
                    div()
                        .flex()
                        .gap(r(4.))
                        .child(abrir_botao.on_click(move |_, _, cx| cx.open_url(&abrir)))
                        .child(copiar_botao.on_click(cx.listener(move |this, _, _, cx| this.copiar_endereco(copiar.clone(), cx)))),
                )
            }
            _ => None,
        };
        let concluindo = matches!(self.estado, EstadoLogin::Concluindo);
        let vazio = self.codigo.read(cx).value().trim().is_empty();
        let entrar = botao_primario(
            "login-entrar",
            None,
            if concluindo { "Entrando…" } else { "Entrar" },
            TamanhoBotao::Controle,
            window,
            cx,
        )
        .loading(concluindo)
        .when(vazio && !concluindo, |b| b.disabled(true).opacity(0.45).tooltip("Cole o código da página"))
        .on_click(cx.listener(|this, _, window, cx| this.concluir(window, cx)));
        let campo = div()
            .flex()
            .items_center()
            .gap(r(8.))
            .child(div().flex_1().min_w_0().child(Input::new(&self.codigo).font_family(FONTE_MONO)))
            .child(entrar);
        GrupoDeOpcoes::new(Some("Entrar com outra conta".into()))
            .linha(passo(
                1,
                primeiro,
                if primeiro == EstadoPasso::Rodando {
                    "Abrindo o login no navegador…"
                } else {
                    "Entre com a outra conta no navegador"
                },
                "A página de login abriu. Se o navegador já estiver na conta principal, use uma janela anônima.",
                botoes_do_navegador.map(IntoElement::into_any_element),
                window,
            ))
            .linha(passo(
                2,
                segundo,
                "Cole o código do fim da página",
                "Depois de autorizar, a página mostra um código para colar aqui.",
                (segundo != EstadoPasso::Esperando).then(|| campo.into_any_element()),
                window,
            ))
            .into_any_element()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EstadoPasso {
    /// Ainda não chegou nele: apagado.
    Esperando,
    Ativo,
    Rodando,
    Feito,
}

/// Um passo numerado: o círculo de 22 (número, spinner ou check), o título, a explicação e o
/// que se faz nele.
fn passo(
    numero: u8,
    estado: EstadoPasso,
    titulo: &'static str,
    texto: &'static str,
    extra: Option<AnyElement>,
    window: &Window,
) -> impl IntoElement + use<> {
    let circulo = div()
        .flex_none()
        .size(r(22.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .text_size(r(12.))
        .font_semibold()
        .map(|c| match estado {
            EstadoPasso::Feito => c
                .bg(Cores::tinta(Cores::verde()))
                .text_color(Cores::verde())
                .child(icone(Lucide::Check, 12., window)),
            EstadoPasso::Rodando => c.bg(Cores::selecao()).child(Spinner::new().xsmall().color(Cores::destaque_texto())),
            EstadoPasso::Ativo => c.bg(Cores::selecao()).text_color(Cores::destaque_texto()).child(numero.to_string()),
            EstadoPasso::Esperando => c.bg(Cores::hover()).text_color(Cores::tenue()).child(numero.to_string()),
        });
    div()
        .flex()
        .items_start()
        .gap(r(12.))
        .py(esp::E12)
        .px(r(12.))
        .when(estado == EstadoPasso::Esperando, |s| s.opacity(0.45))
        .child(circulo)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap(r(2.))
                .child(div().text_size(r(14.)).line_height(r(20.)).font_weight(FontWeight::MEDIUM).text_color(Cores::texto()).child(titulo))
                .child(div().metadado().text_color(Cores::tenue()).child(texto))
                .children(extra.map(|e| div().pt(r(6.)).child(e))),
        )
}

/// O "…" (no hover, sem cobrir o uso) e o check da escolhida, numa coluna própria. O clique
/// nela não chega à linha.
fn acoes_da_linha(menu: Option<AnyElement>, escolhida: bool) -> impl IntoElement + use<> {
    div()
        .id("acoes-da-conta")
        .flex()
        .items_center()
        .gap(r(4.))
        .on_click(|_, _, cx| cx.stop_propagation())
        .child(
            div()
                .w(r(24.))
                .flex_none()
                .opacity(0.)
                .group_hover(GRUPO_DO_CARTAO, |s| s.opacity(1.))
                .children(menu),
        )
        .child(div().w(r(16.)).flex_none().when(escolhida, |s| {
            s.child(Icon::new(Lucide::Check).size(r(14.)).text_color(Cores::destaque_texto()))
        }))
}

impl Render for PainelContas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let estreito = estreito(window, largura::M);
        let motor = self.motor.read(cx);
        let contas = motor.contas.clone();
        let ids: Vec<&str> = contas.iter().map(|c| c.id.as_str()).collect();
        let marcada = linha_marcada(&motor.config.conta, &ids);
        let lista = if self.lista_recolhida {
            div()
                .flex()
                .items_center()
                .gap(r(8.))
                .metadado()
                .text_color(Cores::tenue())
                .child(super::componentes::contagem(contas.len(), "conta conectada", "contas conectadas"))
                .child(
                    botao_fantasma("contas-ver", None, "Ver", TamanhoBotao::Linha, window, cx)
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.lista_recolhida = false;
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            let automatica = marcada.as_deref() == Some(contas::AUTOMATICA);
            let escolher = self.motor.clone();
            let linha_automatica = CartaoDeLista::new("conta-automatica", "Automática")
                .simbolo(Simbolo::Elemento(
                    div()
                        .size(r(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded_full()
                        .bg(Cores::hover())
                        .text_color(Cores::tenue())
                        .child(icone(Lucide::Shuffle, 18., window))
                        .into_any_element(),
                ))
                .subtitulo("A de menor uso; troca sozinha se uma chegar ao limite")
                .acao_fixa(acoes_da_linha(None, automatica))
                .on_click(move |_, _, cx| escolher_conta(&escolher, contas::AUTOMATICA, cx));
            let mut grupo = GrupoDeOpcoes::new(Some("Conta padrão".into())).linha(linha_automatica);
            for (ix, conta) in contas.iter().enumerate() {
                let escolhida = marcada.as_deref() == Some(conta.id.as_str());
                grupo = grupo.linha(self.linha_conta(ix, conta, escolhida, window, cx));
            }
            div()
                .flex()
                .flex_col()
                .gap(r(6.))
                .child(grupo)
                .child(
                    div()
                        .px(r(4.))
                        .metadado()
                        .text_color(Cores::tenue())
                        .child("A padrão vale para as conversas desta pasta que não escolheram outra."),
                )
                .into_any_element()
        };
        let login = match &self.estado {
            EstadoLogin::Parado => div()
                .flex()
                .child(
                    botao_fantasma("contas-entrar", Some(Lucide::Plus.into()), "Entrar com outra conta…", TamanhoBotao::Barra, window, cx)
                        .on_click(cx.listener(|this, _, window, cx| this.adicionar(window, cx))),
                )
                .into_any_element(),
            _ => div()
                .flex()
                .flex_col()
                .gap(r(8.))
                .child(self.passos_do_login(estreito, window, cx))
                .child(
                    div().flex().justify_end().child(
                        botao_fantasma("login-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                            .on_click(cx.listener(|this, _, _, cx| this.cancelar(cx))),
                    ),
                )
                .into_any_element(),
        };
        let erro = self.erro.clone().map(|erro| {
            Alerta::new(Tom::Erro, erro).titulo("Não deu para entrar").acao(
                botao_fantasma("login-de-novo", Some(Lucide::RotateCw.into()), "Tentar de novo", TamanhoBotao::Barra, window, cx)
                    .on_click(cx.listener(|this, _, window, cx| this.adicionar(window, cx))),
            )
        });
        div().flex().flex_col().gap(r(16.)).child(lista).children(erro).child(login)
    }
}

/// Qual linha leva o check para a conta guardada na config ("" é a principal). Uma escolha que
/// não existe mais não marca nenhuma.
fn linha_marcada(config: &str, ids: &[&str]) -> Option<String> {
    let escolha = if config.is_empty() { contas::PRINCIPAL } else { config };
    (escolha == contas::AUTOMATICA || ids.contains(&escolha)).then(|| escolha.to_string())
}

/// "limite atingido, volta às 15:42": a hora em que a conta no limite volta, quando se sabe.
pub(super) fn limite_atingido(volta: Option<u64>) -> String {
    match volta {
        Some(fim) => format!("limite atingido, volta {}", quando_local(fim)),
        None => "limite atingido".to_string(),
    }
}

/// Passa a usar a escolha (uma conta ou a automática) como conta padrão da pasta; grava a
/// config só se mudou.
pub(super) fn escolher_conta(motor: &Entity<Motor>, escolha: &str, cx: &mut App) {
    let valor = valor_da_config(escolha);
    motor.update(cx, |m, cx| {
        if m.config.conta != valor {
            m.mudar_config(|c| c.conta = valor, cx);
        }
    });
}

/// Como a escolha fica guardada na config: a principal é o vazio.
pub(super) fn valor_da_config(escolha: &str) -> String {
    if escolha == contas::PRINCIPAL { String::new() } else { escolha.to_string() }
}

/// Pergunta antes de desconectar uma conta extra (apaga o login dela neste computador); a
/// principal, o login normal do Claude Code, não sai. O Enter só fecha.
pub(super) fn confirmar_desconectar(motor: Entity<Motor>, id: String, window: &mut Window, cx: &mut App) {
    if id == contas::PRINCIPAL {
        return;
    }
    let nome = motor.read(cx).contas.iter().find(|c| c.id == id).map(|c| c.nome.clone()).unwrap_or_default();
    confirmar(
        Confirmacao {
            tom: TomConfirmacao::Perigo,
            icone: Lucide::Unplug.into(),
            titulo: format!("Desconectar “{nome}”?").into(),
            texto: "O Vitral apaga o login dela neste computador.".into(),
            detalhes: vec![(Lucide::MessagesSquare.into(), "As conversas dela passam para a conta padrão".into(), false)],
            ok: "Desconectar".into(),
            cancelar: "Cancelar".into(),
            ok_perigo: true,
        },
        move |_, cx| motor.update(cx, |m, cx| m.remover_conta(&id, cx)),
        None,
        window,
        cx,
    );
}

#[cfg(test)]
mod testes {
    use super::{limite_atingido, linha_marcada, valor_da_config};
    use crate::{contas, modelo::agora};

    #[test]
    fn escolha_guardada_na_config() {
        assert_eq!(valor_da_config(contas::PRINCIPAL), "");
        assert_eq!(valor_da_config(contas::AUTOMATICA), "auto");
        assert_eq!(valor_da_config("abc123"), "abc123");
    }

    #[test]
    fn limite_com_a_hora_em_que_volta() {
        assert_eq!(limite_atingido(None), "limite atingido");
        assert!(limite_atingido(Some(agora())).starts_with("limite atingido, volta às "));
    }

    #[test]
    fn check_na_conta_padrao_como_no_popover() {
        let ids = [contas::PRINCIPAL, "abc"];
        assert_eq!(linha_marcada("", &ids).as_deref(), Some(contas::PRINCIPAL));
        assert_eq!(linha_marcada("abc", &ids).as_deref(), Some("abc"));
        assert_eq!(linha_marcada(contas::AUTOMATICA, &ids).as_deref(), Some(contas::AUTOMATICA));
        assert_eq!(linha_marcada("sumiu", &ids), None);
    }
}
