//! O diagnóstico para colar num pedido de ajuda (Configurações > Vitral > Copiar diagnóstico): as
//! versões, o sistema, onde ficam os dados, as opções ligadas e o fim do registro de quedas. É
//! texto puro, montado na hora do clique. Nada de chave, token ou texto de conversa: o que tem
//! cara de segredo é coberto (`cobrir`), a pasta pessoal vira `~`, as instruções do usuário entram
//! só pelo tamanho e o endereço da memória da equipe só como "ligada".

use std::path::{Path, PathBuf};

/// O que entra no lugar de um segredo.
pub const COBERTO: &str = "[coberto]";

/// Quantas linhas do fim do `queda.log` vão no diagnóstico.
pub const LINHAS_DO_LOG: usize = 40;

/// O que o diagnóstico precisa saber, já lido (a montagem do texto é pura, para os testes).
#[derive(Clone, Debug, Default)]
pub struct Entrada {
    /// "0.9.6 · estável · a1b2c3d".
    pub vitral: String,
    /// "Fedora Linux 44 · kernel 7.2 · wayland · GNOME".
    pub sistema: String,
    pub claude: Option<String>,
    pub codex: Option<String>,
    pub dados: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    pub chats: usize,
    /// "nome: valor" de cada opção que ajuda a entender um problema (nunca um segredo).
    pub opcoes: Vec<(String, String)>,
    /// O conteúdo do `queda.log`, se existe.
    pub quedas: Option<String>,
    /// A pasta pessoal, que sai dos caminhos.
    pub casa: Option<PathBuf>,
}

/// Cobre o que tem cara de segredo num texto qualquer (uma linha de log, um caminho, um valor de
/// opção) e troca a pasta pessoal por `~`. Na dúvida, cobre: perder um pedaço do log atrapalha
/// menos do que vazar uma chave.
pub fn cobrir(texto: &str, casa: Option<&Path>) -> String {
    let texto = match casa.map(|c| c.to_string_lossy().to_string()).filter(|c| c.len() > 1) {
        Some(casa) => texto.replace(&casa, "~"),
        None => texto.to_string(),
    };
    texto.split('\n').map(cobrir_linha).collect::<Vec<_>>().join("\n")
}

/// Os nomes que anunciam um segredo logo depois de `:` ou `=` ("api_key", "X-Auth-Token",
/// "chave"). "key" pega mais do que devia (um "keymap: x" some); na dúvida, cobre.
const NOMES_SENSIVEIS: [&str; 11] =
    ["token", "secret", "segredo", "senha", "password", "passwd", "key", "chave", "authorization", "credential", "cookie"];

/// Os começos das chaves que o Vitral e os agentes usam (Anthropic, OpenAI, ai-memory, GitHub,
/// Linear, Slack, AWS, Google) e do JWT.
const PREFIXOS_DE_CHAVE: [&str; 14] =
    ["sk-", "aim_", "ghp_", "gho_", "ghu_", "ghs_", "github_pat_", "lin_api_", "lin_oauth_", "xoxb-", "xoxp-", "AKIA", "AIza", "eyJ"];

fn cobrir_linha(linha: &str) -> String {
    let mut saida = String::with_capacity(linha.len());
    let mut resto_coberto = false;
    let mut depois_de_bearer = false;
    for pedaco in linha.split_inclusive(char::is_whitespace) {
        let palavra = pedaco.trim_end();
        let espaco = &pedaco[palavra.len()..];
        if palavra.is_empty() {
            saida.push_str(pedaco);
            continue;
        }
        if resto_coberto {
            continue;
        }
        if depois_de_bearer {
            depois_de_bearer = false;
            saida.push_str(COBERTO);
            saida.push_str(espaco);
            continue;
        }
        let (coberta, cobre_o_resto) = cobrir_palavra(palavra);
        depois_de_bearer = matches!(sem_pontuacao(palavra).to_lowercase().as_str(), "bearer" | "basic");
        saida.push_str(&coberta);
        if cobre_o_resto {
            // "senha: abc def": tudo o que vem depois do nome, até o fim da linha.
            resto_coberto = true;
            if !coberta.ends_with(COBERTO) {
                saida.push(' ');
                saida.push_str(COBERTO);
            }
            continue;
        }
        saida.push_str(espaco);
    }
    saida.trim_end_matches(' ').to_string()
}

