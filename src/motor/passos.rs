//! A lista de passos que o agente de um chat mantém, e o que cada chamada mudou nela (rodada do
//! Paseo, 23/11/2026, item 2: toda chamada aparecia na trilha como "Atualizou os passos").
//!
//! Duas famílias de ferramenta do Claude Code mexem nela:
//!
//! - `TodoWrite` manda a lista inteira a cada chamada (`todos`: `content`, `activeForm`,
//!   `status`). O que mudou sai da comparação com a lista de antes.
//! - `TaskCreate`, `TaskUpdate` e `TaskList` mexem num passo por vez, pelo id. Medido no Claude
//!   Code 2.1.328 em 23/11/2026 (Haiku, `-p --output-format stream-json`): sem nada no ambiente,
//!   o `-p` não oferece nenhuma das duas famílias; com `CLAUDE_CODE_ENABLE_TODO_TOOLS=1` ele
//!   oferece `TaskCreate`, `TaskGet`, `TaskList` e `TaskUpdate` (e não o `TodoWrite`). O
//!   `TaskCreate` leva `subject`, `description` e `activeForm`, e o id só vem no resultado ("Task
//!   #4 created successfully: passo x"); o `TaskUpdate` leva `taskId` e `status` (`in_progress`,
//!   `completed`, `deleted`) e responde "Updated task #4 status", ou "Task not found" sem marcar
//!   erro; o `TaskList` responde uma linha por passo, "#4 [in_progress] passo x". A lista é da
//!   sessão: continua de um turno para o outro, com os ids seguindo.

use std::collections::HashMap;

use serde_json::Value;

use super::{EstadoDoPasso, Passo};

/// Até quantas letras do passo vão na frase da trilha.
const MAX_DO_PASSO: usize = 80;

/// As ferramentas que mexem na lista de passos (ou a leem).
pub fn e_de_passos(nome: &str) -> bool {
    matches!(nome, "TodoWrite" | "TaskCreate" | "TaskUpdate" | "TaskList" | "TaskGet")
}

/// A frase que a chamada guardou no resumo ("TodoWrite · Criou 5 passos" dá "Criou 5 passos").
/// `None` na chamada que não disse o que mudou (as de antes disto, e a que não mudou nada).
pub fn frase_do_resumo<'a>(nome: &str, resumo: &'a str) -> Option<&'a str> {
    if !e_de_passos(nome) {
        return None;
    }
    let (_, frase) = resumo.split_once(" · ")?;
    (!frase.is_empty() && !matches!(frase, "lista de afazeres" | "lista de tarefas")).then_some(frase)
}

/// A lista de passos de um chat e as chamadas de tarefa que ainda esperam o resultado.
#[derive(Clone, Debug, Default)]
pub struct Passos {
    lista: Vec<Passo>,
    /// Por id da chamada: a ferramenta e o que ela pediu.
    pendentes: HashMap<String, (String, Value)>,
}

impl Passos {
    pub fn lista(&self) -> &[Passo] {
        &self.lista
    }

    /// O resumo da chamada para a trilha: com a frase do que mudou, quando há, no lugar do
    /// detalhe fixo ("TodoWrite · Começou: Rodar os testes").
    pub fn resumo(&mut self, id: &str, nome: &str, entrada: &Value) -> String {
        match self.chamada(id, nome, entrada) {
            Some(frase) => format!("{nome} · {frase}"),
            None => crate::claude::resumo_ferramenta(nome, entrada),
        }
    }

    /// Uma chamada chegou: o `TodoWrite` já troca a lista; as de tarefa esperam o resultado.
    /// Devolve o que ela mudou, em poucas palavras, ou `None` quando não é de passos ou não mudou
    /// nada que se diga.
    pub fn chamada(&mut self, id: &str, nome: &str, entrada: &Value) -> Option<String> {
        let texto = |chave: &str| entrada[chave].as_str().map(str::trim).filter(|t| !t.is_empty());
        match nome {
            "TodoWrite" => {
                let nova = passos_da_lista(entrada);
                let frase = frase_da_troca(&self.lista, &nova);
                self.lista = nova;
                frase
            }
            "TaskCreate" => {
                self.pendentes.insert(id.to_string(), (nome.to_string(), entrada.clone()));
                texto("subject").map(|assunto| format!("Criou: {}", curto(assunto)))
            }
            "TaskUpdate" => {
                self.pendentes.insert(id.to_string(), (nome.to_string(), entrada.clone()));
                let verbo = match texto("status")? {
                    "in_progress" => "Começou",
                    "completed" => "Concluiu",
                    "deleted" => "Tirou",
                    _ => return None,
                };
                let tarefa = texto("taskId")?;
                Some(match self.lista.iter().find(|p| p.id == tarefa) {
                    Some(passo) => format!("{verbo}: {}", curto(&passo.texto)),
                    // A lista é só da memória: depois de reabrir o Vitral, o passo de antes não está nela.
                    None => format!("{verbo} o passo {tarefa}"),
                })
            }
            "TaskList" => {
                self.pendentes.insert(id.to_string(), (nome.to_string(), Value::Null));
                None
            }
            _ => None,
        }
    }

