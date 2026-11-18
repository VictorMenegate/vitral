//! Peso fixo da pasta: com quanto contexto um chat novo já começa nela, antes de qualquer
//! conversa (as instruções, os servidores MCP, as skills e as ferramentas).
//!
//! A base é MEDIDA: o primeiro `Evento::Contexto` de uma sessão nova, menos a estimativa da
//! primeira mensagem. Fica a última por pasta, em `peso-das-pastas.json` na pasta de dados. Só
//! conta o chat que começa do zero no Claude: sessão retomada, conversa trazida do Claude Code,
//! bloco de passagem e mensagem com anexo ficam de fora, porque o primeiro contexto deles carrega
//! mais do que a pasta.
//!
//! A lista dos arquivos de instrução é LIDA do disco, pelas regras do Claude Code: o `CLAUDE.md`
//! do usuário, os da pasta (e das pastas de cima, até a raiz do repositório), os `@imports` de
//! cada um e o índice da memória.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::*;

/// A base medida de uma pasta.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Peso {
    /// Os tokens com que um chat novo começa.
    pub tokens: u64,
    /// Quando foi medido (segundos Unix).
    pub em: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub modelo: String,
    /// O que o Claude Code disse ter carregado na sessão medida.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub carga: Option<Carga>,
}

/// O que o `init` de uma sessão do Claude Code diz ter carregado.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Carga {
    /// Servidores MCP ligados e o total configurado.
    pub mcp_ligados: u32,
    pub mcp: u32,
    pub ferramentas: u32,
    #[serde(default)]
    pub skills: u32,
}

/// A carga declarada no `system init`; `None` quando ele não traz a lista dos servidores (o
/// Claude Code antigo e o de mentira da bancada).
pub fn carga_do_init(valor: &Value) -> Option<Carga> {
    let servidores = valor["mcp_servers"].as_array()?;
    let quantos = |v: &Value| v.as_array().map_or(0, Vec::len) as u32;
    Some(Carga {
        mcp_ligados: servidores.iter().filter(|s| s["status"] == "connected").count() as u32,
        mcp: servidores.len() as u32,
        ferramentas: quantos(&valor["tools"]),
        skills: quantos(&valor["skills"]),
    })
}

/// Os caracteres da mensagem que abriu a conversa, se ela é a ÚNICA até aqui e foi sem anexo
/// (a imagem pesa no contexto e não dá para estimar). `None` em qualquer outro caso.
pub fn primeira_mensagem(itens: &[Item]) -> Option<usize> {
    let mut aberturas = itens.iter().filter(|i| matches!(i, Item::Usuario { .. } | Item::Entrega { .. }));
    let primeira = aberturas.next()?;
    if aberturas.next().is_some() {
        return None;
    }
    match primeira {
        Item::Usuario { texto, anexos, .. } if anexos.is_empty() => Some(texto.chars().count()),
        Item::Entrega { texto, .. } => Some(texto.chars().count()),
        _ => None,
    }
}

/// Estimativa dos tokens de um texto em português: perto de 3,5 caracteres por token.
pub fn tokens_estimados(caracteres: usize) -> u64 {
    (caracteres as u64 * 2).div_ceil(7)
}

/// De onde vem um arquivo de instrução.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origem {
    /// O `CLAUDE.md` do usuário (`~/.claude`).
    Usuario,
    /// Um `CLAUDE.md` (ou `CLAUDE.local.md`) da pasta ou de uma pasta de cima.
    Pasta,
    /// Trazido por um `@caminho` dentro de outro.
    Importado,
    /// O índice da memória (`MEMORY.md`).
    Memoria,
}

/// Um arquivo de instrução que a pasta carrega, com o tamanho em bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instrucao {
    pub caminho: PathBuf,
    pub bytes: u64,
    pub origem: Origem,
}

/// Até quantos `@imports` encadeados o Claude Code segue.
const PROFUNDIDADE: usize = 5;

