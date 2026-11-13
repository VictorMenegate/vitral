//! Pasta de dados global do Vitral, que substitui o `.vitral/` de cada pasta de trabalho:
//! - `~/.local/share/vitral/` (`Dados::pasta`): `estado.json` (com `.anterior` e, da primeira
//!   abertura depois das versões com projetos, `estado.versao-2.json`), `chats/<id>.json`,
//!   `avulsos/<chat>/` (os chats sem pasta), `exportadas/`, `anexos/` e `migracao.json`. As versões
//!   anteriores deixaram também `notas/`, `mestre/`, `projetos/` e `copias/`, que ficam como estão;
//! - `~/.config/vitral/` (`Dados::config`): `config.json`, `interface.json`, as contas e o
//!   `recentes.json` antigo, que só a migração lê;
//! - `~/.cache/vitral/` (`Dados::cache`): os prompts de cada turno.
//!
//! A pasta de dados também guarda o app instalado (`app/`, do atualizador: as versões, os links
//! `atual` e `anterior` e o `estado.json` dele) e o `formato` dos dados. Nada daqui mexe em `app/`.
//!
//! Toda gravação aguenta a fonte desligar no meio: o arquivo fica inteiro, velho ou novo.

use std::{
    collections::{HashSet, VecDeque},
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, Condvar, Mutex},
};

use anyhow::{Context as _, Result, anyhow, bail};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::modelo::{
    Chat, Config, Estado, EstadoRef, FORMATO_DADOS, Forma, ID_MESTRE, Item, MAX_OCORRENCIAS, PapelAntigo, VERSAO_ESTADO,
    agora,
};

/// Ligada desde a 0.3.0. Numa mudança grande dos dados, desligar deixa `Dados::padrao()` só na
/// bancada (`DENTRO_DA_BANCADA=1`, que isola os XDG): um build do meio da mudança não grava nos dados
/// de verdade, o `--instalar` e o `instalar-dev.sh` recusam, e um release estável não passa nos testes.
pub const DADOS_GLOBAIS: bool = true;

/// A pasta do app instalado dentro da pasta de dados no Linux (`atualizacao::linux::Raiz::app`;
/// no Windows, o app mora em `%LOCALAPPDATA%\Programs\Vitral`, fora dela).
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
}

/// O que `carregar` leu: o estado com as mensagens de cada chat, a configuração e o que avisar.
pub struct Carregado {
    pub estado: Estado,
    pub config: Config,
    pub avisos: Vec<String>,
}

/// Por que o estado gravado não abre: o app abre só para leitura, sem gravar nada por cima.
#[derive(Debug, PartialEq)]
pub enum ErroCarga {
    /// `copia` é a outra versão, quando existe e também não serve.
    Ilegivel { arquivo: PathBuf, copia: Option<PathBuf> },
    VersaoNova { arquivo: PathBuf, versao: u32 },
}

/// Chaves da configuração que mudaram de nome: (nova, antiga).
pub const CHAVES_RENOMEADAS: [(&str, &str); 1] = [("modelo_chats", "modelo_trabalhador")];

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
        Ok(Self { pasta, config, cache, socket, gravador: Default::default() })
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

    pub fn pasta_avulso(&self, chat: &str) -> PathBuf {
        self.pasta.join("avulsos").join(chat)
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
            .unwrap_or_else(|| (Estado::default(), Vec::new()));
        if (1..VERSAO_ESTADO).contains(&estado.versao) {
            self.guardar_estado_antigo(estado.versao);
        }
        estado.versao = VERSAO_ESTADO;
        for chave in crate::modelo::CHAVES_DO_ESTADO_QUE_SAIRAM {
            estado.resto.remove(*chave);
        }
        for chat in &mut estado.chats {
            for chave in crate::modelo::CHAVES_DO_CHAT_QUE_SAIRAM {
                chat.resto.remove(*chave);
            }
        }
        let config = self.ler_config(&mut avisos);
        for chat in &mut estado.chats {
            chat.itens = self.ler_itens(&chat.id, &mut avisos);
        }
        Ok(Carregado { estado, config, avisos })
    }

    /// Antes da primeira gravação no formato de agora, guarda o `estado.json` de uma versão anterior
    /// como veio (`estado.versao-2.json`): ele tinha o que esta versão não grava mais (os projetos,
    /// as tarefas), e a volta para aquela versão o lê. Uma vez só: o que já está guardado fica.
    fn guardar_estado_antigo(&self, versao: u32) {
        let guardado = self.pasta.join(format!("estado.versao-{versao}.json"));
        if !nao_existe(&guardado) {
            return;
        }
        let arquivo = self.arquivo_estado();
        let origem = if nao_existe(&arquivo) { com_sufixo(&arquivo, ".anterior") } else { arquivo };
        if let Err(erro) = copiar_com_fsync(&origem, &guardado) {
            eprintln!("vitral: não guardei {}: {erro:#}", guardado.display());
        }
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
    renomear(&temporario, caminho).with_context(|| caminho.display().to_string())?;
    sincronizar_pasta(pasta)
}

