//! As conexões com o GitHub e o Linear: o grupo "Conexões" das Configurações (quem está conectado,
//! a chave do Linear, o botão do Linear nas conversas e o isolamento dos chats) e o diálogo
//! "Conectar o Linear", que confere a chave com o Linear antes de guardar no chaveiro do sistema.
//! A chave nunca passa por arquivo: o campo é limpo assim que ela vai para o chaveiro.

use std::rc::Rc;

use gpui_kit::component::{
    Disableable as _, Sizable as _, WindowExt as _,
    input::{Input, InputState},
    spinner::Spinner,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    componentes::{
        CabecalhoDialogo, Confirmacao, GrupoDeOpcoes, LinhaDeOpcao, Marca, RodapeDialogo, Selo, Simbolo, TamanhoBotao, Tom,
        TomConfirmacao, aviso, botao_fantasma, botao_icone, botao_primario, confirmar, largura,
    },
    dialogos::{estreito, moldura, topo},
};
use crate::{
    modelo::ContaLinear,
    motor::{EstadoConexao, EstadoGithub, Motor},
    tema::{Cores, Lucide, Tipo as _, esp, tam},
};

/// Onde se cria a chave pessoal no Linear.
const LINK_DA_CHAVE: &str = "https://linear.app/settings/account/security";
const INSTALAR_GH: &str = "sudo dnf install gh";

/// "Workspace Eita · Victor · chave …a1b2".
pub(super) fn resumo_da_conta(conta: &ContaLinear) -> String {
    let mut partes = Vec::new();
    if !conta.workspace.is_empty() {
        partes.push(format!("Workspace {}", conta.workspace));
    }
    if !conta.nome.is_empty() {
        partes.push(conta.nome.clone());
    }
    partes.push(format!("chave …{}", conta.final_da_chave));
    partes.join(" · ")
}

/// "Conectado como VictorMenegate · gist, read:org, repo, workflow".
pub(super) fn conectado_como(login: &str, escopos: &[String]) -> String {
    if escopos.is_empty() { format!("Conectado como {login}") } else { format!("Conectado como {login} · {}", escopos.join(", ")) }
}

/// O que aconteceu com o último "Testar" ou "Destravar" do Linear.
enum Resultado {
    Ok(String),
    Erro(String),
}

/// O grupo "Conexões" das Configurações, com o que está em andamento nele.
pub struct PainelConexoes {
    motor: Entity<Motor>,
    /// Testando, destravando ou desconectando o Linear agora.
    ocupado: bool,
    resultado: Option<Resultado>,
    _observa: Subscription,
}

impl PainelConexoes {
    pub fn new(motor: Entity<Motor>, cx: &mut Context<Self>) -> Self {
        let _observa = cx.observe(&motor, |_, _, cx| cx.notify());
        // Ao abrir as Configurações, o login do gh é conferido de novo.
        motor.update(cx, |m, cx| m.login_github(cx));
        Self { motor, ocupado: false, resultado: None, _observa }
    }

