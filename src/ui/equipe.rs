//! Painel da direita: num chat de projeto, "Este chat" (onde ele trabalha, o que falta juntar, o
//! pedido em aberto e os ajudantes); numa conversa antiga, a equipe do coordenador (com a tarefa do
//! trabalhador aberto em destaque) e as decisões anotadas do projeto.

use std::collections::BTreeMap;

use gpui_kit::component::{
    Disableable as _, ElementExt as _, Sizable as _, WindowExt as _,
    button::{Button, ButtonCustomVariant, ButtonVariants as _},
    clipboard::Clipboard,
    menu::{DropdownMenu as _, PopupMenu, PopupMenuItem},
    scroll::ScrollableElement as _,
    text::TextView,
    tooltip::Tooltip,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Gaveta, Janela,
    componentes::{
        Alerta, BarraSegmentada, BotaoCopiar, CabecalhoDeSecao, CaixaDeCodigo, CartaoDeLista, Estado, EstadoVazio,
        EstadoVisual, PilulaDeEstado, PreviaArrasto, Reflexo as _, Selo, Simbolo, TamanhoBotao, Tom,
        botao_fantasma, botao_icone, botao_perigo, botao_primario, contagem, estado_visual, numero_painel,
        pilula_da_tarefa, rotulo_tarefa, selo_gravidade, variante_perigo,
    },
    menus,
    movimento::{self, Curva},
    paineis::{ArrastoChat, Lado, MAX_PAINEIS},
    sem_arrastar,
};
use crate::{
    modelo::{Chat, EstadoTarefa, Execucao, Forma, Papel, Revisao, Tarefa, Verificacao},
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, altura, dentro_da_borda, esp, halo_foco, r, raio, tam},
};

/// Abaixo desta largura (em rem, uns 330 px a 100%) o painel da direita fica no desenho estreito.
const LIMITE_ESTREITA: f32 = 22.;
/// A partir desta largura (px a 100%), os botões do resumo ganham o rótulo.
const LARGA: f32 = 450.;

/// A ação principal de um cartão (uma só, por prioridade).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Principal {
    VerPedido,
    Retomar,
    Revisar,
}

impl Janela {
    pub(super) fn equipe(
        &mut self,
        motor_entidade: &Entity<Motor>,
        window: &mut Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let motor = motor_entidade.read(cx);
        let chat = self.selecionado.as_ref().and_then(|id| motor.chat(id));
        // Num trabalhador, a direita continua mostrando a equipe, com a tarefa dele em destaque; no
        // chat de projeto, a tarefa do próprio chat.
        let destacada = chat
            .filter(|c| matches!(c.papel, Papel::Trabalhador | Papel::Projeto))
            .and_then(|c| c.tarefa);
        let coordenador = chat.and_then(|c| coordenador_da_equipe(motor, c));

        let mut filhos: Vec<(Bloco, AnyElement)> = Vec::new();
        let mut ix_destacada = None;
        if let Some(chat) = chat {
            // Um botão azul sólido no painel: o do cartão que pede ação ou, sem nenhum, o do aviso.
            let solida = coordenador.as_deref().and_then(|c| primaria(motor, c, destacada));
            if chat.papel == Papel::Mestre {
                filhos.extend(self.distribuido(motor, window, cx));
            } else if chat.papel == Papel::Projeto {
                filhos.push((Bloco::Caixa, self.este_chat(motor_entidade, chat, window, cx)));
                // O cartão da tarefa do próprio chat só com um pedido aberto (ou uma conferência);
                // em conversa, "Este chat" já diz tudo. Os ajudantes vêm embaixo.
                let com_pedido = chat
                    .tarefa
                    .and_then(|t| motor.tarefa(t))
                    .is_some_and(|t| t.dono().is_some() || t.estado != EstadoTarefa::Conversa || t.revisao.is_some());
                let ajudantes = motor.tarefas_do(&chat.id).next().is_some();
                if com_pedido || ajudantes {
                    ix_destacada = self.lista_tarefas(motor, &chat.id, destacada.filter(|_| com_pedido), solida, &mut filhos, window, cx);
                }
            } else if let Some(coordenador) = coordenador.as_ref().filter(|_| chat.papel != Papel::Mestre) {
                ix_destacada = self.lista_tarefas(motor, coordenador, destacada, solida, &mut filhos, window, cx);
            }
            let notas = motor.dados.notas(&chat.projeto);
            if chat.papel == Papel::Coordenador && !notas.trim().is_empty() {
                filhos.push((Bloco::Secao, self.decisoes(&chat.projeto, notas, cx)));
            }
        }
        // Ao trocar de chat, o cartão do trabalhador ativo pode estar fora da tela.
        let atual = self.selecionado.clone();
        let mudou = std::mem::replace(&mut self.equipe_rolada, atual.clone()) != atual;
        if mudou {
            match ix_destacada {
                Some(ix) => self.rolagem_equipe.scroll_to_top_of_item(ix),
                None => self.rolagem_equipe.set_offset(point(px(0.), px(0.))),
            }
        }

        // Com a lista rolada, uma borda separa o cabeçalho, como na conversa.
        let rolado = self.rolagem_equipe.offset().y < px(0.);
        let cabecalho = self.barra_de_abas(motor, super::abas_direita::AbaDireita::Chat, rolado, "arrastar-equipe", window, cx);
        // O respiro de cima e o de baixo vão nos próprios filhos: com padding, a área rolável
        // somava os 20 ao máximo (por causa da barra, absoluta) e rolava sozinha.
        let mut anterior = None;
        let filhos = filhos.into_iter().map(move |(bloco, filho)| {
            let antes = std::mem::replace(&mut anterior, Some(bloco));
            let mt = if antes.is_none() { esp::E4 } else { respiro(antes, bloco) };
            div().flex_none().mt(mt).child(filho)
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .rounded(raio::PECA)
            .when(self.gaveta != Some(Gaveta::Direita), |s| s.bg(Cores::cartao()).reflexo(raio::PECA))
            .overflow_hidden()
            .child(cabecalho)
            .child(
                div()
                    .id("painel-equipe")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.rolagem_equipe)
                    .flex()
                    .flex_col()
                    .px(esp::E12)
                    // Sem encolher: o que passa da altura rola, em vez de espremer os cartões.
                    .children(filhos)
                    .child(div().flex_none().h(esp::E16))
                    .vertical_scrollbar(&self.rolagem_equipe),
            )
            .on_prepaint(self.medir_direita(cx))
    }

    /// Guarda se o painel da direita ficou estreito (abaixo de 22 rem, com folga) quando isso
    /// muda: no quadro seguinte, porque avisar a janela durante o desenho não a redesenha.
    pub(super) fn medir_direita(
        &self,
        cx: &Context<Self>,
    ) -> impl FnOnce(Bounds<Pixels>, &mut Window, &mut App) + 'static {
        let janela = cx.entity().downgrade();
        let (antes, compacta_antes) = (self.direita_estreita, self.direita_compacta);
        move |limites, window, _| {
            let rem = limites.size.width / window.rem_size();
            let estreita = fica_estreita(rem, antes);
            let compacta = fica_compacta(rem, compacta_antes);
            if estreita == antes && compacta == compacta_antes {
                return;
            }
            window.on_next_frame(move |_, cx| {
                let _ = janela.update(cx, |this, cx| {
                    this.direita_estreita = estreita;
                    this.direita_compacta = compacta;
                    cx.notify();
                });
            });
        }
    }

