import { BalaoDoUsuario, PilulaDeArquivo } from 'vitral-ui';
import { balao } from './_capturas';

const coluna = { display: 'flex', flexDirection: 'column', gap: 24, maxWidth: 620, paddingTop: 16 } as const;

export const ComAcoes = () => (
  <div style={coluna}>
    <BalaoDoUsuario acoes hora="12:51">Escreve a mensagem para a Carol</BalaoDoUsuario>
  </div>
);

export const ComAnexosELink = () => (
  <div style={coluna}>
    <BalaoDoUsuario
      anexos={
        <>
          <PilulaDeArquivo noBalao imagem={balao} nome="colado-b51d81ef3e.png" medida="601×263" />
          <PilulaDeArquivo noBalao nome="notas-do-chat.md" medida="2,4 KB" />
        </>
      }
    >
      Melhora como fica as mensagens minhas e confere o <a href="#">src/claude.rs</a>, a função <code>ler_erro</code> está devolvendo o tipo errado.
    </BalaoDoUsuario>
  </div>
);

export const ComandoELongo = () => (
  <div style={{ ...coluna, gap: 12 }}>
    <BalaoDoUsuario comando="/compact" />
    <BalaoDoUsuario linhas={20}>
      Aviso de divisão de trabalho: há uma sessão do Claude Code no VS Code neste mesmo repositório.
      <br />
      Ela enviou ao PR #2 o commit e0d2131 (correção da compilação no Windows, só <a href="#">src/instancia.rs</a>).
      <br />
      Ela não vai tocar em src/agente_codex.rs, src/claude.rs, src/motor.rs nem em src/ui/.
      <br />
      Pode seguir com o resto do pacote de abas e da caixa de mensagem.
      <br />
      Quando terminar, me diga o que mudou nos testes do tema.
      <br />
      Os prints ficam na pasta arquivo/.
      <br />
      O relatório de aparência continua valendo.
      <br />
      Nada de travessão nos textos.
      <br />
      As capturas vão a 1600 x 1000.
      <br />
      Uma sessão por worktree.
      <br />
      O deploy fica para amanhã.
      <br />
      O dono revisa no fim do dia.
      <br />
      E a lista de pendências segue no Linear.
    </BalaoDoUsuario>
  </div>
);
