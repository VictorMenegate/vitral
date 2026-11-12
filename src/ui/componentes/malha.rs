//! A malha que deriva ("Mesh drift"): o fundo vivo do botão do menu do Vitral.
//!
//! O dono montou a receita no Shader Builder da 21st.dev e colou o shader (WebGL) em 12/11/2026,
//! com o pedido "usa tipo isso pro botão do menu". O Vitral não tem WebGL: aqui a MESMA conta do
//! fragment shader é feita em Rust, um ponto por pixel do botão, e pintada com `paint_quad`. No
//! tamanho do botão (uns 34 px) são de mil a quatro mil quadrados por quadro, o que a placa
//! desenha sem esforço; para a janela inteira esta técnica não serve: lá a mesma conta roda em
//! baixa resolução e vira textura (`metal.rs`, que usa a `cor_sem_grao` e o `grao` daqui).
//!
//! O que a receita desenha: cinco bolhas gaussianas (uma quase preta e quatro azuis) andando em
//! órbitas próprias; a cor de cada ponto é a média das cinco, pesada pela distância. Por cima,
//! contraste, saturação reduzida, vinheta, grão de filme (parado) e uma luz que segue o ponteiro.
//! Os números abaixo são os uniformes da receita, sem ajuste.

/// As cinco cores (sRGB, de 0 a 1): #171717, #91C5FF, #3A81F6, #2563EF e #1A4EDA.
pub(super) const CORES: [[f32; 3]; 5] = [
    [0.090, 0.090, 0.090],
    [0.569, 0.773, 1.000],
    [0.227, 0.506, 0.965],
    [0.145, 0.388, 0.937],
    [0.102, 0.306, 0.855],
];
/// `u_scene.z`: os segundos andam 1,09 vez mais rápido (velocidade 52 de 100).
pub(super) const VELOCIDADE: f32 = 1.09;
/// `u_shape`: a escala do campo e o quanto as bolhas se afastam do centro.
pub(super) const ESCALA: f32 = 1.26;
pub(super) const INTENSIDADE: f32 = 0.35;
/// `u_surface`: contraste, brilho e saturação.
pub(super) const CONTRASTE: f32 = 1.37;
pub(super) const BRILHO: f32 = -0.02;
pub(super) const SATURACAO: f32 = 0.68;
/// `u_finish`: vinheta e grão.
pub(super) const VINHETA: f32 = 0.54;
pub(super) const GRAO: f32 = 0.22;
/// `u_transform`: a semente e a deriva lenta do campo inteiro.
pub(super) const SEMENTE: f32 = 1.0;
pub(super) const DERIVA: f32 = 0.06;
/// `u_space.xy`: o deslocamento do campo.
pub(super) const DESLOCAMENTO: (f32, f32) = (-0.05, 0.0);
/// `u_cursor`: a força e o raio da luz do ponteiro (o efeito 4, o "spotlight").
pub(super) const FORCA_DO_PONTEIRO: f32 = 0.50;
pub(super) const RAIO_DO_PONTEIRO: f32 = 0.80;

fn fracao(x: f32) -> f32 {
    x - x.floor()
}

