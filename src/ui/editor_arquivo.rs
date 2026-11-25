//! O editor do painel de arquivo: o texto que você escreve por cima de um arquivo da pasta. A
//! `Sessao` é a conta com o disco (o que foi lido ou gravado por último, o fim de linha, o
//! conflito quando o agente grava o mesmo arquivo) e a `Agenda` é a gravação sozinha, 800 ms
//! depois da última tecla. As duas não sabem de tela: o painel (`painel_arquivo.rs`) liga isto ao
//! editor de código do kit.

use gpui_kit::component::{
    WindowExt as _,
    input::{EditorState, InputEvent, Position, RopeExt as _, TabSize},
};
use gpui_kit::*;

use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime},
};

use super::{
    Janela,
    componentes::{ChaveAviso as _, Tom, aviso},
};

/// Quanto tempo depois da última tecla o texto é gravado sozinho.
pub(super) const SALVAR_SOZINHO: Duration = Duration::from_millis(800);

/// De quanto em quanto tempo o editor confere se o arquivo mudou no disco.
pub(super) const CONFERIR_DISCO: Duration = Duration::from_secs(1);

/// Maior arquivo que se edita aqui; acima disso ele abre só para ler.
pub(super) const LIMITE_DE_EDICAO: u64 = 400_000;

/// Por que o arquivo não entra no editor.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum NaoEdita {
    Grande,
    Binario,
    /// Não é UTF-8 (um Latin-1 antigo): gravar daqui trocaria as letras dele.
    OutraCodificacao,
    Erro(String),
}

impl NaoEdita {
    /// O que o aviso diz quando o clique não abre o editor.
    pub(super) fn frase(&self) -> String {
        match self {
            NaoEdita::Grande => "Arquivo grande: aqui ele abre só para ler.".into(),
            NaoEdita::Binario => "Este arquivo não é texto.".into(),
            NaoEdita::OutraCodificacao => "Este arquivo não está em UTF-8: editar aqui trocaria as letras dele.".into(),
            NaoEdita::Erro(erro) => format!("Não deu para abrir o arquivo para editar: {erro}"),
        }
    }
}

/// O arquivo mudou por fora com texto seu ainda não gravado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Conflito {
    /// Outro (o agente) gravou o arquivo.
    Mudou,
    /// O arquivo saiu do disco.
    Apagado,
}

/// O que a conferência do disco achou.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Disco {
    /// Nada a fazer: o disco está como o editor o conhece (ou igual ao que você escreveu).
    Igual,
    /// O arquivo mudou e você não tinha nada por gravar: o editor passa a mostrar este texto.
    Recarregar(String),
    Conflito(Conflito),
}

/// No que deu o pedido de gravar.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Gravacao {
    Gravou,
    /// Não havia o que gravar.
    Nada,
    /// Não gravou: o disco tem coisa que você não viu.
    Conflito(Conflito),
    Erro(String),
}

/// A conta do editor com o arquivo no disco.
pub(super) struct Sessao {
    caminho: PathBuf,
    /// O texto como o editor leu ou gravou por último, com "\n".
    gravado: String,
    /// O arquivo veio com "\r\n": o editor trabalha com "\n" e a gravação devolve.
    crlf: bool,
    /// A data e o tamanho do arquivo quando foi lido ou gravado por último; `None`, ele não está
    /// no disco.
    marca: Option<(SystemTime, u64)>,
    conflito: Option<Conflito>,
}

/// A data e o tamanho do arquivo, para notar que ele mudou sem ler; `None`, não está no disco.
fn marca_de(caminho: &Path) -> Option<(SystemTime, u64)> {
    let dados = fs::metadata(caminho).ok()?;
    Some((dados.modified().ok()?, dados.len()))
}

/// O texto com "\n" e se ele veio com "\r\n".
fn com_quebra_simples(texto: String) -> (String, bool) {
    if texto.contains("\r\n") { (texto.replace("\r\n", "\n"), true) } else { (texto, false) }
}

/// O que o disco tem agora, em texto.
enum Lido {
    Texto(String, bool),
    Sumiu,
    /// Não deu para ler agora (sem permissão, virou binário): fica para a conferência seguinte.
    Falhou(String),
}

fn ler(caminho: &Path) -> Lido {
    match fs::read(caminho) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(texto) => {
                let (texto, crlf) = com_quebra_simples(texto);
                Lido::Texto(texto, crlf)
            }
            Err(_) => Lido::Falhou("O arquivo deixou de ser texto em UTF-8.".into()),
        },
        Err(erro) if erro.kind() == std::io::ErrorKind::NotFound => Lido::Sumiu,
        Err(erro) => Lido::Falhou(erro.to_string()),
    }
}

impl Sessao {
    /// Lê o arquivo para o editor: a sessão e o texto, com "\n".
    pub(super) fn abrir(caminho: &Path) -> Result<(Sessao, String), NaoEdita> {
        let dados = fs::metadata(caminho).map_err(|erro| NaoEdita::Erro(erro.to_string()))?;
        if dados.is_dir() {
            return Err(NaoEdita::Erro("É uma pasta, não um arquivo.".into()));
        }
        if dados.len() > LIMITE_DE_EDICAO {
            return Err(NaoEdita::Grande);
        }
        // A marca é a de antes da leitura: se o arquivo mudar no meio, a conferência seguinte nota.
        let marca = marca_de(caminho);
        let bytes = fs::read(caminho).map_err(|erro| NaoEdita::Erro(erro.to_string()))?;
        if bytes.len() as u64 > LIMITE_DE_EDICAO {
            return Err(NaoEdita::Grande);
        }
        if bytes.contains(&0) {
            return Err(NaoEdita::Binario);
        }
        let texto = String::from_utf8(bytes).map_err(|_| NaoEdita::OutraCodificacao)?;
        let (texto, crlf) = com_quebra_simples(texto);
        let sessao = Sessao { caminho: caminho.to_path_buf(), gravado: texto.clone(), crlf, marca, conflito: None };
        Ok((sessao, texto))
    }

    pub(super) fn caminho(&self) -> &Path {
        &self.caminho
    }

    pub(super) fn conflito(&self) -> Option<Conflito> {
        self.conflito
    }

    /// O texto do editor é outro que não o lido ou gravado por último: há coisa escrita por você.
    pub(super) fn escrito(&self, atual: &str) -> bool {
        atual != self.gravado
    }

    /// Há texto seu que o disco não tem (ou o arquivo saiu do disco e você escolheu mantê-lo).
    pub(super) fn sujo(&self, atual: &str) -> bool {
        atual != self.gravado || self.marca.is_none()
    }

