//! Uso dos planos no pé do trilho: um botão por serviço (Claude e Codex), o disco da marca com o
//! anel da janela de 5 h por fora e o da semana por dentro. O clique abre o popover com os
//! detalhes; no do Claude também se escolhe a conta padrão, entra com outra ou desconecta uma.
//! Sem o Codex, o botão dele fica apagado e o popover oferece a instalação. No modo recolhido, os
//! mesmos botões, de 24, ficam no cabeçalho do 1º painel, logo depois do logo (o Codex só quando
//! está instalado e tem uso lido).

use gpui_kit::component::{
    Icon, Selectable, StyledExt as _, h_flex,
    menu::{DropdownMenu as _, PopupMenuItem},
    popover::PopoverState,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        Alerta, AnelDeUso, CabecalhoDeSecao, CabecalhoPopover, CartaoDeLista, CorpoPopover, EstadoVazio, GRUPO_DO_CARTAO,
        LarguraPopover, Marca, Selo, Simbolo, TamanhoBotao, Tom, botao_fantasma, botao_icone, botao_primario,
        cor_da_faixa, icone, popover, resumo_uso,
    },
    painel_contas::{confirmar_desconectar, escolher_conta, limite_atingido},
};
use crate::{
    codex::Codex,
    contas::{self, Conta, Creditos, Parada, Plano},
    modelo::agora,
    motor::{Motor, Uso},
    sistema,
    tema::{Cores, Lucide, Tipo as _, esp, halo_foco, r},
};

/// Instalador oficial (vai para `~/.local/bin`, que o Vitral aberto pelo GNOME enxerga) só se
/// ainda não houver um `codex`, e depois o login com a conta do ChatGPT. Roda no `bash -lc` do
/// terminal; cada falha (rede, endereço, binário fora do PATH) deixa uma mensagem.
const INSTALAR_CODEX: &str = "set -o pipefail; export PATH=\"$HOME/.local/bin:$PATH\"; \
    if ! command -v codex >/dev/null; then \
    echo 'Instalando o Codex pelo instalador oficial da OpenAI...'; \
    curl -fsSL https://chatgpt.com/codex/install.sh | sh \
    || echo 'Não deu para instalar o Codex (veja o erro acima).'; fi; \
    if command -v codex >/dev/null; then codex login; \
    else echo 'O Codex não foi encontrado no PATH depois da instalação.'; fi";

const DIAS: [&str; 7] = ["dom.", "seg.", "ter.", "qua.", "qui.", "sex.", "sáb."];

/// Abre o terminal do usuário com a instalação (se faltar) e o login do Codex.
pub(super) fn instalar_codex() {
    if let Some(casa) = dirs::home_dir() {
        sistema::abrir_terminal(&casa, None, Some(INSTALAR_CODEX));
    }
}

impl Janela {
    /// Os botões de uso do Claude (conta padrão) e do Codex no pé do trilho.
    pub(super) fn uso_plano(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        v_flex().gap(r(8.)).items_center().children(self.botoes_uso(false, cx))
    }

    /// Os anéis no cabeçalho do 1º painel com o trilho escondido: de 24, lado a lado, centrados
    /// na linha de 44, e os popovers abrem para baixo.
    pub(super) fn uso_compacto(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let botoes = self.botoes_uso(true, cx);
        (!botoes.is_empty()).then(|| h_flex().flex_none().gap(esp::E4).children(botoes).into_any_element())
    }

    fn botoes_uso(&self, compacto: bool, cx: &Context<Self>) -> Vec<AnyElement> {
        let Some(motor) = self.motor.clone() else {
            return Vec::new();
        };
        let janela = cx.entity().downgrade();
        let (claude, codex) = {
            let m = motor.read(cx);
            let conta = m.conta_padrao();
            // Com uma conta só não há o que distinguir: sem selo e sem a conta na dica.
            let varias = (m.contas.len() > 1).then_some(&conta);
            (
                leitura_claude(
                    &m.uso_da_conta(&conta.id),
                    varias,
                    m.config.conta == contas::AUTOMATICA,
                    m.limite_vence(&conta.id),
                ),
                leitura_codex(m.codex.as_ref(), m.codex_lido),
            )
        };
        let mut botoes = vec![popover_uso(Marca::Claude, claude, compacto, motor.clone(), janela.clone()).into_any_element()];
        // No cabeçalho, o Codex só aparece instalado e com uso lido.
        if !compacto || codex.em_uso() {
            botoes.push(popover_uso(Marca::Codex, codex, compacto, motor, janela).into_any_element());
        }
        botoes
    }
}

/// Botão do serviço e o popover com os detalhes: à direita do trilho ou, no modo recolhido,
/// para baixo do cabeçalho.
fn popover_uso(
    marca: Marca,
    leitura: Leitura,
    compacto: bool,
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
) -> impl IntoElement + use<> {
    let id = if compacto { ("uso-popover-recolhido", marca as usize) } else { ("uso-popover", marca as usize) };
    popover(id)
        .anchor(if compacto { Anchor::TopLeft } else { Anchor::LeftCenter })
        .offset(px(8.))
        .arrow(true)
        .trigger(BotaoUso {
            marca,
            leitura,
            diametro: if compacto { 24. } else { 36. },
            aberto: false,
        })
        .content(move |_, window, cx| {
            let popover = cx.entity();
            detalhes(marca, motor.clone(), janela.clone(), popover, window, cx)
        })
}

/// Botão redondo de um serviço: o disco da marca com os dois anéis do uso sobre um disco preto,
/// para a cor da marca saltar, que clareia no hover. É o gatilho do popover, por isso guarda o
/// estado "aberto".
#[derive(IntoElement)]
struct BotaoUso {
    marca: Marca,
    leitura: Leitura,
    diametro: f32,
    aberto: bool,
}

impl Selectable for BotaoUso {
    fn selected(mut self, selected: bool) -> Self {
        self.aberto = selected;
        self
    }

