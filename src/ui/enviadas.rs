//! Ctrl+R na caixa de mensagem: as suas mensagens já enviadas, de todos os chats, para repetir um
//! pedido sem redigitar nem procurar no chat antigo (como o Ctrl+R de um terminal). A lista é a
//! paleta de escolha do app, com a busca sem acento e por palavras de `acentos.rs`; o Enter põe o
//! texto na caixa do painel ativo, sem enviar, junto ao fim do que já estava escrito.
//!
//! A ordem e o "uma vez só" são puros (`enviadas`), testados no fim.

use std::{collections::HashSet, path::Path, rc::Rc};

use gpui_kit::component::{WindowExt as _, v_flex};
use gpui_kit::*;

use super::{
    Janela, acentos,
    componentes::{EventoEscolha, GrupoDeEscolha, ItemDeEscolha, ListaDeEscolha, Simbolo, desenho_teclas, fio_de_pontos, largura},
    dialogos::{cabecalho, moldura, nome_da_pasta, topo},
    fila::juntar_na_caixa,
    tempo_relativo,
};
use crate::{
    modelo::{Chat, Item},
    tema::{Cores, Lucide, Tipo as _, r},
};

// Ctrl+R: a lista das mensagens já enviadas.
actions!(vitral, [BuscarEnviadas]);

/// Quantas mensagens a lista mostra de cada vez (a busca estreita o resto).
const MAX_NA_LISTA: usize = 60;
/// Quantas letras do começo da mensagem vão na linha.
const LETRAS_DA_LINHA: usize = 120;

/// De onde é a mensagem, na ordem em que os grupos aparecem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Origem {
    DesteChat,
    DestaPasta,
    Outros,
}

impl Origem {
    fn titulo(self) -> &'static str {
        match self {
            Origem::DesteChat => "Deste chat",
            Origem::DestaPasta => "Desta pasta",
            Origem::Outros => "Outros chats",
        }
    }
}

/// Uma mensagem que você já enviou.
#[derive(Clone, Debug, PartialEq)]
struct Enviada {
    texto: String,
    /// O texto pronto para a busca (sem acento, em minúsculas).
    dobrado: String,
    chat: String,
    pasta: String,
    /// Quando foi (0: conversa de antes da hora nas mensagens).
    em: u64,
    origem: Origem,
}

/// As mensagens do usuário de todos os chats: primeiro as do chat aberto, depois as dos outros
/// chats da mesma pasta, depois o resto; em cada grupo, da mais recente à mais antiga. A mesma
/// mensagem (o mesmo texto, sem os espaços das pontas) aparece uma vez só, no melhor lugar dela.
/// `avulsos` é a pasta dos chats Sem pasta, que não contam como "a mesma pasta".
fn enviadas(chats: &[Chat], aberto: Option<&str>, avulsos: &Path) -> Vec<Enviada> {
    let sem_pasta = |c: &Chat| c.cwd.starts_with(avulsos);
    let pasta_aberta = aberto.and_then(|id| chats.iter().find(|c| c.id == id)).filter(|c| !sem_pasta(c)).map(|c| c.cwd.as_path());
    // (origem, quando, ordem de chegada): a ordem de chegada desempata as do mesmo segundo e as
    // conversas sem hora, que seguem a idade do chat.
    let mut todas: Vec<((Origem, std::cmp::Reverse<u64>, std::cmp::Reverse<usize>), Enviada)> = Vec::new();
    for chat in chats {
        let origem = if aberto == Some(chat.id.as_str()) {
            Origem::DesteChat
        } else if pasta_aberta == Some(chat.cwd.as_path()) {
            Origem::DestaPasta
        } else {
            Origem::Outros
        };
        let pasta = if sem_pasta(chat) { "Sem pasta".to_string() } else { nome_da_pasta(&chat.cwd) };
        for item in &chat.itens {
            let Item::Usuario { texto, em, .. } = item else {
                continue;
            };
            let texto = texto.trim();
            if texto.is_empty() {
                continue;
            }
            let quando = if *em > 0 { *em } else { chat.criado_em };
            let ordem = todas.len();
            todas.push((
                (origem, std::cmp::Reverse(quando), std::cmp::Reverse(ordem)),
                Enviada {
                    texto: texto.to_string(),
                    dobrado: acentos::normalizar(texto),
                    chat: chat.titulo.clone(),
                    pasta: pasta.clone(),
                    em: *em,
                    origem,
                },
            ));
        }
    }
    todas.sort_by(|a, b| a.0.cmp(&b.0));
    let mut vistas: HashSet<String> = HashSet::new();
    todas.into_iter().map(|(_, e)| e).filter(|e| vistas.insert(e.texto.clone())).collect()
}

/// As posições das mensagens que têm todas as palavras da busca, na ordem da lista, até `limite`.
fn achadas(lista: &[Enviada], busca: &str, limite: usize) -> Vec<usize> {
    let palavras = acentos::palavras(busca);
    lista
        .iter()
        .enumerate()
        .filter(|(_, e)| acentos::tem_todas(&e.dobrado, &palavras))
        .map(|(i, _)| i)
        .take(limite)
        .collect()
}

