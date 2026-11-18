//! O quadro de ideias: um kanban com quatro colunas (Ideias, Próximas, Fazendo, Feitas). Cada
//! cartão é um pedido pronto para virar chat: "Abrir chat" cria a conversa na pasta do cartão, já
//! com o pedido, e o cartão passa para Fazendo, ligado a ela. Com o Linear ligado (`crate::linear`),
//! o quadro é o do time: cada cartão espelha uma issue, e o que é só do Vitral (a pasta, o chat, o
//! porquê) fica no cartão. O motor guarda tudo em `ideias.json`, na pasta de dados, e o quadro abre
//! com o que foi lido por último; um chat anota cartões pela ferramenta `anotar_ideia`.
//!
//! A ordem dentro de uma coluna é a `ordem` de cada cartão, crescente (a do Linear é o `sortOrder`).

use std::{collections::HashSet, path::PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coluna {
    #[default]
    Ideias,
    Proximas,
    Fazendo,
    Feitas,
}

impl Coluna {
    /// Na ordem do quadro, da esquerda para a direita.
    pub const TODAS: [Coluna; 4] = [Coluna::Ideias, Coluna::Proximas, Coluna::Fazendo, Coluna::Feitas];

    pub fn titulo(self) -> &'static str {
        match self {
            Coluna::Ideias => "Ideias",
            Coluna::Proximas => "Próximas",
            Coluna::Fazendo => "Fazendo",
            Coluna::Feitas => "Feitas",
        }
    }

    /// O que a coluna vazia diz.
    pub fn vazia(self) -> &'static str {
        match self {
            Coluna::Ideias => "Escreva uma ideia aqui em cima, ou peça a um chat: \u{201c}anota isso no quadro\u{201d}.",
            Coluna::Proximas => "Arraste para cá o que vem a seguir.",
            Coluna::Fazendo => "Ao abrir o chat de uma ideia, ela vem para cá.",
            Coluna::Feitas => "Arraste para cá o que ficou pronto.",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ideia {
    pub id: String,
    pub titulo: String,
    /// O texto que vai para o chat. Vazio, vai o título.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub pedido: String,
    /// Por que vale a pena (o que um chat anotou junto); o cartão mostra embaixo do título.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub nota: String,
    /// Fatos curtos do cartão ("P", "conversa").
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub etiquetas: Vec<String>,
    #[serde(default)]
    pub coluna: Coluna,
    /// Onde o chat da ideia abre. Sem pasta, ele abre numa pasta vazia só dele.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pasta: Option<PathBuf>,
    /// O chat que a ideia abriu.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chat: Option<String>,
    /// Quem anotou: vazio é você; "chat:<id>", um chat.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub origem: String,
    #[serde(default)]
    pub criada_em: u64,
    /// O lugar na coluna: a menor fica no alto.
    #[serde(default)]
    pub ordem: f64,
    /// A issue que o cartão espelha. Sem ela, o cartão é só do Vitral.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub linear: Option<NoLinear>,
}

/// O que o cartão sabe da issue dele.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct NoLinear {
    pub id: String,
    /// "EFI-123".
    pub identificador: String,
    #[serde(default)]
    pub url: String,
    /// O nome do estado ("Em revisão"): a coluna diz o tipo, o nome diz qual.
    #[serde(default)]
    pub estado: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub responsavel: Option<String>,
}

impl Ideia {
    /// O que vai para o chat: o pedido ou, sem ele, o título.
    pub fn texto_para_o_chat(&self) -> String {
        let pedido = self.pedido.trim();
        if pedido.is_empty() { self.titulo.trim().to_string() } else { pedido.to_string() }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Quadro {
    #[serde(default)]
    pub ideias: Vec<Ideia>,
    /// A chave curta do time do Linear ("EFI"); sem ela, `linear::TIME_PADRAO`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub time_do_linear: Option<String>,
    /// Campos de versões mais novas, regravados como vieram.
    #[serde(flatten)]
    pub resto: serde_json::Map<String, Value>,
}

impl Quadro {
    /// Os cartões de uma coluna, de cima para baixo.
    pub fn da_coluna(&self, coluna: Coluna) -> Vec<&Ideia> {
        let mut lista: Vec<&Ideia> = self.ideias.iter().filter(|i| i.coluna == coluna).collect();
        lista.sort_by(|a, b| a.ordem.total_cmp(&b.ordem));
        lista
    }

    /// Os cartões de um arquivo antigo (todos com ordem 0) ficam na ordem em que estavam.
    pub fn acertar_ordens(&mut self) {
        let mut vistas: HashSet<u64> = HashSet::new();
        let empatadas = self.ideias.iter().any(|i| !vistas.insert(i.ordem.to_bits()));
        if empatadas {
            for (n, ideia) in self.ideias.iter_mut().enumerate().filter(|(_, i)| i.linear.is_none()) {
                ideia.ordem = n as f64;
            }
        }
    }

    pub fn achar(&self, id: &str) -> Option<&Ideia> {
        self.ideias.iter().find(|i| i.id == id)
    }

    pub fn achar_mut(&mut self, id: &str) -> Option<&mut Ideia> {
        self.ideias.iter_mut().find(|i| i.id == id)
    }

    /// A ordem do alto de uma coluna (uma antes da primeira).
    pub fn ordem_do_alto(&self, coluna: Coluna) -> f64 {
        self.da_coluna(coluna).first().map_or(0., |i| i.ordem - 1.)
    }

    /// Entra no alto da coluna dela.
    pub fn adicionar(&mut self, mut ideia: Ideia) {
        ideia.ordem = self.ordem_do_alto(ideia.coluna);
        self.ideias.push(ideia);
    }

    /// Leva o cartão para `coluna`, logo antes de `antes_de` (um cartão dessa coluna) ou, sem ele,
    /// para o fim dela. Devolve se mudou algo.
    pub fn mover(&mut self, id: &str, coluna: Coluna, antes_de: Option<&str>) -> bool {
        let Some(de) = self.achar(id).map(|i| i.coluna) else {
            return false;
        };
        let ordem_de = |q: &Quadro, c: Coluna| q.da_coluna(c).iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        let antes = (de, ordem_de(self, coluna));
        let outros: Vec<(String, f64)> = self.da_coluna(coluna).iter().filter(|i| i.id != id).map(|i| (i.id.clone(), i.ordem)).collect();
        let alvo = antes_de.filter(|a| *a != id).and_then(|a| outros.iter().position(|(o, _)| o == a));
        let ordem = match alvo {
            Some(p) => match p.checked_sub(1) {
                Some(anterior) => (outros[anterior].1 + outros[p].1) / 2.,
                None => outros[p].1 - 1.,
            },
            None => outros.last().map_or(0., |(_, o)| o + 1.),
        };
        if let Some(ideia) = self.achar_mut(id) {
            ideia.coluna = coluna;
            ideia.ordem = ordem;
        }
        antes != (coluna, ordem_de(self, coluna))
    }

    /// Põe o quadro em dia com as issues lidas do Linear: as novas entram, as que mudaram mudam (o
    /// título, o pedido, a coluna, a ordem), e a de um cartão que não veio mais (cancelada,
    /// arquivada, concluída há mais de 30 dias) sai. O que é só do Vitral fica: os cartões sem
    /// issue, e a pasta, o chat e o porquê dos outros. `presas` são as issues com mudança daqui a
    /// caminho do Linear: o que veio delas é anterior à mudança, e fica para a próxima leitura.
    pub fn juntar_do_linear(&mut self, lidas: &[crate::linear::Issue], presas: &HashSet<String>) {
        let vistas: HashSet<&str> = lidas.iter().map(|i| i.id.as_str()).collect();
        self.ideias.retain(|i| match &i.linear {
            Some(l) => vistas.contains(l.id.as_str()) || presas.contains(&l.id),
            None => true,
        });
        for issue in lidas.iter().filter(|i| !presas.contains(&i.id)) {
            let no = NoLinear {
                id: issue.id.clone(),
                identificador: issue.identificador.clone(),
                url: issue.url.clone(),
                estado: issue.estado.clone(),
                responsavel: issue.responsavel.clone(),
            };
            match self.ideias.iter_mut().find(|i| i.linear.as_ref().is_some_and(|l| l.id == issue.id)) {
                Some(ideia) => {
                    ideia.titulo = issue.titulo.clone();
                    ideia.pedido = issue.descricao.clone();
                    ideia.coluna = issue.coluna;
                    ideia.ordem = issue.ordem;
                    ideia.linear = Some(no);
                }
                None => self.ideias.push(Ideia {
                    id: issue.id.clone(),
                    titulo: issue.titulo.clone(),
                    pedido: issue.descricao.clone(),
                    coluna: issue.coluna,
                    ordem: issue.ordem,
                    linear: Some(no),
                    ..Default::default()
                }),
            }
        }
    }

    /// Desligar o Linear: saem os cartões das issues; os só do Vitral ficam.
    pub fn sem_o_linear(&mut self) {
        self.ideias.retain(|i| i.linear.is_none());
    }

    pub fn apagar(&mut self, id: &str) -> Option<Ideia> {
        let de = self.ideias.iter().position(|i| i.id == id)?;
        Some(self.ideias.remove(de))
    }
}

#[cfg(test)]
mod testes {
    use super::*;

    fn ideia(id: &str, coluna: Coluna) -> Ideia {
        Ideia { id: id.into(), titulo: id.to_uppercase(), coluna, ..Default::default() }
    }

    fn ordem(q: &Quadro, coluna: Coluna) -> Vec<&str> {
        q.da_coluna(coluna).into_iter().map(|i| i.id.as_str()).collect()
    }

    #[test]
    fn a_nova_entra_no_alto_da_coluna() {
        let mut q = Quadro::default();
        q.adicionar(ideia("f", Coluna::Feitas));
        q.adicionar(ideia("a", Coluna::Ideias));
        q.adicionar(ideia("b", Coluna::Ideias));
        assert_eq!(ordem(&q, Coluna::Ideias), ["b", "a"]);
        assert_eq!(ordem(&q, Coluna::Feitas), ["f"]);
    }

    #[test]
    fn mover_vai_para_o_fim_ou_antes_do_alvo() {
        let mut q = Quadro::default();
        for id in ["c", "b", "a"] {
            q.adicionar(ideia(id, Coluna::Ideias));
        }
        q.adicionar(ideia("x", Coluna::Fazendo));
        // Para o fim de outra coluna.
        assert!(q.mover("a", Coluna::Fazendo, None));
        assert_eq!(ordem(&q, Coluna::Fazendo), ["x", "a"]);
        // Antes de um cartão da coluna de destino.
        assert!(q.mover("c", Coluna::Fazendo, Some("x")));
        assert_eq!(ordem(&q, Coluna::Fazendo), ["c", "x", "a"]);
        assert_eq!(ordem(&q, Coluna::Ideias), ["b"]);
        // Dentro da mesma coluna, para cima.
        assert!(q.mover("a", Coluna::Fazendo, Some("c")));
        assert_eq!(ordem(&q, Coluna::Fazendo), ["a", "c", "x"]);
        // Soltar em cima de si mesmo, ou de um alvo de outra coluna, vai para o fim.
        assert!(q.mover("a", Coluna::Fazendo, Some("a")));
        assert_eq!(ordem(&q, Coluna::Fazendo), ["c", "x", "a"]);
        assert!(q.mover("c", Coluna::Fazendo, Some("b")));
        assert_eq!(ordem(&q, Coluna::Fazendo), ["x", "a", "c"]);
        // O último da coluna solto no fim dela não muda nada.
        assert!(!q.mover("c", Coluna::Fazendo, None));
        assert!(!q.mover("nao-existe", Coluna::Feitas, None));
    }

    #[test]
    fn arquivo_antigo_fica_na_ordem_em_que_estava() {
        let mut q: Quadro = serde_json::from_str(r#"{"ideias":[{"id":"b","titulo":"B"},{"id":"a","titulo":"A"}]}"#).unwrap();
        q.acertar_ordens();
        assert_eq!(ordem(&q, Coluna::Ideias), ["b", "a"]);
        q.adicionar(ideia("c", Coluna::Ideias));
        assert_eq!(ordem(&q, Coluna::Ideias), ["c", "b", "a"]);
    }

    fn issue(id: &str, coluna: Coluna, ordem: f64) -> crate::linear::Issue {
        crate::linear::Issue { id: id.into(), identificador: format!("EFI-{id}"), titulo: id.to_uppercase(), coluna, ordem, ..Default::default() }
    }

    #[test]
    fn juntar_o_que_veio_do_linear() {
        let mut q = Quadro::default();
        q.adicionar(ideia("local", Coluna::Ideias));
        let presas = HashSet::new();
        q.juntar_do_linear(&[issue("x", Coluna::Proximas, 5.), issue("y", Coluna::Proximas, -3.)], &presas);
        assert_eq!(ordem(&q, Coluna::Proximas), ["y", "x"]);
        // A pasta e o chat do cartão ficam; o título e a coluna seguem o Linear.
        q.achar_mut("x").unwrap().pasta = Some("/p".into());
        let mut mudada = issue("x", Coluna::Fazendo, 1.);
        mudada.titulo = "Novo".into();
        q.juntar_do_linear(&[mudada.clone(), issue("y", Coluna::Proximas, -3.)], &presas);
        let x = q.achar("x").unwrap();
        assert_eq!((x.titulo.as_str(), x.coluna, x.pasta.as_deref()), ("Novo", Coluna::Fazendo, Some(std::path::Path::new("/p"))));
        // A que não veio sai, menos a que tem mudança a caminho; a só do Vitral fica.
        let presas: HashSet<String> = ["y".to_string()].into();
        q.juntar_do_linear(&[], &presas);
        assert!(q.achar("x").is_none());
        assert!(q.achar("y").is_some() && q.achar("local").is_some());
        q.sem_o_linear();
        assert_eq!(q.ideias.len(), 1);
    }

    #[test]
    fn o_chat_recebe_o_pedido_ou_o_titulo() {
        let mut i = ideia("a", Coluna::Ideias);
        assert_eq!(i.texto_para_o_chat(), "A");
        i.pedido = "  faça X  ".into();
        assert_eq!(i.texto_para_o_chat(), "faça X");
    }

    #[test]
    fn o_arquivo_guarda_so_o_que_tem_e_aceita_campos_novos() {
        let mut q = Quadro::default();
        q.adicionar(ideia("a", Coluna::Proximas));
        let texto = serde_json::to_string(&q).unwrap();
        assert_eq!(texto, r#"{"ideias":[{"id":"a","titulo":"A","coluna":"proximas","criada_em":0,"ordem":0.0}]}"#);
        let lido: Quadro = serde_json::from_str(r#"{"ideias":[{"id":"b","titulo":"B"}],"futuro":1}"#).unwrap();
        assert_eq!(lido.ideias[0].coluna, Coluna::Ideias);
        assert_eq!(lido.resto["futuro"], 1);
    }
}