    /// "Este chat": onde o chat de projeto trabalha e o que dá para fazer com isso. Na cópia, a
    /// pasta de onde ela saiu, o branch, quanto está à frente e atrás e o que falta juntar, com
    /// Revisar e juntar e o parecer do revisor; direto na pasta própria, o commit de cada vez;
    /// direto numa pasta sem git, como ativar; no avulso, a pasta dele; sem a pasta, Localizar.
    fn este_chat(&mut self, motor_e: &Entity<Motor>, chat: &Chat, window: &Window, cx: &Context<Self>) -> AnyElement {
        let motor = motor_e.read(cx);
        let Some(t) = chat.tarefa.and_then(|t| motor.tarefa(t)) else {
            return div().into_any_element();
        };
        let origem = motor.origem_da_tarefa(t);
        let curta = super::dialogos::com_til(&origem);
        let mut linhas: Vec<AnyElement> = Vec::new();
        let mut botoes: Vec<AnyElement> = Vec::new();
        let texto = |t: String| div().corpo_ui().text_color(Cores::texto()).child(t).into_any_element();
        let abrir_pasta = |id: &'static str, pasta: std::path::PathBuf, window: &Window| {
            botao_fantasma(id, Some(Lucide::FolderOpen.into()), "Abrir pasta", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&pasta, cx))
                .into_any_element()
        };
        let sumiu = !origem.as_os_str().is_empty() && !origem.exists();
        if sumiu {
            let indice = motor.pastas_do(&chat.projeto).iter().position(|p| p.caminho == origem);
            let projeto = chat.projeto.clone();
            let alerta = Alerta::new(Tom::Erro, format!("A pasta {curta} não foi encontrada (disco desmontado ou pasta renomeada)."))
                .when_some(indice, |a, indice| {
                    a.acao(
                        botao_fantasma("este-chat-localizar", Some(Lucide::FolderSearch.into()), "Localizar pasta…", TamanhoBotao::Barra, window, cx)
                            .on_click(cx.listener(move |this, _, window, cx| this.localizar_pasta(projeto.clone(), indice, window, cx))),
                    )
                });
            linhas.push(alerta.into_any_element());
        }
        let estreita = self.direita_estreita;
        // O pedido aberto do Mestre: de que divisão e o caminho de volta a ele.
        if t.coordenador == crate::modelo::ID_MESTRE {
            let divisao = t.divisao.map(|d| format!("divisão {d}, ")).unwrap_or_default();
            linhas.push(
                h_flex_linha()
                    .child(svg().path(Lucide::Orbit.path()).flex_none().size(r(14.)).text_color(Cores::destaque_texto()))
                    .child(div().flex_1().min_w_0().corpo_ui().text_color(Cores::texto()).child(format!("Pedido do Mestre ({divisao}rodada {})", t.rodada)))
                    .into_any_element(),
            );
            botoes.push(
                botao_fantasma("este-chat-mestre", Some(Lucide::CornerLeftUp.into()), "Mestre", TamanhoBotao::Barra, window, cx)
                    .tooltip("Abrir o Mestre, que fez o pedido")
                    .on_click(cx.listener(|this, _, window, cx| this.abrir_mestre(window, cx)))
                    .into_any_element(),
            );
        }
        match t.forma {
            // A conversa trazida do Claude Code mexe direto na pasta dela (sem Ativar git: a pasta
            // pode ser a pessoal).
            Forma::Direta if chat.importada.is_some() => {
                if let Some(i) = &chat.importada {
                    linhas.push(texto(crate::importar::descricao(i, &curta)));
                }
                linhas.push(linha_dado(format!("este-chat-pasta-{}", t.id), "Pasta", curta.clone(), origem.display().to_string(), origem.display().to_string(), 56.).into_any_element());
                botoes.push(abrir_pasta("este-chat-abrir", origem.clone(), window));
            }
            _ if chat.projeto.is_empty() => {
                linhas.push(texto("Um chat avulso, numa pasta só dele: os arquivos que ele criar ficam lá.".into()));
                linhas.push(linha_dado(format!("este-chat-pasta-{}", t.id), "Pasta", curta.clone(), origem.display().to_string(), origem.display().to_string(), 56.).into_any_element());
                botoes.push(abrir_pasta("este-chat-abrir", origem.clone(), window));
            }
            Forma::Copia => {
                let copia = t.worktree.clone().filter(|w| w.is_dir());
                linhas.push(texto("Trabalha numa cópia só dele, num branch próprio; Juntar leva as mudanças para a origem.".into()));
                linhas.push(linha_dado(format!("este-chat-origem-{}", t.id), "Origem", curta.clone(), origem.display().to_string(), origem.display().to_string(), 56.).into_any_element());
                if let Some(branch) = &t.branch {
                    let alvo = t.ramo_alvo.clone().map(|r| format!(" → {r}")).unwrap_or_default();
                    linhas.push(linha_dado(format!("este-chat-branch-{}", t.id), "Branch", format!("{branch}{alvo}"), branch.clone(), branch.clone(), 56.).into_any_element());
                }
                match &copia {
                    None if t.preparada => linhas.push(texto("A cópia foi limpa; a próxima mensagem a refaz no mesmo lugar.".into())),
                    None => linhas.push(texto("A cópia nasce na primeira mensagem.".into())),
                    Some(copia) => {
                        let estado = self.estado_da_copia(t.id, copia, &origem, cx);
                        let mut partes = Vec::new();
                        if let Some(e) = estado {
                            partes.push(format!("{} à frente · {} atrás", e.a_frente, e.atras));
                            if e.suja {
                                partes.push("mudanças sem commit".into());
                            }
                        }
                        if let Some(m) = t.mudancas.as_ref().filter(|m| m.arquivos > 0) {
                            partes.push(format!("{} · +{} −{}", contagem(m.arquivos as usize, "arquivo", "arquivos"), m.mais, m.menos));
                        } else if estado.is_some() {
                            partes.push("nada para juntar".into());
                        }
                        if !partes.is_empty() {
                            linhas.push(div().metadado().text_color(Cores::tenue()).child(partes.join(" · ")).into_any_element());
                        }
                        let tarefa = t.id;
                        let tem_mudancas = t.mudancas.as_ref().is_some_and(|m| m.arquivos > 0);
                        // O modo de entrega da pasta vai no rótulo do botão (seção 4.2).
                        let modo_pr = motor.config_da_pasta(&t.projeto, &origem).github.entrega == crate::modelo::ModoEntrega::Pr;
                        let rodando = motor.execucao(&chat.id).rodando() || motor.na_fila(&chat.id) > 0;
                        let suja = estado.as_ref().is_some_and(|e| e.suja);
                        let revisar = |id: &'static str, icone: Lucide, rotulo: String, dica: &'static str| {
                            botao_fantasma(id, Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx)
                                .tooltip(dica)
                                .on_click(cx.listener(move |this, _, window, cx| this.dialogo_diff(tarefa, window, cx)))
                        };
                        match entrega_do_chat(modo_pr, t, suja, rodando) {
                            Entrega::Juntar => {
                                let b = revisar("este-chat-revisar", Lucide::GitMerge, "Revisar e juntar".into(), "Ver as mudanças da cópia e juntá-las na pasta original");
                                botoes.push(if tem_mudancas { b.custom(variante_tinta(cx)) } else { b }.into_any_element());
                            }
                            Entrega::AbrirPr => {
                                let b = revisar("este-chat-revisar", Lucide::GitPullRequest, "Revisar e abrir PR".into(), "Ver as mudanças da cópia, escrever o título e a descrição e abrir o PR");
                                botoes.push(if tem_mudancas { b.custom(variante_tinta(cx)) } else { b }.into_any_element());
                            }
                            Entrega::RevisarEAtualizar => {
                                let b = revisar("este-chat-revisar", Lucide::GitPullRequest, "Revisar e atualizar PR".into(), "Ver o que mudou desde o PR e mandar para ele");
                                botoes.push(b.custom(variante_tinta(cx)).into_any_element());
                            }
                            Entrega::AtualizarPr => {
                                botoes.push(
                                    botao_primario("este-chat-atualizar-pr", Some(Lucide::GitPullRequest.into()), "Atualizar PR", TamanhoBotao::Barra, window, cx)
                                        .tooltip("A rodada passou na verificação: manda as mudanças para o PR")
                                        .on_click(cx.listener(move |this, _, window, cx| this.atualizar_pr_do_usuario(tarefa, window, cx)))
                                        .into_any_element(),
                                );
                                botoes.push(revisar("este-chat-revisar", Lucide::Eye, "Ver mudanças".into(), "Ver as mudanças da cópia").into_any_element());
                            }
                            Entrega::VerPr(n) => {
                                botoes.push(
                                    botao_fantasma("este-chat-ver-pr", Some(Lucide::GitPullRequest.into()), format!("Ver PR #{n}"), TamanhoBotao::Barra, window, cx)
                                        .tooltip("Abrir a aba PR")
                                        .on_click(cx.listener(|this, _, _, cx| this.escolher_aba(super::abas_direita::AbaDireita::Pr, cx)))
                                        .into_any_element(),
                                );
                            }
                            Entrega::Trabalhando(rotulo) => {
                                botoes.push(
                                    botao_fantasma("este-chat-revisar", Some(Lucide::GitPullRequest.into()), rotulo, TamanhoBotao::Barra, window, cx)
                                        .disabled(true)
                                        .tooltip("O chat está trabalhando")
                                        .into_any_element(),
                                );
                            }
                        }
                        let (m, id) = (motor_e.clone(), chat.id.clone());
                        let rotulo = if estreita { "Parecer" } else { "Pedir parecer do revisor" };
                        botoes.push(
                            botao_fantasma("este-chat-parecer", Some(Lucide::ListChecks.into()), rotulo, TamanhoBotao::Barra, window, cx)
                                .tooltip("Um revisor confere o que o chat mudou desde a última junção")
                                .on_click(move |_, _, cx| m.update(cx, |m, cx| m.pedir_parecer(&id, cx)))
                                .into_any_element(),
                        );
                    }
                }
            }
            Forma::DiretaGit => {
                linhas.push(texto(format!("Os arquivos ficam direto em {curta}; cada vez vira um commit (Voltar no tempo desfaz).")));
                botoes.push(abrir_pasta("este-chat-abrir", origem.clone(), window));
            }
            Forma::Direta => {
                linhas.push(texto(format!("Mexe direto em {curta}, que não usa git: não há cópia, revisão nem Voltar no tempo.")));
                if !sumiu {
                    let pasta = origem.clone();
                    let nome = origem.file_name().map_or_else(|| curta.clone(), |n| n.to_string_lossy().to_string());
                    botoes.push(
                        botao_fantasma("este-chat-git", Some(Lucide::GitBranch.into()), format!("Ativar git em {nome}…"), TamanhoBotao::Barra, window, cx)
                            .tooltip("Os próximos chats ganham uma cópia; este continua direto na pasta")
                            .on_click(cx.listener(move |this, _, window, cx| this.confirmar_git_na_pasta(pasta.clone(), window, cx)))
                            .into_any_element(),
                    );
                }
                botoes.push(abrir_pasta("este-chat-abrir", origem.clone(), window));
            }
        }
        div()
            .flex()
            .flex_col()
            .gap(esp::E8)
            .p(dentro_da_borda(12.))
            .rounded(raio::CARTAO)
            .bg(Cores::cartao_elevado())
            .border_1()
            .border_color(Cores::linha())
            .reflexo(raio::CARTAO)
            .children(linhas)
            .when(!botoes.is_empty(), |s| s.child(sem_arrastar().flex().flex_wrap().gap(esp::E4).children(botoes)))
            .into_any_element()
    }

    /// À frente e atrás da cópia, lido em segundo plano (de novo a cada 15 s); até a primeira
    /// leitura, nada.
    fn estado_da_copia(
        &self,
        tarefa: u64,
        copia: &std::path::Path,
        origem: &std::path::Path,
        cx: &Context<Self>,
    ) -> Option<crate::git::EstadoCopia> {
        let mut lidos = self.estados_de_copia.borrow_mut();
        let atual = lidos.get(&tarefa).cloned();
        let vencido = atual.as_ref().is_none_or(|(quando, _, lendo)| !lendo && quando.elapsed() >= std::time::Duration::from_secs(15));
        if vencido {
            let anterior = atual.as_ref().and_then(|(_, e, _)| e.clone());
            lidos.insert(tarefa, (std::time::Instant::now(), anterior, true));
            let (copia, origem) = (copia.to_path_buf(), origem.to_path_buf());
            cx.spawn(async move |this, cx| {
                let estado = cx.background_spawn(async move { crate::git::estado_copia(&copia, &origem).ok() }).await;
                let _ = this.update(cx, |this, cx| {
                    this.estados_de_copia.borrow_mut().insert(tarefa, (std::time::Instant::now(), estado, false));
                    cx.notify();
                });
            })
            .detach();
        }
        atual.and_then(|(_, e, _)| e)
    }

    /// Resumo da equipe, as tarefas abertas de #1 a #N e as concluídas. Devolve a posição do
    /// cartão destacado entre os `filhos`, para o painel rolar até ele.
    #[allow(clippy::too_many_arguments)]
    fn lista_tarefas(
        &self,
        motor: &Motor,
        coordenador: &str,
        destacada: Option<u64>,
        solida: Option<u64>,
        filhos: &mut Vec<(Bloco, AnyElement)>,
        window: &Window,
        cx: &Context<Self>,
    ) -> Option<usize> {
        let mut tarefas: Vec<&Tarefa> = motor.tarefas_da_equipe(coordenador).collect();
        tarefas.sort_by_key(|t| t.id);
        if tarefas.is_empty() {
            filhos.push((
                Bloco::Caixa,
                EstadoVazio::new(Lucide::ListChecks, "Nada aberto aqui")
                    .texto("Os pedidos que esta conversa abriu viraram chats do projeto: estão na lista dele.")
                    .into_any_element(),
            ));
            return None;
        }
        let mut posicao = None;
        let (concluidas, abertas): (Vec<&Tarefa>, Vec<&Tarefa>) =
            tarefas.iter().copied().partition(|t| t.estado.encerrada());
        // O atalho para o chat dono só num ajudante: no chat de projeto, o dono é ele mesmo.
        let de_ajudante = destacada.is_some() && self.selecionado.as_deref() != Some(coordenador);
        filhos.push((Bloco::Resumo, self.resumo_tarefas(motor, coordenador, &tarefas, de_ajudante, window, cx)));
        let cartao = |tarefa: &Tarefa| {
            let destaque = destacada == Some(tarefa.id);
            self.cartao_tarefa(motor, tarefa, destaque, solida == Some(tarefa.id), window, cx)
        };
        for tarefa in &abertas {
            if destacada == Some(tarefa.id) {
                posicao = Some(filhos.len());
            }
            filhos.push((Bloco::Caixa, cartao(tarefa)));
        }
        if !concluidas.is_empty() {
            let chave = format!("equipe-concluidas-{coordenador}");
            let destacada_aqui = destacada.is_some_and(|d| concluidas.iter().any(|t| t.id == d));
            let cabecalho = CabecalhoDeSecao::new("Concluídas")
                .contagem(concluidas.len())
                .recuo(RECUO_SECAO)
                .respiro(4.);
            // Com tarefas abertas, as concluídas recolhem (e abrem deslizando); sem elas, ou com
            // a destacada entre as concluídas, ficam abertas, uma por linha do painel.
            if !abertas.is_empty() && !destacada_aqui {
                let aberta = self.expandidos.contains(&chave);
                let linhas = div()
                    .flex()
                    .flex_col()
                    .gap(esp::E2)
                    .pb(esp::E2)
                    .children(concluidas.iter().map(|t| self.linha_concluida(motor, t, window, cx)));
                let secao = div()
                    .flex()
                    .flex_col()
                    .child(cabecalho.recolhivel(aberta, self.alternar_chave(chave.clone(), cx)))
                    .child(Revelar::new(SharedString::from(chave), aberta, linhas));
                filhos.push((Bloco::Secao, secao.into_any_element()));
            } else {
                filhos.push((Bloco::Secao, cabecalho.into_any_element()));
                for tarefa in &concluidas {
                    if destacada == Some(tarefa.id) {
                        posicao = Some(filhos.len());
                        filhos.push((Bloco::Caixa, cartao(tarefa)));
                    } else {
                        filhos.push((Bloco::Linha, self.linha_concluida(motor, tarefa, window, cx)));
                    }
                }
            }
        }
        posicao
    }

    /// "Decisões": as notas que o coordenador guarda, recolhíveis (abertas de início). O
    /// marcador da lista cai na guia de 24.
    fn decisoes(&self, projeto: &str, notas: String, cx: &Context<Self>) -> AnyElement {
        let chave = format!("equipe-decisoes-{projeto}");
        let aberta = !self.expandidos.contains(&chave);
        let janela = cx.entity().downgrade();
        let alternar = {
            let chave = chave.clone();
            move |_: &mut Window, cx: &mut App| {
                let _ = janela.update(cx, |this, cx| {
                    if !this.expandidos.remove(&chave) {
                        this.expandidos.insert(chave.clone());
                    }
                    cx.notify();
                });
            }
        };
        let texto = div()
            .px(esp::E4)
            .max_w(r(720.))
            .corpo_ui()
            .text_color(Cores::texto())
            .child(TextView::markdown(SharedString::from(format!("notas-{projeto}")), notas));
        div()
            .flex()
            .flex_col()
            .child(
                CabecalhoDeSecao::new("Decisões")
                    .recolhivel(aberta, alternar)
                    .recuo(RECUO_SECAO)
                    .respiro(4.),
            )
            .child(Revelar::new(SharedString::from(chave), aberta, texto))
            .into_any_element()
    }

    /// Uma linha de 28: "Tarefas" com a contagem, a barra com a proporção de cada estado (cada
    /// pedaço diz quais na dica) e os atalhos da equipe, só com o ícone fora da largura larga.
    fn resumo_tarefas(
        &self,
        motor: &Motor,
        coordenador: &str,
        tarefas: &[&Tarefa],
        num_ajudante: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let mut conta = Contagem::default();
        for tarefa in tarefas {
            conta.somar(tarefa.id, &estado_visual(motor, tarefa));
        }
        let abertas = tarefas.iter().any(|t| !t.estado.encerrada());
        let com_rotulo = !self.direita_estreita && self.larguras.direita >= LARGA;
        let botao = |id: &'static str, icone: Lucide, rotulo: &'static str, dica: &'static str| {
            if com_rotulo {
                botao_fantasma(id, Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx).tooltip(dica)
            } else {
                botao_icone(id, icone, format!("{rotulo}\n{dica}"), TamanhoBotao::Barra, window, cx)
            }
        };
        let (c1, c2) = (coordenador.to_string(), coordenador.to_string());
        let acoes = sem_arrastar()
            .flex()
            .flex_none()
            .items_center()
            .gap(esp::E4)
            .when(num_ajudante, |s| {
                s.child(
                    botao("equipe-coordenador", Lucide::CornerLeftUp, "Chat", "Abrir ao lado o chat que criou este ajudante")
                        .on_click(cx.listener(move |this, _, window, cx| {
                            let perto = this.paineis.get(this.ativo).map(|p| (p.id, Lado::Esquerda));
                            this.abrir_ao_lado(c1.clone(), perto, window, cx)
                        })),
                )
            })
            .when(abertas, |s| {
                s.child(
                    botao(
                        "equipe-lado-a-lado",
                        Lucide::Columns3,
                        "Lado a lado",
                        "Este chat e os ajudantes em andamento, um painel para cada",
                    )
                    .on_click(cx.listener(move |this, _, window, cx| this.abrir_equipe(c2.clone(), window, cx))),
                )
            });
        // O texto na guia de 24 e os botões na coluna dos botões dos cartões.
        div()
            .h(altura::BARRA)
            .px(esp::E12)
            .flex()
            .items_center()
            .gap(esp::E8)
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .gap(esp::E6)
                    .child(div().rotulo_secao().whitespace_nowrap().child("Chats"))
                    .child(Selo::contagem(tarefas.len())),
            )
            .child(div().flex_1().min_w_0().px(esp::E4).child(BarraSegmentada::new(conta.partes()).fina()))
            .child(acoes)
            .into_any_element()
    }

    /// Cartão de uma tarefa: título, estado com a trilha de etapas, o motivo principal, os selos
    /// que sobram, os blocos abertos, as ações e, aberto, o detalhe. O ritmo é o da tinta: a 12
    /// da borda em cima (o título) e embaixo (o botão), e 8 entre as linhas; quem não tem fundo
    /// (o motivo, os botões sem o principal) sangra para dentro do vão, e o hover ganha a altura.
    fn cartao_tarefa(
        &self,
        motor: &Motor,
        tarefa: &Tarefa,
        destacada: bool,
        solida: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> AnyElement {
        let id = tarefa.id;
        let estreita = self.direita_estreita;
        let trabalhador = motor.chat(&tarefa.trabalhador).is_some();
        let execucao = motor.execucao(&tarefa.trabalhador);
        let ativa = tarefa.estado.ativa();
        let pedido = ativa && execucao == Execucao::EsperandoVoce;
        let erro = ativa && matches!(execucao, Execucao::Erro(_));
        let parada = motor.tarefa_parada(tarefa);
        let visual = estado_visual(motor, tarefa);
        let etapas = etapas(tarefa, motor.config.revisor, parada);
        let chave_detalhe = format!("equipe-detalhe-{id}");
        let detalhe = destacada || self.expandidos.contains(&chave_detalhe);
        let principal = principal(motor, tarefa);
        let motivo = motivo(tarefa);

        // Título e estado andam juntos (6): com ou sem perna (p, g), a tinta do título fica a 7–10
        // da pílula.
        let l1 = div()
            .flex()
            .items_center()
            .gap(esp::E8)
            .line_height(r(18.))
            .child(div().flex_1().min_w_0().child(rotulo_tarefa(id, &tarefa.titulo, true)))
            .children(self.painel_da_conversa(&tarefa.trabalhador).map(|(n, ativo)| numero_painel(n, ativo)));
        let pilula = {
            let dica: SharedString = match (&visual.detalhe, estreita) {
                (Some(d), true) => format!("{} · {d}\n{}", visual.rotulo, visual.dica).into(),
                _ => visual.dica.clone(),
            };
            let pilula = if estreita {
                PilulaDeEstado::new(visual.estado).rotulo(visual.rotulo.clone())
            } else {
                pilula_da_tarefa(motor, tarefa)
            };
            div()
                .id(SharedString::from(format!("pilula-tarefa-{id}")))
                .min_w_0()
                .overflow_hidden()
                .child(Realce {
                    id: SharedString::from(format!("realce-pilula-{id}")).into(),
                    chave: chave_do_estado(&visual),
                    cor: visual.estado.cor().unwrap_or_else(Cores::tenue),
                    filho: pilula.into_any_element(),
                })
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
        };
        let l2 = div()
            .flex()
            .items_center()
            .gap(esp::E8)
            .h(r(22.))
            .child(pilula)
            .children(super::painel_pr::chip_do_pr(("chip-pr-tarefa", id as usize), tarefa, super::componentes::TamanhoChip::Pequeno, estreita).map(|chip| {
                let chat = tarefa.trabalhador.clone();
                sem_arrastar().flex_none().child(
                    chip.on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.abrir_aba_pr(&chat, window, cx)
                    }))
                    .construir(window, cx),
                )
            }))
            .child(div().flex_1())
            .when(!matches!(tarefa.estado, EstadoTarefa::Falhou | EstadoTarefa::Cancelada), |s| {
                s.child(self.trilha_etapas(tarefa, &etapas, cx))
            });
        // Só o motivo que abre um bloco segura o clique; o resto abre o trabalhador, como o cartão.
        let l3 = motivo.map(|m| {
            let linha = self.linha_motivo(tarefa, m, window, cx);
            if matches!(m, Motivo::Preparacao | Motivo::Verificacao | Motivo::Revisor) {
                sem_arrastar().child(linha).into_any_element()
            } else {
                linha
            }
        });
        let l4 = self.selos_tarefa(tarefa, motivo, destacada, cx);
        let blocos = self.blocos_abertos(tarefa, window, cx);

        let fundo = if pedido {
            Cores::cartao_elevado().blend(Cores::tinta(Cores::ambar()))
        } else {
            Cores::cartao_elevado()
        };
        let borda = if destacada {
            Cores::destaque_borda()
        } else if pedido {
            Cores::tinta_borda(Cores::ambar())
        } else if erro {
            Cores::tinta_borda(Cores::vermelho())
        } else {
            Cores::linha()
        };
        let repouso = !destacada && !pedido && !erro;
        let clicavel = !destacada && trabalhador;
        let acoes = self.acoes_tarefa(motor, tarefa, destacada, principal, solida, erro, detalhe, window, cx);
        // O detalhe é montado fechado também, para deslizar ao fechar; o vão (12 dos dois lados
        // da linha que o separa) vem dentro dele.
        let aberto = Revelar::new(
            SharedString::from(chave_detalhe.clone()),
            detalhe,
            sem_arrastar().pt(esp::E12).child(self.detalhe_tarefa(motor, tarefa)),
        );

        div()
            .id(SharedString::from(format!("tarefa-{id}")))
            .relative()
            .flex()
            .flex_col()
            .gap(esp::E8)
            // O título 18 tem 4 de entrelinha em cima: a tinta dele fica a 12 da borda, como a base.
            .pt(dentro_da_borda(8.))
            .pb(dentro_da_borda(12.))
            .px(dentro_da_borda(12.))
            .rounded(raio::CARTAO)
            .bg(fundo)
            .border_1()
            .border_color(borda)
            .reflexo(raio::CARTAO)
            .when(destacada, |s| {
                s.child(
                    div()
                        .absolute()
                        .left(r(2.))
                        .top(r(14.))
                        .bottom(r(14.))
                        .w(px(3.))
                        .rounded_full()
                        .bg(Cores::destaque()),
                )
            })
            .child(div().flex().flex_col().gap(esp::E6).child(l1).child(l2))
            .children(l3)
            .children(l4)
            .when(!blocos.is_empty(), |s| s.child(sem_arrastar().flex().flex_col().gap(esp::E8).children(blocos)))
            .child(div().flex().flex_col().child(acoes).child(aberto))
            // O cartão inteiro abre o trabalhador ao lado (também com Enter); Espaço abre o detalhe.
            .when(clicavel, |s| {
                let trabalhador = tarefa.trabalhador.clone();
                let coordenador = tarefa.coordenador.clone();
                let chave = chave_detalhe.clone();
                s.cursor_pointer()
                    .tab_index(0)
                    .when(repouso, |s| s.hover(|s| s.border_color(Cores::borda_forte())))
                    .focus_visible(|s| s.border_color(Cores::destaque()).shadow(halo_foco()))
                    .on_drag(
                        ArrastoChat {
                            chat: trabalhador.clone(),
                            titulo: format!("#{id} {}", tarefa.titulo).into(),
                            de_painel: None,
                        },
                        |arrasto, _, _, cx| {
                            cx.new(|_| PreviaArrasto {
                                titulo: arrasto.titulo.clone(),
                                simbolo: Simbolo::Papel(super::componentes::Papel::Trabalhador),
                            })
                        },
                    )
                    .on_click(cx.listener(move |this, evento: &ClickEvent, window, cx| {
                        let espaco = matches!(
                            evento,
                            ClickEvent::Keyboard(KeyboardClickEvent { button: KeyboardButton::Space, .. })
                        );
                        if espaco {
                            if !this.expandidos.remove(&chave) {
                                this.expandidos.insert(chave.clone());
                            }
                            cx.notify();
                        } else {
                            this.abrir_trabalhador(&coordenador, trabalhador.clone(), window, cx)
                        }
                    }))
            })
            .into_any_element()
    }

    /// A linha de ações: a principal, até dois botões fantasma, o "⋯" da cópia e o chevron do
    /// detalhe, numa linha só. No painel estreito (ou apertado), os fantasmas ficam só com o ícone.
    #[allow(clippy::too_many_arguments)]
    fn acoes_tarefa(
        &self,
        motor: &Motor,
        tarefa: &Tarefa,
        destacada: bool,
        principal: Option<Principal>,
        solida: bool,
        erro: bool,
        detalhe: bool,
        window: &Window,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let id = tarefa.id;
        let estreita = self.direita_estreita;
        let trabalhador = motor.chat(&tarefa.trabalhador).is_some();
        let copia = tarefa.worktree.is_some() && tarefa.preparada;
        let pode_apagar = motor.pode_apagar_copia(id);
        let aberto = self.aberta_em_painel(&tarefa.trabalhador);
        let rodar = rodar_do(motor, tarefa).filter(|_| destacada && copia);
        let porta = crate::motor::porta_de(tarefa);
        let (trab, coord) = (tarefa.trabalhador.clone(), tarefa.coordenador.clone());
        let abrir = move |foco: bool| {
            let (trab, coord) = (trab.clone(), coord.clone());
            cx.listener(move |this: &mut Janela, _: &ClickEvent, window, cx| {
                this.abrir_trabalhador(&coord, trab.clone(), window, cx);
                // "Ver pedido" deixa o cursor na caixa do trabalhador, como na lista.
                if foco && let Some(composer) = this.composer_ativo() {
                    let foco = composer.read(cx).focus_handle(cx);
                    window.focus(&foco, cx);
                }
            })
        };

        let botao_principal = principal.map(|p| {
            let botao = |chave: &str, icone: Lucide, rotulo: &'static str| {
                botao_primario(
                    SharedString::from(format!("{chave}-{id}")),
                    Some(icone.into()),
                    rotulo,
                    TamanhoBotao::Barra,
                    window,
                    cx,
                )
                .flex_none()
                .when(!solida, |b| b.custom(variante_tinta(cx)))
            };
            match p {
                Principal::VerPedido => botao("ver-pedido", Lucide::ArrowRight, "Ver pedido")
                    .tooltip("Este chat espera a sua permissão")
                    .on_click(abrir(true)),
                Principal::Retomar => {
                    let trabalhador = tarefa.trabalhador.clone();
                    botao("retomar", Lucide::Play, "Retomar").on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(m) = &this.motor {
                            m.update(cx, |m, cx| m.retomar(&trabalhador, cx));
                        }
                    }))
                }
                Principal::Revisar => botao("revisar", Lucide::Eye, "Revisar")
                    .on_click(cx.listener(move |this, _, window, cx| this.dialogo_diff(id, window, cx))),
            }
        });
        // Os fantasmas: o id, o ícone, o rótulo, a dica, se é de perigo e o clique.
        type Clique = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
        let mut fantasmas: Vec<(&'static str, Lucide, &'static str, String, bool, Clique)> = Vec::new();
        if !destacada && trabalhador {
            let (chave, icone, rotulo, dica) = if erro {
                let dica = "O processo deste chat caiu: abre a conversa dele ao lado";
                ("ver-erro", Lucide::CircleX, "Ver erro", dica)
            } else if aberto {
                ("ir-ao-chat", Lucide::MessageSquare, "Ir à conversa", "A conversa já está aberta num painel")
            } else {
                (
                    "abrir",
                    Lucide::Columns2,
                    "Abrir ao lado",
                    "Abre ao lado (ou arraste o cartão para um painel)",
                )
            };
            fantasmas.push((chave, icone, rotulo, dica.into(), erro, Box::new(abrir(false))));
        }
        // O Revisar já abre as mudanças; com outra principal, elas vão para o "⋯" (menos no destacado).
        let mudancas = copia && principal != Some(Principal::Revisar);
        let mudancas_no_menu = mudancas && principal.is_some() && !destacada;
        if mudancas && !mudancas_no_menu {
            fantasmas.push((
                "diff",
                Lucide::FileDiff,
                "Mudanças",
                "O que mudou na cópia".into(),
                false,
                Box::new(cx.listener(move |this, _: &ClickEvent, window, cx| this.dialogo_diff(id, window, cx))),
            ));
        }
        if let Some((comando, pasta)) = rodar.zip(tarefa.worktree.clone()) {
            fantasmas.push((
                "rodar",
                Lucide::MonitorPlay,
                "Rodar",
                format!("Rodar “{comando}” na cópia"),
                false,
                Box::new(move |_: &ClickEvent, _: &mut Window, _: &mut App| {
                    crate::sistema::abrir_terminal(&pasta, Some(porta), Some(&comando))
                }),
            ));
        }
        // Com a principal e dois fantasmas, os rótulos não cabem a 380 px: ficam só os ícones.
        let com_rotulo = !estreita && !(principal.is_some() && fantasmas.len() >= 2);
        let fantasmas = fantasmas.into_iter().map(|(chave, icone, rotulo, dica, perigo, clique)| {
            let id = SharedString::from(format!("{chave}-{id}"));
            let botao = if com_rotulo {
                let botao = if perigo {
                    botao_perigo(id, Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx)
                } else {
                    botao_fantasma(id, Some(icone.into()), rotulo, TamanhoBotao::Barra, window, cx)
                };
                botao.min_w_0().tooltip(dica)
            } else {
                let botao = botao_icone(id, icone, format!("{rotulo}\n{dica}"), TamanhoBotao::Barra, window, cx);
                if perigo { botao.custom(variante_perigo(cx)) } else { botao }
            };
            botao.on_click(clique)
        });
        let menu = (copia || pode_apagar || (!destacada && trabalhador)).then(|| {
            let botao = botao_icone(
                SharedString::from(format!("menu-copia-t{id}")),
                Lucide::Ellipsis,
                "Mais opções",
                TamanhoBotao::Barra,
                window,
                cx,
            )
            .flex_none();
            if copia || pode_apagar {
                self.menu_copia(id, mudancas_no_menu, botao, cx)
                    .into_any_element()
            } else {
                self.menu_abrir_aqui(tarefa, botao, cx).into_any_element()
            }
        });
        let chave_detalhe = format!("equipe-detalhe-{id}");
        // Sem o principal, nenhum botão tem fundo: a fileira sangra 4 para a tinta ficar no ritmo.
        let sangra = botao_principal.is_none();
        sem_arrastar()
            .flex()
            .items_center()
            .gap(esp::E4)
            .when(sangra, |s| s.my(r(-4.)))
            .children(botao_principal)
            .children(fantasmas)
            .child(div().flex_1())
            .children(menu)
            .when(!destacada, |s| {
                s.child(
                    botao_icone(
                        SharedString::from(format!("detalhe-{id}")),
                        if detalhe { Lucide::ChevronUp } else { Lucide::ChevronDown },
                        if detalhe {
                            "Esconder o objetivo e os critérios"
                        } else {
                            "Mostrar o objetivo e os critérios"
                        },
                        TamanhoBotao::Barra,
                        window,
                        cx,
                    )
                    .flex_none()
                    .on_click(self.alternar(chave_detalhe, cx)),
                )
            })
    }

    /// Pontos das etapas ligados por traços, com o detalhe de cada uma na dica. Os da verificação
    /// e do revisor abrem a saída e o parecer.
    fn trilha_etapas(&self, tarefa: &Tarefa, etapas: &[Etapa], cx: &Context<Self>) -> impl IntoElement + use<> {
        let id = tarefa.id;
        let mut pontos = sem_arrastar().flex().flex_none().items_center();
        for (i, etapa) in etapas.iter().enumerate() {
            if i > 0 {
                let feita = etapas[i - 1].passo == Passo::Ok;
                pontos = pontos.child(
                    div()
                        .w(r(8.))
                        .h(px(2.))
                        .rounded_full()
                        .bg(if feita { Cores::verde().opacity(0.45) } else { Cores::borda_forte() }),
                );
            }
            let ponto = div().size(r(8.)).rounded_full();
            let ponto = match etapa.passo {
                Passo::Pendente => ponto.border_1().border_color(Cores::fantasma()),
                Passo::Atual => ponto.bg(Cores::destaque()).shadow(vec![
                    BoxShadow::new(px(0.), px(0.), Cores::destaque().opacity(0.35)).spread_radius(px(2.)),
                ]),
                Passo::Ok => ponto.bg(Cores::verde()),
                Passo::Aviso => ponto.bg(Cores::ambar()),
                Passo::Falha => ponto.bg(Cores::vermelho()),
                Passo::Parado => ponto.bg(Cores::tenue()),
            };
            let alvo = match etapa.nome {
                "Verificação" if tarefa.ultima_verificacao.is_some() => Some(format!("equipe-verif-{id}")),
                "Revisor" if tarefa.revisao.is_some() => Some(format!("equipe-rev-{id}")),
                _ => None,
            };
            let dica = match &alvo {
                Some(_) => format!("{}: {}\nClique para ver o resultado", etapa.nome, etapa.detalhe),
                None => format!("{}: {}", etapa.nome, etapa.detalhe),
            };
            pontos = pontos.child(
                div()
                    .id(SharedString::from(format!("etapa-{id}-{i}")))
                    .p(esp::E2)
                    .rounded_full()
                    .child(ponto)
                    .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
                    .when_some(alvo, |s, chave| {
                        s.cursor_pointer().hover(|s| s.bg(Cores::hover())).on_click(self.alternar(chave, cx))
                    }),
            );
        }
        pontos
    }

    /// O motivo principal numa linha curta: o ícone na cor, o texto e o tempo ("Verificação
    /// falhou · 19s"); o comando vai para a dica e para o bloco que a linha abre (a saída ou o
    /// parecer). A linha tem 20 e sangra 4 para cima e para baixo: a tinta fica a 8 das vizinhas
    /// e o hover ganha a altura toda.
    fn linha_motivo(&self, tarefa: &Tarefa, motivo: Motivo, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = tarefa.id;
        let estreita = self.direita_estreita;
        let saida = |v: Option<&Verificacao>| v.map(|v| (v.comando.clone(), v.tempo()));
        let (icone, cor, texto, comando, chave, dica): (Lucide, Hsla, String, _, _, String) = match motivo {
            Motivo::Progresso => {
                return div()
                    .my(r(-2.))
                    .metadado()
                    .text_color(Cores::tenue())
                    .line_clamp(2)
                    .text_ellipsis()
                    .child(tarefa.progresso.clone().unwrap_or_default())
                    .into_any_element();
            }
            Motivo::Preparacao => (
                Lucide::CircleX,
                Cores::vermelho(),
                "Preparação falhou".into(),
                saida(tarefa.preparacao.as_ref()),
                Some(format!("equipe-prep-{id}")),
                "Clique para ver a saída da preparação".into(),
            ),
            Motivo::Verificacao => (
                Lucide::CircleX,
                Cores::vermelho(),
                "Verificação falhou".into(),
                saida(tarefa.ultima_verificacao.as_ref()),
                Some(format!("equipe-verif-{id}")),
                "Clique para ver a saída da verificação".into(),
            ),
            Motivo::Revisor => {
                let n = tarefa.revisao.as_ref().map(importantes).unwrap_or(0);
                let texto = if n == 0 {
                    "Revisor pede ajustes".to_string()
                } else {
                    format!("Revisor pede ajustes · {}", contagem(n, "importante", "importantes"))
                };
                (
                    Lucide::Eye,
                    Cores::ambar(),
                    texto,
                    None,
                    Some(format!("equipe-rev-{id}")),
                    "Clique para ver o parecer do revisor".into(),
                )
            }
            Motivo::Sobreposicao => (
                Lucide::GitMerge,
                Cores::ambar(),
                format!("Mesmos arquivos que {}", numeros_sobrepostos(&tarefa.sobreposicao)),
                None,
                None,
                tarefa.sobreposicao.join("\n"),
            ),
        };
        let (comando, tempo) = match comando {
            Some((c, t)) => (Some(c), t),
            None => (None, None),
        };
        let dica = match (&comando, &tempo, estreita) {
            (Some(c), Some(t), true) => format!("“{c}” levou {t}\n{dica}"),
            (Some(c), _, _) => format!("“{c}”\n{dica}"),
            _ => dica,
        };
        let aberto = chave.as_ref().is_some_and(|k| self.expandidos.contains(k));
        let grupo = SharedString::from(format!("motivo-{id}"));
        div()
            .id(grupo.clone())
            .group(grupo.clone())
            .flex()
            .items_center()
            .h(r(20.))
            .my(r(-4.))
            .gap(esp::E6)
            .px(esp::E6)
            .mx(r(-6.))
            .rounded(raio::CONTROLE)
            .child(div().flex_none().text_color(cor).child(icone_em(icone, 14., window)))
            .child(
                div()
                    .min_w_0()
                    .truncate()
                    .metadado()
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(Cores::texto())
                    .child(texto),
            )
            .children(tempo.filter(|_| !estreita).map(|t| {
                div()
                    .flex_none()
                    .metadado()
                    .text_color(Cores::apagado())
                    .group_hover(grupo.clone(), |s| s.text_color(Cores::tenue()))
                    .child(format!("· {t}"))
            }))
            .child(div().flex_1())
            .when(chave.is_some(), |s| {
                s.child(
                    div()
                        .flex_none()
                        .text_color(Cores::tenue())
                        .child(icone_em(if aberto { Lucide::ChevronDown } else { Lucide::ChevronRight }, 14., window)),
                )
            })
            .when(aberto, |s| s.bg(Cores::pressionado()))
            .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
            .when_some(chave, |s, chave| {
                s.cursor_pointer()
                    .when(!aberto, |s| s.hover(|s| s.bg(Cores::hover())))
                    .on_click(self.alternar(chave, cx))
            })
            .into_any_element()
    }

    /// Selos do que não virou motivo: ressalva, mesmos arquivos de outra tarefa e "sem cópia".
    fn selos_tarefa(
        &self,
        tarefa: &Tarefa,
        motivo: Option<Motivo>,
        destacada: bool,
        cx: &Context<Self>,
    ) -> Option<AnyElement> {
        let id = tarefa.id;
        let aberta = !tarefa.estado.encerrada();
        let mut selos: Vec<AnyElement> = Vec::new();
        if let Some(ressalva) = &tarefa.ressalva {
            let selo = selo_com_dica(
                format!("selo-ressalva-{id}"),
                Selo::cor("ressalva", Cores::ambar()).icone(Lucide::TriangleAlert, Cores::ambar()),
                format!("Aprovada com ressalva: {ressalva}"),
            );
            selos.push(if destacada {
                selo.into_any_element()
            } else {
                selo.cursor_pointer()
                    .on_click(self.alternar(format!("equipe-detalhe-{id}"), cx))
                    .into_any_element()
            });
        }
        if aberta && !tarefa.sobreposicao.is_empty() && motivo != Some(Motivo::Sobreposicao) {
            selos.push(
                selo_com_dica(
                    format!("selo-sobre-{id}"),
                    Selo::cor(numeros_sobrepostos(&tarefa.sobreposicao), Cores::ambar())
                        .icone(Lucide::GitMerge, Cores::ambar()),
                    format!("Mesmos arquivos que outros chats:\n{}", tarefa.sobreposicao.join("\n")),
                )
                .into_any_element(),
            );
        }
        // Sem a cópia pronta (B4), a tarefa está sendo preparada ou espera a vez de preparar.
        let preparando = !tarefa.preparada || tarefa.progresso.as_deref().is_some_and(|p| p.starts_with("preparando"));
        if tarefa.worktree.is_none() && aberta && !preparando {
            selos.push(
                selo_com_dica(
                    format!("selo-sem-copia-{id}"),
                    Selo::neutro("sem cópia"),
                    "Projeto sem git: o chat mexe direto na pasta".into(),
                )
                .into_any_element(),
            );
        }
        (!selos.is_empty()).then(|| {
            sem_arrastar()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(esp::E6)
                .children(selos)
                .into_any_element()
        })
    }

    /// A saída da preparação e da verificação e o parecer do revisor, quando abertos.
    fn blocos_abertos(&self, tarefa: &Tarefa, window: &Window, cx: &Context<Self>) -> Vec<AnyElement> {
        let id = tarefa.id;
        let mut blocos = Vec::new();
        let (k_prep, k_verif, k_rev) =
            (format!("equipe-prep-{id}"), format!("equipe-verif-{id}"), format!("equipe-rev-{id}"));
        if self.expandidos.contains(&k_prep)
            && let Some(p) = &tarefa.preparacao
        {
            blocos.push(bloco_saida(&k_prep, p));
        }
        if self.expandidos.contains(&k_verif)
            && let Some(v) = &tarefa.ultima_verificacao
        {
            blocos.push(bloco_saida(&k_verif, v));
        }
        if self.expandidos.contains(&k_rev)
            && let Some(r) = &tarefa.revisao
        {
            blocos.push(self.bloco_revisor(tarefa, r, window, cx));
        }
        blocos
    }

    /// Parecer do revisor: o resumo e os problemas; cada problema abre o Revisar na linha.
    fn bloco_revisor(&self, tarefa: &Tarefa, revisao: &Revisao, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = tarefa.id;
        let tem_diff = tarefa.worktree.is_some();
        let pilula = if revisao.veredito == "aprovar" {
            PilulaDeEstado::new(Estado::Pronto).rotulo("aprovaria")
        } else {
            PilulaDeEstado::new(Estado::EmRevisao).rotulo("pede ajustes")
        };
        div()
            .flex()
            .flex_col()
            .gap(esp::E8)
            .p(esp::E12)
            .rounded(raio::CONTROLE)
            .bg(Cores::recuo())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(esp::E6)
                    .child(div().text_color(Cores::tenue()).child(icone_em(Lucide::Eye, 14., window)))
                    .child(div().rotulo_secao().child("Revisor"))
                    .child(pilula),
            )
            .when(!revisao.resumo.trim().is_empty(), |s| {
                s.child(
                    div()
                        .text_size(r(13.))
                        .line_height(r(18.))
                        .text_color(Cores::texto())
                        .line_clamp(4)
                        .text_ellipsis()
                        .child(revisao.resumo.trim().to_string()),
                )
            })
            .when(revisao.problemas.is_empty(), |s| {
                s.child(div().metadado().text_color(Cores::tenue()).child("Nenhum problema apontado."))
            })
            .when(!revisao.problemas.is_empty(), |s| s.child(div().h(px(1.)).bg(Cores::borda())))
            .children(revisao.problemas.iter().enumerate().map(|(i, p)| {
                let onde = match p.linha {
                    Some(l) => format!("{}:{l}", p.arquivo),
                    None => p.arquivo.clone(),
                };
                let (arquivo, linha) = (p.arquivo.clone(), p.linha);
                div()
                    .id(SharedString::from(format!("problema-{id}-{i}")))
                    .flex()
                    .items_start()
                    .gap(esp::E8)
                    .min_h(r(40.))
                    .py(esp::E6)
                    .px(esp::E6)
                    .mx(r(-6.))
                    .rounded(raio::CONTROLE)
                    // O selo no meio da primeira linha (a do arquivo, de 16).
                    .child(div().flex_none().h(r(16.)).flex().items_center().child(selo_gravidade(&p.gravidade)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .flex_1()
                            .min_w_0()
                            .gap(esp::E2)
                            .child(
                                div()
                                    .overflow_hidden()
                                    .whitespace_nowrap()
                                    .text_ellipsis_start()
                                    .mono_pequeno()
                                    .text_color(Cores::codigo())
                                    .child(onde),
                            )
                            .child(div().metadado().text_color(Cores::tenue()).child(p.descricao.clone())),
                    )
                    .when(tem_diff, |s| {
                        s.child(div().flex_none().text_color(Cores::tenue()).child(icone_em(Lucide::ChevronRight, 14., window)))
                            .cursor_pointer()
                            .hover(|s| s.bg(Cores::hover()))
                            .tooltip(|window, cx| Tooltip::new("Abrir no Revisar, nesta linha").build(window, cx))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.dialogo_diff_em(id, Some((arquivo.clone(), linha)), window, cx)
                            }))
                    })
            }))
            .into_any_element()
    }

    /// Objetivo, critérios, comando de verificação, a cópia isolada (pasta e branch) e a
    /// ressalva da aprovação.
    fn detalhe_tarefa(&self, motor: &Motor, tarefa: &Tarefa) -> impl IntoElement + use<> {
        let id = tarefa.id;
        let largura_rotulo = if self.direita_estreita { 56. } else { 64. };
        let secao = |titulo: &'static str| div().rotulo_secao().child(titulo);
        let markdown = |chave: String, texto: String| {
            div()
                .corpo_ui()
                .text_color(Cores::texto())
                .child(TextView::markdown(SharedString::from(chave), texto))
        };
        let criterios: String = tarefa.criterios.iter().map(|c| format!("- {c}\n")).collect();
        div()
            .flex()
            .flex_col()
            .gap(esp::E12)
            .max_w(r(720.))
            .child(div().h(px(1.)).bg(Cores::linha()))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(esp::E4)
                    .child(secao("Objetivo"))
                    .child(markdown(format!("objetivo-{id}"), tarefa.objetivo.clone())),
            )
            .when(!criterios.is_empty(), |s| {
                s.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(esp::E4)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(esp::E6)
                                .child(secao("Critérios de aceitação"))
                                .child(Selo::contagem(tarefa.criterios.len())),
                        )
                        .child(markdown(format!("criterios-{id}"), criterios)),
                )
            })
            .when_some(tarefa.verificacao.clone(), |s, v| {
                s.child(
                    div().flex().flex_col().gap(esp::E4).child(secao("Verificação")).child(
                        div()
                            .flex()
                            .items_center()
                            .gap(esp::E8)
                            .h(r(32.))
                            .pl(esp::E12)
                            .pr(esp::E4)
                            .rounded(raio::CONTROLE)
                            .bg(Cores::preto())
                            .child(div().flex_1().min_w_0().truncate().mono_pequeno().text_color(Cores::codigo()).child(v.clone()))
                            .child(copiar(format!("copiar-verif-{id}"), v)),
                    ),
                )
            })
            .when_some(tarefa.worktree.clone(), |s, w| {
                let relativo = w
                    .strip_prefix(&motor.dados.pasta)
                    .map(|p| p.display().to_string())
                    .unwrap_or_else(|_| super::dialogos::com_til(&w));
                let absoluto = w.display().to_string();
                s.child(
                    div()
                        .flex()
                        .flex_col()
                        .child(secao("Cópia isolada"))
                        .child(linha_dado(
                            format!("copia-{id}"),
                            "Pasta",
                            relativo,
                            absoluto.clone(),
                            absoluto,
                            largura_rotulo,
                        ))
                        .when_some(tarefa.branch.clone(), |s, b| {
                            s.child(linha_dado(format!("branch-{id}"), "Branch", b.clone(), b.clone(), b, largura_rotulo))
                        }),
                )
            })
            .when_some(tarefa.ressalva.clone(), |s, ressalva| {
                s.child(
                    div().flex().flex_col().gap(esp::E4).child(secao("Aprovada com ressalva")).child(
                        div()
                            .px(esp::E12)
                            .py(esp::E8)
                            .rounded(raio::CONTROLE)
                            .bg(Cores::tinta(Cores::ambar()))
                            .text_size(r(13.))
                            .line_height(r(18.))
                            .text_color(Cores::texto())
                            .child(ressalva),
                    ),
                )
            })
    }

    /// Linha de uma tarefa encerrada (32): o ícone do fim, "#n título", a ressalva se houver e,
    /// no hover, o "⋯".
    fn linha_concluida(&self, motor: &Motor, tarefa: &Tarefa, window: &Window, cx: &Context<Self>) -> AnyElement {
        let id = tarefa.id;
        let (icone, cor) = match tarefa.estado {
            EstadoTarefa::Aprovada => (Lucide::CircleCheck, Cores::verde()),
            EstadoTarefa::Cancelada => (Lucide::Ban, Cores::tenue()),
            _ => (Lucide::CircleX, Cores::vermelho()),
        };
        let numero = format!("#{id}");
        let estilo = HighlightStyle { color: Some(Cores::tenue()), ..Default::default() };
        let titulo = StyledText::new(format!("{numero} {}", tarefa.titulo)).with_highlights([(0..numero.len(), estilo)]);
        let mut cartao = CartaoDeLista::new(SharedString::from(format!("concluida-{id}")), tarefa.titulo.clone())
            .titulo_rico(titulo)
            .uma_linha()
            .simbolo(Simbolo::icone_cor(icone, cor));
        if let Some(ressalva) = &tarefa.ressalva {
            cartao = cartao.meta(selo_com_dica(
                format!("ressalva-concluida-{id}"),
                Selo::cor("ressalva", Cores::ambar()).icone(Lucide::TriangleAlert, Cores::ambar()),
                format!("Aprovada com ressalva: {ressalva}"),
            ));
        }
        let pode_apagar = motor.pode_apagar_copia(id);
        let trabalhador = motor.chat(&tarefa.trabalhador).is_some();
        if pode_apagar || trabalhador {
            let botao = botao_icone(
                SharedString::from(format!("menu-concluida-{id}")),
                Lucide::Ellipsis,
                "Mais opções",
                TamanhoBotao::Linha,
                window,
                cx,
            );
            let menu = if pode_apagar || tarefa.worktree.is_some() {
                self.menu_copia(id, false, botao, cx).into_any_element()
            } else {
                self.menu_abrir_aqui(tarefa, botao, cx).into_any_element()
            };
            cartao = cartao.acao(sem_arrastar().child(menu));
        }
        if trabalhador {
            let (trab, coord) = (tarefa.trabalhador.clone(), tarefa.coordenador.clone());
            cartao = cartao.on_click(cx.listener(move |this, _, window, cx| {
                this.abrir_trabalhador(&coord, trab.clone(), window, cx)
            }));
        }
        // O ícone na guia de 24, como o texto dos cartões.
        div().px(esp::E4).child(cartao).into_any_element()
    }

    /// O "⋯" da cópia de uma tarefa, sobre o `botao` dado: os itens de `menus.rs` e, quando a
    /// ação principal do cartão tomou o lugar das Mudanças, "Ver mudanças" em cima.
    fn menu_copia(
        &self,
        id: u64,
        ver_mudancas: bool,
        botao: Button,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let janela = cx.entity().downgrade();
        let motor = self.motor.clone();
        botao.dropdown_menu_with_anchor(Anchor::TopRight, move |menu, window, cx| {
            let Some(motor) = &motor else {
                return menu;
            };
            let menu = if ver_mudancas {
                let janela = janela.clone();
                menu.item(PopupMenuItem::new("Ver mudanças").icon(Lucide::FileDiff).on_click(move |_, window, cx| {
                    let _ = janela.update(cx, |this, cx| this.dialogo_diff(id, window, cx));
                }))
                .separator()
            } else {
                menu
            };
            menus::itens_da_copia(menu, janela.clone(), motor, id, false, window, cx)
        })
    }

    /// O "⋯" de uma tarefa sem cópia: só abrir o trabalhador neste painel.
    fn menu_abrir_aqui(&self, tarefa: &Tarefa, botao: Button, cx: &Context<Self>) -> impl IntoElement + use<> {
        let janela = cx.entity().downgrade();
        let trabalhador = tarefa.trabalhador.clone();
        botao.dropdown_menu_with_anchor(Anchor::TopRight, move |menu: PopupMenu, _, _| {
            let (janela, trabalhador) = (janela.clone(), trabalhador.clone());
            menu.item(PopupMenuItem::new("Abrir neste painel").icon(Lucide::Replace).on_click(
                move |_, window, cx| {
                    let _ = janela.update(cx, |this, cx| this.selecionar(Some(trabalhador.clone()), window, cx));
                },
            ))
        })
    }

    /// Abre ou fecha um bloco do painel (guardado em `expandidos`).
    fn alternar(&self, chave: String, cx: &Context<Self>) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static {
        let alternar = self.alternar_chave(chave, cx);
        move |_, window, cx| alternar(window, cx)
    }

    fn alternar_chave(&self, chave: String, cx: &Context<Self>) -> impl Fn(&mut Window, &mut App) + 'static {
        let janela = cx.entity().downgrade();
        move |_, cx| {
            let _ = janela.update(cx, |this, cx| {
                if !this.expandidos.remove(&chave) {
                    this.expandidos.insert(chave.clone());
                }
                cx.notify();
            });
        }
    }

    /// Abre o trabalhador à direita do último painel da equipe. Com os painéis cheios, troca o
    /// trabalhador de outra tarefa da mesma equipe em vez de recusar.
    fn abrir_trabalhador(
        &mut self,
        coordenador: &str,
        trabalhador: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let (ultimo, trocar) = {
            let motor = motor.read(cx);
            let colega = |chat: &str| {
                motor
                    .chat(chat)
                    .is_some_and(|c| c.papel == Papel::Trabalhador && c.pai.as_deref() == Some(coordenador))
            };
            let ultimo = self
                .paineis
                .iter()
                .rposition(|p| p.chat.as_deref().is_some_and(|c| c == coordenador || colega(c)));
            let trocar = self
                .paineis
                .iter()
                .rposition(|p| p.chat.as_deref().is_some_and(&colega));
            (ultimo, trocar)
        };
        let aberto = self
            .paineis
            .iter()
            .any(|p| p.chat.as_deref() == Some(trabalhador.as_str()));
        // Sem lugar para outro painel (pela largura, pelo máximo ou porque a Equipe viraria
        // gaveta), troca o de outra tarefa.
        let cheio = !self.cabe_outro_painel()
            || (self.direita_visivel()
                && empurraria_a_direita(self.paineis.len(), super::util_a_100(window, self.lista), self.lista));
        if !aberto
            && cheio
            && let Some(ix) = trocar
        {
            self.ativo = ix;
            self.selecionar(Some(trabalhador), window, cx);
            return;
        }
        let perto = ultimo.map(|ix| (self.paineis[ix].id, Lado::Direita));
        self.abrir_ao_lado(trabalhador, perto, window, cx);
    }

    /// Coordenador e tarefas em andamento lado a lado, quantos couberem na janela (quem pede
    /// permissão vem primeiro). Os painéis de fora da equipe fecham, com o rascunho guardado.
    fn abrir_equipe(&mut self, coordenador: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let mut fila: Vec<(u8, u64, String)> = {
            let motor = motor.read(cx);
            motor
                .tarefas_da_equipe(&coordenador)
                .filter(|t| !t.estado.encerrada() && t.trabalhador != coordenador && motor.chat(&t.trabalhador).is_some())
                .map(|t| {
                    let ordem = match motor.execucao(&t.trabalhador) {
                        Execucao::EsperandoVoce => 0,
                        _ if t.estado.ativa() => 1,
                        _ => 2,
                    };
                    (ordem, t.id, t.trabalhador.clone())
                })
                .collect()
        };
        fila.sort();
        let largura = super::util_a_100(window, self.lista);
        let mut cabem = quantos_cabem(largura, self.lista, true);
        if cabem < 2 && self.lista && quantos_cabem(largura, false, true) >= 2 {
            self.lista = false;
            cabem = quantos_cabem(largura, false, true);
        }
        if cabem < 2 {
            window.push_notification(
                "A janela está estreita para conversas lado a lado. Maximize e tente de novo.",
                cx,
            );
            return;
        }
        let total = fila.len();
        let mut alvo = vec![coordenador.clone()];
        alvo.extend(
            fila.into_iter()
                .take(cabem.min(MAX_PAINEIS) - 1)
                .map(|(_, _, chat)| chat),
        );
        let fora: Vec<u64> = self
            .paineis
            .iter()
            .filter(|p| !p.chat.as_ref().is_some_and(|c| alvo.contains(c)))
            .map(|p| p.id)
            .collect();
        for id in fora {
            self.fechar_painel(id, cx);
        }
        for chat in &alvo {
            if !self.paineis.iter().any(|p| p.chat.as_ref() == Some(chat)) {
                let painel = self.criar_painel(Some(chat.clone()), window, cx);
                self.paineis.push(painel);
            }
        }
        self.paineis
            .sort_by_key(|p| p.chat.as_ref().and_then(|c| alvo.iter().position(|a| a == c)));
        self.ativo = 0;
        self.selecionado = Some(coordenador);
        let mostradas = alvo.len() - 1;
        if total > mostradas {
            let motivo = if cabem >= MAX_PAINEIS {
                format!("cabem até {MAX_PAINEIS} conversas lado a lado")
            } else {
                "maximize a janela para ver mais".to_string()
            };
            window.push_notification(format!("Mostrando {mostradas} de {total} chats ({motivo})."), cx);
        }
        cx.notify();
    }
}

