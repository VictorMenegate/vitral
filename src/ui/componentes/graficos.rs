//! Anéis, barrinhas de sinal, a barra segmentada e a retícula comum (trama, fio de
//! pontos e hachura).

use std::f32::consts::TAU;

use gpui_kit::component::ElementExt as _;

use gpui_kit::component::tooltip::{ManagedTooltipExt as _, Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::base::Marca;
use crate::tema::{ARCO_IRIS, Cores, esp, r, raio};
use crate::ui::movimento::{self, Curva};

// A retícula comum do Vitral: um padrão do shader (`checkerboard`, `pattern_slash`) por quad, no
// lugar de dezenas de quads ou de um `canvas`. Todo uso no app passa por aqui (o teste
// `reticula_so_pelos_graficos` barra o resto), e o desenho mora no kit (`trama`,
// `fio_de_pontos`), que os diálogos e menus do kit também usam: se um renderizador perder o
// padrão, a queda para o liso é uma edição num lugar só.

/// A trama fina (xadrez de 2 px na `cor`, o resto transparente): placa de cabeçalho, vidro da
/// lista, véu dos diálogos, desmanche da rolagem.
pub fn trama(cor: Hsla, window: &Window) -> Background {
    gpui_kit::component::trama(cor, window)
}

/// O fio de pontos (ponto de 2 a cada 4), deitado: todo separador e todo trilho, no lugar da
/// linha cheia. Ocupa a largura do pai; para recuar, ponha dentro de um `div` com `px(..)`.
pub fn fio_de_pontos(cor: Hsla, window: &Window) -> Div {
    gpui_kit::component::fio_de_pontos(cor, window)
}

/// O fio de pontos em pé, na altura do pai.
pub fn fio_de_pontos_em_pe(cor: Hsla, window: &Window) -> Div {
    gpui_kit::component::fio_de_pontos_em_pe(cor, window)
}

/// A hachura diagonal do que saiu ou parou (a linha removida do diff). O shader repete a hachura
/// na vertical a cada `largura + intervalo` px do aparelho: com meio período por linha, os riscos
/// de linhas vizinhas emendam. Largura sempre 1 (os dois números vão empacotados num `f32`, e
/// 1,5 perde exatidão); o risco sai com uns 0,7 px, por isso a cor pede alfa mais alto (0,24).
pub fn hachura(cor: Hsla, altura_da_linha: Pixels, window: &Window) -> Background {
    let periodo = (altura_da_linha.as_f32() * window.scale_factor() / 2.).round().max(2.);
    pattern_slash(cor, 1., periodo - 1.)
}

/// Cor de um uso: a base até 69%, âmbar de 70% a 89%, vermelho a partir de 90%. Usa a
/// porcentagem inteira, a mesma que o texto mostra.
pub fn cor_da_faixa(fracao: f32, base: Hsla) -> Hsla {
    match (fracao * 100.).round() as i32 {
        ..=69 => base,
        70..=89 => Cores::ambar(),
        _ => Cores::vermelho(),
    }
}

/// A fenda de todo anel do app: 60° abertos embaixo (o anel vai das 7 às 5 horas).
const FENDA: f32 = TAU / 6.;

/// Um arco de `de` a `ate` (radianos, 0 no topo, sentido horário; podem ser negativos) com uma
/// tampa redonda em cada ponta: o traço do anel com fenda e do anel que gira.
/// Um traço de anel: o arco de `de` a `ate` (radianos, 0 no alto, no sentido do relógio), na
/// `cor`, com as pontas redondas ou não (o trilho inteiro não tem ponta).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Traco {
    pub de: f32,
    pub ate: f32,
    pub cor: Hsla,
    pub pontas: bool,
}

