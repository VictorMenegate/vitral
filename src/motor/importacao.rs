//! As conversas do Claude Code no motor: um chat por sessão nova, no projeto cuja pasta ligada
//! contém a dela (senão em Avulsos), trabalhando direto nessa pasta (o Claude Code só retoma a
//! sessão a partir dela); o registro do que já veio, a importação da primeira abertura e a marca de
//! quando a conversa sai do Claude Code e continua no Vitral.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use super::*;
use crate::importar::{self, Listagem, Registro, Relatorio, Sessao};

/// Uma importação por vez (a da abertura, a do menu e a vigia não se cruzam).
static IMPORTANDO: AtomicBool = AtomicBool::new(false);

/// A vez de importar. Solta sozinha quando a importação acaba, e também se a tarefa for largada
/// no meio (a janela fechou) ou a leitura entrar em pânico: com a marca presa, nada mais viria
/// do Claude Code até reabrir o app.
struct Vez;

impl Vez {
    fn pegar() -> Option<Self> {
        (!IMPORTANDO.swap(true, Ordering::SeqCst)).then_some(Vez)
    }
}

impl Drop for Vez {
    fn drop(&mut self) {
        IMPORTANDO.store(false, Ordering::SeqCst);
    }
}

/// De quanto em quanto tempo a vigia olha se há conversa nova no Claude Code.
const VIGIA_DO_CLAUDE_CODE: Duration = Duration::from_secs(120);
/// A folga da vigia: ela relê o que mudou desde um pouco antes da passada anterior.
const FOLGA_DA_VIGIA: u64 = 30;
/// Com a leitura incompleta (uma pasta que não abriu), a marca da vigia não anda até a hora da
/// passada, só até um dia antes dela: o que mudou na pasta que falhou tem um dia para ser visto,
/// e uma pasta que nunca abre não faz a vigia reler o histórico inteiro a cada passada.
const JANELA_DA_FALHA: u64 = 24 * 3600;

/// De onde a vigia olha: a passada anterior menos a folga. Um relógio que andou para trás (a
/// passada anterior "no futuro") não cega a vigia: a marca nunca passa de agora. Sem passada
/// anterior, `None`: olha tudo.
fn desde_da_vigia(ultima_vez: u64, agora: u64) -> Option<u64> {
    (ultima_vez > 0).then(|| ultima_vez.min(agora).saturating_sub(FOLGA_DA_VIGIA))
}

/// A marca da vigia depois de uma passada que começou em `inicio`. É a hora do COMEÇO da leitura
/// (o que mudou enquanto ela lia fica para a próxima), e só chega nela se a leitura viu tudo.
fn marca_da_vigia(anterior: u64, inicio: u64, incompleta: bool) -> u64 {
    if incompleta { anterior.max(inicio.saturating_sub(JANELA_DA_FALHA)).min(inicio) } else { inicio }
}

