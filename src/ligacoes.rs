//! Links entre arquivos e pastas nos dois sistemas: é com eles que as contas extras (do Claude e
//! do Codex) dividem com a principal tudo menos o login.
//!
//! - No Linux, link simbólico, para arquivo e para pasta.
//! - No Windows, sem administrador e sem o Modo de Desenvolvedor (que o link simbólico pede): a
//!   pasta vira junção do NTFS (o `mklink /J`, que qualquer usuário cria) e o arquivo tenta o link
//!   simbólico (que funciona com o Modo de Desenvolvedor ligado ou como administrador) e, sem ele,
//!   vira hard link: o mesmo arquivo com dois nomes, que só existe no mesmo disco (a configuração
//!   do Vitral, no `%APPDATA%`, e o `~\.claude` estão os dois no perfil do usuário). Cópia não
//!   serve: a configuração mudada numa conta não chegaria nas outras.
//!
//! O hard link se parte quando um lado grava por troca atômica (um arquivo temporário renomeado por
//! cima, que é como o Claude Code e o Codex gravam a configuração): o nome passa a ser de um arquivo
//! novo, e o outro lado fica com o velho. Por isso os nomes que ficaram como hard link são anotados
//! na pasta da conta (`ANOTADOS`), e o `conferir`, antes de cada turno, refaz o que se partiu,
//! ficando com o texto mais novo.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// O arquivo, na pasta da conta, com os itens que ficaram como hard link, um por linha. Só esses o
/// `conferir` refaz: o arquivo que a conta criou sozinha, quando a principal ainda não tinha o
/// dela, segue separado (como no Linux) e nunca é misturado com o da principal.
const ANOTADOS: &str = ".vitral-hard-links";

/// Liga `destino` a `fonte`, que existe. Devolve se ficou como hard link (só no Windows).
#[cfg(unix)]
pub fn ligar(fonte: &Path, destino: &Path) -> io::Result<bool> {
    std::os::unix::fs::symlink(fonte, destino).map(|()| false)
}

/// Liga `destino` a `fonte`, que existe. Devolve se ficou como hard link: a pasta vira junção, e o
/// arquivo, link simbólico quando o Windows deixa e hard link quando não.
#[cfg(windows)]
pub fn ligar(fonte: &Path, destino: &Path) -> io::Result<bool> {
    if fonte.is_dir() {
        return windows::juncao(fonte, destino).map(|()| false);
    }
    match std::os::windows::fs::symlink_file(fonte, destino) {
        Ok(()) => Ok(false),
        Err(_) => fs::hard_link(fonte, destino).map(|()| true),
    }
}

/// Liga o `item` da pasta `original` na pasta da conta e anota se ficou como hard link.
pub fn ligar_item(original: &Path, pasta: &Path, item: &str) -> io::Result<()> {
    if ligar(&original.join(item), &pasta.join(item))? {
        anotar(pasta, item)?;
    }
    Ok(())
}

fn anotados(pasta: &Path) -> Vec<String> {
    fs::read_to_string(pasta.join(ANOTADOS))
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect()
}

fn anotar(pasta: &Path, item: &str) -> io::Result<()> {
    let mut itens = anotados(pasta);
    if !itens.iter().any(|i| i == item) {
        itens.push(item.to_string());
        fs::write(pasta.join(ANOTADOS), itens.join("\n") + "\n")?;
    }
    Ok(())
}

/// Antes de um turno, numa pasta de conta: liga o que apareceu na principal depois que a conta foi
/// criada e refaz o hard link anotado que se partiu (`religar`). O erro de um item não impede os
/// outros, nem o turno: fica no log.
pub fn conferir(original: &Path, pasta: &Path, itens: &[&str]) {
    if !pasta.is_dir() || !original.is_dir() {
        return;
    }
    let anotados = anotados(pasta);
    for item in itens {
        let (fonte, destino) = (original.join(item), pasta.join(item));
        let feito = if fs::symlink_metadata(&destino).is_err() {
            if fonte.exists() { ligar_item(original, pasta, item) } else { Ok(()) }
        } else if anotados.iter().any(|a| a == item) {
            religar(&fonte, &destino)
        } else {
            Ok(())
        };
        if let Err(erro) = feito {
            eprintln!("vitral: não refez a ligação de {item} em {}: {erro}", pasta.display());
        }
    }
}