pub(super) fn degrau_suave(de: f32, ate: f32, x: f32) -> f32 {
    let t = ((x - de) / (ate - de)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// O `grainHash` da receita (o hash12 do Dave Hoskins): ruído branco por pixel, de 0 a 1.
pub(super) fn grao(x: f32, y: f32) -> f32 {
    let p = [fracao(x * 0.1031), fracao(y * 0.1031), fracao(x * 0.1031)];
    let d = p[0] * (p[1] + 33.33) + p[1] * (p[2] + 33.33) + p[2] * (p[0] + 33.33);
    let p = [p[0] + d, p[1] + d, p[2] + d];
    fracao((p[0] + p[1]) * p[2])
}

/// O que é igual para todos os pontos de um mesmo quadro: onde está cada bolha e a deriva do campo.
/// Feito uma vez por quadro, e não uma vez por pixel (são 10 senos e cossenos).
pub struct Quadro {
    pub(super) bolhas: [(f32, f32); 5],
    pub(super) deriva: (f32, f32),
}

impl Quadro {
    /// O quadro do instante `segundos` (o relógio do `movimento::laco_a`). Os senos são feitos em
    /// `f64`: em `f32` o relógio anda em degraus depois de um dia aberto e congela depois de doze.
    pub fn em(segundos: f64) -> Self {
        let t = segundos * f64::from(VELOCIDADE);
        let alcance = f64::from(0.45 + INTENSIDADE * 0.35);
        let mut bolhas = [(0., 0.); 5];
        for (i, bolha) in bolhas.iter_mut().enumerate() {
            let i = i as f64;
            *bolha = (
                ((t * (0.21 + i * 0.071) + i * 2.4 + f64::from(SEMENTE)).sin() * alcance) as f32,
                ((t * (0.17 + i * 0.093) + i * 1.7).cos() * alcance) as f32,
            );
        }
        let deriva = f64::from(DERIVA);
        Quadro { bolhas, deriva: ((deriva * (t * 0.31).sin()) as f32, (deriva * (t * 0.23).cos()) as f32) }
    }
}

/// A cor (sRGB, de 0 a 1) do ponto (`fx`, `fy`) de uma caixa de `largura` por `altura`, como o
/// `main()` do shader a calcula. As coordenadas são as do `gl_FragCoord`: o centro do pixel, com a
/// origem EMBAIXO à esquerda. `segundos` é o relógio (a fase do `movimento::laco_a`); `ponteiro`, a
/// posição do ponteiro na mesma caixa e no mesmo sentido, ou `None` com ele fora. Só os testes a
/// chamam (os pontos de referência da receita); o desenho monta o `Quadro` uma vez por quadro.
#[cfg(test)]
pub fn cor_da_malha(fx: f32, fy: f32, largura: f32, altura: f32, segundos: f64, ponteiro: Option<(f32, f32)>) -> [f32; 3] {
    cor_no_ponto(&Quadro::em(segundos), fx, fy, largura, altura, ponteiro)
}

/// A mesma conta, com o quadro já montado (é o que o desenho chama, ponto a ponto): a cor da
/// receita mais o grão de filme, que é por pixel e parado.
pub fn cor_no_ponto(quadro: &Quadro, fx: f32, fy: f32, largura: f32, altura: f32, ponteiro: Option<(f32, f32)>) -> [f32; 3] {
    let mut cor = cor_sem_grao(quadro, fx, fy, largura, altura, ponteiro);
    let ruido = (grao(fx + SEMENTE * 17., fy + SEMENTE * 31.) - 0.5) * GRAO;
    for canal in &mut cor {
        *canal = (*canal + ruido).clamp(0., 1.);
    }
    cor
}

/// A cor da receita sem o grão e sem o corte em 0 e 1 (o grão entra antes do corte): bolhas,
/// contraste, saturação, brilho, vinheta e a luz do ponteiro. É o que o campo da janela
/// (`metal.rs`) calcula em baixa resolução; lá o grão vai por cima, como textura.
pub(super) fn cor_sem_grao(quadro: &Quadro, fx: f32, fy: f32, largura: f32, altura: f32, ponteiro: Option<(f32, f32)>) -> [f32; 3] {
    let menor = largura.min(altura).max(1.);
    // O ponto, centrado, com o lado menor da caixa valendo 1.
    let (px, py) = ((fx - 0.5 * largura) / menor, (fy - 0.5 * altura) / menor);
    let mascara = ponteiro.map_or(0., |(mx, my)| {
        let (cx, cy) = ((mx - 0.5 * largura) / menor, (my - 0.5 * altura) / menor);
        1. - degrau_suave(0., RAIO_DO_PONTEIRO, (px - cx).hypot(py - cy))
    });
    // O campo: escala, deslocamento e a deriva lenta.
    let qx = px * ESCALA + DESLOCAMENTO.0 + quadro.deriva.0;
    let qy = py * ESCALA + DESLOCAMENTO.1 + quadro.deriva.1;
    // O `shade()`: a média das cores, pesada pela distância a cada bolha, com um piso da cor 0.
    let mut soma = [CORES[0][0] * 0.15, CORES[0][1] * 0.15, CORES[0][2] * 0.15];
    let mut total = 0.15;
    for (cor, (bx, by)) in CORES.iter().zip(quadro.bolhas) {
        let peso = (-((qx - bx).powi(2) + (qy - by).powi(2)) * 6.).exp();
        for canal in 0..3 {
            soma[canal] += cor[canal] * peso;
        }
        total += peso;
    }
    let mut cor = [soma[0] / total, soma[1] / total, soma[2] / total];
    // O acabamento, na ordem do shader: contraste, saturação, brilho, vinheta, luz do ponteiro.
    for canal in &mut cor {
        *canal = (*canal - 0.5) * CONTRASTE + 0.5;
    }
    let luma = 0.299 * cor[0] + 0.587 * cor[1] + 0.114 * cor[2];
    let do_centro = (fx / largura - 0.5).hypot(fy / altura - 0.5) * std::f32::consts::SQRT_2;
    let vinheta = 1. - VINHETA * degrau_suave(0.35, 1., do_centro);
    for canal in &mut cor {
        *canal = luma + (*canal - luma) * SATURACAO;
        *canal += BRILHO;
        *canal *= vinheta;
        *canal += (0.18 + *canal * 0.12) * mascara * FORCA_DO_PONTEIRO;
    }
    cor
}

/// Quanto de um quadrado de lado `celula`, centrado em (`x`, `y`), fica dentro do retângulo de
/// `largura` por `altura` com cantos de raio `raio` (de 0 a 1): a borda sai suave, sem serrilhado.
pub fn cobertura_do_canto(x: f32, y: f32, largura: f32, altura: f32, raio: f32, celula: f32) -> f32 {
    let raio = raio.min(largura / 2.).min(altura / 2.).max(0.);
    // A distância com sinal até o contorno do retângulo arredondado (negativa por dentro).
    let (dx, dy) = ((x - largura / 2.).abs() - (largura / 2. - raio), (y - altura / 2.).abs() - (altura / 2. - raio));
    let fora = dx.max(0.).hypot(dy.max(0.)) + dx.max(dy).min(0.) - raio;
    (0.5 - fora / celula.max(f32::EPSILON)).clamp(0., 1.)
}

// ---- desenho ----

use gpui_kit::*;

/// Quantos quadrados por lado, no máximo (16 mil no pior caso, um botão ampliado pelo zoom numa
/// tela de escala 2).
const MAXIMO_POR_LADO: f32 = 128.;

/// A fase com que a malha aparece parada (movimento desligado, janela sem foco, captura): em 0 a
/// bolha mais clara fica bem embaixo do V e ele perde contraste (2,4:1); em 4 s ele tem 6,6:1.
pub const FASE_INICIAL_DA_MALHA: f64 = 4.;

/// O fundo vivo do botão do menu: a malha da receita enchendo a caixa onde está, recortada em
/// cantos de raio `raio`. `fase` é o relógio do `movimento::laco_a`. Com `luz`, a luz da receita
/// segue o ponteiro enquanto ele está em cima (quem chama diz quando: com o ponteiro no botão e o
/// laço andando; parada, a malha não tem luz, e a captura sai igual em toda máquina). Não pega
/// clique.
pub fn malha_viva(fase: f64, raio: Pixels, luz: bool) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |limites, _, window, _| {
            let (largura, altura) = (limites.size.width.as_f32(), limites.size.height.as_f32());
            if largura < 1. || altura < 1. {
                return;
            }
            // Um quadrado por pixel do aparelho, até 2 por pixel de desenho (o teto da receita).
            let escala = window.scale_factor().max(0.5);
            let por_pixel = escala.clamp(1., 2.);
            let colunas = (largura * por_pixel).round().clamp(1., MAXIMO_POR_LADO);
            let linhas = (altura * por_pixel).round().clamp(1., MAXIMO_POR_LADO);
            // As bordas, já no pixel do aparelho: a direita de um quadrado é, bit a bit, a esquerda
            // do vizinho, e não sobra fresta nem coluna dobrada em escala fracionária (125%).
            let borda = |i: usize, n: f32, total: f32| ((i as f32 * total / n) * escala).round() / escala;
            // O ponteiro, na caixa e com o y para cima, como o shader espera.
            let ponteiro = Some(window.mouse_position()).filter(|p| luz && limites.contains(p)).map(|p| {
                let (x, y) = ((p.x - limites.origin.x).as_f32(), (p.y - limites.origin.y).as_f32());
                (x * colunas / largura, (altura - y) * linhas / altura)
            });
            let quadro = Quadro::em(fase);
            let raio = raio.as_f32();
            let celula = (largura / colunas).max(altura / linhas);
            for linha in 0..linhas as usize {
                let (cima, baixo) = (borda(linha, linhas, altura), borda(linha + 1, linhas, altura));
                for coluna in 0..colunas as usize {
                    let (esquerda, direita) = (borda(coluna, colunas, largura), borda(coluna + 1, colunas, largura));
                    let cobertura = cobertura_do_canto((esquerda + direita) / 2., (cima + baixo) / 2., largura, altura, raio, celula);
                    if cobertura <= 0. || direita <= esquerda || baixo <= cima {
                        continue;
                    }
                    let [r, g, b] = cor_no_ponto(&quadro, coluna as f32 + 0.5, linhas - linha as f32 - 0.5, colunas, linhas, ponteiro);
                    let caixa = Bounds::from_corners(
                        point(limites.origin.x + px(esquerda), limites.origin.y + px(cima)),
                        point(limites.origin.x + px(direita), limites.origin.y + px(baixo)),
                    );
                    window.paint_quad(fill(caixa, Hsla::from(Rgba { r, g, b, a: cobertura })));
                }
            }
        },
    )
    .size_full()
}