    /// Confere o disco (a data e o tamanho; o conteúdo só quando eles mudam). `atual` é o texto do
    /// editor agora.
    pub(super) fn conferir(&mut self, atual: &str) -> Disco {
        let agora = marca_de(&self.caminho);
        if agora == self.marca {
            return Disco::Igual;
        }
        match ler(&self.caminho) {
            Lido::Sumiu => {
                self.marca = None;
                self.conflito = Some(Conflito::Apagado);
                Disco::Conflito(Conflito::Apagado)
            }
            Lido::Falhou(_) => Disco::Igual,
            Lido::Texto(disco, crlf) => {
                self.marca = agora;
                if disco == self.gravado {
                    // Só a data mudou, ou o arquivo voltou a ser o que o editor conhecia.
                    self.conflito = None;
                    Disco::Igual
                } else if disco == atual {
                    // O disco ficou igual ao que você escreveu: não há o que escolher.
                    self.gravado = disco;
                    self.crlf = crlf;
                    self.conflito = None;
                    Disco::Igual
                } else if atual == self.gravado {
                    self.gravado = disco.clone();
                    self.crlf = crlf;
                    self.conflito = None;
                    Disco::Recarregar(disco)
                } else {
                    self.conflito = Some(Conflito::Mudou);
                    Disco::Conflito(Conflito::Mudou)
                }
            }
        }
    }

    /// Grava o texto, a não ser que o disco tenha mudado sem você ver: aí vira conflito, e quem
    /// decide é você (`recarregar` ou `manter_o_meu`).
    pub(super) fn salvar(&mut self, atual: &str) -> Gravacao {
        if let Some(conflito) = self.conflito {
            return Gravacao::Conflito(conflito);
        }
        if !self.sujo(atual) {
            return Gravacao::Nada;
        }
        // O agente pode ter gravado entre duas conferências.
        match self.conferir(atual) {
            Disco::Conflito(conflito) => return Gravacao::Conflito(conflito),
            Disco::Igual if !self.sujo(atual) => return Gravacao::Nada,
            Disco::Igual | Disco::Recarregar(_) => {}
        }
        let resultado = if self.crlf { gravar_por_inteiro(&self.caminho, atual.replace('\n', "\r\n").as_bytes()) } else { gravar_por_inteiro(&self.caminho, atual.as_bytes()) };
        match resultado {
            Ok(()) => {
                self.gravado = atual.to_string();
                self.marca = marca_de(&self.caminho);
                Gravacao::Gravou
            }
            Err(erro) => Gravacao::Erro(erro.to_string()),
        }
    }

    /// "Recarregar": o editor fica com o que está no disco, e o que você escreveu se perde.
    pub(super) fn recarregar(&mut self) -> Result<String, String> {
        let marca = marca_de(&self.caminho);
        match ler(&self.caminho) {
            Lido::Texto(disco, crlf) => {
                self.gravado = disco.clone();
                self.crlf = crlf;
                self.marca = marca;
                self.conflito = None;
                Ok(disco)
            }
            Lido::Sumiu => Err("O arquivo não existe mais.".into()),
            Lido::Falhou(erro) => Err(erro),
        }
    }

    /// "Manter o meu": o seu texto fica, e a próxima gravação passa por cima do que está no disco
    /// (ou cria o arquivo de novo, se ele foi apagado).
    pub(super) fn manter_o_meu(&mut self) {
        self.conflito = None;
    }
}

/// Grava o arquivo inteiro sem nunca deixá-lo pela metade. O `fs::write` direto trunca o
/// original antes de escrever: disco cheio, cota ou uma queda no meio deixavam só um pedaço do
/// arquivo (conferido com o limite de tamanho de arquivo do sistema, 25/11/2026). Aqui o texto
/// vai para um arquivo novo na mesma pasta, já criado com as permissões do original (o segredo de
/// um `.env` 0600 não fica legível no meio), e só depois de escrito e sincronizado toma o lugar
/// dele (o `rename` na mesma pasta é atômico); com erro, o original fica como estava. O link
/// simbólico continua link: grava-se no arquivo a que ele aponta, mesmo que esse arquivo não
/// exista mais.
///
/// Grava no lugar, como antes, quando trocar o arquivo mudaria o que ele é: o somente leitura (a
/// gravação falha, como falhava, em vez de passar por cima), o de outro dono ou de outro grupo (o
/// novo seria seu), o que tem mais de um nome (link físico; os outros nomes não veriam o texto
/// novo), a pasta onde não se pode criar arquivo e o arquivo que não deixa ser trocado (no
/// Windows, aberto por outro programa sem permitir).
fn gravar_por_inteiro(caminho: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::io::Write as _;
    let destino = destino_do_link(caminho);
    let original = fs::metadata(&destino).ok();
    if original.as_ref().is_some_and(|dados| dados.permissions().readonly()) {
        return fs::write(&destino, bytes);
    }
    #[cfg(unix)]
    if original.as_ref().is_some_and(|dados| std::os::unix::fs::MetadataExt::nlink(dados) > 1) {
        return fs::write(&destino, bytes);
    }
    let pasta = destino.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(Path::new("."));
    let temporario = pasta.join(format!(".{}.vitral-{}", nome_do_arquivo(&destino), crate::modelo::novo_id()));
    let mut abrir = fs::OpenOptions::new();
    abrir.write(true).create_new(true);
    #[cfg(unix)]
    if let Some(dados) = &original {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        abrir.mode(dados.permissions().mode() & 0o777);
    }
    let mut arquivo = match abrir.open(&temporario) {
        Ok(arquivo) => arquivo,
        Err(erro) if erro.kind() == std::io::ErrorKind::PermissionDenied => return fs::write(&destino, bytes),
        Err(erro) => return Err(erro),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        let dono = |dados: &fs::Metadata| (dados.uid(), dados.gid());
        let outro_dono = match (&original, arquivo.metadata()) {
            (Some(original), Ok(novo)) => dono(original) != dono(&novo),
            _ => false,
        };
        if outro_dono {
            drop(arquivo);
            let _ = fs::remove_file(&temporario);
            return fs::write(&destino, bytes);
        }
    }
    let escrito = arquivo.write_all(bytes).and_then(|()| match &original {
        // De novo depois de escrever: a criação passa pelo umask, e o modo tem de ser o do original.
        Some(dados) => arquivo.set_permissions(dados.permissions()),
        None => Ok(()),
    });
    let escrito = escrito.and_then(|()| arquivo.sync_all());
    drop(arquivo);
    match escrito.and_then(|()| fs::rename(&temporario, &destino)) {
        Ok(()) => Ok(()),
        Err(erro) => {
            let _ = fs::remove_file(&temporario);
            if erro.kind() == std::io::ErrorKind::PermissionDenied { fs::write(&destino, bytes) } else { Err(erro) }
        }
    }
}

