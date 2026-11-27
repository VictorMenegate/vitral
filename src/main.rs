// No Windows, o release não abre uma janela de console junto (o de desenvolvimento abre, para ver o
// que o Vitral escreve no stderr).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[macro_use]
mod i18n;
mod agente_acp;
mod agente_codex;
mod agente_gemini;
mod atualizacao;
mod claude;
mod cli_agentes;
mod codex;
mod contas;
mod contas_codex;
mod titulo;
mod dados;
mod diagnostico;
mod git;
mod ideias;
mod imagens_recebidas;
mod importar;
mod limpeza_anexos;
mod linear;
mod instancia;
mod mcp;
mod memoria_equipe;
mod migracao;
mod modelo;
mod opcoes;
mod paleta;
mod motor;
mod perfis;
mod processos;
mod prompts;
mod queda;
mod sistema;
mod tema;
mod ui;
mod versoes_agentes;

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::component::TitleBar;
use gpui_kit::*;

fn main() {
    // `vitral` abre o app; `vitral <pasta>` abre o chat da pasta (ou um novo nela); `vitral --nova
    // <pasta> [mensagem]` já começa uma conversa nela. Com o Vitral aberto, o pedido vai para ele.
    let argumentos: Vec<String> = std::env::args().skip(1).collect();
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
            ui::atalhos::ligar(cx);
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
    // Daqui em diante, um pânico em qualquer thread fica registrado na pasta dos dados.
    queda::instalar(&dados.pasta);
    let queda_anterior = queda::aviso_da_anterior(&dados.pasta);
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
    // Só no canal de teste (o aceite do atualizador no runner do Windows): a versão em
    // VITRAL_TESTE_CAI cai aqui, com a abertura já contada e antes do primeiro quadro, como uma
    // versão nova quebrada. No release (canal estável) a condição nem existe.
    if atualizacao::canal() == "teste" && std::env::var("VITRAL_TESTE_CAI").is_ok_and(|v| v == atualizacao::VERSAO) {
        eprintln!("vitral: a {} cai antes do primeiro quadro (VITRAL_TESTE_CAI)", atualizacao::VERSAO);
        std::process::exit(3);
    }
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
            // O AppUserModelID do Windows (os avisos do sistema saem com ele; o atalho do Menu
            // Iniciar usa o mesmo). Não muda depois: o Windows guarda as preferências por ele.
            #[cfg(windows)]
            cx.set_app_identity("VictorMenegate.Vitral", "Vitral");
            gpui_kit::init(cx);
            // Textos dos componentes do Kit (diff, diálogos, menus de texto) em português.
            gpui_kit::component::set_locale("pt-BR");
            tema::aplicar(cx);
            ui::zoom::iniciar(cx);
            ui::atalhos::ligar(cx);
            // Procura versão nova em segundo plano; na versão instalada, o reinício vai para o link `atual`.
            atualizacao::iniciar(aviso, dados.pasta.clone(), cx);
            let motor = cx.new(|cx| motor::Motor::new(dados, aberto, cx));
            // As cores escolhidas na Aparência, antes da primeira janela.
            if tema::definir_paleta(&motor.read(cx).config.paleta) {
                tema::reaplicar_cores(cx);
            }
            let opcoes = WindowOptions {
                // O tamanho de quando fechou (e maximizada, se estava); na primeira vez, 1440 por 920.
                window_bounds: Some(ui::arranjo::limites_da_janela(cx)),
                window_min_size: Some(size(px(ui::arranjo::TAMANHO_MINIMO.0), px(ui::arranjo::TAMANHO_MINIMO.1))),
                // Já no nível guardado, para o primeiro quadro não sair com o fundo de outro.
                window_background: fundo_inicial(motor.read(cx).config.transparencia),
                window_decorations: Some(WindowDecorations::Client),
                app_id: Some("vitral".into()),
                ..TitleBar::window_options()
            };
            motor.update(cx, |m, cx| {
                if let Some(relatorio) = &relatorio {
                    m.migracao_concluida(relatorio, cx);
                }
                m.avisos_ao_abrir.extend(queda_anterior);
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

/// O fundo que a janela pede ao sistema no nível de transparência das Configurações (o mesmo
/// código em todo sistema).
///
/// Nos níveis transparentes, `Blurred`: o sistema desfoca o que está atrás da janela, e o véu e o
/// grão por cima fazem o fosco. Desfocam o Windows (o acrílico), o macOS, o KDE e o GNOME 51 em
/// diante (no Wayland, pelo protocolo padrão ext-background-effect-v1, que o vendor/gpui-pre-linux
/// acrescenta ao GPUI, ou pelo do KDE). No GNOME 50 ou anterior e no X11 a janela fica só
/// transparente (no GNOME, a extensão Blur my Shell pode desfocar por fora).
///
/// No Opaco, a chapa de metal cobre a janela inteira e o desfoque não apareceria: o pedido é
/// `Transparent`, que não gasta o desfoque. O `Opaque` não serve: no X11 o GPUI passa a desenhar
/// opaco e a margem da sombra das decorações do cliente sairia preta, e ele trocaria o texto para
/// o antisserrilhado por subpixel só nesse nível.
///
/// `VITRAL_FUNDO=mica`, `transparente` ou `opaco` força um fundo em qualquer nível, para comparar
/// (o Mica só existe no Windows 11; nos outros vira transparente).
fn fundo_da_janela(nivel: u8, forcado: Option<&str>) -> WindowBackgroundAppearance {
    match forcado {
        Some("mica") => WindowBackgroundAppearance::MicaBackdrop,
        Some("transparente") => WindowBackgroundAppearance::Transparent,
        Some("opaco") => WindowBackgroundAppearance::Opaque,
        _ if nivel == 0 => WindowBackgroundAppearance::Transparent,
        _ => WindowBackgroundAppearance::Blurred,
    }
}

/// O `VITRAL_FUNDO`, lido uma vez.
fn fundo_forcado() -> Option<&'static str> {
    static FORCADO: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    FORCADO.get_or_init(|| std::env::var("VITRAL_FUNDO").ok()).as_deref()
}

thread_local! {
    /// O último fundo pedido para a janela principal.
    static FUNDO_PEDIDO: std::cell::Cell<Option<WindowBackgroundAppearance>> = const { std::cell::Cell::new(None) };
}

/// O fundo com que a janela principal abre.
fn fundo_inicial(nivel: u8) -> WindowBackgroundAppearance {
    let fundo = fundo_da_janela(nivel, fundo_forcado());
    FUNDO_PEDIDO.set(Some(fundo));
    fundo
}

/// A Janela chama a cada desenho, com o nível de agora: trocado o nível nas Configurações, o
/// pedido ao sistema troca na hora (o `set_background_appearance` vale com a janela aberta). Só
/// pede quando o fundo muda: no Windows e no macOS cada pedido refaz o efeito.
pub fn acompanhar_o_fundo(window: &Window, nivel: u8) {
    let fundo = fundo_da_janela(nivel, fundo_forcado());
    if FUNDO_PEDIDO.replace(Some(fundo)) != Some(fundo) {
        window.set_background_appearance(fundo);
    }
}

#[cfg(test)]
mod testes {
    use gpui_kit::WindowBackgroundAppearance as Fundo;

    /// O nível escolhe o fundo do mesmo jeito em todo sistema (a CI roda no Linux e no Windows):
    /// desfocado nos níveis transparentes e sem desfoque no Opaco, que a chapa cobre.
    #[test]
    fn fundo_desfocado_so_nos_niveis_transparentes() {
        assert_eq!(crate::tema::NOMES_DA_TRANSPARENCIA[0], "Opaco");
        assert_eq!(super::fundo_da_janela(0, None), Fundo::Transparent);
        for nivel in 1..crate::tema::NOMES_DA_TRANSPARENCIA.len() as u8 {
            assert_eq!(super::fundo_da_janela(nivel, None), Fundo::Blurred, "nível {nivel}");
        }
    }

    /// O `VITRAL_FUNDO` força o fundo em qualquer nível; um valor que ele não conhece não força nada.
    #[test]
    fn vitral_fundo_forca_em_qualquer_nivel() {
        for nivel in 0..crate::tema::NOMES_DA_TRANSPARENCIA.len() as u8 {
            assert_eq!(super::fundo_da_janela(nivel, Some("mica")), Fundo::MicaBackdrop);
            assert_eq!(super::fundo_da_janela(nivel, Some("transparente")), Fundo::Transparent);
            assert_eq!(super::fundo_da_janela(nivel, Some("opaco")), Fundo::Opaque);
            assert_eq!(super::fundo_da_janela(nivel, Some("acrilico")), super::fundo_da_janela(nivel, None));
        }
    }

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
