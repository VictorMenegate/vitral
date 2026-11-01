//! Novo projeto e Editar projeto, no mesmo formulário: nome, ícone e cor, descrição, as pastas
//! ligadas (nenhuma, uma ou várias, com Localizar pasta…), as instruções para os chats, as decisões
//! anotadas por eles e, no Avançado, a preparação, os arquivos e o rodar de cada pasta, e a
//! permissão, o modelo e a conta do projeto. Grava uma vez, no Criar ou no Salvar, só o que o
//! formulário mudou (a ordem e o arquivamento mexidos com ele aberto ficam como estão).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    button::ButtonVariants as _,
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela, Vista,
    componentes::{
        Alerta, CabecalhoDeSecao, Campo, Chip, Confirmacao, GatilhoChip, GrupoDeOpcoes, LinhaDeOpcao, QuadradoDeProjeto,
        RodapeDialogo, Simbolo, TamanhoBotao, TamanhoChip, Tom, TomConfirmacao, aviso, aviso_erro, botao_fantasma,
        botao_icone, botao_primario, confirmar, contagem, largura, popover, variante_perigo_calma,
    },
    composer::{DESCRICOES_DE_MODELO, MODELOS, MODOS},
    config_projeto::ConfigDaPasta,
    configuracoes::{Opcao, menu_escolha},
    dialogos::{cabecalho, com_til, moldura, topo},
    icone_projeto::{self, CORES, EscolhaIconeCor, Icone, icone_por_id, ler_cor},
};
use crate::{
    dados,
    modelo::{ConfigProjeto, PastaLigada, Projeto, SugestaoProjeto},
    motor::{MAX_INSTRUCOES, Motor, MudancaProjeto, NovoProjeto},
    tema::{Cores, Lucide, Tipo as _, esp},
};


/// O que o formulário tem, para conferir antes de gravar.
pub(super) struct Formulario<'a> {
    pub nome: &'a str,
    pub instrucoes: &'a str,
    pub pastas: &'a [PathBuf],
}

/// O que não deixa gravar, por campo.
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Problemas {
    pub nome: Option<String>,
    pub instrucoes: Option<String>,
    pub pastas: Option<String>,
}

impl Problemas {
    pub(super) fn nenhum(&self) -> bool {
        *self == Problemas::default()
    }

    /// O primeiro problema, para a dica do botão.
    fn primeiro(&self) -> Option<&str> {
        self.nome.as_deref().or(self.instrucoes.as_deref()).or(self.pastas.as_deref())
    }
}

/// Confere o formulário: o nome (vazio, repetido ou reservado, pela chave), o tamanho das
/// instruções e as pastas repetidas. `ignorar` é o próprio projeto, na edição.
pub(super) fn validar_formulario(f: &Formulario, projetos: &[Projeto], ignorar: Option<&str>) -> Problemas {
    let nome = dados::chave_valida(f.nome, projetos, ignorar).err();
    let n = f.instrucoes.chars().count();
    let instrucoes = (n > MAX_INSTRUCOES).then(|| format!("Até {MAX_INSTRUCOES} caracteres (tem {n})."));
    let pastas = f
        .pastas
        .iter()
        .enumerate()
        .find(|(i, p)| f.pastas[..*i].contains(p))
        .map(|(_, p)| format!("{} está duas vezes na lista.", com_til(p)));
    Problemas { nome, instrucoes, pastas }
}

/// Por que ligar `nova` pede confirmação: ela já é de outro projeto, fica dentro de uma pasta
/// ligada ou contém uma (as do próprio projeto contam também, menos ela mesma).
pub(super) fn conflito_da_pasta(nova: &Path, projetos: &[Projeto], proprio: Option<&str>, na_lista: &[PathBuf]) -> Option<String> {
    let ligadas = projetos
        .iter()
        .filter(|p| Some(p.id.as_str()) != proprio)
        .flat_map(|p| p.pastas.iter().map(move |l| (l.caminho.clone(), Some(p.nome.clone()))))
        .chain(na_lista.iter().map(|l| (l.clone(), None)));
    for (ligada, dono) in ligadas {
        let de = |dono: &Option<String>| match dono {
            Some(nome) => format!(" (do projeto {nome})"),
            None => String::new(),
        };
        if ligada == nova {
            if let Some(nome) = &dono {
                return Some(format!("{} já está ligada ao projeto {nome}.", com_til(nova)));
            }
        } else if nova.starts_with(&ligada) {
            return Some(format!("{} fica dentro de {}{}.", com_til(nova), com_til(&ligada), de(&dono)));
        } else if ligada.starts_with(nova) {
            return Some(format!("{} contém {}{}.", com_til(nova), com_til(&ligada), de(&dono)));
        }
    }
    None
}

/// As decisões que vão para o disco: o que você escreveu e, se um chat anotou algo enquanto o
/// formulário estava aberto, as linhas novas dele no fim.
pub(super) fn juntar_notas(inicial: &str, no_disco: &str, editado: &str) -> String {
    if no_disco == inicial {
        return editado.to_string();
    }
    let antes: Vec<&str> = inicial.lines().collect();
    let mut texto = editado.trim_end().to_string();
    for linha in no_disco.lines().filter(|l| !l.trim().is_empty() && !antes.contains(l) && !editado.contains(*l)) {
        if !texto.is_empty() {
            texto.push('\n');
        }
        texto.push_str(linha);
    }
    texto.push('\n');
    texto
}

/// Uma pasta da lista do formulário.
struct PastaNoEditor {
    caminho: PathBuf,
    propria: bool,
    config: ConfigDaPasta,
}

