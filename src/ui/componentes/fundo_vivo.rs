//! A conta do fundo animado atrás da lista (Configurações › Aparência › Fundo animado): as duas
//! receitas do protótipo (`tarefas/linear/vitral-linear.html`, feitas no Shader Builder da
//! 21st.dev), "Smoke" e "Mesh drift", com os uniformes de lá. O Vitral não tem WebGL: a mesma conta
//! do fragment shader roda aqui numa grade baixa (um ponto a cada `px_por_texel` px) e a placa a
//! estica com filtro linear, como a chapa de metal (`metal.rs`, que pinta e guarda a imagem). O
//! grão, que é por pixel, vai por cima como ladrilho (o fino do fosco, que é o desta força).
//!
//! O que fica de fora da receita, por não aparecer nesta grade: o desfoque de 5 pontos (0,002 e
//! 0,004 do lado menor, menos de 2 px) e o ponteiro (desligado no protótipo, a pedido).

/// Qual das duas contas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Desenho {
    /// Fumaça: ruído fractal dobrado duas vezes sobre si mesmo, pintado pela rampa das cores.
    Fumaca,
    /// Bolhas: a média das cores, pesada pela distância a cada uma, andando em órbitas.
    Bolhas,
}

/// Os uniformes de uma receita (os nomes da receita entre crases).
#[derive(Clone, Copy, Debug)]
pub struct Receita {
    pub desenho: Desenho,
    /// Quantos segundos da receita por segundo do relógio (`vel`; negativo anda para trás).
    pub velocidade: f32,
    /// `u_scale`, `u_intensity`, `u_warp` e `u_detail`.
    pub escala: f32,
    pub intensidade: f32,
    pub dobra: f32,
    pub detalhe: f32,
    /// `u_contrast`, `u_saturation`, `u_hue` (em radianos) e `u_vignette`.
    pub contraste: f32,
    pub saturacao: f32,
    pub matiz: f32,
    pub vinheta: f32,
    /// `u_seed`, `u_rotate` (radianos), `u_drift` e `u_offset`.
    pub semente: f32,
    pub giro: f32,
    pub deriva: f32,
    pub deslocamento: (f32, f32),
    /// O lado da grade, em px de desenho: a fumaça é larga e lenta (o protótipo já a desenha na
    /// metade da resolução).
    pub px_por_texel: f32,
}

/// O Smoke da receita de 25/11/2026 (o protótipo zera a rotação de matiz de 172°, que viraria o
/// azul do tema em laranja).
pub const SMOKE: Receita = Receita {
    desenho: Desenho::Fumaca,
    velocidade: -0.73,
    escala: 0.50,
    intensidade: 0.,
    dobra: 0.,
    detalhe: 1.54,
    contraste: 1.05,
    saturacao: 1.,
    matiz: 0.,
    vinheta: 0.21,
    semente: 4014.,
    giro: 2.55,
    deriva: 0.12,
    deslocamento: (-0.20, 0.38),
    px_por_texel: 12.,
};

/// O Mesh drift do protótipo (é outra receita que a do botão do menu, `malha.rs`).
pub const MESH: Receita = Receita {
    desenho: Desenho::Bolhas,
    velocidade: 0.94,
    escala: 1.50,
    intensidade: 0.20,
    dobra: 0.18,
    detalhe: 1.66,
    contraste: 1.14,
    saturacao: 0.84,
    matiz: 0.03,
    vinheta: 0.34,
    semente: 8816.,
    giro: 0.33,
    deriva: 0.05,
    deslocamento: (-0.13, -0.12),
    px_por_texel: 8.,
};

/// As cores da receita do Smoke (o roxo escuro, #02010A → #916BBF), para o "Da receita".
pub const CORES_DO_SMOKE: [[f32; 3]; 4] =
    [[0.008, 0.004, 0.039], [0.016, 0.020, 0.180], [0.239, 0.173, 0.553], [0.569, 0.420, 0.749]];

fn fracao(x: f32) -> f32 {
    x - x.floor()
}

