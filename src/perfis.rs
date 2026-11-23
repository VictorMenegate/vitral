//! Perfis de agente: um nome para um conjunto de escolhas da caixa de mensagem (quem responde, o
//! modelo, o modo de permissão e o esforço), mais uma nota de quando usar. Ficam no `config.json`
//! (`Config.perfis`), são criados nas Configurações e aparecem no alto do seletor de modelo:
//! escolher um deixa o chat exatamente como o perfil diz, de uma vez.

use serde::{Deserialize, Serialize};

use crate::modelo::{Agente, Chat};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Perfil {
    pub id: String,
    pub nome: String,
    /// Quem responde.
    pub agente: Agente,
    /// O modelo do agente (a família ou a versão exata do Claude, o slug do Codex); vazio é o
    /// padrão do agente.
    pub modelo: String,
    /// O modo de permissão (`manual`, `acceptEdits`, `plan`…); vazio é o das Configurações.
    pub modo: String,
    /// O esforço (`low`…`max`); vazio é o automático.
    pub esforco: String,
    /// Quando usar: a segunda linha no seletor.
    pub nota: String,
}

impl Perfil {
    /// O modelo que o chat passa a ter (`None` é o padrão do agente).
    pub fn modelo_do_chat(&self) -> Option<String> {
        Some(self.modelo.trim().to_string()).filter(|m| !m.is_empty())
    }

    /// O modo e o esforço do perfil no chat; o agente e o modelo vão pelo motor
    /// (`Motor::aplicar_perfil`), que cuida da troca de agente. Vazio volta ao padrão: o chat fica
    /// como o perfil diz, sem sobra da escolha de antes. O Ultracode nunca vem de um perfil (vale
    /// para uma mensagem só, e custa caro).
    pub fn aplicar(&self, chat: &mut Chat) {
        let valor = |texto: &str| Some(texto.trim().to_string()).filter(|v| !v.is_empty());
        chat.permissao = valor(&self.modo);
        chat.esforco = valor(&self.esforco).filter(|e| e != "ultracode");
    }

    /// O chat está como este perfil diz (para o selo "em uso" do seletor). `modo_padrao` é o modo
    /// das Configurações, que vale para o chat que não escolheu um.
    pub fn bate_com(&self, chat: &Chat, modo_padrao: &str) -> bool {
        let modo_do_chat = chat.permissao.as_deref().unwrap_or(modo_padrao);
        let modo_do_perfil = if self.modo.is_empty() { modo_padrao } else { self.modo.as_str() };
        chat.agente == self.agente
            && chat.parceiro.is_none()
            && chat.modelo == self.modelo_do_chat()
            && modo_do_chat == modo_do_perfil
            && chat.esforco.as_deref().unwrap_or_default() == self.esforco
    }
}

/// Por que o perfil não pode ser guardado assim (`None`: pode). `outros` são os que já existem.
pub fn erro(perfil: &Perfil, outros: &[Perfil]) -> Option<&'static str> {
    let nome = perfil.nome.trim();
    if nome.is_empty() {
        return Some("Dê um nome ao perfil");
    }
    if nome.chars().count() > 40 {
        return Some("O nome passa de 40 letras");
    }
    let igual = |a: &str| a.trim().to_lowercase() == nome.to_lowercase();
    if outros.iter().any(|o| o.id != perfil.id && igual(&o.nome)) {
        return Some("Já existe um perfil com esse nome");
    }
    None
}

/// Guarda o perfil: o que tem o id de um que existe fica no lugar dele; o novo (sem id) ganha um
/// e vai para o fim. O nome e a nota perdem os espaços das pontas. Devolve o id.
pub fn guardar(perfis: &mut Vec<Perfil>, mut perfil: Perfil) -> String {
    perfil.nome = perfil.nome.trim().to_string();
    perfil.nota = perfil.nota.trim().to_string();
    if let Some(existente) = perfis.iter_mut().find(|p| !perfil.id.is_empty() && p.id == perfil.id) {
        *existente = perfil.clone();
        return perfil.id;
    }
    if perfil.id.is_empty() || perfis.iter().any(|p| p.id == perfil.id) {
        perfil.id = uuid::Uuid::new_v4().simple().to_string()[..8].to_string();
    }
    let id = perfil.id.clone();
    perfis.push(perfil);
    id
}

pub fn remover(perfis: &mut Vec<Perfil>, id: &str) {
    perfis.retain(|p| p.id != id);
}