/// Como o projeto estava ao abrir, para saber o que mudou.
#[derive(Default)]
struct Inicial {
    nome: String,
    descricao: String,
    instrucoes: String,
    icone: Option<&'static str>,
    cor: Option<u32>,
    pastas: Vec<PathBuf>,
    permissao: String,
    modelo: String,
    conta: String,
}

pub struct EditorProjeto {
    motor: Entity<Motor>,
    janela: WeakEntity<Janela>,
    /// `None`: projeto novo.
    id: Option<String>,
    nome: Entity<InputState>,
    descricao: Entity<TextareaState>,
    instrucoes: Entity<TextareaState>,
    notas: Option<Entity<TextareaState>>,
    notas_iniciais: String,
    pastas: Vec<PastaNoEditor>,
    icone: Option<&'static Icone>,
    cor: Option<u32>,
    escolha: Entity<EscolhaIconeCor>,
    permissao: String,
    modelo: String,
    conta: String,
    inicial: Inicial,
    avancado: bool,
    /// Se cada pasta usa git (lido em segundo plano ao abrir e ao ligar).
    git: HashMap<PathBuf, bool>,
    /// O cartão do Mestre que sugeriu este projeto: Criar responde a ele.
    sugestao: Option<u64>,
    _assinaturas: Vec<Subscription>,
}

