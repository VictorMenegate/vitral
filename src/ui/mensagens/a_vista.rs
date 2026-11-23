//! Só o que está à vista é montado. A conversa continua uma coluna só dentro de uma área que rola
//! (a rolagem em pixels, a âncora de leitura, a mensagem presa no alto e a busca dependem disso),
//! mas o bloco que está longe da vista entra como um espaço da altura que ele mediu da última vez,
//! sem montar nada dele: nem o markdown, nem o layout, nem a pintura. Medido em 23/11/2026 no
//! binário otimizado, na bancada, com três conversas de 450 itens a 150%: cada passo da roda
//! remontava os 60 itens do painel (de 35 a 58 ms de processador por quadro; ficou em 28 a 30,
//! dos quais uns 15 são a casca da janela), e um painel monta de 3 a 10 dos 28 a 33 blocos de
//! uma página.
//!
//! O que garante que nada pula: a altura guardada vale só para o mesmo bloco mostrando a mesma
//! coisa (`ChaveDoBloco`, com a marca do conteúdo) na mesma largura e no mesmo tamanho de letra
//! (`Alturas::comecar`, `Alturas::conferir_largura`); sem altura guardada o bloco é montado; e a
//! conta é sempre para o lado seguro (`blocos_a_montar`): na dúvida, monta.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use gpui_kit::*;

use super::Bloco;

/// O que um bloco da conversa é, para a chave dele.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::ui) enum TipoDaChave {
    Item,
    Trilha,
    Turno,
    Memoria,
    Dia,
    Novo,
    Historico,
    TrilhaDoHistorico,
}

/// A identidade de um bloco entre um quadro e outro: o tipo, os itens que ele cobre e uma marca do
/// que ele mostra (o tamanho do texto, quantas chamadas já voltaram, aberto ou fechado). Mudou
/// qualquer um, é outro bloco, sem altura guardada.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(in crate::ui) struct ChaveDoBloco {
    pub tipo: TipoDaChave,
    pub inicio: usize,
    pub fim: usize,
    pub marca: u64,
}