/// Os traços de um anel numa imagem BGRA de `largura` por `altura` pixels do aparelho, com o
/// centro no meio, o meio do traço a `raio_meio` do centro e a `espessura` dele (tudo em pixels
/// do aparelho). A borda é suavizada em um pixel, pela distância até o traço; os traços se cobrem
/// na ordem (o trilho, depois o progresso). Alfa direto, como as outras imagens do Vitral.
pub(crate) fn tracos_em_bgra(largura: u32, altura: u32, raio_meio: f32, espessura: f32, tracos: &[Traco]) -> Vec<u8> {
    let (cx, cy) = (largura as f32 / 2., altura as f32 / 2.);
    let meia = espessura / 2.;
    let cores: Vec<[f32; 4]> = tracos
        .iter()
        .map(|t| {
            let c = Rgba::from(t.cor);
            [c.r, c.g, c.b, c.a]
        })
        .collect();
    let mut bgra = Vec::with_capacity((largura * altura * 4) as usize);
    for y in 0..altura {
        for x in 0..largura {
            let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
            let raio = (dx * dx + dy * dy).sqrt();
            let angulo = dx.atan2(-dy).rem_euclid(TAU);
            let na_faixa = (raio - raio_meio).abs() - meia;
            // Pré-multiplicado enquanto soma; no fim, de volta ao alfa direto.
            let mut soma = [0f32; 4];
            for (traco, cor) in tracos.iter().zip(&cores) {
                let vao = traco.ate - traco.de;
                let distancia = if vao >= TAU - 1e-4 {
                    na_faixa
                } else {
                    let dentro = (angulo - traco.de).rem_euclid(TAU) <= vao;
                    let mut d = if dentro { na_faixa } else { f32::INFINITY };
                    if traco.pontas {
                        for ponta in [traco.de, traco.ate] {
                            let (px, py) = (cx + raio_meio * ponta.sin(), cy - raio_meio * ponta.cos());
                            let (ex, ey) = (x as f32 + 0.5 - px, y as f32 + 0.5 - py);
                            d = d.min((ex * ex + ey * ey).sqrt() - meia);
                        }
                    }
                    d
                };
                let cobertura = (0.5 - distancia).clamp(0., 1.);
                if cobertura <= 0. {
                    continue;
                }
                let a = cobertura * cor[3];
                for k in 0..3 {
                    soma[k] = cor[k] * a + soma[k] * (1. - a);
                }
                soma[3] = a + soma[3] * (1. - a);
            }
            let canal = |v: f32| (v.clamp(0., 1.) * 255. + 0.5) as u8;
            if soma[3] > 0. {
                let a = soma[3];
                bgra.extend_from_slice(&[canal(soma[2] / a), canal(soma[1] / a), canal(soma[0] / a), canal(a)]);
            } else {
                bgra.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    bgra
}

/// Pinta os traços de um anel centrado em `bounds` como UMA imagem, feita na CPU e guardada.
/// Era um caminho (`paint_path`) por arco, e no GPUI cada lote de caminhos para o desenho da
/// janela e faz uma passada da janela inteira com antisserrilhado 4x: com os anéis da barra de
/// baixo, das caixas e os que giram, a placa de vídeo da AMD integrada passava uns 4 ms por quadro
/// só nisso (medido na bancada em 26/11/2026: sem os anéis, 365 para 244 ms de GPU por segundo).
pub(crate) fn pintar_tracos(bounds: Bounds<Pixels>, raio_fora: f32, espessura: f32, tracos: &[Traco], window: &mut Window, cx: &mut App) {
    let escala = window.scale_factor();
    let largura = (bounds.size.width.as_f32() * escala).ceil().max(1.) as u32;
    let altura = (bounds.size.height.as_f32() * escala).ceil().max(1.) as u32;
    if largura > 512 || altura > 512 || tracos.is_empty() {
        return;
    }
    let raio_meio = (raio_fora - espessura / 2.) * escala;
    let espessura = espessura * escala;
    // Os ângulos em quartos de grau: o mesmo desenho dá a mesma chave.
    let grau = |a: f32| (a.rem_euclid(TAU) * 1440. / TAU).round() as i64;
    let mut chave = std::collections::hash_map::DefaultHasher::new();
    {
        use std::hash::Hash as _;
        (largura, altura, (raio_meio * 64.).round() as i64, (espessura * 64.).round() as i64).hash(&mut chave);
        for t in tracos {
            let vao = (((t.ate - t.de) * 1440. / TAU).round() as i64).min(1440);
            let cor = Rgba::from(t.cor);
            (grau(t.de), vao, t.pontas, [cor.r, cor.g, cor.b, cor.a].map(|c| (c * 255.).round() as u8)).hash(&mut chave);
        }
    }
    let chave = std::hash::Hasher::finish(&chave);
    super::metal::pintar_imagem_numa_colecao(bounds, chave, (largura, altura), window, cx, || {
        tracos_em_bgra(largura, altura, raio_meio, espessura, tracos)
    });
}

pub fn pintar_arco(
    bounds: Bounds<Pixels>,
    raio_fora: f32,
    espessura: f32,
    de: f32,
    ate: f32,
    cor: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    pintar_tracos(bounds, raio_fora, espessura, &[Traco { de, ate, cor, pontas: true }], window, cx);
}

fn pintar_anel(
    bounds: Bounds<Pixels>,
    raio_fora: f32,
    espessura: f32,
    fracao: Option<f32>,
    cor: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let (de, ate) = (-(TAU - FENDA) / 2., (TAU - FENDA) / 2.);
    let mut tracos = vec![Traco { de, ate, cor: Cores::borda_forte(), pontas: true }];
    if let Some(f) = fracao.map(|f| f.clamp(0., 1.)).filter(|f| *f > 0.) {
        tracos.push(Traco { de, ate: de + f * (ate - de), cor, pontas: true });
    }
    pintar_tracos(bounds, raio_fora, espessura, &tracos, window, cx);
}

fn pintar_anel_inteiro(
    bounds: Bounds<Pixels>,
    raio_fora: f32,
    espessura: f32,
    fracao: Option<f32>,
    cor: Hsla,
    window: &mut Window,
    cx: &mut App,
) {
    let mut tracos = vec![Traco { de: 0., ate: TAU, cor: Cores::traco(), pontas: false }];
    if let Some(f) = fracao.map(|f| f.clamp(0., 1.)).filter(|f| *f > 0.) {
        tracos.push(Traco { de: 0., ate: f.max(0.02) * TAU, cor, pontas: true });
    }
    pintar_tracos(bounds, raio_fora, espessura, &tracos, window, cx);
}

/// O ângulo de um anel que gira, em passos de 5 graus: as imagens dele se repetem a cada volta
/// (`pintar_tracos` guarda cada uma), e a 30 quadros por segundo o passo não se vê.
fn giro_em_passos(giro: f32) -> f32 {
    let passo = TAU / 72.;
    (giro / passo).round() * passo
}

/// Anel de progresso: contexto, uso da conta, progresso da Equipe. O arco desliza até a fração
/// nova em 240 ms (o `id` guarda de onde ele vem; precisa ser único entre os irmãos).
#[derive(IntoElement)]
pub struct Anel {
    id: ElementId,
    fracao: f32,
    cor: Option<Hsla>,
    diametro: f32,
    espessura: Option<f32>,
    centro: Option<AnyElement>,
    dica: Option<SharedString>,
    movimento: MovimentoDoAnel,
    /// O trilho inteiro, fino, enchendo a partir do alto, como o `AnelDeUso` (o contexto da barra
    /// de baixo, ao lado das pílulas de uso).
    inteiro: bool,
}

/// Como o arco chega à fração.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum MovimentoDoAnel {
    /// Desliza quando a fração muda; na montagem já está nela.
    Desliza,
    /// Enche de 0 na montagem (o popover de uso que abre) e desliza depois.
    Enche,
    /// Sempre seco (o anel dentro de um `Simbolo`, que não tem id próprio).
    Seco,
}

impl Anel {
    pub fn new(id: impl Into<ElementId>, fracao: f32) -> Self {
        Anel {
            id: id.into(),
            fracao,
            cor: None,
            diametro: 20.,
            espessura: None,
            centro: None,
            dica: None,
            movimento: MovimentoDoAnel::Desliza,
            inteiro: false,
        }
    }

    /// O círculo inteiro, como os anéis de uso (dono, 26/11/2026: "deixa o de contexto um círculo
    /// inteiro igual os outros").
    pub fn inteiro(mut self) -> Self {
        self.inteiro = true;
        self
    }

    /// Enche de 0 até a fração ao aparecer (popover de uso aberto).
    pub fn encher(mut self) -> Self {
        self.movimento = MovimentoDoAnel::Enche;
        self
    }

    /// Sem transição.
    pub fn seco(mut self) -> Self {
        self.movimento = MovimentoDoAnel::Seco;
        self
    }

    /// Sem cor, segue a faixa sobre o destaque (até 70%, âmbar, vermelho).
    pub fn cor(mut self, cor: Hsla) -> Self {
        self.cor = Some(cor);
        self
    }

    /// 20, 22, 28 ou 36.
    pub fn diametro(mut self, d: f32) -> Self {
        self.diametro = d;
        self
    }

    pub fn espessura(mut self, e: f32) -> Self {
        self.espessura = Some(e);
        self
    }

    pub fn centro(mut self, e: impl IntoElement) -> Self {
        self.centro = Some(e.into_any_element());
        self
    }

    pub fn dica(mut self, d: impl Into<SharedString>) -> Self {
        self.dica = Some(d.into());
        self
    }
}

/// A fração deste quadro: a de agora ou a que desliza até ela.
fn fracao_animada(
    id: &ElementId,
    alvo: f32,
    movimento: MovimentoDoAnel,
    window: &mut Window,
    cx: &mut App,
) -> f32 {
    let chave = movimento::chave(id, "fracao");
    match movimento {
        MovimentoDoAnel::Seco => alvo,
        MovimentoDoAnel::Desliza => movimento::valor(chave, alvo, movimento::ENCHER, Curva::Mover, window, cx),
        MovimentoDoAnel::Enche => movimento::valor_desde(chave, 0., alvo, movimento::ENCHER, Curva::Mover, window, cx),
    }
}

impl RenderOnce for Anel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let diametro = self.diametro;
        // Grosso, como o Circular Progress: 3 até 22, 4,7 no de 36, 5,7 no de 44.
        let espessura = self.espessura.unwrap_or((diametro * 0.13).max(3.));
        let alvo = self.fracao.clamp(0., 1.);
        let cor = self.cor.unwrap_or_else(|| cor_da_faixa(alvo, Cores::destaque()));
        let fracao = fracao_animada(&self.id, alvo, self.movimento, window, cx);
        let inteiro = self.inteiro;
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(r(diametro))
            .flex()
            .items_center()
            .justify_center()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let escala = bounds.size.width.as_f32() / diametro;
                        let raio_fora = bounds.size.width.min(bounds.size.height).as_f32() / 2.;
                        if inteiro {
                            pintar_anel_inteiro(bounds, raio_fora, espessura * escala, Some(fracao), cor, window, cx);
                        } else {
                            pintar_anel(bounds, raio_fora, espessura * escala, Some(fracao), cor, window, cx);
                        }
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .children(self.centro)
            .when_some(self.dica, |s, dica| {
                s.dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            })
    }
}