impl EditorProjeto {
    pub fn new(
        motor: Entity<Motor>,
        janela: WeakEntity<Janela>,
        id: Option<String>,
        nome: String,
        pastas_novas: Vec<PathBuf>,
        avancado: bool,
        sugestao: Option<(u64, SugestaoProjeto)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let projeto = id.as_deref().and_then(|id| motor.read(cx).projeto(id)).cloned();
        // A sugestão do Mestre preenche o formulário do projeto novo.
        let pastas_novas = match &sugestao {
            Some((_, s)) if pastas_novas.is_empty() => s.pastas.clone(),
            _ => pastas_novas,
        };
        let notas_iniciais = id.as_deref().map(|id| motor.read(cx).dados.notas(id)).unwrap_or_default();
        let inicial = match &projeto {
            Some(p) => Inicial {
                nome: p.nome.clone(),
                descricao: p.descricao.clone(),
                instrucoes: p.instrucoes.clone(),
                icone: icone_por_id(&p.icone).map(|i| i.id),
                cor: ler_cor(&p.cor),
                pastas: p.pastas.iter().map(|l| l.caminho.clone()).collect(),
                permissao: p.permissao.clone().unwrap_or_default(),
                modelo: p.modelo.clone().unwrap_or_default(),
                conta: p.conta.clone().unwrap_or_default(),
            },
            None => match &sugestao {
                Some((_, s)) => Inicial {
                    nome: s.nome.clone(),
                    descricao: s.descricao.clone(),
                    instrucoes: s.instrucoes.clone(),
                    icone: icone_por_id(&s.icone).map(|i| i.id),
                    cor: ler_cor(&s.cor),
                    ..Default::default()
                },
                None => Inicial { nome: nome.clone(), ..Default::default() },
            },
        };
        let mut assinaturas = Vec::new();
        let campo_nome = cx.new(|cx| InputState::new(window, cx).placeholder("loja-api").default_value(inicial.nome.clone()));
        let texto = |linhas: (usize, usize), dica: &'static str, valor: &str, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(linhas.0, linhas.1)
                    .placeholder(dica)
                    .default_value(valor.to_string())
            })
        };
        let descricao = texto((2, 4), "Ex.: uma loja de roupas em Django, com o painel de pedidos", &inicial.descricao, window, cx);
        let instrucoes = texto((3, 8), "Ex.: rode os testes com make test; use português nos textos da loja", &inicial.instrucoes, window, cx);
        let notas = id
            .is_some()
            .then(|| texto((3, 8), "Nenhuma decisão anotada ainda", &notas_iniciais, window, cx));
        assinaturas.push(cx.subscribe(&campo_nome, |_, _, evento: &InputEvent, cx| {
            if matches!(evento, InputEvent::Change) {
                cx.notify();
            }
        }));
        for campo in [Some(&descricao), Some(&instrucoes), notas.as_ref()].into_iter().flatten() {
            assinaturas.push(cx.subscribe(campo, |_, _, evento: &InputEvent, cx| {
                if matches!(evento, InputEvent::Change) {
                    cx.notify();
                }
            }));
        }
        let mut pastas = Vec::new();
        match &projeto {
            Some(p) => {
                for l in &p.pastas {
                    let config = ConfigDaPasta::new(&l.config, &mut assinaturas, window, cx);
                    pastas.push(PastaNoEditor { caminho: l.caminho.clone(), propria: l.propria, config });
                }
            }
            None => {
                for caminho in pastas_novas {
                    let caminho = caminho.canonicalize().unwrap_or(caminho);
                    let config = ConfigDaPasta::new(&ConfigProjeto::default(), &mut assinaturas, window, cx);
                    pastas.push(PastaNoEditor { caminho, propria: false, config });
                }
            }
        }
        let eu = cx.entity().downgrade();
        let (icone, cor) = (inicial.icone.and_then(icone_por_id), inicial.cor);
        let escolha = cx.new(|_| {
            EscolhaIconeCor::new(icone, cor, move |icone, cor, _, cx| {
                let _ = eu.update(cx, |v, cx| {
                    v.icone = icone;
                    v.cor = cor;
                    cx.notify();
                });
            })
        });
        assinaturas.push(cx.observe(&escolha, |_, _, cx| cx.notify()));
        let (permissao, modelo, conta) = (inicial.permissao.clone(), inicial.modelo.clone(), inicial.conta.clone());
        let mut editor = EditorProjeto {
            motor,
            janela,
            id,
            nome: campo_nome,
            descricao,
            instrucoes,
            notas,
            notas_iniciais,
            pastas,
            icone,
            cor,
            escolha,
            permissao,
            modelo,
            conta,
            inicial,
            avancado,
            git: HashMap::new(),
            sugestao: sugestao.map(|(cartao, _)| cartao),
            _assinaturas: assinaturas,
        };
        editor.ler_git(cx);
        editor
    }

    /// O campo do nome, que recebe o cursor ao abrir.
    pub(super) fn foco(&self, cx: &App) -> FocusHandle {
        self.nome.read(cx).focus_handle(cx)
    }

    fn novo(&self) -> bool {
        self.id.is_none()
    }

    /// Lê, em segundo plano, se cada pasta usa git.
    fn ler_git(&mut self, cx: &mut Context<Self>) {
        let faltam: Vec<PathBuf> = self.pastas.iter().map(|p| p.caminho.clone()).filter(|p| !self.git.contains_key(p)).collect();
        if faltam.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let lidos = cx
                .background_spawn(async move { faltam.into_iter().map(|p| { let g = crate::git::usa_git(&p); (p, g) }).collect::<Vec<_>>() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.git.extend(lidos);
                cx.notify();
            });
        })
        .detach();
    }

    fn caminhos(&self) -> Vec<PathBuf> {
        self.pastas.iter().map(|p| p.caminho.clone()).collect()
    }

    fn problemas(&self, cx: &App) -> Problemas {
        let nome = self.nome.read(cx).value();
        let instrucoes = self.instrucoes.read(cx).value();
        let caminhos = self.caminhos();
        let f = Formulario { nome: &nome, instrucoes: &instrucoes, pastas: &caminhos };
        validar_formulario(&f, &self.motor.read(cx).projetos, self.id.as_deref())
    }

    /// O ícone e a cor que valem agora, com o automático resolvido (a cor do projeto novo sai do
    /// nome escrito, a de um que já existe, do id).
    fn visual_agora(&self, cx: &App) -> (&'static Icone, u32) {
        let (icone, cor) = self.escolha.read(cx).automatico();
        (self.icone.unwrap_or(icone), self.cor.unwrap_or(cor))
    }

    /// Algo mudou desde que abriu (na edição, o Salvar só acende assim).
    fn mudou(&self, cx: &App) -> bool {
        let texto = |e: &Entity<TextareaState>| e.read(cx).value().to_string();
        self.nome.read(cx).value().trim() != self.inicial.nome.trim()
            || texto(&self.descricao).trim() != self.inicial.descricao.trim()
            || texto(&self.instrucoes).trim() != self.inicial.instrucoes.trim()
            || self.notas.as_ref().is_some_and(|n| texto(n).trim() != self.notas_iniciais.trim())
            || self.icone.map(|i| i.id) != self.inicial.icone
            || self.cor != self.inicial.cor
            || self.caminhos() != self.inicial.pastas
            || self.pastas.iter().any(|p| p.config.mudou(cx))
            || self.permissao != self.inicial.permissao
            || self.modelo != self.inicial.modelo
            || self.conta != self.inicial.conta
    }

    /// "Adicionar pasta…": o seletor do sistema (várias de uma vez); a que já é de outro projeto,
    /// ou fica dentro ou em volta de uma ligada, pergunta antes.
    fn adicionar_pastas(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Ligar ao projeto".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else {
                return;
            };
            let _ = this.update_in(cx, |this, window, cx| {
                for caminho in caminhos {
                    this.pedir_para_ligar(caminho, window, cx);
                }
            });
        })
        .detach();
    }

    fn pedir_para_ligar(&mut self, caminho: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let caminho = caminho.canonicalize().unwrap_or(caminho);
        if self.pastas.iter().any(|p| p.caminho == caminho) {
            return;
        }
        let conflito = conflito_da_pasta(&caminho, &self.motor.read(cx).projetos, self.id.as_deref(), &self.caminhos());
        let Some(porque) = conflito else {
            self.ligar(caminho, window, cx);
            return;
        };
        let eu = cx.entity().downgrade();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Atencao,
                icone: Lucide::FolderInput.into(),
                titulo: "Ligar esta pasta mesmo assim?".into(),
                texto: format!("{porque} Os chats de um projeto não escrevem nas pastas do outro.").into(),
                detalhes: Vec::new(),
                ok: "Ligar".into(),
                cancelar: "Cancelar".into(),
                ok_perigo: false,
            },
            move |window, cx| {
                let caminho = caminho.clone();
                let _ = eu.update(cx, |this, cx| this.ligar(caminho, window, cx));
            },
            None,
            window,
            cx,
        );
    }

    /// Põe a pasta na lista; a primeira, sem ícone escolhido, já mostra o palpite dela.
    fn ligar(&mut self, caminho: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        let mut assinaturas = Vec::new();
        let config = ConfigDaPasta::new(&ConfigProjeto::default(), &mut assinaturas, window, cx);
        self._assinaturas.extend(assinaturas);
        self.pastas.push(PastaNoEditor { caminho, propria: false, config });
        self.ler_git(cx);
        cx.notify();
    }

    /// A pasta sumiu (disco desmontado, renomeada): a escolhida passa a ser ela, já no projeto,
    /// com as origens dos chats juntas.
    fn localizar(&mut self, indice: usize, window: &mut Window, cx: &mut Context<Self>) {
        let escolha = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Usar esta pasta".into()),
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(caminhos))) = escolha.await else {
                return;
            };
            let Some(nova) = caminhos.into_iter().next() else {
                return;
            };
            let Ok(Some((antiga, tarefa))) = this.update(cx, |this, cx| {
                let antiga = this.pastas.get(indice).map(|p| p.caminho.clone())?;
                let tarefa = match &this.id {
                    Some(id) => {
                        let id = id.clone();
                        this.motor.update(cx, |m, cx| {
                            let no_motor = m.pastas_do(&id).iter().position(|p| p.caminho == antiga);
                            no_motor.map(|i| m.localizar_pasta(&id, i, &nova, cx))
                        })
                    }
                    None => None,
                };
                Some((antiga, tarefa))
            }) else {
                return;
            };
            let resultado = match tarefa {
                Some(tarefa) => tarefa.await,
                None => Ok(None),
            };
            let _ = this.update_in(cx, |this, window, cx| {
                match resultado {
                    Ok(atencao) => {
                        let nova = nova.canonicalize().unwrap_or(nova);
                        if let Some(p) = this.pastas.iter_mut().find(|p| p.caminho == antiga) {
                            p.caminho = nova.clone();
                        }
                        if let Some(i) = this.inicial.pastas.iter().position(|p| *p == antiga) {
                            this.inicial.pastas[i] = nova;
                        }
                        if let Some(atencao) = atencao {
                            window.push_notification(aviso(Tom::Atencao, format!("Pasta localizada. {atencao}")), cx);
                        }
                        this.ler_git(cx);
                    }
                    Err(erro) => window.push_notification(aviso(Tom::Erro, format!("A pasta não mudou: {erro}")), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Os chats do projeto que trabalham numa pasta (ela é a origem deles).
    fn chats_na_pasta(&self, caminho: &Path, cx: &App) -> usize {
        let Some(id) = &self.id else {
            return 0;
        };
        let m = self.motor.read(cx);
        m.tarefas.iter().filter(|t| t.projeto == *id && !t.estado.encerrada() && t.origem == caminho).count()
    }

    /// Cria ou grava. Devolve se fechou (com o nome que não serve, não).
    pub(super) fn confirmar(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !self.problemas(cx).nenhum() {
            return false;
        }
        if self.novo() { self.criar(window, cx) } else { self.salvar(window, cx) }
        true
    }

    fn criar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let nome = self.nome.read(cx).value().trim().to_string();
        // Sem escolha, a cor é a que a prévia mostrou (a do nome); o id novo daria outra.
        let cor = self.cor.unwrap_or_else(|| icone_projeto::cor_automatica(&nome));
        let novo = NovoProjeto {
            nome: nome.clone(),
            pastas: self.caminhos(),
            icone: self.icone.map(|i| i.id.to_string()).unwrap_or_default(),
            cor: format!("#{cor:06x}"),
            descricao: self.descricao.read(cx).value().trim().to_string(),
            instrucoes: self.instrucoes.read(cx).value().trim().to_string(),
            configs: self.pastas.iter().map(|p| p.config.config(cx)).collect(),
            permissao: Some(self.permissao.clone()),
            modelo: Some(self.modelo.clone()),
            conta: Some(self.conta.clone()),
        };
        if let Some(cartao) = self.sugestao {
            // Da sugestão do Mestre: cria e responde ao cartão numa gravação só; o Mestre continua
            // o pedido (sem chat novo aqui).
            let tarefa = self.motor.update(cx, |m, cx| m.responder_sugestao(cartao, Some(novo), cx));
            let janela = self.janela.clone();
            window
                .spawn(cx, async move |cx| {
                    let resultado = tarefa.await;
                    cx.update(|window, cx| match resultado {
                        Ok(Some(id)) => {
                            window.push_notification(aviso(Tom::Pronto, format!("Projeto {nome} criado; o Mestre continua o pedido")), cx);
                            let _ = janela.update(cx, |this, cx| this.abrir_vista(Vista::Projeto(id.clone()), window, cx));
                        }
                        Ok(None) => {}
                        Err(erro) => window.push_notification(aviso_erro("Não deu para criar o projeto", &erro), cx),
                    })
                    .ok();
                })
                .detach();
            return;
        }
        let tarefa = self.motor.update(cx, |m, cx| m.criar_projeto(novo, cx));
        let janela = self.janela.clone();
        window
            .spawn(cx, async move |cx| {
                let resultado = tarefa.await;
                cx.update(|window, cx| match resultado {
                    Ok(id) => {
                        window.push_notification(aviso(Tom::Pronto, format!("Projeto {nome} criado")), cx);
                        let _ = janela.update(cx, |this, cx| {
                            this.abrir_vista(Vista::Projeto(id.clone()), window, cx);
                            this.nova_conversa(&id, window, cx);
                        });
                    }
                    Err(erro) => window.push_notification(aviso_erro("Não deu para criar o projeto", &erro), cx),
                })
                .ok();
            })
            .detach();
    }

    fn salvar(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.id.clone() else {
            return;
        };
        let texto = |e: &Entity<TextareaState>| e.read(cx).value().trim().to_string();
        let nome = self.nome.read(cx).value().trim().to_string();
        // As pastas com a configuração de cada uma (a pasta própria continua própria).
        let pastas: Vec<PastaLigada> = self
            .pastas
            .iter()
            .map(|p| PastaLigada { caminho: p.caminho.clone(), propria: p.propria, config: p.config.config(cx) })
            .collect();
        let mudanca = MudancaProjeto {
            nome: (nome != self.inicial.nome).then(|| nome.clone()),
            icone: Some(self.icone.map(|i| i.id.to_string()).unwrap_or_default()),
            cor: Some(self.cor.map(|c| format!("#{c:06x}")).unwrap_or_default()),
            descricao: Some(texto(&self.descricao)),
            instrucoes: Some(texto(&self.instrucoes)),
            pasta: None,
            pastas: Some(pastas),
            permissao: Some(Some(self.permissao.clone())),
            modelo: Some(Some(self.modelo.clone())),
            conta: Some(Some(self.conta.clone())),
        };
        let notas = self.notas.as_ref().map(|n| n.read(cx).value().to_string());
        let notas_iniciais = self.notas_iniciais.clone();
        let gravou = self.motor.update(cx, |m, cx| {
            m.validar_nome(&nome, Some(&id))?;
            if let Some(editado) = notas.filter(|n| n.trim() != notas_iniciais.trim()) {
                let no_disco = m.dados.notas(&id);
                if let Err(erro) = m.dados.gravar_notas(&id, &juntar_notas(&notas_iniciais, &no_disco, &editado)) {
                    return Err(format!("as decisões não foram gravadas ({erro})"));
                }
            }
            m.mudar_projeto(&id, mudanca, cx)
        });
        let nota = match gravou {
            Ok(()) => aviso(Tom::Pronto, format!("{nome} salvo")),
            Err(erro) => aviso(Tom::Erro, format!("Não salvou {nome}: {erro}")),
        };
        window.push_notification(nota, cx);
        let _ = self.janela.update(cx, |this, cx| {
            this.git_pastas.clear();
            this.readmes.borrow_mut().clear();
            cx.notify();
        });
    }

    /// "Ícone e cor": o quadrado (com a escolha ainda não salva) é o gatilho do popover da escolha.
    fn icone_e_cor(&self, window: &Window, cx: &App) -> impl IntoElement + use<> {
        let (icone, cor) = self.visual_agora(cx);
        let nome_cor = CORES.iter().find(|(v, _)| *v == cor).map_or("Personalizada", |(_, n)| *n);
        let rotulo: SharedString = format!("{} · {nome_cor}", icone.nome).into();
        let escolhido = self.icone.is_some() || self.cor.is_some();
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
            .anchor(Anchor::TopLeft)
            .offset(px(8.))
            .trigger(GatilhoChip::new(chip(false), chip(true)))
            .content(move |_, window, cx| {
                let automatico = automatico.clone();
                v_flex()
                    .gap(esp::E12)
                    .child(escolha.clone())
                    .child(div().h(px(1.)).bg(Cores::borda()))
                    .child(
                        div().flex().child(
                            botao_fantasma("projeto-icone-auto", Some(Lucide::Undo2.into()), "Automático", TamanhoBotao::Barra, window, cx)
                                .tooltip("Ícone pelo conteúdo da primeira pasta e cor pelo nome")
                                .on_click(move |_, window, cx| automatico.update(cx, |e, cx| e.usar_automatico(window, cx))),
                        ),
                    )
            })
    }

    /// A lista das pastas ligadas: a primeira é a principal (onde os chats nascem); a que sumiu
    /// fica em vermelho, com Localizar pasta…; tirar uma em que chats trabalham avisa.
    fn secao_pastas(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let adicionar = botao_fantasma("projeto-adicionar-pasta", Some(Lucide::FolderPlus.into()), "Adicionar pasta…", TamanhoBotao::Barra, window, cx)
            .on_click(cx.listener(|this, _, window, cx| this.adicionar_pastas(window, cx)));
        let nome = self.nome.read(cx).value().trim().to_string();
        let propria = self.motor.read(cx).dados.pasta_projetos().join(if nome.is_empty() { "<nome>".into() } else { dados::slug(&nome) });
        let vazia = self.pastas.is_empty().then(|| {
            div()
                .metadado()
                .text_color(Cores::tenue())
                .child(format!("Sem pasta: os arquivos ficam em {}, com git.", com_til(&propria)))
        });
        let mut grupo = GrupoDeOpcoes::new(None);
        for (i, p) in self.pastas.iter().enumerate() {
            let existe = p.caminho.is_dir();
            let titulo = if p.propria {
                "Pasta própria".to_string()
            } else {
                p.caminho.file_name().map_or_else(|| p.caminho.display().to_string(), |n| n.to_string_lossy().to_string())
            };
            let mut partes = Vec::new();
            if i == 0 {
                partes.push("principal".to_string());
            }
            partes.push(com_til(&p.caminho));
            if self.git.get(&p.caminho) == Some(&false) && existe {
                partes.push("sem git".into());
            }
            let descricao = if existe {
                div().text_color(Cores::tenue()).child(partes.join(" · "))
            } else {
                div().text_color(Cores::vermelho()).child(format!("{} · não encontrada", com_til(&p.caminho)))
            };
            let controles = h_flex()
                .gap(esp::E4)
                .when(!existe && !self.novo(), |s| {
                    s.child(
                        botao_fantasma(("projeto-localizar", i), Some(Lucide::FolderSearch.into()), "Localizar pasta…", TamanhoBotao::Barra, window, cx)
                            .on_click(cx.listener(move |this, _, window, cx| this.localizar(i, window, cx))),
                    )
                })
                .child(
                    botao_icone(("projeto-tirar-pasta", i), Lucide::X, "Tirar do projeto\nA pasta não é apagada", TamanhoBotao::Barra, window, cx)
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if i < this.pastas.len() {
                                this.pastas.remove(i);
                                cx.notify();
                            }
                        })),
                );
            grupo = grupo.linha(LinhaDeOpcao::new(("projeto-pasta", i), titulo).descricao(descricao).controle(controles));
        }
        // As pastas tiradas em que chats ainda trabalham.
        let tiradas: Vec<(PathBuf, usize)> = self
            .inicial
            .pastas
            .iter()
            .filter(|p| !self.pastas.iter().any(|x| x.caminho == **p))
            .map(|p| (p.clone(), self.chats_na_pasta(p, cx)))
            .filter(|(_, n)| *n > 0)
            .collect();
        let avisos = tiradas.into_iter().map(|(p, n)| {
            Alerta::new(
                Tom::Atencao,
                format!(
                    "{} em {}: eles continuam lá, e a escrita nela continua bloqueada para eles.",
                    contagem(n, "chat trabalha", "chats trabalham"),
                    com_til(&p),
                ),
            )
            .compacto()
        });
        let problema = self.problemas(cx).pastas;
        v_flex()
            .gap(esp::E8)
            .child(
                h_flex()
                    .gap(esp::E8)
                    .child(div().flex_1().rotulo().text_color(Cores::texto()).child("Pastas"))
                    .child(adicionar),
            )
            .children(vazia)
            .when(!self.pastas.is_empty(), |s| s.child(grupo))
            .children(avisos)
            .when_some(problema, |s, p| s.child(div().metadado().text_color(Cores::vermelho()).child(p)))
    }

    /// O Avançado: um grupo por pasta (preparar, arquivos que vão junto, rodar; sem git, como
    /// ativar) e o grupo do projeto (permissão, modelo e conta dos chats novos).
    fn avancado(&self, window: &Window, cx: &Context<Self>) -> impl IntoElement + use<> {
        let mut blocos: Vec<AnyElement> = Vec::new();
        for (i, p) in self.pastas.iter().enumerate() {
            let nome = if p.propria {
                "Pasta própria".to_string()
            } else {
                p.caminho.file_name().map_or_else(|| p.caminho.display().to_string(), |n| n.to_string_lossy().to_string())
            };
            if self.git.get(&p.caminho) == Some(&false) && p.caminho.is_dir() {
                let (janela, pasta) = (self.janela.clone(), p.caminho.clone());
                blocos.push(
                    Alerta::new(Tom::Atencao, format!("{nome} não usa git: os chats trabalham direto nela, sem cópia."))
                        .acao(
                            botao_fantasma(("projeto-ativar-git", i), Some(Lucide::GitBranch.into()), "Ativar git…", TamanhoBotao::Barra, window, cx)
                                .on_click(move |_, window, cx| {
                                    let pasta = pasta.clone();
                                    let _ = janela.update(cx, |this, cx| this.confirmar_git_na_pasta(pasta, window, cx));
                                }),
                        )
                        .into_any_element(),
                );
            }
            blocos.push(p.config.grupo(i, nome.into()).into_any_element());
        }
        let ancora = Anchor::TopRight;
        let modos: Vec<Opcao> = std::iter::once(Opcao::new("", "Padrão do Vitral").descricao("O das Configurações"))
            .chain(MODOS.iter().map(|(valor, nome, descricao)| Opcao::new(*valor, *nome).descricao(*descricao)))
            .collect();
        let modelos: Vec<Opcao> = MODELOS
            .iter()
            .enumerate()
            .map(|(i, (valor, nome))| {
                let descricao = if i == 0 { "O das Configurações" } else { DESCRICOES_DE_MODELO[i - 1] };
                Opcao::new(*valor, *nome).descricao(descricao)
            })
            .collect();
        let contas: Vec<Opcao> = std::iter::once(Opcao::new("", "Padrão do Vitral"))
            .chain(self.motor.read(cx).contas.iter().map(|c| {
                let nome = if c.email.is_empty() { c.nome.clone() } else { c.email.clone() };
                Opcao::new(c.id.clone(), nome)
            }))
            .collect();
        let eu = cx.entity().downgrade();
        let muda = move |f: fn(&mut EditorProjeto, String)| {
            let eu = eu.clone();
            move |valor: String, _: &mut Window, cx: &mut App| {
                let _ = eu.update(cx, |this, cx| {
                    f(this, valor);
                    cx.notify();
                });
            }
        };
        let projeto = GrupoDeOpcoes::new(Some("Chats novos do projeto".into()))
            .linha(LinhaDeOpcao::new("projeto-permissao", "Modo de permissão").controle(menu_escolha(
                "projeto-permissao",
                ancora,
                modos,
                &self.permissao,
                "",
                muda(|e, v| e.permissao = v),
                window,
                cx,
            )))
            .linha(LinhaDeOpcao::new("projeto-modelo", "Modelo").controle(menu_escolha(
                "projeto-modelo",
                ancora,
                modelos,
                &self.modelo,
                "",
                muda(|e, v| e.modelo = v),
                window,
                cx,
            )))
            .linha(LinhaDeOpcao::new("projeto-conta", "Conta").controle(menu_escolha(
                "projeto-conta",
                ancora,
                contas,
                &self.conta,
                "",
                muda(|e, v| e.conta = v),
                window,
                cx,
            )));
        blocos.push(projeto.into_any_element());
        v_flex().gap(esp::E16).children(blocos)
    }
}