/// O arquivo a que o caminho leva, seguindo os links simbólicos um a um (o alvo relativo conta da
/// pasta do link). Diferente do `canonicalize`, não precisa que o alvo exista: o link cujo
/// arquivo foi apagado continua link, e a gravação cria o arquivo de novo no lugar dele.
fn destino_do_link(caminho: &Path) -> PathBuf {
    let mut atual = caminho.to_path_buf();
    // Um laço de links (a → b → a) para aqui; o `rename` então troca o próprio link.
    for _ in 0..40 {
        let Ok(dados) = fs::symlink_metadata(&atual) else { break };
        if !dados.file_type().is_symlink() {
            break;
        }
        let Ok(alvo) = fs::read_link(&atual) else { break };
        atual = match atual.parent() {
            Some(pasta) if alvo.is_relative() => pasta.join(alvo),
            _ => alvo,
        };
    }
    atual
}

/// A gravação sozinha: vence `SALVAR_SOZINHO` depois da última tecla.
#[derive(Default)]
pub(super) struct Agenda {
    prazo: Option<Instant>,
}

impl Agenda {
    pub(super) fn editou(&mut self, agora: Instant) {
        self.prazo = Some(agora + SALVAR_SOZINHO);
    }

    /// Quanto falta para gravar (zero: é agora); `None`, não há gravação marcada.
    pub(super) fn falta(&self, agora: Instant) -> Option<Duration> {
        self.prazo.map(|prazo| prazo.saturating_duration_since(agora))
    }

    pub(super) fn limpar(&mut self) {
        self.prazo = None;
    }
}

/// O recuo que o arquivo usa: de quantos espaços e se é com tabulação. Sem recuo nenhum, 4 espaços.
pub(super) fn recuo_do_texto(texto: &str) -> (usize, bool) {
    let (mut com_tab, mut menor) = (0usize, usize::MAX);
    // As primeiras linhas bastam para saber, e o arquivo pode ter centenas de KB.
    for linha in texto.lines().take(2000) {
        if linha.starts_with('\t') {
            com_tab += 1;
            continue;
        }
        let espacos = linha.len() - linha.trim_start_matches(' ').len();
        // Um espaço só é continuação de comentário (" * nota"), não recuo; a linha só de espaços não conta.
        if espacos >= 2 && espacos < linha.len() {
            menor = menor.min(espacos);
        }
    }
    match (com_tab, menor) {
        (tab, usize::MAX) if tab > 0 => (4, true),
        (_, usize::MAX) => (4, false),
        (_, n) => (n.min(8), false),
    }
}

/// Onde cai, na linha de verdade, o clique dado na coluna `coluna` da linha desenhada (em que a
/// tabulação vale 4 espaços): a posição em letras, como o editor do kit conta a coluna
/// (`RopeExt::position_to_offset`: "the column is in characters").
pub(super) fn coluna_no_texto(linha: &str, coluna: usize) -> u32 {
    let (mut na_tela, mut no_texto) = (0usize, 0u32);
    for letra in linha.chars() {
        let largura: usize = if letra == '\t' { 4 } else { 1 };
        // Da metade da letra em diante, o cursor vai para depois dela.
        if coluna < na_tela + largura.div_ceil(2) {
            break;
        }
        na_tela += largura;
        no_texto += 1;
    }
    no_texto
}

/// O editor aberto no painel do arquivo: o campo de código do kit e a conta dele com o disco.
pub(super) struct Editor {
    pub(super) sessao: Sessao,
    pub(super) estado: Entity<EditorState>,
    agenda: Agenda,
    /// Há tecla desde a última gravação (o ponto ao lado de "editando").
    pub(super) por_salvar: bool,
    /// A última gravação não deu certo (disco cheio, sem permissão): o texto continua aqui.
    pub(super) erro: Option<String>,
    /// A tarefa que espera o prazo da `Agenda` está rodando.
    esperando: bool,
    /// A faixa do conflito já estava na tela no quadro anterior (ela entra uma vez por conflito).
    faixa_vista: std::cell::Cell<bool>,
    /// A cópia em `nao_salvos` já guardada ao fechar, com o texto dela: o mesmo texto não vira
    /// outra cópia no segundo passo do fechar.
    copia: Option<(String, PathBuf)>,
    _gravar: Option<Task<()>>,
    _mudou: Subscription,
    _vigia: Task<()>,
}

impl Editor {
    /// O conflito para a faixa do painel e se ela acabou de aparecer.
    pub(super) fn conflito_para_a_faixa(&self) -> Option<(Conflito, bool)> {
        match self.sessao.conflito() {
            Some(conflito) => Some((conflito, !self.faixa_vista.replace(true))),
            None => {
                self.faixa_vista.set(false);
                None
            }
        }
    }
}

impl Janela {
    /// O editor está aberto sobre este arquivo.
    pub(super) fn editando(&self, completo: &Path) -> bool {
        self.editor_arquivo.as_ref().is_some_and(|e| e.sessao.caminho() == completo)
    }