/// O começo da mensagem numa linha só.
fn linha_da_mensagem(texto: &str) -> String {
    let junto = texto.split_whitespace().collect::<Vec<_>>().join(" ");
    junto.chars().take(LETRAS_DA_LINHA).collect()
}

/// Os grupos da paleta para uma busca: a chave de cada item é a posição dele na lista.
fn grupos(lista: &[Enviada], busca: &str) -> Vec<GrupoDeEscolha> {
    let mut saida: Vec<GrupoDeEscolha> = Vec::new();
    let mut origem_do_ultimo = None;
    for i in achadas(lista, busca, MAX_NA_LISTA) {
        let e = &lista[i];
        if origem_do_ultimo != Some(e.origem) {
            origem_do_ultimo = Some(e.origem);
            saida.push(GrupoDeEscolha { titulo: Some(e.origem.titulo().into()), itens: Vec::new() });
        }
        let quando = if e.em > 0 { format!("  ·  {}", tempo_relativo(e.em)) } else { String::new() };
        let onde = match e.origem {
            Origem::DesteChat => format!("{}{quando}", e.chat),
            _ => format!("{}  ·  {}{quando}", e.chat, e.pasta),
        };
        if let Some(grupo) = saida.last_mut() {
            grupo.itens.push(ItemDeEscolha {
                simbolo: Some(Simbolo::icone(Lucide::CornerLeftUp)),
                descricao: Some(onde.into()),
                ..ItemDeEscolha::new(i.to_string(), linha_da_mensagem(&e.texto))
            });
        }
    }
    saida
}

/// "↑ ↓ escolher": as teclas da lista e o que fazem, no rodapé.
fn dica_teclas(teclas: &[&'static str], texto: &'static str) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(r(6.))
        .metadado()
        .text_color(Cores::tenue())
        .child(desenho_teclas(teclas.iter().map(|t| SharedString::from(*t)).collect()))
        .child(texto)
}

impl Janela {
    /// Ctrl+R: abre a lista das mensagens já enviadas; o Enter põe a escolhida na caixa do painel
    /// ativo (o Ctrl+Z desfaz, como no × da fila) e o Esc devolve o foco à caixa.
    pub(super) fn buscar_enviadas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(motor), Some(painel)) = (self.motor.clone(), self.paineis.get(self.ativo).map(|p| p.id)) else {
            return;
        };
        let lista = {
            let m = motor.read(cx);
            Rc::new(enviadas(&m.chats, self.selecionado.as_deref(), &m.dados.pasta_avulso("")))
        };
        let vista = cx.new(|cx| {
            let mut vista = ListaDeEscolha::new("Buscar nas suas mensagens…", window, cx);
            vista.definir(grupos(&lista, ""), None, cx);
            let filtro = lista.clone();
            vista.filtro_proprio(move |busca| grupos(&filtro, busca));
            vista.rodape(|window, _| {
                v_flex()
                    .w_full()
                    .gap(r(6.))
                    .child(fio_de_pontos(Cores::fio(), window))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(r(12.))
                            .w_full()
                            .child(div().flex_1())
                            .child(dica_teclas(&["↑", "↓"], "escolher"))
                            .child(dica_teclas(&["Enter"], "pôr na caixa")),
                    )
                    .into_any_element()
            });
            vista
        });
        self._assinatura_dialogo = Some(cx.subscribe_in(&vista, window, move |this, _, evento: &EventoEscolha, window, cx| {
            let EventoEscolha::Escolheu(chave) = evento else {
                return;
            };
            window.close_dialog(cx);
            let Some(enviada) = chave.parse::<usize>().ok().and_then(|i| lista.get(i)) else {
                return;
            };
            let Some(composer) = this.paineis.iter().find(|p| p.id == painel).map(|p| p.composer.clone()) else {
                return;
            };
            let atual = composer.read(cx).value().to_string();
            // Pelo `por_texto` da caixa: o Ctrl+Z traz de volta o que estava escrito.
            this.escrever_na_caixa(painel, &juntar_na_caixa(&atual, &enviada.texto), window, cx);
        }));
        // O foco passa antes pela caixa: é para ela que ele volta quando a lista fecha.
        self.foco_antes_do_dialogo(window, cx);
        let no_dialogo = vista.clone();
        window.open_dialog(cx, move |dialogo, window, _| {
            moldura(dialogo, largura::G, window)
                .title(topo(cabecalho("Suas mensagens", Some("De todos os chats, as mais recentes primeiro".into()))))
                .child(no_dialogo.clone())
                .on_ok(|_, _, _| false)
        });
        vista.update(cx, |l, cx| l.abrir(window, cx));
    }
}

#[cfg(test)]
mod testes {
    use std::path::PathBuf;

    use super::{Origem, achadas, enviadas, linha_da_mensagem};
    use crate::modelo::{Chat, Item};

