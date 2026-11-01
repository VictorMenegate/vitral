//! Ícone e cor do atalho de cada projeto no trilho: o que o usuário escolheu (guardado em
//! `ConfigProjeto`) ou, sem escolha, um palpite pelo conteúdo da pasta e uma cor pelo nome.
//! Os SVGs são do Lucide (ISC, `assets/icones/LICENSE-LUCIDE`), embutidos no binário.

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    rc::Rc,
    time::{Duration, Instant},
};

use gpui_kit::component::{
    Icon, Sizable as _, WindowExt as _, h_flex,
    input::{InputEvent, InputState},
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::{
    Janela,
    componentes::{
        EstadoVazio, QuadradoDeProjeto, RodapeDialogo, TamanhoBotao, botao_fantasma,
        botao_primario, campo_de_busca,
    },
};
use crate::{
    modelo::ConfigProjeto,
    motor::Motor,
    tema::{Cores, Lucide, Tipo as _, halo_foco, r, raio, tam},
};

pub struct Icone {
    /// Nome do arquivo no Lucide; é o que vai para a configuração.
    pub id: &'static str,
    pub nome: &'static str,
    svg: &'static [u8],
}

macro_rules! icone {
    ($id:literal, $nome:literal) => {
        Icone {
            id: $id,
            nome: $nome,
            svg: include_bytes!(concat!("../../assets/icones/", $id, ".svg")),
        }
    };
}

/// Ícones para escolher, em fileiras de dez: código, aparelhos, negócio, criação, ferramentas e
/// coisas da vida. O primeiro (pasta) é o automático quando não há palpite.
pub static ICONES: &[Icone] = &[
    icone!("folder", "Pasta"),
    icone!("code", "Código"),
    icone!("square-terminal", "Terminal"),
    icone!("git-branch", "Git"),
    icone!("bug", "Bug"),
    icone!("package", "Pacote"),
    icone!("puzzle", "Extensão"),
    icone!("cpu", "Chip"),
    icone!("server", "Servidor"),
    icone!("database", "Banco de dados"),
    icone!("globe", "Site"),
    icone!("cloud", "Nuvem"),
    icone!("smartphone", "Celular"),
    icone!("monitor", "Computador"),
    icone!("tv", "TV"),
    icone!("gamepad-2", "Jogo"),
    icone!("headphones", "Fone"),
    icone!("wifi", "Rede"),
    icone!("bot", "Robô"),
    icone!("brain", "IA"),
    icone!("shopping-bag", "Loja"),
    icone!("shopping-cart", "Carrinho"),
    icone!("store", "Comércio"),
    icone!("wallet", "Carteira"),
    icone!("chart-line", "Gráfico"),
    icone!("briefcase", "Trabalho"),
    icone!("truck", "Entrega"),
    icone!("mail", "E-mail"),
    icone!("message-circle", "Mensagens"),
    icone!("calendar", "Agenda"),
    icone!("palette", "Paleta"),
    icone!("pen-tool", "Design"),
    icone!("camera", "Câmera"),
    icone!("film", "Vídeo"),
    icone!("image", "Imagem"),
    icone!("music", "Música"),
    icone!("book-open", "Livro"),
    icone!("file-text", "Documento"),
    icone!("newspaper", "Notícias"),
    icone!("graduation-cap", "Estudo"),
    icone!("wrench", "Ferramenta"),
    icone!("hammer", "Martelo"),
    icone!("flask-conical", "Laboratório"),
    icone!("rocket", "Foguete"),
    icone!("zap", "Raio"),
    icone!("lightbulb", "Ideia"),
    icone!("sparkles", "Brilho"),
    icone!("shield", "Segurança"),
    icone!("key", "Chave"),
    icone!("layers", "Camadas"),
    icone!("house", "Casa"),
    icone!("sofa", "Móveis"),
    icone!("baby", "Bebê"),
    icone!("heart", "Coração"),
    icone!("star", "Estrela"),
    icone!("leaf", "Folha"),
    icone!("coffee", "Café"),
    icone!("utensils", "Comida"),
    icone!("map", "Mapa"),
    icone!("trophy", "Troféu"),
];

/// Tons do vidro para os atalhos (decisão 1 da linguagem): sem o azul de destaque e as cores de
/// estado, senão um projeto parece estar com erro ou selecionado.
pub use crate::tema::CORES_PROJETO as CORES;

pub fn icone_por_id(id: &str) -> Option<&'static Icone> {
    ICONES.iter().find(|i| i.id == id)
}

