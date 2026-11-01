//! Pasta de dados global do Vitral, que substitui o `.vitral/` de cada pasta de trabalho:
//! - `~/.local/share/vitral/` (`Dados::pasta`): `estado.json` (com `.anterior`), `chats/<id>.json`,
//!   `notas/<projeto>.md`, `mestre/`, `avulsos/<chat>/`, `projetos/<slug>/` (pastas próprias),
//!   `copias/<projeto>/t<N>/`, `exportadas/`, `anexos/` e `migracao.json`;
//! - `~/.config/vitral/` (`Dados::config`): `config.json`, `interface.json`, as contas e o
//!   `recentes.json` antigo, que só a migração lê;
//! - `~/.cache/vitral/` (`Dados::cache`): os prompts de cada turno.
//!
//! A pasta de dados também guarda o app instalado (`app/`, do atualizador: as versões, os links
//! `atual` e `anterior` e o `estado.json` dele) e o `formato` dos dados. Nada daqui mexe em `app/`.
//!
//! Toda gravação aguenta a fonte desligar no meio: o arquivo fica inteiro, velho ou novo.

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::modelo::{
    CHAVES_RESERVADAS, Cartao, Chat, Config, EstadoCartao, Estado, EstadoRef, EstadoTarefa, FORMATO_DADOS,
    Forma, ID_MESTRE, Item, MAX_OCORRENCIAS, PastaLigada, Papel, Projeto, Tarefa, TipoCartao,
    VERSAO_ESTADO, agora,
};

/// Ligada desde a 0.3.0. Numa mudança grande dos dados, desligar deixa `Dados::padrao()` só na
/// bancada (`DENTRO_DA_BANCADA=1`, que isola os XDG): um build do meio da mudança não grava nos dados
/// de verdade, o `--instalar` e o `instalar-dev.sh` recusam, e um release estável não passa nos testes.
pub const DADOS_GLOBAIS: bool = true;

/// A pasta do app instalado dentro da pasta de dados (`atualizacao::linux::Raiz::app`).
pub const PASTA_DO_APP: &str = "app";

/// Os dados globais nasceram no formato 2: um estado gravado sem o arquivo `formato` é dele.
const PRIMEIRO_FORMATO_GLOBAL: u32 = 2;

/// O formato dos dados em `pasta` (a `Dados::pasta`), que o atualizador compara com o de cada
/// versão antes de voltar: o número do arquivo `formato`; sem ele, o 2 quando já há estado gravado
/// e o 1 quando não há (só o Vitral antigo, cujo `.vitral/` a migração não muda).
pub fn formato_em(pasta: &Path) -> u32 {
    let estado = pasta.join("estado.json");
    let tem_estado = !nao_existe(&estado) || !nao_existe(&com_sufixo(&estado, ".anterior"));
    let pelo_estado = if tem_estado { PRIMEIRO_FORMATO_GLOBAL } else { 1 };
    formato_gravado(pasta).unwrap_or(1).max(pelo_estado)
}

fn formato_gravado(pasta: &Path) -> Option<u32> {
    fs::read_to_string(pasta.join("formato")).ok()?.trim().parse().ok()
}

#[derive(Clone, Debug)]
pub struct Dados {
    pub pasta: PathBuf,
    pub config: PathBuf,
    pub cache: PathBuf,
    socket: PathBuf,
    gravador: Arc<Gravador>,
    /// As notas lidas há pouco: a interface pergunta por elas a cada quadro.
    notas: Arc<Mutex<HashMap<String, (Instant, String)>>>,
}

/// O que `carregar` leu: o estado com as mensagens de cada chat, a configuração e o que avisar.
pub struct Carregado {
    pub estado: Estado,
    pub config: Config,
    pub avisos: Vec<String>,
}

/// Quanto tempo uma leitura das notas vale antes de ir ao disco de novo.
const VALIDADE_DAS_NOTAS: Duration = Duration::from_secs(2);

/// Por que o estado gravado não abre: o app abre só para leitura, sem gravar nada por cima.
#[derive(Debug, PartialEq)]
pub enum ErroCarga {
    /// `copia` é a outra versão, quando existe e também não serve.
    Ilegivel { arquivo: PathBuf, copia: Option<PathBuf> },
    VersaoNova { arquivo: PathBuf, versao: u32 },
}

/// Chaves da configuração que mudaram de nome: (nova, antiga).
pub const CHAVES_RENOMEADAS: [(&str, &str); 2] =
    [("modelo_mestre", "modelo_coordenador"), ("modelo_chats", "modelo_trabalhador")];

impl Dados {
    /// As pastas do usuário. Nunca cai em "." quando falta uma delas.
    pub fn padrao() -> Result<Self> {
        if cfg!(test) {
            panic!("Dados::padrao() num teste: use Dados::em(pasta temporária)");
        }
        if !DADOS_GLOBAIS && std::env::var_os("DENTRO_DA_BANCADA").is_none_or(|v| v != "1") {
            bail!("os dados globais ainda não estão ligados nesta versão: abra o Vitral pela bancada");
        }
        let falta = |qual: &str| anyhow!("sem a pasta de {qual} do usuário (HOME e XDG_* vazios)");
        // No Windows os dados vão para o %LOCALAPPDATA%\vitral, não para o %APPDATA% (o Roaming): as
        // cópias das conversas têm node_modules, e o Roaming sincroniza entre PCs. A configuração fica
        // no %APPDATA%. O cache de lá é a mesma pasta Local: ele vai numa subpasta dos dados. No Linux
        // a pasta local dos dados é a mesma de sempre (~/.local/share).
        let pasta = dirs::data_local_dir().ok_or_else(|| falta("dados"))?.join("vitral");
        let config = dirs::config_dir().ok_or_else(|| falta("configuração"))?.join("vitral");
        let cache = if cfg!(windows) {
            pasta.join("cache")
        } else {
            dirs::cache_dir().ok_or_else(|| falta("cache"))?.join("vitral")
        };
        let socket = match dirs::runtime_dir() {
            Some(execucao) => execucao.join("vitral").join("vitral.sock"),
            // No Windows, o arquivo com a porta e a senha do canal (`instancia.rs`).
            None if cfg!(windows) => cache.join("vitral.porta"),
            None => cache.join("vitral.sock"),
        };
        Ok(Self { pasta, config, cache, socket, gravador: Default::default(), notas: Default::default() })
    }

    /// Para os testes: tudo dentro de `raiz`.
    #[cfg(test)]
    pub fn em(raiz: &Path) -> Self {
        let cache = raiz.join("cache");
        Self {
            pasta: raiz.join("dados"),
            config: raiz.join("config"),
            socket: cache.join("vitral.sock"),
            cache,
            gravador: Default::default(),
            notas: Default::default(),
        }
    }

    pub fn arquivo_estado(&self) -> PathBuf {
        self.pasta.join("estado.json")
    }

    pub fn arquivo_config(&self) -> PathBuf {
        self.config.join("config.json")
    }

    /// As pastas de trabalho abertas no Vitral antigo, da mais recente para a mais antiga.
    pub fn arquivo_recentes(&self) -> PathBuf {
        self.config.join("recentes.json")
    }

    pub fn arquivo_chat(&self, id: &str) -> PathBuf {
        self.pasta.join("chats").join(format!("{id}.json"))
    }

    pub fn arquivo_notas(&self, projeto: &str) -> PathBuf {
        self.pasta.join("notas").join(format!("{projeto}.md"))
    }

    pub fn pasta_mestre(&self) -> PathBuf {
        self.pasta.join("mestre")
    }

    pub fn arquivo_memoria(&self) -> PathBuf {
        self.pasta_mestre().join("memoria.md")
    }

    pub fn pasta_avulso(&self, chat: &str) -> PathBuf {
        self.pasta.join("avulsos").join(chat)
    }

    pub fn pasta_projetos(&self) -> PathBuf {
        self.pasta.join("projetos")
    }

    /// As cópias de um projeto; as dos chats sem projeto ficam em `copias/avulsos` (uma chave
    /// que nenhum projeto pode ter).
    pub fn pasta_copias(&self, projeto: &str) -> PathBuf {
        let chave = if projeto.is_empty() { "avulsos" } else { projeto };
        self.pasta.join("copias").join(chave)
    }

    pub fn pasta_exportadas(&self) -> PathBuf {
        self.pasta.join("exportadas")
    }

    pub fn pasta_anexos(&self) -> PathBuf {
        self.pasta.join("anexos")
    }

    pub fn arquivo_migracao(&self) -> PathBuf {
        self.pasta.join("migracao.json")
    }

    /// Fora da pasta de dados: a trava existe antes da migração, que grava direto nela.
    pub fn arquivo_trava(&self) -> PathBuf {
        self.pasta.with_file_name("vitral.trava")
    }

    pub fn arquivo_socket(&self) -> PathBuf {
        self.socket.clone()
    }

    pub fn arquivo_prompt(&self, chat: &str) -> PathBuf {
        self.cache.join("prompts").join(format!("{chat}.md"))
    }

    pub fn arquivo_formato(&self) -> PathBuf {
        self.pasta.join("formato")
    }

    pub fn formato_no_disco(&self) -> u32 {
        formato_em(&self.pasta)
    }

    /// Grava `FORMATO_DADOS` no `formato` quando o disco diz menos (nunca abaixa: uma versão mais
    /// nova pode ter subido o número).
    pub fn marcar_formato(&self) {
        if formato_gravado(&self.pasta).is_some_and(|n| n >= FORMATO_DADOS) {
            return;
        }
        let feito = fs::create_dir_all(&self.pasta)
            .map_err(anyhow::Error::from)
            .and_then(|()| gravar_seguro(&self.arquivo_formato(), format!("{FORMATO_DADOS}\n").as_bytes(), false));
        if let Err(erro) = feito {
            eprintln!("vitral: não gravei {}: {erro:#}", self.arquivo_formato().display());
        }
    }

    /// Nunca houve estado aqui. Um erro que não seja "não existe" (disco com problema, sem
    /// permissão) conta como já usado: a migração nunca grava por cima de um estado que não leu.
    pub fn primeiro_uso(&self) -> bool {
        let estado = self.arquivo_estado();
        nao_existe(&estado) && nao_existe(&com_sufixo(&estado, ".anterior"))
    }

    /// O estado gravado; `Ok(None)` no primeiro uso. Com o `estado.json` estragado e o `.anterior`
    /// bom, afasta o estragado antes (senão a próxima gravação faria dele o `.anterior`) e fica
    /// com o anterior, com aviso. Um erro de leitura que não é de conteúdo não restaura nada.
    pub fn ler_estado(&self) -> Result<Option<(Estado, Vec<String>)>, ErroCarga> {
        let arquivo = self.arquivo_estado();
        let anterior = com_sufixo(&arquivo, ".anterior");
        let ilegivel = |arquivo: &Path, copia: Option<&Path>| ErroCarga::Ilegivel {
            arquivo: arquivo.to_path_buf(),
            copia: copia.map(Path::to_path_buf),
        };
        let estragado = match ler_estado_de(&arquivo) {
            LeituraEstado::Lido(estado) => return Ok(Some((estado, Vec::new()))),
            LeituraEstado::VersaoNova(versao) => return Err(ErroCarga::VersaoNova { arquivo, versao }),
            LeituraEstado::Falhou => return Err(ilegivel(&arquivo, None)),
            LeituraEstado::Ausente => false,
            LeituraEstado::Estragado => true,
        };
        match ler_estado_de(&anterior) {
            LeituraEstado::Lido(estado) => {
                if estragado {
                    let afastado = self.pasta.join(format!("estado.ilegivel-{}.json", agora()));
                    if fs::rename(&arquivo, &afastado).is_err() {
                        return Err(ilegivel(&arquivo, Some(&anterior)));
                    }
                    let _ = sincronizar_pasta(&self.pasta);
                }
                let aviso = "o estado foi restaurado da cópia anterior; a última mudança pode ter se perdido";
                Ok(Some((estado, vec![aviso.to_string()])))
            }
            LeituraEstado::Ausente if !estragado => Ok(None),
            LeituraEstado::Ausente => Err(ilegivel(&arquivo, None)),
            LeituraEstado::VersaoNova(versao) => Err(ErroCarga::VersaoNova { arquivo: anterior, versao }),
            LeituraEstado::Estragado | LeituraEstado::Falhou if estragado => {
                Err(ilegivel(&arquivo, Some(&anterior)))
            }
            LeituraEstado::Estragado | LeituraEstado::Falhou => Err(ilegivel(&anterior, None)),
        }
    }