/// Quem pediu a importação.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pedido {
    /// A primeira abertura do Vitral: traz tudo e conta ao Mestre.
    PrimeiraAbertura,
    /// O "Importar do Claude Code…" do menu: olha tudo de novo e sempre responde.
    Menu,
    /// A vigia (a cada abertura e de tempos em tempos): só o que mudou, e calada sem novidade.
    Vigia,
}

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
        } else if !registro.pendente && registro.ultima_vez == 0 {
            // Quem já usava o Vitral antes de existir a importação não tem registro. A vigia
            // começa de agora: sem esta marca, a primeira passada dela traria o histórico inteiro
            // do Claude Code sem ninguém pedir. O histórico vem pelo "Importar do Claude Code…".
            registro.ultima_vez = agora();
            registro.gravar(&mut self.resto_estado);
            self.salvar();
        }
        if registro.pendente {
            self.importar(Pedido::PrimeiraAbertura, cx);
        } else {
            self.importar(Pedido::Vigia, cx);
        }
        // A vigia: as conversas do Claude Code (as do terminal e as da extensão do VS Code moram
        // no mesmo lugar, `~/.claude/projects`) aparecem aqui sozinhas, sem importar à mão. É o
        // que deixa quem usa o VS Code passar para o Vitral sem perder nada no caminho.
        cx.spawn(async move |motor, cx| {
            loop {
                cx.background_executor().timer(VIGIA_DO_CLAUDE_CODE).await;
                if motor.update(cx, |m, cx| m.importar(Pedido::Vigia, cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
    }

    /// Traz as conversas do Claude Code que ainda não vieram, lidas em segundo plano. O resultado vai
    /// para os avisos da janela ("42 conversas do Claude Code importadas · 39 em Avulsos"); a da
    /// abertura sem nada novo fica calada.
    pub fn importar_do_claude_code(&mut self, automatica: bool, cx: &mut Context<Self>) {
        self.importar(if automatica { Pedido::PrimeiraAbertura } else { Pedido::Menu }, cx);
    }

    fn importar(&mut self, pedido: Pedido, cx: &mut Context<Self>) {
        if self.somente_leitura.is_some() {
            return;
        }
        let pelo_menu = pedido == Pedido::Menu;
        let Some(vez) = Vez::pegar() else {
            if pelo_menu {
                self.avisos_ao_abrir.push("Já estou trazendo as conversas do Claude Code.".into());
                cx.notify();
            }
            return;
        };
        if pelo_menu {
            self.avisos_ao_abrir.push("Procurando conversas novas no Claude Code…".into());
            cx.notify();
        }
        let raizes = importar::raizes(self.contas.iter().filter_map(|c| c.pasta()));
        let (trazidas, do_vitral) = self.sessoes_conhecidas();
        let de_fora = importar::pastas_de_fora(&self.dados.pasta);
        let agora = agora();
        // A vigia só olha o que mudou desde a passada anterior (com uma folga); sem passada
        // anterior, olha tudo.
        let desde = (pedido == Pedido::Vigia)
            .then(|| Registro::de(&self.resto_estado).and_then(|r| desde_da_vigia(r.ultima_vez, agora)))
            .flatten();
        cx.spawn(async move |motor, cx| {
            // A vez vai junto para a leitura e volta com ela: um pânico lá dentro também a solta.
            let (listagem, _vez) = cx
                .background_spawn(async move {
                    let vez = vez;
                    let pastas = importar::pastas_de_projects(&raizes);
                    let abertas = importar::sessoes_abertas(Path::new("/proc"));
                    let filtro = importar::Filtro {
                        trazidas: &trazidas,
                        do_vitral: &do_vitral,
                        de_fora: &de_fora,
                        abertas: &abertas,
                        agora,
                        desde,
                    };
                    (importar::listar(&pastas, &filtro), vez)
                })
                .await;
            let _ = motor.update(cx, |m, cx| m.trazer(listagem, pedido, agora, cx));
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

    /// `inicio` é a hora em que a leitura começou.
    fn trazer(&mut self, listagem: Listagem, pedido: Pedido, inicio: u64, cx: &mut Context<Self>) {
        let automatica = pedido != Pedido::Menu;
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
        registro.ultima_vez = marca_da_vigia(registro.ultima_vez, inicio, listagem.incompleta);
        registro.gravar(&mut self.resto_estado);
        self.salvar();
        for id in &criados {
            if let Some(c) = self.chat(id) {
                let _ = self.dados.salvar_chat(c);
            }
        }
        if let Some(texto) = relatorio.texto(automatica) {
            // A da primeira abertura fica também na conversa do Mestre, como a da migração; a
            // vigia só registra (uma linha no Mestre a cada conversa nova seria ruído).
            self.registrar("importacao", "vitral", "", "", None, &texto);
            if pedido == Pedido::PrimeiraAbertura {
                self.avisar_no_mestre(&texto);
            }
            if pedido != Pedido::Vigia || !criados.is_empty() {
                self.avisos_ao_abrir.push(texto);
            }
        }
        cx.notify();
    }

    /// O chat de uma sessão, com a tarefa em conversa, na pasta dela e sem as mensagens (que vêm
    /// do arquivo quando o chat abre): só o marco de onde ela continua. Devolve o id e o nome do
    /// destino.
    fn criar_chat_importado(&mut self, s: &Sessao) -> (String, String) {
        let canonica = dunce::canonicalize(&s.cwd).unwrap_or_else(|_| s.cwd.clone());
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
        chat.itens.push(Item::Aviso { texto: importar::marco(&importada), erro: false, em: agora });
        if importada.em_uso {
            chat.itens.push(Item::Aviso { texto: importar::AVISO_EM_USO.into(), erro: false, em: agora });
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

#[cfg(test)]
mod testes {
    use super::{FOLGA_DA_VIGIA, JANELA_DA_FALHA, desde_da_vigia, marca_da_vigia};

    #[test]
    fn a_vigia_olha_desde_a_passada_anterior_menos_a_folga() {
        assert_eq!(desde_da_vigia(0, 5_000), None, "sem passada anterior, olha tudo");
        assert_eq!(desde_da_vigia(1_000, 5_000), Some(1_000 - FOLGA_DA_VIGIA));
        // O relógio andou para trás: a passada anterior ficou "no futuro" e não cega a vigia.
        assert_eq!(desde_da_vigia(9_000, 5_000), Some(5_000 - FOLGA_DA_VIGIA));
    }

    #[test]
    fn a_marca_da_vigia_e_o_comeco_da_leitura_e_so_anda_se_viu_tudo() {
        let inicio = 10 * JANELA_DA_FALHA;
        assert_eq!(marca_da_vigia(123, inicio, false), inicio);
        // Leitura incompleta: a marca recente fica onde estava; a velha (ou nenhuma) anda só até
        // um dia antes, para a vigia não reler o histórico inteiro a cada passada.
        assert_eq!(marca_da_vigia(inicio - 60, inicio, true), inicio - 60);
        assert_eq!(marca_da_vigia(123, inicio, true), inicio - JANELA_DA_FALHA);
        assert_eq!(marca_da_vigia(0, inicio, true), inicio - JANELA_DA_FALHA);
        // E nunca passa do começo desta leitura, mesmo com a marca anterior no futuro.
        assert_eq!(marca_da_vigia(inicio + 500, inicio, true), inicio);
    }
}
