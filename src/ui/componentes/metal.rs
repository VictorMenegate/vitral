//! O motor do metal líquido: a chapa atrás da janela e os recortes dela (as aberturas).
//!
//! O dono colou a receita "Mesh drift" (cinco bolhas azuis derivando, com grão e uma luz que segue
//! o ponteiro) e pediu "tenta focar em vez de glass esse metal líquido" (12/11/2026). O botão do
//! menu faz a conta ponto a ponto (`malha.rs`); para a janela inteira isso seria um milhão de
//! quadrados por quadro. Aqui a MESMA conta roda numa grade de 1 ponto a cada 8 px, vira uma
//! imagem pequena (uns 20 mil pontos, 80 KB) e a placa de vídeo a estica com filtro linear: as
//! bolhas são largas e macias, e a diferença para a conta inteira fica abaixo de um nível de cor.
//! O grão, que é por pixel, vai por cima como um ladrilho fixo de 256 px.
//!
//! Uma chapa só por janela. O `fundo` (o primeiro filho da Janela) calcula o quadro, guarda a
//! imagem no Global `Metal` e pinta a chapa inteira; cada `abertura` pinta um recorte da mesma
//! imagem, sem conta nova nem subida nova para a placa. Este é o único arquivo de `src/ui` que
//! chama `paint_image` e `drop_image` (o teste `metal_so_pelo_motor`, no tema, trava isso).
//!
//! Regras de quem usa:
//! - Texto nunca vai no metal cru. Sobre `Cores::veu_de_placa()`, só `forte` e `texto`; sobre
//!   `Cores::veu_de_botao()`, só ícone branco.
//! - Nenhuma abertura dentro de view com `.cached()` (a dica do kit usa): a textura troca a cada
//!   batida, e o replay da pintura mostraria a que já saiu do atlas.
//! - O `aro_de_metal` precisa de um miolo OPACO pintado por cima: não existe contorno de imagem
//!   no GPUI.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use gpui_kit::*;

use super::malha::{
    BRILHO, CONTRASTE, CORES, DESLOCAMENTO, ESCALA, FORCA_DO_PONTEIRO, GRAO, Quadro, RAIO_DO_PONTEIRO, SATURACAO, SEMENTE,
    VINHETA, degrau_suave, grao,
};
use crate::{tema::Cores, ui::movimento};

/// Um ponto do campo a cada tantos px de desenho (nos dois sentidos). Começou em 6; o portão de
/// custo (12/11/2026) levou a 8: no binário de debug o campo da janela da bancada (1392 por 876)
/// custava 4,2 ms com 6 e ficou em 2,4 ms com 8, sem diferença à vista (as bolhas são largas).
const PX_POR_TEXEL: f32 = 8.;
/// O lado do ladrilho do grão, em pixels do aparelho.
const LADO_DO_GRAO: u32 = 256;
/// Quanto do grão da receita vai nas áreas grandes de metal à vista (a doca da conversa).
const GRAO_LEVE: f32 = 0.3;
/// Fora do ritmo vivo o tempo do campo anda a 40% (o metal deriva devagar); vivo, a 100%. Com a
/// política de hoje (`movimento::QPS_EM_REPOUSO` em 0) o metal fica parado em repouso, e este
/// andamento vale só enquanto outra coisa mantém o laço batendo (o ponteiro parado em cima do
/// botão do menu, o menu aberto).
const ANDAMENTO_EM_REPOUSO: f64 = 0.4;
/// A fase com que o metal aparece parado (movimento desligado, captura): neste instante há bolha
/// no pé da conversa e debaixo da lista.
pub const FASE_INICIAL_DO_METAL: f64 = 66.5;
/// A cor mais clara que o campo entrega: a bolha clara no centro, depois do contraste, da
/// saturação e do brilho, com a luz do ponteiro em cima. É contra ela que o teste de contraste do
/// tema roda (`contraste_dos_tokens_no_pior_caso`), e o `teto_cobre_o_campo` daqui a confere.
#[cfg(test)]
pub const TETO_DO_METAL: u32 = 0xacd9ff;
/// O peso fixo da cor 0 na média (o piso do `shade()` da receita).
const PISO: f32 = 0.15;

// ---- a conta (pura, com teste) ----

/// As cinco cores da receita já com o contraste, a saturação e o brilho. Os três são contas afins
/// (multiplicar e somar) e a cor de um ponto é uma média pesada das cinco: aplicar na paleta, uma
/// vez, dá o mesmo que aplicar em cada ponto, e poupa a conta em 20 mil pontos por quadro.
fn paleta_acabada() -> [[f32; 3]; 5] {
    let mut paleta = CORES;
    for cor in &mut paleta {
        for canal in cor.iter_mut() {
            *canal = (*canal - 0.5) * CONTRASTE + 0.5;
        }
        let luma = 0.299 * cor[0] + 0.587 * cor[1] + 0.114 * cor[2];
        for canal in cor.iter_mut() {
            *canal = luma + (*canal - luma) * SATURACAO + BRILHO;
        }
    }
    paleta
}

/// O centro do texel `i` (coluna ou linha), em px do retângulo da janela: o texel 0 é a folga de
/// fora (o atlas não tem folga e o filtro linear puxaria o vizinho), o 1 é o primeiro de dentro.
fn centro_do_texel(i: u32) -> f32 {
    (i as f32 - 0.5) * PX_POR_TEXEL
}

