//! As conversas do Claude Code no motor: um chat por sessão nova, no projeto cuja pasta ligada
//! contém a dela (senão em Avulsos), trabalhando direto nessa pasta (o Claude Code só retoma a
//! sessão a partir dela); o registro do que já veio, a importação da primeira abertura e a marca de
//! quando a conversa sai do Claude Code e continua no Vitral.

use std::sync::atomic::AtomicBool;

use super::*;
use crate::importar::{self, Listagem, Registro, Relatorio, Sessao};

/// Uma importação por vez (a da abertura e a do menu não se cruzam).
static IMPORTANDO: AtomicBool = AtomicBool::new(false);

impl Motor {
    /// Na abertura: no primeiro uso, a importação fica marcada como pendente (gravada antes de começar);
    /// pendente, ela roda. Uma queda no meio a refaz na abertura seguinte.
    pub fn importar_na_abertura(&mut self, primeiro_uso: bool, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        let gravado = Registro::de(&self.resto_estado);
        let mut registro = gravado.clone().unwrap_or_default();
        if primeiro_uso && gravado.is_none() {
            registro.pendente = true;
            registro.gravar(&mut self.resto_estado);
            self.salvar();
        }
        if registro.pendente {
            self.importar_do_claude_code(true, cx);
        }
    }

    /// Traz as conversas do Claude Code que ainda não vieram, lidas em segundo plano. O resultado vai
    /// para os avisos da janela ("42 conversas do Claude Code importadas · 39 em Avulsos"); a da
    /// abertura sem nada novo fica calada.
    pub fn importar_do_claude_code(&mut self, automatica: bool, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        if IMPORTANDO.swap(true, Ordering::SeqCst) {
            if !automatica {
                self.avisos_ao_abrir.push("Já estou trazendo as conversas do Claude Code.".into());
                cx.notify();
            }
            return;
        }
        if !automatica {
            self.avisos_ao_abrir.push("Procurando conversas novas no Claude Code…".into());
            cx.notify();
        }
        let raizes = importar::raizes(self.contas.iter().filter_map(|c| c.pasta()));
        let (trazidas, do_vitral) = self.sessoes_conhecidas();
        let de_fora = importar::pastas_de_fora(&self.dados.pasta);
        let agora = agora();
        cx.spawn(async move |motor, cx| {
            let listagem = cx
                .background_spawn(async move {
                    let pastas = importar::pastas_de_projects(&raizes);
                    let abertas = importar::sessoes_abertas(Path::new("/proc"));
                    let filtro =
                        importar::Filtro { trazidas: &trazidas, do_vitral: &do_vitral, de_fora: &de_fora, abertas: &abertas, agora };
                    importar::listar(&pastas, &filtro)
                })
                .await;
            let _ = motor.update(cx, |m, cx| m.trazer(listagem, automatica, cx));
            IMPORTANDO.store(false, Ordering::SeqCst);
        })
        .detach();
    }

    /// As sessões que não vêm de novo: as já trazidas (mesmo de chats apagados depois) e as dos chats
    /// do próprio Vitral (inclusive a cópia de cada conversa que continuou aqui).
    fn sessoes_conhecidas(&self) -> (HashSet<String>, HashSet<String>) {
        let mut trazidas: HashSet<String> =
            Registro::de(&self.resto_estado).map(|r| r.importadas.into_iter().collect()).unwrap_or_default();
        trazidas.extend(self.chats.iter().filter_map(|c| c.importada.as_ref().map(|i| i.sessao.clone())));
        let do_vitral = self.chats.iter().map(|c| c.sessao.clone()).collect();
        (trazidas, do_vitral)
    }

