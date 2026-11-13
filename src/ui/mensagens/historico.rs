//! O histórico das conversas trazidas do Claude Code: lido do arquivo da sessão, em segundo plano,
//! quando a conversa aparece num painel, e guardado só na memória. Mostra os últimos itens, com
//! "Carregar mais" para os de antes. Enquanto a conversa não continuou no Vitral, acompanha o
//! arquivo (ela pode seguir aberta no VS Code); depois, para onde ela saiu.

use std::{cell::RefCell, time::Instant};

use super::*;
use crate::importar::{self, ITENS_POR_VEZ, Importada};

/// De quanto em quanto tempo o arquivo de uma conversa que ainda não saiu de lá é conferido.
const CONFERIR_A_CADA: Duration = Duration::from_secs(10);
/// Os índices dos itens do histórico ficam longe dos da conversa: as chaves dos blocos não se cruzam.
const BASE: usize = 1 << 40;

#[derive(Default)]
pub(in crate::ui) struct Historicos(RefCell<HashMap<String, Lido>>);

struct Lido {
    itens: Option<Rc<Vec<Item>>>,
    cortados: usize,
    erro: Option<String>,
    lendo: bool,
    conferido: Instant,
    /// O tamanho e o mtime do arquivo na última leitura, e até onde ela leu.
    marca: Marca,
    mostrar: usize,
}

type Marca = (u64, u64, Option<u64>);

fn marca_do_arquivo(i: &Importada) -> Marca {
    let metadados = std::fs::metadata(&i.arquivo).ok();
    let mtime = metadados
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs());
    (metadados.map_or(0, |m| m.len()), mtime, i.limite())
}

/// O que desenhar agora.
struct Vista {
    itens: Option<Rc<Vec<Item>>>,
    cortados: usize,
    erro: Option<String>,
    mostrar: usize,
}