/// Quantas colunas (ou linhas) de campo cobrem `lado` px, com um texel de folga em cada ponta.
fn texels_para(lado: f32) -> u32 {
    (lado / PX_POR_TEXEL).ceil().max(1.) as u32 + 2
}

/// A vinheta da receita para cada texel de um campo de `colunas` por `linhas` sobre um retângulo
/// de `largura` por `altura` px: só muda quando o tamanho muda, então fica guardada.
pub fn tabela_da_vinheta(colunas: u32, linhas: u32, largura: f32, altura: f32) -> Vec<f32> {
    let mut tabela = Vec::with_capacity((colunas * linhas) as usize);
    for linha in 0..linhas {
        let fy = (altura - centro_do_texel(linha)) / altura - 0.5;
        for coluna in 0..colunas {
            let fx = centro_do_texel(coluna) / largura - 0.5;
            tabela.push(1. - VINHETA * degrau_suave(0.35, 1., fx.hypot(fy) * std::f32::consts::SQRT_2));
        }
    }
    tabela
}

/// O campo da receita (sem o grão) em `colunas` por `linhas` texels, para um retângulo de
/// `largura` por `altura` px: BGRA opaco, `colunas * linhas * 4` bytes, a linha de cima primeiro.
/// É a conta da `cor_sem_grao` na forma separável: a gaussiana de cada bolha é um produto de uma
/// parte que só depende da coluna por outra que só depende da linha, então as exponenciais saem
/// uma vez por coluna e por linha (umas 1.500 no lugar de 100 mil). `ponteiro` é a posição da luz
/// em px do retângulo, com o y para CIMA (como na `cor_sem_grao`); `vinheta`, a tabela acima.
///
/// O laço de dentro é escrito à mão (fatias, índices, nada de iterador nem `clamp`): o app
/// compila sem otimização no debug, onde cada chamada pequena custa uma chamada de verdade.
pub fn pintar_campo(
    colunas: u32,
    linhas: u32,
    largura: f32,
    altura: f32,
    quadro: &Quadro,
    ponteiro: Option<(f32, f32)>,
    vinheta: &[f32],
) -> Vec<u8> {
    let (colunas, linhas) = (colunas as usize, linhas as usize);
    let menor = largura.min(altura).max(1.);
    let paleta = paleta_acabada();
    let [c0, c1, c2, c3, c4] = paleta;
    // O piso: a cor 0 entra com peso fixo, além da bolha dela.
    let (piso_r, piso_g, piso_b) = (c0[0] * PISO, c0[1] * PISO, c0[2] * PISO);
    let (luz_x, luz_y) = ponteiro.map_or((0., 0.), |(x, y)| ((x - 0.5 * largura) / menor, (y - 0.5 * altura) / menor));
    let com_luz = ponteiro.is_some();
    let raio2 = RAIO_DO_PONTEIRO * RAIO_DO_PONTEIRO;

    // Por coluna: o peso de cada bolha em x e a distância em x até a luz.
    let mut em_x = vec![0f32; colunas * 5];
    let mut ate_a_luz_x = vec![0f32; colunas];
    for coluna in 0..colunas {
        let px = (centro_do_texel(coluna as u32) - 0.5 * largura) / menor;
        let qx = px * ESCALA + DESLOCAMENTO.0 + quadro.deriva.0;
        for bolha in 0..5 {
            let d = qx - quadro.bolhas[bolha].0;
            em_x[coluna * 5 + bolha] = (-6. * d * d).exp();
        }
        ate_a_luz_x[coluna] = px - luz_x;
    }
    let (em_x, ate_a_luz_x): (&[f32], &[f32]) = (&em_x, &ate_a_luz_x);

    let mut bgra = vec![255u8; colunas * linhas * 4];
    let saida: &mut [u8] = &mut bgra;
    let mut texel = 0;
    for linha in 0..linhas {
        // O y do shader cresce para cima.
        let py = (altura - centro_do_texel(linha as u32) - 0.5 * altura) / menor;
        let qy = py * ESCALA + DESLOCAMENTO.1 + quadro.deriva.1;
        let mut em_y = [0f32; 5];
        for bolha in 0..5 {
            let d = qy - quadro.bolhas[bolha].1;
            em_y[bolha] = (-6. * d * d).exp();
        }
        let [y0, y1, y2, y3, y4] = em_y;
        let dy = py - luz_y;
        let dy2 = dy * dy;
        let mut x = 0;
        for coluna in 0..colunas {
            let p0 = em_x[x] * y0;
            let p1 = em_x[x + 1] * y1;
            let p2 = em_x[x + 2] * y2;
            let p3 = em_x[x + 3] * y3;
            let p4 = em_x[x + 4] * y4;
            x += 5;
            // A média pesada, já com a vinheta.
            let fator = vinheta[texel] / (PISO + p0 + p1 + p2 + p3 + p4);
            let mut r = (piso_r + p0 * c0[0] + p1 * c1[0] + p2 * c2[0] + p3 * c3[0] + p4 * c4[0]) * fator;
            let mut g = (piso_g + p0 * c0[1] + p1 * c1[1] + p2 * c2[1] + p3 * c3[1] + p4 * c4[1]) * fator;
            let mut b = (piso_b + p0 * c0[2] + p1 * c1[2] + p2 * c2[2] + p3 * c3[2] + p4 * c4[2]) * fator;
            if com_luz {
                let dx = ate_a_luz_x[coluna];
                let d2 = dx * dx + dy2;
                if d2 < raio2 {
                    // 1 - smoothstep(0, raio, distância), vezes a força.
                    let t = d2.sqrt() / RAIO_DO_PONTEIRO;
                    let luz = (1. - t * t * (3. - 2. * t)) * FORCA_DO_PONTEIRO;
                    r += (0.18 + r * 0.12) * luz;
                    g += (0.18 + g * 0.12) * luz;
                    b += (0.18 + b * 0.12) * luz;
                }
            }
            // O `as u8` já corta em 0 e 255.
            let o = texel * 4;
            saida[o] = (b * 255. + 0.5) as u8;
            saida[o + 1] = (g * 255. + 0.5) as u8;
            saida[o + 2] = (r * 255. + 0.5) as u8;
            texel += 1;
        }
    }
    bgra
}