/// Uma linha de ícone e texto dentro de um cartão.
fn h_flex_linha() -> Div {
    div().flex().items_center().gap(esp::E8)
}

/// Do lado de dentro do painel (que já tem 12) até o texto dos cabeçalhos de seção: a guia de 24.
const RECUO_SECAO: f32 = 12.;

/// O que cada linha do painel é, para o respiro entre elas.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum Bloco {
    /// Com fundo ou contorno (cartão, aviso, estado vazio): a tinta começa na borda.
    Caixa,
    /// A linha do resumo (28, o texto no meio).
    Resumo,
    /// Uma seção: o cabeçalho com 4 em cima e 4 embaixo (o texto a 11,6 das duas bordas) e o
    /// que ela abre, que guarda o próprio fim.
    Secao,
    /// Uma tarefa concluída (32, o texto no meio).
    Linha,
}

/// O espaço antes de `agora`, medido na tinta: 8 entre caixas e, antes de cada seção, 16 da
/// borda do cartão, do texto da seção de cima ou da última linha até o texto dela.
fn respiro(antes: Option<Bloco>, agora: Bloco) -> Rems {
    use Bloco::*;
    let Some(antes) = antes else { return r(0.) };
    r(match (antes, agora) {
        (Caixa, Secao) => 4.,
        (Resumo, Secao) => -4.,
        (Linha, Secao) => -6.,
        (Secao, Secao) => -8.,
        (Secao, Linha) => 0.,
        (Linha, Linha) => 2.,
        (Secao, Caixa) => 4.,
        _ => 8.,
    })
}

