//! Zoom da interface inteira, como num navegador: Ctrl + roda ou Ctrl +/-/0 mudam o tamanho base
//! do texto do tema, que o GPUI usa como rem, então textos e espaçamentos em rem crescem juntos.
//! A escolha vale para o app todo e fica em `~/.config/vitral/interface.json`.

use std::{
    path::PathBuf,
    time::{Duration, Instant},
};

use gpui_kit::component::{StyledExt as _, Theme};
use gpui_kit::*;

use super::{AumentarZoom, DiminuirZoom, ZoomPadrao};
use crate::tema::{Cores, LARGURA_AVISO, esp};

const MINIMO: f32 = 0.7;
const MAXIMO: f32 = 2.0;
/// O GPUI no Linux manda 3 linhas por entalhe da roda.
const LINHAS_POR_ENTALHE: f32 = 3.;
/// Rolagem do touchpad que vale um passo de 10%.
const PIXELS_POR_PASSO: f32 = 50.;
/// Depois dessa pausa, o que sobrou do gesto anterior não conta mais.
const PAUSA_GESTO: Duration = Duration::from_millis(500);
const TEMPO_PILULA: Duration = Duration::from_millis(1200);
/// Espera a roda parar antes de gravar, em vez de gravar a cada passo.
const ATRASO_GRAVAR: Duration = Duration::from_millis(700);

struct Zoom {
    fator: f32,
    /// Fator que já está no disco.
    salvo: f32,
    /// Tamanhos do texto do tema a 100%.
    texto: Pixels,
    mono: Pixels,
    /// Rolagem do touchpad que ainda não completou um passo.
    sobra: f32,
    ultima_rolagem: Option<Instant>,
    pilula_ate: Option<Instant>,
    _esconder: Task<()>,
    _gravar: Task<()>,
    _ao_sair: Subscription,
}

impl Global for Zoom {}

/// Aplica o zoom guardado e liga as ações; chamar depois do tema e antes da primeira janela.
pub fn iniciar(cx: &mut App) {
    let fator = arquivo()
        .and_then(|caminho| std::fs::read_to_string(caminho).ok())
        .map_or(1., |texto| zoom_salvo(&texto));
    let tema = Theme::global(cx);
    let (texto, mono) = (tema.font_size, tema.mono_font_size);
    // A gravação atrasada ainda pode estar esperando quando o app fecha.
    let _ao_sair = cx.on_app_quit(|cx| {
        let zoom = cx.global::<Zoom>();
        if zoom.fator != zoom.salvo {
            gravar(zoom.fator);
        }
        async {}
    });
    cx.set_global(Zoom {
        fator,
        salvo: fator,
        texto,
        mono,
        sobra: 0.,
        ultima_rolagem: None,
        pilula_ate: None,
        _esconder: Task::ready(()),
        _gravar: Task::ready(()),
        _ao_sair,
    });
    aplicar(cx);
    cx.on_action(|_: &AumentarZoom, cx| mudar(1, cx));
    cx.on_action(|_: &DiminuirZoom, cx| mudar(-1, cx));
    cx.on_action(|_: &ZoomPadrao, cx| definir(1., cx));
}

/// Escuta a roda na fase de captura, antes das listas: com Ctrl ela vira zoom e nada rola junto.
pub fn captura_roda() -> impl IntoElement + use<> {
    canvas(
        |_, _, _| {},
        |_, _, window, _| {
            window.on_mouse_event(|evento: &ScrollEvent, fase, _, cx| {
                if fase == DispatchPhase::Capture && evento.modifiers.control {
                    cx.stop_propagation();
                    rolar(evento.delta, cx);
                }
            });
        },
    )
    .absolute()
    .size_0()
}

/// Porcentagem no alto da janela por um instante depois de cada mudança. Não tem hitbox, então
/// os cliques passam direto para o que está embaixo.
pub fn pilula(cx: &App) -> Option<impl IntoElement + use<>> {
    let zoom = cx.try_global::<Zoom>()?;
    if zoom.pilula_ate.is_none_or(|ate| Instant::now() >= ate) {
        return None;
    }
    let rotulo = format!("{}%", (zoom.fator * 100.).round() as i32);
    Some(
        deferred(
            div()
                .absolute()
                .top(px(14.))
                .left_0()
                .right_0()
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(esp::E12)
                        .py(esp::E2)
                        .rounded(crate::tema::raio::de(24.))
                        .bg(Cores::dialogo())
                        .border_1()
                        .border_color(Cores::linha())
                        .text_xs()
                        .font_medium()
                        .text_color(Cores::tenue())
                        .child(rotulo),
                ),
        )
        .with_priority(1),
    )
}

