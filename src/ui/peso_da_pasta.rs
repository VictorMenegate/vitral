//! "Começa com 41 mil tokens", no popover do contexto: com quanto um chat novo já começa na pasta
//! da conversa (a base medida pelo motor, `motor/peso.rs`) e, ao abrir a linha, os arquivos de
//! instrução que a pasta carrega, com o tamanho de cada um, e o que a sessão medida ligou
//! (servidores MCP, ferramentas, skills). O clique num arquivo o abre à direita.

use std::path::{Path, PathBuf};

use gpui_kit::component::{h_flex, popover::PopoverState, tooltip::Tooltip, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{contagem, icone},
    dialogos::com_til,
    onde_uso::tokens_curtos,
};
use crate::{
    modelo::{Agente, quando_local},
    motor::{
        Motor,
        peso::{Carga, Instrucao, Origem, Peso, instrucoes, pasta_da_memoria},
    },
    tema::{Cores, Lucide, Tipo as _, esp, r},
};

/// Uma linha do popover do contexto: o rótulo na coluna de 104 e o valor ao lado (o desenho das
/// linhas Modelo, Conta e Tokens de `uso_da_conversa`).
pub(super) fn linha_do_contexto(rotulo: &'static str, valor: impl IntoElement) -> Div {
    h_flex()
        .h(r(28.))
        .gap(esp::E8)
        .child(div().flex_none().w(r(104.)).metadado().text_color(Cores::tenue()).child(rotulo))
        .child(
            h_flex()
                .flex_1()
                .min_w_0()
                .gap(esp::E6)
                .text_size(r(13.))
                .line_height(r(18.))
                .text_color(Cores::texto())
                .child(valor),
        )
}

/// "850 B", "12,4 KB", "1,2 MB".
pub(super) fn tamanho(bytes: u64) -> String {
    let decimal = |valor: f64| format!("{valor:.1}").replace('.', ",");
    match bytes {
        0..=999 => format!("{bytes} B"),
        1_000..=999_949 => format!("{} KB", decimal(bytes as f64 / 1e3)),
        _ => format!("{} MB", decimal(bytes as f64 / 1e6)),
    }
}

/// "3 de 4 servidores MCP ligados · 42 ferramentas · 12 skills": o que a sessão medida carregou.
pub(super) fn texto_da_carga(carga: &Carga) -> String {
    let mcp = match (carga.mcp_ligados, carga.mcp) {
        (_, 0) => "nenhum servidor MCP".to_string(),
        (ligados, total) if ligados == total => {
            format!("{} {}", contagem(total as usize, "servidor MCP", "servidores MCP"), if total == 1 { "ligado" } else { "ligados" })
        }
        (ligados, total) => format!("{ligados} de {total} servidores MCP ligados"),
    };
    let mut partes = vec![mcp, contagem(carga.ferramentas as usize, "ferramenta", "ferramentas")];
    if carga.skills > 0 {
        partes.push(contagem(carga.skills as usize, "skill", "skills"));
    }
    partes.join(" · ")
}

fn rotulo_da_origem(origem: Origem) -> &'static str {
    match origem {
        Origem::Usuario => "do usuário",
        Origem::Pasta => "da pasta",
        Origem::Importado => "por @import",
        Origem::Memoria => "índice da memória",
    }
}

/// Como o painel da direita recebe o arquivo: dentro da pasta do chat, a pasta e o caminho
/// relativo; fora dela, a pasta do arquivo e o nome.
fn para_abrir(arquivo: &Path, pasta: &Path) -> (PathBuf, String) {
    match arquivo.strip_prefix(pasta) {
        Ok(relativo) if !relativo.as_os_str().is_empty() => (pasta.to_path_buf(), relativo.to_string_lossy().replace('\\', "/")),
        _ => (
            arquivo.parent().unwrap_or(Path::new("/")).to_path_buf(),
            arquivo.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
        ),
    }
}

/// O estado da linha no popover aberto: recolhida ou não, e a lista lida do disco (uma vez por
/// abertura do popover, ao abrir a linha).
#[derive(Default)]
struct Vista {
    aberta: bool,
    lista: Option<Vec<Instrucao>>,
}

fn ler_instrucoes(pasta: &Path) -> Vec<Instrucao> {
    let casa = dirs::home_dir();
    let Some(config) = crate::importar::pasta_do_claude() else {
        return Vec::new();
    };
    let memoria = pasta_da_memoria(pasta, &config, casa.as_deref());
    instrucoes(pasta, &config, casa.as_deref(), Some(&memoria))
}