/// De quem é a equipe que a direita mostra com esta conversa: do próprio coordenador (ou do chat de
/// projeto, com a tarefa dele) ou, num trabalhador, do coordenador da tarefa dele.
pub(super) fn coordenador_da_equipe(motor: &Motor, chat: &Chat) -> Option<String> {
    match chat.papel {
        Papel::Coordenador | Papel::Mestre | Papel::Projeto => Some(chat.id.clone()),
        Papel::Trabalhador => chat
            .tarefa
            .and_then(|t| motor.tarefa(t))
            .map(|t| t.coordenador.clone())
            .or_else(|| chat.pai.clone()),
    }
}

/// O que a equipe espera de você, para o botão dela quando ela está fora da vista: quantas
/// tarefas abertas pedem ação ou caíram, se alguma caiu e o resumo ("1 em revisão, 1 parada").
pub(super) fn pedidos_da_equipe(motor: &Motor, coordenador: &str) -> (usize, bool, String) {
    let mut pedidos = Pedidos::default();
    for tarefa in motor.tarefas_da_equipe(coordenador).filter(|t| !t.estado.encerrada()) {
        let erro = tarefa.estado.ativa() && matches!(motor.execucao(&tarefa.trabalhador), Execucao::Erro(_));
        match principal(motor, tarefa) {
            Some(Principal::VerPedido) => pedidos.permissao += 1,
            _ if erro => pedidos.erro += 1,
            Some(Principal::Retomar) => pedidos.paradas += 1,
            Some(Principal::Revisar) => pedidos.revisao += 1,
            None => {}
        }
    }
    (pedidos.total(), pedidos.erro > 0, pedidos.resumo())
}