    fn chat(id: &str, pasta: &str, mensagens: &[(&str, u64)]) -> Chat {
        let mut chat = Chat::novo(id.into(), id, PathBuf::from(pasta));
        chat.criado_em = 1;
        for (texto, em) in mensagens {
            chat.itens.push(Item::Usuario {
                texto: texto.to_string(),
                anexos: Vec::new(),
                ultrathink: false,
                ultracode: false,
                em: *em,
                marca: None,
            });
            chat.itens.push(Item::Assistente { texto: "feito".into(), em: *em });
        }
        chat
    }

    fn textos(lista: &[super::Enviada]) -> Vec<(&str, Origem)> {
        lista.iter().map(|e| (e.texto.as_str(), e.origem)).collect()
    }

    #[test]
    fn primeiro_o_chat_aberto_depois_a_pasta_e_o_resto_do_mais_recente_ao_mais_antigo() {
        let chats = vec![
            chat("outro", "/p/loja", &[("confere o deploy", 50), ("roda a revisão", 90)]),
            chat("aberto", "/p/central", &[("primeira", 10), ("segunda", 20)]),
            chat("vizinho", "/p/central", &[("da mesma pasta", 5)]),
            chat("avulso", "/dados/avulsos/avulso", &[("sem pasta", 99)]),
        ];
        let lista = enviadas(&chats, Some("aberto"), &PathBuf::from("/dados/avulsos"));
        assert_eq!(
            textos(&lista),
            [
                ("segunda", Origem::DesteChat),
                ("primeira", Origem::DesteChat),
                ("da mesma pasta", Origem::DestaPasta),
                ("sem pasta", Origem::Outros),
                ("roda a revisão", Origem::Outros),
                ("confere o deploy", Origem::Outros),
            ]
        );
        assert_eq!(lista[3].pasta, "Sem pasta");
        assert_eq!((lista[4].chat.as_str(), lista[4].pasta.as_str()), ("outro", "loja"));
        // Sem chat aberto, tudo é "outros", só pela hora.
        let lista = enviadas(&chats, None, &PathBuf::from("/dados/avulsos"));
        assert_eq!(lista.iter().map(|e| e.texto.as_str()).collect::<Vec<_>>(), [
            "sem pasta", "roda a revisão", "confere o deploy", "segunda", "primeira", "da mesma pasta"
        ]);
    }

    #[test]
    fn a_mensagem_repetida_aparece_uma_vez_so_no_melhor_lugar() {
        let chats = vec![
            chat("outro", "/p/loja", &[("roda a revisão", 90), ("  roda a revisão \n", 95), ("", 96)]),
            chat("aberto", "/p/central", &[("roda a revisão", 10), ("outra coisa", 20)]),
        ];
        let lista = enviadas(&chats, Some("aberto"), &PathBuf::from("/dados/avulsos"));
        // A repetida fica no chat aberto (o grupo de cima), e a vazia (só anexo) não entra.
        assert_eq!(textos(&lista), [("outra coisa", Origem::DesteChat), ("roda a revisão", Origem::DesteChat)]);
        // No mesmo chat, vale a mais recente.
        let lista = enviadas(&chats, None, &PathBuf::from("/dados/avulsos"));
        assert_eq!(lista.iter().map(|e| (e.texto.as_str(), e.em)).collect::<Vec<_>>(), [("roda a revisão", 95), ("outra coisa", 20)]);
    }

    #[test]
    fn a_conversa_sem_hora_segue_a_idade_do_chat_e_a_ordem_das_mensagens() {
        let mut antigo = chat("antigo", "/p/a", &[("um", 0), ("dois", 0)]);
        antigo.criado_em = 30;
        let chats = vec![antigo, chat("novo", "/p/b", &[("tres", 40)])];
        let lista = enviadas(&chats, None, &PathBuf::from("/x"));
        assert_eq!(lista.iter().map(|e| e.texto.as_str()).collect::<Vec<_>>(), ["tres", "dois", "um"]);
    }

    #[test]
    fn a_busca_e_sem_acento_e_por_palavras_em_qualquer_ordem() {
        let chats = vec![chat("c", "/p/a", &[("Confere a integração do deploy", 3), ("roda a revisão", 2), ("outra", 1)])];
        let lista = enviadas(&chats, None, &PathBuf::from("/x"));
        assert_eq!(achadas(&lista, "deploy integracao", 10), [0]);
        assert_eq!(achadas(&lista, "REVISAO", 10), [1]);
        assert_eq!(achadas(&lista, "", 2), [0, 1]);
        assert!(achadas(&lista, "nada disso", 10).is_empty());
    }

    #[test]
    fn a_linha_e_o_comeco_da_mensagem_sem_quebras() {
        assert_eq!(linha_da_mensagem("roda\n\n  a revisão\tinteira"), "roda a revisão inteira");
        assert_eq!(linha_da_mensagem(&"a".repeat(500)).chars().count(), super::LETRAS_DA_LINHA);
    }
}
