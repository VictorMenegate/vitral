//! O .md como página de blocos, no jeito do Notion (protótipo `tarefas/linear/vitral-linear.html`,
//! `blocosDe` e `serializar`): cada título, parágrafo, item de lista, tarefa, citação, código,
//! divisor e tabela vira um bloco que se edita no lugar. Ao gravar, o bloco que não foi mexido volta
//! ao arquivo exatamente como estava (as linhas dele e as linhas em branco antes dele); só o que
//! mudou é escrito de novo. A tela é a `pagina.rs`; aqui é só a conta, sem GPUI.

use std::ops::Range;

/// O tipo de um bloco.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tipo {
    Texto,
    Titulo1,
    Titulo2,
    Titulo3,
    Lista,
    Numerada,
    Tarefa,
    Citacao,
    Codigo,
    Divisor,
    Tabela,
}

impl Tipo {
    /// Os tipos do menu do "/", na ordem do protótipo, com o nome e a sigla do quadradinho.
    pub const DO_MENU: [(Tipo, &'static str, &'static str); 10] = [
        (Tipo::Texto, "Texto", "T"),
        (Tipo::Titulo1, "Título 1", "H1"),
        (Tipo::Titulo2, "Título 2", "H2"),
        (Tipo::Titulo3, "Título 3", "H3"),
        (Tipo::Lista, "Lista", "•"),
        (Tipo::Numerada, "Lista numerada", "1."),
        (Tipo::Tarefa, "Tarefa", ""),
        (Tipo::Citacao, "Citação", "“"),
        (Tipo::Codigo, "Código", "</>"),
        (Tipo::Divisor, "Divisor", "—"),
    ];

    /// A chave curta do tipo (a do protótipo), que o filtro do "/" também procura.
    pub fn chave(self) -> &'static str {
        match self {
            Tipo::Texto => "p",
            Tipo::Titulo1 => "h1",
            Tipo::Titulo2 => "h2",
            Tipo::Titulo3 => "h3",
            Tipo::Lista => "ul",
            Tipo::Numerada => "ol",
            Tipo::Tarefa => "tarefa",
            Tipo::Citacao => "citacao",
            Tipo::Codigo => "codigo",
            Tipo::Divisor => "divisor",
            Tipo::Tabela => "tabela",
        }
    }

    /// Item de lista: o Enter continua no mesmo tipo e não há linha em branco entre dois seguidos.
    pub fn de_lista(self) -> bool {
        matches!(self, Tipo::Lista | Tipo::Numerada | Tipo::Tarefa)
    }

    /// O texto dele se edita no campo (o divisor e a tabela, não).
    pub fn editavel(self) -> bool {
        !matches!(self, Tipo::Divisor | Tipo::Tabela)
    }

    /// O que aparece no bloco vazio.
    pub fn marcador(self) -> &'static str {
        match self {
            Tipo::Titulo1 => "Título 1",
            Tipo::Titulo2 => "Título 2",
            Tipo::Titulo3 => "Título 3",
            Tipo::Citacao => "Citação",
            Tipo::Codigo => "Código",
            Tipo::Lista | Tipo::Numerada => "Lista",
            Tipo::Tarefa => "Tarefa",
            _ => "'/' para comandos",
        }
    }
}

/// Um bloco da página.
#[derive(Clone, Debug, PartialEq)]
pub struct Bloco {
    pub tipo: Tipo,
    /// O texto em markdown (com `**`, `*`, `` ` `` e links); no código, o código. Um parágrafo de
    /// várias linhas guarda as quebras.
    pub texto: String,
    /// A tarefa feita.
    pub feito: bool,
    /// A linguagem da cerca do código.
    pub lingua: String,
    /// O recuo de um item de lista (a lista dentro de outra).
    pub recuo: String,
    /// As células da tabela, a primeira fileira é o cabeçalho.
    pub tabela: Vec<Vec<String>>,
    /// Fixo enquanto a página está aberta: a tela o usa como id.
    pub id: u64,
    origem: Option<Origem>,
}

/// Como o bloco estava no arquivo.
#[derive(Clone, Debug, PartialEq)]
struct Origem {
    /// As linhas dele, exatamente.
    fonte: Vec<String>,
    /// As linhas (em branco) entre o bloco de antes e ele.
    antes: Vec<String>,
    /// A posição dele entre os blocos do arquivo.
    indice: usize,
    tipo: Tipo,
    texto: String,
    feito: bool,
    lingua: String,
    recuo: String,
    tabela: Vec<Vec<String>>,
}

