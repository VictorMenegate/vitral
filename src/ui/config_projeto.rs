//! Configuração de um projeto: ícone e cor, preparação de cada cópia nova, arquivos fora do git
//! que vão junto para as cópias (como o .env) e o comando para rodar o projeto. Grava no Salvar.

use gpui_kit::component::{
    WindowExt as _,
    input::{Input, InputEvent, InputState},
};
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        Alerta, CabecalhoDialogo, Chip, GatilhoChip, GrupoDeOpcoes, LinhaDeOpcao, QuadradoDeProjeto, Simbolo,
        TamanhoBotao, TamanhoChip, Tom, aviso, botao_fantasma, popover,
    },
    dialogos::{com_til, subtitulo_caminho},
    icone_projeto::{self, CORES, EscolhaIconeCor, Icone, icone_por_id, ler_cor},
};
use crate::{
    modelo::ConfigProjeto,
    motor::Motor,
    tema::{Cores, FONTE_MONO, Lucide, r},
};

/// Ícone e cor escolhidos (`None` é automático).
type Visual = (Option<&'static Icone>, Option<u32>);

pub struct ConfigProjetoView {
    motor: Entity<Motor>,
    projeto: String,
    janela: WeakEntity<Janela>,
    /// Se o projeto usava git ao abrir (depois de "Ativar git" vale o que a Janela guardou).
    usa_git: bool,
    preparacao: Entity<InputState>,
    rodar: Entity<InputState>,
    copiar: Entity<InputState>,
    /// Os três textos como estavam, para saber se algo mudou.
    iniciais: [String; 3],
    escolha: Entity<EscolhaIconeCor>,
    visual: Visual,
    visual_inicial: Visual,
    /// O palpite pelo conteúdo da pasta e a cor pelo nome (o palpite lê a pasta: uma vez só).
    automatico: (&'static Icone, u32),
    _assinaturas: Vec<Subscription>,
}

impl ConfigProjetoView {
    pub fn new(
        motor: Entity<Motor>,
        projeto: String,
        janela: WeakEntity<Janela>,
        usa_git: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let (atual, pasta) = {
            let m = motor.read(cx);
            (m.config.projetos.get(&projeto).cloned().unwrap_or_default(), m.caminho_projeto(&projeto))
        };
        let campo = |valor: &str, dica: &'static str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| InputState::new(window, cx).placeholder(dica).default_value(valor.to_string()))
        };
        let iniciais = [atual.preparacao.clone(), atual.rodar.clone(), atual.copiar.join(", ")];
        let preparacao = campo(&iniciais[0], "npm install", window, cx);
        let rodar = campo(&iniciais[1], "npm run dev -- --port $VITRAL_PORT", window, cx);
        let copiar = campo(&iniciais[2], ".env*", window, cx);
        let mut assinaturas: Vec<Subscription> = [&preparacao, &rodar, &copiar]
            .into_iter()
            .map(|e| {
                cx.subscribe(e, |_, _, evento: &InputEvent, cx| {
                    if matches!(evento, InputEvent::Change) {
                        cx.notify();
                    }
                })
            })
            .collect();
        let eu = cx.entity().downgrade();
        let escolha = cx.new(|cx| {
            EscolhaIconeCor::new(
                motor.clone(),
                projeto.clone(),
                move |icone, cor, _, cx| {
                    let _ = eu.update(cx, |v, cx| {
                        v.visual = (icone, cor);
                        cx.notify();
                    });
                },
                cx,
            )
        });
        assinaturas.push(cx.observe(&escolha, |_, _, cx| cx.notify()));
        let visual_inicial = (icone_por_id(&atual.icone), ler_cor(&atual.cor));
        let automatico = (
            icone_por_id(icone_projeto::palpite(&pasta)).unwrap_or(&icone_projeto::ICONES[0]),
            icone_projeto::cor_automatica(&projeto),
        );
        Self {
            motor,
            projeto,
            janela,
            usa_git,
            preparacao,
            rodar,
            copiar,
            iniciais,
            escolha,
            visual: visual_inicial,
            visual_inicial,
            automatico,
            _assinaturas: assinaturas,
        }
    }

    /// O primeiro campo, que recebe o cursor ao abrir.
    pub(super) fn foco(&self, cx: &App) -> FocusHandle {
        self.preparacao.read(cx).focus_handle(cx)
    }

    fn textos(&self, cx: &App) -> [String; 3] {
        [&self.preparacao, &self.rodar, &self.copiar].map(|e| e.read(cx).value().trim().to_string())
    }

    fn visual_mudou(&self) -> bool {
        let id = |v: &Visual| (v.0.map(|i| i.id), v.1);
        id(&self.visual) != id(&self.visual_inicial)
    }

    /// Algo mudou desde que abriu (o Salvar só fica ativo assim).
    pub(super) fn mudou(&self, cx: &App) -> bool {
        let iniciais = self.iniciais.clone().map(|t| t.trim().to_string());
        self.visual_mudou() || self.textos(cx) != iniciais
    }

    /// O ícone e a cor que valem agora, com o automático resolvido.
    fn visual_agora(&self) -> (&'static Icone, u32) {
        (self.visual.0.unwrap_or(self.automatico.0), self.visual.1.unwrap_or(self.automatico.1))
    }

    fn usa_git(&self, cx: &App) -> bool {
        self.janela
            .upgrade()
            .and_then(|j| j.read(cx).git_projetos.get(&self.projeto).copied())
            .unwrap_or(self.usa_git)
    }

    /// O quadrado do projeto (com a escolha ainda não salva; 32 no diálogo estreito), o nome e o
    /// caminho.
    pub(super) fn cabecalho(&self, estreito: bool, cx: &App) -> CabecalhoDialogo {
        let (icone, cor) = self.visual_agora();
        let caminho = com_til(&self.motor.read(cx).caminho_projeto(&self.projeto));
        let git = if self.usa_git(cx) { "git" } else { "sem git" };
        CabecalhoDialogo::new(self.projeto.clone())
            .simbolo(QuadradoDeProjeto::new(icone, cor, if estreito { 32. } else { 40. }))
            .subtitulo(subtitulo_caminho("projeto-caminho", format!("{caminho} · {git}")))
    }

    pub(super) fn salvar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.visual_mudou() {
            self.escolha.update(cx, |e, cx| e.gravar(cx));
        }
        let [preparacao, rodar, copiar] = self.textos(cx);
        // O ícone e a cor acabaram de ser gravados (ou ficam como estavam).
        let atual = self.motor.read(cx).config.projetos.get(&self.projeto).cloned().unwrap_or_default();
        let config = ConfigProjeto {
            preparacao,
            rodar,
            copiar: copiar.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect(),
            ..atual
        };
        let projeto = self.projeto.clone();
        self.motor.update(cx, |m, cx| {
            m.mudar_config(
                |c| {
                    c.projetos.insert(projeto, config);
                },
                cx,
            )
        });
        window.push_notification(aviso(Tom::Pronto, format!("Configuração de {} salva", self.projeto)), cx);
    }

    /// "Ícone e cor": o chip com a escolha abre o popover da escolha, com "Usar automático".
    fn icone_e_cor(&self, window: &Window, cx: &App) -> impl IntoElement + use<> {
        let (icone, cor) = self.visual_agora();
        let nome_cor = CORES.iter().find(|(v, _)| *v == cor).map_or("Personalizada", |(_, n)| *n);
        let rotulo: SharedString = format!("{} · {nome_cor}", icone.nome).into();
        let escolhido = self.visual.0.is_some() || self.visual.1.is_some();
        let chip = |aberto: bool| {
            Chip::new("projeto-icone", rotulo.clone())
                .tamanho(TamanhoChip::Medio)
                .simbolo(Simbolo::Projeto(icone, cor))
                .mudado(escolhido)
                .seta(true)
                .aberto(aberto)
                .construir(window, cx)
        };
        let (escolha, automatico) = (self.escolha.clone(), self.escolha.clone());
        popover("projeto-popover-icone")
            .anchor(Anchor::TopRight)
            .offset(px(8.))
            .trigger(GatilhoChip::new(chip(false), chip(true)))
            .content(move |_, window, cx| {
                let automatico = automatico.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap(r(12.))
                    .child(escolha.clone())
                    .child(div().h(px(1.)).bg(Cores::borda()))
                    .child(
                        div().flex().child(
                            botao_fantasma("projeto-icone-auto", Some(Lucide::Undo2.into()), "Usar automático", TamanhoBotao::Barra, window, cx)
                                .tooltip("Ícone pelo conteúdo da pasta e cor pelo nome")
                                .on_click(move |_, window, cx| automatico.update(cx, |e, cx| e.usar_automatico(window, cx))),
                        ),
                    )
            })
    }
}