/// As tarefas de uma equipe que pedem você, pelo motivo (o mais urgente primeiro).
#[derive(Default, Clone, Copy, PartialEq, Eq, Debug)]
struct Pedidos {
    permissao: usize,
    erro: usize,
    paradas: usize,
    revisao: usize,
}

impl Pedidos {
    fn total(self) -> usize {
        self.permissao + self.erro + self.paradas + self.revisao
    }

    fn resumo(self) -> String {
        [
            (self.permissao, "pedindo permissão", "pedindo permissão"),
            (self.erro, "com erro", "com erro"),
            (self.paradas, "parada", "paradas"),
            (self.revisao, "em revisão", "em revisão"),
        ]
        .into_iter()
        .filter(|(n, ..)| *n > 0)
        .map(|(n, um, varios)| contagem(n, um, varios))
        .collect::<Vec<_>>()
        .join(", ")
    }
}

/// A ação principal de um cartão, se ele pede alguma.
fn principal(motor: &Motor, tarefa: &Tarefa) -> Option<Principal> {
    let pedido = tarefa.estado.ativa() && motor.execucao(&tarefa.trabalhador) == Execucao::EsperandoVoce;
    // Durante a preparação a cópia já existe, mas o trabalhador ainda não começou nela.
    let copia = tarefa.worktree.is_some() && tarefa.preparada;
    // Entrega cuja conferência ficou no meio não tem o que revisar: só as mudanças.
    let revisar = tarefa.estado == EstadoTarefa::EmRevisao
        && tarefa.progresso.is_none()
        && tarefa.entrega_pendente.is_none()
        && copia;
    if pedido {
        Some(Principal::VerPedido)
    } else if motor.tarefa_parada(tarefa) {
        Some(Principal::Retomar)
    } else if revisar {
        Some(Principal::Revisar)
    } else {
        None
    }
}

