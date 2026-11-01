//! Estado e sinais: pontos de sinal, pílula de estado, selos e o número do painel, e as funções
//! que leem o motor para dar um desenho só a cada situação (`sinal_do_chat`, `sinal_do_projeto`,
//! `estado_visual`, `pilula_da_tarefa`).

use std::{rc::Rc, time::Duration};

use gpui_kit::component::{Icon, Sizable as _, spinner::compasso_recente, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use crate::{
    modelo::{Chat, EstadoTarefa, Execucao, Papel, Tarefa},
    motor::Motor,
    tema::{Cores, Tipo as _, esp, r, raio, tam},
};

/// Sinal de atividade, na ordem de prioridade (o primeiro que valer ganha).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sinal {
    Erro,
    Pedido,
    /// Parada que precisa de Retomar (a parada por você não tem sinal).
    Parada,
    Nova,
    Rodando,
}

impl Sinal {
    pub fn cor(self) -> Hsla {
        match self {
            Sinal::Erro => Cores::vermelho(),
            Sinal::Pedido | Sinal::Parada => Cores::ambar(),
            Sinal::Nova | Sinal::Rodando => Cores::destaque(),
        }
    }

    /// Rodando é aro sem miolo; os outros são ponto cheio.
    pub fn vazado(self) -> bool {
        self == Sinal::Rodando
    }

    /// Ponto de `lado` px (6 dentro do texto, 8 solto ou no canto), sem aro. O de rodando
    /// respira (o aro vai de 0,55 a 1 em 1,6 s) só enquanto o compasso dos spinners já bate.
    pub fn ponto(self, lado: f32) -> PontoDeSinal {
        PontoDeSinal { sinal: self, lado }
    }
}

/// O ponto de um `Sinal`.
#[derive(IntoElement)]
pub struct PontoDeSinal {
    sinal: Sinal,
    lado: f32,
}

/// Uma respiração do ponto de rodando.
const RESPIRO: Duration = Duration::from_millis(1600);

/// A opacidade do aro de rodando na fase `fase` do compasso: 1 no começo de cada respiro, 0,55
/// no meio.
fn opacidade_do_respiro(fase: Duration) -> f32 {
    let t = (fase.as_nanos() % RESPIRO.as_nanos()) as f32 / RESPIRO.as_nanos() as f32;
    0.775 + 0.225 * (t * std::f32::consts::TAU).cos()
}

impl RenderOnce for PontoDeSinal {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let ponto = div().flex_none().size(r(self.lado)).rounded_full();
        if self.sinal.vazado() {
            let alfa = compasso_recente(cx).map_or(1., opacidade_do_respiro);
            ponto.border_2().border_color(self.sinal.cor().opacity(alfa))
        } else {
            ponto.bg(self.sinal.cor())
        }
    }
}

/// Estado de uma tarefa ou chat, como a pílula mostra.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Estado {
    Rodando,
    Verificando,
    EsperandoVoce,
    NaFila,
    EmRevisao,
    Pronto,
    Erro,
    /// Parada por você: neutra.
    Parado,
    /// Parada que precisa de Retomar: âmbar.
    ParadoAtencao,
}

impl Estado {
    pub const TODOS: [Estado; 9] = [
        Estado::Rodando,
        Estado::Verificando,
        Estado::EsperandoVoce,
        Estado::NaFila,
        Estado::EmRevisao,
        Estado::Pronto,
        Estado::Erro,
        Estado::Parado,
        Estado::ParadoAtencao,
    ];

    /// Cor do ponto e da tinta; `None` é neutro.
    pub fn cor(self) -> Option<Hsla> {
        match self {
            Estado::Rodando | Estado::Verificando | Estado::EmRevisao => Some(Cores::destaque()),
            Estado::EsperandoVoce | Estado::ParadoAtencao => Some(Cores::ambar()),
            Estado::Pronto => Some(Cores::verde()),
            Estado::Erro => Some(Cores::vermelho()),
            Estado::NaFila | Estado::Parado => None,
        }
    }

    /// Cor do texto (o destaque puro não passa de 4,5:1 em texto pequeno).
    pub fn cor_texto(self) -> Hsla {
        match self {
            Estado::Rodando | Estado::Verificando | Estado::EmRevisao => Cores::destaque_texto(),
            _ => self.cor().unwrap_or_else(Cores::tenue),
        }
    }

