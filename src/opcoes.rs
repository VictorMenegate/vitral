//! As decisões das opções das Configurações que não dependem da tela: cada uma recebe o que
//! precisa e devolve o que fazer, para o teste não abrir janela. O padrão de cada opção é o
//! comportamento de antes dela.

use std::time::Duration;

/// Há quanto tempo o Ctrl está apertado até o selo com o número aparecer (um toque rápido, de
/// quem vai usar outro atalho, não pisca a tela).
pub const ESPERA_DO_SELO: Duration = Duration::from_millis(150);

/// Os números das abas e da lista aparecem agora? Sempre, ou (com "Só segurando Ctrl") depois de
/// `ESPERA_DO_SELO` com o Ctrl apertado; `ctrl_ha` é `None` com o Ctrl solto.
pub fn numeros_a_vista(so_com_ctrl: bool, ctrl_ha: Option<Duration>) -> bool {
    if !so_com_ctrl {
        return true;
    }
    ctrl_ha.is_some_and(|d| d >= ESPERA_DO_SELO)
}

/// O valor com que a aba Arquivos abre: o que ficou guardado na última vez, se a opção de lembrar
/// está ligada; senão, desligado, como sempre foi.
pub fn valor_ao_abrir(lembrar: bool, guardado: bool) -> bool {
    lembrar && guardado
}

/// A parte Agentes consulta a rede por versão nova? Sozinha, ao abrir, ou só quando o botão pede.
pub fn consulta_a_rede(sozinho: bool, pelo_botao: bool) -> bool {
    sozinho || pelo_botao
}

/// Quais anéis de uso ficam no canto: o que há para mostrar (conta conectada, uso lido) e o que a
/// pessoa escolheu, serviço por serviço (Claude, Codex, Gemini).
pub fn aneis_escolhidos(a_mostrar: [bool; 3], escolhidos: [bool; 3]) -> [bool; 3] {
    [0, 1, 2].map(|i| a_mostrar[i] && escolhidos[i])
}

/// A letra que a caixa de mensagem usa, em %: a da conversa quando a opção está ligada; senão, a
/// da interface (100).
pub fn letra_da_caixa(na_caixa: bool, letra_da_conversa: u32) -> u32 {
    if na_caixa { letra_da_conversa } else { 100 }
}

/// O Enter manda depois (fila) em vez de entrar no turno? `secondary` é o Ctrl+Enter. A opção
/// "Enter vai para a fila" troca os dois, mas só onde há escolha: com um turno recebendo
/// mensagem. Respondendo a um pedido de permissão ou a uma pergunta, o Enter continua sendo a
/// resposta.
pub fn manda_depois(enter_na_fila: bool, secondary: bool, turno_recebe: bool) -> bool {
    if enter_na_fila && turno_recebe { !secondary } else { secondary }
}

#[cfg(test)]
mod testes {
    use super::*;

    const MS: fn(u64) -> Duration = Duration::from_millis;

    #[test]
    fn os_numeros_sempre_aparecem_por_padrao() {
        assert!(numeros_a_vista(false, None));
        assert!(numeros_a_vista(false, Some(MS(0))));
    }

    #[test]
    fn os_numeros_so_com_ctrl_esperam_150_ms_e_somem_ao_soltar() {
        assert!(!numeros_a_vista(true, None), "sem Ctrl");
        assert!(!numeros_a_vista(true, Some(MS(149))), "toque rápido");
        assert!(numeros_a_vista(true, Some(MS(150))));
        assert!(numeros_a_vista(true, Some(MS(2000))));
    }

    #[test]
    fn a_aba_arquivos_so_lembra_com_a_opcao_ligada() {
        assert!(!valor_ao_abrir(false, true), "sem lembrar, abre desligada como sempre");
        assert!(valor_ao_abrir(true, true));
        assert!(!valor_ao_abrir(true, false));
    }

    #[test]
    fn a_versao_nova_so_pelo_botao() {
        assert!(consulta_a_rede(true, false));
        assert!(!consulta_a_rede(false, false), "só pelo botão: abrir a parte não consulta");
        assert!(consulta_a_rede(false, true), "o botão sempre consulta");
    }

    #[test]
    fn os_aneis_escolhidos_cortam_so_o_que_havia_para_mostrar() {
        assert_eq!(aneis_escolhidos([true, true, true], [true, true, true]), [true, true, true]);
        assert_eq!(aneis_escolhidos([true, true, false], [true, false, true]), [true, false, false]);
        assert_eq!(aneis_escolhidos([false, true, true], [true, true, false]), [false, true, false]);
    }

    #[test]
    fn a_letra_da_caixa_segue_a_da_conversa_so_se_ligada() {
        assert_eq!(letra_da_caixa(false, 130), 100);
        assert_eq!(letra_da_caixa(true, 130), 130);
        assert_eq!(letra_da_caixa(true, 100), 100);
    }

    #[test]
    fn o_enter_na_fila_troca_o_ctrl_enter_so_com_turno_que_recebe() {
        // Como era: o Enter entra no turno, o Ctrl+Enter manda depois.
        assert!(!manda_depois(false, false, true));
        assert!(manda_depois(false, true, true));
        // Enter para a fila: o Ctrl+Enter é que entra no turno.
        assert!(manda_depois(true, false, true));
        assert!(!manda_depois(true, true, true));
        // Sem turno que receba (parado, pedido aberto, esperando agentes): nada muda.
        assert!(!manda_depois(true, false, false));
        assert!(manda_depois(true, true, false));
    }
}