    fn trazer(&mut self, listagem: Listagem, automatica: bool, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        let (trazidas, do_vitral) = self.sessoes_conhecidas();
        let mut registro = Registro::de(&self.resto_estado).unwrap_or_default();
        let mut relatorio = Relatorio { ja_trazidas: listagem.ja_trazidas, ..Default::default() };
        let mut criados = Vec::new();
        for sessao in listagem.novas.iter().filter(|s| !trazidas.contains(&s.id) && !do_vitral.contains(&s.id)) {
            let (id, destino) = self.criar_chat_importado(sessao);
            relatorio.contar(&destino, sessao.em_uso);
            registro.importadas.push(sessao.id.clone());
            criados.push(id);
        }
        if automatica {
            registro.pendente = false;
        }
        registro.ultima_vez = agora();
        registro.gravar(&mut self.resto_estado);
        self.salvar();
        for id in &criados {
            if let Some(c) = self.chat(id) {
                let _ = self.dados.salvar_chat(c);
            }
        }
        if let Some(texto) = relatorio.texto(automatica) {
            // A da abertura fica também na conversa do Mestre, como a da migração.
            if automatica {
                self.registrar("importacao", "vitral", "", "", None, &texto);
                self.avisar_no_mestre(&texto);
            }
            self.avisos_ao_abrir.push(texto);
        }
        cx.notify();
    }

    /// O chat de uma sessão, com a tarefa em conversa, na pasta dela e sem as mensagens (que vêm
    /// do arquivo quando o chat abre): só o marco de onde ela continua. Devolve o id e o nome do
    /// destino.
    fn criar_chat_importado(&mut self, s: &Sessao) -> (String, String) {
        let canonica = s.cwd.canonicalize().unwrap_or_else(|_| s.cwd.clone());
        let destino = importar::destino(&canonica, &self.projetos);
        let projeto = destino.map_or_else(String::new, |(p, _)| p.id.clone());
        let forma = importar::forma(&canonica, destino.map(|(_, l)| l));
        let (permissao, modelo, conta) =
            destino.map_or((None, None, None), |(p, _)| (p.permissao.clone(), p.modelo.clone(), p.conta.clone()));
        let id = novo_id();
        let n = self.proxima_tarefa;
        self.proxima_tarefa += 1;
        let porta = dados::porta_livre(&self.tarefas).unwrap_or(0);
        self.tarefas.push(Tarefa {
            id: n,
            projeto: projeto.clone(),
            trabalhador: id.clone(),
            titulo: s.titulo.clone(),
            estado: EstadoTarefa::Conversa,
            forma,
            origem: s.cwd.clone(),
            porta,
            preparada: true,
            criada_em: s.primeira,
            atualizada_em: s.ultima,
            ..Default::default()
        });
        let agora = agora();
        let mut chat = Chat::novo(id.clone(), Papel::Projeto, &projeto, &s.titulo, s.cwd.clone());
        chat.tarefa = Some(n);
        chat.criado_por = importar::CRIADO_POR.into();
        (chat.criado_em, chat.atualizado_em, chat.lido_em) = (s.primeira, s.ultima, agora);
        (chat.permissao, chat.modelo, chat.conta) = (permissao, modelo, conta);
        let importada = s.importada(agora);
        chat.itens.push(Item::Aviso { texto: importar::marco(&importada), erro: false });
        if importada.em_uso {
            chat.itens.push(Item::Aviso { texto: importar::AVISO_EM_USO.into(), erro: false });
        }
        chat.importada = Some(importada);
        self.chats.push(chat);
        (id, self.nome_do_projeto(&projeto))
    }

    /// A primeira vez no Vitral de uma conversa trazida abriu a sessão: o histórico para onde a
    /// original estava, e as próximas vezes retomam a sessão nova. Sem o arquivo original, ela
    /// começou do zero, e o chat avisa.
    pub(super) fn conversa_continuada(&mut self, chat: &str) {
        let Some(i) = self.chat_mut(chat).and_then(|c| c.importada.as_mut()).filter(|i| !i.continuada) else {
            return;
        };
        let sumiu = !i.arquivo.is_file();
        i.marcar_continuada();
        if sumiu {
            self.avisar(
                chat,
                "A conversa original do Claude Code não existe mais; esta continuou numa sessão nova, sem a memória dela.",
                true,
            );
        }
    }
}
