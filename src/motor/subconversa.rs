//! A conversa de um subagente, aberta numa aba para ler: um chat de mentira, só de leitura, que
//! o motor guarda fora do estado (nunca é gravado, nem entra na lista) e monta com o que o Claude
//! Code gravou no arquivo do subagente (`importar::subagente`). Enquanto o subagente trabalha, o
//! arquivo é relido quando ele anda; fechada a aba, a conversa sai da memória.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui_kit::{AppContext as _, Context};

use super::{Motor, TextoChegando};
use crate::{
    claude, importar,
    modelo::{Chat, Execucao, Item, agora},
};

/// O começo do id de uma conversa de subagente: `sub:<chat>:<chamada>`.
const PREFIXO: &str = "sub:";

/// O id da conversa do subagente que a chamada `chamada` do chat `chat` abriu.
pub fn id_da_subconversa(chat: &str, chamada: &str) -> String {
    format!("{PREFIXO}{chat}:{chamada}")
}

/// O chat e a chamada de um id de conversa de subagente; `None` no id de um chat comum.
pub fn partes_do_id(id: &str) -> Option<(&str, &str)> {
    let (chat, chamada) = id.strip_prefix(PREFIXO)?.split_once(':')?;
    (!chat.is_empty() && !chamada.is_empty()).then_some((chat, chamada))
}

/// O subagente da chamada `chamada` ainda trabalha: o turno roda, a chamada não tem resultado e
/// nenhum aviso veio depois dela (o turno que para ou falha deixa um aviso na conversa; o de que
/// os agentes seguem em segundo plano não conta, ver `Item::aviso_que_fecha`).
pub fn subagente_vivo(itens: &[Item], rodando: bool, chamada: &str) -> bool {
    if !rodando {
        return false;
    }
    let Some(posicao) = itens.iter().rposition(|item| matches!(item, Item::Ferramenta { id, .. } if id == chamada)) else {
        return false;
    };
    matches!(&itens[posicao], Item::Ferramenta { resultado: None, .. })
        && !itens[posicao..].iter().any(Item::aviso_que_fecha)
}

/// O aviso que abre a conversa montada com o que o cartão guardou.
pub const SO_O_CARTAO: &str =
    "O Claude Code não guarda mais a conversa inteira deste subagente. Aqui está o que o Vitral guardou: os passos e o relatório.";

/// O resultado de cada passo na conversa montada pelo cartão (ele só guarda o nome do passo).
pub const SEM_RESULTADO: &str = "O Vitral não guardou o resultado deste passo.";

/// O que o cartão do subagente guardou, como conversa: para quando o Claude Code não tem mais o
/// arquivo dele. Um aviso dizendo isso, os passos (o resultado de cada um o cartão não guarda,
/// e a linha diz isso ao abrir) e o relatório.
pub fn conversa_do_cartao(item: &Item) -> Vec<Item> {
    let Item::Ferramenta { id, nome, resultado, sub, em, .. } = item else {
        return Vec::new();
    };
    if !matches!(nome.as_str(), "Task" | "Agent") {
        return Vec::new();
    }
    let mut itens = vec![Item::Aviso { texto: SO_O_CARTAO.to_string(), erro: false, em: *em }];
    let passos = sub.as_ref().map(|s| s.passos.as_slice()).unwrap_or_default();
    itens.extend(passos.iter().enumerate().map(|(i, passo)| Item::Ferramenta {
        id: format!("{id}:passo:{i}"),
        nome: passo.split(" · ").next().unwrap_or_default().to_string(),
        resumo: passo.clone(),
        // Com `None` a linha sairia como "interrompida": o passo terminou, só não foi guardado.
        resultado: Some(SEM_RESULTADO.to_string()),
        erro: false,
        em: *em,
        detalhe: None,
        arquivo: None,
        sub: None,
        mudanca: None,
        durou: 0,
    }));
    if let Some(relatorio) = resultado.as_deref().map(str::trim).filter(|r| !r.is_empty()) {
        let fim = sub.as_ref().map_or(*em, |s| s.fim.max(*em));
        itens.push(Item::Assistente { texto: relatorio.to_string(), em: fim });
    }
    itens
}

