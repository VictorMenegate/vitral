//! Os agentes que falam o ACP (Agent Client Protocol), por um perfil de dados cada (26/11/2026;
//! dono: "GitHub Copilot CLI, Cursor Agent, OpenCode, Qwen Code, Kimi e Grok"). O protocolo é o
//! mesmo do Gemini (`agente_gemini::Protocolo`, que já falava ACP com o Antigravity CLI): o perfil
//! diz o binário, onde o instalador o põe, como abrir o servidor ACP, como instalar, entrar e
//! atualizar, e as diferenças pequenas (o prompt de sistema por `_meta` é só do agy; os outros o
//! recebem na frente do primeiro pedido).
//!
//! As fontes de cada comando estão em `~/.cache/vitral-trabalho/agentes-novos.md` (pesquisa de
//! 26/11/2026). O que não estava confirmado numa fonte segue o padrão do ACP.

use std::path::PathBuf;

use crate::{claude::Opcoes, modelo::Agente};

/// Tudo o que muda de um agente ACP para outro.
#[derive(Debug)]
pub struct PerfilAcp {
    pub agente: Agente,
    /// O nome do programa, sem o `.exe` ("copilot", "agent", "opencode").
    pub programa: &'static str,
    /// Outros nomes do mesmo programa (o `cursor-agent` antigo).
    pub outros_nomes: &'static [&'static str],
    /// Onde o instalador oficial o põe, além do PATH, a partir da pasta pessoal.
    pub pastas: &'static [&'static str],
    /// No Windows, a partir do `%LOCALAPPDATA%`.
    pub pastas_windows: &'static [&'static str],
    /// Os argumentos que abrem o servidor ACP.
    pub acp: &'static [&'static str],
    /// Os argumentos a mais no modo Sem perguntar (vazio: o Vitral aprova cada pedido sozinho).
    pub sem_perguntar: &'static [&'static str],
    /// O instalador oficial no Linux e no Mac (roda no `bash -lc` do terminal).
    pub instalar_unix: &'static str,
    /// O instalador oficial no Windows (PowerShell).
    pub instalar_windows: &'static str,
    /// O comando de entrar, depois de instalar ("copilot login"); no TUI (`login_no_tui`), o
    /// programa que se abre para o `/login` ou o `/auth`.
    pub login: &'static str,
    /// O login só existe dentro do programa (`/login` no Kimi, `/auth` no Qwen).
    pub login_no_tui: Option<&'static str>,
    /// Os argumentos do atualizar ("update", "upgrade"); vazio: reinstalar pelo instalador.
    pub atualizar: &'static [&'static str],
    /// A cor da marca (o disco com a inicial, para os que não têm logo no Vitral).
    pub cor: u32,
    /// Uma linha sobre o agente (a tela inicial e a lista das IAs).
    pub descricao: &'static str,
}

const COPILOT: PerfilAcp = PerfilAcp {
    agente: Agente::Copilot,
    programa: "copilot",
    outros_nomes: &[],
    pastas: &[".local/bin"],
    pastas_windows: &["Microsoft/WinGet/Links"],
    acp: &["--acp"],
    sem_perguntar: &["--allow-all-tools"],
    instalar_unix: "curl -fsSL https://gh.io/copilot-install | bash",
    instalar_windows: "winget install GitHub.Copilot",
    login: "copilot login",
    login_no_tui: None,
    atualizar: &["update"],
    cor: 0x8957e5,
    descricao: "Do GitHub, com a assinatura do Copilot",
};

const CURSOR: PerfilAcp = PerfilAcp {
    agente: Agente::Cursor,
    programa: "agent",
    outros_nomes: &["cursor-agent"],
    pastas: &[".local/bin"],
    pastas_windows: &["cursor-agent"],
    acp: &["acp"],
    sem_perguntar: &[],
    instalar_unix: "curl https://cursor.com/install -fsS | bash",
    instalar_windows: "irm 'https://cursor.com/install?win32=true' | iex",
    login: "agent login",
    login_no_tui: None,
    atualizar: &["update"],
    cor: 0xd6dae6,
    descricao: "Do Cursor, com a conta do Cursor",
};

const OPENCODE: PerfilAcp = PerfilAcp {
    agente: Agente::OpenCode,
    programa: "opencode",
    outros_nomes: &[],
    pastas: &[".opencode/bin", ".local/bin"],
    pastas_windows: &[],
    acp: &["acp"],
    sem_perguntar: &[],
    instalar_unix: "curl -fsSL https://opencode.ai/v2/install | bash",
    instalar_windows: "npm install -g @opencode/cli",
    login: "opencode auth login",
    login_no_tui: None,
    atualizar: &["upgrade"],
    cor: 0xf59e0b,
    descricao: "Código aberto, com o provedor que você escolher",
};