    /// O estado (com as mensagens de cada chat) e a configuração. No primeiro uso, um estado
    /// vazio. A configuração ilegível fica a padrão, com aviso; um item de conversa que esta
    /// versão não conhece fica guardado como veio.
    pub fn carregar(&self) -> Result<Carregado, ErroCarga> {
        let (mut estado, mut avisos) = self
            .ler_estado()?
            .unwrap_or_else(|| (Estado { proxima_tarefa: 1, ..Default::default() }, Vec::new()));
        estado.versao = VERSAO_ESTADO;
        let config = self.ler_config(&mut avisos);
        for chat in &mut estado.chats {
            chat.itens = self.ler_itens(&chat.id, &mut avisos);
        }
        Ok(Carregado { estado, config, avisos })
    }

    fn ler_config(&self, avisos: &mut Vec<String>) -> Config {
        let arquivo = self.arquivo_config();
        match ler_json_r::<Value>(&arquivo) {
            Ok(None) => Config::default(),
            Ok(Some(valor)) => serde_json::from_value(config_sem_legado(valor)).unwrap_or_else(|erro| {
                let copia = arquivo.with_extension(format!("ilegivel-{}.json", agora()));
                let _ = copiar_com_fsync(&arquivo, &copia);
                avisos.push(format!(
                    "a configuração em {} não serve ({erro}); ficou a padrão, e a antiga está em {}",
                    arquivo.display(),
                    copia.display()
                ));
                Config::default()
            }),
            Err(erro) => {
                avisos.push(format!("{erro:#}; a configuração ficou a padrão"));
                Config::default()
            }
        }
    }

    fn ler_itens(&self, chat: &str, avisos: &mut Vec<String>) -> Vec<Item> {
        match ler_json_r::<Vec<Value>>(&self.arquivo_chat(chat)) {
            Ok(itens) => itens
                .unwrap_or_default()
                .into_iter()
                .map(|v| serde_json::from_value(v.clone()).unwrap_or(Item::Desconhecido(v)))
                .collect(),
            Err(erro) => {
                avisos.push(format!("{erro:#}; a conversa abriu vazia"));
                Vec::new()
            }
        }
    }

    /// Serializa aqui e deixa a escrita (com fsync e `.anterior`) para a thread do gravador.
    pub fn salvar_estado(&self, estado: &EstadoRef) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(estado)?;
        self.gravador.gravar(self.arquivo_estado(), Some(bytes), true);
        Ok(())
    }

    /// Por cima do arquivo atual: as chaves que esta versão não conhece ficam, as legadas saem.
    pub fn salvar_config(&self, config: &Config) -> Result<()> {
        let arquivo = self.arquivo_config();
        let atual = ler_valor_sem_escrever(&arquivo).ok().flatten().filter(Value::is_object);
        let mut valor = config_sem_legado(atual.unwrap_or_else(|| Value::Object(Default::default())));
        if let (Some(chaves), Value::Object(novas)) = (valor.as_object_mut(), serde_json::to_value(config)?) {
            chaves.extend(novas);
        }
        self.gravador.gravar(arquivo, Some(serde_json::to_vec_pretty(&valor)?), true);
        Ok(())
    }

    pub fn salvar_chat(&self, chat: &Chat) -> Result<()> {
        let bytes = serde_json::to_vec_pretty(&chat.itens)?;
        self.gravador.gravar(self.arquivo_chat(&chat.id), Some(bytes), false);
        Ok(())
    }

    /// Apaga as mensagens de um chat, na fila, depois do estado que já não o tem.
    pub fn apagar_chat(&self, id: &str) {
        self.gravador.gravar(self.arquivo_chat(id), None, false);
    }

    pub fn apagar_notas(&self, projeto: &str) {
        self.notas.lock().unwrap_or_else(|e| e.into_inner()).remove(projeto);
        self.gravador.gravar(self.arquivo_notas(projeto), None, false);
    }

    /// Espera tudo o que está na fila chegar ao disco (ao fechar o Vitral).
    pub fn esperar_gravacoes(&self) {
        self.gravador.esperar();
    }

    /// A gravação que falhou e ainda não deu certo depois ("caminho: erro").
    pub fn falha_de_gravacao(&self) -> Option<String> {
        self.gravador.falha()
    }

    /// Põe de novo na fila o que falhou.
    pub fn gravar_de_novo(&self) {
        self.gravador.de_novo();
    }

    pub fn notas(&self, projeto: &str) -> String {
        let mut lembradas = self.notas.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((quando, notas)) = lembradas.get(projeto)
            && quando.elapsed() < VALIDADE_DAS_NOTAS
        {
            return notas.clone();
        }
        let notas = fs::read_to_string(self.arquivo_notas(projeto)).unwrap_or_default();
        lembradas.insert(projeto.to_string(), (Instant::now(), notas.clone()));
        notas
    }

    /// Acrescenta uma decisão às notas do projeto, gravando o arquivo inteiro de novo (nunca fica
    /// pela metade).
    pub fn anotar(&self, projeto: &str, texto: &str) -> Result<()> {
        let caminho = self.arquivo_notas(projeto);
        let mut atual = match fs::read_to_string(&caminho) {
            Ok(atual) => atual,
            Err(erro) if erro.kind() == io::ErrorKind::NotFound => String::new(),
            Err(erro) => return Err(anyhow::Error::new(erro).context(caminho.display().to_string())),
        };
        if !atual.is_empty() && !atual.ends_with('\n') {
            atual.push('\n');
        }
        atual.push_str(&format!("- {}\n", texto.trim()));
        self.gravar_notas(projeto, &atual)
    }

    pub fn gravar_notas(&self, projeto: &str, texto: &str) -> Result<()> {
        gravar_texto(&self.arquivo_notas(projeto), texto)?;
        self.notas
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(projeto.to_string(), (Instant::now(), texto.to_string()));
        Ok(())
    }

    pub fn memoria(&self) -> String {
        fs::read_to_string(self.arquivo_memoria()).unwrap_or_default()
    }

    pub fn gravar_memoria(&self, texto: &str) -> Result<()> {
        gravar_texto(&self.arquivo_memoria(), texto)
    }

    /// A pasta própria de um projeto sem pasta: `projetos/<slug>` (`-2`, `-3`… se já existir), com
    /// git e o commit inicial. Tudo acontece em `.<slug>.criando` e só vira a pasta no rename, com
    /// tudo já no disco: uma queda no meio deixa só o `.criando`, que a próxima vez refaz.
    pub fn criar_pasta_propria(&self, nome: &str) -> Result<PathBuf> {
        let base = slug(nome);
        if base.is_empty() {
            bail!("use pelo menos uma letra ou um número no nome");
        }
        let projetos = self.pasta_projetos();
        fs::create_dir_all(&projetos)?;
        let criando = projetos.join(format!(".{base}.criando"));
        if !nao_existe(&criando) {
            fs::remove_dir_all(&criando).with_context(|| criando.display().to_string())?;
        }
        fs::create_dir(&criando)?;
        crate::git::iniciar_repositorio(&criando)?;
        sincronizar_arvore(&criando)?;
        let destino = (1..)
            .map(|n| match n {
                1 => projetos.join(&base),
                n => projetos.join(format!("{base}-{n}")),
            })
            .find(|p| nao_existe(p))
            .expect("sempre há um sufixo livre");
        fs::rename(&criando, &destino)?;
        sincronizar_pasta(&projetos)?;
        Ok(destino)
    }

    /// "Começar do zero": afasta o que há na pasta de dados para `<pasta>.descartado-<hora>`, sem
    /// apagar nada. O app instalado (`app/`) fica onde está. O estado vai primeiro: uma queda no meio
    /// deixa um primeiro uso, que grava por cima do que sobrou; um erro devolve o que já tinha ido.
    pub fn descartar_tudo(&self) -> Result<PathBuf> {
        let mae = self.pasta.parent().context("a pasta de dados não tem pasta-mãe")?;
        let nome = self.pasta.file_name().context("a pasta de dados não tem nome")?.to_string_lossy();
        let base = format!("{nome}.descartado-{}", agora());
        let destino = (1..)
            .map(|n| match n {
                1 => mae.join(&base),
                n => mae.join(format!("{base}-{n}")),
            })
            .find(|p| nao_existe(p))
            .expect("sempre há um sufixo livre");
        let mut entradas: Vec<PathBuf> = fs::read_dir(&self.pasta)
            .with_context(|| self.pasta.display().to_string())?
            .map(|e| e.map(|e| e.path()))
            .collect::<io::Result<_>>()
            .with_context(|| self.pasta.display().to_string())?;
        entradas.retain(|p| p.file_name().is_some_and(|n| n != PASTA_DO_APP));
        entradas.sort_by_key(|p| !p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("estado.json")));
        fs::create_dir(&destino).with_context(|| destino.display().to_string())?;
        let mut foram: Vec<(&Path, PathBuf)> = Vec::new();
        for origem in &entradas {
            let alvo = destino.join(origem.file_name().expect("entrada de pasta tem nome"));
            if let Err(erro) = fs::rename(origem, &alvo) {
                for (de, para) in foram.iter().rev() {
                    let _ = fs::rename(para, de);
                }
                let _ = fs::remove_dir(&destino);
                return Err(erro).with_context(|| origem.display().to_string());
            }
            foram.push((origem, alvo));
        }
        sincronizar_pasta(&destino)?;
        sincronizar_pasta(&self.pasta)?;
        sincronizar_pasta(mae)?;
        Ok(destino)
    }
}

/// Escreve as mensagens dos chats, o estado e a configuração numa thread só, fora da janela: o
/// fsync de conversas de alguns MB a cada poucos segundos engasgava a interface. A ordem fica (um
/// arquivo por vez) e um arquivo que ainda espera a vez só troca de conteúdo. O que falha fica
/// guardado até gravar de novo, para o motor avisar e tentar outra vez.
#[derive(Default)]
struct Gravador {
    fila: Arc<(Mutex<FilaDeGravacao>, Condvar)>,
}

impl std::fmt::Debug for Gravador {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Gravador")
    }
}

struct Gravacao {
    caminho: PathBuf,
    /// `None` apaga o arquivo.
    bytes: Option<Vec<u8>>,
    anterior: bool,
}

#[derive(Default)]
struct FilaDeGravacao {
    pendentes: VecDeque<Gravacao>,
    /// As que falharam, com o erro; saem quando o mesmo arquivo grava.
    falhas: Vec<(Gravacao, String)>,
    gravando: bool,
    fechada: bool,
    /// A thread só abre na primeira gravação (os testes criam muitos `Dados`).
    aberta: bool,
}

impl Gravador {
    fn travar(&self) -> std::sync::MutexGuard<'_, FilaDeGravacao> {
        self.fila.0.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn gravar(&self, caminho: PathBuf, bytes: Option<Vec<u8>>, anterior: bool) {
        let mut f = self.travar();
        if !f.aberta {
            f.aberta = true;
            let fila = self.fila.clone();
            std::thread::spawn(move || trabalhar(&fila));
        }
        // Apagar tira da fila uma gravação do mesmo arquivo, para não recriá-lo.
        if bytes.is_none() {
            f.pendentes.retain(|g| g.caminho != caminho);
        }
        match f.pendentes.iter_mut().find(|g| g.caminho == caminho) {
            Some(pendente) => (pendente.bytes, pendente.anterior) = (bytes, anterior),
            None => f.pendentes.push_back(Gravacao { caminho, bytes, anterior }),
        }
        self.fila.1.notify_all();
    }