    fn is_selected(&self) -> bool {
        self.aberto
    }
}

impl RenderOnce for BotaoUso {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let l = self.leitura;
        let dica = SharedString::from(l.dica);
        let id = self.marca as usize;
        div()
            .id(("uso-botao", id))
            .relative()
            .flex_none()
            .size(r(self.diametro))
            .rounded_full()
            .cursor_pointer()
            .tab_index(0)
            .focus_visible(|s| s.shadow(halo_foco()))
            .bg(Cores::preto())
            .when(self.aberto, |s| s.bg(Cores::preto().blend(Cores::pressionado())))
            .when(!self.aberto, |s| {
                s.hover(|s| s.bg(Cores::preto().blend(Cores::hover())))
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            })
            .child(
                AnelDeUso::new(("uso-anel", id), self.marca, l.fora.map(|f| f as f32))
                    .semana(l.dentro.map(|f| f as f32))
                    .diametro(self.diametro)
                    .bloqueado(l.bloqueado)
                    .apagado(l.apagado),
            )
            .when_some(l.selo, |s, letra| s.child(inicial_da_conta(letra, self.diametro <= 24.)))
    }
}

/// Inicial da conta em uso, pequena no canto de baixo do botão (menor no anel de 24).
fn inicial_da_conta(letra: String, pequena: bool) -> impl IntoElement + use<> {
    div()
        .absolute()
        .right(r(-2.))
        .bottom(r(-2.))
        .size(r(if pequena { 12. } else { 15. }))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .border_1()
        .border_color(Cores::borda_forte())
        .bg(Cores::flutuante())
        .text_color(Cores::texto())
        .text_size(r(if pequena { 8. } else { 9. }))
        .line_height(r(10.))
        .font_semibold()
        .child(letra)
}

/// O que o botão de um serviço mostra: o anel de fora, o de dentro, a dica, se está no limite,
/// se está apagado (sem o serviço ou ainda sem leitura) e, no Claude com várias contas, a
/// inicial da conta em uso.
#[derive(Clone, Debug, PartialEq)]
struct Leitura {
    fora: Option<f64>,
    dentro: Option<f64>,
    dica: String,
    selo: Option<String>,
    bloqueado: bool,
    apagado: bool,
}

impl Leitura {
    /// Há o que mostrar: o serviço instalado e algum uso lido.
    fn em_uso(&self) -> bool {
        !self.apagado && (self.fora.is_some() || self.dentro.is_some())
    }
}

/// Claude: 5 h fora e semana dentro. Com `conta` (há mais de uma), a dica diz qual está em uso
/// e o botão ganha o selo com a inicial dela. No limite, a dica diz quando ela volta.
fn leitura_claude(uso: &Uso, conta: Option<&Conta>, automatica: bool, volta: Option<u64>) -> Leitura {
    let nome = match conta {
        Some(c) if automatica => format!("{} · conta {} (automática)", Marca::Claude.nome(), c.nome),
        Some(c) => format!("{} · conta {}", Marca::Claude.nome(), c.nome),
        None => Marca::Claude.nome().to_string(),
    };
    Leitura {
        fora: uso.cinco_horas,
        dentro: uso.semana,
        dica: if uso.bloqueado {
            format!("{nome} · {}", limite_atingido(volta))
        } else {
            dica_janelas(&nome, uso)
        },
        selo: conta.map(inicial),
        bloqueado: uso.bloqueado,
        apagado: false,
    }
}

/// Codex: como o Claude, mas sem 5 h nem semana a maior das outras janelas (diária, mensal) vai
/// no anel de fora; e a dica diz quando ainda está lendo, não instalado ou sem limites.
fn leitura_codex(codex: Option<&Codex>, lido: bool) -> Leitura {
    let nome = Marca::Codex.nome();
    let so_dica = |dica: String, apagado: bool| Leitura {
        fora: None,
        dentro: None,
        dica,
        selo: None,
        bloqueado: false,
        apagado,
    };
    let Some(codex) = codex else {
        return so_dica(
            if lido { format!("{nome} · não instalado") } else { format!("{nome} · lendo o uso…") },
            true,
        );
    };
    let uso = &codex.uso;
    if uso.cinco_horas.is_none() && uso.semana.is_none() {
        if let Some((rotulo, fracao, _)) = codex.outras.iter().max_by(|a, b| a.1.total_cmp(&b.1)) {
            return Leitura {
                fora: Some(*fracao),
                dentro: None,
                dica: format!("{nome} · {} {}%", rotulo.to_lowercase(), porcentagem(*fracao)),
                selo: None,
                bloqueado: uso.bloqueado,
                apagado: false,
            };
        }
        if codex.sem_limites {
            return so_dica(format!("{nome} · sem limites do plano"), false);
        }
    }
    Leitura {
        fora: uso.cinco_horas,
        dentro: uso.semana,
        dica: dica_janelas(nome, uso),
        selo: None,
        bloqueado: uso.bloqueado,
        apagado: false,
    }
}

/// Dica curta das duas janelas: "Claude · 5 h 7% · semana 38%".
fn dica_janelas(nome: &str, uso: &Uso) -> String {
    let numero = |f: Option<f64>| f.map_or("–".to_string(), |f| format!("{}%", porcentagem(f)));
    match (uso.cinco_horas, uso.semana) {
        (None, None) => format!("{nome} · sem leitura do uso"),
        (None, Some(_)) => format!("{nome} · semana {}", numero(uso.semana)),
        _ => format!(
            "{nome} · 5 h {} · semana {}",
            numero(uso.cinco_horas),
            numero(uso.semana)
        ),
    }
}

/// "5 h 7% · semana 38%": o uso de uma conta na linha dela, com o texto do menu de conta (a
/// janela sem leitura fica de fora).
fn resumo_conta(uso: &Uso) -> String {
    resumo_uso(uso.cinco_horas.map(|f| f as f32), uso.semana.map(|f| f as f32))
}