/// Os caminhos dos `@imports` de um arquivo de instrução, como o Claude Code os lê: um `@caminho`
/// no começo da linha ou depois de um espaço, fora de bloco de código e de trecho entre crases.
/// O que não é caminho (um @usuario, um e-mail) sai depois, por não existir no disco.
pub fn imports_de(texto: &str) -> Vec<String> {
    let mut achados = Vec::new();
    let mut em_bloco = false;
    for linha in texto.lines() {
        let aparada = linha.trim_start();
        if aparada.starts_with("```") || aparada.starts_with("~~~") {
            em_bloco = !em_bloco;
            continue;
        }
        if em_bloco {
            continue;
        }
        // Os trechos entre crases saem: sobram os pedaços de índice par.
        for (n, pedaco) in linha.split('`').enumerate() {
            if n % 2 == 1 {
                continue;
            }
            for palavra in pedaco.split_whitespace() {
                let Some(caminho) = palavra.strip_prefix('@') else {
                    continue;
                };
                let caminho = caminho.trim_end_matches(['.', ',', ';', ':', ')', ']', '"', '\'']);
                if !caminho.is_empty() && !achados.iter().any(|a| a == caminho) {
                    achados.push(caminho.to_string());
                }
            }
        }
    }
    achados
}

/// O caminho de um `@import`: `~/` é a pasta pessoal, o absoluto fica, e o relativo conta da
/// pasta do arquivo que importa.
fn resolver_import(caminho: &str, de: &Path, casa: Option<&Path>) -> PathBuf {
    if let Some(resto) = caminho.strip_prefix("~/")
        && let Some(casa) = casa
    {
        return casa.join(resto);
    }
    let caminho = Path::new(caminho);
    if caminho.is_absolute() { caminho.to_path_buf() } else { de.parent().unwrap_or(Path::new("/")).join(caminho) }
}

/// Põe na lista o arquivo (se existe e ainda não entrou) e, em seguida, os que ele importa.
fn juntar(lista: &mut Vec<Instrucao>, caminho: &Path, origem: Origem, casa: Option<&Path>, nivel: usize) {
    let Ok(dados) = std::fs::metadata(caminho) else {
        return;
    };
    let real = dunce::canonicalize(caminho).unwrap_or_else(|_| caminho.to_path_buf());
    if !dados.is_file() || lista.iter().any(|i| i.caminho == real) {
        return;
    }
    lista.push(Instrucao { caminho: real.clone(), bytes: dados.len(), origem });
    if nivel >= PROFUNDIDADE {
        return;
    }
    let Ok(texto) = std::fs::read_to_string(&real) else {
        return;
    };
    for import in imports_de(&texto) {
        juntar(lista, &resolver_import(&import, &real, casa), Origem::Importado, casa, nivel + 1);
    }
}

/// Os arquivos de instrução que um chat novo carrega em `pasta`: o `CLAUDE.md` do usuário, os da
/// pasta e das de cima até a raiz do repositório (a pasta com `.git`; o Claude Code não sobe além
/// dela) e os `@imports` de cada um, cada arquivo uma vez só. `config` é a pasta do Claude Code
/// (`~/.claude`), `casa` a pasta pessoal e `memoria` a pasta da memória da pasta.
pub fn instrucoes(pasta: &Path, config: &Path, casa: Option<&Path>, memoria: Option<&Path>) -> Vec<Instrucao> {
    let mut lista = Vec::new();
    juntar(&mut lista, &config.join("CLAUDE.md"), Origem::Usuario, casa, 0);
    // De cima para baixo, como o Claude Code lê: a pasta mais de dentro por último.
    let mut pastas: Vec<&Path> = Vec::new();
    for atual in pasta.ancestors() {
        pastas.push(atual);
        if atual.join(".git").exists() || Some(atual) == casa {
            break;
        }
    }
    // Sem repositório nem a pasta pessoal no caminho, vale só a própria pasta.
    let achou_teto = pastas.last().is_some_and(|p| p.join(".git").exists() || Some(*p) == casa);
    if !achou_teto {
        pastas.truncate(1);
    }
    for atual in pastas.into_iter().rev() {
        for nome in ["CLAUDE.md", ".claude/CLAUDE.md", "CLAUDE.local.md"] {
            juntar(&mut lista, &atual.join(nome), Origem::Pasta, casa, 0);
        }
    }
    if let Some(memoria) = memoria {
        juntar(&mut lista, &memoria.join("MEMORY.md"), Origem::Memoria, casa, PROFUNDIDADE);
    }
    lista
}