    /// Espera a resposta de uma ação do Linear e mostra embaixo da linha.
    fn esperar<T: 'static>(
        &mut self,
        resposta: smol::channel::Receiver<Result<T, String>>,
        texto: impl Fn(T) -> Option<String> + 'static,
        cx: &mut Context<Self>,
    ) {
        self.ocupado = true;
        self.resultado = None;
        cx.notify();
        cx.spawn(async move |este, cx| {
            let resposta = resposta.recv().await.unwrap_or_else(|_| Err("O Vitral fechou no meio.".into()));
            let _ = este.update(cx, |este, cx| {
                este.ocupado = false;
                este.resultado = match resposta {
                    Ok(valor) => texto(valor).map(Resultado::Ok),
                    Err(erro) => Some(Resultado::Erro(erro)),
                };
                cx.notify();
            });
        })
        .detach();
    }

    fn testar(&mut self, cx: &mut Context<Self>) {
        let resposta = self.motor.update(cx, |m, cx| m.testar_linear(cx));
        self.esperar(resposta, |conta: ContaLinear| Some(format!("A chave funciona: workspace {}.", conta.workspace)), cx);
    }

    fn destravar(&mut self, cx: &mut Context<Self>) {
        let resposta = self.motor.update(cx, |m, cx| m.destravar_chaveiro(cx));
        self.esperar(resposta, |_| None, cx);
    }

    fn desconectar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let eu = cx.entity().downgrade();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Atencao,
                icone: Lucide::Unplug.into(),
                titulo: "Desconectar o Linear?".into(),
                texto: "A chave sai do chaveiro. As issues ligadas a chats continuam aparecendo, mas sem atualizar.".into(),
                detalhes: Vec::new(),
                ok: "Desconectar".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: true,
            },
            move |_, cx| {
                let _ = eu.update(cx, |este, cx| {
                    let resposta = este.motor.update(cx, |m, cx| m.desconectar_linear(cx));
                    este.esperar(resposta, |_| None, cx);
                });
            },
            None,
            window,
            cx,
        );
    }

    /// A linha do GitHub: o login do gh e o que fazer quando falta.
    fn linha_github(&self, estado: &EstadoGithub, window: &Window, cx: &Context<Self>) -> LinhaDeOpcao {
        let linha = LinhaDeOpcao::new("conexao-github", "GitHub").marca(Simbolo::icone(Lucide::Github));
        let motor = self.motor.clone();
        let tentar = move |id: &'static str, window: &Window, cx: &App| {
            let motor = motor.clone();
            botao_fantasma(id, Some(Lucide::RefreshCw.into()), "Tentar de novo", TamanhoBotao::Barra, window, cx)
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.login_github(cx)))
        };
        let entrar = {
            let motor = self.motor.clone();
            botao_fantasma("conexao-github-entrar", Some(Lucide::Github.into()), "Entrar no GitHub", TamanhoBotao::Barra, window, cx)
                .tooltip("Abre o login do gh num terminal; ao voltar para o Vitral, ele confere")
                .on_click(move |_, _, cx| motor.update(cx, |m, _| m.entrar_no_github()))
        };
        match estado {
            EstadoGithub::Conferindo => linha.descricao(carregando("Conferindo o login do gh…", window)),
            EstadoGithub::SemGh => linha
                .descricao(format!("O gh não está instalado. Instale com {INSTALAR_GH}."))
                .controle(
                    botao_fantasma("conexao-github-copiar", Some(Lucide::Copy.into()), "Copiar o comando", TamanhoBotao::Barra, window, cx)
                        .on_click(|_, _, cx| cx.write_to_clipboard(ClipboardItem::new_string(INSTALAR_GH.into()))),
                ),
            EstadoGithub::SemLogin(None) => linha.descricao("Nenhuma conta conectada no gh.").controle(entrar),
            EstadoGithub::SemLogin(Some(motivo)) => {
                linha.descricao("O gh não está mais conectado.").aviso(motivo.clone()).controle(entrar)
            }
            EstadoGithub::SemConexao => linha.descricao("Sem conexão com o GitHub agora.").controle(tentar("conexao-github-tentar", window, cx)),
            EstadoGithub::Erro(erro) => linha.descricao(erro.clone()).controle(tentar("conexao-github-tentar", window, cx)),
            EstadoGithub::Conectado(login) => {
                let linha = linha.descricao(conectado_como(&login.login, &login.escopos));
                if login.tem_escopo("repo") {
                    let motor = self.motor.clone();
                    return linha.controle(
                        botao_icone("conexao-github-conferir", Lucide::RefreshCw, "Conferir de novo", TamanhoBotao::Barra, window, cx)
                            .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.login_github(cx))),
                    );
                }
                let motor = self.motor.clone();
                linha.controle(
                    div()
                        .flex()
                        .items_center()
                        .gap(esp::E8)
                        .child(Selo::cor("falta o escopo repo", Cores::ambar()))
                        .child(
                            botao_fantasma("conexao-github-escopos", None, "Pedir escopos", TamanhoBotao::Barra, window, cx)
                                .tooltip("Roda gh auth refresh -s repo num terminal")
                                .on_click(move |_, _, cx| motor.update(cx, |m, _| m.pedir_escopos_github())),
                        ),
                )
            }
        }
    }

    /// A linha do Linear: a conta, a chave no chaveiro e o resultado do último "Testar".
    fn linha_linear(&self, conta: Option<&ContaLinear>, chave: &EstadoConexao, window: &Window, cx: &Context<Self>) -> LinhaDeOpcao {
        let linha = LinhaDeOpcao::new("conexao-linear", "Linear").marca(Simbolo::Marca(Marca::Linear));
        let motor = self.motor.clone();
        let conectar = botao_fantasma("conexao-linear-conectar", None, "Conectar…", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, window, cx| abrir_conectar_linear(motor.clone(), None, window, cx));
        let Some(conta) = conta else {
            return linha.descricao("Para ver e começar issues nas conversas.").controle(conectar);
        };
        let ocupado = self.ocupado || matches!(chave, EstadoConexao::Lendo);
        let botao = |id: &'static str, rotulo: &'static str, acao: fn(&mut PainelConexoes, &mut Window, &mut Context<PainelConexoes>)| {
            botao_fantasma(id, None, rotulo, TamanhoBotao::Barra, window, cx)
                .when(ocupado, |b| b.disabled(true))
                .on_click(cx.listener(move |este, _, window, cx| acao(este, window, cx)))
        };
        let primeiro = match chave {
            EstadoConexao::ChaveiroTrancado => botao("conexao-linear-destravar", "Destravar", |este, _, cx| este.destravar(cx)),
            EstadoConexao::ChaveiroIndisponivel(_) => botao("conexao-linear-tentar", "Tentar de novo", |este, _, cx| este.destravar(cx)),
            EstadoConexao::SemChave => {
                let motor = self.motor.clone();
                botao_fantasma("conexao-linear-reconectar", None, "Conectar de novo…", TamanhoBotao::Barra, window, cx)
                    .on_click(move |_, window, cx| abrir_conectar_linear(motor.clone(), None, window, cx))
            }
            _ => botao("conexao-linear-testar", "Testar", |este, _, cx| este.testar(cx)),
        };
        let controles = div()
            .flex()
            .items_center()
            .gap(esp::E4)
            .when(ocupado, |d| d.child(Spinner::new().with_size(gpui_kit::component::Size::Size(tam(window, 14.))).color(Cores::tenue())))
            .child(primeiro)
            .child(botao("conexao-linear-desconectar", "Desconectar", |este, window, cx| este.desconectar(window, cx)));
        let aviso_da_chave = match chave {
            EstadoConexao::ChaveiroTrancado => Some("Linear: chaveiro trancado. As issues não atualizam até destravar.".to_string()),
            EstadoConexao::ChaveiroIndisponivel(motivo) => Some(format!("O chaveiro do sistema não respondeu ({motivo}).")),
            EstadoConexao::SemChave => Some("A chave não está mais no chaveiro.".to_string()),
            _ => None,
        };
        let descricao = div()
            .flex()
            .flex_col()
            .child(resumo_da_conta(conta))
            .when_some(self.resultado.as_ref(), |d, resultado| {
                let (cor, texto) = match resultado {
                    Resultado::Ok(t) => (Cores::verde(), t.clone()),
                    Resultado::Erro(t) => (Cores::vermelho(), t.clone()),
                };
                d.child(div().text_color(cor).child(texto))
            });
        let linha = linha.descricao(descricao).controle(controles);
        match aviso_da_chave {
            Some(texto) => linha.aviso(texto),
            None => linha,
        }
    }
}

