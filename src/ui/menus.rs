//! Menus compartilhados: um construtor por objeto (projeto, cópia de uma tarefa, conversa), o
//! mesmo no clique direito e no "…". Os grupos vêm sempre nesta ordem: abrir ou ir, criar ou
//! editar, organizar, copiar, fora do app, configurar e, por último, o perigoso.

use std::path::{Path, PathBuf};

use gpui_kit::component::{
    IconName, WindowExt as _,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Simbolo, Tom, aviso, cabecalho_objeto},
    icone_projeto, lateral,
};
use crate::{
    modelo::Papel,
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, r},
};

/// O comando de rodar da pasta (em Configurar projeto), quando há.
fn rodar_do(motor: &Motor, projeto: &str, pasta: &Path) -> Option<String> {
    Some(motor.config_da_pasta(projeto, pasta).rodar.trim().to_string()).filter(|r| !r.is_empty())
}

/// Item de rodar em duas linhas: o título e o comando em mono ("$ npm run dev").
fn item_rodar(titulo: &'static str, comando: String) -> PopupMenuItem {
    PopupMenuItem::element(move |_, _| {
        div()
            .flex()
            .flex_col()
            .min_w_0()
            .py(r(4.))
            .child(div().truncate().child(titulo))
            .child(
                div()
                    .truncate()
                    .mono_pequeno()
                    .text_color(Cores::codigo())
                    .child(format!("$ {comando}")),
            )
    })
    .icon(Lucide::MonitorPlay)
}

/// Item que chama um método da janela.
fn acao(
    rotulo: impl Into<SharedString>,
    icone: impl Into<gpui_kit::component::Icon>,
    janela: &WeakEntity<Janela>,
    f: impl Fn(&mut Janela, &mut Window, &mut Context<Janela>) + 'static,
) -> PopupMenuItem {
    let janela = janela.clone();
    PopupMenuItem::new(rotulo).icon(icone).on_click(move |_, window, cx| {
        let _ = janela.update(cx, |this, cx| f(this, window, cx));
    })
}

/// Abrir no VS Code, no terminal e no gerenciador de arquivos (com o `ExternalLink` à direita).
fn itens_fora_do_app(menu: PopupMenu, pasta: PathBuf, porta: Option<u64>, gerenciador: bool) -> PopupMenu {
    let (p1, p2, p3) = (pasta.clone(), pasta.clone(), pasta);
    menu.item(
        PopupMenuItem::new("Abrir no VS Code")
            .icon(Lucide::CodeXml)
            .external()
            .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&p1)),
    )
    .item(
        PopupMenuItem::new("Abrir no terminal")
            .icon(IconName::SquareTerminal)
            .external()
            .on_click(move |_, _, _| crate::sistema::abrir_terminal(&p2, porta, None)),
    )
    .when(gerenciador, |menu| {
        menu.item(
            PopupMenuItem::new("Abrir no gerenciador de arquivos")
                .icon(IconName::FolderOpen)
                .external()
                .on_click(move |_, _, cx| crate::sistema::abrir_pasta(&p3, cx)),
        )
    })
}

/// Itens do projeto: editar e um chat novo; por pasta ligada, rodar e abrir fora do app (a que
/// sumiu, Localizar pasta…); no fim, arquivar e apagar. Com `cabecalho`, o quadrado do projeto, o
/// nome e a pasta principal em cima.
pub(super) fn itens_do_projeto(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: &Entity<Motor>,
    projeto: &str,
    cabecalho: bool,
    _window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let m = motor.read(cx);
    let (icone, cor) = icone_projeto::visual_do(m, projeto);
    let arquivado = m.projeto_arquivado(projeto);
    let exibido = m.nome_do_projeto(projeto);
    let pastas: Vec<(PathBuf, String, Option<String>)> = m
        .pastas_do(projeto)
        .iter()
        .map(|p| {
            let nome = if p.propria {
                "Pasta própria".to_string()
            } else {
                p.caminho.file_name().map_or_else(|| p.caminho.display().to_string(), |n| n.to_string_lossy().to_string())
            };
            (p.caminho.clone(), nome, rodar_do(m, projeto, &p.caminho))
        })
        .collect();
    let caminho = match pastas.first() {
        Some((p, ..)) if p.is_dir() => p.display().to_string(),
        Some((p, ..)) => format!("{} (não encontrada)", p.display()),
        None => "sem pasta".into(),
    };
    let varias = pastas.len() > 1;
    let nome = projeto.to_string();
    let (n1, n2, n3, n4) = (nome.clone(), nome.clone(), nome.clone(), nome.clone());
    let mut menu = menu
        .when(cabecalho, |menu| {
            menu.item(cabecalho_objeto(Simbolo::Projeto(icone, cor), exibido.clone(), Some(caminho.into())))
                .separator()
        })
        .item(acao("Editar projeto…", Lucide::Pencil, &janela, move |this, window, cx| {
            this.dialogo_editar_projeto(n1.clone(), window, cx)
        }))
        .when(!arquivado, |menu| {
            menu.item(acao("Novo chat neste projeto", Lucide::MessageSquarePlus, &janela, move |this, window, cx| {
                this.novo_chat_em(&n2, window, cx)
            }))
        });
    for (i, (pasta, nome_pasta, rodar)) in pastas.into_iter().enumerate() {
        menu = menu.separator();
        if varias {
            menu = menu.label(nome_pasta.clone());
        }
        if !pasta.is_dir() {
            let projeto = nome.clone();
            let rotulo = if varias { "Localizar pasta…".to_string() } else { format!("Localizar {nome_pasta}…") };
            menu = menu.item(acao(rotulo, Lucide::FolderSearch, &janela, move |this, window, cx| {
                this.localizar_pasta(projeto.clone(), i, window, cx)
            }));
            continue;
        }
        menu = match rodar {
            Some(comando) => {
                let (lugar, rodar) = (pasta.clone(), comando.clone());
                menu.item(
                    item_rodar("Rodar o projeto", comando)
                        .on_click(move |_, _, _| crate::sistema::abrir_terminal(&lugar, None, Some(&rodar))),
                )
            }
            None => {
                let projeto = nome.clone();
                menu.item(acao("Definir como rodar…", Lucide::MonitorPlay, &janela, move |this, window, cx| {
                    this.dialogo_config_projeto(projeto.clone(), window, cx)
                }))
            }
        };
        menu = itens_fora_do_app(menu, pasta, None, true);
    }
    menu.separator()
        .item(acao(
            if arquivado { "Desarquivar projeto" } else { "Arquivar projeto…" },
            if arquivado { Lucide::ArchiveRestore } else { Lucide::Archive },
            &janela,
            move |this, window, cx| {
                if arquivado {
                    this.desarquivar_projeto(n3.clone(), cx)
                } else {
                    this.arquivar_projeto(n3.clone(), window, cx)
                }
            },
        ))
        .item(
            acao("Apagar projeto…", Lucide::Trash, &janela, move |this, window, cx| {
                this.perguntar_apagar(n4.clone(), window, cx)
            })
            .danger(true),
        )
}