/// Cor gravada como "#rrggbb". As da paleta antiga viram as novas: a Névoa (parecia
/// desabilitada) vira Areia, e o azul colado no destaque vira o Azul da paleta.
pub fn ler_cor(texto: &str) -> Option<u32> {
    let hex = texto.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    Some(match u32::from_str_radix(hex, 16).ok()? {
        0xa9b5d6 => 0xc8b98f,
        0x4f8dff => 0x8fb6ff,
        cor => cor,
    })
}

/// Cor automática: sempre a mesma para o mesmo nome.
pub fn cor_automatica(nome: &str) -> u32 {
    let soma: u32 = nome.bytes().map(u32::from).sum();
    CORES[soma as usize % CORES.len()].0
}

/// Ícone pelo tipo de projeto, só pelos nomes no topo da pasta (uma leitura de diretório).
pub fn palpite(pasta: &Path) -> &'static str {
    let nomes: HashSet<String> = std::fs::read_dir(pasta)
        .map(|entradas| entradas.flatten().filter_map(|e| e.file_name().into_string().ok()).collect())
        .unwrap_or_default();
    let tem = |nome: &str| nomes.contains(nome);
    let algum = |lista: &[&str]| lista.iter().any(|n| tem(n));
    let comeca = |prefixos: &[&str]| nomes.iter().any(|n| prefixos.iter().any(|p| n.starts_with(p)));
    let extensao = |lista: &[&str]| {
        nomes.iter().any(|n| {
            n.rsplit_once('.')
                .is_some_and(|(base, ext)| !base.is_empty() && lista.contains(&ext))
        })
    };
    if tem("metadata.json") && tem("extension.js") {
        "puzzle"
    } else if tem("project.godot") || (tem("Assets") && tem("ProjectSettings")) {
        "gamepad-2"
    } else if algum(&["pubspec.yaml", "AndroidManifest.xml"]) || (tem("android") && tem("ios")) {
        "smartphone"
    } else if tem("manage.py") {
        "server"
    } else if algum(&["index.html", "angular.json"])
        || comeca(&[
            "next.config.",
            "vite.config.",
            "nuxt.config.",
            "astro.config.",
            "svelte.config.",
            "gatsby-config.",
            "remix.config.",
        ])
    {
        "globe"
    } else if tem("Cargo.toml") {
        "cpu"
    } else if algum(&["Dockerfile", "docker-compose.yml", "docker-compose.yaml", "compose.yml", "compose.yaml"]) {
        "server"
    } else if algum(&["mkdocs.yml", "book.toml"]) {
        "book-open"
    } else if algum(&[
        "package.json",
        "pyproject.toml",
        "requirements.txt",
        "setup.py",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "build.gradle.kts",
        "CMakeLists.txt",
        "meson.build",
        "Makefile",
        "composer.json",
        "Gemfile",
    ]) || extensao(&[
        "rs", "py", "js", "mjs", "cjs", "ts", "tsx", "jsx", "go", "c", "cpp", "h", "java", "kt", "rb",
        "php", "lua", "sh", "swift", "cs",
    ]) {
        "code"
    } else if extensao(&["md", "txt", "pdf", "doc", "docx", "odt"]) {
        "file-text"
    } else {
        "folder"
    }
}