/// O cartão que leva o botão azul sólido (um só no painel): o destacado, se pede ação; senão o
/// primeiro aberto, na ordem da lista, que pede. Os outros levam o mesmo botão em tinta.
fn primaria(motor: &Motor, coordenador: &str, destacada: Option<u64>) -> Option<u64> {
    let pede = |t: &Tarefa| principal(motor, t).is_some();
    if let Some(t) = destacada.and_then(|d| motor.tarefa(d)).filter(|t| pede(t)) {
        return Some(t.id);
    }
    motor
        .tarefas_da_equipe(coordenador)
        .filter(|t| !t.estado.encerrada() && pede(t))
        .map(|t| t.id)
        .min()
}

/// O principal que não é o primeiro: a tinta do destaque com o rótulo em azul claro. Também o
/// Revisar do cartão de entrega, na conversa: o sólido é só do primeiro cartão da Equipe.
pub(super) fn variante_tinta(cx: &App) -> ButtonCustomVariant {
    ButtonCustomVariant::new(cx)
        .color(Cores::tinta(Cores::destaque()))
        .foreground(Cores::destaque_texto())
        .hover(Cores::destaque().opacity(0.2))
        .active(Cores::destaque().opacity(0.26))
}

/// A pílula da tarefa realça quando ela muda de estado com o painel aberto (E12): a tinta da cor
/// vai de 12% a 24% e volta, em 240 ms. Na montagem não toca.
#[derive(IntoElement)]
struct Realce {
    id: ElementId,
    chave: u64,
    cor: Hsla,
    filho: AnyElement,
}

impl RenderOnce for Realce {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let forca = movimento::disparo(self.id, self.chave, movimento::ENCHER, Curva::Sair, window, cx)
            .map_or(0., |t| 1. - (2. * t - 1.).abs());
        div()
            .rounded_full()
            .when(forca > 0., |d| d.bg(self.cor.opacity(0.12 * forca)))
            .child(self.filho)
    }
}

/// O estado como a pílula o mostra (a palavra e a cor), para o realce saber quando mudou.
fn chave_do_estado(visual: &EstadoVisual) -> u64 {
    use std::hash::{Hash as _, Hasher as _};
    let mut h = std::hash::DefaultHasher::new();
    (visual.estado as u8).hash(&mut h);
    visual.rotulo.hash(&mut h);
    h.finish()
}

/// Um bloco que abre e fecha deslizando a altura (E11), em 180 ms. O conteúdo vem montado também
/// fechado, para o fechamento ter o que recortar; parado e fechado, não entra na tela. A altura é
/// medida no próprio desenho (a do quadro anterior), então nada pisca na montagem nem no
/// primeiro quadro de fechar; aberto e parado, a altura é a do conteúdo. Com o movimento
/// reduzido, aparece e some direto.
#[derive(IntoElement)]
struct Revelar {
    id: ElementId,
    aberto: bool,
    filho: AnyElement,
}

impl Revelar {
    fn new(id: impl Into<ElementId>, aberto: bool, filho: impl IntoElement) -> Self {
        Revelar { id: id.into(), aberto, filho: filho.into_any_element() }
    }
}

impl RenderOnce for Revelar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if cx.reduce_motion() {
            return if self.aberto { self.filho } else { div().into_any_element() };
        }
        let alvo = if self.aberto { 1. } else { 0. };
        let t = movimento::valor(movimento::chave(&self.id, "abre"), alvo, movimento::MOVER, Curva::Mover, window, cx);
        if t <= 0. && !self.aberto {
            return div().into_any_element();
        }
        let altura = window.use_keyed_state(movimento::chave(&self.id, "altura"), cx, |_, _| px(0.));
        let parado = t >= 1. && self.aberto;
        let medida = *altura.read(cx);
        div()
            .overflow_hidden()
            .when(!parado, |d| d.h(medida * t))
            .child(div().child(self.filho).on_prepaint(move |limites, _, cx| {
                altura.update(cx, |a, _| *a = limites.size.height);
            }))
            .into_any_element()
    }
}

/// O painel da direita fica estreito abaixo de 22 rem e só volta acima de 22,5 (a folga evita
/// trocar de desenho a cada pixel com a largura parada no limite).
fn fica_estreita(rem: f32, antes: bool) -> bool {
    if antes {
        rem < LIMITE_ESTREITA + 0.5
    } else {
        rem < LIMITE_ESTREITA - 0.5
    }
}

/// O botão de entrega de "Este chat" (seção 4.2): o modo da pasta vai no rótulo.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Entrega {
    Juntar,
    AbrirPr,
    /// A rodada verificada espera o "Atualizar PR" (com "Ver mudanças").
    AtualizarPr,
    /// O PR está aberto e a cópia mudou depois dele.
    RevisarEAtualizar,
    VerPr(u64),
    /// O chat roda: o botão do modo, desabilitado.
    Trabalhando(String),
}

/// Qual botão de entrega o chat mostra. "Mudanças novas" depois do PR é a cópia suja (a
/// publicação faz o commit, então o que o chat mudou depois fica sem commit).
fn entrega_do_chat(modo_pr: bool, t: &Tarefa, suja: bool, rodando: bool) -> Entrega {
    let aberto = t.pr.as_ref().filter(|p| p.aberto());
    if !modo_pr && aberto.is_none() {
        return Entrega::Juntar;
    }
    if rodando {
        let rotulo = if aberto.is_some() { "Revisar e atualizar PR" } else { "Revisar e abrir PR" };
        return Entrega::Trabalhando(rotulo.into());
    }
    match aberto {
        Some(_) if t.pronto_para_atualizar => Entrega::AtualizarPr,
        Some(_) if suja => Entrega::RevisarEAtualizar,
        Some(pr) => Entrega::VerPr(pr.numero),
        None => Entrega::AbrirPr,
    }
}

fn minuscula(texto: &str) -> String {
    let mut letras = texto.chars();
    letras.next().map(|c| c.to_lowercase().chain(letras).collect()).unwrap_or_default()
}

/// As abas da direita ficam só com o símbolo abaixo de 520 px a 100% (em rem) e só voltam acima
/// de 520 + 7,5 (a mesma folga de meio rem).
fn fica_compacta(rem: f32, antes: bool) -> bool {
    let limite = super::abas_direita::LIMITE_DAS_ABAS / 15.;
    if antes { rem < limite + 0.5 } else { rem < limite }
}

/// Ícone no tamanho de desenho, crescendo com o zoom.
fn icone_em(icone: Lucide, lado: f32, window: &Window) -> impl IntoElement {
    gpui_kit::component::Icon::new(icone).with_size(tam(window, lado))
}

/// Um selo com a explicação na dica.
fn selo_com_dica(id: String, selo: Selo, dica: String) -> Stateful<Div> {
    div()
        .id(SharedString::from(id))
        .flex_none()
        .child(selo)
        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx))
}

/// Copiar do kit, de 24 (troca para o check por 2 s).
fn copiar(id: String, valor: String) -> impl IntoElement {
    BotaoCopiar::new(Clipboard::new(SharedString::from(id)).value(SharedString::from(valor)))
}

