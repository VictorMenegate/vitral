mod atualizacao;
mod claude;
mod codex;
mod contas;
mod git;
mod mcp;
mod modelo;
mod motor;
mod processos;
mod prompts;
mod revisor;
mod sistema;
mod tema;
mod ui;

use std::path::PathBuf;

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    // `vitral <pasta>` abre direto nela; sem argumento, volta à última pasta usada.
    // `vitral <pasta> --nova <projeto> <mensagem>` já começa uma conversa.
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    // `vitral --versao [--json]` responde sem janela (o release e o atualizador conferem o binário).
    if let Some(codigo) = atualizacao::sem_janela(&argumentos) {
        std::process::exit(codigo);
    }
    // `vitral --vitrine` mostra os componentes da linguagem visual, sem pasta e sem motor.
    if argumentos.iter().any(|a| a == "--vitrine") {
        gpui_kit::application().with_assets(tema::Icones).run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::component::set_locale("pt-BR");
            tema::aplicar(cx);
            ui::zoom::iniciar(cx);
            ui::atalhos(cx);
            ui::componentes::vitrine::abrir(cx);
        });
        return;
    }
    let raiz = argumentos
        .first()
        .filter(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .and_then(|p| p.canonicalize().ok())
        .or_else(|| modelo::recentes().into_iter().find(|p| p.is_dir()));
    let nova = argumentos.iter().position(|a| a == "--nova").and_then(|i| {
        Some((
            argumentos.get(i + 1)?.clone(),
            argumentos.get(i + 2)?.clone(),
        ))
    });

    // A versão nova que não abriu duas vezes nesta sessão do PC volta para a anterior, com os mesmos
    // argumentos (só na versão instalada; a volta só vai para uma que leia os dados do disco).
    let aviso = match atualizacao::saude::ao_abrir(&|| modelo::maior_formato_no_disco(raiz.as_deref())) {
        atualizacao::saude::AoAbrir::Seguir(aviso) => aviso,
        atualizacao::saude::AoAbrir::Voltar(binario) => {
            use std::os::unix::process::CommandExt as _;
            let erro = std::process::Command::new(&binario).args(&argumentos).exec();
            eprintln!("vitral: não abri {}: {erro}", binario.display());
            None
        }
    };

    // O que um Vitral que caiu deixou rodando; de quebra, já descobre se o systemd abre escopos.
    std::thread::spawn(processos::limpar_sobras);

    gpui_kit::application()
        .with_assets(tema::Icones)
        .run(move |cx| {
            gpui_kit::init(cx);
            // Textos dos componentes do Kit (diff, diálogos, menus de texto) em português.
            gpui_kit::component::set_locale("pt-BR");
            tema::aplicar(cx);
            ui::zoom::iniciar(cx);
            ui::atalhos(cx);
            // Procura versão nova em segundo plano; na versão instalada, o reinício vai para o link `atual`.
            atualizacao::iniciar(aviso, cx);
            let opcoes = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(920.)), cx)),
                window_min_size: Some(size(px(960.), px(600.))),
                window_background: WindowBackgroundAppearance::Transparent,
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("vitral".into()),
                ..TitleBar::window_options()
            };
            gpui_kit::open_window(opcoes, cx, move |window, cx| {
                window.set_window_title("Vitral");
                // 10 s depois do primeiro quadro, a versão nova abriu direito.
                window.on_next_frame(|_, cx| atualizacao::primeiro_quadro(cx));
                let janela = cx.new(|cx| ui::Janela::new(raiz, window, cx));
                if let Some((projeto, mensagem)) = nova {
                    janela.update(cx, |j, cx| {
                        j.comecar_conversa(&projeto, mensagem, window, cx)
                    });
                }
                janela
            })
            .expect("não abriu a janela");
            cx.activate(true);
        });
}
