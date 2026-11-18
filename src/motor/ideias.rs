//! O quadro de ideias no motor: ler, mexer e gravar os cartões (`crate::ideias`), abrir o chat de
//! uma ideia e o Linear (`crate::linear`): com a chave dele, o quadro é o das ideias do time, e cada
//! mudança daqui sobe para a issue em segundo plano. Só o dono põe ideia no quadro: nenhum chat
//! anota sozinho (dono, 18/11/2026: "fica poluindo"). O cartão muda na hora; se o Linear recusar, o aviso aparece no quadro e a
//! leitura seguinte traz de volta o que vale lá.

use super::*;
use crate::ideias::{Coluna, Ideia, NoLinear, Quadro};
use crate::linear;

/// Quanto a edição do texto de um cartão fica parada antes de subir para o Linear.
const PAUSA_DO_TEXTO: std::time::Duration = std::time::Duration::from_millis(1500);

/// O Linear do quadro: a chave, o time lido e o que está a caminho.
#[derive(Default)]
pub struct EstadoDoLinear {
    chave: Option<String>,
    time: Option<linear::Time>,
    lendo: bool,
    ligando: bool,
    lido_em: Option<u64>,
    erro: Option<String>,
    /// Cartões com mudança daqui a caminho (quantas): a leitura não passa por cima deles.
    presas: HashMap<String, u32>,
    /// A edição de texto de cada cartão (a geração dela): sobe quando para de mudar.
    texto: HashMap<String, u64>,
    geracao: u64,
}

impl EstadoDoLinear {
    pub fn da_config(config: &Path) -> Self {
        EstadoDoLinear { chave: linear::ler_chave(config), ..Default::default() }
    }
}

/// O que o quadro mostra do Linear.
#[derive(Clone, Debug, PartialEq)]
pub enum SituacaoDoLinear {
    Desligado { ligando: bool, erro: Option<String> },
    Ligado { time: Option<String>, lendo: bool, lido_em: Option<u64>, erro: Option<String> },
}

/// A descrição da issue: o pedido e, embaixo, o porquê.
fn descricao_para_o_linear(ideia: &Ideia) -> String {
    match (ideia.pedido.trim(), ideia.nota.trim()) {
        (pedido, "") => pedido.to_string(),
        ("", nota) => format!("Por quê: {nota}"),
        (pedido, nota) => format!("{pedido}\n\nPor quê: {nota}"),
    }
}

impl Motor {
    pub fn quadro(&self) -> &Quadro {
        &self.quadro
    }

    pub(super) fn salvar_quadro(&self) {
        if self.somente_leitura.is_some() {
            return;
        }
        if let Err(erro) = self.dados.salvar_ideias(&self.quadro) {
            eprintln!("vitral: não salvou o quadro de ideias: {erro:#}");
        }
    }

    // ---------- o Linear ----------

    pub fn situacao_do_linear(&self) -> SituacaoDoLinear {
        let l = &self.linear;
        match &l.chave {
            None => SituacaoDoLinear::Desligado { ligando: l.ligando, erro: l.erro.clone() },
            Some(_) => SituacaoDoLinear::Ligado {
                time: l.time.as_ref().map(|t| format!("{} ({})", t.nome, t.chave)),
                lendo: l.lendo,
                lido_em: l.lido_em,
                erro: l.erro.clone(),
            },
        }
    }

    fn codigo_do_time(&self) -> String {
        self.quadro.time_do_linear.clone().unwrap_or_else(|| linear::TIME_PADRAO.to_string())
    }