/// O relatório que vai para o cartão do subagente: o que ele entregou pela chamada de entrega
/// (`SubRelatorio`), quando houve; senão, o resultado da chamada como veio.
pub fn relatorio_do_cartao(resultado: String, entregue: Option<String>) -> String {
    match entregue.filter(|texto| !texto.trim().is_empty()) {
        Some(texto) => texto,
        None => resultado,
    }
}

/// As chamadas (Task/Agent) dos subagentes que esperam uma resposta sua: cada pedido de
/// permissão pendente traz o id da chamada que o fez, e `chamadas` diz de qual subagente ela é.
/// Cada subagente uma vez, na ordem dos pedidos.
pub fn subagentes_esperando<'a>(pedidos: impl IntoIterator<Item = &'a str>, chamadas: &HashMap<String, String>) -> Vec<String> {
    let mut esperando: Vec<String> = Vec::new();
    for pai in pedidos.into_iter().filter_map(|pedido| chamadas.get(pedido)) {
        if !esperando.contains(pai) {
            esperando.push(pai.clone());
        }
    }
    esperando
}

/// De onde veio o que a conversa do subagente mostra.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fonte {
    /// A primeira leitura ainda não voltou (ou o subagente acabou de abrir e o arquivo dele ainda
    /// não existe).
    Lendo,
    /// O arquivo do Claude Code: a conversa inteira.
    Arquivo,
    /// O arquivo não existe mais: só o que o cartão guardou.
    Cartao,
}

/// A conversa de um subagente aberta numa aba.
pub struct Subconversa {
    /// O chat de mentira: o id é o `id_da_subconversa`, o título é a tarefa do subagente e os
    /// itens são a conversa dele.
    pub chat: Chat,
    /// O chat que abriu o subagente e a chamada (Task/Agent) que o abriu.
    pub pai: String,
    pub chamada: String,
    pub fonte: Fonte,
    /// O arquivo do Claude Code, depois de achado, com o tamanho e a hora dele na última leitura.
    arquivo: Option<PathBuf>,
    marca: (u64, u64),
    /// Uma leitura está em curso; e, se o subagente andou durante ela, vem outra em seguida.
    lendo: bool,
    de_novo: bool,
}

/// O tamanho e a hora de modificação (em milissegundos) de um arquivo: muda quando ele cresce.
fn marca_do_arquivo(arquivo: &Path) -> (u64, u64) {
    let Ok(dados) = std::fs::metadata(arquivo) else {
        return (0, 0);
    };
    let mexido = dados.modified().ok().and_then(|m| m.duration_since(std::time::UNIX_EPOCH).ok()).map_or(0, |d| d.as_millis() as u64);
    (dados.len(), mexido)
}

/// O que uma leitura em segundo plano devolve: o arquivo achado, a marca dele e a conversa.
struct Lida {
    arquivo: Option<PathBuf>,
    marca: (u64, u64),
    itens: Option<Vec<Item>>,
}

/// Acha o arquivo do subagente (ou usa o já achado) e lê a conversa dele. Bloqueia: roda em
/// segundo plano.
fn ler(projetos: Vec<PathBuf>, sessao: String, chamada: String, de: String, conhecido: Option<PathBuf>) -> Lida {
    let arquivo = conhecido.filter(|a| a.is_file()).or_else(|| importar::subagente::transcript(&projetos, &sessao, &chamada));
    // A marca antes da leitura: o que for gravado durante ela pede outra.
    let marca = arquivo.as_deref().map(marca_do_arquivo).unwrap_or_default();
    let itens = arquivo.as_deref().and_then(|a| importar::subagente::conversa(a, &de).ok());
    Lida { arquivo, marca, itens }
}

impl Motor {
    /// O chat com este id: um chat de verdade ou a conversa de um subagente aberta numa aba. É o
    /// que a interface desenha; dentro do motor quem age é sempre o `chat_gravado`.
    pub fn chat(&self, id: &str) -> Option<&Chat> {
        self.chat_gravado(id).or_else(|| self.subconversa(id).map(|s| &s.chat))
    }

