//! A conversa de um subagente, lida do arquivo que o Claude Code grava para ele: ao lado da
//! sessão `<projeto>/<sessão>.jsonl` fica a pasta `<projeto>/<sessão>/subagents/`, com um
//! `agent-<id>.jsonl` por subagente (uma entrada por linha, todas com `isSidechain`) e um
//! `agent-<id>.meta.json` que diz qual chamada o abriu (`toolUseId`). Medido no Claude Code
//! 2.1.328 (23/11/2026): o arquivo é gravado aos poucos, enquanto o subagente trabalha, e guarda
//! tudo, o pedido, o raciocínio, cada chamada com o resultado e o que ele respondeu. O
//! stream-json só entrega as chamadas e os resultados (o texto, só com
//! `--forward-subagent-text`), e nada disso depois que o processo sai. Por isso a conversa do
//! subagente vem sempre daqui, ao vivo e depois de reabrir o Vitral. Só leitura.

use std::{
    fs,
    io::BufReader,
    path::{Path, PathBuf},
};

use super::{Fio, itens_do_fio};
use crate::modelo::Item;

/// A chamada com que o subagente entrega o relatório a quem o abriu (Claude Code 2.1.271 em
/// diante, no modo automático): o relatório vai no `message` dela, e o texto final não vem.
pub const RELATORIO: &str = "SubagentHandback";

/// Quanto do resultado de cada chamada do subagente fica na conversa dele (como ao vivo).
const MAX_RESULTADO: usize = 20_000;

/// O arquivo da conversa do subagente aberto pela chamada `chamada`: primeiro entre os da sessão
/// do chat, depois entre os das outras sessões das mesmas pastas de projeto (o chat que continuou
/// noutra sessão, ou nasceu de outro, guarda o cartão de um subagente que rodou na anterior).
/// `None` quando o Claude Code não o tem mais (ele apaga as sessões velhas) ou nunca o gravou.
pub fn transcript(projetos: &[PathBuf], sessao: &str, chamada: &str) -> Option<PathBuf> {
    let da_sessao = projetos.iter().find_map(|projeto| entre_os_da_pasta(&projeto.join(sessao).join("subagents"), chamada));
    da_sessao.or_else(|| {
        projetos.iter().find_map(|projeto| {
            fs::read_dir(projeto)
                .ok()?
                .flatten()
                .map(|entrada| entrada.path())
                .filter(|pasta| pasta.file_name().is_some_and(|nome| nome != sessao))
                .find_map(|pasta| entre_os_da_pasta(&pasta.join("subagents"), chamada))
        })
    })
}

/// O `agent-<id>.jsonl` da pasta `subagents` cujo `agent-<id>.meta.json` diz que foi a chamada
/// `chamada` que o abriu.
fn entre_os_da_pasta(pasta: &Path, chamada: &str) -> Option<PathBuf> {
    fs::read_dir(pasta).ok()?.flatten().map(|entrada| entrada.path()).find_map(|meta| {
        let nome = meta.file_name()?.to_str()?;
        let agente = nome.strip_prefix("agent-")?.strip_suffix(".meta.json")?;
        let dados: serde_json::Value = serde_json::from_slice(&fs::read(&meta).ok()?).ok()?;
        let conversa = pasta.join(format!("agent-{agente}.jsonl"));
        (dados["toolUseId"] == chamada && conversa.is_file()).then_some(conversa)
    })
}

/// A conversa inteira do subagente, na ordem: o pedido que ele recebeu (entregue por `de`), o
/// raciocínio, cada chamada com o resultado e o que ele disse.
pub fn conversa(arquivo: &Path, de: &str) -> std::io::Result<Vec<Item>> {
    let leitor = BufReader::with_capacity(1 << 16, fs::File::open(arquivo)?);
    Ok(itens_do_fio(leitor, super::MAX_ITENS, Fio::Subagente { de, resultado: MAX_RESULTADO }).itens)
}