fn sem_pontuacao(palavra: &str) -> &str {
    palavra.trim_matches(|c: char| matches!(c, '"' | '\'' | ',' | ';' | '(' | ')' | '[' | ']' | '{' | '}' | '<' | '>' | '`'))
}

/// Um caminho antes dos dois-pontos ("src/ui/keyboard.rs:10") não é nome de segredo.
fn nome_sensivel(nome: &str) -> bool {
    let nome = sem_pontuacao(nome).to_lowercase();
    !nome.contains(['/', '\\']) && NOMES_SENSIVEIS.iter().any(|s| nome.contains(s))
}

/// Uma palavra (sem espaço) já coberta, e se o resto da linha também deve ser.
fn cobrir_palavra(palavra: &str) -> (String, bool) {
    // "TOKEN=abc", "chave:abc" e o nome sozinho com o separador no fim ("senha:", "\"token\":").
    if let Some(pos) = palavra.find(['=', ':']).filter(|pos| !palavra[*pos..].starts_with("://"))
        && nome_sensivel(&palavra[..pos])
    {
        let valor = sem_pontuacao(&palavra[pos + 1..]);
        return if valor.is_empty() { (palavra.to_string(), true) } else { (format!("{}{COBERTO}", &palavra[..=pos]), true) };
    }
    // "https://usuario:senha@servidor".
    if let Some((esquema, depois)) = palavra.split_once("://")
        && let Some((quem, servidor)) = depois.split_once('@')
        && !quem.contains('/')
    {
        return (format!("{esquema}://{COBERTO}@{}", cobrir_palavra(servidor).0), false);
    }
    let miolo = sem_pontuacao(palavra);
    if tem_cara_de_chave(miolo) {
        return (palavra.replace(miolo, COBERTO), false);
    }
    // Um segredo no meio de uma URL ou de um caminho ("?key=abc", "/v1/sk-ant-…").
    if palavra.contains(['?', '&', '/']) {
        let partes: Vec<String> = palavra
            .split_inclusive(['?', '&', '/'])
            .map(|parte| {
                let fim = parte.trim_end_matches(['?', '&', '/']);
                let separador = &parte[fim.len()..];
                match fim.split_once('=') {
                    Some((nome, valor)) if nome_sensivel(nome) && !valor.is_empty() => format!("{nome}={COBERTO}{separador}"),
                    _ if tem_cara_de_chave(sem_pontuacao(fim)) => format!("{}{separador}", fim.replace(sem_pontuacao(fim), COBERTO)),
                    _ => parte.to_string(),
                }
            })
            .collect();
        return (partes.concat(), false);
    }
    (palavra.to_string(), false)
}

/// Começa como uma chave conhecida, ou é comprida, sem ponto nem barra, com letra e número
/// misturados (um hash, um token opaco, um id de sessão).
fn tem_cara_de_chave(palavra: &str) -> bool {
    if PREFIXOS_DE_CHAVE.iter().any(|p| palavra.starts_with(p)) && palavra.len() >= 12 {
        return true;
    }
    let do_alfabeto = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '=');
    palavra.len() >= 32
        && palavra.chars().all(do_alfabeto)
        && palavra.chars().any(|c| c.is_ascii_digit())
        && palavra.chars().any(|c| c.is_ascii_alphabetic())
}

/// As últimas `linhas` linhas de um registro, sem as vazias do fim.
pub fn fim_do_log(conteudo: &str, linhas: usize) -> String {
    let todas: Vec<&str> = conteudo.trim_end().lines().collect();
    todas[todas.len().saturating_sub(linhas)..].join("\n")
}