    /// Um clique no texto do arquivo aberto: o editor entra no lugar da leitura, com o cursor na
    /// linha (a partir de 0) e na coluna do clique, e a mesma rolagem. O que não dá para editar
    /// aqui (grande, binário, fora do UTF-8) avisa e fica como está.
    pub(super) fn entrar_no_editor(&mut self, linha: usize, coluna: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some((raiz, caminho)) = self.arquivo_aberto.clone() else { return };
        let completo = raiz.join(&caminho);
        if self.editando(&completo) {
            return;
        }
        self.largar_editor(window, cx);
        let (sessao, texto) = match Sessao::abrir(&completo) {
            Ok(aberto) => aberto,
            Err(motivo) => {
                window.push_notification(aviso(Tom::Atencao, motivo.frase()).chave("editar-arquivo"), cx);
                return;
            }
        };
        let (tab_size, hard_tabs) = recuo_do_texto(&texto);
        // A leitura pode estar um segundo atrás do disco: a linha do clique não passa do fim.
        let linha = linha.min(texto.split('\n').count().saturating_sub(1));
        let letra = coluna_no_texto(texto.split('\n').nth(linha).unwrap_or_default(), coluna);
        // Na leitura sem a edição do agente, as fileiras são as linhas: a rolagem passa igual.
        let rolagem = self.arquivo_edicao.is_none().then(|| self.rolagem_do_codigo()).flatten();
        let linguagem = super::painel_arquivo::linguagem(&completo);
        let estado = cx.new(|cx| {
            EditorState::new(window, cx)
                .language(linguagem)
                .line_number(true)
                .indent_guides(false)
                .folding(false)
                // O Ctrl+F continua o do painel (a barra de busca do arquivo).
                .searchable(false)
                .soft_wrap(false)
                .tab_size(TabSize { tab_size, hard_tabs })
                .default_value(texto)
        });
        estado.update(cx, |estado, cx| {
            estado.set_cursor_position(Position::new(linha as u32, letra), window, cx);
            if let Some(rolagem) = rolagem {
                estado.set_scroll_offset(rolagem, cx);
            }
        });
        let mudou = cx.subscribe_in(&estado, window, |this, _, evento: &InputEvent, window, cx| {
            if matches!(evento, InputEvent::Change) {
                this.editor_mudou(window, cx);
            }
        });
        // O agente pode gravar o mesmo arquivo: o disco é conferido a cada segundo.
        let vigia = cx.spawn_in(window, async move |janela, cx| {
            loop {
                cx.background_executor().timer(CONFERIR_DISCO).await;
                if janela.update_in(cx, |this, window, cx| this.conferir_disco_do_editor(window, cx)).is_err() {
                    break;
                }
            }
        });
        // O editor é do arquivo como está: a edição do agente (guardada para o "Mudanças"), a
        // escolha de linhas e a busca saem.
        if let Some(edicao) = self.arquivo_edicao.take() {
            self.edicao_fora = Some(edicao);
        }
        self.esquecer_vista();
        self.arquivo_selecao = None;
        self.arquivo_ancora = None;
        self.arquivo_marcada.set(None);
        self.busca_arquivo.aberta = false;
        self.editor_arquivo = Some(Editor {
            sessao,
            estado,
            agenda: Agenda::default(),
            por_salvar: false,
            erro: None,
            esperando: false,
            faixa_vista: Default::default(),
            copia: None,
            _gravar: None,
            _mudou: mudou,
            _vigia: vigia,
        });
        cx.notify();
    }

    /// Uma tecla no editor: marca a gravação para daqui a 800 ms (cada tecla empurra o prazo). Com
    /// um conflito aberto nada é gravado até você escolher.
    fn editor_mudou(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor_arquivo.as_mut() else { return };
        editor.por_salvar = true;
        cx.notify();
        if editor.sessao.conflito().is_some() {
            return;
        }
        editor.agenda.editou(Instant::now());
        if editor.esperando {
            return;
        }
        editor.esperando = true;
        editor._gravar = Some(cx.spawn_in(window, async move |janela, cx| {
            loop {
                let falta = janela.update(cx, |this, _| this.editor_arquivo.as_ref().and_then(|e| e.agenda.falta(Instant::now())));
                match falta {
                    Ok(Some(falta)) if !falta.is_zero() => cx.background_executor().timer(falta).await,
                    Ok(Some(_)) => {
                        let _ = janela.update_in(cx, |this, window, cx| {
                            this.gravar_editor(window, cx);
                        });
                        break;
                    }
                    _ => break,
                }
            }
            let _ = janela.update(cx, |this, _| {
                if let Some(editor) = this.editor_arquivo.as_mut() {
                    editor.esperando = false;
                }
            });
        }));
    }

    /// Grava o texto do editor agora (o prazo venceu, ou o Ctrl+S). `false` quando o texto não
    /// foi para o disco: um conflito para você decidir, ou um erro de gravação.
    pub(super) fn gravar_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(editor) = self.editor_arquivo.as_mut() else { return true };
        editor.agenda.limpar();
        let texto = editor.estado.read(cx).value();
        let gravou = match editor.sessao.salvar(&texto) {
            Gravacao::Gravou | Gravacao::Nada => {
                editor.por_salvar = false;
                editor.erro = None;
                true
            }
            Gravacao::Conflito(_) => false,
            Gravacao::Erro(erro) => {
                // O mesmo erro a cada tecla não vira um aviso a cada tecla.
                if editor.erro.as_deref() != Some(erro.as_str()) {
                    let nome = nome_do_arquivo(editor.sessao.caminho());
                    window.push_notification(aviso(Tom::Erro, format!("Não deu para salvar {nome}: {erro}")).chave("salvar-arquivo"), cx);
                }
                editor.erro = Some(erro);
                false
            }
        };
        cx.notify();
        gravou
    }

    /// A conferência do disco, a cada segundo com o editor aberto: o arquivo que mudou com o seu
    /// texto já gravado é relido (o cursor e a rolagem ficam); com texto por gravar, vira o
    /// conflito da faixa.
    fn conferir_disco_do_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor_arquivo.as_mut() else { return };
        let texto = editor.estado.read(cx).value();
        let antes = editor.sessao.conflito();
        match editor.sessao.conferir(&texto) {
            Disco::Igual => {
                if antes != editor.sessao.conflito() {
                    // O conflito se desfez sozinho (o disco voltou, ou ficou igual ao seu texto).
                    let sujo = editor.sessao.sujo(&texto);
                    editor.por_salvar = sujo;
                    if sujo {
                        self.editor_mudou(window, cx);
                    }
                    cx.notify();
                }
            }
            Disco::Recarregar(novo) => {
                editor.por_salvar = false;
                let estado = editor.estado.clone();
                por_no_editor(&estado, novo, window, cx);
                cx.notify();
            }
            Disco::Conflito(_) => {
                editor.agenda.limpar();
                cx.notify();
            }
        }
    }

    /// "Recarregar", na faixa do conflito: o editor fica com o que está no disco.
    pub(super) fn recarregar_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor_arquivo.as_mut() else { return };
        match editor.sessao.recarregar() {
            Ok(texto) => {
                editor.agenda.limpar();
                editor.por_salvar = false;
                editor.erro = None;
                let estado = editor.estado.clone();
                por_no_editor(&estado, texto, window, cx);
            }
            Err(erro) => window.push_notification(aviso(Tom::Erro, format!("Não deu para recarregar: {erro}")).chave("salvar-arquivo"), cx),
        }
        cx.notify();
    }

    /// "Manter o meu", na faixa do conflito: o seu texto fica e é gravado por cima no prazo de
    /// sempre (ou no Ctrl+S); o arquivo apagado é criado de novo.
    pub(super) fn manter_o_meu_no_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(editor) = self.editor_arquivo.as_mut() else { return };
        editor.sessao.manter_o_meu();
        self.editor_mudou(window, cx);
    }

    /// Ctrl+S com o foco no arquivo: grava na hora.
    pub(super) fn salvar_arquivo_agora(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editor_arquivo.is_none() {
            cx.propagate();
            return;
        }
        self.gravar_editor(window, cx);
    }

    /// Esc no editor, ou o × ao lado de "editando": volta à leitura, com o que foi escrito gravado.
    pub(super) fn sair_do_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.largar_editor(window, cx);
        window.focus(&self.foco_arquivo, cx);
    }

    /// Fecha o editor, de qualquer jeito que se saia dele (Esc, outro arquivo, fechar a aba): o
    /// que falta gravar é gravado. Com um conflito sem resposta (ou um erro de gravação), nada
    /// passa por cima do disco: o seu texto vai para a área de transferência, e um aviso diz isso.
    pub(super) fn largar_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut editor) = self.editor_arquivo.take() else { return };
        let texto = editor.estado.read(cx).value();
        if let Some(motivo) = gravar_ou_motivo(&mut editor.sessao, &texto) {
            let nome = nome_do_arquivo(editor.sessao.caminho());
            cx.write_to_clipboard(ClipboardItem::new_string(texto.to_string()));
            // O aviso tem duas linhas: o título diz o que houve e o texto, onde o escrito foi parar.
            let frase = format!("{nome}: {motivo}. O que você escreveu está na área de transferência.");
            window.push_notification(aviso(Tom::Atencao, frase).title("Texto não salvo").autohide(false).chave("salvar-arquivo"), cx);
        }
        // A leitura volta com o arquivo de agora, na linha em que o editor estava.
        let primeira = editor.estado.read(cx).visible_row_range().map(|linhas| linhas.start);
        self.reler_arquivo_aberto(primeira);
        if editor.estado.read(cx).focus_handle(cx).contains_focused(window, cx) {
            window.focus(&self.foco_arquivo, cx);
        }
        cx.notify();
    }
}