impl Render for EditorProjeto {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A prévia e o automático acompanham o nome e a primeira pasta.
        let nome = self.nome.read(cx).value().trim().to_string();
        let chave = self.id.clone().unwrap_or_else(|| nome.clone());
        let pasta = self.pastas.first().map(|p| p.caminho.clone());
        self.escolha.update(cx, |e, _| e.contexto(&nome, &chave, pasta.as_deref()));
        let problemas = self.problemas(cx);
        let (icone, cor) = self.visual_agora(cx);
        let erro_nome = problemas.nome.clone().filter(|_| !nome.is_empty()).map(SharedString::from);
        let entrada = Input::new(&self.nome).when(erro_nome.is_some(), |i| i.border_color(Cores::vermelho()));
        let topo = h_flex()
            .items_start()
            .gap(esp::E16)
            .child(QuadradoDeProjeto::new(icone, cor, 56.))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(esp::E8)
                    .child(Campo::new("Nome", entrada).erro(erro_nome))
                    .child(div().flex().child(self.icone_e_cor(window, cx))),
            );
        let instrucoes_n = self.instrucoes.read(cx).value().chars().count();
        let ajuda_instrucoes = format!("Vão no começo de cada chat deste projeto · {instrucoes_n}/{MAX_INSTRUCOES}");
        let eu = cx.entity().downgrade();
        let alternar = move |_: &mut Window, cx: &mut App| {
            let _ = eu.update(cx, |this, cx| {
                this.avancado = !this.avancado;
                cx.notify();
            });
        };
        let id_notas = self.id.clone().unwrap_or_default();
        v_flex()
            .gap(esp::E16)
            .child(topo)
            .child(
                Campo::new("Descrição", Textarea::new(&self.descricao))
                    .opcional()
                    .ajuda("O Mestre usa isto para saber o que mandar para este projeto (vazio: uso o começo do README)."),
            )
            .child(self.secao_pastas(window, cx))
            .child(
                Campo::new("Instruções para os chats", Textarea::new(&self.instrucoes))
                    .opcional()
                    .ajuda(ajuda_instrucoes)
                    .erro(problemas.instrucoes.clone().map(SharedString::from)),
            )
            .when_some(self.notas.clone(), |s, notas| {
                s.child(
                    Campo::new("Decisões anotadas pelos chats", Textarea::new(&notas))
                        .opcional()
                        .ajuda(format!("As convenções que os chats guardam com anotar_decisao (notas/{id_notas}.md).")),
                )
            })
            .child(
                v_flex()
                    .child(CabecalhoDeSecao::new("Avançado").recolhivel(self.avancado, alternar).primeira(true))
                    .when(self.avancado, |s| s.child(div().pt(esp::E8).child(self.avancado(window, cx)))),
            )
    }
}