    /// A conversa de subagente com este id, se está aberta.
    pub fn subconversa(&self, id: &str) -> Option<&Subconversa> {
        partes_do_id(id)?;
        self.subconversas.iter().find(|s| s.chat.id == id)
    }

    /// O subagente da chamada `chamada` do chat `chat` trabalha agora.
    pub fn subagente_vivo(&self, chat: &str, chamada: &str) -> bool {
        let rodando = self.execucao(chat).rodando();
        self.chat_gravado(chat).is_some_and(|c| subagente_vivo(&c.itens, rodando, chamada))
    }

    /// A execução da conversa de um subagente: rodando enquanto ele trabalha (a chamada em curso
    /// gira, a aba fica azul), parada depois.
    pub(super) fn execucao_do_subagente(&self, chat: &str, chamada: &str) -> Execucao {
        if !self.subagente_vivo(chat, chamada) {
            return Execucao::Parado;
        }
        let desde = self.chat_gravado(chat).and_then(|c| c.itens.iter().rev().find(|i| matches!(i, Item::Ferramenta { id, .. } if id == chamada)));
        Execucao::Rodando { desde: desde.map_or(0, Item::em) }
    }

    /// As chamadas (Task/Agent) dos subagentes deste chat que esperam uma resposta sua a um
    /// pedido de permissão.
    pub fn subagentes_esperando(&self, chat: &str) -> Vec<String> {
        let Some(chamadas) = self.chamadas_dos_subs.get(chat) else {
            return Vec::new();
        };
        subagentes_esperando(self.permissoes.iter().filter(|p| p.chat == chat).filter_map(|p| p.chamada.as_deref()), chamadas)
    }

    /// O processo do chat saiu ou outro começou: o que se sabia das chamadas dos subagentes dele
    /// não vale mais.
    pub(super) fn esquecer_subagentes(&mut self, chat: &str) {
        self.chamadas_dos_subs.remove(chat);
        self.segundo_plano.remove(chat);
        self.relatorios.retain(|(de, _), _| de != chat);
        self.comandos_parados.remove(chat);
    }

    /// Abre a conversa do subagente que a chamada `chamada` do chat `chat` abriu e devolve o id
    /// dela (o de uma aba). A leitura do arquivo vai em segundo plano; até ela voltar a conversa
    /// está vazia. `None` se o chat não existe. A chamada pode não estar entre os itens do chat:
    /// a do histórico trazido do Claude Code mora no arquivo de lá, e o subagente dela também.
    pub fn abrir_subconversa(&mut self, chat: &str, chamada: &str, cx: &mut Context<Self>) -> Option<String> {
        if chamada.is_empty() {
            return None;
        }
        let id = id_da_subconversa(chat, chamada);
        if self.subconversa(&id).is_none() {
            let pai = self.chat_gravado(chat)?;
            let (resumo, em) = pai
                .itens
                .iter()
                .rev()
                .find_map(|item| match item {
                    Item::Ferramenta { id, resumo, em, .. } if id == chamada => Some((resumo.clone(), *em)),
                    _ => None,
                })
                .unwrap_or_default();
            let titulo = resumo.split_once(" · ").map_or(resumo.as_str(), |(_, tarefa)| tarefa).trim();
            let mut conversa = Chat::novo(id.clone(), if titulo.is_empty() { "Subagente" } else { titulo }, pai.cwd.clone());
            conversa.agente = crate::modelo::Agente::Claude;
            conversa.criado_por = format!("chat:{chat}");
            conversa.sessao_criada = true;
            let quando = if em > 0 { em } else { agora() };
            (conversa.criado_em, conversa.atualizado_em, conversa.lido_em) = (quando, quando, quando);
            self.subconversas.push(Subconversa {
                chat: conversa,
                pai: chat.to_string(),
                chamada: chamada.to_string(),
                fonte: Fonte::Lendo,
                arquivo: None,
                marca: (0, 0),
                lendo: false,
                de_novo: false,
            });
        }
        self.ler_subconversa(&id, cx);
        Some(id)
    }

