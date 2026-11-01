//! A tela de quando o Vitral abre só para leitura: os dados não abriram (estragados, de uma
//! versão mais nova) ou a migração do Vitral antigo não pôde rodar. Nada é gravado por cima.

use std::{path::Path, sync::Arc};

use crate::dados::Dados;

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        Confirmacao, TamanhoBotao, TelaCheia, Tom, TomConfirmacao, aviso, botao_fantasma, botao_perigo,
        botao_primario, confirmar,
    },
    dialogos::com_til,
};
use crate::{
    motor::SomenteLeitura,
    tema::{Lucide, esp, r},
};

/// O logo completo em PNG de 128 px, mostrado em 64.
const LOGO: &[u8] = include_bytes!("../../assets/vitral-128.png");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Botao {
    AbrirPasta,
    ComecarDoZero,
    TentarDeNovo,
    Sair,
}

/// O título, o texto e os botões de cada caso.
fn textos(leitura: &SomenteLeitura, casa: Option<&Path>) -> (&'static str, String, Vec<Botao>) {
    let curto = |p: &Path| match casa.and_then(|c| p.strip_prefix(c).ok()) {
        Some(resto) => format!("~/{}", resto.display()),
        None => p.display().to_string(),
    };
    match leitura {
        SomenteLeitura::Ilegivel { arquivo, .. } => (
            "Os dados não abriram",
            format!("Não consegui ler {} nem a cópia anterior. Nada foi gravado por cima.", curto(arquivo)),
            vec![Botao::AbrirPasta, Botao::ComecarDoZero, Botao::Sair],
        ),
        SomenteLeitura::VersaoNova { versao, .. } => (
            "Dados de uma versão mais nova",
            format!("Estes dados são de uma versão mais nova do Vitral ({versao}). Atualize o Vitral."),
            vec![Botao::AbrirPasta, Botao::Sair],
        ),
        SomenteLeitura::MigracaoAdiada { raizes, pids } => {
            let pids = pids.iter().map(u32::to_string).collect::<Vec<_>>().join(", ");
            let raizes = raizes.iter().map(|r| curto(r)).collect::<Vec<_>>().join(", ");
            (
                "O Vitral antigo está aberto",
                format!("O Vitral antigo está aberto (pid {pids}). Feche-o para eu trazer as conversas de {raizes}."),
                vec![Botao::TentarDeNovo, Botao::Sair],
            )
        }
        SomenteLeitura::MigracaoFalhou { raiz, erro } => (
            "As conversas antigas não vieram",
            format!("Não consegui trazer as conversas de {} ({erro}). Nada foi gravado.", curto(raiz)),
            vec![Botao::TentarDeNovo, Botao::Sair],
        ),
    }
}

impl Janela {
    pub(super) fn tela_de_recuperacao(&self, leitura: SomenteLeitura, cx: &Context<Self>) -> impl IntoElement + use<> {
        let dados = self.motor.as_ref().map(|m| m.read(cx).dados.clone());
        div()
            .flex()
            .flex_col()
            .size_full()
            .p(esp::E8)
            .child(
                div()
                    .flex()
                    .w_full()
                    .child(self.arrastavel("arrastar-recuperacao").flex_1().h(px(36.)))
                    .child(self.botoes_janela(cx)),
            )
            .children(dados.map(|dados| Recuperacao { leitura, dados }))
    }
}

#[derive(IntoElement)]
struct Recuperacao {
    leitura: SomenteLeitura,
    dados: Dados,
}