/// O rename por cima. No Windows, o antivírus e o indexador às vezes seguram o arquivo por um
/// instante, e o rename volta "Acesso negado" (ou a violação de compartilhamento): até 5
/// tentativas, a cada 20 ms. Um erro de verdade (pasta sem permissão) só demora 80 ms a mais.
pub fn renomear(de: &Path, para: &Path) -> io::Result<()> {
    let mut tentativas = if cfg!(windows) { 5 } else { 1 };
    loop {
        match fs::rename(de, para) {
            // ERROR_SHARING_VIOLATION (32) e ERROR_LOCK_VIOLATION (33) não têm ErrorKind próprio.
            Err(erro)
                if tentativas > 1
                    && (erro.kind() == io::ErrorKind::PermissionDenied || matches!(erro.raw_os_error(), Some(32 | 33))) =>
            {
                tentativas -= 1;
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            resultado => return resultado,
        }
    }
}

pub fn gravar_json_seguro<T: Serialize>(caminho: &Path, valor: &T, anterior: bool) -> Result<()> {
    gravar_seguro(caminho, &serde_json::to_vec_pretty(valor)?, anterior)
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
    let ligado = fs::hard_link(caminho, &link).and_then(|()| renomear(&link, &anterior));
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
/// chaves, vale a nova) e sem `projetos` e `rotinas`, que foram para os projetos e o estado, nem as
/// chaves do que saiu do Vitral (`CHAVES_DA_CONFIG_QUE_SAIRAM`). As outras ficam como vieram.
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
        for chave in crate::modelo::CHAVES_DA_CONFIG_QUE_SAIRAM {
            chaves.remove(*chave);
        }
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

/// O que a normalização pergunta ao disco e ao git: um falso nos testes.
pub trait Consultas {
    fn existe(&self, caminho: &Path) -> bool;
    /// O repositório de onde a cópia saiu.
    fn repositorio_da_copia(&self, copia: &Path) -> Option<PathBuf>;
}

pub struct NoDisco;

impl Consultas for NoDisco {
    fn existe(&self, caminho: &Path) -> bool {
        caminho.exists()
    }

    fn repositorio_da_copia(&self, copia: &Path) -> Option<PathBuf> {
        crate::git::repositorio_da_copia(copia)
    }
}

/// O que a normalização mexeu: avisos, os chats cujas mensagens mudaram (gravar de novo), os
/// chats vazios que ela tirou do estado (apagar o arquivo) e os que começam uma sessão nova do
/// Claude Code (com o bloco de passagem, para não perder o assunto).
#[derive(Debug, Default)]
pub struct Normalizado {
    pub avisos: Vec<String>,
    pub chats_mexidos: Vec<String>,
    pub chats_a_apagar: Vec<String>,
    pub sessoes_novas: Vec<String>,
}

impl Normalizado {
    fn mexeu(&mut self, chat: &str) {
        if !self.chats_mexidos.iter().any(|c| c == chat) {
            self.chats_mexidos.push(chat.to_string());
        }
    }
}

/// Põe o estado carregado nas regras de agora, antes de qualquer turno: a lista é só de chats,
/// cada um mexendo direto na pasta dele. O Mestre e os ajudantes saem da lista; o que trabalhava
/// numa cópia volta para a origem dela; o que era do projeto (arquivado, o modelo) passa para o
/// chat; e o que é velho sai. Os projetos, as tarefas e os papéis só são lidos aqui.
pub fn normalizar(estado: &mut Estado, dados: &Dados, consultas: &dyn Consultas, agora: u64) -> Normalizado {
    let mut n = Normalizado::default();
    tirar_o_mestre(estado);
    converter_trabalhadores_antigos(estado, &mut n);
    tirar_os_ajudantes(estado);
    voltar_para_a_origem(estado, consultas, &mut n);
    herdar_do_projeto(estado);
    podar(estado);
    recolher_vazios(estado, dados, agora, &mut n);
    estado.tarefas.clear();
    estado.projetos.clear();
    for c in &mut estado.chats {
        (c.tarefa, c.pai) = (None, None);
        c.projeto_antigo.clear();
        c.papel_antigo = PapelAntigo::default();
    }
    n
}

/// O Mestre saiu do Vitral: o chat dele sai da lista (o arquivo das mensagens fica no disco).
fn tirar_o_mestre(estado: &mut Estado) {
    estado.chats.retain(|c| c.papel_antigo != PapelAntigo::Mestre && c.id != ID_MESTRE);
    estado.filas.remove(ID_MESTRE);
}

/// O aviso no trabalhador do Vitral antigo que tinha um pedido aberto e virou chat.
pub const TRABALHADOR_ANTIGO: &str = "Este chat nasceu de uma conversa do Vitral antigo; o pedido agora é seu.";

/// Os trabalhadores do coordenador antigo (o pai não é um chat de projeto: esses são os ajudantes)
/// viram chats, sem dono: o do pedido em aberto ganha um aviso; o do pedido encerrado vai para as
/// arquivadas.
fn converter_trabalhadores_antigos(estado: &mut Estado, n: &mut Normalizado) {
    let de_projeto: HashSet<String> =
        estado.chats.iter().filter(|c| c.papel_antigo == PapelAntigo::Projeto).map(|c| c.id.clone()).collect();
    let tarefas = &estado.tarefas;
    for c in estado.chats.iter_mut().filter(|c| c.papel_antigo == PapelAntigo::Trabalhador) {
        if c.pai.as_ref().is_some_and(|p| de_projeto.contains(p)) {
            continue;
        }
        c.papel_antigo = PapelAntigo::Projeto;
        c.pai = None;
        let tarefa = c.tarefa.and_then(|id| tarefas.iter().find(|t| t.id == id));
        if tarefa.is_some_and(|t| matches!(t.estado.as_str(), "Aprovada" | "Falhou" | "Cancelada")) {
            c.arquivado = true;
        } else {
            c.itens.push(Item::Aviso { texto: TRABALHADOR_ANTIGO.into(), erro: false, em: agora() });
        }
        n.mexeu(&c.id);
    }
}

/// Os ajudantes (os chats que um chat de projeto criava) saem da lista; o arquivo das mensagens
/// deles e as cópias ficam no disco.
fn tirar_os_ajudantes(estado: &mut Estado) {
    let ajudantes: Vec<String> =
        estado.chats.iter().filter(|c| c.papel_antigo == PapelAntigo::Trabalhador).map(|c| c.id.clone()).collect();
    estado.chats.retain(|c| !ajudantes.contains(&c.id));
    estado.filas.retain(|c, _| !ajudantes.contains(c));
}

/// O aviso no chat que trabalhava numa cópia isolada e agora mexe direto na pasta de origem. A
/// cópia não é apagada (`existe`: ela ainda está no disco).
pub fn aviso_da_copia(copia: &Path, existe: bool, origem: &Path, sessao_nova: bool) -> String {
    let mut texto = if existe {
        format!(
            "Este chat trabalhava numa cópia isolada, que continua em {}; agora ele mexe direto em {}.",
            copia.display(),
            origem.display()
        )
    } else {
        format!("Este chat trabalhava numa cópia isolada, que já foi apagada; agora ele mexe direto em {}.", origem.display())
    };
    if sessao_nova {
        texto.push_str(
            " A sessão do Claude Code ficou presa à pasta da cópia: a próxima mensagem começa uma sessão nova, \
             com um resumo desta conversa.",
        );
    }
    texto
}

/// O aviso no chat que trabalhava numa cópia cuja origem não se sabe mais: ele fica na cópia.
pub fn aviso_da_copia_sem_origem(copia: &Path) -> String {
    format!(
        "Este chat trabalhava numa cópia isolada e não achei a pasta de onde ela saiu; ele continua mexendo \
         direto na cópia, em {}.",
        copia.display()
    )
}

/// Cada chat mexe direto na pasta dele. O que trabalhava numa cópia (git worktree) passa a mexer na
/// origem dela, com um aviso; a sessão do Claude Code criada na cópia não abre na origem, então ele
/// começa uma nova (com o bloco de passagem). A cópia fica no disco.
fn voltar_para_a_origem(estado: &mut Estado, consultas: &dyn Consultas, n: &mut Normalizado) {
    let projetos = &estado.projetos;
    for c in estado.chats.iter_mut() {
        let Some(t) = c.tarefa.and_then(|id| estado.tarefas.iter().find(|t| t.id == id)) else {
            continue;
        };
        if t.forma != Forma::Copia {
            continue;
        }
        let copia = c.cwd.clone();
        let origem = Some(t.origem.clone())
            .filter(|o| !o.as_os_str().is_empty())
            .or_else(|| t.worktree.as_deref().and_then(|w| consultas.repositorio_da_copia(w)))
            .or_else(|| consultas.repositorio_da_copia(&copia))
            .or_else(|| {
                projetos.iter().find(|p| p.id == c.projeto_antigo).and_then(|p| p.pastas.first()).map(|p| p.caminho.clone())
            });
        let Some(origem) = origem.filter(|o| *o != copia) else {
            c.itens.push(Item::Aviso { texto: aviso_da_copia_sem_origem(&copia), erro: false, em: agora() });
            n.mexeu(&c.id);
            continue;
        };
        let existe = consultas.existe(&copia);
        // A cópia que nem chegou a nascer (ela nascia na primeira mensagem): o chat só muda de pasta.
        if !existe && !c.sessao_criada && c.itens.is_empty() {
            c.cwd = origem;
            continue;
        }
        let sessao_nova = c.sessao_criada;
        if sessao_nova {
            c.sessao = uuid::Uuid::new_v4().to_string();
            c.sessao_criada = false;
            c.contexto = None;
            n.sessoes_novas.push(c.id.clone());
        }
        c.itens.push(Item::Aviso { texto: aviso_da_copia(&copia, existe, &origem, sessao_nova), erro: false, em: agora() });
        c.cwd = origem;
        n.mexeu(&c.id);
    }
}

/// O que era do projeto passa para os chats dele: o projeto arquivado leva as conversas para as
/// arquivadas, e o modelo do projeto vale para quem não escolheu outro.
fn herdar_do_projeto(estado: &mut Estado) {
    for c in estado.chats.iter_mut() {
        let Some(p) = estado.projetos.iter().find(|p| !c.projeto_antigo.is_empty() && p.id == c.projeto_antigo) else {
            continue;
        };
        c.arquivado |= p.arquivado;
        if c.modelo.is_none() {
            c.modelo = p.modelo.clone();
        }
    }
}

/// As ocorrências guardam só as últimas.
fn podar(estado: &mut Estado) {
    let sobra = estado.ocorrencias.len().saturating_sub(MAX_OCORRENCIAS);
    estado.ocorrencias.drain(..sobra);
}

/// Chats que nasceram e nunca receberam nada há mais de um dia saem (fora o último aberto, o fixado,
/// o que veio da migração e a conversa do Vitral antigo, que não nascia vazia por engano).
fn recolher_vazios(estado: &mut Estado, dados: &Dados, agora: u64, n: &mut Normalizado) {
    let interface = ler_valor_sem_escrever(&dados.config.join("interface.json")).ok().flatten();
    let ultimo = interface.as_ref().and_then(|i| i.get("ultimo_chat")?.as_str().map(str::to_string));
    let limite = agora.saturating_sub(86_400);
    let vazios: Vec<String> = estado
        .chats
        .iter()
        .filter(|c| {
            c.papel_antigo != PapelAntigo::Coordenador
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
    estado.filas.retain(|c, _| !vazios.contains(c));
    n.chats_a_apagar = vazios;
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::{PastaAntiga, ProjetoAntigo, TarefaAntiga, novo_id};

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

    /// No Windows, um arquivo aberto sem compartilhar (como o antivírus faz ao ler) recusa o rename
    /// por cima: as novas tentativas esperam ele soltar.
    #[cfg(windows)]
    #[test]
    fn rename_espera_quem_segura_o_arquivo() {
        use std::os::windows::fs::OpenOptionsExt as _;
        let pasta = pasta_de_teste("preso");
        let (de, para) = (pasta.join("estado.json.tmp"), pasta.join("estado.json"));
        fs::write(&de, "novo").unwrap();
        fs::write(&para, "velho").unwrap();
        let preso = fs::OpenOptions::new().read(true).share_mode(0).open(&para).unwrap();
        assert!(fs::rename(&de, &para).is_err(), "o Windows devia recusar com o arquivo preso");
        let soltar = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(30));
            drop(preso);
        });
        renomear(&de, &para).unwrap();
        soltar.join().unwrap();
        assert_eq!(fs::read_to_string(&para).unwrap(), "novo");
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
        // No Linux o app mora dentro da pasta de dados; no Windows, ao lado (`Programs\Vitral`).
        if cfg!(target_os = "linux") {
            assert_eq!(app, dados_no_mesmo_lugar.pasta.join(PASTA_DO_APP));
        } else {
            assert!(!app.starts_with(&dados_no_mesmo_lugar.pasta), "{}", app.display());
        }
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
            dados.arquivo_prompt("c1"),
        ] {
            assert!(caminho.starts_with(&raiz), "{}", caminho.display());
        }
        assert_eq!(dados.arquivo_trava(), raiz.join("vitral.trava"));
        let _ = fs::remove_dir_all(&raiz);
    }

    struct Falso {
        existem: Vec<PathBuf>,
        repositorios: Vec<(PathBuf, PathBuf)>,
    }

    impl Consultas for Falso {
        fn existe(&self, caminho: &Path) -> bool {
            self.existem.iter().any(|e| e == caminho)
        }
        fn repositorio_da_copia(&self, copia: &Path) -> Option<PathBuf> {
            self.repositorios.iter().find(|(c, _)| c == copia).map(|(_, r)| r.clone())
        }
    }

    fn falso() -> Falso {
        Falso { existem: Vec::new(), repositorios: Vec::new() }
    }

    /// Um chat como as versões com projetos gravavam: com o papel e o projeto.
    fn chat(id: &str, papel: PapelAntigo, projeto: &str) -> Chat {
        let mut c = Chat::novo(id.into(), id, PathBuf::from("/x"));
        (c.papel_antigo, c.projeto_antigo, c.criado_em) = (papel, projeto.into(), 0);
        c
    }

    fn tarefa(id: u64, trabalhador: &str) -> TarefaAntiga {
        TarefaAntiga { id, trabalhador: trabalhador.into(), ..Default::default() }
    }

    fn projeto(id: &str, pasta: &str) -> ProjetoAntigo {
        ProjetoAntigo { id: id.into(), pastas: vec![PastaAntiga { caminho: pasta.into() }], ..Default::default() }
    }

    fn aviso(c: &Chat) -> Option<&str> {
        match c.itens.last() {
            Some(Item::Aviso { texto, .. }) => Some(texto),
            _ => None,
        }
    }

    const AGORA: u64 = 100 * 86_400;

    #[test]
    fn o_mestre_sai_da_lista_e_o_pedido_dele_fica_com_o_usuario() {
        let dados = Dados::em(Path::new("/d"));
        let mut chat_do_pedido = chat("c1", PapelAntigo::Projeto, "loja");
        chat_do_pedido.tarefa = Some(1);
        chat_do_pedido.itens.push(Item::Usuario { texto: "faça".into(), anexos: Vec::new(), ultrathink: false, ultracode: false, em: 0 });
        let mut estado = Estado {
            chats: vec![chat(ID_MESTRE, PapelAntigo::Mestre, ""), chat("m2", PapelAntigo::Mestre, ""), chat_do_pedido],
            tarefas: vec![TarefaAntiga { coordenador: ID_MESTRE.into(), forma: Forma::Direta, ..tarefa(1, "c1") }],
            ocorrencias: (0..MAX_OCORRENCIAS as u64 + 5)
                .map(|q| crate::modelo::Ocorrencia { quando: q, ..Default::default() })
                .collect(),
            ..Default::default()
        };
        estado.filas.insert(ID_MESTRE.into(), vec![Default::default()]);
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        // Os dois Mestres saem da lista; o arquivo das mensagens deles não é apagado.
        assert_eq!(estado.chats.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c1"]);
        assert!(n.chats_a_apagar.is_empty());
        assert!(!estado.filas.contains_key(ID_MESTRE));
        // O chat do pedido fica, sem a tarefa e sem aviso (ele já mexia direto na pasta).
        assert_eq!((estado.chats[0].tarefa, estado.chats[0].itens.len()), (None, 1));
        assert!(n.chats_mexidos.is_empty());
        assert_eq!(estado.ocorrencias.len(), MAX_OCORRENCIAS);
        assert_eq!(estado.ocorrencias[0].quando, 5);
        // De novo: nada muda.
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        assert!(n.chats_mexidos.is_empty());
    }

    #[test]
    fn chat_que_trabalhava_numa_copia_volta_para_a_origem() {
        let dados = Dados::em(Path::new("/d"));
        let na_copia = |id: &str, n: u64, sessao: bool, falou: bool| {
            let mut c = chat(id, PapelAntigo::Projeto, "loja");
            (c.tarefa, c.cwd, c.sessao_criada) = (Some(n), PathBuf::from(format!("/d/copias/loja/t{n}")), sessao);
            // Recente: o chat vazio e velho seria recolhido.
            c.criado_em = AGORA;
            if falou {
                c.itens.push(Item::Assistente { texto: "feito".into(), em: 0 });
            }
            c
        };
        let copia = |n: u64| PathBuf::from(format!("/d/copias/loja/t{n}"));
        let direto = Chat { tarefa: Some(6), cwd: "/p/receitas".into(), criado_em: AGORA, ..chat("direto", PapelAntigo::Projeto, "loja") };
        let mut estado = Estado {
            projetos: vec![projeto("loja", "/p/loja")],
            chats: vec![
                na_copia("com-sessao", 1, true, true),
                na_copia("sem-sessao", 2, false, true),
                na_copia("nunca-rodou", 3, false, false),
                na_copia("pelo-git", 4, true, true),
                Chat { projeto_antigo: "sumido".into(), ..na_copia("sem-origem", 5, false, true) },
                na_copia("pelo-projeto", 7, false, true),
                direto,
            ],
            tarefas: vec![
                TarefaAntiga { origem: "/p/loja".into(), worktree: Some(copia(1)), ..tarefa(1, "com-sessao") },
                TarefaAntiga { origem: "/p/loja".into(), worktree: Some(copia(2)), ..tarefa(2, "sem-sessao") },
                TarefaAntiga { origem: "/p/loja".into(), ..tarefa(3, "nunca-rodou") },
                TarefaAntiga { worktree: Some(copia(4)), ..tarefa(4, "pelo-git") },
                tarefa(5, "sem-origem"),
                TarefaAntiga { forma: Forma::DiretaGit, origem: "/p/receitas".into(), ..tarefa(6, "direto") },
                tarefa(7, "pelo-projeto"),
            ],
            ..Default::default()
        };
        let sessao_antiga = estado.chats[0].sessao.clone();
        let consultas = Falso {
            existem: vec![copia(1), copia(2), copia(4), copia(5), copia(7), "/p/loja".into()],
            repositorios: vec![(copia(4), "/p/git".into())],
        };
        let n = normalizar(&mut estado, &dados, &consultas, AGORA);
        let chat_ = |id: &str| estado.chats.iter().find(|c| c.id == id).unwrap();
        // Com sessão: mexe direto na origem, com uma sessão nova (a da cópia não abre lá) e o aviso.
        let c = chat_("com-sessao");
        assert_eq!(c.cwd, PathBuf::from("/p/loja"));
        assert!(!c.sessao_criada && c.sessao != sessao_antiga);
        assert_eq!(aviso(c), Some(aviso_da_copia(&copia(1), true, Path::new("/p/loja"), true).as_str()));
        assert!(aviso(c).unwrap().starts_with(
            "Este chat trabalhava numa cópia isolada, que continua em /d/copias/loja/t1; agora ele mexe direto em /p/loja."
        ));
        // Sem sessão: só a pasta muda, com o aviso.
        let c = chat_("sem-sessao");
        assert_eq!((c.cwd.as_path(), aviso(c)), (Path::new("/p/loja"), Some(aviso_da_copia(&copia(2), true, Path::new("/p/loja"), false).as_str())));
        // A cópia nem chegou a nascer: muda de pasta calado.
        let c = chat_("nunca-rodou");
        assert_eq!((c.cwd.as_path(), c.itens.len()), (Path::new("/p/loja"), 0));
        // Sem a origem gravada: a do git da cópia; sem o git, a pasta principal do projeto.
        assert_eq!(chat_("pelo-git").cwd, PathBuf::from("/p/git"));
        assert_eq!(chat_("pelo-projeto").cwd, PathBuf::from("/p/loja"));
        // Sem origem nenhuma: fica na cópia, avisado.
        let c = chat_("sem-origem");
        assert_eq!((c.cwd.clone(), aviso(c)), (copia(5), Some(aviso_da_copia_sem_origem(&copia(5)).as_str())));
        // Quem já mexia direto fica como estava.
        assert_eq!((chat_("direto").cwd.as_path(), chat_("direto").itens.len()), (Path::new("/p/receitas"), 0));
        assert_eq!(n.sessoes_novas, ["com-sessao", "pelo-git"]);
        let mut mexidos = n.chats_mexidos.clone();
        mexidos.sort();
        assert_eq!(mexidos, ["com-sessao", "pelo-git", "pelo-projeto", "sem-origem", "sem-sessao"]);
        assert!(estado.chats.iter().all(|c| c.tarefa.is_none()) && estado.tarefas.is_empty());
        // Na abertura seguinte (o estado já gravado sem as tarefas), nada muda.
        let n = normalizar(&mut estado, &dados, &consultas, AGORA);
        assert!(n.chats_mexidos.is_empty() && n.sessoes_novas.is_empty());
    }

    #[test]
    fn os_chats_herdam_o_que_era_do_projeto() {
        let dados = Dados::em(Path::new("/d"));
        let com_modelo = Chat { modelo: Some("haiku".into()), ..chat("escolheu", PapelAntigo::Projeto, "loja") };
        let mut estado = Estado {
            projetos: vec![
                ProjetoAntigo { modelo: Some("sonnet".into()), ..projeto("loja", "/p/loja") },
                ProjetoAntigo { arquivado: true, ..projeto("velho", "/p/velho") },
            ],
            chats: vec![
                chat("segue", PapelAntigo::Projeto, "loja"),
                com_modelo,
                chat("do-velho", PapelAntigo::Projeto, "velho"),
                chat("avulso", PapelAntigo::Projeto, ""),
            ],
            ..Default::default()
        };
        for c in &mut estado.chats {
            c.itens.push(Item::Assistente { texto: "oi".into(), em: 0 });
        }
        normalizar(&mut estado, &dados, &falso(), AGORA);
        let chat_ = |id: &str| estado.chats.iter().find(|c| c.id == id).unwrap();
        // O modelo do projeto vale para quem seguia o padrão; quem escolheu fica com o seu.
        assert_eq!(chat_("segue").modelo.as_deref(), Some("sonnet"));
        assert_eq!(chat_("escolheu").modelo.as_deref(), Some("haiku"));
        // O projeto arquivado leva as conversas dele para as arquivadas.
        assert!(chat_("do-velho").arquivado && !chat_("segue").arquivado && !chat_("avulso").arquivado);
        // Nada do projeto fica: nem a lista, nem a marca nos chats.
        assert!(estado.projetos.is_empty());
        assert!(estado.chats.iter().all(|c| c.projeto_antigo.is_empty() && c.papel_antigo == PapelAntigo::default()));
    }

    #[test]
    fn conversas_antigas_viram_chats_e_os_trabalhadores_tambem() {
        let dados = Dados::em(Path::new("/d"));
        let mut antigo = chat("c", PapelAntigo::Coordenador, "loja");
        antigo.itens.push(Item::Assistente { texto: "distribuí as tarefas".into(), em: 0 });
        // A conversa do Vitral antigo que nunca recebeu nada não é recolhida (não nasceu vazia por engano).
        let antigo_vazio = chat("vazio", PapelAntigo::Coordenador, "loja");
        // Com mensagens: um chat vazio e velho seria recolhido.
        let conversou = |mut c: Chat| {
            c.itens.push(Item::Assistente { texto: "feito".into(), em: 0 });
            c
        };
        let trabalhador = |id: &str, n: u64| conversou(Chat { pai: Some("c".into()), tarefa: Some(n), ..chat(id, PapelAntigo::Trabalhador, "loja") });
        let pai = conversou(Chat { tarefa: Some(9), ..chat("p", PapelAntigo::Projeto, "loja") });
        let ajudante = conversou(Chat { pai: Some("p".into()), tarefa: Some(10), ..chat("a", PapelAntigo::Trabalhador, "loja") });
        let dono = |t: TarefaAntiga, d: &str, estado: &str| TarefaAntiga { coordenador: d.into(), estado: estado.into(), forma: Forma::Direta, ..t };
        let mut estado = Estado {
            chats: vec![antigo, antigo_vazio, trabalhador("w1", 1), trabalhador("w2", 2), trabalhador("w3", 3), pai, ajudante],
            tarefas: vec![
                dono(tarefa(1, "w1"), "c", "Rodando"),
                dono(tarefa(2, "w2"), "c", "EmRevisao"),
                dono(tarefa(3, "w3"), "c", "Aprovada"),
                dono(tarefa(9, "p"), "", "Conversa"),
                dono(tarefa(10, "a"), "p", "Rodando"),
            ],
            ..Default::default()
        };
        estado.filas.insert("a".into(), vec![Default::default()]);
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        let chat_ = |id: &str| estado.chats.iter().find(|c| c.id == id).unwrap();
        // A conversa antiga continua, agora um chat como os outros.
        assert_eq!(chat_("c").itens.len(), 1);
        assert!(estado.chats.iter().any(|c| c.id == "vazio"));
        // Rodando e em revisão: chat sem dono, com o aviso.
        for id in ["w1", "w2"] {
            let c = chat_(id);
            assert_eq!((c.pai.as_deref(), c.arquivado), (None, false), "{id}");
            assert_eq!(aviso(c), Some(TRABALHADOR_ANTIGO), "{id}");
            assert!(n.chats_mexidos.contains(&id.to_string()), "{id}");
        }
        // Encerrada: vai para as arquivadas, sem aviso.
        let c = chat_("w3");
        assert_eq!((c.arquivado, c.itens.len()), (true, 1));
        // O ajudante de um chat de projeto sai da lista (o arquivo dele fica), com a fila.
        assert!(estado.chats.iter().all(|c| c.id != "a"));
        assert!(!estado.filas.contains_key("a") && n.chats_a_apagar.is_empty());
        assert!(estado.chats.iter().all(|c| c.papel_antigo == PapelAntigo::default() && c.pai.is_none()));
        // Na abertura seguinte, nada muda de novo.
        let n = normalizar(&mut estado, &dados, &falso(), AGORA);
        assert!(n.chats_mexidos.is_empty(), "{:?}", n.chats_mexidos);
    }

    /// Um `estado.json` da versão com projetos, como estava no disco: um projeto com a pasta, outro
    /// arquivado, um chat numa cópia isolada, o Mestre, um ajudante, cartões e rotinas. Abre com
    /// só os chats, cada um com a pasta dele, e nada do usuário é apagado.
    #[test]
    fn estado_com_projetos_abre_so_com_chats_de_pasta() {
        let raiz = pasta_de_teste("versao-2");
        let dados = Dados::em(&raiz.join("dados"));
        let (loja, velho) = (raiz.join("loja"), raiz.join("velho"));
        let copia = dados.pasta.join("copias").join("loja").join("t7");
        let propria = dados.pasta.join("projetos").join("notas-da-loja");
        for pasta in [&loja, &velho, &copia, &propria, &dados.pasta.join("chats"), &dados.pasta.join("notas")] {
            fs::create_dir_all(pasta).unwrap();
        }
        fs::write(dados.pasta.join("notas").join("loja.md"), "- usar Decimal\n").unwrap();
        let chat = |id: &str, projeto: &str, papel: &str, cwd: &Path, extra: Value| {
            let mut c = serde_json::json!({
                "id": id, "projeto": projeto, "titulo": format!("chat {id}"), "papel": papel, "pai": null,
                "tarefa": null, "sessao": format!("s-{id}"), "sessao_criada": true, "cwd": cwd, "modelo": null,
                "criado_em": 1, "atualizado_em": 2, "custo_usd": 0.0, "isolado": true
            });
            c.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            c
        };
        let antigo = serde_json::json!({
            "versao": 2,
            "projetos": [
                {"id": "loja", "nome": "Loja", "ordem": 0, "icone": "store", "modelo": "sonnet", "instrucoes": "seja breve",
                 "pastas": [{"caminho": loja, "propria": false, "preparacao": "npm i"}, {"caminho": propria, "propria": true}]},
                {"id": "velho", "nome": "Velho", "ordem": 1, "arquivado": true, "pastas": [{"caminho": velho}]}
            ],
            "chats": [
                chat(ID_MESTRE, "", "Mestre", &raiz, serde_json::json!({})),
                chat("c1", "loja", "Projeto", &loja, serde_json::json!({})),
                chat("c2", "loja", "Projeto", &copia, serde_json::json!({"tarefa": 7})),
                chat("h1", "loja", "Trabalhador", &loja, serde_json::json!({"pai": "c1", "tarefa": 8})),
                chat("v1", "velho", "Projeto", &velho, serde_json::json!({"modelo": "opus"})),
            ],
            "tarefas": [
                {"id": 7, "projeto": "loja", "coordenador": "", "trabalhador": "c2", "titulo": "t", "objetivo": "o",
                 "criterios": [], "estado": "Conversa", "rodada": 1, "criada_em": 0, "forma": "Copia",
                 "origem": loja, "worktree": copia},
                {"id": 8, "projeto": "loja", "coordenador": "c1", "trabalhador": "h1", "titulo": "t", "objetivo": "o",
                 "criterios": [], "estado": "Rodando", "rodada": 1, "criada_em": 0, "forma": "Direta", "origem": loja}
            ],
            "proxima_tarefa": 9, "cartoes": [{"id": 1, "chat": "mestre"}], "proximo_cartao": 2,
            "rotinas": [{"id": "r1", "projeto": "loja", "texto": "Rode os testes", "hora": 540}],
            "filas": {"h1": [{"de": null, "texto": "depois", "anexos": []}]},
            "ocorrencias": [], "visto_em": 0
        });
        let bytes = serde_json::to_vec_pretty(&antigo).unwrap();
        fs::write(dados.arquivo_estado(), &bytes).unwrap();
        for id in [ID_MESTRE, "c1", "c2", "h1", "v1"] {
            fs::write(dados.arquivo_chat(id), r#"[{"Usuario": {"texto": "oi"}}, {"Assistente": {"texto": "olá"}}]"#).unwrap();
        }
        fs::create_dir_all(&dados.config).unwrap();
        fs::write(dados.arquivo_config(), r#"{"fileira_de_projetos": true, "rotinas": [{"id": "r2"}], "permissao": "auto"}"#).unwrap();

        let Carregado { mut estado, config, avisos } = dados.carregar().unwrap();
        assert!(avisos.is_empty(), "{avisos:?}");
        assert_eq!(config.permissao, "auto");
        // O estado da versão anterior fica guardado como veio, para a volta de versão.
        assert_eq!(fs::read(dados.pasta.join("estado.versao-2.json")).unwrap(), bytes);
        let consultas = Falso { existem: vec![copia.clone(), loja.clone(), velho.clone()], repositorios: Vec::new() };
        let n = normalizar(&mut estado, &dados, &consultas, AGORA);

        // Só os chats do usuário: o Mestre e o ajudante saem da lista (os arquivos ficam).
        let ids: Vec<&str> = estado.chats.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["c1", "c2", "v1"]);
        assert!(n.chats_a_apagar.is_empty() && estado.filas.is_empty());
        let chat_ = |id: &str| estado.chats.iter().find(|c| c.id == id).unwrap();
        assert_eq!((chat_("c1").cwd.as_path(), chat_("c1").modelo.as_deref()), (loja.as_path(), Some("sonnet")));
        assert_eq!(chat_("c1").itens.len(), 2);
        // O da cópia mexe direto na origem, com o aviso e uma sessão nova; a cópia fica no disco.
        let c2 = chat_("c2");
        assert_eq!(c2.cwd, loja);
        assert!(!c2.sessao_criada && c2.sessao != "s-c2");
        assert!(aviso(c2).unwrap().starts_with(&format!(
            "Este chat trabalhava numa cópia isolada, que continua em {}; agora ele mexe direto em {}.",
            copia.display(),
            loja.display()
        )));
        assert_eq!(n.sessoes_novas, ["c2"]);
        // O do projeto arquivado vai para as arquivadas, com o modelo que tinha.
        assert!(chat_("v1").arquivado);
        assert_eq!(chat_("v1").modelo.as_deref(), Some("opus"));

        // Gravado, o estado novo só tem chats, sem as chaves das versões antigas.
        dados.salvar_estado(&estado.por_referencia()).unwrap();
        for c in &estado.chats {
            dados.salvar_chat(c).unwrap();
        }
        dados.esperar_gravacoes();
        let gravado: Value = serde_json::from_slice(&fs::read(dados.arquivo_estado()).unwrap()).unwrap();
        assert_eq!(gravado["versao"], VERSAO_ESTADO);
        for chave in ["projetos", "tarefas", "cartoes", "proximo_cartao", "proxima_tarefa", "rotinas"] {
            assert!(gravado.get(chave).is_none(), "{chave}");
        }
        for c in gravado["chats"].as_array().unwrap() {
            for chave in ["projeto", "papel", "pai", "tarefa", "isolado"] {
                assert!(c.get(chave).is_none(), "{chave} em {}", c["id"]);
            }
        }
        // Nada do usuário foi apagado: as mensagens do Mestre e do ajudante, a cópia, a pasta
        // própria e as notas continuam lá.
        for arquivo in [dados.arquivo_chat(ID_MESTRE), dados.arquivo_chat("h1"), dados.pasta.join("notas").join("loja.md")] {
            assert!(arquivo.is_file(), "{}", arquivo.display());
        }
        assert!(copia.is_dir() && propria.is_dir() && loja.is_dir() && velho.is_dir());

        // Reaberto: nada muda, e o estado da versão 2 guardado continua o de antes.
        let Carregado { estado: mut de_novo, .. } = dados.carregar().unwrap();
        let n = normalizar(&mut de_novo, &dados, &consultas, AGORA);
        assert!(n.chats_mexidos.is_empty() && n.sessoes_novas.is_empty() && n.chats_a_apagar.is_empty());
        assert_eq!(de_novo.chats.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c1", "c2", "v1"]);
        assert_eq!(fs::read(dados.pasta.join("estado.versao-2.json")).unwrap(), bytes);
        let _ = fs::remove_dir_all(&raiz);
    }

    #[test]
    #[should_panic(expected = "Dados::em")]
    fn dados_padrao_nao_roda_em_teste() {
        let _ = Dados::padrao();
    }
}