    fn esperar(&self) {
        let mut f = self.travar();
        while f.gravando || !f.pendentes.is_empty() {
            f = self.fila.1.wait(f).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn falha(&self) -> Option<String> {
        self.travar().falhas.first().map(|(g, erro)| format!("{}: {erro}", g.caminho.display()))
    }

    fn de_novo(&self) {
        let falhas = std::mem::take(&mut self.travar().falhas);
        for (g, _) in falhas {
            self.gravar(g.caminho, g.bytes, g.anterior);
        }
    }
}

fn trabalhar(fila: &(Mutex<FilaDeGravacao>, Condvar)) {
    let (trava, sinal) = fila;
    loop {
        let gravacao = {
            let mut f = trava.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                if let Some(proxima) = f.pendentes.pop_front() {
                    f.gravando = true;
                    break proxima;
                }
                if f.fechada {
                    return;
                }
                f = sinal.wait(f).unwrap_or_else(|e| e.into_inner());
            }
        };
        let feito = match &gravacao.bytes {
            Some(bytes) => gravar_seguro(&gravacao.caminho, bytes, gravacao.anterior).map_err(|e| format!("{e:#}")),
            None => match fs::remove_file(&gravacao.caminho) {
                Err(erro) if erro.kind() != io::ErrorKind::NotFound => Err(erro.to_string()),
                _ => Ok(()),
            },
        };
        let mut f = trava.lock().unwrap_or_else(|e| e.into_inner());
        f.falhas.retain(|(g, _)| g.caminho != gravacao.caminho);
        if let Err(erro) = feito {
            eprintln!("vitral: não gravou {}: {erro}", gravacao.caminho.display());
            // Uma gravação mais nova do mesmo arquivo, já na fila, vale mais que a que falhou.
            if !f.pendentes.iter().any(|g| g.caminho == gravacao.caminho) {
                f.falhas.push((gravacao, erro));
            }
        }
        f.gravando = false;
        sinal.notify_all();
    }
}

impl Drop for Gravador {
    /// A thread termina o que já está na fila e sai.
    fn drop(&mut self) {
        self.travar().fechada = true;
        self.fila.1.notify_all();
    }
}

enum LeituraEstado {
    Ausente,
    Lido(Estado),
    VersaoNova(u32),
    /// O conteúdo não serve (JSON quebrado ou num formato que não é o do estado).
    Estragado,
    /// Não deu para ler o arquivo (permissão, disco).
    Falhou,
}

fn ler_estado_de(arquivo: &Path) -> LeituraEstado {
    let valor = match ler_valor_sem_escrever(arquivo) {
        Ok(None) => return LeituraEstado::Ausente,
        Ok(Some(valor)) => valor,
        Err(erro) if e_ilegivel(&erro) => return LeituraEstado::Estragado,
        Err(_) => return LeituraEstado::Falhou,
    };
    let versao = valor.get("versao").and_then(Value::as_u64).unwrap_or(0);
    if versao > u64::from(VERSAO_ESTADO) {
        return LeituraEstado::VersaoNova(u32::try_from(versao).unwrap_or(u32::MAX));
    }
    match serde_json::from_value(valor) {
        Ok(estado) => LeituraEstado::Lido(estado),
        Err(_) => LeituraEstado::Estragado,
    }
}

/// O caminho com um sufixo no nome do arquivo (`estado.json` → `estado.json.anterior`).
pub fn com_sufixo(caminho: &Path, sufixo: &str) -> PathBuf {
    let mut nome = caminho.file_name().unwrap_or_default().to_os_string();
    nome.push(sufixo);
    caminho.with_file_name(nome)
}

/// Só "não existe" de verdade: um erro de leitura do disco não conta como ausência.
pub fn nao_existe(caminho: &Path) -> bool {
    matches!(fs::symlink_metadata(caminho), Err(e) if e.kind() == io::ErrorKind::NotFound)
}

/// Grava inteiro ou não grava: escreve `<nome>.tmp` com fsync, (com `anterior`) faz do `<nome>`
/// atual o `<nome>.anterior` por um link duro, troca pelo rename e faz fsync da pasta. Uma queda
/// em qualquer ponto deixa o arquivo velho inteiro ou o novo inteiro. Sem o link, só avisa.
pub fn gravar_seguro(caminho: &Path, bytes: &[u8], anterior: bool) -> Result<()> {
    let pasta = caminho.parent().context("caminho sem pasta")?;
    fs::create_dir_all(pasta).with_context(|| pasta.display().to_string())?;
    let temporario = escrever_temporario(caminho, bytes)?;
    if anterior {
        ligar_anterior(caminho);
    }
    fs::rename(&temporario, caminho).with_context(|| caminho.display().to_string())?;
    sincronizar_pasta(pasta)
}

pub fn gravar_json_seguro<T: Serialize>(caminho: &Path, valor: &T, anterior: bool) -> Result<()> {
    gravar_seguro(caminho, &serde_json::to_vec_pretty(valor)?, anterior)
}

pub fn gravar_texto(caminho: &Path, texto: &str) -> Result<()> {
    gravar_seguro(caminho, texto.as_bytes(), false)
}

/// Os mesmos bytes, gravados como `gravar_seguro` (o destino nunca fica pela metade).
pub fn copiar_com_fsync(origem: &Path, destino: &Path) -> Result<()> {
    let bytes = fs::read(origem).with_context(|| origem.display().to_string())?;
    gravar_seguro(destino, &bytes, false)
}

fn escrever_temporario(caminho: &Path, bytes: &[u8]) -> Result<PathBuf> {
    use std::io::Write as _;
    let temporario = com_sufixo(caminho, ".tmp");
    let mut arquivo =
        fs::File::create(&temporario).with_context(|| temporario.display().to_string())?;
    arquivo.write_all(bytes)?;
    arquivo.sync_all()?;
    Ok(temporario)
}

/// O link vai num temporário e entra por rename: em nenhum momento falta o `.anterior`. Sem o
/// arquivo (a primeira gravação, ou logo depois de restaurar do `.anterior`), o `.anterior` fica
/// como está: apagá-lo deixaria, numa queda antes do rename, a pasta sem estado nenhum.
fn ligar_anterior(caminho: &Path) {
    if nao_existe(caminho) {
        return;
    }
    let anterior = com_sufixo(caminho, ".anterior");
    let link = com_sufixo(&anterior, ".tmp");
    let _ = fs::remove_file(&link);
    let ligado = fs::hard_link(caminho, &link).and_then(|()| fs::rename(&link, &anterior));
    // Com o `.anterior` já sendo o mesmo arquivo (a queda veio logo depois do link), o rename não
    // faz nada e deixa o link.
    let _ = fs::remove_file(&link);
    if let Err(erro) = ligado {
        eprintln!("vitral: não guardou a versão anterior de {}: {erro}", caminho.display());
    }
}

pub fn sincronizar_pasta(pasta: &Path) -> Result<()> {
    fsync_da_pasta(pasta).with_context(|| format!("fsync de {}", pasta.display()))
}

/// O fsync da pasta, que grava no disco os renames de dentro dela. No Windows uma pasta não abre
/// como arquivo e não há o que fazer: no NTFS o rename é atômico e entra no diário do sistema de
/// arquivos.
pub fn fsync_da_pasta(pasta: &Path) -> io::Result<()> {
    if cfg!(windows) {
        return Ok(());
    }
    fs::File::open(pasta)?.sync_all()
}

/// fsync de cada arquivo e pasta abaixo de `raiz` (sem seguir links), e dela por último.
fn sincronizar_arvore(raiz: &Path) -> Result<()> {
    for entrada in fs::read_dir(raiz)? {
        let entrada = entrada?;
        let tipo = entrada.file_type()?;
        if tipo.is_dir() {
            sincronizar_arvore(&entrada.path())?;
        } else if tipo.is_file() {
            fsync_do_arquivo(&entrada.path())?;
        }
    }
    sincronizar_pasta(raiz)
}

/// fsync de um arquivo já escrito. No Windows o FlushFileBuffers pede o arquivo aberto para escrita,
/// e o que é só leitura (os objetos do git) fica como está.
fn fsync_do_arquivo(caminho: &Path) -> io::Result<()> {
    if cfg!(windows) {
        return match fs::OpenOptions::new().write(true).open(caminho) {
            Ok(arquivo) => arquivo.sync_all(),
            Err(erro) if erro.kind() == io::ErrorKind::PermissionDenied => Ok(()),
            Err(erro) => Err(erro),
        };
    }
    fs::File::open(caminho)?.sync_all()
}

fn ler_bytes(caminho: &Path) -> Result<Option<Vec<u8>>> {
    match fs::read(caminho) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(erro) if erro.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(erro) => Err(anyhow::Error::new(erro).context(caminho.display().to_string())),
    }
}

/// `Ok(None)`: não existe. Ilegível, guarda uma cópia `.ilegivel-<ts>.json` ao lado antes de
/// responder o erro, para a próxima gravação não levar o que havia.
pub fn ler_json_r<T: DeserializeOwned>(caminho: &Path) -> Result<Option<T>> {
    let Some(bytes) = ler_bytes(caminho)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|erro| {
        let copia = caminho.with_extension(format!("ilegivel-{}.json", agora()));
        let onde = match copiar_com_fsync(caminho, &copia) {
            Ok(()) => format!("; cópia em {}", copia.display()),
            Err(_) => String::new(),
        };
        anyhow::Error::new(erro).context(format!("{} ilegível{onde}", caminho.display()))
    })
}

/// Como `ler_json_r`, mas nunca escreve nada, nem a cópia: é o que lê as pastas antigas.
pub fn ler_valor_sem_escrever(caminho: &Path) -> Result<Option<Value>> {
    let Some(bytes) = ler_bytes(caminho)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|erro| anyhow::Error::new(erro).context(format!("{} ilegível", caminho.display())))
}

/// O erro é de conteúdo (JSON quebrado), não de leitura do disco.
pub fn e_ilegivel(erro: &anyhow::Error) -> bool {
    erro.downcast_ref::<serde_json::Error>().is_some()
}

/// Põe a configuração no formato dos dados globais: os modelos com os nomes novos (com as duas
/// chaves, vale a nova) e sem `projetos` e `rotinas`, que foram para os projetos e o estado. As
/// outras chaves ficam como vieram.
pub fn config_sem_legado(mut valor: Value) -> Value {
    if let Some(chaves) = valor.as_object_mut() {
        for (nova, antiga) in CHAVES_RENOMEADAS {
            if let Some(v) = chaves.remove(antiga)
                && !chaves.contains_key(nova)
            {
                chaves.insert(nova.to_string(), v);
            }
        }
        chaves.remove("projetos");
        chaves.remove("rotinas");
    }
    valor
}

/// Minúsculas sem acento, com um hífen no lugar de cada trecho que não é letra ou número.
fn sem_acento(texto: &str) -> String {
    let mut s = String::new();
    for c in texto.to_lowercase().chars() {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' => 'a',
            'é' | 'ê' => 'e',
            'í' => 'i',
            'ó' | 'ô' | 'õ' => 'o',
            'ú' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            s.push(c);
        } else if !s.ends_with('-') {
            s.push('-');
        }
    }
    s.trim_matches('-').to_string()
}

/// Para nomes de branch e de pasta: até 30 caracteres.
pub fn slug(texto: &str) -> String {
    sem_acento(texto).chars().take(30).collect()
}

/// A chave de um nome de projeto, inteira: com o corte do `slug`, dois nomes longos que só
/// diferem no fim teriam a mesma chave.
pub fn chave(nome: &str) -> String {
    sem_acento(nome)
}

/// A chave de um projeto novo ou renomeado, ou por que o nome não serve. `ignorar` é o id do
/// próprio projeto, na edição.
pub fn chave_valida(nome: &str, outros: &[Projeto], ignorar: Option<&str>) -> Result<String, String> {
    if nome.trim().is_empty() {
        return Err("Dê um nome ao projeto.".into());
    }
    let chave = chave(nome);
    if chave.is_empty() {
        return Err("Use pelo menos uma letra ou um número no nome.".into());
    }
    if CHAVES_RESERVADAS.contains(&chave.as_str()) {
        return Err("Nome reservado.".into());
    }
    if outros.iter().any(|p| Some(p.id.as_str()) != ignorar && p.chave() == chave) {
        return Err("Já existe um projeto com esse nome.".into());
    }
    Ok(chave)
}

/// A faixa de 10 portas que nenhuma outra tarefa usa (`41000 + k·10`); `None` quando acabam.
pub fn porta_livre(tarefas: &[Tarefa]) -> Option<u64> {
    let usadas: HashSet<u64> = tarefas.iter().map(|t| t.porta).filter(|&p| p != 0).collect();
    (0..400).map(|k| 41_000 + k * 10).find(|p| !usadas.contains(p))
}

/// A pasta onde um chat novo do projeto trabalha: a escolhida (uma das ligadas) ou a principal.
pub fn escolher_origem<'a>(projeto: &'a Projeto, pasta: Option<&Path>) -> Result<&'a PastaLigada, String> {
    if projeto.pastas.is_empty() {
        return Err("Ligue uma pasta a este projeto antes (Editar projeto).".into());
    }
    match pasta {
        None => Ok(&projeto.pastas[0]),
        Some(pasta) => {
            let canonica = dunce::canonicalize(pasta).ok();
            projeto
                .pastas
                .iter()
                .find(|l| l.caminho == pasta || canonica.as_deref() == Some(l.caminho.as_path()))
                .ok_or_else(|| format!("{} não é uma pasta do projeto {}.", pasta.display(), projeto.nome))
        }
    }
}

/// Como o chat trabalha, decidido com a pasta presente: a própria vai direto (com um commit por vez);
/// a raiz de um repositório com commit ganha cópia; o resto vai direto, sem git. Uma pasta dentro de
/// um repositório (sem ser a raiz) vai direto, com o aviso para ligar a raiz.
pub fn escolher_forma(propria: bool, usa_git: bool, topo: Option<&Path>, origem: &Path) -> (Forma, Option<String>) {
    if propria {
        return (Forma::DiretaGit, None);
    }
    if usa_git {
        return (Forma::Copia, None);
    }
    match topo.filter(|t| *t != origem) {
        Some(raiz) => (
            Forma::Direta,
            Some(format!("Esta pasta está dentro do repositório {}; ligue a raiz para ter cópia.", raiz.display())),
        ),
        None => (Forma::Direta, None),
    }
}

