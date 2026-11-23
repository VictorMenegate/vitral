//! O histórico das conversas trazidas do Claude Code: lido do arquivo da sessão, em segundo plano,
//! quando a conversa aparece num painel, e guardado só na memória. As mensagens anteriores
//! aparecem ao subir, em páginas, preservando o ponto de leitura. Enquanto a conversa não continuou, acompanha o
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
    limite: usize,
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
pub(super) struct Vista {
    itens: Option<Rc<Vec<Item>>>,
    cortados: usize,
    erro: Option<String>,
}

impl Janela {
    /// O histórico guardado, e a leitura (ou a releitura, quando o arquivo mudou) em segundo plano.
    pub(super) fn historico_de(&self, chat: &Chat, importada: &Importada, cx: &Context<Self>) -> Vista {
        let mut todos = self.historicos.0.borrow_mut();
        let lido = todos.entry(chat.id.clone()).or_insert_with(|| Lido {
            itens: None,
            cortados: 0,
            erro: None,
            lendo: false,
            conferido: Instant::now(),
            marca: (0, 0, None),
            limite: importar::MAX_ITENS,
        });
        let nunca = lido.itens.is_none() && lido.erro.is_none();
        let conferir = !lido.lendo && (nunca || lido.conferido.elapsed() >= CONFERIR_A_CADA);
        if conferir {
            lido.conferido = Instant::now();
            let marca = marca_do_arquivo(importada);
            if nunca || marca != lido.marca {
                lido.lendo = true;
                let (id, arquivo, limite, maximo) = (chat.id.clone(), importada.arquivo.clone(), importada.limite(), lido.limite);
                cx.spawn(async move |this, cx| {
                    let (lidos, marca) = cx
                        .background_spawn(async move {
                            let lido = if maximo == importar::MAX_ITENS { importar::historico(&arquivo, limite) }
                                else { importar::historico_com_maximo(&arquivo, limite, maximo) };
                            (lido, marca)
                        })
                        .await;
                    let _ = this.update(cx, |this, cx| {
                        if let Some(l) = this.historicos.0.borrow_mut().get_mut(&id) {
                            match lidos {
                                Ok(h) => {
                                    if l.itens.is_none() || h.cortados < l.cortados {
                                        for p in this.paineis.iter_mut().filter(|p| p.chat.as_deref() == Some(id.as_str()) && !p.seguindo) {
                                            p.preservar_topo = Some((p.altura_conteudo, p.rolagem.offset()));
                                        }
                                    }
                                    (l.itens, l.cortados, l.erro) = (Some(Rc::new(h.itens)), h.cortados, None);
                                }
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
        Vista { itens: lido.itens.clone(), cortados: lido.cortados, erro: lido.erro.clone() }
    }

    /// Os blocos do histórico, antes dos da conversa: a janela de itens pedida por este painel,
    /// planejada como os itens do chat (só o que está à vista é montado), sem ações de edição nem
    /// entrada animada. O `contexto` é o do histórico: sem busca e sem nada a corrigir.
    pub(super) fn blocos_do_historico<'a>(
        &'a self,
        contexto: &'a Contexto<'a>,
        importada: &Importada,
        vista: &'a Vista,
        ix: usize,
        window: &'a Window,
        cx: &'a Context<Self>,
    ) -> Vec<Planejado<'a>> {
        let Some(itens) = &vista.itens else {
            let texto = match &vista.erro {
                Some(erro) => format!(
                    "Não deu para ler a conversa do Claude Code em {} ({erro}).",
                    crate::ui::dialogos::com_til(&importada.arquivo)
                ),
                None => "Lendo a conversa do Claude Code…".to_string(),
            };
            let (bloco, elemento) = aviso_do_chat(&texto, false, &Realce::default());
            return vec![Planejado::pronto(bloco, None, elemento)];
        };
        // O histórico do Claude Code fica fora da busca e da rolagem da conversa: ele é lido do
        // arquivo sob demanda, e nem todo ele está em memória para ser varrido.
        let inicio = itens.len().saturating_sub(self.paineis[ix].mostrar_historico);
        let mut blocos: Vec<Planejado<'a>> = Vec::new();
        let mostrados: Vec<(usize, &'a Item)> = itens.iter().enumerate().skip(inicio).map(|(i, item)| (BASE + vista.cortados + i, item)).collect();
        // A conversa trazida tem a hora de cada linha do arquivo dela: os dias também se separam.
        let horas: Vec<u64> = mostrados.iter().map(|(_, item)| item.em()).collect();
        let datas: HashMap<usize, String> = super::separadores_de_data(&horas, crate::modelo::agora()).into_iter().collect();
        let mut i = 0;
        while i < mostrados.len() {
            if let Some(dia) = datas.get(&i) {
                let chave = ChaveDoBloco { tipo: TipoDaChave::Dia, inicio: mostrados[i].0, fim: mostrados[i].0, marca: dia.len() as u64 };
                let dia = dia.clone();
                blocos.push(Planejado::de_tipo(Bloco::Dia, None, Some(chave), move || super::separador_de_dia(&dia)));
            }
            if matches!(mostrados[i].1, Item::Ferramenta { .. }) {
                let mut fim = i + 1;
                while fim < mostrados.len() && matches!(mostrados[fim].1, Item::Ferramenta { .. }) && !datas.contains_key(&fim) {
                    fim += 1;
                }
                let chamadas: Vec<(usize, &'a Item)> = mostrados[i..fim].to_vec();
                let chave = super::chave_da_faixa(TipoDaChave::TrilhaDoHistorico, &chamadas, &[chamadas.len() as u64]);
                let fica = super::fica_montado(chamadas.iter().map(|(_, item)| *item));
                blocos.push(Planejado::de_tipo(Bloco::Log, None, chave, move || self.trilha(contexto, &chamadas, false, cx)).fica_montado(fica));
                i = fim;
                continue;
            }
            let (indice, item) = mostrados[i];
            let chave = super::chave_do_item(TipoDaChave::Historico, indice, item, &[]);
            let mut bloco = Planejado::novo(None, chave, move || self.item(contexto, indice, item, window, cx)).fica_montado(super::fica_montado([item]));
            // As mensagens suas do histórico: o prepaint de cada uma diz onde ela ficou, e a
            // mensagem presa no alto escolhe entre elas e as do chat (ver `mensagens`).
            if let Item::Usuario { texto, .. } = item
                && !texto.trim().is_empty()
            {
                bloco.presa = Some(super::texto_da_presa(texto));
            }
            blocos.push(bloco);
            i += 1;
        }
        blocos
    }

    pub(super) fn mais_historico(&mut self, ix: usize) -> bool {
        let painel = &mut self.paineis[ix];
        let Some(id) = &painel.chat else { return false; };
        let mut historicos = self.historicos.0.borrow_mut();
        let Some(lido) = historicos.get_mut(id) else { return false; };
        let total = lido.itens.as_ref().map_or(0, |i| i.len());
        if total <= painel.mostrar_historico && lido.cortados == 0 { return false; }
        if lido.lendo { return false; }
        painel.mostrar_historico += ITENS_POR_VEZ;
        if painel.mostrar_historico > total && lido.cortados > 0 {
            lido.limite += importar::MAX_ITENS;
            lido.marca = (0, 0, None);
            lido.conferido = Instant::now() - CONFERIR_A_CADA;
        }
        true
    }
}
