//! O comando que você roda na pasta do chat com "!" na caixa de mensagem, sem turno do agente (o
//! modo "!" do Claude Code no terminal). Ele vira um `Item::Comando` na conversa, com a saída
//! chegando enquanto roda; nada disso vai para a sessão do agente, e o custo e a hora do chat não
//! mudam. Roda num escopo de `processos` (o Esc e o fechar do Vitral derrubam tudo o que ele abriu).
//!
//! O comando com trecho arriscado entra na conversa parado, esperando você segurar o botão
//! (`pedir_comando_seu` com `confirmar`); quem decide o que é arriscado é a interface, com a mesma
//! lista dos pedidos de permissão.

use std::{
    io::Read as _,
    process::Stdio,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use super::*;
use crate::processos;

/// Quanto da saída fica guardado (o fim dela): um `cat` num arquivo enorme não vai inteiro para o
/// arquivo da conversa.
const MAX_DA_SAIDA: usize = 200_000;
/// De quanto em quanto tempo a saída que chegou vai para a conversa.
const PASSO: Duration = Duration::from_millis(120);
/// Depois do pedido de parar (SIGTERM em tudo), quanto esperar antes de matar de vez.
const FOLGA_PARA_PARAR: Duration = Duration::from_secs(2);
/// O `milis` do item de um comando que começou a rodar e ainda não acabou (0: nem começou). Se o
/// Vitral fechar no meio, é o que fica gravado.
pub const COMANDO_COMECOU: u64 = 1;
/// O que a saída cortada diz no começo.
pub const SAIDA_CORTADA: &str = "(o começo da saída foi cortado)";

/// O comando de um chat que ainda não acabou: o item dele na conversa e, rodando, o pedido de parar.
/// Sem o pedido de parar, ele espera a confirmação (trecho arriscado).
pub(super) struct ComandoSeu {
    item: String,
    parar: Option<Arc<AtomicBool>>,
}

/// Como está o comando de um item da conversa.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SituacaoDoComando {
    /// Tem trecho arriscado: espera você segurar o botão.
    EsperandoVoce,
    Rodando,
    /// Terminou, foi parado ou o Vitral fechou no meio (o item diz qual).
    Fim,
}

/// O que a thread do comando vai deixando para a conversa.
#[derive(Default)]
struct Andamento {
    saida: Vec<u8>,
    cortou: bool,
    /// O código de saída (`None`: não deu para saber) e quanto durou, quando acabou.
    fim: Option<(Option<i32>, u64)>,
    /// O comando nem abriu.
    falha: Option<String>,
}

impl Andamento {
    /// Junta um pedaço da saída e guarda só o fim, cortando numa quebra de linha.
    fn juntar(&mut self, pedaco: &[u8]) {
        self.saida.extend_from_slice(pedaco);
        if self.saida.len() > MAX_DA_SAIDA {
            let sobra = self.saida.len() - MAX_DA_SAIDA;
            let corte = self.saida[sobra..].iter().position(|b| *b == b'\n').map_or(sobra, |i| sobra + i + 1);
            self.saida.drain(..corte);
            self.cortou = true;
        }
    }

    fn texto(&self) -> String {
        let texto = texto_da_saida(&self.saida);
        if self.cortou { format!("{SAIDA_CORTADA}\n{texto}") } else { texto }
    }
}

impl Motor {
    /// Um comando seu roda agora neste chat (o que espera a confirmação não conta).
    pub fn comando_seu_rodando(&self, chat: &str) -> bool {
        self.comandos_seus.get(chat).is_some_and(|c| c.parar.is_some())
    }

    /// Como está o comando do item `item` do chat.
    pub fn situacao_do_comando(&self, chat: &str, item: &str) -> SituacaoDoComando {
        match self.comandos_seus.get(chat).filter(|c| c.item == item) {
            Some(ComandoSeu { parar: None, .. }) => SituacaoDoComando::EsperandoVoce,
            Some(_) => SituacaoDoComando::Rodando,
            None => SituacaoDoComando::Fim,
        }
    }