thread_local! {
    /// Palpite de cada pasta e quando foi feito: o trilho redesenha a cada quadro e não pode ler o
    /// disco toda vez.
    static PALPITES: RefCell<HashMap<PathBuf, (Instant, &'static str)>> = RefCell::default();
}

fn palpite_guardado(pasta: &Path) -> &'static str {
    PALPITES.with_borrow_mut(|cache| {
        if let Some((quando, id)) = cache.get(pasta)
            && quando.elapsed() < Duration::from_secs(60)
        {
            return *id;
        }
        let id = palpite(pasta);
        cache.insert(pasta.to_path_buf(), (Instant::now(), id));
        id
    })
}

/// Ícone e cor de um projeto: a escolha do usuário ou, sem ela, os automáticos.
fn escolha(config: Option<&ConfigProjeto>, nome: &str, pasta: &Path) -> (&'static Icone, u32) {
    let icone = config
        .and_then(|c| icone_por_id(&c.icone))
        .or_else(|| icone_por_id(palpite_guardado(pasta)))
        .unwrap_or(&ICONES[0]);
    (icone, cor_escolhida(config, nome))
}

fn cor_escolhida(config: Option<&ConfigProjeto>, nome: &str) -> u32 {
    config.and_then(|c| ler_cor(&c.cor)).unwrap_or_else(|| cor_automatica(nome))
}

/// O que o trilho mostra para o projeto.
pub fn visual(motor: &Motor, projeto: &str) -> (&'static Icone, u32) {
    escolha(motor.config.projetos.get(projeto), projeto, &motor.caminho_projeto(projeto))
}

/// Desenha direto dos bytes embutidos (o GPUI guarda o resultado pelo hash), sem precisar
/// registrar os SVGs no AssetSource do app. O tamanho é de desenho, em rem: cresce com o zoom.
pub fn desenho(icone: &'static Icone, tamanho: f32, cor: impl Into<Hsla>) -> Icon {
    Icon::empty()
        .data(icone.svg)
        .size(r(tamanho))
        .text_color(cor)
}

/// Grava a escolha na configuração da pasta (`None` volta ao automático).
fn gravar(motor: &Entity<Motor>, projeto: &str, icone: Option<&'static str>, cor: Option<u32>, cx: &mut App) {
    let projeto = projeto.to_string();
    motor.update(cx, |m, cx| {
        m.mudar_config(
            |c| {
                let p = c.projetos.entry(projeto).or_default();
                p.icone = icone.unwrap_or_default().to_string();
                p.cor = cor.map(|c| format!("#{c:06x}")).unwrap_or_default();
            },
            cx,
        )
    });
}

/// "Usar automático" do clique direito no projeto: apaga a escolha gravada.
pub(super) fn usar_automatico(motor: &Entity<Motor>, projeto: &str, cx: &mut App) {
    gravar(motor, projeto, None, None, cx);
}

/// Os ícones cujo nome (ou arquivo) tem a busca, sem diferença de maiúsculas.
fn filtrar(busca: &str) -> Vec<&'static Icone> {
    let busca = busca.trim().to_lowercase();
    ICONES
        .iter()
        .filter(|i| busca.is_empty() || i.nome.to_lowercase().contains(&busca) || i.id.contains(&busca))
        .collect()
}

/// Nome da cor na paleta, ou "Personalizada" para uma gravada à mão.
fn nome_da_cor(cor: u32) -> &'static str {
    CORES.iter().find(|(valor, _)| *valor == cor).map(|(_, nome)| *nome).unwrap_or("Personalizada")
}