/// Uma linha de dado do detalhe (28): o rótulo numa coluna fixa, o valor em mono cortado no
/// começo (o fim do caminho é o que importa), o valor inteiro na dica e copiar.
fn linha_dado(
    id: String,
    rotulo: &'static str,
    valor: String,
    dica: String,
    copia: String,
    largura_rotulo: f32,
) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(esp::E8)
        .h(r(28.))
        .child(div().flex_none().w(r(largura_rotulo)).metadado().text_color(Cores::tenue()).child(rotulo))
        .child(
            div()
                .id(SharedString::from(id.clone()))
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .whitespace_nowrap()
                .text_ellipsis_start()
                .mono_pequeno()
                .text_color(Cores::texto())
                .child(valor)
                .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
        )
        .child(copiar(format!("{id}-copiar"), copia))
}

/// A saída de uma verificação (ou da preparação): o comando, o fim da saída e copiar tudo.
fn bloco_saida(chave: &str, v: &Verificacao) -> AnyElement {
    let texto = if v.saida.trim().is_empty() {
        "(sem saída)".to_string()
    } else {
        v.saida.trim_end().to_string()
    };
    CaixaDeCodigo::new(SharedString::from(format!("saida-{chave}")), texto.into())
        .comando(v.comando.clone())
        .when_some(v.tempo(), |c, t| c.tempo(t))
        .erro(!v.passou)
        .into_any_element()
}

/// "#1, #3": as tarefas da sobreposição ("#1: src/precos.py"), sem repetir.
fn numeros_sobrepostos(sobreposicao: &[String]) -> String {
    let mut numeros: Vec<&str> = Vec::new();
    for item in sobreposicao {
        let numero = item.split_once(':').map_or(item.as_str(), |(n, _)| n).trim();
        if !numeros.contains(&numero) {
            numeros.push(numero);
        }
    }
    numeros.join(", ")
}

fn importantes(revisao: &Revisao) -> usize {
    revisao.problemas.iter().filter(|p| p.gravidade != "baixa").count()
}

fn rodar_do(motor: &Motor, tarefa: &Tarefa) -> Option<String> {
    let pasta = motor.origem_da_tarefa(tarefa);
    Some(motor.config_da_pasta(&tarefa.projeto, &pasta).rodar.trim().to_string()).filter(|r| !r.is_empty())
}

/// Quantas conversas cabem lado a lado com a lista (se pedida) e a Equipe ao lado, na área útil
/// da janela medida a 100%: a mesma conta do orçamento (senão a Equipe viraria gaveta).
fn quantos_cabem(util: f32, lista: bool, direita: bool) -> usize {
    let fixo = if lista { super::LATERAL } else { 0. } + if direita { super::DIREITA } else { 0. };
    ((util - fixo) / super::MIN_CONVERSA).floor().max(0.) as usize
}

/// Mais um painel ao lado dos `abertos` tiraria a Equipe ancorada do lado (ela viraria gaveta
/// debaixo do cursor de quem está clicando nela).
fn empurraria_a_direita(abertos: usize, util: f32, lista: bool) -> bool {
    abertos + 1 > quantos_cabem(util, lista, true)
}

/// As partes da barra, na ordem em que aparecem.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Grupo {
    Aprovada,
    EmRevisao,
    Verificando,
    Rodando,
    EsperandoVoce,
    NaFila,
    Parada,
    Erro,
    Falhou,
    Cancelada,
}

impl Grupo {
    /// O grupo de uma tarefa, pelas palavras da pílula: "preparando" e "ajustando" contam como
    /// rodando, "esperando resposta" como na fila.
    fn de(visual: &EstadoVisual) -> Grupo {
        match visual.estado {
            Estado::Pronto => Grupo::Aprovada,
            Estado::EmRevisao => Grupo::EmRevisao,
            Estado::Verificando => Grupo::Verificando,
            Estado::Rodando => Grupo::Rodando,
            Estado::EsperandoVoce => Grupo::EsperandoVoce,
            Estado::NaFila => Grupo::NaFila,
            Estado::Parado | Estado::ParadoAtencao if visual.rotulo == "cancelada" => Grupo::Cancelada,
            Estado::Parado | Estado::ParadoAtencao => Grupo::Parada,
            Estado::Erro if visual.rotulo == "erro" => Grupo::Erro,
            Estado::Erro => Grupo::Falhou,
        }
    }

    fn nomes(self) -> (&'static str, &'static str) {
        match self {
            Grupo::Aprovada => ("aprovada", "aprovadas"),
            Grupo::EmRevisao => ("em revisão", "em revisão"),
            Grupo::Verificando => ("verificando", "verificando"),
            Grupo::Rodando => ("rodando", "rodando"),
            Grupo::EsperandoVoce => ("esperando você", "esperando você"),
            Grupo::NaFila => ("na fila", "na fila"),
            Grupo::Parada => ("parada", "paradas"),
            Grupo::Erro => ("erro", "erros"),
            Grupo::Falhou => ("falhou", "falharam"),
            Grupo::Cancelada => ("cancelada", "canceladas"),
        }
    }
}

/// Quantas tarefas há em cada parte da barra (e quais), com o estado que dá a cor.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Contagem {
    grupos: BTreeMap<Grupo, (Estado, Vec<u64>)>,
}

impl Contagem {
    pub(super) fn somar(&mut self, tarefa: u64, visual: &EstadoVisual) {
        let (estado, tarefas) = self.grupos.entry(Grupo::de(visual)).or_insert((visual.estado, Vec::new()));
        // Uma parada que pede Retomar pinta o grupo de âmbar.
        if visual.estado == Estado::ParadoAtencao {
            *estado = Estado::ParadoAtencao;
        }
        tarefas.push(tarefa);
    }

    fn texto(grupo: Grupo, n: usize) -> String {
        let (um, varios) = grupo.nomes();
        contagem(n, um, varios)
    }