/// Escala o texto do tema; o Root do gpui-component o usa como rem a cada quadro. Os avisos têm
/// largura em px no tema, então crescem junto aqui.
fn aplicar(cx: &mut App) {
    let zoom = cx.global::<Zoom>();
    let (texto, mono, fator) = (zoom.texto * zoom.fator, zoom.mono * zoom.fator, zoom.fator);
    // Os raios em px acompanham o zoom (ver `tema::definir_zoom_dos_raios`).
    crate::tema::definir_zoom_dos_raios(fator);
    Theme::update(cx, |tema| {
        tema.font_size = texto;
        tema.mono_font_size = mono;
        // Os raios que o kit usa nos componentes dele (botões, campos, menus) crescem junto.
        tema.radius = crate::tema::raio::controle();
        tema.radius_lg = crate::tema::raio::peca();
        tema.notification.width = px(LARGURA_AVISO * fator);
    });
}

fn rolar(delta: ScrollDelta, cx: &mut App) {
    if !cx.has_global::<Zoom>() {
        return;
    }
    let zoom = cx.global_mut::<Zoom>();
    let agora = Instant::now();
    if zoom.ultima_rolagem.is_none_or(|antes| agora - antes > PAUSA_GESTO) {
        zoom.sobra = 0.;
    }
    zoom.ultima_rolagem = Some(agora);
    let passos = passos_da_rolagem(&mut zoom.sobra, delta);
    if passos != 0 {
        mudar(passos, cx);
    }
}

fn mudar(passos: i32, cx: &mut App) {
    if let Some(zoom) = cx.try_global::<Zoom>() {
        definir(passo(zoom.fator, passos), cx);
    }
}

/// Troca o fator, mostra a pílula por um instante e agenda a gravação (a anterior é cancelada).
fn definir(fator: f32, cx: &mut App) {
    if !cx.has_global::<Zoom>() {
        return;
    }
    let esconder = cx.spawn(async |cx| {
        cx.background_executor().timer(TEMPO_PILULA).await;
        cx.refresh();
    });
    let gravacao = cx.spawn(async |cx| {
        cx.background_executor().timer(ATRASO_GRAVAR).await;
        let pendente = cx.update(|cx| {
            let zoom = cx.global::<Zoom>();
            (zoom.fator != zoom.salvo).then_some(zoom.fator)
        });
        let Some(fator) = pendente else {
            return;
        };
        if cx.background_executor().spawn(async move { gravar(fator) }).await {
            cx.update(|cx| cx.global_mut::<Zoom>().salvo = fator);
        }
    });
    let zoom = cx.global_mut::<Zoom>();
    let mudou = zoom.fator != fator;
    zoom.fator = fator;
    zoom.pilula_ate = Some(Instant::now() + TEMPO_PILULA);
    zoom._esconder = esconder;
    zoom._gravar = gravacao;
    if mudou {
        aplicar(cx);
    } else {
        // No limite a pílula aparece do mesmo jeito, para mostrar que não dá mais.
        cx.refresh_windows();
    }
}

static PASTA: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

/// A pasta da configuração (`Dados::config`), onde fica o interface.json. Sem ela (a vitrine fora
/// da bancada), o zoom vale só até fechar.
pub fn usar_pasta(pasta: PathBuf) {
    let _ = PASTA.set(pasta);
}

fn arquivo() -> Option<PathBuf> {
    Some(PASTA.get()?.join("interface.json"))
}

fn gravar(fator: f32) -> bool {
    gravar_na_interface("zoom", serde_json::json!((f64::from(fator) * 100.).round() / 100.))
}