impl Bloco {
    pub fn novo(tipo: Tipo, texto: impl Into<String>) -> Bloco {
        Bloco {
            tipo,
            texto: texto.into(),
            feito: false,
            lingua: String::new(),
            recuo: String::new(),
            tabela: Vec::new(),
            id: 0,
            origem: None,
        }
    }

    /// Igual ao que veio do arquivo: volta com as linhas de lá.
    fn intacto(&self) -> Option<&Origem> {
        self.origem.as_ref().filter(|o| {
            o.tipo == self.tipo
                && o.texto == self.texto
                && o.feito == self.feito
                && o.lingua == self.lingua
                && o.recuo == self.recuo
                && o.tabela == self.tabela
        })
    }

    /// O bloco escrito de novo (`n`: o número do item, na lista numerada).
    fn linhas(&self, n: usize) -> Vec<String> {
        let r = &self.recuo;
        let por_linha = |prefixo: &str| self.texto.split('\n').map(|l| format!("{prefixo}{l}")).collect::<Vec<_>>();
        match self.tipo {
            Tipo::Texto if self.texto.trim().is_empty() => Vec::new(),
            Tipo::Texto => self.texto.split('\n').map(str::to_string).collect(),
            Tipo::Titulo1 => vec![format!("# {}", uma_linha(&self.texto))],
            Tipo::Titulo2 => vec![format!("## {}", uma_linha(&self.texto))],
            Tipo::Titulo3 => vec![format!("### {}", uma_linha(&self.texto))],
            Tipo::Lista => vec![format!("{r}- {}", uma_linha(&self.texto))],
            Tipo::Numerada => vec![format!("{r}{n}. {}", uma_linha(&self.texto))],
            Tipo::Tarefa => vec![format!("{r}- [{}] {}", if self.feito { 'x' } else { ' ' }, uma_linha(&self.texto))],
            Tipo::Citacao => por_linha("> "),
            Tipo::Codigo => {
                let mut l = vec![format!("```{}", self.lingua)];
                l.extend(self.texto.split('\n').map(str::to_string));
                l.push("```".into());
                l
            }
            Tipo::Divisor => vec!["---".into()],
            Tipo::Tabela => {
                let Some(cabeca) = self.tabela.first() else { return Vec::new() };
                let fileira = |r: &Vec<String>| format!("| {} |", r.join(" | "));
                let mut l = vec![fileira(cabeca), format!("| {} |", vec!["---"; cabeca.len()].join(" | "))];
                l.extend(self.tabela[1..].iter().map(fileira));
                l
            }
        }
    }
}

/// O título e o item de lista são de uma linha só: a quebra vira espaço.
fn uma_linha(texto: &str) -> String {
    texto.split('\n').map(str::trim).collect::<Vec<_>>().join(" ")
}

/// O frontmatter (o `---` do começo), mostrado como as propriedades da página e gravado como veio.
#[derive(Clone, Debug, PartialEq)]
pub struct Frente {
    pub pares: Vec<(String, String)>,
    fonte: Vec<String>,
}

/// A página inteira.
#[derive(Clone, Debug, PartialEq)]
pub struct Pagina {
    pub frente: Option<Frente>,
    pub blocos: Vec<Bloco>,
    /// As linhas em branco depois do último bloco (com a do fim do arquivo).
    fim: Vec<String>,
    proximo_id: u64,
    /// Quantos blocos vieram do arquivo: os de id maior nasceram na página.
    lidos: u64,
}

fn eh_titulo(l: &str) -> Option<(usize, &str)> {
    let n = l.chars().take_while(|c| *c == '#').count();
    let resto = &l[n..];
    ((1..=3).contains(&n) && resto.starts_with(char::is_whitespace)).then(|| (n, resto.trim_start()))
}

fn eh_divisor(l: &str) -> bool {
    let t = l.trim_end();
    t.len() >= 3 && (t.chars().all(|c| c == '-') || t.chars().all(|c| c == '*'))
}

/// "- x", "* x" (com o recuo): o recuo e o texto.
fn eh_item(l: &str) -> Option<(&str, &str)> {
    let sem = l.trim_start();
    let recuo = &l[..l.len() - sem.len()];
    let resto = sem.strip_prefix("- ").or_else(|| sem.strip_prefix("* "))?;
    Some((recuo, resto.trim_start()))
}