    /// Os segmentos da barra: tamanho, cor e a dica ("1 em revisão: #2").
    pub(super) fn partes(&self) -> Vec<(usize, Hsla, SharedString)> {
        self.grupos
            .iter()
            .map(|(grupo, (estado, tarefas))| {
                let numeros: Vec<String> = tarefas.iter().map(|t| format!("#{t}")).collect();
                let dica = format!("{}: {}", Self::texto(*grupo, tarefas.len()), numeros.join(", "));
                (tarefas.len(), estado.cor().unwrap_or_else(Cores::tenue), dica.into())
            })
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Passo {
    Pendente,
    Atual,
    Ok,
    Aviso,
    Falha,
    /// O trabalho parou no meio e espera o Retomar.
    Parado,
}

#[derive(Debug)]
struct Etapa {
    nome: &'static str,
    passo: Passo,
    detalhe: String,
}

/// " em 12s": quanto a verificação levou.
fn em_tempo(v: &Verificacao) -> String {
    v.tempo().map(|t| format!(" em {t}")).unwrap_or_default()
}

/// As etapas da tarefa (trabalho, verificação, revisor, coordenador, juntada), tiradas dos campos
/// que já existem. Enquanto o trabalhador ajusta, verificação e revisor ficam pendentes: o
/// resultado guardado é da rodada anterior. Também quando a conferência da entrega ficou no meio:
/// aí elas ficam em âmbar até o Retomar.
fn etapas(t: &Tarefa, revisor_ligado: bool, parada: bool) -> Vec<Etapa> {
    let ativa = t.estado.ativa();
    let conferindo = t.conferindo();
    let cortada = t.conferencia_cortada();
    let valem = t.resultados_valem();
    let aprovada = t.estado == EstadoTarefa::Aprovada;
    let mut lista = vec![Etapa {
        nome: "Trabalho",
        passo: match (parada && ativa, ativa) {
            (true, _) => Passo::Parado,
            (false, true) => Passo::Atual,
            (false, false) => Passo::Ok,
        },
        detalhe: if parada && ativa {
            "parada".into()
        } else if ativa {
            format!("rodada {}", t.rodada)
        } else {
            format!("entregue na rodada {}", t.rodada)
        },
    }];
    if let Some(comando) = &t.verificacao {
        let (passo, detalhe) = match &t.ultima_verificacao {
            _ if conferindo => (Passo::Atual, format!("rodando “{comando}”")),
            _ if cortada => (Passo::Aviso, format!("“{comando}” ficou no meio; roda de novo ao retomar")),
            Some(v) if valem && v.passou => (Passo::Ok, format!("“{}” passou{}", v.comando, em_tempo(v))),
            Some(v) if valem => (Passo::Falha, format!("“{}” falhou{}", v.comando, em_tempo(v))),
            _ => (Passo::Pendente, format!("“{comando}” roda na entrega")),
        };
        lista.push(Etapa {
            nome: "Verificação",
            passo,
            detalhe,
        });
    }
    if revisor_ligado || t.revisao.is_some() {
        let (passo, detalhe) = match &t.revisao {
            _ if conferindo => (Passo::Atual, "revisando a entrega".into()),
            _ if cortada => (Passo::Aviso, "ficou no meio; revisa de novo ao retomar".into()),
            Some(r) if valem && r.veredito == "aprovar" => (
                Passo::Ok,
                format!("aprovaria ({})", contagem(r.problemas.len(), "observação", "observações")),
            ),
            Some(r) if valem => (
                Passo::Aviso,
                format!("pede ajustes ({})", contagem(importantes(r), "importante", "importantes")),
            ),
            None if valem => (Passo::Pendente, "não revisou esta entrega".into()),
            _ => (Passo::Pendente, "revisa na entrega".into()),
        };
        lista.push(Etapa {
            nome: "Revisor",
            passo,
            detalhe,
        });
    }
    let decidindo = t.estado == EstadoTarefa::EmRevisao && !conferindo && !cortada;
    lista.push(Etapa {
        nome: "Decisão",
        passo: if aprovada {
            Passo::Ok
        } else if decidindo {
            Passo::Atual
        } else {
            Passo::Pendente
        },
        detalhe: if aprovada {
            "aprovou".into()
        } else if decidindo {
            "quem pediu decide: juntar ou pedir ajuste".into()
        } else {
            "decide depois da conferência".into()
        },
    });
    if let Some(pr) = t.pr.as_ref() {
        let (passo, detalhe) = if t.pronto_para_atualizar {
            (Passo::Atual, format!("rodada pronta para atualizar o PR #{}", pr.numero))
        } else {
            (Passo::Ok, format!("PR #{} {}", pr.numero, pr.estado.nome()))
        };
        lista.push(Etapa { nome: "PR", passo, detalhe });
        let rotulo = super::painel_pr::rotulo_checks(&pr.checks);
        let passo = match pr.checks.estado {
            crate::github::EstadoChecks::Passou => Passo::Ok,
            crate::github::EstadoChecks::Falhou => Passo::Falha,
            crate::github::EstadoChecks::Pendente => Passo::Atual,
            crate::github::EstadoChecks::Nenhum => Passo::Pendente,
        };
        lista.push(Etapa { nome: "Checks", passo, detalhe: minuscula(&rotulo.frase) });
    } else if let Some(a) = t.prs_anteriores.last().filter(|_| t.estado != EstadoTarefa::EmRevisao) {
        lista.push(Etapa { nome: "PR", passo: Passo::Ok, detalhe: format!("PR #{} {}", a.numero, a.estado.nome()) });
    } else if t.branch.is_some() {
        lista.push(Etapa {
            nome: "Juntada",
            passo: if aprovada { Passo::Ok } else { Passo::Pendente },
            detalhe: if aprovada {
                "juntada ao projeto".into()
            } else {
                "entra no projeto ao aprovar".into()
            },
        });
    }
    lista
}

/// O sinal que ganha uma linha só para ele no cartão; os outros viram selos.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Motivo {
    Preparacao,
    Verificacao,
    Revisor,
    Sobreposicao,
    Progresso,
}

/// Por prioridade: preparação que falhou, verificação que falhou, revisor que pede ajustes,
/// mesmos arquivos de outra tarefa e o progresso. Com a tarefa rodando, o progresso vem antes
/// (verificação e revisor ainda são da rodada anterior). A parada não tem linha: a pílula e o
/// Retomar já dizem tudo.
fn motivo(t: &Tarefa) -> Option<Motivo> {
    if t.estado.encerrada() {
        return None;
    }
    if t.preparacao.as_ref().is_some_and(|p| !p.passou) {
        return Some(Motivo::Preparacao);
    }
    let conferida =
        t.estado == EstadoTarefa::EmRevisao && t.progresso.is_none() && t.entrega_pendente.is_none();
    if conferida && t.ultima_verificacao.as_ref().is_some_and(|v| !v.passou) {
        return Some(Motivo::Verificacao);
    }
    if conferida && t.revisao.as_ref().is_some_and(|r| r.veredito != "aprovar") {
        return Some(Motivo::Revisor);
    }
    // O texto que o próprio Vitral põe ao preparar, esperar ou conferir já sai na pílula.
    let progresso = t.progresso.as_deref().filter(|p| {
        !(p.starts_with("preparando") || p.starts_with("esperando") || p.starts_with("verificando"))
    });
    match (progresso.is_some(), t.sobreposicao.is_empty()) {
        (true, _) if t.estado.ativa() => Some(Motivo::Progresso),
        (_, false) => Some(Motivo::Sobreposicao),
        (true, true) => Some(Motivo::Progresso),
        (false, true) => None,
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use super::{
        Bloco, Contagem, EstadoTarefa, Grupo, Motivo, Passo, Pedidos, Revisao, Tarefa, Verificacao,
        empurraria_a_direita, etapas, fica_estreita, motivo, numeros_sobrepostos, quantos_cabem, respiro,
    };
    use crate::modelo::Problema;
    use crate::ui::componentes::{Estado, EstadoVisual};

    fn tarefa(estado: &str) -> Tarefa {
        // Pelo JSON, para o teste não quebrar quando a Tarefa ganhar campos com padrão.
        let json = format!(
            r#"{{"id": 2, "projeto": "loja", "coordenador": "c", "trabalhador": "t", "titulo": "Preços",
                "objetivo": "o", "criterios": [], "verificacao": "cargo test", "estado": "{estado}",
                "rodada": 1, "branch": "vitral/tarefa-2", "criada_em": 0}}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    fn verificacao(passou: bool) -> Verificacao {
        Verificacao {
            comando: "cargo test".into(),
            passou,
            saida: String::new(),
            duracao_s: None,
        }
    }

    fn passos(t: &Tarefa, revisor: bool, parada: bool) -> Vec<(&'static str, Passo)> {
        etapas(t, revisor, parada).into_iter().map(|e| (e.nome, e.passo)).collect()
    }

    fn visual(estado: Estado, rotulo: &str) -> EstadoVisual {
        EstadoVisual { estado, rotulo: rotulo.to_string().into(), detalhe: None, dica: "".into() }
    }

    #[test]
    fn barra_usa_as_palavras_da_pilula_na_ordem() {
        let mut conta = Contagem::default();
        assert!(conta.partes().is_empty());
        conta.somar(1, &visual(Estado::Rodando, "rodando"));
        conta.somar(2, &visual(Estado::Rodando, "ajustando"));
        conta.somar(3, &visual(Estado::Parado, "parada"));
        conta.somar(4, &visual(Estado::EmRevisao, "em revisão"));
        conta.somar(5, &visual(Estado::Pronto, "aprovada"));
        conta.somar(6, &visual(Estado::Pronto, "aprovada"));
        conta.somar(7, &visual(Estado::Parado, "cancelada"));
        conta.somar(8, &visual(Estado::Verificando, "verificando"));
        conta.somar(9, &visual(Estado::NaFila, "esperando resposta"));
        let dicas: Vec<String> = conta.partes().into_iter().map(|(_, _, d)| d.to_string()).collect();
        assert_eq!(
            dicas.join(" · "),
            "2 aprovadas: #5, #6 · 1 em revisão: #4 · 1 verificando: #8 · 2 rodando: #1, #2 · \
             1 na fila: #9 · 1 parada: #3 · 1 cancelada: #7"
        );
        let tamanhos: Vec<usize> = conta.partes().into_iter().map(|(n, _, _)| n).collect();
        assert_eq!(tamanhos, [2, 1, 1, 2, 1, 1, 1]);
    }

    #[test]
    fn grupos_separam_erro_de_falha_e_parada_de_cancelada() {
        assert_eq!(Grupo::de(&visual(Estado::Erro, "erro")), Grupo::Erro);
        assert_eq!(Grupo::de(&visual(Estado::Erro, "falhou")), Grupo::Falhou);
        assert_eq!(Grupo::de(&visual(Estado::ParadoAtencao, "parada")), Grupo::Parada);
        assert_eq!(Grupo::de(&visual(Estado::Parado, "cancelada")), Grupo::Cancelada);
        // Uma parada que pede Retomar pinta o grupo inteiro de âmbar.
        let mut conta = Contagem::default();
        conta.somar(1, &visual(Estado::Parado, "parada"));
        conta.somar(2, &visual(Estado::ParadoAtencao, "parada"));
        assert_eq!(conta.grupos[&Grupo::Parada].0, Estado::ParadoAtencao);
        assert_eq!(conta.partes().len(), 1);
    }

    /// A conta do `respiro`: da tinta de cima até a borda do bloco (o texto do cabeçalho fica a
    /// 11,6 das bordas da faixa de 4 + 24 + 4; o do resumo a 9,6 da de baixo; o da linha
    /// concluída a 10,9), mais o respiro, mais os 11,6 do cabeçalho de baixo.
    #[test]
    fn secoes_ficam_a_16_na_tinta() {
        use Bloco::*;
        let embaixo = |b: Bloco| match b {
            Caixa => 0.,
            Resumo => 9.6,
            Secao => 11.6,
            Linha => 10.9,
        };
        let px = |antes: Option<Bloco>, agora: Bloco| (respiro(antes, agora).0 * 15.).round();
        for antes in [Caixa, Resumo, Secao, Linha] {
            let vao = embaixo(antes) + px(Some(antes), Secao) + 11.6;
            assert!((14. ..=18.).contains(&vao), "{antes:?}: {vao}");
        }
        // Entre caixas, 8; a primeira, colada no topo do painel; as concluídas, 2 entre elas.
        assert_eq!(px(Some(Caixa), Caixa), 8.);
        assert_eq!(px(Some(Resumo), Caixa), 8.);
        assert_eq!(px(None, Resumo), 0.);
        assert_eq!(px(Some(Linha), Linha), 2.);
    }

    #[test]
    fn largura_estreita_com_folga() {
        assert!(fica_estreita(20., false));
        assert!(!fica_estreita(21.8, false));
        assert!(fica_estreita(22.3, true));
        assert!(!fica_estreita(22.6, true));
        // 380 px a 100% (25,3 rem) é normal; a 150% (16,9 rem), estreito.
        assert!(!fica_estreita(380. / 15., false));
        assert!(fica_estreita(380. / 22.5, false));
    }

    #[test]
    fn sobreposicao_mostra_cada_tarefa_uma_vez() {
        let itens = vec!["#1: src/precos.py".to_string(), "#1: src/cupom.py".into(), "#3: a.rs".into()];
        assert_eq!(numeros_sobrepostos(&itens), "#1, #3");
        assert_eq!(numeros_sobrepostos(&["#4".to_string()]), "#4");
    }

    #[test]
    fn etapas_seguem_a_tarefa_e_ignoram_resultado_da_rodada_anterior() {
        let mut t = tarefa("Rodando");
        assert_eq!(
            passos(&t, true, false),
            [
                ("Trabalho", Passo::Atual),
                ("Verificação", Passo::Pendente),
                ("Revisor", Passo::Pendente),
                ("Decisão", Passo::Pendente),
                ("Juntada", Passo::Pendente),
            ]
        );
        assert_eq!(passos(&t, true, true)[0], ("Trabalho", Passo::Parado));

        t.estado = EstadoTarefa::EmRevisao;
        t.progresso = Some("verificando e revisando a entrega".into());
        assert_eq!(passos(&t, true, false)[1..3], [("Verificação", Passo::Atual), ("Revisor", Passo::Atual)]);

        t.progresso = None;
        t.ultima_verificacao = Some(verificacao(false));
        t.revisao = Some(Revisao {
            veredito: "ajustar".into(),
            resumo: String::new(),
            problemas: vec![Problema {
                gravidade: "alta".into(),
                ..Default::default()
            }],
        });
        assert_eq!(
            passos(&t, true, false)[1..4],
            [("Verificação", Passo::Falha), ("Revisor", Passo::Aviso), ("Decisão", Passo::Atual)]
        );
        // A dica cita o comando entre aspas curvas, nunca entre crases.
        let detalhe = &etapas(&t, true, false)[1].detalhe;
        assert_eq!(detalhe, "“cargo test” falhou");

        // Pedido o ajuste, os resultados guardados são da rodada anterior.
        t.estado = EstadoTarefa::Ajustando;
        t.rodada = 2;
        assert_eq!(passos(&t, true, false)[1..3], [("Verificação", Passo::Pendente), ("Revisor", Passo::Pendente)]);

        // Entregou a rodada 2 e a conferência caiu de novo: o que está guardado é da rodada 1, e o
        // coordenador ainda não tem o que decidir.
        let mut cortada = t.clone();
        cortada.estado = EstadoTarefa::EmRevisao;
        cortada.entrega_pendente = Some(false);
        cortada.parada = Some("a verificação foi interrompida".into());
        assert_eq!(
            passos(&cortada, true, true)[..4],
            [
                ("Trabalho", Passo::Ok),
                ("Verificação", Passo::Aviso),
                ("Revisor", Passo::Aviso),
                ("Decisão", Passo::Pendente),
            ]
        );
        cortada.parada = None;
        assert_eq!(motivo(&cortada), None);

        t.estado = EstadoTarefa::Aprovada;
        t.ultima_verificacao = Some(verificacao(true));
        assert_eq!(passos(&t, false, false).last(), Some(&("Juntada", Passo::Ok)));

        // Sem comando de verificação, sem revisor e sem git: só trabalho e coordenador.
        let mut simples = tarefa("Rodando");
        simples.verificacao = None;
        simples.branch = None;
        assert_eq!(passos(&simples, false, false), [("Trabalho", Passo::Atual), ("Decisão", Passo::Pendente)]);
    }

    #[test]
    fn o_botao_de_entrega_segue_o_modo_e_o_pr() {
        use super::{Entrega, entrega_do_chat};
        use crate::modelo::Pr;
        let sem_pr = tarefa("Conversa");
        assert_eq!(entrega_do_chat(false, &sem_pr, false, false), Entrega::Juntar);
        assert_eq!(entrega_do_chat(true, &sem_pr, false, false), Entrega::AbrirPr);
        assert_eq!(entrega_do_chat(true, &sem_pr, false, true), Entrega::Trabalhando("Revisar e abrir PR".into()));
        let mut com_pr = tarefa("NoPr");
        com_pr.pr = Some(Pr { numero: 12, ..Default::default() });
        assert_eq!(entrega_do_chat(true, &com_pr, false, false), Entrega::VerPr(12));
        assert_eq!(entrega_do_chat(true, &com_pr, true, false), Entrega::RevisarEAtualizar);
        assert_eq!(entrega_do_chat(true, &com_pr, true, true), Entrega::Trabalhando("Revisar e atualizar PR".into()));
        com_pr.pronto_para_atualizar = true;
        assert_eq!(entrega_do_chat(true, &com_pr, true, false), Entrega::AtualizarPr);
        // Com o PR aberto, a pasta que voltou a juntar continua mostrando o PR.
        com_pr.pronto_para_atualizar = false;
        assert_eq!(entrega_do_chat(false, &com_pr, false, false), Entrega::VerPr(12));
    }

    #[test]
    fn motivo_segue_a_prioridade() {
        let mut t = tarefa("EmRevisao");
        assert_eq!(motivo(&t), None);
        t.sobreposicao = vec!["#1: src/precos.py".into()];
        assert_eq!(motivo(&t), Some(Motivo::Sobreposicao));
        t.revisao = Some(Revisao {
            veredito: "ajustar".into(),
            ..Default::default()
        });
        assert_eq!(motivo(&t), Some(Motivo::Revisor));
        t.ultima_verificacao = Some(verificacao(false));
        assert_eq!(motivo(&t), Some(Motivo::Verificacao));
        t.preparacao = Some(Verificacao {
            comando: "npm install".into(),
            passou: false,
            saida: String::new(),
            duracao_s: None,
        });
        assert_eq!(motivo(&t), Some(Motivo::Preparacao));

        // Rodando: o progresso do trabalhador vem antes do que é da rodada anterior.
        let mut rodando = tarefa("Ajustando");
        rodando.ultima_verificacao = Some(verificacao(false));
        rodando.sobreposicao = vec!["#1: a.rs".into()];
        rodando.progresso = Some("escrevendo os testes".into());
        assert_eq!(motivo(&rodando), Some(Motivo::Progresso));
        rodando.progresso = Some("esperando resposta do coordenador".into());
        assert_eq!(motivo(&rodando), Some(Motivo::Sobreposicao));

        // Parada no meio não ganha linha (a pílula e o Retomar dizem); aprovada, nada.
        rodando.parada = Some("o Vitral foi fechado no meio".into());
        assert_eq!(motivo(&rodando), Some(Motivo::Sobreposicao));
        let mut aprovada = tarefa("Aprovada");
        aprovada.ultima_verificacao = Some(verificacao(false));
        aprovada.parada = Some("você interrompeu".into());
        assert_eq!(motivo(&aprovada), None);
    }

    #[test]
    fn lado_a_lado_cabe_conforme_a_largura() {
        // Área útil: a janela menos a sombra, a margem e o trilho (2560 → 2448, 1440 → 1326).
        assert_eq!(quantos_cabem(2448., true, true), 6);
        assert_eq!(quantos_cabem(1326., true, true), 2);
        assert_eq!(quantos_cabem(1326., false, true), 3);
        assert_eq!(quantos_cabem(488., true, true), 0);
    }

    #[test]
    fn terceiro_painel_tiraria_a_equipe_do_lado() {
        // Janela inicial com a lista: coordenador e um trabalhador cabem com a Equipe, um terceiro não.
        assert!(!empurraria_a_direita(1, 1326., true));
        assert!(empurraria_a_direita(2, 1326., true));
        // Sem a lista, cabe mais um.
        assert!(!empurraria_a_direita(2, 1326., false));
    }

    #[test]
    fn resumo_do_que_a_equipe_espera() {
        assert_eq!(Pedidos::default().total(), 0);
        assert_eq!(Pedidos::default().resumo(), "");
        let um = Pedidos { revisao: 1, paradas: 1, ..Default::default() };
        assert_eq!(um.total(), 2);
        assert_eq!(um.resumo(), "1 parada, 1 em revisão");
        let varios = Pedidos { permissao: 1, erro: 2, paradas: 2, revisao: 3 };
        assert_eq!(varios.total(), 8);
        assert_eq!(varios.resumo(), "1 pedindo permissão, 2 com erro, 2 paradas, 3 em revisão");
    }
}