/// Spinner de 12 e o texto, na cor da descrição.
fn carregando(texto: &'static str, window: &Window) -> AnyElement {
    div()
        .flex()
        .items_center()
        .gap(esp::E6)
        .child(Spinner::new().with_size(gpui_kit::component::Size::Size(tam(window, 12.))).color(Cores::tenue()))
        .child(texto)
        .into_any_element()
}

impl Render for PainelConexoes {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (estado, esconder) = {
            let m = self.motor.read(cx);
            (m.conexoes(), m.config.esconder_botao_linear)
        };
        let alternar = self.motor.clone();
        let mostrar_botao = LinhaDeOpcao::new("conexao-linear-botao", "Mostrar o botão do Linear nas conversas")
            .descricao("No cabeçalho dos chats de projeto e do Mestre.")
            .interruptor(!esconder, move |ligado, _, cx| {
                alternar.update(cx, |m, cx| m.mudar_config(|c| c.esconder_botao_linear = !ligado, cx))
            });
        let isolamento = LinhaDeOpcao::new("conexao-isolamento", "Isolamento dos chats").info(
            "Os chats que recebem texto de fora (uma issue, um aviso de PR) e o que o Vitral roda nas cópias deles ficam \
             no bwrap: sem o barramento de sessão (e com ele o chaveiro), sem o agente ssh, o ~/.ssh e o login do gh. A \
             rede continua aberta.",
        );
        let isolamento = match estado.isolamento {
            None => isolamento.descricao(carregando("Testando o bwrap…", window)),
            Some(true) => isolamento
                .descricao("Os chats que recebem texto de fora rodam sem o chaveiro, o ssh e o login do gh.")
                .controle(Selo::cor("ativo", Cores::verde())),
            Some(false) => isolamento
                .descricao("Esses chats rodam só com os bloqueios de comando.")
                .aviso(if cfg!(target_os = "linux") { "O bwrap não funciona aqui." } else { "Só no Linux." })
                .controle(Selo::cor("indisponível", Cores::ambar())),
        };
        // No diálogo estreito, os controles descem para baixo do texto, como nos outros grupos.
        let estreito = estreito(window, largura::G);
        GrupoDeOpcoes::new(Some("Conexões".into()))
            .linha(self.linha_github(&estado.github, window, cx).embaixo(estreito))
            .linha(self.linha_linear(estado.linear.as_ref(), &estado.chave, window, cx).embaixo(estreito))
            .linha(mostrar_botao)
            .linha(isolamento)
    }
}