/// O interface.json inteiro, se existe e é um objeto (as Animações também ficam nele).
pub(super) fn interface() -> Option<serde_json::Value> {
    let texto = std::fs::read_to_string(arquivo()?).ok()?;
    serde_json::from_str::<serde_json::Value>(&texto).ok().filter(|valor| valor.is_object())
}

/// Grava uma chave em interface.json sem perder as outras que o arquivo tiver. Uma gravação por
/// vez (a vista e o último chat vão em segundo plano): duas ao mesmo tempo perderiam uma chave.
pub(super) fn gravar_na_interface(chave: &str, valor: serde_json::Value) -> bool {
    static VEZ: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let Some(caminho) = arquivo() else {
        return false;
    };
    let _vez = VEZ.lock().unwrap_or_else(|e| e.into_inner());
    let existente = std::fs::read_to_string(&caminho).ok();
    match crate::dados::gravar_json_seguro(&caminho, &com_chave(existente.as_deref(), chave, valor), false) {
        Ok(()) => true,
        Err(erro) => {
            eprintln!("vitral: não gravou {} ({erro})", caminho.display());
            false
        }
    }
}

/// Conteúdo de interface.json com uma chave trocada; um arquivo estragado vira um novo.
fn com_chave(existente: Option<&str>, chave: &str, novo: serde_json::Value) -> serde_json::Value {
    let mut valor = existente
        .and_then(|texto| serde_json::from_str::<serde_json::Value>(texto).ok())
        .filter(|valor| valor.is_object())
        .unwrap_or_else(|| serde_json::json!({}));
    valor[chave] = novo;
    valor
}

/// Fator guardado em interface.json; 100% se o arquivo não tem zoom ou está estragado.
fn zoom_salvo(texto: &str) -> f32 {
    serde_json::from_str::<serde_json::Value>(texto)
        .ok()
        .and_then(|valor| valor.get("zoom")?.as_f64())
        .map_or(1., |zoom| limitar(zoom as f32))
}

/// Arredonda para um múltiplo de 10% dentro dos limites; valor sem sentido volta a 100%.
fn limitar(fator: f32) -> f32 {
    if !fator.is_finite() || fator <= 0. {
        return 1.;
    }
    ((fator * 10.).round() / 10.).clamp(MINIMO, MAXIMO)
}

/// Fator depois de `passos` de 10% (negativo diminui).
fn passo(fator: f32, passos: i32) -> f32 {
    limitar(((fator * 10.).round() + passos as f32) / 10.)
}

/// Quantos passos de zoom uma rolagem vale: um por entalhe da roda; no touchpad os pixels se
/// acumulam em `sobra` e dão um passo a cada 50 px, senão um gesto iria de 70% a 200% de uma vez.
/// Para cima (y positivo) aumenta, como nos navegadores.
fn passos_da_rolagem(sobra: &mut f32, delta: ScrollDelta) -> i32 {
    let distancia = match delta {
        ScrollDelta::Lines(linhas) => linhas.y / LINHAS_POR_ENTALHE * PIXELS_POR_PASSO,
        ScrollDelta::Pixels(pixels) => pixels.y.as_f32(),
    };
    // Ao inverter o sentido, o resto do sentido antigo não atrasa o novo.
    if distancia * *sobra < 0. {
        *sobra = 0.;
    }
    *sobra += distancia;
    let passos = (*sobra / PIXELS_POR_PASSO).trunc();
    *sobra -= passos * PIXELS_POR_PASSO;
    passos as i32
}

#[cfg(test)]
mod testes {
    // Sem `super::*`: o glob do gpui_kit traz a macro `test` do GPUI no lugar da do Rust.
    use super::{MAXIMO, MINIMO, com_chave, passo, passos_da_rolagem, zoom_salvo};
    use gpui_kit::{ScrollDelta, point, px};

    fn linhas(y: f32) -> ScrollDelta {
        ScrollDelta::Lines(point(0., y))
    }

    fn pixels(y: f32) -> ScrollDelta {
        ScrollDelta::Pixels(point(px(0.), px(y)))
    }

