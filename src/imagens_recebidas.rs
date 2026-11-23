//! As imagens que uma ferramenta devolve no resultado (o print do navegador por MCP, a imagem
//! gerada, o Read de uma imagem): o base64 vira um arquivo na pasta do chat, dentro dos dados do
//! Vitral (`imagens/<chat>/`), e a conversa guarda só o caminho. O histórico e o estado nunca
//! recebem o base64: a `Item::Ferramenta` tem os caminhos, e a interface mostra a miniatura que
//! abre no visor, como a das imagens anexadas.
//!
//! O espaço tem três tetos: por imagem (`TETO_POR_IMAGEM`; a maior não é guardada, e o resultado
//! diz isso), por chat (`TETO_POR_CHAT`; passou, saem as mais antigas da pasta dele) e de todas
//! as conversas juntas (`TETO_TOTAL`, conferido ao abrir o Vitral). A limpeza: a conversa que sai
//! do Vitral leva a pasta dela (`Dados::apagar_chat`), e ao abrir saem as pastas de chats que não
//! existem mais (`varrer`).
//!
//! A ideia veio do Paseo (`provider-image-output.ts`): o nome do arquivo é o hash do conteúdo,
//! então a mesma imagem recebida duas vezes é gravada uma vez só.

use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
};

use serde_json::Value;

use crate::modelo::Item;

/// A maior imagem que se guarda. A API do Claude aceita até 5 MB por imagem e o Claude Code
/// encolhe o que passa disso; os 10 MB dão folga para o que um MCP ou o Codex mandem.
pub const TETO_POR_IMAGEM: usize = 10 * 1024 * 1024;
/// Quanto as imagens de um chat ocupam no máximo: passou, saem as mais antigas. Um print de
/// navegador tem de 100 KB a 1 MB, então cabem de cem a mil.
pub const TETO_POR_CHAT: u64 = 100 * 1024 * 1024;
/// Quanto as imagens de todas as conversas ocupam no máximo, conferido ao abrir o Vitral.
pub const TETO_TOTAL: u64 = 1024 * 1024 * 1024;

/// Uma imagem que chegou num resultado, já fora do base64 e ainda fora do disco.
pub struct ImagemRecebida {
    /// "png", "jpg", "gif" ou "webp": os formatos que a conversa sabe mostrar.
    pub extensao: &'static str,
    pub bytes: Vec<u8>,
}

/// Só o formato e o tamanho: os bytes de uma imagem não vão para log nenhum.
impl std::fmt::Debug for ImagemRecebida {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImagemRecebida({}, {} bytes)", self.extensao, self.bytes.len())
    }
}

/// O bloco de imagem de um resultado: no formato da API do Claude (`source.data`, com o
/// `media_type`) ou no do MCP, que é como o Codex o repassa (`data` e `mimeType`). `None` quando o
/// bloco não é imagem. A imagem que não dá para guardar (grande demais, formato que a conversa
/// não mostra, base64 quebrado) devolve o texto que fica no lugar dela no resultado.
pub fn do_bloco(bloco: &Value) -> Option<Result<ImagemRecebida, String>> {
    if bloco["type"] != "image" {
        return None;
    }
    let fonte = &bloco["source"];
    let (tipo, dados) = if fonte.is_object() {
        (fonte["media_type"].as_str(), fonte["data"].as_str())
    } else {
        (bloco["mimeType"].as_str(), bloco["data"].as_str())
    };
    Some(de_base64(tipo.unwrap_or_default(), dados.unwrap_or_default()))
}

/// Uma imagem em base64 (puro ou numa URL `data:image/png;base64,…`), com o tipo que quem mandou
/// declarou. O formato vale pelo começo dos bytes; o tipo declarado só desempata.
pub fn de_base64(tipo: &str, dados: &str) -> Result<ImagemRecebida, String> {
    let (tipo, dados) = match dados.strip_prefix("data:").and_then(|resto| resto.split_once(";base64,")) {
        Some((da_url, puro)) if da_url.len() < 100 => (da_url, puro),
        _ => (tipo, dados),
    };
    let nome = if tipo.trim().is_empty() { "imagem" } else { tipo.trim() };
    // Antes de decodificar: o que nem de longe cabe no teto não ocupa memória à toa (a folga é
    // para as quebras de linha de um base64 em colunas).
    let estimado = dados.len() / 4 * 3;
    if estimado > TETO_POR_IMAGEM + TETO_POR_IMAGEM / 8 {
        return Err(marca_do_teto(nome, estimado));
    }
    let Some(bytes) = decodificar(dados).filter(|b| !b.is_empty()) else {
        return Err(format!("({nome})"));
    };
    if bytes.len() > TETO_POR_IMAGEM {
        return Err(marca_do_teto(nome, bytes.len()));
    }
    let Some(extensao) = extensao_do_conteudo(&bytes).or_else(|| extensao_do_tipo(tipo)) else {
        return Err(format!("({nome})"));
    };
    Ok(ImagemRecebida { extensao, bytes })
}

