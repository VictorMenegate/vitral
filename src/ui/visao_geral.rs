//! O topo da lista em cada vista: na visão geral, o título (e, no fim da lista, os chats mais
//! recentes); num projeto, o quadrado, o nome, a descrição e as pastas ligadas; nos Avulsos, o que
//! eles são.

use std::{collections::HashSet, path::Path};

use gpui_kit::component::{
    h_flex,
    menu::DropdownMenu as _,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela, Vista,
    componentes::{Chip, QuadradoDeProjeto, Simbolo, TamanhoBotao, TamanhoChip, botao_icone, icone},
    dialogos::com_til,
    icone_projeto, menus,
};
use crate::{
    modelo::Chat,
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, esp, r, raio},
};

/// Quantos chats a visão geral mostra em "Recentes".
pub(super) const RECENTES: usize = 15;

/// Os `n` chats mais recentes entre os `candidatos` que ainda não apareceram nas seções de cima.
pub(super) fn recentes<'a>(candidatos: &[&'a Chat], acima: &HashSet<&str>, n: usize) -> Vec<&'a Chat> {
    let mut lista: Vec<&Chat> = candidatos.iter().copied().filter(|c| !acima.contains(c.id.as_str())).collect();
    lista.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
    lista.truncate(n);
    lista
}

/// A primeira linha de texto do README da pasta (sem títulos, imagens nem selos), cortada em 160.
pub(super) fn linha_do_readme(pasta: &Path) -> Option<String> {
    let texto = ["README.md", "readme.md", "README.markdown", "README", "README.txt"]
        .iter()
        .find_map(|nome| std::fs::read_to_string(pasta.join(nome)).ok())?;
    primeira_frase(&texto)
}

/// O primeiro parágrafo de verdade de um markdown: pula títulos, imagens, selos, HTML e cercas.
fn primeira_frase(texto: &str) -> Option<String> {
    let mut no_codigo = false;
    for linha in texto.lines().map(str::trim) {
        if linha.starts_with("```") {
            no_codigo = !no_codigo;
            continue;
        }
        let pula = linha.is_empty()
            || linha.starts_with('#')
            || linha.starts_with('!')
            || linha.starts_with('<')
            || linha.starts_with("[!")
            || linha.starts_with('|')
            || linha.chars().all(|c| matches!(c, '=' | '-' | '*' | '_'));
        if no_codigo || pula {
            continue;
        }
        let limpa: String = linha.replace("**", "").replace('`', "");
        return Some(limpa.chars().take(160).collect());
    }
    None
}

impl Janela {
    /// A descrição do projeto ou, sem ela, a primeira linha do README da pasta principal (lida uma
    /// vez por pasta; "do README" vai na dica).
    fn descricao_da_vista(&self, motor: &Motor, projeto: &str) -> Option<(String, bool)> {
        let p = motor.projeto(projeto)?;
        if !p.descricao.trim().is_empty() {
            return Some((p.descricao.lines().next().unwrap_or_default().trim().to_string(), false));
        }
        let pasta = p.principal()?.caminho.clone();
        let mut lidos = self.readmes.borrow_mut();
        let linha = lidos.entry(pasta.clone()).or_insert_with(|| linha_do_readme(&pasta)).clone()?;
        Some((linha, true))
    }