/// "- [ ] x" e "- [x] x": o recuo, se está feita e o texto.
fn eh_tarefa(l: &str) -> Option<(&str, bool, &str)> {
    let (recuo, resto) = eh_item(l)?;
    let (feito, texto) = if let Some(t) = resto.strip_prefix("[ ] ") {
        (false, t)
    } else if let Some(t) = resto.strip_prefix("[x] ").or_else(|| resto.strip_prefix("[X] ")) {
        (true, t)
    } else {
        return None;
    };
    Some((recuo, feito, texto.trim_start()))
}

/// "1. x": o recuo e o texto.
fn eh_numerada(l: &str) -> Option<(&str, &str)> {
    let sem = l.trim_start();
    let recuo = &l[..l.len() - sem.len()];
    let digitos = sem.chars().take_while(char::is_ascii_digit).count();
    if digitos == 0 {
        return None;
    }
    let resto = sem[digitos..].strip_prefix(". ")?;
    Some((recuo, resto.trim_start()))
}

fn eh_linha_de_tabela(l: &str) -> bool {
    let t = l.trim_end();
    t.starts_with('|') && t.ends_with('|') && t.len() > 1
}

fn eh_separador_de_tabela(l: &str) -> bool {
    let t = l.trim().trim_start_matches('|').trim_start().trim_start_matches(':');
    t.starts_with("---")
}

fn celulas(l: &str) -> Vec<String> {
    let t = l.trim();
    let t = t.strip_prefix('|').unwrap_or(t);
    let t = t.strip_suffix('|').unwrap_or(t);
    t.split('|').map(|c| c.trim().to_string()).collect()
}

/// A linha começa outro bloco (fecha o parágrafo).
fn comeca_bloco(l: &str) -> bool {
    let sem = l.trim_start();
    l.starts_with('#') && l.trim_start_matches('#').starts_with(char::is_whitespace)
        || sem.starts_with("```")
        || l.starts_with('>')
        || eh_item(l).is_some()
        || eh_numerada(l).is_some()
        || l.starts_with('|')
        || eh_divisor(l)
}

impl Pagina {
    /// Lê o texto do arquivo (com `\n`; o `\r` do Windows fica a cargo da sessão do editor).
    pub fn ler(texto: &str) -> Pagina {
        let linhas: Vec<&str> = texto.split('\n').collect();
        let mut i = 0;
        let mut frente = None;
        if linhas.first() == Some(&"---")
            && let Some(f) = linhas.iter().skip(1).position(|l| *l == "---").map(|p| p + 1)
        {
            let pares = linhas[1..f]
                .iter()
                .filter_map(|l| l.split_once(':').map(|(k, v)| (k.trim().to_string(), v.trim().to_string())))
                .collect();
            frente = Some(Frente { pares, fonte: linhas[..=f].iter().map(|l| l.to_string()).collect() });
            i = f + 1;
        }
        let mut blocos: Vec<Bloco> = Vec::new();
        let mut brancas: Vec<String> = Vec::new();
        let n = linhas.len();
        while i < n {
            let l = linhas[i];
            if l.trim().is_empty() {
                brancas.push(l.to_string());
                i += 1;
                continue;
            }
            let ini = i;
            let mut b = Bloco::novo(Tipo::Texto, "");
            if let Some(cerca) = l.trim_start().strip_prefix("```") {
                b.tipo = Tipo::Codigo;
                b.lingua = cerca.trim().to_string();
                i += 1;
                let mut codigo = Vec::new();
                while i < n && !linhas[i].trim_start().starts_with("```") {
                    codigo.push(linhas[i]);
                    i += 1;
                }
                // A cerca de fechar (num arquivo sem ela, o código vai até o fim).
                if i < n {
                    i += 1;
                }
                b.texto = codigo.join("\n");
            } else if let Some((nivel, texto)) = eh_titulo(l) {
                b.tipo = [Tipo::Titulo1, Tipo::Titulo2, Tipo::Titulo3][nivel - 1];
                b.texto = texto.to_string();
                i += 1;
            } else if eh_divisor(l) {
                b.tipo = Tipo::Divisor;
                i += 1;
            } else if l.starts_with('>') {
                b.tipo = Tipo::Citacao;
                let mut q = Vec::new();
                while i < n && linhas[i].starts_with('>') {
                    let c = &linhas[i][1..];
                    q.push(c.strip_prefix(' ').unwrap_or(c));
                    i += 1;
                }
                b.texto = q.join("\n");
            } else if eh_linha_de_tabela(l) && linhas.get(i + 1).is_some_and(|s| eh_separador_de_tabela(s)) {
                b.tipo = Tipo::Tabela;
                b.tabela.push(celulas(l));
                i += 2;
                while i < n && linhas[i].starts_with('|') {
                    b.tabela.push(celulas(linhas[i]));
                    i += 1;
                }
            } else if let Some((recuo, feito, texto)) = eh_tarefa(l) {
                (b.tipo, b.recuo, b.feito, b.texto) = (Tipo::Tarefa, recuo.into(), feito, texto.into());
                i += 1;
            } else if let Some((recuo, texto)) = eh_item(l) {
                (b.tipo, b.recuo, b.texto) = (Tipo::Lista, recuo.into(), texto.into());
                i += 1;
            } else if let Some((recuo, texto)) = eh_numerada(l) {
                (b.tipo, b.recuo, b.texto) = (Tipo::Numerada, recuo.into(), texto.into());
                i += 1;
            } else {
                let mut p = vec![l];
                i += 1;
                while i < n && !linhas[i].trim().is_empty() && !comeca_bloco(linhas[i]) {
                    p.push(linhas[i]);
                    i += 1;
                }
                b.texto = p.join("\n");
            }
            b.id = blocos.len() as u64 + 1;
            b.origem = Some(Origem {
                fonte: linhas[ini..i].iter().map(|l| l.to_string()).collect(),
                antes: std::mem::take(&mut brancas),
                indice: blocos.len(),
                tipo: b.tipo,
                texto: b.texto.clone(),
                feito: b.feito,
                lingua: b.lingua.clone(),
                recuo: b.recuo.clone(),
                tabela: b.tabela.clone(),
            });
            blocos.push(b);
        }
        let proximo_id = blocos.len() as u64 + 1;
        Pagina { frente, blocos, fim: brancas, proximo_id, lidos: proximo_id - 1 }
    }