impl Janela {
    /// Novo projeto, vazio (o "+" do trilho, Ctrl+Shift+N, o menu do Vitral).
    pub(super) fn dialogo_criar_projeto(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_editor_de_projeto(None, String::new(), Vec::new(), false, window, cx);
    }

    /// "Criar projeto…" da sugestão do Mestre: o formulário com o nome, a descrição, as
    /// instruções, o ícone, a cor e as pastas que ele sugeriu; Criar responde ao cartão.
    pub(super) fn dialogo_projeto_da_sugestao(&mut self, cartao: u64, window: &mut Window, cx: &mut Context<Self>) {
        let sugestao = self.motor.as_ref().and_then(|m| match &m.read(cx).cartao(cartao)?.tipo {
            crate::modelo::TipoCartao::Projeto(s) => Some(s.clone()),
            _ => None,
        });
        if let Some(s) = sugestao {
            self.sugestao_no_editor = Some((cartao, s));
            self.abrir_editor_de_projeto(None, String::new(), Vec::new(), false, window, cx);
        }
    }

    /// Novo projeto com o nome já escrito ("Criar o projeto “ops”") ou com uma pasta para ligar
    /// (`vitral <pasta>`).
    pub(super) fn dialogo_projeto_novo(&mut self, nome: String, pasta: Option<PathBuf>, window: &mut Window, cx: &mut Context<Self>) {
        let nome = match (&pasta, nome.is_empty()) {
            (Some(p), true) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            _ => nome,
        };
        self.abrir_editor_de_projeto(None, nome, pasta.into_iter().collect(), false, window, cx);
    }