/// O "carregando" do app: o mesmo anel com fenda, aberto, girando (uma volta a cada 1,2 s, no
/// laço de 20 qps). Sem trilho. Com o movimento reduzido fica parado, com a fenda embaixo.
#[derive(IntoElement)]
pub struct AnelGirando {
    lado: f32,
    cor: Hsla,
    vivo: Option<String>,
}

impl AnelGirando {
    /// 12 no nó da ferramenta e na linha de estado, 14 no lugar do spinner pequeno do kit.
    pub fn new(lado: f32) -> Self {
        AnelGirando { lado, cor: Cores::destaque(), vivo: None }
    }

    pub fn cor(mut self, cor: Hsla) -> Self {
        self.cor = cor;
        self
    }

    /// Gira numa vista própria de `chave` (`vista_viva`): a batida redesenha só o anel, e não a
    /// vista em volta (a Janela, as mensagens).
    pub fn vivo(mut self, chave: impl Into<String>) -> Self {
        self.vivo = Some(chave.into());
        self
    }

}

impl RenderOnce for AnelGirando {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (lado, cor) = (self.lado, self.cor);
        if let Some(chave) = self.vivo.filter(|_| !cx.reduce_motion()) {
            let versao = u64::from(lado.to_bits()) << 32 ^ u64::from(Rgba::from(cor).r.to_bits() ^ Rgba::from(cor).b.to_bits().rotate_left(8));
            let estilo = StyleRefinement::default().flex_none().size(r(lado));
            return div().flex_none().size(r(lado)).child(super::vista_viva::em_vista_propria(chave, estilo, versao, cx, move |window, cx| {
                RenderOnce::render(AnelGirando { lado, cor, vivo: None }, window, cx).into_any_element()
            }));
        }
        // O arco cobre 72% da volta; parado, a fenda de 28% fica centrada embaixo.
        let parado = std::f32::consts::PI + TAU * 0.14;
        let giro = if cx.reduce_motion() {
            parado
        } else {
            let fase = movimento::laco_a(window, cx, movimento::QPS_DO_LACO);
            parado + ((fase / 1.2).fract() as f32) * TAU
        };
        div().flex_none().size(r(lado)).child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, cx| {
                    let escala = bounds.size.width.as_f32() / lado;
                    let raio_fora = bounds.size.width.min(bounds.size.height).as_f32() / 2.;
                    let espessura = (lado * 0.16).max(1.5) * escala;
                    let giro = giro_em_passos(giro);
                    pintar_arco(bounds, raio_fora, espessura, giro, giro + TAU * 0.72, cor, window, cx);
                },
            )
            .size_full(),
        )
    }
}