/// O ladrilho fixo do grão de filme, `lado` por `lado`: BGRA com alfa reto. A receita SOMA o
/// ruído à cor (de -0,11 a +0,11); por cima de uma imagem só dá para misturar, então o ruído
/// positivo vira branco e o negativo vira preto. Os dois fatores do alfa (1,5 e 2,8) foram
/// calibrados na bancada contra o botão do menu, que faz a soma de verdade: no azul fundo e no
/// médio (de 0,1 a 0,5 de claro, quase todo o campo) o desvio do grão fica a menos de 6% do dele;
/// na bolha clara o grão daqui sai uns 25% mais forte, puxado para o escuro.
pub fn pintar_grao(lado: u32) -> Vec<u8> {
    pintar_grao_com(lado, 1.)
}

/// O mesmo ladrilho com o grão na `forca` dada (1 é o da receita). Em área grande o grão cheio
/// vira chuvisco: a doca da conversa usa o leve.
pub fn pintar_grao_com(lado: u32, forca: f32) -> Vec<u8> {
    let mut bgra = Vec::with_capacity((lado * lado * 4) as usize);
    for y in 0..lado {
        for x in 0..lado {
            let ruido = (grao(x as f32 + 0.5 + SEMENTE * 17., y as f32 + 0.5 + SEMENTE * 31.) - 0.5) * GRAO * forca;
            let (tom, alfa) = if ruido > 0. { (255, ruido * 1.5) } else { (0, -ruido * 2.8) };
            bgra.extend_from_slice(&[tom, tom, tom, (alfa.min(1.) * 255. + 0.5) as u8]);
        }
    }
    bgra
}