impl RenderOnce for Recuperacao {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (titulo, texto, botoes) = textos(&self.leitura, dirs::home_dir().as_deref());
        let logo = Arc::new(Image::from_bytes(ImageFormat::Png, LOGO.to_vec()));
        let fileira = botoes.into_iter().enumerate().map(|(i, botao)| {
            let dados = self.dados.clone();
            let t = TamanhoBotao::Controle;
            match botao {
                Botao::AbrirPasta => botao_fantasma(("rec", i), Some(Lucide::FolderOpen.into()), "Abrir a pasta de dados", t, window, cx)
                    .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&dados.pasta, cx))
                    .into_any_element(),
                Botao::ComecarDoZero => botao_perigo(("rec", i), None, "Começar do zero", t, window, cx)
                    .on_click(move |_, window, cx| comecar_do_zero(dados.clone(), window, cx))
                    .into_any_element(),
                Botao::TentarDeNovo => botao_primario(("rec", i), None, "Tentar de novo", t, window, cx)
                    .on_click(|_, window, cx| reabrir(window, cx))
                    .into_any_element(),
                Botao::Sair => botao_fantasma(("rec", i), None, "Sair", t, window, cx)
                    .on_click(|_, _, cx| cx.quit())
                    .into_any_element(),
            }
        });
        TelaCheia::new(titulo)
            .simbolo(img(logo).flex_none().size(r(64.)))
            .texto(texto)
            .child(div().flex().flex_wrap().gap(esp::E8).children(fileira))
    }
}

/// Abre o Vitral de novo no lugar deste processo (a trava sai no exec).
fn reabrir(window: &mut Window, cx: &mut App) {
    let erro = crate::instancia::reexecutar();
    window.push_notification(aviso(Tom::Erro, format!("Não consegui abrir de novo: {erro}")), cx);
}

/// Afasta a pasta de dados inteira (nada é apagado) e abre de novo, do zero.
fn comecar_do_zero(dados: Dados, window: &mut Window, cx: &mut App) {
    let destino = format!("{}.descartado-…", com_til(&dados.pasta));
    confirmar(
        Confirmacao {
            tom: TomConfirmacao::Perigo,
            icone: Lucide::Trash.into(),
            titulo: "Começar do zero?".into(),
            texto: format!("As conversas atuais vão para {destino}; nada é apagado.").into(),
            detalhes: Vec::new(),
            ok: "Começar do zero".into(),
            cancelar: "Cancelar".into(),
            ok_perigo: true,
        },
        move |window, cx| match dados.descartar_tudo() {
            Ok(_) => reabrir(window, cx),
            Err(erro) => window.push_notification(aviso(Tom::Erro, format!("Nada foi mexido: {erro:#}")), cx),
        },
        None,
        window,
        cx,
    );
}

#[cfg(test)]
mod testes {
    use std::path::{Path, PathBuf};

    use super::{Botao, textos};
    use crate::motor::SomenteLeitura;

    #[test]
    fn cada_caso_diz_o_que_houve_e_o_que_da_para_fazer() {
        let casa = Path::new("/home/v");
        let ilegivel = SomenteLeitura::Ilegivel { arquivo: "/home/v/.local/share/vitral/estado.json".into(), copia: None };
        let (_, texto, botoes) = textos(&ilegivel, Some(casa));
        assert_eq!(texto, "Não consegui ler ~/.local/share/vitral/estado.json nem a cópia anterior. Nada foi gravado por cima.");
        assert_eq!(botoes, [Botao::AbrirPasta, Botao::ComecarDoZero, Botao::Sair]);
        let nova = SomenteLeitura::VersaoNova { arquivo: PathBuf::new(), versao: 3 };
        let (_, texto, botoes) = textos(&nova, Some(casa));
        assert!(texto.contains("(3)"));
        assert_eq!(botoes, [Botao::AbrirPasta, Botao::Sair]);
        let adiada = SomenteLeitura::MigracaoAdiada { raizes: vec!["/home/v/Projetos".into()], pids: vec![42] };
        let (_, texto, botoes) = textos(&adiada, Some(casa));
        assert_eq!(texto, "O Vitral antigo está aberto (pid 42). Feche-o para eu trazer as conversas de ~/Projetos.");
        assert_eq!(botoes, [Botao::TentarDeNovo, Botao::Sair]);
        let falhou = SomenteLeitura::MigracaoFalhou { raiz: "/d".into(), erro: "disco cheio".into() };
        assert!(textos(&falhou, None).1.contains("/d (disco cheio)"));
    }
}
