//! Comentar linhas do arquivo (ou do diff) aberto à direita e mandar tudo de uma vez: com um
//! trecho escolhido no número da linha, um campo curto guarda o comentário preso àquelas linhas, e
//! o trecho fica marcado no visor. Os comentários se acumulam por chat (o da caixa do painel
//! ativo), em vários arquivos, e sobrevivem à troca de arquivo. "Enviar N comentários" monta uma
//! mensagem só, com a menção de cada trecho (o formato de `mencao.rs`) seguida do comentário, e a
//! põe na caixa para revisar e enviar; os comentários guardados somem nessa hora.
//!
//! Vale para o arquivo aberto de qualquer jeito: pela lista de Mudanças do chat, pela seção Na
//! pasta ou por um link da conversa. Ficam só na memória: fechar o Vitral os larga.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui_kit::component::{
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu as _, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Reflexo as _, TamanhoBotao, botao_fantasma, botao_icone, contagem},
    mencao::{rotulo_da_mencao, texto_da_mencao},
};
use crate::{
    modelo::Chat,
    tema::{Cores, Lucide, Tipo as _, altura, dentro_da_borda, esp, r, raio},
};

/// Um comentário preso a um trecho de um arquivo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Comentario {
    /// A pasta e o caminho relativo com que o arquivo estava aberto (para voltar a ele).
    pub raiz: PathBuf,
    pub caminho: String,
    /// A primeira e a última linha (iguais numa linha só).
    pub linhas: (u32, u32),
    pub texto: String,
}

impl Comentario {
    fn inteiro(&self) -> PathBuf {
        self.raiz.join(&self.caminho)
    }

    fn no_mesmo_trecho(&self, outro: &Comentario) -> bool {
        self.raiz == outro.raiz && self.caminho == outro.caminho && self.linhas == outro.linhas
    }
}

/// Os comentários guardados de cada chat e o campo em que se escreve o próximo.
pub(super) struct Comentarios {
    por_chat: HashMap<String, Vec<Comentario>>,
    campo: Entity<InputState>,
    _enter: Subscription,
}

impl Comentarios {
    pub(super) fn new(window: &mut Window, cx: &mut Context<Janela>) -> Self {
        let campo = cx.new(|cx| InputState::new(window, cx).placeholder("Comentar este trecho… (Enter guarda)"));
        let enter = cx.subscribe_in(&campo, window, |this, _, evento: &InputEvent, window, cx| {
            if matches!(evento, InputEvent::PressEnter { .. }) {
                this.guardar_comentario(window, cx);
            }
        });
        Comentarios { por_chat: HashMap::new(), campo, _enter: enter }
    }

    fn do_chat(&self, chat: &str) -> &[Comentario] {
        self.por_chat.get(chat).map_or(&[], Vec::as_slice)
    }
}

/// Guarda o comentário do trecho: troca o que já havia nas mesmas linhas do mesmo arquivo; com o
/// texto vazio, tira o que havia.
fn guardar(lista: &mut Vec<Comentario>, novo: Comentario) {
    let novo = Comentario { texto: novo.texto.trim().to_string(), ..novo };
    let antigo = lista.iter().position(|c| c.no_mesmo_trecho(&novo));
    match (antigo, novo.texto.is_empty()) {
        (Some(i), true) => {
            lista.remove(i);
        }
        (Some(i), false) => lista[i] = novo,
        (None, true) => {}
        (None, false) => lista.push(novo),
    }
}

/// A ordem em que os comentários vão na mensagem: os arquivos na ordem em que foram comentados e,
/// em cada um, pelas linhas.
fn em_ordem(lista: &[Comentario]) -> Vec<&Comentario> {
    let mut arquivos: Vec<PathBuf> = Vec::new();
    for c in lista {
        let inteiro = c.inteiro();
        if !arquivos.contains(&inteiro) {
            arquivos.push(inteiro);
        }
    }
    let mut saida: Vec<&Comentario> = lista.iter().collect();
    saida.sort_by_key(|c| (arquivos.iter().position(|a| *a == c.inteiro()), c.linhas));
    saida
}

/// O caminho como a menção o escreve: relativo à pasta do chat quando o arquivo está nela; senão,
/// o caminho inteiro (como o Mencionar do painel).
fn caminho_da_mencao(inteiro: &Path, cwd: Option<&Path>) -> String {
    cwd.and_then(|cwd| inteiro.strip_prefix(dunce::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf())).ok().map(crate::git::caminho_em_texto))
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| inteiro.display().to_string())
}