    /// Os ids das conversas de subagente abertas.
    pub fn subconversas_abertas(&self) -> impl Iterator<Item = &str> {
        self.subconversas.iter().map(|s| s.chat.id.as_str())
    }

    /// A aba fechou: a conversa do subagente sai da memória (o arquivo dela continua lá).
    pub fn fechar_subconversa(&mut self, id: &str) {
        self.subconversas.retain(|s| s.chat.id != id);
    }

    /// O subagente da chamada `chamada` andou (usou uma ferramenta, disse algo, terminou): se a
    /// conversa dele está aberta, o arquivo é relido.
    pub(super) fn subconversa_andou(&mut self, chat: &str, chamada: &str, cx: &mut Context<Self>) {
        if self.subconversas.is_empty() {
            return;
        }
        let id = id_da_subconversa(chat, chamada);
        if self.subconversa(&id).is_some() {
            self.ler_subconversa(&id, cx);
        }
    }

    /// Uma vez por segundo, enquanto algum chat trabalha: o arquivo de cada conversa de
    /// subagente aberta e ainda viva é conferido, e relido se cresceu. É o que traz o que o
    /// subagente diz entre uma chamada e outra, que o stream-json não avisa.
    pub fn conferir_subconversas(&mut self, cx: &mut Context<Self>) {
        let mexidas: Vec<String> = self
            .subconversas
            .iter()
            .filter(|s| !s.lendo)
            .filter(|s| match &s.arquivo {
                Some(arquivo) => marca_do_arquivo(arquivo) != s.marca,
                // Ainda sem arquivo: o subagente que acabou de abrir pode não ter gravado nada.
                None => s.fonte == Fonte::Lendo || self.subagente_vivo(&s.pai, &s.chamada),
            })
            .map(|s| s.chat.id.clone())
            .collect();
        for id in mexidas {
            self.ler_subconversa(&id, cx);
        }
    }

    /// Lê (ou relê) em segundo plano o arquivo da conversa do subagente. Com uma leitura em
    /// curso, só marca que é para ler de novo quando ela acabar.
    fn ler_subconversa(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(posicao) = self.subconversas.iter().position(|s| s.chat.id == id) else {
            return;
        };
        if self.subconversas[posicao].lendo {
            self.subconversas[posicao].de_novo = true;
            return;
        }
        let (pai, chamada, conhecido) = {
            let s = &self.subconversas[posicao];
            (s.pai.clone(), s.chamada.clone(), s.arquivo.clone())
        };
        let Some(chat) = self.chat_gravado(&pai) else {
            return;
        };
        // Onde o Claude Code pode ter gravado: a pasta do projeto na conta do chat e na normal
        // (as contas extras ligam o mesmo `projects`), e a da conversa trazida de lá.
        let mut projetos = claude::pastas_do_projeto(self.conta_para(chat).pasta().as_deref(), &chat.cwd);
        if let Some(pasta) = chat.importada.as_ref().and_then(|i| i.arquivo.parent()) {
            projetos.push(pasta.to_path_buf());
        }
        let (sessao, de) = (chat.sessao.clone(), format!("chat {} \"{}\"", chat.id, chat.titulo));
        (self.subconversas[posicao].lendo, self.subconversas[posicao].de_novo) = (true, false);
        let id = id.to_string();
        cx.spawn(async move |motor, cx| {
            let lida = cx.background_spawn(async move { ler(projetos, sessao, chamada, de, conhecido) }).await;
            let _ = motor.update(cx, |motor, cx| motor.subconversa_lida(&id, lida, cx));
        })
        .detach();
    }

