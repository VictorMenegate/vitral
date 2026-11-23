//! Perfis de agente nas Configurações (`crate::perfis`): o grupo com a lista e o diálogo de criar
//! e editar. Um perfil guarda quem responde, o modelo, o modo e o esforço com um nome; ele aparece
//! no alto do seletor de modelo da caixa, e escolher um deixa o chat como ele diz.

use gpui_kit::component::{
    Disableable as _, WindowExt as _,
    input::{Input, InputState},
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{Opcao, menu_escolha};
use crate::{
    agente_codex,
    modelo::Agente,
    motor::Motor,
    perfis::{self, Perfil},
    tema::{Cores, Lucide, Tipo as _, esp, r},
    ui::{
        componentes::{
            CabecalhoDialogo, Campo, GrupoDeOpcoes, LinhaDeOpcao, Marca, RodapeDialogo, Simbolo, TamanhoBotao, botao_fantasma,
            botao_icone, botao_perigo, botao_primario, icone_do_modo, largura,
        },
        composer::{DESCRICOES_DE_MODELO, ESFORCOS, MODELOS, MODOS, nome_curto_modelo},
        dialogos,
    },
};

/// "Opus", "Sonnet 5", "GPT-5.1 Codex": o nome de mostrar do modelo de um perfil.
fn nome_do_modelo(perfil: &Perfil) -> Option<String> {
    let modelo = perfil.modelo.trim();
    if modelo.is_empty() {
        return None;
    }
    Some(match perfil.agente {
        Agente::Codex => agente_codex::nome_do_modelo(modelo),
        Agente::Claude => MODELOS.iter().find(|(v, _)| *v == modelo).map_or_else(|| nome_curto_modelo(modelo), |(_, nome)| nome.to_string()),
    })
}

/// "Claude · Opus · Planejar · esforço alto": o que o perfil escolhe, numa linha. O que ele deixa
/// no padrão não aparece, menos o modelo, que sempre diz alguma coisa.
pub(in crate::ui) fn resumo(perfil: &Perfil) -> String {
    let mut partes = vec![perfil.agente.nome().to_string(), nome_do_modelo(perfil).unwrap_or_else(|| "modelo padrão".into())];
    if let Some((_, nome, _)) = MODOS.iter().find(|(v, ..)| *v == perfil.modo) {
        partes.push(nome.to_string());
    }
    if let Some((_, nome)) = ESFORCOS.iter().find(|(v, _)| !v.is_empty() && *v == perfil.esforco) {
        partes.push(format!("esforço {}", nome.to_lowercase()));
    }
    partes.join(" · ")
}

/// A segunda linha do perfil na lista e no seletor: a nota de quando usar, ou o resumo.
pub(in crate::ui) fn segunda_linha(perfil: &Perfil) -> String {
    if perfil.nota.trim().is_empty() { resumo(perfil) } else { perfil.nota.trim().to_string() }
}

fn marca(agente: Agente) -> Simbolo {
    Simbolo::Marca(if agente == Agente::Codex { Marca::Codex } else { Marca::Claude })
}

/// O grupo **Perfis de agente**: cada perfil numa linha (o resumo embaixo do nome, a nota na
/// dica), com subir, descer e editar, e o Novo perfil no fim.
pub(super) fn grupo(motor: &Entity<Motor>, window: &Window, cx: &App) -> GrupoDeOpcoes {
    let lista = motor.read(cx).config.perfis.clone();
    let total = lista.len();
    let mut grupo = GrupoDeOpcoes::new(Some("Perfis de agente".into()));
    for (i, perfil) in lista.iter().enumerate() {
        let id = perfil.id.clone();
        let mover = |para: i32, icone: Lucide, dica: &'static str, desligado: bool| {
            let (motor, id) = (motor.clone(), id.clone());
            botao_icone(SharedString::from(format!("cfg-perfil-{dica}-{id}")), icone, dica, TamanhoBotao::Barra, window, cx)
                .disabled(desligado)
                .when(desligado, |b| b.opacity(crate::tema::opacidade::DESABILITADO))
                .on_click(move |_, _, cx| motor.update(cx, |m, cx| m.mudar_config(|c| perfis::mover(&mut c.perfis, &id, para), cx)))
        };
        let (motor_e, perfil_e) = (motor.clone(), perfil.clone());
        let editar = botao_fantasma(SharedString::from(format!("cfg-perfil-editar-{id}")), None, "Editar…", TamanhoBotao::Barra, window, cx)
            .on_click(move |_, window, cx| abrir_edicao(motor_e.clone(), Some(perfil_e.clone()), window, cx));
        let botoes = div()
            .flex()
            .items_center()
            .gap(r(2.))
            .when(total > 1, |d| {
                d.child(mover(-1, Lucide::ArrowUp, "Subir", i == 0)).child(mover(1, Lucide::ArrowDown, "Descer", i + 1 == total))
            })
            .child(div().pl(r(6.)).child(editar));
        // Com nota, ela é a descrição e o resumo vai embaixo, mais apagado; sem nota, só o resumo.
        let descricao = v_flex()
            .when(!perfil.nota.trim().is_empty(), |d| d.child(div().line_clamp(2).child(perfil.nota.trim().to_string())))
            .child(div().text_color(Cores::apagado()).child(resumo(perfil)));
        grupo = grupo.linha(
            LinhaDeOpcao::new(SharedString::from(format!("cfg-perfil-{id}")), perfil.nome.clone())
                .marca(marca(perfil.agente))
                .descricao(descricao)
                .controle(botoes),
        );
    }
    let motor_n = motor.clone();
    let novo = botao_fantasma("cfg-perfil-novo", Some(Lucide::Plus.into()), "Novo perfil…", TamanhoBotao::Barra, window, cx)
        .on_click(move |_, window, cx| abrir_edicao(motor_n.clone(), None, window, cx));
    grupo.linha(
        LinhaDeOpcao::new("cfg-perfis", if total == 0 { "Nenhum perfil ainda" } else { "Outro perfil" })
            .descricao(if total == 0 {
                "Um perfil guarda quem responde, o modelo, o modo e o esforço com um nome. Ele aparece no alto do seletor de modelo da caixa."
            } else {
                "Os perfis aparecem no alto do seletor de modelo, nesta ordem."
            })
            .controle(novo),
    )
}

