mod atualizacao;
mod claude;
mod codex;
mod contas;
mod dados;
mod git;
mod github;
mod importar;
mod instancia;
mod isolamento;
mod issues;
mod linear;
mod mcp;
mod migracao;
mod modelo;
mod motor;
mod processos;
mod prompts;
mod remoto;
mod revisor;
mod segredos;
mod sistema;
mod tema;
mod ui;
mod vigia;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    // `vitral` abre o app; `vitral <pasta>` liga ou abre o projeto da pasta; `vitral --nova
    // <projeto> [mensagem]` já começa uma conversa. Com o Vitral aberto, o pedido vai para ele.
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
    // O hook Stop dos ajudantes no Windows (exec form, sem shell): responde e sai, sem tocar em nada.
    if argumentos.first().is_some_and(|a| a == "--hook-verificacao") {
        std::process::exit(revisor::rodar_hook_verificacao());
    }
    // `vitral --versao [--json]` e `vitral --instalar` respondem sem janela (o release e o
    // atualizador conferem o binário), antes de tocar nos dados.
    if let Some(codigo) = atualizacao::sem_janela(&argumentos) {
        std::process::exit(codigo);
    }
    // `vitral --vitrine` mostra os componentes da linguagem visual, sem dados e sem motor.
    if argumentos.iter().any(|a| a == "--vitrine") {
        if let Ok(dados) = dados::Dados::padrao() {
            ui::zoom::usar_pasta(dados.config.clone());
        }
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
    let dados = match dados::Dados::padrao() {
        Ok(dados) => dados,
        Err(erro) => {
            eprintln!("vitral: {erro:#}");
            std::process::exit(1);
        }
    };
    let atual = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
    let abertura = instancia::Abertura::de_args(&argumentos, &atual);

    // A ordem: trava, saúde da versão, socket, migração, janela. Com a trava ocupada, o pedido vai ao
    // Vitral aberto (que pode estar migrando: ele já ouve o socket) e este sai sem janela.
    let _trava = match instancia::travar(&dados.arquivo_trava()) {
        Ok(Some(trava)) => trava,
        Ok(None) => match instancia::entregar(&dados.arquivo_socket(), &abertura, Duration::from_secs(10)) {
            Ok(()) => return,
            Err(_) => {
                eprintln!("O Vitral já está aberto, mas não respondeu");
                std::process::exit(1);
            }
        },
        Err(erro) => {
            eprintln!("vitral: não travou {}: {erro}", dados.arquivo_trava().display());
            std::process::exit(1);
        }
    };
    // A versão nova que não abriu duas vezes nesta sessão do PC volta para a anterior, com os mesmos
    // argumentos (só na versão instalada). Depois da trava, para não contar a abertura que só entrega
    // o pedido ao Vitral aberto; antes da migração, para contar a que cai migrando. A volta só vai
    // para uma versão que leia o formato dos dados do disco (o exec solta a trava).
    let aviso = match atualizacao::saude::ao_abrir(&|| dados.formato_no_disco()) {
        atualizacao::saude::AoAbrir::Seguir(aviso) => aviso,
        atualizacao::saude::AoAbrir::Voltar(binario) => {
            let erro = instancia::trocar_por(&binario, &argumentos);
            eprintln!("vitral: não abri {}: {erro}", binario.display());
            None
        }
    };
    let pedidos = instancia::ouvir(&dados.arquivo_socket())
        .map_err(|erro| eprintln!("vitral: sem o socket {} ({erro}); outra abertura não chega aqui", dados.arquivo_socket().display()))
        .ok();
    ui::zoom::usar_pasta(dados.config.clone());
    contas::usar_pasta(dados.config.clone());

    // Antes de qualquer thread que abra processos: um fork ainda sem o exec se chama "vitral".
    let na_bancada = std::env::var_os("DENTRO_DA_BANCADA").is_some_and(|v| v == "1");
    let raizes = migracao::raizes_antigas(&dados, abertura.pasta.as_deref());
    // Na primeira abertura com os dados novos, as conversas do Claude Code vêm junto.
    let primeiro_uso = dados.primeiro_uso();
    let migrou = migracao::migrar(&dados, &raizes, Path::new("/proc"), std::process::id(), na_bancada, modelo::agora());
    let (aberto, relatorio) = match migrou {
        migracao::Resultado::Adiada { raizes, pids } => (Err(motor::SomenteLeitura::MigracaoAdiada { raizes, pids }), None),
        migracao::Resultado::Falhou { raiz, erro } => (Err(motor::SomenteLeitura::MigracaoFalhou { raiz, erro }), None),
        migracao::Resultado::Migrou(relatorio) => (dados.carregar().map_err(Into::into), Some(relatorio)),
        migracao::Resultado::NaoPrecisa | migracao::Resultado::SemRaizes => (dados.carregar().map_err(Into::into), None),
    };
    // Os dados abriram no formato desta versão: o atualizador não volta para uma que não o lê.
    if aberto.is_ok() {
        dados.marcar_formato();
    }

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
            atualizacao::iniciar(aviso, dados.pasta.clone(), cx);
            let opcoes = WindowOptions {
                window_bounds: Some(WindowBounds::centered(size(px(1440.), px(920.)), cx)),
                window_min_size: Some(size(px(960.), px(600.))),
                window_background: WindowBackgroundAppearance::Transparent,
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("vitral".into()),
                ..TitleBar::window_options()
            };
            let motor = cx.new(|cx| motor::Motor::new(dados, aberto, cx));
            motor.update(cx, |m, cx| {
                if let Some(relatorio) = &relatorio {
                    m.migracao_concluida(relatorio, cx);
                }
                m.importar_na_abertura(primeiro_uso, cx);
                m.pedir_abertura(abertura, cx);
            });
            // Os pedidos de outras aberturas, que esperaram a janela.
            if let Some(pedidos) = pedidos {
                let motor = motor.downgrade();
                cx.spawn(async move |cx| {
                    while let Ok(abertura) = pedidos.recv().await {
                        if motor.update(cx, |m, cx| m.pedir_abertura(abertura, cx)).is_err() {
                            break;
                        }
                    }
                })
                .detach();
            }
            gpui_kit::open_window(opcoes, cx, move |window, cx| {
                window.set_window_title("Vitral");
                // 10 s depois do primeiro quadro, a versão nova abriu direito.
                window.on_next_frame(|_, cx| atualizacao::primeiro_quadro(cx));
                cx.new(|cx| ui::Janela::new(motor, window, cx))
            })
            .expect("não abriu a janela");
            cx.activate(true);
        });
}