    /// A leitura voltou: a conversa passa a ser o que o arquivo tem; sem arquivo, e com o
    /// subagente já parado, o que o cartão guardou.
    fn subconversa_lida(&mut self, id: &str, lida: Lida, cx: &mut Context<Self>) {
        let Some(posicao) = self.subconversas.iter().position(|s| s.chat.id == id) else {
            return;
        };
        let (pai, chamada, antes) = {
            let s = &self.subconversas[posicao];
            (s.pai.clone(), s.chamada.clone(), s.fonte)
        };
        let vivo = self.subagente_vivo(&pai, &chamada);
        let do_cartao = || {
            let item = self.chat_gravado(&pai)?.itens.iter().rev().find(|i| matches!(i, Item::Ferramenta { id, .. } if *id == chamada))?;
            Some(conversa_do_cartao(item))
        };
        let (itens, fonte) = match lida.itens {
            Some(itens) => (Some(itens), Fonte::Arquivo),
            // Sem arquivo: o que já veio dele fica; o subagente que ainda trabalha pode não ter
            // gravado nada; parado, o que o cartão guardou.
            None if antes == Fonte::Arquivo => (None, Fonte::Arquivo),
            None if vivo => (None, Fonte::Lendo),
            None => (do_cartao(), Fonte::Cartao),
        };
        let s = &mut self.subconversas[posicao];
        (s.arquivo, s.marca, s.fonte, s.lendo) = (lida.arquivo, lida.marca, fonte, false);
        if let Some(itens) = itens {
            let ultimo = itens.last().map_or(0, Item::em).max(s.chat.atualizado_em);
            (s.chat.atualizado_em, s.chat.lido_em) = (ultimo, ultimo);
            s.chat.itens = itens;
        }
        let de_novo = std::mem::take(&mut s.de_novo);
        // A conversa cresceu: só os painéis que a mostram redesenham (como o texto chegando). A
        // fonte mudou (chegou a primeira leitura, ou só há o cartão): o pé da aba diz outra coisa.
        if fonte == antes {
            cx.emit(TextoChegando { chats: vec![id.to_string()] });
        } else {
            cx.notify();
        }
        if de_novo {
            self.ler_subconversa(id, cx);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::Subagente;

    fn tarefa(id: &str, resultado: Option<&str>, sub: Option<Subagente>) -> Item {
        Item::Ferramenta {
            id: id.into(),
            nome: "Task".into(),
            resumo: "Task · Explorar o carrinho".into(),
            resultado: resultado.map(str::to_string),
            erro: false,
            em: 100,
            detalhe: None,
            arquivo: None,
            sub,
            mudanca: None,
            durou: 0,
        }
    }

    #[test]
    fn o_id_da_conversa_do_subagente_leva_o_chat_e_a_chamada() {
        let id = id_da_subconversa("ab12cd34ef", "toolu_01XN9Vempxf5");
        assert_eq!(id, "sub:ab12cd34ef:toolu_01XN9Vempxf5");
        assert_eq!(partes_do_id(&id), Some(("ab12cd34ef", "toolu_01XN9Vempxf5")));
        // O id de um chat comum não é de subagente, e um id pela metade também não.
        assert_eq!(partes_do_id("ab12cd34ef"), None);
        assert_eq!(partes_do_id("sub:ab12cd34ef"), None);
        assert_eq!(partes_do_id("sub::toolu_x"), None);
        assert_eq!(partes_do_id("sub:ab12cd34ef:"), None);
    }

    #[test]
    fn o_subagente_vive_enquanto_o_turno_roda_e_a_chamada_nao_tem_resultado() {
        let itens = vec![
            tarefa("parado", None, None),
            Item::Aviso { texto: "Interrompido.".into(), erro: false, em: 110 },
            tarefa("pronto", Some("feito"), None),
            tarefa("vivo", None, None),
        ];
        assert!(subagente_vivo(&itens, true, "vivo"));
        // Com resultado, terminou; sem resultado e com um aviso depois, o turno dele foi parado.
        assert!(!subagente_vivo(&itens, true, "pronto"));
        assert!(!subagente_vivo(&itens, true, "parado"));
        // Com o chat parado ninguém trabalha; a chamada que não existe também não.
        assert!(!subagente_vivo(&itens, false, "vivo"));
        assert!(!subagente_vivo(&itens, true, "outra"));
    }

    /// O agente em segundo plano continua vivo depois da resposta do turno: a aba dele segue
    /// acompanhando o arquivo. O aviso de que os agentes seguem não é o de um turno que parou.
    #[test]
    fn o_agente_em_segundo_plano_vive_depois_da_resposta() {
        let itens = vec![
            tarefa("vivo", None, None),
            Item::Assistente { texto: "Lancei.".into(), em: 105 },
            Item::Aviso { texto: crate::modelo::AGENTES_SEGUEM.into(), erro: false, em: 106 },
        ];
        assert!(subagente_vivo(&itens, true, "vivo"));
        assert!(!subagente_vivo(&itens, false, "vivo"));
    }

    /// Sem o arquivo do Claude Code (ele apaga as sessões velhas), a aba mostra o que o cartão
    /// guardou, e diz que é só isso.
    #[test]
    fn sem_o_arquivo_a_conversa_e_o_que_o_cartao_guardou() {
        let sub = Subagente { ferramentas: 2, passos: vec!["Grep · cupom".into(), "Read · views.py".into()], fim: 160, ..Default::default() };
        let itens = conversa_do_cartao(&tarefa("a", Some("Dois lugares mexem no cupom."), Some(sub)));
        let textos: Vec<String> = itens
            .iter()
            .map(|item| match item {
                Item::Aviso { texto, erro: false, .. } => format!("!:{texto}"),
                // Cada passo com o resultado que diz que ele não foi guardado: sem resultado
                // nenhum, a linha de uma chamada num turno parado sai como "interrompida".
                Item::Ferramenta { nome, resumo, resultado: Some(r), erro: false, .. } if r == SEM_RESULTADO => format!("F:{nome}:{resumo}"),
                Item::Assistente { texto, .. } => format!("A:{texto}"),
                outro => format!("{outro:?}"),
            })
            .collect();
        assert_eq!(
            textos,
            [
                "!:O Claude Code não guarda mais a conversa inteira deste subagente. Aqui está o que o Vitral guardou: os passos e o relatório.",
                "F:Grep:Grep · cupom",
                "F:Read:Read · views.py",
                "A:Dois lugares mexem no cupom.",
            ]
        );
        // O cartão sem nada guardado (veio do histórico do Claude Code) só tem o aviso.
        assert_eq!(conversa_do_cartao(&tarefa("b", None, None)).len(), 1);
        // O que não é subagente não tem conversa.
        assert!(conversa_do_cartao(&Item::Assistente { texto: "oi".into(), em: 0 }).is_empty());
    }

    /// No modo automático, o resultado da chamada só diz que o relatório foi entregue: o cartão
    /// fica com o relatório de verdade.
    #[test]
    fn o_cartao_fica_com_o_relatorio_entregue() {
        let aviso = "This agent's report was delivered to you as a message from \"a1ef\" (its SubagentHandback call). Read it there; it is not repeated here.";
        assert_eq!(relatorio_do_cartao(aviso.into(), Some("A última palavra é banana.".into())), "A última palavra é banana.");
        // Sem entrega (o subagente respondeu em texto), o resultado como veio.
        assert_eq!(relatorio_do_cartao("Achei 3 arquivos.".into(), None), "Achei 3 arquivos.");
        assert_eq!(relatorio_do_cartao("Achei 3 arquivos.".into(), Some("  ".into())), "Achei 3 arquivos.");
    }

    /// Item 1: o subagente que pediu permissão é o que "espera você".
    #[test]
    fn o_pedido_de_permissao_diz_qual_subagente_espera() {
        let chamadas: HashMap<String, String> =
            [("toolu_a1", "pai_a"), ("toolu_a2", "pai_a"), ("toolu_b1", "pai_b")].into_iter().map(|(c, p)| (c.to_string(), p.to_string())).collect();
        // Dois pedidos do mesmo subagente contam uma vez; o pedido da conversa principal não conta.
        assert_eq!(subagentes_esperando(["toolu_a2", "toolu_principal", "toolu_a1"], &chamadas), ["pai_a"]);
        assert_eq!(subagentes_esperando(["toolu_b1", "toolu_a1"], &chamadas), ["pai_b", "pai_a"]);
        assert!(subagentes_esperando([], &chamadas).is_empty());
    }
}