// ---------- o diálogo "Conectar o Linear" ----------

enum EstadoDialogo {
    Parado,
    Testando,
    Erro(String),
}

/// O corpo do diálogo: os dois passos, o campo mascarado e o estado do teste.
pub struct ConectarLinear {
    motor: Entity<Motor>,
    campo: Entity<InputState>,
    estado: EstadoDialogo,
    /// Do popover das issues: volta para ele depois de conectar.
    ao_conectar: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    _assinaturas: Vec<Subscription>,
}

impl ConectarLinear {
    fn new(motor: Entity<Motor>, ao_conectar: Option<Rc<dyn Fn(&mut Window, &mut App)>>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let campo = cx.new(|cx| InputState::new(window, cx).masked(true).placeholder("Cole a chave (começa com lin_api_)"));
        let assinaturas = vec![
            cx.observe(&motor, |_, _, cx| cx.notify()),
            cx.subscribe(&campo, |este: &mut Self, _, evento: &gpui_kit::component::input::InputEvent, cx| {
                if matches!(evento, gpui_kit::component::input::InputEvent::Change) {
                    if matches!(este.estado, EstadoDialogo::Erro(_)) {
                        este.estado = EstadoDialogo::Parado;
                    }
                    cx.notify();
                }
            }),
        ];
        Self { motor, campo, estado: EstadoDialogo::Parado, ao_conectar, _assinaturas: assinaturas }
    }

