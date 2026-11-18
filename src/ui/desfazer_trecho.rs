//! Desfazer só um trecho da mudança, no visor do diff aberto pela seção Na pasta: com o ponteiro
//! numa linha mudada, um botão pequeno ao lado do número volta só aquele bloco ao último commit;
//! a faixa da edição tem o mesmo botão para a mudança à vista (a das setas). Pergunta antes, como
//! o Desfazer do arquivo, e relê o diff depois. Quem monta e aplica o remendo é `git/remendo.rs`.
//!
//! Só vale para o diff do git (arquivo rastreado): o arquivo novo abre sem diff e a edição de uma
//! chamada do chat não é a do disco inteiro, então os dois ficam sem o botão.

use std::{path::PathBuf, rc::Rc};

use gpui_kit::component::WindowExt as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{Confirmacao, Tom, TomConfirmacao, aviso, confirmar},
    edicao::{Edicao, Fileira, Marca, Vista},
};
use crate::tema::Lucide;

/// O diff do git que o visor mostra: de qual arquivo, o texto do diff como foi lido (o desfazer
/// confere com ele se o arquivo mudou), quantos blocos tem e a edição feita dele.
pub(super) struct DiffDoGit {
    raiz: PathBuf,
    caminho: String,
    diff: String,
    blocos: usize,
    edicao: Rc<Edicao>,
}

/// O bloco de mudança de cada fileira do visor (o índice em `Vista::mudancas`), ou nada na que
/// não mudou.
pub(super) fn bloco_de_cada_fileira(vista: &Vista) -> Vec<Option<usize>> {
    let mut saida = vec![None; vista.fileiras.len()];
    for (k, mudanca) in vista.mudancas.iter().enumerate() {
        let limite = vista.mudancas.get(k + 1).map_or(vista.fileiras.len(), |proxima| proxima.fileira);
        for i in mudanca.fileira..limite.min(vista.fileiras.len()) {
            let mudou = match &vista.fileiras[i] {
                Fileira::Arquivo { entrou, .. } => *entrou,
                Fileira::Saiu(_) => true,
                Fileira::Solta(marca, _) => *marca != Marca::Igual,
                Fileira::Nota(_) => false,
            };
            if !mudou {
                break;
            }
            saida[i] = Some(k);
        }
    }
    saida
}

/// O que a pergunta diz do bloco: onde ele está no arquivo de agora.
fn onde_esta_o_bloco(linhas: Option<(usize, usize)>) -> String {
    match linhas {
        Some((a, b)) if b > a => format!("O bloco das linhas {a} a {b} do arquivo de agora"),
        Some((a, _)) => format!("O bloco da linha {a} do arquivo de agora"),
        None => "O bloco à vista".to_string(),
    }
}

impl Janela {
    /// Dá para desfazer por trecho no que o visor mostra: o arquivo aberto e a edição à vista são
    /// os do diff do git, todo trecho foi achado no arquivo e os blocos do diff são as mudanças
    /// do visor (o número de um é o do outro).
    pub(super) fn desfaz_por_trecho(&self, vista: &Vista) -> bool {
        let (Some(d), Some(aberta)) = (&self.diff_do_git, &self.arquivo_edicao) else { return false };
        Rc::ptr_eq(aberta, &d.edicao)
            && self.arquivo_aberto.as_ref().is_some_and(|(raiz, caminho)| *raiz == d.raiz && *caminho == d.caminho)
            && vista.perdidos == 0
            && d.blocos > 0
            && d.blocos == vista.mudancas.len()
    }

    /// Lê o diff do git do arquivo em segundo plano e o mostra no visor. `bloco`: a mudança que
    /// fica à vista (depois de desfazer um trecho, a que tomou o lugar dele).
    pub(super) fn abrir_diff_lido_do_git(&mut self, raiz: PathBuf, caminho: String, bloco: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        cx.spawn_in(window, async move |this, cx| {
            let (r, c) = (raiz.clone(), caminho.clone());
            let diff = cx.background_spawn(async move { crate::git::diff_do_arquivo(&r, &c) }).await;
            let _ = this.update_in(cx, |this, window, cx| this.mostrar_diff_do_git(raiz, caminho, diff, bloco, window, cx));
        })
        .detach();
    }

    /// Abre o arquivo com o diff do git por cima e guarda o diff, para o desfazer por trecho.
    fn mostrar_diff_do_git(&mut self, raiz: PathBuf, caminho: String, diff: String, bloco: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let trechos = super::edicao::trechos(&diff);
        // Depois de desfazer o último trecho não há mais diff: o arquivo como está, sem a edição.
        if trechos.is_empty() && bloco.is_some() {
            self.diff_do_git = None;
            self.edicao_pedida = None;
            self.reler_arquivo_aberto();
            self.abrir_arquivo(raiz, caminho, window, cx);
            return;
        }
        let (entram, saem) = super::edicao::contar(&trechos);
        let sem_registro = trechos.is_empty();
        let edicao = Rc::new(Edicao { trechos, entram, saem, sem_registro });
        let blocos = crate::git::remendo::blocos(&diff);
        self.diff_do_git = Some(DiffDoGit { raiz: raiz.clone(), caminho: caminho.clone(), diff, blocos, edicao: edicao.clone() });
        self.edicao_pedida = Some(edicao);
        // O arquivo mudou no disco agora mesmo: o visor não espera o segundo dele para reler.
        self.reler_arquivo_aberto();
        self.abrir_arquivo(raiz, caminho, window, cx);
        if let Some(bloco) = bloco {
            self.edicao_atual = bloco.min(blocos.saturating_sub(1));
        }
    }

