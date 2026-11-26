//! As cores do tema que quem usa escolhe (Configurações › Aparência › Cores), como no protótipo
//! `tarefas/linear/vitral-linear.html` (dono, 25/11/2026: "tem que ficar igual o protótipo"):
//! Destaque, Fundo e Contraste, e os mesmos três para a lista, viram a paleta inteira
//! (`tema::superficie::Variante`) pelas contas do `aplicar()` de lá, na mesma ordem e em `f64`,
//! para dar o mesmo arredondamento do navegador. Aqui também moram o fundo animado e as bordas
//! finas, que a mesma janela do protótipo escolhe.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::tema::superficie::{LINEAR, Variante};

/// Uma cor sem alfa, 0xRRGGBB. No `config.json` e no Copiar vai como "#rrggbb".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cor(pub u32);

impl Cor {
    pub fn hex(self) -> String {
        format!("#{:06x}", self.0 & 0xffffff)
    }

    /// "#3170ff", "3170FF" ou "#37f".
    pub fn ler(texto: &str) -> Option<Cor> {
        let t = texto.trim().trim_start_matches('#');
        let cheio = match t.len() {
            3 => t.chars().flat_map(|c| [c, c]).collect::<String>(),
            6 => t.to_string(),
            _ => return None,
        };
        u32::from_str_radix(&cheio, 16).ok().map(Cor)
    }

    fn rgb(self) -> [f64; 3] {
        [f64::from((self.0 >> 16) & 0xff), f64::from((self.0 >> 8) & 0xff), f64::from(self.0 & 0xff)]
    }
}

impl Serialize for Cor {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&self.hex())
    }
}

impl<'de> Deserialize<'de> for Cor {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let texto = String::deserialize(d)?;
        Cor::ler(&texto).ok_or_else(|| serde::de::Error::custom(format!("cor inválida: {texto}")))
    }
}

/// O que se move atrás da lista.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FundoVivo {
    #[default]
    Smoke,
    Mesh,
    Desligado,
}

/// De onde saem as cores do fundo animado: do tema (as quatro da lista, do escuro ao escolhido) ou
/// as da receita do Smoke (o roxo de 25/11).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CoresDoFundo {
    #[default]
    Tema,
    Receita,
}

/// As cores próprias da lista. Desligada, a lista usa o destaque do tema e o fundo 30% mais
/// escuro que o dele.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DaLista {
    pub ligada: bool,
    #[serde(alias = "acento")]
    pub destaque: Cor,
    pub fundo: Cor,
    pub contraste: u8,
}

impl Default for DaLista {
    fn default() -> Self {
        Paleta::default().lista
    }
}

/// O tema inteiro, como a janela de Aparência do protótipo o guarda.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Paleta {
    /// Um dos `TEMAS` ou "personalizado" (mexeu numa cor, vira este).
    pub nome: String,
    #[serde(alias = "acento")]
    pub destaque: Cor,
    pub fundo: Cor,
    /// De 0 a 100.
    pub contraste: u8,
    pub lista: DaLista,
    #[serde(alias = "fundoVivo")]
    pub fundo_vivo: FundoVivo,
    #[serde(alias = "fundoCores")]
    pub cores_do_fundo: CoresDoFundo,
    pub bordas: bool,
}

/// As cores que o dono deixou na Aparência do protótipo (25/11/2026): é o tema com que o Vitral
/// abre, e o que a variante `LINEAR` do tema guardava fixo.
impl Default for Paleta {
    fn default() -> Self {
        Paleta {
            nome: PERSONALIZADO.into(),
            destaque: Cor(0x3170ff),
            fundo: Cor(0x090a15),
            contraste: 74,
            lista: DaLista { ligada: true, destaque: Cor(0x3170ff), fundo: Cor(0x030926), contraste: 50 },
            fundo_vivo: FundoVivo::Smoke,
            cores_do_fundo: CoresDoFundo::Tema,
            bordas: true,
        }
    }
}

pub const PERSONALIZADO: &str = "personalizado";

/// Os temas prontos, na ordem do menu, e o nome de cada um. Grafite e Noite vêm do protótipo; o
/// Linear de lá saiu e entraram outras cores (dono, 26/11/2026: "tira o linear que tá nos temas e
/// coloca outras cores"). O destaque de cada um aguenta o texto branco por cima (3:1 ou mais).
pub const TEMAS: [(&str, &str); 6] = [
    ("grafite", "Grafite"),
    ("noite", "Noite"),
    ("violeta", "Violeta"),
    ("floresta", "Floresta"),
    ("brasa", "Brasa"),
    ("oceano", "Oceano"),
];