/// O que o diálogo de edição guarda entre um desenho e outro (o nome e a nota moram nos campos).
struct Rascunho {
    perfil: Perfil,
    /// Já tentou salvar: o "Dê um nome" só aparece depois disso.
    tentou: bool,
    /// O Excluir pede o segundo clique.
    excluindo: bool,
}

fn opcoes_de_modelo(agente: Agente, escolhido: &str) -> Vec<Opcao> {
    let mut opcoes = Vec::new();
    match agente {
        Agente::Claude => {
            for (i, (valor, nome)) in MODELOS.iter().enumerate() {
                let descricao = if i == 0 { "O das Configurações, em Chats" } else { DESCRICOES_DE_MODELO[i - 1] };
                opcoes.push(Opcao::new(*valor, *nome).descricao(descricao).simbolo(Simbolo::Marca(Marca::Claude)));
            }
        }
        Agente::Codex => {
            opcoes.push(Opcao::new("", "Padrão do Codex"));
            opcoes.extend(agente_codex::modelos().into_iter().map(|m| Opcao::new(m.slug, m.nome)));
        }
    }
    // Um modelo guardado que saiu da lista (versão exata, catálogo que mudou) continua à vista.
    if !escolhido.is_empty() && !opcoes.iter().any(|o| o.valor == escolhido) {
        let nome = match agente {
            Agente::Claude => nome_curto_modelo(escolhido),
            Agente::Codex => agente_codex::nome_do_modelo(escolhido),
        };
        opcoes.push(Opcao::new(escolhido, nome));
    }
    opcoes
}