/// Refaz o hard link partido entre `fonte` (o da principal) e `destino` (o da conta), ficando com o
/// texto mais novo. A conta gravou por último: o arquivo dela vai para o lugar do da principal, por
/// troca atômica, como o próprio agente grava, e as outras contas, que ficaram com o velho, voltam
/// para ele no `conferir` delas. A principal (ou outra conta) gravou por último: a conta volta a ser
/// um nome do arquivo da principal, também por troca. O que ainda é o mesmo arquivo dos dois lados,
/// ou virou link, fica como está.
fn religar(fonte: &Path, destino: &Path) -> io::Result<()> {
    let dados = fs::symlink_metadata(destino)?;
    if !dados.is_file() || !fonte.is_file() || mesmo_arquivo(fonte, destino)? {
        return Ok(());
    }
    // O arquivo de verdade: o `~/.claude/settings.json` que é link para outro lugar continua link.
    let fonte = dunce::canonicalize(fonte)?;
    if dados.modified()? > fs::metadata(&fonte)?.modified()? {
        fs::rename(destino, &fonte)?;
        return fs::hard_link(&fonte, destino);
    }
    let temporario = temporario_ao_lado(destino);
    fs::hard_link(&fonte, &temporario)?;
    fs::rename(&temporario, destino).inspect_err(|_| {
        let _ = fs::remove_file(&temporario);
    })
}

fn temporario_ao_lado(caminho: &Path) -> PathBuf {
    let nome = caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    caminho.with_file_name(format!(".{nome}.vitral-{}", crate::modelo::novo_id()))
}

/// Os dois caminhos são o mesmo arquivo no disco: dois nomes de um hard link, ou um link e o
/// arquivo para onde ele aponta.
#[cfg(unix)]
pub fn mesmo_arquivo(a: &Path, b: &Path) -> io::Result<bool> {
    use std::os::unix::fs::MetadataExt as _;
    let (a, b) = (fs::metadata(a)?, fs::metadata(b)?);
    Ok((a.dev(), a.ino()) == (b.dev(), b.ino()))
}

/// Os dois caminhos são o mesmo arquivo no disco: dois nomes de um hard link, ou um link e o
/// arquivo para onde ele aponta.
#[cfg(windows)]
pub fn mesmo_arquivo(a: &Path, b: &Path) -> io::Result<bool> {
    let id = |caminho: &Path| {
        windows::informacoes(caminho).map(|i| (i.dwVolumeSerialNumber, i.nFileIndexHigh, i.nFileIndexLow))
    };
    Ok(id(a)? == id(b)?)
}

/// Quantos nomes (hard links) o arquivo tem. No Linux é o `nlink` do metadata; no Windows a
/// pergunta só tem resposta pelo handle do arquivo.
#[cfg(windows)]
pub fn nomes(caminho: &Path) -> io::Result<u32> {
    windows::informacoes(caminho).map(|i| i.nNumberOfLinks)
}

#[cfg(windows)]
mod windows {
    use std::{
        fs, io,
        os::windows::{ffi::OsStrExt as _, fs::OpenOptionsExt as _, io::AsRawHandle as _},
        path::{Component, Path, Prefix},
    };

    use windows_sys::Win32::{
        Foundation::HANDLE,
        Storage::FileSystem::{
            BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            GetFileInformationByHandle,
        },
        System::IO::DeviceIoControl,
    };

    /// `FSCTL_SET_REPARSE_POINT` (winioctl.h) e `IO_REPARSE_TAG_MOUNT_POINT` (winnt.h): dois números
    /// que ficam aqui para não ligar mais duas features do windows-sys.
    const FSCTL_SET_REPARSE_POINT: u32 = 0x0009_00A4;
    const IO_REPARSE_TAG_MOUNT_POINT: u32 = 0xA000_0003;

    /// O volume, o índice e o número de nomes do arquivo.
    pub fn informacoes(caminho: &Path) -> io::Result<BY_HANDLE_FILE_INFORMATION> {
        let arquivo = fs::File::open(caminho)?;
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: o handle é do arquivo aberto aqui, vivo durante a chamada, e a struct também.
        if unsafe { GetFileInformationByHandle(arquivo.as_raw_handle() as HANDLE, &mut info) } == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(info)
    }