const W: [f64; 3] = [255., 255., 255.];
const K: [f64; 3] = [0., 0., 0.];

/// `a.map((v, i) => v + (b[i] - v) * t)` do protótipo.
fn mix(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// O `css()` do protótipo: cada canal cortado em 0 e 255 e arredondado, com o alfa no fim.
fn rgba(c: [f64; 3], alfa: u32) -> u32 {
    let canal = |v: f64| v.clamp(0., 255.).round() as u32;
    (canal(c[0]) << 24) | (canal(c[1]) << 16) | (canal(c[2]) << 8) | (alfa & 0xff)
}

impl Paleta {
    /// Um dos temas prontos, com o fundo animado, as cores do fundo e as bordas desta (trocar de
    /// tema não mexe nelas).
    pub fn com_tema(&self, nome: &str) -> Option<Paleta> {
        let (destaque, fundo, contraste, lista) = match nome {
            "grafite" => (0x2b63ff, 0x14161f, 50, (0x2b63ff, 0x0a1533, 50)),
            "noite" => (0x3170ff, 0x121a30, 55, (0x3170ff, 0x0b1940, 50)),
            "violeta" => (0x7c5cff, 0x15131f, 50, (0x7c5cff, 0x150f2e, 50)),
            "floresta" => (0x1f9d55, 0x111814, 50, (0x1f9d55, 0x0b1f14, 50)),
            "brasa" => (0xe5533d, 0x1a1413, 50, (0xe5533d, 0x24110d, 50)),
            "oceano" => (0x0e9f8f, 0x0f1719, 50, (0x0e9f8f, 0x082022, 50)),
            _ => return None,
        };
        Some(Paleta {
            nome: nome.into(),
            destaque: Cor(destaque),
            fundo: Cor(fundo),
            contraste,
            lista: DaLista { ligada: true, destaque: Cor(lista.0), fundo: Cor(lista.1), contraste: lista.2 },
            ..self.clone()
        })
    }

    /// As cores da lista que valem: as dela, ou, desligada, as do tema com o fundo 30% mais escuro.
    fn lista_que_vale(&self) -> ([f64; 3], [f64; 3], f64) {
        if self.lista.ligada {
            (self.lista.fundo.rgb(), self.lista.destaque.rgb(), f64::from(self.lista.contraste.min(100)) / 100.)
        } else {
            (mix(self.fundo.rgb(), K, 0.3), self.destaque.rgb(), f64::from(self.contraste.min(100)) / 100.)
        }
    }

    /// A paleta inteira: as contas do `aplicar()` do protótipo sobre o acabamento `LINEAR` (chapado,
    /// sem aro nem sombra), que dá o que não é cor (a margem, o raio, os brancos de hover e borda).
    pub fn variante(&self) -> Variante {
        let b = self.fundo.rgb();
        let a = self.destaque.rgb();
        let f = f64::from(self.contraste.min(100)) / 100.;
        let (lb, la, lf) = self.lista_que_vale();
        let moldura = mix(b, K, 0.45 + 0.3 * f);
        let caixa = rgba(mix(b, W, 0.02 + 0.03 * f), 0xff);
        let tenue = mix(W, b, 0.45 - 0.15 * f);
        Variante {
            vidro: rgba(lb, 0xff),
            vidro_leitura: rgba(lb, 0xe3),
            cartao: rgba(b, 0xf2),
            cartao_elevado: rgba(mix(b, W, 0.035 + 0.04 * f), 0xff),
            flutuante: rgba(moldura, 0xff),
            dialogo: rgba(mix(moldura, b, 0.25), 0xff),
            gaveta: rgba(b, 0xfa),
            preto: rgba(moldura, 0xff),
            selecao: rgba(mix(lb, a, 0.22), 0xff),
            caixa_quente: caixa,
            caixa_preta: caixa,
            texto: rgba(mix(W, b, 0.12 - 0.08 * f), 0xff),
            texto_2: rgba(mix(W, b, 0.18 - 0.08 * f), 0xff),
            tenue: rgba(tenue, 0xff),
            apagado: rgba(mix(W, b, 0.55 - 0.15 * f), 0xff),
            fantasma: rgba(tenue, 0x99),
            base_da_tinta: rgba(b, 0xff),
            destaque: rgba(a, 0xff),
            destaque_hover: rgba(mix(a, W, 0.15), 0xff),
            destaque_ativo: rgba(mix(a, K, 0.12), 0xff),
            destaque_texto: rgba(mix(a, W, 0.45), 0xff),
            // O fio do protótipo (`--t-fio`): a moldura pela metade.
            divisoria: rgba(moldura, 0x80),
            texto_da_lista: rgba(mix(W, lb, 0.18 - 0.08 * lf), 0xff),
            tenue_da_lista: rgba(mix(W, lb, 0.45 - 0.15 * lf), 0xff),
            // A lista escolhe com o destaque dela.
            selecao_da_lista: rgba(mix(lb, la, 0.22), 0xff),
            ..LINEAR
        }
    }

    /// As quatro cores do fundo animado, de 0 a 1, do mais escuro ao mais claro (`coresDoFundo` do
    /// protótipo): a sombra funda do fundo da lista, o fundo dela e um quinto do caminho para o
    /// destaque dela.
    pub fn cores_do_tema_no_fundo(&self) -> [[f32; 3]; 4] {
        let (lb, la, _) = self.lista_que_vale();
        let em_0_a_1 = |c: [f64; 3]| c.map(|v| (v.clamp(0., 255.) / 255.) as f32);
        [em_0_a_1(mix(lb, K, 0.7)), em_0_a_1(mix(lb, K, 0.4)), em_0_a_1(lb), em_0_a_1(mix(lb, la, 0.22))]
    }

    /// O texto do Copiar: o mesmo JSON do Copiar do protótipo, que o Importar de lá também lê.
    pub fn para_compartilhar(&self) -> String {
        serde_json::json!({
            "acento": self.destaque.hex(),
            "fundo": self.fundo.hex(),
            "contraste": self.contraste,
            "lista": {
                "ligada": self.lista.ligada,
                "acento": self.lista.destaque.hex(),
                "fundo": self.lista.fundo.hex(),
                "contraste": self.lista.contraste,
            },
        })
        .to_string()
    }

    /// O Importar: um tema copiado daqui ou do protótipo. As cores entram; o fundo animado e as
    /// bordas continuam os desta. `None` se o texto não é um tema.
    pub fn importada(&self, texto: &str) -> Option<Paleta> {
        #[derive(Deserialize)]
        struct Compartilhada {
            #[serde(alias = "acento")]
            destaque: Cor,
            fundo: Cor,
            #[serde(default)]
            contraste: Option<u8>,
            #[serde(default)]
            lista: Option<DaLista>,
        }
        let c: Compartilhada = serde_json::from_str(texto.trim()).ok()?;
        let base = self.com_tema("grafite")?;
        Some(Paleta {
            nome: PERSONALIZADO.into(),
            destaque: c.destaque,
            fundo: c.fundo,
            contraste: c.contraste.unwrap_or(base.contraste).min(100),
            lista: c.lista.unwrap_or(base.lista),
            ..self.clone()
        })
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    /// A paleta que o Vitral abre é a que o dono deixou no protótipo, e as contas dão as mesmas cores
    /// que o navegador calculou lá (medidas com getComputedStyle em 26/11/2026) e que a variante
    /// `LINEAR` guardava.
    #[test]
    fn a_paleta_padrao_da_as_cores_medidas_no_prototipo() {
        let v = Paleta::default().variante();
        let rgb = |c: u32| c >> 8;
        assert_eq!(rgb(v.vidro), 0x030926, "--t-base");
        assert_eq!(rgb(v.cartao), 0x090a15, "--t-painel");
        assert_eq!(rgb(v.caixa_preta), 0x13141f, "--t-caixa");
        assert_eq!(rgb(v.cartao_elevado), 0x191a24, "--t-cartao");
        assert_eq!(rgb(v.preto), 0x030307, "--t-moldura");
        assert_eq!(rgb(v.selecao), 0x0d2056, "--t-escolhido");
        assert_eq!(rgb(v.selecao_da_lista), 0x0d2056, "--t-lista-escolhido");
        assert_eq!(rgb(v.texto_da_lista), 0xdcdde1, "--t-lista-texto");
        assert_eq!(rgb(v.tenue_da_lista), 0xa1a3ae, "--t-lista-tenue");
        assert_eq!(rgb(v.texto), 0xf0f0f1, "--texto");
        assert_eq!(rgb(v.texto_2), 0xe1e1e3, "--texto-2");
        assert_eq!(rgb(v.tenue), 0xacacb0, "--tenue");
        assert_eq!(rgb(v.apagado), 0x939398, "--apagado");
        assert_eq!(rgb(v.destaque), 0x3170ff, "--azul");
        assert_eq!(rgb(v.destaque_hover), 0x5085ff, "--azul-hover");
        assert_eq!(rgb(v.destaque_texto), 0x8eb0ff, "--azul-texto");
        assert_eq!(v.divisoria, 0x03030780, "--t-fio");
        // E o resto (raio, margem, sem aro) é o do acabamento Linear.
        assert_eq!((v.margem, v.raio_peca, v.aro, v.sombra), (LINEAR.margem, LINEAR.raio_peca, 0, 0.));
    }

    #[test]
    fn as_cores_do_fundo_vivo_sao_as_do_prototipo() {
        let c = Paleta::default().cores_do_tema_no_fundo();
        let perto = |a: f32, b: f32| (a - b).abs() < 1e-6;
        assert!(perto(c[0][2], 0.044_705_882), "{:?}", c[0]);
        assert!(perto(c[2][0], 0.011_764_706), "{:?}", c[2]);
        assert!(perto(c[3][2], 0.336_235_29), "{:?}", c[3]);
    }

    #[test]
    fn a_lista_desligada_segue_o_tema() {
        let mut p = Paleta::default();
        p.lista = DaLista { ligada: false, destaque: Cor(0xff0000), fundo: Cor(0xffffff), contraste: 0 };
        let v = p.variante();
        // O fundo do tema 30% mais escuro, e o escolhido no destaque do tema.
        assert_eq!(v.vidro >> 8, 0x06070f);
        assert_eq!(v.selecao_da_lista, v.selecao);
    }

    #[test]
    fn trocar_de_tema_mantem_o_fundo_e_as_bordas() {
        let mut p = Paleta::default();
        p.fundo_vivo = FundoVivo::Desligado;
        p.bordas = false;
        let noite = p.com_tema("noite").unwrap();
        assert_eq!((noite.nome.as_str(), noite.fundo, noite.contraste), ("noite", Cor(0x121a30), 55));
        assert_eq!((noite.fundo_vivo, noite.bordas), (FundoVivo::Desligado, false));
        assert!(p.com_tema("outro").is_none());
    }

    #[test]
    fn copiar_e_importar_dao_o_mesmo_tema() {
        let p = Paleta::default().com_tema("brasa").unwrap();
        let texto = p.para_compartilhar();
        assert!(texto.contains("\"acento\":\"#e5533d\""), "{texto}");
        let de_volta = Paleta::default().importada(&texto).unwrap();
        assert_eq!((de_volta.destaque, de_volta.fundo, de_volta.contraste, de_volta.lista), (p.destaque, p.fundo, p.contraste, p.lista));
        assert_eq!(de_volta.nome, PERSONALIZADO);
        // O que o Copiar do protótipo dá também entra; o que não é tema, não.
        let do_prototipo = r##"{"acento":"#3170ff","fundo":"#090a15","contraste":74,"lista":{"ligada":true,"acento":"#3170ff","fundo":"#030926","contraste":50}}"##;
        assert_eq!(Paleta::default().importada(do_prototipo).unwrap().variante().vidro >> 8, 0x030926);
        assert!(Paleta::default().importada("olá").is_none());
        assert!(Paleta::default().importada(r##"{"acento":"azul","fundo":"#000"}"##).is_none());
    }

    #[test]
    fn todo_tema_pronto_existe_e_o_destaque_aguenta_o_branco() {
        let luminancia = |c: u32| {
            let canal = |v: u32| {
                let v = f64::from(v & 0xff) / 255.;
                if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            0.2126 * canal(c >> 16) + 0.7152 * canal(c >> 8) + 0.0722 * canal(c)
        };
        for (nome, _) in TEMAS {
            let p = Paleta::default().com_tema(nome).unwrap_or_else(|| panic!("{nome} sem cores"));
            let contraste = 1.05 / (luminancia(p.destaque.0) + 0.05);
            assert!(contraste >= 3., "{nome}: branco sobre o destaque dá {contraste:.2}");
        }
        assert!(TEMAS.iter().all(|(nome, _)| *nome != "linear"));
    }

    #[test]
    fn a_cor_le_e_escreve_hex() {
        assert_eq!(Cor::ler("#37F"), Some(Cor(0x3377ff)));
        assert_eq!(Cor::ler(" 3170ff "), Some(Cor(0x3170ff)));
        assert_eq!(Cor::ler("#12345"), None);
        assert_eq!(Cor(0x0068ff).hex(), "#0068ff");
    }

    #[test]
    fn o_config_sem_paleta_abre_com_a_padrao() {
        let p: Paleta = serde_json::from_str(r#"{"fundoVivo":"mesh"}"#).unwrap();
        assert_eq!(p.fundo_vivo, FundoVivo::Mesh);
        assert_eq!(p.destaque, Paleta::default().destaque);
    }
}