#[cfg(test)]
mod testes {
    /// `dunce::canonicalize` no lugar do da std: no Windows o da std devolve `\\?\C:\…`, que estraga
    /// as regras de permissão, a comparação com o que o git devolve e o que aparece na tela. (O
    /// atualizador do Linux fica de fora: só roda lá.)
    #[test]
    fn canonicalize_sem_o_prefixo_do_windows() {
        let raiz = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src"));
        let mut erros = Vec::new();
        for caminho in crate::tema::testes::arquivos_rs(raiz) {
            let relativo = caminho.strip_prefix(raiz).unwrap();
            if relativo == std::path::Path::new("atualizacao").join("linux.rs") {
                continue;
            }
            let texto = std::fs::read_to_string(&caminho).unwrap();
            for (numero, linha) in texto.lines().enumerate() {
                let sem_dunce = linha.replace("dunce::canonicalize", "");
                if sem_dunce.contains(concat!(".canonicalize", "()")) || sem_dunce.contains(concat!("fs::canonicalize", "(")) {
                    erros.push(format!("{}:{}: {}", relativo.display(), numero + 1, linha.trim()));
                }
            }
        }
        assert!(erros.is_empty(), "canonicalize da std (use dunce::canonicalize):\n{}", erros.join("\n"));
    }
}