/// Uma imagem que já está num arquivo de fora (a que o Codex gerou e gravou na pasta dele), para
/// o Vitral guardar a cópia dele. `None` quando não dá para ler, passa do teto ou não é imagem.
pub fn do_arquivo(caminho: &Path) -> Option<ImagemRecebida> {
    let tamanho = fs::metadata(caminho).ok()?.len();
    if tamanho == 0 || tamanho > TETO_POR_IMAGEM as u64 {
        return None;
    }
    let bytes = fs::read(caminho).ok()?;
    Some(ImagemRecebida { extensao: extensao_do_conteudo(&bytes)?, bytes })
}

/// O que fica no resultado no lugar da imagem grande demais: "(image/png, 14,2 MB: passa do teto
/// de 10 MB e não foi guardada)".
fn marca_do_teto(nome: &str, bytes: usize) -> String {
    let mb = |n: usize| format!("{:.1}", n as f64 / 1_048_576.).replace('.', ",");
    format!("({nome}, {} MB: passa do teto de {} MB e não foi guardada)", mb(bytes), TETO_POR_IMAGEM / 1_048_576)
}

/// O formato pela assinatura do arquivo: os quatro que a conversa mostra (`claude::tipo_imagem`).
fn extensao_do_conteudo(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        Some("webp")
    } else {
        None
    }
}

fn extensao_do_tipo(tipo: &str) -> Option<&'static str> {
    match tipo.trim().to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// O base64 de volta aos bytes: o alfabeto comum e o de URL, com ou sem o `=` do fim, pulando
/// espaços e quebras de linha. `None` quando há um caractere de fora.
fn decodificar(texto: &str) -> Option<Vec<u8>> {
    let mut saida = Vec::with_capacity(texto.len() / 4 * 3 + 3);
    let (mut acumulado, mut bits) = (0u32, 0u8);
    for byte in texto.bytes() {
        let valor = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' => break,
            b' ' | b'\n' | b'\r' | b'\t' => continue,
            _ => return None,
        };
        acumulado = acumulado << 6 | valor as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            saida.push((acumulado >> bits) as u8);
            acumulado &= (1 << bits) - 1;
        }
    }
    Some(saida)
}