/// Uso de um serviço no trilho: o disco da marca com UM anel em volta, que mostra a janela mais
/// apertada das duas (5 h ou semana). Eram dois anéis, um dentro do outro; com a fenda e o trilho
/// eles liam como "duas barras" (dono, 12/11/2026). As duas porcentagens seguem na dica e no
/// popover de uso.
#[derive(IntoElement)]
pub struct AnelDeUso {
    id: ElementId,
    marca: Marca,
    cinco_h: Option<f32>,
    semana: Option<f32>,
    diametro: f32,
    bloqueado: bool,
    apagado: bool,
    encher: bool,
    atencao: bool,
    /// Só o anel, sem o disco da marca no meio (as pílulas da barra de baixo já dizem o nome).
    sem_marca: bool,
}

impl AnelDeUso {
    pub fn new(id: impl Into<ElementId>, marca: Marca, cinco_h: Option<f32>) -> Self {
        AnelDeUso {
            id: id.into(),
            marca,
            cinco_h,
            semana: None,
            diametro: 36.,
            bloqueado: false,
            apagado: false,
            encher: false,
            atencao: false,
            sem_marca: false,
        }
    }

    /// Sem o disco da marca no meio (dono, 26/11/2026: "tira os ícones do meio").
    pub fn sem_marca(mut self) -> Self {
        self.sem_marca = true;
        self
    }