const QWEN: PerfilAcp = PerfilAcp {
    agente: Agente::Qwen,
    programa: "qwen",
    outros_nomes: &[],
    pastas: &[".local/bin"],
    pastas_windows: &[],
    acp: &["--acp"],
    sem_perguntar: &[],
    instalar_unix: "curl -fsSL https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.sh | bash",
    instalar_windows: "irm https://qwen-code-assets.oss-cn-hangzhou.aliyuncs.com/installation/install-qwen-standalone.ps1 | iex",
    login: "qwen",
    login_no_tui: Some("/auth"),
    atualizar: &[],
    cor: 0x615ced,
    descricao: "Da Alibaba, com a conta Qwen ou uma chave",
};

const KIMI: PerfilAcp = PerfilAcp {
    agente: Agente::Kimi,
    programa: "kimi",
    outros_nomes: &[],
    pastas: &[".local/bin", ".kimi-code/bin"],
    pastas_windows: &[],
    acp: &["acp"],
    sem_perguntar: &[],
    instalar_unix: "curl -fsSL https://code.kimi.com/kimi-code/install.sh | bash",
    instalar_windows: "irm https://code.kimi.com/kimi-code/install.ps1 | iex",
    login: "kimi",
    login_no_tui: Some("/login"),
    atualizar: &["upgrade"],
    cor: 0x2f80ed,
    descricao: "Da Moonshot, com a conta Kimi ou uma chave",
};

const GROK: PerfilAcp = PerfilAcp {
    agente: Agente::Grok,
    programa: "grok",
    outros_nomes: &[],
    pastas: &[".grok/bin", ".local/bin"],
    pastas_windows: &[],
    acp: &["agent", "stdio"],
    sem_perguntar: &[],
    instalar_unix: "curl -fsSL https://x.ai/cli/install.sh | bash",
    instalar_windows: "irm https://x.ai/cli/install.ps1 | iex",
    login: "grok login",
    login_no_tui: None,
    atualizar: &["update"],
    cor: 0xa1a1aa,
    descricao: "Da xAI, com a conta da xAI",
};

/// O perfil de um agente novo (o Gemini e os de antes têm o caminho próprio).
pub fn perfil(agente: Agente) -> Option<&'static PerfilAcp> {
    Some(match agente {
        Agente::Copilot => &COPILOT,
        Agente::Cursor => &CURSOR,
        Agente::OpenCode => &OPENCODE,
        Agente::Qwen => &QWEN,
        Agente::Kimi => &KIMI,
        Agente::Grok => &GROK,
        Agente::Claude | Agente::Codex | Agente::Gemini => return None,
    })
}

impl PerfilAcp {
    /// O binário do PATH ou o das pastas do instalador (o Vitral aberto pela dock nem sempre tem o
    /// PATH que o instalador mudou).
    pub fn binario(&self) -> PathBuf {
        let casa = dirs::home_dir().unwrap_or_default();
        let mut pastas: Vec<PathBuf> = self.pastas.iter().map(|p| casa.join(p)).collect();
        if cfg!(windows)
            && let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
        {
            pastas.extend(self.pastas_windows.iter().map(|p| local.join(p)));
        }
        let nomes: Vec<String> = std::iter::once(self.programa)
            .chain(self.outros_nomes.iter().copied())
            .map(|n| if cfg!(windows) { format!("{n}.exe") } else { n.to_string() })
            .collect();
        // Pula os atalhos de outros programas com o mesmo nome: o `copilot` que o VS Code põe em
        // /usr/bin só diz "GitHub Copilot CLI was not found" (conferido em 26/11/2026).
        let do_path: Vec<PathBuf> = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
        do_path
            .iter()
            .chain(&pastas)
            .flat_map(|pasta| nomes.iter().map(move |nome| pasta.join(nome)))
            .find(|caminho| caminho.is_file() && !e_atalho_de_outro(caminho))
            .unwrap_or_else(|| PathBuf::from(&nomes[0]))
    }

    pub fn instalado(&self) -> bool {
        self.binario().is_file()
    }

    /// Os argumentos do processo: o servidor ACP e, no Sem perguntar, o que o agente tiver para isso.
    pub fn argumentos(&self, opcoes: &Opcoes) -> Vec<String> {
        let mut args: Vec<String> = self.acp.iter().map(|a| a.to_string()).collect();
        if opcoes.permissao == "bypassPermissions" {
            args.extend(self.sem_perguntar.iter().map(|a| a.to_string()));
        }
        args
    }