/// Grava o que falta do texto do editor, ou diz por que ele não iria para o disco: um conflito
/// sem resposta com texto seu (nada passa por cima do que o agente gravou) ou um erro de gravação.
/// `None`: o disco tem o texto, ou não havia o que gravar.
fn gravar_ou_motivo(sessao: &mut Sessao, texto: &str) -> Option<&'static str> {
    match sessao.conflito() {
        Some(Conflito::Mudou) => sessao.escrito(texto).then_some("o arquivo mudou no disco"),
        Some(Conflito::Apagado) => sessao.escrito(texto).then_some("o arquivo foi apagado"),
        None => match sessao.salvar(texto) {
            Gravacao::Gravou | Gravacao::Nada => None,
            Gravacao::Conflito(_) => Some("o arquivo mudou no disco"),
            Gravacao::Erro(_) => Some("a gravação falhou"),
        },
    }
}

/// Onde fica o texto do editor que não pôde ir para o arquivo quando o Vitral fechou: um arquivo
/// novo na pasta `nao_salvos`, com o nome do original e a hora ("carrinho-2026-11-25-143210.py").
/// Nunca passa por cima de outro.
pub(super) fn guardar_nao_salvo(pasta: &Path, original: &Path, texto: &str) -> std::io::Result<PathBuf> {
    fs::create_dir_all(pasta)?;
    let hora = crate::modelo::no_fuso_local(crate::modelo::agora() as i64).format("%Y-%m-%d-%H%M%S");
    let base = original.file_stem().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| "texto".into());
    let extensao = original.extension().map(|e| format!(".{}", e.to_string_lossy())).unwrap_or_default();
    for n in 0..100 {
        let sufixo = if n == 0 { String::new() } else { format!("-{n}") };
        let caminho = pasta.join(format!("{base}-{hora}{sufixo}{extensao}"));
        match fs::OpenOptions::new().write(true).create_new(true).open(&caminho) {
            Ok(mut arquivo) => {
                use std::io::Write as _;
                let escrito = arquivo.write_all(texto.as_bytes()).and_then(|()| arquivo.sync_all());
                drop(arquivo);
                // A cópia pela metade não fica parecendo a cópia do texto.
                return match escrito {
                    Ok(()) => Ok(caminho),
                    Err(erro) => {
                        let _ = fs::remove_file(&caminho);
                        Err(erro)
                    }
                };
            }
            Err(erro) if erro.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(erro) => return Err(erro),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "nome livre não encontrado"))
}

impl Janela {
    /// A pasta onde vai o texto do editor que não pôde ser gravado ao fechar.
    pub(super) fn pasta_nao_salvos(&self, cx: &App) -> Option<PathBuf> {
        self.motor.as_ref().map(|motor| motor.read(cx).dados.pasta_nao_salvos())
    }

    /// Antes de fechar ou reiniciar: grava o que falta no editor e, se o texto não pôde ir para o
    /// arquivo (um conflito sem resposta, um erro de gravação), devolve o nome do arquivo e o
    /// motivo, para a pergunta de fechar. O fechar não passa sem ela: o texto do editor só existe
    /// na tela.
    pub(super) fn texto_do_editor_em_risco(&mut self, cx: &App) -> Option<(String, &'static str)> {
        let editor = self.editor_arquivo.as_mut()?;
        let texto = editor.estado.read(cx).value();
        let motivo = gravar_ou_motivo(&mut editor.sessao, &texto);
        if motivo.is_none() {
            editor.por_salvar = false;
            editor.erro = None;
        }
        motivo.map(|motivo| (nome_do_arquivo(editor.sessao.caminho()), motivo))
    }

    /// O "Fechar mesmo assim" com texto em risco: o texto do editor vai para a cópia em
    /// `nao_salvos` antes de qualquer coisa fechar. Com erro (disco cheio, sem a pasta de dados), o
    /// erro, e quem chamou não fecha: a cópia é a única coisa que guardaria o texto.
    pub(super) fn copiar_texto_do_editor(&mut self, cx: &App) -> Result<(), String> {
        let pasta = self.pasta_nao_salvos(cx);
        let Some(editor) = self.editor_arquivo.as_mut() else { return Ok(()) };
        let texto = editor.estado.read(cx).value().to_string();
        if editor.copia.as_ref().is_some_and(|(copiado, caminho)| *copiado == texto && caminho.is_file()) {
            return Ok(());
        }
        let pasta = pasta.ok_or_else(|| "a pasta de dados do Vitral não está aberta".to_string())?;
        let caminho = guardar_nao_salvo(&pasta, editor.sessao.caminho(), &texto).map_err(|erro| erro.to_string())?;
        editor.copia = Some((texto, caminho));
        Ok(())
    }