impl Janela {
    /// O histórico guardado, e a leitura (ou a releitura, quando o arquivo mudou) em segundo plano.
    fn historico_de(&self, chat: &Chat, importada: &Importada, cx: &Context<Self>) -> Vista {
        let mut todos = self.historicos.0.borrow_mut();
        let lido = todos.entry(chat.id.clone()).or_insert_with(|| Lido {
            itens: None,
            cortados: 0,
            erro: None,
            lendo: false,
            conferido: Instant::now(),
            marca: (0, 0, None),
            mostrar: ITENS_POR_VEZ,
        });
        let nunca = lido.itens.is_none() && lido.erro.is_none();
        let conferir = !lido.lendo && (nunca || lido.conferido.elapsed() >= CONFERIR_A_CADA);
        if conferir {
            lido.conferido = Instant::now();
            let marca = marca_do_arquivo(importada);
            if nunca || marca != lido.marca {
                lido.lendo = true;
                let (id, arquivo, limite) = (chat.id.clone(), importada.arquivo.clone(), importada.limite());
                cx.spawn(async move |this, cx| {
                    let (lidos, marca) = cx
                        .background_spawn(async move { (importar::historico(&arquivo, limite), marca) })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if let Some(l) = this.historicos.0.borrow_mut().get_mut(&id) {
                            match lidos {
                                Ok(h) => (l.itens, l.cortados, l.erro) = (Some(Rc::new(h.itens)), h.cortados, None),
                                Err(erro) => l.erro = Some(erro.to_string()),
                            }
                            (l.marca, l.lendo, l.conferido) = (marca, false, Instant::now());
                        }
                        cx.notify();
                    });
                })
                .detach();
            }
        }
        Vista { itens: lido.itens.clone(), cortados: lido.cortados, erro: lido.erro.clone(), mostrar: lido.mostrar }
    }

    /// Os blocos do histórico, antes dos da conversa: "Carregar mais" quando há itens antes dos
    /// mostrados e os últimos itens, desenhados como os do chat, sem ações nem entrada animada.
    pub(super) fn blocos_do_historico(
        &self,
        motor: &Motor,
        chat: &Chat,
        nivel: LarguraPainel,
        window: &Window,
        cx: &Context<Self>,
    ) -> Vec<(Bloco, Option<usize>, AnyElement)> {
        let Some(importada) = &chat.importada else {
            return Vec::new();
        };
        let vista = self.historico_de(chat, importada, cx);
        let chave = |s: &str| format!("{}:historico-{s}", chat.id);
        let Some(itens) = vista.itens else {
            let texto = match &vista.erro {
                Some(erro) => format!(
                    "Não deu para ler a conversa do Claude Code em {} ({erro}).",
                    crate::ui::dialogos::com_til(&importada.arquivo)
                ),
                None => "Lendo a conversa do Claude Code…".to_string(),
            };
            let (bloco, elemento) = aviso_do_chat(&chave("lendo"), &texto, false, &Realce::default());
            return vec![(bloco, None, elemento)];
        };
        // O histórico do Claude Code fica fora da busca e da rolagem da conversa: ele é lido do
        // arquivo sob demanda, e nem todo ele está em memória para ser varrido.
        let (sem_busca, sem_rolagem) = (busca_conversa::Indice::default(), ScrollHandle::new());
        let contexto = Contexto {
            motor,
            chat,
            rodando: false,
            nivel,
            // O histórico trazido do Claude Code é só para ler: nada dele se corrige aqui.
            painel: 0,
            editavel: None,
            marco: None,
            // A busca da conversa não varre o histórico do Claude Code: ele é lido do arquivo,
            // sob demanda, e nem todo ele está em memória.
            busca: &sem_busca,
            rolagem: &sem_rolagem,
        };
        let inicio = itens.len().saturating_sub(vista.mostrar);
        let mut blocos: Vec<(Bloco, Option<usize>, AnyElement)> = Vec::new();
        if inicio > 0 {
            let id = chat.id.clone();
            let mais = inicio.min(ITENS_POR_VEZ);
            let botao = botao_fantasma(
                SharedString::from(chave("mais")),
                Some(Lucide::ChevronUp.into()),
                format!("Carregar mais ({mais} de {} anteriores)", inicio + vista.cortados),
                TamanhoBotao::Barra,
                window,
                cx,
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if let Some(l) = this.historicos.0.borrow_mut().get_mut(&id) {
                    l.mostrar += ITENS_POR_VEZ;
                }
                cx.notify();
            }));
            blocos.push((Bloco::Log, None, h_flex().w_full().justify_center().child(botao).into_any_element()));
        } else if vista.cortados > 0 {
            let texto = format!("As {} mensagens mais antigas ficaram só no Claude Code.", vista.cortados);
            let (bloco, elemento) = aviso_do_chat(&chave("cortados"), &texto, false, &Realce::default());
            blocos.push((bloco, None, elemento));
        }
        let mostrados: Vec<(usize, &Item)> = itens.iter().enumerate().skip(inicio).map(|(i, item)| (BASE + i, item)).collect();
        // A conversa trazida tem a hora de cada linha do arquivo dela: os dias também se separam.
        let horas: Vec<u64> = mostrados.iter().map(|(_, item)| item.em()).collect();
        let datas: HashMap<usize, String> = super::separadores_de_data(&horas, crate::modelo::agora()).into_iter().collect();
        let mut i = 0;
        while i < mostrados.len() {
            if let Some(dia) = datas.get(&i) {
                blocos.push((Bloco::Marco, None, super::separador_de_dia(dia)));
            }
            if matches!(mostrados[i].1, Item::Ferramenta { .. }) {
                let mut fim = i + 1;
                while fim < mostrados.len() && matches!(mostrados[fim].1, Item::Ferramenta { .. }) && !datas.contains_key(&fim) {
                    fim += 1;
                }
                blocos.push((Bloco::Log, None, self.trilha(&contexto, &mostrados[i..fim], false, cx)));
                i = fim;
                continue;
            }
            let (indice, item) = mostrados[i];
            let (bloco, elemento) = self.item(&contexto, indice, item, window, cx);
            blocos.push((bloco, None, elemento));
            i += 1;
        }
        blocos
    }
}
