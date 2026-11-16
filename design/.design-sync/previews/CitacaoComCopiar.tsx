import { CitacaoComCopiar, RespostaDoAgente } from 'vitral-ui';

const coluna = { display: 'flex', flexDirection: 'column', gap: 12, maxWidth: 620 } as const;

export const Citacao = () => (
  <div style={coluna}>
    <RespostaDoAgente>
      <p>Para a Carol:</p>
    </RespostaDoAgente>
    <CitacaoComCopiar>
      Carol, arrumei as duas coisas. Na contagem que está aberta, aperta <b>"Só os contados"</b> lá em cima: ela vai comparar só os 8 produtos que vocês contaram, e não os 422.
    </CitacaoComCopiar>
  </div>
);

export const CodigoCopiado = () => (
  <div style={coluna}>
    <CitacaoComCopiar tipo="codigo" copiado>
      cargo test tema{'\n'}
      <span style={{ color: 'var(--sintaxe-palavra-chave)' }}>fn</span> <span style={{ color: 'var(--sintaxe-funcao)' }}>balao</span>() -&gt;{' '}
      <span style={{ color: 'var(--sintaxe-tipo)' }}>Hsla</span> {'{ '}
      <span style={{ color: 'var(--sintaxe-comentario)', fontStyle: 'italic' }}>// o destaque a 16%</span> {'}'}
    </CitacaoComCopiar>
  </div>
);
