//! Conversas lado a lado: cada painel tem a sua conversa, caixa de mensagem e rolagem. Dá para
//! abrir uma conversa ao lado, arrastar conversas da lista (ou a aba de um painel) para um lado
//! de outro painel e fechar painéis. O painel ativo é o da última interação.

use gpui_kit::component::{
    WindowExt as _, h_flex,
    input::{InputEvent, InputState, TextareaState},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Flutuante as _, ListaDeEscolha, MedidaPainel, icone},
    movimento,
};
use crate::{
    modelo,
    tema::{Cores, Lucide, Tipo as _, r, raio},
};

/// Mais que isso fica apertado na tela (com a janela estreita, cabem menos).
pub const MAX_PAINEIS: usize = 4;

/// Quantas abas um painel guarda no cabeçalho.
pub const MAX_ABAS: usize = 8;

pub struct Painel {
    pub id: u64,
    pub chat: Option<String>,
    /// As abas do painel, na ordem em que abriram: os chats que ficam no cabeçalho dele, a um
    /// clique. O `chat` é a aba à vista. Clicar num chat da lista abre uma aba aqui (o `render` da
    /// Janela é quem põe o chat à vista na lista e tira os que sumiram).
    pub abas: Vec<String>,
    pub composer: Entity<TextareaState>,
    /// Máximo de linhas da caixa, que diminui em janela baixa.
    pub linhas_caixa: usize,
    pub rolagem: ScrollHandle,
    pub leitura: super::mensagens::Leitura,
    pub seguindo: bool,
    /// Um bloco desta conversa acabou de abrir: no próximo desenho o painel fica onde está, em
    /// vez de pular para o fim do que abriu.
    pub manter_lugar: bool,
    /// Esconde as chamadas de ferramenta e mostra só as mensagens (vale só para este painel).
    pub modo_foco: bool,
    /// Largura do cartão medida uma vez por quadro: o nível do painel e a faixa da caixa. Muda
    /// só quando passa de um limite (com folga), e aí o painel redesenha no quadro seguinte.
    pub medida: MedidaPainel,
    /// A aba no Tab (Enter foca a caixa, Delete fecha, Shift+F10 abre o menu).
    pub foco_aba: FocusHandle,
    /// O menu "⋯" do painel está aberto (o Shift+F10 na aba abre o mesmo).
    pub menu_aberto: bool,
    /// O popover de esforço da caixa (as teclas ←/→, Espaço e Enter vão para ele).
    pub foco_esforco: FocusHandle,
    /// A busca do popover de modelo da caixa.
    pub busca_modelo: Entity<ListaDeEscolha>,
    /// A busca dentro desta conversa (Ctrl+F): a barra, o termo e as ocorrências.
    pub busca: super::busca_conversa::Busca,
    /// Os blocos da conversa daqui em diante podem entrar com fade: o tamanho dela quando abriu
    /// no painel. Trocar de conversa recomeça, e o que já estava lá nunca anima.
    pub base_animacao: usize,
    /// Até onde os blocos já apareceram: um índice daqui para a frente é novo neste quadro.
    pub vistos: usize,
    /// A faixa do turno e a pílula do fim estavam à vista no quadro anterior (entram uma vez).
    pub faixa_vista: bool,
    pub pilula_vista: bool,
    /// Acabou de abrir ao lado: o conteúdo entra com fade no primeiro quadro.
    pub entra: bool,
    /// Sobe a cada envio, para o pulso do enviar.
    pub envios: u64,
}

/// O que está sendo arrastado: uma conversa, da lista ou da aba de um painel.
#[derive(Clone)]
pub struct ArrastoChat {
    pub chat: String,
    pub titulo: SharedString,
    /// Painel de onde a aba saiu (para mover em vez de abrir de novo).
    pub de_painel: Option<u64>,
}

/// Um projeto arrastado no trilho, para mudar de lugar.
#[derive(Clone)]
pub struct ArrastoProjeto {
    pub id: String,
    pub nome: SharedString,
}

/// Onde a conversa arrastada cai num painel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lado {
    Esquerda,
    Centro,
    Direita,
}

/// O que acontece ao soltar uma conversa num painel. A faixa mostra isso e `soltar` faz o mesmo.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Efeito {
    /// Sem nenhum painel: abre no lugar vazio.
    Abrir,
    AbrirAoLado(Lado),
    /// A conversa abre neste painel, como mais uma aba dele (saindo do painel de origem, se havia,
    /// que fecha quando era a única aba dele). `cheio` quando veio da borda, mas não cabe outro
    /// painel ao lado.
    Trocar { origem: Option<u64>, cheio: bool },
    /// Muda de lugar o painel em que a conversa já está.
    Mover { origem: u64, lado: Lado },
    Nenhum,
}

impl Efeito {
    fn rotulo(self) -> Option<(Lucide, &'static str)> {
        Some(match self {
            Efeito::Abrir => (Lucide::Plus, "Abrir aqui"),
            Efeito::AbrirAoLado(Lado::Esquerda) => (Lucide::PanelLeft, "Abrir ao lado"),
            Efeito::AbrirAoLado(_) => (Lucide::PanelRight, "Abrir ao lado"),
            Efeito::Trocar { .. } => (Lucide::Plus, "Abrir aqui, como aba"),
            Efeito::Mover { lado: Lado::Esquerda, .. } => (Lucide::ArrowLeft, "Mover para cá"),
            Efeito::Mover { .. } => (Lucide::ArrowRight, "Mover para cá"),
            Efeito::Nenhum => return None,
        })
    }
}