/// A pasta da memória de uma pasta de trabalho: o `autoMemoryDirectory` do `settings.json` do
/// Claude Code, quando há, ou `projects/<pasta codificada>/memory`.
pub fn pasta_da_memoria(pasta: &Path, config: &Path, casa: Option<&Path>) -> PathBuf {
    let escolhida = std::fs::read_to_string(config.join("settings.json"))
        .ok()
        .and_then(|texto| serde_json::from_str::<Value>(&texto).ok())
        .and_then(|v| v["autoMemoryDirectory"].as_str().map(str::to_string))
        .filter(|s| !s.trim().is_empty());
    match escolhida {
        Some(caminho) => resolver_import(&caminho, &config.join("settings.json"), casa),
        None => {
            let real = dunce::canonicalize(pasta).unwrap_or_else(|_| pasta.to_path_buf());
            config.join("projects").join(crate::claude::nome_do_projeto(&real.to_string_lossy())).join("memory")
        }
    }
}

/// As bases medidas, por pasta, e a carga do `init` de cada turno em andamento.
#[derive(Debug, Default)]
pub struct Pesos {
    por_pasta: HashMap<String, Peso>,
    cargas: HashMap<String, Carga>,
}

impl Pesos {
    /// Lê o `peso-das-pastas.json`; ausente ou ilegível, começa vazio (é só uma medida).
    pub fn ler(arquivo: &Path) -> Pesos {
        let por_pasta = std::fs::read_to_string(arquivo)
            .ok()
            .and_then(|texto| serde_json::from_str(&texto).ok())
            .unwrap_or_default();
        Pesos { por_pasta, cargas: HashMap::new() }
    }

    pub fn da_pasta(&self, pasta: &Path) -> Option<&Peso> {
        self.por_pasta.get(pasta.to_string_lossy().as_ref())
    }
}

/// A base de um chat que começa do zero: o primeiro contexto menos a primeira mensagem. `None`
/// quando o chat não serve de medida (ver o topo do arquivo).
pub fn base_medida(chat: &Chat, contexto: u64) -> Option<u64> {
    if chat.agente != Agente::Claude || chat.contexto.is_some() || chat.importada.is_some() {
        return None;
    }
    let mensagem = tokens_estimados(primeira_mensagem(&chat.itens)?);
    Some(contexto.saturating_sub(mensagem)).filter(|base| *base > 0)
}

impl Motor {
    /// A base medida da pasta, se algum chat novo já rodou nela.
    pub fn peso_da_pasta(&self, pasta: &Path) -> Option<&Peso> {
        self.pesos.da_pasta(pasta)
    }

    /// O `init` do turno disse o que carregou: fica à espera do primeiro contexto.
    pub(super) fn anotar_carga(&mut self, chat_id: &str, carga: Carga) {
        self.pesos.cargas.insert(chat_id.to_string(), carga);
    }

    /// Chegou um `Evento::Contexto` (ainda não gravado no chat): se é o primeiro de um chat que
    /// começa do zero, vira a base medida da pasta dele.
    pub(super) fn medir_peso(&mut self, chat_id: &str, contexto: u64) {
        let carga = self.pesos.cargas.remove(chat_id);
        let Some(c) = self.chat(chat_id) else {
            return;
        };
        let Some(tokens) = base_medida(c, contexto) else {
            return;
        };
        let peso = Peso { tokens, em: agora(), modelo: c.modelo_atual.clone().unwrap_or_default(), carga };
        self.pesos.por_pasta.insert(c.cwd.to_string_lossy().into_owned(), peso);
        if self.somente_leitura.is_none()
            && let Ok(bytes) = serde_json::to_vec_pretty(&self.pesos.por_pasta)
        {
            self.dados.salvar_pesos(bytes);
        }
    }
}