    #[test]
    fn passos_de_dez_por_cento_dentro_dos_limites() {
        assert_eq!(passo(1., 1), 1.1);
        assert_eq!(passo(1., -1), 0.9);
        assert_eq!(passo(1.1, -1), 1.);
        assert_eq!(passo(0.7, -1), MINIMO);
        assert_eq!(passo(2., 1), MAXIMO);
        assert_eq!(passo(1.9, 5), MAXIMO);
        // Subir do mínimo ao máximo, passo a passo, não acumula erro de arredondamento.
        let mut fator = MINIMO;
        for _ in 0..13 {
            fator = passo(fator, 1);
        }
        assert_eq!(fator, MAXIMO);
    }

    #[test]
    fn um_entalhe_da_roda_e_um_passo() {
        let mut sobra = 0.;
        assert_eq!(passos_da_rolagem(&mut sobra, linhas(3.)), 1);
        assert_eq!(passos_da_rolagem(&mut sobra, linhas(-3.)), -1);
        assert_eq!(passos_da_rolagem(&mut sobra, linhas(6.)), 2);
        // Roda de alta resolução: oito pedaços de um entalhe.
        let mut total = 0;
        for _ in 0..8 {
            total += passos_da_rolagem(&mut sobra, linhas(3. / 8.));
        }
        assert_eq!(total, 1);
    }

    #[test]
    fn touchpad_acumula_pixels_ate_um_passo() {
        let mut sobra = 0.;
        assert_eq!(passos_da_rolagem(&mut sobra, pixels(20.)), 0);
        assert_eq!(passos_da_rolagem(&mut sobra, pixels(20.)), 0);
        assert_eq!(passos_da_rolagem(&mut sobra, pixels(20.)), 1);
        assert_eq!(sobra, 10.);
        // Um gesto longo de 400 px dá 8 passos, não pula do mínimo ao máximo de uma vez.
        let mut sobra = 0.;
        let total: i32 = (0..40).map(|_| passos_da_rolagem(&mut sobra, pixels(10.))).sum();
        assert_eq!(total, 8);
        // Inverter o sentido descarta o que sobrou do outro.
        let mut sobra = 0.;
        passos_da_rolagem(&mut sobra, pixels(45.));
        assert_eq!(passos_da_rolagem(&mut sobra, pixels(-50.)), -1);
        assert_eq!(sobra, 0.);
    }

    #[test]
    fn le_interface_json_ausente_antigo_ou_estragado() {
        assert_eq!(zoom_salvo(r#"{"zoom": 1.2}"#), 1.2);
        assert_eq!(zoom_salvo(r#"{"zoom": 1.15}"#), 1.2);
        assert_eq!(zoom_salvo(r#"{"zoom": 9}"#), MAXIMO);
        assert_eq!(zoom_salvo(r#"{"zoom": 0.2}"#), MINIMO);
        assert_eq!(zoom_salvo(r#"{"zoom": 0}"#), 1.);
        assert_eq!(zoom_salvo(r#"{"zoom": "grande"}"#), 1.);
        assert_eq!(zoom_salvo(r#"{"outra_coisa": true}"#), 1.);
        assert_eq!(zoom_salvo("[1.5]"), 1.);
        assert_eq!(zoom_salvo(""), 1.);
        assert_eq!(zoom_salvo("{\"zoom\": 1.3"), 1.);
    }

    #[test]
    fn gravar_mantem_as_outras_chaves() {
        let zoom = |fator: f32| serde_json::json!((f64::from(fator) * 100.).round() / 100.);
        let valor = com_chave(Some(r#"{"zoom": 1.5, "animacoes": "desligadas"}"#), "zoom", zoom(1.1));
        assert_eq!(valor, serde_json::json!({"zoom": 1.1, "animacoes": "desligadas"}));
        let valor = com_chave(Some(r#"{"zoom": 1.5}"#), "animacoes", serde_json::json!("gnome"));
        assert_eq!(valor, serde_json::json!({"zoom": 1.5, "animacoes": "gnome"}));
        assert_eq!(com_chave(None, "zoom", zoom(0.9)), serde_json::json!({"zoom": 0.9}));
        assert_eq!(com_chave(Some("estragado"), "zoom", zoom(2.)), serde_json::json!({"zoom": 2.0}));
        assert_eq!(zoom_salvo(&com_chave(None, "zoom", zoom(passo(1., 3))).to_string()), 1.3);
    }
}