/// A pasta do chat antes do primeiro turno: a cópia (ainda por criar) em `copias/<projeto>/t<N>`, ou a
/// própria origem nas formas diretas.
pub fn cwd_inicial(forma: Forma, copias: &Path, tarefa: u64, origem: &Path) -> PathBuf {
    match forma {
        Forma::Copia => copias.join(format!("t{tarefa}")),
        Forma::DiretaGit | Forma::Direta => origem.to_path_buf(),
    }
}

/// O coordenador do Vitral antigo que nunca conversou: sem mensagens, sem tarefas e sem sessão do
/// Claude Code. Ele vira um chat de projeto.
pub fn coordenador_vazio(c: &Chat, tarefas: &[Tarefa], sessao_existe: bool) -> bool {
    c.papel == Papel::Coordenador
        && c.itens.is_empty()
        && !tarefas.iter().any(|t| t.coordenador == c.id)
        && !sessao_existe
}

/// O que a normalização pergunta ao disco e ao git: um falso nos testes.
pub trait Consultas {
    fn existe(&self, caminho: &Path) -> bool;
    /// O repositório de onde a cópia saiu.
    fn repositorio_da_copia(&self, copia: &Path) -> Option<PathBuf>;
    /// Os N das pastas `copias/*/t<N>` (também nas pastas que "Começar do zero" afastou).
    fn numeros_de_copias(&self, dados: &Dados) -> Vec<u64>;
    /// A pasta é a raiz de um repositório com commit.
    fn usa_git(&self, pasta: &Path) -> bool;
    fn topo_do_repositorio(&self, pasta: &Path) -> Option<PathBuf>;
    /// O Claude Code tem a sessão gravada (na conta principal; as extras compartilham `projects`).
    fn sessao_existe(&self, cwd: &Path, sessao: &str) -> bool;
}

pub struct NoDisco;

impl Consultas for NoDisco {
    fn existe(&self, caminho: &Path) -> bool {
        caminho.exists()
    }

    fn repositorio_da_copia(&self, copia: &Path) -> Option<PathBuf> {
        crate::git::repositorio_da_copia(copia)
    }

    fn usa_git(&self, pasta: &Path) -> bool {
        crate::git::usa_git(pasta)
    }

    fn topo_do_repositorio(&self, pasta: &Path) -> Option<PathBuf> {
        crate::git::topo_do_repositorio(pasta)
    }

    fn sessao_existe(&self, cwd: &Path, sessao: &str) -> bool {
        crate::claude::sessao_existe(None, cwd, sessao)
    }

    fn numeros_de_copias(&self, dados: &Dados) -> Vec<u64> {
        let mut areas = vec![dados.pasta.join("copias")];
        if let (Some(mae), Some(nome)) = (dados.pasta.parent(), dados.pasta.file_name()) {
            let prefixo = format!("{}.descartado-", nome.to_string_lossy());
            let afastadas = fs::read_dir(mae).into_iter().flatten().flatten();
            areas.extend(
                afastadas
                    .filter(|e| e.file_name().to_string_lossy().starts_with(&prefixo))
                    .map(|e| e.path().join("copias")),
            );
        }
        let pastas = |p: &Path| fs::read_dir(p).into_iter().flatten().flatten().map(|e| e.path());
        areas
            .iter()
            .flat_map(|a| pastas(a).collect::<Vec<_>>())
            .flat_map(|projeto| pastas(&projeto).collect::<Vec<_>>())
            .filter_map(|t| t.file_name()?.to_str()?.strip_prefix('t')?.parse().ok())
            .collect()
    }
}

/// O que a normalização mexeu: avisos, os chats cujas mensagens mudaram (gravar de novo) e os
/// chats vazios que ela tirou do estado (apagar o arquivo).
#[derive(Debug, Default)]
pub struct Normalizado {
    pub avisos: Vec<String>,
    pub chats_mexidos: Vec<String>,
    pub chats_a_apagar: Vec<String>,
}

impl Normalizado {
    fn mexeu(&mut self, chat: &str) {
        if !self.chats_mexidos.iter().any(|c| c == chat) {
            self.chats_mexidos.push(chat.to_string());
        }
    }
}

/// Cartão decidido há mais que isso sai do estado e vira uma linha na conversa.
const DIAS_DO_CARTAO: u64 = 30;

/// Põe o estado carregado nas regras dos dados globais, antes de qualquer turno: o Mestre existe,
/// todo projeto citado existe, as tarefas sabem a origem e têm portas, os cartões abertos
/// aparecem, o que é velho sai e o número da próxima tarefa não repete nenhum.
pub fn normalizar(estado: &mut Estado, dados: &Dados, consultas: &dyn Consultas, agora: u64) -> Normalizado {
    let mut n = Normalizado::default();
    garantir_mestre(estado, dados, &mut n);
    recriar_projetos(estado, dados, consultas, agora, &mut n);
    achar_origens(estado, consultas);
    converter_vazios(estado, dados, consultas, agora);
    converter_trabalhadores_antigos(estado, &mut n);
    distribuir_portas(estado);
    mostrar_cartoes(estado, &mut n);
    podar(estado, agora, &mut n);
    recolher_vazios(estado, dados, agora, &mut n);
    let maior = estado.tarefas.iter().map(|t| t.id).max().unwrap_or(0);
    let copias = consultas.numeros_de_copias(dados).into_iter().max().unwrap_or(0);
    estado.proxima_tarefa = estado.proxima_tarefa.max(maior + 1).max(copias + 1).max(1);
    n
}

fn garantir_mestre(estado: &mut Estado, dados: &Dados, n: &mut Normalizado) {
    if !estado.chats.iter().any(|c| c.id == ID_MESTRE) {
        let mestre = Chat::novo(ID_MESTRE.into(), Papel::Mestre, "", "Mestre", dados.pasta_mestre());
        estado.chats.insert(0, mestre);
        n.mexeu(ID_MESTRE);
    }
    let outros: Vec<String> = estado
        .chats
        .iter()
        .filter(|c| c.papel == Papel::Mestre && c.id != ID_MESTRE)
        .map(|c| c.id.clone())
        .collect();
    if !outros.is_empty() {
        n.avisos.push(format!("Havia mais de um Mestre; {} foi para as arquivadas.", outros.join(", ")));
    }
    for c in &mut estado.chats {
        if outros.contains(&c.id) {
            c.arquivado = true;
        }
    }
    for t in &mut estado.tarefas {
        if outros.contains(&t.coordenador) {
            t.coordenador = ID_MESTRE.into();
        }
    }
    for c in &mut estado.cartoes {
        if outros.contains(&c.chat) {
            c.chat = ID_MESTRE.into();
        }
    }
    if let Some(m) = estado.chats.iter_mut().find(|c| c.id == ID_MESTRE) {
        (m.papel, m.arquivado, m.fixado) = (Papel::Mestre, false, false);
        m.projeto = crate::modelo::SEM_PROJETO.into();
        if !m.sessao_criada {
            m.cwd = dados.pasta_mestre();
        }
    }
}

/// Um projeto citado por um chat, uma tarefa ou uma rotina que não está na lista volta como
/// provisório, com as pastas que as tarefas dele usavam: nada some por um projeto que faltou.
fn recriar_projetos(estado: &mut Estado, dados: &Dados, consultas: &dyn Consultas, agora: u64, n: &mut Normalizado) {
    let existentes: HashSet<String> = estado.projetos.iter().map(|p| p.id.clone()).collect();
    let mut citados: Vec<String> = estado
        .chats
        .iter()
        .map(|c| &c.projeto)
        .chain(estado.tarefas.iter().map(|t| &t.projeto))
        .chain(estado.rotinas.iter().map(|r| &r.projeto))
        .filter(|p| !p.is_empty() && !existentes.contains(*p))
        .cloned()
        .collect();
    citados.sort();
    citados.dedup();
    for id in citados {
        let mut pastas: Vec<PastaLigada> = Vec::new();
        for t in estado.tarefas.iter().filter(|t| t.projeto == id) {
            let vazia = t.origem.as_os_str().is_empty();
            if !vazia && consultas.existe(&t.origem) && !pastas.iter().any(|p| p.caminho == t.origem) {
                pastas.push(PastaLigada { caminho: t.origem.clone(), ..Default::default() });
            }
        }
        let propria = dados.pasta_projetos().join(slug(&id));
        if !slug(&id).is_empty() && consultas.existe(&propria) {
            pastas.push(PastaLigada { caminho: propria, propria: true, ..Default::default() });
        }
        let nome = (0..)
            .map(|i| match i {
                0 => id.clone(),
                1 => format!("{id} (recriado)"),
                i => format!("{id} (recriado {i})"),
            })
            .find(|nome| chave_valida(nome, &estado.projetos, None).is_ok() || chave(nome).is_empty() && *nome == id)
            .expect("sempre há um nome livre");
        let ordem = estado.projetos.iter().map(|p| p.ordem + 1).max().unwrap_or(0);
        n.avisos.push(format!("O projeto {nome} foi recriado: havia conversas dele."));
        estado.projetos.push(Projeto {
            id,
            nome,
            descricao: "Recriado: havia conversas deste projeto.".into(),
            pastas,
            ordem,
            criado_em: agora,
            provisorio: true,
            ..Default::default()
        });
    }
}

/// A tarefa sem origem gravada: a de uma cópia é o repositório dela; a direta, a pasta principal
/// do projeto. Sem nenhuma das duas, a junção recusa até alguém localizar a pasta.
fn achar_origens(estado: &mut Estado, consultas: &dyn Consultas) {
    for t in estado.tarefas.iter_mut().filter(|t| t.origem.as_os_str().is_empty()) {
        let da_copia = t.worktree.as_deref().and_then(|w| consultas.repositorio_da_copia(w));
        let principal = || {
            estado.projetos.iter().find(|p| p.id == t.projeto).and_then(|p| p.principal()).map(|p| p.caminho.clone())
        };
        match da_copia.or_else(principal) {
            Some(origem) => t.origem = origem,
            None if !t.estado.encerrada() => {
                t.falha_copia = Some("origem desconhecida: use Localizar pasta…".into());
            }
            None => {}
        }
    }
}

/// Os coordenadores do Vitral antigo que nunca conversaram viram chats de projeto, com a tarefa em
/// Conversa e a pasta como a de um chat novo (a cópia só nasce na primeira mensagem). O de um projeto
/// sem pasta à mão fica como está.
fn converter_vazios(estado: &mut Estado, dados: &Dados, consultas: &dyn Consultas, agora: u64) {
    let maior = estado.tarefas.iter().map(|t| t.id).max().unwrap_or(0);
    let copias = consultas.numeros_de_copias(dados).into_iter().max().unwrap_or(0);
    let mut proxima = estado.proxima_tarefa.max(maior + 1).max(copias + 1).max(1);
    for i in 0..estado.chats.len() {
        let c = &estado.chats[i];
        if !coordenador_vazio(c, &estado.tarefas, consultas.sessao_existe(&c.cwd, &c.sessao)) {
            continue;
        }
        let (forma, origem) = if c.projeto.is_empty() {
            (Forma::Direta, dados.pasta_avulso(&c.id))
        } else {
            let Some(pasta) = estado
                .projetos
                .iter()
                .find(|p| p.id == c.projeto)
                .and_then(|p| escolher_origem(p, None).ok())
                .filter(|p| consultas.existe(&p.caminho))
            else {
                continue;
            };
            let topo = consultas.topo_do_repositorio(&pasta.caminho);
            let (forma, _) = escolher_forma(pasta.propria, consultas.usa_git(&pasta.caminho), topo.as_deref(), &pasta.caminho);
            (forma, pasta.caminho.clone())
        };
        let id = proxima;
        proxima += 1;
        let (chat, projeto, titulo) = (c.id.clone(), c.projeto.clone(), c.titulo.clone());
        let cwd = cwd_inicial(forma, &dados.pasta_copias(&projeto), id, &origem);
        estado.tarefas.push(Tarefa {
            id,
            projeto,
            trabalhador: chat,
            titulo,
            estado: EstadoTarefa::Conversa,
            forma,
            origem,
            preparada: forma != Forma::Copia,
            criada_em: agora,
            atualizada_em: agora,
            ..Default::default()
        });
        let c = &mut estado.chats[i];
        (c.papel, c.tarefa, c.cwd, c.pai) = (Papel::Projeto, Some(id), cwd, None);
    }
    estado.proxima_tarefa = proxima;
}

/// O aviso no trabalhador do Vitral antigo que tinha um pedido aberto e virou chat de projeto.
pub const TRABALHADOR_ANTIGO: &str =
    "Este chat nasceu de uma conversa do Vitral antigo; o pedido agora é seu: continue a conversa ou use Juntar.";