    /// O resultado de uma chamada chegou: é aqui que as de tarefa mudam a lista.
    pub fn resultado(&mut self, id: &str, conteudo: &str, erro: bool) {
        let Some((nome, entrada)) = self.pendentes.remove(id) else { return };
        if erro {
            return;
        }
        let texto = |chave: &str| entrada[chave].as_str().map(str::trim).filter(|t| !t.is_empty());
        match nome.as_str() {
            "TaskCreate" => {
                let (Some(tarefa), Some(assunto)) = (id_criado(conteudo), texto("subject")) else { return };
                // O passo 1 de novo: a lista recomeçou (uma sessão nova do Claude Code).
                if tarefa == "1" {
                    self.lista.clear();
                }
                self.lista.retain(|p| p.id != tarefa);
                self.lista.push(Passo {
                    id: tarefa,
                    texto: assunto.to_string(),
                    fazendo: texto("activeForm").unwrap_or(assunto).to_string(),
                    estado: EstadoDoPasso::Falta,
                });
            }
            "TaskUpdate" => {
                // "Task not found" não vem marcado como erro.
                if !conteudo.trim_start().starts_with("Updated task") {
                    return;
                }
                let Some(tarefa) = texto("taskId") else { return };
                if texto("status") == Some("deleted") {
                    self.lista.retain(|p| p.id != tarefa);
                    return;
                }
                let Some(passo) = self.lista.iter_mut().find(|p| p.id == tarefa) else { return };
                if let Some(estado) = texto("status") {
                    passo.estado = estado_de(estado);
                }
                if let Some(assunto) = texto("subject") {
                    // O gerúndio que só repetia o assunto acompanha o assunto novo.
                    if passo.fazendo == passo.texto {
                        passo.fazendo = assunto.to_string();
                    }
                    passo.texto = assunto.to_string();
                }
                if let Some(fazendo) = texto("activeForm") {
                    passo.fazendo = fazendo.to_string();
                }
            }
            "TaskList" => {
                let lidos: Vec<Passo> = conteudo.lines().filter_map(|linha| self.passo_da_linha(linha)).collect();
                // Sem nenhuma linha de passo ("No tasks found"), a lista está vazia.
                if !lidos.is_empty() || !conteudo.contains('#') {
                    self.lista = lidos;
                }
            }
            _ => {}
        }
    }

    /// "#4 [in_progress] passo x": um passo da resposta do `TaskList`, com o gerúndio que a lista
    /// já tinha dele.
    fn passo_da_linha(&self, linha: &str) -> Option<Passo> {
        let (tarefa, resto) = linha.trim().strip_prefix('#')?.split_once(' ')?;
        let (estado, texto) = resto.strip_prefix('[')?.split_once("] ")?;
        if tarefa.is_empty() || !tarefa.bytes().all(|b| b.is_ascii_digit()) || texto.trim().is_empty() {
            return None;
        }
        let texto = texto.trim().to_string();
        let fazendo = self.lista.iter().find(|p| p.id == tarefa && p.texto == texto).map_or_else(|| texto.clone(), |p| p.fazendo.clone());
        Some(Passo { id: tarefa.to_string(), texto, fazendo, estado: estado_de(estado) })
    }
}

fn estado_de(estado: &str) -> EstadoDoPasso {
    match estado {
        "completed" => EstadoDoPasso::Feito,
        "in_progress" => EstadoDoPasso::Fazendo,
        _ => EstadoDoPasso::Falta,
    }
}

/// O id de "Task #4 created successfully: passo x".
fn id_criado(conteudo: &str) -> Option<String> {
    let resto = conteudo.trim_start().strip_prefix("Task #")?;
    let id: String = resto.chars().take_while(char::is_ascii_digit).collect();
    (!id.is_empty() && resto[id.len()..].starts_with(" created")).then_some(id)
}