    /// Pergunta e desfaz só o bloco `bloco` (o índice da mudança no visor) do diff do git aberto.
    pub(super) fn pedir_desfazer_trecho(&mut self, bloco: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(vista) = self.vista_aberta() else { return };
        if !self.desfaz_por_trecho(&vista) {
            return;
        }
        let Some(d) = &self.diff_do_git else { return };
        let (raiz, caminho, diff) = (d.raiz.clone(), d.caminho.clone(), d.diff.clone());
        let nome = caminho.rsplit('/').next().unwrap_or(&caminho).to_string();
        let onde = onde_esta_o_bloco(vista.mudancas.get(bloco).and_then(|m| m.linhas));
        let janela = cx.entity().downgrade();
        confirmar(
            Confirmacao {
                tom: TomConfirmacao::Perigo,
                icone: Lucide::RotateCcw.into(),
                titulo: format!("Desfazer este trecho de {nome}?").into(),
                texto: "Só este bloco volta a como está no último commit. O resto do arquivo fica.".into(),
                detalhes: vec![(Lucide::FileDiff.into(), onde.into(), false)],
                ok: "Desfazer o trecho".into(),
                cancelar: "Deixar como está".into(),
                ok_perigo: true,
            },
            move |window, cx| {
                let (raiz, caminho, diff, janela) = (raiz.clone(), caminho.clone(), diff.clone(), janela.clone());
                window
                    .spawn(cx, async move |cx| {
                        let (r, c) = (raiz.clone(), caminho.clone());
                        let feito = cx.background_spawn(async move { crate::git::remendo::desfazer_bloco(&r, &c, &diff, bloco) }).await;
                        let _ = cx.update(|window, cx| {
                            match &feito {
                                Ok(()) => window.push_notification(aviso(Tom::Pronto, "Trecho de volta ao último commit"), cx),
                                Err(erro) => window.push_notification(aviso(Tom::Atencao, format!("Não desfiz o trecho: {erro:#}")), cx),
                            }
                            // Feito ou não, o visor mostra o diff de agora (o arquivo pode ter
                            // mudado por fora), e a lista de Na pasta refaz a conta das linhas.
                            let _ = janela.update(cx, |this, cx| {
                                this.git_da_pasta.pedir_releitura();
                                if this.arquivo_aberto.as_ref().is_some_and(|(r, c)| *r == raiz && *c == caminho) {
                                    this.abrir_diff_lido_do_git(raiz, caminho, Some(bloco), window, cx);
                                }
                                cx.notify();
                            });
                        });
                    })
                    .detach();
            },
            None,
            window,
            cx,
        );
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use super::{bloco_de_cada_fileira, onde_esta_o_bloco};
    use crate::ui::edicao::{trechos, vista};

    #[test]
    fn cada_fileira_mudada_sabe_o_bloco_dela() {
        // Dois blocos no mesmo trecho e um terceiro em outro: os mesmos três que o remendo conta.
        let diff = "--- a/x\n+++ b/x\n@@ -1,7 +1,7 @@\n a\n-b\n+B\n c\n d\n-e\n+E\n+E2\n-f\n g\n@@ -20,3 +20,2 @@\n s\n-t\n u\n";
        let mut arquivo: Vec<String> = ["a", "B", "c", "d", "E", "E2", "g"].iter().map(|l| l.to_string()).collect();
        arquivo.extend((8..20).map(|n| format!("linha {n}")));
        arquivo.extend(["s".to_string(), "u".to_string()]);
        let v = vista(&trechos(diff), &arquivo);
        assert_eq!(v.perdidos, 0);
        assert_eq!(v.mudancas.len(), crate::git::remendo::blocos(diff));
        let blocos = bloco_de_cada_fileira(&v);
        assert_eq!(blocos.len(), v.fileiras.len());
        // a, −b, +B, c, d, −e, +E, +E2, −f, g…
        assert_eq!(blocos[..10], [None, Some(0), Some(0), None, None, Some(1), Some(1), Some(1), Some(1), None]);
        let do_terceiro: Vec<usize> = blocos.iter().enumerate().filter(|(_, b)| **b == Some(2)).map(|(i, _)| i).collect();
        assert_eq!(do_terceiro.len(), 1);
        assert_eq!(blocos.iter().flatten().max(), Some(&2));
    }

    #[test]
    fn a_pergunta_diz_onde_o_bloco_esta() {
        assert_eq!(onde_esta_o_bloco(Some((14, 16))), "O bloco das linhas 14 a 16 do arquivo de agora");
        assert_eq!(onde_esta_o_bloco(Some((3, 3))), "O bloco da linha 3 do arquivo de agora");
        assert_eq!(onde_esta_o_bloco(None), "O bloco à vista");
    }
}