/// O conteúdo do popover, lido do motor a cada desenho (acompanha as releituras).
fn detalhes(
    marca: Marca,
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    popover: Entity<PopoverState>,
    window: &mut Window,
    cx: &App,
) -> AnyElement {
    let agora = agora() as i64;
    let popover = popover.downgrade();
    match marca {
        Marca::Claude => detalhes_claude(&motor, janela, popover, agora, window, cx).into_any_element(),
        Marca::Codex => detalhes_codex(&motor, janela, popover, agora, window, cx).into_any_element(),
    }
}

/// Conta padrão em detalhe (janelas, limites por modelo, créditos) e as contas, onde se troca
/// a conta padrão, entra com outra ou desconecta uma.
fn detalhes_claude(
    entidade: &Entity<Motor>,
    janela: WeakEntity<Janela>,
    popover: WeakEntity<PopoverState>,
    agora: i64,
    window: &Window,
    cx: &App,
) -> CorpoPopover {
    let motor = entidade.read(cx);
    let base = Marca::Claude.cor();
    let conta = motor.conta_padrao();
    let uso = motor.uso_da_conta(&conta.id);
    let volta = motor.limite_vence(&conta.id);
    let plano = motor.plano_da_conta(&conta.id).cloned().unwrap_or_default();
    let plano_em_uso = nome_do_plano(Some(&plano), &conta);
    let contexto = if motor.config.conta == contas::AUTOMATICA {
        format!("Conta {} · automática", conta.nome)
    } else {
        format!("Conta {}", conta.nome)
    };
    let ids: Vec<&str> = motor.contas.iter().map(|c| c.id.as_str()).collect();
    let marcada = linha_marcada(&motor.config.conta, &ids);
    let linhas: Vec<LinhaConta> = motor
        .contas
        .iter()
        .map(|c| LinhaConta {
            id: c.id.clone(),
            rotulo: if c.email.is_empty() { c.nome.clone() } else { c.email.clone() },
            uso: motor.uso_da_conta(&c.id),
            volta: motor.limite_vence(&c.id),
        })
        .collect();
    let medido_em = uso.medido_em;
    let secao = secao_contas(linhas, marcada, entidade.clone(), janela.clone(), popover.clone(), window, cx);
    // 400, e não 360: o e-mail da conta inteiro numa linha.
    CorpoPopover::new(LarguraPopover::P400)
        .cabecalho(CabecalhoPopover::Objeto {
            simbolo: Simbolo::Marca(Marca::Claude),
            nome: Marca::Claude.nome().into(),
            detalhe: Some(contexto.into()),
            selo: plano_em_uso.map(Selo::neutro),
        })
        .when(uso.bloqueado, |s| s.child(Alerta::new(Tom::Erro, maiuscula(&limite_atingido(volta)))))
        .child(linha_janela("Sessão de 5 h".into(), uso.cinco_horas, uso.renova_5h, base, agora))
        .child(linha_janela("Semana".into(), uso.semana, uso.renova_semana, base, agora))
        .children(plano.por_modelo.into_iter().map(move |(modelo, fracao, renova)| {
            // A mesma regra do `Uso::juntar`: o limite que já renovou volta a zero.
            let fracao = if renova.is_some_and(|r| r <= agora) { 0.0 } else { fracao };
            linha_janela(format!("Semana · {modelo}").into(), Some(fracao), renova, base, agora)
        }))
        .when_some(plano.creditos, |s, creditos| s.child(linha_creditos(&creditos)))
        .child(separador())
        .child(secao)
        .rodape(rodape(Marca::Claude, medido_em, agora, entidade.clone(), janela, popover, window, cx))
}

/// Codex: as janelas da última resposta neste computador ou, sem dados, o que falta fazer.
fn detalhes_codex(
    entidade: &Entity<Motor>,
    janela: WeakEntity<Janela>,
    popover: WeakEntity<PopoverState>,
    agora: i64,
    window: &Window,
    cx: &App,
) -> CorpoPopover {
    let motor = entidade.read(cx);
    let base = Marca::Codex.cor();
    let cabecalho = |plano: Option<String>, contexto: &'static str| CabecalhoPopover::Objeto {
        simbolo: Simbolo::Marca(Marca::Codex),
        nome: Marca::Codex.nome().into(),
        detalhe: Some(contexto.into()),
        selo: plano.map(Selo::neutro),
    };
    let medido_em = motor.codex.as_ref().and_then(|c| c.uso.medido_em);
    let pe = rodape(Marca::Codex, medido_em, agora, entidade.clone(), janela, popover.clone(), window, cx);
    let corpo = CorpoPopover::new(LarguraPopover::P360).rodape(pe);
    let Some(codex) = motor.codex.clone() else {
        if !motor.codex_lido {
            return corpo.cabecalho(cabecalho(None, "Lendo o uso neste computador…"));
        }
        return corpo.cabecalho(cabecalho(None, "Não instalado neste computador")).child(
            EstadoVazio::new(Lucide::Download, "Codex não instalado")
                .texto(
                    "Instale o Codex da OpenAI e entre com a conta do ChatGPT. O uso do plano \
                     aparece aqui depois da primeira resposta.",
                )
                .acao(botao_instalar("Instalar e entrar", popover, window, cx)),
        );
    };
    let sem_dado = codex.uso.cinco_horas.is_none() && codex.uso.semana.is_none() && codex.outras.is_empty();
    let uso = codex.uso;
    corpo
        .cabecalho(cabecalho(codex.plano.clone(), "Último uso neste computador"))
        .when(codex.sem_limites, |s| s.child(texto("Login com chave de API: sem limites do plano.")))
        .when(sem_dado && !codex.sem_limites, |s| {
            s.child(texto(
                "Ainda sem leitura. O uso aparece depois da próxima resposta do Codex neste \
                 computador; se ainda não entrou, entre com a conta do ChatGPT.",
            ))
            .when(!codex.logado, |s| {
                s.child(h_flex().justify_end().child(botao_instalar("Entrar", popover.clone(), window, cx)))
            })
        })
        .when(uso.bloqueado, |s| s.child(Alerta::new(Tom::Erro, "Limite atingido")))
        .when_some(uso.cinco_horas, |s, fracao| {
            s.child(linha_janela("Sessão de 5 h".into(), Some(fracao), uso.renova_5h, base, agora))
        })
        .when_some(uso.semana, |s, fracao| {
            s.child(linha_janela("Semana".into(), Some(fracao), uso.renova_semana, base, agora))
        })
        .children(
            codex
                .outras
                .into_iter()
                .map(move |(rotulo, fracao, renova)| linha_janela(rotulo.into(), Some(fracao), renova, base, agora)),
        )
        .when(!sem_dado, |s| {
            s.child(texto(
                "Uso feito em outro computador ou no Codex na web só aparece aqui depois da \
                 próxima resposta neste computador.",
            ))
        })
}