    fn vazio(&self, cx: &App) -> bool {
        self.campo.read(cx).value().trim().is_empty()
    }

    /// "Testar e salvar" (e o Enter): confere com o Linear e grava no chaveiro, em segundo plano.
    fn testar_e_salvar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if matches!(self.estado, EstadoDialogo::Testando) || self.vazio(cx) {
            return;
        }
        let chave = self.campo.read(cx).value().to_string();
        let resposta = self.motor.update(cx, |m, cx| m.conectar_linear(chave, cx));
        self.estado = EstadoDialogo::Testando;
        cx.notify();
        cx.spawn_in(window, async move |este, cx| {
            let resposta = resposta.recv().await.unwrap_or_else(|_| Err("O Vitral fechou no meio.".into()));
            let _ = este.update_in(cx, |este, window, cx| {
                match resposta {
                    Ok(conta) => {
                        // A chave já está no chaveiro: sai do campo antes de qualquer outra coisa.
                        este.campo.update(cx, |c, cx| c.set_value("", window, cx));
                        este.estado = EstadoDialogo::Parado;
                        window.close_dialog(cx);
                        let workspace = if conta.workspace.is_empty() { conta.nome.clone() } else { conta.workspace.clone() };
                        window.push_notification(aviso(Tom::Pronto, format!("Linear conectado: {workspace}")), cx);
                        if let Some(depois) = este.ao_conectar.clone() {
                            depois(window, cx);
                        }
                    }
                    Err(erro) => este.estado = EstadoDialogo::Erro(erro),
                }
                cx.notify();
            });
        })
        .detach();
    }
}

impl Render for ConectarLinear {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let isolado = self.motor.read(cx).isolamento_disponivel();
        let passo = |n: &'static str, texto: &'static str| {
            div().flex().gap(esp::E6).corpo_ui().text_color(Cores::texto()).child(div().flex_none().text_color(Cores::tenue()).child(n)).child(texto)
        };
        let recuo = |filho: AnyElement| div().pl(esp::E16).child(filho);
        let abrir = botao_fantasma("linear-abrir-chave", Some(Lucide::ExternalLink.into()), "Abrir no navegador", TamanhoBotao::Linha, window, cx)
            .on_click(|_, _, cx| cx.open_url(LINK_DA_CHAVE));
        let primeiro = v_flex()
            .gap(esp::E4)
            .child(passo("1.", "Crie uma chave pessoal de API no Linear"))
            .child(recuo(
                v_flex()
                    .gap(esp::E4)
                    .items_start()
                    .child(div().metadado().text_color(Cores::tenue()).child("Configurações › Conta › Segurança e acesso"))
                    .child(abrir)
                    .child(
                        div()
                            .metadado()
                            .text_color(Cores::tenue())
                            .child("Permissões: Leitura, Escrita e Criar comentários. Limite aos times que você usa."),
                    )
                    .into_any_element(),
            ));
        let testando = matches!(self.estado, EstadoDialogo::Testando);
        let segundo = v_flex()
            .gap(esp::E8)
            .child(passo("2.", "Cole a chave aqui"))
            .child(Input::new(&self.campo).mask_toggle().disabled(testando));
        let nota = if isolado {
            div().metadado().text_color(Cores::tenue()).child(
                "A chave fica no chaveiro do sistema (Senhas e chaves), nunca em arquivo. Os chats que trabalham em issues e \
                 PRs rodam isolados e não alcançam o chaveiro. Outros programas abertos na sua conta conseguem ler o \
                 chaveiro destravado; por isso, use uma chave só com estas permissões.",
            )
        } else {
            div().metadado().text_color(Cores::ambar()).child(
                "A chave fica no chaveiro do sistema (Senhas e chaves), nunca em arquivo. Sem o isolamento, um chat consegue \
                 ler esta chave.",
            )
        };
        let estado = match &self.estado {
            EstadoDialogo::Parado => None,
            EstadoDialogo::Testando => Some(
                div()
                    .flex()
                    .items_center()
                    .gap(esp::E6)
                    .metadado()
                    .text_color(Cores::tenue())
                    .child(Spinner::new().with_size(gpui_kit::component::Size::Size(tam(window, 12.))).color(Cores::tenue()))
                    .child("Conferindo a chave com o Linear…")
                    .into_any_element(),
            ),
            EstadoDialogo::Erro(erro) => Some(div().metadado().text_color(Cores::vermelho()).child(erro.clone()).into_any_element()),
        };
        v_flex().gap(esp::E16).child(primeiro).child(segundo).child(nota).children(estado)
    }
}