    /// O Vitral está fechando: o que falta gravar no editor vai para o disco agora (o prazo da
    /// gravação sozinha não teria tempo de vencer). O texto que não pode ir para o arquivo (um
    /// conflito sem resposta passa por cima do que o agente gravou; a gravação que falhou) vai
    /// para uma cópia em `nao_salvos`, uma vez só por texto. Pela pergunta de fechar ela já foi
    /// feita no clique; o caminho que não passa pela pergunta (sair da sessão do sistema) a faz
    /// aqui.
    pub(super) fn gravar_editor_ao_sair(&mut self, cx: &App) {
        let Some(editor) = self.editor_arquivo.as_mut() else { return };
        let texto = editor.estado.read(cx).value();
        if gravar_ou_motivo(&mut editor.sessao, &texto).is_none() {
            return;
        }
        if let Err(erro) = self.copiar_texto_do_editor(cx) {
            eprintln!("vitral: o texto do editor não pôde ser guardado numa cópia: {erro}");
        }
    }
}

/// Troca o texto do editor pelo do disco, sem levar o foco, com o cursor na mesma linha e coluna
/// e a rolagem onde estava.
fn por_no_editor(estado: &Entity<EditorState>, texto: String, window: &mut Window, cx: &mut App) {
    estado.update(cx, |estado, cx| {
        let (cursor, rolagem) = (estado.cursor_position(), estado.scroll_offset());
        estado.set_value(texto, window, cx);
        let lugar = estado.text().position_to_offset(&cursor);
        estado.set_selected_range(lugar..lugar, cx);
        estado.set_scroll_offset(rolagem, cx);
    });
}

