//! O que liga as conversas umas às outras e ao usuário: as ferramentas `listar_chats`,
//! `mensagem_para_chat` e `abrir_chat`, o freio de ciclo das mensagens automáticas entre dois chats
//! e o resumo do que aconteceu enquanto o usuário esteve fora. A lógica fica em funções puras,
//! testadas no fim.

use super::*;

/// Mensagens automáticas seguidas entre dois chats antes do freio.
pub const LIMITE_DO_CICLO: u32 = 6;

/// Por que o próximo turno começa numa sessão nova, com o bloco de passagem.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotivoPassagem {
    /// O Claude Code apagou a sessão gravada (`cleanupPeriodDays`).
    Expirou,
    /// O usuário corrigiu a última mensagem: a conversa voltou ao ponto antes dela, e a sessão
    /// do Claude Code não volta atrás; a próxima começa numa nova, já sem o que saiu.
    Reenvio,
    /// O chat trabalhava numa cópia isolada, que saiu do Vitral: a sessão ficou presa à pasta da
    /// cópia, e a próxima começa na pasta de origem.
    SaiuDaCopia,
    /// O usuário trocou quem responde (Claude ou Codex): a conversa de um não abre no outro.
    TrocouDeAgente,
}

/// "Enquanto você esteve fora": só na memória, montado das ocorrências; nunca vai para o disco.
#[derive(Clone, Debug, PartialEq)]
pub struct ResumoVolta {
    pub desde: u64,
    pub ate: u64,
    pub linhas: Vec<LinhaResumo>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LinhaResumo {
    /// "terminou", "precisa" ou "falhou".
    pub grupo: &'static str,
    pub chat: String,
    pub texto: String,
}

/// As mensagens automáticas entre pares de chats sem o usuário no meio (só na memória).
#[derive(Default)]
pub struct Ciclos {
    pares: HashMap<(String, String), u32>,
}

impl Ciclos {
    fn par(a: &str, b: &str) -> (String, String) {
        if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
    }

    /// Mais uma mensagem automática entre os dois: quantas já foram, com esta.
    pub fn contar(&mut self, a: &str, b: &str) -> u32 {
        let n = self.pares.entry(Self::par(a, b)).or_default();
        *n += 1;
        *n
    }

    /// O usuário escreveu para o chat: os pares dele recomeçam.
    pub fn zerar(&mut self, chat: &str) {
        self.pares.retain(|(a, b), _| a != chat && b != chat);
    }

    /// Os pares que passaram do limite e esperam o usuário.
    pub fn travados(&self) -> Vec<(String, String)> {
        let mut pares: Vec<(String, String)> =
            self.pares.iter().filter(|(_, n)| **n > LIMITE_DO_CICLO).map(|(p, _)| p.clone()).collect();
        pares.sort();
        pares
    }
}

// ---------- puras ----------

/// Caminho com a pasta pessoal como "~".
pub fn com_til(caminho: &Path) -> String {
    match dirs::home_dir().as_deref().and_then(|c| caminho.strip_prefix(c).ok()) {
        Some(resto) if resto.as_os_str().is_empty() => "~".into(),
        Some(resto) => format!("~/{}", resto.display()),
        None => caminho.display().to_string(),
    }
}

/// "agora", "há 12 min", "há 3 h", "há 2 dias".
pub fn ha_quanto(segundos: u64) -> String {
    match segundos {
        0..60 => "agora".into(),
        60..3600 => format!("há {} min", segundos / 60),
        3600..86_400 => format!("há {} h", segundos / 3600),
        _ if segundos < 2 * 86_400 => "há 1 dia".into(),
        _ => format!("há {} dias", segundos / 86_400),
    }
}

/// O chat que o texto cita pelo id: "a1b2c3", "chat a1b2c3", "#a1b2c3" ou "@a1b2c3".
pub fn achar_chat<'a>(chats: &'a [Chat], texto: &str) -> Option<&'a Chat> {
    let limpo = texto.trim().trim_start_matches(['@', '#']);
    let limpo = limpo.strip_prefix("chat ").unwrap_or(limpo).trim();
    chats.iter().find(|c| c.id == limpo)
}

/// O título do chat que outro chat abriu: o que ele deu, numa linha e com no máximo 60 letras; sem
/// título, a primeira linha do pedido.
pub fn titulo_do_chat_aberto(titulo: &str, pedido: &str) -> String {
    let linha = |t: &str| t.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or_default().to_string();
    let base = match linha(titulo) {
        t if t.is_empty() => linha(pedido),
        t => t,
    };
    if base.chars().count() <= 60 {
        return base;
    }
    let mut curto: String = base.chars().take(59).collect();
    curto.push('…');
    curto
}

