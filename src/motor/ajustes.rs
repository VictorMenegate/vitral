//! O que as Configurações pedem ao motor além de gravar uma opção: mandar a notificação de teste.

use super::*;

/// O título e o corpo da notificação de teste.
pub const TITULO_DO_TESTE: &str = "Vitral";
pub const CORPO_DO_TESTE: &str = "Se você está vendo isto, as notificações do sistema funcionam.";

/// Os argumentos do `notify-send` da notificação de teste: os mesmos da de verdade (o nome, o
/// ícone e a entrada do Vitral), sem a ação nem a espera pelo clique, e sem urgência.
#[cfg_attr(windows, allow(dead_code))]
pub fn argumentos_do_teste() -> Vec<String> {
    ["-a", "Vitral", "-i", "vitral", "-h", "string:desktop-entry:vitral", TITULO_DO_TESTE, CORPO_DO_TESTE].map(String::from).to_vec()
}

impl Motor {
    /// Manda agora uma notificação do sistema, com a janela na frente ou não, para conferir que
    /// elas chegam. O erro é o que mostrar na linha das Configurações.
    pub fn testar_notificacao(&self) -> Result<(), String> {
        #[cfg(windows)]
        {
            let aviso = gpui_kit::SystemNotification {
                tag: "vitral-teste".into(),
                title: TITULO_DO_TESTE.into(),
                body: CORPO_DO_TESTE.into(),
                actions: Vec::new(),
            };
            self.sinais.try_send(Sinal::Aviso(aviso)).map_err(|_| "Não consegui mandar a notificação".to_string())
        }
        #[cfg(not(windows))]
        {
            crate::processos::novo_comando("notify-send")
                .args(argumentos_do_teste())
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .map(|mut filho| {
                    // Sem a ação, ele sai logo: a thread só recolhe o processo.
                    std::thread::spawn(move || {
                        let _ = filho.wait();
                    });
                })
                .map_err(|erro| match erro.kind() {
                    std::io::ErrorKind::NotFound => "Não achei o notify-send neste computador (pacote libnotify)".to_string(),
                    _ => format!("Não consegui mandar a notificação: {erro}"),
                })
        }
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    #[test]
    fn a_notificacao_de_teste_sai_como_as_do_vitral_sem_esperar_clique() {
        let args = argumentos_do_teste();
        // O nome, o ícone e a entrada do app, como nas de verdade: é o que o sistema usa para
        // decidir se mostra (as notificações do Vitral desligadas calam o teste também).
        let de_verdade = argumentos_do_aviso("t", "c", None, false);
        for par in [["-a", "Vitral"], ["-i", "vitral"], ["-h", "string:desktop-entry:vitral"]] {
            assert!(args.windows(2).any(|w| w == par), "{par:?}");
            assert!(de_verdade.windows(2).any(|w| w == par), "{par:?}");
        }
        // Sem ação e sem `-p`, o notify-send não fica esperando; sem urgência, some sozinha.
        assert!(!args.iter().any(|a| a == "-A" || a == "-p" || a == "-u"));
        assert_eq!(&args[args.len() - 2..], ["Vitral", "Se você está vendo isto, as notificações do sistema funcionam."]);
    }
}
