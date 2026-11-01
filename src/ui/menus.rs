//! Menus compartilhados: um construtor por objeto (projeto, cópia de uma tarefa, conversa), o
//! mesmo no clique direito e no "…". Os grupos vêm sempre nesta ordem: abrir ou ir, criar ou
//! editar, organizar, copiar, fora do app, configurar e, por último, o perigoso.

use std::path::PathBuf;

use gpui_kit::component::{
    IconName, WindowExt as _,
    menu::{PopupMenu, PopupMenuItem},
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Simbolo, cabecalho_objeto},
    icone_projeto, lateral,
};
use crate::{
    modelo::Papel,
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, r},
};

/// O comando de rodar do projeto (em Configurar projeto), quando há.
fn rodar_do(motor: &Motor, projeto: &str) -> Option<String> {
    motor
        .config
        .projetos
        .get(projeto)
        .map(|p| p.rodar.trim().to_string())
        .filter(|r| !r.is_empty())
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
fn itens_fora_do_app(menu: PopupMenu, pasta: PathBuf, tarefa: Option<u64>, gerenciador: bool) -> PopupMenu {
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
            .on_click(move |_, _, _| crate::sistema::abrir_terminal(&p2, tarefa, None)),
    )
    .when(gerenciador, |menu| {
        menu.item(
            PopupMenuItem::new("Abrir no gerenciador de arquivos")
                .icon(IconName::FolderOpen)
                .external()
                .on_click(move |_, _, cx| cx.open_url(&format!("file://{}", p3.display()))),
        )
    })
}