/// Um BMP de 32 bits com alfa a partir de um BGRA: cabeçalho de arquivo (14 bytes) e o
/// `BITMAPV4HEADER` (108), com a altura NEGATIVA (a linha de cima primeiro) e as máscaras dizendo
/// onde está cada canal, seguido dos bytes como estão. É a porta de entrada sem dependência nova:
/// o `image` que o GPUI já compila decodifica isto e devolve os mesmos bytes.
pub fn bmp_de_bgra(largura: u32, altura: u32, bgra: &[u8]) -> Vec<u8> {
    const CABECALHOS: u32 = 14 + 108;
    debug_assert_eq!(bgra.len(), (largura * altura * 4) as usize);
    let mut bmp = Vec::with_capacity(CABECALHOS as usize + bgra.len());
    // O cabeçalho de arquivo: "BM", o tamanho, dois reservados e onde começam os pixels.
    bmp.extend_from_slice(b"BM");
    bmp.extend_from_slice(&(CABECALHOS + bgra.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&0u32.to_le_bytes());
    bmp.extend_from_slice(&CABECALHOS.to_le_bytes());
    // O BITMAPV4HEADER.
    bmp.extend_from_slice(&108u32.to_le_bytes());
    bmp.extend_from_slice(&(largura as i32).to_le_bytes());
    bmp.extend_from_slice(&(-(altura as i32)).to_le_bytes());
    bmp.extend_from_slice(&1u16.to_le_bytes()); // planos
    bmp.extend_from_slice(&32u16.to_le_bytes()); // bits por pixel
    bmp.extend_from_slice(&3u32.to_le_bytes()); // compressão 3: os canais pelas máscaras
    bmp.extend_from_slice(&(bgra.len() as u32).to_le_bytes());
    bmp.extend_from_slice(&[0; 16]); // resolução e paleta: sem uso
    for mascara in [0x00ff_0000u32, 0x0000_ff00, 0x0000_00ff, 0xff00_0000] {
        bmp.extend_from_slice(&mascara.to_le_bytes()); // vermelho, verde, azul, alfa
    }
    bmp.extend_from_slice(&[0; 52]); // espaço de cor, pontos de referência e gama: sem uso
    debug_assert_eq!(bmp.len(), CABECALHOS as usize);
    bmp.extend_from_slice(bgra);
    bmp
}

/// O retângulo onde um campo de tamanho `campo` tem de ficar para cobrir a `abertura` inteira,
/// centrado e sem deformar (o "cover" das imagens).
pub fn caixa_que_cobre(abertura: Bounds<Pixels>, campo: Size<Pixels>) -> Bounds<Pixels> {
    let (largura, altura) = (campo.width.as_f32().max(1.), campo.height.as_f32().max(1.));
    let escala = (abertura.size.width.as_f32() / largura).max(abertura.size.height.as_f32() / altura);
    Bounds::centered_at(abertura.center(), size(px(largura * escala), px(altura * escala)))
}

// ---- o estado ----

/// O metal de cada janela e o ladrilho do grão (um só, para sempre: id estável, uma subida por
/// janela).
#[derive(Default)]
struct Metal {
    janelas: HashMap<WindowId, DaJanela>,
    grao: Option<Arc<RenderImage>>,
    /// O grão leve (um terço do da receita), para as áreas grandes de metal à vista.
    grao_leve: Option<Arc<RenderImage>>,
    /// O grão já foi tentado (se a imagem falhou, não tenta de novo a cada quadro).
    grao_tentado: bool,
}

impl Global for Metal {}

/// O que decide se o campo precisa ser refeito: o tamanho, o instante e onde está a luz.
type Chave = (u32, u32, u32, u32, u64, Option<(i32, i32)>);

#[derive(Default)]
struct DaJanela {
    /// O quadro de agora: as aberturas leem daqui.
    imagem: Option<Arc<RenderImage>>,
    /// Onde o campo inteiro (com a folga) cai na janela.
    onde: Bounds<Pixels>,
    chave: Option<Chave>,
    vinheta: Vec<f32>,
    /// Para que tamanho a `vinheta` foi feita.
    vinheta_de: (u32, u32, u32, u32),
    /// O tempo do campo e a última fase lida do laço.
    tempo: f64,
    fase_vista: f64,
    /// Se o metal está no ritmo vivo (um chat à vista trabalhando).
    vivo: bool,
}

/// O tempo gasto para fazer os campos (conta, BMP e decodificação) e quantos foram, para o
/// `VITRAL_QUADROS`.
static NANOS_DOS_CAMPOS: AtomicU64 = AtomicU64::new(0);
static CAMPOS: AtomicU32 = AtomicU32::new(0);

/// O tempo médio de um campo desde a última pergunta, e quantos foram feitos (o `VITRAL_QUADROS`
/// escreve junto dos quadros por segundo). `None` se nenhum foi feito.
pub fn custo_do_campo() -> Option<(u32, Duration)> {
    let quantos = CAMPOS.swap(0, Ordering::Relaxed);
    let nanos = NANOS_DOS_CAMPOS.swap(0, Ordering::Relaxed);
    (quantos > 0).then(|| (quantos, Duration::from_nanos(nanos / u64::from(quantos))))
}

/// A imagem que a placa recebe, a partir do BGRA: passa pelo BMP e pelo decodificador do GPUI.
fn imagem_de_bgra(largura: u32, altura: u32, bgra: &[u8], svg: SvgRenderer) -> Option<Arc<RenderImage>> {
    static AVISADO: AtomicBool = AtomicBool::new(false);
    match Image::from_bytes(ImageFormat::Bmp, bmp_de_bgra(largura, altura, bgra)).to_image_data(svg) {
        Ok(imagem) => Some(imagem),
        Err(erro) => {
            if !AVISADO.swap(true, Ordering::Relaxed) {
                eprintln!("vitral: o metal não virou imagem: {erro:#}");
            }
            None
        }
    }
}

/// A janela fechou: o metal dela sai do mapa (a textura some com o atlas da janela).
pub fn esquecer_janela(janela: WindowId, cx: &mut App) {
    if cx.has_global::<Metal>() {
        cx.global_mut::<Metal>().janelas.remove(&janela);
    }
}

// ---- os elementos ----

/// Como uma abertura enquadra a chapa.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Encaixe {
    /// Um buraco na casca: mostra exatamente o pedaço da chapa que está atrás dela (a doca).
    Continuo,
    /// A chapa inteira encolhida até cobrir a caixa, centrada: um botão mostra o miolo do campo,
    /// com as bolhas pequenas, como no botão do menu; uma placa, a faixa do meio.
    Cobrir,
}