/// Barra de 6, pílula, na trilha da borda.
fn barra(fracao: f64, cor: Hsla) -> Div {
    div()
        .h(r(6.))
        .w_full()
        .rounded_full()
        .bg(Cores::borda())
        .child(div().h_full().w(relative(fracao.clamp(0., 1.) as f32)).rounded_full().bg(cor))
}

/// Uma janela no popover: rótulo e porcentagem (em âmbar ou vermelho a partir de 70%), a barra
/// e quando renova.
fn linha_janela(
    rotulo: SharedString,
    fracao: Option<f64>,
    renova: Option<i64>,
    base: Hsla,
    agora: i64,
) -> impl IntoElement + use<> {
    let cor = cor_da_faixa(fracao.unwrap_or(0.) as f32, base);
    v_flex()
        .gap(r(4.))
        .child(
            h_flex()
                .justify_between()
                .items_baseline()
                .gap(r(8.))
                .child(div().rotulo().text_color(Cores::texto()).child(rotulo))
                .child(match fracao.map(porcentagem) {
                    Some(p) => div()
                        .text_size(r(13.))
                        .font_semibold()
                        .text_color(if p >= 70 { cor } else { Cores::forte() })
                        .child(format!("{p}%")),
                    None => div().metadado().text_color(Cores::apagado()).child("sem dado"),
                }),
        )
        // Sem dado, só o rótulo: a barra vazia diria 0%.
        .when_some(fracao, |s, f| s.child(barra(f, cor)))
        .when_some(texto_renovacao(renova, agora), |s, t| {
            s.child(div().metadado().text_color(Cores::tenue()).child(t))
        })
}

/// Créditos extras usados no mês, com a barra quando há limite; esgotados (limite do mês
/// atingido, sem créditos), a barra fica cheia e em vermelho.
fn linha_creditos(creditos: &Creditos) -> impl IntoElement + use<> {
    let usados = dinheiro(creditos.usados, creditos.casas, &creditos.moeda);
    let (fracao, legenda, esgotado) = legenda_creditos(creditos);
    v_flex()
        .gap(r(4.))
        .child(
            h_flex()
                .justify_between()
                .items_baseline()
                .gap(r(8.))
                .child(div().rotulo().text_color(Cores::texto()).child("Créditos extras"))
                .child(
                    div()
                        .text_size(r(13.))
                        .font_semibold()
                        .text_color(if esgotado { Cores::vermelho() } else { Cores::forte() })
                        .child(usados),
                ),
        )
        .when_some(fracao, |s, fracao| {
            let cor = if esgotado { Cores::vermelho() } else { cor_da_faixa(fracao as f32, Marca::Claude.cor()) };
            s.child(barra(fracao, cor))
        })
        .child(
            div()
                .metadado()
                .text_color(if esgotado { Cores::vermelho() } else { Cores::tenue() })
                .child(legenda),
        )
}

/// Fração da barra (`None`: sem barra), a legenda embaixo e se os créditos se esgotaram.
fn legenda_creditos(creditos: &Creditos) -> (Option<f64>, String, bool) {
    let valor = |v: i64| dinheiro(v, creditos.casas, &creditos.moeda);
    let limite = creditos.limite.filter(|l| *l > 0);
    match (creditos.parada, limite) {
        (Some(Parada::LimiteDoMes), Some(l)) => {
            (Some(1.0), format!("limite do mês de {} atingido", valor(l)), true)
        }
        (Some(Parada::LimiteDoMes), None) => (Some(1.0), "limite do mês atingido".into(), true),
        (Some(Parada::SemCreditos), _) => (Some(1.0), "sem créditos".into(), true),
        (Some(Parada::Desligados), _) => (None, "usados no mês · desligados agora".into(), false),
        (None, Some(l)) => {
            let fracao = creditos.usados as f64 / l as f64;
            let texto = format!("{}% do limite do mês de {}", porcentagem(fracao), valor(l));
            (Some(fracao), texto, false)
        }
        (None, None) => (None, "usados no mês · sem limite".into(), false),
    }
}

/// Uma conta na lista do popover.
struct LinhaConta {
    id: String,
    /// E-mail ou, sem ele (a principal antes do `auth status`), o nome.
    rotulo: String,
    uso: Uso,
    /// Quando ela sai do limite (só com `uso.bloqueado`).
    volta: Option<u64>,
}