fn curto(texto: &str) -> String {
    let linha = texto.lines().next().unwrap_or_default().trim();
    if linha.chars().count() > MAX_DO_PASSO {
        format!("{}…", linha.chars().take(MAX_DO_PASSO - 1).collect::<String>().trim_end())
    } else {
        linha.to_string()
    }
}

/// Os passos da entrada do TodoWrite (`todos`: `content`, `activeForm`, `status`).
pub(super) fn passos_da_lista(entrada: &Value) -> Vec<Passo> {
    let Some(lista) = entrada.get("todos").and_then(|t| t.as_array()) else {
        return Vec::new();
    };
    lista
        .iter()
        .filter_map(|p| {
            let texto = p.get("content")?.as_str()?.trim().to_string();
            let fazendo = p.get("activeForm").and_then(|a| a.as_str()).map(str::trim).unwrap_or(&texto).to_string();
            let estado = estado_de(p.get("status").and_then(|s| s.as_str()).unwrap_or_default());
            (!texto.is_empty()).then_some(Passo { id: String::new(), texto, fazendo, estado })
        })
        .collect()
}

/// O que mudou de uma lista do `TodoWrite` para a seguinte: "Criou 5 passos", "Começou: Rodar os
/// testes", "Concluiu: Rodar os testes" (vários juntos, "Concluiu 2 passos"; mais de uma coisa
/// na mesma chamada, "Concluiu: Ler o código · Começou: Corrigir"). Os passos são casados pelo
/// texto. `None` quando nada disso mudou (a ordem, ou um passo que voltou a pendente).
fn frase_da_troca(antes: &[Passo], depois: &[Passo]) -> Option<String> {
    let de_antes = |passo: &Passo| antes.iter().find(|p| p.texto == passo.texto);
    // Uma lista nova (a primeira, ou outra no lugar da que havia).
    if !depois.is_empty() && depois.iter().all(|p| de_antes(p).is_none()) {
        return Some(format!("Criou {}", contagem(depois.len())));
    }
    let virou = |estado: EstadoDoPasso| -> Vec<&Passo> {
        depois.iter().filter(|p| p.estado == estado && de_antes(p).is_some_and(|a| a.estado != estado)).collect()
    };
    let criados: Vec<&Passo> = depois.iter().filter(|p| de_antes(p).is_none()).collect();
    let tirados: Vec<&Passo> = antes.iter().filter(|a| !depois.iter().any(|p| p.texto == a.texto)).collect();
    let parte = |verbo: &str, passos: &[&Passo]| match passos {
        [] => None,
        [um] => Some(format!("{verbo}: {}", curto(&um.texto))),
        varios => Some(format!("{verbo} {}", contagem(varios.len()))),
    };
    let partes: Vec<String> = [
        parte("Concluiu", &virou(EstadoDoPasso::Feito)),
        parte("Começou", &virou(EstadoDoPasso::Fazendo)),
        parte("Criou", &criados),
        parte("Tirou", &tirados),
    ]
    .into_iter()
    .flatten()
    .collect();
    (!partes.is_empty()).then(|| partes.join(" · "))
}