impl Janela {
    /// "Ícone e cor…" do menu do projeto: a escolha num diálogo de 460, que só grava ao salvar.
    pub(super) fn dialogo_icone_projeto(&mut self, projeto: String, window: &mut Window, cx: &mut Context<Self>) {
        let Some(motor) = self.motor.clone() else {
            return;
        };
        let escolha = cx.new(|cx| EscolhaIconeCor::sem_retorno(motor, projeto.clone(), cx));
        window.open_dialog(cx, move |dialogo, window, cx| {
            let (automatico, salvar, enter) = (escolha.clone(), escolha.clone(), escolha.clone());
            let rodape = RodapeDialogo::new()
                .esquerda(
                    botao_fantasma("icone-automatico", Some(Lucide::Undo2.into()), "Usar automático", TamanhoBotao::Controle, window, cx)
                        .tooltip("Ícone pelo conteúdo da pasta e cor pelo nome")
                        .on_click(move |_, window, cx| automatico.update(cx, |e, cx| e.usar_automatico(window, cx))),
                )
                .cancelar(
                    botao_fantasma("icone-cancelar", None, "Cancelar", TamanhoBotao::Controle, window, cx)
                        .on_click(|_, window, cx| window.close_dialog(cx)),
                )
                .principal(
                    botao_primario("icone-salvar", None, "Salvar", TamanhoBotao::Controle, window, cx).on_click(
                        move |_, window, cx| {
                            salvar.update(cx, |e, cx| e.gravar(cx));
                            window.close_dialog(cx);
                        },
                    ),
                );
            // A moldura dos diálogos; 460 porque a grade de ícones tem 396.
            super::dialogos::moldura(dialogo, 460., window)
                .title(super::dialogos::topo(super::dialogos::cabecalho("Ícone e cor do projeto", Some(projeto.clone()))))
                .child(escolha.clone())
                .footer(rodape)
                .on_ok(move |_, _, cx| {
                    enter.update(cx, |e, cx| e.gravar(cx));
                    true
                })
        });
    }
}

