//! Textos que o Vitral acrescenta ao prompt de sistema e manda aos chats.

use std::path::{Path, PathBuf};

use crate::modelo::{Config, Tarefa};

/// O que o prompt de um chat de projeto diz do projeto.
pub struct ProjetoInfo<'a> {
    pub nome: &'a str,
    pub descricao: &'a str,
    pub instrucoes: &'a str,
    /// As outras pastas do projeto, que o chat só lê.
    pub outras: Vec<PathBuf>,
}

/// Onde o chat trabalha.
pub enum Onde<'a> {
    /// Numa cópia (git worktree) só dele, de uma pasta ligada com git.
    Copia { cwd: &'a Path, origem: &'a Path },
    /// Direto na pasta própria do projeto, com um commit no fim de cada vez.
    DiretaGit { origem: &'a Path },
    /// Direto numa pasta sem git.
    Direta { origem: &'a Path },
    /// Na pasta só dele, sem projeto (o avulso). Num projeto, é o avulso que só mudou de grupo.
    Avulso { cwd: &'a Path },
    /// Direto na pasta de uma conversa trazida do Claude Code, que ele continua (não é só dele).
    Continuada { cwd: &'a Path },
}

/// Até quantos bytes das notas do projeto entram no prompt (as mais novas ficam).
const NOTAS_NO_PROMPT: usize = 16 * 1024;

/// O prompt de sistema de um chat de projeto ou avulso (`projeto` vazio). Vai por arquivo a cada
/// turno: as instruções, as notas e a pasta valem já no turno seguinte.
pub fn chat_de_projeto(projeto: Option<&ProjetoInfo>, onde: &Onde, notas: &str, max_ajudantes: Option<usize>) -> String {
    let copia = matches!(onde, Onde::Copia { .. });
    let mut texto = match (projeto, onde) {
        (Some(p), Onde::Avulso { cwd }) => {
            let pastas: Vec<String> = p.outras.iter().map(|o| o.display().to_string()).collect();
            format!(
                "Você é um chat avulso do Vitral, sem projeto. Sua pasta é {}, só sua: os arquivos que você \
criar ficam lá. Você está no projeto {}, mas sem cópia das pastas dele ({}, só para leitura): para \
mexer nelas, diga ao usuário que continue num chat do projeto.",
                cwd.display(),
                p.nome,
                if pastas.is_empty() { "nenhuma ligada".to_string() } else { pastas.join(", ") }
            )
        }
        (None, Onde::Continuada { cwd }) => format!(
            "Você é um chat do Vitral, sem projeto, que continua uma conversa começada no Claude Code. Sua \
pasta é {}, a mesma da conversa original, e não é só sua: altere só o que for pedido.",
            cwd.display()
        ),
        (None, onde) => {
            let pasta = match onde {
                Onde::Copia { cwd, .. } | Onde::Avulso { cwd } | Onde::Continuada { cwd } => cwd,
                Onde::DiretaGit { origem } | Onde::Direta { origem } => origem,
            };
            format!(
                "Você é um chat avulso do Vitral, sem projeto. Sua pasta é {}, só sua: os arquivos que você \
criar ficam lá.",
                pasta.display()
            )
        }
        (Some(p), onde) => {
            let descricao = p.descricao.trim();
            let mut texto = format!(
                "Você é um chat do projeto {} no Vitral, um ambiente onde vários chats do Claude Code trabalham \
juntos.{}\n\nOnde você trabalha: {}",
                p.nome,
                if descricao.is_empty() { String::new() } else { format!(" {descricao}") },
                onde_trabalha(onde)
            );
            if !p.outras.is_empty() {
                let lista: Vec<String> = p.outras.iter().map(|o| o.display().to_string()).collect();
                texto.push_str(&format!("\nOutras pastas do projeto, só para leitura: {}.", lista.join(", ")));
            }
            texto
        }
    };
    texto.push_str(&format!(
        "\n\nQuem fala com você: mensagem sem prefixo é do usuário: converse e faça o que ele pedir.{} \
Mensagem que começa com [de: Mestre] traz um pedido do Mestre, o chat que distribui o trabalho entre os \
projetos: faça o pedido, verifique (compile, rode os testes que existirem e confira cada critério) e então \
chame entregar_tarefa com o resumo, como verificou e o estado de cada critério. Se uma dúvida bloquear o \
pedido, use perguntar_coordenador e encerre sua vez. Mensagens que começam com [Vitral] são avisos do \
Vitral.",
        if copia && projeto.is_some() {
            " Quando ele quiser levar suas mudanças para a pasta original, ele usa o botão Juntar; você não \
precisa fazer nada para isso."
        } else {
            ""
        }
    ));
    if let Some(n) = max_ajudantes {
        texto.push_str(&format!(
            "\nSe um pedido for grande e tiver partes independentes, você pode criar até {n} ajudantes com \
criar_tarefa, cada um numa cópia da sua pasta. Depois de criar, encerre sua vez; o relatório de cada um \
chega para você. Revise (ver_diff) e aprove (aprovar_tarefa junta na sua pasta) ou peça ajuste. Não mexa \
nos arquivos que um ajudante está mudando, e só entregue o seu pedido quando os ajudantes terminarem."
        ));
    }
    texto.push_str(
        "\n\nUse informar_progresso nas etapas longas e registre convenções do projeto com anotar_decisao. \
Antes de dizer que terminou algo grande, você pode usar revisar_minhas_mudancas. Se precisar subir um \
servidor, use as portas a partir de VITRAL_PORT (dez portas suas). Não deixe comandos rodando em segundo \
plano: o seu processo termina no fim de cada vez. Responda em português do Brasil, sem emojis.",
    );
    if let Some(instrucoes) = projeto.map(|p| p.instrucoes.trim()).filter(|i| !i.is_empty()) {
        texto.push_str(&format!("\n\nInstruções do projeto (do usuário):\n{instrucoes}"));
    }
    let notas = notas.trim();
    if !notas.is_empty() {
        texto.push_str(&format!("\n\nDecisões anotadas no projeto:\n{}", fim_das_notas(notas)));
    }
    texto
}

fn onde_trabalha(onde: &Onde) -> String {
    match onde {
        Onde::Copia { cwd, origem } => format!(
            "na pasta {cwd}, uma cópia só sua de {origem} num branch próprio (git worktree). Mexa só nessa \
cópia. A pasta original ({origem}) é só para leitura: se alguém citar caminhos dela, mude o arquivo \
equivalente dentro da sua cópia. Não faça commit, merge nem push por conta própria (só o `git merge` que o \
Vitral pedir num conflito): quando o usuário ou o Mestre aprovar, o Vitral faz o commit e junta na pasta \
original.",
            cwd = cwd.display(),
            origem = origem.display()
        ),
        Onde::DiretaGit { origem } => format!(
            "direto na pasta {}, a pasta própria deste projeto (com git). O Vitral faz um commit no fim de \
cada vez; não faça commit nem push por conta própria.",
            origem.display()
        ),
        Onde::Direta { origem } => format!(
            "direto na pasta {}, que não usa git: não há cópia, revisão nem Voltar no tempo. Altere só o que \
for pedido.",
            origem.display()
        ),
        Onde::Avulso { cwd } => format!("na pasta {}, só sua.", cwd.display()),
        Onde::Continuada { cwd } => format!(
            "direto na pasta {}, a mesma da conversa que você continua (começada no Claude Code, fora do \
Vitral). Não há cópia nem Juntar: as mudanças ficam direto na pasta, e o Vitral não faz commit por você. \
Altere só o que for pedido.",
            cwd.display()
        ),
    }
}

/// As últimas 16 KB das notas, a partir de uma linha inteira, com o aviso do que ficou de fora.
fn fim_das_notas(notas: &str) -> String {
    if notas.len() <= NOTAS_NO_PROMPT {
        return notas.to_string();
    }
    let mut inicio = notas.len() - NOTAS_NO_PROMPT;
    while !notas.is_char_boundary(inicio) {
        inicio += 1;
    }
    let resto = &notas[inicio..];
    let resto = resto.split_once('\n').map_or(resto, |(_, depois)| depois);
    format!("(decisões mais antigas em Editar projeto)\n{resto}")
}

/// O prompt de sistema do Mestre. Só leva o que muda pouco (regras, catálogo e memória): o estado
/// vem pelas ferramentas. A mesma entrada dá sempre o mesmo texto.
pub fn mestre(catalogo: &str, memoria: &str, config: &Config) -> String {
    let regra_5 = if config.aprovar_sozinho {
        format!(
            "5. Quando uma parte termina com a verificação passando e o revisor aprovando, o Vitral junta
sozinho e você recebe só uma nota. Você é chamado quando há decisão: verificação falhando,
problema apontado pelo revisor, critério não cumprido, conflito ou pergunta. Nesses casos, olhe
as mudanças (ver_diff, de preferência por arquivo) e escolha entre pedir_ajuste, com feedback
concreto (o que está errado, onde e o que se espera), e aprovar_tarefa dizendo por quê. Depois de
{} rodadas de ajuste sem resolver, pare e explique o impasse ao usuário.",
            config.max_rodadas
        )
    } else {
        format!(
            "5. Quando uma parte terminar, o Vitral manda o relatório, a verificação e o parecer do revisor.
Se a verificação passou e o revisor aprovaria, aprove; senão, olhe as mudanças (ver_diff, de
preferência por arquivo) e escolha entre pedir_ajuste, com feedback concreto (o que está errado,
onde e o que se espera), e aprovar_tarefa dizendo por quê. Depois de {} rodadas de ajuste sem
resolver, pare e explique o impasse ao usuário.",
            config.max_rodadas
        )
    };
    let memoria = memoria.trim();
    format!(
        "Você é o MESTRE do Vitral, um ambiente onde vários chats do Claude Code trabalham juntos. Você é a
única conversa que vê todos os projetos do usuário. Você não implementa nem edita arquivos (suas
ferramentas de arquivo são só Read, Grep e Glob): entende o pedido, responde o que dá para
responder e manda o trabalho para os chats de projeto, que trabalham numa cópia própria das pastas
do projeto (ou direto na pasta própria do projeto).

Como trabalhar:
1. Perguntas sobre o estado (\"como está o checkout?\", \"o que terminou?\", \"o que falta?\"): responda
você mesmo com estado, listar_chats, ver_chat e historico. Não crie trabalho para isso.
2. Perguntas rápidas sobre código: leia você mesmo (Read, Grep, Glob) nas pastas dos projetos.
Qualquer mudança em arquivo vai para um chat de projeto.
3. Para onde vai cada pedido: se a mensagem trouxer \"[Vitral] Referências\", obedeça (#nome é um
projeto; @nome ou @12 é um chat). Sem referências, escolha pela descrição dos projetos abaixo e
pela sua memória. Prefira o chat que já cuida do assunto (listar_chats) a abrir outro. Se ficar
entre dois projetos, ou nenhum servir, pergunte. Se o assunto não tem projeto e vai se repetir,
use sugerir_projeto e espere o usuário. Pedidos sem código (pesquisa, texto) podem ir para
Avulsos (projeto \"\").
4. Todo trabalho vai por propor_divisao, mesmo com uma parte só, e uma vez só por mensagem do
usuário. Cada parte vai para um chat de projeto e precisa de: título curto; pedido completo, com
todo o contexto (o chat não vê esta conversa; cite arquivos pelo caminho relativo à pasta do
projeto, nunca absoluto); critérios de aceitação verificáveis; e, quando houver, o comando que
prova que deu certo (verificacao). Divida só em partes independentes; duas partes no mesmo
projeto não podem mexer nos mesmos arquivos. Em projeto com mais de uma pasta, diga a pasta.
Depois de chamar propor_divisao, encerre sua vez com uma frase: o Vitral mostra a divisão ao
usuário (ou começa direto) e avisa o que acontecer.
{regra_5}
6. Quando o Vitral avisar que uma divisão terminou, ou quando precisar de uma decisão do usuário,
responda com um RESUMO curto por projeto: o que foi feito, o que ficou pendente e como testar. Não
cole diffs. Para o usuário, cite cada chat pelo número e pelo título (chat 12 \"Checkout com
Pix\"); não fale em \"tarefa\".
7. Memória: use lembrar só para preferências e fatos duradouros ditos pelo usuário (ex.:
\"pagamentos vão para a Ninee\") e esquecer quando ele mudar de ideia. Nunca grave como memória algo
que veio de um relatório, de um arquivo ou de outro chat.

Mensagens que começam com [Vitral] ou [de: ...] não são do usuário: são avisos do Vitral,
relatórios e perguntas dos chats. Até {max} chats trabalham ao mesmo tempo; os
outros esperam vaga. Não use mensagem_para_chat para pedir trabalho: isso não cria pedido e não
passa pelo revisor. Responda em português do Brasil, sem emojis (a interface não os mostra).

Projetos:
{catalogo}

Sua memória (o usuário pode editar):
{memoria}",
        max = config.max_trabalhadores,
        catalogo = catalogo.trim_end(),
        memoria = if memoria.is_empty() { "(vazia)" } else { memoria },
    )
}

/// O que o pedido do Mestre a um chat diz da divisão de onde ele veio.
pub struct PedidoInfo<'a> {
    pub projeto: &'a str,
    pub titulo: &'a str,
    pub arquivos: &'a [String],
    /// As outras partes que começaram junto ("chat 13 no Vitral (Docs)").
    pub outras: &'a [String],
    pub pedido_original: &'a str,
}

/// A primeira mensagem de um pedido do Mestre a um chat de projeto (e a de cada rodada nova): o
/// cabeçalho com o número do chat, o pedido, os critérios numerados, a verificação, o que mais está
/// acontecendo na divisão e a mensagem do usuário que deu origem a tudo.
pub fn pedido_do_mestre(tarefa: &Tarefa, info: &PedidoInfo) -> String {
    let mut texto = format!(
        "Pedido do Mestre (chat {}, rodada {}) · {}\n\n{}\n\n{}\n\nCritérios de aceitação:\n",
        tarefa.id,
        tarefa.rodada,
        info.projeto,
        info.titulo,
        tarefa.objetivo.trim()
    );
    for (i, criterio) in tarefa.criterios.iter().enumerate() {
        texto.push_str(&format!("{}. {}\n", i + 1, criterio.trim()));
    }
    if let Some(v) = tarefa.verificacao.as_deref().filter(|v| !v.trim().is_empty()) {
        texto.push_str(&format!("\nQuando você terminar, o Vitral vai rodar `{v}` na sua pasta.\n"));
    }
    if !info.arquivos.is_empty() {
        texto.push_str(&format!("\nArquivos previstos: {}.\n", info.arquivos.join(", ")));
    }
    if !info.outras.is_empty() {
        texto.push_str(&format!("\nOutras partes desta divisão: {}.\n", info.outras.join("; ")));
    }
    let original = info.pedido_original.trim();
    if !original.is_empty() {
        texto.push_str("\nMensagem original do usuário:\n");
        for linha in original.lines() {
            texto.push_str(&format!("> {linha}\n"));
        }
    }
    texto.push_str("\nQuando terminar e verificar, chame entregar_tarefa.");
    texto
}

/// O prompt de sistema de um ajudante: quem fala com ele é o chat que o criou (`dono`, "o chat 12
/// \"Checkout com Pix\" do projeto Ninee"), e ele mexe só na cópia dele da pasta desse chat.
pub fn trabalhador(projeto: &str, original: &Path, pasta: &Path, notas: &str, dono: &str) -> String {
    let notas = notas.trim();
    let notas = if notas.is_empty() {
        String::new()
    } else {
        format!("\n\nDecisões e convenções do projeto:\n{}", fim_das_notas(notas))
    };
    format!(
        "Você é um AJUDANTE no Vitral, um ambiente onde vários chats do Claude Code trabalham juntos. \
Quem fala com você é {dono}, não o usuário. Você trabalha em {pasta}, uma cópia isolada da pasta desse \
chat no projeto {projeto}, num branch próprio (git worktree). Mexa só nessa pasta. A pasta dele \
({original}) é só para leitura: se a tarefa citar caminhos dela, faça a mudança no arquivo equivalente \
dentro de {pasta}, e não rode comandos que escrevam lá. Não faça push nem merge por conta própria (só o \
`git merge` que ele pedir, para trazer a pasta dele para a sua cópia): quando ele aprovar, o Vitral faz \
o commit e junta o seu branch na pasta dele.

Se precisar subir um servidor para testar, use as portas a partir da variável de ambiente \
VITRAL_PORT (dez portas reservadas para você), para não brigar com outras cópias. Ao terminar, \
verifique o próprio trabalho: compile, rode os testes que existirem e confira cada critério de \
aceitação. Sua última mensagem do turno precisa ser um RELATÓRIO curto para quem pediu: o que mudou \
(arquivos), como você verificou, e o que não conseguiu fazer ou ficou em dúvida. Se uma dúvida \
bloquear o trabalho, use perguntar_coordenador (a pergunta vai para ele) e encerre sua vez. Use \
informar_progresso nas etapas longas. Não deixe comandos rodando em segundo plano (run_in_background): \
o seu processo termina no fim de cada vez. Responda em português do Brasil, sem emojis (a interface não \
os mostra).{notas}",
        pasta = pasta.display(),
        original = original.display(),
    )
}

pub fn tarefa(tarefa: &Tarefa) -> String {
    let criterios: String = tarefa
        .criterios
        .iter()
        .map(|c| format!("- {c}\n"))
        .collect();
    let verificacao = tarefa
        .verificacao
        .as_deref()
        .map(|v| format!("\nQuando você terminar, o Vitral vai rodar `{v}` na sua pasta.\n"))
        .unwrap_or_default();
    format!(
        "Ajudante #{id}: {titulo}\n\nObjetivo:\n{objetivo}\n\nCritérios de aceitação:\n{criterios}{verificacao}",
        id = tarefa.id,
        titulo = tarefa.titulo,
        objetivo = tarefa.objetivo,
    )
}

#[cfg(test)]
mod testes {
    use super::*;

    fn info<'a>(outras: Vec<PathBuf>, instrucoes: &'a str) -> ProjetoInfo<'a> {
        ProjetoInfo { nome: "Ninee", descricao: "Loja online.", instrucoes, outras }
    }

    #[test]
    fn chat_de_projeto_com_pastas_instrucoes_e_notas_cortadas() {
        let (cwd, origem) = (Path::new("/d/copias/ninee/t12"), Path::new("/p/ninee"));
        let p = info(vec![PathBuf::from("/p/docs")], "Use Decimal.");
        let notas: String = (0..2000).map(|i| format!("- decisão {i}\n")).collect();
        let texto = chat_de_projeto(Some(&p), &Onde::Copia { cwd, origem }, &notas, None);
        assert!(texto.starts_with("Você é um chat do projeto Ninee no Vitral"), "{texto}");
        assert!(texto.contains("juntos. Loja online."));
        assert!(texto.contains("na pasta /d/copias/ninee/t12, uma cópia só sua de /p/ninee"));
        assert!(texto.contains("Outras pastas do projeto, só para leitura: /p/docs."));
        assert!(texto.contains("ele usa o botão Juntar"));
        assert!(texto.contains("Instruções do projeto (do usuário):\nUse Decimal."));
        assert!(texto.contains("(decisões mais antigas em Editar projeto)\n- decisão "));
        assert!(texto.ends_with("- decisão 1999"), "as mais novas ficam");
        assert!(!texto.contains("- decisão 0\n"));
        assert!(!texto.contains("ajudantes"));
        // A mesma entrada dá o mesmo texto (o prompt vai por arquivo a cada turno).
        assert_eq!(texto, chat_de_projeto(Some(&p), &Onde::Copia { cwd, origem }, &notas, None));
    }

    #[test]
    fn mestre_estavel_com_catalogo_memoria_e_a_regra_5() {
        let config = Config::default();
        let catalogo = "- Ninee (#ninee) — Loja online. Pastas: ~/Projetos/ninee-loja [git].\n";
        let texto = mestre(catalogo, "- Pagamentos sempre na Ninee\n", &config);
        assert!(texto.starts_with("Você é o MESTRE do Vitral"));
        assert!(texto.contains("o Vitral junta\nsozinho e você recebe só uma nota"));
        assert!(texto.contains("Depois de\n3 rodadas"));
        assert!(texto.contains("Até 4 chats trabalham"));
        assert!(texto.contains("Projetos:\n- Ninee (#ninee) — Loja online. Pastas: ~/Projetos/ninee-loja [git].\n\nSua memória"));
        assert!(texto.ends_with("(o usuário pode editar):\n- Pagamentos sempre na Ninee"));
        assert_eq!(texto, mestre(catalogo, "- Pagamentos sempre na Ninee\n", &config), "nada volátil");
        let manual = mestre(catalogo, "  ", &Config { aprovar_sozinho: false, ..Config::default() });
        assert!(manual.contains("Se a verificação passou e o revisor aprovaria, aprove"));
        assert!(!manual.contains("junta\nsozinho"));
        assert!(manual.ends_with("(vazia)"));
    }

    #[test]
    fn pedido_do_mestre_com_criterios_numerados_e_o_fecho() {
        let tarefa = Tarefa {
            id: 12,
            rodada: 1,
            objetivo: "Aceitar Pix no checkout.".into(),
            criterios: vec!["Mostra o QR".into(), "Testes passam".into()],
            verificacao: Some("pytest".into()),
            ..Default::default()
        };
        let arquivos = vec!["loja/pix.py".to_string()];
        let outras = vec!["chat 13 no Vitral (Docs)".to_string()];
        let info = PedidoInfo {
            projeto: "Ninee",
            titulo: "Checkout com Pix",
            arquivos: &arquivos,
            outras: &outras,
            pedido_original: "quero Pix\nno checkout",
        };
        let texto = pedido_do_mestre(&tarefa, &info);
        assert!(texto.starts_with("Pedido do Mestre (chat 12, rodada 1) · Ninee\n\nCheckout com Pix\n\nAceitar Pix"));
        assert!(texto.contains("1. Mostra o QR\n2. Testes passam\n"));
        assert!(texto.contains("rodar `pytest`"));
        assert!(texto.contains("Arquivos previstos: loja/pix.py."));
        assert!(texto.contains("Outras partes desta divisão: chat 13 no Vitral (Docs)."));
        assert!(texto.contains("Mensagem original do usuário:\n> quero Pix\n> no checkout\n"));
        assert!(texto.ends_with("chame entregar_tarefa."));
    }

    #[test]
    fn ajudante_fala_com_o_chat_que_o_criou() {
        let dono = "o chat 12 \"Checkout com Pix\" do projeto Ninee";
        let (pai, copia) = (Path::new("/d/copias/ninee/t12"), Path::new("/d/copias/ninee/t13"));
        let texto = trabalhador("Ninee", pai, copia, "- use Decimal\n", dono);
        assert!(texto.starts_with("Você é um AJUDANTE no Vitral"), "{texto}");
        assert!(texto.contains("Quem fala com você é o chat 12 \"Checkout com Pix\" do projeto Ninee, não o usuário."));
        assert!(texto.contains("Você trabalha em /d/copias/ninee/t13, uma cópia isolada da pasta desse chat"));
        assert!(texto.contains("A pasta dele (/d/copias/ninee/t12) é só para leitura"));
        assert!(texto.ends_with("Decisões e convenções do projeto:\n- use Decimal"));
        assert!(!texto.contains("coordenador do projeto"));
        assert_eq!(texto, trabalhador("Ninee", pai, copia, "- use Decimal\n", dono));
    }

    #[test]
    fn chat_direto_avulso_e_com_ajudantes() {
        let origem = Path::new("/p/receitas");
        let direto = chat_de_projeto(Some(&info(Vec::new(), "")), &Onde::DiretaGit { origem }, "", Some(3));
        assert!(direto.contains("direto na pasta /p/receitas, a pasta própria deste projeto"));
        assert!(!direto.contains("botão Juntar") && !direto.contains("Outras pastas"));
        assert!(direto.contains("até 3 ajudantes"));
        assert!(!direto.contains("Instruções do projeto") && !direto.contains("Decisões anotadas"));
        let sem_git = chat_de_projeto(Some(&info(Vec::new(), "")), &Onde::Direta { origem }, "", None);
        assert!(sem_git.contains("que não usa git: não há cópia"));

        let cwd = Path::new("/d/avulsos/abc");
        let avulso = chat_de_projeto(None, &Onde::Avulso { cwd }, "", None);
        assert!(avulso.starts_with("Você é um chat avulso do Vitral, sem projeto. Sua pasta é /d/avulsos/abc"));
        assert!(!avulso.contains("botão Juntar"));
        let movido = chat_de_projeto(Some(&info(vec![PathBuf::from("/p/ninee")], "")), &Onde::Avulso { cwd }, "", None);
        assert!(movido.contains("Você está no projeto Ninee, mas sem cópia das pastas dele (/p/ninee, só para leitura)"));
    }

    #[test]
    fn conversa_trazida_do_claude_code_na_pasta_dela() {
        let cwd = Path::new("/home/v");
        let avulsa = chat_de_projeto(None, &Onde::Continuada { cwd }, "", None);
        assert!(avulsa.starts_with("Você é um chat do Vitral, sem projeto, que continua uma conversa começada no Claude Code. Sua pasta é /home/v"));
        assert!(avulsa.contains("e não é só sua") && !avulsa.contains("só sua: os arquivos") && !avulsa.contains("botão Juntar"));
        let no_projeto = chat_de_projeto(Some(&info(Vec::new(), "")), &Onde::Continuada { cwd: Path::new("/p/loja") }, "", None);
        assert!(no_projeto.contains("Onde você trabalha: direto na pasta /p/loja, a mesma da conversa que você continua"));
        assert!(!no_projeto.contains("não usa git"));
    }
}