/// A mensagem dos comentários: uma linha por comentário, com a menção do trecho
/// ("@src/ui/fila.rs#L120-134") e o texto depois.
pub(super) fn mensagem(lista: &[Comentario], cwd: Option<&Path>) -> String {
    em_ordem(lista)
        .into_iter()
        .map(|c| format!("{} {}", texto_da_mencao(&caminho_da_mencao(&c.inteiro(), cwd), Some(c.linhas)), c.texto))
        .collect::<Vec<_>>()
        .join("\n")
}

/// "Enviar 3 comentários".
fn rotulo_de_enviar(n: usize) -> String {
    format!("Enviar {}", contagem(n, "comentário", "comentários"))
}

/// Uma linha da lista dos comentários: "fila.rs:120-134 · troque o nome".
fn linha_do_comentario(c: &Comentario) -> String {
    format!("{} · {}", rotulo_da_mencao(&c.caminho, Some(c.linhas)), c.texto)
}

impl Janela {
    /// O chat dos comentários: o da caixa do painel ativo, para onde a mensagem vai.
    fn chat_dos_comentarios(&self) -> Option<String> {
        self.paineis.get(self.ativo).and_then(|p| p.chat.clone())
    }

    /// Os trechos comentados do arquivo aberto (pelo chat da caixa), para o visor marcá-los.
    pub(super) fn linhas_comentadas(&self, raiz: &Path, caminho: &str) -> Vec<(u32, u32)> {
        let Some(chat) = self.chat_dos_comentarios() else { return Vec::new() };
        self.comentarios.do_chat(&chat).iter().filter(|c| c.raiz == raiz && c.caminho == caminho).map(|c| c.linhas).collect()
    }

    /// O trecho escolhido no arquivo aberto, como um comentário ainda sem texto.
    fn trecho_escolhido(&self) -> Option<Comentario> {
        let (raiz, caminho) = self.arquivo_aberto.clone()?;
        Some(Comentario { raiz, caminho, linhas: self.arquivo_selecao?, texto: String::new() })
    }

    /// A escolha de linhas mudou: o campo mostra o comentário que o trecho já tem, ou fica vazio.
    pub(super) fn acertar_campo_do_comentario(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let texto = self
            .trecho_escolhido()
            .zip(self.chat_dos_comentarios())
            .and_then(|(trecho, chat)| self.comentarios.do_chat(&chat).iter().find(|c| c.no_mesmo_trecho(&trecho)).map(|c| c.texto.clone()))
            .unwrap_or_default();
        self.comentarios.campo.update(cx, |c, cx| c.set_value(texto, window, cx));
    }

