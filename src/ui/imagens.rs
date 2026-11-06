//! As imagens do chat: o que cada arquivo anexado tem (se ainda está no disco, o tamanho e as
//! medidas, lidas do cabeçalho sem decodificar a imagem), as contas da miniatura que a bolha e a
//! caixa desenham, e os caminhos de imagem que o markdown de uma resposta aponta.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use gpui_kit::SharedString;

/// Altura da miniatura de uma imagem na bolha. O quadrado de 56 cortado (`ObjectFit::Cover`) não
/// deixava ler nada de uma captura de tela; com a proporção certa, 120 de altura já mostra do que
/// a imagem trata, e ainda cabem duas lado a lado no painel estreito.
pub const ALTURA_MINIATURA: f32 = 120.;
/// Até onde a miniatura cresce para o lado: um panorama não toma o balão inteiro.
pub const LARGURA_MAXIMA_MINIATURA: f32 = 260.;
/// Altura da miniatura dentro do chip da caixa de mensagem (o chip tem 28 de alto).
pub const ALTURA_CHIP: f32 = 20.;
/// Largura máxima da miniatura do chip: ela fica no lugar do ícone, antes do nome.
pub const LARGURA_MAXIMA_CHIP: f32 = 44.;
/// Lado mínimo da caixa da miniatura. Uma imagem de 24 px fica centrada num quadrado em vez de
/// virar um risco entre as vizinhas, e o alvo do clique continua clicável.
pub const LADO_MINIMO_MINIATURA: f32 = 56.;

/// O que a bolha e a caixa precisam saber de um anexo, lido do disco uma vez por arquivo.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Anexo {
    /// Falso quando o arquivo saiu do disco depois de a mensagem ter sido mandada.
    pub existe: bool,
    /// "5 KB", "1,4 MB"; `None` quando o arquivo não está mais lá.
    pub tamanho: Option<SharedString>,
    /// Largura e altura em pixels; `None` no que não é imagem (ou num formato que este leitor não
    /// conhece).
    pub medidas: Option<(u32, u32)>,
}

impl Anexo {
    /// "1200 × 700 · 5 KB", para a dica e para o visor; vazio quando nada se sabe.
    pub fn detalhe(&self) -> String {
        let mut partes = Vec::new();
        if let Some((largura, altura)) = self.medidas {
            partes.push(format!("{largura} × {altura}"));
        }
        if let Some(tamanho) = &self.tamanho {
            partes.push(tamanho.to_string());
        }
        partes.join(" · ")
    }
}

thread_local! {
    /// O que já foi lido de cada anexo: o disco é lido uma vez por arquivo, não a cada quadro.
    static LIDOS: RefCell<HashMap<PathBuf, Anexo>> = RefCell::new(HashMap::new());
}

/// Quanto do começo do arquivo basta para achar as medidas. O PNG, o GIF e o WebP dizem tudo nos
/// primeiros 30 bytes; o JPEG pode levar alguns blocos de metadado (EXIF, miniatura) até o SOF.
const CABECALHO: usize = 128 * 1024;

/// O anexo visto do disco, com o resultado guardado: a bolha pergunta a cada quadro.
pub fn anexo(caminho: &Path) -> Anexo {
    LIDOS.with_borrow_mut(|lidos| lidos.entry(caminho.to_path_buf()).or_insert_with(|| ler(caminho)).clone())
}

fn ler(caminho: &Path) -> Anexo {
    let Ok(dados) = std::fs::metadata(caminho) else {
        return Anexo::default();
    };
    let medidas = crate::claude::tipo_imagem(caminho).and_then(|_| medidas_de(&comeco(caminho)));
    Anexo { existe: true, tamanho: Some(tamanho_curto(dados.len()).into()), medidas }
}

/// Os primeiros `CABECALHO` bytes do arquivo (ou ele inteiro, se for menor).
fn comeco(caminho: &Path) -> Vec<u8> {
    use std::io::Read as _;
    let Ok(arquivo) = std::fs::File::open(caminho) else {
        return Vec::new();
    };
    let mut bytes = Vec::new();
    let _ = arquivo.take(CABECALHO as u64).read_to_end(&mut bytes);
    bytes
}

/// "2 KB", "1,4 MB".
pub fn tamanho_curto(bytes: u64) -> String {
    match bytes {
        0..1024 => format!("{bytes} B"),
        1024..1_048_576 => format!("{} KB", (bytes as f64 / 1024.).round() as u64),
        _ => format!("{:.1} MB", bytes as f64 / 1_048_576.).replace('.', ","),
    }
}

