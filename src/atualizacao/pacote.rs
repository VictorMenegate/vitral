//! O pacote de uma versão: abrir o `.tar.gz` do release com segurança e conferir o binário novo
//! antes de trocar de versão (`--versao --json`, com prazo).

use std::{
    fs,
    io::{self, Read as _, Write as _},
    path::{Component, Path},
    process::Stdio,
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, bail, ensure};
use flate2::read::GzDecoder;
use serde::Deserialize;

use crate::{dados::fsync_da_pasta, processos::novo_comando};

/// O que um pacote pode ter depois de aberto (o binário tem dezenas de MB; o resto, texto).
pub const LIMITE_EXTRAIDO: u64 = 512 * 1024 * 1024;
const LIMITE_ENTRADAS: usize = 256;
/// Prazo do `--versao --json` do binário novo.
pub const PRAZO_CONFERENCIA: Duration = Duration::from_secs(10);

/// Abre `pacote` (um `.tar.gz`) em `destino`, que precisa ser uma pasta nova e vazia. Só passam
/// arquivos e pastas comuns sob `prefixo/` (a pasta `vitral-<versão>-<plataforma>/` do release),
/// que perde o prefixo no caminho. Recusa caminho absoluto, `..`, links, dispositivos, entrada
/// repetida e soma acima de `limite`, e o pacote sem o `binario` (`vitral` no Linux, `vitral.exe`
/// no Windows); ele sai com 0755 e o resto com 0644, seja qual for a permissão no pacote. Cada
/// arquivo e cada pasta vão para o disco (`sync_all`) antes de voltar.
pub fn extrair(pacote: &Path, destino: &Path, prefixo: &str, binario: &str, limite: u64) -> Result<()> {
    let arquivo = fs::File::open(pacote).with_context(|| pacote.display().to_string())?;
    let mut tar = tar::Archive::new(GzDecoder::new(io::BufReader::new(arquivo)));
    let mut total = 0u64;
    let mut pastas = vec![destino.to_path_buf()];
    for (indice, entrada) in tar.entries().context("o pacote não se abre")?.enumerate() {
        ensure!(indice < LIMITE_ENTRADAS, "o pacote tem entradas demais");
        let mut entrada = entrada.context("o pacote está corrompido")?;
        let tipo = entrada.header().entry_type();
        if tipo.is_pax_global_extensions() {
            continue;
        }
        let caminho = entrada.path().context("caminho ilegível no pacote")?.into_owned();
        let relativo = dentro_do_prefixo(&caminho, prefixo)?;
        if tipo.is_dir() {
            if let Some(relativo) = relativo {
                let pasta = destino.join(relativo);
                fs::create_dir_all(&pasta)?;
                pastas.push(pasta);
            }
            continue;
        }
        ensure!(tipo.is_file(), "o pacote tem algo que não é arquivo nem pasta: {}", caminho.display());
        let Some(relativo) = relativo else {
            bail!("arquivo fora da pasta do pacote: {}", caminho.display());
        };
        let tamanho = entrada.size();
        total = total.saturating_add(tamanho);
        ensure!(total <= limite, "o pacote passa do limite de {} MB aberto", limite / (1024 * 1024));
        let alvo = destino.join(relativo);
        if let Some(pai) = alvo.parent()
            && pai != destino
        {
            fs::create_dir_all(pai)?;
            pastas.push(pai.to_path_buf());
        }
        let mut saida = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&alvo)
            .with_context(|| format!("entrada repetida ou inválida: {}", caminho.display()))?;
        // O cabeçalho pode mentir o tamanho: lê no máximo o que ele disse, mais um byte para pegar.
        let copiado = io::copy(&mut (&mut entrada).take(tamanho + 1), &mut saida)?;
        ensure!(copiado == tamanho, "{} veio com tamanho diferente do cabeçalho", caminho.display());
        let modo = if relativo == Path::new(binario) { 0o755 } else { 0o644 };
        if let Some(modo) = permissao(modo) {
            saida.set_permissions(modo)?;
        }
        saida.sync_all()?;
    }
    for pasta in pastas.iter().rev() {
        if let Some(modo) = permissao(0o755) {
            fs::set_permissions(pasta, modo)?;
        }
        fsync_da_pasta(pasta)?;
    }
    ensure!(
        fs::symlink_metadata(destino.join(binario)).is_ok_and(|m| m.is_file()),
        "o pacote não tem o {binario}"
    );
    Ok(())
}