fn misturar(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn degrau_suave(de: f32, ate: f32, x: f32) -> f32 {
    let t = ((x - de) / (ate - de)).clamp(0., 1.);
    t * t * (3. - 2. * t)
}

/// O `hash21` da receita.
fn hash21(x: f32, y: f32) -> f32 {
    let (x, y) = (fracao(x * 234.34), fracao(y * 435.345));
    let d = x * (x + 34.23) + y * (y + 34.23);
    fracao((x + d) * (y + d))
}

/// O `noise` da receita: ruído de valor, com o degrau suave entre os quatro cantos.
fn ruido(x: f32, y: f32) -> f32 {
    let (ix, iy) = (x.floor(), y.floor());
    let (fx, fy) = (x - ix, y - iy);
    let (ux, uy) = (fx * fx * (3. - 2. * fx), fy * fy * (3. - 2. * fy));
    let embaixo = misturar(hash21(ix, iy), hash21(ix + 1., iy), ux);
    let em_cima = misturar(hash21(ix, iy + 1.), hash21(ix + 1., iy + 1.), ux);
    misturar(embaixo, em_cima, uy)
}

/// O `fbm` da receita: cinco oitavas.
fn fbm(mut x: f32, mut y: f32) -> f32 {
    let (mut v, mut a) = (0., 0.5);
    for _ in 0..5 {
        v += a * ruido(x, y);
        x = x * 2.03 + 17.;
        y = y * 2.03 + 9.2;
        a *= 0.5;
    }
    v
}

/// A `palette` da receita com quatro cores: a rampa do escuro ao claro, com degrau suave.
fn rampa(cores: &[[f32; 3]; 4], x: f32) -> [f32; 3] {
    let f = x.clamp(0., 1.) * 3.;
    let mut cor = cores[0];
    for (i, proxima) in cores.iter().enumerate().skip(1) {
        let t = degrau_suave(0., 1., (f - (i - 1) as f32).clamp(0., 1.));
        for c in 0..3 {
            cor[c] = misturar(cor[c], proxima[c], t);
        }
    }
    cor
}

/// O `shade` da fumaça: dois dobramentos de domínio e a rampa.
fn fumaca(receita: &Receita, cores: &[[f32; 3]; 4], x: f32, y: f32, t: f32) -> [f32; 3] {
    let dobra = 2. + receita.intensidade * 4.;
    let (qx, qy) = (fbm(x + t * 0.08, y + t * 0.08), fbm(x + 5.2 - t * 0.06, y + 1.3 - t * 0.06));
    let (dx, dy) = (x + dobra * qx, y + dobra * qy);
    let (rx, ry) = (fbm(dx + 1.7, dy + 9.2), fbm(dx + 8.3, dy + 2.8));
    rampa(cores, fbm(x + 3. * rx + receita.semente, y + 3. * ry + receita.semente))
}

/// As órbitas das quatro bolhas num instante (o mesmo para todos os pontos do quadro).
fn bolhas(receita: &Receita, t: f64) -> [(f32, f32); 4] {
    let alcance = f64::from(0.45 + receita.intensidade * 0.35);
    let semente = f64::from(receita.semente);
    let mut b = [(0., 0.); 4];
    for (i, bolha) in b.iter_mut().enumerate() {
        let i = i as f64;
        *bolha = (
            ((t * (0.21 + i * 0.071) + i * 2.4 + semente).sin() * alcance) as f32,
            ((t * (0.17 + i * 0.093) + i * 1.7).cos() * alcance) as f32,
        );
    }
    b
}

/// O `shade` das bolhas: a média das cores, pesada pela distância a cada bolha, com um piso da
/// primeira.
fn media_das_bolhas(cores: &[[f32; 3]; 4], orbitas: &[(f32, f32); 4], x: f32, y: f32) -> [f32; 3] {
    let mut soma = [cores[0][0] * 0.15, cores[0][1] * 0.15, cores[0][2] * 0.15];
    let mut total = 0.15;
    for (cor, (bx, by)) in cores.iter().zip(orbitas) {
        let peso = (-((x - bx).powi(2) + (y - by).powi(2)) * 6.).exp();
        for c in 0..3 {
            soma[c] += cor[c] * peso;
        }
        total += peso;
    }
    [soma[0] / total, soma[1] / total, soma[2] / total]
}

/// O `hueRotate` da receita (pelo YIQ).
fn girar_matiz(c: [f32; 3], a: f32) -> [f32; 3] {
    let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
    let i = 0.596 * c[0] - 0.274 * c[1] - 0.322 * c[2];
    let q = 0.211 * c[0] - 0.523 * c[1] + 0.312 * c[2];
    let (ca, sa) = (a.cos(), a.sin());
    let (i, q) = (i * ca - q * sa, i * sa + q * ca);
    [y + 0.956 * i + 0.621 * q, y - 0.272 * i - 0.647 * q, y - 1.106 * i + 1.703 * q]
}

/// A cor (de 0 a 1, sem o grão) do ponto (`fx`, `fy`) de uma caixa de `largura` por `altura`, no
/// sentido do `gl_FragCoord` (origem embaixo à esquerda), no instante `t` da receita (os segundos
/// do relógio vezes a velocidade). `orbitas` só vale para as bolhas.
fn cor_no_ponto(
    receita: &Receita,
    cores: &[[f32; 3]; 4],
    orbitas: &[(f32, f32); 4],
    fx: f32,
    fy: f32,
    largura: f32,
    altura: f32,
    t: f32,
) -> [f32; 3] {
    let menor = largura.min(altura).max(1.);
    let (mut x, mut y) = ((fx - 0.5 * largura) / menor * receita.escala, (fy - 0.5 * altura) / menor * receita.escala);
    // O `mat2(cr, -sr, sr, cr) * p` do GLSL (por colunas).
    let (cr, sr) = (receita.giro.cos(), receita.giro.sin());
    (x, y) = (cr * x + sr * y, -sr * x + cr * y);
    x += receita.deslocamento.0 + receita.deriva * (t * 0.31).sin();
    y += receita.deslocamento.1 + receita.deriva * (t * 0.23).cos();
    if receita.dobra > 0. {
        let (dx, dy) = (x * receita.detalhe, y * receita.detalhe);
        let (wx, wy) = (fbm(dx + receita.semente, dy + receita.semente), fbm(dx + 5.2, dy + 1.3));
        x += receita.dobra * (wx - 0.5);
        y += receita.dobra * (wy - 0.5);
    }
    let mut cor = match receita.desenho {
        Desenho::Fumaca => fumaca(receita, cores, x, y, t),
        Desenho::Bolhas => media_das_bolhas(cores, orbitas, x, y),
    };
    for c in &mut cor {
        *c = (*c - 0.5) * receita.contraste + 0.5;
    }
    if (receita.saturacao - 1.).abs() > 0.0001 {
        let luma = 0.299 * cor[0] + 0.587 * cor[1] + 0.114 * cor[2];
        for c in &mut cor {
            *c = misturar(luma, *c, receita.saturacao);
        }
    }
    if receita.matiz.abs() > 0.0001 {
        cor = girar_matiz(cor, receita.matiz);
    }
    let do_centro = (fx / largura - 0.5).hypot(fy / altura - 0.5) * std::f32::consts::SQRT_2;
    let vinheta = 1. - receita.vinheta * degrau_suave(0.35, 1., do_centro);
    cor.map(|c| c * vinheta)
}

/// O centro do texel `i`, em px da caixa: o texel 0 é a folga de fora (o filtro linear puxaria o
/// vizinho), o 1 é o primeiro de dentro.
pub fn centro_do_texel(i: u32, px_por_texel: f32) -> f32 {
    (i as f32 - 0.5) * px_por_texel
}

/// Quantos texels cobrem `lado` px, com um de folga em cada ponta.
pub fn texels_para(lado: f32, px_por_texel: f32) -> u32 {
    (lado / px_por_texel).ceil().max(1.) as u32 + 2
}

/// O campo inteiro em BGRA opaco, `colunas * linhas * 4` bytes, a linha de cima primeiro, para uma
/// caixa de `largura` por `altura` px no instante `segundos` do relógio.
pub fn pintar(
    receita: &Receita,
    cores: &[[f32; 3]; 4],
    colunas: u32,
    linhas: u32,
    largura: f32,
    altura: f32,
    segundos: f64,
) -> Vec<u8> {
    let t = segundos * f64::from(receita.velocidade);
    let orbitas = bolhas(receita, t);
    // As órbitas em f64 (em f32 o relógio anda em degraus depois de um dia aberto); o resto da
    // conta em f32, como no shader (o relógio só anda com o fundo vivo).
    let t = t as f32;
    let mut bgra = Vec::with_capacity((colunas * linhas * 4) as usize);
    for linha in 0..linhas {
        let fy = altura - centro_do_texel(linha, receita.px_por_texel);
        for coluna in 0..colunas {
            let fx = centro_do_texel(coluna, receita.px_por_texel);
            let cor = cor_no_ponto(receita, cores, &orbitas, fx, fy, largura, altura, t);
            let canal = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
            bgra.extend_from_slice(&[canal(cor[2]), canal(cor[1]), canal(cor[0]), 255]);
        }
    }
    bgra
}

#[cfg(test)]
mod testes {
    use super::*;

    const ESCURAS: [[f32; 3]; 4] = [[0.0035, 0.0106, 0.0447], [0.0071, 0.0212, 0.0894], [0.0118, 0.0353, 0.149], [0.0515, 0.1242, 0.3362]];

    #[test]
    fn a_rampa_vai_da_primeira_a_ultima_cor() {
        assert_eq!(rampa(&ESCURAS, 0.), ESCURAS[0]);
        assert_eq!(rampa(&ESCURAS, 1.), ESCURAS[3]);
        assert_eq!(rampa(&ESCURAS, 2. / 3.), ESCURAS[2]);
    }

    #[test]
    fn o_ruido_fica_entre_zero_e_um() {
        for i in 0..400 {
            let (x, y) = (i as f32 * 0.37 - 40., i as f32 * 0.91 + 4014.);
            let v = fbm(x, y);
            assert!((0. ..=1.).contains(&v), "fbm({x}, {y}) = {v}");
        }
    }

    /// Com as cores do tema, a fumaça nunca passa da mais clara (a vinheta e o contraste de 1,05 só
    /// escurecem ou mexem um pouco): o fundo não clareia o texto da lista.
    #[test]
    fn a_fumaca_do_tema_fica_no_escuro() {
        let (colunas, linhas) = (40, 24);
        let bgra = pintar(&SMOKE, &ESCURAS, colunas, linhas, 480., 288., 12.5);
        assert_eq!(bgra.len(), (colunas * linhas * 4) as usize);
        let mais_claro = bgra.chunks(4).map(|p| p[0].max(p[1]).max(p[2])).max().unwrap();
        assert!(mais_claro <= 92, "o mais claro saiu {mais_claro}");
        // E ela anda: outro instante, outro campo.
        assert_ne!(bgra, pintar(&SMOKE, &ESCURAS, colunas, linhas, 480., 288., 20.));
    }

    #[test]
    fn as_bolhas_ficam_entre_as_cores_da_receita() {
        let bgra = pintar(&MESH, &CORES_DO_SMOKE, 30, 20, 360., 240., 3.);
        let mais_claro = bgra.chunks(4).map(|p| p[0].max(p[1]).max(p[2])).max().unwrap();
        assert!(mais_claro < 230, "{mais_claro}");
        assert!(bgra.chunks(4).all(|p| p[3] == 255));
    }

    /// O custo de um campo da janela da bancada (1392 por 876) no binário de teste; o de verdade é
    /// o otimizado. `cargo test custo_do_fundo -- --ignored --nocapture`.
    #[test]
    #[ignore]
    fn custo_do_fundo() {
        for receita in [SMOKE, MESH] {
            let (l, a) = (1392., 876.);
            let (c, li) = (texels_para(l, receita.px_por_texel), texels_para(a, receita.px_por_texel));
            let inicio = std::time::Instant::now();
            let _ = pintar(&receita, &ESCURAS, c, li, l, a, 30.);
            println!("{:?}: {c}x{li} em {:?}", receita.desenho, inicio.elapsed());
        }
    }
}