/// As linhas do peso da pasta para o popover do contexto do chat; nada num chat do Codex (ele não
/// diz o contexto, e as instruções dele são outras).
pub(super) fn secao(
    motor: &Entity<Motor>,
    chat_id: &str,
    janela: WeakEntity<Janela>,
    popover: WeakEntity<PopoverState>,
    window: &mut Window,
    cx: &mut App,
) -> Option<AnyElement> {
    let (pasta, peso): (PathBuf, Option<Peso>) = {
        let m = motor.read(cx);
        let c = m.chat(chat_id).filter(|c| c.agente == Agente::Claude)?;
        (c.cwd.clone(), m.peso_da_pasta(&c.cwd).cloned())
    };
    let vista = window.use_keyed_state(SharedString::from(format!("peso-da-pasta-{chat_id}")), cx, |_, _| Vista::default());
    let (aberta, lista) = {
        let v = vista.read(cx);
        (v.aberta, v.lista.clone().unwrap_or_default())
    };
    let valor = match &peso {
        Some(p) => div().child(tokens_curtos(p.tokens)),
        None => div().text_color(Cores::tenue()).child("sem medida ainda"),
    };
    let dica: SharedString = match &peso {
        Some(p) => format!(
            "O contexto com que um chat novo começa nesta pasta, medido {}\nClique para ver os arquivos de instrução",
            quando_local(p.em)
        ),
        None => "A medida aparece depois do primeiro turno de um chat novo nesta pasta\nClique para ver os arquivos de instrução".into(),
    }
    .into();
    let alternar = {
        let (vista, pasta) = (vista.clone(), pasta.clone());
        move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
            vista.update(cx, |v, cx| {
                v.aberta = !v.aberta;
                if v.aberta && v.lista.is_none() {
                    v.lista = Some(ler_instrucoes(&pasta));
                }
                cx.notify();
            });
        }
    };
    let cabeca = linha_do_contexto(
        "Começa com",
        h_flex()
            .flex_1()
            .min_w_0()
            .gap(esp::E6)
            .child(valor)
            .child(div().flex_1())
            .child(div().flex_none().text_color(Cores::tenue()).child(icone(if aberta { Lucide::ChevronDown } else { Lucide::ChevronRight }, 12., window))),
    )
    .id("peso-da-pasta")
    .cursor_pointer()
    .hover(|s| s.bg(Cores::hover()))
    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
    .on_click(alternar);
    if !aberta {
        return Some(cabeca.into_any_element());
    }
    let total: u64 = lista.iter().map(|i| i.bytes).sum();
    let linhas = lista.iter().enumerate().map(|(n, instrucao)| {
        let (abrir_em, caminho) = para_abrir(&instrucao.caminho, &pasta);
        let (janela, popover) = (janela.clone(), popover.clone());
        let nome = instrucao.caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let onde = instrucao.caminho.parent().map(com_til).unwrap_or_default();
        h_flex()
            .id(("peso-instrucao", n))
            .items_start()
            .gap(esp::E8)
            .py(esp::E4)
            .cursor_pointer()
            .hover(|s| s.bg(Cores::hover()))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().text_size(r(13.)).line_height(r(18.)).text_color(Cores::texto()).child(nome))
                    .child(
                        div()
                            .metadado()
                            .text_color(Cores::apagado())
                            .child(format!("{} · {onde}", rotulo_da_origem(instrucao.origem))),
                    ),
            )
            .child(div().flex_none().metadado().text_color(Cores::tenue()).child(tamanho(instrucao.bytes)))
            .on_click(move |_, window, cx| {
                let _ = popover.update(cx, |p, cx| p.dismiss(window, cx));
                let (abrir_em, caminho) = (abrir_em.clone(), caminho.clone());
                let _ = janela.update(cx, |j, cx| j.abrir_arquivo(abrir_em, caminho, window, cx));
            })
    });
    let resumo = if lista.is_empty() {
        "Nenhum arquivo de instrução nesta pasta.".to_string()
    } else {
        format!("{} de instrução · {}", contagem(lista.len(), "arquivo", "arquivos"), tamanho(total))
    };
    let carga = peso.as_ref().and_then(|p| p.carga.as_ref()).map(texto_da_carga);
    Some(
        v_flex()
            .child(cabeca)
            .child(
                v_flex()
                    .id("peso-da-pasta-lista")
                    .max_h(r(240.))
                    .overflow_y_scroll()
                    .children(linhas),
            )
            .child(div().pt(esp::E4).metadado().text_color(Cores::tenue()).child(resumo))
            .children(carga.map(|c| div().metadado().text_color(Cores::tenue()).child(c)))
            .into_any_element(),
    )
}

#[cfg(test)]
mod testes {
    use super::{para_abrir, tamanho, texto_da_carga};
    use crate::motor::peso::Carga;
    use std::path::{Path, PathBuf};

    #[test]
    fn tamanho_do_arquivo() {
        assert_eq!(tamanho(0), "0 B");
        assert_eq!(tamanho(850), "850 B");
        assert_eq!(tamanho(12_440), "12,4 KB");
        assert_eq!(tamanho(1_240_000), "1,2 MB");
    }

    #[test]
    fn carga_em_uma_frase() {
        let carga = Carga { mcp_ligados: 3, mcp: 4, ferramentas: 42, skills: 12 };
        assert_eq!(texto_da_carga(&carga), "3 de 4 servidores MCP ligados · 42 ferramentas · 12 skills");
        let um = Carga { mcp_ligados: 1, mcp: 1, ferramentas: 1, skills: 0 };
        assert_eq!(texto_da_carga(&um), "1 servidor MCP ligado · 1 ferramenta");
        let dois = Carga { mcp_ligados: 2, mcp: 2, ferramentas: 30, skills: 0 };
        assert_eq!(texto_da_carga(&dois), "2 servidores MCP ligados · 30 ferramentas");
        assert_eq!(texto_da_carga(&Carga { ferramentas: 20, ..Carga::default() }), "nenhum servidor MCP · 20 ferramentas");
    }

    #[test]
    fn arquivo_dentro_e_fora_da_pasta_do_chat() {
        let pasta = Path::new("/casa/eita");
        assert_eq!(para_abrir(Path::new("/casa/eita/central/AGENTS.md"), pasta), (PathBuf::from("/casa/eita"), "central/AGENTS.md".to_string()));
        assert_eq!(para_abrir(Path::new("/casa/.claude/CLAUDE.md"), pasta), (PathBuf::from("/casa/.claude"), "CLAUDE.md".to_string()));
    }
}