/// O fundo da janela: a chapa de metal inteira, com o grão. Vai como PRIMEIRO filho da raiz da
/// janela (absoluto, sem clique) e é chamado no `render`: é aqui que o tempo do campo anda e o
/// próximo quadro é pedido. `trabalhando`: algum chat à vista está rodando (o metal corre).
/// `raios`: os cantos da janela.
pub fn fundo(trabalhando: bool, raios: Corners<Pixels>, window: &mut Window, cx: &mut App) -> impl IntoElement + use<> {
    let anima = movimento::anima(window, cx);
    let janela = window.window_handle().window_id();
    // O metal só anda com um chat à vista trabalhando. Ele já acordou com qualquer movimento do
    // ponteiro (e a luz seguia o ponteiro): isso redesenhava a janela inteira 20 vezes por segundo
    // sempre que o ponteiro mexia, e a rolagem engasgava (dono, 12/11/2026).
    let vivo = anima && trabalhando;
    let ritmo = if vivo { movimento::QPS_DO_LACO } else { movimento::QPS_EM_REPOUSO };
    let fase = movimento::laco_a(window, cx, ritmo);
    let estado = cx.default_global::<Metal>().janelas.entry(janela).or_default();
    // O campo não usa a fase direto: soma o quanto ela andou, mais devagar em repouso. Assim a
    // troca de ritmo não dá salto.
    let passo = (fase - estado.fase_vista).clamp(0., 0.25);
    estado.fase_vista = fase;
    if anima {
        estado.tempo += passo * if vivo { 1. } else { ANDAMENTO_EM_REPOUSO };
    }
    estado.vivo = vivo;
    let tempo = estado.tempo;

    canvas(
        |_, _, _| {},
        move |limites, _, window, cx| pintar_o_fundo(limites, raios, tempo, vivo, window, cx),
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn pintar_o_fundo(limites: Bounds<Pixels>, raios: Corners<Pixels>, tempo: f64, vivo: bool, window: &mut Window, cx: &mut App) {
    let (largura, altura) = (limites.size.width.as_f32(), limites.size.height.as_f32());
    if largura < 1. || altura < 1. {
        return;
    }
    let raios = raios.clamp_radii_for_quad_size(limites.size);
    let (colunas, linhas) = (texels_para(largura), texels_para(altura));
    // A luz mora no campo: a posição do ponteiro em texel inteiro (é o que entra na chave). Só
    // com o metal vivo (aí o campo já é refeito a cada batida) e o ponteiro dentro da janela:
    // parado, o ponteiro não refaz o campo.
    let ponteiro = Some(window.mouse_position())
        .filter(|p| vivo && window.is_window_hovered() && limites.contains(p))
        .map(|p| {
            let (x, y) = ((p.x - limites.origin.x).as_f32(), (p.y - limites.origin.y).as_f32());
            ((x / PX_POR_TEXEL).floor() as i32 + 1, (y / PX_POR_TEXEL).floor() as i32 + 1)
        });
    let chave: Chave = (colunas, linhas, largura.to_bits(), altura.to_bits(), tempo.to_bits(), ponteiro);
    let janela = window.window_handle().window_id();
    let svg = cx.svg_renderer();
    let metal = cx.default_global::<Metal>();
    if !metal.grao_tentado {
        metal.grao_tentado = true;
        metal.grao = imagem_de_bgra(LADO_DO_GRAO, LADO_DO_GRAO, &pintar_grao(LADO_DO_GRAO), svg.clone());
        metal.grao_leve =
            imagem_de_bgra(LADO_DO_GRAO, LADO_DO_GRAO, &pintar_grao_com(LADO_DO_GRAO, GRAO_LEVE), svg.clone());
    }
    let grao = metal.grao.clone();
    let estado = metal.janelas.entry(janela).or_default();
    // O campo inteiro, com um texel de folga para fora de cada lado.
    let folga = px(PX_POR_TEXEL);
    let onde = Bounds::new(
        point(limites.origin.x - folga, limites.origin.y - folga),
        size(px(colunas as f32 * PX_POR_TEXEL), px(linhas as f32 * PX_POR_TEXEL)),
    );
    estado.onde = onde;
    // Igual ao guardado, a imagem de antes serve: parado, o custo é zero.
    if estado.chave != Some(chave) || estado.imagem.is_none() {
        let inicio = Instant::now();
        let vinheta_de = (colunas, linhas, largura.to_bits(), altura.to_bits());
        if estado.vinheta_de != vinheta_de {
            estado.vinheta = tabela_da_vinheta(colunas, linhas, largura, altura);
            estado.vinheta_de = vinheta_de;
        }
        let quadro = Quadro::em(tempo + FASE_INICIAL_DO_METAL);
        let luz = ponteiro.map(|(coluna, linha)| (centro_do_texel(coluna as u32), altura - centro_do_texel(linha as u32)));
        let bgra = pintar_campo(colunas, linhas, largura, altura, &quadro, luz, &estado.vinheta);
        let nova = imagem_de_bgra(colunas, linhas, &bgra, svg);
        NANOS_DOS_CAMPOS.fetch_add(inicio.elapsed().as_nanos() as u64, Ordering::Relaxed);
        CAMPOS.fetch_add(1, Ordering::Relaxed);
        // Se falhar, fica a imagem anterior (e a chave velha: tenta de novo no próximo quadro).
        if let Some(nova) = nova {
            estado.chave = Some(chave);
            // A ordem importa: a nova entra no atlas ANTES de a velha sair, para a textura do
            // atlas nunca ficar sem ninguém (seria recriada a cada quadro).
            let _ = window.paint_image(limites, onde, raios, nova.clone(), 0, false);
            if let Some(velha) = estado.imagem.replace(nova) {
                let _ = window.drop_image(velha);
            }
            pintar_o_grao(limites, raios, grao.as_ref(), window);
            return;
        }
    }
    match estado.imagem.clone() {
        Some(imagem) => {
            let _ = window.paint_image(limites, onde, raios, imagem, 0, false);
        }
        // A reserva: sem imagem, um azul liso.
        None => window.paint_quad(fill(limites, Cores::destaque_ativo()).corner_radii(raios)),
    }
    pintar_o_grao(limites, raios, grao.as_ref(), window);
}

/// O grão por cima de `limites`: um texel do ladrilho por pixel do aparelho. Uma grade de
/// ladrilhos CENTRADA na caixa, para as peças das pontas terem pelo menos meio ladrilho e o canto
/// arredondado caber inteiro numa peça; cada peça leva o raio só nos cantos que são os da caixa.
fn pintar_o_grao(limites: Bounds<Pixels>, raios: Corners<Pixels>, grao: Option<&Arc<RenderImage>>, window: &mut Window) {
    let Some(grao) = grao else { return };
    let escala = window.scale_factor().max(0.25);
    // Tudo em pixel do aparelho, inteiro: fora do pixel o filtro linear borraria o grão.
    let no_pixel = |v: Pixels| (v.as_f32() * escala).round();
    let (esquerda, cima) = (no_pixel(limites.left()), no_pixel(limites.top()));
    let (direita, baixo) = (no_pixel(limites.right()), no_pixel(limites.bottom()));
    let (largura, altura) = (direita - esquerda, baixo - cima);
    if largura < 1. || altura < 1. {
        return;
    }
    let lado = LADO_DO_GRAO as f32;
    let (nx, ny) = ((largura / lado).ceil() as usize, (altura / lado).ceil() as usize);
    let x0 = esquerda - ((nx as f32 * lado - largura) / 2.).floor();
    let y0 = cima - ((ny as f32 * lado - altura) / 2.).floor();
    let caixa = Bounds::from_corners(point(px(esquerda / escala), px(cima / escala)), point(px(direita / escala), px(baixo / escala)));
    let zero = px(0.);
    for j in 0..ny {
        for i in 0..nx {
            let (x, y) = (x0 + i as f32 * lado, y0 + j as f32 * lado);
            let ladrilho = Bounds::from_corners(
                point(px(x / escala), px(y / escala)),
                point(px((x + lado) / escala), px((y + lado) / escala)),
            );
            let (primeira, ultima) = (i == 0, i == nx - 1);
            let (de_cima, de_baixo) = (j == 0, j == ny - 1);
            let cantos = Corners {
                top_left: if primeira && de_cima { raios.top_left } else { zero },
                top_right: if ultima && de_cima { raios.top_right } else { zero },
                bottom_left: if primeira && de_baixo { raios.bottom_left } else { zero },
                bottom_right: if ultima && de_baixo { raios.bottom_right } else { zero },
            };
            let _ = window.paint_image(ladrilho.intersect(&caixa), ladrilho, cantos, grao.clone(), 0, false);
        }
    }
}

/// Um recorte da chapa enchendo a caixa onde está (absoluto, sem clique), com o grão e, se houver,
/// o `veu` (o escuro por cima: `Cores::veu_de_placa()` debaixo de texto, `Cores::veu_de_botao()`
/// debaixo de ícone). `raios`: os cantos da caixa. Numa janela sem `fundo` (ou antes do primeiro
/// quadro), pinta a reserva: o azul liso, com o mesmo véu.
pub fn abertura(encaixe: Encaixe, veu: Option<Hsla>, raios: Corners<Pixels>) -> impl IntoElement {
    abertura_com_grao(encaixe, veu, raios, false)
}

/// A abertura das áreas grandes (a doca da conversa): o mesmo recorte, com o grão leve. Com o
/// grão cheio, numa faixa de 1000 px o metal lê como chuvisco, e não como degradê.
pub fn abertura_lisa(encaixe: Encaixe, veu: Option<Hsla>, raios: Corners<Pixels>) -> impl IntoElement {
    abertura_com_grao(encaixe, veu, raios, true)
}

/// O fosco: o grão leve do metal e um véu leitoso bem leve, enchendo o pai (absoluto, sem
/// clique). Vai por cima de um fundo meio transparente, para ele ler como vidro fosco e não como
/// um buraco na janela: sem desfoque do sistema, é o grão que diz que ali há uma superfície
/// (dono, 13/11/2026: "deixa um transparente fosco pra não ficar assim igual tá faltando"). Só
/// na base: o grão cheio, e o grão debaixo de texto corrido, viram chuvisco.
pub fn fosco(raios: Corners<Pixels>) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |limites, _, window, cx| {
            if limites.size.width < px(1.) || limites.size.height < px(1.) {
                return;
            }
            let raios = raios.clamp_radii_for_quad_size(limites.size);
            window.paint_quad(fill(limites, gpui_kit::rgba(0xffffff0a)).corner_radii(raios));
            let grao = cx.try_global::<Metal>().and_then(|metal| metal.grao_leve.clone());
            pintar_o_grao(limites, raios, grao.as_ref(), window);
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn abertura_com_grao(encaixe: Encaixe, veu: Option<Hsla>, raios: Corners<Pixels>, leve: bool) -> impl IntoElement {
    canvas(
        |_, _, _| {},
        move |limites, _, window, cx| {
            if limites.size.width < px(1.) || limites.size.height < px(1.) {
                return;
            }
            // O quad não corta o raio sozinho: com a pílula (999) num disco de 36 o véu sumia.
            let raios = raios.clamp_radii_for_quad_size(limites.size);
            let janela = window.window_handle().window_id();
            let (chapa, grao) = cx.try_global::<Metal>().map_or((None, None), |metal| {
                let grao = if leve { metal.grao_leve.clone() } else { metal.grao.clone() };
                (metal.janelas.get(&janela).and_then(|j| Some((j.imagem.clone()?, j.onde))), grao)
            });
            match chapa {
                Some((imagem, onde)) => {
                    let moldura = match encaixe {
                        Encaixe::Continuo => onde,
                        Encaixe::Cobrir => chapa_que_cobre(limites, onde),
                    };
                    let _ = window.paint_image(limites, moldura, raios, imagem, 0, false);
                    pintar_o_grao(limites, raios, grao.as_ref(), window);
                }
                None => window.paint_quad(fill(limites, Cores::destaque_ativo()).corner_radii(raios)),
            }
            if let Some(veu) = veu {
                window.paint_quad(fill(limites, veu).corner_radii(raios));
            }
        },
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

/// Onde a imagem da chapa (que tem um texel de folga em volta) tem de ficar para o campo de dentro
/// cobrir a `abertura`: a folga fica sempre para fora do recorte.
fn chapa_que_cobre(abertura: Bounds<Pixels>, onde: Bounds<Pixels>) -> Bounds<Pixels> {
    let folga = px(PX_POR_TEXEL);
    let campo = size((onde.size.width - folga * 2.).max(px(1.)), (onde.size.height - folga * 2.).max(px(1.)));
    let dentro = caixa_que_cobre(abertura, campo);
    dentro.dilate(folga * (dentro.size.width.as_f32() / campo.width.as_f32()))
}

/// O aro de metal: "é aqui" (a caixa com foco) e "espera você" (o pedido pendente). Vai como
/// PRIMEIRO filho de um `div().relative()`; o miolo, OPACO, vem depois, por cima, e o que sobra à
/// vista é a moldura de `espessura` em volta dele. `raio` é o do miolo; `t`, de 0 a 1, o quanto
/// o aro aparece (a opacidade vai no `div`: o `canvas` sozinho a ignora).
pub fn aro_de_metal(raio: Pixels, espessura: Pixels, t: f32) -> Div {
    div()
        .absolute()
        .top(-espessura)
        .left(-espessura)
        .right(-espessura)
        .bottom(-espessura)
        .opacity(t.clamp(0., 1.))
        .child(abertura(Encaixe::Cobrir, None, Corners::all(raio + espessura)))
}

#[cfg(test)]
mod testes {
    use std::sync::Arc;

    use gpui_kit::{Bounds, DevicePixels, Image, ImageFormat, Pixels, Rgba, SvgRenderer, point, px, rgb, size};

    use super::{
        BRILHO, CONTRASTE, CORES, PX_POR_TEXEL, Quadro, SATURACAO, TETO_DO_METAL, bmp_de_bgra, caixa_que_cobre, centro_do_texel,
        chapa_que_cobre, paleta_acabada, pintar_campo, pintar_grao, tabela_da_vinheta, texels_para,
    };
    use crate::ui::componentes::malha::cor_sem_grao;

    fn luminancia(cor: [f32; 3]) -> f32 {
        let canal = |v: f32| if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
        0.2126 * canal(cor[0]) + 0.7152 * canal(cor[1]) + 0.0722 * canal(cor[2])
    }

    /// A conta separável (e com a paleta já acabada) dá a mesma cor que a `cor_sem_grao`, ponto a
    /// ponto, no centro de cada texel: com e sem a luz do ponteiro.
    #[test]
    fn campo_igual_ao_ponto_a_ponto() {
        let (largura, altura) = (1400f32, 880f32);
        let (colunas, linhas) = (texels_para(largura), texels_para(altura));
        assert_eq!((colunas, linhas), (177, 112));
        let vinheta = tabela_da_vinheta(colunas, linhas, largura, altura);
        let texels = [(0, 0), (1, 1), (176, 111), (88, 56), (30, 98), (150, 15), (5, 74), (112, 45), (67, 4), (175, 110)];
        for tempo in [0., 66.5, 1234.5] {
            let quadro = Quadro::em(tempo);
            for ponteiro in [None, Some((300f32, 500f32)), Some((1390., 10.))] {
                let bgra = pintar_campo(colunas, linhas, largura, altura, &quadro, ponteiro, &vinheta);
                assert_eq!(bgra.len(), (colunas * linhas * 4) as usize);
                for (coluna, linha) in texels {
                    let (fx, fy) = (centro_do_texel(coluna), altura - centro_do_texel(linha));
                    let esperado = cor_sem_grao(&quadro, fx, fy, largura, altura, ponteiro);
                    let o = ((linha * colunas + coluna) * 4) as usize;
                    let pintado = [bgra[o + 2], bgra[o + 1], bgra[o]];
                    assert_eq!(bgra[o + 3], 255);
                    for canal in 0..3 {
                        let (a, b) = (f32::from(pintado[canal]) / 255., esperado[canal].clamp(0., 1.));
                        assert!((a - b).abs() <= 1. / 255., "t {tempo} texel ({coluna}, {linha}) canal {canal}: {a} x {b}");
                    }
                }
            }
        }
    }

    /// Nenhum ponto do campo, em nenhum instante e com a luz em cima, passa do `TETO_DO_METAL`: é
    /// contra ele que o contraste do tema é conferido.
    #[test]
    fn teto_cobre_o_campo() {
        let teto = Rgba::from(rgb(TETO_DO_METAL));
        let teto = luminancia([teto.r, teto.g, teto.b]);
        let (largura, altura) = (1400f32, 880f32);
        let mut maior = 0f32;
        for passo in 0..1500 {
            let quadro = Quadro::em(f64::from(passo) * 0.37);
            for (x, y) in [(700., 440.), (350., 440.), (1050., 440.), (700., 220.), (700., 660.), (200., 150.), (1200., 700.)] {
                // A luz bem em cima do ponto: o pior caso.
                let cor = cor_sem_grao(&quadro, x, y, largura, altura, Some((x, y))).map(|c| c.clamp(0., 1.));
                maior = maior.max(luminancia(cor));
            }
        }
        assert!(maior <= teto, "o campo chega a {maior:.3} e o teto é {teto:.3}");
        // E o teto não é folgado demais: a bolha clara chega perto dele.
        assert!(maior > teto * 0.8, "o campo só chega a {maior:.3}; o teto {teto:.3} está folgado");
    }

    /// O BMP montado à mão tem o cabeçalho que o decodificador espera e volta do `Image` do GPUI
    /// com o mesmo tamanho e os mesmos bytes, o alfa do grão inclusive.
    #[test]
    fn bmp_volta_igual() {
        let (largura, altura) = (5u32, 3u32);
        let bgra: Vec<u8> = (0..largura * altura * 4).map(|i| (i * 37 % 251) as u8).collect();
        let bmp = bmp_de_bgra(largura, altura, &bgra);
        let u32_em = |o: usize| u32::from_le_bytes(bmp[o..o + 4].try_into().unwrap());
        assert_eq!(&bmp[..2], b"BM");
        assert_eq!(u32_em(2) as usize, bmp.len());
        assert_eq!(u32_em(10), 122);
        assert_eq!(u32_em(14), 108);
        assert_eq!(u32_em(18), largura);
        assert_eq!(u32_em(22) as i32, -(altura as i32));
        assert_eq!((bmp[26], bmp[28]), (1, 32));
        assert_eq!(u32_em(30), 3);
        assert_eq!([u32_em(54), u32_em(58), u32_em(62), u32_em(66)], [0x00ff_0000, 0x0000_ff00, 0x0000_00ff, 0xff00_0000]);
        assert_eq!(&bmp[122..], &bgra[..]);

        let imagem = Image::from_bytes(ImageFormat::Bmp, bmp).to_image_data(SvgRenderer::new(Arc::new(()))).unwrap();
        assert_eq!(imagem.size(0), size(DevicePixels(largura as i32), DevicePixels(altura as i32)));
        assert_eq!(imagem.as_bytes(0).unwrap(), &bgra[..]);

        // O grão de verdade: alfa reto, preto ou branco, e some na média.
        let grao = pintar_grao(64);
        let volta = Image::from_bytes(ImageFormat::Bmp, bmp_de_bgra(64, 64, &grao))
            .to_image_data(SvgRenderer::new(Arc::new(())))
            .unwrap();
        assert_eq!(volta.as_bytes(0).unwrap(), &grao[..]);
        let (mut claros, mut escuros) = (0, 0);
        for pixel in grao.chunks_exact(4) {
            assert!(pixel[0] == pixel[1] && pixel[1] == pixel[2] && (pixel[0] == 0 || pixel[0] == 255));
            assert!(pixel[3] <= 80, "alfa {}", pixel[3]);
            if pixel[0] == 255 { claros += 1 } else { escuros += 1 }
        }
        assert!(claros > 1500 && escuros > 1500, "{claros} claros, {escuros} escuros");
    }

    /// O campo cobre a abertura inteira, sem deformar e centrado; a chapa com folga, também, e a
    /// folga fica para fora.
    #[test]
    fn cobre_sem_deformar() {
        let campo = size(px(1400.), px(880.));
        let caixas = [
            Bounds::new(point(px(10.), px(20.)), size(px(36.), px(36.))),
            Bounds::new(point(px(0.), px(0.)), size(px(300.), px(52.))),
            Bounds::new(point(px(500.), px(100.)), size(px(40.), px(400.))),
            Bounds::new(point(px(3.), px(7.)), size(px(1400.), px(880.))),
        ];
        for caixa in caixas {
            let cobre = caixa_que_cobre(caixa, campo);
            let perto = |a: Pixels, b: Pixels| (a - b).as_f32().abs() < 0.01;
            assert!(cobre.left() <= caixa.left() + px(0.01) && cobre.right() + px(0.01) >= caixa.right(), "{caixa:?}");
            assert!(cobre.top() <= caixa.top() + px(0.01) && cobre.bottom() + px(0.01) >= caixa.bottom(), "{caixa:?}");
            // A proporção do campo, e pelo menos um lado justo.
            assert!((cobre.size.width.as_f32() / cobre.size.height.as_f32() - 1400. / 880.).abs() < 1e-3);
            assert!(perto(cobre.size.width, caixa.size.width) || perto(cobre.size.height, caixa.size.height));
            assert!(perto(cobre.center().x, caixa.center().x) && perto(cobre.center().y, caixa.center().y));

            // A imagem com a folga: o campo de dentro cobre a caixa do mesmo jeito.
            let onde = Bounds::new(point(px(-PX_POR_TEXEL), px(-PX_POR_TEXEL)), size(px(1400. + 2. * PX_POR_TEXEL), px(880. + 2. * PX_POR_TEXEL)));
            let chapa = chapa_que_cobre(caixa, onde);
            let margem = px(PX_POR_TEXEL * cobre.size.width.as_f32() / 1400.);
            assert!(perto(chapa.left() + margem, cobre.left()) && perto(chapa.top() + margem, cobre.top()), "{caixa:?}");
            assert!(perto(chapa.size.width, cobre.size.width + margem * 2.));
        }
    }

    /// Contraste, saturação e brilho aplicados na paleta dão o mesmo que aplicados na média: é o
    /// que deixa o `pintar_campo` fazer a conta uma vez por cor, e não uma vez por ponto.
    #[test]
    fn paleta_acabada_e_a_conta_afim_da_receita() {
        // Uma cor qualquer entre duas da paleta: acabar a média dá o mesmo que a média das acabadas.
        let acabar = |cor: [f32; 3]| {
            let c = cor.map(|v| (v - 0.5) * CONTRASTE + 0.5);
            let luma = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            c.map(|v| luma + (v - luma) * SATURACAO + BRILHO)
        };
        let paleta = paleta_acabada();
        for (a, b, peso) in [(0, 1, 0.3f32), (2, 4, 0.5), (1, 3, 0.9)] {
            let mistura: [f32; 3] = std::array::from_fn(|k| CORES[a][k] * peso + CORES[b][k] * (1. - peso));
            let esperado = acabar(mistura);
            for k in 0..3 {
                let das_acabadas = paleta[a][k] * peso + paleta[b][k] * (1. - peso);
                assert!((das_acabadas - esperado[k]).abs() < 1e-5);
            }
        }
    }
}