#[cfg(test)]
mod testes {
    use super::*;
    use serde_json::json;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-subagente-{nome}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&pasta);
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    /// Uma linha do arquivo do subagente, com os campos que o Claude Code 2.1.328 grava.
    fn linha(tipo: &str, quando: &str, mensagem: serde_json::Value) -> String {
        json!({
            "parentUuid": null, "isSidechain": true, "agentId": "a9548503256f36964", "type": tipo,
            "timestamp": quando, "sessionId": "s", "version": "2.1.328", "cwd": "/tmp/p", "message": mensagem,
        })
        .to_string()
    }

    /// O arquivo como o Claude Code o gravou para um subagente que leu um arquivo, rodou um
    /// comando e respondeu (medido em 23/11/2026, com o Haiku): o pedido é uma mensagem do usuário
    /// com o texto solto, entre as linhas há anexos e as gravações das chamadas à API, e o
    /// raciocínio vem vazio.
    fn transcript_medido() -> String {
        [
            linha("user", "2026-11-23T16:33:32.501Z", json!({ "role": "user", "content": "Leia notas.txt, rode `wc -l notas.txt` e diga a última palavra." })),
            json!({ "isSidechain": true, "type": "attachment", "attachment": { "type": "x" }, "timestamp": "2026-11-23T16:33:32.534Z" }).to_string(),
            r#"{"type":"api-request-blob","sessionId":"s","agentId":"a9548503256f36964","hash":"fa0e","content":[{"type":"text","text":"Leia notas.txt"}]}"#.to_string(),
            r#"{"type":"api-request","sessionId":"s","agentId":"a9548503256f36964","id":"fc9f"}"#.to_string(),
            linha("assistant", "2026-11-23T16:33:33.598Z", json!({ "id": "m1", "role": "assistant", "content": [{ "type": "thinking", "thinking": "" }] })),
            linha("assistant", "2026-11-23T16:33:33.762Z", json!({ "id": "m1", "role": "assistant", "content": [
                { "type": "tool_use", "id": "toolu_ler", "name": "Read", "input": { "file_path": "/tmp/p/notas.txt" } }] })),
            linha("user", "2026-11-23T16:33:33.906Z", json!({ "role": "user", "content": [
                { "tool_use_id": "toolu_ler", "type": "tool_result", "content": "1\tlinha um\n2\tlinha dois\n3\tbanana" }] })),
            linha("assistant", "2026-11-23T16:33:34.011Z", json!({ "id": "m1", "role": "assistant", "content": [
                { "type": "tool_use", "id": "toolu_wc", "name": "Bash", "input": { "command": "wc -l notas.txt", "description": "Count lines" } }] })),
            linha("user", "2026-11-23T16:33:34.468Z", json!({ "role": "user", "content": [
                { "tool_use_id": "toolu_wc", "type": "tool_result", "content": "3 notas.txt", "is_error": false }] })),
            linha("assistant", "2026-11-23T16:33:36.627Z", json!({ "id": "m2", "role": "assistant", "content": [{ "type": "thinking", "thinking": "São três linhas." }] })),
            linha("assistant", "2026-11-23T16:33:36.732Z", json!({ "id": "m2", "role": "assistant", "content": [
                { "type": "text", "text": "Última palavra: **banana**. `wc -l notas.txt` deu **3**." }] })),
        ]
        .join("\n")
    }

    fn em_texto(itens: &[Item]) -> Vec<String> {
        itens
            .iter()
            .map(|item| match item {
                Item::Entrega { de, texto, .. } => format!("E:{de}={texto}"),
                Item::Usuario { texto, .. } => format!("U:{texto}"),
                Item::Assistente { texto, .. } => format!("A:{texto}"),
                Item::Raciocinio { texto, .. } => format!("R:{texto}"),
                Item::Ferramenta { resumo, resultado, erro, .. } => {
                    format!("F:{resumo}={}{}", resultado.as_deref().unwrap_or("(sem resultado)"), if *erro { " (erro)" } else { "" })
                }
                Item::Aviso { texto, .. } => format!("!:{texto}"),
                outro => format!("{outro:?}"),
            })
            .collect()
    }

    /// Item 2: a conversa inteira do subagente, o que ele leu, rodou e respondeu.
    #[test]
    fn le_a_conversa_inteira_do_subagente() {
        let raiz = pasta_de_teste("conversa");
        let arquivo = raiz.join("agent-a9548503256f36964.jsonl");
        fs::write(&arquivo, transcript_medido()).unwrap();
        let itens = conversa(&arquivo, "chat ab12cd34ef \"Carrinho\"").unwrap();
        assert_eq!(
            em_texto(&itens),
            [
                "E:chat ab12cd34ef \"Carrinho\"=Leia notas.txt, rode `wc -l notas.txt` e diga a última palavra.",
                "F:Read · notas.txt=1\tlinha um\n2\tlinha dois\n3\tbanana",
                "F:Bash · Count lines=3 notas.txt",
                "R:São três linhas.",
                "A:Última palavra: **banana**. `wc -l notas.txt` deu **3**.",
            ]
        );
        // A hora de cada item é a da linha do arquivo.
        assert_eq!(itens[0].em(), 1795451612);
        // O arquivo que ainda está sendo gravado (a última linha pela metade) dá o que já está lá.
        fs::write(&arquivo, format!("{}\n{{\"isSidechain\":true,\"type\":\"assis", transcript_medido())).unwrap();
        assert_eq!(conversa(&arquivo, "x").unwrap().len(), 5);
        // Sem o arquivo, o erro de leitura (quem chama mostra o que o cartão guardou).
        assert!(conversa(&raiz.join("nao-existe.jsonl"), "x").is_err());
    }

    /// No modo automático (2.1.271 em diante) o subagente entrega o relatório pela chamada
    /// `SubagentHandback`, e o texto final não vem: o relatório é a resposta dele.
    #[test]
    fn o_relatorio_entregue_pela_chamada_vira_a_resposta() {
        let raiz = pasta_de_teste("relatorio");
        let arquivo = raiz.join("agent-a1.jsonl");
        let conteudo = [
            linha("user", "2026-11-23T16:41:19.000Z", json!({ "role": "user", "content": "Leia notas.txt e diga a última palavra." })),
            linha("assistant", "2026-11-23T16:41:20.000Z", json!({ "id": "m1", "content": [
                { "type": "tool_use", "id": "toolu_ler", "name": "Read", "input": { "file_path": "notas.txt" } }] })),
            linha("user", "2026-11-23T16:41:20.500Z", json!({ "content": [{ "tool_use_id": "toolu_ler", "type": "tool_result", "content": "banana" }] })),
            linha("assistant", "2026-11-23T16:41:21.000Z", json!({ "id": "m2", "content": [
                { "type": "tool_use", "id": "toolu_entrega", "name": "SubagentHandback", "input": { "message": "A última palavra é banana." } }] })),
            linha("user", "2026-11-23T16:41:21.100Z", json!({ "content": [{ "tool_use_id": "toolu_entrega", "type": "tool_result",
                "content": [{ "type": "text", "text": "{\"success\":true,\"message\":\"Report delivered to your caller.\"}" }] }] })),
        ]
        .join("\n");
        fs::write(&arquivo, conteudo).unwrap();
        assert_eq!(
            em_texto(&conversa(&arquivo, "Claude").unwrap()),
            ["E:Claude=Leia notas.txt e diga a última palavra.", "F:Read · notas.txt=banana", "A:A última palavra é banana."]
        );
    }

    /// Depois de reabrir o Vitral, a conversa do subagente é achada pela chamada que o abriu.
    #[test]
    fn acha_o_arquivo_pela_chamada_que_abriu_o_subagente() {
        let raiz = pasta_de_teste("transcript");
        let projeto = raiz.join("projects").join("-tmp-p");
        let gravar = |sessao: &str, agente: &str, chamada: Option<&str>, com_conversa: bool| {
            let pasta = projeto.join(sessao).join("subagents");
            fs::create_dir_all(&pasta).unwrap();
            let meta = match chamada {
                Some(chamada) => json!({ "agentType": "general-purpose", "description": "x", "toolUseId": chamada, "spawnDepth": 1 }),
                None => json!({ "agentType": "general-purpose", "spawnDepth": 1 }),
            };
            fs::write(pasta.join(format!("agent-{agente}.meta.json")), meta.to_string()).unwrap();
            if com_conversa {
                fs::write(pasta.join(format!("agent-{agente}.jsonl")), "").unwrap();
            }
            pasta.join(format!("agent-{agente}.jsonl"))
        };
        let da_sessao = gravar("sessao-atual", "a1", Some("toolu_um"), true);
        gravar("sessao-atual", "a2", Some("toolu_dois"), true);
        gravar("sessao-atual", "a3", None, true);
        let da_anterior = gravar("sessao-anterior", "b1", Some("toolu_velha"), true);
        gravar("sessao-anterior", "b2", Some("toolu_sem_conversa"), false);
        // Um arquivo estragado entre os meta não atrapalha.
        fs::write(projeto.join("sessao-atual").join("subagents").join("agent-zz.meta.json"), "{").unwrap();
        let projetos = [raiz.join("projects").join("-nao-existe"), projeto.clone()];
        assert_eq!(transcript(&projetos, "sessao-atual", "toolu_um"), Some(da_sessao));
        // O subagente de uma sessão anterior do mesmo projeto (o chat continuou noutra).
        assert_eq!(transcript(&projetos, "sessao-atual", "toolu_velha"), Some(da_anterior));
        // Sem o arquivo da conversa, sem a chamada ou sem a pasta: nada.
        assert_eq!(transcript(&projetos, "sessao-atual", "toolu_sem_conversa"), None);
        assert_eq!(transcript(&projetos, "sessao-atual", "toolu_que_nao_existe"), None);
        assert_eq!(transcript(&[], "sessao-atual", "toolu_um"), None);
    }
}