/// Só o usuário falou neste turno (nem o Vitral, nem outro chat): só assim um chat abre outro.
pub fn turno_so_do_usuario(entradas: &[Entrada]) -> bool {
    !entradas.is_empty() && entradas.iter().all(|e| e.de.is_none())
}

/// O que o usuário perdeu fora da janela, das ocorrências do período (sem o que ele mesmo fez).
pub fn resumo_desde(desde: u64, ate: u64, ocorrencias: &[Ocorrencia]) -> Option<ResumoVolta> {
    let mut linhas: Vec<LinhaResumo> = Vec::new();
    for o in ocorrencias.iter().filter(|o| o.quando > desde && o.quando <= ate && o.autor != "usuario") {
        let grupo = match o.tipo.as_str() {
            // "entrega" e "juntou" vêm das versões com ajudantes e cópias.
            "terminou" | "entrega" | "juntou" => "terminou",
            "permissao" | "ciclo" | "pergunta" => "precisa",
            "falhou" | "parou" | "conflito" => "falhou",
            _ => continue,
        };
        // Uma linha por chat em cada grupo: a mais nova (a junção vem depois da entrega).
        if !o.chat.is_empty() {
            linhas.retain(|l| !(l.grupo == grupo && l.chat == o.chat));
        }
        linhas.push(LinhaResumo { grupo, chat: o.chat.clone(), texto: o.texto.clone() });
    }
    if linhas.is_empty() {
        return None;
    }
    let ordem = |g: &str| ["terminou", "precisa", "falhou"].iter().position(|x| *x == g).unwrap_or(3);
    linhas.sort_by_key(|l| ordem(l.grupo));
    Some(ResumoVolta { desde, ate, linhas })
}

// ---------- no motor ----------

impl Motor {
    /// Os pares de chats parados no freio de ciclo.
    pub fn ciclos_travados(&self) -> Vec<(String, String)> {
        self.ciclos.travados()
    }

    /// A janela perdeu o foco (ou o app vai fechar): a hora fica gravada.
    pub fn usuario_saiu(&mut self) {
        self.visto_em = agora();
        self.salvar();
    }

    /// A janela voltou depois de `minutos_para_resumo` fora (ou abriu, com uma saída gravada): monta o
    /// resumo do que aconteceu, só na memória.
    pub fn usuario_voltou(&mut self, cx: &mut Context<Self>) {
        let desde = self.visto_em;
        let ate = agora();
        if desde == 0 || ate.saturating_sub(desde) < self.config.minutos_para_resumo * 60 {
            return;
        }
        if let Some(resumo) = resumo_desde(desde, ate, &self.ocorrencias) {
            self.resumo_volta = Some(resumo);
            cx.notify();
        }
    }

    pub fn fechar_resumo(&mut self, cx: &mut Context<Self>) {
        self.resumo_volta = None;
        cx.notify();
    }

    /// As ferramentas que ligam um chat aos outros (`listar_chats`, `mensagem_para_chat`,
    /// `abrir_chat`). Devolve a chamada que não é daqui.
    pub(super) fn ferramenta_de_conversa(&mut self, chat: &Chat, chamada: Chamada, cx: &mut Context<Self>) -> Result<(), Chamada> {
        let a = chamada.argumentos.clone();
        let texto = |chave: &str| a[chave].as_str().unwrap_or_default().trim().to_string();
        let resposta = match chamada.ferramenta.as_str() {
            "listar_chats" => Resposta::ok(self.texto_listar_chats(chat, &a)),
            "mensagem_para_chat" => self.mensagem_para_chat(chat, &texto("chat"), texto("texto"), cx),
            "abrir_chat" => self.abrir_chat_irmao(chat, &texto("titulo"), texto("pedido"), cx),
            _ => return Err(chamada),
        };
        let _ = chamada.resposta.try_send(resposta);
        Ok(())
    }

    /// "rodando", "esperando você", "na fila", "erro", "parada: motivo" ou "parado".
    fn estado_para_o_claude(&self, c: &Chat) -> String {
        match self.execucao(&c.id) {
            Execucao::EsperandoVoce => "esperando você".into(),
            Execucao::Rodando { .. } => "rodando".into(),
            Execucao::Erro(_) => "erro".into(),
            Execucao::Parado if self.na_fila(&c.id) > 0 => "na fila".into(),
            Execucao::Parado => match self.parada(&c.id) {
                Some(motivo) => format!("parada: {motivo}"),
                None => "parado".into(),
            },
        }
    }