/// Largura e altura pelo cabeçalho do arquivo, sem decodificar a imagem: PNG, JPEG, GIF e WebP,
/// os quatro formatos que o anexo aceita (`claude::tipo_imagem`). `None` quando o começo não bate
/// com nenhum deles — a miniatura então volta ao quadrado de antes.
pub fn medidas_de(bytes: &[u8]) -> Option<(u32, u32)> {
    let be32 = |i: usize| Some(u32::from_be_bytes(bytes.get(i..i + 4)?.try_into().ok()?));
    let le16 = |i: usize| Some(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?) as u32);
    // PNG: o IHDR é sempre o primeiro bloco (8 da assinatura + 4 do tamanho + 4 do nome).
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return Some((be32(16)?, be32(20)?));
    }
    // GIF: a tela lógica vem logo depois da versão, em little-endian.
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return Some((le16(6)?, le16(8)?));
    }
    if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP".as_slice()) {
        return medidas_webp(bytes);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return medidas_jpeg(bytes);
    }
    None
}

/// WebP tem três formas: a estendida (VP8X, com a tela em 24 bits), a com perdas (VP8, 14 bits
/// depois do código de início do quadro-chave) e a sem perdas (VP8L, 14 bits empacotados).
fn medidas_webp(bytes: &[u8]) -> Option<(u32, u32)> {
    let le24 = |i: usize| -> Option<u32> {
        let b = bytes.get(i..i + 3)?;
        Some(u32::from(b[0]) | u32::from(b[1]) << 8 | u32::from(b[2]) << 16)
    };
    match bytes.get(12..16)? {
        b"VP8X" => Some((le24(24)? + 1, le24(27)? + 1)),
        b"VP8 " => {
            // Quadro-chave: 3 bytes de marca, o código 9D 01 2A e então as medidas, 14 bits cada.
            (bytes.get(23..26)? == [0x9D, 0x01, 0x2A]).then_some(())?;
            let le16 = |i: usize| Some(u16::from_le_bytes(bytes.get(i..i + 2)?.try_into().ok()?) as u32);
            Some((le16(26)? & 0x3FFF, le16(28)? & 0x3FFF))
        }
        b"VP8L" => {
            (*bytes.get(20)? == 0x2F).then_some(())?;
            let n = u32::from_le_bytes(bytes.get(21..25)?.try_into().ok()?);
            Some(((n & 0x3FFF) + 1, (n >> 14 & 0x3FFF) + 1))
        }
        _ => None,
    }
}

/// JPEG: anda de marcador em marcador até um SOF (o bloco que diz o tamanho do quadro), pulando
/// os blocos de metadado pelo comprimento que cada um declara.
fn medidas_jpeg(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            // Fora de sincronia (arquivo truncado ou estranho): não dá para confiar no resto.
            return None;
        }
        let marcador = bytes[i + 1];
        // Byte de enchimento entre marcadores.
        if marcador == 0xFF {
            i += 1;
            continue;
        }
        // Marcadores sem corpo: início e fim da imagem, reinício e TEM.
        if matches!(marcador, 0xD8 | 0xD9 | 0x01 | 0xD0..=0xD7) {
            i += 2;
            continue;
        }
        let comprimento = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        // SOF0 a SOF15, menos o DHT (C4), o JPG (C8) e o DAC (CC), que caem na mesma faixa.
        if (0xC0..=0xCF).contains(&marcador) && !matches!(marcador, 0xC4 | 0xC8 | 0xCC) {
            let altura = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let largura = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((largura, altura));
        }
        if comprimento < 2 {
            return None;
        }
        i += 2 + comprimento;
    }
    None
}

/// A imagem dentro da miniatura: entra na proporção dela, sem nunca esticar, até `altura` de alto
/// e `largura_max` de largo. Quem desenha põe isso numa caixa de pelo menos
/// `LADO_MINIMO_MINIATURA`, com a imagem centrada.
pub fn medida_da_miniatura(medidas: Option<(u32, u32)>, altura: f32, largura_max: f32) -> (f32, f32) {
    let Some((largura, alta)) = medidas.filter(|(l, a)| *l > 0 && *a > 0) else {
        return (LADO_MINIMO_MINIATURA, LADO_MINIMO_MINIATURA.min(altura));
    };
    let (largura, alta) = (largura as f32, alta as f32);
    // Nunca aumentar: uma imagem de 24 px continua com 24, nítida.
    let escala = (altura / alta).min(largura_max / largura).min(1.);
    ((largura * escala).round().max(1.), (alta * escala).round().max(1.))
}