fn contagem(n: usize) -> String {
    format!("{n} {}", if n == 1 { "passo" } else { "passos" })
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn lista(passos: &[(&str, &str)]) -> Value {
        json!({ "todos": passos.iter().map(|(texto, estado)| json!({ "content": texto, "activeForm": format!("Fazendo: {texto}"), "status": estado })).collect::<Vec<_>>() })
    }

    #[test]
    fn le_os_passos_do_todowrite() {
        let entrada = json!({"todos": [
            {"content": "Ler o código", "activeForm": "Lendo o código", "status": "completed"},
            {"content": "Corrigir", "activeForm": "Corrigindo", "status": "in_progress"},
            {"content": "Testar", "status": "pending"},
            {"content": "  ", "status": "pending"}
        ]});
        let passos = passos_da_lista(&entrada);
        assert_eq!(passos.iter().map(|p| p.estado).collect::<Vec<_>>(), [EstadoDoPasso::Feito, EstadoDoPasso::Fazendo, EstadoDoPasso::Falta]);
        assert_eq!(passos[1].fazendo, "Corrigindo");
        assert_eq!(passos[2].fazendo, "Testar");
        assert!(passos_da_lista(&json!({})).is_empty());
    }

    /// Rodada do Paseo, item 2: "Criou 5 passos", "Começou: rodar os testes", "Concluiu: rodar os
    /// testes" (vários juntos: "Concluiu 2 passos").
    #[test]
    fn o_todowrite_diz_o_que_mudou_na_lista() {
        let mut p = Passos::default();
        let cinco = [("ler o código", "pending"), ("corrigir", "pending"), ("rodar os testes", "pending"), ("documentar", "pending"), ("relatório", "pending")];
        assert_eq!(p.chamada("c1", "TodoWrite", &lista(&cinco)).as_deref(), Some("Criou 5 passos"));
        assert_eq!(p.lista().len(), 5);
        let mut agora = cinco;
        agora[2].1 = "in_progress";
        assert_eq!(p.chamada("c2", "TodoWrite", &lista(&agora)).as_deref(), Some("Começou: rodar os testes"));
        agora[2].1 = "completed";
        assert_eq!(p.chamada("c3", "TodoWrite", &lista(&agora)).as_deref(), Some("Concluiu: rodar os testes"));
        // Vários juntos.
        (agora[0].1, agora[1].1) = ("completed", "completed");
        assert_eq!(p.chamada("c4", "TodoWrite", &lista(&agora)).as_deref(), Some("Concluiu 2 passos"));
        // O caso de todo dia: um termina e o seguinte começa na mesma chamada.
        (agora[3].1, agora[4].1) = ("completed", "in_progress");
        assert_eq!(p.chamada("c5", "TodoWrite", &lista(&agora)).as_deref(), Some("Concluiu: documentar · Começou: relatório"));
        // A mesma lista de novo não diz nada (a trilha fica com "Atualizou os passos").
        assert_eq!(p.chamada("c6", "TodoWrite", &lista(&agora)), None);
        // Um passo a mais e um a menos.
        let mut mais = agora.to_vec();
        mais.push(("publicar", "pending"));
        assert_eq!(p.chamada("c7", "TodoWrite", &lista(&mais)).as_deref(), Some("Criou: publicar"));
        mais.remove(0);
        assert_eq!(p.chamada("c8", "TodoWrite", &lista(&mais)).as_deref(), Some("Tirou: ler o código"));
        // Outra lista no lugar: criou de novo. Uma só, no singular.
        assert_eq!(p.chamada("c9", "TodoWrite", &lista(&[("outra coisa", "in_progress")])).as_deref(), Some("Criou 1 passo"));
        // O resumo que a trilha guarda leva a frase; sem frase, o de sempre.
        assert_eq!(p.resumo("c10", "TodoWrite", &lista(&[("outra coisa", "completed")])), "TodoWrite · Concluiu: outra coisa");
        assert_eq!(p.resumo("c11", "TodoWrite", &lista(&[("outra coisa", "completed")])), "TodoWrite · lista de afazeres");
        assert_eq!(p.resumo("c12", "Read", &json!({ "file_path": "/p/a.rs" })), "Read · a.rs");
    }

    /// As ferramentas de tarefa, com as respostas medidas no Claude Code 2.1.328.
    #[test]
    fn as_tarefas_por_id_montam_a_mesma_lista() {
        let mut p = Passos::default();
        let criar = |p: &mut Passos, chamada: &str, assunto: &str, fazendo: &str| {
            p.chamada(chamada, "TaskCreate", &json!({ "subject": assunto, "description": assunto, "activeForm": fazendo }))
        };
        assert_eq!(criar(&mut p, "a", "criar a.txt", "Criando a.txt").as_deref(), Some("Criou: criar a.txt"));
        // O passo só entra na lista com o id, que vem no resultado.
        assert!(p.lista().is_empty());
        p.resultado("a", "Task #1 created successfully: criar a.txt", false);
        criar(&mut p, "b", "criar b.txt", "Criando b.txt");
        criar(&mut p, "c", "listar a pasta", "Listando a pasta");
        p.resultado("c", "Task #3 created successfully: listar a pasta", false);
        p.resultado("b", "Task #2 created successfully: criar b.txt", false);
        assert_eq!(p.lista().iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1", "3", "2"]);
        assert!(p.lista().iter().all(|x| x.estado == EstadoDoPasso::Falta));

        let mudar = |p: &mut Passos, chamada: &str, tarefa: &str, estado: &str| p.chamada(chamada, "TaskUpdate", &json!({ "taskId": tarefa, "status": estado }));
        assert_eq!(mudar(&mut p, "d", "1", "in_progress").as_deref(), Some("Começou: criar a.txt"));
        p.resultado("d", "Updated task #1 status", false);
        assert_eq!((p.lista()[0].estado, p.lista()[0].fazendo.as_str()), (EstadoDoPasso::Fazendo, "Criando a.txt"));
        assert_eq!(mudar(&mut p, "e", "1", "completed").as_deref(), Some("Concluiu: criar a.txt"));
        p.resultado("e", "Updated task #1 status", false);
        assert_eq!(p.lista()[0].estado, EstadoDoPasso::Feito);
        // O passo que não existe: a frase diz o número, e o "Task not found" (que não vem como
        // erro) não mexe na lista.
        assert_eq!(mudar(&mut p, "f", "99", "completed").as_deref(), Some("Concluiu o passo 99"));
        p.resultado("f", "Task not found", false);
        assert_eq!(p.lista().len(), 3);
        // Apagar tira da lista.
        assert_eq!(mudar(&mut p, "g", "2", "deleted").as_deref(), Some("Tirou: criar b.txt"));
        p.resultado("g", "Updated task #2 deleted", false);
        assert_eq!(p.lista().iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), ["1", "3"]);
        // Só o texto mudou: sem frase.
        assert_eq!(p.chamada("h", "TaskUpdate", &json!({ "taskId": "3", "subject": "listar tudo" })), None);
        p.resultado("h", "Updated task #3 subject", false);
        assert_eq!(p.lista()[1].texto, "listar tudo");
        // Feitos e total, como a pílula conta.
        let feitos = p.lista().iter().filter(|x| x.estado == EstadoDoPasso::Feito).count();
        assert_eq!((feitos, p.lista().len()), (1, 2));
    }

    #[test]
    fn o_tasklist_refaz_a_lista_e_o_passo_1_recomeca() {
        // Depois de reabrir o Vitral a lista da memória está vazia: o TaskList a traz de volta.
        let mut p = Passos::default();
        assert_eq!(p.chamada("l", "TaskList", &json!({})), None);
        p.resultado("l", "#1 [completed] criar a.txt\n#2 [in_progress] criar b.txt\n#4 [pending] passo x", false);
        assert_eq!(
            p.lista().iter().map(|x| (x.id.as_str(), x.texto.as_str(), x.estado)).collect::<Vec<_>>(),
            [("1", "criar a.txt", EstadoDoPasso::Feito), ("2", "criar b.txt", EstadoDoPasso::Fazendo), ("4", "passo x", EstadoDoPasso::Falta)]
        );
        // Uma sessão nova do Claude Code recomeça do 1: a lista velha sai.
        p.chamada("n", "TaskCreate", &json!({ "subject": "outro plano" }));
        p.resultado("n", "Task #1 created successfully: outro plano", false);
        assert_eq!(p.lista().iter().map(|x| x.texto.as_str()).collect::<Vec<_>>(), ["outro plano"]);
        // O resultado com erro (a permissão negada) não muda nada.
        p.chamada("o", "TaskCreate", &json!({ "subject": "negado" }));
        p.resultado("o", "The user doesn't want to proceed with this tool use.", true);
        assert_eq!(p.lista().len(), 1);
    }

    #[test]
    fn a_frase_sai_do_resumo_guardado() {
        assert_eq!(frase_do_resumo("TodoWrite", "TodoWrite · Criou 5 passos"), Some("Criou 5 passos"));
        assert_eq!(frase_do_resumo("TaskUpdate", "TaskUpdate · Começou: rodar os testes"), Some("Começou: rodar os testes"));
        // As chamadas de antes disto, e as que não mudaram nada.
        assert_eq!(frase_do_resumo("TodoWrite", "TodoWrite · lista de afazeres"), None);
        assert_eq!(frase_do_resumo("TodoWrite", "TodoWrite · lista de tarefas"), None);
        assert_eq!(frase_do_resumo("TodoWrite", "TodoWrite"), None);
        assert_eq!(frase_do_resumo("TaskList", "TaskList"), None);
        // Outras ferramentas não têm frase de passos.
        assert_eq!(frase_do_resumo("Bash", "Bash · Criou 5 passos"), None);
        assert!(e_de_passos("TaskCreate") && !e_de_passos("TaskStop") && !e_de_passos("Task"));
    }
}
