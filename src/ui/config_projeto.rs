//! A configuração de uma pasta ligada, em Editar projeto: no Avançado, a preparação de cada cópia
//! nova, os arquivos fora do git que vão junto para as cópias (como o .env) e o comando para rodar;
//! em "GitHub e Linear", como as mudanças chegam ao GitHub (o desenho fica em `projeto.rs`).

use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::*;

use super::componentes::{GrupoDeOpcoes, LinhaDeOpcao};
use crate::{
    modelo::{ConfigGithub, ConfigProjeto, ModoEntrega},
    tema::FONTE_MONO,
};

/// Os três campos de uma pasta e como estavam ao abrir, mais o GitHub dela (a seção "GitHub e
/// Linear" do editor mexe nele; o prefixo dos ramos é um campo).
pub(super) struct ConfigDaPasta {
    preparacao: Entity<InputState>,
    rodar: Entity<InputState>,
    copiar: Entity<InputState>,
    inicial: [String; 3],
    pub(super) github: ConfigGithub,
    github_inicial: ConfigGithub,
    pub(super) prefixo: Entity<InputState>,
    /// "Mais opções" do GitHub aberto.
    pub(super) mais_opcoes: bool,
}

/// O texto embaixo de "Ao aprovar", que muda com a escolha.
pub(super) fn descricao_da_entrega(modo: ModoEntrega) -> &'static str {
    match modo {
        ModoEntrega::Juntar => "Juntar aqui leva as mudanças aprovadas do chat para esta pasta.",
        ModoEntrega::Pr => {
            "Abrir PR manda as mudanças para o GitHub e acompanha checks e revisão. Os chats desta pasta rodam isolados: \
             sem gh, sem git push e sem o chaveiro; o Vitral faz o push."
        }
    }
}

/// O ramo de destino como aparece no menu: o padrão do repositório entre parênteses.
pub(super) fn nome_do_ramo_padrao(padrao: &str) -> String {
    if padrao.is_empty() { "Padrão do repositório".into() } else { format!("Padrão ({padrao})") }
}

/// Os três textos de uma configuração, na ordem dos campos.
fn textos(c: &ConfigProjeto) -> [String; 3] {
    [c.preparacao.trim().to_string(), c.rodar.trim().to_string(), c.copiar.join(", ")]
}

/// "a, b,, c" vira a lista dos arquivos que vão junto.
pub(super) fn lista_de_copiar(texto: &str) -> Vec<String> {
    texto.split(',').map(|p| p.trim().to_string()).filter(|p| !p.is_empty()).collect()
}

impl ConfigDaPasta {
    /// Os campos já com a configuração da pasta; cada mudança redesenha quem os mostra.
    pub(super) fn new<V: 'static>(
        config: &ConfigProjeto,
        assinaturas: &mut Vec<Subscription>,
        window: &mut Window,
        cx: &mut Context<V>,
    ) -> Self {
        let inicial = textos(config);
        let mut campo = |valor: &str, dica: &'static str| {
            let campo = cx.new(|cx| InputState::new(window, cx).placeholder(dica).default_value(valor.to_string()));
            assinaturas.push(cx.subscribe(&campo, |_, _, evento: &InputEvent, cx| {
                if matches!(evento, InputEvent::Change) {
                    cx.notify();
                }
            }));
            campo
        };
        let preparacao = campo(&inicial[0], "npm install");
        let rodar = campo(&inicial[1], "npm run dev -- --port $VITRAL_PORT");
        let copiar = campo(&inicial[2], ".env*, CLAUDE.local.md");
        let prefixo = campo(config.github.prefixo_ramo.trim(), crate::issues::PREFIXO_PADRAO);
        ConfigDaPasta {
            preparacao,
            rodar,
            copiar,
            inicial,
            github: config.github.clone(),
            github_inicial: config.github.clone(),
            prefixo,
            mais_opcoes: false,
        }
    }

    /// A configuração como está nos campos.
    pub(super) fn config(&self, cx: &App) -> ConfigProjeto {
        ConfigProjeto {
            preparacao: self.preparacao.read(cx).value().trim().to_string(),
            rodar: self.rodar.read(cx).value().trim().to_string(),
            copiar: lista_de_copiar(&self.copiar.read(cx).value()),
            github: ConfigGithub { prefixo_ramo: self.prefixo.read(cx).value().trim().to_string(), ..self.github.clone() },
        }
    }

    pub(super) fn mudou(&self, cx: &App) -> bool {
        let config = self.config(cx);
        textos(&config) != self.inicial || config.github != self.github_inicial
    }

    /// As linhas da pasta num grupo do Avançado.
    pub(super) fn grupo(&self, indice: usize, titulo: SharedString) -> GrupoDeOpcoes {
        let campo = |e: &Entity<InputState>| Input::new(e).font_family(FONTE_MONO);
        GrupoDeOpcoes::new(Some(titulo))
            .linha(
                LinhaDeOpcao::new(("pasta-preparar", indice), "Preparar cada cópia")
                    .descricao("Roda em cada cópia nova antes de o chat começar.")
                    .campo(campo(&self.preparacao)),
            )
            .linha(
                LinhaDeOpcao::new(("pasta-copiar", indice), "Arquivos que vão junto")
                    .descricao("Fora do git, separados por vírgula; * no fim pega o prefixo. Vazio: .env* e CLAUDE.local.md.")
                    .campo(campo(&self.copiar)),
            )
            .linha(
                LinhaDeOpcao::new(("pasta-rodar", indice), "Comando para rodar")
                    .descricao("Abre num terminal. Cada chat tem dez portas a partir de $VITRAL_PORT.")
                    .campo(campo(&self.rodar)),
            )
    }
}

#[cfg(test)]
mod testes {
    use super::{ModoEntrega, descricao_da_entrega, lista_de_copiar, nome_do_ramo_padrao};

    #[test]
    fn copiar_separa_por_virgula_e_tira_os_vazios() {
        assert_eq!(lista_de_copiar(".env*, CLAUDE.local.md,, "), vec![".env*", "CLAUDE.local.md"]);
        assert!(lista_de_copiar("  ").is_empty());
    }

    #[test]
    fn textos_do_github_da_pasta() {
        assert!(descricao_da_entrega(ModoEntrega::Pr).contains("rodam isolados"));
        assert!(descricao_da_entrega(ModoEntrega::Juntar).starts_with("Juntar aqui"));
        assert_eq!(nome_do_ramo_padrao("main"), "Padrão (main)");
        assert_eq!(nome_do_ramo_padrao(""), "Padrão do repositório");
    }
}