/// "Conta padrão": a automática e uma linha por conta, com o check na escolhida. Clicar numa
/// linha a torna a padrão da pasta; embaixo, entrar com outra conta.
fn secao_contas(
    linhas: Vec<LinhaConta>,
    marcada: Option<String>,
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    popover: WeakEntity<PopoverState>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let automatica = marcada.as_deref() == Some(contas::AUTOMATICA);
    let escolher = motor.clone();
    let entrar = popover.clone();
    // Como os itens de menu do kit: o realce do hover a 8 da borda, e o símbolo e o texto na guia
    // de 16 do cabeçalho (o "Entrar com outra conta…" também, pelo padding dele).
    v_flex()
        .mx(r(-8.))
        .gap(r(2.))
        .child(CabecalhoDeSecao::new("Conta padrão").primeira(true))
        .child(
            CartaoDeLista::new("uso-conta-automatica", "Automática")
                .simbolo(Simbolo::Elemento(circulo_automatica(window)))
                .subtitulo("a de menor uso em cada turno")
                .acao_fixa(marca(automatica))
                .on_click(move |_, _, cx| escolher_conta(&escolher, contas::AUTOMATICA, cx)),
        )
        .children(linhas.into_iter().map(|linha| {
            let escolhida = marcada.as_deref() == Some(linha.id.as_str());
            linha_conta(linha, escolhida, motor.clone(), popover.clone(), window, cx)
        }))
        .child(
            h_flex().pt(r(4.)).child(
                botao_fantasma("uso-entrar-conta", Some(Lucide::Plus.into()), "Entrar com outra conta…", TamanhoBotao::Linha, window, cx)
                    .on_click(move |_, window, cx| {
                        let _ = entrar.update(cx, |p, cx| p.dismiss(window, cx));
                        let _ = janela.update(cx, |this, cx| this.dialogo_entrar_conta(window, cx));
                    }),
            ),
        )
}

/// O símbolo da Automática, do tamanho do anel das contas.
fn circulo_automatica(window: &Window) -> AnyElement {
    div()
        .flex_none()
        .size(r(30.))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .bg(Cores::hover())
        .text_color(Cores::tenue())
        .child(icone(Lucide::Shuffle, 15., window))
        .into_any_element()
}

/// Uma conta: o disco da marca com os anéis de 5 h e da semana (o desenho do trilho), o e-mail
/// inteiro em cima (cortado no meio só se não couber) e o uso (ou quando volta do limite)
/// embaixo, o "…" das extras e o check na escolhida.
fn linha_conta(
    linha: LinhaConta,
    escolhida: bool,
    motor: Entity<Motor>,
    popover: WeakEntity<PopoverState>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    let id = linha.id.clone();
    let escolher = motor.clone();
    let uso = linha.uso;
    let anel = AnelDeUso::new(SharedString::from(format!("uso-conta-anel-{}", linha.id)), Marca::Claude, uso.cinco_horas.map(|f| f as f32))
        .semana(uso.semana.map(|f| f as f32))
        .diametro(30.)
        .bloqueado(uso.bloqueado);
    let meta = if uso.bloqueado {
        div().text_color(Cores::vermelho()).child(limite_atingido(linha.volta))
    } else {
        div().child(resumo_conta(&uso))
    };
    let removivel = linha.id != contas::PRINCIPAL;
    let menu = removivel.then(|| menu_conta(linha.id.clone(), motor, popover, window, cx));
    CartaoDeLista::new(SharedString::from(format!("uso-conta-{}", linha.id)), linha.rotulo)
        .titulo_no_meio()
        .simbolo(Simbolo::Elemento(anel.into_any_element()))
        .subtitulo(meta)
        .acao_fixa(
            h_flex()
                .gap(r(2.))
                .children(menu.map(|m| {
                    // O "…" aparece no hover do cartão; com opacidade, o menu aberto não some
                    // quando o ponteiro sai dele.
                    div().opacity(0.).group_hover(GRUPO_DO_CARTAO, |s| s.opacity(1.)).child(m)
                }))
                .child(marca(escolhida)),
        )
        .on_click(move |_, _, cx| escolher_conta(&escolher, &id, cx))
}

/// O "…" de uma conta extra: desconectar (com a confirmação). Abre para cima, por dentro do
/// popover: um clique fora dele fecharia o popover junto.
fn menu_conta(
    id: String,
    motor: Entity<Motor>,
    popover: WeakEntity<PopoverState>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    botao_icone(SharedString::from(format!("uso-conta-menu-{id}")), Lucide::Ellipsis, "Mais opções da conta", TamanhoBotao::Linha, window, cx)
        .dropdown_menu_with_anchor(Anchor::BottomRight, move |menu, _, _| {
            let (sair, id_sair, popover) = (motor.clone(), id.clone(), popover.clone());
            menu.item(
                PopupMenuItem::new("Desconectar…")
                    .icon(Lucide::Unplug)
                    .danger(true)
                    .on_click(move |_, window, cx| {
                        let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
                        confirmar_desconectar(sair.clone(), id_sair.clone(), window, cx);
                    }),
            )
        })
}

/// O check à direita (o espaço fica reservado nas outras linhas).
fn marca(escolhida: bool) -> impl IntoElement + use<> {
    div().w(r(16.)).flex_none().when(escolhida, |s| {
        s.child(Icon::new(Lucide::Check).size(r(14.)).text_color(Cores::destaque_texto()))
    })
}

/// Qual linha leva a marca para a conta guardada na config ("" é a principal). Uma escolha que
/// não existe mais não marca nenhuma: o motor cai na de menor uso, mas sem trocar no limite.
fn linha_marcada(config: &str, ids: &[&str]) -> Option<String> {
    let escolha = if config.is_empty() { contas::PRINCIPAL } else { config };
    (escolha == contas::AUTOMATICA || ids.contains(&escolha)).then(|| escolha.to_string())
}

/// Inicial do e-mail ou, sem ele, do nome.
fn inicial(conta: &Conta) -> String {
    conta
        .email
        .chars()
        .chain(conta.nome.chars())
        .find(|c| c.is_alphanumeric())
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "?".into())
}

/// Nome do plano lido do uso ("Max 20x") ou, sem ele, o do `auth status`.
fn nome_do_plano(plano: Option<&Plano>, conta: &Conta) -> Option<String> {
    plano
        .and_then(|p| p.nome.clone())
        .or_else(|| (!conta.plano.is_empty()).then(|| maiuscula(&conta.plano)))
}