    /// No ritmo de agora o limite acaba antes de renovar: o arco fica âmbar mesmo abaixo de 70%.
    pub fn atencao(mut self, sim: bool) -> Self {
        self.atencao = sim;
        self
    }

    /// Os arcos enchem de 0 ao aparecer (popover de uso aberto); depois, deslizam.
    pub fn encher(mut self) -> Self {
        self.encher = true;
        self
    }

    pub fn semana(mut self, f: Option<f32>) -> Self {
        self.semana = f;
        self
    }

    /// O anel já é um só; fica pelo nome, para quem o chamava.
    pub fn unico(self) -> Self {
        self
    }

    /// 36 no trilho, 32 recolhido, 36 nas Contas, 20 no menu.
    pub fn diametro(mut self, d: f32) -> Self {
        self.diametro = d;
        self
    }

    /// Limite atingido: anel cheio em vermelho.
    pub fn bloqueado(mut self, sim: bool) -> Self {
        self.bloqueado = sim;
        self
    }

    /// Serviço ausente: disco cinza, sem arco.
    pub fn apagado(mut self, sim: bool) -> Self {
        self.apagado = sim;
        self
    }
}

impl RenderOnce for AnelDeUso {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let diametro = self.diametro;
        let base = self.marca.cor();
        // A janela mais apertada das duas é a que o anel mostra.
        let apertada = match (self.cinco_h, self.semana) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
        let fora = if self.apagado {
            None
        } else if self.bloqueado {
            Some(1.)
        } else {
            apertada
        };
        let cor_fora = if self.bloqueado {
            Cores::vermelho()
        } else if self.atencao && (fora.unwrap_or(0.) * 100.).round() < 70. {
            Cores::ambar()
        } else {
            cor_da_faixa(fora.unwrap_or(0.), base)
        };
        // Sem número, sem arco.
        let alvo = fora.unwrap_or(0.).clamp(0., 1.);
        let chave = movimento::chave(&self.id, "arcos");
        let f = if self.encher {
            movimento::valor_desde(chave, 0., alvo, movimento::ENCHER, Curva::Mover, window, cx)
        } else {
            movimento::valor(chave, alvo, movimento::ENCHER, Curva::Mover, window, cx)
        };
        let fora = fora.map(|_| f);
        let disco = diametro - 10.;
        div()
            .id(self.id)
            .relative()
            .flex_none()
            .size(r(diametro))
            .flex()
            .items_center()
            .justify_center()
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, cx| {
                        let escala = bounds.size.width.as_f32() / diametro;
                        let raio_fora = bounds.size.width.min(bounds.size.height).as_f32() / 2.;
                        // Um anel fino e inteiro, que enche a partir do alto.
                        pintar_anel_inteiro(bounds, raio_fora, 2. * escala, fora, cor_fora, window, cx);
                    },
                )
                .absolute()
                .top_0()
                .left_0()
                .size_full(),
            )
            .when(!self.sem_marca, |s| {
                s.child({
                    let disco = self.marca.disco(disco);
                    if self.apagado { disco.apagada() } else { disco }
                })
            })
    }
}