    fn linha_do_chat(&self, c: &Chat) -> String {
        let quando = ha_quanto(agora().saturating_sub(c.atualizado_em));
        format!("- chat {} | {} | {} | {} | {quando}", c.id, c.titulo, self.estado_para_o_claude(c), com_til(&c.cwd))
    }

    fn texto_listar_chats(&self, quem: &Chat, a: &Value) -> String {
        let ativos = a["ativos"].as_bool().unwrap_or(false) || a["ativos"].as_str() == Some("true");
        let mut lista: Vec<&Chat> = self
            .chats
            .iter()
            .filter(|c| c.id != quem.id && !c.arquivado)
            .filter(|c| !ativos || self.execucao(&c.id).rodando() || self.na_fila(&c.id) > 0)
            .collect();
        lista.sort_by_key(|c| std::cmp::Reverse(c.atualizado_em));
        let fora = lista.len().saturating_sub(40);
        let mut texto: String = lista.iter().take(40).map(|c| self.linha_do_chat(c) + "\n").collect();
        if fora > 0 {
            texto.push_str(&format!("({fora} ficaram de fora)\n"));
        }
        if texto.is_empty() {
            texto.push_str("Nenhum chat.");
        }
        texto
    }

    /// `mensagem_para_chat`, com o freio de ciclo: depois de 6 mensagens seguidas entre os dois sem o
    /// usuário no meio, a próxima fica guardada como nota.
    fn mensagem_para_chat(&mut self, chat: &Chat, destino: &str, corpo: String, cx: &mut Context<Self>) -> Resposta {
        let Some(alvo) = achar_chat(&self.chats, destino).cloned() else {
            return Resposta::erro(format!("Não existe chat \"{destino}\". Use listar_chats."));
        };
        if alvo.id == chat.id {
            return Resposta::erro("Esse é você mesmo.");
        }
        if corpo.is_empty() {
            return Resposta::erro("Escreva o texto da mensagem.");
        }
        let id = &alvo.id;
        if alvo.arquivado {
            return Resposta::erro(format!("O chat {id} está arquivado; a mensagem não foi entregue."));
        }
        let de = format!("chat {} \"{}\"", chat.id, chat.titulo);
        let nota = self.contar_ciclo(&chat.id, &alvo.id);
        self.entregar_entrada(&alvo.id, Entrada { de: Some(de), texto: corpo, remetente: Some(chat.id.clone()), nota, ..Default::default() }, cx);
        if nota {
            return Resposta::ok(
                "Muitas mensagens seguidas entre vocês dois: esta ficou guardada e entra quando o usuário falar com um dos \
                 dois. Encerre sua vez.",
            );
        }
        Resposta::ok("Mensagem entregue (ou na fila, se o chat estiver ocupado).")
    }

    /// `abrir_chat`: um chat novo à vista do usuário, na mesma pasta de quem pediu, já com o pedido.
    /// Só num turno em que só o usuário falou: o texto de um relatório ou de outro chat não abre
    /// chats sozinho (nem um chat aberto assim abre outro em cadeia).
    fn abrir_chat_irmao(&mut self, chat: &Chat, titulo: &str, pedido: String, cx: &mut Context<Self>) -> Resposta {
        if pedido.is_empty() {
            return Resposta::erro("Escreva o pedido do chat novo.");
        }
        let entradas = self.ultima_entrada.get(&chat.id).map(|(_, m)| m.clone()).unwrap_or_default();
        if !turno_so_do_usuario(&entradas) {
            return Resposta::erro(
                "Só abro outro chat quando o usuário pede neste turno. Diga a ele o que você abriria e encerre sua vez.",
            );
        }
        // O chat sem pasta abre o outro também sem pasta (cada um numa vazia só dele).
        let pasta = (!self.sem_pasta(chat)).then(|| chat.cwd.clone());
        let id = match self.novo_chat(pasta.as_deref(), &format!("chat:{}", chat.id), cx) {
            Ok(id) => id,
            Err(erro) => return Resposta::erro(format!("Não abriu o chat: {erro}")),
        };
        let titulo = titulo_do_chat_aberto(titulo, &pedido);
        if let Some(c) = self.chat_mut(&id) {
            c.titulo = titulo.clone();
        }
        let de = format!("chat {} \"{}\"", chat.id, chat.titulo);
        self.entregar_entrada(&id, Entrada { de: Some(de), texto: pedido, remetente: Some(chat.id.clone()), ..Default::default() }, cx);
        let texto = format!("Abriu o chat {id} \"{titulo}\"");
        self.registrar("chat", &chat.id, &chat.id, &texto);
        Resposta::ok(format!(
            "Chat {id} \"{titulo}\" aberto em {}, já com o pedido. O usuário o vê na lista e continua por lá; conte a \
             ele o título. Nada volta para você sozinho.",
            com_til(&self.chat(&id).map_or_else(|| chat.cwd.clone(), |c| c.cwd.clone()))
        ))
    }

