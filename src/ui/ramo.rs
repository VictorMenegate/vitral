//! O ramo do git à vista em cada chat: o texto dele para as dicas (a linha do chat na lista e a
//! aba), a leitura guardada por uns segundos e o aviso, acima da caixa, de que a pasta trocou de
//! ramo desde o último turno do chat ("A pasta agora está em main; o chat trabalhava em feat/x").
//! O cabeçalho da seção Na pasta mostra o ramo que o retrato do git traz (`git_da_pasta.rs`).
//!
//! O ramo do último turno é o motor que anota (`motor/ramo.rs`, em `Chat::ramo`); o de agora vem
//! dos arquivos do git (`git::ramo_no_disco`), sem abrir processo.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use gpui_kit::component::{h_flex, tooltip::Tooltip};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela, composer,
    componentes::{FaixaCaixa, TamanhoBotao, botao_icone},
    mensagens::{coluna, icone_de},
};
use crate::{
    git::RamoDaPasta,
    modelo::Chat,
    motor::Motor,
    tema::{Cores, Lucide, esp, r, tam},
};

/// De quanto em quanto tempo o ramo de uma pasta é relido (o mesmo passo da seção Na pasta).
const RELER_A_CADA: Duration = Duration::from_secs(4);

/// O ramo de agora de cada pasta de chat à vista, com a hora da leitura.
#[derive(Default)]
pub(super) struct Ramos {
    lidos: HashMap<PathBuf, (Instant, Option<RamoDaPasta>)>,
}

impl Ramos {
    /// O ramo da pasta, relido do disco no máximo a cada 4 s.
    fn da_pasta(&mut self, pasta: &Path) -> Option<&RamoDaPasta> {
        let velho = self.lidos.get(pasta).is_none_or(|(em, _)| em.elapsed() >= RELER_A_CADA);
        if velho {
            self.lidos.insert(pasta.to_path_buf(), (Instant::now(), crate::git::ramo_no_disco(pasta)));
        }
        self.lidos.get(pasta).and_then(|(_, ramo)| ramo.as_ref())
    }
}

/// Onde a pasta está, numa frase curta: "No ramo main", "No ramo feat/x, num worktree", "Com a
/// cabeça solta em 1a2b3c4".
pub(super) fn onde_esta(ramo: &RamoDaPasta) -> String {
    let base = if ramo.solta { format!("Com a cabeça solta em {}", ramo.nome) } else { format!("No ramo {}", ramo.nome) };
    if ramo.worktree { format!("{base}, num worktree") } else { base }
}

/// O texto de uma dica com o ramo da pasta numa linha a mais (nada, fora de um repositório). Lê o
/// disco: é para chamar na hora em que a dica aparece, não a cada quadro.
pub(super) fn com_ramo(texto: &str, pasta: &Path) -> String {
    match crate::git::ramo_no_disco(pasta) {
        Some(ramo) => format!("{texto}\n{}", onde_esta(&ramo)),
        None => texto.to_string(),
    }
}

/// Como o aviso chama o lugar: "em main" ou, com a cabeça solta, "no commit 1a2b3c4".
fn lugar(nome: &str, solta: bool) -> String {
    if solta { format!("no commit {nome}") } else { format!("em {nome}") }
}

/// O aviso de troca de ramo: `gravado` é o ramo do último turno do chat; `agora`, o da pasta. Nada
/// quando o chat ainda não rodou num repositório, quando a pasta saiu do git ou quando os dois são
/// o mesmo.
fn aviso_de_troca(gravado: Option<&str>, agora: Option<&RamoDaPasta>) -> Option<String> {
    let (gravado, agora) = (gravado?, agora?);
    if gravado == agora.nome {
        return None;
    }
    // O que foi anotado com a cabeça solta é um commit curto; um ramo com cara de commit é raro o
    // bastante para a frase não precisar de outra anotação.
    let era_commit = gravado.len() == 7 && gravado.chars().all(|c| c.is_ascii_hexdigit());
    Some(format!("A pasta agora está {}; o chat trabalhava {}", lugar(&agora.nome, agora.solta), lugar(gravado, era_commit)))
}