/// Abre o diálogo de um perfil: novo (`None`) ou o que existe. Salvar grava no `config.json`.
pub(super) fn abrir_edicao(motor: Entity<Motor>, existente: Option<Perfil>, window: &mut Window, cx: &mut App) {
    let editando = existente.is_some();
    let inicial = existente.unwrap_or_else(|| Perfil { agente: motor.read(cx).config.agente_padrao, ..Perfil::default() });
    let nome = cx.new(|cx| InputState::new(window, cx).placeholder("Planejar, Revisor, Rápido…").default_value(inicial.nome.clone()));
    let nota = cx.new(|cx| {
        InputState::new(window, cx).placeholder("Antes de uma mudança grande, para rever um diff…").default_value(inicial.nota.clone())
    });
    let rascunho = cx.new(|_| Rascunho { perfil: inicial, tentou: false, excluindo: false });
    let foco = nome.read(cx).focus_handle(cx);
    window.open_dialog(cx, move |dialogo, window, cx| {
        let estreito = dialogos::estreito(window, largura::M);
        let ancora = if estreito { Anchor::TopLeft } else { Anchor::TopRight };
        let (perfil, tentou, excluindo) = {
            let r = rascunho.read(cx);
            (r.perfil.clone(), r.tentou, r.excluindo)
        };
        let completo = Perfil { nome: nome.read(cx).value().to_string(), nota: nota.read(cx).value().to_string(), ..perfil.clone() };
        let erro = perfis::erro(&completo, &motor.read(cx).config.perfis);
        // O nome vazio só vira erro depois de tentar salvar; o repetido, na hora.
        let erro_a_vista = erro.filter(|e| tentou || !e.starts_with("Dê")).map(SharedString::from);
        let mudar = |f: fn(&mut Perfil, String)| {
            let rascunho = rascunho.clone();
            move |valor: String, _: &mut Window, cx: &mut App| {
                rascunho.update(cx, |r, _| f(&mut r.perfil, valor));
                cx.refresh_windows();
            }
        };
        let com = |linha: LinhaDeOpcao, controle: AnyElement| if estreito { linha.campo(controle) } else { linha.controle(controle) };
        let agentes = vec![
            Opcao::new("claude", "Claude").descricao("Claude Code").simbolo(Simbolo::Marca(Marca::Claude)),
            Opcao::new("codex", "Codex")
                .descricao(if agente_codex::instalado() { "Codex, da OpenAI" } else { "Não achei o Codex neste computador" })
                .simbolo(Simbolo::Marca(Marca::Codex)),
        ];
        let mut modos = vec![Opcao::new("", "O das Configurações").descricao("O modo de permissão padrão, em Chats")];
        modos.extend(MODOS.iter().map(|(valor, nome, descricao)| {
            let (icone, cor) = icone_do_modo(valor);
            Opcao::new(*valor, *nome).descricao(*descricao).simbolo(Simbolo::Icone(icone, Some(cor)))
        }));
        let esforcos = ESFORCOS.iter().map(|(valor, nome)| Opcao::new(*valor, if valor.is_empty() { "Automático" } else { nome })).collect();
        let escolhas = GrupoDeOpcoes::new(None)
            .linha(com(
                LinhaDeOpcao::new("perfil-agente", "Quem responde"),
                menu_escolha(
                    "perfil-agente",
                    ancora,
                    agentes,
                    if perfil.agente == Agente::Codex { "codex" } else { "claude" },
                    "",
                    mudar(|p, v| {
                        let agente = if v == "codex" { Agente::Codex } else { Agente::Claude };
                        // O modelo de um não serve no outro.
                        if p.agente != agente {
                            (p.agente, p.modelo) = (agente, String::new());
                        }
                    }),
                    window,
                    cx,
                ),
            ))
            .linha(com(
                LinhaDeOpcao::new("perfil-modelo", "Modelo"),
                menu_escolha("perfil-modelo", ancora, opcoes_de_modelo(perfil.agente, &perfil.modelo), &perfil.modelo, "", mudar(|p, v| p.modelo = v), window, cx),
            ))
            .linha(com(
                LinhaDeOpcao::new("perfil-modo", "Modo de permissão"),
                menu_escolha("perfil-modo", ancora, modos, &perfil.modo, "", mudar(|p, v| p.modo = v), window, cx),
            ))
            .linha(com(
                LinhaDeOpcao::new("perfil-esforco", "Esforço"),
                menu_escolha("perfil-esforco", ancora, esforcos, &perfil.esforco, "", mudar(|p, v| p.esforco = v), window, cx),
            ));
        let salvar = {
            let (motor, rascunho, nome, nota) = (motor.clone(), rascunho.clone(), nome.clone(), nota.clone());
            move |window: &mut Window, cx: &mut App| {
                let completo = Perfil {
                    nome: nome.read(cx).value().to_string(),
                    nota: nota.read(cx).value().to_string(),
                    ..rascunho.read(cx).perfil.clone()
                };
                if perfis::erro(&completo, &motor.read(cx).config.perfis).is_some() {
                    rascunho.update(cx, |r, _| r.tentou = true);
                    cx.refresh_windows();
                    return;
                }
                motor.update(cx, |m, cx| {
                    m.mudar_config(
                        |c| {
                            perfis::guardar(&mut c.perfis, completo);
                        },
                        cx,
                    )
                });
                window.close_dialog(cx);
            }
        };
        let (rascunho_x, motor_x, id_x) = (rascunho.clone(), motor.clone(), perfil.id.clone());
        let salvar_no_enter = salvar.clone();
        dialogos::moldura(dialogo, largura::M, window)
            .title(dialogos::topo(
                CabecalhoDialogo::new(if editando { "Editar perfil" } else { "Novo perfil" })
                    .subtitulo(div().font_weight(FontWeight::NORMAL).child("Aparece no alto do seletor de modelo")),
            ))
            .on_ok(move |_, window, cx| {
                salvar_no_enter(window, cx);
                false
            })
            .child(
                v_flex()
                    .gap(esp::E16)
                    .child(Campo::new("Nome", Input::new(&nome)).erro(erro_a_vista))
                    .child(escolhas)
                    .child(Campo::new("Quando usar", Input::new(&nota)).opcional().ajuda("Fica embaixo do nome, no seletor."))
                    .child(div().metadado().text_color(Cores::tenue()).child(
                        "Escolher o perfil deixa o chat como ele diz: o que ficar no padrão aqui volta ao padrão lá.",
                    )),
            )
            .footer(
                RodapeDialogo::new()
                    .when(editando, |rodape| {
                        rodape.esquerda(
                            botao_perigo("perfil-excluir", None, if excluindo { "Excluir mesmo" } else { "Excluir" }, TamanhoBotao::Controle, window, cx)
                                .on_click(move |_, window, cx| {
                                    if !rascunho_x.read(cx).excluindo {
                                        rascunho_x.update(cx, |r, _| r.excluindo = true);
                                        cx.refresh_windows();
                                        return;
                                    }
                                    let id = id_x.clone();
                                    motor_x.update(cx, |m, cx| m.mudar_config(|c| perfis::remover(&mut c.perfis, &id), cx));
                                    window.close_dialog(cx);
                                }),
                        )
                    })
                    .cancelar(
                        botao_fantasma("perfil-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .principal(
                        botao_primario("perfil-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx)
                            .on_click(move |_, window, cx| salvar(window, cx)),
                    ),
            )
    });
    window.focus(&foco, cx);
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` traz um `test` que tomaria o lugar do `#[test]`.
    use super::{resumo, segunda_linha};
    use crate::{modelo::Agente, perfis::Perfil, ui::composer::nome_curto_modelo};

    fn perfil(nome: &str) -> Perfil {
        Perfil { nome: nome.into(), ..Perfil::default() }
    }

    #[test]
    fn o_resumo_diz_o_que_o_perfil_escolhe_e_cala_o_que_fica_no_padrao() {
        let cheio = Perfil { modelo: "opus".into(), modo: "plan".into(), esforco: "high".into(), ..perfil("Planejar") };
        assert_eq!(resumo(&cheio), "Claude · Opus · Planejar · esforço alto");
        assert_eq!(resumo(&perfil("Dia a dia")), "Claude · modelo padrão");
        let codex = Perfil { agente: Agente::Codex, modo: "acceptEdits".into(), ..perfil("Codex rápido") };
        assert_eq!(resumo(&codex), "Codex · modelo padrão · Aceitar edições");
        // A versão exata guardada aparece pelo nome curto, não pelo id.
        let exato = Perfil { modelo: "claude-opus-4-5".into(), ..perfil("Fixo") };
        assert_eq!(resumo(&exato), format!("Claude · {}", nome_curto_modelo("claude-opus-4-5")));
    }

    #[test]
    fn a_segunda_linha_e_a_nota_e_sem_nota_o_resumo() {
        let com_nota = Perfil { nota: "  Antes de mudança grande ".into(), modo: "plan".into(), ..perfil("Planejar") };
        assert_eq!(segunda_linha(&com_nota), "Antes de mudança grande");
        assert_eq!(segunda_linha(&perfil("Simples")), "Claude · modelo padrão");
    }
}