    /// Conta mais uma mensagem automática entre os dois chats e diz se ela vai como nota (passou do
    /// limite). Na primeira que passa, os dois ganham o aviso e fica a ocorrência.
    pub(super) fn contar_ciclo(&mut self, de: &str, para: &str) -> bool {
        let n = self.ciclos.contar(de, para);
        if n == LIMITE_DO_CICLO + 1 {
            let nome = |m: &Motor, id: &str| match m.chat(id) {
                Some(c) => format!("chat {} \"{}\"", c.id, c.titulo),
                None => id.to_string(),
            };
            let aviso = format!("Muitas mensagens seguidas entre {} e {}; esperando você.", nome(self, de), nome(self, para));
            self.avisar(de, &aviso, true);
            self.avisar(para, &aviso, true);
            self.registrar("ciclo", "vitral", para, &aviso);
        }
        n > LIMITE_DO_CICLO
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn titulo_do_chat_aberto_por_outro() {
        assert_eq!(titulo_do_chat_aberto("  Cupom no carrinho \n", "pedido"), "Cupom no carrinho");
        assert_eq!(titulo_do_chat_aberto("", "\n  Arruma o rodapé\nmais contexto"), "Arruma o rodapé");
        let longo = titulo_do_chat_aberto(&"a".repeat(80), "");
        assert_eq!(longo.chars().count(), 60);
        assert!(longo.ends_with('…'));
    }

    fn ocorrencia(quando: u64, tipo: &str, autor: &str, chat: &str) -> Ocorrencia {
        Ocorrencia { quando, tipo: tipo.into(), autor: autor.into(), chat: chat.into(), texto: format!("{tipo} {chat}"), ..Default::default() }
    }

    #[test]
    fn resumo_ao_voltar_por_grupo_sem_o_que_o_usuario_fez() {
        let ocorrencias = vec![
            ocorrencia(5, "juntou", "vitral", "c1"),
            ocorrencia(11, "entrega", "c12", "c12"),
            ocorrencia(12, "juntou", "vitral", "c12"),
            ocorrencia(13, "juntou", "usuario", "c3"),
            ocorrencia(14, "permissao", "c7", "c7"),
            ocorrencia(15, "conflito", "vitral", "c9"),
            ocorrencia(16, "pedido", "c3", "c9"),
            ocorrencia(30, "falhou", "vitral", "c2"),
        ];
        let r = resumo_desde(10, 20, &ocorrencias).unwrap();
        let linhas: Vec<(&str, &str)> = r.linhas.iter().map(|l| (l.grupo, l.texto.as_str())).collect();
        assert_eq!(linhas, [("terminou", "juntou c12"), ("precisa", "permissao c7"), ("falhou", "conflito c9")]);
        assert_eq!((r.desde, r.ate), (10, 20));
        assert_eq!(resumo_desde(20, 25, &ocorrencias), None);
        assert_eq!(resumo_desde(12, 13, &ocorrencias), None, "só o que o usuário fez");
    }

    #[test]
    fn freio_de_ciclo() {
        let mut ciclos = Ciclos::default();
        for n in 1..=LIMITE_DO_CICLO {
            assert_eq!(ciclos.contar("c12", "c7"), n);
        }
        assert!(ciclos.travados().is_empty());
        assert_eq!(ciclos.contar("c7", "c12"), LIMITE_DO_CICLO + 1, "o par não tem sentido");
        assert_eq!(ciclos.travados(), [("c12".to_string(), "c7".to_string())]);
        assert_eq!(ciclos.contar("c3", "c12"), 1, "outro par conta à parte");
        ciclos.zerar("c12");
        assert!(ciclos.travados().is_empty());
        assert_eq!(ciclos.contar("c7", "c12"), 1);
    }

}