/// A marca de um conteúdo: junta números (tamanhos, contagens, estados) num só.
pub(in crate::ui) fn marca(partes: &[u64]) -> u64 {
    // FNV-1a de 64 bits sobre os números e a quantidade deles: barato e estável entre quadros.
    let mut h: u64 = 0xcbf2_9ce4_8422_2325 ^ partes.len() as u64;
    for parte in partes {
        for byte in parte.to_le_bytes() {
            h = (h ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// O que se sabe de um bloco antes de montar: a altura guardada (se houver) e o espaço antes dele.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) struct Medida {
    pub altura: Option<Pixels>,
    pub margem: Pixels,
}

/// Onde a vista está na conversa.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::ui) enum Vista {
    /// Sem conta: tudo montado (a busca aberta, uma seleção de texto, a abertura do chat).
    Tudo,
    /// No fim da conversa, seguindo o que chega: a vista são os últimos `altura` pixels.
    NoFim { altura: Pixels },
    /// Com `topo` (a partir do alto do conteúdo) no alto da vista.
    Em { topo: Pixels, altura: Pixels },
}

/// Quanto além da vista, para cima e para baixo, ainda é montado: meia vista. A conta é refeita a
/// cada desenho com a rolagem daquele desenho (a roda, a barra e o teclado avisam a vista antes de
/// ela ser pintada), então a sobra não é o que impede um vazio na tela: ela só guarda de uma
/// altura guardada que já não vale, e do ponteiro que passa por um bloco no instante em que ele
/// entra. Com uma vista inteira de cada lado (a primeira conta, 23/11/2026), um painel a 150%
/// montava 18 dos 28 blocos de uma página, e a medida quase não mudava.
pub(in crate::ui) fn sobra_da_vista(altura: Pixels) -> Pixels {
    (altura / 2.).max(SOBRA_MINIMA)
}

/// A sobra de uma vista baixa: uns cinco passos da roda.
const SOBRA_MINIMA: Pixels = px(300.);

/// Quais blocos montar de verdade. `antes` e `depois` são o respiro do alto e do pé da lista.
/// Sempre para o lado seguro: um bloco sem altura é montado, e a posição de quem vem depois dele
/// só conta como certa para o lado que não depende dela.
pub(in crate::ui) fn blocos_a_montar(medidas: &[Medida], vista: Vista, sobra: Pixels, antes: Pixels, depois: Pixels) -> Vec<bool> {
    match vista {
        Vista::Tudo => vec![true; medidas.len()],
        // Uma vista ainda sem altura (o primeiro desenho) não esconde nada.
        Vista::NoFim { altura } | Vista::Em { altura, .. } if altura <= px(0.) => vec![true; medidas.len()],
        Vista::NoFim { altura } => {
            // Do fim para o começo: quanto há abaixo do pé de cada bloco, no mínimo (quem não tem
            // altura conta zero, e por isso os de antes dele ficam mais perto do fim do que estão:
            // montam-se blocos a mais, nunca a menos).
            let mut lista = vec![true; medidas.len()];
            let mut abaixo = depois;
            for (k, medida) in medidas.iter().enumerate().rev() {
                if let Some(h) = medida.altura {
                    lista[k] = abaixo <= altura + sobra;
                    abaixo += h;
                }
                abaixo += medida.margem;
            }
            lista
        }
        Vista::Em { topo, altura } => {
            // Do começo para o fim, com duas contas do alto de cada bloco: a certa (só enquanto
            // todos os de antes têm altura) decide quem ficou acima da vista; a mínima (quem não
            // tem altura conta zero) decide quem ficou abaixo dela.
            let mut lista = vec![true; medidas.len()];
            let (mut certo, mut minimo) = (Some(antes), antes);
            for (k, medida) in medidas.iter().enumerate() {
                let alto_minimo = minimo + medida.margem;
                match medida.altura {
                    Some(h) => {
                        let abaixo = alto_minimo > topo + altura + sobra;
                        let acima = certo.is_some_and(|c| c + medida.margem + h < topo - sobra);
                        lista[k] = !(abaixo || acima);
                        certo = certo.map(|c| c + medida.margem + h);
                        minimo = alto_minimo + h;
                    }
                    None => {
                        certo = None;
                        minimo = alto_minimo;
                    }
                }
            }
            lista
        }
    }
}

/// O que identifica as alturas guardadas de um painel: o chat, o tamanho de letra (o rem, que
/// leva o zoom) e o que muda a cara de todos os blocos de uma vez.
#[derive(Clone, PartialEq, Debug)]
pub(in crate::ui) struct Assinatura {
    pub chat: String,
    pub rem: Pixels,
    pub foco: bool,
    pub estreito: bool,
}

/// Acima disto as alturas guardadas recomeçam do zero (ver `Alturas::comecar`).
const LIMITE_DE_ALTURAS: usize = 20_000;

/// Quanto tempo a altura de um bloco de texto longo precisa ficar igual para valer na conta: o
/// kit lê o markdown de mais de 4 KB em segundo plano, e até ele chegar o bloco mede quase nada.
const ASSENTO: Duration = Duration::from_millis(400);

/// As alturas medidas dos blocos de um painel, por chave.
#[derive(Default)]
pub(in crate::ui) struct Alturas {
    assinatura: Option<Assinatura>,
    largura: Option<Pixels>,
    quadro: u64,
    /// A altura, o tipo, o desenho em que foi medida e, no bloco de texto longo, desde quando ela
    /// é essa.
    mapa: HashMap<ChaveDoBloco, (Pixels, Bloco, u64, Option<Instant>)>,
    /// Os blocos de texto longo que entraram como espaço no último desenho.
    espacos: HashSet<ChaveDoBloco>,
    /// Os blocos de texto longo que voltaram a ser montados, e quando: o kit relê o markdown
    /// deles em segundo plano, e até o texto chegar a caixa fica na altura guardada (`segurar`).
    voltando: HashMap<ChaveDoBloco, Instant>,
}

impl Alturas {
    /// Começa um desenho: outra conversa, outro tamanho de letra ou outro modo esquecem tudo.
    pub(in crate::ui) fn comecar(&mut self, assinatura: Assinatura) {
        self.quadro += 1;
        if self.assinatura.as_ref() != Some(&assinatura) {
            self.mapa.clear();
            self.espacos.clear();
            self.voltando.clear();
            self.largura = None;
            self.assinatura = Some(assinatura);
        }
        // Chaves de blocos que não existem mais (o turno que recolheu, o texto que cresceu) ficam
        // para trás: numa conversa muito mexida, recomeça em vez de crescer sem fim.
        if self.mapa.len() > LIMITE_DE_ALTURAS {
            self.mapa.clear();
        }
    }

    /// A altura e o tipo guardados de um bloco.
    pub(in crate::ui) fn de(&self, chave: &ChaveDoBloco) -> Option<(Pixels, Bloco)> {
        self.mapa.get(chave).map(|(altura, bloco, ..)| (*altura, *bloco))
    }

    /// A altura de um bloco de texto longo, só depois de assentada (`ASSENTO`). Esse bloco fica
    /// sempre montado (o texto dele chega depois do primeiro desenho); a altura serve para saber
    /// onde ficam os blocos de depois dele. Sem ela, um relatório comprido no meio da conversa
    /// deixava montado tudo o que vinha depois (visto em 23/11/2026).
    pub(in crate::ui) fn assentada(&self, chave: &ChaveDoBloco, agora: Instant) -> Option<(Pixels, Bloco)> {
        let (altura, bloco, _, desde) = self.mapa.get(chave)?;
        desde.is_some_and(|desde| agora.saturating_duration_since(desde) >= ASSENTO).then_some((*altura, *bloco))
    }

    /// Um bloco de texto longo entrou como espaço neste desenho.
    pub(in crate::ui) fn virou_espaco(&mut self, chave: ChaveDoBloco) {
        self.voltando.remove(&chave);
        self.espacos.insert(chave);
    }

    /// Um bloco de texto longo vai ser montado. Se ele era espaço no desenho anterior (ou voltou
    /// há menos que o `ASSENTO`), devolve a altura guardada, que a caixa dele mantém como mínima:
    /// o texto relido chega um ou dois desenhos depois, e sem isso o bloco encolhia e crescia
    /// de novo, empurrando o que a pessoa lê logo abaixo.
    pub(in crate::ui) fn segurar(&mut self, chave: &ChaveDoBloco, agora: Instant) -> Option<Pixels> {
        if self.espacos.remove(chave) {
            self.voltando.insert(*chave, agora);
        }
        let desde = *self.voltando.get(chave)?;
        if agora.saturating_duration_since(desde) >= ASSENTO {
            self.voltando.remove(chave);
            return None;
        }
        self.mapa.get(chave).map(|(altura, ..)| *altura)
    }

    /// Anota o que um bloco de texto longo mediu neste desenho: a altura que mudou recomeça a
    /// contagem do assento. Enquanto o bloco está voltando (`segurar`), o que ele mede é a altura
    /// mínima da caixa, e nada muda.
    pub(in crate::ui) fn anotar_assentando(&mut self, chave: ChaveDoBloco, altura: Pixels, bloco: Bloco, agora: Instant) {
        if self.voltando.contains_key(&chave) {
            if let Some(entrada) = self.mapa.get_mut(&chave) {
                entrada.2 = self.quadro;
            }
            return;
        }
        let desde = match self.mapa.get(&chave) {
            Some((antes, _, _, Some(desde))) if *antes == altura => *desde,
            _ => agora,
        };
        self.mapa.insert(chave, (altura, bloco, self.quadro, Some(desde)));
    }

    /// Anota o que um bloco montado mediu neste desenho.
    pub(in crate::ui) fn anotar(&mut self, chave: ChaveDoBloco, altura: Pixels, bloco: Bloco) {
        self.mapa.insert(chave, (altura, bloco, self.quadro, None));
    }

    /// Confere a largura da coluna depois do layout. Mudou (a janela, o painel ao lado, o Ctrl+B):
    /// as alturas de antes não valem mais, e só ficam as medidas neste desenho. Devolve se algum
    /// espaço deste desenho usou altura velha (é preciso desenhar de novo).
    pub(in crate::ui) fn conferir_largura(&mut self, largura: Pixels, espacos: usize) -> bool {
        let mudou = self.largura.is_some_and(|antes| antes != largura);
        self.largura = Some(largura);
        if !mudou {
            return false;
        }
        let quadro = self.quadro;
        self.mapa.retain(|_, (_, _, medido, _)| *medido == quadro);
        espacos > 0
    }

    #[cfg(test)]
    fn guardadas(&self) -> usize {
        self.mapa.len()
    }
}

#[cfg(test)]
mod testes {
    // Sem `use super::*`: ele traria o `test` do gpui, e o #[test] viraria recursão.
    use gpui_kit::px;

    use super::{Alturas, Assinatura, ChaveDoBloco, Medida, TipoDaChave, Vista, blocos_a_montar, marca, sobra_da_vista};
    use crate::ui::mensagens::Bloco;

    fn chave(i: usize) -> ChaveDoBloco {
        ChaveDoBloco { tipo: TipoDaChave::Item, inicio: i, fim: i + 1, marca: 0 }
    }

    fn conversa(n: usize, altura: f32, margem: f32) -> Vec<Medida> {
        (0..n).map(|_| Medida { altura: Some(px(altura)), margem: px(margem) }).collect()
    }

    /// Onde cada bloco fica (alto e pé) com as alturas dadas.
    fn posicoes(medidas: &[Medida], antes: f32) -> Vec<(f32, f32)> {
        let mut y = antes;
        medidas
            .iter()
            .map(|m| {
                let alto = y + m.margem.as_f32();
                y = alto + m.altura.map_or(0., |a| a.as_f32());
                (alto, y)
            })
            .collect()
    }

    fn montados(lista: &[bool]) -> usize {
        lista.iter().filter(|m| **m).count()
    }

    // O aceite do pedido: numa conversa de 400 blocos já medidos, o painel no fim monta só o que
    // cabe na vista e na sobra, e nunca deixa de montar um bloco que aparece.
    #[test]
    fn conversa_de_400_no_fim_monta_so_o_que_esta_a_vista() {
        let medidas = conversa(400, 120., 12.);
        let (vista, sobra) = (px(900.), px(900.));
        let lista = blocos_a_montar(&medidas, Vista::NoFim { altura: vista }, sobra, px(12.), px(32.));
        assert_eq!(lista.len(), 400);
        // 1800 px de vista e sobra, 132 por bloco: 14 blocos, e não os 400.
        assert!(montados(&lista) <= 15, "montou {}", montados(&lista));
        assert!(montados(&lista) >= 7, "montou {}", montados(&lista));
        // Os do fim, sem buraco: do primeiro montado em diante, todos.
        let primeiro = lista.iter().position(|m| *m).unwrap();
        assert!(lista[primeiro..].iter().all(|m| *m));
        // E todo bloco que encosta na vista está montado.
        let pos = posicoes(&medidas, 12.);
        let fim = pos.last().unwrap().1 + 32.;
        for (k, (alto, pe)) in pos.iter().enumerate() {
            if *pe >= fim - 900. && *alto <= fim {
                assert!(lista[k], "o bloco {k} aparece e não foi montado");
            }
        }
    }

    #[test]
    fn conversa_de_400_rolada_no_meio_monta_so_a_janela() {
        let medidas = conversa(400, 120., 12.);
        let (topo, vista, sobra) = (px(20_000.), px(900.), px(900.));
        let lista = blocos_a_montar(&medidas, Vista::Em { topo, altura: vista }, sobra, px(12.), px(32.));
        // 900 de vista e 900 de sobra para cada lado: 2700 px, 21 blocos de 132.
        assert!(montados(&lista) <= 23, "montou {}", montados(&lista));
        let pos = posicoes(&medidas, 12.);
        for (k, (alto, pe)) in pos.iter().enumerate() {
            let aparece = *pe >= 20_000. && *alto <= 20_900.;
            if aparece {
                assert!(lista[k], "o bloco {k} aparece e não foi montado");
            }
            // Longe da vista e da sobra, é espaço.
            if *pe < 20_000. - 900. - 1. || *alto > 20_900. + 900. + 1. {
                assert!(!lista[k], "o bloco {k} está longe e foi montado");
            }
        }
    }

    #[test]
    fn sem_conta_monta_tudo() {
        let medidas = conversa(400, 120., 12.);
        assert_eq!(montados(&blocos_a_montar(&medidas, Vista::Tudo, px(900.), px(12.), px(32.))), 400);
    }

    #[test]
    fn bloco_sem_altura_e_sempre_montado() {
        let mut medidas = conversa(400, 120., 12.);
        for k in [0, 57, 200, 399] {
            medidas[k].altura = None;
        }
        for vista in [Vista::NoFim { altura: px(900.) }, Vista::Em { topo: px(20_000.), altura: px(900.) }] {
            let lista = blocos_a_montar(&medidas, vista, px(900.), px(12.), px(32.));
            for k in [0, 57, 200, 399] {
                assert!(lista[k], "o bloco {k} não tem altura e virou espaço ({vista:?})");
            }
        }
    }

    // O texto chegando: o último bloco muda de altura a cada pedaço (chave nova, sem altura), e
    // os de antes dele continuam contados a partir do fim, pelo lado seguro.
    #[test]
    fn no_fim_o_bloco_que_cresce_nao_derruba_a_conta() {
        let mut medidas = conversa(400, 120., 12.);
        medidas[399].altura = None;
        let lista = blocos_a_montar(&medidas, Vista::NoFim { altura: px(900.) }, px(900.), px(12.), px(32.));
        assert!(lista[399]);
        assert!(montados(&lista) <= 16, "montou {}", montados(&lista));
        assert!(lista[390..].iter().all(|m| *m));
        assert!(!lista[300]);
    }

    // Um bloco sem altura acima da vista (abriu um "Trabalhou…" lá em cima): não dá para saber
    // onde os seguintes ficaram. O que está antes dele continua certo; depois dele, só vira
    // espaço o que ficaria abaixo da vista mesmo com ele medindo zero.
    #[test]
    fn rolado_com_um_desconhecido_acima_fica_no_lado_seguro() {
        let mut medidas = conversa(400, 120., 12.);
        medidas[100].altura = None;
        let lista = blocos_a_montar(&medidas, Vista::Em { topo: px(20_000.), altura: px(900.) }, px(900.), px(12.), px(32.));
        // Antes do desconhecido e longe da vista: espaço.
        assert!(!lista[10]);
        assert!(lista[100]);
        // Depois dele, o que pode estar à vista (ele pode medir de zero a muito) é montado.
        let pos = posicoes(&medidas, 12.);
        for (k, (alto, _)) in pos.iter().enumerate().skip(101) {
            if *alto <= 20_900. + 900. {
                assert!(lista[k], "o bloco {k} pode aparecer e não foi montado");
            }
        }
        // Bem abaixo da vista, mesmo com o desconhecido medindo zero: espaço.
        assert!(!lista[399]);
    }

    // A conversa curta (cabe na tela) fica toda montada, no fim ou não.
    #[test]
    fn conversa_que_cabe_na_vista_fica_inteira() {
        let medidas = conversa(5, 100., 12.);
        for vista in [Vista::NoFim { altura: px(900.) }, Vista::Em { topo: px(0.), altura: px(900.) }] {
            assert_eq!(montados(&blocos_a_montar(&medidas, vista, px(900.), px(12.), px(32.))), 5);
        }
        assert!(blocos_a_montar(&[], Vista::NoFim { altura: px(900.) }, px(900.), px(12.), px(32.)).is_empty());
    }

    // Uma vista ainda sem altura (o primeiro desenho) não esconde nada.
    #[test]
    fn vista_sem_altura_monta_tudo() {
        let medidas = conversa(50, 120., 12.);
        for vista in [Vista::NoFim { altura: px(0.) }, Vista::Em { topo: px(100.), altura: px(0.) }] {
            assert_eq!(montados(&blocos_a_montar(&medidas, vista, px(0.), px(12.), px(32.))), 50);
        }
    }

    #[test]
    fn a_sobra_e_meia_vista_e_nunca_pequena() {
        assert_eq!(sobra_da_vista(px(900.)), px(450.));
        assert!(sobra_da_vista(px(200.)) >= px(300.));
    }

    #[test]
    fn alturas_valem_para_a_mesma_conversa_e_o_mesmo_tamanho() {
        let a = Assinatura { chat: "a".into(), rem: px(16.), foco: false, estreito: false };
        let mut alturas = Alturas::default();
        alturas.comecar(a.clone());
        alturas.anotar(chave(3), px(120.), Bloco::Texto);
        assert_eq!(alturas.de(&chave(3)), Some((px(120.), Bloco::Texto)));
        assert_eq!(alturas.de(&chave(4)), None);
        // O mesmo bloco mostrando outra coisa (outra marca) não tem altura.
        assert_eq!(alturas.de(&ChaveDoBloco { marca: 7, ..chave(3) }), None);
        // O desenho seguinte da mesma conversa guarda.
        alturas.comecar(a.clone());
        assert_eq!(alturas.de(&chave(3)), Some((px(120.), Bloco::Texto)));
        // Outro zoom, outro modo ou outra conversa esquecem.
        for outra in [
            Assinatura { rem: px(24.), ..a.clone() },
            Assinatura { foco: true, ..a.clone() },
            Assinatura { estreito: true, ..a.clone() },
            Assinatura { chat: "b".into(), ..a.clone() },
        ] {
            let mut alturas = Alturas::default();
            alturas.comecar(a.clone());
            alturas.anotar(chave(3), px(120.), Bloco::Texto);
            alturas.comecar(outra);
            assert_eq!(alturas.de(&chave(3)), None);
        }
    }

    #[test]
    fn largura_nova_esquece_o_que_foi_medido_na_antiga() {
        let a = Assinatura { chat: "a".into(), rem: px(16.), foco: false, estreito: false };
        let mut alturas = Alturas::default();
        alturas.comecar(a.clone());
        alturas.anotar(chave(1), px(100.), Bloco::Texto);
        alturas.anotar(chave(2), px(100.), Bloco::Texto);
        // O primeiro desenho só anota a largura.
        assert!(!alturas.conferir_largura(px(500.), 0));
        assert_eq!(alturas.guardadas(), 2);
        // Mesma largura: nada muda, com espaços ou sem.
        alturas.comecar(a.clone());
        assert!(!alturas.conferir_largura(px(500.), 5));
        assert_eq!(alturas.guardadas(), 2);
        // Largura nova: o bloco 1 foi montado e medido neste desenho e fica; o 2 era espaço com a
        // altura da largura antiga e sai. Houve espaço com altura velha: desenha de novo.
        alturas.comecar(a.clone());
        alturas.anotar(chave(1), px(140.), Bloco::Texto);
        assert!(alturas.conferir_largura(px(400.), 1));
        assert_eq!(alturas.de(&chave(1)), Some((px(140.), Bloco::Texto)));
        assert_eq!(alturas.de(&chave(2)), None);
        // Largura nova sem nenhum espaço no desenho: nada a refazer.
        alturas.comecar(a);
        alturas.anotar(chave(1), px(150.), Bloco::Texto);
        assert!(!alturas.conferir_largura(px(300.), 0));
    }

    // O bloco de texto longo: a altura dele só entra na conta depois de ficar igual por um tempo
    // (o texto chega depois do primeiro desenho), e uma altura nova recomeça a espera.
    #[test]
    fn a_altura_do_texto_longo_so_vale_depois_de_assentar() {
        use std::time::{Duration, Instant};
        let a = Assinatura { chat: "a".into(), rem: px(16.), foco: false, estreito: false };
        let mut alturas = Alturas::default();
        alturas.comecar(a.clone());
        let t0 = Instant::now();
        // O primeiro desenho mede o bloco sem o texto.
        alturas.anotar_assentando(chave(5), px(24.), Bloco::Texto, t0);
        assert_eq!(alturas.assentada(&chave(5), t0), None);
        assert_eq!(alturas.assentada(&chave(5), t0 + Duration::from_millis(100)), None);
        // O texto chegou: a altura mudou, e a espera recomeça dali.
        let t1 = t0 + Duration::from_millis(120);
        alturas.anotar_assentando(chave(5), px(900.), Bloco::Texto, t1);
        assert_eq!(alturas.assentada(&chave(5), t0 + Duration::from_millis(450)), None);
        // A mesma altura nos desenhos seguintes não recomeça nada.
        alturas.comecar(a.clone());
        alturas.anotar_assentando(chave(5), px(900.), Bloco::Texto, t1 + Duration::from_millis(200));
        assert_eq!(alturas.assentada(&chave(5), t1 + Duration::from_millis(450)), Some((px(900.), Bloco::Texto)));
        // O bloco comum não passa por isso, e não tem altura "assentada".
        alturas.anotar(chave(6), px(100.), Bloco::Texto);
        assert_eq!(alturas.de(&chave(6)), Some((px(100.), Bloco::Texto)));
        assert_eq!(alturas.assentada(&chave(6), t1 + Duration::from_secs(5)), None);
    }

    // O texto longo que saiu da vista e voltou: o kit relê o markdown dele em segundo plano, e a
    // caixa fica na altura guardada até o texto chegar, sem mexer na altura guardada.
    #[test]
    fn o_texto_longo_que_volta_a_vista_fica_na_altura_guardada() {
        use std::time::{Duration, Instant};
        let a = Assinatura { chat: "a".into(), rem: px(16.), foco: false, estreito: false };
        let mut alturas = Alturas::default();
        alturas.comecar(a.clone());
        let t0 = Instant::now();
        alturas.anotar_assentando(chave(5), px(900.), Bloco::Texto, t0);
        let depois = t0 + Duration::from_secs(2);
        assert_eq!(alturas.assentada(&chave(5), depois), Some((px(900.), Bloco::Texto)));
        // Montado desde sempre: nada a segurar.
        assert_eq!(alturas.segurar(&chave(5), depois), None);
        // Saiu da vista: virou espaço. Voltou: a caixa fica com pelo menos os 900.
        alturas.comecar(a.clone());
        alturas.virou_espaco(chave(5));
        alturas.comecar(a.clone());
        assert_eq!(alturas.segurar(&chave(5), depois), Some(px(900.)));
        // O primeiro desenho da volta mede a caixa segurada (ou o texto ainda por chegar): a
        // altura guardada e a contagem dela não mudam.
        alturas.anotar_assentando(chave(5), px(900.), Bloco::Texto, depois);
        assert_eq!(alturas.assentada(&chave(5), depois), Some((px(900.), Bloco::Texto)));
        // Nos desenhos seguintes, ainda dentro do tempo, continua segurado.
        alturas.comecar(a.clone());
        assert_eq!(alturas.segurar(&chave(5), depois + Duration::from_millis(200)), Some(px(900.)));
        // Passado o tempo, o bloco volta a medir sozinho.
        alturas.comecar(a.clone());
        let tarde = depois + Duration::from_millis(600);
        assert_eq!(alturas.segurar(&chave(5), tarde), None);
        alturas.anotar_assentando(chave(5), px(900.), Bloco::Texto, tarde);
        assert_eq!(alturas.assentada(&chave(5), tarde), Some((px(900.), Bloco::Texto)));
        // Virar espaço de novo no meio da volta recomeça do zero na próxima.
        alturas.virou_espaco(chave(5));
        assert_eq!(alturas.segurar(&chave(5), tarde + Duration::from_secs(1)), Some(px(900.)));
        // Sem altura guardada não há o que segurar.
        alturas.virou_espaco(chave(9));
        assert_eq!(alturas.segurar(&chave(9), tarde), None);
    }

    // O aceite do texto longo: com a altura assentada ele não derruba a conta dos blocos de
    // depois (antes, tudo depois dele ficava montado com o painel rolado).
    #[test]
    fn um_bloco_alto_no_meio_nao_monta_os_de_depois() {
        let mut medidas = conversa(60, 120., 12.);
        medidas[10].altura = Some(px(4000.));
        // A vista logo depois do bloco alto: 12 + 10 * 132 + 12 + 4000 = 5344 é o pé dele.
        let lista = blocos_a_montar(&medidas, Vista::Em { topo: px(5400.), altura: px(900.) }, px(450.), px(12.), px(32.));
        assert!(montados(&lista) <= 16, "montou {}", montados(&lista));
        assert!(lista[11] && lista[12]);
        assert!(!lista[40]);
        // Sem a altura dele, todos os de depois que poderiam aparecer ficam montados.
        medidas[10].altura = None;
        let lista = blocos_a_montar(&medidas, Vista::Em { topo: px(5400.), altura: px(900.) }, px(450.), px(12.), px(32.));
        assert!(montados(&lista) >= 40, "montou {}", montados(&lista));
    }

    #[test]
    fn a_marca_muda_com_qualquer_parte() {
        assert_eq!(marca(&[1, 2, 3]), marca(&[1, 2, 3]));
        assert_ne!(marca(&[1, 2, 3]), marca(&[1, 2, 4]));
        assert_ne!(marca(&[1, 2]), marca(&[2, 1]));
        assert_ne!(marca(&[0]), marca(&[0, 0]));
    }
}