/// Campo de comando ou de lista de arquivos: largura toda, em mono.
fn campo(entrada: &Entity<InputState>) -> impl IntoElement + use<> {
    Input::new(entrada).font_family(FONTE_MONO)
}

impl Render for ConfigProjetoView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sem_git = !self.usa_git(cx);
        let aviso_git = sem_git.then(|| {
            let (janela, projeto) = (self.janela.clone(), self.projeto.clone());
            Alerta::new(Tom::Atencao, "Sem git, as tarefas trabalham direto na pasta.").acao(
                botao_fantasma("projeto-ativar-git", Some(Lucide::GitBranch.into()), "Ativar git…", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| {
                        let projeto = projeto.clone();
                        let _ = janela.update(cx, |this, cx| this.confirmar_git(projeto, window, cx));
                    }),
            )
        });
        let descricao_icone = if self.visual.0.is_none() && self.visual.1.is_none() {
            "Automático: pelo conteúdo da pasta e pelo nome."
        } else {
            "Aparece no trilho, na lista e nos diálogos."
        };
        div()
            .flex()
            .flex_col()
            .gap(r(24.))
            .children(aviso_git)
            .child(
                GrupoDeOpcoes::new(Some("Aparência".into())).linha(
                    LinhaDeOpcao::new("projeto-aparencia", "Ícone e cor")
                        .descricao(descricao_icone)
                        .controle(self.icone_e_cor(window, cx)),
                ),
            )
            .child(
                GrupoDeOpcoes::new(Some("Cópias das tarefas".into()))
                    .linha(
                        LinhaDeOpcao::new("projeto-preparar", "Preparar cada cópia")
                            .descricao("Roda em cada cópia nova antes de o trabalhador começar.")
                            .campo(campo(&self.preparacao)),
                    )
                    .linha(
                        LinhaDeOpcao::new("projeto-copiar", "Arquivos que vão junto")
                            .descricao("Fora do git, separados por vírgula; * no fim pega o prefixo.")
                            .campo(campo(&self.copiar)),
                    ),
            )
            .child(
                GrupoDeOpcoes::new(Some("Rodar".into())).linha(
                    LinhaDeOpcao::new("projeto-rodar", "Comando para rodar o projeto")
                        .descricao("Abre num terminal. Cada tarefa tem dez portas a partir de $VITRAL_PORT.")
                        .campo(campo(&self.rodar)),
                ),
            )
    }
}