/// O nome do arquivo, ou o caminho inteiro quando ele não tem nome (raiz, "..").
pub fn nome_do_arquivo(caminho: &Path) -> String {
    caminho
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| caminho.display().to_string())
}

/// Os caminhos de imagem que um texto aponta em `![alt](caminho)` e que estão no disco, na ordem
/// e sem repetir. É por eles que a resposta do assistente ganha miniatura: o kit desenha a imagem
/// do markdown na altura de uma linha de texto, tamanho de ícone, e um print não se vê ali.
/// Caminho citado solto ou entre crases fica de fora de propósito: nem toda menção a um .png é um
/// convite para abrir a imagem.
pub fn imagens_do_texto(texto: &str, base: Option<&Path>) -> Vec<PathBuf> {
    let mut achados: Vec<PathBuf> = Vec::new();
    let mut resto = texto;
    while let Some(abre) = resto.find("![") {
        resto = &resto[abre + 2..];
        let Some(meio) = resto.find("](") else { break };
        let depois = &resto[meio + 2..];
        let Some(fim) = depois.find(')') else { break };
        // A URL vai até o primeiro espaço (o que vem depois é o título) e pode vir entre <>.
        let alvo = depois[..fim].trim();
        let alvo = alvo.split_whitespace().next().unwrap_or(alvo).trim_matches(['<', '>']);
        if let Some(caminho) = caminho_de_imagem(alvo, base)
            && !achados.contains(&caminho)
        {
            achados.push(caminho);
        }
        resto = &depois[fim + 1..];
    }
    achados
}

/// O caminho do disco que uma URL do markdown aponta, quando ela é uma imagem que está lá.
fn caminho_de_imagem(uri: &str, base: Option<&Path>) -> Option<PathBuf> {
    let texto = uri.split(['#', '?']).next().unwrap_or(uri);
    if texto.is_empty() || texto.contains("://") && !texto.starts_with("file://") {
        return None;
    }
    let caminho = if let Some(resto) = texto.strip_prefix("file://") {
        PathBuf::from(sem_porcentagem(resto))
    } else if let Some(resto) = texto.strip_prefix("~/") {
        dirs::home_dir()?.join(sem_porcentagem(resto))
    } else {
        let bruto = PathBuf::from(sem_porcentagem(texto));
        if bruto.is_absolute() { bruto } else { base?.join(bruto) }
    };
    crate::claude::tipo_imagem(&caminho)?;
    caminho.is_file().then_some(caminho)
}