    /// Editar projeto (o menu do projeto, o clique duplo no nome, "Ícone e cor…").
    pub(super) fn dialogo_editar_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_editor_de_projeto(Some(projeto), String::new(), Vec::new(), false, window, cx);
    }

    /// Editar projeto já no Avançado ("Configurar projeto…", "Definir como rodar…").
    pub(super) fn dialogo_config_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        self.abrir_editor_de_projeto(Some(projeto), String::new(), Vec::new(), true, window, cx);
    }

    fn abrir_editor_de_projeto(
        &mut self,
        id: Option<String>,
        nome: String,
        pastas: Vec<PathBuf>,
        avancado: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        if let Some(id) = &id
            && motor.read(cx).projeto(id).is_none()
        {
            return;
        }
        self.foco_antes_do_dialogo(window, cx);
        let janela = cx.entity().downgrade();
        let sugestao = self.sugestao_no_editor.take();
        let da_sugestao = sugestao.is_some();
        let editor =
            cx.new(|cx| EditorProjeto::new(motor.clone(), janela.clone(), id.clone(), nome, pastas, avancado, sugestao, window, cx));
        let foco = editor.read(cx).foco(cx);
        let titulo = if id.is_some() { "Editar projeto" } else { "Novo projeto" };
        window.open_dialog(cx, move |dialogo, window, cx| {
            let e = editor.read(cx);
            let problemas = e.problemas(cx);
            let novo = e.novo();
            let motivo = problemas
                .primeiro()
                .map(str::to_string)
                .or_else(|| (!novo && !e.mudou(cx)).then(|| "Nada mudou".to_string()));
            let subtitulo = match e.id.as_deref() {
                Some(id) => Some(motor.read(cx).nome_do_projeto(id)),
                None if da_sugestao => Some("Sugerido pelo Mestre; confira antes de criar".to_string()),
                None => None,
            };
            let (no_botao, no_enter) = (editor.clone(), editor.clone());
            let principal = botao_primario("projeto-confirmar", None, if novo { "Criar" } else { "Salvar" }, TamanhoBotao::Controle, window, cx)
                .when_some(motivo, |b, motivo| b.disabled(true).opacity(0.45).tooltip(motivo))
                .on_click(move |_, window, cx| {
                    if no_botao.update(cx, |e, cx| e.confirmar(window, cx)) {
                        window.close_dialog(cx);
                    }
                });
            let mut rodape = RodapeDialogo::new().cancelar(
                botao_fantasma("projeto-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                    .on_click(|_, window, cx| window.close_dialog(cx)),
            );
            if let Some(id) = e.id.clone() {
                let (j1, j2, i1, i2) = (janela.clone(), janela.clone(), id.clone(), id);
                rodape = rodape
                    .esquerda(
                        botao_fantasma("projeto-arquivar", Some(Lucide::Archive.into()), "Arquivar projeto…", TamanhoBotao::Controle, window, cx)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = j1.update(cx, |this, cx| this.arquivar_projeto(i1.clone(), window, cx));
                            }),
                    )
                    .esquerda(
                        botao_fantasma("projeto-apagar", Some(Lucide::Trash.into()), "Apagar…", TamanhoBotao::Controle, window, cx)
                            .custom(variante_perigo_calma(cx))
                            .tooltip("Apagar projeto…\nAs pastas ligadas ficam como estão")
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let _ = j2.update(cx, |this, cx| this.perguntar_apagar(i2.clone(), window, cx));
                            }),
                    );
            }
            moldura(dialogo, largura::G, window)
                .title(topo(cabecalho(titulo, subtitulo)))
                .child(editor.clone())
                .footer(rodape.principal(principal))
                .on_ok(move |_, window, cx| no_enter.update(cx, |e, cx| e.confirmar(window, cx)))
        });
        window.focus(&foco, cx);
    }
}