/// Separa o conteúdo de um resultado no texto e nas imagens. O texto leva, no lugar de cada
/// imagem que não deu para guardar, a marca dela.
pub fn partes_do_resultado(conteudo: &Value) -> (String, Vec<ImagemRecebida>) {
    let mut imagens = Vec::new();
    let texto = match conteudo {
        Value::String(s) => s.clone(),
        Value::Array(partes) => partes
            .iter()
            .filter_map(|parte| match do_bloco(parte) {
                Some(Ok(imagem)) => {
                    imagens.push(imagem);
                    None
                }
                Some(Err(marca)) => Some(marca),
                None => parte["text"].as_str().map(str::to_string),
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        outro => outro.to_string(),
    };
    (texto, imagens)
}

/// O nome do arquivo de uma imagem: o começo do SHA-256 do conteúdo (16 dígitos, para o nome
/// caber no visor) e a extensão do formato.
fn nome_do_arquivo(imagem: &ImagemRecebida) -> String {
    use sha2::{Digest as _, Sha256};
    let hash = Sha256::digest(&imagem.bytes);
    let hex: String = hash.iter().take(8).map(|b| format!("{b:02x}")).collect();
    format!("imagem-{hex}.{}", imagem.extensao)
}

/// Grava a imagem em `pasta` (a do chat) e devolve o caminho. O nome é o hash do conteúdo: a
/// mesma imagem de novo não grava outro arquivo, só renova a data do que já está lá.
pub fn guardar(pasta: &Path, imagem: &ImagemRecebida) -> io::Result<PathBuf> {
    criar_pasta(pasta)?;
    let caminho = pasta.join(nome_do_arquivo(imagem));
    // Já está lá, e inteira (uma queda no meio da gravação deixaria um arquivo menor).
    if fs::metadata(&caminho).is_ok_and(|m| m.is_file() && m.len() == imagem.bytes.len() as u64) {
        if let Ok(arquivo) = fs::File::options().write(true).open(&caminho) {
            let _ = arquivo.set_modified(std::time::SystemTime::now());
        }
        return Ok(caminho);
    }
    // Num arquivo ao lado e então o nome certo: quem lê a pasta nunca vê uma imagem pela metade.
    let parcial = pasta.join(format!(".parcial-{}", crate::modelo::novo_id()));
    let gravado = gravar_so_do_dono(&parcial, &imagem.bytes).and_then(|()| crate::dados::renomear(&parcial, &caminho));
    if gravado.is_err() {
        let _ = fs::remove_file(&parcial);
    }
    gravado.map(|()| caminho)
}

/// A pasta das imagens de um chat, só do dono: um print de tela pode mostrar o que estava aberto
/// no computador.
fn criar_pasta(pasta: &Path) -> io::Result<()> {
    if pasta.is_dir() {
        return Ok(());
    }
    fs::create_dir_all(pasta)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(pasta, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn gravar_so_do_dono(caminho: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write as _;
    let mut opcoes = fs::File::options();
    opcoes.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        opcoes.mode(0o600);
    }
    opcoes.open(caminho)?.write_all(bytes)
}

/// As imagens de uma pasta, da mais antiga para a mais nova: o caminho, o tamanho e a data.
fn imagens_de(pasta: &Path) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut lista: Vec<_> = fs::read_dir(pasta)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entrada| {
            let dados = entrada.metadata().ok().filter(|m| m.is_file())?;
            Some((entrada.path(), dados.len(), dados.modified().ok()?))
        })
        .collect();
    lista.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    lista
}

/// Apaga as primeiras de `lista` (as mais antigas) até o total caber em `teto`, pulando as de
/// `poupar`. Devolve as que saíram.
fn tirar_ate_caber(lista: Vec<(PathBuf, u64, std::time::SystemTime)>, teto: u64, poupar: &[PathBuf]) -> Vec<PathBuf> {
    let mut total: u64 = lista.iter().map(|(_, tamanho, _)| tamanho).sum();
    let mut sairam = Vec::new();
    for (caminho, tamanho, _) in lista {
        if total <= teto {
            break;
        }
        if poupar.contains(&caminho) || fs::remove_file(&caminho).is_err() {
            continue;
        }
        total -= tamanho;
        sairam.push(caminho);
    }
    sairam
}

/// Tira de `pasta` as imagens mais antigas até o que sobra caber em `teto`. As de `poupar` (as
/// que acabaram de chegar) ficam. Devolve as que saíram.
pub fn podar(pasta: &Path, teto: u64, poupar: &[PathBuf]) -> Vec<PathBuf> {
    tirar_ate_caber(imagens_de(pasta), teto, poupar)
}

/// O que o motor faz com as imagens de um resultado: grava cada uma na pasta do chat, mantém a
/// pasta no teto e devolve os caminhos. A que não gravou (disco cheio, sem permissão) deixa a
/// marca dela no fim de `conteudo`.
pub fn guardar_todas(pasta: &Path, imagens: &[ImagemRecebida], conteudo: &mut String) -> Vec<PathBuf> {
    let mut caminhos = Vec::new();
    for imagem in imagens {
        match guardar(pasta, imagem) {
            Ok(caminho) => {
                if !caminhos.contains(&caminho) {
                    caminhos.push(caminho);
                }
            }
            Err(erro) => {
                if !conteudo.is_empty() {
                    conteudo.push('\n');
                }
                conteudo.push_str(&format!("(imagem {}: não deu para guardar: {erro})", imagem.extensao));
            }
        }
    }
    if !caminhos.is_empty() {
        podar(pasta, TETO_POR_CHAT, &caminhos);
    }
    caminhos
}

/// As imagens que o resultado da chamada `id` trouxe: vão para o disco, em `pasta`, e a chamada
/// guarda os caminhos. A que não gravou deixa a marca dela no fim do resultado. Sem a chamada na
/// conversa (as de bastidor, que o Vitral não mostra), nada é gravado. Devolve se achou a chamada.
pub fn guardar_na_chamada(itens: &mut [Item], id: &str, pasta: &Path, imagens: &[ImagemRecebida]) -> bool {
    let Some((resultado, da_chamada)) = itens.iter_mut().rev().find_map(|item| match item {
        Item::Ferramenta { id: id_item, resultado, imagens, .. } if id_item == id => Some((resultado, imagens)),
        _ => None,
    }) else {
        return false;
    };
    let mut marcas = String::new();
    for caminho in guardar_todas(pasta, imagens, &mut marcas) {
        if !da_chamada.contains(&caminho) {
            da_chamada.push(caminho);
        }
    }
    if !marcas.is_empty() {
        let texto = resultado.get_or_insert_with(String::new);
        if !texto.is_empty() {
            texto.push('\n');
        }
        texto.push_str(&marcas);
    }
    true
}

/// A pasta das imagens de um chat que saiu do Vitral.
pub fn apagar_do_chat(pasta: &Path) {
    if let Err(erro) = fs::remove_dir_all(pasta)
        && erro.kind() != io::ErrorKind::NotFound
    {
        eprintln!("vitral: não apagou {}: {erro}", pasta.display());
    }
}

/// A faxina de quando o Vitral abre: em `raiz` (a pasta `imagens/` dos dados), saem as pastas de
/// chats que não estão em `vivos` e, se o que sobrou passa de `teto`, as imagens mais antigas de
/// qualquer chat, até caber.
pub fn varrer(raiz: &Path, vivos: &HashSet<String>, teto: u64) {
    let mut todas = Vec::new();
    for entrada in fs::read_dir(raiz).into_iter().flatten().flatten() {
        let pasta = entrada.path();
        if !pasta.is_dir() {
            continue;
        }
        if vivos.contains(entrada.file_name().to_string_lossy().as_ref()) {
            todas.extend(imagens_de(&pasta));
        } else {
            apagar_do_chat(&pasta);
        }
    }
    todas.sort_by(|a, b| a.2.cmp(&b.2).then_with(|| a.0.cmp(&b.0)));
    tirar_ate_caber(todas, teto, &[]);
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::{dados::Dados, modelo::novo_id};
    use serde_json::json;
    use std::time::{Duration, SystemTime};

    fn raiz_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-imagens-{nome}-{}", novo_id()));
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    /// Um PNG de mentira: a assinatura, o IHDR com as medidas e `recheio` bytes depois.
    fn png(largura: u32, altura: u32, recheio: usize) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
        bytes.extend([0, 0, 0, 13]);
        bytes.extend(b"IHDR");
        bytes.extend(largura.to_be_bytes());
        bytes.extend(altura.to_be_bytes());
        bytes.extend((0..recheio).map(|i| (i * 31 % 251) as u8));
        bytes
    }

    /// O base64 de referência dos testes (o do `claude.rs` é privado de lá).
    fn em_base64(bytes: &[u8]) -> String {
        const ALFABETO: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut saida = String::new();
        for bloco in bytes.chunks(3) {
            let b = [bloco[0], *bloco.get(1).unwrap_or(&0), *bloco.get(2).unwrap_or(&0)];
            let n = (b[0] as u32) << 16 | (b[1] as u32) << 8 | b[2] as u32;
            for i in 0..4 {
                saida.push(if i <= bloco.len() { ALFABETO[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
            }
        }
        saida
    }

    fn arquivos_de(pasta: &Path) -> Vec<PathBuf> {
        let mut lista: Vec<PathBuf> = fs::read_dir(pasta).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
        lista.sort();
        lista
    }

    fn envelhecer(arquivo: &Path, segundos: u64) {
        let quando = SystemTime::now() - Duration::from_secs(segundos);
        fs::File::options().write(true).open(arquivo).unwrap().set_modified(quando).unwrap();
    }

    #[test]
    fn le_o_bloco_do_claude_e_o_do_mcp() {
        let imagem = png(1280, 800, 500);
        let do_claude = json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": em_base64(&imagem)}});
        let lida = do_bloco(&do_claude).expect("é imagem").expect("dá para guardar");
        assert_eq!((lida.extensao, &lida.bytes), ("png", &imagem));

        // O MCP (e o Codex, que repassa o que o servidor MCP devolveu): `data` e `mimeType`.
        let do_mcp = json!({"type": "image", "data": em_base64(&imagem), "mimeType": "image/png"});
        let lida = do_bloco(&do_mcp).expect("é imagem").expect("dá para guardar");
        assert_eq!((lida.extensao, &lida.bytes), ("png", &imagem));

        // Texto não é imagem.
        assert!(do_bloco(&json!({"type": "text", "text": "oi"})).is_none());
    }

    #[test]
    fn o_base64_pode_vir_numa_url_data_e_com_quebras_de_linha() {
        let imagem = png(40, 20, 100);
        let puro = em_base64(&imagem);
        let lida = de_base64("", &format!("data:image/png;base64,{puro}")).unwrap();
        assert_eq!((lida.extensao, &lida.bytes), ("png", &imagem));
        let (a, b) = puro.split_at(40);
        let lida = de_base64("image/png", &format!("{a}\n{b}\n")).unwrap();
        assert_eq!(lida.bytes, imagem);
    }

    /// O formato vale pelo conteúdo: um JPEG declarado como PNG é gravado como .jpg, para a
    /// miniatura e o visor o lerem certo.
    #[test]
    fn a_extensao_segue_o_conteudo_e_nao_o_tipo_declarado() {
        let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0, 16, b'J', b'F', b'I', b'F', 0, 1, 2, 3];
        assert_eq!(de_base64("image/png", &em_base64(&jpeg)).unwrap().extensao, "jpg");
        let gif = b"GIF89a\x18\x00\x24\x00resto";
        assert_eq!(de_base64("image/gif", &em_base64(gif)).unwrap().extensao, "gif");
        let webp = b"RIFF\0\0\0\0WEBPVP8X0123456789";
        assert_eq!(de_base64("", &em_base64(webp)).unwrap().extensao, "webp");
    }

    /// O que a conversa não sabe mostrar fica como antes: a marca do tipo no texto do resultado.
    #[test]
    fn o_que_nao_e_imagem_conhecida_vira_a_marca_no_texto() {
        assert_eq!(de_base64("image/svg+xml", &em_base64(b"<svg></svg>")).unwrap_err(), "(image/svg+xml)");
        // Base64 quebrado e bloco sem dados (a imagem por URL, que não vem no resultado).
        assert_eq!(de_base64("image/png", "isto não é base64!").unwrap_err(), "(image/png)");
        let por_url = json!({"type": "image", "source": {"type": "url", "url": "https://exemplo.com/a.png"}});
        assert_eq!(do_bloco(&por_url).unwrap().unwrap_err(), "(imagem)");
    }

    #[test]
    fn imagem_acima_do_teto_nao_e_guardada_e_o_texto_diz() {
        let grande = png(4000, 4000, TETO_POR_IMAGEM + 1);
        let erro = de_base64("image/png", &em_base64(&grande)).unwrap_err();
        assert!(erro.contains("image/png") && erro.contains("10 MB") && erro.contains("não foi guardada"), "{erro}");
        // No teto ainda entra.
        let no_teto = png(4000, 4000, TETO_POR_IMAGEM - 24);
        assert_eq!(no_teto.len(), TETO_POR_IMAGEM);
        assert_eq!(de_base64("image/png", &em_base64(&no_teto)).unwrap().bytes.len(), TETO_POR_IMAGEM);
    }

    #[test]
    fn o_resultado_se_separa_em_texto_e_imagens() {
        let (um, dois) = (png(10, 10, 50), png(20, 20, 60));
        let bloco = |bytes: &[u8]| json!({"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": em_base64(bytes)}});
        let conteudo = json!([
            {"type": "text", "text": "Print tirado."},
            bloco(&um),
            {"type": "image", "source": {"type": "base64", "media_type": "image/bmp", "data": em_base64(b"BM123456")}},
            bloco(&dois),
            {"type": "text", "text": "Fim."},
        ]);
        let (texto, imagens) = partes_do_resultado(&conteudo);
        assert_eq!(texto, "Print tirado.\n(image/bmp)\nFim.");
        assert_eq!(imagens.iter().map(|i| &i.bytes).collect::<Vec<_>>(), vec![&um, &dois]);

        // Só a imagem (o Read de um .png): o texto fica vazio, sem a marca de antes.
        let (texto, imagens) = partes_do_resultado(&json!([bloco(&um)]));
        assert_eq!((texto.as_str(), imagens.len()), ("", 1));
        // Os resultados de sempre continuam iguais.
        assert_eq!(partes_do_resultado(&json!("saída do comando")).0, "saída do comando");
        assert_eq!(partes_do_resultado(&Value::Null).0, "");
        assert_eq!(partes_do_resultado(&json!([{"type": "text", "text": "a"}, {"type": "text", "text": "b"}])).0, "a\nb");
    }

    #[test]
    fn grava_na_pasta_do_chat_uma_vez_so() {
        let raiz = raiz_de_teste("gravar");
        let dados = Dados::em(&raiz);
        let pasta = dados.pasta_imagens("chat-1");
        assert_eq!(pasta, dados.pasta.join("imagens").join("chat-1"));
        let imagem = ImagemRecebida { extensao: "png", bytes: png(1280, 800, 2000) };

        let caminho = guardar(&pasta, &imagem).unwrap();
        assert_eq!(caminho.parent(), Some(pasta.as_path()));
        assert_eq!(caminho.extension().and_then(|e| e.to_str()), Some("png"));
        assert_eq!(fs::read(&caminho).unwrap(), imagem.bytes);

        // A mesma imagem de novo: o mesmo arquivo, sem outro ao lado.
        assert_eq!(guardar(&pasta, &imagem).unwrap(), caminho);
        assert_eq!(arquivos_de(&pasta), vec![caminho.clone()]);

        // Outra imagem, outro arquivo; e o outro chat tem a pasta dele.
        let outra = ImagemRecebida { extensao: "jpg", bytes: vec![0xFF, 0xD8, 0xFF, 1, 2, 3] };
        let segundo = guardar(&pasta, &outra).unwrap();
        assert_ne!(segundo, caminho);
        assert_eq!(arquivos_de(&pasta).len(), 2);
        let do_outro_chat = guardar(&dados.pasta_imagens("chat-2"), &imagem).unwrap();
        assert!(do_outro_chat.starts_with(dados.pasta_imagens("chat-2")));
        let _ = fs::remove_dir_all(&raiz);
    }

    /// Um arquivo que ficou pela metade (queda no meio da gravação) é regravado inteiro.
    #[test]
    fn regrava_o_arquivo_que_ficou_pela_metade() {
        let raiz = raiz_de_teste("metade");
        let imagem = ImagemRecebida { extensao: "png", bytes: png(100, 100, 900) };
        let caminho = guardar(&raiz, &imagem).unwrap();
        fs::write(&caminho, &imagem.bytes[..10]).unwrap();
        assert_eq!(guardar(&raiz, &imagem).unwrap(), caminho);
        assert_eq!(fs::read(&caminho).unwrap(), imagem.bytes);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[cfg(unix)]
    #[test]
    fn a_pasta_e_as_imagens_sao_so_do_dono() {
        use std::os::unix::fs::PermissionsExt as _;
        let raiz = raiz_de_teste("dono");
        let pasta = raiz.join("imagens").join("chat");
        let caminho = guardar(&pasta, &ImagemRecebida { extensao: "png", bytes: png(1, 1, 10) }).unwrap();
        assert_eq!(fs::metadata(&pasta).unwrap().permissions().mode() & 0o777, 0o700);
        assert_eq!(fs::metadata(&caminho).unwrap().permissions().mode() & 0o777, 0o600);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn o_teto_do_chat_tira_as_mais_antigas() {
        let raiz = raiz_de_teste("teto");
        let imagem = |n: u32| ImagemRecebida { extensao: "png", bytes: png(n, n, 1000) };
        let tamanho = imagem(1).bytes.len() as u64;
        let caminhos: Vec<PathBuf> = (1..=4).map(|n| guardar(&raiz, &imagem(n)).unwrap()).collect();
        // A 1 é a mais antiga, a 4 a mais nova.
        for (i, caminho) in caminhos.iter().enumerate() {
            envelhecer(caminho, 400 - 100 * i as u64);
        }
        // Cabem duas e meia: saem as duas mais antigas.
        let sairam = podar(&raiz, tamanho * 2 + tamanho / 2, &[]);
        assert_eq!(sairam, vec![caminhos[0].clone(), caminhos[1].clone()]);
        assert_eq!(arquivos_de(&raiz).len(), 2);
        assert!(caminhos[2].is_file() && caminhos[3].is_file());

        // A que acabou de chegar fica, mesmo sendo a mais antiga no relógio e passando do teto.
        envelhecer(&caminhos[3], 900);
        let sairam = podar(&raiz, tamanho / 2, std::slice::from_ref(&caminhos[3]));
        assert_eq!(sairam, vec![caminhos[2].clone()]);
        assert!(caminhos[3].is_file());

        // Dentro do teto, nada sai.
        assert!(podar(&raiz, tamanho * 10, &[]).is_empty());
        let _ = fs::remove_dir_all(&raiz);
    }

    /// A imagem que chega de novo passa a ser a mais nova: a poda não a leva antes das outras.
    #[test]
    fn a_imagem_recebida_de_novo_renova_a_data() {
        let raiz = raiz_de_teste("renova");
        let imagem = |n: u32| ImagemRecebida { extensao: "png", bytes: png(n, n, 1000) };
        let (um, dois) = (guardar(&raiz, &imagem(1)).unwrap(), guardar(&raiz, &imagem(2)).unwrap());
        envelhecer(&um, 500);
        envelhecer(&dois, 100);
        assert_eq!(guardar(&raiz, &imagem(1)).unwrap(), um);
        let tamanho = imagem(1).bytes.len() as u64;
        assert_eq!(podar(&raiz, tamanho, &[]), vec![dois]);
        assert!(um.is_file());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn guardar_todas_devolve_os_caminhos_e_mantem_o_teto_do_chat() {
        let raiz = raiz_de_teste("todas");
        let imagens = vec![
            ImagemRecebida { extensao: "png", bytes: png(1, 1, 300) },
            ImagemRecebida { extensao: "png", bytes: png(2, 2, 300) },
        ];
        let mut conteudo = String::from("Print tirado.");
        let caminhos = guardar_todas(&raiz, &imagens, &mut conteudo);
        assert_eq!(caminhos.len(), 2);
        assert_eq!(fs::read(&caminhos[0]).unwrap(), imagens[0].bytes);
        assert_eq!(fs::read(&caminhos[1]).unwrap(), imagens[1].bytes);
        assert_eq!(conteudo, "Print tirado.");

        // Sem onde gravar (a pasta é um arquivo): o resultado diz, e nenhum caminho volta.
        let arquivo = raiz.join("nao-e-pasta");
        fs::write(&arquivo, b"x").unwrap();
        let mut conteudo = String::new();
        assert!(guardar_todas(&arquivo, &imagens[..1], &mut conteudo).is_empty());
        assert!(conteudo.starts_with("(imagem png: não deu para guardar"), "{conteudo}");
        let _ = fs::remove_dir_all(&raiz);
    }

    fn chamada(id: &str, resultado: Option<&str>) -> Item {
        Item::Ferramenta {
            id: id.into(),
            nome: "mcp__chrome-devtools__take_screenshot".into(),
            resumo: "mcp__chrome-devtools__take_screenshot".into(),
            resultado: resultado.map(str::to_string),
            erro: false,
            em: 0,
            detalhe: None,
            arquivo: None,
            sub: None,
            mudanca: None,
            imagens: Vec::new(),
        }
    }

    fn imagens_de(item: &Item) -> &[PathBuf] {
        match item {
            Item::Ferramenta { imagens, .. } => imagens,
            _ => &[],
        }
    }

    #[test]
    fn a_chamada_guarda_os_caminhos_das_imagens_do_resultado() {
        let raiz = raiz_de_teste("chamada");
        let pasta = raiz.join("imagens").join("chat");
        let imagens = vec![
            ImagemRecebida { extensao: "png", bytes: png(1280, 800, 3000) },
            ImagemRecebida { extensao: "png", bytes: png(640, 400, 3000) },
        ];
        let mut itens = vec![chamada("t1", Some("ok")), chamada("t2", Some("Print tirado."))];
        assert!(guardar_na_chamada(&mut itens, "t2", &pasta, &imagens));
        let guardadas = imagens_de(&itens[1]).to_vec();
        assert_eq!(guardadas.len(), 2);
        assert!(guardadas.iter().all(|c| c.starts_with(&pasta)));
        assert_eq!(fs::read(&guardadas[0]).unwrap(), imagens[0].bytes);
        assert_eq!(fs::read(&guardadas[1]).unwrap(), imagens[1].bytes);
        assert!(imagens_de(&itens[0]).is_empty());

        // O que vai para o disco com a conversa são os caminhos: nem um pedaço do base64.
        let gravado = serde_json::to_string(&itens).unwrap();
        assert!(gravado.contains(&guardadas[0].display().to_string()));
        assert!(!gravado.contains(&em_base64(&imagens[0].bytes)[..80]));
        assert!(gravado.len() < 2000, "{} bytes", gravado.len());
        // E a conversa relida tem as mesmas imagens.
        let relidos: Vec<Item> = serde_json::from_str(&gravado).unwrap();
        assert_eq!(imagens_de(&relidos[1]), guardadas.as_slice());

        // A chamada que não está na conversa (bastidor): nada é gravado.
        let antes = arquivos_de(&pasta).len();
        let nova = [ImagemRecebida { extensao: "png", bytes: png(5, 5, 50) }];
        assert!(!guardar_na_chamada(&mut itens, "t9", &pasta, &nova));
        assert_eq!(arquivos_de(&pasta).len(), antes);
        let _ = fs::remove_dir_all(&raiz);
    }

    /// A conversa de antes desta versão (sem o campo) abre igual, e a chamada sem imagem não
    /// ganha o campo no arquivo.
    #[test]
    fn a_chamada_sem_imagem_fica_como_antes_no_arquivo() {
        let antiga = r#"[{"Ferramenta":{"id":"t1","nome":"Read","resumo":"Read · a.png","resultado":"(image/png)","erro":false}}]"#;
        let itens: Vec<Item> = serde_json::from_str(antiga).unwrap();
        assert!(matches!(&itens[0], Item::Ferramenta { imagens, .. } if imagens.is_empty()));
        assert!(!serde_json::to_string(&itens).unwrap().contains("imagens"));
    }

    #[test]
    fn apagar_a_conversa_apaga_as_imagens_dela() {
        let raiz = raiz_de_teste("apagar");
        let dados = Dados::em(&raiz);
        let imagem = ImagemRecebida { extensao: "png", bytes: png(3, 3, 100) };
        let da_c1 = guardar(&dados.pasta_imagens("c1"), &imagem).unwrap();
        let da_c2 = guardar(&dados.pasta_imagens("c2"), &imagem).unwrap();
        dados.apagar_chat("c1");
        dados.apagar_chat("nunca-teve-imagem");
        dados.esperar_gravacoes();
        assert!(!da_c1.exists() && !dados.pasta_imagens("c1").exists());
        assert!(da_c2.is_file());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn a_faxina_tira_as_pastas_de_chats_que_sairam() {
        let raiz = raiz_de_teste("faxina");
        let dados = Dados::em(&raiz);
        let imagem = ImagemRecebida { extensao: "png", bytes: png(3, 3, 100) };
        let vivo = guardar(&dados.pasta_imagens("vivo"), &imagem).unwrap();
        let orfa = guardar(&dados.pasta_imagens("saiu"), &imagem).unwrap();
        let vivos: HashSet<String> = ["vivo".to_string()].into();
        varrer(&dados.pasta_imagens(""), &vivos, TETO_TOTAL);
        assert!(vivo.is_file());
        assert!(!orfa.exists() && !dados.pasta_imagens("saiu").exists());
        // Sem a pasta de imagens (ninguém recebeu nenhuma), não faz nada nem a cria.
        let vazio = raiz.join("outro");
        varrer(&vazio, &vivos, TETO_TOTAL);
        assert!(!vazio.exists());
        let _ = fs::remove_dir_all(&raiz);
    }

    /// Quem a faxina poupa além dos chats do estado: os que ainda têm a conversa gravada.
    #[test]
    fn os_chats_com_conversa_gravada_saem_dos_arquivos() {
        let raiz = raiz_de_teste("gravados");
        let dados = Dados::em(&raiz);
        assert!(dados.chats_gravados().is_empty());
        fs::create_dir_all(dados.pasta.join("chats")).unwrap();
        for nome in ["c1.json", "c2.json", "c2.ilegivel-1.json", "solto.txt"] {
            fs::write(dados.pasta.join("chats").join(nome), b"[]").unwrap();
        }
        let mut gravados = dados.chats_gravados();
        gravados.sort();
        assert_eq!(gravados, ["c1", "c2", "c2.ilegivel-1"]);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn a_faxina_mantem_o_teto_de_todas_as_conversas() {
        let raiz = raiz_de_teste("total");
        let imagem = |n: u32| ImagemRecebida { extensao: "png", bytes: png(n, n, 1000) };
        let tamanho = imagem(1).bytes.len() as u64;
        let a1 = guardar(&raiz.join("a"), &imagem(1)).unwrap();
        let b1 = guardar(&raiz.join("b"), &imagem(2)).unwrap();
        let a2 = guardar(&raiz.join("a"), &imagem(3)).unwrap();
        envelhecer(&a1, 300);
        envelhecer(&b1, 200);
        envelhecer(&a2, 100);
        let vivos: HashSet<String> = ["a".to_string(), "b".to_string()].into();
        // Cabem duas: sai a mais antiga de todas, seja de que chat for.
        varrer(&raiz, &vivos, tamanho * 2);
        assert!(!a1.exists() && b1.is_file() && a2.is_file());
        let _ = fs::remove_dir_all(&raiz);
    }
}