/// Itens da cópia isolada de uma tarefa: rodar e abrir na cópia (com o `VITRAL_PORT` dela),
/// copiar o caminho, abrir o trabalhador neste painel, configurar e, quando a cópia já não vai
/// ser juntada, apagar.
pub(super) fn itens_da_copia(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: &Entity<Motor>,
    tarefa: u64,
    cabecalho: bool,
    _window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let m = motor.read(cx);
    let Some(t) = m.tarefa(tarefa) else {
        return menu;
    };
    let origem = m.origem_da_tarefa(t);
    let pasta = t.worktree.clone().unwrap_or_else(|| origem.clone());
    let rodar = rodar_do(m, &t.projeto, &origem);
    let porta = crate::motor::porta_de(t);
    let pode_apagar = m.pode_apagar_copia(tarefa);
    let (projeto, trabalhador, branch) = (t.projeto.clone(), t.trabalhador.clone(), t.branch.clone());
    let caminho = pasta.display().to_string();
    menu.when(cabecalho, |menu| {
        menu.item(cabecalho_objeto(
            Simbolo::icone(Lucide::Wrench),
            format!("Cópia do chat {tarefa}"),
            branch.map(Into::into),
        ))
        .separator()
    })
    .when_some(rodar, |menu, comando| {
        let pasta = pasta.clone();
        let rodar = comando.clone();
        menu.item(
            item_rodar("Rodar na cópia", comando)
                .on_click(move |_, _, _| crate::sistema::abrir_terminal(&pasta, Some(porta), Some(&rodar))),
        )
    })
    .map(|menu| {
        let (p1, p2) = (pasta.clone(), pasta.clone());
        menu.item(
            PopupMenuItem::new("Abrir no terminal")
                .icon(IconName::SquareTerminal)
                .external()
                .on_click(move |_, _, _| crate::sistema::abrir_terminal(&p1, Some(porta), None)),
        )
        .item(
            PopupMenuItem::new("Abrir no VS Code")
                .icon(Lucide::CodeXml)
                .external()
                .on_click(move |_, _, _| crate::sistema::abrir_no_editor(&p2)),
        )
    })
    .item(PopupMenuItem::new("Copiar o caminho").icon(IconName::Copy).on_click(
        move |_, window, cx| {
            cx.write_to_clipboard(ClipboardItem::new_string(caminho.clone()));
            window.push_notification("Caminho copiado.", cx);
        },
    ))
    .separator()
    .item(acao("Abrir neste painel", IconName::Replace, &janela, move |this, window, cx| {
        this.selecionar(Some(trabalhador.clone()), window, cx)
    }))
    .item(acao("Configurar projeto…", IconName::Settings, &janela, move |this, window, cx| {
        this.dialogo_config_projeto(projeto.clone(), window, cx)
    }))
    .when(pode_apagar, |menu| {
        menu.separator().item(
            acao("Apagar a cópia…", Lucide::Trash, &janela, move |this, window, cx| {
                this.perguntar_limpeza(tarefa, window, cx)
            })
            .danger(true),
        )
    })
}