/// Itens do projeto: rodar, abrir fora do app, ícone e cor, configurar e, no fim, arquivar e
/// mover para a lixeira. Com `cabecalho`, o quadrado do projeto, o nome e o caminho em cima.
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
    let pasta = m.caminho_projeto(projeto);
    let rodar = rodar_do(m, projeto);
    let (icone, cor) = icone_projeto::visual(m, projeto);
    let escolhido = m.config.projetos.get(projeto).is_some_and(|c| !c.icone.is_empty() || !c.cor.is_empty());
    let arquivado = m.projeto_arquivado(projeto);
    let nome = projeto.to_string();
    let (n1, n2, n3, n4) = (nome.clone(), nome.clone(), nome.clone(), nome.clone());
    let (n5, n6) = (nome.clone(), nome.clone());
    let motor = motor.clone();
    menu.when(cabecalho, |menu| {
        menu.item(cabecalho_objeto(
            Simbolo::Projeto(icone, cor),
            nome.clone(),
            Some(pasta.display().to_string().into()),
        ))
        .separator()
    })
    .item(match rodar {
        Some(comando) => {
            let pasta = pasta.clone();
            let rodar = comando.clone();
            item_rodar("Rodar o projeto", comando)
                .on_click(move |_, _, _| crate::sistema::abrir_terminal(&pasta, None, Some(&rodar)))
        }
        None => acao("Definir como rodar…", Lucide::MonitorPlay, &janela, move |this, window, cx| {
            this.dialogo_config_projeto(n1.clone(), window, cx)
        }),
    })
    .separator()
    .map(|menu| itens_fora_do_app(menu, pasta, None, true))
    .separator()
    .item(acao("Ícone e cor…", IconName::Palette, &janela, move |this, window, cx| {
        this.dialogo_icone_projeto(n2.clone(), window, cx)
    }))
    .when(escolhido, |menu| {
        menu.item(
            PopupMenuItem::new("Usar automático")
                .icon(Lucide::Undo2)
                .on_click(move |_, _, cx| icone_projeto::usar_automatico(&motor, &n4, cx)),
        )
    })
    .item(acao("Configurar projeto…", IconName::Settings, &janela, move |this, window, cx| {
        this.dialogo_config_projeto(n3.clone(), window, cx)
    }))
    .separator()
    .item(acao(
        if arquivado { "Desarquivar projeto" } else { "Arquivar projeto" },
        if arquivado { Lucide::ArchiveRestore } else { Lucide::Archive },
        &janela,
        move |this, window, cx| {
            if arquivado {
                this.desarquivar_projeto(n5.clone(), cx)
            } else {
                this.arquivar_projeto(n5.clone(), window, cx)
            }
        },
    ))
    .item(
        acao("Mover para a lixeira…", Lucide::Trash, &janela, move |this, window, cx| {
            this.perguntar_lixeira(n6.clone(), window, cx)
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
    let pasta = t.worktree.clone().unwrap_or_else(|| m.caminho_projeto(&t.projeto));
    let rodar = rodar_do(m, &t.projeto);
    let pode_apagar = m.pode_apagar_copia(tarefa);
    let (projeto, trabalhador, branch) = (t.projeto.clone(), t.trabalhador.clone(), t.branch.clone());
    let caminho = pasta.display().to_string();
    menu.when(cabecalho, |menu| {
        menu.item(cabecalho_objeto(
            Simbolo::icone(Lucide::Wrench),
            format!("Cópia da tarefa #{tarefa}"),
            branch.map(Into::into),
        ))
        .separator()
    })
    .when_some(rodar, |menu, comando| {
        let pasta = pasta.clone();
        let rodar = comando.clone();
        menu.item(
            item_rodar("Rodar na cópia", comando)
                .on_click(move |_, _, _| crate::sistema::abrir_terminal(&pasta, Some(tarefa), Some(&rodar))),
        )
    })
    .map(|menu| {
        let (p1, p2) = (pasta.clone(), pasta.clone());
        menu.item(
            PopupMenuItem::new("Abrir no terminal")
                .icon(IconName::SquareTerminal)
                .external()
                .on_click(move |_, _, _| crate::sistema::abrir_terminal(&p1, Some(tarefa), None)),
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
    let (fixado, arquivado, pasta) = (c.fixado, c.arquivado, c.cwd.clone());
    // Arquivada com o projeto: quem volta é o projeto.
    let do_arquivado = (!arquivado && motor.read(cx).projeto_arquivado(&c.projeto)).then(|| c.projeto.clone());
    let tarefa = c.tarefa.filter(|_| c.papel == Papel::Trabalhador);
    let cabe = janela
        .upgrade()
        .map(|j| {
            let j = j.read(cx);
            j.cabe_outro_painel() && !j.aberta_em_painel(chat)
        })
        .unwrap_or(false);
    let id = chat.to_string();
    let (i1, i2, i3, i4) = (id.clone(), id.clone(), id.clone(), id);
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
    .separator()
    .item(
        PopupMenuItem::new(if fixado { "Desafixar" } else { "Fixar" })
            .icon(if fixado { Lucide::PinOff } else { Lucide::Pin })
            .on_click(move |_, _, cx| m1.update(cx, |m, cx| m.alternar_fixado(&i3, cx))),
    )
    .map(|menu| match do_arquivado {
        Some(projeto) => menu.item(acao(
            format!("Desarquivar o projeto {projeto}"),
            Lucide::ArchiveRestore,
            &janela,
            move |this, _, cx| this.desarquivar_projeto(projeto.clone(), cx),
        )),
        None => menu.item(acao(
            if arquivado { "Desarquivar" } else { "Arquivar" },
            if arquivado { Lucide::ArchiveRestore } else { Lucide::Archive },
            &janela,
            move |this, window, cx| {
                m2.update(cx, |m, cx| m.alternar_arquivado(&i4, cx));
                // Arquivar se desfaz no aviso; desarquivar não avisa.
                if !arquivado {
                    lateral::avisar_arquivadas(m2.clone(), vec![i4.clone()], "Conversa arquivada".into(), window, cx);
                }
                // Só pergunta ao arquivar um coordenador parado com cópias sobrando.
                this.oferecer_limpeza_ao_arquivar(&i4, window, cx)
            },
        )),
    })
    .separator()
    .map(|menu| itens_fora_do_app(menu, pasta, tarefa, false))
}