/// Abre "Conectar o Linear" (das Configurações ou do popover das issues, que passa `ao_conectar`
/// para voltar a ele já com as issues).
pub(super) fn abrir_conectar_linear(
    motor: Entity<Motor>,
    ao_conectar: Option<Rc<dyn Fn(&mut Window, &mut App)>>,
    window: &mut Window,
    cx: &mut App,
) {
    let corpo = cx.new(|cx| ConectarLinear::new(motor, ao_conectar, window, cx));
    let foco = corpo.read(cx).campo.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialogo, window, cx| {
        let c = corpo.read(cx);
        let ocupado = matches!(c.estado, EstadoDialogo::Testando);
        let vazio = c.vazio(cx);
        let (no_botao, no_enter, ao_cancelar) = (corpo.clone(), corpo.clone(), corpo.clone());
        let principal = botao_primario(
            "linear-testar-salvar",
            None,
            if ocupado { "Testando…" } else { "Testar e salvar" },
            TamanhoBotao::Controle,
            window,
            cx,
        )
        .when(vazio || ocupado, |b| b.disabled(true).opacity(0.45))
        .when(vazio, |b| b.tooltip("Cole a chave"))
        .on_click(move |_, window, cx| no_botao.update(cx, |c, cx| c.testar_e_salvar(window, cx)));
        let cancelar = botao_fantasma("linear-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx).on_click(move |_, window, cx| {
            ao_cancelar.update(cx, |c, cx| c.campo.update(cx, |campo, cx| campo.set_value("", window, cx)));
            window.close_dialog(cx);
        });
        moldura(dialogo, largura::P, window)
            .title(topo(CabecalhoDialogo::new("Conectar o Linear").simbolo(Marca::Linear.disco(20.))))
            .child(corpo.clone())
            .footer(RodapeDialogo::new().cancelar(cancelar).principal(principal))
            // O Enter testa e salva; o diálogo só fecha quando o Linear aceitar.
            .on_ok(move |_, window, cx| {
                no_enter.update(cx, |c, cx| c.testar_e_salvar(window, cx));
                false
            })
    });
    window.focus(&foco, cx);
}

#[cfg(test)]
mod testes {
    // Sem glob: o `gpui_kit::*` traz um `test` que tomaria o lugar do `#[test]`.
    use super::{ContaLinear, conectado_como, resumo_da_conta};

    #[test]
    fn textos_da_conta() {
        let conta = ContaLinear { workspace: "Eita".into(), nome: "Victor".into(), final_da_chave: "a1b2".into(), ..Default::default() };
        assert_eq!(resumo_da_conta(&conta), "Workspace Eita · Victor · chave …a1b2");
        assert_eq!(resumo_da_conta(&ContaLinear { final_da_chave: "zz99".into(), ..Default::default() }), "chave …zz99");
        let escopos = ["gist", "read:org", "repo", "workflow"].map(String::from);
        assert_eq!(conectado_como("VictorMenegate", &escopos), "Conectado como VictorMenegate · gist, read:org, repo, workflow");
        assert_eq!(conectado_como("x", &[]), "Conectado como x");
    }
}