    /// O texto do arquivo: o que não mudou, como veio; o resto, escrito de novo.
    pub fn escrever(&self) -> String {
        self.montar().0.join("\n")
    }

    /// As linhas do arquivo (de 1) do pedaço `a..b` do texto do bloco `id`: a menção de um trecho
    /// selecionado na página.
    pub fn linhas_do_trecho(&self, id: u64, a: usize, b: usize) -> Option<(u32, u32)> {
        let (_, onde) = self.montar();
        let &(_, inicio) = onde.iter().find(|(i, _)| *i == id)?;
        let bloco = &self.blocos[self.indice(id)?];
        // No código, a primeira linha é a da cerca.
        let base = inicio + 1 + usize::from(bloco.tipo == Tipo::Codigo);
        let quebras = |ate: usize| bloco.texto.get(..ate.min(bloco.texto.len())).map_or(0, |t| t.matches('\n').count());
        let (a, b) = (a.min(b), a.max(b));
        let fim = if b > a { b - 1 } else { b };
        Some(((base + quebras(a)) as u32, (base + quebras(fim)) as u32))
    }

    /// As linhas do arquivo e onde começa cada bloco nelas (a posição da primeira linha dele).
    fn montar(&self) -> (Vec<String>, Vec<(u64, usize)>) {
        let mut onde = Vec::new();
        let mut saida: Vec<String> = Vec::new();
        if let Some(f) = &self.frente {
            saida.extend(f.fonte.iter().cloned());
        }
        // A posição no arquivo do bloco de antes, para saber se as linhas em branco dele valem.
        let mut anterior: Option<usize> = None;
        let mut tipo_anterior: Option<Tipo> = None;
        let mut numero = 0;
        for b in &self.blocos {
            numero = if b.tipo == Tipo::Numerada { if tipo_anterior == Some(Tipo::Numerada) { numero + 1 } else { 1 } } else { 0 };
            let (linhas, origem) = match b.intacto() {
                Some(o) => (o.fonte.clone(), Some(o)),
                None => (b.linhas(numero), b.origem.as_ref()),
            };
            if linhas.is_empty() {
                continue;
            }
            // As linhas em branco de antes: as do arquivo quando o vizinho de antes é o mesmo de
            // lá; senão, uma (nenhuma entre dois itens de lista seguidos).
            let mesmo_vizinho = origem.is_some_and(|o| match anterior {
                Some(a) => o.indice == a + 1,
                None => o.indice == 0,
            });
            match origem {
                Some(o) if mesmo_vizinho => saida.extend(o.antes.iter().cloned()),
                _ if saida.is_empty() => {}
                _ if b.tipo.de_lista() && tipo_anterior.is_some_and(Tipo::de_lista) => {}
                _ => saida.push(String::new()),
            }
            onde.push((b.id, saida.len()));
            saida.extend(linhas);
            anterior = b.origem.as_ref().map(|o| o.indice);
            tipo_anterior = Some(b.tipo);
        }
        saida.extend(self.fim.iter().cloned());
        // Arquivo novo sem nada no fim: termina com a quebra de linha.
        if self.fim.is_empty() && !saida.is_empty() && self.blocos.iter().all(|b| b.origem.is_none()) {
            saida.push(String::new());
        }
        (saida, onde)
    }

