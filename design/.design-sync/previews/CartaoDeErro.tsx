import { CartaoDeErro } from 'vitral-ui';

const coluna = { display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 560 } as const;

export const Limite = () => (
  <div style={coluna}>
    <CartaoDeErro
      tom="vermelho"
      icone="gauge"
      titulo="O Codex chegou ao limite de uso"
      frase="Volta às 12:30."
      acoes={[
        { rotulo: 'Ver o uso', icone: 'external-link' },
        { rotulo: 'Continuar no Claude', icone: 'shuffle' },
      ]}
      detalhes="You've hit your usage limit. Try again at 12:30 PM."
    />
  </div>
);

export const PrecisaDeVoce = () => (
  <div style={coluna}>
    <CartaoDeErro
      tom="espera"
      icone="key-round"
      titulo="O Claude Code perdeu o login desta conta"
      frase="Entre nela de novo e tente outra vez."
      acoes={[{ rotulo: 'Entrar de novo', icone: 'key-round' }]}
      detalhes="Invalid API key · Please run /login"
    />
  </div>
);

export const ComRessalva = () => (
  <div style={coluna}>
    <CartaoDeErro tom="ambar" icone="zap" titulo="O Ultracode não ligou" frase="O modelo claude-sonnet-4-6 não tem o esforço Extra." />
  </div>
);

export const SemConexaoComDetalhes = () => (
  <div style={coluna}>
    <CartaoDeErro
      tom="neutro"
      icone="wifi-off"
      titulo="Sem conexão com o Claude"
      frase="A internet caiu ou o serviço não respondeu. Quando voltar, tente outra vez."
      detalhes={
        <>
          API Error: Connection error. getaddrinfo ENOTFOUND <span style={{ color: 'var(--destaque-texto)' }}>api.anthropic.com</span>
        </>
      }
      detalhesAbertos
    />
  </div>
);