impl Janela {
    /// A faixa "A pasta agora está em main; o chat trabalhava em feat/x", acima da caixa, com o ×
    /// que a dispensa (o chat passa a valer no ramo de agora). Só com o chat quieto: o turno que
    /// começa anota o ramo dele, e o aviso some sozinho.
    pub(super) fn aviso_do_ramo(&mut self, motor: &Motor, chat: &Chat, painel_id: u64, window: &Window, cx: &Context<Self>) -> Option<AnyElement> {
        let gravado = chat.ramo.as_deref()?;
        if motor.execucao(&chat.id).rodando() {
            return None;
        }
        let texto = SharedString::from(aviso_de_troca(Some(gravado), self.ramos.da_pasta(&chat.cwd))?);
        // As mesmas bordas da faixa do turno: o texto da caixa à esquerda, o enviar à direita.
        let estreita = self.paineis.iter().any(|p| p.id == painel_id && p.medida.faixa == FaixaCaixa::Estreita);
        let margem = tam(window, if estreita { 12. } else { 16. });
        let (recuo, fim) = (margem + composer::recuo_do_texto(estreita, window), margem + composer::borda_ao_conteudo(estreita, window));
        let (motor_e, id) = (self.motor.clone(), chat.id.clone());
        let dispensar = botao_icone(("ramo-entendi", painel_id as usize), Lucide::X, "Entendi: o chat segue no ramo de agora", TamanhoBotao::Linha, window, cx)
            .on_click(move |_, _, cx| {
                if let Some(m) = &motor_e {
                    m.update(cx, |m, cx| m.aceitar_ramo(&id, cx));
                }
            });
        let dica = texto.clone();
        Some(
            h_flex()
                .map(coluna)
                .pl(recuo)
                .pr(fim)
                .h(r(28.))
                .gap(esp::E8)
                .text_color(Cores::tenue())
                .child(icone_de(Lucide::GitBranch, 14.).text_color(Cores::espera()))
                .child(
                    div()
                        .id(("aviso-do-ramo", painel_id as usize))
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(r(13.))
                        .line_height(r(18.))
                        .child(texto)
                        .tooltip(move |window, cx| Tooltip::new(dica.clone()).build(window, cx)),
                )
                .child(div().flex_none().child(dispensar))
                .into_any_element(),
        )
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use super::{aviso_de_troca, onde_esta};
    use crate::git::RamoDaPasta;

    fn ramo(nome: &str, solta: bool, worktree: bool) -> RamoDaPasta {
        RamoDaPasta { nome: nome.into(), solta, worktree }
    }

    #[test]
    fn o_ramo_numa_frase() {
        assert_eq!(onde_esta(&ramo("main", false, false)), "No ramo main");
        assert_eq!(onde_esta(&ramo("feat/x", false, true)), "No ramo feat/x, num worktree");
        assert_eq!(onde_esta(&ramo("1a2b3c4", true, false)), "Com a cabeça solta em 1a2b3c4");
    }

    #[test]
    fn o_aviso_so_quando_o_ramo_mudou() {
        let main = ramo("main", false, false);
        assert_eq!(aviso_de_troca(Some("feat/x"), Some(&main)).as_deref(), Some("A pasta agora está em main; o chat trabalhava em feat/x"));
        assert_eq!(aviso_de_troca(Some("main"), Some(&main)), None);
        // O chat que ainda não rodou num repositório e a pasta que saiu do git não avisam.
        assert_eq!(aviso_de_troca(None, Some(&main)), None);
        assert_eq!(aviso_de_troca(Some("main"), None), None);
        // Com a cabeça solta, de um lado e do outro.
        let solta = ramo("1a2b3c4", true, false);
        assert_eq!(aviso_de_troca(Some("main"), Some(&solta)).as_deref(), Some("A pasta agora está no commit 1a2b3c4; o chat trabalhava em main"));
        assert_eq!(aviso_de_troca(Some("1a2b3c4"), Some(&main)).as_deref(), Some("A pasta agora está em main; o chat trabalhava no commit 1a2b3c4"));
    }
}