/// `%20` e companhia de volta ao caractere: o markdown escapa o espaço de um caminho.
fn sem_porcentagem(texto: &str) -> String {
    let mut bytes = Vec::with_capacity(texto.len());
    let mut resto = texto.as_bytes();
    while let Some((primeiro, depois)) = resto.split_first() {
        let par = (*primeiro == b'%').then(|| depois.get(..2)).flatten();
        match par.and_then(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok()) {
            Some(byte) => {
                bytes.push(byte);
                resto = &depois[2..];
            }
            None => {
                bytes.push(*primeiro);
                resto = depois;
            }
        }
    }
    String::from_utf8_lossy(&bytes).into_owned()
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn tamanho_em_bytes_kb_e_mb() {
        assert_eq!(tamanho_curto(512), "512 B");
        assert_eq!(tamanho_curto(2048), "2 KB");
        assert_eq!(tamanho_curto(1_468_006), "1,4 MB");
    }

    #[test]
    fn mede_png_gif_e_webp_pelo_cabecalho() {
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend(1200u32.to_be_bytes());
        png.extend(700u32.to_be_bytes());
        assert_eq!(medidas_de(&png), Some((1200, 700)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend(24u16.to_le_bytes());
        gif.extend(36u16.to_le_bytes());
        assert_eq!(medidas_de(&gif), Some((24, 36)));

        // VP8X: a tela vem em 24 bits, menos um.
        let mut webp = b"RIFF\0\0\0\0WEBPVP8X".to_vec();
        webp.extend([0; 8]);
        webp.extend(&799u32.to_le_bytes()[..3]);
        webp.extend(&599u32.to_le_bytes()[..3]);
        assert_eq!(medidas_de(&webp), Some((800, 600)));
    }

    #[test]
    fn mede_jpeg_pulando_os_metadados() {
        let mut jpeg = vec![0xFF, 0xD8];
        // APP0 de 16 bytes, que precisa ser pulado inteiro.
        jpeg.extend([0xFF, 0xE0, 0x00, 0x10]);
        jpeg.extend([0u8; 14]);
        // SOF0: comprimento, precisão, altura, largura.
        jpeg.extend([0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend(600u16.to_be_bytes());
        jpeg.extend(900u16.to_be_bytes());
        jpeg.extend([0u8; 8]);
        assert_eq!(medidas_de(&jpeg), Some((900, 600)));
        // Um DHT na faixa dos SOF não pode ser confundido com um.
        assert_eq!(medidas_de(&[0xFF, 0xD8, 0xFF, 0xC4, 0x00, 0x04, 0, 0, 0, 0, 0]), None);
        assert_eq!(medidas_de(b"nada disso"), None);
    }

    #[test]
    fn miniatura_cabe_na_caixa_sem_esticar() {
        // Captura larga: a altura manda, a largura sai da proporção.
        assert_eq!(medida_da_miniatura(Some((1200, 700)), 120., 260.), (206., 120.));
        // Panorama: a largura máxima manda.
        assert_eq!(medida_da_miniatura(Some((3000, 400)), 120., 260.), (260., 35.));
        // Miúda: fica no tamanho dela.
        assert_eq!(medida_da_miniatura(Some((24, 24)), 120., 260.), (24., 24.));
        // Sem medidas, o quadrado de antes.
        assert_eq!(medida_da_miniatura(None, 120., 260.), (56., 56.));
    }

    #[test]
    fn caminho_do_markdown_so_com_imagem_que_existe() {
        let pasta = std::env::temp_dir().join(format!("vitral-img-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(pasta.join("prints")).unwrap();
        let imagem = pasta.join("prints").join("um dois.png");
        std::fs::write(&imagem, b"\x89PNG\r\n\x1a\n").unwrap();
        std::fs::write(pasta.join("segredo.txt"), b"nada").unwrap();

        let absoluto = imagem.display().to_string();
        assert_eq!(caminho_de_imagem(&absoluto, None), Some(imagem.clone()));
        assert_eq!(caminho_de_imagem(&format!("file://{absoluto}"), None), Some(imagem.clone()));
        assert_eq!(caminho_de_imagem("prints/um%20dois.png", Some(&pasta)), Some(imagem.clone()));
        // Relativo sem pasta do chat, arquivo que não existe, que não é imagem e URL remota.
        assert_eq!(caminho_de_imagem("prints/um dois.png", None), None);
        assert_eq!(caminho_de_imagem("prints/outra.png", Some(&pasta)), None);
        assert_eq!(caminho_de_imagem("segredo.txt", Some(&pasta)), None);
        assert_eq!(caminho_de_imagem("https://exemplo.com/a.png", Some(&pasta)), None);
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn acha_as_imagens_do_markdown_da_resposta() {
        let pasta = std::env::temp_dir().join(format!("vitral-img-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        for nome in ["um.png", "dois.png"] {
            std::fs::write(pasta.join(nome), b"\x89PNG\r\n\x1a\n").unwrap();
        }
        let texto = "Pronto: ![print](um.png \"o print\") e de novo ![](um.png).\n\
                     Outro ![dois](<dois.png>), um que não existe ![](tres.png) e `quatro.png` solto.";
        assert_eq!(
            imagens_do_texto(texto, Some(&pasta)),
            vec![pasta.join("um.png"), pasta.join("dois.png")]
        );
        assert!(imagens_do_texto("sem imagem nenhuma", Some(&pasta)).is_empty());
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[test]
    fn anexo_que_sumiu_nao_tem_tamanho() {
        let pasta = std::env::temp_dir().join(format!("vitral-img-{}", crate::modelo::novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        let imagem = pasta.join("a.png");
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend([0, 0, 0, 13]);
        png.extend(b"IHDR");
        png.extend(40u32.to_be_bytes());
        png.extend(20u32.to_be_bytes());
        std::fs::write(&imagem, &png).unwrap();
        let lido = ler(&imagem);
        assert!(lido.existe);
        assert_eq!(lido.medidas, Some((40, 20)));
        assert_eq!(lido.detalhe(), "40 × 20 · 24 B");
        assert_eq!(ler(&pasta.join("nao-existe.png")), Anexo::default());
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