    /// Texto padrão, sempre minúsculo (é adjetivo).
    pub fn rotulo(self) -> &'static str {
        match self {
            Estado::Rodando => "rodando",
            Estado::Verificando => "verificando",
            Estado::EsperandoVoce => "esperando você",
            Estado::NaFila => "na fila",
            Estado::EmRevisao => "em revisão",
            Estado::Pronto => "pronto",
            Estado::Erro => "falhou",
            Estado::Parado | Estado::ParadoAtencao => "parada",
        }
    }
}

/// "● rodando · 2 min 13 s": pílula de 22 na tinta da cor, sem borda.
#[derive(IntoElement)]
pub struct PilulaDeEstado {
    estado: Estado,
    rotulo: Option<SharedString>,
    detalhe: Option<SharedString>,
    compacta: bool,
}

impl PilulaDeEstado {
    pub fn new(estado: Estado) -> Self {
        PilulaDeEstado { estado, rotulo: None, detalhe: None, compacta: false }
    }

    /// Troca o texto padrão ("aprovada", "cancelada").
    pub fn rotulo(mut self, r: impl Into<SharedString>) -> Self {
        self.rotulo = Some(r.into());
        self
    }

    /// Depois de " · ", em tênue: "2 min 13 s", "rodada 2".
    pub fn detalhe(mut self, d: impl Into<SharedString>) -> Self {
        self.detalhe = Some(d.into());
        self
    }

    /// Só o ponto, numa área de 14 onde a dica pega; o texto vai para a dica.
    pub fn compacta(mut self) -> Self {
        self.compacta = true;
        self
    }
}

impl RenderOnce for PilulaDeEstado {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let cor = self.estado.cor();
        let rotulo = self.rotulo.unwrap_or_else(|| self.estado.rotulo().into());
        let ponto = div()
            .flex_none()
            .size(r(6.))
            .rounded_full()
            .bg(cor.unwrap_or_else(Cores::tenue));
        if self.compacta {
            let dica: SharedString = match &self.detalhe {
                Some(d) => format!("{rotulo} · {d}").into(),
                None => rotulo.clone(),
            };
            return div()
                .id(SharedString::from(format!("pilula-{:?}-{rotulo}", self.estado)))
                .flex_none()
                .size(r(14.))
                .flex()
                .items_center()
                .justify_center()
                .child(ponto)
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                .into_any_element();
        }
        div()
            .flex_none()
            .h(r(22.))
            .px(r(8.))
            .flex()
            .items_center()
            .gap(r(6.))
            .rounded_full()
            .bg(cor.map(Cores::tinta).unwrap_or_else(Cores::hover))
            .child(ponto)
            .child(
                div()
                    .metadado()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(self.estado.cor_texto())
                    .whitespace_nowrap()
                    .child(rotulo)
                    .when_some(self.detalhe, |s, d| {
                        s.child(div().text_color(Cores::tenue()).child(format!(" · {d}")))
                            .flex()
                    }),
            )
            .into_any_element()
    }
}