    /// Um bloco novo, com id próprio (não grava nada: entra onde a tela pôr).
    pub fn bloco(&mut self, tipo: Tipo, texto: impl Into<String>) -> Bloco {
        let mut b = Bloco::novo(tipo, texto);
        b.id = self.proximo_id;
        self.proximo_id += 1;
        b
    }

    /// O bloco nasceu na página (não veio do arquivo).
    pub fn novo(&self, id: u64) -> bool {
        id > self.lidos
    }

    /// Onde está o bloco de `id`.
    pub fn indice(&self, id: u64) -> Option<usize> {
        self.blocos.iter().position(|b| b.id == id)
    }

    /// O número do item da lista numerada em `i` (1 no primeiro de cada sequência).
    pub fn numero(&self, i: usize) -> usize {
        self.blocos[..=i].iter().rev().take_while(|b| b.tipo == Tipo::Numerada).count()
    }
}

/// O atalho de markdown digitado no começo de um bloco de texto: "# " vira título, "- " vira lista e
/// assim por diante. `antes` é o que está antes do cursor.
pub fn atalho(antes: &str) -> Option<Tipo> {
    Some(match antes {
        "# " => Tipo::Titulo1,
        "## " => Tipo::Titulo2,
        "### " => Tipo::Titulo3,
        "- " | "* " => Tipo::Lista,
        "1. " => Tipo::Numerada,
        "[] " | "[ ] " => Tipo::Tarefa,
        "> " => Tipo::Citacao,
        "```" => Tipo::Codigo,
        "---" => Tipo::Divisor,
        _ => return None,
    })
}

/// O "/" digitado no começo do bloco ou depois de um espaço, e o que veio depois dele (o filtro do
/// menu): o filtro e quantos caracteres apagar ao escolher.
pub fn barra(antes: &str) -> Option<(String, usize)> {
    let inicio = antes.rfind('/')?;
    let filtro = &antes[inicio + 1..];
    let antes_da_barra = &antes[..inicio];
    let no_lugar = antes_da_barra.is_empty() || antes_da_barra.ends_with(char::is_whitespace);
    (no_lugar && !filtro.contains(char::is_whitespace)).then(|| (filtro.to_string(), filtro.chars().count() + 1))
}

/// Os tipos do menu do "/" que casam com o filtro (sem acento, pelo nome ou pela chave).
pub fn tipos_do_filtro(filtro: &str) -> Vec<(Tipo, &'static str, &'static str)> {
    let sem = |s: &str| crate::ui::acentos::normalizar(s);
    let f = sem(filtro);
    Tipo::DO_MENU.into_iter().filter(|(t, nome, _)| f.is_empty() || sem(nome).contains(&f) || t.chave().starts_with(&f)).collect()
}

// ---- o texto de um bloco na leitura: o markdown de dentro da linha, sem as marcas ----

/// O estilo de um pedaço do texto lido.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Estilo {
    pub negrito: bool,
    pub italico: bool,
    pub codigo: bool,
    pub riscado: bool,
    pub link: Option<String>,
}

/// O texto sem as marcas, os pedaços com estilo e, para cada byte dele (e um depois do fim), onde
/// ele está no markdown: o clique na leitura põe o cursor no lugar certo do campo.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Lido {
    pub texto: String,
    pub pedacos: Vec<(Range<usize>, Estilo)>,
    pub no_markdown: Vec<usize>,
}