#[cfg(test)]
mod testes {
    use super::{Carga, Origem, base_medida, carga_do_init, imports_de, instrucoes, pasta_da_memoria, primeira_mensagem, tokens_estimados};
    use crate::modelo::{Chat, Item};
    use serde_json::json;
    use std::{fs, path::PathBuf};

    fn usuario(texto: &str, anexos: Vec<PathBuf>) -> Item {
        Item::Usuario { texto: texto.into(), anexos, ultrathink: false, ultracode: false, em: 1, marca: None }
    }

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-peso-{nome}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&pasta).unwrap();
        dunce::canonicalize(&pasta).unwrap()
    }

    #[test]
    fn imports_de_um_arquivo_de_instrucao() {
        let texto = "@AGENTS.md\n\
            Veja também @docs/regras.md, e @~/notas/minhas.md.\n\
            O pacote `@types/node` e o e-mail fulano@exemplo.com não são imports.\n\
            ```\n@dentro/do/bloco.md\n```\n\
            ~~~\n@outro/bloco.md\n~~~\n\
            De novo @AGENTS.md (uma vez só) e @/abs/caminho.md";
        assert_eq!(imports_de(texto), ["AGENTS.md", "docs/regras.md", "~/notas/minhas.md", "/abs/caminho.md"]);
        assert!(imports_de("sem import nenhum, só um @ solto").is_empty());
    }

    #[test]
    fn le_os_imports_encadeados_da_pasta_do_usuario_e_a_memoria() {
        let raiz = pasta_de_teste("instrucoes");
        let (casa, config) = (raiz.join("casa"), raiz.join("casa/.claude"));
        let projeto = raiz.join("casa/trabalho");
        let dentro = projeto.join("central");
        let memoria = config.join("memoria");
        for pasta in [&config, &dentro, &memoria, &projeto.join(".git"), &projeto.join("docs")] {
            fs::create_dir_all(pasta).unwrap();
        }
        let (do_usuario, do_projeto) = ("do usuário\n@~/trabalho/AGENTS.md\n", "@AGENTS.md\n");
        let (agentes, regras) = ("mapa\n@docs/regras.md\n@nao-existe.md\n", "regras, e de volta ao @../AGENTS.md\n");
        let indice = "índice\n@nao-segue.md\n";
        fs::write(config.join("CLAUDE.md"), do_usuario).unwrap();
        fs::write(projeto.join("CLAUDE.md"), do_projeto).unwrap();
        fs::write(projeto.join("AGENTS.md"), agentes).unwrap();
        fs::write(projeto.join("docs/regras.md"), regras).unwrap();
        fs::write(dentro.join("CLAUDE.md"), "da central").unwrap();
        fs::write(dentro.join("CLAUDE.local.md"), "local").unwrap();
        fs::write(memoria.join("MEMORY.md"), indice).unwrap();
        fs::write(memoria.join("nao-segue.md"), "x").unwrap();
        // Acima do repositório: não entra.
        fs::write(casa.join("CLAUDE.md"), "de fora").unwrap();
        let lista = instrucoes(&dentro, &config, Some(&casa), Some(&memoria));
        let nomes: Vec<(String, Origem, usize)> = lista
            .iter()
            .map(|i| (i.caminho.strip_prefix(&casa).unwrap().to_string_lossy().replace('\\', "/"), i.origem, i.bytes as usize))
            .collect();
        assert_eq!(
            nomes,
            [
                (".claude/CLAUDE.md".to_string(), Origem::Usuario, do_usuario.len()),
                // Importado pelo do usuário: entra uma vez só, e o CLAUDE.md da pasta não o repete.
                ("trabalho/AGENTS.md".to_string(), Origem::Importado, agentes.len()),
                ("trabalho/docs/regras.md".to_string(), Origem::Importado, regras.len()),
                ("trabalho/CLAUDE.md".to_string(), Origem::Pasta, do_projeto.len()),
                ("trabalho/central/CLAUDE.md".to_string(), Origem::Pasta, 10),
                ("trabalho/central/CLAUDE.local.md".to_string(), Origem::Pasta, 5),
                // O índice da memória entra sem seguir os @ dele.
                (".claude/memoria/MEMORY.md".to_string(), Origem::Memoria, indice.len()),
            ]
        );
        // Pasta sem repositório e fora da pasta pessoal: só os arquivos dela.
        let solta = raiz.join("solta/dentro");
        fs::create_dir_all(&solta).unwrap();
        fs::write(raiz.join("solta/CLAUDE.md"), "de cima").unwrap();
        fs::write(solta.join("CLAUDE.md"), "a própria").unwrap();
        let lista = instrucoes(&solta, &raiz.join("sem-config"), Some(&casa), None);
        assert_eq!(lista.len(), 1);
        assert!(lista[0].caminho.ends_with("solta/dentro/CLAUDE.md"));
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn memoria_escolhida_no_settings_ou_a_da_pasta() {
        let raiz = pasta_de_teste("memoria");
        let config = raiz.join(".claude");
        fs::create_dir_all(&config).unwrap();
        let pasta = raiz.join("proj");
        fs::create_dir_all(&pasta).unwrap();
        let padrao = pasta_da_memoria(&pasta, &config, Some(&raiz));
        assert!(padrao.starts_with(config.join("projects")) && padrao.ends_with("memory"), "{padrao:?}");
        fs::write(config.join("settings.json"), r#"{"autoMemoryDirectory": "~/uma/memoria"}"#).unwrap();
        assert_eq!(pasta_da_memoria(&pasta, &config, Some(&raiz)), raiz.join("uma/memoria"));
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn base_so_do_chat_que_comeca_do_zero() {
        let mut chat = Chat::novo("c".into(), "t", PathBuf::from("/p"));
        // 70 caracteres são 20 tokens: saem do primeiro contexto.
        chat.itens = vec![usuario(&"a".repeat(70), Vec::new())];
        assert_eq!(tokens_estimados(70), 20);
        assert_eq!(base_medida(&chat, 41_020), Some(41_000));
        // O segundo contexto da conversa já não é a base.
        chat.contexto = Some(41_020);
        assert_eq!(base_medida(&chat, 55_000), None);
        chat.contexto = None;
        // Com anexo, com duas mensagens ou sem nenhuma: não mede.
        chat.itens = vec![usuario("oi", vec![PathBuf::from("/x.png")])];
        assert_eq!(base_medida(&chat, 41_000), None);
        chat.itens = vec![usuario("oi", Vec::new()), usuario("de novo", Vec::new())];
        assert_eq!(primeira_mensagem(&chat.itens), None);
        assert_eq!(base_medida(&chat, 41_000), None);
        assert_eq!(base_medida(&Chat::novo("v".into(), "t", PathBuf::from("/p")), 41_000), None);
        // O Codex não manda o contexto, e não mede.
        chat.itens = vec![usuario("oi", Vec::new())];
        chat.agente = crate::modelo::Agente::Codex;
        assert_eq!(base_medida(&chat, 41_000), None);
    }

    #[test]
    fn carga_declarada_no_init() {
        let init = json!({
            "type": "system", "subtype": "init",
            "tools": ["Bash", "Read", "Edit"],
            "skills": ["easypanel"],
            "mcp_servers": [{ "name": "central", "status": "connected" }, { "name": "gmail", "status": "needs-auth" }]
        });
        assert_eq!(carga_do_init(&init), Some(Carga { mcp_ligados: 1, mcp: 2, ferramentas: 3, skills: 1 }));
        assert_eq!(carga_do_init(&json!({ "type": "system", "subtype": "init", "tools": ["Bash"] })), None);
    }
}