/// O texto inteiro do diagnóstico, já coberto.
pub fn texto(e: &Entrada) -> String {
    let casa = e.casa.as_deref();
    let caminho = |p: &Path| cobrir(&p.display().to_string(), casa);
    let ou = |v: &Option<String>, falta: &str| v.clone().unwrap_or_else(|| falta.to_string());
    let mut t = String::from("Diagnóstico do Vitral\n\n");
    t.push_str(&format!("Vitral: {}\n", e.vitral));
    t.push_str(&format!("Sistema: {}\n", e.sistema));
    t.push_str(&format!("Claude Code: {}\n", ou(&e.claude, "não achei")));
    t.push_str(&format!("Codex: {}\n", ou(&e.codex, "não instalado")));
    t.push_str(&format!("\nDados: {}\nConfiguração: {}\nCache: {}\nChats: {}\n", caminho(&e.dados), caminho(&e.config), caminho(&e.cache), e.chats));
    if !e.opcoes.is_empty() {
        t.push_str("\nOpções:\n");
        for (nome, valor) in &e.opcoes {
            t.push_str(&format!("  {nome}: {}\n", cobrir(valor, casa)));
        }
    }
    match e.quedas.as_deref().map(|q| fim_do_log(q, LINHAS_DO_LOG)).filter(|q| !q.is_empty()) {
        Some(fim) => t.push_str(&format!("\nFim do registro de quedas (queda.log):\n{}\n", cobrir(&fim, casa))),
        None => t.push_str("\nRegistro de quedas: vazio\n"),
    }
    t
}

/// "Fedora Linux 44 (Workstation Edition) · kernel 7.2.15 · wayland · GNOME", com o que der para
/// ler; no Windows e no macOS, o nome do sistema e a arquitetura.
pub fn sistema() -> String {
    let mut partes = Vec::new();
    let distro = std::fs::read_to_string("/etc/os-release").ok().and_then(|t| {
        t.lines().find_map(|l| l.strip_prefix("PRETTY_NAME=")).map(|v| v.trim_matches('"').to_string())
    });
    partes.push(distro.unwrap_or_else(|| format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)));
    if let Ok(kernel) = std::fs::read_to_string("/proc/sys/kernel/osrelease") {
        partes.push(format!("kernel {}", kernel.trim()));
    }
    for variavel in ["XDG_SESSION_TYPE", "XDG_CURRENT_DESKTOP"] {
        if let Some(valor) = std::env::var(variavel).ok().filter(|v| !v.is_empty()) {
            partes.push(valor);
        }
    }
    partes.join(" · ")
}

#[cfg(test)]
mod testes {
    use super::*;

    fn sem_casa(texto: &str) -> String {
        cobrir(texto, None)
    }

    #[test]
    fn chaves_conhecidas_sao_cobertas() {
        for chave in [
            "sk-ant-api03-AbCdEfGh1234567890",
            "sk-proj-abcdefghijklmnop",
            "aim_3f9a8b7c6d5e4f3a2b1c",
            "ghp_abcdefghijklmnopqrstuvwxyz0123456789",
            "github_pat_11ABCDEFG0abcdefghij",
            "lin_api_abcdefghijklmnopqrstuvwx",
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.abc",
            "AKIAIOSFODNN7EXAMPLE",
        ] {
            let coberto = sem_casa(&format!("erro ao usar {chave} no pedido"));
            assert!(!coberto.contains(chave), "{chave} ficou em {coberto}");
            assert_eq!(coberto, format!("erro ao usar {COBERTO} no pedido"));
        }
    }