/// Efeito de soltar no `lado` do painel `painel`, com os painéis na ordem `ids`. `origem` é o
/// painel em que a conversa já está aberta (a aba arrastada ou a conversa da lista);
/// `origem_com_outras_abas`, se esse painel tem mais abas além dela.
fn efeito_de(
    ids: &[u64],
    origem: Option<u64>,
    painel: u64,
    lado: Lado,
    cabe_mais: bool,
    origem_com_outras_abas: bool,
) -> Efeito {
    if ids.is_empty() {
        return Efeito::Abrir;
    }
    let origem = origem.filter(|o| ids.contains(o));
    // Como no VS Code: uma aba que sai de um painel com outras abas, solta na borda de um painel
    // (até do próprio), vira um painel novo daquele lado; o de origem fica com as outras abas.
    if origem.is_some() && origem_com_outras_abas && lado != Lado::Centro {
        return match (cabe_mais, origem == Some(painel)) {
            (true, _) => Efeito::AbrirAoLado(lado),
            (false, true) => Efeito::Nenhum,
            (false, false) => Efeito::Trocar { origem, cheio: true },
        };
    }
    if origem == Some(painel) {
        return Efeito::Nenhum;
    }
    match (origem, lado) {
        (_, Lado::Centro) => Efeito::Trocar { origem, cheio: false },
        (Some(origem), lado) => match posicao_ao_mover(ids, origem, painel, lado) {
            // Na borda que já encosta na origem, a ordem não muda.
            Some(ix) if ids.get(ix) != Some(&origem) => Efeito::Mover { origem, lado },
            _ => Efeito::Nenhum,
        },
        (None, _) if !cabe_mais => Efeito::Trocar { origem: None, cheio: true },
        (None, lado) => Efeito::AbrirAoLado(lado),
    }
}

/// Posição em que o painel `origem` fica ao ser movido para o `lado` de `destino`.
fn posicao_ao_mover(ids: &[u64], origem: u64, destino: u64, lado: Lado) -> Option<usize> {
    let sem_origem: Vec<u64> = ids.iter().copied().filter(|id| *id != origem).collect();
    let alvo = sem_origem.iter().position(|id| *id == destino)?;
    Some(if lado == Lado::Esquerda { alvo } else { alvo + 1 })
}

/// Onde fica o painel da posição `de` depois de andar `passo` casas entre `n` (nada nas pontas).
fn destino_ao_mover(de: usize, passo: isize, n: usize) -> Option<usize> {
    de.checked_add_signed(passo).filter(|ix| *ix < n)
}

/// Por que não cabe outro painel: o máximo de lado a lado ou a largura da janela.
pub(super) fn motivo_sem_lugar(abertos: usize) -> String {
    if abertos >= MAX_PAINEIS {
        format!("Já há {abertos} lado a lado")
    } else {
        "Não cabe outro nesta largura".to_string()
    }
}

/// Quantos blocos uma conversa tem agora: os itens e, com o streaming, o texto que vai pousar no
/// índice seguinte. É a base das entradas (E5): o que já está na tela não anima.
pub(super) fn tamanho_da_conversa(itens: usize, escrevendo: bool) -> usize {
    itens + usize::from(escrevendo)
}

/// Índice do painel ativo depois de fechar o painel `fechado`: os da direita andam uma casa.
fn ativo_apos_fechar(ativo: usize, fechado: usize, restantes: usize) -> usize {
    let ativo = if fechado < ativo { ativo - 1 } else { ativo };
    ativo.min(restantes.saturating_sub(1))
}

/// O que cai num painel durante um arrasto: uma conversa (com o efeito) ou arquivos de fora.
#[derive(Clone, PartialEq, Debug)]
pub enum Soltura {
    Conversa(Efeito, Option<String>),
    Arquivos,
}

/// A forma da soltura por cima do cartão: metade para abrir ao lado, barra de inserção para
/// mover, o painel inteiro para trocar ou anexar. O rótulo é opaco, e tudo entra em 120 ms.
#[derive(IntoElement)]
pub struct FormaSoltura {
    id: ElementId,
    soltura: Soltura,
}

impl FormaSoltura {
    pub fn new(id: impl Into<ElementId>, soltura: Soltura) -> Self {
        FormaSoltura { id: id.into(), soltura }
    }
}

impl RenderOnce for FormaSoltura {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (icone_rotulo, texto, detalhe) = match &self.soltura {
            Soltura::Arquivos => (Lucide::File, "Solte para anexar à mensagem", None),
            Soltura::Conversa(efeito, detalhe) => match efeito.rotulo() {
                Some((i, t)) => (i, t, detalhe.clone()),
                None => return div().into_any_element(),
            },
        };
        let duas = detalhe.is_some();
        let rotulo = div()
            .flex()
            .flex_col()
            .items_center()
            .flutuante(cx)
            .border_1()
            .border_color(Cores::borda_forte())
            .map(|s| {
                if duas {
                    s.rounded(raio::CARTAO).py(r(8.)).px(r(12.)).gap(r(2.))
                } else {
                    s.rounded(raio::de(32.)).h(r(32.)).justify_center().px(r(12.))
                }
            })
            .child(
                h_flex()
                    .gap(r(6.))
                    .child(icone(icone_rotulo, 16., window).text_color(Cores::destaque_texto()))
                    .child(div().rotulo().text_color(Cores::forte()).child(texto)),
            )
            .when_some(detalhe, |s, d| s.child(div().metadado().text_color(Cores::tenue()).child(d)));
        let base = || {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .flex()
                .items_center()
                .justify_center()
                .rounded(crate::tema::raio_da_peca())
        };
        let metade = |lado: Lado| match lado {
            Lado::Esquerda => base().left_0().w(relative(0.5)),
            _ => base().right_0().w(relative(0.5)),
        };
        let realce = |s: Div| s.bg(Cores::selecao()).border_2().border_color(Cores::destaque());
        let forma = match self.soltura {
            Soltura::Arquivos => realce(base().left_0().right_0()).border_dashed(),
            Soltura::Conversa(Efeito::AbrirAoLado(lado), _) => realce(metade(lado)),
            Soltura::Conversa(Efeito::Mover { lado, .. }, _) => {
                let barra = div()
                    .absolute()
                    .top(r(12.))
                    .bottom(r(12.))
                    .w(px(4.))
                    .rounded_full()
                    .bg(Cores::destaque())
                    .map(|b| if lado == Lado::Esquerda { b.left_0() } else { b.right_0() });
                metade(lado).bg(Cores::hover()).child(barra)
            }
            Soltura::Conversa(..) => realce(base().left_0().right_0()),
        };
        forma
            .child(rotulo)
            .with_animation(self.id, movimento::animacao(movimento::COR, movimento::Curva::Sair), |s, delta| s.opacity(delta))
            .into_any_element()
    }
}