/// Barrinhas de intensidade (esforço): 5 barras de 3 com folga 2, alinhadas embaixo.
#[derive(IntoElement)]
pub struct BarrinhasDeSinal {
    nivel: u8,
    total: u8,
    cor: Hsla,
    altura: f32,
    sugerido: bool,
    arco_iris: bool,
}

impl BarrinhasDeSinal {
    pub fn new(nivel: u8, total: u8) -> Self {
        BarrinhasDeSinal { nivel, total, cor: Cores::ciano(), altura: 14., sugerido: false, arco_iris: false }
    }

    pub fn cor(mut self, cor: Hsla) -> Self {
        self.cor = cor;
        self
    }

    /// 14 no chip, 12 no cabeçalho do popover.
    pub fn altura(mut self, a: f32) -> Self {
        self.altura = a;
        self
    }

    /// "Padrão": o nível do modelo, com as ativas a 45%.
    pub fn sugerido(mut self) -> Self {
        self.sugerido = true;
        self
    }

    /// Todas acesas, cada uma numa cor do arco-íris (o Ultrathink armado).
    pub fn arco_iris(mut self) -> Self {
        self.arco_iris = true;
        self
    }
}

impl RenderOnce for BarrinhasDeSinal {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        // Barras de 2 com vão de 2, de 4 a 12, embaixo na caixa de 14: 18 x 12 de desenho, a medida
        // de um ícone ao lado do escudo e do glifo. Eram de 3, até 14: 23 x 14, quase o dobro do
        // escudo (relatório de aparência, 16/11/2026, V11). O vão de 2 deixa as posições inteiras
        // a 100% e a 150%.
        const ALTURAS: [f32; 5] = [4., 6., 8., 10., 12.];
        let escala = self.altura / 14.;
        let total = self.total.max(1) as usize;
        let ativa = if self.sugerido { self.cor.opacity(0.45) } else { self.cor };
        div()
            .flex_none()
            .flex()
            .items_end()
            .gap(r(2. * escala))
            .h(r(self.altura))
            .pb(r(escala))
            .children((0..total).map(|i| {
                let altura = ALTURAS[(i * (ALTURAS.len() - 1)) / (total - 1).max(1)];
                let cor = if self.arco_iris {
                    rgb(ARCO_IRIS[i * (ARCO_IRIS.len() - 1) / (total - 1).max(1)]).into()
                } else if i < self.nivel as usize {
                    ativa
                } else {
                    Cores::barra_apagada()
                };
                div().w(r(2. * escala)).h(r(altura * escala)).rounded_full().bg(cor)
            }))
    }
}