/// A parte mínima do menu da conversa (F33): abrir ao lado, renomear, fixar, arquivar e abrir a
/// pasta dela fora do app. Exportar e continuar no terminal entram com o F33 inteiro. Sem
/// `ao_lado` (o "⋯" do próprio painel, que já tem "Nova conversa ao lado"), começa no renomear.
pub(super) fn menu_conversa(
    menu: PopupMenu,
    janela: WeakEntity<Janela>,
    motor: &Entity<Motor>,
    chat: &str,
    ao_lado: bool,
    _window: &mut Window,
    cx: &mut App,
) -> PopupMenu {
    let Some(c) = motor.read(cx).chat(chat) else {
        return menu;
    };
    // O Mestre não se renomeia, não se fixa, não se move nem se arquiva: fica a memória dele.
    if c.papel == Papel::Mestre {
        let cabe = janela.upgrade().is_some_and(|j| {
            let j = j.read(cx);
            j.cabe_outro_painel() && !j.aberta_em_painel(chat)
        });
        let id = chat.to_string();
        return menu
            .when(ao_lado, |menu| {
                menu.item(
                    acao("Abrir ao lado", Lucide::Columns2, &janela, move |this, window, cx| {
                        this.abrir_ao_lado(id.clone(), None, window, cx)
                    })
                    .disabled(!cabe),
                )
                .separator()
            })
            .item(acao("Memória do Mestre…", Lucide::Brain, &janela, |this, window, cx| this.dialogo_memoria(window, cx)));
    }
    let (fixado, arquivado, pasta) = (c.fixado, c.arquivado, c.cwd.clone());
    // Arquivada com o projeto: quem volta é o projeto.
    let do_arquivado = (!arquivado && motor.read(cx).projeto_arquivado(&c.projeto))
        .then(|| (c.projeto.clone(), motor.read(cx).nome_do_projeto(&c.projeto)));
    let porta = c
        .tarefa
        .filter(|_| c.papel == Papel::Trabalhador)
        .and_then(|t| motor.read(cx).tarefa(t))
        .map(crate::motor::porta_de);
    let cabe = janela
        .upgrade()
        .map(|j| {
            let j = j.read(cx);
            j.cabe_outro_painel() && !j.aberta_em_painel(chat)
        })
        .unwrap_or(false);
    // O Mestre não tem projeto, e o ajudante vai junto com o chat dele.
    let movel = c.papel != Papel::Mestre && !(c.papel == Papel::Trabalhador && c.pai.is_some());
    let id = chat.to_string();
    let (i1, i2, i3, i4, i5) = (id.clone(), id.clone(), id.clone(), id.clone(), id);
    let (m1, m2) = (motor.clone(), motor.clone());
    menu.when(ao_lado, |menu| {
        menu.item(
            acao("Abrir ao lado", Lucide::Columns2, &janela, move |this, window, cx| {
                this.abrir_ao_lado(i1.clone(), None, window, cx)
            })
            .disabled(!cabe),
        )
        .separator()
    })
    .item(acao("Renomear…", Lucide::Pencil, &janela, move |this, window, cx| {
        this.dialogo_renomear(i2.clone(), window, cx)
    }))
    .when(movel, |menu| {
        menu.item(acao("Mover para projeto…", Lucide::FolderInput, &janela, move |this, window, cx| {
            this.dialogo_mover_chat(i5.clone(), window, cx)
        }))
    })
    .separator()
    .item(
        PopupMenuItem::new(if fixado { "Desafixar" } else { "Fixar" })
            .icon(if fixado { Lucide::PinOff } else { Lucide::Pin })
            .on_click(move |_, _, cx| m1.update(cx, |m, cx| m.alternar_fixado(&i3, cx))),
    )
    .map(|menu| match do_arquivado {
        Some((projeto, nome)) => menu.item(acao(
            format!("Desarquivar o projeto {nome}"),
            Lucide::ArchiveRestore,
            &janela,
            move |this, _, cx| this.desarquivar_projeto(projeto.clone(), cx),
        )),
        None => menu.item(acao(
            if arquivado { "Desarquivar" } else { "Arquivar" },
            if arquivado { Lucide::ArchiveRestore } else { Lucide::Archive },
            &janela,
            move |this, window, cx| {
                if arquivado {
                    // Desarquivar não avisa.
                    m2.update(cx, |m, cx| m.alternar_arquivado(&i4, cx));
                    return;
                }
                // O chat vazio sai em vez de arquivar; arquivar se desfaz no aviso.
                if m2.update(cx, |m, cx| m.arquivar_chat(&i4, cx)) {
                    this.largar_chats(std::slice::from_ref(&i4), true, window, cx);
                    window.push_notification(aviso(Tom::Info, "Chat vazio apagado"), cx);
                    return;
                }
                lateral::avisar_arquivadas(m2.clone(), vec![i4.clone()], "Conversa arquivada".into(), window, cx);
                // Só pergunta ao arquivar um coordenador parado com cópias sobrando.
                this.oferecer_limpeza_ao_arquivar(&i4, window, cx)
            },
        )),
    })
    .separator()
    .map(|menu| itens_fora_do_app(menu, pasta, porta, false))
}