#[cfg(test)]
mod testes {
    use std::path::PathBuf;

    use super::{Formulario, MAX_INSTRUCOES, Problemas, conflito_da_pasta, juntar_notas, validar_formulario};
    use crate::modelo::{PastaLigada, Projeto};

    fn projeto(id: &str, nome: &str, pastas: &[&str]) -> Projeto {
        Projeto {
            id: id.into(),
            nome: nome.into(),
            pastas: pastas.iter().map(|p| PastaLigada { caminho: PathBuf::from(p), ..Default::default() }).collect(),
            ..Default::default()
        }
    }

    #[test]
    fn validar_formulario_confere_nome_instrucoes_e_pastas() {
        let projetos = vec![projeto("a1", "Loja Ninee", &["/p/loja"]), projeto("b2", "site", &[])];
        fn ok(nome: &str) -> Formulario<'_> {
            Formulario { nome, instrucoes: "", pastas: &[] }
        }
        assert!(validar_formulario(&ok("Receitas"), &projetos, None).nenhum());
        // Repetido sem diferenciar maiúsculas nem acentos; na edição, o próprio nome serve.
        assert!(validar_formulario(&ok("loja ninée"), &projetos, None).nome.is_some());
        assert!(validar_formulario(&ok("Loja Ninee"), &projetos, Some("a1")).nenhum());
        assert_eq!(validar_formulario(&ok("Mestre"), &projetos, None).nome.as_deref(), Some("Nome reservado."));
        assert!(validar_formulario(&ok("   "), &projetos, None).nome.is_some());
        let longas = "x".repeat(MAX_INSTRUCOES + 1);
        let f = Formulario { nome: "Receitas", instrucoes: &longas, pastas: &[] };
        assert!(validar_formulario(&f, &projetos, None).instrucoes.is_some());
        let pastas = vec![PathBuf::from("/p/a"), PathBuf::from("/p/b"), PathBuf::from("/p/a")];
        let f = Formulario { nome: "Receitas", instrucoes: "", pastas: &pastas };
        let p = validar_formulario(&f, &projetos, None);
        assert_eq!(p, Problemas { pastas: p.pastas.clone(), ..Default::default() });
        assert!(p.pastas.is_some());
    }

    #[test]
    fn ligar_pasta_de_outro_projeto_ou_aninhada_pede_confirmacao() {
        let projetos = vec![projeto("a1", "loja", &["/p/loja"]), projeto("b2", "site", &["/p/site"])];
        assert!(conflito_da_pasta(&PathBuf::from("/p/outra"), &projetos, None, &[]).is_none());
        assert!(conflito_da_pasta(&PathBuf::from("/p/loja"), &projetos, None, &[]).unwrap().contains("loja"));
        assert!(conflito_da_pasta(&PathBuf::from("/p/loja/api"), &projetos, None, &[]).unwrap().contains("dentro"));
        assert!(conflito_da_pasta(&PathBuf::from("/p"), &projetos, None, &[]).unwrap().contains("contém"));
        // A pasta do próprio projeto (já na lista) não conta como de outro, mas uma dentro dela sim.
        let lista = vec![PathBuf::from("/p/loja")];
        assert!(conflito_da_pasta(&PathBuf::from("/p/loja/docs"), &projetos, Some("a1"), &lista).unwrap().contains("dentro"));
        assert!(conflito_da_pasta(&PathBuf::from("/q"), &projetos, Some("a1"), &lista).is_none());
    }

    #[test]
    fn notas_anotadas_com_o_formulario_aberto_nao_se_perdem() {
        let inicial = "- usar pt-BR\n";
        assert_eq!(juntar_notas(inicial, inicial, "- usar pt-BR\n- testes com make\n"), "- usar pt-BR\n- testes com make\n");
        let disco = "- usar pt-BR\n- porta 8000\n";
        assert_eq!(juntar_notas(inicial, disco, "- usar português\n"), "- usar português\n- porta 8000\n");
        // A linha que você também escreveu não repete.
        assert_eq!(juntar_notas(inicial, disco, "- usar pt-BR\n- porta 8000\n"), "- usar pt-BR\n- porta 8000\n");
    }
}