impl Janela {
    /// Número (1, 2…) do painel em que a conversa está aberta e se é o ativo, quando há mais
    /// de um painel (com um só, a marca de selecionada basta).
    pub(super) fn painel_da_conversa(&self, chat: &str) -> Option<(usize, bool)> {
        if self.paineis.len() < 2 {
            return None;
        }
        let ix = self.paineis.iter().position(|p| p.chat.as_deref() == Some(chat))?;
        Some((ix + 1, ix == self.ativo))
    }

    /// A conversa está aberta em algum painel.
    pub(super) fn aberta_em_painel(&self, chat: &str) -> bool {
        self.paineis.iter().any(|p| p.chat.as_deref() == Some(chat))
    }

    pub(super) fn criar_painel(
        &mut self,
        chat: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Painel {
        self.proximo_painel += 1;
        let id = self.proximo_painel;
        let composer = cx.new(|cx| {
            TextareaState::new(window, cx)
                .auto_grow(1, super::composer::LINHAS_CAIXA)
                .submit_on_enter(true)
                .placeholder("Mensagem…")
        });
        self._assinaturas.push(cx.subscribe_in(
            &composer,
            window,
            move |this, estado, evento, window, cx| match evento {
                // Ctrl+Enter (o "secundário" do kit) manda depois: com o turno rodando, a
                // mensagem espera a vez na fila em vez de entrar nele.
                InputEvent::PressEnter { shift: false, secondary } => {
                    this.ativar_painel(id, cx);
                    if *secondary {
                        this.enviar_depois(id, window, cx);
                    } else {
                        this.enviar_do_painel(id, window, cx);
                    }
                }
                InputEvent::Focus => this.ativar_painel(id, cx),
                InputEvent::Change => {
                    // "@" digitado no começo ou depois de um espaço abre a lista de arquivos; no
                    // Mestre, que não tem pasta, o "@" cita um chat (o menu da caixa).
                    let texto = estado.read(cx).value().to_string();
                    this.ativar_painel(id, cx);
                    // O rascunho vai para o disco pouco depois de a digitação parar.
                    this.agendar_gravacao(cx);
                    let mestre = this.selecionado.as_deref() == Some(modelo::ID_MESTRE);
                    if (texto == "@" || texto.ends_with(" @"))
                        && !mestre
                        && let Some(chat) = this.selecionado.clone()
                    {
                        this.dialogo_arquivos_projeto(chat, window, cx);
                    }
                    cx.notify();
                }
                _ => {}
            },
        ));
        // A busca dentro da conversa (Ctrl+F): digitar refaz o índice no desenho seguinte, e o
        // Enter (com Shift, para trás) anda entre os achados sem sair do campo.
        let busca_na_conversa = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar na conversa"));
        self._assinaturas.push(cx.subscribe_in(
            &busca_na_conversa,
            window,
            move |this, _, evento, _, cx| match evento {
                InputEvent::PressEnter { shift, .. } => this.andar_na_busca(if *shift { -1 } else { 1 }, cx),
                InputEvent::Change => {
                    this.ativar_painel(id, cx);
                    cx.notify();
                }
                _ => {}
            },
        ));
        // A aba alcançada pelo Tab ativa o painel dela: o Ctrl+W e o menu valem para ele.
        let foco_aba = cx.focus_handle();
        self._assinaturas.push(cx.on_focus(&foco_aba, window, move |this, _, cx| this.ativar_painel(id, cx)));
        let mut painel = Painel {
            id,
            chat: None,
            abas: Vec::new(),
            composer,
            linhas_caixa: super::composer::LINHAS_CAIXA,
            rolagem: ScrollHandle::new(),
            leitura: Default::default(),
            seguindo: true,
            manter_lugar: false,
            modo_foco: false,
            medida: MedidaPainel::default(),
            foco_aba,
            menu_aberto: false,
            foco_esforco: cx.focus_handle(),
            busca_modelo: cx.new(|cx| ListaDeEscolha::new("Buscar modelo", window, cx)),
            busca: super::busca_conversa::Busca::nova(busca_na_conversa),
            base_animacao: 0,
            vistos: 0,
            faixa_vista: false,
            pilula_vista: false,
            entra: false,
            envios: 0,
        };
        if let Some(chat) = chat {
            self.preparar_composer(&mut painel, None, Some(chat), window, cx);
        }
        painel
    }

    /// Troca a conversa de um painel guardando o rascunho da anterior.
    fn preparar_composer(
        &mut self,
        painel: &mut Painel,
        antigo: Option<String>,
        novo: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let texto = painel.composer.read(cx).value().to_string();
        if let Some(antigo) = antigo {
            self.guardar_rascunho(antigo, texto, cx);
        }
        let rascunho = novo
            .as_ref()
            .and_then(|c| self.rascunhos.remove(c))
            .unwrap_or_default();
        let (papel, tamanho) = novo
            .as_ref()
            .and_then(|id| {
                let m = self.motor.as_ref()?.read(cx);
                let chat = m.chat(id)?;
                Some((Some(chat.papel), tamanho_da_conversa(chat.itens.len(), m.parcial(id).is_some())))
            })
            .unwrap_or((None, 0));
        let (avulso, ajudante) = novo
            .as_ref()
            .and_then(|id| {
                let m = self.motor.as_ref()?.read(cx);
                let c = m.chat(id)?;
                let pai = c.pai.as_deref().and_then(|p| m.chat(p));
                Some((c.projeto.is_empty(), pai.is_some_and(|p| p.papel == modelo::Papel::Projeto)))
            })
            .unwrap_or_default();
        let dica = match papel {
            Some(modelo::Papel::Mestre) => "Peça ao Mestre… (#projeto, @chat)",
            Some(modelo::Papel::Trabalhador) if ajudante => "Mensagem direta para este ajudante…",
            Some(modelo::Papel::Trabalhador) => novo
                .as_ref()
                .and_then(|id| Some(self.motor.as_ref()?.read(cx).dica_do_trabalhador(id)))
                .unwrap_or("Mensagem direta para este ajudante…"),
            Some(modelo::Papel::Projeto) if avulso => "Mensagem…",
            Some(modelo::Papel::Projeto) => "Mensagem para este chat…",
            _ => "Mensagem…",
        };
        painel.composer.update(cx, |c, cx| {
            c.set_value(rascunho, window, cx);
            c.set_placeholder(dica, window, cx);
        });
        if let (Some(id), Some(motor)) = (&novo, &self.motor) {
            motor.update(cx, |m, _| m.marcar_lido(id));
        }
        painel.chat = novo;
        painel.seguindo = true;
        painel.rolagem.scroll_to_bottom();
        painel.base_animacao = tamanho;
        painel.vistos = tamanho;
        painel.faixa_vista = false;
        painel.pilula_vista = false;
    }

    /// Envia o que está na caixa do painel e, se foi mesmo (a caixa esvaziou e os anexos saíram;
    /// completar um comando de barra não conta), sobe a geração do pulso do enviar.
    pub(super) fn enviar_do_painel(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let Some(composer) = self.paineis.iter().find(|p| p.id == id).map(|p| p.composer.clone()) else {
            return;
        };
        let chat = self.selecionado.clone();
        let anexos = |this: &Self| {
            chat.as_ref().is_some_and(|c| this.anexos.get(c).is_some_and(|a| !a.is_empty()) || this.tem_issue_anexada(c))
        };
        let tinha = !composer.read(cx).value().trim().is_empty() || anexos(self);
        self.enviar(window, cx);
        let foi = tinha && composer.read(cx).value().is_empty() && !anexos(self);
        if foi && let Some(p) = self.paineis.iter_mut().find(|p| p.id == id) {
            p.envios += 1;
        }
    }

    /// Composer do painel ativo.
    pub(super) fn composer_ativo(&self) -> Option<Entity<TextareaState>> {
        self.paineis.get(self.ativo).map(|p| p.composer.clone())
    }

    /// Alt+1…4: põe o foco na caixa de mensagem do painel com esse número.
    pub(super) fn focar_painel(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.get(ix) else {
            return;
        };
        let id = painel.id;
        let foco = painel.composer.read(cx).focus_handle(cx);
        // A caixa pode estar embaixo de uma gaveta.
        if self.gaveta.take().is_some() {
            cx.notify();
        }
        window.focus(&foco, cx);
        self.ativar_painel(id, cx);
    }

    /// Qualquer clique num cartão ativa o painel, lembrando qual era o ativo antes.
    pub(super) fn clique_no_painel(&mut self, id: u64, cx: &mut Context<Self>) {
        self.ativo_antes_do_clique = self.paineis.get(self.ativo).map(|p| p.id);
        self.ativar_painel(id, cx);
    }

    pub(super) fn ativar_painel(&mut self, id: u64, cx: &mut Context<Self>) {
        if let Some(ix) = self.paineis.iter().position(|p| p.id == id)
            && ix != self.ativo
        {
            self.ativo = ix;
            self.selecionado = self.paineis[ix].chat.clone();
            cx.notify();
        }
    }

    /// Abre a conversa no painel ativo (ou num novo, se não houver nenhum).
    pub fn selecionar(&mut self, chat: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        // "Nova conversa ao lado": a conversa que o diálogo acabou de criar vai para um painel novo.
        if std::mem::take(&mut self.nova_ao_lado)
            && let Some(chat) = chat.clone().filter(|_| !self.paineis.is_empty() && self.cabe_outro_painel())
        {
            return self.abrir_ao_lado(chat, None, window, cx);
        }
        if self.paineis.is_empty() {
            let painel = self.criar_painel(chat.clone(), window, cx);
            self.paineis.push(painel);
            self.ativo = 0;
        } else {
            // Se a conversa já está aberta noutro painel, só ativa ele.
            if let Some(ix) = chat
                .as_ref()
                .and_then(|c| self.paineis.iter().position(|p| p.chat.as_ref() == Some(c)))
            {
                self.ativo = ix;
            } else {
                let ix = self.ativo.min(self.paineis.len() - 1);
                let mut painel = self.paineis.remove(ix);
                let antigo = painel.chat.clone();
                self.preparar_composer(&mut painel, antigo, chat.clone(), window, cx);
                self.paineis.insert(ix, painel);
                self.ativo = ix;
            }
        }
        self.selecionado = self.paineis[self.ativo].chat.clone();
        // Só com o ativo já trocado: a gaveta devolve o foco à caixa dele (o foco na caixa
        // antiga, no quadro seguinte, ativaria de novo o painel antigo).
        self.recolher_gaveta_da_lista(window, cx);
        cx.notify();
    }

    /// Ainda cabe um painel a mais lado a lado: até 4, se a largura da janela der.
    pub(super) fn cabe_outro_painel(&self) -> bool {
        self.paineis.len() < self.orcamento.cabem
    }

    /// Abre a conversa num painel novo ao lado do ativo (ou de `perto`, do lado pedido).
    pub(super) fn abrir_ao_lado(
        &mut self,
        chat: String,
        perto: Option<(u64, Lado)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(ix) = self.paineis.iter().position(|p| p.chat.as_deref() == Some(chat.as_str())) {
            self.ativo = ix;
            self.selecionado = Some(chat);
        } else if !self.cabe_outro_painel() {
            let aviso = if self.paineis.len() >= MAX_PAINEIS {
                format!("Cabem até {MAX_PAINEIS} conversas lado a lado; feche uma para abrir outra.")
            } else {
                "Não cabe outro painel nesta largura. Aumente a janela ou feche um painel.".into()
            };
            window.push_notification(aviso, cx);
        } else {
            let (alvo, lado) = perto
                .and_then(|(id, lado)| Some((self.paineis.iter().position(|p| p.id == id)?, lado)))
                .unwrap_or((self.ativo, Lado::Direita));
            let ix = match lado {
                Lado::Esquerda => alvo,
                _ => (alvo + 1).min(self.paineis.len()),
            };
            let mut painel = self.criar_painel(Some(chat.clone()), window, cx);
            painel.entra = true;
            self.paineis.insert(ix, painel);
            self.ativo = ix;
            self.selecionado = Some(chat);
        }
        // Como em `selecionar`: a gaveta só fecha com o ativo certo.
        self.recolher_gaveta_da_lista(window, cx);
        cx.notify();
    }

    pub(super) fn fechar_painel(&mut self, id: u64, cx: &mut Context<Self>) {
        let Some(ix) = self.paineis.iter().position(|p| p.id == id) else {
            return;
        };
        let painel = self.paineis.remove(ix);
        if let Some(chat) = painel.chat {
            let texto = painel.composer.read(cx).value().to_string();
            self.guardar_rascunho(chat, texto, cx);
        }
        self.ativo = ativo_apos_fechar(self.ativo, ix, self.paineis.len());
        self.selecionado = self.paineis.get(self.ativo).and_then(|p| p.chat.clone());
        cx.notify();
    }

    /// Ctrl+W: fecha a aba à vista do painel ativo; na última aba, fecha o painel (a conversa
    /// continua na lista) e põe o cursor na caixa do que fica ativo.
    pub(super) fn fechar_painel_ativo(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.get(self.ativo) else {
            return;
        };
        let id = painel.id;
        let chat = painel.chat.clone().filter(|_| painel.abas.len() > 1);
        // Pelo teclado não há "painel ativo antes do clique" para voltar.
        self.ativo_antes_do_clique = None;
        match chat {
            Some(chat) => self.fechar_aba(id, &chat, window, cx),
            None => self.fechar_e_focar(id, window, cx),
        }
    }

    /// Clique numa aba que não está à vista: o painel passa a mostrar esse chat.
    pub(super) fn abrir_aba(&mut self, painel_id: u64, chat: String, window: &mut Window, cx: &mut Context<Self>) {
        self.ativar_painel(painel_id, cx);
        self.selecionar(Some(chat), window, cx);
    }

    /// Fecha uma aba do painel (a conversa continua na lista). Se era a que estava à vista, o
    /// painel mostra a vizinha (a de antes; sem ela, a de depois); se era a última, o painel fecha.
    pub(super) fn fechar_aba(&mut self, painel_id: u64, chat: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(painel) = self.paineis.iter_mut().find(|p| p.id == painel_id) else {
            return;
        };
        let a_vista = painel.chat.as_deref() == Some(chat);
        let lugar = painel.abas.iter().position(|a| a == chat);
        painel.abas.retain(|a| a != chat);
        if !a_vista {
            cx.notify();
            return;
        }
        // A vizinha que passa a aparecer: a de antes ou, sem ela, a primeira; nunca um chat que já
        // está à vista noutro painel (um chat só aparece num lugar).
        let restantes = painel.abas.clone();
        let a_vista_noutro =
            |id: &String| self.paineis.iter().any(|p| p.id != painel_id && p.chat.as_ref() == Some(id));
        let vizinha = lugar
            .and_then(|i| restantes.get(i.saturating_sub(1)))
            .filter(|id| !a_vista_noutro(id))
            .or_else(|| restantes.iter().find(|id| !a_vista_noutro(id)))
            .cloned();
        match vizinha {
            Some(vizinha) => self.abrir_aba(painel_id, vizinha, window, cx),
            None => self.fechar_painel_clicado(painel_id, window, cx),
        }
    }

    /// Chamado a cada `render`: cada painel tem o chat à vista entre as abas, sem aba de chat que
    /// sumiu, e no máximo `MAX_ABAS` (a mais antiga que não está à vista sai primeiro).
    pub(super) fn acertar_abas(&mut self, motor: &crate::motor::Motor) {
        // O que cada painel mostra agora: um chat à vista num painel não fica como aba de fundo
        // em outro (a aba não abriria: um chat só aparece num lugar por vez).
        let a_vista: Vec<(u64, String)> = self.paineis.iter().filter_map(|p| Some((p.id, p.chat.clone()?))).collect();
        for painel in &mut self.paineis {
            let id_do_painel = painel.id;
            painel.abas.retain(|id| {
                motor.chat(id).is_some() && !a_vista.iter().any(|(outro, chat)| *outro != id_do_painel && chat == id)
            });
            // Sem chat à vista (o painel vazio que sobra quando o último chat sai), sem abas.
            if painel.chat.is_none() {
                painel.abas.clear();
            }
            if let Some(chat) = &painel.chat
                && !painel.abas.contains(chat)
            {
                painel.abas.push(chat.clone());
            }
            while painel.abas.len() > MAX_ABAS {
                let Some(velha) = painel.abas.iter().position(|a| Some(a) != painel.chat.as_ref()) else {
                    break;
                };
                painel.abas.remove(velha);
            }
        }
    }

    /// Fecha o painel pelo teclado ou pelo menu e põe o foco na caixa do que fica ativo.
    pub(super) fn fechar_e_focar(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        self.fechar_painel(id, cx);
        if !self.paineis.is_empty() {
            self.focar_painel(self.ativo, window, cx);
        }
    }

    /// Fecha todos os painéis menos este, que fica ativo.
    pub(super) fn fechar_outros(&mut self, id: u64, cx: &mut Context<Self>) {
        let outros: Vec<u64> = self.paineis.iter().map(|p| p.id).filter(|p| *p != id).collect();
        for outro in outros {
            self.fechar_painel(outro, cx);
        }
        if let Some(ix) = self.paineis.iter().position(|p| p.id == id) {
            self.ativo = ix;
            self.selecionado = self.paineis[ix].chat.clone();
        }
        cx.notify();
    }

    /// "Nova conversa ao lado": o diálogo de sempre, e a conversa criada abre num painel novo
    /// (`selecionar` consome a marca). Fechado o diálogo sem criar nada, a marca cai.
    pub(super) fn nova_conversa_ao_lado(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.nova_ao_lado = true;
        self.dialogo_nova_conversa(window, cx);
        if !self.nova_ao_lado {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(std::time::Duration::from_millis(300)).await;
                let segue = this.update_in(cx, |this, window, cx| {
                    let aberto = this.nova_ao_lado && window.has_active_dialog(cx);
                    if !aberto {
                        this.nova_ao_lado = false;
                    }
                    aberto
                });
                if !matches!(segue, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// Ctrl+PgUp e Ctrl+PgDn: o foco vai para a caixa do painel vizinho, dando a volta nas pontas.
    pub(super) fn painel_vizinho(&mut self, passo: isize, window: &mut Window, cx: &mut Context<Self>) {
        let n = self.paineis.len() as isize;
        if n > 1 {
            let ix = (self.ativo as isize + passo).rem_euclid(n) as usize;
            self.focar_painel(ix, window, cx);
        }
    }

    /// Ctrl+Shift+PgUp e Ctrl+Shift+PgDn: o painel ativo troca de lugar com o vizinho.
    pub(super) fn mover_painel(&mut self, passo: isize, cx: &mut Context<Self>) {
        if let Some(id) = self.paineis.get(self.ativo).map(|p| p.id) {
            self.mover_este_painel(id, passo, cx);
        }
    }

    /// O painel `id` anda `passo` casas (o menu "⋯" e os atalhos), e fica ativo.
    pub(super) fn mover_este_painel(&mut self, id: u64, passo: isize, cx: &mut Context<Self>) {
        let Some(de) = self.paineis.iter().position(|p| p.id == id) else {
            return;
        };
        let Some(para) = destino_ao_mover(de, passo, self.paineis.len()) else {
            return;
        };
        let movido = self.paineis.remove(de);
        self.paineis.insert(para, movido);
        self.ativo = para;
        self.selecionado = self.paineis[para].chat.clone();
        cx.notify();
    }

    /// Guarda a medida do cartão do painel quando ela muda de nível ou de faixa: no quadro
    /// seguinte, porque avisar a janela durante o desenho não a redesenha.
    pub(super) fn medir_painel(
        janela: WeakEntity<Janela>,
        painel: u64,
        atual: MedidaPainel,
    ) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
        move |limites, window, _| {
            let mut nova = atual;
            if !nova.atualizar(limites.size.width, window.rem_size()) {
                return;
            }
            window.on_next_frame(move |_, cx| {
                let _ = janela.update(cx, |this, cx| {
                    if let Some(p) = this.paineis.iter_mut().find(|p| p.id == painel) {
                        p.medida = nova;
                        cx.notify();
                    }
                });
            });
        }
    }

    /// × ou clique do meio na aba: o clique ativou o painel, então ao fechá-lo o ativo volta a
    /// ser o de antes, como ao fechar uma aba de fundo. Se o foco estava no painel fechado (ou
    /// sumiu com ele), vai para a caixa do que fica ativo, como no Ctrl+W.
    pub(super) fn fechar_painel_clicado(&mut self, id: u64, window: &mut Window, cx: &mut Context<Self>) {
        let antes = self.ativo_antes_do_clique.take().filter(|a| *a != id);
        let tinha_foco = self.paineis.iter().find(|p| p.id == id).is_some_and(|p| {
            p.foco_aba.contains_focused(window, cx)
                || p.foco_esforco.contains_focused(window, cx)
                || p.composer.read(cx).focus_handle(cx).contains_focused(window, cx)
        });
        self.fechar_painel(id, cx);
        if let Some(ix) = antes.and_then(|a| self.paineis.iter().position(|p| p.id == a)) {
            self.ativo = ix;
            self.selecionado = self.paineis[ix].chat.clone();
        }
        if !self.paineis.is_empty() && (tinha_foco || window.focused(cx).is_none()) {
            self.focar_painel(self.ativo, window, cx);
        }
    }

    /// O que soltar a conversa no `lado` do painel faria agora.
    pub(super) fn efeito_soltura(&self, arrasto: &ArrastoChat, painel: u64, lado: Lado) -> Efeito {
        // Da lista, uma conversa já aberta noutro painel se comporta como a aba dele.
        let origem = self.origem_do_arrasto(arrasto);
        let ids: Vec<u64> = self.paineis.iter().map(|p| p.id).collect();
        let com_outras_abas =
            origem.and_then(|o| self.paineis.iter().find(|p| p.id == o)).is_some_and(|p| p.abas.len() > 1);
        efeito_de(&ids, origem, painel, lado, self.cabe_outro_painel(), com_outras_abas)
    }

    /// O painel em que a conversa arrastada já está: o da aba arrastada ou, vinda da lista, o que
    /// a mostra agora. A faixa da soltura e a soltura usam a mesma conta (com contas diferentes, a
    /// conversa da lista que estava à vista num painel de várias abas fechava o painel inteiro).
    fn origem_do_arrasto(&self, arrasto: &ArrastoChat) -> Option<u64> {
        arrasto.de_painel.filter(|id| self.paineis.iter().any(|p| p.id == *id)).or_else(|| {
            self.paineis.iter().find(|p| p.chat.as_deref() == Some(arrasto.chat.as_str())).map(|p| p.id)
        })
    }

    /// O painel de origem da conversa arrastada, se ele tem outras abas além dela.
    fn painel_com_outras_abas(&self, arrasto: &ArrastoChat) -> Option<u64> {
        let origem = self.origem_do_arrasto(arrasto)?;
        self.paineis
            .iter()
            .find(|p| p.id == origem && p.abas.len() > 1 && p.abas.contains(&arrasto.chat))
            .map(|p| p.id)
    }

    /// Solta uma conversa arrastada num painel fazendo o que a faixa mostrou.
    pub(super) fn soltar(
        &mut self,
        arrasto: &ArrastoChat,
        painel: u64,
        efeito: Efeito,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.alvo_arrasto = None;
        match efeito {
            Efeito::Nenhum => cx.notify(),
            Efeito::Abrir => self.selecionar(Some(arrasto.chat.clone()), window, cx),
            Efeito::AbrirAoLado(lado) => {
                // A aba que sai de um painel com outras abas: ele fica com as outras, e ela abre
                // num painel novo.
                if let Some(origem) = self.painel_com_outras_abas(arrasto) {
                    self.fechar_aba(origem, &arrasto.chat, window, cx);
                }
                self.abrir_ao_lado(arrasto.chat.clone(), Some((painel, lado)), window, cx)
            }
            // O painel inteiro muda de lugar.
            Efeito::Mover { origem, lado } => {
                let ids: Vec<u64> = self.paineis.iter().map(|p| p.id).collect();
                if let (Some(de), Some(ix)) = (
                    self.paineis.iter().position(|p| p.id == origem),
                    posicao_ao_mover(&ids, origem, painel, lado),
                ) {
                    let movido = self.paineis.remove(de);
                    let ix = ix.min(self.paineis.len());
                    self.paineis.insert(ix, movido);
                    self.ativo = ix;
                    self.selecionado = self.paineis[ix].chat.clone();
                }
                cx.notify();
            }
            Efeito::Trocar { origem, .. } => {
                // A conversa sai da origem antes (a aba, se o painel tem outras; senão o painel
                // inteiro): os índices andam, e o destino é achado de novo pelo id.
                if let Some(origem) = origem {
                    match self.painel_com_outras_abas(arrasto) {
                        Some(com_abas) if com_abas == origem => self.fechar_aba(origem, &arrasto.chat, window, cx),
                        _ => self.fechar_painel(origem, cx),
                    }
                }
                if let Some(ix) = self.paineis.iter().position(|p| p.id == painel) {
                    self.ativo = ix;
                }
                // Abre como mais uma aba do painel (as que já estavam ficam).
                self.selecionar(Some(arrasto.chat.clone()), window, cx);
            }
        }
    }

    /// Zona de soltura de um painel: guarda o lado sob o ponteiro e mostra o que vai acontecer.
    pub(super) fn zona_de_soltura(
        &self,
        painel: u64,
        conteudo: AnyElement,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let alvo = self
            .alvo_arrasto
            .filter(|(id, ..)| *id == painel)
            .map(|(_, _, efeito)| efeito);
        let abertos = self.paineis.len();
        div()
            .id(("painel", painel as usize))
            .relative()
            .size_full()
            .child(conteudo)
            .on_drag_move(cx.listener(move |this, evento: &DragMoveEvent<ArrastoChat>, _, cx| {
                // A aba de origem fica marcada enquanto o arrasto dura (só a que está à vista: uma
                // aba compacta arrastada não traceja a vizinha).
                let arrasto = evento.drag(cx);
                let origem = arrasto
                    .de_painel
                    .filter(|id| this.paineis.iter().any(|p| p.id == *id && p.chat.as_deref() == Some(arrasto.chat.as_str())));
                if this.arrastando_aba != origem {
                    this.arrastando_aba = origem;
                    cx.notify();
                }
                let limites = evento.bounds;
                let x = evento.event.position.x;
                if !limites.contains(&evento.event.position) {
                    if this.alvo_arrasto.is_some_and(|(id, ..)| id == painel) {
                        this.alvo_arrasto = None;
                        cx.notify();
                    }
                    return;
                }
                let terco = limites.size.width / 3.;
                let lado = if x < limites.origin.x + terco {
                    Lado::Esquerda
                } else if x > limites.origin.x + terco * 2. {
                    Lado::Direita
                } else {
                    Lado::Centro
                };
                let novo = Some((painel, lado, this.efeito_soltura(evento.drag(cx), painel, lado)));
                if this.alvo_arrasto != novo {
                    this.alvo_arrasto = novo;
                    cx.notify();
                }
            }))
            .on_drop(cx.listener(move |this, arrasto: &ArrastoChat, window, cx| {
                this.arrastando_aba = None;
                let efeito = match this.alvo_arrasto.filter(|(id, ..)| *id == painel) {
                    Some((_, _, efeito)) => efeito,
                    None => this.efeito_soltura(arrasto, painel, Lado::Centro),
                };
                this.soltar(arrasto, painel, efeito, window, cx);
            }))
            .when_some(alvo.filter(|e| *e != Efeito::Nenhum), |s, efeito| {
                // Da borda, sem lugar para outro painel, a soltura troca e diz por quê.
                let detalhe = matches!(efeito, Efeito::Trocar { cheio: true, .. }).then(|| motivo_sem_lugar(abertos));
                s.child(FormaSoltura::new(("soltura", painel as usize), Soltura::Conversa(efeito, detalhe)))
            })
    }
}

#[cfg(test)]
mod testes_soltura {
    use super::{Efeito, Lado, ativo_apos_fechar, destino_ao_mover, efeito_de, motivo_sem_lugar, posicao_ao_mover};

    const A: u64 = 1;
    const B: u64 = 2;
    const C: u64 = 3;
    const D: u64 = 4;

    #[test]
    fn sem_paineis_abre_no_lugar_vazio() {
        assert_eq!(efeito_de(&[], None, 0, Lado::Esquerda, true, false), Efeito::Abrir);
    }

    #[test]
    fn aba_no_meio_de_outro_painel_troca_e_fecha_a_origem() {
        assert_eq!(
            efeito_de(&[A, B, C], Some(A), B, Lado::Centro, true, false),
            Efeito::Trocar { origem: Some(A), cheio: false }
        );
    }

    #[test]
    fn soltar_no_proprio_painel_ou_onde_ja_esta_nao_faz_nada() {
        for lado in [Lado::Esquerda, Lado::Centro, Lado::Direita] {
            assert_eq!(efeito_de(&[A, B], Some(A), A, lado, true, false), Efeito::Nenhum);
        }
        // A já encosta na esquerda de B, e C na direita.
        assert_eq!(efeito_de(&[A, B, C], Some(A), B, Lado::Esquerda, true, false), Efeito::Nenhum);
        assert_eq!(efeito_de(&[A, B, C], Some(C), B, Lado::Direita, true, false), Efeito::Nenhum);
    }

    #[test]
    fn aba_na_borda_move_o_painel() {
        assert_eq!(
            efeito_de(&[A, B, C], Some(A), B, Lado::Direita, true, false),
            Efeito::Mover { origem: A, lado: Lado::Direita }
        );
        // Com 4 painéis mover continua valendo: não abre painel novo.
        assert_eq!(
            efeito_de(&[A, B, C, D], Some(D), A, Lado::Esquerda, false, false),
            Efeito::Mover { origem: D, lado: Lado::Esquerda }
        );
    }

    #[test]
    fn da_lista_abre_ao_lado_ou_troca_quando_nao_cabe() {
        assert_eq!(
            efeito_de(&[A, B], None, B, Lado::Direita, true, false),
            Efeito::AbrirAoLado(Lado::Direita)
        );
        assert_eq!(
            efeito_de(&[A, B, C, D], None, B, Lado::Direita, false, false),
            Efeito::Trocar { origem: None, cheio: true }
        );
        assert_eq!(
            efeito_de(&[A, B, C, D], None, B, Lado::Centro, false, false),
            Efeito::Trocar { origem: None, cheio: false }
        );
        // Origem que não está mais na tela conta como conversa da lista.
        assert_eq!(
            efeito_de(&[A, B], Some(9), A, Lado::Esquerda, true, false),
            Efeito::AbrirAoLado(Lado::Esquerda)
        );
    }

    #[test]
    fn posicao_ao_mover_conta_sem_a_origem() {
        // [A, B, C, D]: A para a direita de C fica [B, C, A, D].
        assert_eq!(posicao_ao_mover(&[A, B, C, D], A, C, Lado::Direita), Some(2));
        assert_eq!(posicao_ao_mover(&[A, B, C, D], D, A, Lado::Esquerda), Some(0));
        assert_eq!(posicao_ao_mover(&[A, B], A, 9, Lado::Esquerda), None);
    }

    #[test]
    fn mover_o_painel_para_nas_pontas() {
        assert_eq!(destino_ao_mover(0, 1, 3), Some(1));
        assert_eq!(destino_ao_mover(2, -1, 3), Some(1));
        assert_eq!(destino_ao_mover(0, -1, 3), None);
        assert_eq!(destino_ao_mover(2, 1, 3), None);
        assert_eq!(destino_ao_mover(0, 1, 1), None);
    }

    #[test]
    fn sem_lugar_diz_se_e_o_maximo_ou_a_largura() {
        assert_eq!(motivo_sem_lugar(4), "Já há 4 lado a lado");
        assert_eq!(motivo_sem_lugar(2), "Não cabe outro nesta largura");
    }

    #[test]
    fn fechar_a_esquerda_do_ativo_mantem_o_mesmo_painel_ativo() {
        // [A, B, C] com C ativo: fechar A deixa C (agora no índice 1) ativo.
        assert_eq!(ativo_apos_fechar(2, 0, 2), 1);
        // Fechar o ativo passa para o vizinho da direita, ou o da esquerda se era o último.
        assert_eq!(ativo_apos_fechar(0, 0, 2), 0);
        assert_eq!(ativo_apos_fechar(2, 2, 2), 1);
        assert_eq!(ativo_apos_fechar(1, 2, 2), 1);
        assert_eq!(ativo_apos_fechar(0, 0, 0), 0);
    }

    #[test]
    fn aba_de_painel_com_outras_abas_vira_painel_novo_na_borda() {
        // Na borda de outro painel e na do próprio, se couber mais um.
        assert_eq!(efeito_de(&[A, B], Some(A), B, Lado::Direita, true, true), Efeito::AbrirAoLado(Lado::Direita));
        assert_eq!(efeito_de(&[A, B], Some(A), A, Lado::Esquerda, true, true), Efeito::AbrirAoLado(Lado::Esquerda));
        // No meio de outro painel entra como aba dele; no meio do próprio, nada.
        assert_eq!(efeito_de(&[A, B], Some(A), B, Lado::Centro, true, true), Efeito::Trocar { origem: Some(A), cheio: false });
        assert_eq!(efeito_de(&[A, B], Some(A), A, Lado::Centro, true, true), Efeito::Nenhum);
    }

    #[test]
    fn aba_de_painel_com_outras_abas_sem_lugar_para_mais_um() {
        assert_eq!(efeito_de(&[A, B, C, D], Some(A), A, Lado::Direita, false, true), Efeito::Nenhum);
        assert_eq!(
            efeito_de(&[A, B, C, D], Some(A), B, Lado::Direita, false, true),
            Efeito::Trocar { origem: Some(A), cheio: true }
        );
    }
}