fn nome_do_arquivo(caminho: &Path) -> String {
    caminho.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

#[cfg(test)]
mod testes {
    use super::{
        Agenda, Conflito, Disco, Gravacao, LIMITE_DE_EDICAO, NaoEdita, Sessao, coluna_no_texto, gravar_ou_motivo, gravar_por_inteiro,
        guardar_nao_salvo, recuo_do_texto,
    };
    use std::{
        fs,
        path::{Path, PathBuf},
        time::{Duration, Instant},
    };

    fn pasta(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-teste-editor-{nome}-{}", crate::modelo::novo_id()));
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    /// Outro (o agente) grava o arquivo. A data anda 2 s, para a mudança aparecer mesmo com as
    /// duas gravações no mesmo instante do relógio do disco.
    fn gravar_por_fora(caminho: &Path, texto: &str) {
        let antes = fs::metadata(caminho).and_then(|m| m.modified()).ok();
        fs::write(caminho, texto).unwrap();
        if let Some(antes) = antes {
            let arquivo = fs::File::options().write(true).open(caminho).unwrap();
            arquivo.set_modified(antes + Duration::from_secs(2)).unwrap();
        }
    }

    #[test]
    fn editar_e_salvar_grava_no_disco() {
        let pasta = pasta("salvar");
        let arquivo = pasta.join("carrinho.py");
        fs::write(&arquivo, "def total():\n\treturn 1\n").unwrap();
        let (mut sessao, texto) = Sessao::abrir(&arquivo).unwrap();
        // A tabulação chega como está: o editor não pode gravar 4 espaços no lugar dela.
        assert_eq!(texto, "def total():\n\treturn 1\n");
        assert!(!sessao.sujo(&texto));
        let novo = "def total():\n\treturn 2\n";
        assert!(sessao.sujo(novo));
        assert_eq!(sessao.salvar(novo), Gravacao::Gravou);
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), novo);
        assert!(!sessao.sujo(novo));
        // Sem nada novo, não grava de novo; e a gravação do próprio editor não é "mudou no disco".
        assert_eq!(sessao.salvar(novo), Gravacao::Nada);
        assert_eq!(sessao.conferir(novo), Disco::Igual);
        assert_eq!(sessao.conflito(), None);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_fim_de_linha_do_windows_volta_como_veio() {
        let pasta = pasta("crlf");
        let arquivo = pasta.join("notas.txt");
        fs::write(&arquivo, "um\r\ndois\r\n").unwrap();
        let (mut sessao, texto) = Sessao::abrir(&arquivo).unwrap();
        assert_eq!(texto, "um\ndois\n");
        assert_eq!(sessao.salvar("um\ntrês\n"), Gravacao::Gravou);
        assert_eq!(fs::read(&arquivo).unwrap(), b"um\r\ntr\xc3\xaas\r\n");
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn so_texto_utf8_e_pequeno_entra_no_editor() {
        let pasta = pasta("entra");
        let abrir = |nome: &str, bytes: &[u8]| {
            let caminho = pasta.join(nome);
            fs::write(&caminho, bytes).unwrap();
            Sessao::abrir(&caminho).map(|(_, texto)| texto)
        };
        assert_eq!(abrir("vazio.txt", b""), Ok(String::new()));
        assert_eq!(abrir("ok.rs", b"fn main() {}\n"), Ok("fn main() {}\n".to_string()));
        assert_eq!(abrir("grande.log", &vec![b'a'; LIMITE_DE_EDICAO as usize + 1]), Err(NaoEdita::Grande));
        assert_eq!(abrir("binario.bin", b"a\0b"), Err(NaoEdita::Binario));
        // "ação" em Latin-1: lido como UTF-8 com troca e gravado de volta, as letras se perderiam.
        assert_eq!(abrir("antigo.txt", b"a\xe7\xe3o\n"), Err(NaoEdita::OutraCodificacao));
        assert!(matches!(Sessao::abrir(&pasta.join("sumiu.rs")), Err(NaoEdita::Erro(_))));
        assert!(matches!(Sessao::abrir(&pasta), Err(NaoEdita::Erro(_))));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_disco_mudou_com_o_texto_salvo_recarrega_sozinho() {
        let pasta = pasta("recarrega");
        let arquivo = pasta.join("pedido.py");
        fs::write(&arquivo, "a = 1\n").unwrap();
        let (mut sessao, texto) = Sessao::abrir(&arquivo).unwrap();
        gravar_por_fora(&arquivo, "a = 2\n");
        assert_eq!(sessao.conferir(&texto), Disco::Recarregar("a = 2\n".to_string()));
        assert_eq!(sessao.conflito(), None);
        assert!(!sessao.sujo("a = 2\n"));
        // Conferido de novo sem nada novo no disco, nada a fazer.
        assert_eq!(sessao.conferir("a = 2\n"), Disco::Igual);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_disco_mudou_com_texto_nao_salvo_e_conflito_e_recarregar_perde_o_meu() {
        let pasta = pasta("conflito");
        let arquivo = pasta.join("pedido.py");
        fs::write(&arquivo, "a = 1\n").unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        let meu = "a = 1  # meu\n";
        gravar_por_fora(&arquivo, "a = 2  # do agente\n");
        assert_eq!(sessao.conferir(meu), Disco::Conflito(Conflito::Mudou));
        assert_eq!(sessao.conflito(), Some(Conflito::Mudou));
        // Com o conflito aberto nada é gravado: o que o agente escreveu continua no disco.
        assert_eq!(sessao.salvar(meu), Gravacao::Conflito(Conflito::Mudou));
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), "a = 2  # do agente\n");
        // "Recarregar": o editor fica com o do disco.
        assert_eq!(sessao.recarregar(), Ok("a = 2  # do agente\n".to_string()));
        assert_eq!(sessao.conflito(), None);
        assert!(!sessao.sujo("a = 2  # do agente\n"));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn manter_o_meu_sobrescreve_ao_salvar() {
        let pasta = pasta("manter");
        let arquivo = pasta.join("pedido.py");
        fs::write(&arquivo, "a = 1\n").unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        let meu = "a = 1  # meu\n";
        gravar_por_fora(&arquivo, "a = 2  # do agente\n");
        assert_eq!(sessao.conferir(meu), Disco::Conflito(Conflito::Mudou));
        sessao.manter_o_meu();
        assert_eq!(sessao.conflito(), None);
        assert!(sessao.sujo(meu));
        // Escolher não grava: o disco só muda na gravação seguinte.
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), "a = 2  # do agente\n");
        assert_eq!(sessao.salvar(meu), Gravacao::Gravou);
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), meu);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn salvar_nota_a_mudanca_que_a_conferencia_ainda_nao_viu() {
        let pasta = pasta("corrida");
        let arquivo = pasta.join("pedido.py");
        fs::write(&arquivo, "a = 1\n").unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        // O agente grava entre duas conferências e a gravação sozinha vence logo depois.
        gravar_por_fora(&arquivo, "a = 2  # do agente\n");
        assert_eq!(sessao.salvar("a = 1  # meu\n"), Gravacao::Conflito(Conflito::Mudou));
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), "a = 2  # do agente\n");
        assert_eq!(sessao.conflito(), Some(Conflito::Mudou));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn apagado_enquanto_edita_mantem_o_texto() {
        let pasta = pasta("apagado");
        let arquivo = pasta.join("rascunho.md");
        fs::write(&arquivo, "# Rascunho\n").unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        let meu = "# Rascunho\n\nmais uma linha\n";
        fs::remove_file(&arquivo).unwrap();
        assert_eq!(sessao.conferir(meu), Disco::Conflito(Conflito::Apagado));
        // Nada recria o arquivo sem você mandar.
        assert_eq!(sessao.salvar(meu), Gravacao::Conflito(Conflito::Apagado));
        assert!(!arquivo.exists());
        assert!(sessao.recarregar().is_err());
        assert_eq!(sessao.conflito(), Some(Conflito::Apagado));
        // "Manter o meu": a gravação seguinte cria o arquivo de novo, com o seu texto.
        sessao.manter_o_meu();
        assert_eq!(sessao.salvar(meu), Gravacao::Gravou);
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), meu);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn apagado_com_o_texto_salvo_tambem_mantem_o_texto() {
        let pasta = pasta("apagado-limpo");
        let arquivo = pasta.join("rascunho.md");
        fs::write(&arquivo, "# Rascunho\n").unwrap();
        let (mut sessao, texto) = Sessao::abrir(&arquivo).unwrap();
        fs::remove_file(&arquivo).unwrap();
        assert_eq!(sessao.conferir(&texto), Disco::Conflito(Conflito::Apagado));
        sessao.manter_o_meu();
        assert!(sessao.sujo(&texto));
        assert_eq!(sessao.salvar(&texto), Gravacao::Gravou);
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), "# Rascunho\n");
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn o_conflito_some_quando_o_disco_fica_igual_ao_meu() {
        let pasta = pasta("igual");
        let arquivo = pasta.join("pedido.py");
        fs::write(&arquivo, "a = 1\n").unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        let meu = "a = 3\n";
        gravar_por_fora(&arquivo, "a = 2\n");
        assert_eq!(sessao.conferir(meu), Disco::Conflito(Conflito::Mudou));
        gravar_por_fora(&arquivo, meu);
        assert_eq!(sessao.conferir(meu), Disco::Igual);
        assert_eq!(sessao.conflito(), None);
        assert!(!sessao.sujo(meu));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn a_gravacao_sozinha_espera_800_ms_depois_da_ultima_tecla() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        let mut agenda = Agenda::default();
        assert_eq!(agenda.falta(t0), None);
        agenda.editou(t0);
        assert_eq!(agenda.falta(t0), Some(ms(800)));
        // Outra tecla meio segundo depois empurra a gravação.
        agenda.editou(t0 + ms(500));
        assert_eq!(agenda.falta(t0 + ms(900)), Some(ms(400)));
        assert_eq!(agenda.falta(t0 + ms(1300)), Some(Duration::ZERO));
        assert_eq!(agenda.falta(t0 + ms(5000)), Some(Duration::ZERO));
        agenda.limpar();
        assert_eq!(agenda.falta(t0 + ms(5000)), None);
    }

    #[test]
    fn o_recuo_do_editor_e_o_do_arquivo() {
        assert_eq!(recuo_do_texto("fn a() {\n    let x = 1;\n        y\n}\n"), (4, false));
        assert_eq!(recuo_do_texto("a:\n  b: 1\n    c: 2\n"), (2, false));
        assert_eq!(recuo_do_texto("def a():\n\treturn 1\n"), (4, true));
        assert_eq!(recuo_do_texto("sem recuo\nnenhum\n"), (4, false));
        // Uma linha de continuação com 1 espaço (um comentário de bloco) não é o recuo do arquivo.
        assert_eq!(recuo_do_texto("/*\n * nota\n */\nfn a() {\n    x\n}\n"), (4, false));
    }

    #[test]
    fn o_clique_cai_na_coluna_certa() {
        // Na tela a tabulação vale 4 colunas; no arquivo, uma letra.
        assert_eq!(coluna_no_texto("\treturn 1", 0), 0);
        assert_eq!(coluna_no_texto("\treturn 1", 1), 0);
        assert_eq!(coluna_no_texto("\treturn 1", 3), 1);
        assert_eq!(coluna_no_texto("\treturn 1", 4), 1);
        assert_eq!(coluna_no_texto("\treturn 1", 6), 3);
        assert_eq!(coluna_no_texto("ação!", 2), 2);
        // Depois do fim da linha, o fim dela.
        assert_eq!(coluna_no_texto("ação!", 80), 5);
        assert_eq!(coluna_no_texto("", 3), 0);
        // O editor do kit conta a coluna em letras: o emoji vale uma.
        assert_eq!(coluna_no_texto("😀x", 1), 1);
    }

    /// A gravação nunca deixa o arquivo pela metade: o texto vai inteiro para um arquivo novo que
    /// toma o lugar do original, sem sobrar temporário na pasta, com as permissões dele, e o link
    /// continua link. Rodado sob o limite de tamanho de arquivo do sistema
    /// (`ulimit -f 1` com o SIGXFSZ ignorado), a gravação falha e o original fica inteiro.
    #[test]
    fn gravar_nunca_deixa_o_arquivo_pela_metade() {
        let pasta = pasta("inteiro");
        let arquivo = pasta.join("grande.txt");
        let original = "original\n".repeat(50);
        fs::write(&arquivo, &original).unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        let novo = "x".repeat(4000);
        match sessao.salvar(&novo) {
            Gravacao::Gravou => assert_eq!(fs::read_to_string(&arquivo).unwrap(), novo),
            Gravacao::Erro(_) => assert_eq!(fs::read_to_string(&arquivo).unwrap(), original, "o erro não corta o original"),
            outro => panic!("{outro:?}"),
        }
        let sobras: Vec<_> = fs::read_dir(&pasta).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(sobras, [std::ffi::OsString::from("grande.txt")], "nenhum temporário fica na pasta");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let script = pasta.join("rodar.sh");
            fs::write(&script, "echo 1\n").unwrap();
            fs::set_permissions(&script, fs::Permissions::from_mode(0o750)).unwrap();
            gravar_por_inteiro(&script, b"echo 2\n").unwrap();
            assert_eq!(fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o750, "as permissões do original ficam");
            // O link simbólico continua link, e o texto vai para o arquivo a que ele aponta.
            let link = pasta.join("atalho.sh");
            std::os::unix::fs::symlink(&script, &link).unwrap();
            gravar_por_inteiro(&link, b"echo 3\n").unwrap();
            assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
            assert_eq!(fs::read_to_string(&script).unwrap(), "echo 3\n");
            // O link físico: os dois nomes veem o texto novo.
            let outro_nome = pasta.join("mesmo.sh");
            fs::hard_link(&script, &outro_nome).unwrap();
            gravar_por_inteiro(&script, b"echo 4\n").unwrap();
            assert_eq!(fs::read_to_string(&outro_nome).unwrap(), "echo 4\n");
            // O link cujo arquivo foi apagado continua link, e o arquivo nasce de novo no lugar
            // (o alvo relativo conta da pasta do link).
            fs::create_dir_all(pasta.join("comum")).unwrap();
            let sem_alvo = pasta.join("config.yml");
            std::os::unix::fs::symlink("comum/config.yml", &sem_alvo).unwrap();
            gravar_por_inteiro(&sem_alvo, b"a: 1\n").unwrap();
            assert!(fs::symlink_metadata(&sem_alvo).unwrap().file_type().is_symlink());
            assert_eq!(fs::read_to_string(pasta.join("comum/config.yml")).unwrap(), "a: 1\n");
            // O somente leitura falha como falhava, sem ser trocado por um arquivo novo (como root,
            // que escreve em qualquer arquivo, o teste não diz nada).
            let travado = pasta.join("travado.lock");
            fs::write(&travado, "fixo\n").unwrap();
            fs::set_permissions(&travado, fs::Permissions::from_mode(0o444)).unwrap();
            if unsafe { libc::geteuid() } != 0 {
                assert!(gravar_por_inteiro(&travado, b"novo\n").is_err());
                assert_eq!(fs::read_to_string(&travado).unwrap(), "fixo\n");
            }
            fs::set_permissions(&travado, fs::Permissions::from_mode(0o644)).unwrap();
            let sobras: Vec<String> = fs::read_dir(&pasta).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).filter(|n| n.contains(".vitral-")).collect();
            assert!(sobras.is_empty(), "{sobras:?}");
        }
        let _ = fs::remove_dir_all(&pasta);
    }

    /// Ao fechar: o texto com um conflito sem resposta não passa por cima do disco e vai para uma
    /// cópia com o nome do arquivo e a hora, sem pisar em outra; sem conflito, ele é gravado.
    #[test]
    fn o_texto_que_nao_pode_ir_para_o_arquivo_vai_para_uma_copia() {
        let pasta = pasta("nao-salvo");
        let arquivo = pasta.join("carrinho.py");
        fs::write(&arquivo, "a = 1\n").unwrap();
        let (mut sessao, _) = Sessao::abrir(&arquivo).unwrap();
        gravar_por_fora(&arquivo, "a = 2  # do agente\n");
        let meu = "a = 3  # meu\n";
        assert_eq!(sessao.conferir(meu), Disco::Conflito(Conflito::Mudou));
        assert_eq!(gravar_ou_motivo(&mut sessao, meu), Some("o arquivo mudou no disco"));
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), "a = 2  # do agente\n", "o disco fica com o agente");
        let guardados = pasta.join("nao_salvos");
        let primeira = guardar_nao_salvo(&guardados, &arquivo, meu).unwrap();
        let segunda = guardar_nao_salvo(&guardados, &arquivo, "outro").unwrap();
        assert_ne!(primeira, segunda, "a segunda cópia no mesmo segundo não pisa na primeira");
        assert_eq!(fs::read_to_string(&primeira).unwrap(), meu);
        let nome = primeira.file_name().unwrap().to_string_lossy().into_owned();
        assert!(nome.starts_with("carrinho-") && nome.ends_with(".py"), "{nome}");
        // Sem conflito, o texto vai para o arquivo e não há motivo.
        let (mut limpa, _) = Sessao::abrir(&arquivo).unwrap();
        assert_eq!(gravar_ou_motivo(&mut limpa, "a = 4\n"), None);
        assert_eq!(fs::read_to_string(&arquivo).unwrap(), "a = 4\n");
        let _ = fs::remove_dir_all(&pasta);
    }
}