/// Os trabalhadores do coordenador antigo (o pai não é um chat de projeto: esses são os ajudantes)
/// viram chats de projeto, sem dono. O pedido aberto (rodando ou em revisão) fica com o usuário, em
/// conversa; o encerrado fica como está e o chat vai para as arquivadas. O coordenador que conversou
/// fica como está, só para leitura.
fn converter_trabalhadores_antigos(estado: &mut Estado, n: &mut Normalizado) {
    let de_projeto: HashSet<String> =
        estado.chats.iter().filter(|c| c.papel == Papel::Projeto).map(|c| c.id.clone()).collect();
    for c in estado.chats.iter_mut().filter(|c| c.papel == Papel::Trabalhador) {
        if c.pai.as_ref().is_some_and(|p| de_projeto.contains(p)) {
            continue;
        }
        let Some(t) = estado
            .tarefas
            .iter_mut()
            .find(|t| c.tarefa.map_or(t.trabalhador == c.id, |n| n == t.id))
        else {
            continue;
        };
        if converter_trabalhador(c, t) {
            n.mexeu(&c.id);
        }
    }
}

/// Um trabalhador do Vitral antigo vira chat de projeto; diz se ganhou o aviso na conversa.
fn converter_trabalhador(c: &mut Chat, t: &mut Tarefa) -> bool {
    (c.papel, c.pai, c.tarefa) = (Papel::Projeto, None, Some(t.id));
    t.coordenador.clear();
    if t.estado.encerrada() {
        c.arquivado = true;
        return false;
    }
    t.estado = EstadoTarefa::Conversa;
    (t.entrega, t.entrega_pendente, t.progresso, t.parada) = (None, None, None, None);
    c.itens.push(Item::Aviso { texto: TRABALHADOR_ANTIGO.into(), erro: false });
    true
}

/// Tarefa em andamento de chat à vista ganha uma faixa de portas livre; a encerrada e a de chat
/// arquivado soltam a dela.
fn distribuir_portas(estado: &mut Estado) {
    let arquivados: HashSet<&str> = estado.projetos.iter().filter(|p| p.arquivado).map(|p| p.id.as_str()).collect();
    let a_vista: HashSet<&str> = estado
        .chats
        .iter()
        .filter(|c| !c.arquivado && !arquivados.contains(c.projeto.as_str()))
        .map(|c| c.id.as_str())
        .collect();
    let mut tarefas = std::mem::take(&mut estado.tarefas);
    for t in &mut tarefas {
        if t.estado.encerrada() || !a_vista.contains(t.trabalhador.as_str()) {
            t.porta = 0;
        }
    }
    for i in 0..tarefas.len() {
        let t = &tarefas[i];
        if t.porta == 0 && !t.estado.encerrada() && a_vista.contains(t.trabalhador.as_str()) {
            tarefas[i].porta = porta_livre(&tarefas).unwrap_or(0);
        }
    }
    estado.tarefas = tarefas;
}

/// Cartão aberto sem o item na conversa dele (uma queda entre gravar o estado e a conversa)
/// ganha o item no fim.
fn mostrar_cartoes(estado: &mut Estado, n: &mut Normalizado) {
    for cartao in estado.cartoes.iter().filter(|c| c.estado == EstadoCartao::Aberto) {
        let Some(chat) = estado.chats.iter_mut().find(|c| c.id == cartao.chat) else {
            continue;
        };
        if !chat.itens.iter().any(|i| matches!(i, Item::Cartao { id } if *id == cartao.id)) {
            chat.itens.push(Item::Cartao { id: cartao.id });
            n.mexeu(&chat.id);
        }
    }
}

/// Cartões decididos há mais de 30 dias saem e ficam como uma linha na conversa; as ocorrências
/// guardam só as últimas.
fn podar(estado: &mut Estado, agora: u64, n: &mut Normalizado) {
    let limite = agora.saturating_sub(DIAS_DO_CARTAO * 86_400);
    let velho = |c: &Cartao| {
        let quando = if c.decidido_em > 0 { c.decidido_em } else { c.criado_em };
        c.estado != EstadoCartao::Aberto && quando < limite
    };
    let (velhos, ficam): (Vec<Cartao>, Vec<Cartao>) = std::mem::take(&mut estado.cartoes).into_iter().partition(velho);
    estado.cartoes = ficam;
    for cartao in velhos {
        let Some(chat) = estado.chats.iter_mut().find(|c| c.id == cartao.chat) else {
            continue;
        };
        for item in chat.itens.iter_mut().filter(|i| matches!(i, Item::Cartao { id } if *id == cartao.id)) {
            *item = Item::Aviso { texto: resumo_do_cartao(&cartao), erro: false };
            n.mexeu(&chat.id);
        }
    }
    let sobra = estado.ocorrencias.len().saturating_sub(MAX_OCORRENCIAS);
    estado.ocorrencias.drain(..sobra);
}

/// A linha que fica no lugar de um cartão podado ("Divisão 3 começada em 12/10 · 2 partes").
pub fn resumo_do_cartao(c: &Cartao) -> String {
    let quando = crate::modelo::data_curta(if c.decidido_em > 0 { c.decidido_em } else { c.criado_em });
    let (aceito, a) = match c.estado {
        EstadoCartao::Aceito => ("aceito", "aceita"),
        EstadoCartao::Recusado => ("recusado", "recusada"),
        EstadoCartao::Substituido => ("substituído", "substituída"),
        EstadoCartao::Aberto => ("aberto", "aberta"),
    };
    match &c.tipo {
        TipoCartao::Divisao(d) => {
            let partes = d.partes.iter().filter(|p| !p.removida).count();
            let verbo = if c.estado == EstadoCartao::Aceito { "começada" } else { a };
            let partes = if partes == 1 { "1 parte".to_string() } else { format!("{partes} partes") };
            format!("Divisão {} {verbo} em {quando} · {partes}", c.id)
        }
        TipoCartao::Projeto(p) => format!("Sugestão do projeto {} {a} em {quando}", p.nome),
        TipoCartao::Memoria { texto } => format!("Memória {a} em {quando}: {texto}"),
        TipoCartao::Desconhecido(_) => format!("Cartão {} {aceito} em {quando}", c.id),
    }
}