#[derive(Clone)]
enum TipoSelo {
    Neutro,
    Cor(Hsla),
    Novo,
    Contagem(usize),
    Capacidades(Vec<(Icon, Hsla, &'static str)>),
    Ponto(Hsla),
}

/// Selo pequeno: rótulo, contagem, ponto ou capacidades de um modelo.
#[derive(IntoElement, Clone)]
pub struct Selo {
    tipo: TipoSelo,
    rotulo: Option<SharedString>,
    icone: Option<(Icon, Hsla)>,
    tingida: Option<Hsla>,
    solida: Option<Hsla>,
    vazado: bool,
    lado: f32,
    clicavel: Option<(ElementId, Rc<dyn Fn(&mut Window, &mut App)>)>,
}

impl Selo {
    fn de(tipo: TipoSelo, rotulo: Option<SharedString>) -> Self {
        Selo {
            tipo,
            rotulo,
            icone: None,
            tingida: None,
            solida: None,
            vazado: false,
            lado: 8.,
            clicavel: None,
        }
    }

    /// "1M de contexto", "padrão": contorno, 11/600 tênue.
    pub fn neutro(r: impl Into<SharedString>) -> Self {
        Self::de(TipoSelo::Neutro, Some(r.into()))
    }

    /// "rápido", "beta": tinta da cor, rótulo na cor.
    pub fn cor(r: impl Into<SharedString>, cor: Hsla) -> Self {
        Self::de(TipoSelo::Cor(cor), Some(r.into()))
    }

    pub fn novo() -> Self {
        Self::de(TipoSelo::Novo, Some("novo".into()))
    }

    /// Número neutro; `tingida` põe na tinta da cor e `solida`, sobre a cor cheia.
    pub fn contagem(n: usize) -> Self {
        Self::de(TipoSelo::Contagem(n), None)
    }

    pub fn tingida(mut self, cor: Hsla) -> Self {
        self.tingida = Some(cor);
        self
    }

    /// Só quando fica por cima de algo (sino, avatar).
    pub fn solida(mut self, cor: Hsla) -> Self {
        self.solida = Some(cor);
        self
    }

    pub fn ponto(cor: Hsla) -> Self {
        Self::de(TipoSelo::Ponto(cor), None)
    }

    /// Ponto só com o aro (rodando).
    pub fn vazado(mut self) -> Self {
        self.vazado = true;
        self
    }

    /// Lado do ponto (8 por padrão).
    pub fn lado(mut self, px: f32) -> Self {
        self.lado = px;
        self
    }

    /// Ícone antes do rótulo.
    pub fn icone(mut self, icone: impl Into<Icon>, cor: Hsla) -> Self {
        self.icone = Some((icone.into(), cor));
        self
    }

    /// Ícones de 14 separados por divisória, cada um com a sua dica.
    pub fn capacidades(itens: Vec<(Icon, Hsla, &'static str)>) -> Self {
        Self::de(TipoSelo::Capacidades(itens), None)
    }

    pub fn clicavel(mut self, id: impl Into<ElementId>, f: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.clicavel = Some((id.into(), Rc::new(f)));
        self
    }
}

impl RenderOnce for Selo {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        let pilula = || div().flex_none().flex().items_center().rounded_full().micro().whitespace_nowrap();
        let corpo = match self.tipo {
            TipoSelo::Ponto(cor) => {
                let ponto = div().flex_none().size(r(self.lado)).rounded_full();
                if self.vazado { ponto.border_2().border_color(cor) } else { ponto.bg(cor) }.into_any_element()
            }
            TipoSelo::Contagem(n) => {
                let texto = if n > 99 { "99+".to_string() } else { n.to_string() };
                let (fundo, cor) = match (self.solida, self.tingida) {
                    (Some(c), _) => (c, Cores::sobre_cor()),
                    (None, Some(c)) => (Cores::tinta(c), c),
                    (None, None) => (Cores::borda(), Cores::tenue()),
                };
                pilula()
                    .min_w(r(18.))
                    .h(r(18.))
                    .px(esp::E4)
                    .justify_center()
                    .bg(fundo)
                    .text_color(cor)
                    .child(texto)
                    .into_any_element()
            }
            TipoSelo::Capacidades(itens) => {
                let n = itens.len();
                let id: Vec<&str> = itens.iter().map(|(_, _, dica)| *dica).collect();
                pilula()
                    .id(SharedString::from(format!("capacidades-{}", id.join("-"))))
                    .h(r(22.))
                    .px(r(4.))
                    .border_1()
                    .border_color(Cores::borda())
                    .children(itens.into_iter().enumerate().map(|(i, (icone, cor, dica))| {
                        div()
                            .id(("capacidade", i))
                            .flex()
                            .items_center()
                            .px(r(4.))
                            .h_full()
                            .when(i + 1 < n, |s| s.border_r_1().border_color(Cores::borda()))
                            .child(icone.with_size(tam(window, 14.)).text_color(cor))
                            .tooltip(move |window, cx| Tooltip::new(dica).build(window, cx))
                    }))
                    .into_any_element()
            }
            tipo => {
                let (fundo, borda, cor) = match tipo {
                    TipoSelo::Cor(c) => (Cores::tinta(c), None, c),
                    TipoSelo::Novo => (Cores::destaque().opacity(0.18), None, Cores::destaque_texto()),
                    _ => (gpui_kit::transparent_black(), Some(Cores::borda()), Cores::tenue()),
                };
                pilula()
                    .h(r(20.))
                    .px(r(6.))
                    .gap(r(4.))
                    .bg(fundo)
                    .when_some(borda, |s, b| s.border_1().border_color(b))
                    .text_color(cor)
                    .when_some(self.icone, |s, (icone, cor)| {
                        s.child(icone.with_size(tam(window, 12.)).text_color(cor))
                    })
                    .children(self.rotulo)
                    .into_any_element()
            }
        };
        match self.clicavel {
            Some((id, f)) => div()
                .id(id)
                .cursor_pointer()
                .child(corpo)
                .on_click(move |_, window, cx| f(window, cx))
                .into_any_element(),
            None => corpo,
        }
    }
}

/// Gravidade de um problema do revisor, em minúsculas.
pub fn selo_gravidade(gravidade: &str) -> Selo {
    match gravidade.trim().to_lowercase().as_str() {
        "alta" => Selo::cor("alta", Cores::vermelho()),
        "média" | "media" => Selo::cor("média", Cores::ambar()),
        outra => Selo::neutro(if outra.is_empty() { "baixa".to_string() } else { outra.to_string() }),
    }
}

/// Número do painel (Alt+1…4): destaque cheio no ativo, recuo com borda nos outros.
pub fn numero_painel(n: usize, ativo: bool) -> impl IntoElement {
    div()
        .flex_none()
        .size(r(18.))
        .flex()
        .items_center()
        .justify_center()
        .rounded(raio::PILULA)
        .micro()
        .map(|s| {
            if ativo {
                s.bg(Cores::destaque()).text_color(Cores::sobre_cor())
            } else {
                s.bg(Cores::recuo()).border_1().border_color(Cores::borda()).text_color(Cores::tenue())
            }
        })
        .child(n.to_string())
}

/// O que um chat tem para o sinal, já filtrado (arquivado não espera nada).
#[derive(Clone, Copy, Default, Debug)]
struct FatosDoSinal {
    erro: bool,
    pedido: bool,
    /// Parou e precisa de Retomar (a parada por você não conta).
    parada: bool,
    nova: bool,
    /// Processo ativo ou na fila esperando vaga.
    rodando: bool,
}

fn fatos_do_sinal(motor: &Motor, c: &Chat) -> FatosDoSinal {
    let execucao = motor.execucao(&c.id);
    let espera = !motor.arquivada(c);
    FatosDoSinal {
        erro: espera && matches!(execucao, Execucao::Erro(_)),
        pedido: espera && execucao == Execucao::EsperandoVoce,
        parada: espera && motor.precisa_retomar(&c.id),
        nova: espera && c.papel.na_lista() && motor.nao_lido(c),
        rodando: matches!(execucao, Execucao::Rodando { .. }) || motor.esperando_vaga(&c.id),
    }
}

/// O sinal mais forte de um grupo de chats: erro, pedido, parada, resposta nova e rodando.
fn sinal_de(fatos: &[FatosDoSinal]) -> Option<Sinal> {
    let algum = |f: fn(&FatosDoSinal) -> bool| fatos.iter().any(f);
    if algum(|f| f.erro) {
        Some(Sinal::Erro)
    } else if algum(|f| f.pedido) {
        Some(Sinal::Pedido)
    } else if algum(|f| f.parada) {
        Some(Sinal::Parada)
    } else if algum(|f| f.nova) {
        Some(Sinal::Nova)
    } else if algum(|f| f.rodando) {
        Some(Sinal::Rodando)
    } else {
        None
    }
}

/// O sinal de um chat; no coordenador vale a família toda (os trabalhadores não arquivados).
pub fn sinal_do_chat(motor: &Motor, chat: &Chat) -> Option<Sinal> {
    let fatos: Vec<FatosDoSinal> = match chat.papel {
        Papel::Coordenador | Papel::Mestre | Papel::Projeto => {
            motor.familia(chat).into_iter().map(|c| fatos_do_sinal(motor, c)).collect()
        }
        Papel::Trabalhador => vec![fatos_do_sinal(motor, chat)],
    };
    sinal_de(&fatos)
}

/// O sinal de um projeto: o mais forte entre as conversas dele que não estão arquivadas.
pub fn sinal_do_projeto(motor: &Motor, projeto: &str) -> Option<Sinal> {
    let fatos: Vec<FatosDoSinal> = motor
        .chats
        .iter()
        .filter(|c| c.projeto == projeto && !c.arquivado && c.papel != Papel::Mestre)
        .map(|c| fatos_do_sinal(motor, c))
        .collect();
    sinal_de(&fatos)
}

/// Como uma tarefa aparece em todo lugar (pílula, legenda, chip do chat): a mesma palavra e a
/// mesma cor, com o detalhe curto e a explicação inteira na dica.
#[derive(Clone, Debug, PartialEq)]
pub struct EstadoVisual {
    pub estado: Estado,
    pub rotulo: SharedString,
    pub detalhe: Option<SharedString>,
    pub dica: SharedString,
}

/// O que a tabela de estados precisa do motor, além da tarefa.
#[derive(Clone, Debug, Default)]
struct FatosDaTarefa {
    execucao: Option<Execucao>,
    /// `Motor::tarefa_parada`: parou no meio e nada a faz andar até alguém mandar retomar.
    parada: bool,
    /// A parada não foi você: pede Retomar (âmbar).
    precisa_retomar: bool,
    esperando_vaga: bool,
    revisor_ligado: bool,
    /// Há quanto tempo o turno do trabalhador roda ("2 min 13 s").
    decorrido: Option<String>,
}

/// O motivo da parada em poucas palavras ("Vitral fechou", "verificação parou"), o mesmo na
/// pílula, na prévia da lista e no sino. Numa tarefa em revisão, quem parou foi a revisão.
pub fn motivo_curto(motivo: &str, em_revisao: bool) -> String {
    if motivo.starts_with("você") {
        "você interrompeu".into()
    } else if motivo.contains("verificação") {
        "verificação parou".into()
    } else if motivo.contains("preparação") {
        "preparação parou".into()
    } else if em_revisao {
        "revisão parou".into()
    } else if motivo.contains("Vitral") {
        "Vitral fechou".into()
    } else {
        motivo.to_string()
    }
}

fn estado_visual_de(t: &Tarefa, f: &FatosDaTarefa) -> EstadoVisual {
    let ativa = t.estado.ativa();
    let progresso = t.progresso.as_deref().unwrap_or_default();
    let visual = |estado: Estado, rotulo: &str, detalhe: Option<String>, dica: String| EstadoVisual {
        estado,
        rotulo: rotulo.to_string().into(),
        detalhe: detalhe.map(Into::into),
        dica: dica.into(),
    };
    match &f.execucao {
        Some(Execucao::EsperandoVoce) if ativa => {
            return visual(
                Estado::EsperandoVoce,
                "esperando você",
                Some("permissão".into()),
                "O chat espera a sua permissão para continuar.".into(),
            );
        }
        Some(Execucao::Erro(erro)) if ativa => {
            let linha = erro.lines().next().unwrap_or_default().to_string();
            return visual(Estado::Erro, "erro", Some("o processo caiu".into()), format!("O processo caiu: {linha}"));
        }
        _ => {}
    }
    if f.parada {
        let motivo = t.parada.clone().unwrap_or_default();
        let estado = if f.precisa_retomar { Estado::ParadoAtencao } else { Estado::Parado };
        let dica = if f.precisa_retomar {
            format!("Parada: {motivo}. Use Retomar para continuar.")
        } else {
            format!("Parada: {motivo}.")
        };
        return visual(estado, "parada", Some(motivo_curto(&motivo, t.estado == EstadoTarefa::EmRevisao)), dica);
    }
    match t.estado {
        EstadoTarefa::Rodando | EstadoTarefa::Ajustando if f.esperando_vaga => visual(
            Estado::NaFila,
            "na fila",
            Some("esperando vaga".into()),
            "Começa quando outro chat terminar.".into(),
        ),
        EstadoTarefa::Rodando | EstadoTarefa::Ajustando if !t.preparada || progresso.starts_with("preparando") => {
            visual(Estado::Rodando, "preparando", Some("a cópia".into()), "Preparando a cópia isolada do chat.".into())
        }
        EstadoTarefa::Rodando | EstadoTarefa::Ajustando if progresso.starts_with("esperando") => visual(
            Estado::NaFila,
            "esperando resposta",
            None,
            "O chat perguntou algo e espera a resposta de quem pediu.".into(),
        ),
        EstadoTarefa::Rodando => {
            visual(Estado::Rodando, "rodando", f.decorrido.clone(), "O chat está trabalhando no pedido.".into())
        }
        EstadoTarefa::Ajustando => {
            let detalhe = match &f.decorrido {
                Some(d) => format!("rodada {} · {d}", t.rodada),
                None => format!("rodada {}", t.rodada),
            };
            visual(Estado::Rodando, "ajustando", Some(detalhe), "O chat ajusta o que foi pedido.".into())
        }
        EstadoTarefa::EmRevisao if t.conferindo() => {
            let detalhe = match (t.verificacao.is_some(), f.revisor_ligado) {
                (true, true) => "testes e revisor",
                (true, false) => "testes",
                (false, true) => "revisor",
                (false, false) => "a entrega",
            };
            visual(
                Estado::Verificando,
                "verificando",
                Some(detalhe.into()),
                "A verificação e o revisor conferem a entrega antes de quem pediu decidir.".into(),
            )
        }
        EstadoTarefa::EmRevisao => {
            let (com_quem, quem) = match t.dono() {
                None => ("com você", "Você decide"),
                Some(crate::modelo::ID_MESTRE) => ("com o Mestre", "O Mestre decide"),
                Some(_) => ("com o chat dele", "O chat que pediu decide"),
            };
            visual(Estado::EmRevisao, "em revisão", Some(com_quem.into()), format!("{quem}: juntar ou pedir ajuste."))
        }
        EstadoTarefa::Aprovada => visual(
            Estado::Pronto,
            "aprovada",
            t.branch.is_some().then(|| "juntada".into()),
            "Aprovada e juntada ao projeto.".into(),
        ),
        EstadoTarefa::Falhou => visual(Estado::Erro, "falhou", None, "O pedido falhou.".into()),
        EstadoTarefa::Cancelada => visual(Estado::Parado, "cancelada", None, "O pedido foi cancelado.".into()),
        EstadoTarefa::Conversa => visual(Estado::Parado, "conversa", None, "Pronto para conversar.".into()),
        // Provisório (G5): a pílula do PR com os checks.
        EstadoTarefa::NoPr => visual(
            Estado::Parado,
            "PR aberto",
            t.pr.as_ref().map(|pr| format!("#{}", pr.numero)),
            "O PR está aberto no GitHub; o Vitral acompanha os checks e a revisão.".into(),
        ),
    }
}

/// O estado de uma tarefa como aparece em toda a interface (tabela 3.4 da Equipe).
pub fn estado_visual(motor: &Motor, t: &Tarefa) -> EstadoVisual {
    let execucao = motor.execucao(&t.trabalhador);
    let decorrido = match execucao {
        Execucao::Rodando { desde } => Some(crate::ui::duracao(desde)),
        _ => None,
    };
    let fatos = FatosDaTarefa {
        execucao: Some(execucao),
        parada: motor.tarefa_parada(t),
        precisa_retomar: motor.precisa_retomar(&t.trabalhador),
        esperando_vaga: motor.esperando_vaga(&t.trabalhador),
        revisor_ligado: motor.config.revisor,
        decorrido,
    };
    estado_visual_de(t, &fatos)
}

/// A pílula de uma tarefa: na lista, no chat, na Equipe e no Mestre.
pub fn pilula_da_tarefa(motor: &Motor, t: &Tarefa) -> PilulaDeEstado {
    let visual = estado_visual(motor, t);
    let pilula = PilulaDeEstado::new(visual.estado).rotulo(visual.rotulo);
    match visual.detalhe {
        Some(d) => pilula.detalhe(d),
        None => pilula,
    }
}

#[cfg(test)]
mod testes {
    use super::{Estado, EstadoVisual, FatosDaTarefa, FatosDoSinal, Sinal, estado_visual_de, motivo_curto, sinal_de};
    use crate::modelo::{Execucao, Tarefa};

    fn tarefa(estado: &str) -> Tarefa {
        let json = format!(
            r#"{{"id": 2, "projeto": "loja", "coordenador": "c", "trabalhador": "t", "titulo": "Preços",
                "objetivo": "o", "criterios": [], "verificacao": "cargo test", "estado": "{estado}",
                "rodada": 1, "branch": "vitral/tarefa-2", "criada_em": 0}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn ver(t: &Tarefa, f: FatosDaTarefa) -> (Estado, String, Option<String>) {
        let EstadoVisual { estado, rotulo, detalhe, .. } = estado_visual_de(t, &f);
        (estado, rotulo.to_string(), detalhe.map(|d| d.to_string()))
    }

    fn com(rotulo: &str, detalhe: Option<&str>) -> (String, Option<String>) {
        (rotulo.into(), detalhe.map(Into::into))
    }

    #[test]
    fn estado_visual_segue_a_tabela_da_equipe() {
        let rodando = tarefa("Rodando");
        let caso = |t: &Tarefa, f: FatosDaTarefa| {
            let (estado, rotulo, detalhe) = ver(t, f);
            (estado, (rotulo, detalhe))
        };
        let pedido = FatosDaTarefa { execucao: Some(Execucao::EsperandoVoce), ..Default::default() };
        assert_eq!(caso(&rodando, pedido), (Estado::EsperandoVoce, com("esperando você", Some("permissão"))));
        let erro = FatosDaTarefa { execucao: Some(Execucao::Erro("529\nresto".into())), ..Default::default() };
        assert_eq!(caso(&rodando, erro), (Estado::Erro, com("erro", Some("o processo caiu"))));
        let fila = FatosDaTarefa { esperando_vaga: true, ..Default::default() };
        assert_eq!(caso(&rodando, fila), (Estado::NaFila, com("na fila", Some("esperando vaga"))));
        let correndo = FatosDaTarefa { decorrido: Some("2 min 13 s".into()), ..Default::default() };
        assert_eq!(caso(&rodando, correndo.clone()), (Estado::Rodando, com("rodando", Some("2 min 13 s"))));

        let mut preparando = tarefa("Rodando");
        preparando.preparada = false;
        assert_eq!(caso(&preparando, Default::default()), (Estado::Rodando, com("preparando", Some("a cópia"))));
        preparando.preparada = true;
        preparando.progresso = Some("esperando resposta do coordenador".into());
        assert_eq!(caso(&preparando, Default::default()), (Estado::NaFila, com("esperando resposta", None)));

        let mut ajustando = tarefa("Ajustando");
        ajustando.rodada = 2;
        let tres = FatosDaTarefa { decorrido: Some("3 min 00 s".into()), ..Default::default() };
        assert_eq!(caso(&ajustando, tres), (Estado::Rodando, com("ajustando", Some("rodada 2 · 3 min 00 s"))));

        let mut conferindo = tarefa("EmRevisao");
        conferindo.progresso = Some("verificando e revisando a entrega".into());
        let revisor = FatosDaTarefa { revisor_ligado: true, ..Default::default() };
        assert_eq!(caso(&conferindo, revisor.clone()), (Estado::Verificando, com("verificando", Some("testes e revisor"))));
        conferindo.verificacao = None;
        assert_eq!(caso(&conferindo, revisor), (Estado::Verificando, com("verificando", Some("revisor"))));
        assert_eq!(
            caso(&tarefa("EmRevisao"), Default::default()),
            (Estado::EmRevisao, com("em revisão", Some("com o chat dele")))
        );
        assert_eq!(caso(&tarefa("Aprovada"), Default::default()), (Estado::Pronto, com("aprovada", Some("juntada"))));
        assert_eq!(caso(&tarefa("Falhou"), Default::default()), (Estado::Erro, com("falhou", None)));
        assert_eq!(caso(&tarefa("Cancelada"), Default::default()), (Estado::Parado, com("cancelada", None)));
    }

    #[test]
    fn motivo_da_parada_em_poucas_palavras() {
        assert_eq!(motivo_curto("o Vitral foi fechado no meio", false), "Vitral fechou");
        assert_eq!(motivo_curto("o Vitral foi fechado no meio do turno", false), "Vitral fechou");
        assert_eq!(motivo_curto("o Vitral foi fechado no meio", true), "revisão parou");
        assert_eq!(motivo_curto("a verificação foi interrompida", true), "verificação parou");
        assert_eq!(motivo_curto("a preparação da cópia foi interrompida", false), "preparação parou");
        assert_eq!(motivo_curto("você interrompeu", true), "você interrompeu");
        assert_eq!(motivo_curto("outro motivo", false), "outro motivo");
    }

    #[test]
    fn parada_pelo_vitral_e_ambar_e_por_voce_e_neutra() {
        let mut t = tarefa("Rodando");
        t.parada = Some("o Vitral foi fechado no meio".into());
        let atencao = FatosDaTarefa { parada: true, precisa_retomar: true, ..Default::default() };
        assert_eq!(ver(&t, atencao), (Estado::ParadoAtencao, "parada".into(), Some("Vitral fechou".into())));
        t.parada = Some("você interrompeu".into());
        let voce = FatosDaTarefa { parada: true, ..Default::default() };
        assert_eq!(ver(&t, voce), (Estado::Parado, "parada".into(), Some("você interrompeu".into())));
        let mut revisao = tarefa("EmRevisao");
        revisao.parada = Some("a verificação foi interrompida".into());
        let cortada = FatosDaTarefa { parada: true, precisa_retomar: true, ..Default::default() };
        assert_eq!(ver(&revisao, cortada).2.as_deref(), Some("verificação parou"));
        // O pedido vence a parada: o trabalhador que pede está rodando.
        let pedido = FatosDaTarefa { execucao: Some(Execucao::EsperandoVoce), parada: true, ..Default::default() };
        assert_eq!(ver(&tarefa("Rodando"), pedido).0, Estado::EsperandoVoce);
    }

    /// O âmbar fica só para o que precisa de você; a revisão anda na família do rodando.
    #[test]
    fn ambar_so_no_que_precisa_de_voce() {
        use crate::tema::Cores;
        let ambar: Vec<Estado> = Estado::TODOS.into_iter().filter(|e| e.cor() == Some(Cores::ambar())).collect();
        assert_eq!(ambar, [Estado::EsperandoVoce, Estado::ParadoAtencao]);
        assert_eq!(Estado::EmRevisao.cor(), Some(Cores::destaque()));
        assert_eq!(Estado::EmRevisao.cor_texto(), Cores::destaque_texto());
        assert_eq!(Estado::Rodando.cor_texto(), Cores::destaque_texto());
    }

    #[test]
    fn rodando_respira_de_um_a_055() {
        use super::opacidade_do_respiro;
        use std::time::Duration;
        assert!((opacidade_do_respiro(Duration::ZERO) - 1.).abs() < 1e-4);
        assert!((opacidade_do_respiro(Duration::from_millis(800)) - 0.55).abs() < 1e-4);
        assert!((opacidade_do_respiro(Duration::from_millis(1600)) - 1.).abs() < 1e-4);
    }

    #[test]
    fn sinal_segue_a_prioridade_da_familia() {
        let nada = FatosDoSinal::default();
        assert_eq!(sinal_de(&[nada, nada]), None);
        let rodando = FatosDoSinal { rodando: true, ..nada };
        let nova = FatosDoSinal { nova: true, ..nada };
        let parada = FatosDoSinal { parada: true, ..nada };
        let pedido = FatosDoSinal { pedido: true, rodando: true, ..nada };
        let erro = FatosDoSinal { erro: true, ..nada };
        assert_eq!(sinal_de(&[rodando]), Some(Sinal::Rodando));
        assert_eq!(sinal_de(&[rodando, nova]), Some(Sinal::Nova));
        assert_eq!(sinal_de(&[nova, parada]), Some(Sinal::Parada));
        assert_eq!(sinal_de(&[parada, pedido]), Some(Sinal::Pedido));
        assert_eq!(sinal_de(&[pedido, erro, nova]), Some(Sinal::Erro));
        // Parada por você não pede Retomar: sem ponto.
        assert_eq!(sinal_de(&[FatosDoSinal { parada: false, ..nada }]), None);
        assert!(Sinal::Rodando.vazado() && !Sinal::Nova.vazado());
    }
}