/// Proporção de estados numa barra fina, com dica por segmento.
#[derive(IntoElement)]
pub struct BarraSegmentada {
    partes: Vec<(usize, Hsla, SharedString)>,
    fina: bool,
}

impl BarraSegmentada {
    pub fn new(partes: Vec<(usize, Hsla, SharedString)>) -> Self {
        BarraSegmentada { partes, fina: false }
    }

    pub fn fina(mut self) -> Self {
        self.fina = true;
        self
    }
}

impl RenderOnce for BarraSegmentada {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let total: usize = self.partes.iter().map(|(n, _, _)| n).sum::<usize>().max(1);
        let id: Vec<&str> = self.partes.iter().map(|(_, _, dica)| dica.as_ref()).collect();
        div()
            .id(SharedString::from(format!("barra-{}", id.join("-"))))
            .w_full()
            .flex()
            .gap(esp::E2)
            .h(r(if self.fina { 4. } else { 6. }))
            .rounded(raio::PILULA)
            .bg(Cores::recuo())
            .children(self.partes.into_iter().enumerate().filter(|(_, (n, _, _))| *n > 0).map(
                |(i, (n, cor, dica))| {
                    div()
                        .id(("segmento", i))
                        .h_full()
                        .w(relative(n as f32 / total as f32))
                        .rounded(raio::PILULA)
                        .bg(cor)
                        .dica_no_ponteiro(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                },
            ))
    }
}

#[cfg(test)]
mod testes {
    use std::f32::consts::TAU;

    use gpui_kit::{Hsla, hsla};

    use super::{Traco, cor_da_faixa, tracos_em_bgra};
    use crate::tema::Cores;

    /// O alfa do pixel (x, y) de uma imagem BGRA de `largura`.
    fn alfa(bgra: &[u8], largura: u32, x: u32, y: u32) -> u8 {
        bgra[((y * largura + x) * 4 + 3) as usize]
    }

    /// O anel em imagem: o trilho inteiro cobre a faixa toda e nada fora dela; o arco de um quarto
    /// (do alto até a direita, com pontas redondas) cobre o alto e a direita, e não a esquerda.
    #[test]
    fn o_anel_vira_imagem_no_lugar_certo() {
        let branco: Hsla = hsla(0., 0., 1., 1.);
        let (lado, raio_meio, espessura) = (40u32, 15., 4.);
        let trilho = tracos_em_bgra(lado, lado, raio_meio, espessura, &[Traco { de: 0., ate: TAU, cor: branco, pontas: false }]);
        assert_eq!(trilho.len(), (lado * lado * 4) as usize);
        // No alto, embaixo, à esquerda e à direita, no meio do traço: cheio.
        for (x, y) in [(20, 5), (20, 34), (5, 20), (34, 20)] {
            assert_eq!(alfa(&trilho, lado, x, y), 255, "({x}, {y})");
        }
        // O centro e os cantos: vazios.
        for (x, y) in [(20, 20), (0, 0), (39, 39)] {
            assert_eq!(alfa(&trilho, lado, x, y), 0, "({x}, {y})");
        }
        let quarto = tracos_em_bgra(lado, lado, raio_meio, espessura, &[Traco { de: 0., ate: TAU / 4., cor: branco, pontas: true }]);
        assert_eq!(alfa(&quarto, lado, 20, 5), 255, "o começo, no alto");
        assert_eq!(alfa(&quarto, lado, 34, 20), 255, "o fim, à direita");
        assert_eq!(alfa(&quarto, lado, 30, 9), 255, "o meio do arco");
        assert_eq!(alfa(&quarto, lado, 5, 20), 0, "a esquerda fica vazia");
        assert_eq!(alfa(&quarto, lado, 20, 34), 0, "embaixo fica vazio");
        // Um arco que passa da volta (o anel girando) dá a mesma imagem que o mesmo arco na volta.
        let girado = tracos_em_bgra(lado, lado, raio_meio, espessura, &[Traco { de: TAU * 3., ate: TAU * 3. + TAU / 4., cor: branco, pontas: true }]);
        assert_eq!(girado, quarto);
    }