    /// O que vai ao terminal para instalar (se ainda não houver o programa) e entrar.
    pub fn script_de_instalar(&self) -> String {
        let nome = self.agente.nome();
        if cfg!(windows) {
            let entrar = match self.login_no_tui {
                Some(comando) => format!("Write-Host 'Para entrar, digite {comando} no {nome} que abre agora.'; {}", self.login),
                None => self.login.to_string(),
            };
            format!(
                "if (-not (Get-Command {programa} -ErrorAction SilentlyContinue)) {{ Write-Host 'Instalando o {nome} pelo instalador oficial...'; \
                 try {{ {instalar} }} catch {{ Write-Host \"Não deu para instalar o {nome}: $_\" }} }}; \
                 if (Get-Command {programa} -ErrorAction SilentlyContinue) {{ {entrar} }} else {{ Write-Host 'O {nome} não foi encontrado depois da instalação; feche e abra o terminal.' }}",
                programa = self.programa,
                instalar = self.instalar_windows,
            )
        } else {
            let pastas: Vec<String> = self.pastas.iter().map(|p| format!("$HOME/{p}")).collect();
            let entrar = match self.login_no_tui {
                Some(comando) => format!("echo 'Para entrar, digite {comando} no {nome} que abre agora.'; {}", self.login),
                None => self.login.to_string(),
            };
            format!(
                "set -o pipefail; export PATH=\"{pastas}:$PATH\"; \
                 if ! command -v {programa} >/dev/null; then echo 'Instalando o {nome} pelo instalador oficial...'; \
                 {instalar} || echo 'Não deu para instalar o {nome} (veja o erro acima).'; fi; \
                 if command -v {programa} >/dev/null; then {entrar}; else echo 'O {programa} não foi encontrado no PATH depois da instalação.'; fi",
                pastas = pastas.join(":"),
                programa = self.programa,
                instalar = self.instalar_unix,
            )
        }
    }

    /// O que vai ao terminal para atualizar: o comando do próprio agente, ou o instalador de novo.
    pub fn script_de_atualizar(&self) -> String {
        if self.atualizar.is_empty() {
            return if cfg!(windows) { self.instalar_windows.to_string() } else { self.instalar_unix.to_string() };
        }
        let binario = self.binario();
        let args = self.atualizar.join(" ");
        if cfg!(windows) { format!("& '{}' {args}", binario.display()) } else { format!("'{}' {args}", binario.display()) }
    }

    /// Abre o terminal do usuário com o instalador (se faltar) e o login.
    pub fn instalar(&self) {
        abrir_no_terminal(&self.script_de_instalar());
    }

    /// Abre o terminal com o atualizar (alguns perguntam antes, como o `kimi upgrade`).
    pub fn atualizar_no_terminal(&self) {
        abrir_no_terminal(&self.script_de_atualizar());
    }

    /// Abre o terminal com o login.
    pub fn entrar(&self) {
        let binario = self.binario();
        let comando = match self.login_no_tui {
            Some(dentro) => format!("echo 'Digite {dentro} para entrar.'; '{}'", binario.display()),
            None => {
                let resto = self.login.split_once(' ').map_or("", |(_, r)| r);
                format!("'{}' {resto}", binario.display())
            }
        };
        abrir_no_terminal(&comando);
    }
}

/// O caminho é um atalho de outro programa (o `*-shim` do VS Code), e não o agente.
fn e_atalho_de_outro(caminho: &std::path::Path) -> bool {
    dunce::canonicalize(caminho).unwrap_or_else(|_| caminho.to_path_buf()).to_string_lossy().contains("-shim")
}

fn abrir_no_terminal(comando: &str) {
    let Some(casa) = dirs::home_dir() else {
        return;
    };
    if cfg!(windows) {
        crate::sistema::abrir_powershell(&casa, comando);
    } else {
        crate::sistema::abrir_terminal(&casa, Some(comando));
    }
}

#[cfg(test)]
mod testes {
    use super::perfil;
    use crate::modelo::Agente;

    /// Cada agente novo tem perfil, e os de antes seguem pelo caminho próprio.
    #[test]
    fn os_seis_novos_tem_perfil() {
        for agente in [Agente::Copilot, Agente::Cursor, Agente::OpenCode, Agente::Qwen, Agente::Kimi, Agente::Grok] {
            let p = perfil(agente).expect("perfil");
            assert_eq!(p.agente, agente);
            assert!(!p.acp.is_empty() && !p.instalar_unix.is_empty() && !p.instalar_windows.is_empty());
        }
        assert!(perfil(Agente::Claude).is_none() && perfil(Agente::Codex).is_none() && perfil(Agente::Gemini).is_none());
    }

    /// O script de instalar só instala se faltar o programa e depois entra; o do Kimi avisa do
    /// `/login`, que é dentro dele.
    #[cfg(unix)]
    #[test]
    fn o_instalar_confere_antes_e_entra_depois() {
        let kimi = perfil(Agente::Kimi).unwrap().script_de_instalar();
        assert!(kimi.contains("if ! command -v kimi"));
        assert!(kimi.contains("code.kimi.com/kimi-code/install.sh"));
        assert!(kimi.contains("/login"));
        let grok = perfil(Agente::Grok).unwrap().script_de_instalar();
        assert!(grok.contains("$HOME/.grok/bin") && grok.ends_with("fi"));
    }
}