    /// Põe o comando na conversa e o roda na pasta do chat. Com `confirmar` (trecho arriscado), ele
    /// fica parado até o `confirmar_comando_seu`. Um por chat de cada vez.
    pub fn pedir_comando_seu(&mut self, chat: &str, comando: &str, confirmar: bool, cx: &mut Context<Self>) -> Result<(), String> {
        let comando = comando.trim();
        if self.somente_leitura.is_some() {
            return Err("o Vitral está aberto só para leitura".into());
        }
        if comando.is_empty() {
            return Err("faltou o comando depois do “!”".into());
        }
        if self.comandos_seus.contains_key(chat) {
            return Err("já há um comando seu neste chat: espere ele acabar ou pare com o Esc".into());
        }
        let Some(c) = self.chat_mut(chat) else {
            return Err("conversa não encontrada".into());
        };
        if !c.cwd.is_dir() {
            return Err(format!("a pasta {} não foi encontrada", c.cwd.display()));
        }
        let item = novo_id();
        c.itens.push(Item::Comando {
            id: item.clone(),
            comando: comando.to_string(),
            saida: String::new(),
            codigo: None,
            milis: 0,
            em: agora(),
        });
        self.chats_sujos.insert(chat.to_string());
        self.comandos_seus.insert(chat.to_string(), ComandoSeu { item, parar: None });
        if !confirmar {
            self.confirmar_comando_seu(chat, cx);
        }
        cx.notify();
        Ok(())
    }