    /// O Enter do campo e o botão Comentar: guarda o texto preso ao trecho escolhido (vazio, tira
    /// o comentário que ele tinha) e larga a escolha, que passa a aparecer marcada.
    fn guardar_comentario(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(trecho), Some(chat)) = (self.trecho_escolhido(), self.chat_dos_comentarios()) else { return };
        let texto = self.comentarios.campo.read(cx).value().to_string();
        let lista = self.comentarios.por_chat.entry(chat.clone()).or_default();
        guardar(lista, Comentario { texto, ..trecho });
        if lista.is_empty() {
            self.comentarios.por_chat.remove(&chat);
        }
        self.comentarios.campo.update(cx, |c, cx| c.set_value("", window, cx));
        self.arquivo_selecao = None;
        self.arquivo_ancora = None;
        cx.notify();
    }

    /// "Enviar N comentários": a mensagem vai para o fim da caixa do painel ativo, com cada
    /// menção já como pílula, e os comentários guardados somem. O que já estava escrito fica.
    fn enviar_comentarios(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(chat), Some(composer)) = (self.chat_dos_comentarios(), self.composer_ativo()) else { return };
        let Some(lista) = self.comentarios.por_chat.remove(&chat).filter(|l| !l.is_empty()) else { return };
        let cwd = self.motor.as_ref().and_then(|m| m.read(cx).chat(&chat).map(|c| c.cwd.clone()));
        let mensagem = mensagem(&lista, cwd.as_deref());
        composer.update(cx, |c, cx| {
            let atual = c.value().to_string();
            let emenda = if atual.is_empty() || atual.ends_with('\n') { "" } else { "\n" };
            let inicio = atual.len() + emenda.len();
            c.set_selected_range(atual.len()..atual.len(), cx);
            c.insert(format!("{emenda}{mensagem}"), window, cx);
            // As menções viram pílulas, de trás para a frente (a pílula tem o mesmo texto da faixa).
            if let Some(cwd) = &cwd {
                for (faixa, mencao) in super::mencao::mencoes_no_texto(&mensagem).into_iter().rev() {
                    let Some(inteiro) = super::links_arquivo::resolver_mencao(cwd, &mencao.caminho) else { continue };
                    let ficha = super::composer::ficha_da_mencao(&mensagem[faixa.clone()], &inteiro, mencao.linhas);
                    let _ = c.replace_range_with_token(inicio + faixa.start..inicio + faixa.end, ficha, window, cx);
                }
            }
            let fim = c.value().len();
            c.set_selected_range(fim..fim, cx);
            c.insert(" ", window, cx);
            c.set_scroll_offset(point(px(0.), px(-1.0e6)), cx);
        });
        // Vindo do arquivo na gaveta, a caixa está embaixo dela.
        self.gaveta = None;
        let foco = composer.read(cx).focus_handle(cx);
        window.focus(&foco, cx);
        cx.notify();
    }

    /// Larga os comentários guardados do chat da caixa.
    fn descartar_comentarios(&mut self, cx: &mut Context<Self>) {
        if let Some(chat) = self.chat_dos_comentarios() {
            self.comentarios.por_chat.remove(&chat);
            cx.notify();
        }
    }

    /// Abre o arquivo de um comentário guardado, com o trecho escolhido e o texto no campo (para
    /// corrigir, ou apagar o texto e tirar o comentário).
    fn ir_ao_comentario(&mut self, comentario: &Comentario, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_arquivo(comentario.raiz.clone(), comentario.caminho.clone(), window, cx);
        self.arquivo_linha.set(Some(comentario.linhas.0));
        self.arquivo_selecao = Some(comentario.linhas);
        self.arquivo_ancora = Some(comentario.linhas.0);
        self.acertar_campo_do_comentario(window, cx);
        cx.notify();
    }

    /// As faixas dos comentários no arquivo aberto, embaixo do cabeçalho: com linhas escolhidas, o
    /// campo do comentário; com comentários guardados, quantos são (a lista abre no botão) e o
    /// "Enviar N comentários".
    pub(super) fn faixas_dos_comentarios(&self, reserva: Option<Pixels>, na_gaveta: bool, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let Some(chat) = self.chat_dos_comentarios() else { return Vec::new() };
        let lista = self.comentarios.do_chat(&chat);
        let estreita = self.direita_estreita;
        let faixa = || {
            div()
                .flex()
                .flex_none()
                .items_center()
                .h(altura::CONTROLE)
                .pl(esp::E12)
                .map(|s| match reserva {
                    Some(folga) => s.pr(folga),
                    None => s.pr(esp::E8),
                })
                .gap(esp::E8)
                .border_b_1()
                .border_color(if na_gaveta { Cores::borda() } else { Cores::linha() })
        };
        let mut faixas = Vec::new();
        if self.arquivo_selecao.is_some() {
            let guardar = if estreita {
                botao_icone("guardar-comentario", Lucide::Check, "Guardar o comentário (Enter)", TamanhoBotao::Linha, window, cx)
            } else {
                botao_fantasma("guardar-comentario", None, "Comentar", TamanhoBotao::Linha, window, cx)
                    .tooltip("Guarda o comentário preso às linhas escolhidas (Enter). Vazio, tira o que o trecho tinha.")
            };
            faixas.push(
                faixa()
                    .child(div().flex_none().text_color(Cores::tenue()).child(super::componentes::icone(Lucide::MessageSquarePlus, 14., window)))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.comentarios.campo).appearance(false).w_full()))
                    .child(div().flex_none().child(guardar.on_click(cx.listener(|this, _, window, cx| this.guardar_comentario(window, cx)))))
                    .into_any_element(),
            );
        }
        if !lista.is_empty() {
            let arquivos = em_ordem(lista).iter().map(|c| c.inteiro()).collect::<std::collections::HashSet<_>>().len();
            let onde = format!("{} em {}", contagem(lista.len(), "comentário", "comentários"), contagem(arquivos, "arquivo", "arquivos"));
            let rotulo = if estreita { format!("Enviar {}", lista.len()) } else { rotulo_de_enviar(lista.len()) };
            faixas.push(
                faixa()
                    .child(self.menu_dos_comentarios(lista, window, cx))
                    .child(div().flex_1().min_w_0().truncate().metadado().text_color(Cores::tenue()).when(!estreita, |s| s.child(onde)))
                    .child(div().flex_none().child(self.botao_de_enviar("enviar-comentarios", rotulo, window, cx)))
                    .into_any_element(),
            );
        }
        faixas
    }

    /// O botão "Enviar N comentários" (o da faixa do arquivo e o do Este chat).
    fn botao_de_enviar(&self, id: &'static str, rotulo: String, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        botao_fantasma(id, Some(Lucide::Send.into()), rotulo, TamanhoBotao::Linha, window, cx)
            .tooltip("Põe na caixa uma mensagem só, com a menção de cada trecho e o comentário, para revisar e enviar")
            .on_click(cx.listener(|this, _, window, cx| this.enviar_comentarios(window, cx)))
    }

    /// O botão que abre a lista dos comentários guardados: cada um leva ao trecho dele, e
    /// "Descartar todos" larga a lista.
    fn menu_dos_comentarios(&self, lista: &[Comentario], window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let janela = cx.entity().downgrade();
        let itens: Vec<Comentario> = em_ordem(lista).into_iter().cloned().collect();
        botao_icone("ver-comentarios", Lucide::MessagesSquare, "Ver os comentários guardados", TamanhoBotao::Linha, window, cx)
            .dropdown_menu_with_anchor(Anchor::TopLeft, move |mut menu, _, _| {
                for comentario in &itens {
                    let (janela, alvo) = (janela.clone(), comentario.clone());
                    menu = menu.item(PopupMenuItem::new(linha_do_comentario(comentario)).icon(Lucide::MessageSquare).on_click(move |_, window, cx| {
                        let _ = janela.update(cx, |this, cx| this.ir_ao_comentario(&alvo, window, cx));
                    }));
                }
                let janela = janela.clone();
                menu.separator().item(PopupMenuItem::new("Descartar todos").icon(Lucide::Trash).on_click(move |_, _, cx| {
                    let _ = janela.update(cx, |this, cx| this.descartar_comentarios(cx));
                }))
            })
    }

    /// O cartão dos comentários no Este chat: os guardados do chat, cada um com o trecho e o
    /// texto (o clique abre o arquivo nele), e o "Enviar N comentários". Nada sem comentário.
    pub(super) fn cartao_dos_comentarios(&self, chat: &Chat, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        // O Enviar manda para a caixa do painel ativo: o cartão só aparece no chat dela.
        if self.chat_dos_comentarios().as_deref() != Some(chat.id.as_str()) {
            return None;
        }
        let lista = self.comentarios.do_chat(&chat.id);
        if lista.is_empty() {
            return None;
        }
        let linhas = em_ordem(lista).into_iter().enumerate().map(|(i, c)| {
            let alvo = c.clone();
            div()
                .id(("comentario-guardado", i))
                .flex()
                .items_start()
                .gap(esp::E8)
                .min_h(r(28.))
                .px(esp::E6)
                .py(esp::E4)
                .rounded(raio::CONTROLE)
                .cursor_pointer()
                .hover(|s| s.bg(Cores::hover()))
                .child(div().flex_none().mono_pequeno().text_color(Cores::tenue()).child(rotulo_da_mencao(&c.caminho, Some(c.linhas))))
                .child(div().flex_1().min_w_0().corpo_ui().text_color(Cores::texto()).child(c.texto.clone()))
                .on_click(cx.listener(move |this, _, window, cx| this.ir_ao_comentario(&alvo, window, cx)))
        });
        let descartar = botao_fantasma("descartar-comentarios", None, "Descartar", TamanhoBotao::Linha, window, cx)
            .on_click(cx.listener(|this, _, _, cx| this.descartar_comentarios(cx)));
        Some(
            div()
                .flex()
                .flex_col()
                .gap(esp::E8)
                .p(dentro_da_borda(12.))
                .rounded(raio::CARTAO)
                .bg(Cores::cartao_elevado())
                .border_1()
                .border_color(Cores::linha())
                .reflexo(raio::cartao())
                .child(div().rotulo().text_color(Cores::forte()).child("Comentários"))
                .child(div().flex().flex_col().gap(esp::E2).children(linhas))
                .child(
                    super::sem_arrastar()
                        .flex()
                        .flex_wrap()
                        .gap(esp::E4)
                        .child(self.botao_de_enviar("enviar-comentarios-do-chat", rotulo_de_enviar(lista.len()), window, cx))
                        .child(descartar),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use std::path::{Path, PathBuf};

    use super::{Comentario, guardar, linha_do_comentario, mensagem, rotulo_de_enviar};
    use crate::ui::mencao::{Mencao, mencoes_no_texto};

    fn comentario(caminho: &str, linhas: (u32, u32), texto: &str) -> Comentario {
        Comentario { raiz: PathBuf::from("/p/loja"), caminho: caminho.into(), linhas, texto: texto.into() }
    }

    #[test]
    fn guardar_troca_o_do_mesmo_trecho_e_o_vazio_tira() {
        let mut lista = Vec::new();
        guardar(&mut lista, comentario("src/a.rs", (3, 5), "  troque o nome  "));
        guardar(&mut lista, comentario("src/b.rs", (9, 9), "falta o erro"));
        assert_eq!(lista.len(), 2);
        assert_eq!(lista[0].texto, "troque o nome");
        // O mesmo trecho de novo troca o texto; outro trecho do mesmo arquivo soma.
        guardar(&mut lista, comentario("src/a.rs", (3, 5), "troque o nome para total"));
        guardar(&mut lista, comentario("src/a.rs", (1, 1), "sobra este use"));
        assert_eq!(lista.len(), 3);
        assert_eq!(lista[0].texto, "troque o nome para total");
        // Vazio tira o que havia, e não cria nada onde não havia.
        guardar(&mut lista, comentario("src/b.rs", (9, 9), "   "));
        guardar(&mut lista, comentario("src/c.rs", (2, 2), ""));
        assert_eq!(lista.iter().map(|c| c.caminho.as_str()).collect::<Vec<_>>(), ["src/a.rs", "src/a.rs"]);
    }

    #[test]
    fn a_mensagem_tem_a_mencao_de_cada_trecho_e_o_comentario() {
        let lista = [
            comentario("src/b.rs", (9, 9), "falta tratar o erro"),
            comentario("src/a.rs", (30, 42), "isto repete o de cima"),
            comentario("src/b.rs", (2, 4), "troque o nome"),
        ];
        let texto = mensagem(&lista, Some(Path::new("/p/loja")));
        // Os arquivos na ordem em que foram comentados; em cada um, pelas linhas.
        assert_eq!(
            texto,
            "@src/b.rs#L2-4 troque o nome\n@src/b.rs#L9 falta tratar o erro\n@src/a.rs#L30-42 isto repete o de cima"
        );
        // Cada trecho sai como uma menção que a caixa e o balão leem (as pílulas).
        let mencoes: Vec<Mencao> = mencoes_no_texto(&texto).into_iter().map(|(_, m)| m).collect();
        assert_eq!(
            mencoes,
            [
                Mencao { caminho: "src/b.rs".into(), linhas: Some((2, 4)) },
                Mencao { caminho: "src/b.rs".into(), linhas: Some((9, 9)) },
                Mencao { caminho: "src/a.rs".into(), linhas: Some((30, 42)) },
            ]
        );
        // Fora da pasta do chat, o caminho inteiro.
        assert_eq!(mensagem(&lista[..1], Some(Path::new("/p/outra"))), "@/p/loja/src/b.rs#L9 falta tratar o erro");
        assert_eq!(mensagem(&lista[..1], None), "@/p/loja/src/b.rs#L9 falta tratar o erro");
        assert_eq!(mensagem(&[], None), "");
    }

    #[test]
    fn os_rotulos_da_lista_e_do_botao() {
        assert_eq!(rotulo_de_enviar(1), "Enviar 1 comentário");
        assert_eq!(rotulo_de_enviar(3), "Enviar 3 comentários");
        assert_eq!(linha_do_comentario(&comentario("src/ui/fila.rs", (120, 134), "troque")), "fila.rs:120-134 · troque");
    }
}