/// O markdown de dentro da linha: **negrito**, *itálico* (e com `_`), `código`, ~~riscado~~,
/// [links](endereço) e o `\` que escapa. As quebras viram espaço, como na leitura do markdown.
pub fn ler_linha(md: &str) -> Lido {
    let mut lido = Lido::default();
    let mut estilos: Vec<Estilo> = Vec::new();
    juntar(md, 0, &Estilo::default(), &mut lido, &mut estilos);
    lido.no_markdown.push(md.len());
    // Os pedaços: bytes seguidos com o mesmo estilo.
    let mut inicio = 0;
    for i in 1..=estilos.len() {
        if i == estilos.len() || estilos[i] != estilos[inicio] {
            if estilos[inicio] != Estilo::default() {
                lido.pedacos.push((inicio..i, estilos[inicio].clone()));
            }
            inicio = i;
        }
    }
    lido
}

fn juntar(md: &str, base: usize, estilo: &Estilo, lido: &mut Lido, estilos: &mut Vec<Estilo>) {
    let por = |c: char, onde: usize, estilo: &Estilo, lido: &mut Lido, estilos: &mut Vec<Estilo>| {
        let c = if c == '\n' { ' ' } else { c };
        for _ in 0..c.len_utf8() {
            lido.no_markdown.push(base + onde);
            estilos.push(estilo.clone());
        }
        lido.texto.push(c);
    };
    let mut i = 0;
    while i < md.len() {
        let resto = &md[i..];
        let c = resto.chars().next().unwrap_or(' ');
        let antes = md[..i].chars().next_back();
        if c == '\\' && resto.len() > 1 {
            let proximo = resto[1..].chars().next().unwrap_or(' ');
            if proximo.is_ascii_punctuation() {
                por(proximo, i + 1, estilo, lido, estilos);
                i += 1 + proximo.len_utf8();
                continue;
            }
        }
        if c == '`'
            && let Some(f) = resto[1..].find('`')
        {
            let s = Estilo { codigo: true, ..estilo.clone() };
            for (k, ch) in resto[1..1 + f].char_indices() {
                por(ch, i + 1 + k, &s, lido, estilos);
            }
            i += f + 2;
            continue;
        }
        for (marca, qual) in [("**", 0), ("__", 0), ("~~", 2)] {
            if resto.starts_with(marca)
                && let Some(f) = resto[2..].find(marca).filter(|f| *f > 0)
            {
                let s = if qual == 0 { Estilo { negrito: true, ..estilo.clone() } } else { Estilo { riscado: true, ..estilo.clone() } };
                juntar(&resto[2..2 + f], base + i + 2, &s, lido, estilos);
                i += f + 4;
                break;
            }
        }
        if i > md.len() || !md.is_char_boundary(i) || &md[i..] != resto {
            continue;
        }
        if (c == '*' || c == '_') && !resto[1..].starts_with(c) {
            // O "_" só marca no começo de palavra (snake_case não vira itálico).
            let comeca = c == '*' || antes.is_none_or(|a| !a.is_alphanumeric());
            if let Some(f) = resto[1..].find(c).filter(|f| *f > 0 && comeca) {
                let depois = resto[1 + f + 1..].chars().next();
                if c == '*' || depois.is_none_or(|d| !d.is_alphanumeric()) {
                    let s = Estilo { italico: true, ..estilo.clone() };
                    juntar(&resto[1..1 + f], base + i + 1, &s, lido, estilos);
                    i += f + 2;
                    continue;
                }
            }
        }
        if c == '['
            && let Some(fecha) = resto.find("](")
            && let Some(fim) = resto[fecha + 2..].find(')')
        {
            let endereco = resto[fecha + 2..fecha + 2 + fim].to_string();
            let s = Estilo { link: Some(endereco), ..estilo.clone() };
            juntar(&resto[1..fecha], base + i + 1, &s, lido, estilos);
            i += fecha + 2 + fim + 1;
            continue;
        }
        por(c, i, estilo, lido, estilos);
        i += c.len_utf8();
    }
}

/// O negrito, o itálico ou o código em volta da seleção `a..b` do texto (Ctrl+B, Ctrl+I, Ctrl+E).
/// Já com a marca em volta, tira. Devolve o texto e a seleção nova.
pub fn marcar(texto: &str, a: usize, b: usize, marca: &str) -> (String, Range<usize>) {
    let (a, b) = (a.min(b), a.max(b));
    let m = marca.len();
    if a >= m && texto[..a].ends_with(marca) && texto[b..].starts_with(marca) {
        let novo = format!("{}{}{}", &texto[..a - m], &texto[a..b], &texto[b + m..]);
        return (novo, a - m..b - m);
    }
    let novo = format!("{}{marca}{}{marca}{}", &texto[..a], &texto[a..b], &texto[b..]);
    (novo, a + m..b + m)
}