    /// Roda o comando que esperava a confirmação.
    pub fn confirmar_comando_seu(&mut self, chat: &str, cx: &mut Context<Self>) {
        let Some(item) = self.comandos_seus.get(chat).filter(|c| c.parar.is_none()).map(|c| c.item.clone()) else {
            return;
        };
        let Some((comando, pasta)) = self.chat_mut(chat).and_then(|c| {
            let pasta = c.cwd.clone();
            c.itens.iter_mut().rev().find_map(|i| match i {
                Item::Comando { id, comando, milis, .. } if *id == item => {
                    *milis = COMANDO_COMECOU;
                    Some((comando.clone(), pasta.clone()))
                }
                _ => None,
            })
        }) else {
            self.comandos_seus.remove(chat);
            return;
        };
        let parar = Arc::new(AtomicBool::new(false));
        let andamento = Arc::new(Mutex::new(Andamento::default()));
        if let Some(c) = self.comandos_seus.get_mut(chat) {
            c.parar = Some(parar.clone());
        }
        let (da_thread, pedido) = (andamento.clone(), parar.clone());
        std::thread::spawn(move || {
            let comeco = Instant::now();
            let resultado = rodar(&comando, &pasta, &da_thread, &pedido);
            let mut a = da_thread.lock().unwrap_or_else(|e| e.into_inner());
            match resultado {
                Ok(codigo) => a.fim = Some((codigo, comeco.elapsed().as_millis() as u64)),
                Err(erro) => a.falha = Some(erro),
            }
        });
        let chat = chat.to_string();
        cx.spawn(async move |motor, cx| {
            let mut visto = (0usize, false);
            loop {
                cx.background_executor().timer(PASSO).await;
                let (texto, fim, falha) = {
                    let a = andamento.lock().unwrap_or_else(|e| e.into_inner());
                    let mudou = (a.saida.len(), a.cortou) != visto || a.fim.is_some() || a.falha.is_some();
                    visto = (a.saida.len(), a.cortou);
                    (mudou.then(|| a.texto()), a.fim, a.falha.clone())
                };
                let acabou = fim.is_some() || falha.is_some();
                let segue = motor.update(cx, |m, cx| m.andar_comando_seu(&chat, &item, texto, fim, falha, &parar, cx));
                if acabou || !segue.unwrap_or(false) {
                    // O item saiu da conversa (a mensagem de antes foi corrigida, o chat foi
                    // apagado) ou o motor fechou: o comando não tem mais onde aparecer.
                    parar.store(true, Ordering::Relaxed);
                    return;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Leva para o item o que o comando produziu desde a última passada e, no fim, o código de
    /// saída. Devolve `false` quando o item não está mais na conversa.
    #[allow(clippy::too_many_arguments)]
    fn andar_comando_seu(
        &mut self,
        chat: &str,
        item: &str,
        texto: Option<String>,
        fim: Option<(Option<i32>, u64)>,
        falha: Option<String>,
        parar: &AtomicBool,
        cx: &mut Context<Self>,
    ) -> bool {
        let parado = parar.load(Ordering::Relaxed);
        let acabou = fim.is_some() || falha.is_some();
        let achou = self.chat_mut(chat).and_then(|c| {
            c.itens.iter_mut().rev().find_map(|i| match i {
                Item::Comando { id, saida, codigo, milis, .. } if id == item => Some((saida, codigo, milis)),
                _ => None,
            })
        });
        let Some((saida, codigo, milis)) = achou else {
            self.comandos_seus.remove(chat);
            return false;
        };
        if let Some(texto) = texto {
            *saida = texto;
        }
        if let Some(erro) = falha {
            *saida = format!("Não abriu o comando: {erro}");
            (*codigo, *milis) = (Some(-1), COMANDO_COMECOU + 1);
        }
        if let Some((codigo_do_fim, durou)) = fim {
            // Parado por você, o código do sinal não diz nada: fica sem código ("Interrompido").
            (*codigo, *milis) = (codigo_do_fim.filter(|_| !parado), durou.max(COMANDO_COMECOU + 1));
        }
        if acabou {
            self.comandos_seus.remove(chat);
            if let Some(c) = self.chat(chat) {
                let _ = self.dados.salvar_chat(c);
            }
        } else {
            self.chats_sujos.insert(chat.to_string());
        }
        cx.notify();
        true
    }

    /// Para o comando seu que roda no chat (o Esc e o botão Parar); o que esperava a confirmação
    /// sai da conversa.
    pub fn parar_comando_seu(&mut self, chat: &str, cx: &mut Context<Self>) {
        match self.comandos_seus.get(chat) {
            Some(ComandoSeu { parar: Some(parar), .. }) => parar.store(true, Ordering::Relaxed),
            Some(ComandoSeu { item, parar: None }) => {
                let item = item.clone();
                self.comandos_seus.remove(chat);
                if let Some(c) = self.chat_mut(chat) {
                    c.itens.retain(|i| !matches!(i, Item::Comando { id, .. } if *id == item));
                }
                self.chats_sujos.insert(chat.to_string());
            }
            None => {}
        }
        cx.notify();
    }

    /// O chat saiu do Vitral: o comando dele para.
    pub(super) fn esquecer_comando_seu(&mut self, chat: &str) {
        if let Some(ComandoSeu { parar: Some(parar), .. }) = self.comandos_seus.remove(chat) {
            parar.store(true, Ordering::Relaxed);
        }
    }
}

/// O shell que roda o comando e o argumento que recebe o texto dele.
#[cfg(unix)]
const SHELL: (&str, &str) = ("bash", "-c");
#[cfg(windows)]
const SHELL: (&str, &str) = ("cmd", "/C");

/// Roda o comando até o fim (ou até o pedido de parar) e devolve o código de saída. A saída padrão
/// e a de erro vão juntas, na ordem em que saem, para o `andamento`.
fn rodar(comando: &str, pasta: &Path, andamento: &Arc<Mutex<Andamento>>, parar: &AtomicBool) -> Result<Option<i32>, String> {
    let (mut leitor, escritor) = std::io::pipe().map_err(|e| e.to_string())?;
    let mut processo = processos::comando(SHELL.0, "comando");
    processo
        .args([SHELL.1, comando])
        .current_dir(pasta)
        .stdin(Stdio::null())
        .stdout(escritor.try_clone().map_err(|e| e.to_string())?)
        .stderr(escritor);
    // Aberto de dentro de uma sessão do Claude Code, o que o comando abrir herdaria a marca dela.
    for variavel in ["CLAUDECODE", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_CODE_SSE_PORT"] {
        processo.env_remove(variavel);
    }
    // O `abrir` leva o `Command` junto: as pontas de escrita do pipe que ele guardava fecham, e a
    // leitura acaba quando o último processo com a saída aberta sair.
    let (mut filho, escopo) = processos::abrir(processo).map_err(|e| format!("{}: {e}", SHELL.0))?;
    let para_a_leitura = andamento.clone();
    let leitura = std::thread::spawn(move || {
        let mut pedaco = [0u8; 8192];
        loop {
            match leitor.read(&mut pedaco) {
                Ok(0) | Err(_) => return,
                Ok(n) => para_a_leitura.lock().unwrap_or_else(|e| e.into_inner()).juntar(&pedaco[..n]),
            }
        }
    });
    let pid = filho.id();
    let mut parado_em: Option<Instant> = None;
    // Sem colher (o `codigo_de_saida` só olha): o número do grupo continua dele até o `matar`.
    let codigo = loop {
        if let Some(codigo) = processos::codigo_de_saida(pid) {
            break Some(codigo);
        }
        if parar.load(Ordering::Relaxed) {
            match parado_em {
                None => {
                    escopo.parar();
                    parado_em = Some(Instant::now());
                }
                Some(quando) if quando.elapsed() > FOLGA_PARA_PARAR => break None,
                Some(_) => {}
            }
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    // O que o comando deixou no fundo sai com ele: senão a saída ficaria aberta para sempre.
    escopo.matar();
    let _ = filho.wait();
    let _ = leitura.join();
    Ok(codigo)
}

/// A saída como texto de mostrar: sem as sequências de cor do terminal, com as quebras do Windows
/// como quebra comum e, numa linha reescrita com `\r` (barra de progresso), só o que ficou nela.
pub fn texto_da_saida(bytes: &[u8]) -> String {
    let cru = String::from_utf8_lossy(bytes);
    let mut limpo = String::with_capacity(cru.len());
    let mut letras = cru.chars().peekable();
    while let Some(c) = letras.next() {
        match c {
            // ESC [ ... letra (cor, cursor) e ESC ] ... BEL (título da janela).
            '\u{1b}' => match letras.peek() {
                Some('[') => {
                    letras.next();
                    for d in letras.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&d) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    letras.next();
                    while let Some(d) = letras.next() {
                        if d == '\u{7}' || (d == '\u{1b}' && letras.next_if_eq(&'\\').is_some()) {
                            break;
                        }
                    }
                }
                _ => {
                    letras.next();
                }
            },
            '\r' if letras.peek() == Some(&'\n') => {}
            '\r' => {
                let comeco = limpo.rfind('\n').map_or(0, |i| i + 1);
                limpo.truncate(comeco);
            }
            _ => limpo.push(c),
        }
    }
    limpo.trim_end().to_string()
}

/// O trecho que o "Mandar ao agente" cita na próxima mensagem: o comando, como ele saiu e o fim da
/// saída (até `limite` letras, cortando numa linha inteira).
pub fn citacao_do_comando(comando: &str, saida: &str, codigo: Option<i32>, limite: usize) -> String {
    let como = match codigo {
        Some(0) => String::new(),
        Some(n) => format!(" (saiu com {n})"),
        None => " (interrompido)".to_string(),
    };
    let saida = saida.trim();
    let total = saida.chars().count();
    let corpo = if saida.is_empty() {
        "(sem saída)".to_string()
    } else if total <= limite {
        saida.to_string()
    } else {
        let fim: String = saida.chars().skip(total - limite).collect();
        let inteiro = fim.split_once('\n').map_or(fim.as_str(), |(_, resto)| resto);
        format!("(só o fim da saída)\n{inteiro}")
    };
    format!("$ {comando}{como}\n{corpo}")
}

#[cfg(test)]
mod testes {
    use super::*;

    #[cfg(unix)]
    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-comando-{nome}-{}", novo_id()));
        std::fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    #[test]
    fn saida_sem_cor_e_sem_a_linha_reescrita() {
        assert_eq!(texto_da_saida(b"\x1b[32mok\x1b[0m\r\nfim\n"), "ok\nfim");
        // A barra de progresso reescreve a linha: fica o último estado dela.
        assert_eq!(texto_da_saida(b"antes\n10%\r50%\r100%\ndepois"), "antes\n100%\ndepois");
        // O título da janela (OSC) some inteiro, com BEL ou com ESC \.
        assert_eq!(texto_da_saida(b"\x1b]0;titulo\x07a\x1b]0;x\x1b\\b"), "ab");
        // Bytes que não são UTF-8 não derrubam nada.
        assert_eq!(texto_da_saida(&[b'a', 0xff, b'b']), "a\u{fffd}b");
    }

    #[test]
    fn a_saida_grande_guarda_so_o_fim_em_linhas_inteiras() {
        let mut a = Andamento::default();
        let linha = "0123456789\n".repeat(1000);
        for _ in 0..30 {
            a.juntar(linha.as_bytes());
        }
        assert!(a.cortou);
        assert!(a.saida.len() <= MAX_DA_SAIDA);
        assert!(a.saida.starts_with(b"0123456789\n"), "corta numa quebra de linha");
        assert!(a.texto().starts_with(SAIDA_CORTADA));
    }

    #[test]
    fn a_citacao_leva_o_comando_o_codigo_e_o_fim_da_saida() {
        assert_eq!(citacao_do_comando("git status", "limpo\n", Some(0), 100), "$ git status\nlimpo");
        assert_eq!(citacao_do_comando("false", "", Some(1), 100), "$ false (saiu com 1)\n(sem saída)");
        assert_eq!(citacao_do_comando("sleep 9", "a", None, 100), "$ sleep 9 (interrompido)\na");
        let longa = "linha um\nlinha dois\nlinha tres";
        assert_eq!(citacao_do_comando("x", longa, Some(0), 14), "$ x\n(só o fim da saída)\nlinha tres");
    }

    #[cfg(unix)]
    #[test]
    fn roda_na_pasta_junta_as_duas_saidas_e_devolve_o_codigo() {
        let pasta = pasta_de_teste("roda");
        std::fs::write(pasta.join("aqui.txt"), "").unwrap();
        let andamento = Arc::new(Mutex::new(Andamento::default()));
        let parar = AtomicBool::new(false);
        let codigo = rodar("ls; echo erro >&2; exit 3", &pasta, &andamento, &parar).unwrap();
        assert_eq!(codigo, Some(3));
        assert_eq!(andamento.lock().unwrap().texto(), "aqui.txt\nerro");
        let _ = std::fs::remove_dir_all(&pasta);
    }

    #[cfg(unix)]
    #[test]
    fn o_pedido_de_parar_derruba_o_comando_longo() {
        let pasta = pasta_de_teste("parar");
        let andamento = Arc::new(Mutex::new(Andamento::default()));
        let parar = Arc::new(AtomicBool::new(false));
        let pedido = parar.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            pedido.store(true, Ordering::Relaxed);
        });
        let comeco = Instant::now();
        let _ = rodar("echo comecou; sleep 60", &pasta, &andamento, &parar).unwrap();
        assert!(comeco.elapsed() < Duration::from_secs(10), "{:?}", comeco.elapsed());
        assert_eq!(andamento.lock().unwrap().texto(), "comecou");
        let _ = std::fs::remove_dir_all(&pasta);
    }
}