    #[test]
    fn valor_depois_de_nome_sensivel_e_coberto_ate_o_fim_da_linha() {
        assert_eq!(sem_casa("ANTHROPIC_API_KEY=abc123"), format!("ANTHROPIC_API_KEY={COBERTO}"));
        assert_eq!(sem_casa("senha: minha senha comprida"), format!("senha: {COBERTO}"));
        assert_eq!(sem_casa(r#"  "token": "abc","#), format!(r#"  "token": {COBERTO}"#));
        assert_eq!(sem_casa("Authorization: Bearer abc.def.ghi"), format!("Authorization: {COBERTO}"));
        assert_eq!(sem_casa("mandou Bearer abc.def e seguiu"), format!("mandou Bearer {COBERTO} e seguiu"));
        // A palavra solta, sem `:` nem `=`, não cobre a frase.
        assert_eq!(sem_casa("o token da Shopee venceu ontem"), "o token da Shopee venceu ontem");
        // Só a linha do segredo é coberta.
        assert_eq!(sem_casa("chave=abc\nlinha seguinte"), format!("chave={COBERTO}\nlinha seguinte"));
    }

    #[test]
    fn senha_na_url_e_token_opaco_sao_cobertos() {
        assert_eq!(sem_casa("git clone https://victor:s3nha@github.com/x/y.git"), format!("git clone https://{COBERTO}@github.com/x/y.git"));
        assert_eq!(sem_casa("GET https://api.x.com/v1?key=abc123&page=2"), format!("GET https://api.x.com/v1?key={COBERTO}&page=2"));
        let opaco = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert_eq!(sem_casa(&format!("hash {opaco}")), format!("hash {COBERTO}"));
        // O que não é segredo fica: versão, caminho comprido, número, frase.
        for fica in ["versão 0.9.6 · estável · a1b2c3d", "/usr/lib64/rustlib/src/rust/library/core/src/panicking.rs:75", "thread main panicked em src/ui/mod.rs:1589"] {
            assert_eq!(sem_casa(fica), fica);
        }
    }

    #[test]
    fn a_pasta_pessoal_vira_til() {
        let casa = PathBuf::from("/home/victor");
        assert_eq!(cobrir("em /home/victor/.local/share/vitral/queda.log", Some(&casa)), "em ~/.local/share/vitral/queda.log");
        // Uma casa vazia ou "/" não troca nada.
        assert_eq!(cobrir("/a/b", Some(Path::new("/"))), "/a/b");
    }

    #[test]
    fn so_o_fim_do_registro_entra() {
        let log: String = (1..=100).map(|n| format!("linha {n}\n")).collect();
        let fim = fim_do_log(&format!("{log}\n\n"), 40);
        assert_eq!(fim.lines().count(), 40);
        assert!(fim.starts_with("linha 61\n") && fim.ends_with("linha 100"));
        assert_eq!(fim_do_log("uma\nduas", 40), "uma\nduas");
        assert_eq!(fim_do_log("", 40), "");
    }

    #[test]
    fn o_diagnostico_tem_versoes_sistema_caminhos_e_o_fim_do_log_sem_segredo() {
        let entrada = Entrada {
            vitral: "0.9.6 · estável · a1b2c3d".into(),
            sistema: "Fedora Linux 44 · kernel 7.2.15 · wayland · GNOME".into(),
            claude: Some("2.1.328".into()),
            codex: None,
            dados: "/home/victor/.local/share/vitral".into(),
            config: "/home/victor/.config/vitral".into(),
            cache: "/home/victor/.cache/vitral".into(),
            chats: 42,
            opcoes: vec![("Memória da equipe".into(), "ligada".into()), ("Pasta de trabalho".into(), "/home/victor/eita".into())],
            quedas: Some("== 2026-11-20 10:00:00 · versão 0.9.5 · thread main\nfalhou com token=abc123\nem /home/victor/eita/vitral/src/x.rs:10\n".into()),
            casa: Some("/home/victor".into()),
        };
        let t = texto(&entrada);
        for tem in [
            "Vitral: 0.9.6 · estável · a1b2c3d",
            "Sistema: Fedora Linux 44",
            "Claude Code: 2.1.328",
            "Codex: não instalado",
            "Dados: ~/.local/share/vitral",
            "Configuração: ~/.config/vitral",
            "Cache: ~/.cache/vitral",
            "Chats: 42",
            "Memória da equipe: ligada",
            "Pasta de trabalho: ~/eita",
            "thread main",
            "em ~/eita/vitral/src/x.rs:10",
        ] {
            assert!(t.contains(tem), "falta {tem:?} em:\n{t}");
        }
        assert!(!t.contains("abc123") && !t.contains("/home/victor"), "{t}");
        // Sem registro de quedas, o texto diz isso em vez de sumir com a parte.
        let limpo = texto(&Entrada { quedas: None, ..entrada });
        assert!(limpo.contains("Registro de quedas: vazio"));
    }
}