/// "atualizado há 3 min", o botão de reler agora e, no do Claude, o diálogo das contas.
fn rodape(
    marca: Marca,
    medido_em: Option<i64>,
    agora: i64,
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    popover: WeakEntity<PopoverState>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    h_flex()
        .gap(r(4.))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .metadado()
                .text_color(Cores::apagado())
                .child(atualizado(medido_em, agora)),
        )
        .child(
            botao_fantasma(("uso-atualizar", marca as usize), Some(Lucide::RotateCw.into()), "Atualizar", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.reler_uso(cx))),
        )
        .when(marca == Marca::Claude, |s| {
            s.child(
                botao_fantasma("uso-contas", Some(Lucide::CircleUser.into()), "Contas…", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| {
                        let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
                        let _ = janela.update(cx, |this, cx| this.dialogo_contas(window, cx));
                    }),
            )
        })
}

/// Abre o terminal com a instalação (se faltar) e o login do Codex.
fn botao_instalar(
    rotulo: &'static str,
    popover: WeakEntity<PopoverState>,
    window: &Window,
    cx: &App,
) -> impl IntoElement + use<> {
    botao_primario("uso-instalar-codex", Some(Lucide::Download.into()), rotulo, TamanhoBotao::Controle, window, cx)
        .on_click(move |_, window, cx| {
            let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
            instalar_codex();
        })
}

fn texto(conteudo: &'static str) -> impl IntoElement + use<> {
    div().metadado().text_color(Cores::tenue()).child(conteudo)
}

fn separador() -> impl IntoElement + use<> {
    div().h(px(1.)).bg(Cores::borda())
}

/// Porcentagem inteira mostrada (a cor da faixa segue o mesmo número).
fn porcentagem(fracao: f64) -> u32 {
    (fracao * 100.0).round().max(0.0) as u32
}

/// "renova em 2 h 13 min · 15:59", "renova em 2 dias 4 h · seg. às 03:59" ou "renovou às 12:59".
fn texto_renovacao(renova: Option<i64>, agora: i64) -> Option<String> {
    let renova = renova?;
    let hoje = local(agora).0;
    let instante = local(renova);
    if renova <= agora {
        return Some(format!("renovou {}", quando(instante, hoje)));
    }
    let horario = if instante.0 == hoje {
        format!("{:02}:{:02}", instante.2, instante.3)
    } else {
        quando(instante, hoje)
    };
    Some(format!("renova em {} · {horario}", duracao(renova.saturating_sub(agora))))
}

/// Tempo até renovar, arredondado para cima no minuto: "42 min", "2 h 13 min", "1 dia 15 h".
fn duracao(segundos: i64) -> String {
    let minutos = segundos.max(0).saturating_add(59) / 60;
    match minutos {
        m if m < 60 => format!("{m} min"),
        m if m < 24 * 60 => match m % 60 {
            0 => format!("{} h", m / 60),
            resto => format!("{} h {resto} min", m / 60),
        },
        m => {
            let (dias, horas) = (m / 1440, m % 1440 / 60);
            let dias = if dias == 1 { "1 dia".to_string() } else { format!("{dias} dias") };
            if horas == 0 { dias } else { format!("{dias} {horas} h") }
        }
    }
}

/// Valor em unidades mínimas no jeito brasileiro: 4232 em BRL com 2 casas vira "R$ 42,32".
fn dinheiro(minimo: i64, casas: u32, moeda: &str) -> String {
    let casas = casas.min(6);
    let base = 10_u64.pow(casas);
    // `unsigned_abs`: o `abs` de `i64::MIN` estoura.
    let (inteiro, fracao) = (minimo.unsigned_abs() / base, minimo.unsigned_abs() % base);
    let digitos = inteiro.to_string();
    let mut numero = String::new();
    for (i, c) in digitos.chars().enumerate() {
        if i > 0 && (digitos.len() - i) % 3 == 0 {
            numero.push('.');
        }
        numero.push(c);
    }
    if casas > 0 {
        numero.push_str(&format!(",{fracao:0largura$}", largura = casas as usize));
    }
    let sinal = if minimo < 0 { "-" } else { "" };
    match moeda.to_ascii_uppercase().as_str() {
        "BRL" => format!("{sinal}R$ {numero}"),
        "USD" => format!("{sinal}US$ {numero}"),
        "EUR" => format!("{sinal}€ {numero}"),
        "GBP" => format!("{sinal}£ {numero}"),
        outra => format!("{sinal}{numero} {outra}"),
    }
}

fn maiuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras
        .next()
        .map(|c| c.to_uppercase().chain(letras).collect())
        .unwrap_or_default()
}

/// Há quanto tempo o número foi medido (pelo `fetchedAtMs`, pelo último evento do Claude Code
/// ou pela linha do Codex).
fn atualizado(medido_em: Option<i64>, agora: i64) -> String {
    let Some(medido_em) = medido_em else {
        return "ainda sem leitura do plano".into();
    };
    match agora.saturating_sub(medido_em).max(0) {
        s if s < 60 => "atualizado agora".into(),
        s if s < 3600 => format!("atualizado há {} min", s / 60),
        s if s < 86_400 => format!("atualizado há {} h", s / 3600),
        s if s < 2 * 86_400 => "atualizado há 1 dia".into(),
        s => format!("atualizado há {} dias", s / 86_400),
    }
}

/// Dia (contado no fuso local desde 1970), dia da semana, hora e minuto de um instante Unix.
fn local(segundos: i64) -> (i64, usize, i32, i32) {
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let instante = segundos as libc::time_t;
    unsafe {
        libc::localtime_r(&instante, &mut tm);
    }
    let dia = segundos.saturating_add(tm.tm_gmtoff).div_euclid(86_400);
    (dia, tm.tm_wday as usize % 7, tm.tm_hour, tm.tm_min)
}