#[cfg(test)]
mod testes {
    use super::{Quadro, cobertura_do_canto, cor_da_malha, cor_no_ponto};

    fn perto(a: [f32; 3], b: [f32; 3]) -> bool {
        a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.004)
    }

    /// Os números de referência saem da porta em Python do shader (a mesma conta, em precisão
    /// dupla), conferida contra a receita por um segundo cálculo independente. Se a conta daqui
    /// mudar, o botão deixa de ser o que o dono montou. A tolerância de 0,004 cobre a diferença de
    /// f32 para f64 SÓ nestes pontos: o grão é um `fract` de um produto grande e, em 0,6% dos
    /// pixels, o f32 (que é o que a placa de vídeo faz) cai do outro lado. Não troque os pontos
    /// sem conferir.
    #[test]
    fn malha_igual_a_da_receita() {
        assert!(perto(cor_da_malha(0.5, 0.5, 34., 34., 0.0, None), [0.1286, 0.2157, 0.3552]));
        assert!(perto(cor_da_malha(17.5, 17.5, 34., 34., 0.0, None), [0.4586, 0.6263, 0.8647]));
        assert!(perto(cor_da_malha(10.5, 20.5, 34., 34., 0.0, None), [0.3082, 0.4752, 0.7697]));
        assert!(perto(cor_da_malha(30.5, 5.5, 34., 34., 7.5, None), [0.0000, 0.0170, 0.1344]));
        assert!(perto(cor_da_malha(40.5, 12.5, 68., 68., 23.0, None), [0.1333, 0.2366, 0.4614]));
        assert!(perto(cor_da_malha(10.5, 20.5, 34., 34., 0.0, Some((25.5, 8.5))), [0.3309, 0.5000, 0.7982]));
        assert!(perto(cor_da_malha(60.5, 60.5, 68., 68., 11.0, Some((50.0, 50.0))), [0.2631, 0.3581, 0.4830]));
    }

    #[test]
    fn quadro_montado_uma_vez_da_a_mesma_cor() {
        let quadro = Quadro::em(7.5);
        for (x, y) in [(0.5, 0.5), (12.5, 30.5), (33.5, 2.5)] {
            assert_eq!(cor_no_ponto(&quadro, x, y, 34., 34., None), cor_da_malha(x, y, 34., 34., 7.5, None));
        }
    }

    #[test]
    fn malha_anda_com_o_tempo_e_clareia_sob_o_ponteiro() {
        let parado = cor_da_malha(10.5, 20.5, 34., 34., 0., None);
        assert_ne!(parado, cor_da_malha(10.5, 20.5, 34., 34., 6., None));
        let com_luz = cor_da_malha(10.5, 20.5, 34., 34., 0., Some((10.5, 20.5)));
        assert!(com_luz.iter().zip(parado).all(|(c, p)| *c > p), "{com_luz:?} {parado:?}");
        // Toda cor fica entre 0 e 1, em qualquer ponto e em qualquer hora.
        for t in [0., 3.3, 91., 4_000.] {
            for (x, y) in [(0.5, 0.5), (33.5, 33.5), (17., 17.), (0.5, 33.5)] {
                assert!(cor_da_malha(x, y, 34., 34., t, Some((30., 2.))).iter().all(|c| (0. ..=1.).contains(c)));
            }
        }
    }

    #[test]
    fn canto_arredondado_com_borda_suave() {
        // O meio está dentro; o canto do quadrado, fora; a borda reta, pela metade.
        assert_eq!(cobertura_do_canto(17., 17., 34., 34., 10., 1.), 1.);
        assert_eq!(cobertura_do_canto(0.5, 0.5, 34., 34., 10., 1.), 0.);
        assert_eq!(cobertura_do_canto(17., 0., 34., 34., 10., 1.), 0.5);
        // No arco, o valor cai aos poucos de dentro para fora.
        let (dentro, meio, fora) = (
            cobertura_do_canto(4.5, 4.5, 34., 34., 10., 1.),
            cobertura_do_canto(3.2, 3.2, 34., 34., 10., 1.),
            cobertura_do_canto(2.0, 2.0, 34., 34., 10., 1.),
        );
        assert!(dentro == 1. && fora == 0. && meio > 0. && meio < 1., "{dentro} {meio} {fora}");
        // Raio maior que a metade do lado vira círculo, sem estourar.
        assert_eq!(cobertura_do_canto(17., 17., 34., 34., 99., 1.), 1.);
    }
}