    /// O cabeçalho do projeto aberto na lista: o quadrado, o nome (o clique duplo abre Editar
    /// projeto), a descrição numa linha, o "…" com o menu do projeto e um chip por pasta ligada.
    pub(super) fn cabecalho_projeto(
        &self,
        motor_e: &Entity<Motor>,
        projeto: &str,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let motor = motor_e.read(cx);
        let (icone_p, cor) = icone_projeto::visual_do(motor, projeto);
        let nome = motor.nome_do_projeto(projeto);
        let descricao = self.descricao_da_vista(motor, projeto);
        let (janela, entidade, p_menu) = (cx.entity().downgrade(), motor_e.clone(), projeto.to_string());
        let mais = botao_icone("projeto-da-vista-menu", Lucide::Ellipsis, "Mais opções do projeto", TamanhoBotao::Linha, window, cx)
            .dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
                menus::itens_do_projeto(menu, janela.clone(), &entidade, &p_menu, true, window, cx)
            });
        let p_editar = projeto.to_string();
        let titulo = div()
            .id("projeto-da-vista-nome")
            .min_w_0()
            .truncate()
            .titulo_cartao()
            .text_color(Cores::forte())
            .cursor_pointer()
            .child(nome.clone())
            .tooltip(|window, cx| Tooltip::new("Clique duplo para editar o projeto").build(window, cx))
            .on_click(cx.listener(move |this, ev: &ClickEvent, window, cx| {
                if ev.click_count() >= 2 {
                    this.dialogo_editar_projeto(p_editar.clone(), window, cx);
                }
            }));
        let linha_descricao = descricao.map(|(texto, do_readme)| {
            let dica: SharedString = if do_readme {
                format!("Do README. Escreva a descrição em Editar projeto.\n{texto}").into()
            } else {
                texto.clone().into()
            };
            div()
                .id("projeto-da-vista-descricao")
                .min_w_0()
                .truncate()
                .metadado()
                .text_color(Cores::tenue())
                .when(do_readme, |d| d.italic())
                .child(texto)
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        });
        let chips: Vec<AnyElement> = motor
            .pastas_do(projeto)
            .iter()
            .enumerate()
            .map(|(i, pasta)| self.chip_da_pasta(projeto, i, &pasta.caminho, pasta.propria, window, cx))
            .collect();
        v_flex()
            .flex_none()
            .gap(esp::E8)
            .pb(esp::E8)
            .px(esp::E8)
            .child(
                h_flex()
                    .gap(esp::E12)
                    .child(QuadradoDeProjeto::new(icone_p, cor, 40.))
                    .child(v_flex().flex_1().min_w_0().child(titulo).children(linha_descricao))
                    .child(div().flex_none().child(mais)),
            )
            .when(!chips.is_empty(), |s| s.child(div().flex().flex_wrap().gap(esp::E4).children(chips)))
            .into_any_element()
    }

    /// O chip de uma pasta ligada: o nome, com o caminho na dica. A que sumiu fica em vermelho e
    /// o clique localiza; a pasta própria diz isso; o clique numa que existe abre os arquivos dela.
    fn chip_da_pasta(
        &self,
        projeto: &str,
        indice: usize,
        caminho: &Path,
        propria: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let existe = caminho.is_dir();
        let nome = caminho.file_name().map_or_else(|| caminho.display().to_string(), |n| n.to_string_lossy().to_string());
        let (rotulo, simbolo) = match (existe, propria) {
            (false, _) => (format!("{nome} · não encontrada"), Simbolo::icone_cor(Lucide::FolderX, Cores::vermelho())),
            (true, true) => ("pasta própria".to_string(), Simbolo::icone(Lucide::GitBranch)),
            (true, false) => (nome, Simbolo::icone(Lucide::Folder)),
        };
        let dica = if existe {
            com_til(caminho)
        } else {
            format!("{} não foi encontrada (disco desmontado ou pasta renomeada).\nClique para localizar.", com_til(caminho))
        };
        let (projeto, pasta) = (projeto.to_string(), caminho.to_path_buf());
        let mut chip = Chip::new(SharedString::from(format!("chip-pasta-{indice}")), rotulo)
            .tamanho(TamanhoChip::Pequeno)
            .simbolo(simbolo)
            .dica(dica);
        if !existe {
            chip = chip.cor_rotulo(Cores::vermelho());
        }
        let janela = cx.entity().downgrade();
        chip.on_click(move |_, window, cx| {
            let _ = janela.update(cx, |this, cx| {
                if existe {
                    this.mostrar_arquivos_da_pasta(&projeto, pasta.clone(), window, cx);
                } else {
                    this.localizar_pasta(projeto.clone(), indice, window, cx);
                }
            });
        })
        .construir(window, cx)
        .into_any_element()
    }

    /// O topo dos Avulsos: o que eles são.
    pub(super) fn cabecalho_avulsos(&self, window: &Window) -> AnyElement {
        h_flex()
            .flex_none()
            .gap(esp::E12)
            .pb(esp::E8)
            .px(esp::E8)
            .child(
                div()
                    .flex_none()
                    .size(r(40.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(raio::do_icone(40.))
                    .bg(Cores::hover())
                    .text_color(Cores::tenue())
                    .child(icone(Lucide::MessageCircle, 22., window)),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(div().truncate().titulo_cartao().text_color(Cores::forte()).child("Avulsos"))
                    .child(
                        div()
                            .truncate()
                            .metadado()
                            .text_color(Cores::tenue())
                            .child("Chats sem projeto, cada um numa pasta só dele"),
                    ),
            )
            .into_any_element()
    }

    /// A aba Arquivos do projeto, já na pasta (ou na cópia) escolhida.
    pub(super) fn mostrar_arquivos_da_pasta(
        &mut self,
        projeto: &str,
        pasta: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.pasta_arquivos.insert(projeto.to_string(), pasta);
        if self.vista.projeto() != Some(projeto) {
            self.mudar_vista(Vista::Projeto(projeto.to_string()), cx);
        }
        if self.aba != super::AbaLateral::Arquivos {
            self.trocar_aba(super::AbaLateral::Arquivos, window, cx);
        }
        self.mostrar_lista(window, cx);
    }
}

#[cfg(test)]
mod testes {
    use std::collections::HashSet;

    use super::{primeira_frase, recentes};
    use crate::modelo::{Chat, Papel};

    fn chat(id: &str, quando: u64) -> Chat {
        let mut c = Chat::novo(id.into(), Papel::Projeto, "p", id, "/tmp".into());
        c.atualizado_em = quando;
        c
    }

    #[test]
    fn recentes_sem_os_de_cima_e_no_limite() {
        let chats: Vec<Chat> = (0..20).map(|i| chat(&format!("c{i}"), 1000 + i)).collect();
        let refs: Vec<&Chat> = chats.iter().collect();
        let acima: HashSet<&str> = ["c19", "c17"].into_iter().collect();
        let lista = recentes(&refs, &acima, 15);
        assert_eq!(lista.len(), 15);
        // Do mais recente para o mais antigo, sem os que já estão em cima.
        assert_eq!(lista[0].id, "c18");
        assert_eq!(lista[1].id, "c16");
        assert!(lista.iter().all(|c| c.id != "c19" && c.id != "c17"));
        assert_eq!(lista.last().unwrap().id, "c3");
        // Com menos que o limite, todos.
        assert_eq!(recentes(&refs[..3], &HashSet::new(), 15).len(), 3);
    }

    #[test]
    fn a_frase_do_readme_pula_titulos_e_selos() {
        let readme = "# Ninee\n\n![logo](x.png)\n[![ci](b.svg)](l)\n\nLoja de roupas em **Django**.\nMais texto.";
        assert_eq!(primeira_frase(readme).as_deref(), Some("Loja de roupas em Django."));
        assert_eq!(primeira_frase("```\ncodigo\n```\nDepois do código").as_deref(), Some("Depois do código"));
        assert_eq!(primeira_frase("# Só título\n\n---\n"), None);
    }
}