/// O modo Unix como permissão; fora do Unix não há o que dar (nem o bit de execução).
#[cfg(unix)]
fn permissao(modo: u32) -> Option<fs::Permissions> {
    use std::os::unix::fs::PermissionsExt as _;
    Some(fs::Permissions::from_mode(modo))
}

#[cfg(not(unix))]
fn permissao(_modo: u32) -> Option<fs::Permissions> {
    None
}

/// O caminho sem o prefixo (None para a própria pasta do prefixo). Só componentes comuns.
fn dentro_do_prefixo<'a>(caminho: &'a Path, prefixo: &str) -> Result<Option<&'a Path>> {
    let mut partes = caminho.components();
    ensure!(
        partes.clone().all(|c| matches!(c, Component::Normal(_))),
        "caminho proibido no pacote: {}",
        caminho.display()
    );
    ensure!(
        partes.next() == Some(Component::Normal(prefixo.as_ref())),
        "fora da pasta {prefixo}/: {}",
        caminho.display()
    );
    let resto = partes.as_path();
    Ok((!resto.as_os_str().is_empty()).then_some(resto))
}

/// O que o binário novo respondeu ao `--versao --json`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Resposta {
    pub versao: String,
    pub formato_dados: u32,
}

/// Roda `binario --versao --json` sem tela e com prazo, e confere que é a `versao` esperada. Pega o
/// binário zerado por uma queda de energia, a biblioteca faltando e o pacote trocado.
pub fn conferir_binario(binario: &Path, versao: &str, prazo: Duration) -> Result<Resposta> {
    let mut filho = abrir_sem_tela(binario)?;
    let limite = Instant::now() + prazo;
    let estado = loop {
        if let Some(estado) = filho.try_wait()? {
            break estado;
        }
        if Instant::now() >= limite {
            let _ = filho.kill();
            let _ = filho.wait();
            bail!("não respondeu em {} s", prazo.as_secs());
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let mut saida = Vec::new();
    if let Some(stdout) = filho.stdout.take() {
        stdout.take(64 * 1024).read_to_end(&mut saida)?;
    }
    ensure!(estado.success(), "saiu com {estado}");
    let resposta: Resposta = serde_json::from_slice(&saida)
        .with_context(|| format!("respondeu outra coisa: {}", String::from_utf8_lossy(&saida).trim()))?;
    ensure!(resposta.versao == versao, "diz ser a {} em vez da {versao}", resposta.versao);
    Ok(resposta)
}

/// Um arquivo que acabou de ser escrito pode dar "Text file busy" por um instante, enquanto outra
/// thread abre um processo (o filho herda o arquivo até o exec): tenta de novo.
fn abrir_sem_tela(binario: &Path) -> Result<std::process::Child> {
    let mut tentativas = 0;
    loop {
        let resultado = novo_comando(binario)
            .args(["--versao", "--json"])
            .env_remove("DISPLAY")
            .env_remove("WAYLAND_DISPLAY")
            .env_remove("WAYLAND_SOCKET")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn();
        match resultado {
            Err(erro) if texto_ocupado(&erro) && tentativas < 20 => {
                tentativas += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            resultado => return resultado.with_context(|| format!("não abriu {}", binario.display())),
        }
    }
}

#[cfg(unix)]
fn texto_ocupado(erro: &io::Error) -> bool {
    erro.raw_os_error() == Some(libc::ETXTBSY)
}

/// No Windows, o antivírus lê o .exe recém-gravado e segura o arquivo por um instante
/// (ERROR_SHARING_VIOLATION).
#[cfg(not(unix))]
fn texto_ocupado(erro: &io::Error) -> bool {
    erro.raw_os_error() == Some(32)
}

/// Grava `bytes` em `caminho` sem nunca deixar o arquivo pela metade: temporário ao lado,
/// `sync_all`, rename e `sync_all` da pasta.
pub fn gravar_atomico(caminho: &Path, bytes: &[u8], modo: u32) -> io::Result<()> {
    let pasta = caminho.parent().ok_or_else(|| io::Error::other("caminho sem pasta"))?;
    fs::create_dir_all(pasta)?;
    let nome = caminho.file_name().unwrap_or_default().to_string_lossy();
    let temporario = pasta.join(format!(".{nome}.{}.tmp", std::process::id()));
    {
        let mut arquivo = fs::File::create(&temporario)?;
        arquivo.write_all(bytes)?;
        if let Some(modo) = permissao(modo) {
            arquivo.set_permissions(modo)?;
        }
        arquivo.sync_all()?;
    }
    if let Err(erro) = trocar_por_cima(&temporario, caminho) {
        let _ = fs::remove_file(&temporario);
        return Err(erro);
    }
    fsync_da_pasta(pasta)
}

/// `de` passa a ser `para` de uma vez (o velho ou o novo, nunca pela metade), por cima do que
/// houver. No Linux, o rename (o fsync da pasta fica com quem chama).
#[cfg(not(windows))]
pub fn trocar_por_cima(de: &Path, para: &Path) -> io::Result<()> {
    fs::rename(de, para)
}

/// No Windows, o `MoveFileExW` com `MOVEFILE_WRITE_THROUGH`: só volta depois de a troca estar no
/// disco (lá não há fsync de pasta). O antivírus e o indexador às vezes seguram o arquivo por um
/// instante: até 5 tentativas, a cada 20 ms, como o `dados::renomear`.
#[cfg(windows)]
pub fn trocar_por_cima(de: &Path, para: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt as _;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH, MoveFileExW};

    let largo = |caminho: &Path| caminho.as_os_str().encode_wide().chain([0]).collect::<Vec<u16>>();
    let (de, para) = (largo(de), largo(para));
    let mut tentativas = 5;
    loop {
        // SAFETY: dois caminhos terminados em zero, vivos durante a chamada.
        if unsafe { MoveFileExW(de.as_ptr(), para.as_ptr(), MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH) } != 0 {
            return Ok(());
        }
        let erro = io::Error::last_os_error();
        // ERROR_ACCESS_DENIED (5), ERROR_SHARING_VIOLATION (32) e ERROR_LOCK_VIOLATION (33).
        if tentativas == 1 || !matches!(erro.raw_os_error(), Some(5 | 32 | 33)) {
            return Err(erro);
        }
        tentativas -= 1;
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[cfg(test)]
pub mod testes {
    use super::*;
    use std::path::PathBuf;

    pub fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-atualizacao-{nome}-{}", uuid::Uuid::new_v4().simple()));
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    /// Um "vitral" de mentira que responde ao `--versao --json` como o de verdade.
    pub fn binario_falso(versao: &str) -> Vec<u8> {
        format!(
            "#!/bin/sh\nprintf '%s\\n' '{{\"versao\":\"{versao}\",\"formato_dados\":1,\"canal\":\"estavel\",\"commit\":\"\",\"plataforma\":\"linux-x86_64\"}}'\n"
        )
        .into_bytes()
    }

    pub enum Entrada<'a> {
        #[cfg_attr(not(unix), allow(dead_code, reason = "só os testes do Unix abrem um pacote com pasta"))]
        Pasta(&'a str),
        Arquivo(&'a str, &'a [u8]),
        /// Cabeçalho escrito à mão (caminho e tipo crus), para o que o `tar::Builder` não deixa fazer.
        Crua { caminho: &'a str, tipo: tar::EntryType, dados: &'a [u8], ligacao: &'a str },
    }

    pub fn tar_gz(entradas: &[Entrada]) -> Vec<u8> {
        let mut construtor = tar::Builder::new(flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast()));
        for entrada in entradas {
            let mut cabecalho = tar::Header::new_gnu();
            cabecalho.set_mtime(0);
            match *entrada {
                Entrada::Pasta(caminho) => {
                    cabecalho.set_entry_type(tar::EntryType::Directory);
                    cabecalho.set_mode(0o755);
                    cabecalho.set_size(0);
                    construtor.append_data(&mut cabecalho, caminho, io::empty()).unwrap();
                }
                Entrada::Arquivo(caminho, dados) => {
                    cabecalho.set_entry_type(tar::EntryType::Regular);
                    cabecalho.set_mode(0o600);
                    cabecalho.set_size(dados.len() as u64);
                    construtor.append_data(&mut cabecalho, caminho, dados).unwrap();
                }
                Entrada::Crua { caminho, tipo, dados, ligacao } => {
                    let velho = cabecalho.as_old_mut();
                    velho.name[..caminho.len()].copy_from_slice(caminho.as_bytes());
                    velho.linkname[..ligacao.len()].copy_from_slice(ligacao.as_bytes());
                    cabecalho.set_entry_type(tipo);
                    cabecalho.set_mode(0o777);
                    cabecalho.set_size(dados.len() as u64);
                    cabecalho.set_cksum();
                    construtor.append(&cabecalho, dados).unwrap();
                }
            }
        }
        construtor.into_inner().unwrap().finish().unwrap()
    }

    #[cfg_attr(not(unix), allow(dead_code, reason = "o binário de mentira é um script do sh"))]
    pub fn pacote_bom(prefixo: &str, versao: &str) -> Vec<u8> {
        let binario = binario_falso(versao);
        let vitral = format!("{prefixo}/vitral");
        let novidades = format!("{prefixo}/NOVIDADES.md");
        tar_gz(&[
            Entrada::Pasta(prefixo),
            Entrada::Arquivo(&vitral, &binario),
            Entrada::Arquivo(&novidades, b"## novidades\n"),
        ])
    }

    fn abrir(bytes: &[u8], prefixo: &str, limite: u64) -> (PathBuf, Result<()>) {
        let pasta = pasta_de_teste("extrair");
        fs::write(pasta.join("pacote.tar.gz"), bytes).unwrap();
        let destino = pasta.join("novo");
        fs::create_dir(&destino).unwrap();
        let resultado = extrair(&pasta.join("pacote.tar.gz"), &destino, prefixo, "vitral", limite);
        (destino, resultado)
    }

    const PREFIXO: &str = "vitral-0.9.0-linux-x86_64";

    // O binário de mentira é um script do sh, e as permissões são as do Unix.
    #[cfg(unix)]
    #[test]
    fn abre_o_pacote_e_acerta_as_permissoes() {
        use std::os::unix::fs::PermissionsExt as _;
        let (destino, resultado) = abrir(&pacote_bom(PREFIXO, "0.9.0"), PREFIXO, LIMITE_EXTRAIDO);
        resultado.unwrap();
        let modo = |n: &str| fs::metadata(destino.join(n)).unwrap().permissions().mode() & 0o777;
        assert_eq!(modo("vitral"), 0o755);
        assert_eq!(modo("NOVIDADES.md"), 0o644);
        let resposta = conferir_binario(&destino.join("vitral"), "0.9.0", PRAZO_CONFERENCIA).unwrap();
        assert_eq!(resposta, Resposta { versao: "0.9.0".into(), formato_dados: 1 });
        fs::remove_dir_all(destino.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn abre_o_pacote_do_tar_do_gnu() {
        // O mesmo formato que o release.yml gera (tar do GNU com --sort=name --owner=0 …).
        let pacote = include_bytes!("teste/ponta/vitral-0.9.0-linux-x86_64.tar.gz");
        let (destino, resultado) = abrir(pacote, PREFIXO, LIMITE_EXTRAIDO);
        resultado.unwrap();
        assert!(destino.join("NOVIDADES.md").is_file());
        conferir_binario(&destino.join("vitral"), "0.9.0", PRAZO_CONFERENCIA).unwrap();
        fs::remove_dir_all(destino.parent().unwrap()).unwrap();
    }

    #[test]
    fn recusa_o_que_sai_da_pasta_ou_nao_e_arquivo() {
        let binario = binario_falso("0.9.0");
        let vitral = format!("{PREFIXO}/vitral");
        let casos: Vec<(&str, Vec<u8>)> = vec![
            (
                "..",
                tar_gz(&[
                    Entrada::Arquivo(&vitral, &binario),
                    Entrada::Crua { caminho: "vitral-0.9.0-linux-x86_64/../../fora", tipo: tar::EntryType::Regular, dados: b"x", ligacao: "" },
                ]),
            ),
            (
                "absoluto",
                tar_gz(&[
                    Entrada::Arquivo(&vitral, &binario),
                    Entrada::Crua { caminho: "/vitral-0.9.0-linux-x86_64/x", tipo: tar::EntryType::Regular, dados: b"x", ligacao: "" },
                ]),
            ),
            (
                "link",
                tar_gz(&[Entrada::Crua { caminho: "vitral-0.9.0-linux-x86_64/vitral", tipo: tar::EntryType::Symlink, dados: b"", ligacao: "/bin/sh" }]),
            ),
            (
                "link físico",
                tar_gz(&[
                    Entrada::Arquivo("vitral-0.9.0-linux-x86_64/LEIA-ME.md", b"oi"),
                    Entrada::Crua { caminho: "vitral-0.9.0-linux-x86_64/vitral", tipo: tar::EntryType::Link, dados: b"", ligacao: "vitral-0.9.0-linux-x86_64/LEIA-ME.md" },
                ]),
            ),
            (
                "dispositivo",
                tar_gz(&[
                    Entrada::Arquivo(&vitral, &binario),
                    Entrada::Crua { caminho: "vitral-0.9.0-linux-x86_64/tty", tipo: tar::EntryType::Char, dados: b"", ligacao: "" },
                ]),
            ),
            ("outra pasta", tar_gz(&[Entrada::Arquivo("vitral-0.8.0-linux-x86_64/vitral", &binario)])),
            ("solto", tar_gz(&[Entrada::Arquivo(&vitral, &binario), Entrada::Arquivo("vitral", &binario)])),
            ("repetido", tar_gz(&[Entrada::Arquivo(&vitral, &binario), Entrada::Arquivo(&vitral, b"outro")])),
            ("sem o vitral", tar_gz(&[Entrada::Arquivo("vitral-0.9.0-linux-x86_64/LEIA-ME.md", b"oi")])),
            ("não é tar", b"isto nao e um pacote".to_vec()),
        ];
        for (nome, bytes) in casos {
            let (destino, resultado) = abrir(&bytes, PREFIXO, LIMITE_EXTRAIDO);
            assert!(resultado.is_err(), "{nome} passou");
            // Nada foi escrito fora da pasta nova.
            let pai = destino.parent().unwrap();
            let mut nomes: Vec<_> = fs::read_dir(pai).unwrap().map(|e| e.unwrap().file_name()).collect();
            nomes.sort();
            assert_eq!(nomes, ["novo", "pacote.tar.gz"], "{nome}");
            assert!(!pai.parent().unwrap().join("fora").exists());
            fs::remove_dir_all(pai).unwrap();
        }
    }

    #[test]
    fn recusa_o_pacote_maior_que_o_limite() {
        let grande = vec![b'x'; 4000];
        let binario = binario_falso("0.9.0");
        let vitral = format!("{PREFIXO}/vitral");
        let leia = format!("{PREFIXO}/LEIA-ME.md");
        let bytes = tar_gz(&[Entrada::Arquivo(&vitral, &binario), Entrada::Arquivo(&leia, &grande)]);
        let (destino, resultado) = abrir(&bytes, PREFIXO, 2000);
        assert!(resultado.unwrap_err().to_string().contains("limite"));
        fs::remove_dir_all(destino.parent().unwrap()).unwrap();
        let (destino, resultado) = abrir(&bytes, PREFIXO, 8000);
        resultado.unwrap();
        fs::remove_dir_all(destino.parent().unwrap()).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn o_binario_que_nao_responde_direito_e_recusado() {
        let pasta = pasta_de_teste("conferir");
        let escrever = |nome: &str, texto: &str| {
            let caminho = pasta.join(nome);
            gravar_atomico(&caminho, texto.as_bytes(), 0o755).unwrap();
            caminho
        };
        let zerado = escrever("zerado", "");
        assert!(conferir_binario(&zerado, "0.9.0", PRAZO_CONFERENCIA).is_err());
        let outra = escrever("outra", std::str::from_utf8(&binario_falso("0.8.0")).unwrap());
        let erro = conferir_binario(&outra, "0.9.0", PRAZO_CONFERENCIA).unwrap_err();
        assert!(erro.to_string().contains("0.8.0"), "{erro}");
        let falha = escrever("falha", "#!/bin/sh\nexit 3\n");
        assert!(conferir_binario(&falha, "0.9.0", PRAZO_CONFERENCIA).is_err());
        let lixo = escrever("lixo", "#!/bin/sh\necho oi\n");
        assert!(conferir_binario(&lixo, "0.9.0", PRAZO_CONFERENCIA).is_err());
        let preso = escrever("preso", "#!/bin/sh\nexec sleep 30\n");
        let antes = Instant::now();
        assert!(conferir_binario(&preso, "0.9.0", Duration::from_millis(300)).is_err());
        assert!(antes.elapsed() < Duration::from_secs(5));
        fs::remove_dir_all(&pasta).unwrap();
    }
}
