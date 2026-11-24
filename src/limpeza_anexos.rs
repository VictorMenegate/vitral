//! As imagens coladas na caixa viram `anexos/colado-<id>.<ext>` nos dados do Vitral, e a mensagem,
//! a fila e o rascunho guardam só o caminho. Colar e não enviar (ou apagar a conversa) deixava o
//! arquivo para sempre. Ao abrir o Vitral, em segundo plano, saem os `colado-*` que nenhum arquivo
//! de dados cita: o `estado.json` (filas, rascunhos), o `.anterior` dele e todas as conversas em
//! `chats/`, as arquivadas também. O que não tem o padrão `colado-…` (anexo que veio de arquivo
//! do usuário, e o que o `anexos/` guarda de outros nomes) nunca é tocado.
//!
//! Três cuidados: só vai embora o que está parado há mais de um dia (`CARENCIA`); qualquer arquivo
//! de dados que não se consiga ler cancela a limpeza inteira (sem saber o que ele cita, não se
//! apaga nada); e o que está na caixa agora, em memória, entra como citado.

use std::{
    collections::HashSet,
    fs,
    path::Path,
    time::{Duration, SystemTime},
};

use crate::dados::Dados;

/// Um arquivo colado só é apagado depois de tanto tempo parado.
pub const CARENCIA: Duration = Duration::from_secs(24 * 60 * 60);

const EXTENSOES: [&str; 4] = ["png", "jpg", "gif", "webp"];

/// Se `nome` é o nome de uma imagem colada: `colado-<letras e números>.<png|jpg|gif|webp>`.
pub fn nome_de_colado(nome: &str) -> bool {
    let Some(resto) = nome.strip_prefix("colado-") else { return false };
    let Some((id, extensao)) = resto.rsplit_once('.') else { return false };
    !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric()) && EXTENSOES.contains(&extensao)
}

/// Os nomes `colado-…` que o texto cita (de qualquer jeito: caminho inteiro, JSON, Markdown).
pub fn colados_citados(texto: &str) -> HashSet<String> {
    let mut achados = HashSet::new();
    let mut resto = texto;
    while let Some(i) = resto.find("colado-") {
        let a_partir = &resto[i..];
        let fim = a_partir
            .find(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '.')))
            .unwrap_or(a_partir.len());
        // Um ponto de fim de frase não faz parte do nome.
        let candidato = a_partir[..fim].trim_end_matches('.');
        if nome_de_colado(candidato) {
            achados.insert(candidato.to_string());
        }
        resto = &a_partir["colado-".len()..];
    }
    achados
}

/// Um arquivo da pasta dos anexos: o nome e há quanto tempo está parado.
pub struct Presente {
    pub nome: String,
    pub parado_ha: Duration,
}

/// Quais dos arquivos presentes podem ir embora: colados, não citados e parados além da carência.
pub fn orfaos(presentes: &[Presente], citados: &HashSet<String>, carencia: Duration) -> Vec<String> {
    presentes
        .iter()
        .filter(|p| nome_de_colado(&p.nome) && !citados.contains(&p.nome) && p.parado_ha > carencia)
        .map(|p| p.nome.clone())
        .collect()
}

/// O que os arquivos de dados citam. `None` se algum não pôde ser lido.
fn citados_nos_dados(dados: &Dados) -> Option<HashSet<String>> {
    let mut citados = HashSet::new();
    let mut ler = |arquivo: &Path| -> Option<()> {
        let bytes = fs::read(arquivo).ok()?;
        citados.extend(colados_citados(&String::from_utf8_lossy(&bytes)));
        Some(())
    };
    // Os arquivos soltos da pasta de dados (estado, cópia anterior, ideias…) e as conversas.
    for pasta in [dados.pasta.clone(), dados.pasta.join("chats")] {
        let entradas = match fs::read_dir(&pasta) {
            Ok(e) => e,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(_) => return None,
        };
        for entrada in entradas {
            let entrada = entrada.ok()?;
            if entrada.file_type().ok()?.is_file() {
                ler(&entrada.path())?;
            }
        }
    }
    Some(citados)
}

/// Apaga os `colado-*` órfãos de `anexos/`; devolve quantos saíram. `em_uso`: o que está na caixa
/// agora. Bloqueia: chamar em segundo plano.
pub fn limpar(dados: &Dados, em_uso: &HashSet<String>) -> usize {
    let Some(mut citados) = citados_nos_dados(dados) else { return 0 };
    citados.extend(em_uso.iter().cloned());
    let pasta = dados.pasta_anexos();
    let Ok(entradas) = fs::read_dir(&pasta) else { return 0 };
    let agora = SystemTime::now();
    let presentes: Vec<Presente> = entradas
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .filter_map(|e| {
            let parado_ha = e.metadata().ok()?.modified().ok().and_then(|m| agora.duration_since(m).ok()).unwrap_or_default();
            Some(Presente { nome: e.file_name().to_string_lossy().into_owned(), parado_ha })
        })
        .collect();
    orfaos(&presentes, &citados, CARENCIA)
        .iter()
        .filter(|nome| fs::remove_file(pasta.join(nome)).is_ok())
        .count()
}