    /// Cria a pasta `juncao` apontando para `alvo`, como o `mklink /J`: uma pasta vazia com um
    /// ponto de nova análise de montagem (`IO_REPARSE_TAG_MOUNT_POINT`), gravado pelo
    /// `FSCTL_SET_REPARSE_POINT`. Não pede privilégio nenhum, só poder escrever na pasta de cima.
    /// Só aponta para uma pasta num disco deste computador (rede não).
    pub fn juncao(alvo: &Path, juncao: &Path) -> io::Result<()> {
        let alvo = std::path::absolute(alvo)?;
        let mut nome: Vec<u16> = alvo.as_os_str().encode_wide().collect();
        let fora = || io::Error::new(io::ErrorKind::Unsupported, "a junção só aponta para uma pasta num disco deste computador");
        match alvo.components().next() {
            Some(Component::Prefix(prefixo)) => match prefixo.kind() {
                Prefix::Disk(_) => {}
                // `\\?\C:\…`: o mesmo caminho sem o `\\?\`.
                Prefix::VerbatimDisk(_) => nome = nome.split_off(4),
                _ => return Err(fora()),
            },
            _ => return Err(fora()),
        }
        // O nome de verdade é o do NT (`\??\C:\…`); o outro é o que o Explorer mostra.
        let substituto: Vec<u16> = r"\??\".encode_utf16().chain(nome.iter().copied()).collect();
        let em_bytes = |unidades: usize| {
            u16::try_from(unidades * 2)
                .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "caminho comprido demais para uma junção"))
        };
        // O REPARSE_DATA_BUFFER de montagem, em unidades de 16 bits: a etiqueta (32 bits), o tamanho
        // do que vem depois do cabeçalho e um campo reservado; o deslocamento e o tamanho, em
        // bytes, de cada nome; e os dois nomes, cada um com o NUL do fim (que não entra no tamanho).
        let depois_do_cabecalho = em_bytes(4 + substituto.len() + 1 + nome.len() + 1)?;
        let (tamanho_substituto, tamanho_nome) = (em_bytes(substituto.len())?, em_bytes(nome.len())?);
        let mut buffer: Vec<u16> = vec![
            IO_REPARSE_TAG_MOUNT_POINT as u16,
            (IO_REPARSE_TAG_MOUNT_POINT >> 16) as u16,
            depois_do_cabecalho,
            0,
            0,
            tamanho_substituto,
            tamanho_substituto + 2,
            tamanho_nome,
        ];
        buffer.extend(&substituto);
        buffer.push(0);
        buffer.extend(&nome);
        buffer.push(0);
        fs::create_dir(juncao)?;
        let feito = gravar_ponto(juncao, &buffer);
        if feito.is_err() {
            let _ = fs::remove_dir(juncao);
        }
        feito
    }

    fn gravar_ponto(pasta: &Path, buffer: &[u16]) -> io::Result<()> {
        // A pasta só abre com o `BACKUP_SEMANTICS`; o `OPEN_REPARSE_POINT` abre ela mesma, e não
        // para onde ela aponta.
        let pasta = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
            .open(pasta)?;
        let mut devolvidos = 0u32;
        // SAFETY: o handle e o buffer (com o tamanho dele em bytes) vivem durante a chamada, que é
        // síncrona: sem saída e sem OVERLAPPED.
        let gravou = unsafe {
            DeviceIoControl(
                pasta.as_raw_handle() as HANDLE,
                FSCTL_SET_REPARSE_POINT,
                buffer.as_ptr().cast(),
                (buffer.len() * 2) as u32,
                std::ptr::null_mut(),
                0,
                &mut devolvidos,
                std::ptr::null_mut(),
            )
        };
        if gravou == 0 { Err(io::Error::last_os_error()) } else { Ok(()) }
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::time::{Duration, SystemTime};

    fn pasta_de_teste(nome: &str) -> (PathBuf, PathBuf, PathBuf) {
        let base = std::env::temp_dir().join(format!("vitral-ligacoes-{nome}-{}", crate::modelo::novo_id()));
        let (principal, conta) = (base.join("principal"), base.join("conta"));
        fs::create_dir_all(&principal).unwrap();
        fs::create_dir_all(&conta).unwrap();
        (base, principal, conta)
    }

    /// Grava como o Claude Code e o Codex gravam: um temporário renomeado por cima, com a hora
    /// dada (a do sistema de arquivos pode ser a mesma para duas gravações seguidas).
    fn trocar(caminho: &Path, texto: &str, hora: SystemTime) {
        let temporario = temporario_ao_lado(caminho);
        fs::write(&temporario, texto).unwrap();
        fs::File::options().write(true).open(&temporario).unwrap().set_modified(hora).unwrap();
        fs::rename(&temporario, caminho).unwrap();
    }

    #[test]
    fn o_hard_link_partido_volta_com_o_texto_mais_novo() {
        let (base, principal, conta) = pasta_de_teste("partido");
        let (fonte, destino) = (principal.join("settings.json"), conta.join("settings.json"));
        let agora = SystemTime::now();
        trocar(&fonte, "a", agora);
        fs::hard_link(&fonte, &destino).unwrap();
        anotar(&conta, "settings.json").unwrap();
        anotar(&conta, "settings.json").unwrap();
        assert_eq!(anotados(&conta), ["settings.json"], "anotar de novo não repete");
        assert!(mesmo_arquivo(&fonte, &destino).unwrap());
        // A principal grava: a conta ficou com o arquivo velho, e volta para o novo.
        trocar(&fonte, "b", agora + Duration::from_secs(10));
        assert!(!mesmo_arquivo(&fonte, &destino).unwrap());
        conferir(&principal, &conta, &["settings.json"]);
        assert!(mesmo_arquivo(&fonte, &destino).unwrap());
        assert_eq!(fs::read_to_string(&destino).unwrap(), "b");
        // A conta grava: o texto dela vira o da principal.
        trocar(&destino, "c", agora + Duration::from_secs(20));
        conferir(&principal, &conta, &["settings.json"]);
        assert!(mesmo_arquivo(&fonte, &destino).unwrap());
        assert_eq!(fs::read_to_string(&fonte).unwrap(), "c");
        // Conferir de novo não muda nada.
        conferir(&principal, &conta, &["settings.json"]);
        assert_eq!(fs::read_to_string(&destino).unwrap(), "c");
        let sobras: Vec<_> = fs::read_dir(&conta).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(sobras.len(), 2, "sem temporário esquecido: {sobras:?}");
        let _ = fs::remove_dir_all(base);
    }

    #[test]
    fn o_arquivo_proprio_da_conta_fica_e_o_que_falta_e_ligado() {
        let (base, principal, conta) = pasta_de_teste("proprio");
        // A conta criou o dela antes de a principal ter um: não é hard link anotado, fica separado.
        fs::write(conta.join("history.jsonl"), "da conta").unwrap();
        fs::write(principal.join("history.jsonl"), "da principal").unwrap();
        // Apareceu na principal depois da conta criada: é ligado no próximo turno.
        fs::write(principal.join("AGENTS.md"), "regras").unwrap();
        fs::create_dir_all(principal.join("skills")).unwrap();
        conferir(&principal, &conta, &["history.jsonl", "AGENTS.md", "skills", "rules"]);
        assert_eq!(fs::read_to_string(conta.join("history.jsonl")).unwrap(), "da conta");
        assert_eq!(fs::read_to_string(principal.join("history.jsonl")).unwrap(), "da principal");
        assert_eq!(fs::read_to_string(conta.join("AGENTS.md")).unwrap(), "regras");
        fs::write(conta.join("skills").join("nova.md"), "x").unwrap();
        assert!(principal.join("skills").join("nova.md").is_file(), "a pasta é a mesma");
        assert!(fs::symlink_metadata(conta.join("rules")).is_err(), "o que a principal não tem não é criado");
        let _ = fs::remove_dir_all(base);
    }

    /// Apagar a conta tira os links, sem entrar neles: a principal fica inteira (no Windows, o
    /// `remove_dir_all` não atravessa a junção).
    #[test]
    fn apagar_a_pasta_da_conta_nao_apaga_a_principal() {
        let (base, principal, conta) = pasta_de_teste("apagar");
        fs::create_dir_all(principal.join("projects").join("p")).unwrap();
        fs::write(principal.join("projects").join("p").join("s.jsonl"), "{}").unwrap();
        fs::write(principal.join("CLAUDE.md"), "eu").unwrap();
        for item in ["projects", "CLAUDE.md"] {
            ligar_item(&principal, &conta, item).unwrap();
        }
        assert_eq!(fs::read_to_string(conta.join("projects").join("p").join("s.jsonl")).unwrap(), "{}");
        fs::remove_dir_all(&conta).unwrap();
        assert!(principal.join("projects").join("p").join("s.jsonl").is_file());
        assert_eq!(fs::read_to_string(principal.join("CLAUDE.md")).unwrap(), "eu");
        let _ = fs::remove_dir_all(base);
    }
}