/// Quem embute a escolha recebe cada mudança (`None` é automático).
type AoMudar = Rc<dyn Fn(Option<&'static Icone>, Option<u32>, &mut Window, &mut App)>;

/// A escolha de ícone e cor do projeto: prévia, busca, grade de ícones e cores. A escolhida
/// fica na cor do projeto. O diálogo "Ícone e cor…" grava ao salvar; embutida (Configurar
/// projeto, Novo projeto), cada mudança vai para quem embutiu.
pub struct EscolhaIconeCor {
    motor: Entity<Motor>,
    projeto: String,
    /// `None` é automático.
    icone: Option<&'static Icone>,
    cor: Option<u32>,
    ao_mudar: Option<AoMudar>,
    /// Criada no primeiro desenho (o campo precisa da janela).
    busca: Option<Entity<InputState>>,
    _assinatura: Option<Subscription>,
}

impl EscolhaIconeCor {
    pub fn new(
        motor: Entity<Motor>,
        projeto: String,
        ao_mudar: impl Fn(Option<&'static Icone>, Option<u32>, &mut Window, &mut App) + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        Self { ao_mudar: Some(Rc::new(ao_mudar)), ..Self::sem_retorno(motor, projeto, cx) }
    }

    fn sem_retorno(motor: Entity<Motor>, projeto: String, cx: &mut Context<Self>) -> Self {
        let config = motor.read(cx).config.projetos.get(&projeto);
        let icone = config.and_then(|c| icone_por_id(&c.icone));
        let cor = config.and_then(|c| ler_cor(&c.cor));
        Self { motor, projeto, icone, cor, ao_mudar: None, busca: None, _assinatura: None }
    }

    /// Grava a escolha na configuração da pasta (quem embute decide quando).
    pub fn gravar(&self, cx: &mut App) {
        gravar(&self.motor, &self.projeto, self.icone.map(|i| i.id), self.cor, cx);
    }

    /// Volta ao palpite pelo conteúdo da pasta e à cor pelo nome.
    pub fn usar_automatico(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.icone = None;
        self.cor = None;
        self.mudou(window, cx);
    }

    /// Avisa quem embutiu a escolha.
    fn mudou(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(f) = &self.ao_mudar {
            f(self.icone, self.cor, window, cx);
        }
        cx.notify();
    }
}

impl Render for EscolhaIconeCor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.busca.is_none() {
            let busca = cx.new(|cx| InputState::new(window, cx).placeholder("Buscar ícone"));
            self._assinatura = Some(cx.subscribe(&busca, |_, _, evento: &InputEvent, cx| {
                if matches!(evento, InputEvent::Change) {
                    cx.notify();
                }
            }));
            self.busca = Some(busca);
        }
        let busca = self.busca.clone().expect("criada acima");
        let consulta = busca.read(cx).value().to_string();
        let motor = self.motor.read(cx);
        let (icone_auto, cor_auto) = escolha(None, &self.projeto, &motor.caminho_projeto(&self.projeto));
        let icone = self.icone.unwrap_or(icone_auto);
        let cor = self.cor.unwrap_or(cor_auto);
        let cor_hsla = Hsla::from(rgb(cor));
        let descricao = format!(
            "{}{} · {}{}",
            icone.nome,
            if self.icone.is_none() { " (automático)" } else { "" },
            nome_da_cor(cor),
            if self.cor.is_none() { " (automática)" } else { "" },
        );

        let previa = h_flex()
            .gap(r(16.))
            .child(QuadradoDeProjeto::new(icone, cor, 56.))
            .child(
                v_flex()
                    .gap(r(2.))
                    .min_w_0()
                    .child(div().titulo_cartao().text_color(Cores::texto()).truncate().child(self.projeto.clone()))
                    .child(div().text_size(r(13.)).text_color(Cores::tenue()).child(descricao)),
            );

        let achados = filtrar(&consulta);
        let grade = if achados.is_empty() {
            EstadoVazio::new(Lucide::Search, format!("Nenhum ícone com “{}”", consulta.trim())).into_any_element()
        } else {
            div()
                .id("grade-icones")
                .flex()
                .flex_wrap()
                .gap(r(4.))
                .w(r(396.))
                .max_h(r(236.))
                .overflow_y_scroll()
                .children(achados.into_iter().map(|item| {
                    let escolhido = item.id == icone.id;
                    div()
                        .id(SharedString::from(format!("icone-projeto-{}", item.id)))
                        .size(r(36.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(raio::CONTROLE)
                        .cursor_pointer()
                        .border_1()
                        .border_color(if escolhido { Cores::tinta_borda(cor_hsla) } else { transparent_black() })
                        .when(escolhido, |s| s.bg(Cores::tinta(cor_hsla)))
                        .when(!escolhido, |s| s.hover(|s| s.bg(Cores::hover())))
                        .tab_index(0)
                        .focus_visible(|s| s.shadow(halo_foco()))
                        .child(
                            desenho(item, 18., if escolhido { cor_hsla } else { Cores::tenue() })
                                .with_size(tam(window, 18.)),
                        )
                        .tooltip(move |window, cx| Tooltip::new(item.nome).build(window, cx))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.icone = Some(item);
                            this.mudou(window, cx);
                        }))
                }))
                .into_any_element()
        };

        let cores = h_flex().gap(r(8.)).children(CORES.iter().map(|&(valor, nome)| {
            let escolhida = valor == cor;
            div()
                .id(SharedString::from(format!("cor-projeto-{valor:06x}")))
                .size(r(32.))
                .p(r(2.))
                .rounded_full()
                .cursor_pointer()
                .border_2()
                .border_color(if escolhida { rgb(valor).into() } else { transparent_black() })
                .when(!escolhida, |s| s.hover(|s| s.border_color(Cores::borda_forte())))
                .tab_index(0)
                .focus_visible(|s| s.shadow(halo_foco()))
                .child(div().size_full().rounded_full().bg(rgb(valor)))
                .tooltip(move |window, cx| Tooltip::new(nome).build(window, cx))
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.cor = Some(valor);
                    this.mudou(window, cx);
                }))
        }));

        v_flex()
            .gap(r(16.))
            .child(previa)
            .child(
                v_flex()
                    .gap(r(8.))
                    .child(
                        h_flex()
                            .gap(r(8.))
                            .child(div().flex_1().rotulo_secao().child("Ícone"))
                            .child(div().w(r(200.)).child(campo_de_busca("busca-icone", &busca, None, window, cx))),
                    )
                    .child(grade),
            )
            .child(v_flex().gap(r(8.)).child(div().rotulo_secao().child("Cor")).child(cores))
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: o `gpui_kit::*` de lá traz um `test` que esconde o `#[test]`.
    use super::{CORES, ConfigProjeto, cor_automatica, escolha, filtrar, icone_por_id, ler_cor, nome_da_cor, palpite};

    #[test]
    fn cores_de_projeto_sem_cores_de_estado() {
        for estado in [0x3a7fff, 0x4ade80, 0xf5b642, 0xf87171] {
            assert!(CORES.iter().all(|&(cor, _)| cor != estado));
        }
        assert!(CORES.iter().any(|&(cor, _)| cor == cor_automatica("site")));
    }

    #[test]
    fn busca_de_icone_pelo_nome_em_portugues() {
        assert_eq!(filtrar("").len(), 60);
        let nomes = |busca: &str| filtrar(busca).iter().map(|i| i.nome).collect::<Vec<_>>();
        assert_eq!(nomes("banco"), ["Banco de dados"]);
        assert_eq!(nomes(" LOJA "), ["Loja"]);
        assert!(nomes("cart").contains(&"Carrinho") && nomes("cart").contains(&"Carteira"));
        assert!(filtrar("xyzzy").is_empty());
        assert_eq!(nome_da_cor(0xc8b98f), "Areia");
        assert_eq!(nome_da_cor(0x123456), "Personalizada");
    }

    #[test]
    fn palpite_pelo_conteudo_da_pasta() {
        let base = std::env::temp_dir().join(format!("vitral-icones-{}", crate::modelo::novo_id()));
        let pasta = |nome: &str, arquivos: &[&str]| {
            let p = base.join(nome);
            std::fs::create_dir_all(&p).unwrap();
            for a in arquivos {
                std::fs::write(p.join(a), "").unwrap();
            }
            p
        };
        let casos = [
            (pasta("site", &["package.json", "next.config.ts"]), "globe"),
            (pasta("loja", &["manage.py", "requirements.txt", "Dockerfile"]), "server"),
            (pasta("extensao", &["metadata.json", "extension.js", "prefs.js"]), "puzzle"),
            (pasta("app", &["Cargo.toml", "Cargo.lock"]), "cpu"),
            (pasta("scripts", &["auditoria.mjs"]), "code"),
            (pasta("notas", &["ideias.md"]), "file-text"),
            (pasta("vazia", &[]), "folder"),
            (base.join("nao-existe"), "folder"),
        ];
        for (caminho, esperado) in &casos {
            assert_eq!(palpite(caminho), *esperado, "{}", caminho.display());
            assert!(icone_por_id(esperado).is_some());
        }
        // Escolha gravada ganha do palpite; cor inválida volta à automática.
        let config = ConfigProjeto {
            icone: "rocket".into(),
            cor: "azul".into(),
            ..Default::default()
        };
        let (icone, cor) = escolha(Some(&config), "site", &casos[0].0);
        assert_eq!(icone.id, "rocket");
        assert_eq!(cor, cor_automatica("site"));
        assert_eq!(escolha(None, "site", &casos[0].0).0.id, "globe");
        assert_eq!(ler_cor("#b39dff"), Some(0xb39dff));
        assert_eq!(ler_cor("#a9b5d6"), Some(0xc8b98f));
        assert_eq!(ler_cor("#4f8dff"), Some(0x8fb6ff));
        let _ = std::fs::remove_dir_all(&base);
    }
}