#[cfg(test)]
mod testes {
    use std::{collections::HashSet, fs, time::Duration};

    use super::{CARENCIA, Presente, colados_citados, limpar, nome_de_colado, orfaos};
    use crate::{dados::Dados, modelo::novo_id};

    fn presente(nome: &str, dias: u64) -> Presente {
        Presente { nome: nome.into(), parado_ha: Duration::from_secs(dias * 24 * 3600 + 60) }
    }

    #[test]
    fn so_o_nome_exato_de_uma_colada_conta() {
        assert!(nome_de_colado("colado-a1b2c3d4e5.png"));
        assert!(nome_de_colado("colado-0123456789.jpg"));
        assert!(!nome_de_colado("colado-.png"));
        assert!(!nome_de_colado("colado-abc.txt"));
        assert!(!nome_de_colado("meu-colado-abc.png"));
        assert!(!nome_de_colado("foto.png"));
        assert!(!nome_de_colado("colado-a/b.png"));
    }

    #[test]
    fn acha_o_nome_dentro_de_json_e_de_caminho() {
        let texto = r#"{"anexos":["/home/v/.local/share/vitral/anexos/colado-abc123.png"],"t":"veja colado-zz9.webp."}"#;
        let achados = colados_citados(texto);
        assert_eq!(achados, ["colado-abc123.png", "colado-zz9.webp"].iter().map(|s| s.to_string()).collect());
        assert!(colados_citados("nada aqui, só colado- e colado-x.txt").is_empty());
    }

    /// Nada que esteja citado é apagado, nem parado há um mês; o não citado e velho sai; o recente
    /// fica (pode estar na caixa de outra janela); o que não é `colado-` nunca é tocado.
    #[test]
    fn o_citado_e_o_recente_ficam() {
        let presentes = [
            presente("colado-citado1.png", 30),
            presente("colado-orfao1.png", 30),
            presente("colado-recente.png", 0),
            presente("do-usuario.png", 30),
        ];
        let citados: HashSet<String> = ["colado-citado1.png".to_string()].into();
        assert_eq!(orfaos(&presentes, &citados, CARENCIA), ["colado-orfao1.png"]);
    }

    /// Ponta a ponta no disco: citado pelo estado, por uma conversa arquivada e pela caixa em
    /// memória, todos ficam; o órfão velho sai; e com um arquivo de dados ilegível não sai nada.
    #[test]
    fn limpa_o_disco_sem_apagar_o_citado() {
        let raiz = std::env::temp_dir().join(format!("vitral-limpeza-{}", novo_id()));
        let dados = Dados::em(&raiz);
        fs::create_dir_all(dados.pasta_anexos()).unwrap();
        fs::create_dir_all(dados.pasta.join("chats")).unwrap();
        let velho = std::time::SystemTime::now() - Duration::from_secs(5 * 24 * 3600);
        for nome in ["colado-noestado.png", "colado-numachat.png", "colado-nacaixa.png", "colado-orfao.png", "colado-orfao2.jpg", "meu.png"] {
            let arquivo = dados.pasta_anexos().join(nome);
            fs::write(&arquivo, b"x").unwrap();
            fs::File::options().write(true).open(&arquivo).unwrap().set_modified(velho).unwrap();
        }
        fs::write(dados.arquivo_estado(), r#"{"filas":{"a":[{"anexos":["/x/anexos/colado-noestado.png"]}]}}"#).unwrap();
        fs::write(dados.arquivo_chat("arquivada"), r#"[{"Usuario":{"anexos":["/x/anexos/colado-numachat.png"]}}]"#).unwrap();
        let em_uso: HashSet<String> = ["colado-nacaixa.png".to_string()].into();

        // Um arquivo de dados que não abre (sem permissão de leitura) cancela tudo.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let ilegivel = dados.pasta.join("chats").join("estragado.json");
            fs::write(&ilegivel, "x").unwrap();
            fs::set_permissions(&ilegivel, fs::Permissions::from_mode(0o000)).unwrap();
            if fs::read(&ilegivel).is_err() {
                assert_eq!(limpar(&dados, &em_uso), 0, "com um chat ilegível nada é apagado");
                assert!(dados.pasta_anexos().join("colado-orfao.png").exists());
            }
            fs::remove_file(&ilegivel).unwrap();
        }

        assert_eq!(limpar(&dados, &em_uso), 2);
        let sobrou: HashSet<String> =
            fs::read_dir(dados.pasta_anexos()).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        let esperado: HashSet<String> =
            ["colado-noestado.png", "colado-numachat.png", "colado-nacaixa.png", "meu.png"].iter().map(|s| s.to_string()).collect();
        assert_eq!(sobrou, esperado);
        let _ = fs::remove_dir_all(&raiz);
    }
}