/// "às 12:59", "amanhã às 12:59" ou "dom. às 03:59", visto do dia `hoje`.
fn quando((dia, semana, hora, minuto): (i64, usize, i32, i32), hoje: i64) -> String {
    let horario = format!("às {hora:02}:{minuto:02}");
    match dia - hoje {
        0 => horario,
        1 => format!("amanhã {horario}"),
        -1 => format!("ontem {horario}"),
        _ => format!("{} {horario}", DIAS[semana % 7]),
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` traz um `test` próprio que encobriria o da biblioteca.
    use super::{
        atualizado, dinheiro, duracao, inicial, legenda_creditos, leitura_claude, leitura_codex,
        linha_marcada, nome_do_plano, porcentagem, quando, resumo_conta, texto_renovacao,
    };
    use crate::{
        codex::Codex,
        contas::{self, Conta, Creditos, Parada, Plano},
        motor::Uso,
        ui::painel_contas::valor_da_config,
    };

    fn conta(id: &str, nome: &str, email: &str, plano: &str) -> Conta {
        Conta {
            id: id.into(),
            nome: nome.into(),
            email: email.into(),
            plano: plano.into(),
        }
    }

    #[test]
    fn horario_de_renovacao_visto_de_hoje() {
        assert_eq!(quando((100, 6, 12, 59), 100), "às 12:59");
        assert_eq!(quando((101, 0, 3, 5), 100), "amanhã às 03:05");
        assert_eq!(quando((102, 1, 3, 59), 100), "seg. às 03:59");
        assert_eq!(quando((99, 5, 23, 0), 100), "ontem às 23:00");
        assert_eq!(duracao(42 * 60), "42 min");
        assert_eq!(duracao(2 * 3600 + 13 * 60 - 30), "2 h 13 min");
        assert_eq!(duracao(3 * 3600), "3 h");
        assert_eq!(duracao(39 * 3600), "1 dia 15 h");
        assert_eq!(duracao(2 * 86_400), "2 dias");
        assert_eq!(texto_renovacao(None, 0), None);
        let texto = texto_renovacao(Some(1_000_000 + 2 * 3600 + 13 * 60), 1_000_000).unwrap();
        assert!(texto.starts_with("renova em 2 h 13 min · "), "{texto}");
        assert!(texto_renovacao(Some(50), 100).unwrap().starts_with("renovou "));
    }

    #[test]
    fn textos_do_botao_e_da_lista() {
        let uso = Uso {
            cinco_horas: Some(0.07),
            semana: Some(0.38),
            ..Uso::default()
        };
        assert_eq!(porcentagem(0.07), 7);
        assert_eq!(leitura_claude(&uso, None, false, None).dica, "Claude · 5 h 7% · semana 38%");
        assert_eq!(resumo_conta(&uso), "5 h 7% · semana 38%");
        // A janela sem leitura fica de fora, como no menu de conta.
        assert_eq!(resumo_conta(&Uso { semana: Some(0.73), ..Uso::default() }), "semana 73%");
        assert_eq!(resumo_conta(&Uso::default()), "uso ainda não lido");
        // O Codex sem instalar (ou ainda sendo lido) fica apagado, mas aparece.
        let ausente = leitura_codex(None, true);
        assert_eq!(ausente.dica, "Codex · não instalado");
        assert!(ausente.apagado);
        assert!(leitura_codex(None, false).apagado);
        assert_eq!(leitura_codex(None, false).dica, "Codex · lendo o uso…");
        let vazio = Codex::default();
        assert_eq!(leitura_codex(Some(&vazio), true).dica, "Codex · sem leitura do uso");
        assert!(!leitura_codex(Some(&vazio), true).apagado);
        let so_semana = Codex {
            uso: Uso { semana: Some(0.75), ..Uso::default() },
            ..Codex::default()
        };
        assert_eq!(leitura_codex(Some(&so_semana), true).dica, "Codex · semana 75%");
        // Só janelas diária e mensal: a maior vai no anel de fora e na dica.
        let outras = Codex {
            outras: vec![("Mês".into(), 0.3, None), ("Dia".into(), 0.8, None)],
            ..Codex::default()
        };
        let leitura = leitura_codex(Some(&outras), true);
        assert_eq!((leitura.fora, leitura.dentro), (Some(0.8), None));
        assert_eq!(leitura.dica, "Codex · dia 80%");
        let sem_limites = Codex { sem_limites: true, ..Codex::default() };
        assert_eq!(leitura_codex(Some(&sem_limites), true).dica, "Codex · sem limites do plano");
        assert_eq!(atualizado(None, 100), "ainda sem leitura do plano");
        assert_eq!(atualizado(Some(1000), 1030), "atualizado agora");
        assert_eq!(atualizado(Some(1000), 1000 + 3 * 60 + 10), "atualizado há 3 min");
        assert_eq!(atualizado(Some(0), 2 * 3600), "atualizado há 2 h");
    }

    #[test]
    fn no_limite_o_anel_fica_cheio_e_a_dica_diz_quando_volta() {
        let uso = Uso {
            cinco_horas: Some(1.0),
            bloqueado: true,
            ..Uso::default()
        };
        let leitura = leitura_claude(&uso, None, false, None);
        assert!(leitura.bloqueado);
        assert_eq!(leitura.dica, "Claude · limite atingido");
        let volta = leitura_claude(&uso, None, false, Some(crate::modelo::agora()));
        assert!(volta.dica.starts_with("Claude · limite atingido, volta às "), "{}", volta.dica);
    }

    #[test]
    fn linha_marcada_para_cada_config() {
        let ids = [contas::PRINCIPAL, "extra1", "extra2"];
        // Vazio é a principal; "auto" marca a automática; um id marca a própria conta.
        assert_eq!(linha_marcada("", &ids).as_deref(), Some(contas::PRINCIPAL));
        assert_eq!(linha_marcada(contas::PRINCIPAL, &ids).as_deref(), Some(contas::PRINCIPAL));
        assert_eq!(linha_marcada(contas::AUTOMATICA, &ids).as_deref(), Some(contas::AUTOMATICA));
        assert_eq!(linha_marcada("extra2", &ids).as_deref(), Some("extra2"));
        // Conta que saiu do cadastro: nenhuma linha marcada.
        assert_eq!(linha_marcada("sumiu", &ids), None);
        // Escolher uma linha grava um valor que volta a marcar a mesma linha.
        for id in ids.iter().copied().chain([contas::AUTOMATICA]) {
            assert_eq!(linha_marcada(&valor_da_config(id), &ids).as_deref(), Some(id));
        }
    }

    #[test]
    fn selo_e_dica_da_conta_em_uso() {
        let uso = Uso {
            cinco_horas: Some(0.07),
            semana: Some(0.38),
            ..Uso::default()
        };
        let principal = conta(contas::PRINCIPAL, "Principal", "", "");
        let extra = conta("x1", "victor", "victor@exemplo.com", "max");
        // Uma conta só: nem selo nem conta na dica.
        assert_eq!(leitura_claude(&uso, None, false, None).selo, None);
        let leitura = leitura_claude(&uso, Some(&principal), false, None);
        assert_eq!(leitura.dica, "Claude · conta Principal · 5 h 7% · semana 38%");
        assert_eq!(leitura.selo.as_deref(), Some("P"));
        let leitura = leitura_claude(&uso, Some(&extra), true, None);
        assert_eq!(leitura.dica, "Claude · conta victor (automática) · 5 h 7% · semana 38%");
        assert_eq!(leitura.selo.as_deref(), Some("V"));
        assert_eq!(inicial(&conta("x2", "", "", "")), "?");
        assert_eq!(inicial(&conta("x3", "Conta 3", "_ana@exemplo.com", "")), "A");
        // O plano lido do uso vale mais que o do `auth status`.
        let lido = Plano { nome: Some("Max 20x".into()), ..Plano::default() };
        assert_eq!(nome_do_plano(Some(&lido), &extra).as_deref(), Some("Max 20x"));
        assert_eq!(nome_do_plano(None, &extra).as_deref(), Some("Max"));
        assert_eq!(nome_do_plano(Some(&Plano::default()), &principal), None);
    }

    #[test]
    fn dinheiro_com_a_moeda_certa() {
        assert_eq!(dinheiro(4232, 2, "BRL"), "R$ 42,32");
        assert_eq!(dinheiro(123_456_789, 2, "BRL"), "R$ 1.234.567,89");
        assert_eq!(dinheiro(5, 2, "usd"), "US$ 0,05");
        assert_eq!(dinheiro(1000, 0, "JPY"), "1.000 JPY");
        assert_eq!(dinheiro(-150, 2, "EUR"), "-€ 1,50");
        // Valor absurdo não derruba o desenho.
        assert_eq!(dinheiro(i64::MIN, 2, "BRL"), "-R$ 92.233.720.368.547.758,08");
        assert_eq!(atualizado(Some(i64::MIN), 100), "atualizado há 106751991167300 dias");
        assert_eq!(duracao(i64::MAX), "106751991167300 dias 15 h");
    }

    #[test]
    fn creditos_no_limite_e_sem_creditos() {
        let creditos = |limite: Option<i64>, parada: Option<Parada>| Creditos {
            usados: 10_000,
            limite,
            moeda: "BRL".into(),
            casas: 2,
            parada,
        };
        let no_limite = legenda_creditos(&creditos(Some(10_000), Some(Parada::LimiteDoMes)));
        assert_eq!(no_limite, (Some(1.0), "limite do mês de R$ 100,00 atingido".into(), true));
        let sem_limite = legenda_creditos(&creditos(None, Some(Parada::LimiteDoMes)));
        assert_eq!(sem_limite, (Some(1.0), "limite do mês atingido".into(), true));
        let acabou = legenda_creditos(&creditos(None, Some(Parada::SemCreditos)));
        assert_eq!(acabou, (Some(1.0), "sem créditos".into(), true));
        let desligados = legenda_creditos(&creditos(Some(20_000), Some(Parada::Desligados)));
        assert_eq!(desligados, (None, "usados no mês · desligados agora".into(), false));
        let metade = legenda_creditos(&creditos(Some(20_000), None));
        assert_eq!(metade, (Some(0.5), "50% do limite do mês de R$ 200,00".into(), false));
        let livre = legenda_creditos(&creditos(None, None));
        assert_eq!(livre, (None, "usados no mês · sem limite".into(), false));
    }

    #[test]
    fn junta_ficando_com_a_medicao_mais_recente() {
        let guardado = Uso {
            cinco_horas: Some(0.07),
            semana: Some(0.38),
            renova_5h: Some(5000),
            renova_semana: Some(9000),
            medido_em: Some(1000),
            ..Uso::default()
        };
        let evento = Uso {
            cinco_horas: Some(0.10),
            renova_5h: Some(5000),
            medido_em: Some(2000),
            bloqueado: true,
            ..Uso::default()
        };
        // O evento é mais novo: a janela de 5 h vem dele e a semana fica a do arquivo.
        let junto = guardado.juntar(evento, 2000);
        assert_eq!(junto.cinco_horas, Some(0.10));
        assert_eq!(junto.semana, Some(0.38));
        assert_eq!(junto.renova_semana, Some(9000));
        assert_eq!(junto.medido_em, Some(2000));
        assert!(!junto.bloqueado);
        // Uma leitura mais velha do arquivo não desfaz o que veio depois.
        assert_eq!(junto.juntar(guardado, 2000), junto);
        // Passou a hora de renovar: a janela de 5 h volta a zero.
        let renovado = junto.juntar(Uso::default(), 6000);
        assert_eq!(renovado.cinco_horas, Some(0.0));
        assert_eq!(renovado.semana, Some(0.38));
    }
}