    /// Um trabalho no Linear em segundo plano, com o time (lido antes, se ainda não foi). `presa`:
    /// o cartão que a leitura não deve mexer enquanto o trabalho não volta.
    fn no_linear<R: Send + 'static>(
        &mut self,
        presa: Option<String>,
        trabalho: impl FnOnce(&str, &linear::Time) -> Result<R, String> + Send + 'static,
        depois: impl FnOnce(&mut Motor, Result<R, String>, &mut Context<Motor>) + 'static,
        cx: &mut Context<Self>,
    ) {
        let Some(chave) = self.linear.chave.clone() else {
            return;
        };
        let (time, codigo) = (self.linear.time.clone(), self.codigo_do_time());
        if let Some(id) = &presa {
            *self.linear.presas.entry(id.clone()).or_default() += 1;
        }
        cx.spawn(async move |this, cx| {
            let (time_lido, resultado) = cx
                .background_spawn(async move {
                    match time.map(Ok).unwrap_or_else(|| linear::time(&chave, &codigo)) {
                        Ok(time) => {
                            let resultado = trabalho(&chave, &time);
                            (Some(time), resultado)
                        }
                        Err(erro) => (None, Err(erro)),
                    }
                })
                .await;
            let _ = this.update(cx, |m, cx| {
                if let Some(id) = &presa
                    && let Some(n) = m.linear.presas.get_mut(id)
                {
                    *n -= 1;
                    if *n == 0 {
                        m.linear.presas.remove(id);
                    }
                }
                if m.linear.time.is_none() {
                    m.linear.time = time_lido;
                }
                match &resultado {
                    Ok(_) => m.linear.erro = None,
                    Err(erro) => m.linear.erro = Some(erro.clone()),
                }
                depois(m, resultado, cx);
                cx.notify();
            });
        })
        .detach();
    }

    /// Lê as issues do time e põe o quadro em dia (ao abrir o quadro, a cada minuto com ele
    /// aberto e no ⟳).
    pub fn ler_linear(&mut self, cx: &mut Context<Self>) {
        if self.linear.chave.is_none() || self.linear.lendo {
            return;
        }
        self.linear.lendo = true;
        cx.notify();
        self.no_linear(
            None,
            |chave, time| linear::issues(chave, &time.id),
            |m, resultado, _| {
                m.linear.lendo = false;
                if let Ok(issues) = resultado {
                    let presas: HashSet<String> = m.linear.presas.keys().chain(m.linear.texto.keys()).cloned().collect();
                    m.quadro.juntar_do_linear(&issues, &presas);
                    m.linear.lido_em = Some(agora());
                    m.salvar_quadro();
                }
            },
            cx,
        );
    }

    /// "Testar e ligar": a chave vale se ela vê o time; aí fica gravada e o quadro é lido.
    pub fn ligar_linear(&mut self, chave: String, cx: &mut Context<Self>) {
        let chave = chave.trim().to_string();
        if chave.is_empty() || self.linear.ligando {
            return;
        }
        let (codigo, config) = (self.codigo_do_time(), self.dados.config.clone());
        self.linear.ligando = true;
        self.linear.erro = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let resultado = cx.background_spawn(async move { linear::time(&chave, &codigo).map(|time| (chave, time)) }).await;
            let _ = this.update(cx, |m, cx| {
                m.linear.ligando = false;
                match resultado {
                    Ok((chave, time)) => match linear::gravar_chave(&config, &chave) {
                        Ok(()) => {
                            m.linear.chave = Some(chave);
                            m.linear.time = Some(time);
                            m.ler_linear(cx);
                        }
                        Err(erro) => m.linear.erro = Some(format!("Não gravei a chave: {erro}")),
                    },
                    Err(erro) => m.linear.erro = Some(erro),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Apaga a chave; os cartões das issues saem do quadro (os só do Vitral ficam).
    pub fn desligar_linear(&mut self, cx: &mut Context<Self>) {
        let _ = linear::gravar_chave(&self.dados.config, "");
        self.linear = EstadoDoLinear::default();
        self.quadro.sem_o_linear();
        self.salvar_quadro();
        cx.notify();
    }

    /// Uma mudança do cartão sobe para a issue dele. `coluna`: a coluna nova, que vira o estado.
    fn empurrar(&mut self, id: &str, mut campos: Value, coluna: Option<Coluna>, cx: &mut Context<Self>) {
        let Some(issue) = self.quadro.achar(id).and_then(|i| i.linear.as_ref()).map(|l| l.id.clone()) else {
            return;
        };
        self.no_linear(
            Some(id.to_string()),
            move |chave, time| {
                if let Some(coluna) = coluna {
                    let estado = time.estado_da_coluna(coluna).ok_or_else(|| format!("O time não tem estado para {}.", coluna.titulo()))?;
                    campos["stateId"] = Value::String(estado.id.clone());
                }
                linear::atualizar(chave, &issue, campos)
            },
            |m, resultado, cx| {
                if resultado.is_err() {
                    m.ler_linear(cx);
                }
            },
            cx,
        );
    }

    /// A ideia nova (do campo do quadro) vira issue do time, com a etiqueta do quadro.
    /// Os cartões de antes de ligar o Linear continuam só do Vitral.
    fn criar_no_linear(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(ideia) = self.quadro.achar(id).cloned().filter(|i| i.linear.is_none()) else {
            return;
        };
        let descricao = descricao_para_o_linear(&ideia);
        let local = id.to_string();
        self.no_linear(
            Some(local.clone()),
            move |chave, time| {
                let estado = time
                    .estado_da_coluna(ideia.coluna)
                    .ok_or_else(|| format!("O time não tem estado para {}.", ideia.coluna.titulo()))?;
                linear::criar(chave, &time.id, &estado.id, &ideia.titulo, &descricao, ideia.ordem)
            },
            move |m, resultado, _| {
                let Ok(issue) = resultado else {
                    return;
                };
                // A leitura pode ter trazido a issue antes desta resposta: o que é só do Vitral
                // passa para o cartão dela.
                let Some(mut cartao) = m.quadro.apagar(&local) else {
                    return;
                };
                let no = NoLinear {
                    id: issue.id.clone(),
                    identificador: issue.identificador.clone(),
                    url: issue.url.clone(),
                    estado: issue.estado.clone(),
                    responsavel: issue.responsavel.clone(),
                };
                match m.quadro.achar_mut(&issue.id) {
                    Some(lido) => {
                        lido.pasta = cartao.pasta.take();
                        lido.chat = cartao.chat.take();
                        lido.nota = std::mem::take(&mut cartao.nota);
                        lido.etiquetas = std::mem::take(&mut cartao.etiquetas);
                        lido.origem = std::mem::take(&mut cartao.origem);
                    }
                    None => {
                        cartao.id = issue.id.clone();
                        cartao.pedido = issue.descricao.clone();
                        cartao.ordem = issue.ordem;
                        cartao.linear = Some(no);
                        m.quadro.ideias.push(cartao);
                    }
                }
                m.salvar_quadro();
            },
            cx,
        );
    }

    /// Lendo o Linear (ou testando a chave): o quadro mostra que está carregando.
    pub fn lendo_o_linear(&self) -> bool {
        self.linear.chave.is_some() && self.linear.lendo || self.linear.ligando
    }

    pub fn linear_ligado(&self) -> bool {
        self.linear.chave.is_some()
    }

    // ---------- os cartões ----------

    /// Um cartão novo no alto da coluna Ideias (e, com o Linear e sem `so_no_vitral`, a issue).
    /// `chat`: a conversa de que a ideia é (aberto o quadro pelo menu dela). Devolve o id.
    pub fn nova_ideia(
        &mut self,
        titulo: &str,
        pasta: Option<PathBuf>,
        chat: Option<String>,
        so_no_vitral: bool,
        cx: &mut Context<Self>,
    ) -> Option<String> {
        let titulo = titulo.trim();
        if titulo.is_empty() || self.somente_leitura.is_some() {
            return None;
        }
        let id = novo_id();
        self.quadro.adicionar(Ideia { id: id.clone(), titulo: titulo.to_string(), pasta, chat, criada_em: agora(), ..Default::default() });
        self.salvar_quadro();
        if !so_no_vitral {
            self.criar_no_linear(&id, cx);
        }
        cx.notify();
        Some(id)
    }

    /// Muda um cartão (a edição, a cada tecla): grava na próxima passada e, com o Linear, o título
    /// e o pedido sobem quando a edição para por um instante.
    pub fn editar_ideia(&mut self, id: &str, f: impl FnOnce(&mut Ideia), cx: &mut Context<Self>) {
        let Some(ideia) = self.quadro.achar_mut(id) else {
            return;
        };
        let antes = ideia.clone();
        f(ideia);
        if *ideia == antes {
            return;
        }
        let texto_mudou = ideia.titulo != antes.titulo || ideia.pedido != antes.pedido;
        self.quadro_sujo = true;
        cx.notify();
        if !texto_mudou || ideia.linear.is_none() || self.linear.chave.is_none() {
            return;
        }
        self.linear.geracao += 1;
        let geracao = self.linear.geracao;
        self.linear.texto.insert(id.to_string(), geracao);
        let id = id.to_string();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(PAUSA_DO_TEXTO).await;
            let _ = this.update(cx, |m, cx| {
                if m.linear.texto.get(&id) != Some(&geracao) {
                    return;
                }
                m.linear.texto.remove(&id);
                let Some(ideia) = m.quadro.achar(&id) else {
                    return;
                };
                let titulo = if ideia.titulo.trim().is_empty() { "Sem título".to_string() } else { ideia.titulo.trim().to_string() };
                let campos = json!({ "title": titulo, "description": ideia.pedido });
                m.empurrar(&id, campos, None, cx);
            });
        })
        .detach();
    }

    pub fn mover_ideia(&mut self, id: &str, coluna: Coluna, antes_de: Option<&str>, cx: &mut Context<Self>) {
        let de = self.quadro.achar(id).map(|i| i.coluna);
        if self.quadro.mover(id, coluna, antes_de) {
            self.salvar_quadro();
            let ordem = self.quadro.achar(id).map_or(0., |i| i.ordem);
            self.empurrar(id, json!({ "sortOrder": ordem }), (de != Some(coluna)).then_some(coluna), cx);
            cx.notify();
        }
    }

    /// Apaga o cartão; o da issue arquiva a issue (o Linear guarda, e o Desfazer desarquiva).
    pub fn apagar_ideia(&mut self, id: &str, cx: &mut Context<Self>) -> Option<Ideia> {
        let apagada = self.quadro.apagar(id)?;
        self.salvar_quadro();
        if let Some(no) = &apagada.linear {
            let issue = no.id.clone();
            self.no_linear(Some(id.to_string()), move |chave, _| linear::arquivar(chave, &issue, true), |_, _, _| {}, cx);
        }
        cx.notify();
        Some(apagada)
    }

    /// O "Desfazer" do aviso: o cartão volta (e a issue sai do arquivo).
    pub fn devolver_ideia(&mut self, ideia: Ideia, cx: &mut Context<Self>) {
        if self.quadro.achar(&ideia.id).is_some() {
            return;
        }
        let (id, issue) = (ideia.id.clone(), ideia.linear.as_ref().map(|l| l.id.clone()));
        self.quadro.ideias.push(ideia);
        self.salvar_quadro();
        if let Some(issue) = issue {
            self.no_linear(Some(id), move |chave, _| linear::arquivar(chave, &issue, false), |_, _, _| {}, cx);
        }
        cx.notify();
    }

    /// Manda a ideia para o chat dela (o pedido entra na fila dele, sem cortar um turno que esteja
    /// rodando) ou, sem chat (ou com `novo`), abre um: a conversa nasce na pasta do cartão (sem ela,
    /// na `pasta_padrao`, que fica no cartão), com o título dele, e recebe o pedido como a primeira
    /// mensagem. O cartão vai para Fazendo, ligado ao chat. Devolve o chat.
    pub fn mandar_ideia(&mut self, id: &str, pasta_padrao: Option<PathBuf>, novo: bool, cx: &mut Context<Self>) -> Result<String, String> {
        let ideia = self.quadro.achar(id).cloned().ok_or("o cartão não existe mais")?;
        let texto = ideia.texto_para_o_chat();
        if texto.is_empty() {
            return Err("o cartão está vazio".into());
        }
        let existente = ideia.chat.clone().filter(|c| !novo && self.chat(c).is_some_and(|c| !c.arquivado));
        if let Some(chat) = existente {
            self.enviar_do_usuario(&chat, texto, Vec::new(), Reforco { depois: true, ..Default::default() }, cx);
            if ideia.coluna != Coluna::Fazendo {
                self.mover_ideia(id, Coluna::Fazendo, None, cx);
            }
            self.registrar("ideia", "", &chat, &format!("Mandou a ideia \"{}\" no chat", ideia.titulo));
            cx.notify();
            return Ok(chat);
        }
        let pasta = ideia.pasta.clone().or(pasta_padrao);
        let chat = self.novo_chat(pasta.as_deref(), &format!("ideia:{id}"), cx)?;
        if let Some(c) = self.chat_mut(&chat) {
            c.titulo = super::conversas::titulo_do_chat_aberto(&ideia.titulo, &texto);
        }
        self.enviar_do_usuario(&chat, texto, Vec::new(), Reforco::default(), cx);
        if let Some(cartao) = self.quadro.achar_mut(id) {
            cartao.chat = Some(chat.clone());
            cartao.pasta = pasta;
        }
        if ideia.coluna != Coluna::Fazendo {
            self.mover_ideia(id, Coluna::Fazendo, None, cx);
        }
        self.registrar("ideia", "", &chat, &format!("Abriu o chat da ideia \"{}\"", ideia.titulo));
        self.salvar_quadro();
        cx.notify();
        Ok(chat)
    }

}

#[cfg(test)]
mod testes {
    use super::descricao_para_o_linear;
    use crate::ideias::Ideia;

    #[test]
    fn a_descricao_leva_o_pedido_e_o_porque() {
        let ideia = |pedido: &str, nota: &str| Ideia { pedido: pedido.into(), nota: nota.into(), ..Default::default() };
        assert_eq!(descricao_para_o_linear(&ideia("Faça X", "")), "Faça X");
        assert_eq!(descricao_para_o_linear(&ideia("Faça X", "poupa tempo")), "Faça X\n\nPor quê: poupa tempo");
        assert_eq!(descricao_para_o_linear(&ideia("", "poupa tempo")), "Por quê: poupa tempo");
    }
}