    #[test]
    fn faixa_de_uso_nos_limites() {
        let base = Cores::claude();
        assert_eq!(cor_da_faixa(0.69, base), base);
        assert_eq!(cor_da_faixa(0.70, base), Cores::ambar());
        assert_eq!(cor_da_faixa(0.89, base), Cores::ambar());
        assert_eq!(cor_da_faixa(0.90, base), Cores::vermelho());
        assert_eq!(cor_da_faixa(0., base), base);
        assert_eq!(cor_da_faixa(1.2, base), Cores::vermelho());
    }
}

// ---------- os anéis que giram dentro da conversa ----------
//
// Dentro das mensagens (uma vista em cache), um anel girando redesenhava a conversa inteira a cada
// batida; parado, ele lia como travado (dono, 18/11/2026: "o loading tá travado", duas vezes). Então
// cada anel da conversa (o "Pensando…", a chamada rodando, a revisão da memória) deixa só o lugar,
// medido no prepaint (`lugar_do_anel`), e quem gira é `AneisDaConversa`, por cima das mensagens,
// numa vista própria: a batida redesenha só ela.

thread_local! {
    /// Os lugares dos anéis de cada painel, na janela, com a cor: zerados a cada desenho das
    /// mensagens dele (`limpar_aneis`), preenchidos no prepaint.
    static ANEIS: std::cell::RefCell<std::collections::HashMap<u64, Vec<(Bounds<Pixels>, Hsla)>>> = Default::default();
}

/// Começo do desenho das mensagens de um painel: os lugares de antes saem.
pub fn limpar_aneis(painel: u64) {
    ANEIS.with(|a| a.borrow_mut().remove(&painel));
}

/// O lugar de um anel girando dentro da conversa: vazio, do tamanho dele; `AneisDaConversa` pinta
/// o anel ali.
pub fn lugar_do_anel(painel: u64, cor: Hsla, lado: f32) -> Div {
    div().flex_none().size(r(lado)).on_prepaint(move |limites, _, _| {
        ANEIS.with(|a| a.borrow_mut().entry(painel).or_default().push((limites, cor)));
    })
}

/// Os anéis da conversa de um painel, girando por cima dela numa vista própria. `ativo`: o turno
/// (ou a revisão da memória) está rodando; só então a batida anda.
#[derive(IntoElement)]
pub struct AneisDaConversa {
    pub chave: String,
    pub painel: u64,
    pub ativo: bool,
}

impl RenderOnce for AneisDaConversa {
    fn render(self, _: &mut Window, cx: &mut App) -> impl IntoElement {
        let (painel, ativo) = (self.painel, self.ativo);
        super::vista_viva::em_vista_propria(self.chave, StyleRefinement::default().size_full(), u64::from(ativo), cx, move |window, cx| {
            let parado = std::f32::consts::PI + TAU * 0.14;
            let giro = if ativo && !cx.reduce_motion() {
                let fase = movimento::laco_a(window, cx, movimento::QPS_DO_LACO);
                parado + ((fase / 1.2).fract() as f32) * TAU
            } else {
                parado
            };
            canvas(
                |_, _, _| {},
                move |limites, _, window, cx| {
                    if !ativo {
                        return;
                    }
                    let aneis = ANEIS.with(|a| a.borrow().get(&painel).cloned().unwrap_or_default());
                    for (b, cor) in aneis.into_iter().filter(|(b, _)| limites.intersects(b)) {
                        let lado = b.size.width.as_f32().max(1.);
                        let raio_fora = b.size.width.min(b.size.height).as_f32() / 2.;
                        let espessura = (lado * 0.16).max(1.5);
                        let giro = giro_em_passos(giro);
                        pintar_arco(b, raio_fora, espessura, giro, giro + TAU * 0.72, cor, window, cx);
                    }
                },
            )
            .size_full()
            .into_any_element()
        })
    }
}