#[cfg(test)]
mod testes {
    use super::*;

    const EXEMPLO: &str = "---
titulo: Plano
data: 2026-11-26
---

# Plano da semana

Um parágrafo com **negrito**
quebrado em duas linhas.


## Tarefas

- [ ] Ligar para o fornecedor
- [x] Conferir o estoque
- item solto
  - item de dentro
1. primeiro
2. segundo

> Uma citação
> de duas linhas

```rust
fn main() {}
```

---

| Coluna | Outra |
| --- | --- |
| a | b |

Fim.
";

    #[test]
    fn sem_mexer_o_arquivo_volta_igual() {
        for texto in [EXEMPLO, "", "só uma linha", "sem fim de linha\n\nnem aqui", "\n\n# título depois de brancas\n\n\n", "```\nsem cerca de fechar"] {
            assert_eq!(Pagina::ler(texto).escrever(), texto, "{texto:?}");
        }
    }

    #[test]
    fn os_blocos_saem_com_o_tipo_certo() {
        let p = Pagina::ler(EXEMPLO);
        let tipos: Vec<Tipo> = p.blocos.iter().map(|b| b.tipo).collect();
        use Tipo::*;
        assert_eq!(
            tipos,
            [Titulo1, Texto, Titulo2, Tarefa, Tarefa, Lista, Lista, Numerada, Numerada, Citacao, Codigo, Divisor, Tabela, Texto]
        );
        assert_eq!(p.frente.as_ref().unwrap().pares[0], ("titulo".into(), "Plano".into()));
        assert_eq!(p.blocos[1].texto, "Um parágrafo com **negrito**\nquebrado em duas linhas.");
        assert!(p.blocos[4].feito);
        assert_eq!(p.blocos[6].recuo, "  ");
        assert_eq!(p.blocos[9].texto, "Uma citação\nde duas linhas");
        assert_eq!((p.blocos[10].lingua.as_str(), p.blocos[10].texto.as_str()), ("rust", "fn main() {}"));
        assert_eq!(p.blocos[12].tabela, vec![vec!["Coluna".to_string(), "Outra".into()], vec!["a".into(), "b".into()]]);
        assert_eq!(p.numero(8), 2);
    }

    /// Só a linha do bloco mexido muda no arquivo.
    #[test]
    fn mexer_num_bloco_so_muda_as_linhas_dele() {
        let mut p = Pagina::ler(EXEMPLO);
        p.blocos[3].feito = true;
        p.blocos[0].texto = "Plano do mês".into();
        let novo = p.escrever();
        let esperado = EXEMPLO.replace("# Plano da semana", "# Plano do mês").replace("- [ ] Ligar", "- [x] Ligar");
        assert_eq!(novo, esperado);
    }

    #[test]
    fn um_bloco_novo_entra_com_uma_linha_em_branco_e_a_lista_sem() {
        let mut p = Pagina::ler("# A\n\ntexto\n");
        let novo = p.bloco(Tipo::Lista, "um");
        p.blocos.push(novo);
        let outro = p.bloco(Tipo::Lista, "dois");
        p.blocos.push(outro);
        assert_eq!(p.escrever(), "# A\n\ntexto\n\n- um\n- dois\n");
        // O bloco vazio de texto não vai para o arquivo.
        let vazio = p.bloco(Tipo::Texto, "");
        p.blocos.insert(1, vazio);
        assert_eq!(p.escrever(), "# A\n\ntexto\n\n- um\n- dois\n");
    }

    #[test]
    fn mover_um_bloco_leva_as_linhas_dele_e_ajusta_as_brancas() {
        let mut p = Pagina::ler("# A\n\num\n\n\ndois\n");
        let dois = p.blocos.remove(2);
        p.blocos.insert(1, dois);
        assert_eq!(p.escrever(), "# A\n\ndois\n\num\n");
    }

    #[test]
    fn a_lista_numerada_mexida_conta_de_novo() {
        let mut p = Pagina::ler("1. a\n2. b\n");
        p.blocos[1].texto = "b mudado".into();
        let c = p.bloco(Tipo::Numerada, "c");
        p.blocos.push(c);
        assert_eq!(p.escrever(), "1. a\n2. b mudado\n3. c\n");
    }