/// Chats de projeto que nasceram e nunca receberam nada há mais de um dia saem, com a tarefa
/// (fora o último aberto, o fixado e o que veio da migração).
fn recolher_vazios(estado: &mut Estado, dados: &Dados, agora: u64, n: &mut Normalizado) {
    let interface = ler_valor_sem_escrever(&dados.config.join("interface.json")).ok().flatten();
    let ultimo = interface.as_ref().and_then(|i| i.get("ultimo_chat")?.as_str().map(str::to_string));
    let limite = agora.saturating_sub(86_400);
    let vazios: Vec<String> = estado
        .chats
        .iter()
        .filter(|c| {
            c.papel == Papel::Projeto
                && c.itens.is_empty()
                && !c.sessao_criada
                && !c.fixado
                && c.criado_por != "migracao"
                && c.criado_em < limite
                && ultimo.as_deref() != Some(c.id.as_str())
                && estado.filas.get(&c.id).is_none_or(Vec::is_empty)
        })
        .map(|c| c.id.clone())
        .collect();
    estado.chats.retain(|c| !vazios.contains(&c.id));
    estado.tarefas.retain(|t| !vazios.contains(&t.trabalhador));
    estado.filas.retain(|c, _| !vazios.contains(c));
    n.chats_a_apagar = vazios;
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::novo_id;

    fn pasta_de_teste(nome: &str) -> PathBuf {
        let pasta = std::env::temp_dir().join(format!("vitral-dados-{nome}-{}", novo_id()));
        fs::create_dir_all(&pasta).unwrap();
        pasta
    }

    #[test]
    fn release_estavel_so_com_os_dados_globais_ligados() {
        // Desligados, o Vitral sai na hora fora da bancada: um release assim não abriria.
        if option_env!("VITRAL_CANAL") == Some("estavel") {
            assert!(DADOS_GLOBAIS, "DADOS_GLOBAIS desligada num release estável");
        }
    }

    #[test]
    fn anterior_guarda_a_versao_de_antes() {
        let pasta = pasta_de_teste("anterior");
        let arquivo = pasta.join("estado.json");
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 1}), true).unwrap();
        assert!(nao_existe(&com_sufixo(&arquivo, ".anterior")));
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 2}), true).unwrap();
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 3}), true).unwrap();
        let ler = |c: &Path| ler_valor_sem_escrever(c).unwrap().unwrap()["n"].as_u64();
        assert_eq!(ler(&arquivo), Some(3));
        assert_eq!(ler(&com_sufixo(&arquivo, ".anterior")), Some(2));
        assert!(nao_existe(&com_sufixo(&arquivo, ".tmp")));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn queda_entre_o_link_e_o_rename_deixa_o_arquivo_legivel() {
        let pasta = pasta_de_teste("queda");
        let arquivo = pasta.join("estado.json");
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 1}), true).unwrap();
        // A fonte desliga depois do link e antes do rename.
        escrever_temporario(&arquivo, br#"{"n": 2}"#).unwrap();
        ligar_anterior(&arquivo);
        let ler = |c: &Path| ler_valor_sem_escrever(c).unwrap().unwrap()["n"].as_u64();
        assert_eq!(ler(&arquivo), Some(1));
        assert_eq!(ler(&com_sufixo(&arquivo, ".anterior")), Some(1));
        // Ao voltar, a próxima gravação passa por cima do temporário que sobrou.
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 3}), true).unwrap();
        assert_eq!(ler(&arquivo), Some(3));
        assert_eq!(ler(&com_sufixo(&arquivo, ".anterior")), Some(1));
        assert_eq!(fs::read_dir(&pasta).unwrap().count(), 2);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn falha_no_link_nao_impede_a_gravacao() {
        let pasta = pasta_de_teste("link");
        let arquivo = pasta.join("config.json");
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 1}), true).unwrap();
        // Uma pasta no lugar do `.anterior`: nem sai nem recebe o link.
        fs::create_dir(com_sufixo(&arquivo, ".anterior")).unwrap();
        gravar_json_seguro(&arquivo, &serde_json::json!({"n": 2}), true).unwrap();
        assert_eq!(ler_valor_sem_escrever(&arquivo).unwrap().unwrap()["n"], 2);
        assert!(com_sufixo(&arquivo, ".anterior").is_dir());
        assert!(nao_existe(&com_sufixo(&arquivo, ".anterior.tmp")));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn sem_o_arquivo_o_anterior_fica() {
        let pasta = pasta_de_teste("so-anterior");
        let dados = Dados::em(&pasta);
        fs::create_dir_all(&dados.pasta).unwrap();
        let anterior = com_sufixo(&dados.arquivo_estado(), ".anterior");
        fs::write(&anterior, estado_valido(2, 1)).unwrap();
        // Restaurado do `.anterior` (o estado.json estragado foi afastado)...
        let (estado, _) = dados.ler_estado().unwrap().unwrap();
        assert_eq!(estado.chats.len(), 1);
        // ...a fonte desliga antes do rename da gravação seguinte: o `.anterior` continua lá.
        escrever_temporario(&dados.arquivo_estado(), b"{}").unwrap();
        ligar_anterior(&dados.arquivo_estado());
        assert!(!dados.primeiro_uso());
        assert_eq!(fs::read_to_string(&anterior).unwrap(), estado_valido(2, 1));
        gravar_json_seguro(&dados.arquivo_estado(), &serde_json::json!({"n": 1}), true).unwrap();
        assert_eq!(fs::read_to_string(&anterior).unwrap(), estado_valido(2, 1));
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn copia_com_fsync_igual_byte_a_byte() {
        let pasta = pasta_de_teste("copia");
        let bytes: Vec<u8> = (0..=255).collect();
        fs::write(pasta.join("a.bin"), &bytes).unwrap();
        copiar_com_fsync(&pasta.join("a.bin"), &pasta.join("sub").join("b.bin")).unwrap();
        assert_eq!(fs::read(pasta.join("sub").join("b.bin")).unwrap(), bytes);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn ler_json_ausente_ilegivel_e_bom() {
        let pasta = pasta_de_teste("ler");
        let arquivo = pasta.join("contas.json");
        assert!(ler_json_r::<Value>(&arquivo).unwrap().is_none());
        fs::write(&arquivo, "{ quebrado").unwrap();
        let erro = ler_json_r::<Value>(&arquivo).unwrap_err();
        assert!(e_ilegivel(&erro), "{erro:#}");
        let copias: Vec<_> = fs::read_dir(&pasta)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().into_string().unwrap())
            .filter(|n| n.starts_with("contas.ilegivel-") && n.ends_with(".json"))
            .collect();
        assert_eq!(copias.len(), 1, "{copias:?}");
        fs::write(&arquivo, r#"{"ok": true}"#).unwrap();
        assert_eq!(ler_json_r::<Value>(&arquivo).unwrap().unwrap()["ok"], true);
        let _ = fs::remove_dir_all(&pasta);
    }

    #[test]
    fn ler_sem_escrever_nao_deixa_copia() {
        let pasta = pasta_de_teste("sem-escrever");
        fs::write(pasta.join("estado.json"), "[1,").unwrap();
        assert!(e_ilegivel(&ler_valor_sem_escrever(&pasta.join("estado.json")).unwrap_err()));
        assert_eq!(fs::read_dir(&pasta).unwrap().count(), 1);
        let _ = fs::remove_dir_all(&pasta);
    }

    fn tem_commit(pasta: &Path) -> bool {
        crate::git::novo_git()
            .arg("-C")
            .arg(pasta)
            .args(["rev-parse", "--verify", "-q", "HEAD"])
            .output()
            .is_ok_and(|s| s.status.success())
    }

    #[test]
    fn pasta_propria_com_acento_barra_repetida_e_simbolos() {
        let raiz = pasta_de_teste("propria");
        let dados = Dados::em(&raiz);
        let receitas = dados.criar_pasta_propria("Receitas da Vó").unwrap();
        assert_eq!(receitas, dados.pasta_projetos().join("receitas-da-vo"));
        assert!(tem_commit(&receitas));
        assert!(receitas.join(".gitignore").is_file());
        assert_eq!(
            dados.criar_pasta_propria("notas/2026").unwrap(),
            dados.pasta_projetos().join("notas-2026")
        );
        assert_eq!(
            dados.criar_pasta_propria("receitas da vó").unwrap(),
            dados.pasta_projetos().join("receitas-da-vo-2")
        );
        assert!(dados.criar_pasta_propria("!!! ???").is_err());
        let sobras: Vec<_> = fs::read_dir(dados.pasta_projetos())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".criando"))
            .collect();
        assert!(sobras.is_empty());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn pasta_propria_refaz_o_que_a_queda_deixou() {
        let raiz = pasta_de_teste("propria-queda");
        let dados = Dados::em(&raiz);
        // A fonte desligou no meio da criação: sobrou o `.criando`, sem git.
        let criando = dados.pasta_projetos().join(".loja.criando");
        fs::create_dir_all(&criando).unwrap();
        fs::write(criando.join("pela-metade.txt"), "x").unwrap();
        let loja = dados.criar_pasta_propria("Loja").unwrap();
        assert_eq!(loja, dados.pasta_projetos().join("loja"));
        assert!(tem_commit(&loja));
        assert!(!loja.join("pela-metade.txt").exists());
        assert!(nao_existe(&criando));
        let _ = fs::remove_dir_all(&raiz);
    }

    fn projeto(id: &str, nome: &str) -> Projeto {
        Projeto { id: id.into(), nome: nome.into(), ..Default::default() }
    }

    #[test]
    fn chave_de_projeto() {
        let outros = [projeto("p1", "Loja"), projeto("p2", "Receitas da Vó")];
        assert_eq!(chave_valida("  Site  ", &outros, None), Ok("site".into()));
        let repetido = Err("Já existe um projeto com esse nome.".to_string());
        assert_eq!(chave_valida("loja", &outros, None), repetido);
        assert_eq!(chave_valida("LÓJA", &outros, None), repetido);
        assert_eq!(chave_valida("receitas da vo", &outros, None), repetido);
        // Na edição, o próprio projeto não conta.
        assert_eq!(chave_valida("LOJA", &outros, Some("p1")), Ok("loja".into()));
        assert_eq!(chave_valida("Mestre", &outros, None), Err("Nome reservado.".into()));
        assert_eq!(chave_valida("avulsos", &outros, None), Err("Nome reservado.".into()));
        assert_eq!(chave_valida("   ", &outros, None), Err("Dê um nome ao projeto.".into()));
        assert!(chave_valida("!!!", &outros, None).is_err());
        // Nomes longos que só diferem no fim continuam diferentes.
        let longo = [projeto("p3", "verificacao-leads-apoio-cliente-a")];
        assert!(chave_valida("verificacao-leads-apoio-cliente-b", &longo, None).is_ok());
    }

    #[test]
    fn slug_igual_ao_dos_branches() {
        assert_eq!(slug("Corrigir o preço do cupom!"), "corrigir-o-preco-do-cupom");
        assert_eq!(slug("  --Ação  "), "acao");
        assert_eq!(slug(&"a".repeat(40)).len(), 30);
    }

    #[test]
    fn primeiro_uso_conta_o_anterior() {
        let raiz = pasta_de_teste("primeiro");
        let dados = Dados::em(&raiz);
        assert!(dados.primeiro_uso());
        fs::create_dir_all(&dados.pasta).unwrap();
        fs::write(com_sufixo(&dados.arquivo_estado(), ".anterior"), "{}").unwrap();
        assert!(!dados.primeiro_uso());
        let _ = fs::remove_dir_all(&raiz);
    }

    fn estado_valido(versao: u32, chats: usize) -> String {
        let chats: Vec<Value> = (0..chats)
            .map(|i| {
                serde_json::json!({
                    "id": format!("c{i}"), "projeto": "loja", "titulo": "t", "papel": "Coordenador",
                    "pai": null, "tarefa": null, "sessao": "s", "sessao_criada": false,
                    "cwd": "/x/loja", "modelo": null, "criado_em": 0, "atualizado_em": 0,
                    "custo_usd": 0.0
                })
            })
            .collect();
        serde_json::json!({"versao": versao, "chats": chats, "tarefas": [], "proxima_tarefa": 1})
            .to_string()
    }

    #[test]
    fn estado_estragado_volta_do_anterior_e_e_afastado() {
        let raiz = pasta_de_teste("restaurar");
        let dados = Dados::em(&raiz);
        assert!(matches!(dados.ler_estado(), Ok(None)));
        fs::create_dir_all(&dados.pasta).unwrap();
        fs::write(dados.arquivo_estado(), "{\"chats\": [").unwrap();
        fs::write(com_sufixo(&dados.arquivo_estado(), ".anterior"), estado_valido(2, 2)).unwrap();
        let (estado, avisos) = dados.ler_estado().unwrap().unwrap();
        assert_eq!(estado.chats.len(), 2);
        assert_eq!(avisos.len(), 1);
        // O estragado saiu do caminho: a próxima gravação liga o bom como `.anterior`.
        assert!(nao_existe(&dados.arquivo_estado()));
        let afastados = fs::read_dir(&dados.pasta)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with("estado.ilegivel-"))
            .count();
        assert_eq!(afastados, 1);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn estado_de_versao_mais_nova_nao_abre() {
        let raiz = pasta_de_teste("versao");
        let dados = Dados::em(&raiz);
        fs::create_dir_all(&dados.pasta).unwrap();
        fs::write(dados.arquivo_estado(), estado_valido(VERSAO_ESTADO + 1, 0)).unwrap();
        assert_eq!(
            dados.ler_estado().err(),
            Some(ErroCarga::VersaoNova { arquivo: dados.arquivo_estado(), versao: VERSAO_ESTADO + 1 })
        );
        let _ = fs::remove_dir_all(&raiz);
    }

    // O link `atual` do atualizador é do Linux.
    #[cfg(unix)]
    #[test]
    fn descartar_afasta_os_dados_e_deixa_o_app_instalado() {
        let raiz = pasta_de_teste("descartar");
        let dados = Dados::em(&raiz);
        fs::create_dir_all(dados.pasta.join("chats")).unwrap();
        fs::write(dados.arquivo_estado(), "{}").unwrap();
        fs::write(dados.arquivo_formato(), "2\n").unwrap();
        // O que o atualizador põe na mesma pasta: as versões, o link e o estado.json dele.
        let app = dados.pasta.join(PASTA_DO_APP);
        fs::create_dir_all(app.join("0.2.0")).unwrap();
        fs::write(app.join("0.2.0/vitral"), "binário").unwrap();
        std::os::unix::fs::symlink("0.2.0", app.join("atual")).unwrap();
        fs::write(app.join("estado.json"), r#"{"recusadas": []}"#).unwrap();
        let afastada = dados.descartar_tudo().unwrap();
        assert!(afastada.join("estado.json").is_file());
        assert!(afastada.join("chats").is_dir());
        assert!(afastada.join("formato").is_file());
        assert!(!afastada.join(PASTA_DO_APP).exists());
        assert!(afastada.file_name().unwrap().to_string_lossy().starts_with("dados.descartado-"));
        let ficou: Vec<_> = fs::read_dir(&dados.pasta).unwrap().flatten().map(|e| e.file_name()).collect();
        assert_eq!(ficou, [PASTA_DO_APP]);
        assert_eq!(fs::read_to_string(app.join("atual/vitral")).unwrap(), "binário");
        assert_eq!(fs::read_to_string(app.join("estado.json")).unwrap(), r#"{"recusadas": []}"#);
        assert!(dados.primeiro_uso());
        assert_eq!(dados.formato_no_disco(), 1);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn o_estado_do_atualizador_nao_e_o_dos_dados() {
        let raiz = pasta_de_teste("dois-estados");
        let dados = Dados::em(&raiz);
        let app = crate::atualizacao::instalador::Raiz { dados: raiz.clone(), bin: None }.app();
        let dados_no_mesmo_lugar = Dados { pasta: raiz.join("vitral"), ..dados.clone() };
        assert_eq!(app, dados_no_mesmo_lugar.pasta.join(PASTA_DO_APP));
        assert_ne!(app.join("estado.json"), dados_no_mesmo_lugar.arquivo_estado());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn formato_dos_dados_para_a_volta_de_versao() {
        let raiz = pasta_de_teste("formato");
        let dados = Dados::em(&raiz);
        // Sem nada (só o Vitral antigo), o 1; com estado gravado e sem o arquivo, o dos dados globais.
        assert_eq!(dados.formato_no_disco(), 1);
        fs::create_dir_all(&dados.pasta).unwrap();
        fs::write(dados.arquivo_estado(), "{}").unwrap();
        assert_eq!(dados.formato_no_disco(), 2);
        dados.marcar_formato();
        assert_eq!(fs::read_to_string(dados.arquivo_formato()).unwrap(), format!("{FORMATO_DADOS}\n"));
        assert_eq!(dados.formato_no_disco(), FORMATO_DADOS);
        // Uma versão mais nova subiu o número: esta não abaixa.
        fs::write(dados.arquivo_formato(), "7\n").unwrap();
        dados.marcar_formato();
        assert_eq!(dados.formato_no_disco(), 7);
        fs::write(dados.arquivo_formato(), "lixo").unwrap();
        assert_eq!(dados.formato_no_disco(), 2);
        dados.marcar_formato();
        assert_eq!(dados.formato_no_disco(), FORMATO_DADOS);
        // A 0.2.0-rc.2 instalada (formato 1) não é destino de volta com os dados globais no disco.
        use crate::atualizacao::{estado_app::EstadoApp, saude::pode_voltar};
        let app = EstadoApp { formatos: [("0.2.0-rc.2".to_string(), 1)].into(), ..Default::default() };
        assert!(!pode_voltar(&app, "0.2.0-rc.2", dados.formato_no_disco()));
        assert!(pode_voltar(&app, "0.2.0-rc.2", Dados::em(&raiz.join("vazia")).formato_no_disco()));
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn caminhos_dos_testes_ficam_na_pasta_temporaria() {
        let raiz = pasta_de_teste("caminhos");
        let dados = Dados::em(&raiz);
        for caminho in [
            dados.arquivo_estado(),
            dados.arquivo_config(),
            dados.arquivo_trava(),
            dados.arquivo_socket(),
            dados.arquivo_prompt("mestre"),
            dados.arquivo_memoria(),
        ] {
            assert!(caminho.starts_with(&raiz), "{}", caminho.display());
        }
        assert_eq!(dados.arquivo_trava(), raiz.join("vitral.trava"));
        let _ = fs::remove_dir_all(&raiz);
    }

    struct Falso {
        existem: Vec<PathBuf>,
        repositorios: Vec<(PathBuf, PathBuf)>,
        copias: Vec<u64>,
        com_git: Vec<PathBuf>,
        sessoes: Vec<String>,
    }

    impl Consultas for Falso {
        fn existe(&self, caminho: &Path) -> bool {
            self.existem.iter().any(|e| e == caminho)
        }
        fn repositorio_da_copia(&self, copia: &Path) -> Option<PathBuf> {
            self.repositorios.iter().find(|(c, _)| c == copia).map(|(_, r)| r.clone())
        }
        fn numeros_de_copias(&self, _: &Dados) -> Vec<u64> {
            self.copias.clone()
        }
        fn usa_git(&self, pasta: &Path) -> bool {
            self.com_git.iter().any(|g| g == pasta)
        }
        fn topo_do_repositorio(&self, pasta: &Path) -> Option<PathBuf> {
            self.usa_git(pasta).then(|| pasta.to_path_buf())
        }
        fn sessao_existe(&self, _: &Path, sessao: &str) -> bool {
            self.sessoes.iter().any(|s| s == sessao)
        }
    }

    fn falso() -> Falso {
        Falso { existem: Vec::new(), repositorios: Vec::new(), copias: Vec::new(), com_git: Vec::new(), sessoes: Vec::new() }
    }

    fn chat(id: &str, papel: Papel, projeto: &str) -> Chat {
        let mut c = Chat::novo(id.into(), papel, projeto, id, PathBuf::from("/x"));
        c.criado_em = 0;
        c
    }

    fn tarefa(id: u64, projeto: &str, trabalhador: &str) -> Tarefa {
        Tarefa { id, projeto: projeto.into(), trabalhador: trabalhador.into(), ..Default::default() }
    }

    const AGORA: u64 = 100 * 86_400;

    #[test]
    fn orfao_vira_provisorio_com_as_pastas_das_origens() {
        let raiz = pasta_de_teste("orfao");
        let dados = Dados::em(&raiz);
        let mut estado = Estado {
            projetos: vec![Projeto { id: "loja".into(), nome: "loja".into(), ordem: 4, ..Default::default() }],
            chats: vec![chat("c1", Papel::Coordenador, "site"), chat("c2", Papel::Coordenador, "loja")],
            tarefas: vec![
                Tarefa { origem: "/p/site".into(), ..tarefa(1, "site", "w1") },
                Tarefa { origem: "/p/site".into(), ..tarefa(2, "site", "w2") },
                Tarefa { origem: "/sumiu".into(), ..tarefa(3, "site", "w3") },
            ],
            rotinas: vec![crate::modelo::Rotina { id: "r".into(), projeto: "blog".into(), ..Default::default() }],
            ..Default::default()
        };
        let propria = dados.pasta_projetos().join("site");
        let consultas = Falso { existem: vec!["/p/site".into(), propria.clone()], ..falso() };
        let n = normalizar(&mut estado, &dados, &consultas, AGORA);
        let site = estado.projetos.iter().find(|p| p.id == "site").unwrap();
        assert!(site.provisorio && site.nome == "site" && site.ordem == 6);
        let pastas: Vec<(&Path, bool)> = site.pastas.iter().map(|p| (p.caminho.as_path(), p.propria)).collect();
        assert_eq!(pastas, [(Path::new("/p/site"), false), (propria.as_path(), true)]);
        let blog = estado.projetos.iter().find(|p| p.id == "blog").unwrap();
        assert!(blog.provisorio && blog.pastas.is_empty());
        assert_eq!(estado.projetos.len(), 3);
        assert_eq!(n.avisos.iter().filter(|a| a.contains("recriado")).count(), 2);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn proxima_tarefa_passa_das_copias_e_das_tarefas() {
        let dados = Dados::em(Path::new("/nao/existe"));
        let mut estado = Estado { tarefas: vec![tarefa(7, "", "w")], proxima_tarefa: 3, ..Default::default() };
        normalizar(&mut estado, &dados, &Falso { copias: vec![12, 4], ..falso() }, AGORA);
        assert_eq!(estado.proxima_tarefa, 13);
        normalizar(&mut estado, &dados, &falso(), AGORA);
        assert_eq!(estado.proxima_tarefa, 13);
        let mut vazio = Estado::default();
        normalizar(&mut vazio, &dados, &falso(), AGORA);
        assert_eq!(vazio.proxima_tarefa, 1);
    }

    #[test]
    fn mestre_existe_e_o_duplicado_e_arquivado_e_reapontado() {
        let dados = Dados::em(Path::new("/d"));
        let mut estado = Estado::default();
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        let mestre = &estado.chats[0];
        assert_eq!((mestre.id.as_str(), mestre.papel, mestre.projeto.as_str()), (ID_MESTRE, Papel::Mestre, ""));
        assert_eq!(mestre.cwd, dados.pasta_mestre());
        assert_eq!(n.chats_mexidos, [ID_MESTRE]);
        // De novo: nada muda.
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        assert!(n.chats_mexidos.is_empty() && estado.chats.len() == 1);

        let mut velho = chat("m2", Papel::Mestre, "");
        velho.arquivado = false;
        estado.chats.push(velho);
        estado.tarefas.push(Tarefa { coordenador: "m2".into(), ..tarefa(1, "", "w") });
        estado.cartoes.push(Cartao {
            id: 1,
            chat: "m2".into(),
            criado_em: AGORA,
            decidido_em: 0,
            estado: EstadoCartao::Aberto,
            tipo: TipoCartao::Memoria { texto: "x".into() },
            resto: Default::default(),
        });
        estado.chats[0].arquivado = true;
        normalizar(&mut estado, &dados, &falso(), AGORA);
        assert!(!estado.chats[0].arquivado);
        assert!(estado.chats.iter().find(|c| c.id == "m2").unwrap().arquivado);
        assert_eq!(estado.tarefas[0].coordenador, ID_MESTRE);
        assert_eq!(estado.cartoes[0].chat, ID_MESTRE);
        // O cartão aberto ganhou o item no Mestre.
        assert!(matches!(estado.chats[0].itens.as_slice(), [Item::Cartao { id: 1 }]));
    }

    #[test]
    fn cartao_aberto_sem_item_ganha_item_e_o_velho_vira_linha() {
        let dados = Dados::em(Path::new("/d"));
        let cartao = |id, estado, decidido_em| Cartao {
            id,
            chat: ID_MESTRE.into(),
            criado_em: 1,
            decidido_em,
            estado,
            tipo: TipoCartao::Divisao(crate::modelo::Divisao {
                partes: vec![Default::default(), Default::default()],
                ..Default::default()
            }),
            resto: Default::default(),
        };
        let mut mestre = chat(ID_MESTRE, Papel::Mestre, "");
        mestre.itens = vec![Item::Cartao { id: 2 }, Item::Cartao { id: 3 }];
        let mut estado = Estado {
            chats: vec![mestre],
            cartoes: vec![
                cartao(1, EstadoCartao::Aberto, 0),
                cartao(2, EstadoCartao::Aceito, AGORA - 31 * 86_400),
                cartao(3, EstadoCartao::Recusado, AGORA - 86_400),
            ],
            ocorrencias: (0..MAX_OCORRENCIAS as u64 + 5)
                .map(|q| crate::modelo::Ocorrencia { quando: q, ..Default::default() })
                .collect(),
            ..Default::default()
        };
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        assert_eq!(estado.cartoes.iter().map(|c| c.id).collect::<Vec<_>>(), [1, 3]);
        let itens = &estado.chats[0].itens;
        assert!(matches!(&itens[0], Item::Aviso { texto, .. } if texto.starts_with("Divisão 2 começada em ") && texto.ends_with(" · 2 partes")), "{itens:?}");
        assert!(matches!(itens[1], Item::Cartao { id: 3 }));
        assert!(matches!(itens[2], Item::Cartao { id: 1 }));
        assert_eq!(n.chats_mexidos, [ID_MESTRE]);
        assert_eq!(estado.ocorrencias.len(), MAX_OCORRENCIAS);
        assert_eq!(estado.ocorrencias[0].quando, 5);
    }

    #[test]
    fn origem_vem_da_copia_ou_da_pasta_principal() {
        let dados = Dados::em(Path::new("/d"));
        let mut estado = Estado {
            projetos: vec![Projeto {
                id: "loja".into(),
                nome: "loja".into(),
                pastas: vec![PastaLigada { caminho: "/p/loja".into(), ..Default::default() }],
                ..Default::default()
            }],
            tarefas: vec![
                Tarefa { worktree: Some("/v/wt/t1".into()), ..tarefa(1, "loja", "w1") },
                tarefa(2, "loja", "w2"),
                tarefa(3, "sem", "w3"),
                Tarefa { origem: "/outra".into(), ..tarefa(4, "loja", "w4") },
            ],
            ..Default::default()
        };
        let consultas = Falso { repositorios: vec![("/v/wt/t1".into(), "/repo".into())], ..falso() };
        normalizar(&mut estado, &dados, &consultas, AGORA);
        let origem = |id: u64| estado.tarefas.iter().find(|t| t.id == id).unwrap().origem.clone();
        assert_eq!(origem(1), PathBuf::from("/repo"));
        assert_eq!(origem(2), PathBuf::from("/p/loja"));
        assert_eq!(origem(4), PathBuf::from("/outra"));
        let sem = estado.tarefas.iter().find(|t| t.id == 3).unwrap();
        assert!(sem.origem.as_os_str().is_empty() && sem.falha_copia.is_some());
    }

    #[test]
    fn portas_livres_soltas_ao_arquivar_e_quando_acabam() {
        let dados = Dados::em(Path::new("/d"));
        let mut arquivado = chat("w3", Papel::Trabalhador, "p");
        arquivado.arquivado = true;
        let mut estado = Estado {
            chats: vec![chat("w1", Papel::Trabalhador, "p"), chat("w2", Papel::Trabalhador, "p"), arquivado],
            tarefas: vec![
                Tarefa { porta: 41_000, ..tarefa(1, "p", "w1") },
                tarefa(2, "p", "w2"),
                Tarefa { porta: 41_020, ..tarefa(3, "p", "w3") },
            ],
            ..Default::default()
        };
        normalizar(&mut estado, &dados, &falso(), AGORA);
        let portas: Vec<u64> = estado.tarefas.iter().map(|t| t.porta).collect();
        assert_eq!(portas, [41_000, 41_010, 0]);
        let cheias: Vec<Tarefa> = (0..400).map(|k| Tarefa { porta: 41_000 + k * 10, ..tarefa(k, "p", "w") }).collect();
        assert_eq!(porta_livre(&cheias), None);
        assert_eq!(porta_livre(&cheias[1..]), Some(41_000));
    }

    #[test]
    fn chat_de_projeto_vazio_e_velho_sai_e_os_outros_ficam() {
        let raiz = pasta_de_teste("vazios");
        let dados = Dados::em(&raiz);
        let mut fixado = chat("f", Papel::Projeto, "p");
        fixado.fixado = true;
        let mut migrado = chat("m", Papel::Projeto, "p");
        migrado.criado_por = "migracao".into();
        let mut novo = chat("n", Papel::Projeto, "p");
        novo.criado_em = AGORA;
        let mut com_itens = chat("i", Papel::Projeto, "p");
        com_itens.itens.push(Item::Assistente { texto: "oi".into() });
        let mut estado = Estado {
            chats: vec![chat("v", Papel::Projeto, "p"), chat("u", Papel::Projeto, "p"), fixado, migrado, novo, com_itens],
            tarefas: vec![tarefa(1, "p", "v"), tarefa(2, "p", "u")],
            ..Default::default()
        };
        fs::create_dir_all(&dados.config).unwrap();
        fs::write(dados.config.join("interface.json"), r#"{"ultimo_chat": "u"}"#).unwrap();
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        assert_eq!(n.chats_a_apagar, ["v"]);
        assert!(!estado.chats.iter().any(|c| c.id == "v"));
        assert_eq!(estado.tarefas.iter().map(|t| t.id).collect::<Vec<_>>(), [2]);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn config_com_o_nome_antigo_le_e_grava_sem_o_legado() {
        let raiz = pasta_de_teste("config");
        let dados = Dados::em(&raiz);
        fs::create_dir_all(&dados.config).unwrap();
        fs::write(
            dados.arquivo_config(),
            r#"{"modelo_coordenador": "opus", "modelo_mestre": "sonnet", "modelo_trabalhador": "haiku",
                "projetos": {"a": {}}, "rotinas": [], "de_uma_versao_nova": 7}"#,
        )
        .unwrap();
        let carregado = dados.carregar().unwrap();
        assert!(carregado.avisos.is_empty(), "{:?}", carregado.avisos);
        assert_eq!((carregado.config.modelo_mestre.as_str(), carregado.config.modelo_chats.as_str()), ("sonnet", "haiku"));
        dados.salvar_config(&carregado.config).unwrap();
        dados.esperar_gravacoes();
        let gravado = ler_valor_sem_escrever(&dados.arquivo_config()).unwrap().unwrap();
        for legada in ["modelo_coordenador", "modelo_trabalhador", "projetos", "rotinas"] {
            assert!(gravado.get(legada).is_none(), "{legada}");
        }
        assert_eq!(gravado["de_uma_versao_nova"], 7);
        assert_eq!(gravado["modelo_chats"], "haiku");
        // Só o nome antigo: vai para o novo.
        fs::write(dados.arquivo_config(), r#"{"modelo_coordenador": "opus"}"#).unwrap();
        assert_eq!(dados.carregar().unwrap().config.modelo_mestre, "opus");
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn carregar_le_os_itens_um_a_um_e_grava_o_estado_com_anterior() {
        let raiz = pasta_de_teste("carregar");
        let dados = Dados::em(&raiz);
        let primeiro = dados.carregar().unwrap();
        assert_eq!((primeiro.estado.versao, primeiro.estado.proxima_tarefa), (VERSAO_ESTADO, 1));
        let mut estado = primeiro.estado;
        let mut c = chat("c1", Papel::Coordenador, "");
        c.itens = vec![Item::Assistente { texto: "oi".into() }];
        estado.chats.push(c);
        dados.salvar_estado(&estado.por_referencia()).unwrap();
        dados.salvar_chat(&estado.chats[0]).unwrap();
        dados.esperar_gravacoes();
        let futuro = serde_json::json!([{"Assistente": {"texto": "oi"}}, {"Coisa": {"nova": true}}]);
        fs::write(dados.arquivo_chat("c1"), futuro.to_string()).unwrap();
        estado.proxima_tarefa = 5;
        dados.salvar_estado(&estado.por_referencia()).unwrap();
        dados.esperar_gravacoes();
        let lido = dados.carregar().unwrap();
        assert_eq!(lido.estado.proxima_tarefa, 5);
        assert!(matches!(&lido.estado.chats[0].itens[1], Item::Desconhecido(v) if v["Coisa"]["nova"] == true));
        let anterior = ler_valor_sem_escrever(&com_sufixo(&dados.arquivo_estado(), ".anterior")).unwrap().unwrap();
        assert_eq!(anterior["proxima_tarefa"], 1);
        dados.salvar_chat(&lido.estado.chats[0]).unwrap();
        dados.esperar_gravacoes();
        assert_eq!(ler_valor_sem_escrever(&dados.arquivo_chat("c1")).unwrap().unwrap(), futuro);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn gravacao_que_falha_fica_guardada_ate_dar_certo() {
        let raiz = pasta_de_teste("falha");
        let dados = Dados::em(&raiz);
        // A pasta de dados é um arquivo: nada grava lá dentro.
        fs::write(&dados.pasta, "").unwrap();
        dados.salvar_estado(&Estado::default().por_referencia()).unwrap();
        dados.esperar_gravacoes();
        assert!(dados.falha_de_gravacao().is_some_and(|f| f.contains("estado.json")));
        fs::remove_file(&dados.pasta).unwrap();
        dados.gravar_de_novo();
        dados.esperar_gravacoes();
        assert_eq!(dados.falha_de_gravacao(), None);
        assert!(dados.arquivo_estado().is_file());
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn notas_atomicas_e_lembradas() {
        let raiz = pasta_de_teste("notas");
        let dados = Dados::em(&raiz);
        assert_eq!(dados.notas("loja"), "");
        dados.anotar("loja", "usar Decimal para preços").unwrap();
        dados.anotar("loja", " e centavos ").unwrap();
        assert_eq!(dados.notas("loja"), "- usar Decimal para preços\n- e centavos\n");
        assert!(nao_existe(&com_sufixo(&dados.arquivo_notas("loja"), ".tmp")));
        dados.apagar_notas("loja");
        dados.esperar_gravacoes();
        assert_eq!(dados.notas("loja"), "");
        dados.gravar_memoria("- pagamentos na Ninee\n").unwrap();
        assert_eq!(dados.memoria(), "- pagamentos na Ninee\n");
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    fn origem_forma_e_pasta_de_um_chat_novo() {
        let loja = Projeto {
            id: "loja".into(),
            nome: "Loja".into(),
            pastas: vec![
                PastaLigada { caminho: "/p/loja".into(), ..Default::default() },
                PastaLigada { caminho: "/p/docs".into(), ..Default::default() },
            ],
            ..Default::default()
        };
        assert_eq!(escolher_origem(&loja, None).unwrap().caminho, PathBuf::from("/p/loja"));
        assert_eq!(escolher_origem(&loja, Some(Path::new("/p/docs"))).unwrap().caminho, PathBuf::from("/p/docs"));
        assert!(escolher_origem(&loja, Some(Path::new("/p/outra"))).unwrap_err().contains("não é uma pasta do projeto Loja"));
        let provisorio = Projeto { provisorio: true, ..projeto("x", "x") };
        assert_eq!(escolher_origem(&provisorio, None).unwrap_err(), "Ligue uma pasta a este projeto antes (Editar projeto).");

        let origem = Path::new("/p/loja");
        assert_eq!(escolher_forma(true, false, None, origem), (Forma::DiretaGit, None));
        assert_eq!(escolher_forma(false, true, Some(origem), origem), (Forma::Copia, None));
        assert_eq!(escolher_forma(false, false, None, origem), (Forma::Direta, None));
        let (forma, aviso) = escolher_forma(false, false, Some(Path::new("/p")), origem);
        assert_eq!(forma, Forma::Direta);
        assert_eq!(aviso.as_deref(), Some("Esta pasta está dentro do repositório /p; ligue a raiz para ter cópia."));

        let copias = Path::new("/d/copias/loja");
        assert_eq!(cwd_inicial(Forma::Copia, copias, 12, origem), PathBuf::from("/d/copias/loja/t12"));
        assert_eq!(cwd_inicial(Forma::DiretaGit, copias, 12, origem), origem);
        assert_eq!(cwd_inicial(Forma::Direta, copias, 12, origem), origem);
    }

    #[test]
    fn coordenador_vazio_vira_chat_de_projeto() {
        let dados = Dados::em(Path::new("/d"));
        let com_conversa = {
            let mut c = chat("falou", Papel::Coordenador, "loja");
            c.itens.push(Item::Assistente { texto: "oi".into() });
            c
        };
        let com_sessao = Chat { sessao: "s-gravada".into(), ..chat("sessao", Papel::Coordenador, "loja") };
        let mut estado = Estado {
            projetos: vec![
                Projeto {
                    id: "loja".into(),
                    nome: "loja".into(),
                    pastas: vec![PastaLigada { caminho: "/p/loja".into(), ..Default::default() }],
                    ..Default::default()
                },
                projeto("vazio", "vazio"),
            ],
            chats: vec![
                Chat { criado_por: "migracao".into(), ..chat("novo", Papel::Coordenador, "loja") },
                Chat { criado_por: "migracao".into(), ..chat("solto", Papel::Coordenador, "") },
                chat("com-tarefa", Papel::Coordenador, "loja"),
                chat("sem-pasta", Papel::Coordenador, "vazio"),
                com_conversa,
                com_sessao,
            ],
            tarefas: vec![Tarefa { coordenador: "com-tarefa".into(), ..tarefa(4, "loja", "w") }],
            proxima_tarefa: 2,
            ..Default::default()
        };
        assert!(coordenador_vazio(&estado.chats[0], &estado.tarefas, false));
        assert!(!coordenador_vazio(&estado.chats[0], &estado.tarefas, true));
        assert!(!coordenador_vazio(&estado.chats[2], &estado.tarefas, false));
        assert!(!coordenador_vazio(&estado.chats[4], &estado.tarefas, false));
        let consultas = Falso {
            existem: vec!["/p/loja".into()],
            com_git: vec!["/p/loja".into()],
            sessoes: vec!["s-gravada".into()],
            ..falso()
        };
        normalizar(&mut estado, &dados, &consultas, AGORA);
        let papel = |id: &str| estado.chats.iter().find(|c| c.id == id).map(|c| c.papel);
        assert_eq!(papel("novo"), Some(Papel::Projeto));
        assert_eq!(papel("solto"), Some(Papel::Projeto));
        for (id, fica) in [("com-tarefa", Papel::Coordenador), ("sem-pasta", Papel::Coordenador), ("falou", Papel::Coordenador), ("sessao", Papel::Coordenador)] {
            assert_eq!(papel(id), Some(fica), "{id}");
        }
        let novo = estado.chats.iter().find(|c| c.id == "novo").unwrap();
        let t = estado.tarefas.iter().find(|t| Some(t.id) == novo.tarefa).unwrap();
        assert_eq!((t.estado, t.forma, t.preparada), (EstadoTarefa::Conversa, Forma::Copia, false));
        assert_eq!(t.origem, PathBuf::from("/p/loja"));
        assert_eq!(novo.cwd, dados.pasta_copias("loja").join(format!("t{}", t.id)));
        assert!(t.porta > 0);
        let solto = estado.chats.iter().find(|c| c.id == "solto").unwrap();
        let t = estado.tarefas.iter().find(|t| Some(t.id) == solto.tarefa).unwrap();
        assert_eq!((t.forma, t.preparada), (Forma::Direta, true));
        assert_eq!((solto.cwd.clone(), t.origem.clone()), (dados.pasta_avulso("solto"), dados.pasta_avulso("solto")));
        let numeros: Vec<u64> = estado.tarefas.iter().map(|t| t.id).collect();
        assert_eq!(numeros, [4, 5, 6], "os números novos passam do maior");
        assert_eq!(estado.proxima_tarefa, 7);
    }

    #[test]
    fn conversas_antigas_ficam_so_leitura_e_os_trabalhadores_viram_chats() {
        let dados = Dados::em(Path::new("/d"));
        let mut antigo = chat("c", Papel::Coordenador, "loja");
        antigo.itens.push(Item::Assistente { texto: "distribuí as tarefas".into() });
        // Com mensagens: um chat de projeto vazio e velho seria recolhido.
        let conversou = |mut c: Chat| {
            c.itens.push(Item::Assistente { texto: "feito".into() });
            c
        };
        let trabalhador = |id: &str, n: u64| conversou(Chat { pai: Some("c".into()), tarefa: Some(n), ..chat(id, Papel::Trabalhador, "loja") });
        let pai = conversou(Chat { tarefa: Some(9), ..chat("p", Papel::Projeto, "loja") });
        let ajudante = conversou(Chat { pai: Some("p".into()), tarefa: Some(10), ..chat("a", Papel::Trabalhador, "loja") });
        let dono = |t: Tarefa, d: &str| Tarefa { coordenador: d.into(), progresso: Some("rodando".into()), ..t };
        let mut estado = Estado {
            chats: vec![antigo, trabalhador("w1", 1), trabalhador("w2", 2), trabalhador("w3", 3), pai, ajudante],
            tarefas: vec![
                dono(Tarefa { estado: EstadoTarefa::Rodando, ..tarefa(1, "loja", "w1") }, "c"),
                dono(Tarefa { estado: EstadoTarefa::EmRevisao, entrega_pendente: Some(false), ..tarefa(2, "loja", "w2") }, "c"),
                dono(Tarefa { estado: EstadoTarefa::Aprovada, ..tarefa(3, "loja", "w3") }, "c"),
                Tarefa { estado: EstadoTarefa::Conversa, ..tarefa(9, "loja", "p") },
                dono(Tarefa { estado: EstadoTarefa::Rodando, ..tarefa(10, "loja", "a") }, "p"),
            ],
            proxima_tarefa: 11,
            ..Default::default()
        };
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        let chat_ = |id: &str| estado.chats.iter().find(|c| c.id == id).unwrap();
        let tarefa_ = |id: u64| estado.tarefas.iter().find(|t| t.id == id).unwrap();
        // A conversa antiga continua como estava, só para leitura.
        assert_eq!((chat_("c").papel, chat_("c").itens.len()), (Papel::Coordenador, 1));
        // Rodando e em revisão: chat de projeto em conversa, sem dono, com o aviso.
        for (id, n_) in [("w1", 1), ("w2", 2)] {
            let (c, t) = (chat_(id), tarefa_(n_));
            assert_eq!((c.papel, c.pai.as_deref(), c.arquivado), (Papel::Projeto, None, false), "{id}");
            assert_eq!((t.estado, t.coordenador.as_str(), t.entrega_pendente, t.progresso.as_deref()), (EstadoTarefa::Conversa, "", None, None), "{id}");
            assert!(matches!(c.itens.last(), Some(Item::Aviso { texto, .. }) if texto == TRABALHADOR_ANTIGO), "{id}");
            assert!(n.chats_mexidos.contains(&id.to_string()), "{id}");
        }
        // Encerrada: fica como está, e o chat vai para as arquivadas.
        let (c, t) = (chat_("w3"), tarefa_(3));
        assert_eq!((c.papel, c.arquivado, c.itens.len()), (Papel::Projeto, true, 1));
        assert_eq!((t.estado, t.coordenador.as_str()), (EstadoTarefa::Aprovada, ""));
        // O ajudante de um chat de projeto não é do Vitral antigo.
        assert_eq!((chat_("a").papel, tarefa_(10).coordenador.as_str(), tarefa_(10).estado), (Papel::Trabalhador, "p", EstadoTarefa::Rodando));
        // Na abertura seguinte, nada muda de novo.
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        assert!(n.chats_mexidos.is_empty(), "{:?}", n.chats_mexidos);
    }

    #[test]
    #[should_panic(expected = "Dados::em")]
    fn dados_padrao_nao_roda_em_teste() {
        let _ = Dados::padrao();
    }
}