/// Troca o perfil de lugar com o vizinho de cima (`-1`) ou de baixo (`1`); na ponta, nada muda.
pub fn mover(perfis: &mut [Perfil], id: &str, para: i32) {
    let Some(de) = perfis.iter().position(|p| p.id == id) else { return };
    let ate = de as i64 + i64::from(para.signum());
    if ate >= 0 && (ate as usize) < perfis.len() {
        perfis.swap(de, ate as usize);
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use crate::modelo::Config;

    fn perfil(nome: &str) -> Perfil {
        Perfil { nome: nome.into(), ..Perfil::default() }
    }

    fn chat_novo() -> Chat {
        Chat::novo("c1".into(), "Chat", std::path::PathBuf::from("/p"))
    }

    #[test]
    fn config_antiga_abre_sem_perfis_e_a_nova_guarda_todos_os_campos() {
        let antiga: Config = serde_json::from_str(r#"{"permissao":"manual"}"#).unwrap();
        assert!(antiga.perfis.is_empty());
        let mut config = Config::default();
        let revisor = Perfil {
            id: "a1".into(),
            nome: "Revisor".into(),
            agente: Agente::Codex,
            modelo: "gpt-5.1-codex".into(),
            modo: "plan".into(),
            esforco: "high".into(),
            nota: "Para rever um diff antes do merge".into(),
        };
        config.perfis.push(revisor.clone());
        let volta: Config = serde_json::from_str(&serde_json::to_string(&config).unwrap()).unwrap();
        assert_eq!(volta.perfis, vec![revisor]);
    }

    #[test]
    fn aplicar_deixa_o_chat_como_o_perfil_diz() {
        let mut chat = Chat { permissao: Some("bypassPermissions".into()), esforco: Some("max".into()), ..chat_novo() };
        let plano = Perfil { modo: "plan".into(), esforco: "high".into(), modelo: " opus ".into(), ..perfil("Planejar") };
        plano.aplicar(&mut chat);
        assert_eq!((chat.permissao.as_deref(), chat.esforco.as_deref()), (Some("plan"), Some("high")));
        assert_eq!(plano.modelo_do_chat().as_deref(), Some("opus"));
        // Vazio volta ao padrão: não sobra a escolha de antes.
        perfil("Dia a dia").aplicar(&mut chat);
        assert_eq!((chat.permissao, chat.esforco), (None, None));
        assert_eq!(perfil("Dia a dia").modelo_do_chat(), None);
        // O Ultracode vale para uma mensagem só: perfil não o liga.
        let mut chat = chat_novo();
        Perfil { esforco: "ultracode".into(), ..perfil("x") }.aplicar(&mut chat);
        assert_eq!(chat.esforco, None);
    }

    #[test]
    fn o_perfil_em_uso_e_o_que_bate_com_o_chat() {
        let plano = Perfil { modo: "plan".into(), modelo: "opus".into(), ..perfil("Planejar") };
        let mut chat = Chat { modelo: Some("opus".into()), permissao: Some("plan".into()), ..chat_novo() };
        assert!(plano.bate_com(&chat, "manual"));
        chat.esforco = Some("max".into());
        assert!(!plano.bate_com(&chat, "manual"));
        // Sem modo no perfil, vale o das Configurações: o chat que não escolheu modo bate.
        let simples = Perfil { modelo: "opus".into(), ..perfil("Opus") };
        let chat = Chat { modelo: Some("opus".into()), ..chat_novo() };
        assert!(simples.bate_com(&chat, "manual"));
        let do_codex = Perfil { agente: Agente::Codex, ..perfil("Codex") };
        assert!(!do_codex.bate_com(&chat, "manual"));
    }

    #[test]
    fn perfil_sem_nome_ou_com_nome_repetido_nao_e_guardado() {
        let existentes = vec![Perfil { id: "a".into(), ..perfil("Revisor") }];
        assert_eq!(erro(&perfil("  "), &existentes), Some("Dê um nome ao perfil"));
        assert_eq!(erro(&perfil(" revisor "), &existentes), Some("Já existe um perfil com esse nome"));
        // Editar o próprio perfil sem trocar o nome não esbarra nele mesmo.
        assert_eq!(erro(&Perfil { id: "a".into(), ..perfil("Revisor") }, &existentes), None);
        assert_eq!(erro(&perfil("Planejar"), &existentes), None);
        assert!(erro(&perfil(&"x".repeat(41)), &existentes).is_some());
    }

    #[test]
    fn guardar_cria_no_fim_edita_no_lugar_e_mover_troca_com_o_vizinho() {
        let mut perfis = Vec::new();
        let a = guardar(&mut perfis, perfil("  Planejar "));
        let b = guardar(&mut perfis, perfil("Revisor"));
        assert!(!a.is_empty() && a != b);
        assert_eq!(perfis.iter().map(|p| p.nome.as_str()).collect::<Vec<_>>(), ["Planejar", "Revisor"]);
        // Editar mantém o id e o lugar.
        let editado = Perfil { id: a.clone(), nota: " só lê ".into(), ..perfil("Plano") };
        assert_eq!(guardar(&mut perfis, editado), a);
        assert_eq!((perfis[0].nome.as_str(), perfis[0].nota.as_str(), perfis.len()), ("Plano", "só lê", 2));
        mover(&mut perfis, &a, 1);
        assert_eq!(perfis[1].id, a);
        // Na ponta, nada muda.
        mover(&mut perfis, &a, 1);
        assert_eq!(perfis[1].id, a);
        mover(&mut perfis, &a, -1);
        assert_eq!(perfis[0].id, a);
        remover(&mut perfis, &a);
        assert_eq!(perfis.iter().map(|p| p.id.clone()).collect::<Vec<_>>(), [b]);
    }
}