    #[test]
    fn trocar_o_tipo_escreve_no_jeito_do_tipo_novo() {
        let mut p = Pagina::ler("texto\n");
        p.blocos[0].tipo = Tipo::Citacao;
        assert_eq!(p.escrever(), "> texto\n");
        p.blocos[0].tipo = Tipo::Codigo;
        p.blocos[0].texto = "a\nb".into();
        assert_eq!(p.escrever(), "```\na\nb\n```\n");
        p.blocos[0].tipo = Tipo::Divisor;
        assert_eq!(p.escrever(), "---\n");
    }

    #[test]
    fn um_arquivo_novo_termina_com_quebra_de_linha() {
        let mut p = Pagina::ler("");
        let b = p.bloco(Tipo::Titulo1, "Novo");
        p.blocos.push(b);
        assert_eq!(p.escrever(), "# Novo\n");
    }

    #[test]
    fn o_trecho_selecionado_da_as_linhas_do_arquivo() {
        let p = Pagina::ler(EXEMPLO);
        // "Plano da semana" é a linha 6 (depois do frontmatter de 4 e uma branca).
        assert_eq!(p.linhas_do_trecho(p.blocos[0].id, 2, 7), Some((6, 6)));
        // A segunda linha do parágrafo.
        let par = &p.blocos[1];
        let segunda = par.texto.find("quebrado").unwrap();
        assert_eq!(p.linhas_do_trecho(par.id, 0, segunda + 3), Some((8, 9)));
        // No código, a linha de dentro da cerca.
        let cod = &p.blocos[10];
        assert_eq!(p.linhas_do_trecho(cod.id, 0, 2), Some((25, 25)));
        assert_eq!(EXEMPLO.lines().nth(24), Some("fn main() {}"));
    }

    #[test]
    fn os_atalhos_e_a_barra() {
        assert_eq!(atalho("# "), Some(Tipo::Titulo1));
        assert_eq!(atalho("1. "), Some(Tipo::Numerada));
        assert_eq!(atalho("[] "), Some(Tipo::Tarefa));
        assert_eq!(atalho("#"), None);
        assert_eq!(barra("/"), Some((String::new(), 1)));
        assert_eq!(barra("abc /tit"), Some(("tit".into(), 4)));
        assert_eq!(barra("a/b"), None);
        assert_eq!(barra("/a b"), None);
        let nomes: Vec<&str> = tipos_do_filtro("tit").iter().map(|(_, n, _)| *n).collect();
        assert_eq!(nomes, ["Título 1", "Título 2", "Título 3"]);
        assert_eq!(tipos_do_filtro("citacao")[0].0, Tipo::Citacao);
        assert_eq!(tipos_do_filtro("lis")[0].0, Tipo::Lista);
        assert!(tipos_do_filtro("ul").iter().any(|(t, _, _)| *t == Tipo::Lista));
    }

    #[test]
    fn a_leitura_tira_as_marcas_e_guarda_onde_cada_letra_esta() {
        let l = ler_linha("um **forte** e `cod` e *it* e [site](https://x.y) e snake_case_aqui");
        assert_eq!(l.texto, "um forte e cod e it e site e snake_case_aqui");
        let estilo_de = |trecho: &str| {
            let i = l.texto.find(trecho).unwrap();
            l.pedacos.iter().find(|(r, _)| r.contains(&i)).map(|(_, e)| e.clone()).unwrap_or_default()
        };
        assert!(estilo_de("forte").negrito);
        assert!(estilo_de("cod").codigo);
        assert!(estilo_de("it").italico);
        assert_eq!(estilo_de("site").link.as_deref(), Some("https://x.y"));
        assert_eq!(estilo_de("snake"), Estilo::default());
        // "forte" começa no byte 5 do markdown ("um **").
        assert_eq!(l.no_markdown[l.texto.find("forte").unwrap()], 5);
        assert_eq!(*l.no_markdown.last().unwrap(), "um **forte** e `cod` e *it* e [site](https://x.y) e snake_case_aqui".len());
        // Acento e escape.
        let l = ler_linha("é \\*literal\\* **ção**");
        assert_eq!(l.texto, "é *literal* ção");
        assert!(l.pedacos.iter().any(|(r, e)| e.negrito && &l.texto[r.clone()] == "ção"));
    }

    #[test]
    fn marcar_poe_e_tira_a_marca() {
        assert_eq!(marcar("um dois", 3, 7, "**"), ("um **dois**".to_string(), 5..9));
        assert_eq!(marcar("um **dois**", 5, 9, "**"), ("um dois".to_string(), 3..7));
        assert_eq!(marcar("abc", 1, 1, "`"), ("a``bc".to_string(), 2..2));
    }
}
